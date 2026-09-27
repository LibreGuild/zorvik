//! Network tools (`tools.*`): TLS inspector, port check, ping and the local
//! network interfaces. Port check and ping validate and resolve the target in
//! the call (so input errors come back as the call's error), then stream
//! [`ToolEvent`]s tagged with the caller's run id. `tools.cancel` stops a run
//! or an inspection in progress.

use std::future::Future;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_util::sync::CancellationToken;
use ts_rs::TS;
use zorvik_engine::EngineError;
use zorvik_engine::tools::{
    self, PingMode, PingOptions, PingReply, PingStarted, PingSummary, Pinger, PortResult, PortScan, PortScanOptions,
    PortScanSummary, TlsInspectOptions,
};

use crate::{Api, ApiError, ApiResult, StreamEvent, lock, ok, params, remove_if_current};

/// Progress of a streaming tool run. A port check ends with `portsDone`, a
/// ping with `pingDone` (also when cancelled).
#[derive(Debug, Clone, Serialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(export)]
pub enum ToolEvent {
    /// Port check: one port finished (in completion order).
    PortResult(PortResult),
    PortsDone(PortScanSummary),
    /// Ping: the mode and address used (the same value `tools.ping` returns).
    PingStarted(PingStarted),
    PingReply(PingReply),
    PingDone(PingSummary),
}

/// Result of `tools.portCheck` (results follow as events).
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PortCheckStarted {
    /// The address being checked (the host is resolved once).
    pub address: String,
    pub total: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TlsInspectParams {
    /// `host`, `host:port` or a URL.
    host: String,
    /// Used when `host` has no port (default 443).
    #[serde(default)]
    port: Option<u16>,
    /// Server name to send instead of the host (e.g. when `host` is an IP).
    #[serde(default)]
    sni: Option<String>,
    /// Lets `tools.cancel` abort the inspection.
    #[serde(default)]
    run_id: Option<String>,
}

/// `"80,443,8000-8010"` or `[80, 443]`.
#[derive(Deserialize)]
#[serde(untagged)]
enum PortsParam {
    Spec(String),
    List(Vec<u16>),
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PortCheckParams {
    run_id: String,
    host: String,
    ports: PortsParam,
    #[serde(default)]
    timeout_ms: Option<u64>,
    #[serde(default)]
    concurrency: Option<usize>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PingParams {
    run_id: String,
    host: String,
    /// Echoes to send; 0 = until cancelled. Default 4.
    #[serde(default)]
    count: Option<u32>,
    #[serde(default)]
    interval_ms: Option<u64>,
    #[serde(default)]
    timeout_ms: Option<u64>,
    #[serde(default)]
    mode: PingMode,
    /// TCP mode port (default 443).
    #[serde(default)]
    port: Option<u16>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RunParam {
    run_id: String,
}

/// Run `fut` unless `cancel` fires first.
async fn cancellable<T>(
    cancel: &CancellationToken,
    fut: impl Future<Output = zorvik_engine::Result<T>>,
) -> ApiResult<T> {
    tokio::select! {
        r = fut => r.map_err(ApiError::from),
        _ = cancel.cancelled() => Err(EngineError::cancelled().into()),
    }
}

impl Api {
    /// `tools.*` methods.
    pub(crate) async fn call_tool(&self, method: &str, p: Value) -> ApiResult<Value> {
        match method {
            "tools.tlsInspect" => {
                let TlsInspectParams { host, port, sni, run_id } = params(p)?;
                let opts = crate::request_options(&self.settings(), &Default::default())?;
                let options = TlsInspectOptions {
                    sni,
                    proxy: opts.proxy,
                    ca_cert_path: opts.tls.ca_cert_path,
                    connect_timeout: opts.connect_timeout.min(Duration::from_secs(30)),
                    ..Default::default()
                };
                let (generation, cancel) = match &run_id {
                    Some(id) => self.begin_tool_run(id),
                    None => (0, CancellationToken::new()),
                };
                let report = cancellable(&cancel, tools::inspect_tls(&host, port.unwrap_or(0), &options)).await;
                if let Some(id) = &run_id {
                    remove_if_current(&self.inner.tool_runs, id, |(g, _)| *g == generation);
                }
                ok(report?)
            }
            "tools.portCheck" => {
                let p: PortCheckParams = params(p)?;
                let ports = match p.ports {
                    PortsParam::Spec(spec) => tools::parse_ports(&spec)?,
                    PortsParam::List(list) => list,
                };
                let defaults = PortScanOptions::default();
                let options = PortScanOptions {
                    timeout: p.timeout_ms.map_or(defaults.timeout, Duration::from_millis),
                    concurrency: p.concurrency.unwrap_or(defaults.concurrency),
                };
                let (generation, cancel) = self.begin_tool_run(&p.run_id);
                let scan = match cancellable(&cancel, PortScan::prepare(&p.host, ports, options)).await {
                    Ok(scan) => scan,
                    Err(e) => {
                        remove_if_current(&self.inner.tool_runs, &p.run_id, |(g, _)| *g == generation);
                        return Err(e);
                    }
                };
                let started = PortCheckStarted { address: scan.address().to_string(), total: scan.total() as u32 };
                let inner = self.inner.clone();
                let run_id = p.run_id;
                tokio::spawn(async move {
                    let emit = |event| inner.sink.emit(StreamEvent::Tool { run_id: run_id.clone(), event });
                    let summary = scan.run(&cancel, |r| emit(ToolEvent::PortResult(r))).await;
                    emit(ToolEvent::PortsDone(summary));
                    remove_if_current(&inner.tool_runs, &run_id, |(g, _)| *g == generation);
                });
                ok(started)
            }
            "tools.ping" => {
                let p: PingParams = params(p)?;
                let defaults = PingOptions::default();
                let options = PingOptions {
                    count: p.count.unwrap_or(defaults.count),
                    interval: p.interval_ms.map_or(defaults.interval, Duration::from_millis),
                    timeout: p.timeout_ms.map_or(defaults.timeout, Duration::from_millis),
                    mode: p.mode,
                    port: p.port.unwrap_or(defaults.port),
                };
                let (generation, cancel) = self.begin_tool_run(&p.run_id);
                let pinger = match cancellable(&cancel, Pinger::prepare(&p.host, options)).await {
                    Ok(pinger) => pinger,
                    Err(e) => {
                        remove_if_current(&self.inner.tool_runs, &p.run_id, |(g, _)| *g == generation);
                        return Err(e);
                    }
                };
                let started = pinger.started().clone();
                let inner = self.inner.clone();
                let run_id = p.run_id;
                let first = started.clone();
                tokio::spawn(async move {
                    let emit = |event| inner.sink.emit(StreamEvent::Tool { run_id: run_id.clone(), event });
                    emit(ToolEvent::PingStarted(first));
                    let summary = pinger.run(&cancel, |r| emit(ToolEvent::PingReply(r))).await;
                    emit(ToolEvent::PingDone(summary));
                    remove_if_current(&inner.tool_runs, &run_id, |(g, _)| *g == generation);
                });
                ok(started)
            }
            "tools.interfaces" => {
                let list = tokio::task::spawn_blocking(tools::list_interfaces).await.map_err(crate::join_err)??;
                ok(list)
            }
            "tools.cancel" => {
                let RunParam { run_id } = params(p)?;
                // The run reports its own end (`portsDone` / `pingDone` with `cancelled`).
                if let Some((_, cancel)) = lock(&self.inner.tool_runs).remove(&run_id) {
                    cancel.cancel();
                }
                ok(())
            }
            other => Err(ApiError::new("notFound", format!("Unknown method '{other}'"))),
        }
    }

    /// Register a run (replacing, and stopping, an older one with the same id).
    fn begin_tool_run(&self, run_id: &str) -> (u64, CancellationToken) {
        let generation = self.next_generation();
        let cancel = CancellationToken::new();
        if let Some((_, old)) = lock(&self.inner.tool_runs).insert(run_id.to_string(), (generation, cancel.clone())) {
            old.cancel();
        }
        (generation, cancel)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use serde_json::json;

    use super::*;
    use crate::EventSink;

    #[derive(Default)]
    struct Collect(Mutex<Vec<(String, ToolEvent)>>);

    impl EventSink for Collect {
        fn emit(&self, event: StreamEvent) {
            if let StreamEvent::Tool { run_id, event } = event {
                self.0.lock().unwrap().push((run_id, event));
            }
        }
    }

    fn api() -> (Api, Arc<Collect>, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let events = Arc::new(Collect::default());
        (Api::new(dir.path().to_path_buf(), events.clone()), events, dir)
    }

    async fn wait_for(events: &Collect, pred: impl Fn(&[(String, ToolEvent)]) -> bool) -> Vec<(String, ToolEvent)> {
        for _ in 0..500 {
            {
                let list = events.0.lock().unwrap();
                if pred(&list) {
                    return list.clone();
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("events not received: {:?}", events.0.lock().unwrap());
    }

    #[test]
    fn events_are_tagged_by_type() {
        let e = ToolEvent::PortResult(PortResult { port: 80, open: true, ms: 1.5, error: None, message: None });
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["type"], "portResult");
        assert_eq!(v["port"], 80);
    }

    #[tokio::test]
    async fn port_check_streams_results_and_a_summary() {
        let (api, events, _dir) = api();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let open = listener.local_addr().unwrap().port();
        let closed = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
        let started = api
            .call("tools.portCheck", json!({ "runId": "r1", "host": "127.0.0.1", "ports": format!("{open},{closed}") }))
            .await
            .unwrap();
        assert_eq!(started["address"], "127.0.0.1");
        assert_eq!(started["total"], 2);
        let list = wait_for(&events, |l| l.iter().any(|(_, e)| matches!(e, ToolEvent::PortsDone(_)))).await;
        let results: Vec<&PortResult> = list
            .iter()
            .filter_map(|(id, e)| match e {
                ToolEvent::PortResult(r) if id == "r1" => Some(r),
                _ => None,
            })
            .collect();
        assert_eq!(results.len(), 2);
        let Some((_, ToolEvent::PortsDone(summary))) = list.last() else { panic!("no summary last") };
        assert_eq!(summary.open, vec![open]);
        assert!(!summary.cancelled);
        drop(listener);
    }

    #[tokio::test]
    async fn port_check_rejects_bad_input_in_the_call() {
        let (api, events, _dir) = api();
        let err = api.call("tools.portCheck", json!({ "runId": "r", "host": "127.0.0.1", "ports": "1-5000" })).await;
        assert!(err.unwrap_err().message.contains("at most 1024"));
        let err = api.call("tools.portCheck", json!({ "runId": "r", "host": "", "ports": "80" })).await;
        assert!(err.is_err());
        assert!(events.0.lock().unwrap().is_empty());
        assert!(lock(&api.inner.tool_runs).is_empty());
    }

    #[tokio::test]
    async fn tcp_ping_runs_until_cancelled() {
        let (api, events, _dir) = api();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((s, _)) = listener.accept().await {
                drop(s);
            }
        });
        let started = api
            .call(
                "tools.ping",
                json!({ "runId": "p1", "host": "127.0.0.1", "count": 0, "intervalMs": 200, "mode": "tcp", "port": port }),
            )
            .await
            .unwrap();
        assert_eq!(started["mode"], "tcp");
        wait_for(&events, |l| l.iter().filter(|(_, e)| matches!(e, ToolEvent::PingReply(_))).count() >= 2).await;
        api.call("tools.cancel", json!({ "runId": "p1" })).await.unwrap();
        let list = wait_for(&events, |l| l.iter().any(|(_, e)| matches!(e, ToolEvent::PingDone(_)))).await;
        assert!(matches!(list.first(), Some((_, ToolEvent::PingStarted(_)))));
        let Some((_, ToolEvent::PingDone(summary))) = list.last() else { panic!("done is not last") };
        assert!(summary.cancelled);
        assert!(summary.received >= 2 && summary.received == summary.sent);
    }

    #[tokio::test]
    async fn interfaces_and_unknown_methods() {
        let (api, _, _dir) = api();
        let list = api.call("tools.interfaces", json!({})).await.unwrap();
        assert!(!list.as_array().unwrap().is_empty());
        assert_eq!(api.call("tools.nope", json!({})).await.unwrap_err().code, "notFound");
        // Cancelling an unknown run is not an error.
        api.call("tools.cancel", json!({ "runId": "missing" })).await.unwrap();
    }

    #[tokio::test]
    async fn tls_inspect_reports_connection_errors() {
        let (api, _, _dir) = api();
        let port = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
        // Spawned like the transports do: the call's future must be `Send`.
        let call = tokio::spawn(async move {
            api.call("tools.tlsInspect", json!({ "host": format!("127.0.0.1:{port}"), "runId": "t1" })).await
        });
        let err = call.await.unwrap().unwrap_err();
        assert_eq!(err.code, "network");
    }
}

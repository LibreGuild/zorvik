//! Tools (`tools.*`): TLS inspector, port check, ping and the local network
//! interfaces. Port check and ping validate and resolve the target in the call
//! (so input errors come back as the call's error), then stream [`ToolEvent`]s
//! tagged with the caller's run id. `tools.cancel` stops a run or an inspection
//! in progress.
//!
//! Offline tools run on a blocking thread: JSON compare, the regex tester and the
//! certificate decoder. `tools.recentResponses` / `tools.recentResponse` give them
//! the bodies of the last responses sent from the app.

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
use zorvik_workspace::formats::json_diff::{self, JsonDiffOptions};
use zorvik_workspace::formats::regex_test::{self, RegexFlags};

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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct JsonDiffParams {
    left: String,
    right: String,
    #[serde(flatten)]
    options: JsonDiffOptions,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RegexTestParams {
    pattern: String,
    #[serde(default)]
    flags: RegexFlags,
    text: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CertDecodeParams {
    /// PEM text (or base64) pasted by the user…
    #[serde(default)]
    text: Option<String>,
    /// …or a file the user picked (PEM or DER).
    #[serde(default)]
    path: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HistoryIdParam {
    history_id: i64,
}

/// A response sent from the app whose body is still kept (the last ones, in memory).
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RecentResponseInfo {
    #[ts(type = "number")]
    pub history_id: i64,
    pub method: String,
    pub url: String,
    pub status: u16,
    /// Unix epoch milliseconds.
    #[ts(type = "number")]
    pub created_at: i64,
    /// Characters of the body kept.
    pub chars: u32,
    /// Only the start of the body is kept.
    pub cut: bool,
}

/// Body of a recent response (`tools.recentResponse`).
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RecentResponseBody {
    pub text: String,
    pub cut: bool,
}

/// Responses `tools.recentResponses` looks through (newest history entries).
const RECENT_LOOKUP: u32 = 200;

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
            "tools.jsonDiff" => {
                let JsonDiffParams { left, right, options } = params(p)?;
                let diff = tokio::task::spawn_blocking(move || json_diff::diff_json(&left, &right, options))
                    .await
                    .map_err(crate::join_err)?
                    .map_err(ApiError::invalid)?;
                ok(diff)
            }
            "tools.regexTest" => {
                let RegexTestParams { pattern, flags, text } = params(p)?;
                let result = tokio::task::spawn_blocking(move || regex_test::test_regex(&pattern, flags, &text))
                    .await
                    .map_err(crate::join_err)?
                    .map_err(|message| ApiError::new("regex", message))?;
                ok(result)
            }
            "tools.certDecode" => {
                let CertDecodeParams { text, path } = params(p)?;
                let report = tokio::task::spawn_blocking(move || -> ApiResult<_> {
                    let bytes = match (text, path) {
                        (Some(text), _) => text.into_bytes(),
                        (None, Some(path)) => read_limited(&path, tools::certs::MAX_INPUT)?,
                        (None, None) => return Err(ApiError::invalid("Give text or path")),
                    };
                    tools::decode_certificates(&bytes).map_err(|e| ApiError::invalid(e.message))
                })
                .await
                .map_err(crate::join_err)??;
                ok(report)
            }
            "tools.recentResponses" => {
                // Without a workspace there is no history to pick from.
                let Ok(ws) = self.ws() else { return ok(Vec::<RecentResponseInfo>::new()) };
                let Some(history) = &self.inner.history else { return ok(Vec::<RecentResponseInfo>::new()) };
                let entries = history.list(&ws.root().to_string_lossy(), "", RECENT_LOOKUP, 0)?;
                let list: Vec<RecentResponseInfo> = entries
                    .into_iter()
                    .filter_map(|e| {
                        let r = self.recent_response(e.id)?;
                        let body = r.body.as_ref()?;
                        Some(RecentResponseInfo {
                            history_id: e.id,
                            method: e.method,
                            url: e.url,
                            status: r.status,
                            created_at: e.created_at,
                            chars: body.chars().count() as u32,
                            cut: r.body_cut,
                        })
                    })
                    .collect();
                ok(list)
            }
            "tools.recentResponse" => {
                let HistoryIdParam { history_id } = params(p)?;
                let gone = || ApiError::new("notFound", "That response is no longer kept; send the request again");
                let r = self.recent_response(history_id).ok_or_else(gone)?;
                let text = r.body.ok_or_else(gone)?;
                ok(RecentResponseBody { text, cut: r.body_cut })
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

/// The file at `path`, refused when larger than `max` bytes.
fn read_limited(path: &str, max: usize) -> ApiResult<Vec<u8>> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|e| ApiError::new("io", format!("Could not open {path}: {e}")))?;
    let mut bytes = Vec::new();
    file.take(max as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| ApiError::new("io", format!("Could not read {path}: {e}")))?;
    if bytes.len() > max {
        return Err(ApiError::invalid(format!("{path} is larger than {} MB", max / (1024 * 1024))));
    }
    Ok(bytes)
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
    async fn json_diff_and_regex_test_run_on_their_input() {
        let (api, _, _dir) = api();
        let diff = api
            .call(
                "tools.jsonDiff",
                json!({ "left": r#"{"a": 1, "b": [1, 2]}"#, "right": r#"{"b": [1, 2, 3], "a": 1.0}"# }),
            )
            .await
            .unwrap();
        assert_eq!((diff["added"].as_u64(), diff["changed"].as_u64()), (Some(1), Some(0)));
        assert_eq!(diff["differences"][0]["path"], "$.b[2]");
        let any_order = json!({ "left": "[1, 2]", "right": "[2, 1]", "ignoreArrayOrder": true });
        assert_eq!(api.call("tools.jsonDiff", any_order).await.unwrap()["differences"], json!([]));
        let err = api.call("tools.jsonDiff", json!({ "left": "{", "right": "{}" })).await.unwrap_err();
        assert_eq!(err.code, "invalidInput");
        assert!(err.message.contains("left document"), "{}", err.message);

        let r = api
            .call(
                "tools.regexTest",
                json!({ "pattern": "id=(\\d+)", "flags": { "ignoreCase": true }, "text": "ID=7 id=8" }),
            )
            .await
            .unwrap();
        assert_eq!((r["count"].as_u64(), r["capture"].as_str()), (Some(2), Some("7")));
        assert_eq!(r["pattern"], "(?i)id=(\\d+)");
        let err = api.call("tools.regexTest", json!({ "pattern": "(", "text": "x" })).await.unwrap_err();
        assert_eq!(err.code, "regex");
    }

    #[tokio::test]
    async fn cert_decode_reads_text_or_a_file() {
        let (api, _, dir) = api();
        let certs = zorvik_testkit::TestCerts::generate();
        let text = format!("{}{}{}", certs.cert_pem, certs.ca_pem, certs.key_pem);
        let r = api.call("tools.certDecode", json!({ "text": text })).await.unwrap();
        assert_eq!(r["items"].as_array().unwrap().len(), 3);
        assert_eq!(r["items"][0]["certificate"]["details"]["commonName"], "localhost");
        assert_eq!(r["items"][2]["key"]["matches"], json!([0]));
        // The private key is never sent back.
        assert!(!r.to_string().contains("PRIVATE KEY-----"));

        let path = certs.write_ca(dir.path());
        let r = api.call("tools.certDecode", json!({ "path": path })).await.unwrap();
        assert_eq!(r["items"][0]["certificate"]["details"]["commonName"], "Zorvik Test CA");
        let missing = dir.path().join("missing.pem");
        assert_eq!(api.call("tools.certDecode", json!({ "path": missing })).await.unwrap_err().code, "io");
        let err = api.call("tools.certDecode", json!({ "text": "hello" })).await.unwrap_err();
        assert_eq!(err.code, "invalidInput");
        let big = dir.path().join("big.pem");
        std::fs::write(&big, vec![b'a'; tools::certs::MAX_INPUT + 1]).unwrap();
        assert!(api.call("tools.certDecode", json!({ "path": big })).await.unwrap_err().message.contains("larger"));
    }

    #[tokio::test]
    async fn recent_responses_list_the_bodies_still_kept() {
        let (api, _, _dir) = api();
        // No workspace, no history.
        assert_eq!(api.call("tools.recentResponses", json!({})).await.unwrap(), json!([]));
        let ws = tempfile::tempdir().unwrap();
        api.call("workspace.create", json!({ "path": ws.path(), "name": "T" })).await.unwrap();
        assert_eq!(api.call("tools.recentResponses", json!({})).await.unwrap(), json!([]));
        let server = zorvik_testkit::TestServer::start().await;
        let request = json!({ "name": "r", "seq": 0, "method": "GET", "url": server.url("/json") });
        api.call("http.send", json!({ "requestId": "s1", "request": request })).await.unwrap();
        let list = api.call("tools.recentResponses", json!({})).await.unwrap();
        let first = &list[0];
        assert_eq!(first["method"], "GET");
        assert_eq!(first["status"], 200);
        assert!(first["url"].as_str().unwrap().ends_with("/json"));
        let body = api.call("tools.recentResponse", json!({ "historyId": first["historyId"] })).await.unwrap();
        assert!(serde_json::from_str::<Value>(body["text"].as_str().unwrap()).is_ok());
        assert_eq!(body["cut"], false);
        let gone = api.call("tools.recentResponse", json!({ "historyId": -1 })).await.unwrap_err();
        assert_eq!(gone.code, "notFound");
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

//! Saved servers (files) and running servers. A running server keeps going
//! when its tab closes or another workspace is opened; only Stop (or quitting
//! the app) ends it. Its traffic is kept in a bounded log for the UI.
//!
//! "Start with workspace" only starts configurations this computer has
//! started or saved before (see `Manager::trusted`): servers come from Git,
//! and opening a cloned or pulled repository must not open listeners by itself.
//! Server templates never see secret variables: a server answers whoever
//! connects to it.

use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_util::sync::CancellationToken;
use ts_rs::TS;
use zorvik_servers::{OutgoingMessage, Reporter, RunningServer, ServerEvent, ServerStats, StartOptions, TrafficEntry};
use zorvik_workspace::Workspace;
use zorvik_workspace::formats::{Server, ServerKind};
use zorvik_workspace::store::server_fingerprint;
use zorvik_workspace::vars::VarContext;

use crate::{Api, ApiError, ApiResult, Inner, StreamEvent, lock, ok, params};

/// Traffic entries kept per running server (oldest dropped first).
const LOG_MAX_ENTRIES: usize = 5000;
const LOG_MAX_BYTES: usize = 32 << 20;
/// Trusted server configurations, in the app data dir.
const TRUSTED_FILE: &str = "trusted-servers.json";
/// Configurations remembered per server (most recent last).
const TRUSTED_PER_SERVER: usize = 8;

/// A running server as the UI lists it (title bar, sidebar, server tab).
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RunningServerInfo {
    pub run_id: String,
    /// Workspace folder the server belongs to.
    pub workspace_path: String,
    pub workspace_name: String,
    /// Saved server id (file stem under `servers/`).
    pub server_id: String,
    pub name: String,
    pub kind: ServerKind,
    /// Address clients use, e.g. `http://127.0.0.1:3000`.
    pub url: String,
    pub host: String,
    pub port: u16,
    pub tls: bool,
    /// Unix epoch milliseconds.
    pub started_at: f64,
    pub stats: ServerStats,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AutoStartResult {
    pub started: Vec<RunningServerInfo>,
    /// "Name: reason" for servers that could not start.
    pub errors: Vec<String>,
}

#[derive(Default)]
struct TrafficLog {
    entries: VecDeque<TrafficEntry>,
    bytes: usize,
}

fn entry_bytes(e: &TrafficEntry) -> usize {
    let http = e.http.as_ref().map_or(0, |h| h.request_body.len() + h.response_body.len() + 256);
    e.summary.len() + e.text.as_ref().map_or(0, String::len) + e.base64.as_ref().map_or(0, String::len) + http + 64
}

impl TrafficLog {
    fn push(&mut self, entry: TrafficEntry) {
        self.bytes += entry_bytes(&entry);
        self.entries.push_back(entry);
        while self.entries.len() > LOG_MAX_ENTRIES || (self.bytes > LOG_MAX_BYTES && self.entries.len() > 1) {
            if let Some(old) = self.entries.pop_front() {
                self.bytes -= entry_bytes(&old);
            }
        }
    }
}

struct Running {
    info: RunningServerInfo,
    handle: Arc<RunningServer>,
    reporter: Reporter,
    log: Arc<Mutex<TrafficLog>>,
    /// Stops the stats ticker.
    ticker: CancellationToken,
}

impl Drop for Running {
    fn drop(&mut self) {
        self.ticker.cancel();
        self.handle.stop();
    }
}

/// Fingerprints (see [`server_fingerprint`]) by `<workspace key>/<server id>`.
type Trusted = HashMap<String, Vec<String>>;

#[derive(Default)]
pub(crate) struct Manager {
    running: Mutex<HashMap<String, Running>>,
    /// Configurations the user started or saved on this computer: only those
    /// start with the workspace. Loaded on first use.
    trusted: Mutex<Option<Trusted>>,
}

impl Manager {
    /// Every running server, oldest first.
    pub(crate) fn running(&self) -> Vec<RunningServerInfo> {
        let mut list: Vec<RunningServerInfo> = lock(&self.running).values().map(|r| self.info(r)).collect();
        list.sort_by(|a, b| a.started_at.total_cmp(&b.started_at));
        list
    }

    fn info(&self, run: &Running) -> RunningServerInfo {
        RunningServerInfo { stats: run.reporter.stats(), ..run.info.clone() }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct IdParam {
    id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RunParam {
    run_id: String,
}

fn not_running() -> ApiError {
    ApiError::new("notFound", "The server is not running")
}

impl Api {
    /// `server.*` methods.
    pub(crate) async fn call_server(&self, method: &str, p: Value) -> ApiResult<Value> {
        match method {
            "server.list" => ok(self.ws()?.list_servers()?),
            "server.read" => {
                let IdParam { id } = params(p)?;
                ok(self.ws()?.read_server(&id)?)
            }
            "server.create" => {
                #[derive(Deserialize)]
                struct P {
                    server: Server,
                }
                let P { server } = params(p)?;
                let ws = self.ws()?;
                let id = ws.create_server(&server)?;
                self.trust_server(&ws, &id, &server);
                ok(id)
            }
            "server.save" => {
                #[derive(Deserialize)]
                struct P {
                    id: String,
                    server: Server,
                }
                let P { id, server } = params(p)?;
                let ws = self.ws()?;
                let new_id = ws.save_server(&id, &server)?;
                if new_id != id {
                    self.forget_server(&ws, &id);
                }
                self.trust_server(&ws, &new_id, &server);
                // A running copy follows the rename.
                let root = root_of(&ws);
                for run in lock(&self.inner.servers.running).values_mut() {
                    if run.info.workspace_path == root && run.info.server_id == id {
                        run.info.server_id = new_id.clone();
                        run.info.name = server.name.clone();
                    }
                }
                ok(new_id)
            }
            "server.duplicate" => {
                let IdParam { id } = params(p)?;
                ok(self.ws()?.duplicate_server(&id)?)
            }
            "server.delete" => {
                let IdParam { id } = params(p)?;
                let ws = self.ws()?;
                let root = root_of(&ws);
                lock(&self.inner.servers.running)
                    .retain(|_, r| !(r.info.workspace_path == root && r.info.server_id == id));
                self.forget_server(&ws, &id);
                tokio::task::spawn_blocking(move || ws.delete_server(&id)).await.map_err(crate::join_err)??;
                ok(())
            }
            "server.reorder" => {
                #[derive(Deserialize)]
                struct P {
                    ids: Vec<String>,
                }
                let P { ids } = params(p)?;
                ok(self.ws()?.reorder_servers(&ids)?)
            }

            "server.start" => {
                #[derive(Deserialize)]
                struct P {
                    id: String,
                    /// The configuration to run (may have unsaved edits).
                    server: Server,
                }
                let P { id, server } = params(p)?;
                let ws = self.ws()?;
                let info = self.start_server(&ws, id.clone(), server.clone()).await?;
                self.trust_server(&ws, &id, &server);
                ok(info)
            }
            "server.autoStart" => {
                let ws = self.ws()?;
                let mut result = AutoStartResult { started: Vec::new(), errors: Vec::new() };
                for node in ws.list_servers()?.into_iter().filter(|n| n.auto_start && n.error.is_none()) {
                    if self.find_run(&ws, &node.id).is_some() {
                        continue;
                    }
                    let started = match ws.read_server(&node.id) {
                        Ok(server) if !self.is_trusted(&ws, &node.id, &server) => Err(ApiError::invalid(
                            "not started automatically because it is new or was changed outside Zorvik \
                             (e.g. by a Git pull). Start it once to let it start with the workspace.",
                        )),
                        Ok(server) => self.start_server(&ws, node.id.clone(), server).await,
                        Err(e) => Err(e.into()),
                    };
                    match started {
                        Ok(info) => result.started.push(info),
                        Err(e) => result.errors.push(format!("{}: {}", node.name, e.message)),
                    }
                }
                ok(result)
            }
            "server.stop" => {
                let RunParam { run_id } = params(p)?;
                // Dropping the entry stops the server; its Stopped event updates the UI.
                lock(&self.inner.servers.running).remove(&run_id).ok_or_else(not_running)?;
                ok(())
            }
            "server.stopAll" => {
                self.stop_all_servers();
                ok(())
            }
            "server.running" => ok(self.inner.servers.running()),
            "server.update" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct P {
                    run_id: String,
                    server: Server,
                }
                #[derive(Serialize)]
                #[serde(rename_all = "camelCase")]
                struct Updated {
                    /// `false`: the address, port, TLS or kind changed; restart to apply.
                    applied: bool,
                }
                let P { run_id, server } = params(p)?;
                let vars = self.server_vars(&self.ws()?);
                let mut running = lock(&self.inner.servers.running);
                let run = running.get_mut(&run_id).ok_or_else(not_running)?;
                let name = server.name.clone();
                let applied = run.handle.update(server, vars);
                if applied {
                    run.info.name = name;
                }
                ok(Updated { applied })
            }
            "server.send" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct P {
                    run_id: String,
                    /// A connection, or every client when missing.
                    conn: Option<u64>,
                    message: OutgoingMessage,
                }
                let P { run_id, conn, message } = params(p)?;
                let handle =
                    lock(&self.inner.servers.running).get(&run_id).map(|r| r.handle.clone()).ok_or_else(not_running)?;
                let count = handle.send(conn, message).await.map_err(|e| ApiError::invalid(e.message))?;
                ok(count)
            }
            "server.disconnect" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct P {
                    run_id: String,
                    conn: u64,
                }
                let P { run_id, conn } = params(p)?;
                lock(&self.inner.servers.running).get(&run_id).ok_or_else(not_running)?.handle.disconnect(conn);
                ok(())
            }
            "server.log" => {
                let RunParam { run_id } = params(p)?;
                ok(self.server_log(&run_id).ok_or_else(not_running)?)
            }
            "server.clearLog" => {
                let RunParam { run_id } = params(p)?;
                let log =
                    lock(&self.inner.servers.running).get(&run_id).map(|r| r.log.clone()).ok_or_else(not_running)?;
                *lock(&log) = TrafficLog::default();
                ok(())
            }
            other => Err(ApiError::new("notFound", format!("Unknown method '{other}'"))),
        }
    }

    /// Stop a running server (nothing when it already stopped).
    pub(crate) fn stop_run(&self, run_id: &str) {
        let run = lock(&self.inner.servers.running).remove(run_id);
        drop(run);
    }

    /// Stop the servers running from a saved server of `ws`.
    pub(crate) fn stop_server_file(&self, ws: &Workspace, server_id: &str) {
        if let Some(run_id) = self.find_run(ws, server_id) {
            self.stop_run(&run_id);
        }
    }

    /// Stop every server of the workspace in folder `root`.
    pub(crate) fn stop_servers_in(&self, root: &str) {
        let stopped: Vec<Running> = {
            let mut running = lock(&self.inner.servers.running);
            let ids: Vec<String> =
                running.iter().filter(|(_, r)| r.info.workspace_path == root).map(|(id, _)| id.clone()).collect();
            ids.into_iter().filter_map(|id| running.remove(&id)).collect()
        };
        drop(stopped);
    }

    /// Run `f` on a running server's log in place (`None`: it isn't running).
    pub(crate) fn with_server_log<T>(&self, run_id: &str, f: impl FnOnce(&VecDeque<TrafficEntry>) -> T) -> Option<T> {
        let log = lock(&self.inner.servers.running).get(run_id).map(|r| r.log.clone())?;
        let log = lock(&log);
        Some(f(&log.entries))
    }

    /// What a running server logged, oldest first (`None`: it isn't running).
    pub(crate) fn server_log(&self, run_id: &str) -> Option<Vec<TrafficEntry>> {
        let log = lock(&self.inner.servers.running).get(run_id).map(|r| r.log.clone())?;
        let entries = lock(&log).entries.iter().cloned().collect();
        Some(entries)
    }

    fn find_run(&self, ws: &Workspace, server_id: &str) -> Option<String> {
        let root = root_of(ws);
        lock(&self.inner.servers.running)
            .iter()
            .find(|(_, r)| r.info.workspace_path == root && r.info.server_id == server_id)
            .map(|(id, _)| id.clone())
    }

    pub(crate) async fn start_server(
        &self,
        ws: &Workspace,
        server_id: String,
        server: Server,
    ) -> ApiResult<RunningServerInfo> {
        if self.find_run(ws, &server_id).is_some() {
            return Err(ApiError::invalid(format!("'{}' is already running", server.name)));
        }
        let run_id = uuid::Uuid::new_v4().to_string();
        let log: Arc<Mutex<TrafficLog>> = Arc::default();
        let stopped: Arc<OnceLock<Option<String>>> = Arc::default();
        let reporter =
            Reporter::new(event_sink(Arc::downgrade(&self.inner), run_id.clone(), log.clone(), stopped.clone()));
        let options = StartOptions {
            base_dir: ws.root().to_path_buf(),
            client: self.inner.client.clone(),
            request_options: crate::request_options(&self.settings(), &Default::default())?,
        };
        let (name, kind, host, tls) = (server.name.clone(), server.kind, server.host.clone(), server.tls.enabled);
        let handle = zorvik_servers::start(server, self.server_vars(ws), options, reporter.clone())
            .await
            .map_err(|e| ApiError::new("server", e.message))?;
        let info = RunningServerInfo {
            run_id: run_id.clone(),
            workspace_path: root_of(ws),
            workspace_name: ws.meta().name.clone(),
            server_id: server_id.clone(),
            name,
            kind,
            url: handle.url.clone(),
            host,
            port: handle.addr.port(),
            tls,
            started_at: crate::now_ms() as f64,
            stats: ServerStats::default(),
        };
        let ticker = CancellationToken::new();
        let run = Running {
            info: info.clone(),
            handle: Arc::new(handle),
            reporter: reporter.clone(),
            log,
            ticker: ticker.clone(),
        };
        self.list_run(run, &stopped)?;
        tokio::spawn(stats_ticker(self.inner.sink.clone(), run_id, reporter, ticker));
        Ok(info)
    }

    /// Add a started server to the running list. Refused when it already
    /// stopped (a server that fails at once may report Stopped before it is
    /// listed; it would stay listed forever) or when it was started twice at
    /// the same time (the first one is kept; this one stops when dropped).
    fn list_run(&self, run: Running, stopped: &OnceLock<Option<String>>) -> ApiResult<()> {
        let mut running = lock(&self.inner.servers.running);
        if let Some(error) = stopped.get() {
            return Err(ApiError::new("server", error.clone().unwrap_or_else(|| "The server stopped".into())));
        }
        let info = &run.info;
        if running.values().any(|r| r.info.workspace_path == info.workspace_path && r.info.server_id == info.server_id)
        {
            return Err(ApiError::invalid(format!("'{}' is already running", info.name)));
        }
        running.insert(info.run_id.clone(), run);
        Ok(())
    }

    /// Environment and workspace variables for server templates, without
    /// secrets (they stay `{{name}}`): whoever connects gets the answer.
    fn server_vars(&self, ws: &Workspace) -> VarContext {
        let public = |vars: Vec<zorvik_workspace::formats::Variable>| vars.into_iter().filter(|v| !v.secret).collect();
        let (env, workspace): (Vec<_>, Vec<_>) =
            (public(self.active_env_vars(ws)), public(ws.meta().variables.clone()));
        let mut ctx = VarContext::new();
        ctx.push_layer(&env).push_layer(&workspace);
        ctx
    }

    /// Run `f` on the trusted configurations (loaded on first use); it
    /// returns whether it changed them, which saves them.
    fn update_trusted<T>(&self, f: impl FnOnce(&mut Trusted) -> (T, bool)) -> T {
        let path = self.inner.data_dir.join(TRUSTED_FILE);
        let mut trusted = lock(&self.inner.servers.trusted);
        let map = trusted.get_or_insert_with(|| load_trusted(&path));
        let (result, changed) = f(map);
        if changed && let Err(e) = zorvik_workspace::store::save_json(&path, map) {
            tracing::warn!("could not save trusted servers: {}", e.message);
        }
        result
    }

    /// The user started or saved this configuration of server `id` here.
    pub(crate) fn trust_server(&self, ws: &Workspace, id: &str, server: &Server) {
        let (key, print) = (trust_key(ws, id), server_fingerprint(server));
        self.update_trusted(|map| {
            let list = map.entry(key).or_default();
            if list.last() == Some(&print) {
                return ((), false);
            }
            list.retain(|p| *p != print);
            list.push(print);
            let extra = list.len().saturating_sub(TRUSTED_PER_SERVER);
            list.drain(..extra);
            ((), true)
        });
    }

    fn forget_server(&self, ws: &Workspace, id: &str) {
        let key = trust_key(ws, id);
        self.update_trusted(|map| ((), map.remove(&key).is_some()));
    }

    /// Whether this computer started or saved exactly this configuration of `id`.
    pub(crate) fn is_trusted(&self, ws: &Workspace, id: &str, server: &Server) -> bool {
        let (key, print) = (trust_key(ws, id), server_fingerprint(server));
        self.update_trusted(|map| (map.get(&key).is_some_and(|list| list.contains(&print)), false))
    }

    /// Servers running right now (quitting asks first when there are any).
    pub fn running_server_count(&self) -> usize {
        lock(&self.inner.servers.running).len()
    }

    /// The window wants to close while servers run: ask the UI to confirm.
    pub fn request_quit(&self) {
        let running = self.running_server_count() as u32;
        let load_test = self.load_test_running();
        self.inner.sink.emit(StreamEvent::QuitRequested { running, load_test });
    }

    /// Stop every running server (quit, or "Stop all").
    pub fn stop_all_servers(&self) {
        let stopped: Vec<Running> = lock(&self.inner.servers.running).drain().map(|(_, r)| r).collect();
        drop(stopped);
    }
}

fn root_of(ws: &Workspace) -> String {
    ws.root().to_string_lossy().into_owned()
}

fn trust_key(ws: &Workspace, id: &str) -> String {
    format!("{}/{id}", ws.local_key())
}

fn load_trusted(path: &Path) -> Trusted {
    std::fs::read(path).ok().and_then(|d| serde_json::from_slice(&d).ok()).unwrap_or_default()
}

/// Where a server's reporter sends events: its log, the UI, and (when it
/// stops) the manager. Holds the API weakly so servers don't keep it alive.
/// `stopped` is set before the manager is told, so a stop that comes before
/// the server is listed is noticed (see `Api::list_run`).
fn event_sink(
    inner: Weak<Inner>,
    run_id: String,
    log: Arc<Mutex<TrafficLog>>,
    stopped: Arc<OnceLock<Option<String>>>,
) -> impl Fn(ServerEvent) + Send + Sync {
    move |event| {
        let Some(inner) = inner.upgrade() else { return };
        match &event {
            ServerEvent::Traffic { entry } => lock(&log).push(entry.clone()),
            ServerEvent::Stopped { error } => {
                let _ = stopped.set(error.clone());
                // Take it out of the map first, then drop it without the lock held.
                let removed = lock(&inner.servers.running).remove(&run_id);
                drop(removed);
            }
            ServerEvent::Stats { .. } => {}
        }
        inner.sink.emit(StreamEvent::Server { run_id: run_id.clone(), event });
    }
}

/// Counters change with every message; the UI gets them at most 4 times a second.
async fn stats_ticker(sink: Arc<dyn crate::EventSink>, run_id: String, reporter: Reporter, stop: CancellationToken) {
    let mut last = ServerStats::default();
    loop {
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(250)) => {}
            _ = stop.cancelled() => return,
        }
        let stats = reporter.stats();
        if stats != last {
            last = stats;
            sink.emit(StreamEvent::Server { run_id: run_id.clone(), event: ServerEvent::Stats { stats } });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Quiet;

    impl crate::EventSink for Quiet {
        fn emit(&self, _: StreamEvent) {}
    }

    /// A server can fail right after it starts listening (e.g. a relay pointed
    /// at itself), so its Stopped event may come before `start_server` lists it.
    /// Listing it anyway would leave it "running" forever.
    #[tokio::test]
    async fn a_server_that_stopped_before_it_was_listed_is_not_listed() {
        let dir = tempfile::tempdir().unwrap();
        let api = Api::new(dir.path().to_path_buf(), Arc::new(Quiet));
        let log: Arc<Mutex<TrafficLog>> = Arc::default();
        let stopped: Arc<OnceLock<Option<String>>> = Arc::default();
        let reporter = Reporter::new(event_sink(Arc::downgrade(&api.inner), "r1".into(), log.clone(), stopped.clone()));
        let options = StartOptions {
            base_dir: dir.path().to_path_buf(),
            client: api.inner.client.clone(),
            request_options: Default::default(),
        };
        let server = Server { port: 0, ..Server::new("Echo", ServerKind::Tcp) };
        let handle = zorvik_servers::start(server, VarContext::new(), options, reporter.clone()).await.unwrap();
        handle.stop();
        for _ in 0..200 {
            if stopped.get().is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(stopped.get(), Some(&None));

        let info = RunningServerInfo {
            run_id: "r1".into(),
            workspace_path: "/ws".into(),
            workspace_name: "Ws".into(),
            server_id: "Echo".into(),
            name: "Echo".into(),
            kind: ServerKind::Tcp,
            url: handle.url.clone(),
            host: "127.0.0.1".into(),
            port: handle.addr.port(),
            tls: false,
            started_at: 0.0,
            stats: ServerStats::default(),
        };
        let run = Running { info, handle: Arc::new(handle), reporter, log, ticker: CancellationToken::new() };
        let ticker = run.ticker.clone();
        let err = api.list_run(run, &stopped).unwrap_err();
        assert_eq!(err.message, "The server stopped");
        assert_eq!(api.running_server_count(), 0);
        assert!(ticker.is_cancelled(), "the stats ticker is stopped too");
    }
}

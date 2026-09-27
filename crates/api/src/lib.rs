//! The RPC surface used by the UI. One `call(method, params)` entry point is
//! shared by the Tauri shell (IPC) and the dev bridge (HTTP), so both run the
//! exact same code. The methods are the arms of the `match` in `Api::dispatch`.

pub mod agents;
mod batch;
mod dns;
mod dto;
mod graphql;
mod grpc;
mod load;
mod mock;
pub mod runner;
mod scripting;
mod servers;
mod sockets;
mod state;
mod tools;
mod watcher;

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use base64::Engine as _;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use zorvik_engine::{Client, CookieJar, EngineError, ErrorKind, HttpResponse, SseParser, WsOutgoing, WsSession};
use zorvik_workspace::Workspace;
use zorvik_workspace::formats::curl::{CurlFlavor, parse_curl, to_curl};
use zorvik_workspace::formats::{
    Auth, Environment, FolderMeta, ImportError, ImportSummary, Request, Variable, WorkspaceMeta,
};
use zorvik_workspace::history::{History, NewEntry};
use zorvik_workspace::localvalues::{GLOBALS, LocalValues, overlay};
use zorvik_workspace::oauth2::{self, TokenCache};
use zorvik_workspace::resolve::{
    Inheritance, Resolved, apply_token, check_url_variables, effective_auth, render_oauth2, resolve,
};
use zorvik_workspace::secrets::{SecretStore, env_scope, workspace_scope};
use zorvik_workspace::settings::Settings;
use zorvik_workspace::vars::{DYNAMIC_VARIABLES, VarContext};

pub use agents::AgentSessionHandle;
pub use batch::BatchingSink;
pub use dns::DnsQueryResult;
pub use dto::*;
pub use graphql::GraphqlSchema;
pub use grpc::{GrpcDescribeResult, GrpcInvokeResult, GrpcStartResult};
pub use load::{ActiveLoadRun, LoadRunRecord, LoadStarted};
pub use scripting::{
    Iteration, LocalValue, ScriptFailure, ScriptReport, ScriptVars, ScriptedSend, SendContext, Sent, send_scripted,
    values_of,
};
pub use servers::{AutoStartResult, RunningServerInfo};
pub use sockets::SocketConnectResult;
use state::LocalState;
pub use tools::ToolEvent;

/// Receives events for the UI (WebSocket/SSE messages, file changes).
pub trait EventSink: Send + Sync + 'static {
    fn emit(&self, event: StreamEvent);

    /// Open a URL in the system browser. Default: ask the UI to do it.
    fn open_url(&self, url: &str) -> Result<(), String> {
        self.emit(StreamEvent::OpenUrl { url: url.to_string() });
        Ok(())
    }
}

type ApiResult<T> = Result<T, ApiError>;

impl ApiError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self { code: code.into(), message: message.into(), network_kind: None }
    }
    fn invalid(message: impl Into<String>) -> Self {
        Self::new("invalidInput", message)
    }
}

impl From<zorvik_workspace::Error> for ApiError {
    fn from(e: zorvik_workspace::Error) -> Self {
        let code = serde_json::to_value(e.code).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
        Self { code, message: e.message, network_kind: e.engine.map(|x| x.kind) }
    }
}

impl From<EngineError> for ApiError {
    fn from(e: EngineError) -> Self {
        Self { code: "network".into(), message: e.message, network_kind: Some(e.kind) }
    }
}

impl From<ImportError> for ApiError {
    fn from(e: ImportError) -> Self {
        Self::new("parse", e.0)
    }
}

/// Response bodies kept for "save to file" (bounded by count and bytes).
#[derive(Default)]
struct ResponseStore {
    items: VecDeque<(String, Arc<Vec<u8>>)>,
    bytes: usize,
}

impl ResponseStore {
    const MAX_ITEMS: usize = 30;
    const MAX_BYTES: usize = 512 * 1024 * 1024;

    fn put(&mut self, id: String, body: Arc<Vec<u8>>) {
        self.bytes += body.len();
        self.items.push_back((id, body));
        while self.items.len() > Self::MAX_ITEMS || (self.bytes > Self::MAX_BYTES && self.items.len() > 1) {
            if let Some((_, b)) = self.items.pop_front() {
                self.bytes -= b.len();
            }
        }
    }

    fn get(&self, id: &str) -> Option<Arc<Vec<u8>>> {
        self.items.iter().find(|(i, _)| i == id).map(|(_, b)| b.clone())
    }
}

struct Inner {
    data_dir: PathBuf,
    sink: Arc<dyn EventSink>,
    client: Arc<Client>,
    settings: RwLock<Settings>,
    workspace: RwLock<Option<Workspace>>,
    local: Mutex<LocalState>,
    secrets: SecretStore,
    /// Values scripts set on environments, workspaces and globals.
    local_values: LocalValues,
    history: Option<History>,
    tokens: TokenCache,
    jars: Mutex<HashMap<String, Arc<CookieJar>>>,
    /// Cancellation handles keyed by caller id, tagged with a generation so a
    /// finished request never removes a newer one with the same id.
    inflight: Mutex<HashMap<String, (u64, CancellationToken)>>,
    ws_sessions: Mutex<HashMap<String, WsEntry>>,
    socket_sessions: Mutex<HashMap<String, sockets::SocketEntry>>,
    servers: servers::Manager,
    load: load::LoadState,
    /// Introspected GraphQL schemas (`graphql.schema`).
    graphql: graphql::SchemaCache,
    /// gRPC descriptor cache and streaming calls (`grpc.*`).
    grpc: grpc::GrpcState,
    /// The collection run in progress and the last finished ones (`runner.*`).
    runner: runner::RunnerState,
    /// Registered before connecting so `sse.close` can abort a pending connect.
    sse_streams: Mutex<HashMap<String, (u64, CancellationToken)>>,
    generation: std::sync::atomic::AtomicU64,
    responses: Mutex<ResponseStore>,
    watcher: Mutex<Option<watcher::Watcher>>,
    /// Network tool runs (TLS inspect, port check, ping) by run id, for `tools.cancel`.
    tool_runs: Mutex<HashMap<String, (u64, CancellationToken)>>,
    /// AI agents connected over MCP (`agent.*`).
    agents: agents::AgentHub,
}

/// A WebSocket by caller id. Registered before connecting (`session` is `None`
/// until then) so `ws.close` can abort a pending connect; the generation keeps
/// an old connection from removing a newer one with the same id.
struct WsEntry {
    generation: u64,
    cancel: CancellationToken,
    session: Option<WsSession>,
}

#[derive(Clone)]
pub struct Api {
    inner: Arc<Inner>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Remove `id` unless a newer call has replaced it since.
fn remove_if_current<V>(map: &Mutex<HashMap<String, V>>, id: &str, is_current: impl FnOnce(&V) -> bool) {
    let mut map = lock(map);
    if map.get(id).is_some_and(is_current) {
        map.remove(id);
    }
}

fn params<T: DeserializeOwned>(value: Value) -> ApiResult<T> {
    let value = if value.is_null() { json!({}) } else { value };
    serde_json::from_value(value).map_err(|e| ApiError::invalid(format!("Invalid parameters: {e}")))
}

fn ok<T: serde::Serialize>(value: T) -> ApiResult<Value> {
    serde_json::to_value(value).map_err(|e| ApiError::new("internal", e.to_string()))
}

fn now_ms() -> i64 {
    (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000) as i64
}

/// Request options from the settings; during an AI agent's call, limited to the
/// hosts the user approved (redirects and OAuth token requests included).
fn request_options(
    settings: &Settings,
    overrides: &zorvik_workspace::formats::RequestSettings,
) -> ApiResult<zorvik_engine::RequestOptions> {
    let mut opts = settings.request_options(overrides)?;
    opts.host_guard = agents::scope_guard();
    Ok(opts)
}

/// The `zorvik` command-line tool installed with the app (next to its executable).
pub fn cli_path() -> Option<PathBuf> {
    // An AppImage runs from a temporary mount: a path there stops working when it closes.
    if std::env::var_os("APPIMAGE").is_some() {
        return None;
    }
    let exe = std::env::current_exe().ok()?;
    // macOS runs an app from a disk image or a quarantine copy (App Translocation) in
    // places that disappear: a path there would break agents' setup and the PATH link.
    let path = exe.to_string_lossy();
    if cfg!(target_os = "macos") && (path.starts_with("/Volumes/") || path.contains("/AppTranslocation/")) {
        return None;
    }
    let cli = exe.parent()?.join(format!("zorvik{}", std::env::consts::EXE_SUFFIX));
    // The app itself may be `zorvik` (CLI tests); only a separate file counts.
    (cli.is_file() && cli != exe).then_some(cli)
}

/// Where macOS gets the `zorvik` command (the app bundle can't add itself to PATH).
#[cfg(target_os = "macos")]
const MAC_CLI_LINK: &str = "/usr/local/bin/zorvik";

/// Whether typing `zorvik` in a terminal runs this CLI.
fn cli_on_path(cli: &Path) -> bool {
    let same = |p: &Path| std::fs::canonicalize(p).ok() == std::fs::canonicalize(cli).ok();
    // Apps started from the Dock have a short PATH: look where terminals look.
    #[cfg(target_os = "macos")]
    if same(Path::new(MAC_CLI_LINK)) {
        return true;
    }
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|dir| same(&dir.join(cli.file_name().unwrap_or_default())))
    })
}

/// macOS: link `zorvik` into /usr/local/bin (asks for an administrator password).
/// The Windows installer and the Linux packages put it on PATH themselves.
fn install_cli() -> ApiResult<()> {
    let Some(cli) = cli_path() else {
        return Err(ApiError::new("notFound", "The zorvik command-line tool is not installed with this app"));
    };
    #[cfg(target_os = "macos")]
    {
        let quoted = |s: &str| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""));
        let script = format!(
            "do shell script \"mkdir -p /usr/local/bin && ln -sfn \" & quoted form of {} & \" {MAC_CLI_LINK}\" with administrator privileges",
            quoted(&cli.to_string_lossy())
        );
        let out = std::process::Command::new("osascript")
            .args(["-e", &script])
            .output()
            .map_err(|e| ApiError::new("io", format!("Could not run osascript: {e}")))?;
        if !out.status.success() {
            let message = String::from_utf8_lossy(&out.stderr);
            return Err(ApiError::new(
                if message.contains("-128") { "cancelled" } else { "io" },
                format!("Could not add zorvik to PATH: {}", message.trim()),
            ));
        }
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = cli;
        Err(ApiError::invalid("The installer puts zorvik on PATH on this system"))
    }
}

const MAX_DISPLAY_TEXT: usize = 10 * 1024 * 1024;
const MAX_PRETTY: usize = 5 * 1024 * 1024;
const MAX_IMAGE_PREVIEW: usize = 8 * 1024 * 1024;
const MAX_IMPORT_FILE: u64 = 50 * 1024 * 1024;
const MASK: &str = "••••••";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PathParam {
    path: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SendParams {
    /// Caller-chosen id used to cancel the request.
    request_id: String,
    request: Request,
    /// Saved location of the request (for folder auth/header inheritance).
    path: Option<String>,
    /// Send the request exactly as given: no workspace variables, headers,
    /// auth or cookies, and no history entry (tools that check other sites).
    #[serde(default)]
    standalone: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AuthParams {
    auth: Auth,
    path: Option<String>,
}

impl Api {
    /// `data_dir` holds settings, history, secrets, cookies and tokens.
    pub fn new(data_dir: PathBuf, sink: Arc<dyn EventSink>) -> Self {
        let _ = std::fs::create_dir_all(&data_dir);
        let history = match History::open(&data_dir.join("history.sqlite3")) {
            Ok(h) => Some(h),
            Err(e) => {
                tracing::error!("history disabled: {}", e.message);
                None
            }
        };
        let inner = Inner {
            settings: RwLock::new(Settings::load(&data_dir.join("settings.json"))),
            local: Mutex::new(LocalState::load(&data_dir.join("state.json"))),
            secrets: SecretStore::open(data_dir.join("secrets.json")),
            local_values: LocalValues::open(data_dir.join("local-values.json")),
            tokens: TokenCache::persistent(data_dir.join("oauth-tokens.json")),
            history,
            data_dir,
            sink,
            client: Arc::new(Client::new()),
            workspace: RwLock::new(None),
            jars: Mutex::new(HashMap::new()),
            inflight: Mutex::new(HashMap::new()),
            ws_sessions: Mutex::new(HashMap::new()),
            socket_sessions: Mutex::new(HashMap::new()),
            servers: servers::Manager::default(),
            load: load::LoadState::default(),
            graphql: Default::default(),
            grpc: Default::default(),
            runner: Default::default(),
            sse_streams: Mutex::new(HashMap::new()),
            responses: Mutex::new(ResponseStore::default()),
            watcher: Mutex::new(None),
            tool_runs: Mutex::new(HashMap::new()),
            agents: Default::default(),
            generation: Default::default(),
        };
        Self { inner: Arc::new(inner) }
    }

    /// Dispatch one RPC call.
    pub async fn call(&self, method: &str, p: Value) -> ApiResult<Value> {
        // The dispatch future holds the state of every method (tens of KB).
        // Boxed, callers' futures stay small; unoptimized builds copy a future
        // onto the stack at each await, which overflowed test threads.
        Box::pin(self.dispatch(method, p)).await
    }

    async fn dispatch(&self, method: &str, p: Value) -> ApiResult<Value> {
        match method.split_once('.').map_or("", |(group, _)| group) {
            "server" => return self.call_server(method, p).await,
            "socket" => return self.call_socket(method, p).await,
            "tools" => return self.call_tool(method, p).await,
            "dns" => return self.call_dns(method, p).await,
            "mock" => return self.call_mock(method, p).await,
            "load" => return self.call_load(method, p).await,
            "graphql" => return self.call_graphql(method, p).await,
            "grpc" => return self.call_grpc(method, p).await,
            "runner" => return self.call_runner(method, p).await,
            "agent" => return self.call_agent(method, p).await,
            _ => {}
        }
        match method {
            "app.info" => ok(self.app_info()),
            "app.installCli" => {
                tokio::task::spawn_blocking(install_cli).await.map_err(join_err)??;
                ok(self.app_info())
            }
            "settings.get" => ok(self.settings()),
            "settings.save" => {
                #[derive(Deserialize)]
                struct P {
                    settings: Settings,
                }
                let P { settings } = params(p)?;
                zorvik_engine::ProxySettings::from_mode(&settings.proxy).map_err(|e| ApiError::invalid(e.message))?;
                settings.save(&self.inner.data_dir.join("settings.json"))?;
                *self.inner.settings.write().unwrap_or_else(|e| e.into_inner()) = settings;
                ok(())
            }

            "workspace.recent" => ok(lock(&self.inner.local).recent.clone()),
            "workspace.removeRecent" => {
                let PathParam { path } = params(p)?;
                let mut local = lock(&self.inner.local);
                local.recent.retain(|r| r.path != path);
                local.save(&self.inner.data_dir.join("state.json"));
                ok(())
            }
            "workspace.create" => {
                #[derive(Deserialize)]
                struct P {
                    path: String,
                    name: String,
                }
                let P { path, name } = params(p)?;
                let ws = Workspace::create(Path::new(&path), &name)?;
                ok(self.activate(ws)?)
            }
            "workspace.open" => {
                let PathParam { path } = params(p)?;
                let ws = Workspace::open(Path::new(&path))?;
                ok(self.activate(ws)?)
            }
            "workspace.current" => match self.try_ws() {
                Some(ws) => ok(Some(self.workspace_info(&ws)?)),
                None => ok(None::<WorkspaceInfo>),
            },
            "workspace.close" => {
                *self.inner.workspace.write().unwrap_or_else(|e| e.into_inner()) = None;
                *lock(&self.inner.watcher) = None;
                let mut local = lock(&self.inner.local);
                local.last_workspace = None;
                local.save(&self.inner.data_dir.join("state.json"));
                ok(())
            }
            "workspace.reload" => {
                let mut ws = self.ws()?;
                ws.reload_meta()?;
                self.update_ws(&ws);
                ok(self.workspace_info(&ws)?)
            }
            "workspace.tree" => ok(self.ws()?.tree()?),
            "workspace.saveMeta" => {
                #[derive(Deserialize)]
                struct P {
                    meta: WorkspaceMeta,
                }
                let P { mut meta } = params(p)?;
                let mut ws = self.ws()?;
                meta.variables = self.inner.secrets.conceal(&workspace_scope(&ws.local_key()), &meta.variables)?;
                ws.save_meta(meta)?;
                if self.update_ws(&ws) {
                    self.touch_recent(&ws);
                }
                ok(self.revealed_meta(&ws))
            }

            "request.read" => {
                let PathParam { path } = params(p)?;
                ok(self.ws()?.read_request(&path)?)
            }
            "request.save" => {
                #[derive(Deserialize)]
                struct P {
                    path: String,
                    request: Request,
                }
                let P { path, request } = params(p)?;
                ok(self.ws()?.save_request(&path, &request)?)
            }
            "request.create" => {
                #[derive(Deserialize)]
                struct P {
                    parent: String,
                    request: Request,
                }
                let P { parent, request } = params(p)?;
                ok(self.ws()?.create_request(&parent, request)?)
            }
            "folder.create" => {
                #[derive(Deserialize)]
                struct P {
                    parent: String,
                    name: String,
                }
                let P { parent, name } = params(p)?;
                ok(self.ws()?.create_folder(&parent, &name)?)
            }
            "folder.read" => {
                let PathParam { path } = params(p)?;
                ok(self.ws()?.read_folder(&path)?)
            }
            "folder.save" => {
                #[derive(Deserialize)]
                struct P {
                    path: String,
                    meta: FolderMeta,
                }
                let P { path, meta } = params(p)?;
                ok(self.ws()?.save_folder(&path, &meta)?)
            }
            "item.rename" => {
                #[derive(Deserialize)]
                struct P {
                    path: String,
                    name: String,
                }
                let P { path, name } = params(p)?;
                let ws = self.ws()?;
                let new_path = ws.rename(&path, &name)?;
                self.retarget_load_tests(&ws, &path, &new_path);
                ok(new_path)
            }
            "item.delete" => {
                let PathParam { path } = params(p)?;
                let ws = self.ws()?;
                ok(tokio::task::spawn_blocking(move || ws.delete(&path)).await.map_err(join_err)??)
            }
            "item.duplicate" => {
                let PathParam { path } = params(p)?;
                ok(self.ws()?.duplicate(&path)?)
            }
            "item.move" => {
                #[derive(Deserialize)]
                struct P {
                    path: String,
                    parent: String,
                    index: Option<usize>,
                }
                let P { path, parent, index } = params(p)?;
                let ws = self.ws()?;
                let new_path = ws.move_item(&path, &parent, index)?;
                self.retarget_load_tests(&ws, &path, &new_path);
                ok(new_path)
            }

            "env.list" => ok(self.environments(&self.ws()?)?),
            "env.create" => {
                #[derive(Deserialize)]
                struct P {
                    environment: Environment,
                }
                let P { mut environment } = params(p)?;
                let ws = self.ws()?;
                let secret_vars = environment.variables.clone();
                environment.variables = secret_vars
                    .iter()
                    .map(|v| if v.secret { Variable { value: String::new(), ..v.clone() } } else { v.clone() })
                    .collect();
                let id = ws.create_environment(&environment)?;
                self.inner.secrets.conceal(&env_scope(&ws.local_key(), &id), &secret_vars)?;
                ok(id)
            }
            "env.save" => {
                #[derive(Deserialize)]
                struct P {
                    id: String,
                    environment: Environment,
                }
                let P { id, mut environment } = params(p)?;
                let ws = self.ws()?;
                let ws_id = ws.meta().id.clone();
                let key = ws.local_key();
                environment.variables = self.inner.secrets.conceal(&env_scope(&key, &id), &environment.variables)?;
                let new_id = ws.save_environment(&id, &environment)?;
                if new_id != id {
                    self.inner.secrets.rename_scope(&env_scope(&key, &id), &env_scope(&key, &new_id))?;
                    self.inner.local_values.rename_scope(&env_scope(&key, &id), &env_scope(&key, &new_id))?;
                    let mut local = lock(&self.inner.local);
                    if local.active_env.get(&ws_id) == Some(&id) {
                        local.active_env.insert(ws_id, new_id.clone());
                        local.save(&self.inner.data_dir.join("state.json"));
                    }
                }
                ok(new_id)
            }
            "env.delete" => {
                #[derive(Deserialize)]
                struct P {
                    id: String,
                }
                let P { id } = params(p)?;
                let ws = self.ws()?;
                let ws_id = ws.meta().id.clone();
                let key = ws.local_key();
                let id2 = id.clone();
                tokio::task::spawn_blocking(move || ws.delete_environment(&id2)).await.map_err(join_err)??;
                self.inner.secrets.remove_scope(&env_scope(&key, &id))?;
                self.inner.local_values.clear(&env_scope(&key, &id), None)?;
                let mut local = lock(&self.inner.local);
                if local.active_env.get(&ws_id) == Some(&id) {
                    local.active_env.remove(&ws_id);
                    local.save(&self.inner.data_dir.join("state.json"));
                }
                ok(())
            }
            "env.setActive" => {
                #[derive(Deserialize)]
                struct P {
                    id: Option<String>,
                }
                let P { id } = params(p)?;
                let ws = self.ws()?;
                if let Some(id) = &id {
                    ws.read_environment(id)?;
                }
                let mut local = lock(&self.inner.local);
                match id {
                    Some(id) => local.active_env.insert(ws.meta().id.clone(), id),
                    None => local.active_env.remove(&ws.meta().id),
                };
                local.save(&self.inner.data_dir.join("state.json"));
                ok(())
            }
            "vars.list" => ok(self.variable_infos(&self.ws()?)),
            "vars.local" => ok(self.local_values()),
            "vars.clearLocal" => ok(self.clear_local_values(params(p)?)?),
            "vars.render" => {
                #[derive(Deserialize)]
                struct P {
                    text: String,
                }
                let P { text } = params(p)?;
                let ws = self.ws()?;
                let mut missing = std::collections::BTreeSet::new();
                ok(self.var_context(&ws).render(&text, &mut missing))
            }

            "http.send" => ok(self.http_send(params(p)?).await?),
            "http.cancel" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct P {
                    request_id: String,
                }
                let P { request_id } = params(p)?;
                if let Some((_, token)) = lock(&self.inner.inflight).remove(&request_id) {
                    token.cancel();
                }
                ok(())
            }
            "response.save" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct P {
                    response_id: String,
                    path: String,
                }
                let P { response_id, path } = params(p)?;
                let body = lock(&self.inner.responses).get(&response_id).ok_or_else(|| {
                    ApiError::new("notFound", "That response is no longer available; send the request again")
                })?;
                let target = PathBuf::from(&path);
                tokio::task::spawn_blocking(move || std::fs::write(&target, body.as_slice()))
                    .await
                    .map_err(join_err)?
                    .map_err(|e| ApiError::new("io", format!("Could not save to {path}: {e}")))?;
                ok(())
            }

            "ws.connect" => ok(self.ws_connect(params(p)?).await?),
            "ws.send" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct P {
                    conn_id: String,
                    message: WsOutgoing,
                }
                let P { conn_id, message } = params(p)?;
                let sessions = lock(&self.inner.ws_sessions);
                let session = sessions
                    .get(&conn_id)
                    .and_then(|e| e.session.as_ref())
                    .ok_or_else(|| ApiError::new("notFound", "WebSocket is not connected"))?;
                session.send(message)?;
                ok(())
            }
            "ws.close" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct P {
                    conn_id: String,
                    code: Option<u16>,
                    reason: Option<String>,
                }
                let P { conn_id, code, reason } = params(p)?;
                let mut sessions = lock(&self.inner.ws_sessions);
                if let Some(entry) = sessions.get(&conn_id) {
                    match &entry.session {
                        Some(session) => {
                            let _ = session.send(WsOutgoing::Close { code, reason });
                        }
                        // Still connecting: abort it.
                        None => {
                            entry.cancel.cancel();
                            sessions.remove(&conn_id);
                        }
                    }
                }
                ok(())
            }
            "sse.connect" => ok(self.sse_connect(params(p)?).await?),
            "sse.close" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct P {
                    conn_id: String,
                }
                let P { conn_id } = params(p)?;
                if let Some((_, token)) = lock(&self.inner.sse_streams).remove(&conn_id) {
                    token.cancel();
                }
                ok(())
            }

            "oauth2.getToken" => ok(self.oauth2_get_token(params(p)?).await?),
            "oauth2.status" => {
                let AuthParams { auth, path } = params(p)?;
                let config = self.oauth2_config(&auth, path.as_deref())?;
                ok(token_status(self.inner.tokens.get(&oauth2::cache_key(&self.ws()?.local_key(), &config))))
            }
            "oauth2.clear" => {
                let AuthParams { auth, path } = params(p)?;
                let config = self.oauth2_config(&auth, path.as_deref())?;
                self.inner.tokens.remove(&oauth2::cache_key(&self.ws()?.local_key(), &config));
                ok(())
            }

            "history.list" => {
                #[derive(Deserialize)]
                struct P {
                    #[serde(default)]
                    search: String,
                    limit: Option<u32>,
                    offset: Option<u32>,
                }
                let P { search, limit, offset } = params(p)?;
                let ws = self.ws()?;
                let Some(history) = &self.inner.history else { return ok(Vec::<()>::new()) };
                ok(history.list(
                    &ws.root().to_string_lossy(),
                    &search,
                    limit.unwrap_or(200).min(1000),
                    offset.unwrap_or(0),
                )?)
            }
            "history.delete" => {
                #[derive(Deserialize)]
                struct P {
                    id: i64,
                }
                let P { id } = params(p)?;
                if let Some(history) = &self.inner.history {
                    history.delete(id)?;
                }
                ok(())
            }
            "history.clear" => {
                let ws = self.ws()?;
                if let Some(history) = &self.inner.history {
                    history.clear(&ws.root().to_string_lossy())?;
                }
                ok(())
            }

            "cookies.list" => ok(self.jar(&self.ws()?).list()),
            "cookies.delete" => {
                #[derive(Deserialize)]
                struct P {
                    domain: String,
                    path: String,
                    name: String,
                }
                let P { domain, path, name } = params(p)?;
                let ws = self.ws()?;
                let jar = self.jar(&ws);
                jar.remove(&domain, &path, &name);
                self.save_jar(&ws, &jar);
                ok(())
            }
            "cookies.clear" => {
                let ws = self.ws()?;
                let jar = self.jar(&ws);
                jar.clear();
                self.save_jar(&ws, &jar);
                ok(())
            }

            "import.curl" => {
                #[derive(Deserialize)]
                struct P {
                    text: String,
                }
                let P { text } = params(p)?;
                let parsed = parse_curl(&text)?;
                ok(CurlImportResult { request: parsed.request, warnings: parsed.warnings })
            }
            "import.file" => ok(self.import_file(params(p)?).await?),
            "import.url" => ok(self.import_url(params(p)?).await?),
            "export.curl" => ok(self.export_curl(params(p)?)?),

            other => Err(ApiError::new("notFound", format!("Unknown method '{other}'"))),
        }
    }

    // ---- state helpers ------------------------------------------------------

    fn next_generation(&self) -> u64 {
        self.inner.generation.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    fn settings(&self) -> Settings {
        let mut settings = self.inner.settings.read().unwrap_or_else(|e| e.into_inner()).clone();
        // An AI agent's requests, runs and gRPC files stay inside the workspace folder.
        if agents::in_agent_call() {
            settings.files_outside_workspace = false;
        }
        settings
    }

    fn try_ws(&self) -> Option<Workspace> {
        self.inner.workspace.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn ws(&self) -> ApiResult<Workspace> {
        self.try_ws().ok_or_else(|| ApiError::new("noWorkspace", "No workspace is open"))
    }

    /// Store an updated copy of the open workspace, unless it was closed or
    /// another one was opened meanwhile. Returns whether it was stored.
    fn update_ws(&self, ws: &Workspace) -> bool {
        let mut current = self.inner.workspace.write().unwrap_or_else(|e| e.into_inner());
        let same = current.as_ref().is_some_and(|c| c.root() == ws.root());
        if same {
            *current = Some(ws.clone());
        }
        same
    }

    fn app_info(&self) -> AppInfo {
        AppInfo {
            version: env!("CARGO_PKG_VERSION").into(),
            build: option_env!("ZORVIK_BUILD").map(str::to_string),
            data_dir: self.inner.data_dir.to_string_lossy().into_owned(),
            platform: std::env::consts::OS.into(),
            dynamic_variables: DYNAMIC_VARIABLES.iter().map(|s| s.to_string()).collect(),
            cli_path: cli_path().map(|p| p.to_string_lossy().into_owned()),
            cli_on_path: cli_path().is_some_and(|p| cli_on_path(&p)),
        }
    }

    /// Reopen the last workspace from a previous session, if it still exists.
    pub fn restore_last_workspace(&self) {
        let last = lock(&self.inner.local).last_workspace.clone();
        if let Some(path) = last
            && let Ok(ws) = Workspace::open(Path::new(&path))
        {
            let _ = self.activate(ws);
        }
    }

    fn activate(&self, ws: Workspace) -> ApiResult<WorkspaceInfo> {
        self.adopt_local_data(&ws);
        let info = self.workspace_info(&ws)?;
        *self.inner.workspace.write().unwrap_or_else(|e| e.into_inner()) = Some(ws.clone());
        *lock(&self.inner.watcher) = watcher::Watcher::start(ws.root(), self.inner.sink.clone())
            .map_err(|e| tracing::warn!("file watching disabled: {e}"))
            .ok();
        self.touch_recent(&ws);
        Ok(info)
    }

    /// Secrets and cookies used to be keyed by the workspace id alone; move them to the
    /// id-and-folder key (the first folder opened with that id keeps them).
    fn adopt_local_data(&self, ws: &Workspace) {
        let (id, key) = (&ws.meta().id, ws.local_key());
        if let Err(e) = self.inner.secrets.move_prefix(id, &key) {
            tracing::warn!("could not move secrets: {}", e.message);
        }
        let cookies = self.inner.data_dir.join("cookies");
        let (old, new) = (cookies.join(format!("{id}.json")), cookies.join(format!("{key}.json")));
        if old.exists() && !new.exists() {
            let _ = std::fs::rename(old, new);
        }
    }

    fn touch_recent(&self, ws: &Workspace) {
        let path = ws.root().to_string_lossy().into_owned();
        let mut local = lock(&self.inner.local);
        local.recent.retain(|r| r.path != path);
        local
            .recent
            .insert(0, RecentWorkspace { path: path.clone(), name: ws.meta().name.clone(), opened_at: now_ms() });
        local.recent.truncate(12);
        local.last_workspace = Some(path);
        local.save(&self.inner.data_dir.join("state.json"));
    }

    fn revealed_meta(&self, ws: &Workspace) -> WorkspaceMeta {
        let mut meta = ws.meta().clone();
        self.inner.secrets.reveal(&workspace_scope(&ws.local_key()), &mut meta.variables);
        meta
    }

    fn environments(&self, ws: &Workspace) -> ApiResult<Vec<zorvik_workspace::EnvironmentEntry>> {
        let mut envs = ws.list_environments()?;
        for e in &mut envs {
            self.inner.secrets.reveal(&env_scope(&ws.local_key(), &e.id), &mut e.environment.variables);
        }
        Ok(envs)
    }

    fn active_env_id(&self, ws: &Workspace) -> Option<String> {
        lock(&self.inner.local).active_env.get(&ws.meta().id).cloned()
    }

    fn workspace_info(&self, ws: &Workspace) -> ApiResult<WorkspaceInfo> {
        let environments = self.environments(ws)?;
        let active = self.active_env_id(ws).filter(|id| environments.iter().any(|e| &e.id == id));
        Ok(WorkspaceInfo {
            path: ws.root().to_string_lossy().into_owned(),
            meta: self.revealed_meta(ws),
            tree: ws.tree()?,
            environments,
            active_environment: active,
        })
    }

    /// Active environment variables, with secrets revealed and script-set local values applied.
    fn active_env_vars(&self, ws: &Workspace) -> Vec<Variable> {
        let Some(id) = self.active_env_id(ws) else { return Vec::new() };
        let Ok(env) = ws.read_environment(&id) else { return Vec::new() };
        let mut vars = env.variables;
        let scope = env_scope(&ws.local_key(), &id);
        self.inner.secrets.reveal(&scope, &mut vars);
        overlay(&vars, &self.inner.local_values.get(&scope))
    }

    fn var_context(&self, ws: &Workspace) -> VarContext {
        let mut ctx = VarContext::new();
        ctx.push_layer(&self.active_env_vars(ws));
        ctx.push_layer(&self.workspace_vars(ws));
        ctx.push_layer(&self.global_vars());
        ctx
    }

    fn variable_infos(&self, ws: &Workspace) -> Vec<VariableInfo> {
        let mut out: Vec<VariableInfo> = Vec::new();
        let key = ws.local_key();
        let env_local = self.active_env_id(ws).map(|id| self.inner.local_values.get(&env_scope(&key, &id)));
        let layers = [
            ("environment", self.active_env_vars(ws), env_local.unwrap_or_default()),
            ("workspace", self.workspace_vars(ws), self.inner.local_values.get(&workspace_scope(&key))),
            ("globals", self.global_vars(), self.inner.local_values.get(GLOBALS)),
        ];
        for (source, vars, local) in layers {
            for v in vars.into_iter().filter(|v| v.enabled && !v.key.trim().is_empty()) {
                if out.iter().any(|o| o.key == v.key) {
                    continue;
                }
                out.push(VariableInfo {
                    local: local.contains_key(v.key.trim()),
                    key: v.key,
                    value: if v.secret { MASK.into() } else { v.value },
                    source: source.into(),
                    secret: v.secret,
                });
            }
        }
        out
    }

    fn jar(&self, ws: &Workspace) -> Arc<CookieJar> {
        let key = ws.local_key();
        lock(&self.inner.jars)
            .entry(key.clone())
            .or_insert_with(|| {
                let path = self.inner.data_dir.join("cookies").join(format!("{key}.json"));
                Arc::new(std::fs::read(path).map(|d| CookieJar::from_json(&d)).unwrap_or_default())
            })
            .clone()
    }

    fn save_jar(&self, ws: &Workspace, jar: &CookieJar) {
        let path = self.inner.data_dir.join("cookies").join(format!("{}.json", ws.local_key()));
        if let Err(e) = zorvik_workspace::fsutil::atomic_write(&path, &jar.to_json()) {
            tracing::warn!("could not save cookies: {}", e.message);
        }
    }

    /// Resolve a request against the open workspace.
    fn prepare(&self, ws: &Workspace, request: &Request, path: Option<&str>) -> ApiResult<Resolved> {
        let resolved = self.resolve_only(ws, request, path)?;
        check_url_variables(&resolved)?;
        Ok(resolved)
    }

    /// Resolve with nothing from the workspace (only dynamic variables such as `{{$uuid}}`).
    fn resolve_standalone(&self, ws: &Workspace, request: &Request) -> ApiResult<Resolved> {
        let meta = WorkspaceMeta {
            version: zorvik_workspace::formats::FORMAT_VERSION,
            id: String::new(),
            name: String::new(),
            variables: Vec::new(),
            auth: Auth::None,
            headers: Vec::new(),
            scripts: Default::default(),
            docs: String::new(),
        };
        let inherit = Inheritance { workspace: &meta, folders: &[], base_dir: ws.root(), outside_files: false };
        Ok(resolve(request, &inherit, &VarContext::new())?)
    }

    /// Resolve without refusing undefined URL variables (used for export).
    fn resolve_only(&self, ws: &Workspace, request: &Request, path: Option<&str>) -> ApiResult<Resolved> {
        let folders = path.map(|p| ws.ancestors(p)).unwrap_or_default();
        let meta = self.revealed_meta(ws);
        let outside_files = self.settings().files_outside_workspace;
        let inherit = Inheritance { workspace: &meta, folders: &folders, base_dir: ws.root(), outside_files };
        Ok(resolve(request, &inherit, &self.var_context(ws))?)
    }

    async fn authorize(
        &self,
        ws: &Workspace,
        resolved: &mut Resolved,
        opts: &zorvik_engine::RequestOptions,
    ) -> ApiResult<()> {
        if let Some(config) = resolved.oauth2.clone() {
            let token =
                oauth2::ensure_token(&self.inner.client, opts, &config, &self.inner.tokens, &ws.local_key()).await?;
            apply_token(resolved, &token.access_token);
        }
        Ok(())
    }

    // ---- HTTP -----------------------------------------------------------------

    async fn http_send(&self, p: SendParams) -> ApiResult<SendResult> {
        let ws = self.ws()?;
        let settings = self.settings();
        let started = std::time::Instant::now();
        let cancel = CancellationToken::new();
        let generation = self.next_generation();
        if let Some((_, previous)) =
            lock(&self.inner.inflight).insert(p.request_id.clone(), (generation, cancel.clone()))
        {
            previous.cancel();
        }

        // Tool requests (standalone) run no scripts.
        let mut vars = (!p.standalone).then(|| self.script_vars(&ws));
        let mut report = None;
        let outcome = async {
            let jar = (settings.cookie_jar && !p.standalone).then(|| self.jar(&ws));
            let Some(vars) = vars.as_mut() else {
                let mut resolved = self.resolve_standalone(&ws, &p.request)?;
                let opts = request_options(&settings, &p.request.settings)?;
                return tokio::select! {
                    r = async {
                        self.authorize(&ws, &mut resolved, &opts).await?;
                        let url = resolved.request.url.clone();
                        let response = self.inner.client.send(resolved.request.clone(), &opts, jar.as_deref()).await?;
                        Ok::<_, ApiError>((url, response))
                    } => r.map(|(url, response)| (url, response, resolved.unresolved, jar)),
                    _ = cancel.cancelled() => Err(EngineError::cancelled().into()),
                };
            };
            let meta = self.revealed_meta(&ws);
            let cx = SendContext {
                ws: &ws,
                meta: &meta,
                client: &self.inner.client,
                settings: &settings,
                tokens: &self.inner.tokens,
                jar: jar.as_deref(),
                guard: agents::scope_guard(),
            };
            let scripted = tokio::select! {
                // Boxed: the whole pipeline is a big future (debug builds would overflow the stack).
                r = Box::pin(send_scripted(&cx, p.request.clone(), p.path.as_deref(), vars, Iteration::default())) => r,
                _ = cancel.cancelled() => return Err(EngineError::cancelled().into()),
            };
            report = scripted.report;
            let sent = scripted.result?;
            Ok((sent.url, sent.response, sent.unresolved, jar))
        }
        .await;
        {
            let mut inflight = lock(&self.inner.inflight);
            if inflight.get(&p.request_id).is_some_and(|(g, _)| *g == generation) {
                inflight.remove(&p.request_id);
            }
        }
        // Secret values as they were when the request went out are hidden in history
        // too: a post-response script may change them (e.g. rotate a key) first.
        let history_url = match &outcome {
            Ok((url, ..)) if vars.as_ref().is_some_and(|v| !v.changes.is_empty()) => Some(self.history_url(&ws, url)),
            _ => None,
        };
        if let Some(vars) = &vars
            && let Some(warning) = self.keep_local_values(&ws, vars)
            && let Some(report) = &mut report
        {
            report
                .console
                .push(zorvik_script::ConsoleEntry { level: zorvik_script::ConsoleLevel::Warn, message: warning });
        }

        let workspace_key = ws.root().to_string_lossy().into_owned();
        match outcome {
            // Tool requests leave no trace in history or the cookie jar.
            Ok((_, response, unresolved, _)) if p.standalone => Ok(self.send_result(response, unresolved)),
            Err(err) if p.standalone => Err(err),
            Ok((url, response, unresolved, jar)) => {
                if let Some(jar) = jar {
                    self.save_jar(&ws, &jar);
                }
                self.record_history(
                    &workspace_key,
                    &p,
                    &self.history_url(&ws, history_url.as_deref().unwrap_or(&url)),
                    Some(response.meta.status),
                    None,
                    Some(response.timing.total_ms),
                    Some(response.body.len() as i64),
                    settings.history_limit,
                );
                Ok(SendResult { scripts: report, ..self.send_result(response, unresolved) })
            }
            Err(err) => {
                // A pre-request script error means nothing was sent.
                if err.network_kind != Some(ErrorKind::Cancelled) && err.code != "script" {
                    let elapsed = started.elapsed().as_secs_f64() * 1000.0;
                    self.record_history(
                        &workspace_key,
                        &p,
                        &p.request.url,
                        None,
                        Some(&err.message),
                        Some(elapsed),
                        None,
                        settings.history_limit,
                    );
                }
                Err(err)
            }
        }
    }

    /// The sent URL as history shows it: secret variable values (e.g. an API key in
    /// the query) are put back as `{{name}}` so they never land in the history file.
    fn history_url(&self, ws: &Workspace, url: &str) -> String {
        let mut url = url.to_string();
        let layers = [self.active_env_vars(ws), self.workspace_vars(ws)];
        for v in layers.iter().flatten().filter(|v| v.secret && v.value.len() >= 3) {
            let encoded: String = url::form_urlencoded::byte_serialize(v.value.as_bytes()).collect();
            for form in [&v.value, &encoded] {
                url = url.replace(form.as_str(), &format!("{{{{{}}}}}", v.key));
            }
        }
        url
    }

    #[allow(clippy::too_many_arguments)]
    fn record_history(
        &self,
        workspace: &str,
        p: &SendParams,
        url: &str,
        status: Option<u16>,
        error: Option<&str>,
        duration_ms: Option<f64>,
        size: Option<i64>,
        limit: u32,
    ) {
        let Some(history) = &self.inner.history else { return };
        let entry = NewEntry {
            workspace,
            request_path: p.path.as_deref(),
            url,
            status,
            error,
            duration_ms,
            size,
            request: &p.request,
        };
        if let Err(e) = history.add(entry, limit) {
            tracing::warn!("history write failed: {}", e.message);
        }
    }

    fn send_result(&self, response: HttpResponse, unresolved: Vec<String>) -> SendResult {
        let response_id = uuid::Uuid::new_v4().to_string();
        let content_type =
            response.meta.headers.iter().find(|h| h.name.eq_ignore_ascii_case("content-type")).map(|h| h.value.clone());
        let body = Arc::new(response.body);
        lock(&self.inner.responses).put(response_id.clone(), body.clone());
        SendResult {
            response_id,
            body: display_body(&body, content_type, response.body_truncated, response.decode_warning),
            meta: response.meta,
            timing: response.timing,
            body_wire_size: response.body_wire_size,
            unresolved,
            scripts: None,
        }
    }

    // ---- WebSocket / SSE ----------------------------------------------------

    async fn ws_connect(&self, p: StreamParams) -> ApiResult<StreamOpened> {
        let ws = self.ws()?;
        let settings = self.settings();
        let cancel = CancellationToken::new();
        let generation = self.next_generation();
        // Replacing an entry drops its session, which closes that connection.
        let entry = WsEntry { generation, cancel: cancel.clone(), session: None };
        if let Some(old) = lock(&self.inner.ws_sessions).insert(p.conn_id.clone(), entry) {
            old.cancel.cancel();
        }
        let connected = tokio::select! {
            r = async {
                let mut resolved = self.prepare(&ws, &p.request, p.path.as_deref())?;
                let opts = request_options(&settings, &p.request.settings)?;
                self.authorize(&ws, &mut resolved, &opts).await?;
                let jar = settings.cookie_jar.then(|| self.jar(&ws));
                let conn = self.inner.client.websocket(resolved.request, &opts, jar.as_deref()).await?;
                if let Some(jar) = &jar {
                    self.save_jar(&ws, jar);
                }
                Ok::<_, ApiError>((conn, resolved.unresolved))
            } => r,
            _ = cancel.cancelled() => Err(EngineError::cancelled().into()),
        };
        let (conn, unresolved) = match connected {
            Ok(c) => c,
            Err(e) => {
                remove_if_current(&self.inner.ws_sessions, &p.conn_id, |entry| entry.generation == generation);
                return Err(e);
            }
        };
        let zorvik_engine::WsConnected { meta, timing, session, mut events } = conn;
        match lock(&self.inner.ws_sessions).get_mut(&p.conn_id) {
            Some(entry) if entry.generation == generation => entry.session = Some(session),
            // Closed or replaced while connecting: dropping the session closes the socket.
            _ => return Err(EngineError::cancelled().into()),
        }
        let inner = self.inner.clone();
        let conn_id = p.conn_id.clone();
        tokio::spawn(async move {
            while let Some(event) = events.recv().await {
                let closed = matches!(event, zorvik_engine::WsEvent::Closed { .. });
                inner.sink.emit(StreamEvent::Ws { conn_id: conn_id.clone(), event });
                if closed {
                    break;
                }
            }
            remove_if_current(&inner.ws_sessions, &conn_id, |entry| entry.generation == generation);
        });
        Ok(StreamOpened { meta, timing, unresolved })
    }

    async fn sse_connect(&self, p: StreamParams) -> ApiResult<StreamOpened> {
        let ws = self.ws()?;
        let settings = self.settings();
        let cancel = CancellationToken::new();
        let generation = self.next_generation();
        if let Some((_, old)) = lock(&self.inner.sse_streams).insert(p.conn_id.clone(), (generation, cancel.clone())) {
            old.cancel();
        }
        let opened = tokio::select! {
            r = async {
                let mut resolved = self.prepare(&ws, &p.request, p.path.as_deref())?;
                let opts = request_options(&settings, &p.request.settings)?;
                self.authorize(&ws, &mut resolved, &opts).await?;
                if !resolved.request.headers.iter().any(|h| h.name.eq_ignore_ascii_case("accept")) {
                    resolved.request.headers.push(zorvik_engine::Header::new("Accept", "text/event-stream"));
                }
                if !resolved.request.headers.iter().any(|h| h.name.eq_ignore_ascii_case("cache-control")) {
                    resolved.request.headers.push(zorvik_engine::Header::new("Cache-Control", "no-cache"));
                }
                let jar = settings.cookie_jar.then(|| self.jar(&ws));
                let stream = self.inner.client.open_stream(resolved.request, &opts, jar.as_deref()).await?;
                if let Some(jar) = &jar {
                    self.save_jar(&ws, jar);
                }
                Ok::<_, ApiError>((stream, resolved.unresolved))
            } => r,
            _ = cancel.cancelled() => Err(EngineError::cancelled().into()),
        };
        let (stream, unresolved) = match opened {
            Ok(s) => s,
            Err(e) => {
                remove_if_current(&self.inner.sse_streams, &p.conn_id, |(g, _)| *g == generation);
                return Err(e);
            }
        };
        let zorvik_engine::StreamingResponse { meta, timing, mut body } = stream;
        let is_event_stream = meta.headers.iter().any(|h| {
            h.name.eq_ignore_ascii_case("content-type") && h.value.to_ascii_lowercase().contains("text/event-stream")
        });
        let ok_status = (200..300).contains(&meta.status);
        let inner = self.inner.clone();
        let conn_id = p.conn_id.clone();
        tokio::spawn(async move {
            let emit = |event: SseStreamEvent| inner.sink.emit(StreamEvent::Sse { conn_id: conn_id.clone(), event });
            if !ok_status || !is_event_stream {
                // Not an event stream: show the start of the body as an error.
                let mut text = Vec::new();
                tokio::select! {
                    _ = cancel.cancelled() => {}
                    _ = async {
                        while let Some(Ok(chunk)) = body.next_chunk().await {
                            text.extend_from_slice(&chunk);
                            if text.len() > 4096 {
                                break;
                            }
                        }
                    } => {}
                }
                let snippet: String = String::from_utf8_lossy(&text).chars().take(2000).collect();
                emit(SseStreamEvent::Error {
                    message: if ok_status {
                        format!(
                            "Response is not an event stream (Content-Type is not text/event-stream). Body: {snippet}"
                        )
                    } else {
                        format!("Server responded with an error. Body: {snippet}")
                    },
                });
                emit(SseStreamEvent::Closed { reason: "Stream ended".into() });
                remove_if_current(&inner.sse_streams, &conn_id, |(g, _)| *g == generation);
                return;
            }
            let mut parser = SseParser::new();
            let reason = loop {
                tokio::select! {
                    _ = cancel.cancelled() => break "Closed by you".to_string(),
                    chunk = body.next_chunk() => match chunk {
                        Some(Ok(bytes)) => {
                            for event in parser.feed(&bytes) {
                                emit(SseStreamEvent::Event { event, timestamp: now_ms() as f64 });
                            }
                        }
                        Some(Err(e)) => {
                            emit(SseStreamEvent::Error { message: e.message });
                            break "Connection lost".to_string();
                        }
                        None => break "Server closed the stream".to_string(),
                    }
                }
            };
            emit(SseStreamEvent::Closed { reason });
            remove_if_current(&inner.sse_streams, &conn_id, |(g, _)| *g == generation);
        });
        Ok(StreamOpened { meta, timing, unresolved })
    }

    // ---- OAuth 2.0 ------------------------------------------------------------

    fn oauth2_config(&self, auth: &Auth, path: Option<&str>) -> ApiResult<zorvik_workspace::formats::OAuth2Config> {
        let ws = self.ws()?;
        let folders = path.map(|p| ws.ancestors(p)).unwrap_or_default();
        let meta = self.revealed_meta(&ws);
        let inherit = Inheritance { workspace: &meta, folders: &folders, base_dir: ws.root(), outside_files: false };
        let Auth::OAuth2(config) = effective_auth(auth, &inherit) else {
            return Err(ApiError::invalid("This request does not use OAuth 2.0"));
        };
        let ctx = self.var_context(&ws);
        let mut missing = std::collections::BTreeSet::new();
        Ok(render_oauth2(config, &mut |s| ctx.render(s, &mut missing)))
    }

    async fn oauth2_get_token(&self, p: AuthParams) -> ApiResult<TokenStatus> {
        let config = self.oauth2_config(&p.auth, p.path.as_deref())?;
        let opts = request_options(&self.settings(), &Default::default())?;
        let sink = self.inner.sink.clone();
        let token = oauth2::authorization_code_flow(
            &self.inner.client,
            &opts,
            &config,
            &self.inner.tokens,
            &self.ws()?.local_key(),
            // The URL comes from (possibly shared) workspace files: only hand web URLs to the OS.
            move |url| match url::Url::parse(url) {
                Ok(u) if matches!(u.scheme(), "http" | "https") => sink.open_url(url),
                _ => Err("the authorization URL must start with http:// or https://".into()),
            },
            Duration::from_secs(300),
        )
        .await?;
        Ok(token_status(Some(token)))
    }

    // ---- import / export --------------------------------------------------------

    async fn import_file(&self, p: ImportFileParams) -> ApiResult<ImportSummary> {
        let text = match (&p.text, &p.path) {
            (Some(t), _) => t.clone(),
            (None, Some(path)) => {
                let meta =
                    std::fs::metadata(path).map_err(|e| ApiError::new("io", format!("Could not read {path}: {e}")))?;
                if meta.len() > MAX_IMPORT_FILE {
                    return Err(ApiError::invalid("File is larger than 50 MB"));
                }
                let bytes =
                    std::fs::read(path).map_err(|e| ApiError::new("io", format!("Could not read {path}: {e}")))?;
                String::from_utf8_lossy(&bytes).into_owned()
            }
            (None, None) => return Err(ApiError::invalid("Nothing to import")),
        };
        self.import_text(text, p.parent).await
    }

    async fn import_url(&self, p: ImportUrlParams) -> ApiResult<ImportSummary> {
        let mut opts = request_options(&self.settings(), &Default::default())?;
        opts.max_body_bytes = MAX_IMPORT_FILE as usize;
        let req = zorvik_engine::HttpRequest {
            method: "GET".into(),
            url: p.url.clone(),
            headers: vec![zorvik_engine::Header::new("Accept", "application/json, application/yaml, */*")],
            body: Default::default(),
        };
        let resp = self.inner.client.send(req, &opts, None).await?;
        if !(200..300).contains(&resp.meta.status) {
            return Err(ApiError::new(
                "network",
                format!("Download failed: HTTP {} {}", resp.meta.status, resp.meta.status_text),
            ));
        }
        if resp.body_truncated {
            return Err(ApiError::invalid("The download is larger than 50 MB"));
        }
        self.import_text(String::from_utf8_lossy(&resp.body).into_owned(), p.parent).await
    }

    async fn import_text(&self, text: String, parent: String) -> ApiResult<ImportSummary> {
        let ws = self.ws()?;
        let key = ws.local_key();
        let text = text.trim_start_matches('\u{feff}').to_string();
        let kind = detect_import(&text);
        let secrets_env = match kind {
            ImportKind::PostmanEnvironment => {
                let env = zorvik_workspace::formats::postman::import_postman_environment(&text)?;
                Some(env)
            }
            _ => None,
        };
        if let Some(env) = secrets_env {
            let concealed: Vec<Variable> = env
                .variables
                .iter()
                .map(|v| if v.secret { Variable { value: String::new(), ..v.clone() } } else { v.clone() })
                .collect();
            let id = ws.create_environment(&Environment { name: env.name.clone(), variables: concealed })?;
            self.inner.secrets.conceal(&env_scope(&key, &id), &env.variables)?;
            return Ok(ImportSummary {
                name: env.name,
                requests: 0,
                folders: 0,
                environments: 1,
                workspace_variables: 0,
                warnings: vec![],
                folder_path: None,
            });
        }
        let collection = match kind {
            // Parsing a big spec is CPU-bound: keep it off the async workers.
            ImportKind::Postman | ImportKind::OpenApi => tokio::task::spawn_blocking(move || match kind {
                ImportKind::Postman => zorvik_workspace::formats::postman::import_postman_collection(&text),
                _ => zorvik_workspace::formats::openapi::import_openapi(&text),
            })
            .await
            .map_err(join_err)??,
            ImportKind::Curl => {
                let parsed = parse_curl(&text)?;
                let path = ws.create_request(&parent, parsed.request.clone())?;
                return Ok(ImportSummary {
                    name: parsed.request.name,
                    requests: 1,
                    folders: 0,
                    environments: 0,
                    workspace_variables: 0,
                    warnings: parsed.warnings,
                    folder_path: Some(zorvik_workspace::store::parent_of(&path)),
                });
            }
            ImportKind::PostmanEnvironment => unreachable!(),
            ImportKind::Unknown => {
                return Err(ApiError::new(
                    "parse",
                    "Unrecognized file. Supported: Postman collection v2/v2.1, Postman environment, OpenAPI 3.x / Swagger 2.0 (JSON or YAML), cURL command.",
                ));
            }
        };
        // Collection variables become an environment. Secret values go to the secret store,
        // never into the workspace file (not even briefly).
        let mut collection = collection;
        let variables = collection.variables.clone();
        for v in collection.variables.iter_mut().filter(|v| v.secret) {
            v.value.clear();
        }
        let workspace_variables = std::mem::take(&mut collection.workspace_variables);
        let ws2 = ws.clone();
        let mut summary =
            tokio::task::spawn_blocking(move || ws2.write_imported(&parent, &collection)).await.map_err(join_err)??;
        if summary.environments > 0
            && variables.iter().any(|v| v.secret && !v.value.is_empty())
            && let Ok(envs) = ws.list_environments()
            && let Some(entry) = envs.iter().find(|e| e.environment.name == summary.name)
        {
            self.inner.secrets.conceal(&env_scope(&key, &entry.id), &variables)?;
        }
        if !workspace_variables.is_empty() {
            let (added, kept) = self.add_workspace_variables(&ws, &workspace_variables)?;
            summary.workspace_variables = added;
            if !kept.is_empty() {
                summary.warnings.push(format!(
                    "Collection variables already in the workspace kept their value: {}.",
                    kept.join(", ")
                ));
            }
        }
        Ok(summary)
    }

    /// Add imported collection variables the workspace doesn't have (secret values go to
    /// the secret store, as when saving the workspace). Returns how many were added and the
    /// names that already had a different value.
    fn add_workspace_variables(&self, ws: &Workspace, variables: &[Variable]) -> ApiResult<(u32, Vec<String>)> {
        let mut ws = ws.clone();
        let mut meta = self.revealed_meta(&ws);
        let (mut added, mut kept) = (0, Vec::new());
        for v in variables {
            match meta.variables.iter().find(|m| m.key == v.key) {
                Some(existing) if existing.value != v.value => kept.push(v.key.clone()),
                Some(_) => {}
                None => {
                    meta.variables.push(v.clone());
                    added += 1;
                }
            }
        }
        if added > 0 {
            meta.variables = self.inner.secrets.conceal(&workspace_scope(&ws.local_key()), &meta.variables)?;
            ws.save_meta(meta)?;
            self.update_ws(&ws);
        }
        Ok((added, kept))
    }

    fn export_curl(&self, p: ExportCurlParams) -> ApiResult<String> {
        let ws = self.ws()?;
        let mut resolved = if p.resolve_variables {
            self.resolve_only(&ws, &p.request, p.path.as_deref())?
        } else {
            let folders = p.path.as_deref().map(|x| ws.ancestors(x)).unwrap_or_default();
            let meta = ws.meta().clone();
            let outside_files = self.settings().files_outside_workspace;
            let inherit = Inheritance { workspace: &meta, folders: &folders, base_dir: ws.root(), outside_files };
            resolve(&p.request, &inherit, &VarContext::new())?
        };
        if let Some(config) = &resolved.oauth2 {
            let token = self.inner.tokens.get(&oauth2::cache_key(&ws.local_key(), config)).map(|t| t.access_token);
            apply_token(&mut resolved, token.as_deref().unwrap_or("<access-token>"));
        }
        Ok(to_curl(&resolved.request, p.flavor))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StreamParams {
    conn_id: String,
    request: Request,
    path: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ImportFileParams {
    path: Option<String>,
    text: Option<String>,
    #[serde(default)]
    parent: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ImportUrlParams {
    url: String,
    #[serde(default)]
    parent: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExportCurlParams {
    request: Request,
    path: Option<String>,
    flavor: CurlFlavor,
    #[serde(default = "yes")]
    resolve_variables: bool,
}

fn yes() -> bool {
    true
}

pub(crate) fn join_err(e: tokio::task::JoinError) -> ApiError {
    ApiError::new("internal", format!("Background task failed: {e}"))
}

#[derive(Debug, PartialEq, Eq)]
enum ImportKind {
    Postman,
    PostmanEnvironment,
    OpenApi,
    Curl,
    Unknown,
}

fn detect_import(text: &str) -> ImportKind {
    let trimmed = text.trim_start();
    let lower_head: String = trimmed.chars().take(4096).collect::<String>().to_ascii_lowercase();
    if lower_head.starts_with("curl ") || lower_head.starts_with("curl.exe ") || lower_head.starts_with("$ curl ") {
        return ImportKind::Curl;
    }
    if let Ok(json) = serde_json::from_str::<Value>(trimmed) {
        if json
            .get("info")
            .and_then(|i| i.get("schema"))
            .and_then(|s| s.as_str())
            .is_some_and(|s| s.contains("getpostman"))
            || (json.get("info").is_some() && json.get("item").is_some())
        {
            return ImportKind::Postman;
        }
        if json.get("values").is_some_and(|v| v.is_array()) && json.get("name").is_some() {
            return ImportKind::PostmanEnvironment;
        }
        if json.get("openapi").is_some() || json.get("swagger").is_some() {
            return ImportKind::OpenApi;
        }
        return ImportKind::Unknown;
    }
    if lower_head.contains("openapi:") || lower_head.contains("swagger:") {
        return ImportKind::OpenApi;
    }
    ImportKind::Unknown
}

fn token_status(token: Option<oauth2::TokenSet>) -> TokenStatus {
    match token {
        Some(t) => {
            let chars: Vec<char> = t.access_token.chars().collect();
            let preview = if chars.len() > 12 {
                format!(
                    "{}…{}",
                    chars[..6].iter().collect::<String>(),
                    chars[chars.len() - 4..].iter().collect::<String>()
                )
            } else {
                MASK.to_string()
            };
            TokenStatus {
                has_token: true,
                token_preview: Some(preview),
                expires_at: t.expires_at,
                scope: t.scope,
                has_refresh_token: t.refresh_token.is_some(),
            }
        }
        None => TokenStatus {
            has_token: false,
            token_preview: None,
            expires_at: None,
            scope: None,
            has_refresh_token: false,
        },
    }
}

fn display_body(
    body: &[u8],
    content_type: Option<String>,
    download_truncated: bool,
    decode_warning: Option<String>,
) -> ResponseBody {
    let ct = content_type.clone().unwrap_or_default().to_ascii_lowercase();
    let is_image = ct.starts_with("image/") && !ct.contains("svg");
    let looks_binary = !is_image && is_binary(body, &ct);
    let base = ResponseBody {
        kind: "text".into(),
        content_type,
        size: body.len() as u64,
        text: None,
        pretty: None,
        base64: None,
        display_truncated: false,
        download_truncated,
        decode_warning,
    };
    if is_image {
        let fits = body.len() <= MAX_IMAGE_PREVIEW;
        return ResponseBody {
            kind: "image".into(),
            base64: fits.then(|| base64::engine::general_purpose::STANDARD.encode(body)),
            display_truncated: !fits,
            ..base
        };
    }
    if looks_binary {
        // Show a hex preview of the first bytes.
        let shown = &body[..body.len().min(64 * 1024)];
        return ResponseBody {
            kind: "binary".into(),
            base64: Some(base64::engine::general_purpose::STANDARD.encode(shown)),
            display_truncated: shown.len() < body.len(),
            ..base
        };
    }
    let shown = &body[..body.len().min(MAX_DISPLAY_TEXT)];
    let text = String::from_utf8_lossy(shown).into_owned();
    let pretty = if body.len() <= MAX_PRETTY && (ct.contains("json") || text.trim_start().starts_with(['{', '['])) {
        zorvik_engine::pretty_json(&text)
    } else {
        None
    };
    ResponseBody { text: Some(text), pretty, display_truncated: shown.len() < body.len(), ..base }
}

fn is_binary(body: &[u8], content_type: &str) -> bool {
    let textual = content_type.starts_with("text/")
        || ["json", "xml", "javascript", "html", "yaml", "csv", "x-www-form-urlencoded", "graphql", "svg"]
            .iter()
            .any(|t| content_type.contains(t));
    if textual {
        return false;
    }
    let sample = &body[..body.len().min(8192)];
    if sample.contains(&0) {
        return true;
    }
    std::str::from_utf8(sample).is_err_and(|e| e.error_len().is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_import_kinds() {
        assert_eq!(
            detect_import(
                r#"{"info":{"schema":"https://schema.getpostman.com/json/collection/v2.1.0/collection.json"},"item":[]}"#
            ),
            ImportKind::Postman
        );
        assert_eq!(detect_import(r#"{"name":"Dev","values":[]}"#), ImportKind::PostmanEnvironment);
        assert_eq!(detect_import(r#"{"openapi":"3.0.0"}"#), ImportKind::OpenApi);
        assert_eq!(detect_import("openapi: 3.1.0\ninfo:\n  title: x"), ImportKind::OpenApi);
        assert_eq!(detect_import("curl https://x"), ImportKind::Curl);
        assert_eq!(detect_import("hello"), ImportKind::Unknown);
    }

    #[test]
    fn display_body_kinds() {
        let b = display_body(br#"{"a":1}"#, Some("application/json".into()), false, None);
        assert_eq!(b.kind, "text");
        assert_eq!(b.pretty.unwrap(), "{\n  \"a\": 1\n}");
        let b = display_body(&[0x89, b'P', b'N', b'G', 0], Some("image/png".into()), false, None);
        assert_eq!(b.kind, "image");
        let b = display_body(&[0, 1, 2, 255], Some("application/octet-stream".into()), false, None);
        assert_eq!(b.kind, "binary");
        let b = display_body("héllo".as_bytes(), None, false, None);
        assert_eq!(b.text.as_deref(), Some("héllo"));
    }

    #[test]
    fn response_store_is_bounded() {
        let mut store = ResponseStore::default();
        for i in 0..40 {
            store.put(i.to_string(), Arc::new(vec![0; 10]));
        }
        assert!(store.get("0").is_none());
        assert!(store.get("39").is_some());
        assert_eq!(store.items.len(), ResponseStore::MAX_ITEMS);
    }
}

//! MCP client sessions (`mcp.*`): connect to an MCP server (a URL, or a program the app starts
//! over stdio), list the tools, resources and prompts it offers, and watch every message
//! (`StreamEvent::Mcp`). A request's call is sent like any request (scripts, tests, history):
//! on the session its tab opened when there is one, otherwise in one go ([`call_once`]).
//!
//! Workspaces come from Git, so a program they name runs only once the user trusted that exact
//! command (with its working directory and environment) on this computer; `zorvik run` needs
//! `--allow-programs`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;
use ts_rs::TS;
use zorvik_engine::mcp::{McpClient, McpConnected, McpServerInfo, McpTarget, McpTransport as Transport, list_all};
use zorvik_engine::{
    Client, CookieJar, EngineError, Header, HttpResponse, RequestOptions, ResponseMeta, SentRequest, Timing,
};
use zorvik_workspace::Workspace;
use zorvik_workspace::formats::{KeyValue, McpCallKind, McpTransport, Request};
use zorvik_workspace::resolve::{Resolved, ResolvedMcp};

use crate::{Api, ApiError, ApiResult, StreamEvent, lock, ok, params, remove_if_current};

/// Programs trusted per workspace, in the app data dir.
pub(crate) const TRUSTED_FILE: &str = "trusted-programs.json";
/// Commands remembered per workspace (most recent last).
const TRUSTED_PER_WORKSPACE: usize = 64;
/// How long a call may take when the request sets no timeout (tools can be slow).
const CALL_TIMEOUT: Duration = Duration::from_secs(60);
/// Pages of a list read at most.
const MAX_PAGES: usize = 50;

/// An open session by caller id (a request tab's id). Registered before connecting (`open` is
/// `None` until then) so `mcp.close` can abort a pending connect.
pub(crate) struct McpEntry {
    generation: u64,
    cancel: CancellationToken,
    open: Option<OpenSession>,
}

/// A connected session, which sends of its tab's request reuse while they go to the same server.
#[derive(Clone)]
pub struct OpenSession {
    /// The server it was opened for ([`session_key`]).
    key: String,
    info: McpServerInfo,
    client: Arc<McpClient>,
}

/// What makes a session the one a request goes to: its address and transport.
fn session_key(address: &str, mcp: &ResolvedMcp) -> String {
    format!("{:?}\n{}", mcp.transport, address.trim())
}

/// What `mcp.connect` established.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct McpOpened {
    pub info: McpServerInfo,
    /// Variables that were referenced but not defined.
    pub unresolved: Vec<String>,
}

/// What the server offers (only what its capabilities announce).
#[derive(Debug, Clone, Default, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct McpCatalog {
    #[ts(type = "Array<Record<string, unknown>>")]
    pub tools: Vec<Value>,
    #[ts(type = "Array<Record<string, unknown>>")]
    pub resources: Vec<Value>,
    #[ts(type = "Array<Record<string, unknown>>")]
    pub resource_templates: Vec<Value>,
    #[ts(type = "Array<Record<string, unknown>>")]
    pub prompts: Vec<Value>,
    /// Lists that couldn't be read, in words.
    pub problems: Vec<String>,
}

/// A JSON-RPC error answer.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct McpCallError {
    #[ts(type = "number")]
    pub code: i64,
    pub message: String,
    #[ts(type = "unknown")]
    pub data: Option<Value>,
}

/// The answer to a call: a result (which may report a failed tool with `isError`), or an error.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct McpCallResult {
    /// `tools/call`, `resources/read` or `prompts/get`.
    pub method: String,
    /// The method's parameters as sent.
    #[ts(type = "Record<string, unknown>")]
    pub params: Value,
    #[ts(type = "Record<string, unknown> | null")]
    pub result: Option<Value>,
    pub error: Option<McpCallError>,
    /// A tool result that says the tool failed.
    pub is_error: bool,
    pub duration_ms: f64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConnectParams {
    conn_id: String,
    request: Request,
    path: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConnParam {
    conn_id: String,
}

#[derive(Deserialize)]
struct RequestParam {
    request: Request,
    path: Option<String>,
}

/// The program an MCP request starts, as the user is asked to trust it.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct McpProgram {
    pub command: String,
    /// The folder it starts in.
    pub cwd: String,
    pub env: Vec<KeyValue>,
    /// Already trusted for this workspace.
    pub trusted: bool,
}

// ---- trusted programs --------------------------------------------------------------------------

/// The programs the user allowed workspaces to start, by workspace.
pub struct TrustedPrograms {
    path: PathBuf,
    map: Mutex<Option<HashMap<String, Vec<String>>>>,
}

impl TrustedPrograms {
    pub fn new(path: PathBuf) -> Self {
        Self { path, map: Mutex::new(None) }
    }

    fn with<T>(&self, f: impl FnOnce(&mut HashMap<String, Vec<String>>) -> (T, bool)) -> T {
        let mut map = lock(&self.map);
        let map = map.get_or_insert_with(|| {
            std::fs::read(&self.path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
        });
        let (result, changed) = f(map);
        if changed && let Err(e) = zorvik_workspace::store::save_json(&self.path, map) {
            tracing::warn!("could not save trusted programs: {}", e.message);
        }
        result
    }

    pub fn is_trusted(&self, workspace: &str, print: &str) -> bool {
        self.with(|map| (map.get(workspace).is_some_and(|l| l.iter().any(|p| p == print)), false))
    }

    pub fn trust(&self, workspace: &str, print: String) {
        self.with(|map| {
            let list = map.entry(workspace.to_string()).or_default();
            list.retain(|p| *p != print);
            list.push(print);
            let extra = list.len().saturating_sub(TRUSTED_PER_WORKSPACE);
            list.drain(..extra);
            ((), true)
        });
    }
}

/// Which programs a send may start.
#[derive(Clone, Copy)]
pub enum Programs<'a> {
    /// The app: the ones the user trusted for this workspace.
    Trusted(&'a TrustedPrograms),
    /// An AI agent's send: only the program the user was just asked about (its fingerprint), so
    /// a script or variable can't swap in another one afterwards; `None` when nobody was asked.
    Approved(Option<&'a str>),
    /// `zorvik run --allow-programs`.
    Any,
    /// `zorvik run` without it.
    None,
}

impl Programs<'_> {
    /// Whether programs run from the app (a GUI app's PATH needs the login shell's added).
    fn in_app(self) -> bool {
        matches!(self, Programs::Trusted(_) | Programs::Approved(_))
    }
}

/// A program's fingerprint: its command line, working directory and environment.
pub(crate) fn fingerprint(command: &str, mcp: &ResolvedMcp) -> String {
    let mut env: Vec<String> = mcp.env.iter().map(|(k, v)| format!("{k}={v}")).collect();
    env.sort();
    let text = format!("{}\0{}\0{}", command.trim(), mcp.cwd.display(), env.join("\0"));
    Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// Whether a send may start `command` (when the request is a program at all).
pub(crate) fn check_program(
    programs: Programs<'_>,
    workspace: &str,
    command: &str,
    mcp: &ResolvedMcp,
) -> ApiResult<()> {
    if !mcp.is_program(command) {
        return Ok(());
    }
    match programs {
        Programs::Any => Ok(()),
        Programs::Approved(Some(approved)) if fingerprint(command, mcp) == approved => Ok(()),
        Programs::Approved(_) => Err(ApiError::new(
            "untrustedProgram",
            format!(
                "This request starts a program the user wasn't asked about ({}). Send the request itself so the user is asked; scripts and variables can't change the program after that.",
                command.trim()
            ),
        )),
        Programs::Trusted(trusted) if trusted.is_trusted(workspace, &fingerprint(command, mcp)) => Ok(()),
        Programs::Trusted(_) => Err(ApiError::new(
            "untrustedProgram",
            format!(
                "This request starts a program on your computer: {}. Connect from its tab once to allow it.",
                command.trim()
            ),
        )),
        Programs::None => Err(ApiError::new(
            "untrustedProgram",
            format!(
                "This request starts a program ({}); zorvik run starts programs only with --allow-programs",
                command.trim()
            ),
        )),
    }
}

// ---- connecting ----------------------------------------------------------------------------

/// The PATH of a login shell: apps started from the Dock or Finder get a short PATH that
/// misses where `npx`, `uvx` or `docker` usually live.
async fn login_path() -> Option<String> {
    static PATH: tokio::sync::OnceCell<Option<String>> = tokio::sync::OnceCell::const_new();
    PATH.get_or_init(|| async {
        #[cfg(unix)]
        {
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
            let run = tokio::process::Command::new(shell)
                .args(["-ilc", "printf '\\n__ZORVIK_PATH__%s\\n' \"$PATH\""])
                .stdin(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .output();
            let output = tokio::time::timeout(Duration::from_secs(5), run).await.ok()?.ok()?;
            let text = String::from_utf8_lossy(&output.stdout);
            let path = text.lines().find_map(|l| l.strip_prefix("__ZORVIK_PATH__"))?.trim().to_string();
            (!path.is_empty()).then_some(path)
        }
        #[cfg(not(unix))]
        {
            None
        }
    })
    .await
    .clone()
}

/// Where the server is, for the engine.
pub(crate) async fn target(resolved: &Resolved, mcp: &ResolvedMcp, fix_path: bool) -> McpTarget {
    let address = resolved.request.url.trim().to_string();
    let mut env = mcp.env.clone();
    if fix_path
        && mcp.is_program(&address)
        && !env.iter().any(|(k, _)| k.eq_ignore_ascii_case("PATH"))
        && let Some(path) = login_path().await
    {
        env.push(("PATH".into(), path));
    }
    McpTarget {
        address,
        transport: match mcp.transport {
            McpTransport::Auto => Transport::Auto,
            McpTransport::StreamableHttp => Transport::StreamableHttp,
            McpTransport::Sse => Transport::Sse,
            McpTransport::Stdio => Transport::Stdio,
        },
        headers: resolved.request.headers.clone(),
        env,
        cwd: Some(mcp.cwd.clone()),
    }
}

/// A resource URI template with its `{name}` parts filled in from the arguments.
fn fill_uri(template: &str, args: &Map<String, Value>) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        let Some(len) = rest[open..].find('}') else { break };
        out.push_str(&rest[..open]);
        let name = rest[open + 1..open + len].trim_start_matches(['+', '#', '/', '?', '&']);
        match args.get(name) {
            Some(Value::String(s)) => out.push_str(s),
            Some(other) => out.push_str(&other.to_string()),
            None => out.push_str(&rest[open..open + len + 1]),
        }
        rest = &rest[open + len + 1..];
    }
    out.push_str(rest);
    out
}

/// The method and parameters of a request's call.
pub(crate) fn call_message(mcp: &ResolvedMcp) -> ApiResult<(&'static str, Value)> {
    if mcp.name.is_empty() {
        let what = match mcp.call {
            McpCallKind::Tool => "tool",
            McpCallKind::Resource => "resource URI",
            McpCallKind::Prompt => "prompt",
        };
        return Err(ApiError::invalid(format!("Choose the {what} to call")));
    }
    Ok(match mcp.call {
        McpCallKind::Tool => ("tools/call", json!({ "name": mcp.name, "arguments": mcp.arguments })),
        McpCallKind::Resource => ("resources/read", json!({ "uri": fill_uri(&mcp.name, &mcp.arguments) })),
        McpCallKind::Prompt => {
            // Prompt arguments are strings.
            let args: Map<String, Value> = mcp
                .arguments
                .iter()
                .map(|(k, v)| (k.clone(), Value::String(v.as_str().map_or_else(|| v.to_string(), String::from))))
                .collect();
            ("prompts/get", json!({ "name": mcp.name, "arguments": args }))
        }
    })
}

/// Make the call on an open session.
pub(crate) async fn make_call(client: &McpClient, mcp: &ResolvedMcp, timeout: Duration) -> ApiResult<McpCallResult> {
    let (method, params) = call_message(mcp)?;
    let started = Instant::now();
    let answer = client.request(method, params.clone(), timeout).await;
    let duration_ms = started.elapsed().as_secs_f64() * 1000.0;
    Ok(match answer {
        Ok(result) => McpCallResult {
            method: method.into(),
            params,
            is_error: result["isError"] == true,
            result: Some(result),
            error: None,
            duration_ms,
        },
        // The connection failing is not an answer from the server.
        Err(e) if e.code == 0 => return Err(ApiError::new("mcp", e.message)),
        Err(e) => McpCallResult {
            method: method.into(),
            params,
            result: None,
            error: Some(McpCallError { code: e.code, message: e.message, data: e.data }),
            is_error: false,
            duration_ms,
        },
    })
}

fn timeout_of(opts: &RequestOptions) -> Duration {
    opts.timeout.map_or(CALL_TIMEOUT, |t| t.max(Duration::from_secs(5)))
}

/// Make a send's call: on `session` when it is open to the request's server, otherwise
/// connecting (when the program may start), calling and disconnecting.
pub(crate) async fn call(
    client: &Arc<Client>,
    resolved: &Resolved,
    opts: &RequestOptions,
    jar: Option<Arc<CookieJar>>,
    programs: Programs<'_>,
    workspace: &str,
    session: Option<&OpenSession>,
) -> ApiResult<(McpServerInfo, McpCallResult)> {
    let mcp = resolved.mcp.as_ref().ok_or_else(|| ApiError::invalid("This is not an MCP request"))?;
    // A call without a name fails before anything starts.
    call_message(mcp)?;
    let address = resolved.request.url.trim();
    if let Some(session) = session.filter(|s| s.key == session_key(address, mcp)) {
        let result = make_call(&session.client, mcp, timeout_of(opts)).await?;
        // Connecting is not part of this call's time.
        return Ok((McpServerInfo { connect_ms: 0.0, ..session.info.clone() }, result));
    }
    check_program(programs, workspace, address, mcp)?;
    Box::pin(call_once(client, resolved, opts, jar, programs.in_app())).await
}

/// Connect, make the request's call and disconnect.
async fn call_once(
    client: &Arc<Client>,
    resolved: &Resolved,
    opts: &RequestOptions,
    jar: Option<Arc<CookieJar>>,
    fix_path: bool,
) -> ApiResult<(McpServerInfo, McpCallResult)> {
    let mcp = resolved.mcp.clone().ok_or_else(|| ApiError::invalid("This is not an MCP request"))?;
    call_message(&mcp)?;
    let target = target(resolved, &mcp, fix_path).await;
    let connected = Box::pin(client.mcp(target, opts, jar)).await?;
    let result = make_call(&connected.client, &mcp, timeout_of(opts)).await?;
    Ok((connected.info, result))
}

/// A call's answer as an HTTP-like response for runs and scripts: the JSON-RPC result (or
/// error) is the body; the status is 200 for a result and 500 for an error.
pub(crate) fn as_response(address: &str, info: &McpServerInfo, call: &McpCallResult) -> HttpResponse {
    let body = match (&call.result, &call.error) {
        (Some(result), _) => serde_json::to_vec_pretty(result).unwrap_or_default(),
        (None, Some(error)) => serde_json::to_vec_pretty(&json!({ "error": error })).unwrap_or_default(),
        (None, None) => Vec::new(),
    };
    let (status, status_text) = if call.error.is_some() { (500, "MCP error") } else { (200, "OK") };
    let headers = vec![Header::new("Content-Type", "application/json")];
    HttpResponse {
        meta: ResponseMeta {
            status,
            status_text: status_text.into(),
            http_version: format!("MCP {} ({})", info.protocol_version, info.transport),
            headers_size: 0,
            headers,
            url: address.to_string(),
            remote_addr: None,
            tls: None,
            redirects: Vec::new(),
            request: SentRequest {
                method: call.method.clone(),
                url: address.to_string(),
                http_version: format!("MCP {}", info.protocol_version),
                headers: Vec::new(),
                body_size: call.params.to_string().len() as u64,
                proxy: None,
            },
            cookies: Vec::new(),
        },
        timing: Timing {
            total_ms: info.connect_ms + call.duration_ms,
            ttfb_ms: call.duration_ms,
            ..Default::default()
        },
        body_wire_size: body.len() as u64,
        body,
        body_truncated: false,
        decode_warning: None,
    }
}

impl Api {
    /// `mcp.*` methods.
    pub(crate) async fn call_mcp(&self, method: &str, p: Value) -> ApiResult<Value> {
        match method {
            "mcp.connect" => ok(self.mcp_connect(params(p)?).await?),
            "mcp.catalog" => {
                let ConnParam { conn_id } = params(p)?;
                ok(catalog(&*self.mcp_client(&conn_id)?).await)
            }
            "mcp.close" => {
                let ConnParam { conn_id } = params(p)?;
                // Dropping the client ends the session (its Closed event follows); a connect
                // still in progress is aborted.
                if let Some(entry) = lock(&self.inner.mcp_sessions).remove(&conn_id) {
                    entry.cancel.cancel();
                }
                ok(())
            }
            "mcp.program" => {
                let RequestParam { request, path } = params(p)?;
                let ws = self.ws()?;
                let resolved = self.prepare(&ws, &request, path.as_deref())?;
                let mcp = resolved.mcp.ok_or_else(|| ApiError::invalid("This is not an MCP request"))?;
                // Secret values show as {{name}}.
                let env = mcp.env.iter().map(|(k, v)| KeyValue::new(k.clone(), self.history_url(&ws, v))).collect();
                ok(McpProgram {
                    command: self.history_url(&ws, resolved.request.url.trim()),
                    cwd: mcp.cwd.display().to_string(),
                    env,
                    trusted: self.inner.programs.is_trusted(&ws.local_key(), &fingerprint(&resolved.request.url, &mcp)),
                })
            }
            "mcp.trust" => {
                let RequestParam { request, path } = params(p)?;
                let ws = self.ws()?;
                let resolved = self.prepare(&ws, &request, path.as_deref())?;
                let mcp = resolved.mcp.ok_or_else(|| ApiError::invalid("This is not an MCP request"))?;
                self.inner.programs.trust(&ws.local_key(), fingerprint(&resolved.request.url, &mcp));
                ok(())
            }
            other => Err(ApiError::new("notFound", format!("Unknown method '{other}'"))),
        }
    }

    /// Connect for one piece of work and disconnect (agents). Starts only the program the user
    /// was asked about (`approved`, its fingerprint), not the trusted ones.
    async fn connect_approved(
        &self,
        ws: &Workspace,
        request: &Request,
        path: Option<&str>,
        approved: Option<&str>,
    ) -> ApiResult<(McpConnected, Resolved, RequestOptions)> {
        let settings = self.settings();
        let mut resolved = self.prepare(ws, request, path)?;
        let mcp = resolved.mcp.clone().ok_or_else(|| ApiError::invalid("This is not an MCP request"))?;
        check_program(Programs::Approved(approved), &ws.local_key(), &resolved.request.url, &mcp)?;
        let opts = crate::request_options(&settings, &request.settings)?;
        self.authorize(ws, &mut resolved, &opts).await?;
        let jar = settings.cookie_jar.then(|| self.jar(ws));
        let target = target(&resolved, &mcp, true).await;
        let connected = Box::pin(self.inner.client.mcp(target, &opts, jar.clone())).await?;
        if let Some(jar) = &jar {
            self.save_jar(ws, jar);
        }
        Ok((connected, resolved, opts))
    }

    /// What a request's server offers, for an agent the user approved it for.
    pub(crate) async fn mcp_catalog_approved(
        &self,
        ws: &Workspace,
        request: &Request,
        path: Option<&str>,
        approved: Option<&str>,
    ) -> ApiResult<(McpServerInfo, McpCatalog)> {
        let (connected, _, _) = self.connect_approved(ws, request, path, approved).await?;
        let catalog = catalog(&connected.client).await;
        Ok((connected.info, catalog))
    }

    fn mcp_client(&self, conn_id: &str) -> ApiResult<Arc<McpClient>> {
        self.mcp_session(conn_id).map(|s| s.client).ok_or_else(|| ApiError::new("notFound", "Not connected"))
    }

    /// The open session of `conn_id` (a tab), for its sends.
    pub(crate) fn mcp_session(&self, conn_id: &str) -> Option<OpenSession> {
        lock(&self.inner.mcp_sessions).get(conn_id).and_then(|e| e.open.clone())
    }

    async fn mcp_connect(&self, p: ConnectParams) -> ApiResult<McpOpened> {
        let ws = self.ws()?;
        let settings = self.settings();
        let cancel = CancellationToken::new();
        let generation = self.next_generation();
        let entry = McpEntry { generation, cancel: cancel.clone(), open: None };
        if let Some(old) = lock(&self.inner.mcp_sessions).insert(p.conn_id.clone(), entry) {
            old.cancel.cancel();
        }
        let connected = tokio::select! {
            r = async {
                let mut resolved = self.prepare(&ws, &p.request, p.path.as_deref())?;
                let mcp = resolved.mcp.clone().ok_or_else(|| ApiError::invalid("This is not an MCP request"))?;
                check_program(Programs::Trusted(&self.inner.programs), &ws.local_key(), &resolved.request.url, &mcp)?;
                let opts = crate::request_options(&settings, &p.request.settings)?;
                self.authorize(&ws, &mut resolved, &opts).await?;
                let jar = settings.cookie_jar.then(|| self.jar(&ws));
                let target = target(&resolved, &mcp, true).await;
                let conn = Box::pin(self.inner.client.mcp(target, &opts, jar.clone())).await?;
                if let Some(jar) = &jar {
                    self.save_jar(&ws, jar);
                }
                let key = session_key(&resolved.request.url, &mcp);
                Ok::<_, ApiError>((conn, key, resolved.unresolved))
            } => r,
            _ = cancel.cancelled() => Err(EngineError::cancelled().into()),
        };
        let (conn, key, unresolved) = match connected {
            Ok(c) => c,
            Err(e) => {
                remove_if_current(&self.inner.mcp_sessions, &p.conn_id, |e| e.generation == generation);
                return Err(e);
            }
        };
        let McpConnected { info, client, mut events } = conn;
        let client = Arc::new(client);
        match lock(&self.inner.mcp_sessions).get_mut(&p.conn_id) {
            Some(entry) if entry.generation == generation => {
                entry.open = Some(OpenSession { key, info: info.clone(), client: client.clone() })
            }
            // Closed or replaced while connecting: dropping the client ends the session.
            _ => return Err(EngineError::cancelled().into()),
        }
        let inner = self.inner.clone();
        let conn_id = p.conn_id.clone();
        tokio::spawn(async move {
            while let Some(event) = events.recv().await {
                let closed = matches!(event, zorvik_engine::mcp::McpEvent::Closed { .. });
                // A session replaced by a newer connect of the same tab goes quietly: its events
                // would read as the new one's. One closed by `mcp.close` still says so.
                let replaced = lock(&inner.mcp_sessions).get(&conn_id).is_some_and(|e| e.generation != generation);
                if !replaced {
                    inner.sink.emit(StreamEvent::Mcp { conn_id: conn_id.clone(), event });
                }
                if closed {
                    break;
                }
            }
            remove_if_current(&inner.mcp_sessions, &conn_id, |e| e.generation == generation);
        });
        // Ends the session when the entry goes (closed, replaced, or the connection ended).
        drop(client);
        Ok(McpOpened { info, unresolved })
    }
}

/// Every list the server's capabilities announce.
async fn catalog(client: &McpClient) -> McpCatalog {
    let mut out = McpCatalog::default();
    let timeout = CALL_TIMEOUT;
    let lists: [(&str, &str, &str); 4] = [
        ("tools/list", "tools", "tools"),
        ("resources/list", "resources", "resources"),
        ("resources/templates/list", "resourceTemplates", "resource templates"),
        ("prompts/list", "prompts", "prompts"),
    ];
    for (method, key, what) in lists {
        match list_all(client, method, key, timeout, MAX_PAGES).await {
            Ok(items) => match key {
                "tools" => out.tools = items,
                "resources" => out.resources = items,
                "resourceTemplates" => out.resource_templates = items,
                _ => out.prompts = items,
            },
            // Servers answer -32601 for what they don't offer.
            Err(e) if e.code == -32601 => {}
            Err(e) => out.problems.push(format!("Couldn't list the {what}: {e}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolved_mcp(call: McpCallKind, name: &str, args: Value) -> ResolvedMcp {
        ResolvedMcp {
            transport: McpTransport::Auto,
            env: vec![("B".into(), "2".into()), ("A".into(), "1".into())],
            cwd: PathBuf::from("/ws"),
            call,
            name: name.into(),
            arguments: args.as_object().cloned().unwrap_or_default(),
        }
    }

    #[test]
    fn calls_as_messages() {
        let tool = resolved_mcp(McpCallKind::Tool, "get_weather", json!({ "city": "Lisbon" }));
        assert_eq!(
            call_message(&tool).unwrap(),
            ("tools/call", json!({ "name": "get_weather", "arguments": { "city": "Lisbon" } }))
        );
        let read = resolved_mcp(McpCallKind::Resource, "weather://{city}/{day}", json!({ "city": "Porto", "day": 3 }));
        assert_eq!(call_message(&read).unwrap(), ("resources/read", json!({ "uri": "weather://Porto/3" })));
        let prompt = resolved_mcp(McpCallKind::Prompt, "plan", json!({ "days": 2, "city": "Faro" }));
        assert_eq!(call_message(&prompt).unwrap().1["arguments"], json!({ "days": "2", "city": "Faro" }));
        let e = call_message(&resolved_mcp(McpCallKind::Resource, "", json!({}))).unwrap_err();
        assert_eq!(e.message, "Choose the resource URI to call");
        assert_eq!(fill_uri("a://{x}/{missing}", &Map::new()), "a://{x}/{missing}");
    }

    #[test]
    fn programs_need_trust() {
        let dir = tempfile::tempdir().unwrap();
        let trusted = TrustedPrograms::new(dir.path().join(TRUSTED_FILE));
        let mcp = resolved_mcp(McpCallKind::Tool, "t", json!({}));
        let command = "npx -y some-server";
        let e = check_program(Programs::Trusted(&trusted), "ws", command, &mcp).unwrap_err();
        assert_eq!(e.code, "untrustedProgram");
        trusted.trust("ws", fingerprint(command, &mcp));
        assert!(check_program(Programs::Trusted(&trusted), "ws", command, &mcp).is_ok());
        assert!(check_program(Programs::Trusted(&trusted), "other", command, &mcp).is_err(), "per workspace");
        // Another environment is another program.
        let mut changed = mcp.clone();
        changed.env.push(("NODE_OPTIONS".into(), "--require evil.js".into()));
        assert!(check_program(Programs::Trusted(&trusted), "ws", command, &changed).is_err());
        // The order of variables doesn't matter; the file keeps the trust.
        let mut reordered = mcp.clone();
        reordered.env.reverse();
        assert!(
            TrustedPrograms::new(dir.path().join(TRUSTED_FILE)).is_trusted("ws", &fingerprint(command, &reordered))
        );
        // URLs aren't programs; the CLI needs its flag.
        assert!(check_program(Programs::None, "ws", "https://mcp.example.com/mcp", &mcp).is_ok());
        assert!(check_program(Programs::None, "ws", command, &mcp).unwrap_err().message.contains("--allow-programs"));
        assert!(check_program(Programs::Any, "ws", command, &mcp).is_ok());
    }
}

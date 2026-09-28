//! MCP (Model Context Protocol) client: connect to a server over Streamable HTTP, the older
//! HTTP+SSE transport, or stdio (a program started for the session), initialize, and send
//! requests. Every JSON-RPC message in both directions is reported as an [`McpEvent`], so the
//! app can show the conversation; requests the server sends (ping, roots) are answered here.

mod http;
mod stdio;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};
use ts_rs::TS;

use crate::cookies::CookieJar;
use crate::error::{EngineError, ErrorKind, Result};
use crate::http::{Client, Header, RequestOptions};
use crate::ws::Direction;

pub use stdio::split_command;

/// Protocol versions the client speaks, newest first; it asks for the first.
pub const PROTOCOL_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];
/// Largest message accepted (a tool result with images can be large).
pub const MAX_MESSAGE: usize = 32 << 20;

/// How the client reaches the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpTransport {
    /// `http(s)://…`: Streamable HTTP, falling back to HTTP+SSE; anything else: a program (stdio).
    Auto,
    StreamableHttp,
    /// The HTTP+SSE transport of protocol version 2024-11-05.
    Sse,
    Stdio,
}

/// Where the server is.
pub struct McpTarget {
    /// The endpoint URL, or the command line of a stdio server.
    pub address: String,
    pub transport: McpTransport,
    /// HTTP: headers sent with every request (auth, …).
    pub headers: Vec<Header>,
    /// stdio: environment variables added for the program.
    pub env: Vec<(String, String)>,
    /// stdio: the program's working directory.
    pub cwd: Option<PathBuf>,
}

/// What `initialize` established.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct McpServerInfo {
    /// `Streamable HTTP`, `HTTP+SSE` or `stdio`.
    pub transport: String,
    pub protocol_version: String,
    pub name: String,
    pub title: Option<String>,
    pub version: String,
    pub instructions: Option<String>,
    /// The server's capabilities as it sent them (`tools`, `resources`, `prompts`, …).
    #[ts(type = "Record<string, unknown>")]
    pub capabilities: Value,
    /// HTTP: the session id the server gave, if any.
    pub session_id: Option<String>,
    /// stdio: the program's process id.
    pub pid: Option<u32>,
    /// Connecting and initializing, in milliseconds.
    pub connect_ms: f64,
}

/// Something that happened on the connection.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(export)]
pub enum McpEvent {
    /// A JSON-RPC message, as sent or received.
    #[serde(rename_all = "camelCase")]
    Message {
        direction: Direction,
        /// The message as JSON text.
        text: String,
        /// Requests and notifications: their method.
        method: Option<String>,
        /// Requests and responses: their id (as JSON).
        id: Option<String>,
        #[ts(type = "number")]
        size: u64,
        timestamp: f64,
    },
    /// stdio: a line the program wrote to stderr (its log).
    #[serde(rename_all = "camelCase")]
    Stderr { text: String, timestamp: f64 },
    /// Something worth showing that isn't a message (e.g. a fallback to HTTP+SSE).
    #[serde(rename_all = "camelCase")]
    Info { text: String, timestamp: f64 },
    #[serde(rename_all = "camelCase")]
    Error { message: String },
    #[serde(rename_all = "camelCase")]
    Closed { reason: String },
}

pub(crate) fn now_ms() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0)
}

/// What the session hands a transport.
pub(crate) enum Outbound {
    Message(Value),
    /// The session ended: close the connection (stop the program, delete the HTTP session).
    Close,
}

/// What a transport delivers to the session.
pub(crate) enum Inbound {
    Message(Value),
    Stderr(String),
    Info(String),
    Error(String),
    Closed(String),
}

/// A transport's two ends, as the session sees them.
pub(crate) struct Link {
    /// Messages to the server.
    pub tx: mpsc::UnboundedSender<Outbound>,
    pub rx: mpsc::UnboundedReceiver<Inbound>,
    pub label: &'static str,
    pub pid: Option<u32>,
    /// HTTP: the session id and the negotiated protocol version, shared with the transport,
    /// which sends them as headers.
    pub http: Option<Arc<http::HttpState>>,
}

/// An error answer to a request, or the connection failing.
#[derive(Debug, Clone, PartialEq)]
pub struct McpError {
    /// The JSON-RPC error code (0 when the connection failed or the request timed out).
    pub code: i64,
    pub message: String,
    pub data: Option<Value>,
}

impl McpError {
    fn local(message: impl Into<String>) -> Self {
        Self { code: 0, message: message.into(), data: None }
    }
}

impl std::fmt::Display for McpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.code {
            0 => f.write_str(&self.message),
            code => write!(f, "{} ({code})", self.message),
        }
    }
}

type Pending = Arc<Mutex<HashMap<String, oneshot::Sender<std::result::Result<Value, McpError>>>>>;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// A connected, initialized MCP session. Dropping it (or [`McpClient::close`]) ends it: a stdio
/// program is stopped, an HTTP session is deleted.
pub struct McpClient {
    out: mpsc::UnboundedSender<Outbound>,
    pending: Pending,
    next_id: AtomicU64,
    events: mpsc::UnboundedSender<McpEvent>,
    closed: Arc<tokio::sync::Notify>,
}

pub struct McpConnected {
    pub info: McpServerInfo,
    pub client: McpClient,
    pub events: mpsc::UnboundedReceiver<McpEvent>,
}

fn event_for(direction: Direction, message: &Value) -> McpEvent {
    let text = message.to_string();
    McpEvent::Message {
        direction,
        size: text.len() as u64,
        method: message["method"].as_str().map(String::from),
        id: message.get("id").filter(|id| !id.is_null()).map(Value::to_string),
        text,
        timestamp: now_ms(),
    }
}

impl McpClient {
    /// Send a request and wait for its answer (at most `timeout`; then it is cancelled).
    pub async fn request(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> std::result::Result<Value, McpError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let key = json!(id).to_string();
        let (tx, rx) = oneshot::channel();
        lock(&self.pending).insert(key.clone(), tx);
        let mut message = json!({ "jsonrpc": "2.0", "id": id, "method": method });
        if !params.is_null() {
            message["params"] = params;
        }
        let _ = self.events.send(event_for(Direction::Sent, &message));
        if self.out.send(Outbound::Message(message)).is_err() {
            lock(&self.pending).remove(&key);
            return Err(McpError::local("The connection is closed"));
        }
        // Given up on (a timeout, or the caller cancelled and dropped this future): the server
        // is told, and the answer is no longer waited for.
        let mut waiting = Waiting { client: self, key, id, reason: "Cancelled" };
        let answer = match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(answer)) => answer,
            Ok(Err(_)) => Err(McpError::local("The connection closed before the server answered")),
            Err(_) => {
                waiting.reason = "Timed out";
                return Err(McpError::local(format!(
                    "The server didn't answer {method} within {}",
                    crate::error::human_duration(timeout)
                )));
            }
        };
        waiting.reason = "";
        answer
    }

    /// Send a notification.
    pub fn notify(&self, method: &str, params: Value) {
        let mut message = json!({ "jsonrpc": "2.0", "method": method });
        if !params.is_null() {
            message["params"] = params;
        }
        let _ = self.events.send(event_for(Direction::Sent, &message));
        let _ = self.out.send(Outbound::Message(message));
    }

    /// Resolves when the connection has ended.
    pub async fn closed(&self) {
        self.closed.notified().await
    }
}

/// A request still waiting for its answer: when it's given up, it is forgotten and cancelled.
struct Waiting<'a> {
    client: &'a McpClient,
    key: String,
    id: u64,
    /// Empty once answered.
    reason: &'static str,
}

impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        if self.reason.is_empty() {
            return;
        }
        if lock(&self.client.pending).remove(&self.key).is_some() {
            self.client.notify("notifications/cancelled", json!({ "requestId": self.id, "reason": self.reason }));
        }
    }
}

impl Drop for McpClient {
    fn drop(&mut self) {
        let _ = self.out.send(Outbound::Close);
    }
}

/// Collect every page of a list (`tools/list`, `resources/list`, …) into one array (at most
/// `max_pages` pages).
pub async fn list_all(
    client: &McpClient,
    method: &str,
    key: &str,
    timeout: Duration,
    max_pages: usize,
) -> std::result::Result<Vec<Value>, McpError> {
    let mut items = Vec::new();
    let mut cursor: Option<String> = None;
    for _ in 0..max_pages {
        let params = match &cursor {
            Some(c) => json!({ "cursor": c }),
            None => Value::Null,
        };
        let page = client.request(method, params, timeout).await?;
        if let Some(list) = page[key].as_array() {
            items.extend(list.iter().cloned());
        }
        match page["nextCursor"].as_str() {
            Some(next) if !next.is_empty() && Some(next) != cursor.as_deref() => cursor = Some(next.to_string()),
            _ => break,
        }
    }
    Ok(items)
}

impl Client {
    /// Connect to an MCP server and initialize the session.
    pub async fn mcp(
        self: &Arc<Self>,
        target: McpTarget,
        opts: &RequestOptions,
        jar: Option<Arc<CookieJar>>,
    ) -> Result<McpConnected> {
        let started = Instant::now();
        let address = target.address.trim().to_string();
        if address.is_empty() {
            return Err(EngineError::invalid("Enter the server's URL, or the command that starts it"));
        }
        let is_http = address.starts_with("http://") || address.starts_with("https://");
        let transport = match target.transport {
            McpTransport::Auto if is_http => McpTransport::StreamableHttp,
            McpTransport::Auto => McpTransport::Stdio,
            McpTransport::StreamableHttp | McpTransport::Sse if !is_http => {
                return Err(EngineError::invalid(format!("'{address}' is not an http:// or https:// URL")));
            }
            other => other,
        };
        let wait = opts.timeout.unwrap_or(Duration::from_secs(30));
        let (events_tx, events_rx) = mpsc::unbounded_channel();
        let mut notes = Vec::new();
        let (link, info_response) = match transport {
            McpTransport::Stdio => {
                let link = stdio::spawn(&address, &target.env, target.cwd.as_deref())?;
                let (link, answer) = initialize(link, &events_tx, wait).await?;
                (link, answer)
            }
            McpTransport::Sse => {
                let link = http::connect_sse(self.clone(), &address, &target.headers, opts, jar.clone(), wait).await?;
                initialize(link, &events_tx, wait).await?
            }
            _ => {
                let link = http::streamable(self.clone(), &address, &target.headers, opts, jar.clone());
                match initialize(link, &events_tx, wait).await {
                    Ok(ok) => ok,
                    // Servers of the 2024-11-05 protocol answer the POST with an error status.
                    Err(e) if target.transport == McpTransport::Auto && http::is_old_server(&e) => {
                        notes.push(format!(
                            "The server didn't take Streamable HTTP ({}); using the older HTTP+SSE transport",
                            e.message
                        ));
                        let link =
                            http::connect_sse(self.clone(), &address, &target.headers, opts, jar.clone(), wait).await?;
                        initialize(link, &events_tx, wait).await?
                    }
                    Err(e) => return Err(e),
                }
            }
        };
        let Link { tx, rx, label, pid, http } = link;
        let protocol_version = info_response["protocolVersion"].as_str().unwrap_or_default().to_string();
        if let Some(http) = &http {
            http.set_protocol_version(&protocol_version);
        }
        let server = &info_response["serverInfo"];
        let info = McpServerInfo {
            transport: label.to_string(),
            protocol_version: protocol_version.clone(),
            name: server["name"].as_str().unwrap_or("(unnamed)").to_string(),
            title: server["title"].as_str().map(String::from),
            version: server["version"].as_str().unwrap_or_default().to_string(),
            instructions: info_response["instructions"].as_str().filter(|i| !i.is_empty()).map(String::from),
            capabilities: info_response["capabilities"].clone(),
            session_id: http.as_ref().and_then(|h| h.session_id()),
            pid,
            connect_ms: started.elapsed().as_secs_f64() * 1000.0,
        };
        if !PROTOCOL_VERSIONS.contains(&protocol_version.as_str()) {
            notes.push(format!(
                "The server answered with protocol version {protocol_version}, which Zorvik doesn't know"
            ));
        }
        for note in notes {
            let _ = events_tx.send(McpEvent::Info { text: note, timestamp: now_ms() });
        }
        let pending: Pending = Arc::default();
        let closed = Arc::new(tokio::sync::Notify::new());
        let client = McpClient {
            out: tx.clone(),
            pending: pending.clone(),
            next_id: AtomicU64::new(1),
            events: events_tx.clone(),
            closed: closed.clone(),
        };
        client.notify("notifications/initialized", Value::Null);
        tokio::spawn(route(rx, tx, pending, events_tx, closed));
        Ok(McpConnected { info, client, events: events_rx })
    }
}

/// Send `initialize` on a fresh link and wait for its answer (the session's messages flow
/// through [`route`] only afterwards).
/// Log lines of a program kept while it initializes, for the error when it doesn't.
const INIT_LOG_LINES: usize = 20;

async fn initialize(mut link: Link, events: &mpsc::UnboundedSender<McpEvent>, wait: Duration) -> Result<(Link, Value)> {
    let request = json!({
        "jsonrpc": "2.0",
        "id": 0,
        "method": "initialize",
        "params": {
            "protocolVersion": PROTOCOL_VERSIONS[0],
            "capabilities": { "roots": { "listChanged": false } },
            "clientInfo": { "name": "zorvik", "title": "Zorvik", "version": env!("CARGO_PKG_VERSION") },
        },
    });
    let _ = events.send(event_for(Direction::Sent, &request));
    link.tx.send(Outbound::Message(request)).map_err(|_| EngineError::new(ErrorKind::Io, "The connection closed"))?;
    // A program's last log lines: when it fails to start, they usually say why.
    let mut log: std::collections::VecDeque<String> = std::collections::VecDeque::new();
    let answer = async {
        loop {
            match link.rx.recv().await {
                Some(Inbound::Message(message)) => {
                    let _ = events.send(event_for(Direction::Received, &message));
                    if message.get("id") == Some(&json!(0)) && message.get("method").is_none() {
                        if let Some(error) = message.get("error") {
                            return Err(EngineError::new(
                                ErrorKind::Protocol,
                                format!("The server refused to initialize: {}", error_text(error)),
                            ));
                        }
                        return Ok(message["result"].clone());
                    }
                    // A request before the answer (e.g. ping): answered like later ones.
                    if let Some(reply) = server_request(&message) {
                        let _ = events.send(event_for(Direction::Sent, &reply));
                        let _ = link.tx.send(Outbound::Message(reply));
                    }
                }
                Some(Inbound::Stderr(text)) => {
                    if log.len() == INIT_LOG_LINES {
                        log.pop_front();
                    }
                    log.push_back(text.clone());
                    let _ = events.send(McpEvent::Stderr { text, timestamp: now_ms() });
                }
                Some(Inbound::Info(text)) => {
                    let _ = events.send(McpEvent::Info { text, timestamp: now_ms() });
                }
                Some(Inbound::Error(message)) => return Err(EngineError::new(ErrorKind::Protocol, message)),
                Some(Inbound::Closed(reason)) => {
                    return Err(EngineError::new(
                        ErrorKind::Connect,
                        format!("The server closed the connection: {reason}"),
                    ));
                }
                None => return Err(EngineError::new(ErrorKind::Connect, "The connection closed")),
            }
        }
    };
    let result = tokio::time::timeout(wait, answer)
        .await
        .map_err(|_| EngineError::timeout("Waiting for the server to initialize", wait))
        .and_then(|r| r)
        .map_err(|mut e| {
            let lines: Vec<&str> = log.iter().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
            if !lines.is_empty() {
                e.message = format!("{}\n\nThe program's last lines:\n{}", e.message, lines.join("\n"));
            }
            e
        })?;
    if !result.is_object() {
        return Err(EngineError::new(ErrorKind::Protocol, "The server's initialize answer has no result"));
    }
    Ok((link, result))
}

pub(crate) fn error_text(error: &Value) -> String {
    let message = error["message"].as_str().unwrap_or("error");
    match error["code"].as_i64() {
        Some(code) => format!("{message} ({code})"),
        None => message.to_string(),
    }
}

/// The answer to a request the server sends the client, or `None` for anything else.
fn server_request(message: &Value) -> Option<Value> {
    let method = message["method"].as_str()?;
    let id = message.get("id").filter(|id| !id.is_null())?;
    Some(match method {
        "ping" => json!({ "jsonrpc": "2.0", "id": id, "result": {} }),
        "roots/list" => json!({ "jsonrpc": "2.0", "id": id, "result": { "roots": [] } }),
        other => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": -32601, "message": format!("Zorvik doesn't support {other}") },
        }),
    })
}

/// The session: answers go to the requests waiting for them, the server's requests are
/// answered, everything is reported.
async fn route(
    mut rx: mpsc::UnboundedReceiver<Inbound>,
    tx: mpsc::UnboundedSender<Outbound>,
    pending: Pending,
    events: mpsc::UnboundedSender<McpEvent>,
    closed: Arc<tokio::sync::Notify>,
) {
    let reason = loop {
        match rx.recv().await {
            Some(Inbound::Message(message)) => {
                let _ = events.send(event_for(Direction::Received, &message));
                if message.get("method").is_some() {
                    if let Some(reply) = server_request(&message) {
                        let _ = events.send(event_for(Direction::Sent, &reply));
                        let _ = tx.send(Outbound::Message(reply));
                    }
                    continue;
                }
                let Some(id) = message.get("id").filter(|id| !id.is_null()) else { continue };
                let Some(waiting) = lock(&pending).remove(&id.to_string()) else { continue };
                let answer = match message.get("error") {
                    Some(error) => Err(McpError {
                        code: error["code"].as_i64().unwrap_or(0),
                        message: error["message"].as_str().unwrap_or("error").to_string(),
                        data: error.get("data").cloned(),
                    }),
                    None => Ok(message.get("result").cloned().unwrap_or(Value::Null)),
                };
                let _ = waiting.send(answer);
            }
            Some(Inbound::Stderr(text)) => {
                let _ = events.send(McpEvent::Stderr { text, timestamp: now_ms() });
            }
            Some(Inbound::Info(text)) => {
                let _ = events.send(McpEvent::Info { text, timestamp: now_ms() });
            }
            Some(Inbound::Error(message)) => {
                let _ = events.send(McpEvent::Error { message });
            }
            Some(Inbound::Closed(reason)) => break reason,
            None => break "Disconnected".to_string(),
        }
    };
    for (_, waiting) in lock(&pending).drain() {
        let _ = waiting.send(Err(McpError::local(format!("The connection closed ({reason})"))));
    }
    let _ = events.send(McpEvent::Closed { reason });
    closed.notify_waiters();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_from_the_server_are_answered() {
        assert_eq!(
            server_request(&json!({ "id": 3, "method": "ping" })),
            Some(json!({ "jsonrpc": "2.0", "id": 3, "result": {} }))
        );
        assert_eq!(
            server_request(&json!({ "id": "a", "method": "roots/list" })).unwrap()["result"],
            json!({ "roots": [] })
        );
        let sampling = server_request(&json!({ "id": 4, "method": "sampling/createMessage" })).unwrap();
        assert_eq!(sampling["error"]["code"], -32601);
        assert!(
            server_request(&json!({ "method": "notifications/message" })).is_none(),
            "notifications need no answer"
        );
    }

    #[test]
    fn messages_as_events() {
        match event_for(Direction::Sent, &json!({ "jsonrpc": "2.0", "id": 7, "method": "tools/list" })) {
            McpEvent::Message { method, id, direction, .. } => {
                assert_eq!(
                    (method.as_deref(), id.as_deref(), direction),
                    (Some("tools/list"), Some("7"), Direction::Sent)
                );
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(error_text(&json!({ "code": -32602, "message": "Unknown tool" })), "Unknown tool (-32602)");
        assert_eq!(McpError { code: 0, message: "closed".into(), data: None }.to_string(), "closed");
    }
}

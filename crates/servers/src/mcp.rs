//! MCP server (a mock): tools, resources and prompts from its settings, answered from
//! templates. It speaks Streamable HTTP on its path (POST messages, GET a stream for its
//! notifications, DELETE to end a session), the older HTTP+SSE transport (`GET /sse`, then
//! `POST /messages?sessionId=…`), and stdio for `zorvik serve --stdio`. Every request and
//! answer is logged; when its settings change, connected clients are told (`list_changed`).

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use http::header::{self, HeaderValue};
use http::{Method, Request, Response, StatusCode};
use http_body_util::{BodyExt, Limited};
use hyper::body::Incoming;
use serde_json::{Map, Value, json};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;
use zorvik_engine::mcp::PROTOCOL_VERSIONS;
use zorvik_formats::{McpServerConfig, McpToolMock, Server};
use zorvik_workspace::vars::VarContext;

use crate::http::{Body, Handler, HandlerFuture, Peer, add_cors, parse_query, preflight, serve};
use crate::report::{Reporter, TrafficDirection};
use crate::{Control, Ctx, Live};

/// Largest message a client may POST.
const MAX_BODY: usize = 4 << 20;
/// Events waiting for a slow stream before more are dropped.
const BUFFERED_EVENTS: usize = 64;
/// Longest tool delay.
const MAX_DELAY: Duration = Duration::from_secs(3600);
/// A comment on idle streams, so proxies keep them open.
const KEEPALIVE: Duration = Duration::from_secs(15);

pub(crate) async fn run(listener: TcpListener, ctx: Ctx) -> Result<(), String> {
    let mcp =
        Arc::new(Mcp { live: ctx.live.clone(), reporter: ctx.reporter.clone(), sessions: Mutex::new(HashMap::new()) });
    tokio::spawn(announce_changes(mcp.clone(), ctx.live.clone(), ctx.cancel.clone()));
    tokio::spawn(report_problems(ctx.live.clone(), ctx.reporter.clone(), ctx.cancel.clone()));
    let handler: Handler = {
        let mcp = mcp.clone();
        Arc::new(move |req: Request<Incoming>, peer: Peer| -> HandlerFuture {
            let mcp = mcp.clone();
            Box::pin(async move { Ok(mcp.handle(req, peer).await) })
        })
    };
    serve(listener, ctx, handler, false, move |control| match control {
        Control::Send { reply, .. } => {
            let _ = reply.send(Err("An MCP server answers its clients' requests: there is nothing to send".into()));
        }
        Control::Disconnect { conn } => mcp.end_conn(conn),
    })
    .await
}

/// Serve one client over stdin and stdout until stdin closes (`zorvik serve --stdio`).
pub async fn serve_stdio(
    server: Server,
    vars: VarContext,
    reporter: Reporter,
    input: impl AsyncRead + Unpin,
    mut output: impl AsyncWrite + Unpin,
) -> Result<(), String> {
    for problem in server.mcp.problems() {
        reporter.error(None, None, problem);
    }
    let (_live_tx, live) = watch::channel(Arc::new(Live { server, vars }));
    let mcp = Mcp { live, reporter: reporter.clone(), sessions: Mutex::new(HashMap::new()) };
    let conn = reporter.next_conn();
    let peer: SocketAddr = ([127, 0, 0, 1], 0).into();
    reporter.opened(conn, &peer);
    let mut lines = BufReader::new(input).lines();
    let reason = loop {
        let line = match lines.next_line().await {
            Ok(Some(line)) => line,
            Ok(None) => break "stdin closed".to_string(),
            Err(e) => break format!("reading stdin failed: {e}"),
        };
        if line.trim().is_empty() {
            continue;
        }
        let answer = match serde_json::from_str::<Value>(&line) {
            Ok(message) => mcp.answer_all(message, Some(conn), Some(&peer)).await,
            Err(e) => Some(parse_error(&e)),
        };
        if let Some(answer) = answer {
            let mut text = answer.to_string();
            text.push('\n');
            if output.write_all(text.as_bytes()).await.is_err() || output.flush().await.is_err() {
                break "stdout closed".to_string();
            }
        }
    };
    reporter.closed(conn, &peer, &reason);
    Ok(())
}

/// A client's session: its connection in the log, and its open event streams.
struct Session {
    conn: u64,
    peer: SocketAddr,
    /// Streamable HTTP GET streams, for notifications.
    streams: Mutex<Vec<mpsc::Sender<Bytes>>>,
    /// HTTP+SSE: the stream every answer goes to.
    legacy: Option<mpsc::Sender<Bytes>>,
    closed: CancellationToken,
}

struct Mcp {
    live: watch::Receiver<Arc<Live>>,
    reporter: Reporter,
    sessions: Mutex<HashMap<String, Arc<Session>>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn random_id() -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    (0..24).map(|_| ALPHABET[rand::random_range(0..ALPHABET.len())] as char).collect()
}

fn sse_event(event: Option<&str>, data: &str) -> Bytes {
    let mut out = String::new();
    if let Some(event) = event {
        out.push_str(&format!("event: {event}\n"));
    }
    for line in data.split('\n') {
        out.push_str(&format!("data: {line}\n"));
    }
    out.push('\n');
    Bytes::from(out)
}

fn json_response(status: StatusCode, value: &Value) -> Response<Body> {
    let mut response = Response::new(Body::full(value.to_string()));
    *response.status_mut() = status;
    response.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json"));
    response
}

fn text_response(status: StatusCode, text: String) -> Response<Body> {
    let mut response = Response::new(Body::full(text));
    *response.status_mut() = status;
    response.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("text/plain; charset=utf-8"));
    response
}

fn stream_response(rx: mpsc::Receiver<Bytes>) -> Response<Body> {
    let mut response = Response::new(Body::Stream(rx));
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/event-stream"));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    headers.insert("x-accel-buffering", HeaderValue::from_static("no"));
    response
}

fn parse_error(e: &serde_json::Error) -> Value {
    json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": format!("Parse error: {e}") } })
}

fn rpc_error(id: &Value, code: i64, message: impl Into<String>) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message.into() } })
}

impl Mcp {
    fn config(&self) -> Arc<Live> {
        self.live.borrow().clone()
    }

    async fn handle(self: Arc<Self>, req: Request<Incoming>, peer: Peer) -> Response<Body> {
        let live = self.config();
        let config = &live.server.mcp;
        let origin = req.headers().get(header::ORIGIN).cloned();
        let cors = config.cors;
        let path = format!("/{}", config.path.trim().trim_matches('/'));
        let mut response = if req.method() == Method::OPTIONS {
            // A browser's preflight: permission only with CORS on.
            let mut response = Response::new(Body::empty());
            *response.status_mut() = StatusCode::NO_CONTENT;
            if cors {
                *response.headers_mut() = preflight(req.headers());
            }
            response
        } else {
            self.route(req, peer, &path).await
        };
        if cors {
            add_cors(response.headers_mut(), origin.as_ref());
            response
                .headers_mut()
                .insert(header::ACCESS_CONTROL_EXPOSE_HEADERS, HeaderValue::from_static("Mcp-Session-Id"));
        }
        response
    }

    async fn route(self: &Arc<Self>, req: Request<Incoming>, peer: Peer, path: &str) -> Response<Body> {
        let request_path = req.uri().path().trim_end_matches('/').to_string();
        let session_header =
            req.headers().get("mcp-session-id").and_then(|v| v.to_str().ok()).map(|v| v.trim().to_string());
        match (req.method().clone(), request_path.as_str()) {
            (Method::POST, p) if p == path.trim_end_matches('/') => self.post(req, peer, session_header).await,
            (Method::GET, p) if p == path.trim_end_matches('/') => self.listen(session_header),
            (Method::DELETE, p) if p == path.trim_end_matches('/') => match session_header {
                Some(id) => match self.end_session(&id, "the client ended the session") {
                    true => Response::new(Body::empty()),
                    false => text_response(StatusCode::NOT_FOUND, "No such session\n".into()),
                },
                None => text_response(StatusCode::BAD_REQUEST, "Mcp-Session-Id is missing\n".into()),
            },
            (Method::GET, "/sse") => self.open_legacy(peer),
            (Method::POST, "/messages") => self.post_legacy(req).await,
            _ => text_response(
                StatusCode::NOT_FOUND,
                format!(
                    "This is an MCP server: clients connect to {path} (or /sse for the older HTTP+SSE transport)\n"
                ),
            ),
        }
    }

    /// Streamable HTTP: one message or a batch; answers come back as JSON.
    async fn post(self: &Arc<Self>, req: Request<Incoming>, peer: Peer, session_id: Option<String>) -> Response<Body> {
        let body = match Limited::new(req.into_body(), MAX_BODY).collect().await {
            Ok(body) => body.to_bytes(),
            Err(_) => return text_response(StatusCode::PAYLOAD_TOO_LARGE, "The message is larger than 4 MB\n".into()),
        };
        let message = match serde_json::from_slice::<Value>(&body) {
            Ok(m) => m,
            Err(e) => return json_response(StatusCode::BAD_REQUEST, &parse_error(&e)),
        };
        let initializing = message["method"] == "initialize";
        let (session, new_id) = if initializing {
            let id = random_id();
            let session = Arc::new(Session {
                conn: self.reporter.next_conn(),
                peer: peer.addr,
                streams: Mutex::new(Vec::new()),
                legacy: None,
                closed: CancellationToken::new(),
            });
            self.reporter.opened(session.conn, &session.peer);
            lock(&self.sessions).insert(id.clone(), session.clone());
            (Some(session), Some(id))
        } else {
            match &session_id {
                Some(id) => match lock(&self.sessions).get(id).cloned() {
                    Some(session) => (Some(session), None),
                    None => return text_response(StatusCode::NOT_FOUND, "No such session: initialize again\n".into()),
                },
                // Clients that don't keep the session id are answered anyway.
                None => (None, None),
            }
        };
        let conn = session.as_ref().map(|s| s.conn);
        let answer = self.answer_all(message, conn, Some(&peer.addr)).await;
        let mut response = match answer {
            Some(answer) => json_response(StatusCode::OK, &answer),
            None => {
                let mut response = Response::new(Body::empty());
                *response.status_mut() = StatusCode::ACCEPTED;
                response
            }
        };
        if let Some(id) = new_id
            && let Ok(value) = HeaderValue::from_str(&id)
        {
            response.headers_mut().insert("mcp-session-id", value);
        }
        response
    }

    /// Streamable HTTP GET: a stream for the session's notifications.
    fn listen(&self, session_id: Option<String>) -> Response<Body> {
        let Some(session) = session_id.and_then(|id| lock(&self.sessions).get(&id).cloned()) else {
            let mut response =
                text_response(StatusCode::METHOD_NOT_ALLOWED, "Open a session first (initialize)\n".into());
            response.headers_mut().insert(header::ALLOW, HeaderValue::from_static("POST"));
            return response;
        };
        let (tx, rx) = mpsc::channel(BUFFERED_EVENTS);
        {
            // Streams the client closed (or reconnected from) are dropped.
            let mut streams = lock(&session.streams);
            streams.retain(|s| !s.is_closed());
            streams.push(tx.clone());
        }
        tokio::spawn(keep_alive(tx, session.closed.clone()));
        stream_response(rx)
    }

    /// HTTP+SSE: a new session whose answers all go down this stream.
    fn open_legacy(self: &Arc<Self>, peer: Peer) -> Response<Body> {
        let id = random_id();
        let (tx, rx) = mpsc::channel(BUFFERED_EVENTS);
        let session = Arc::new(Session {
            conn: self.reporter.next_conn(),
            peer: peer.addr,
            streams: Mutex::new(Vec::new()),
            legacy: Some(tx.clone()),
            closed: CancellationToken::new(),
        });
        self.reporter.opened(session.conn, &session.peer);
        lock(&self.sessions).insert(id.clone(), session.clone());
        let _ = tx.try_send(sse_event(Some("endpoint"), &format!("/messages?sessionId={id}")));
        // The session ends when the client leaves the stream.
        let mcp = self.clone();
        let closed = session.closed.clone();
        tokio::spawn(async move {
            tokio::select! {
                _ = tx.closed() => {}
                _ = closed.cancelled() => {}
            }
            mcp.end_session(&id, "the client left");
        });
        tokio::spawn(keep_alive(session.legacy.clone().expect("legacy stream"), session.closed.clone()));
        stream_response(rx)
    }

    /// HTTP+SSE: a message for a session; the answer goes to its stream.
    async fn post_legacy(self: &Arc<Self>, req: Request<Incoming>) -> Response<Body> {
        let query = parse_query(req.uri().query().unwrap_or_default());
        let id = query.iter().find(|(k, _)| k == "sessionId").map(|(_, v)| v.clone()).unwrap_or_default();
        let Some(session) = lock(&self.sessions).get(&id).cloned() else {
            return text_response(StatusCode::NOT_FOUND, "No such session: open /sse first\n".into());
        };
        let Some(stream) = session.legacy.clone() else {
            return text_response(StatusCode::BAD_REQUEST, "This session doesn't use the HTTP+SSE transport\n".into());
        };
        let body = match Limited::new(req.into_body(), MAX_BODY).collect().await {
            Ok(body) => body.to_bytes(),
            Err(_) => return text_response(StatusCode::PAYLOAD_TOO_LARGE, "The message is larger than 4 MB\n".into()),
        };
        let message = match serde_json::from_slice::<Value>(&body) {
            Ok(m) => m,
            Err(e) => return json_response(StatusCode::BAD_REQUEST, &parse_error(&e)),
        };
        let mcp = self.clone();
        // Answered on the stream; the POST is accepted right away (a tool may take a while).
        tokio::spawn(async move {
            if let Some(answer) = mcp.answer_all(message, Some(session.conn), Some(&session.peer)).await {
                let _ = stream.send(sse_event(Some("message"), &answer.to_string())).await;
            }
        });
        let mut response = text_response(StatusCode::ACCEPTED, "Accepted\n".into());
        *response.status_mut() = StatusCode::ACCEPTED;
        response
    }

    fn end_session(&self, id: &str, reason: &str) -> bool {
        let Some(session) = lock(&self.sessions).remove(id) else { return false };
        session.closed.cancel();
        self.reporter.closed(session.conn, &session.peer, reason);
        true
    }

    fn end_conn(&self, conn: u64) {
        let id = lock(&self.sessions).iter().find(|(_, s)| s.conn == conn).map(|(id, _)| id.clone());
        if let Some(id) = id {
            self.end_session(&id, "closed by you");
        }
    }

    /// Answer a message or a batch (`None` when nothing needs an answer).
    async fn answer_all(&self, message: Value, conn: Option<u64>, peer: Option<&SocketAddr>) -> Option<Value> {
        match message {
            Value::Array(batch) => {
                let mut answers = Vec::new();
                for m in batch {
                    if let Some(a) = self.answer_logged(m, conn, peer).await {
                        answers.push(a);
                    }
                }
                (!answers.is_empty()).then_some(Value::Array(answers))
            }
            message => self.answer_logged(message, conn, peer).await,
        }
    }

    async fn answer_logged(&self, message: Value, conn: Option<u64>, peer: Option<&SocketAddr>) -> Option<Value> {
        let text = message.to_string();
        self.reporter.data(conn, peer, TrafficDirection::In, text.as_bytes(), summary(&message));
        let answer = self.answer(&message).await?;
        let out = answer.to_string();
        let what = match answer.get("error") {
            Some(e) => format!("error: {}", e["message"].as_str().unwrap_or_default()),
            None if answer["result"]["isError"] == true => format!("{} failed (isError)", summary(&message)),
            None => format!("result of {}", summary(&message)),
        };
        self.reporter.data(conn, peer, TrafficDirection::Out, out.as_bytes(), what);
        Some(answer)
    }

    /// The answer to one message (notifications and responses get none).
    async fn answer(&self, message: &Value) -> Option<Value> {
        let method = message["method"].as_str()?;
        let id = message.get("id").filter(|id| !id.is_null())?.clone();
        let live = self.config();
        let config = &live.server.mcp;
        let params = &message["params"];
        let result = match method {
            "initialize" => Ok(initialize_result(&live.server, config, params["protocolVersion"].as_str())),
            "ping" | "logging/setLevel" | "resources/subscribe" | "resources/unsubscribe" => Ok(json!({})),
            "tools/list" => {
                Ok(json!({ "tools": config.tools.iter().filter(|t| t.enabled).map(tool_entry).collect::<Vec<_>>() }))
            }
            "tools/call" => self.call_tool(config, params, &live.vars).await,
            "resources/list" => Ok(json!({ "resources": resources(config, false) })),
            "resources/templates/list" => Ok(json!({ "resourceTemplates": resources(config, true) })),
            "resources/read" => read_resource(config, params, &live.vars),
            "prompts/list" => Ok(
                json!({ "prompts": config.prompts.iter().filter(|p| p.enabled).map(prompt_entry).collect::<Vec<_>>() }),
            ),
            "prompts/get" => get_prompt(config, params, &live.vars),
            "completion/complete" => Ok(json!({ "completion": { "values": [], "hasMore": false } })),
            other => Err((-32601, format!("Method not found: {other}"))),
        };
        Some(match result {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err((code, message)) => rpc_error(&id, code, message),
        })
    }

    async fn call_tool(
        &self,
        config: &McpServerConfig,
        params: &Value,
        vars: &VarContext,
    ) -> Result<Value, (i64, String)> {
        let name = params["name"].as_str().unwrap_or_default();
        let Some(tool) = config.tools.iter().find(|t| t.enabled && t.name == name) else {
            return Err((-32602, format!("Unknown tool: {name}")));
        };
        let args = params["arguments"].as_object().cloned().unwrap_or_default();
        // Invalid arguments are a failed call the model can read and correct.
        if let Some(missing) = missing_argument(&tool.input_schema, &args) {
            return Ok(tool_failure(format!("Missing required argument: {missing}")));
        }
        if tool.delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(tool.delay_ms).min(MAX_DELAY)).await;
        }
        let text = render(&tool.result, &args, &Map::new(), vars);
        let mut result = json!({ "content": [{ "type": "text", "text": text }] });
        if !tool.output_schema.trim().is_empty() {
            match serde_json::from_str::<Value>(&text) {
                Ok(structured) => result["structuredContent"] = structured,
                Err(_) => {
                    return Ok(tool_failure("This mock's result isn't JSON, but the tool has an output schema".into()));
                }
            }
        }
        if tool.is_error {
            result["isError"] = json!(true);
        }
        Ok(result)
    }
}

async fn keep_alive(tx: mpsc::Sender<Bytes>, closed: CancellationToken) {
    loop {
        tokio::select! {
            _ = tokio::time::sleep(KEEPALIVE) => {
                if tx.send(Bytes::from_static(b": keep-alive\n\n")).await.is_err() {
                    return;
                }
            }
            _ = closed.cancelled() => return,
            _ = tx.closed() => return,
        }
    }
}

/// Settings clients would trip over are reported when they appear.
async fn report_problems(mut live: watch::Receiver<Arc<Live>>, reporter: Reporter, stopped: CancellationToken) {
    let mut reported: Vec<String> = Vec::new();
    loop {
        let problems = live.borrow_and_update().server.mcp.problems();
        for p in problems.iter().filter(|p| !reported.contains(p)) {
            reporter.error(None, None, p.clone());
        }
        reported = problems;
        tokio::select! {
            changed = live.changed() => if changed.is_err() { return },
            _ = stopped.cancelled() => return,
        }
    }
}

/// When the settings change, tell every session with a stream that the lists changed.
async fn announce_changes(mcp: Arc<Mcp>, mut live: watch::Receiver<Arc<Live>>, stopped: CancellationToken) {
    let mut last = live.borrow_and_update().server.mcp.clone();
    loop {
        tokio::select! {
            changed = live.changed() => if changed.is_err() { return },
            _ = stopped.cancelled() => return,
        }
        let current = live.borrow_and_update().server.mcp.clone();
        let mut notices = Vec::new();
        if current.tools != last.tools {
            notices.push("notifications/tools/list_changed");
        }
        if current.resources != last.resources {
            notices.push("notifications/resources/list_changed");
        }
        if current.prompts != last.prompts {
            notices.push("notifications/prompts/list_changed");
        }
        last = current;
        if notices.is_empty() {
            continue;
        }
        let sessions: Vec<Arc<Session>> = lock(&mcp.sessions).values().cloned().collect();
        for session in sessions {
            let mut targets: Vec<mpsc::Sender<Bytes>> = lock(&session.streams).clone();
            targets.extend(session.legacy.clone());
            for notice in &notices {
                let message = json!({ "jsonrpc": "2.0", "method": notice }).to_string();
                let event = if session.legacy.is_some() { Some("message") } else { None };
                for tx in &targets {
                    if tx.try_send(sse_event(event, &message)).is_ok() {
                        mcp.reporter.data(
                            Some(session.conn),
                            Some(&session.peer),
                            TrafficDirection::Out,
                            message.as_bytes(),
                            *notice,
                        );
                    }
                }
            }
        }
    }
}

/// The log's one-line summary of a message: its method, with the tool, prompt or URI.
fn summary(message: &Value) -> String {
    let method = message["method"].as_str().unwrap_or("response");
    let params = &message["params"];
    let detail = match method {
        "tools/call" | "prompts/get" => params["name"].as_str(),
        "resources/read" => params["uri"].as_str(),
        _ => None,
    };
    match detail {
        Some(d) => format!("{method} {d}"),
        None => method.to_string(),
    }
}

fn initialize_result(server: &Server, config: &McpServerConfig, requested: Option<&str>) -> Value {
    let version =
        requested.and_then(|r| PROTOCOL_VERSIONS.iter().find(|v| **v == r)).copied().unwrap_or(PROTOCOL_VERSIONS[0]);
    let name = if config.server_name.trim().is_empty() { server.name.trim() } else { config.server_name.trim() };
    let mut result = json!({
        "protocolVersion": version,
        "capabilities": {
            "tools": { "listChanged": true },
            "resources": { "listChanged": true },
            "prompts": { "listChanged": true },
            "logging": {},
            "completions": {},
        },
        "serverInfo": { "name": name, "version": if config.version.trim().is_empty() { "1.0.0" } else { config.version.trim() } },
    });
    if !config.instructions.trim().is_empty() {
        result["instructions"] = json!(config.instructions);
    }
    result
}

/// A JSON text setting as a value; blank or invalid gives `fallback`.
fn json_or(text: &str, fallback: Value) -> Value {
    serde_json::from_str::<Value>(text).ok().filter(Value::is_object).unwrap_or(fallback)
}

fn tool_entry(tool: &McpToolMock) -> Value {
    let mut entry =
        json!({ "name": tool.name, "inputSchema": json_or(&tool.input_schema, json!({ "type": "object" })) });
    if !tool.title.is_empty() {
        entry["title"] = json!(tool.title);
    }
    if !tool.description.is_empty() {
        entry["description"] = json!(tool.description);
    }
    if !tool.output_schema.trim().is_empty() {
        entry["outputSchema"] = json_or(&tool.output_schema, json!({ "type": "object" }));
    }
    entry
}

fn tool_failure(text: String) -> Value {
    json!({ "content": [{ "type": "text", "text": text }], "isError": true })
}

/// The first property the schema requires that `args` lacks.
fn missing_argument(schema: &str, args: &Map<String, Value>) -> Option<String> {
    let schema = serde_json::from_str::<Value>(schema).ok()?;
    schema["required"]
        .as_array()?
        .iter()
        .filter_map(Value::as_str)
        .find(|name| args.get(*name).is_none_or(Value::is_null))
        .map(String::from)
}

fn is_template(uri: &str) -> bool {
    uri.contains('{')
}

fn resources(config: &McpServerConfig, templates: bool) -> Vec<Value> {
    config
        .resources
        .iter()
        .filter(|r| r.enabled && is_template(&r.uri) == templates)
        .map(|r| {
            let mut entry = json!({ "name": if r.name.is_empty() { &r.uri } else { &r.name } });
            entry[if templates { "uriTemplate" } else { "uri" }] = json!(r.uri);
            if !r.title.is_empty() {
                entry["title"] = json!(r.title);
            }
            if !r.description.is_empty() {
                entry["description"] = json!(r.description);
            }
            entry["mimeType"] = json!(if r.mime_type.is_empty() { "text/plain" } else { &r.mime_type });
            entry
        })
        .collect()
}

/// The values a URI takes for a template's `{name}` parts, when it matches.
fn match_template(template: &str, uri: &str) -> Option<Map<String, Value>> {
    let mut params = Map::new();
    let mut rest = uri;
    let mut pattern = template;
    loop {
        match pattern.find('{') {
            None => return (rest == pattern).then_some(params),
            Some(open) => {
                let literal = &pattern[..open];
                rest = rest.strip_prefix(literal)?;
                let close = pattern[open..].find('}')? + open;
                let name = pattern[open + 1..close].trim_start_matches(['+', '#', '/', '?', '&']);
                pattern = &pattern[close + 1..];
                // A value runs to the next literal part (or the end).
                let next = pattern.find('{').map_or(pattern, |i| &pattern[..i]);
                let end = if next.is_empty() { rest.len() } else { rest.find(next)? };
                if end == 0 {
                    return None;
                }
                params.insert(name.to_string(), Value::String(rest[..end].to_string()));
                rest = &rest[end..];
            }
        }
    }
}

fn read_resource(config: &McpServerConfig, params: &Value, vars: &VarContext) -> Result<Value, (i64, String)> {
    let uri = params["uri"].as_str().unwrap_or_default();
    let found = config.resources.iter().filter(|r| r.enabled).find_map(|r| {
        if is_template(&r.uri) {
            match_template(&r.uri, uri).map(|p| (r, p))
        } else {
            (r.uri == uri).then(|| (r, Map::new()))
        }
    });
    let Some((resource, values)) = found else {
        return Err((-32002, format!("Resource not found: {uri}")));
    };
    let mime = if resource.mime_type.is_empty() { "text/plain" } else { &resource.mime_type };
    let text = render(&resource.text, &Map::new(), &values, vars);
    Ok(json!({ "contents": [{ "uri": uri, "mimeType": mime, "text": text }] }))
}

fn prompt_entry(prompt: &zorvik_formats::McpPromptMock) -> Value {
    let mut entry = json!({ "name": prompt.name });
    if !prompt.title.is_empty() {
        entry["title"] = json!(prompt.title);
    }
    if !prompt.description.is_empty() {
        entry["description"] = json!(prompt.description);
    }
    if !prompt.arguments.is_empty() {
        entry["arguments"] = prompt
            .arguments
            .iter()
            .map(|a| {
                let mut arg = json!({ "name": a.name, "required": a.required });
                if !a.description.is_empty() {
                    arg["description"] = json!(a.description);
                }
                arg
            })
            .collect();
    }
    entry
}

fn get_prompt(config: &McpServerConfig, params: &Value, vars: &VarContext) -> Result<Value, (i64, String)> {
    let name = params["name"].as_str().unwrap_or_default();
    let Some(prompt) = config.prompts.iter().find(|p| p.enabled && p.name == name) else {
        return Err((-32602, format!("Unknown prompt: {name}")));
    };
    let args = params["arguments"].as_object().cloned().unwrap_or_default();
    if let Some(missing) = prompt.arguments.iter().find(|a| a.required && args.get(&a.name).is_none_or(Value::is_null))
    {
        return Err((-32602, format!("Missing required argument: {}", missing.name)));
    }
    let messages: Vec<Value> = prompt
        .messages
        .iter()
        .map(|m| {
            let role = if m.role == "assistant" { "assistant" } else { "user" };
            json!({ "role": role, "content": { "type": "text", "text": render(&m.text, &args, &Map::new(), vars) } })
        })
        .collect();
    let mut result = json!({ "messages": messages });
    if !prompt.description.is_empty() {
        result["description"] = json!(prompt.description);
    }
    Ok(result)
}

/// A template with `{{args}}`, `{{args.name}}`, `{{params.name}}` (the client's values: text
/// as it is, anything else as JSON), dynamic variables and environment variables. The
/// client's values go in after the variables are filled in: a client sending `{{token}}` gets
/// that text back, never the variable's value.
fn render(template: &str, args: &Map<String, Value>, params: &Map<String, Value>, vars: &VarContext) -> String {
    let as_text = |v: &Value| match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    let fill = |text: &str| crate::template::render(text, None, vars);
    let mut out = String::new();
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        let Some(len) = rest[start..].find("}}") else { break };
        let key = rest[start + 2..start + len].trim();
        let value = if key == "args" {
            Some(Value::Object(args.clone()).to_string())
        } else if let Some(name) = key.strip_prefix("args.") {
            Some(args.get(name).map(as_text).unwrap_or_default())
        } else {
            key.strip_prefix("params.").map(|name| params.get(name).map(as_text).unwrap_or_default())
        };
        match value {
            Some(value) => {
                out.push_str(&fill(&rest[..start]));
                out.push_str(&value);
            }
            None => out.push_str(&fill(&rest[..start + len + 2])),
        }
        rest = &rest[start + len + 2..];
    }
    out.push_str(&fill(rest));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_match_uris() {
        let m = match_template("weather://{city}/today", "weather://Lisbon/today").unwrap();
        assert_eq!(m["city"], "Lisbon");
        let m = match_template("file:///{dir}/{name}.md", "file:///docs/intro.md").unwrap();
        assert_eq!((m["dir"].as_str(), m["name"].as_str()), (Some("docs"), Some("intro")));
        assert!(match_template("weather://{city}/today", "weather://Lisbon/tomorrow").is_none());
        assert!(match_template("weather://{city}", "weather://").is_none());
        assert!(match_template("a://x", "a://x").is_some());
    }

    #[test]
    fn answers_use_the_arguments_and_never_expand_them() {
        let mut vars = VarContext::new();
        vars.push_layer(&[zorvik_formats::Variable {
            key: "token".into(),
            value: "s3cret".into(),
            enabled: true,
            secret: true,
        }]);
        let args: Map<String, Value> = serde_json::from_value(json!({ "city": "{{token}}", "days": 3 })).unwrap();
        let out =
            render("{{args.city}} for {{args.days}} days, {{token}} {{args.nope}}|{{args}}", &args, &Map::new(), &vars);
        assert_eq!(out, r#"{{token}} for 3 days, s3cret |{"city":"{{token}}","days":3}"#);
        assert_eq!(missing_argument(r#"{"required": ["city", "days"]}"#, &args), None);
        assert_eq!(missing_argument(r#"{"required": ["units"]}"#, &args).as_deref(), Some("units"));
        assert_eq!(missing_argument("", &args), None);
    }

    #[test]
    fn summaries_name_what_was_asked() {
        assert_eq!(
            summary(&json!({ "method": "tools/call", "params": { "name": "get_weather" } })),
            "tools/call get_weather"
        );
        assert_eq!(
            summary(&json!({ "method": "resources/read", "params": { "uri": "a://b" } })),
            "resources/read a://b"
        );
        assert_eq!(summary(&json!({ "id": 1, "result": {} })), "response");
    }
}

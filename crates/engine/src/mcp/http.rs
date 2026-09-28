//! MCP over HTTP: the Streamable HTTP transport (protocol 2025-03-26 and later) and the older
//! HTTP+SSE transport (2024-11-05).
//!
//! Streamable HTTP: every message is a POST to the endpoint. The answer is JSON, or an event
//! stream carrying the response (and anything the server sends before it); `202` acknowledges
//! notifications. The server may hand out a session id (`Mcp-Session-Id`) with the
//! `initialize` answer, which later requests carry along with `MCP-Protocol-Version`. A GET
//! opens a stream for messages the server starts; ending the session sends a DELETE.
//!
//! HTTP+SSE: a GET opens an event stream whose first `endpoint` event says where to POST
//! messages; every answer comes back on the stream.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use serde_json::{Value, json};
use tokio::sync::mpsc;

use super::{Inbound, Link, MAX_MESSAGE, Outbound, lock};
use crate::cookies::CookieJar;
use crate::error::{EngineError, ErrorKind, Result};
use crate::http::{Client, Header, HttpRequest, RequestOptions, StreamingResponse};
use crate::sse::SseParser;

/// Body bytes shown when the server answers with an error status.
const MAX_ERROR_BODY: usize = 1024;

/// The session id and protocol version Streamable HTTP requests carry.
#[derive(Default)]
pub(crate) struct HttpState {
    session_id: Mutex<Option<String>>,
    protocol_version: Mutex<Option<String>>,
}

impl HttpState {
    pub fn session_id(&self) -> Option<String> {
        lock(&self.session_id).clone()
    }

    pub fn set_protocol_version(&self, version: &str) {
        if !version.is_empty() {
            *lock(&self.protocol_version) = Some(version.to_string());
        }
    }

    fn headers(&self, base: &[Header]) -> Vec<Header> {
        let mut headers = base.to_vec();
        if let Some(id) = lock(&self.session_id).as_ref() {
            headers.push(Header::new("Mcp-Session-Id", id.clone()));
        }
        if let Some(version) = lock(&self.protocol_version).as_ref() {
            headers.push(Header::new("MCP-Protocol-Version", version.clone()));
        }
        headers
    }
}

/// Everything a transport task needs to make requests.
#[derive(Clone)]
struct Http {
    client: Arc<Client>,
    opts: RequestOptions,
    jar: Option<Arc<CookieJar>>,
    headers: Vec<Header>,
}

impl Http {
    async fn open(
        &self,
        method: &str,
        url: &str,
        headers: Vec<Header>,
        body: Option<&Value>,
    ) -> Result<StreamingResponse> {
        let mut request = HttpRequest {
            method: method.into(),
            url: url.into(),
            headers,
            body: body.map(|b| Bytes::from(b.to_string())).unwrap_or_default(),
        };
        if body.is_some() && !request.headers.iter().any(|h| h.name.eq_ignore_ascii_case("content-type")) {
            request.headers.push(Header::new("Content-Type", "application/json"));
        }
        self.client.open_stream(request, &self.opts, self.jar.as_deref()).await
    }
}

fn header<'a>(response: &'a StreamingResponse, name: &str) -> Option<&'a str> {
    response.meta.headers.iter().find(|h| h.name.eq_ignore_ascii_case(name)).map(|h| h.value.as_str())
}

fn is_event_stream(response: &StreamingResponse) -> bool {
    header(response, "content-type").is_some_and(|t| t.to_ascii_lowercase().contains("text/event-stream"))
}

/// The start of an error answer's body.
async fn error_body(response: &mut StreamingResponse) -> String {
    let mut bytes = Vec::new();
    let read = async {
        while let Some(Ok(chunk)) = response.body.next_chunk().await {
            bytes.extend_from_slice(&chunk);
            if bytes.len() > MAX_ERROR_BODY {
                break;
            }
        }
    };
    let _ = tokio::time::timeout(Duration::from_secs(3), read).await;
    let text: String = String::from_utf8_lossy(&bytes).chars().take(MAX_ERROR_BODY).collect();
    text.trim().to_string()
}

/// An error status in words, e.g. `HTTP 401 Unauthorized: …`.
fn status_text(response: &StreamingResponse, body: &str) -> String {
    let mut text = format!("HTTP {} {}", response.meta.status, response.meta.status_text);
    if response.meta.status == 401 {
        text.push_str(": the server wants credentials (set them in the Auth tab)");
    } else if !body.is_empty() {
        text.push_str(": ");
        text.push_str(body);
    }
    text
}

/// Whether initializing failed the way a 2024-11-05 server fails a Streamable HTTP POST.
pub(super) fn is_old_server(e: &EngineError) -> bool {
    ["HTTP 400", "HTTP 404", "HTTP 405"].iter().any(|s| e.message.contains(s))
}

/// A JSON-RPC error answer for `request`, when the HTTP exchange itself failed.
fn failed(request: &Value, message: String) -> Option<Value> {
    let id = request.get("id").filter(|id| !id.is_null() && request.get("method").is_some())?;
    Some(json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32000, "message": message } }))
}

/// Deliver the messages of a JSON body (one message or a batch).
async fn json_messages(
    mut response: StreamingResponse,
    events: &mpsc::UnboundedSender<Inbound>,
) -> std::result::Result<(), String> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response.body.next_chunk().await {
        let chunk = chunk.map_err(|e| e.message)?;
        bytes.extend_from_slice(&chunk);
        if bytes.len() > MAX_MESSAGE {
            return Err(format!("The server sent a message larger than {} MB", MAX_MESSAGE >> 20));
        }
    }
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return Ok(());
    }
    match serde_json::from_slice::<Value>(&bytes) {
        Ok(Value::Array(batch)) => batch.into_iter().for_each(|m| {
            let _ = events.send(Inbound::Message(m));
        }),
        Ok(message) => {
            let _ = events.send(Inbound::Message(message));
        }
        Err(e) => return Err(format!("The server's answer is not JSON: {e}")),
    }
    Ok(())
}

/// Deliver the messages of an event stream until it ends; returns the `endpoint` event's data
/// (HTTP+SSE) through `endpoint` when it comes.
async fn stream_messages(
    mut response: StreamingResponse,
    events: &mpsc::UnboundedSender<Inbound>,
    mut endpoint: Option<tokio::sync::oneshot::Sender<String>>,
) -> std::result::Result<(), String> {
    let mut parser = SseParser::new();
    while let Some(chunk) = response.body.next_chunk().await {
        let chunk = chunk.map_err(|e| e.message)?;
        for event in parser.feed(&chunk) {
            if event.event == "endpoint" {
                if let Some(tx) = endpoint.take() {
                    let _ = tx.send(event.data);
                }
                continue;
            }
            if event.data.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<Value>(&event.data) {
                Ok(Value::Array(batch)) => batch.into_iter().for_each(|m| {
                    let _ = events.send(Inbound::Message(m));
                }),
                Ok(message) => {
                    let _ = events.send(Inbound::Message(message));
                }
                Err(_) => {
                    let start: String = event.data.chars().take(200).collect();
                    let _ = events.send(Inbound::Error(format!("The server sent an event that isn't JSON: {start}")));
                }
            }
        }
    }
    Ok(())
}

// ---- Streamable HTTP ---------------------------------------------------------------------

pub(super) fn streamable(
    client: Arc<Client>,
    url: &str,
    headers: &[Header],
    opts: &RequestOptions,
    jar: Option<Arc<CookieJar>>,
) -> Link {
    let state = Arc::new(HttpState::default());
    let http = Http { client, opts: opts.clone(), jar, headers: headers.to_vec() };
    let (tx, mut rx) = mpsc::unbounded_channel::<Outbound>();
    let (in_tx, in_rx) = mpsc::unbounded_channel();
    let url = url.to_string();
    let task_state = state.clone();
    tokio::spawn(async move {
        let mut listener: Option<tokio::task::JoinHandle<()>> = None;
        // Requests are posted side by side (answers may take a while, or stream).
        let mut requests = tokio::task::JoinSet::new();
        while let Some(Outbound::Message(message)) = rx.recv().await {
            while requests.try_join_next().is_some() {}
            // The server's own stream, once the session is set up.
            if message["method"] == "notifications/initialized" && listener.is_none() {
                listener = Some(tokio::spawn(listen(http.clone(), url.clone(), task_state.clone(), in_tx.clone())));
            }
            let post = post(http.clone(), url.clone(), task_state.clone(), message.clone(), in_tx.clone());
            if message.get("method").is_some() && message.get("id").is_some_and(|id| !id.is_null()) {
                requests.spawn(post);
            } else {
                // Notifications and answers go out in order: `notifications/initialized` must
                // reach the server before the requests that follow it.
                post.await;
            }
        }
        if let Some(task) = listener {
            task.abort();
        }
        // Answers still streaming in are for a session that is over.
        requests.abort_all();
        // End the session on the server (best effort).
        if task_state.session_id().is_some() {
            let delete = http.open("DELETE", &url, task_state.headers(&http.headers), None);
            let _ = tokio::time::timeout(Duration::from_secs(5), delete).await;
        }
        let _ = in_tx.send(Inbound::Closed("Disconnected".into()));
    });
    Link { tx, rx: in_rx, label: "Streamable HTTP", pid: None, http: Some(state) }
}

/// POST one message and deliver what comes back.
async fn post(http: Http, url: String, state: Arc<HttpState>, message: Value, events: mpsc::UnboundedSender<Inbound>) {
    let mut headers = state.headers(&http.headers);
    headers.push(Header::new("Accept", "application/json, text/event-stream"));
    let is_initialize = message["method"] == "initialize";
    let mut response = match http.open("POST", &url, headers, Some(&message)).await {
        Ok(r) => r,
        Err(e) => {
            let text = format!("Couldn't reach the server: {}", e.message);
            let _ = events.send(match failed(&message, text.clone()) {
                Some(answer) => Inbound::Message(answer),
                None => Inbound::Error(text),
            });
            return;
        }
    };
    if is_initialize && let Some(id) = header(&response, "mcp-session-id") {
        *lock(&state.session_id) = Some(id.to_string());
    }
    let status = response.meta.status;
    if status == 404 && state.session_id().is_some() && !is_initialize {
        let _ = events.send(Inbound::Closed("the server ended the session (HTTP 404)".into()));
        return;
    }
    if !(200..300).contains(&status) {
        let body = error_body(&mut response).await;
        let text = status_text(&response, &body);
        let _ = events.send(match failed(&message, text.clone()) {
            Some(answer) => Inbound::Message(answer),
            None => Inbound::Error(format!("The server refused a message: {text}")),
        });
        return;
    }
    let result = if is_event_stream(&response) {
        stream_messages(response, &events, None).await
    } else if status == 202 {
        Ok(())
    } else {
        json_messages(response, &events).await
    };
    if let Err(text) = result {
        let _ = events.send(match failed(&message, text.clone()) {
            Some(answer) => Inbound::Message(answer),
            None => Inbound::Error(text),
        });
    }
}

/// The GET stream for messages the server starts (servers may not offer one: 405).
async fn listen(http: Http, url: String, state: Arc<HttpState>, events: mpsc::UnboundedSender<Inbound>) {
    let mut headers = state.headers(&http.headers);
    headers.push(Header::new("Accept", "text/event-stream"));
    let Ok(response) = http.open("GET", &url, headers, None).await else { return };
    if !(200..300).contains(&response.meta.status) || !is_event_stream(&response) {
        return;
    }
    if let Err(e) = stream_messages(response, &events, None).await {
        let _ = events.send(Inbound::Info(format!("The server's event stream ended: {e}")));
    }
}

// ---- HTTP+SSE (2024-11-05) ----------------------------------------------------------------

pub(super) async fn connect_sse(
    client: Arc<Client>,
    url: &str,
    headers: &[Header],
    opts: &RequestOptions,
    jar: Option<Arc<CookieJar>>,
    wait: Duration,
) -> Result<Link> {
    let http = Http { client, opts: opts.clone(), jar, headers: headers.to_vec() };
    let mut get_headers = headers.to_vec();
    get_headers.push(Header::new("Accept", "text/event-stream"));
    let mut response = http.open("GET", url, get_headers, None).await?;
    if !(200..300).contains(&response.meta.status) || !is_event_stream(&response) {
        let body = error_body(&mut response).await;
        return Err(EngineError::new(
            ErrorKind::Protocol,
            format!("The server didn't open an MCP event stream ({})", status_text(&response, &body)),
        ));
    }
    let (in_tx, in_rx) = mpsc::unbounded_channel();
    let (endpoint_tx, endpoint_rx) = tokio::sync::oneshot::channel();
    let reader_events = in_tx.clone();
    let reader = tokio::spawn(async move {
        let reason = match stream_messages(response, &reader_events, Some(endpoint_tx)).await {
            Ok(()) => "the server ended the event stream".to_string(),
            Err(e) => format!("the event stream failed: {e}"),
        };
        let _ = reader_events.send(Inbound::Closed(reason));
    });
    let endpoint = match tokio::time::timeout(wait, endpoint_rx).await {
        Ok(Ok(endpoint)) => endpoint,
        _ => {
            reader.abort();
            return Err(EngineError::new(
                ErrorKind::Protocol,
                "The server's event stream didn't say where to send messages (no endpoint event)",
            ));
        }
    };
    let not_a_url = |e: url::ParseError| {
        EngineError::new(ErrorKind::Protocol, format!("The server's message endpoint '{endpoint}' is not a URL: {e}"))
    };
    let base = url::Url::parse(url).map_err(not_a_url)?;
    let post_url = base.join(endpoint.trim()).map_err(not_a_url)?;
    // Messages (and the headers with them, auth included) go only to the server connected to.
    if post_url.origin() != base.origin() {
        reader.abort();
        return Err(EngineError::new(
            ErrorKind::Protocol,
            format!(
                "The server asked for messages to go to another site ({post_url}); Zorvik only sends them to {base}"
            ),
        ));
    }
    let post_url = post_url.to_string();
    let (tx, mut rx) = mpsc::unbounded_channel::<Outbound>();
    tokio::spawn(async move {
        // One at a time, in order: answers come back on the event stream.
        while let Some(Outbound::Message(message)) = rx.recv().await {
            let result = http.open("POST", &post_url, http.headers.clone(), Some(&message)).await;
            let problem = match result {
                Ok(mut r) if !(200..300).contains(&r.meta.status) => {
                    let body = error_body(&mut r).await;
                    Some(status_text(&r, &body))
                }
                Ok(_) => None,
                Err(e) => Some(format!("Couldn't reach the server: {}", e.message)),
            };
            if let Some(text) = problem {
                let _ = in_tx.send(match failed(&message, text.clone()) {
                    Some(answer) => Inbound::Message(answer),
                    None => Inbound::Error(text),
                });
            }
        }
        reader.abort();
        let _ = in_tx.send(Inbound::Closed("Disconnected".into()));
    });
    Ok(Link { tx, rx: in_rx, label: "HTTP+SSE", pid: None, http: None })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_exchanges_answer_their_request() {
        let answer = failed(&json!({ "jsonrpc": "2.0", "id": 4, "method": "tools/call" }), "HTTP 500".into()).unwrap();
        assert_eq!((answer["id"].clone(), answer["error"]["message"].clone()), (json!(4), json!("HTTP 500")));
        assert!(failed(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }), "x".into()).is_none());
        assert!(
            failed(&json!({ "jsonrpc": "2.0", "id": 4, "result": {} }), "x".into()).is_none(),
            "a response needs no answer"
        );
        assert!(is_old_server(&EngineError::new(
            ErrorKind::Protocol,
            "The server refused to initialize: HTTP 405 Method Not Allowed"
        )));
        assert!(!is_old_server(&EngineError::new(ErrorKind::Protocol, "HTTP 401 Unauthorized")));
    }
}

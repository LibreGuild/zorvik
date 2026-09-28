//! GraphQL subscriptions: over WebSocket with the `graphql-transport-ws` protocol (the
//! graphql-ws library) or the older `subscriptions-transport-ws`, and over Server-Sent Events
//! (graphql-sse, "distinct connections" mode). Each returns a socket session: the operation's
//! results arrive as received messages; dropping the session ends the subscription.

use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tokio::sync::mpsc;

use crate::cookies::CookieJar;
use crate::error::{EngineError, ErrorKind, Result};
use crate::http::{Client, Header, HttpRequest, RequestOptions};
use crate::socket::{SocketConnected, SocketEvent, SocketOpened, SocketOutgoing, SocketSession, ms};
use crate::sse::SseParser;
use crate::ws::{Direction, WsConnected, WsEvent, WsOutgoing, WsSession};

/// The id of the one operation a session runs.
const ID: &str = "1";
/// How long the server gets to accept the connection when no request timeout is set.
const ACK_TIMEOUT: Duration = Duration::from_secs(10);
/// Body bytes shown when the server answers a subscription with something else than events.
const MAX_ERROR_BODY: usize = 2048;

/// The WebSocket protocol a subscription speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphqlWsProtocol {
    /// `graphql-transport-ws`: the graphql-ws library, Apollo Server 4+, Hasura, …
    TransportWs,
    /// `subscriptions-transport-ws` (subprotocol `graphql-ws`): Apollo Server 2 and 3.
    Legacy,
}

impl GraphqlWsProtocol {
    fn subprotocol(self) -> &'static str {
        match self {
            GraphqlWsProtocol::TransportWs => "graphql-transport-ws",
            GraphqlWsProtocol::Legacy => "graphql-ws",
        }
    }

    fn label(self) -> &'static str {
        match self {
            GraphqlWsProtocol::TransportWs => "graphql-transport-ws",
            GraphqlWsProtocol::Legacy => "subscriptions-transport-ws",
        }
    }
}

impl Client {
    /// Start a subscription over WebSocket. `req` gives the URL (http(s) or ws(s)) and the
    /// handshake headers; `operation` is `{"query", "variables", "operationName"}`. Returns once
    /// the server accepted the connection and the subscription was sent.
    pub async fn graphql_ws(
        &self,
        mut req: HttpRequest,
        protocol: GraphqlWsProtocol,
        operation: Value,
        connection_params: Option<Value>,
        opts: &RequestOptions,
        jar: Option<&CookieJar>,
    ) -> Result<SocketConnected> {
        let started = Instant::now();
        // Body headers describe the HTTP body the operation would have had, not the handshake.
        req.headers.retain(|h| !h.name.eq_ignore_ascii_case("content-type"));
        if !req.headers.iter().any(|h| h.name.eq_ignore_ascii_case("sec-websocket-protocol")) {
            req.headers.push(Header::new("Sec-WebSocket-Protocol", protocol.subprotocol()));
        }
        let WsConnected { meta, mut timing, session, mut events } = self.websocket(req, opts, jar).await?;

        let mut init = json!({ "type": "connection_init" });
        if let Some(params) = connection_params {
            init["payload"] = params;
        }
        send_json(&session, &init)?;
        let wait = opts.timeout.unwrap_or(ACK_TIMEOUT);
        let ack = async {
            loop {
                match events.recv().await {
                    Some(WsEvent::Message { direction: Direction::Received, text: Some(text), .. }) => {
                        let message = parse(&text)?;
                        match message["type"].as_str().unwrap_or_default() {
                            "connection_ack" => return Ok(()),
                            "ping" => send_json(&session, &pong(&message))?,
                            "connection_error" => {
                                return Err(EngineError::new(
                                    ErrorKind::Protocol,
                                    format!("The server refused the connection: {}", message["payload"]),
                                ));
                            }
                            _ => {}
                        }
                    }
                    Some(WsEvent::Closed { code, reason, .. }) => {
                        return Err(EngineError::new(
                            ErrorKind::Protocol,
                            format!(
                                "The server closed the connection before accepting it ({})",
                                close_reason(code, &reason)
                            ),
                        ));
                    }
                    Some(WsEvent::Error { message }) => return Err(EngineError::new(ErrorKind::Io, message)),
                    Some(_) => {}
                    None => return Err(EngineError::new(ErrorKind::Io, "The connection closed")),
                }
            }
        };
        tokio::time::timeout(wait, ack).await.map_err(|_| {
            EngineError::timeout("Waiting for the server to accept the connection (connection_ack)", wait)
        })??;

        let start = match protocol {
            GraphqlWsProtocol::TransportWs => "subscribe",
            GraphqlWsProtocol::Legacy => "start",
        };
        send_json(&session, &json!({ "id": ID, "type": start, "payload": operation }))?;
        timing.total_ms = ms(started.elapsed());

        let (out_tx, out_rx) = mpsc::unbounded_channel();
        let (ev_tx, ev_rx) = mpsc::unbounded_channel();
        let _ = ev_tx.send(sent(&operation));
        tokio::spawn(relay_ws(session, events, out_rx, ev_tx, protocol));
        let opened = SocketOpened {
            protocol: format!("GraphQL over WebSocket ({})", protocol.label()),
            remote_addr: meta.remote_addr.clone(),
            local_addr: None,
            tls: meta.tls.clone(),
            timing,
        };
        Ok(SocketConnected { opened, meta: Some(meta), session: SocketSession { tx: out_tx }, events: ev_rx })
    }

    /// Start a subscription over Server-Sent Events: `req` is the POST that carries the operation.
    pub async fn graphql_sse(
        &self,
        mut req: HttpRequest,
        opts: &RequestOptions,
        jar: Option<&CookieJar>,
    ) -> Result<SocketConnected> {
        if !req.headers.iter().any(|h| h.name.eq_ignore_ascii_case("accept")) {
            req.headers.push(Header::new("Accept", "text/event-stream"));
        }
        let operation = serde_json::from_slice::<Value>(&req.body).unwrap_or(Value::Null);
        let stream = self.open_stream(req, opts, jar).await?;
        let crate::http::StreamingResponse { meta, timing, mut body } = stream;
        let is_stream = meta.headers.iter().any(|h| {
            h.name.eq_ignore_ascii_case("content-type") && h.value.to_ascii_lowercase().contains("text/event-stream")
        });
        if !is_stream || !(200..300).contains(&meta.status) {
            let mut bytes = Vec::new();
            let read = async {
                while let Some(Ok(chunk)) = body.next_chunk().await {
                    bytes.extend_from_slice(&chunk);
                    if bytes.len() > MAX_ERROR_BODY {
                        break;
                    }
                }
            };
            let _ = tokio::time::timeout(Duration::from_secs(2), read).await;
            let text: String = String::from_utf8_lossy(&bytes).chars().take(MAX_ERROR_BODY).collect();
            let what = if is_stream { "answered" } else { "didn't start an event stream" };
            return Err(EngineError::new(
                ErrorKind::Protocol,
                format!("The server {what} ({} {}): {}", meta.status, meta.status_text, text.trim()),
            ));
        }

        let (out_tx, mut out_rx) = mpsc::unbounded_channel::<SocketOutgoing>();
        let (ev_tx, ev_rx) = mpsc::unbounded_channel();
        let _ = ev_tx.send(sent(&operation));
        tokio::spawn(async move {
            let mut parser = SseParser::new();
            loop {
                tokio::select! {
                    out = out_rx.recv() => if out.is_none() {
                        // Dropping the body closes the connection.
                        let _ = ev_tx.send(SocketEvent::Closed { reason: "Unsubscribed".into(), by_client: true });
                        return;
                    },
                    chunk = body.next_chunk() => match chunk {
                        Some(Ok(bytes)) => {
                            for event in parser.feed(&bytes) {
                                match event.event.as_str() {
                                    "next" | "message" => {
                                        let _ = ev_tx.send(SocketEvent::message(Direction::Received, event.data.as_bytes()));
                                    }
                                    "complete" => {
                                        let reason = "The server completed the subscription".into();
                                        let _ = ev_tx.send(SocketEvent::Closed { reason, by_client: false });
                                        return;
                                    }
                                    other => {
                                        let _ = ev_tx.send(SocketEvent::info(format!("Event \"{other}\": {}", event.data)));
                                    }
                                }
                            }
                        }
                        Some(Err(e)) => {
                            let _ = ev_tx.send(SocketEvent::Error { message: e.message });
                            let _ = ev_tx.send(SocketEvent::Closed { reason: "Connection lost".into(), by_client: false });
                            return;
                        }
                        None => {
                            let _ = ev_tx.send(SocketEvent::Closed { reason: "The server ended the stream".into(), by_client: false });
                            return;
                        }
                    }
                }
            }
        });
        let opened = SocketOpened {
            protocol: format!("GraphQL over SSE ({})", meta.http_version),
            remote_addr: meta.remote_addr.clone(),
            local_addr: None,
            tls: meta.tls.clone(),
            timing,
        };
        Ok(SocketConnected { opened, meta: Some(meta), session: SocketSession { tx: out_tx }, events: ev_rx })
    }
}

/// The operation, shown as the first (sent) message.
fn sent(operation: &Value) -> SocketEvent {
    let text = operation.to_string();
    let mut event = SocketEvent::message(Direction::Sent, text.as_bytes());
    if let SocketEvent::Message { detail, .. } = &mut event {
        *detail = Some("subscribe".into());
    }
    event
}

fn send_json(session: &WsSession, value: &Value) -> Result<()> {
    session.send(WsOutgoing::Text { text: value.to_string() })
}

fn parse(text: &str) -> Result<Value> {
    serde_json::from_str::<Value>(text).ok().filter(Value::is_object).ok_or_else(|| {
        let start: String = text.chars().take(200).collect();
        EngineError::new(
            ErrorKind::Protocol,
            format!("The server sent something that isn't a GraphQL message: {start}"),
        )
    })
}

fn pong(ping: &Value) -> Value {
    let mut pong = json!({ "type": "pong" });
    if let Some(payload) = ping.get("payload") {
        pong["payload"] = payload.clone();
    }
    pong
}

/// A close code and reason in words (the graphql-transport-ws codes explained).
fn close_reason(code: Option<u16>, reason: &str) -> String {
    let meaning = match code {
        Some(4400) => "the server couldn't read a message",
        Some(4401) => "not authorized: the connection needs credentials",
        Some(4403) => "forbidden: check the connection params and auth",
        Some(4406) => "the server doesn't speak this subscription protocol; try the other WebSocket protocol",
        Some(4408) => "the server waited too long for the connection to start",
        Some(4409) => "a subscription with this id already runs",
        Some(4429) => "too many connection attempts",
        Some(4500) => "the server failed",
        _ => "",
    };
    let parts: Vec<String> = [
        code.map(|c| format!("code {c}")),
        (!reason.is_empty()).then(|| reason.to_string()),
        (!meaning.is_empty()).then(|| meaning.to_string()),
    ]
    .into_iter()
    .flatten()
    .collect();
    if parts.is_empty() { "no reason given".into() } else { parts.join(" · ") }
}

/// Turns the WebSocket's messages into the subscription's results until one side ends it.
async fn relay_ws(
    session: WsSession,
    mut ws_events: mpsc::UnboundedReceiver<WsEvent>,
    mut outgoing: mpsc::UnboundedReceiver<SocketOutgoing>,
    events: mpsc::UnboundedSender<SocketEvent>,
    protocol: GraphqlWsProtocol,
) {
    let stop = |session: &WsSession| {
        let _ = match protocol {
            GraphqlWsProtocol::TransportWs => send_json(session, &json!({ "id": ID, "type": "complete" })),
            GraphqlWsProtocol::Legacy => send_json(session, &json!({ "id": ID, "type": "stop" }))
                .and_then(|()| send_json(session, &json!({ "type": "connection_terminate" }))),
        };
        let _ = session.send(WsOutgoing::Close { code: Some(1000), reason: None });
    };
    loop {
        tokio::select! {
            out = outgoing.recv() => match out {
                None => {
                    stop(&session);
                    let _ = events.send(SocketEvent::Closed { reason: "Unsubscribed".into(), by_client: true });
                    return;
                }
                Some(_) => {
                    let _ = events.send(SocketEvent::Error { message: "A subscription only receives".into() });
                }
            },
            event = ws_events.recv() => match event {
                Some(WsEvent::Message { direction: Direction::Received, text: Some(text), .. }) => {
                    let message = match parse(&text) {
                        Ok(m) => m,
                        Err(e) => {
                            let _ = events.send(SocketEvent::Error { message: e.message });
                            continue;
                        }
                    };
                    let payload = &message["payload"];
                    match message["type"].as_str().unwrap_or_default() {
                        // `data` is subscriptions-transport-ws.
                        "next" | "data" => {
                            let text = payload.to_string();
                            let _ = events.send(SocketEvent::message(Direction::Received, text.as_bytes()));
                        }
                        "error" => {
                            let _ = events.send(SocketEvent::Error {
                                message: format!("The server rejected the subscription: {payload}"),
                            });
                            stop(&session);
                            let _ = events.send(SocketEvent::Closed { reason: "The subscription failed".into(), by_client: false });
                            return;
                        }
                        "complete" => {
                            let _ = session.send(WsOutgoing::Close { code: Some(1000), reason: None });
                            let reason = "The server completed the subscription".into();
                            let _ = events.send(SocketEvent::Closed { reason, by_client: false });
                            return;
                        }
                        "ping" => {
                            let _ = send_json(&session, &pong(&message));
                        }
                        // Keep-alives and the acknowledgement need no answer.
                        "pong" | "ka" | "connection_ack" => {}
                        other => {
                            let _ = events.send(SocketEvent::info(format!("Message \"{other}\": {text}")));
                        }
                    }
                }
                Some(WsEvent::Message { direction: Direction::Received, .. }) => {
                    let _ = events.send(SocketEvent::Error { message: "Ignored a binary message (GraphQL messages are text)".into() });
                }
                Some(WsEvent::Message { .. }) => {}
                Some(WsEvent::Error { message }) => {
                    let _ = events.send(SocketEvent::Error { message });
                }
                Some(WsEvent::Closed { code, reason, by_client }) => {
                    let _ = events.send(SocketEvent::Closed { reason: close_reason(code, &reason), by_client });
                    return;
                }
                None => {
                    let _ = events.send(SocketEvent::Closed { reason: "Connection closed".into(), by_client: false });
                    return;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_codes_in_words() {
        assert_eq!(
            close_reason(Some(4403), "Forbidden"),
            "code 4403 · Forbidden · forbidden: check the connection params and auth"
        );
        assert_eq!(close_reason(Some(1000), ""), "code 1000");
        assert_eq!(close_reason(None, ""), "no reason given");
    }

    #[test]
    fn messages_must_be_json_objects() {
        assert!(parse(r#"{"type":"next"}"#).is_ok());
        let e = parse("hello").unwrap_err();
        assert!(e.message.contains("isn't a GraphQL message: hello"), "{}", e.message);
        assert!(parse("[1]").is_err());
        assert_eq!(
            pong(&json!({ "type": "ping", "payload": { "a": 1 } })),
            json!({ "type": "pong", "payload": { "a": 1 } })
        );
        assert_eq!(pong(&json!({ "type": "ping" })), json!({ "type": "pong" }));
    }

    #[test]
    fn the_operation_is_the_first_message() {
        match sent(&json!({ "query": "subscription { a }" })) {
            SocketEvent::Message { direction: Direction::Sent, text, detail, .. } => {
                assert_eq!(text.as_deref(), Some(r#"{"query":"subscription { a }"}"#));
                assert_eq!(detail.as_deref(), Some("subscribe"));
            }
            other => panic!("{other:?}"),
        }
    }
}

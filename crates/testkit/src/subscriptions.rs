//! GraphQL subscriptions for testing: `/graphql-ws` speaks `graphql-transport-ws` or the older
//! `subscriptions-transport-ws` (the subprotocol the client asks for), `/graphql-sse` answers
//! a POST with an event stream (graphql-sse).
//!
//! Every subscription sends `{"data": {"tick": n}}` for n = 1..=`variables.count` (default 3),
//! `variables.intervalMs` apart (default 10), then completes; `count: 0` never ends. The first
//! result also carries the received operation under `extensions.received`. A query containing
//! `boom` is rejected with an `error` message. `/graphql-ws?auth=1` wants
//! `{"token": "graphql-token"}` in the `connection_init` payload (else it closes with 4403).

use std::collections::HashMap;
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::extract::Query;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use futures_util::StreamExt;
use serde_json::{Value, json};
use tokio::sync::mpsc;

use crate::graphql::GRAPHQL_TOKEN;

pub(crate) async fn ws(
    upgrade: WebSocketUpgrade,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let offered = headers.get("sec-websocket-protocol").and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
    let offered: Vec<&str> = offered.split(',').map(str::trim).collect();
    let legacy = match () {
        _ if offered.contains(&"graphql-transport-ws") => false,
        _ if offered.contains(&"graphql-ws") => true,
        _ => return (StatusCode::BAD_REQUEST, "Ask for graphql-transport-ws or graphql-ws").into_response(),
    };
    let protocol = if legacy { "graphql-ws" } else { "graphql-transport-ws" };
    let auth = q.get("auth").is_some_and(|v| v == "1");
    upgrade.protocols([protocol]).on_upgrade(move |socket| serve_ws(socket, legacy, auth))
}

async fn serve_ws(mut socket: WebSocket, legacy: bool, auth: bool) {
    // connection_init first.
    let init = loop {
        match socket.next().await {
            Some(Ok(Message::Text(text))) => break serde_json::from_str::<Value>(&text).unwrap_or_default(),
            Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue,
            _ => return,
        }
    };
    if init["type"] != "connection_init" {
        let _ = socket.send(close(4400, "Expected connection_init")).await;
        return;
    }
    if auth && init["payload"]["token"] != GRAPHQL_TOKEN {
        let _ = socket.send(close(4403, "Forbidden")).await;
        return;
    }
    let _ = socket.send(text(json!({ "type": "connection_ack" }))).await;
    if !legacy {
        let _ = socket.send(text(json!({ "type": "ping" }))).await;
    }

    let (tx, mut rx) = mpsc::unbounded_channel::<Value>();
    let mut running: Option<tokio::task::JoinHandle<()>> = None;
    loop {
        tokio::select! {
            out = rx.recv() => {
                let Some(out) = out else { return };
                if socket.send(text(out)).await.is_err() {
                    return;
                }
            }
            incoming = socket.next() => {
                let message = match incoming {
                    Some(Ok(Message::Text(t))) => serde_json::from_str::<Value>(&t).unwrap_or_default(),
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    Some(Ok(_)) => continue,
                };
                let id = message["id"].clone();
                match message["type"].as_str().unwrap_or_default() {
                    "subscribe" | "start" => {
                        let payload = message["payload"].clone();
                        if payload["query"].as_str().unwrap_or_default().contains("boom") {
                            let errors = json!([{ "message": "Cannot query field \"boom\" on type \"Subscription\"." }]);
                            let errors = if legacy { json!({ "errors": errors }) } else { errors };
                            let _ = tx.send(json!({ "id": id, "type": "error", "payload": errors }));
                            continue;
                        }
                        let tx = tx.clone();
                        let next = if legacy { "data" } else { "next" };
                        running = Some(tokio::spawn(async move {
                            for result in results(&payload) {
                                tokio::time::sleep(interval(&payload)).await;
                                if tx.send(json!({ "id": id, "type": next, "payload": result })).is_err() {
                                    return;
                                }
                            }
                            let _ = tx.send(json!({ "id": id, "type": "complete" }));
                        }));
                    }
                    "complete" | "stop" => {
                        if let Some(task) = running.take() {
                            task.abort();
                        }
                    }
                    "ping" => {
                        let _ = tx.send(json!({ "type": "pong" }));
                    }
                    "connection_terminate" => break,
                    _ => {}
                }
            }
        }
    }
    if let Some(task) = running {
        task.abort();
    }
}

/// `POST /graphql-sse`: the operation's results as `next` events, then `complete`.
pub(crate) async fn sse(body: Bytes) -> Response {
    let Ok(payload) = serde_json::from_slice::<Value>(&body) else {
        return (StatusCode::BAD_REQUEST, axum::Json(json!({ "errors": [{ "message": "Body is not JSON" }] })))
            .into_response();
    };
    if payload["query"].as_str().unwrap_or_default().contains("boom") {
        let errors = json!({ "errors": [{ "message": "Cannot query field \"boom\" on type \"Subscription\"." }] });
        return (StatusCode::BAD_REQUEST, axum::Json(errors)).into_response();
    }
    let (tx, rx) = mpsc::unbounded_channel::<Result<Bytes, std::io::Error>>();
    tokio::spawn(async move {
        for result in results(&payload) {
            tokio::time::sleep(interval(&payload)).await;
            if tx.send(Ok(Bytes::from(format!("event: next\ndata: {result}\n\n")))).is_err() {
                return;
            }
        }
        let _ = tx.send(Ok(Bytes::from_static(b"event: complete\ndata:\n\n")));
    });
    let stream = tokio_stream_from(rx);
    ([(header::CONTENT_TYPE, "text/event-stream"), (header::CACHE_CONTROL, "no-cache")], Body::from_stream(stream))
        .into_response()
}

fn tokio_stream_from<T: Send + 'static>(
    mut rx: mpsc::UnboundedReceiver<T>,
) -> impl futures_util::Stream<Item = T> + Send + 'static {
    futures_util::stream::poll_fn(move |cx| rx.poll_recv(cx))
}

/// The results a subscription sends (endless with `count: 0`).
fn results(payload: &Value) -> Box<dyn Iterator<Item = Value> + Send> {
    let count = payload["variables"]["count"].as_u64().unwrap_or(3);
    let received = payload.clone();
    let ticks: Box<dyn Iterator<Item = u64> + Send> = if count == 0 { Box::new(1..) } else { Box::new(1..=count) };
    Box::new(ticks.map(move |n| {
        let mut result = json!({ "data": { "tick": n } });
        if n == 1 {
            result["extensions"] = json!({ "received": received });
        }
        result
    }))
}

fn interval(payload: &Value) -> Duration {
    Duration::from_millis(payload["variables"]["intervalMs"].as_u64().unwrap_or(10).min(10_000))
}

fn text(value: Value) -> Message {
    Message::Text(value.to_string().into())
}

fn close(code: u16, reason: &'static str) -> Message {
    Message::Close(Some(CloseFrame { code, reason: reason.into() }))
}

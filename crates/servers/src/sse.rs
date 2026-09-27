//! Server-Sent Events server: every GET (any path) opens an event stream that
//! plays the configured events in order (once or on repeat), plus events sent
//! from the UI. A comment every 15 s keeps idle streams (and proxies) alive.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use http::header::{self, HeaderMap, HeaderValue};
use http::{Method, Request, Response, StatusCode};
use hyper::body::Incoming;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, watch};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use zorvik_engine::Header;
use zorvik_formats::SseEventTemplate;

use crate::connections::{Commands, ConnCommand, Connections};
use crate::http::{Body, Handler, HandlerFuture, Peer, header_list, parse_query, serve};
use crate::report::{OutgoingMessage, Reporter, TrafficDirection};
use crate::template::{RequestValues, render};
use crate::{Control, Ctx, Live};

const KEEPALIVE: Duration = Duration::from_secs(15);
/// Pause between rounds when repeating without an interval (instead of a flood).
const REPEAT_PAUSE: Duration = Duration::from_secs(1);
/// Events buffered for a slow client before sending waits.
const BUFFERED_EVENTS: usize = 64;

/// An event sent from the UI.
#[derive(Clone)]
struct Pushed {
    event: String,
    data: String,
    id: String,
}

pub(crate) async fn run(listener: TcpListener, ctx: Ctx) -> Result<(), String> {
    let conns: Arc<Connections<Pushed>> = Arc::default();
    let state = Arc::new(Sse {
        live: ctx.live.clone(),
        reporter: ctx.reporter.clone(),
        conns: conns.clone(),
        stopped: ctx.cancel.clone(),
    });
    let handler: Handler = Arc::new(move |req: Request<Incoming>, peer: Peer| -> HandlerFuture {
        let state = state.clone();
        Box::pin(async move { Ok(state.open(req, peer)) })
    });
    serve(listener, ctx, handler, false, move |control| match control {
        Control::Send { conn, message, reply } => {
            let pushed = match message {
                OutgoingMessage::Event { event, data, id } => Ok(Pushed { event, data, id }),
                OutgoingMessage::Text { text } => Ok(Pushed { event: String::new(), data: text, id: String::new() }),
                OutgoingMessage::Binary { .. } => {
                    Err("Event streams carry text: send an event or text instead of binary data".to_string())
                }
            };
            let _ = reply.send(pushed.and_then(|p| conns.send(conn, p)));
        }
        Control::Disconnect { conn } => conns.close(conn),
    })
    .await
}

struct Sse {
    live: watch::Receiver<Arc<Live>>,
    reporter: Reporter,
    conns: Arc<Connections<Pushed>>,
    stopped: CancellationToken,
}

/// What a stream knows about the request that opened it (for `{{request.…}}`).
struct Opened {
    path: String,
    query_string: String,
    query: Vec<(String, String)>,
    headers: Vec<Header>,
    last_event_id: Option<String>,
}

impl Sse {
    fn open(&self, req: Request<Incoming>, peer: Peer) -> Response<Body> {
        let mut headers = HeaderMap::new();
        headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
        if req.method() != Method::GET {
            self.reporter.info(format!(
                "{} {} from {} → 405 (event streams are opened with GET)",
                req.method(),
                req.uri().path(),
                peer.addr
            ));
            headers.insert(header::ALLOW, HeaderValue::from_static("GET"));
            let mut response = Response::new(Body::full("Event streams are opened with GET\n"));
            *response.status_mut() = StatusCode::METHOD_NOT_ALLOWED;
            *response.headers_mut() = headers;
            return response;
        }
        let opened = Opened {
            path: req.uri().path().to_string(),
            query_string: req.uri().query().unwrap_or_default().to_string(),
            query: parse_query(req.uri().query().unwrap_or_default()),
            headers: header_list(req.headers()),
            last_event_id: req
                .headers()
                .get("last-event-id")
                .and_then(|v| v.to_str().ok())
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty()),
        };
        let (tx, rx) = mpsc::channel(BUFFERED_EVENTS);
        let conn = self.reporter.next_conn();
        tokio::spawn(session(
            Stream { conn, peer: peer.addr, tx, closed: peer.closed.clone(), opened },
            self.live.clone(),
            self.reporter.clone(),
            self.conns.clone(),
            self.stopped.clone(),
        ));
        headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/event-stream"));
        // Proxies such as nginx would otherwise hold events back.
        headers.insert("x-accel-buffering", HeaderValue::from_static("no"));
        let mut response = Response::new(Body::Stream(rx));
        *response.headers_mut() = headers;
        response
    }
}

struct Stream {
    conn: u64,
    peer: SocketAddr,
    tx: mpsc::Sender<Bytes>,
    /// The client connection ended (or the server stopped).
    closed: CancellationToken,
    opened: Opened,
}

async fn session(
    stream: Stream,
    live: watch::Receiver<Arc<Live>>,
    reporter: Reporter,
    conns: Arc<Connections<Pushed>>,
    stopped: CancellationToken,
) {
    let (conn, peer) = (stream.conn, stream.peer);
    reporter.opened(conn, &peer);
    let commands = conns.add(conn);
    let reason = play(&stream, live, &reporter, commands).await;
    conns.remove(conn);
    let reason = if stopped.is_cancelled() { "server stopped" } else { reason };
    reporter.closed(conn, &peer, reason);
}

/// Wire form of one event: `id:`, `event:` and one `data:` line per line of data.
fn format_event(event: &str, data: &str, id: &str) -> String {
    let one_line = |s: &str| s.replace(['\r', '\n'], " ");
    let mut out = String::with_capacity(data.len() + 32);
    if !id.is_empty() {
        out.push_str(&format!("id: {}\n", one_line(id)));
    }
    if !event.is_empty() {
        out.push_str(&format!("event: {}\n", one_line(event)));
    }
    for line in data.replace("\r\n", "\n").split(['\n', '\r']) {
        out.push_str("data: ");
        out.push_str(line);
        out.push('\n');
    }
    out.push('\n');
    out
}

/// Stream events until the client leaves, the UI disconnects it or the server stops.
async fn play(
    stream: &Stream,
    live: watch::Receiver<Arc<Live>>,
    reporter: &Reporter,
    mut commands: Commands<Pushed>,
) -> &'static str {
    let opened = &stream.opened;
    let values = RequestValues {
        method: "GET",
        path: &opened.path,
        query_string: &opened.query_string,
        query: &opened.query,
        headers: &opened.headers,
        ..Default::default()
    };
    // A reconnecting EventSource continues after the last event it saw.
    let mut next = {
        let current = live.borrow().clone();
        let events = &current.server.sse.events;
        opened
            .last_event_id
            .as_ref()
            .and_then(|last| events.iter().position(|e| !e.id.is_empty() && e.id == *last))
            .map_or(0, |i| i + 1)
    };
    let mut due = Instant::now();
    let mut keepalive = tokio::time::interval_at(Instant::now() + KEEPALIVE, KEEPALIVE);

    macro_rules! send {
        ($event:expr, $data:expr, $id:expr) => {{
            let (event, data, id): (&str, &str, &str) = ($event, $data, $id);
            let wire = format_event(event, data, id);
            tokio::select! {
                sent = stream.tx.send(Bytes::from(wire)) => if sent.is_err() { return "client disconnected" },
                _ = stream.closed.cancelled() => return "client disconnected",
            }
            let label = if event.is_empty() { "message" } else { event };
            reporter.data(Some(stream.conn), Some(&stream.peer), TrafficDirection::Out, data.as_bytes(), label);
        }};
    }

    loop {
        // Edits apply to the next event.
        let current = live.borrow().clone();
        let config = &current.server.sse;
        let events: &[SseEventTemplate] = &config.events;
        if next >= events.len() && config.repeat && !events.is_empty() {
            next = 0;
        }
        let pending = next < events.len();
        tokio::select! {
            _ = tokio::time::sleep_until(due), if pending => {
                let e = &events[next];
                let data = render(&e.data, Some(&values), &current.vars);
                let id = render(&e.id, Some(&values), &current.vars);
                send!(&e.event, &data, &id);
                next += 1;
                let now = Instant::now();
                due = if config.interval_ms > 0 {
                    now + Duration::from_millis(config.interval_ms)
                } else if next >= events.len() && config.repeat {
                    now + REPEAT_PAUSE
                } else {
                    now
                };
            }
            _ = keepalive.tick() => {
                let alive = tokio::select! {
                    sent = stream.tx.send(Bytes::from_static(b":keepalive\n\n")) => sent.is_ok(),
                    _ = stream.closed.cancelled() => false,
                };
                if !alive {
                    return "client disconnected";
                }
            }
            command = commands.recv() => match command {
                Some(ConnCommand::Send(p)) => {
                    let data = render(&p.data, Some(&values), &current.vars);
                    send!(&p.event, &data, &p.id);
                }
                Some(ConnCommand::Close) | None => return "closed by you",
            },
            _ = stream.tx.closed() => return "client disconnected",
            _ = stream.closed.cancelled() => return "client disconnected",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_wire_format() {
        assert_eq!(format_event("", "hi", ""), "data: hi\n\n");
        assert_eq!(format_event("tick", "a\r\nb\nc", "7"), "id: 7\nevent: tick\ndata: a\ndata: b\ndata: c\n\n");
        assert_eq!(format_event("x\ny", "", "1\r2"), "id: 1 2\nevent: x y\ndata: \n\n");
    }
}

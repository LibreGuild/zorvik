//! Socket.IO client: Socket.IO 3 and 4 (Engine.IO protocol 4) over WebSocket or HTTP
//! long-polling. A session joins one namespace, the URL's path (`http://host:3000/chat`
//! joins `/chat`). Events arrive as received messages whose topic is the event name;
//! [`SocketOutgoing::Emit`] sends one, optionally asking for an acknowledgement.

pub mod packet;

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use base64::Engine as _;
use serde_json::Value;
use tokio::sync::mpsc;
use url::Url;

use crate::cookies::CookieJar;
use crate::error::{EngineError, ErrorKind, Result};
use crate::http::{Client, Header, HttpRequest, RequestOptions, ResponseMeta, Timing};
use crate::socket::{SocketConnected, SocketEvent, SocketOpened, SocketOutgoing, SocketSession, ms};
use crate::ws::{Direction, WsConnected, WsEvent, WsMessageKind, WsOutgoing};
use packet::{CLOSE, Frame, Handshake, Kind, MESSAGE, OPEN, PING, PONG, Packet};

/// How long each step of connecting may take when no request timeout is set.
const STEP_TIMEOUT: Duration = Duration::from_secs(10);
/// Body characters shown when the server refuses the session.
const MAX_ERROR_BODY: usize = 300;

/// How the client reaches the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SocketIoTransport {
    /// WebSocket, or long-polling when the server doesn't take WebSocket.
    Auto,
    Websocket,
    Polling,
}

pub struct SocketIoConfig {
    /// The server's Socket.IO path (`/socket.io/` unless the server changed it).
    pub path: String,
    /// The connection's `auth` payload.
    pub auth: Option<Value>,
    pub transport: SocketIoTransport,
}

/// What the transport delivers.
enum Incoming {
    Frame(Frame),
    Error(String),
    Closed(String),
}

/// An open Engine.IO session.
struct Transport {
    tx: mpsc::UnboundedSender<Frame>,
    rx: mpsc::UnboundedReceiver<Incoming>,
    handshake: Handshake,
    label: &'static str,
    meta: ResponseMeta,
    timing: Timing,
}

/// The Engine.IO endpoint of a Socket.IO URL, and the namespace it names.
#[derive(Debug, PartialEq)]
struct Endpoint {
    /// `http(s)://host/socket.io/?…&EIO=4`
    url: Url,
    namespace: String,
}

fn endpoint(raw: &str, path: &str) -> Result<Endpoint> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(EngineError::invalid("URL is empty"));
    }
    let with_scheme = if crate::http::has_scheme(raw) { raw.to_string() } else { format!("http://{raw}") };
    let mut url = Url::parse(&with_scheme).map_err(|e| EngineError::invalid(format!("Invalid URL '{raw}': {e}")))?;
    let scheme = match url.scheme() {
        "http" | "ws" => "http",
        "https" | "wss" => "https",
        other => {
            return Err(EngineError::invalid(format!("Unsupported scheme '{other}' (use http, https, ws or wss)")));
        }
    };
    let _ = url.set_scheme(scheme);
    if url.host_str().is_none_or(str::is_empty) {
        return Err(EngineError::invalid(format!("URL '{raw}' has no host")));
    }
    let namespace = match url.path().trim_end_matches('/') {
        "" => "/".to_string(),
        path => path.to_string(),
    };
    let path = format!("/{}/", path.trim().trim_matches('/'));
    url.set_path(if path == "//" { "/" } else { &path });
    url.set_fragment(None);
    url.query_pairs_mut().append_pair("EIO", "4");
    Ok(Endpoint { url, namespace })
}

fn with_query(url: &Url, pairs: &[(&str, &str)]) -> Url {
    let mut url = url.clone();
    url.query_pairs_mut().extend_pairs(pairs);
    url
}

/// Cache busting for polling requests, like socket.io-client's `t`.
fn stamp() -> String {
    static N: AtomicU64 = AtomicU64::new(0);
    format!("{:x}", crate::socket::now_ms() as u64 ^ N.fetch_add(1, Ordering::Relaxed))
}

/// A refusal in words, naming the Socket.IO 2 case.
fn refused(status: u16, body: &str) -> EngineError {
    let body: String = body.trim().chars().take(MAX_ERROR_BODY).collect();
    let message = if body.contains("Unsupported protocol version") {
        "This server runs Socket.IO 2 (Engine.IO 3); Zorvik connects to Socket.IO 3 and 4".to_string()
    } else {
        format!("The server refused the Socket.IO session ({status}): {body}")
    };
    EngineError::new(ErrorKind::Protocol, message)
}

/// Whether a WebSocket error means the server doesn't take WebSocket (so polling may work).
fn upgrade_refused(e: &EngineError) -> bool {
    e.kind == ErrorKind::Protocol && e.message.starts_with("Server rejected the WebSocket upgrade")
}

impl Client {
    /// Connect to a Socket.IO server and join the URL's namespace. `req` gives the URL and
    /// the handshake headers.
    pub async fn socketio(
        self: &Arc<Self>,
        req: HttpRequest,
        config: SocketIoConfig,
        opts: &RequestOptions,
        jar: Option<Arc<CookieJar>>,
    ) -> Result<SocketConnected> {
        let started = Instant::now();
        let Endpoint { url, namespace } = endpoint(&req.url, &config.path)?;
        let headers: Vec<Header> = req
            .headers
            .into_iter()
            .filter(|h| !h.name.eq_ignore_ascii_case("content-type") && !h.name.eq_ignore_ascii_case("content-length"))
            .collect();
        let wait = opts.timeout.unwrap_or(STEP_TIMEOUT);
        let mut notes = Vec::new();
        let mut transport = match config.transport {
            SocketIoTransport::Websocket => self.open_ws(&url, &headers, opts, jar.as_deref(), wait).await?,
            SocketIoTransport::Polling => self.open_polling(&url, &headers, opts, jar.clone(), wait).await?,
            SocketIoTransport::Auto => match self.open_ws(&url, &headers, opts, jar.as_deref(), wait).await {
                Ok(t) => t,
                Err(e) if upgrade_refused(&e) => {
                    let transport = self.open_polling(&url, &headers, opts, jar.clone(), wait).await?;
                    let status =
                        e.message.split(": ").nth(1).and_then(|rest| rest.split(" — ").next()).unwrap_or("refused");
                    notes.push(format!("The server didn't take WebSocket ({status}); using HTTP long-polling"));
                    transport
                }
                Err(e) => return Err(e),
            },
        };

        let connect = Packet::new(Kind::Connect, &namespace, config.auth);
        send_packet(&transport.tx, &connect);
        let sid = tokio::time::timeout(wait, join(&mut transport, &namespace))
            .await
            .map_err(|_| EngineError::timeout(&format!("Waiting to join the namespace {namespace}"), wait))??;
        notes.push(match sid {
            Some(sid) => format!("Joined {namespace} as {sid}"),
            None => format!("Joined {namespace}"),
        });

        let Transport { tx, rx, handshake, label, meta, mut timing } = transport;
        timing.total_ms = ms(started.elapsed());
        let (out_tx, out_rx) = mpsc::unbounded_channel();
        let (ev_tx, ev_rx) = mpsc::unbounded_channel();
        for note in notes {
            let _ = ev_tx.send(SocketEvent::info(note));
        }
        let alive = Duration::from_millis(handshake.ping_interval + handshake.ping_timeout);
        tokio::spawn(relay(namespace, tx, rx, out_rx, ev_tx, alive));
        let opened = SocketOpened {
            protocol: format!("Socket.IO over {label}"),
            remote_addr: meta.remote_addr.clone(),
            local_addr: None,
            tls: meta.tls.clone(),
            timing,
        };
        Ok(SocketConnected { opened, meta: Some(meta), session: SocketSession { tx: out_tx }, events: ev_rx })
    }

    async fn open_ws(
        &self,
        url: &Url,
        headers: &[Header],
        opts: &RequestOptions,
        jar: Option<&CookieJar>,
        wait: Duration,
    ) -> Result<Transport> {
        let request = HttpRequest {
            method: "GET".into(),
            url: with_query(url, &[("transport", "websocket")]).to_string(),
            headers: headers.to_vec(),
            body: Default::default(),
        };
        let WsConnected { meta, timing, session, events } = match self.websocket(request, opts, jar).await {
            Err(e) if e.message.contains("Unsupported protocol version") => return Err(refused(400, &e.message)),
            other => other?,
        };
        let (tx, mut rx) = ws_frames(session, events);
        let handshake = tokio::time::timeout(wait, open_packet(&mut rx))
            .await
            .map_err(|_| EngineError::timeout("Waiting for the Socket.IO open packet", wait))??;
        Ok(Transport { tx, rx, handshake, label: "WebSocket", meta, timing })
    }

    async fn open_polling(
        self: &Arc<Self>,
        url: &Url,
        headers: &[Header],
        opts: &RequestOptions,
        jar: Option<Arc<CookieJar>>,
        wait: Duration,
    ) -> Result<Transport> {
        let polling = with_query(url, &[("transport", "polling")]);
        let mut first_opts = opts.clone();
        first_opts.timeout = Some(wait);
        let get = HttpRequest {
            method: "GET".into(),
            url: with_query(&polling, &[("t", &stamp())]).to_string(),
            headers: headers.to_vec(),
            body: Default::default(),
        };
        let response = self.send(get, &first_opts, jar.as_deref()).await?;
        let body = String::from_utf8_lossy(&response.body).into_owned();
        if !(200..300).contains(&response.meta.status) {
            return Err(refused(response.meta.status, &body));
        }
        let mut frames = packet::split_payload(&body).into_iter();
        let handshake = match frames.next() {
            Some(Frame::Text(t)) if t.starts_with(OPEN) => Handshake::parse(&t[1..]).map_err(protocol)?,
            _ => return Err(refused(response.meta.status, &body)),
        };
        let session = with_query(&polling, &[("sid", &handshake.sid)]);
        let (in_tx, rx) = mpsc::unbounded_channel();
        for frame in frames {
            let _ = in_tx.send(Incoming::Frame(frame));
        }
        let (tx, out_rx) = mpsc::unbounded_channel();
        // A poll is answered within the ping interval (the server pings); allow for slow ones.
        let mut poll_opts = opts.clone();
        poll_opts.timeout = Some(Duration::from_millis(handshake.ping_interval + handshake.ping_timeout) + wait);
        let poller = tokio::spawn(poll(
            self.clone(),
            session.clone(),
            headers.to_vec(),
            poll_opts.clone(),
            jar.clone(),
            in_tx.clone(),
        ));
        tokio::spawn(post(self.clone(), session, headers.to_vec(), poll_opts, jar, out_rx, in_tx, poller));
        Ok(Transport { tx, rx, handshake, label: "HTTP long-polling", meta: response.meta, timing: response.timing })
    }
}

fn protocol(message: String) -> EngineError {
    EngineError::new(ErrorKind::Protocol, format!("The server sent {message}"))
}

/// A packet and its binary attachments, if any.
fn send_packet(tx: &mpsc::UnboundedSender<Frame>, packet: &Packet) {
    for frame in packet.clone().frames() {
        let _ = tx.send(frame);
    }
}

/// Wait for the Engine.IO open packet.
async fn open_packet(rx: &mut mpsc::UnboundedReceiver<Incoming>) -> Result<Handshake> {
    match rx.recv().await {
        Some(Incoming::Frame(Frame::Text(t))) if t.starts_with(OPEN) => Handshake::parse(&t[1..]).map_err(protocol),
        Some(Incoming::Frame(Frame::Text(t))) => Err(EngineError::new(
            ErrorKind::Protocol,
            format!("This isn't a Socket.IO server (it sent {})", preview(&t)),
        )),
        Some(Incoming::Frame(Frame::Binary(_))) => {
            Err(EngineError::new(ErrorKind::Protocol, "This isn't a Socket.IO server (it sent binary data first)"))
        }
        Some(Incoming::Error(message)) => Err(EngineError::new(ErrorKind::Io, message)),
        Some(Incoming::Closed(reason)) => Err(EngineError::new(
            ErrorKind::Protocol,
            format!("The server closed the connection before opening a Socket.IO session ({reason})"),
        )),
        None => Err(EngineError::new(ErrorKind::Protocol, "The connection closed before a Socket.IO session opened")),
    }
}

/// Wait for the namespace's CONNECT answer; returns the socket id.
async fn join(transport: &mut Transport, namespace: &str) -> Result<Option<String>> {
    loop {
        match transport.rx.recv().await {
            Some(Incoming::Frame(Frame::Text(t))) => match t.chars().next() {
                Some(PING) => {
                    let _ = transport.tx.send(Frame::Text(format!("{PONG}{}", &t[1..])));
                }
                Some(MESSAGE) => {
                    let packet = Packet::decode(&t[1..]).map_err(protocol)?;
                    if packet.namespace != namespace {
                        continue;
                    }
                    match packet.kind {
                        Kind::Connect => {
                            return Ok(packet.data.as_ref().and_then(|d| d["sid"].as_str()).map(String::from));
                        }
                        Kind::ConnectError => {
                            let data = packet.data.unwrap_or(Value::Null);
                            let message = data["message"]
                                .as_str()
                                .or(data.as_str())
                                .map(String::from)
                                .unwrap_or_else(|| data.to_string());
                            return Err(EngineError::new(
                                ErrorKind::Protocol,
                                format!("The server refused to let this client join {namespace}: {message}"),
                            ));
                        }
                        _ => {}
                    }
                }
                Some(CLOSE) => {
                    return Err(EngineError::new(ErrorKind::Protocol, "The server closed the Socket.IO session"));
                }
                _ => {}
            },
            Some(Incoming::Frame(Frame::Binary(_))) => {}
            Some(Incoming::Error(message)) => return Err(EngineError::new(ErrorKind::Io, message)),
            Some(Incoming::Closed(reason)) => {
                let message = format!("The connection closed while joining {namespace} ({reason})");
                return Err(EngineError::new(ErrorKind::Protocol, message));
            }
            None => {
                let message = format!("The connection closed while joining {namespace}");
                return Err(EngineError::new(ErrorKind::Protocol, message));
            }
        }
    }
}

fn preview(text: &str) -> String {
    let start: String = text.chars().take(80).collect();
    if start.len() < text.len() { format!("\"{start}…\"") } else { format!("\"{start}\"") }
}

/// Engine.IO packets over a WebSocket: one per message.
fn ws_frames(
    session: crate::ws::WsSession,
    mut events: mpsc::UnboundedReceiver<WsEvent>,
) -> (mpsc::UnboundedSender<Frame>, mpsc::UnboundedReceiver<Incoming>) {
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Frame>();
    let (in_tx, in_rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                out = out_rx.recv() => match out {
                    Some(Frame::Text(text)) => {
                        let _ = session.send(WsOutgoing::Text { text });
                    }
                    Some(Frame::Binary(bytes)) => {
                        let base64 = base64::engine::general_purpose::STANDARD.encode(bytes);
                        let _ = session.send(WsOutgoing::Binary { base64 });
                    }
                    None => {
                        let _ = session.send(WsOutgoing::Close { code: Some(1000), reason: None });
                        // Let the close go out before the session is dropped.
                        let _ = tokio::time::timeout(Duration::from_secs(2), async {
                            while let Some(event) = events.recv().await {
                                if matches!(event, WsEvent::Closed { .. }) {
                                    break;
                                }
                            }
                        })
                        .await;
                        return;
                    }
                },
                event = events.recv() => {
                    let incoming = match event {
                        Some(WsEvent::Message { direction: Direction::Received, kind: WsMessageKind::Text, text, .. }) => {
                            Incoming::Frame(Frame::Text(text.unwrap_or_default()))
                        }
                        Some(WsEvent::Message { direction: Direction::Received, kind: WsMessageKind::Binary, base64, .. }) => {
                            let bytes = base64::engine::general_purpose::STANDARD.decode(base64.unwrap_or_default()).unwrap_or_default();
                            Incoming::Frame(Frame::Binary(bytes))
                        }
                        Some(WsEvent::Message { .. }) => continue,
                        Some(WsEvent::Error { message }) => Incoming::Error(message),
                        Some(WsEvent::Closed { code, reason, .. }) => {
                            let words = match (code, reason.is_empty()) {
                                (Some(code), false) => format!("code {code} · {reason}"),
                                (Some(code), true) => format!("code {code}"),
                                (None, false) => reason,
                                (None, true) => "connection closed".into(),
                            };
                            let _ = in_tx.send(Incoming::Closed(words));
                            return;
                        }
                        None => {
                            let _ = in_tx.send(Incoming::Closed("connection closed".into()));
                            return;
                        }
                    };
                    if in_tx.send(incoming).is_err() {
                        return;
                    }
                }
            }
        }
    });
    (out_tx, in_rx)
}

/// Long-polling: GET the next packets until the session ends.
async fn poll(
    client: Arc<Client>,
    session: Url,
    headers: Vec<Header>,
    opts: RequestOptions,
    jar: Option<Arc<CookieJar>>,
    events: mpsc::UnboundedSender<Incoming>,
) {
    loop {
        let get = HttpRequest {
            method: "GET".into(),
            url: with_query(&session, &[("t", &stamp())]).to_string(),
            headers: headers.clone(),
            body: Default::default(),
        };
        let response = match client.send(get, &opts, jar.as_deref()).await {
            Ok(r) => r,
            Err(e) => {
                let _ = events.send(Incoming::Closed(format!("connection lost: {}", e.message)));
                return;
            }
        };
        let body = String::from_utf8_lossy(&response.body).into_owned();
        if !(200..300).contains(&response.meta.status) {
            let message = refused(response.meta.status, &body).message;
            let _ = events.send(Incoming::Closed(message));
            return;
        }
        for frame in packet::split_payload(&body) {
            let close = matches!(&frame, Frame::Text(t) if t.starts_with(CLOSE));
            if events.send(Incoming::Frame(frame)).is_err() || close {
                return;
            }
        }
    }
}

/// Long-polling: POST what the session sends, several packets at once when they queue up.
/// Ends the session (a close packet) when the sending side goes away.
#[allow(clippy::too_many_arguments)]
async fn post(
    client: Arc<Client>,
    session: Url,
    headers: Vec<Header>,
    opts: RequestOptions,
    jar: Option<Arc<CookieJar>>,
    mut outgoing: mpsc::UnboundedReceiver<Frame>,
    events: mpsc::UnboundedSender<Incoming>,
    poller: tokio::task::JoinHandle<()>,
) {
    let mut headers = headers;
    headers.push(Header::new("Content-Type", "text/plain;charset=UTF-8"));
    loop {
        let (frames, closing) = match outgoing.recv().await {
            Some(first) => {
                let mut frames = vec![first];
                while let Ok(more) = outgoing.try_recv() {
                    frames.push(more);
                }
                (frames, false)
            }
            None => (vec![Frame::Text(CLOSE.to_string())], true),
        };
        let request = HttpRequest {
            method: "POST".into(),
            url: with_query(&session, &[("t", &stamp())]).to_string(),
            headers: headers.clone(),
            body: packet::join_payload(&frames).into_bytes().into(),
        };
        let result = client.send(request, &opts, jar.as_deref()).await;
        if closing {
            poller.abort();
            return;
        }
        match result {
            Ok(r) if (200..300).contains(&r.meta.status) => {}
            Ok(r) => {
                let body = String::from_utf8_lossy(&r.body).into_owned();
                let _ = events.send(Incoming::Error(format!("Not sent: {}", refused(r.meta.status, &body).message)));
            }
            Err(e) => {
                let _ = events.send(Incoming::Error(format!("Not sent: {}", e.message)));
            }
        }
    }
}

/// A received event, acknowledgement or binary one, as a message for the log.
fn received(packet: &Packet) -> Option<SocketEvent> {
    let (topic, args, detail) = match packet.kind {
        Kind::Event | Kind::BinaryEvent => {
            let (name, args) = packet.event_parts()?;
            let detail = packet.id.map(|id| format!("asks for an acknowledgement (#{id})"));
            (Some(name.to_string()), Value::Array(args.to_vec()), detail)
        }
        Kind::Ack | Kind::BinaryAck => {
            let id = packet.id.unwrap_or_default();
            (None, packet.data.clone().unwrap_or(Value::Null), Some(format!("acknowledgement of #{id}")))
        }
        _ => return None,
    };
    let text = args.to_string();
    let mut event = SocketEvent::message(Direction::Received, text.as_bytes());
    if let SocketEvent::Message { topic: t, detail: d, .. } = &mut event {
        *t = topic;
        *d = detail;
    }
    Some(event)
}

/// The session: heartbeat, received events, emits, and the goodbye.
async fn relay(
    namespace: String,
    tx: mpsc::UnboundedSender<Frame>,
    mut rx: mpsc::UnboundedReceiver<Incoming>,
    mut outgoing: mpsc::UnboundedReceiver<SocketOutgoing>,
    events: mpsc::UnboundedSender<SocketEvent>,
    alive: Duration,
) {
    let closed = |reason: String, by_client: bool| SocketEvent::Closed { reason, by_client };
    let mut last_id = 0u64;
    // A binary event and the attachments received for it so far.
    let mut assembling: Option<(Packet, Vec<Vec<u8>>)> = None;
    let deadline = tokio::time::sleep(alive);
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            _ = &mut deadline => {
                let _ = events.send(closed(format!("The server stopped answering (no ping for {} s)", alive.as_secs()), false));
                return;
            }
            out = outgoing.recv() => match out {
                None => {
                    send_packet(&tx, &Packet::new(Kind::Disconnect, &namespace, None));
                    let _ = events.send(closed("Disconnected".into(), true));
                    return;
                }
                Some(SocketOutgoing::Emit { event, args, base64, ack }) => {
                    let id = ack.then(|| {
                        last_id += 1;
                        last_id
                    });
                    // A binary argument is `{"base64": …}`, like received ones are shown.
                    let list = match base64 {
                        Some(data) => {
                            let bytes = base64::engine::general_purpose::STANDARD.decode(&data).map(|b| b.len());
                            vec![serde_json::json!({ "base64": data, "bytes": bytes.unwrap_or_default() })]
                        }
                        None => match packet::parse_args(&args) {
                            Ok(list) => list,
                            Err(message) => {
                                let _ = events.send(SocketEvent::Error { message });
                                continue;
                            }
                        },
                    };
                    let text = Value::Array(list.clone()).to_string();
                    send_packet(&tx, &Packet::event(&namespace, &event, list, id));
                    let mut sent = SocketEvent::message(Direction::Sent, text.as_bytes());
                    if let SocketEvent::Message { topic, detail, .. } = &mut sent {
                        *topic = Some(event);
                        *detail = id.map(|id| format!("asks for an acknowledgement (#{id})"));
                    }
                    let _ = events.send(sent);
                }
                Some(_) => {
                    let _ = events.send(SocketEvent::Error { message: "Socket.IO sends events: enter an event name".into() });
                }
            },
            incoming = rx.recv() => match incoming {
                Some(Incoming::Frame(Frame::Text(text))) => {
                    deadline.as_mut().reset(tokio::time::Instant::now() + alive);
                    match text.chars().next() {
                        Some(PING) => {
                            let _ = tx.send(Frame::Text(format!("{PONG}{}", &text[1..])));
                        }
                        Some(MESSAGE) => {
                            let packet = match Packet::decode(&text[1..]) {
                                Ok(p) => p,
                                Err(e) => {
                                    let _ = events.send(SocketEvent::Error { message: format!("The server sent {e}") });
                                    continue;
                                }
                            };
                            if packet.namespace != namespace {
                                continue;
                            }
                            match packet.kind {
                                Kind::Disconnect => {
                                    let _ = events.send(closed("The server disconnected this client".into(), false));
                                    return;
                                }
                                Kind::ConnectError => {
                                    let message = packet.data.map(|d| d.to_string()).unwrap_or_default();
                                    let _ = events.send(SocketEvent::Error { message: format!("Connection error: {message}") });
                                }
                                Kind::BinaryEvent | Kind::BinaryAck if packet.attachments > 0 => {
                                    assembling = Some((packet, Vec::new()));
                                }
                                _ => {
                                    if let Some(event) = received(&packet) {
                                        let _ = events.send(event);
                                    }
                                }
                            }
                        }
                        Some(CLOSE) => {
                            let _ = events.send(closed("The server closed the session".into(), false));
                            return;
                        }
                        // Noops and pongs need nothing.
                        _ => {}
                    }
                }
                Some(Incoming::Frame(Frame::Binary(bytes))) => {
                    deadline.as_mut().reset(tokio::time::Instant::now() + alive);
                    if let Some((packet, attachments)) = assembling.as_mut() {
                        attachments.push(bytes);
                        if attachments.iter().map(Vec::len).sum::<usize>() > packet::MAX_ATTACHMENT_BYTES {
                            assembling = None;
                            let _ = events.send(SocketEvent::Error {
                                message: format!("The server sent more than {} MB of attachments for one event; it was dropped", packet::MAX_ATTACHMENT_BYTES >> 20),
                            });
                        } else if attachments.len() >= packet.attachments {
                            let (mut packet, attachments) = assembling.take().expect("assembling");
                            if let Some(data) = packet.data.as_mut() {
                                packet::fill_placeholders(data, &attachments);
                            }
                            if let Some(event) = received(&packet) {
                                let _ = events.send(event);
                            }
                        }
                    }
                }
                Some(Incoming::Error(message)) => {
                    let _ = events.send(SocketEvent::Error { message });
                }
                Some(Incoming::Closed(reason)) => {
                    let _ = events.send(closed(format!("Connection closed ({reason})"), false));
                    return;
                }
                None => {
                    let _ = events.send(closed("Connection closed".into(), false));
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
    fn urls_name_the_endpoint_and_the_namespace() {
        let e = endpoint("http://localhost:3000/chat?token=a", "/socket.io/").unwrap();
        assert_eq!(e.url.as_str(), "http://localhost:3000/socket.io/?token=a&EIO=4");
        assert_eq!(e.namespace, "/chat");
        let e = endpoint("wss://example.com", "socket.io").unwrap();
        assert_eq!((e.url.as_str(), e.namespace.as_str()), ("https://example.com/socket.io/?EIO=4", "/"));
        let e = endpoint("localhost:3000/admin/", "/io").unwrap();
        assert_eq!((e.url.as_str(), e.namespace.as_str()), ("http://localhost:3000/io/?EIO=4", "/admin"));
        assert!(endpoint("ftp://h", "/socket.io/").is_err());
        assert!(endpoint(" ", "/socket.io/").is_err());
    }

    #[test]
    fn refusals_in_words() {
        let e = refused(400, r#"{"code":5,"message":"Unsupported protocol version"}"#);
        assert!(e.message.contains("Socket.IO 2"), "{}", e.message);
        let e = refused(400, r#"{"code":0,"message":"Transport unknown"}"#);
        assert_eq!(
            e.message,
            r#"The server refused the Socket.IO session (400): {"code":0,"message":"Transport unknown"}"#
        );
    }

    #[test]
    fn received_events_and_acknowledgements() {
        let packet = Packet::decode(r#"27["news",{"a":1},2]"#).unwrap();
        match received(&packet).unwrap() {
            SocketEvent::Message { text, topic, detail, .. } => {
                assert_eq!(text.as_deref(), Some(r#"[{"a":1},2]"#));
                assert_eq!(topic.as_deref(), Some("news"));
                assert_eq!(detail.as_deref(), Some("asks for an acknowledgement (#7)"));
            }
            other => panic!("{other:?}"),
        }
        match received(&Packet::decode(r#"33["ok"]"#).unwrap()).unwrap() {
            SocketEvent::Message { text, topic, detail, .. } => {
                assert_eq!(
                    (text.as_deref(), topic, detail.as_deref()),
                    (Some(r#"["ok"]"#), None, Some("acknowledgement of #3"))
                );
            }
            other => panic!("{other:?}"),
        }
        assert!(received(&Packet::decode("0").unwrap()).is_none());
    }
}

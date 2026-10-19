//! Socket.IO server (Socket.IO 3 and 4, Engine.IO 4): HTTP long-polling and WebSocket,
//! including the upgrade from polling that socket.io-client makes by default. Clients may
//! join any namespace. Events are answered by echo or by rules (acknowledgements, replies,
//! broadcasts to the namespace); scheduled events go to a namespace on a timer; the UI can
//! emit to one client or all of them.
//!
//! One Engine.IO session is one connection in the log. Each session has a task that owns
//! its Socket.IO state; the HTTP handlers and the WebSocket only move packets in and out.

use std::collections::{BTreeSet, HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use http::header::{self, HeaderValue};
use http::{Method, Request, Response, StatusCode};
use http_body_util::{BodyExt, Limited};
use hyper::body::Incoming;
use hyper_util::rt::TokioIo;
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio::sync::{Notify, mpsc, watch};
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::protocol::{Role, WebSocketConfig};
use tokio_tungstenite::tungstenite::{self, Message};
use tokio_util::sync::CancellationToken;
use zorvik_engine::socketio::packet::{
    self, CLOSE, Frame, Handshake, Kind, MESSAGE, NOOP, OPEN, PING, PONG, Packet, UPGRADE,
};
use zorvik_formats::matching::MessageMatcher;
use zorvik_formats::server::MIN_SCHEDULE_MS;
use zorvik_formats::{MatchKind, ReplyMode, SocketIoRule};
use zorvik_workspace::vars::VarContext;

use crate::http::{Abort, Body, Handler, HandlerFuture, Peer, add_cors, preflight, serve};
use crate::report::{OutgoingMessage, Reporter, TrafficDirection};
use crate::{Control, Ctx, Live, PerConfig};

const PING_INTERVAL: Duration = Duration::from_secs(25);
const PING_TIMEOUT: Duration = Duration::from_secs(20);
/// Largest long-polling POST and WebSocket message.
const MAX_PAYLOAD: usize = 1_000_000;
/// Packets waiting for a client that doesn't read; more are refused.
const MAX_QUEUED: usize = 256;
/// Longest rule delay.
const MAX_DELAY: Duration = Duration::from_secs(3600);

pub(crate) async fn run(listener: TcpListener, ctx: Ctx) -> Result<(), String> {
    let server = Arc::new(Server {
        live: ctx.live.clone(),
        patterns: PerConfig::new(ctx.live.clone(), |live, _| {
            live.server.socketio.rules.iter().map(Pattern::new).collect()
        }),
        reporter: ctx.reporter.clone(),
        stopped: ctx.cancel.clone(),
        sessions: Mutex::new(HashMap::new()),
    });
    crate::report_problems(&ctx, |live| problems(&live.server.socketio.rules));
    tokio::spawn(schedule(server.clone(), ctx.cancel.clone()));
    let handler: Handler = {
        let server = server.clone();
        Arc::new(move |req: Request<Incoming>, peer: Peer| -> HandlerFuture {
            let server = server.clone();
            Box::pin(async move { Ok::<_, Abort>(server.handle(req, peer).await) })
        })
    };
    serve(listener, ctx, handler, false, move |control| match control {
        Control::Send { conn, message, reply } => {
            let _ = reply.send(server.send_from_ui(conn, message));
        }
        Control::Disconnect { conn } => {
            if let Some(session) = server.session_by_conn(conn) {
                session.disconnect();
            }
        }
    })
    .await
}

/// Scheduled events: each goes to every client of its namespace every `everyMs`. A change of
/// the scheduled events starts the timers again; other edits keep them running.
async fn schedule(server: Arc<Server>, stopped: CancellationToken) {
    use tokio::time::{Instant, sleep_until};
    let mut live = server.live.clone();
    let mut current = live.borrow_and_update().clone();
    loop {
        let start = Instant::now();
        // When each enabled event is next due, and how often; arguments that aren't JSON are
        // reported once, not with every tick.
        let mut due: Vec<Option<(Instant, Duration)>> = current
            .server
            .socketio
            .schedule
            .iter()
            .map(|m| {
                let every = Duration::from_millis(m.every_ms.max(MIN_SCHEDULE_MS));
                (m.enabled && !m.event.trim().is_empty()).then_some((start + every, every))
            })
            .collect();
        loop {
            let next = due.iter().flatten().map(|(at, _)| *at).min();
            tokio::select! {
                _ = sleep_until(next.unwrap_or(start)), if next.is_some() => {
                    let now = Instant::now();
                    for (i, slot) in due.iter_mut().enumerate() {
                        let Some((at, every)) = slot else { continue };
                        if *at > now {
                            continue;
                        }
                        // A late tick is not caught up with a burst.
                        let next = *at + *every;
                        *at = if next <= now { now + *every } else { next };
                        let m = &current.server.socketio.schedule[i];
                        let ns = format!("/{}", m.namespace.trim().trim_start_matches('/'));
                        match render_args(&m.args, None, &current.vars) {
                            Ok(args) => {
                                for session in server.members(&ns) {
                                    server.emit(&session, &ns, m.event.trim(), args.clone());
                                }
                            }
                            Err(e) => {
                                server.reporter.error(None, None, format!("Scheduled event {}: {e}", i + 1));
                                *slot = None;
                            }
                        }
                    }
                }
                changed = live.changed() => {
                    if changed.is_err() {
                        return;
                    }
                    let next = live.borrow_and_update().clone();
                    let restart = next.server.socketio.schedule != current.server.socketio.schedule;
                    current = next;
                    if restart {
                        break;
                    }
                }
                _ = stopped.cancelled() => return,
            }
        }
    }
}

/// Rules that can never match (bad regex or JSON condition).
fn problems(rules: &[SocketIoRule]) -> Vec<String> {
    rules
        .iter()
        .enumerate()
        .filter(|(_, r)| r.enabled)
        .filter_map(|(i, r)| match Pattern::new(r) {
            Pattern::Invalid(e) => Some(format!("Rule {}: {e}", i + 1)),
            _ => None,
        })
        .collect()
}

/// How a rule's pattern is checked, compiled once per configuration.
enum Pattern {
    /// Any, contains or exact: compared as text.
    Text,
    /// A regex or JSON condition.
    Compiled(MessageMatcher),
    /// A regex or JSON condition that doesn't parse (why): the rule never matches.
    Invalid(String),
}

impl Pattern {
    fn new(rule: &SocketIoRule) -> Self {
        match rule.matcher {
            MatchKind::Regex | MatchKind::Json => match MessageMatcher::new(rule.matcher, rule.pattern.as_bytes()) {
                Ok(m) => Self::Compiled(m),
                Err(e) => Self::Invalid(e),
            },
            MatchKind::Any | MatchKind::Contains | MatchKind::Exact => Self::Text,
        }
    }
}

struct Server {
    live: watch::Receiver<Arc<Live>>,
    /// The rules' patterns, compiled (one per rule of the configuration they come with).
    patterns: PerConfig<Vec<Pattern>>,
    reporter: Reporter,
    stopped: CancellationToken,
    sessions: Mutex<HashMap<String, Arc<Session>>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// An Engine.IO session: one client connection.
struct Session {
    sid: String,
    conn: u64,
    peer: SocketAddr,
    /// Packets from the client (POST bodies, WebSocket messages) for the session's task.
    inbox: mpsc::UnboundedSender<Frame>,
    /// Packets for the client while it polls.
    outbox: Mutex<VecDeque<Frame>>,
    ready: Notify,
    /// Packets for the client once it is on WebSocket.
    ws: Mutex<Option<mpsc::Sender<Frame>>>,
    /// A long-polling GET is waiting.
    polling: AtomicBool,
    /// Joined namespaces.
    namespaces: Mutex<BTreeSet<String>>,
    /// Why the session ended (set once).
    ended: Mutex<Option<String>>,
    closed: CancellationToken,
}

impl Session {
    /// Queue a packet for the client.
    fn push(&self, frame: Frame) -> Result<(), String> {
        if let Some(ws) = lock(&self.ws).as_ref() {
            return ws.try_send(frame).map_err(|_| self.not_reading());
        }
        let mut outbox = lock(&self.outbox);
        if outbox.len() >= MAX_QUEUED {
            return Err(self.not_reading());
        }
        outbox.push_back(frame);
        drop(outbox);
        self.ready.notify_one();
        Ok(())
    }

    fn not_reading(&self) -> String {
        format!("Not sent: client #{} is not reading ({MAX_QUEUED} packets are still waiting)", self.conn)
    }

    /// A packet and its binary attachments, if any.
    fn push_packet(&self, packet: &Packet) -> Result<(), String> {
        for frame in packet.clone().frames() {
            self.push(frame)?;
        }
        Ok(())
    }

    fn close(&self, reason: &str) {
        lock(&self.ended).get_or_insert_with(|| reason.to_string());
        self.closed.cancel();
    }

    /// Disconnect the client from its namespaces and end the session (from the UI).
    fn disconnect(&self) {
        let namespaces: Vec<String> = lock(&self.namespaces).iter().cloned().collect();
        for ns in namespaces {
            let _ = self.push_packet(&Packet::new(Kind::Disconnect, &ns, None));
        }
        let _ = self.push(Frame::Text(CLOSE.to_string()));
        self.close("closed by you");
    }
}

fn error(status: StatusCode, code: u8, message: &str) -> Response<Body> {
    let body = json!({ "code": code, "message": message }).to_string();
    let mut response = Response::new(Body::full(body));
    *response.status_mut() = status;
    response.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json"));
    response
}

fn text(body: String) -> Response<Body> {
    let mut response = Response::new(Body::full(body));
    response.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("text/plain; charset=UTF-8"));
    response
}

impl Server {
    fn config(&self) -> Arc<Live> {
        self.live.borrow().clone()
    }

    fn session(&self, sid: &str) -> Option<Arc<Session>> {
        lock(&self.sessions).get(sid).cloned()
    }

    fn session_by_conn(&self, conn: u64) -> Option<Arc<Session>> {
        lock(&self.sessions).values().find(|s| s.conn == conn && !s.closed.is_cancelled()).cloned()
    }

    async fn handle(self: Arc<Self>, req: Request<Incoming>, peer: Peer) -> Response<Body> {
        let live = self.config();
        let config = &live.server.socketio;
        let cors = config.cors;
        let origin = req.headers().get(header::ORIGIN).cloned();
        if req.method() == Method::OPTIONS && !cors {
            // A browser's preflight: without CORS, other origins get no permission.
            let mut response = Response::new(Body::empty());
            *response.status_mut() = StatusCode::NO_CONTENT;
            return response;
        }
        let mut response = self.route(req, peer, &config.path).await;
        if cors && response.status() != StatusCode::SWITCHING_PROTOCOLS {
            add_cors(response.headers_mut(), origin.as_ref());
        }
        response
    }

    async fn route(self: &Arc<Self>, req: Request<Incoming>, peer: Peer, path: &str) -> Response<Body> {
        let wanted = format!("/{}", path.trim().trim_matches('/'));
        if req.uri().path().trim_end_matches('/') != wanted.trim_end_matches('/') {
            let mut response = text(format!("This is a Socket.IO server: clients connect to {wanted}/"));
            *response.status_mut() = StatusCode::NOT_FOUND;
            return response;
        }
        if req.method() == Method::OPTIONS {
            let mut response = Response::new(Body::empty());
            *response.status_mut() = StatusCode::NO_CONTENT;
            *response.headers_mut() = preflight(req.headers());
            return response;
        }
        let query = crate::http::parse_query(req.uri().query().unwrap_or_default());
        let get = |name: &str| query.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str());
        if get("EIO") != Some("4") {
            return error(StatusCode::BAD_REQUEST, 5, "Unsupported protocol version");
        }
        let sid = get("sid").map(String::from);
        match (get("transport"), sid, req.method().clone()) {
            (Some("websocket"), sid, Method::GET) => self.upgrade(req, peer, sid),
            (Some("polling"), None, Method::GET) => self.open_polling(peer),
            (Some("polling"), Some(sid), method) => {
                let Some(session) = self.session(&sid) else {
                    return error(StatusCode::BAD_REQUEST, 1, "Session ID unknown");
                };
                match method {
                    Method::GET => self.poll(&session).await,
                    Method::POST => receive(&session, req).await,
                    _ => error(StatusCode::BAD_REQUEST, 2, "Bad handshake method"),
                }
            }
            (Some("polling" | "websocket"), _, _) => error(StatusCode::BAD_REQUEST, 2, "Bad handshake method"),
            _ => error(StatusCode::BAD_REQUEST, 0, "Transport unknown"),
        }
    }

    /// A new session: register it and start its task.
    fn open(self: &Arc<Self>, peer: SocketAddr, ws: Option<mpsc::Sender<Frame>>) -> (Arc<Session>, Handshake) {
        let sid = random_id();
        let (inbox, inbox_rx) = mpsc::unbounded_channel();
        let session = Arc::new(Session {
            sid: sid.clone(),
            conn: self.reporter.next_conn(),
            peer,
            inbox,
            outbox: Mutex::new(VecDeque::new()),
            ready: Notify::new(),
            ws: Mutex::new(ws.clone()),
            polling: AtomicBool::new(false),
            namespaces: Mutex::new(BTreeSet::new()),
            ended: Mutex::new(None),
            closed: self.stopped.child_token(),
        });
        let handshake = Handshake {
            sid,
            upgrades: if ws.is_some() { Vec::new() } else { vec!["websocket".into()] },
            ping_interval: PING_INTERVAL.as_millis() as u64,
            ping_timeout: PING_TIMEOUT.as_millis() as u64,
            max_payload: MAX_PAYLOAD as u64,
        };
        lock(&self.sessions).insert(session.sid.clone(), session.clone());
        self.reporter.opened(session.conn, &peer);
        tokio::spawn(self.clone().run_session(session.clone(), inbox_rx));
        (session, handshake)
    }

    fn open_polling(self: &Arc<Self>, peer: Peer) -> Response<Body> {
        let (_, handshake) = self.open(peer.addr, None);
        text(format!("{OPEN}{}", handshake.to_json()))
    }

    /// A long-polling GET: answered with the waiting packets (the task pings often enough).
    async fn poll(&self, session: &Arc<Session>) -> Response<Body> {
        if session.polling.swap(true, Ordering::SeqCst) {
            session.close("it polled twice at once");
            return error(StatusCode::BAD_REQUEST, 3, "Bad request");
        }
        let frames = loop {
            let frames: Vec<Frame> = lock(&session.outbox).drain(..).collect();
            if !frames.is_empty() {
                break frames;
            }
            if session.closed.is_cancelled() {
                break vec![Frame::Text(CLOSE.to_string())];
            }
            tokio::select! {
                _ = session.ready.notified() => {}
                _ = session.closed.cancelled() => {}
            }
        };
        session.polling.store(false, Ordering::SeqCst);
        text(packet::join_payload(&frames))
    }

    /// A WebSocket: a new session, or the upgrade of a polling one (`sid`).
    fn upgrade(self: &Arc<Self>, mut req: Request<Incoming>, peer: Peer, sid: Option<String>) -> Response<Body> {
        let key = req.headers().get("sec-websocket-key").map(|k| k.as_bytes().to_vec());
        let is_upgrade = req
            .headers()
            .get(header::UPGRADE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.eq_ignore_ascii_case("websocket"));
        let Some(key) = key.filter(|_| is_upgrade) else {
            return error(StatusCode::BAD_REQUEST, 3, "Bad request");
        };
        let existing = match &sid {
            Some(sid) => match self.session(sid) {
                Some(session) => Some(session),
                None => return error(StatusCode::BAD_REQUEST, 1, "Session ID unknown"),
            },
            None => None,
        };
        let on_upgrade = hyper::upgrade::on(&mut req);
        let server = self.clone();
        tokio::spawn(async move {
            let Ok(upgraded) = on_upgrade.await else { return };
            let config =
                WebSocketConfig::default().max_message_size(Some(MAX_PAYLOAD)).max_frame_size(Some(MAX_PAYLOAD));
            let ws = WebSocketStream::from_raw_socket(TokioIo::new(upgraded), Role::Server, Some(config)).await;
            server.serve_ws(ws, peer.addr, existing).await;
        });
        let mut response = Response::new(Body::empty());
        *response.status_mut() = StatusCode::SWITCHING_PROTOCOLS;
        let headers = response.headers_mut();
        headers.insert(header::UPGRADE, HeaderValue::from_static("websocket"));
        headers.insert(header::CONNECTION, HeaderValue::from_static("Upgrade"));
        let accept = tungstenite::handshake::derive_accept_key(&key);
        if let Ok(v) = HeaderValue::from_str(&accept) {
            headers.insert(header::SEC_WEBSOCKET_ACCEPT, v);
        }
        response
    }

    async fn serve_ws<S>(self: Arc<Self>, ws: WebSocketStream<S>, peer: SocketAddr, existing: Option<Arc<Session>>)
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    {
        let (mut sink, mut source) = ws.split();
        let (tx, mut rx) = mpsc::channel::<Frame>(MAX_QUEUED);
        let session = match existing {
            Some(session) => {
                // The upgrade: "2probe" → "3probe", then "5" moves the session here.
                let probed = matches!(source.next().await, Some(Ok(Message::Text(t))) if t.as_str() == "2probe");
                if !probed || sink.send(Message::text("3probe")).await.is_err() {
                    return;
                }
                // Ends the waiting poll so the client can finish upgrading.
                let _ = session.push(Frame::Text(NOOP.to_string()));
                let upgraded =
                    matches!(source.next().await, Some(Ok(Message::Text(t))) if t.as_str() == UPGRADE.to_string());
                if !upgraded {
                    return;
                }
                let waiting: Vec<Frame> = {
                    let mut ws = lock(&session.ws);
                    *ws = Some(tx.clone());
                    lock(&session.outbox).drain(..).collect()
                };
                session.ready.notify_one();
                for frame in waiting.into_iter().filter(|f| !matches!(f, Frame::Text(t) if t == &NOOP.to_string())) {
                    let _ = tx.try_send(frame);
                }
                session
            }
            None => {
                let (session, handshake) = self.open(peer, Some(tx.clone()));
                if sink.send(Message::text(format!("{OPEN}{}", handshake.to_json()))).await.is_err() {
                    session.close("connection closed");
                    return;
                }
                session
            }
        };
        let closed = session.closed.clone();
        let reason = loop {
            tokio::select! {
                out = rx.recv() => match out {
                    Some(Frame::Text(t)) => if sink.send(Message::text(t)).await.is_err() { break "connection closed" },
                    Some(Frame::Binary(b)) => if sink.send(Message::binary(Bytes::from(b))).await.is_err() { break "connection closed" },
                    None => break "connection closed",
                },
                incoming = source.next() => match incoming {
                    Some(Ok(Message::Text(t))) => { let _ = session.inbox.send(Frame::Text(t.to_string())); }
                    Some(Ok(Message::Binary(b))) => { let _ = session.inbox.send(Frame::Binary(b.to_vec())); }
                    Some(Ok(Message::Close(_))) | None => break "closed by the client",
                    Some(Ok(_)) => {}
                    Some(Err(_)) => break "connection lost",
                },
                _ = closed.cancelled() => {
                    // Whatever the task queued last (a disconnect, a close) goes out first.
                    while let Ok(frame) = rx.try_recv() {
                        let message = match frame {
                            Frame::Text(t) => Message::text(t),
                            Frame::Binary(b) => Message::binary(Bytes::from(b)),
                        };
                        let _ = sink.send(message).await;
                    }
                    let _ = tokio::time::timeout(Duration::from_secs(2), sink.send(Message::Close(None))).await;
                    return;
                }
            }
        };
        session.close(reason);
    }

    /// Everything the session does, until it ends.
    async fn run_session(self: Arc<Self>, session: Arc<Session>, mut inbox: mpsc::UnboundedReceiver<Frame>) {
        let mut ping = tokio::time::interval_at(tokio::time::Instant::now() + PING_INTERVAL, PING_INTERVAL);
        let pong_deadline = tokio::time::sleep(Duration::ZERO);
        tokio::pin!(pong_deadline);
        let mut awaiting_pong = false;
        // A binary event and the attachments received for it so far.
        let mut assembling: Option<(Packet, Vec<Vec<u8>>)> = None;
        loop {
            tokio::select! {
                frame = inbox.recv() => match frame {
                    Some(Frame::Text(t)) => match t.chars().next() {
                        Some(PONG) => awaiting_pong = false,
                        Some(PING) => { let _ = session.push(Frame::Text(format!("{PONG}{}", &t[1..]))); }
                        Some(CLOSE) => session.close("closed by the client"),
                        Some(MESSAGE) => match Packet::decode(&t[1..]) {
                            Ok(p) if p.kind.is_binary() && p.attachments > 0 => assembling = Some((p, Vec::new())),
                            Ok(p) => self.packet(&session, p),
                            Err(e) => self.reporter.error(Some(session.conn), Some(&session.peer), format!("The client sent {e}")),
                        },
                        _ => {}
                    },
                    Some(Frame::Binary(bytes)) => {
                        if let Some((p, attachments)) = assembling.as_mut() {
                            attachments.push(bytes);
                            if attachments.iter().map(Vec::len).sum::<usize>() > packet::MAX_ATTACHMENT_BYTES {
                                assembling = None;
                                session.close("the client sent too many attachment bytes for one event");
                            } else if attachments.len() >= p.attachments {
                                let (mut p, attachments) = assembling.take().expect("assembling");
                                if let Some(data) = p.data.as_mut() {
                                    packet::fill_placeholders(data, &attachments);
                                }
                                self.packet(&session, p);
                            }
                        }
                    }
                    None => session.close("connection closed"),
                },
                _ = ping.tick() => {
                    let _ = session.push(Frame::Text(PING.to_string()));
                    awaiting_pong = true;
                    pong_deadline.as_mut().reset(tokio::time::Instant::now() + PING_TIMEOUT);
                }
                _ = &mut pong_deadline, if awaiting_pong => session.close("no answer to the server's ping"),
                _ = session.closed.cancelled() => break,
            }
        }
        let reason = lock(&session.ended).clone().unwrap_or_else(|| "server stopped".into());
        session.ready.notify_one();
        self.reporter.closed(session.conn, &session.peer, &reason);
        // A polling client gets the last packets (a disconnect) with its next poll.
        if lock(&session.ws).is_none() && !self.stopped.is_cancelled() {
            let _ = tokio::time::timeout(Duration::from_secs(5), async {
                while !lock(&session.outbox).is_empty() {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            })
            .await;
        }
        lock(&self.sessions).remove(&session.sid);
    }

    /// A Socket.IO packet from the client.
    fn packet(self: &Arc<Self>, session: &Arc<Session>, p: Packet) {
        let (conn, peer) = (Some(session.conn), Some(&session.peer));
        let ns = p.namespace.clone();
        match p.kind {
            Kind::Connect => {
                lock(&session.namespaces).insert(ns.clone());
                let auth = p.data.map(|d| d.to_string()).unwrap_or_default();
                self.reporter.data(conn, peer, TrafficDirection::In, auth.as_bytes(), format!("joined {ns}"));
                let socket_id = random_id();
                let _ = session.push_packet(&Packet::new(Kind::Connect, &ns, Some(json!({ "sid": socket_id }))));
                let live = self.config();
                let config = &live.server.socketio;
                if !config.greeting_event.trim().is_empty() {
                    match render_args(&config.greeting_args, None, &live.vars) {
                        Ok(args) => self.emit(session, &ns, config.greeting_event.trim(), args),
                        Err(e) => self.reporter.error(conn, peer, format!("Greeting: {e}")),
                    }
                }
            }
            Kind::Disconnect => {
                lock(&session.namespaces).remove(&ns);
                self.reporter.data(conn, peer, TrafficDirection::In, b"", format!("left {ns}"));
            }
            Kind::Ack | Kind::BinaryAck => {
                let data = p.data.map(|d| d.to_string()).unwrap_or_default();
                let summary = format!("{}acknowledgement #{}", prefix(&ns), p.id.unwrap_or_default());
                self.reporter.data(conn, peer, TrafficDirection::In, data.as_bytes(), summary);
            }
            Kind::Event | Kind::BinaryEvent => {
                let Some((name, args)) = p.event_parts() else { return };
                let (name, args) = (name.to_string(), args.to_vec());
                let shown = Value::Array(args.clone()).to_string();
                let asks = p.id.map(|id| format!(" (asks for acknowledgement #{id})")).unwrap_or_default();
                let summary = format!("{}{name}{asks}", prefix(&ns));
                self.reporter.data(conn, peer, TrafficDirection::In, shown.as_bytes(), summary);
                self.answer(session, &ns, &name, args, p.id);
            }
            Kind::ConnectError => {}
        }
    }

    /// Echo or the first matching rule.
    fn answer(self: &Arc<Self>, session: &Arc<Session>, ns: &str, name: &str, args: Vec<Value>, id: Option<u64>) {
        let (live, patterns) = self.patterns.get();
        let config = &live.server.socketio;
        match config.mode {
            ReplyMode::Echo => {
                if let Some(id) = id {
                    self.ack(session, ns, id, args.clone());
                }
                self.emit(session, ns, name, args);
            }
            ReplyMode::Rules => {
                let arguments = Arguments::new(&args);
                let Some(rule) = config
                    .rules
                    .iter()
                    .zip(patterns.iter())
                    .find(|(rule, pattern)| rule_matches(rule, pattern, name, &arguments))
                    .map(|(rule, _)| rule.clone())
                else {
                    return;
                };
                let event = (name.to_string(), args);
                let server = self.clone();
                let session = session.clone();
                let ns = ns.to_string();
                let vars = live.vars.clone();
                tokio::spawn(async move {
                    if rule.delay_ms > 0 {
                        tokio::select! {
                            _ = tokio::time::sleep(Duration::from_millis(rule.delay_ms).min(MAX_DELAY)) => {}
                            _ = session.closed.cancelled() => return,
                        }
                    }
                    server.apply(&session, &ns, &rule, &event, id, &vars);
                });
            }
            ReplyMode::Manual | ReplyMode::Discard => {}
        }
    }

    fn apply(
        &self,
        session: &Arc<Session>,
        ns: &str,
        rule: &SocketIoRule,
        event: &(String, Vec<Value>),
        id: Option<u64>,
        vars: &VarContext,
    ) {
        let (conn, peer) = (Some(session.conn), Some(&session.peer));
        if let Some(id) = id {
            match render_args(&rule.ack, Some(event), vars) {
                Ok(args) => self.ack(session, ns, id, args),
                Err(e) => self.reporter.error(conn, peer, format!("Acknowledgement: {e}")),
            }
        }
        let reply = rule.reply_event.trim();
        if reply.is_empty() {
            return;
        }
        let args = match render_args(&rule.reply_args, Some(event), vars) {
            Ok(args) => args,
            Err(e) => return self.reporter.error(conn, peer, format!("Reply: {e}")),
        };
        if rule.broadcast {
            for other in self.members(ns) {
                self.emit(&other, ns, reply, args.clone());
            }
        } else {
            self.emit(session, ns, reply, args);
        }
    }

    /// Sessions that joined `ns`.
    fn members(&self, ns: &str) -> Vec<Arc<Session>> {
        let sessions = lock(&self.sessions);
        sessions.values().filter(|s| !s.closed.is_cancelled() && lock(&s.namespaces).contains(ns)).cloned().collect()
    }

    fn emit(&self, session: &Session, ns: &str, name: &str, args: Vec<Value>) {
        if let Err(e) = self.try_emit(session, ns, name, args) {
            self.reporter.error(Some(session.conn), Some(&session.peer), e);
        }
    }

    fn try_emit(&self, session: &Session, ns: &str, name: &str, args: Vec<Value>) -> Result<(), String> {
        let shown = Value::Array(args.clone()).to_string();
        session.push_packet(&Packet::event(ns, name, args, None))?;
        let summary = format!("{}{name}", prefix(ns));
        self.reporter.data(Some(session.conn), Some(&session.peer), TrafficDirection::Out, shown.as_bytes(), summary);
        Ok(())
    }

    fn ack(&self, session: &Session, ns: &str, id: u64, args: Vec<Value>) {
        let shown = Value::Array(args.clone()).to_string();
        let packet = Packet { id: Some(id), ..Packet::new(Kind::Ack, ns, Some(Value::Array(args))) };
        match session.push_packet(&packet) {
            Ok(()) => {
                let summary = format!("{}acknowledgement #{id}", prefix(ns));
                self.reporter.data(
                    Some(session.conn),
                    Some(&session.peer),
                    TrafficDirection::Out,
                    shown.as_bytes(),
                    summary,
                );
            }
            Err(e) => self.reporter.error(Some(session.conn), Some(&session.peer), e),
        }
    }

    /// An emit from the UI (or an agent) to one client or every client of the namespace.
    fn send_from_ui(&self, conn: Option<u64>, message: OutgoingMessage) -> Result<usize, String> {
        use base64::Engine as _;
        let (event, args, ns) = match message {
            OutgoingMessage::Emit { event, args, namespace } => {
                let live = self.config();
                let args = render_args(&args, None, &live.vars)?;
                (event.trim().to_string(), args, namespace)
            }
            OutgoingMessage::Text { text } => ("message".into(), vec![Value::String(text)], String::new()),
            OutgoingMessage::Binary { base64 } => {
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(&base64)
                    .map_err(|e| format!("Binary payload is not valid base64: {e}"))?;
                let data = json!({ "base64": base64, "bytes": bytes.len() });
                ("message".into(), vec![data], String::new())
            }
            OutgoingMessage::Event { .. } => {
                return Err("A Socket.IO server emits events: give an event name and arguments".into());
            }
        };
        if event.is_empty() {
            return Err("Enter the name of the event to emit".into());
        }
        let ns = match ns.trim() {
            "" => "/".to_string(),
            ns if ns.starts_with('/') => ns.to_string(),
            ns => format!("/{ns}"),
        };
        let targets = match conn {
            Some(id) => {
                let session = self.session_by_conn(id).ok_or_else(|| format!("Connection #{id} is closed"))?;
                if !lock(&session.namespaces).contains(&ns) {
                    return Err(format!("Client #{id} hasn't joined {ns}"));
                }
                vec![session]
            }
            None => self.members(&ns),
        };
        if targets.is_empty() {
            return Err(if lock(&self.sessions).is_empty() {
                "No client is connected".to_string()
            } else {
                format!("No client has joined {ns}")
            });
        }
        let mut sent = 0;
        let mut last_error = None;
        for session in &targets {
            match self.try_emit(session, &ns, &event, args.clone()) {
                Ok(()) => sent += 1,
                Err(e) => last_error = Some(e),
            }
        }
        match (sent, last_error) {
            (0, Some(e)) => Err(e),
            _ => Ok(sent),
        }
    }
}

/// A POST: the client's packets.
async fn receive(session: &Arc<Session>, req: Request<Incoming>) -> Response<Body> {
    let body = match Limited::new(req.into_body(), MAX_PAYLOAD).collect().await {
        Ok(body) => body.to_bytes(),
        Err(_) => {
            session.close("it sent more than 1 MB at once");
            return error(StatusCode::PAYLOAD_TOO_LARGE, 3, "Payload too large");
        }
    };
    for frame in packet::split_payload(&String::from_utf8_lossy(&body)) {
        let _ = session.inbox.send(frame);
    }
    text("ok".into())
}

/// `/chat ` before an event in the log (nothing for the main namespace).
fn prefix(ns: &str) -> String {
    if ns == "/" { String::new() } else { format!("{ns} ") }
}

fn random_id() -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    (0..20).map(|_| ALPHABET[rand::random_range(0..ALPHABET.len())] as char).collect()
}

/// An event's arguments as rules see them, worked out once per event.
struct Arguments<'a> {
    /// The arguments as a JSON array.
    value: Value,
    /// The same as JSON text (`["hi",{"n":1}]`).
    json: String,
    /// The text of a single text argument.
    single: Option<&'a str>,
}

impl<'a> Arguments<'a> {
    fn new(args: &'a [Value]) -> Self {
        let value = Value::Array(args.to_vec());
        let json = value.to_string();
        let single = match args {
            [Value::String(s)] => Some(s.as_str()),
            _ => None,
        };
        Self { value, json, single }
    }

    fn texts(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.json.as_str()).chain(self.single)
    }
}

/// Whether `rule` answers the event: its name (or `*`), then its pattern (compiled from the
/// rule) against the arguments as JSON (`["hi",{"n":1}]`) or, for one text argument, that text.
fn rule_matches(rule: &SocketIoRule, pattern: &Pattern, name: &str, args: &Arguments) -> bool {
    let wanted = rule.event.trim();
    if !rule.enabled || !(wanted == "*" || wanted.is_empty() || wanted == name) {
        return false;
    }
    match (rule.matcher, pattern) {
        (MatchKind::Any, _) => true,
        (MatchKind::Contains, _) => args.texts().any(|c| c.contains(rule.pattern.as_str())),
        (MatchKind::Exact, _) => args.texts().any(|c| c.trim() == rule.pattern.trim()),
        (MatchKind::Json, Pattern::Compiled(m)) => {
            m.matches_json(&args.value) == Some(true) || args.single.is_some_and(|s| m.matches(s.as_bytes()))
        }
        (MatchKind::Regex, Pattern::Compiled(m)) => args.texts().any(|c| m.matches(c.as_bytes())),
        _ => false,
    }
}

/// Reply or acknowledgement arguments: JSON with `{{event.name}}`, `{{event.args}}` and
/// `{{event.arg0}}`… (the client's values as JSON), plus variables and dynamic values. The
/// client's values go in after the variables are filled in: a client sending `{{token}}`
/// gets that text back, never the variable's value.
fn render_args(template: &str, event: Option<&(String, Vec<Value>)>, vars: &VarContext) -> Result<Vec<Value>, String> {
    let mut out = String::new();
    let mut rest = template;
    let fill = |text: &str| crate::template::render(text, None, vars);
    while let Some(start) = rest.find("{{event.") {
        let Some(len) = rest[start..].find("}}") else { break };
        let key = &rest[start + 8..start + len];
        let value = match (key.trim(), event) {
            ("name", Some((name, _))) => Some(Value::String(name.clone()).to_string()),
            ("args", Some((_, args))) => Some(Value::Array(args.clone()).to_string()),
            (key, Some((_, args))) => key
                .strip_prefix("arg")
                .and_then(|n| n.parse::<usize>().ok())
                .map(|n| args.get(n).cloned().unwrap_or(Value::Null).to_string()),
            _ => None,
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
    packet::parse_args(&out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(event: &str, matcher: MatchKind, pattern: &str) -> SocketIoRule {
        SocketIoRule { event: event.into(), matcher, pattern: pattern.into(), ..Default::default() }
    }

    fn rule_matches(rule: &SocketIoRule, name: &str, args: &[Value]) -> bool {
        super::rule_matches(rule, &Pattern::new(rule), name, &Arguments::new(args))
    }

    #[test]
    fn rules_match_by_event_and_arguments() {
        let hi = [json!("hi there")];
        assert!(rule_matches(&rule("chat", MatchKind::Any, ""), "chat", &hi));
        assert!(!rule_matches(&rule("chat", MatchKind::Any, ""), "other", &hi));
        assert!(rule_matches(&rule("*", MatchKind::Contains, "there"), "x", &hi));
        assert!(rule_matches(&rule("chat", MatchKind::Exact, "hi there"), "chat", &hi), "one text argument as text");
        assert!(rule_matches(&rule("chat", MatchKind::Exact, r#"["hi there"]"#), "chat", &hi), "or as JSON");
        let obj = [json!({ "id": 7 }), json!(2)];
        assert!(rule_matches(&rule("save", MatchKind::Regex, r#""id":\d+"#), "save", &obj));
        assert!(!rule_matches(&rule("save", MatchKind::Regex, "("), "save", &obj), "a bad regex never matches");
        let off = SocketIoRule { enabled: false, ..rule("*", MatchKind::Any, "") };
        assert!(!rule_matches(&off, "x", &hi));
        // JSON conditions look at the arguments, or at one text argument that is JSON.
        assert!(rule_matches(&rule("save", MatchKind::Json, "$[0].id == 7"), "save", &obj));
        assert!(!rule_matches(&rule("save", MatchKind::Json, "$[0].id == 8"), "save", &obj));
        assert!(rule_matches(&rule("raw", MatchKind::Json, "$.a == 1"), "raw", &[json!(r#"{"a": 1}"#)]));
        assert!(!rule_matches(&rule("save", MatchKind::Json, "$..id"), "save", &obj), "a bad condition never matches");
        // Both kinds of patterns that can't be used are reported.
        let found = problems(&[
            rule("a", MatchKind::Regex, "("),
            rule("b", MatchKind::Json, "$..id == 1"),
            rule("c", MatchKind::Json, "$[0].id == 7"),
            SocketIoRule { enabled: false, ..rule("d", MatchKind::Regex, "(") },
        ]);
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found[0].starts_with("Rule 1: invalid regex"), "{found:?}");
        assert!(found[1].starts_with("Rule 2: ") && found[1].contains("not a JSON path"), "{found:?}");
    }

    #[test]
    fn replies_use_the_event_and_never_expand_client_text() {
        let mut vars = VarContext::new();
        vars.push_layer(&[zorvik_formats::Variable {
            key: "token".into(),
            value: "s3cret".into(),
            enabled: true,
            secret: true,
        }]);
        let event = ("chat".to_string(), vec![json!("{{token}}"), json!({ "n": 1 })]);
        let args = render_args(
            r#"[{{event.name}}, {{event.arg0}}, {{event.arg1}}, {{event.arg5}}, "{{token}}"]"#,
            Some(&event),
            &vars,
        )
        .unwrap();
        assert_eq!(args, [json!("chat"), json!("{{token}}"), json!({ "n": 1 }), Value::Null, json!("s3cret")]);
        assert_eq!(render_args("{{event.args}}", Some(&event), &vars).unwrap(), event.1);
        assert_eq!(render_args("", Some(&event), &vars).unwrap(), Vec::<Value>::new());
        assert_eq!(render_args(r#"{"id": "{{$uuid}}"}"#, None, &vars).unwrap()[0]["id"].as_str().unwrap().len(), 36);
        assert!(render_args("{{event.arg0}} oops", Some(&event), &vars).unwrap_err().contains("not valid JSON"));
    }
}

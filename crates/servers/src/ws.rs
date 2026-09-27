//! WebSocket server: echo, rule-based replies, manual sends from the UI, or
//! discard. Any path is accepted; text frames match rules as text.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::{CloseFrame, WebSocketConfig};
use tokio_tungstenite::tungstenite::{self, Message};
use tokio_util::sync::CancellationToken;
use zorvik_engine::framing::PayloadEncoding;
use zorvik_formats::ReplyMode;

use crate::connections::{ConnCommand, Connections};
use crate::report::{OutgoingMessage, Reporter, TrafficDirection};
use crate::rules::Replier;
use crate::tcp::TLS_HANDSHAKE_TIMEOUT;
use crate::template::render;
use crate::{Control, Ctx, Live, message_bytes};

/// Largest message (and frame) a client may send.
const MAX_MESSAGE: usize = 16 << 20;
/// Time a client gets for the upgrade request.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a closing connection waits for the client's close frame.
const CLOSE_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone)]
struct Outgoing {
    bytes: Vec<u8>,
    is_text: bool,
}

pub(crate) async fn run(listener: TcpListener, mut ctx: Ctx) -> Result<(), String> {
    let conns: Arc<Connections<Outgoing>> = Arc::default();
    let acceptor = ctx.tls.clone().map(tokio_rustls::TlsAcceptor::from);
    let mut live = ctx.live.clone();
    let first = live.borrow_and_update().clone();
    let mut reported = report_rule_problems(&first, &ctx.reporter, &[]);
    loop {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok((stream, peer)) => {
                    let _ = stream.set_nodelay(true);
                    tokio::spawn(connection(
                        stream,
                        ctx.reporter.next_conn(),
                        peer,
                        acceptor.clone(),
                        ctx.live.clone(),
                        ctx.reporter.clone(),
                        conns.clone(),
                        ctx.cancel.child_token(),
                    ));
                }
                Err(e) => {
                    ctx.reporter.error(None, None, format!("Accept failed: {e}"));
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            },
            Some(control) = ctx.control.recv() => match control {
                Control::Send { conn, message, reply } => {
                    let result = match message {
                        OutgoingMessage::Event { .. } => {
                            Err("A WebSocket server sends text or binary messages, not Server-Sent Events".to_string())
                        }
                        message => message_bytes(&message)
                            .and_then(|(bytes, is_text)| conns.send(conn, Outgoing { bytes, is_text })),
                    };
                    let _ = reply.send(result);
                }
                Control::Disconnect { conn } => conns.close(conn),
            },
            Ok(()) = live.changed() => {
                let current = live.borrow_and_update().clone();
                reported = report_rule_problems(&current, &ctx.reporter, &reported);
            }
            _ = ctx.cancel.cancelled() => return Ok(()),
        }
    }
}

/// Rules that can never match (bad regex) are reported when they appear (not
/// again on every edit while they stay broken). Returns the current problems.
fn report_rule_problems(live: &Live, reporter: &Reporter, already: &[String]) -> Vec<String> {
    let ws = &live.server.websocket;
    if ws.mode != ReplyMode::Rules {
        return Vec::new();
    }
    let problems = Replier::new(&ws.rules, PayloadEncoding::Text).problems;
    for problem in problems.iter().filter(|p| !already.contains(p)) {
        reporter.error(None, None, problem.clone());
    }
    problems
}

#[allow(clippy::too_many_arguments)]
async fn connection(
    stream: TcpStream,
    conn: u64,
    peer: SocketAddr,
    acceptor: Option<tokio_rustls::TlsAcceptor>,
    live: watch::Receiver<Arc<Live>>,
    reporter: Reporter,
    conns: Arc<Connections<Outgoing>>,
    cancel: CancellationToken,
) {
    match acceptor {
        None => upgrade(stream, conn, peer, live, &reporter, &conns, &cancel).await,
        Some(acceptor) => match tokio::time::timeout(TLS_HANDSHAKE_TIMEOUT, acceptor.accept(stream)).await {
            Ok(Ok(tls)) => upgrade(tls, conn, peer, live, &reporter, &conns, &cancel).await,
            Ok(Err(e)) => reporter.error(Some(conn), Some(&peer), format!("TLS handshake failed: {e}")),
            Err(_) => reporter.error(Some(conn), Some(&peer), "TLS handshake timed out"),
        },
    }
}

/// WebSocket handshake, then serve; the client counts as connected once upgraded.
async fn upgrade<S: AsyncRead + AsyncWrite + Unpin>(
    stream: S,
    conn: u64,
    peer: SocketAddr,
    live: watch::Receiver<Arc<Live>>,
    reporter: &Reporter,
    conns: &Connections<Outgoing>,
    cancel: &CancellationToken,
) {
    let config = WebSocketConfig::default().max_message_size(Some(MAX_MESSAGE)).max_frame_size(Some(MAX_MESSAGE));
    let handshake = tokio_tungstenite::accept_async_with_config(stream, Some(config));
    let ws = tokio::select! {
        result = tokio::time::timeout(HANDSHAKE_TIMEOUT, handshake) => match result {
            Ok(Ok(ws)) => ws,
            Ok(Err(e)) => {
                reporter.error(Some(conn), Some(&peer), format!("Not a WebSocket client: {}", handshake_error(&e)));
                return;
            }
            Err(_) => {
                reporter.error(Some(conn), Some(&peer), "The WebSocket handshake timed out");
                return;
            }
        },
        _ = cancel.cancelled() => return,
    };
    reporter.opened(conn, &peer);
    let reason = serve(ws, conn, peer, live, reporter, conns, cancel).await;
    conns.remove(conn);
    reporter.closed(conn, &peer, &reason);
}

fn handshake_error(e: &tungstenite::Error) -> String {
    use tungstenite::error::ProtocolError as P;
    match e {
        tungstenite::Error::Protocol(P::WrongHttpMethod) => "the request is not a GET upgrade request".into(),
        tungstenite::Error::Protocol(
            P::MissingConnectionUpgradeHeader | P::MissingUpgradeWebSocketHeader | P::MissingSecWebSocketKey,
        ) => "the request has no WebSocket upgrade headers (plain HTTP?)".into(),
        tungstenite::Error::Io(e) => format!("connection error during the handshake: {e}"),
        other => other.to_string(),
    }
}

/// Why the client closed, e.g. `closed by the client (1001 going away)`.
fn close_reason(frame: Option<&CloseFrame>) -> String {
    match frame {
        None => "closed by the client".into(),
        Some(f) if f.reason.is_empty() => format!("closed by the client ({})", u16::from(f.code)),
        Some(f) => format!("closed by the client ({} {})", u16::from(f.code), f.reason),
    }
}

async fn serve<S: AsyncRead + AsyncWrite + Unpin>(
    ws: WebSocketStream<S>,
    conn: u64,
    peer: SocketAddr,
    mut live: watch::Receiver<Arc<Live>>,
    reporter: &Reporter,
    conns: &Connections<Outgoing>,
    cancel: &CancellationToken,
) -> String {
    let (mut sink, mut source) = ws.split();
    let mut commands = conns.add(conn);
    let mut current = live.borrow_and_update().clone();
    let mut replier = Replier::new(&current.server.websocket.rules, PayloadEncoding::Text);

    macro_rules! send_out {
        ($bytes:expr, $is_text:expr) => {{
            let bytes: Vec<u8> = $bytes;
            let is_text: bool = $is_text;
            let message = if is_text {
                Message::text(String::from_utf8_lossy(&bytes).into_owned())
            } else {
                Message::binary(Bytes::from(bytes.clone()))
            };
            // A client that stops reading blocks the send: stopping the server or
            // disconnecting the client still ends the connection.
            tokio::select! {
                sent = sink.send(message) => {
                    if let Err(e) = sent {
                        return format!("Write failed: {e}");
                    }
                }
                _ = commands.closing() => return "closed by you".into(),
                _ = cancel.cancelled() => return "server stopped".into(),
            }
            reporter.data(Some(conn), Some(&peer), TrafficDirection::Out, &bytes, if is_text { "" } else { "binary" });
        }};
    }

    let greeting = &current.server.websocket.greeting;
    if !greeting.is_empty() {
        let text = render(greeting, None, &current.vars);
        send_out!(text.into_bytes(), true);
    }
    loop {
        tokio::select! {
            incoming = source.next() => {
                let (bytes, is_text) = match incoming {
                    Some(Ok(Message::Text(text))) => (text.as_bytes().to_vec(), true),
                    Some(Ok(Message::Binary(data))) => (data.to_vec(), false),
                    // Pings are answered by tungstenite.
                    Some(Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_))) => continue,
                    Some(Ok(Message::Close(frame))) => {
                        // Sends tungstenite's reply to the close handshake.
                        let _ = tokio::time::timeout(CLOSE_TIMEOUT, sink.flush()).await;
                        return close_reason(frame.as_ref());
                    }
                    Some(Err(e)) => return read_error(&e),
                    None => return "connection closed".into(),
                };
                reporter.data(Some(conn), Some(&peer), TrafficDirection::In, &bytes, if is_text { "" } else { "binary" });
                if live.has_changed().unwrap_or(false) {
                    current = live.borrow_and_update().clone();
                    replier = Replier::new(&current.server.websocket.rules, PayloadEncoding::Text);
                }
                match current.server.websocket.mode {
                    ReplyMode::Echo => send_out!(bytes, is_text),
                    ReplyMode::Rules => {
                        if let Some(reply) = replier.reply(&bytes, &current.vars) {
                            if reply.delay_ms > 0 {
                                tokio::select! {
                                    _ = tokio::time::sleep(Duration::from_millis(reply.delay_ms)) => {}
                                    _ = cancel.cancelled() => return "server stopped".into(),
                                }
                            }
                            send_out!(reply.bytes, true);
                        }
                    }
                    ReplyMode::Manual | ReplyMode::Discard => {}
                }
            }
            command = commands.recv() => match command {
                Some(ConnCommand::Send(out)) => send_out!(out.bytes, out.is_text),
                Some(ConnCommand::Close) | None => {
                    close(&mut sink, &mut source, CloseCode::Normal, "").await;
                    return "closed by you".into();
                }
            },
            _ = cancel.cancelled() => {
                close(&mut sink, &mut source, CloseCode::Away, "server stopped").await;
                return "server stopped".into();
            }
        }
    }
}

fn read_error(e: &tungstenite::Error) -> String {
    use tungstenite::error::{CapacityError, ProtocolError};
    match e {
        tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed => "connection closed".into(),
        tungstenite::Error::Protocol(ProtocolError::ResetWithoutClosingHandshake) => {
            "the client dropped the connection without a close frame".into()
        }
        tungstenite::Error::Capacity(CapacityError::MessageTooLong { size, max_size }) => {
            format!("message of {size} bytes is larger than the limit ({max_size} bytes)")
        }
        other => format!("Read failed: {other}"),
    }
}

/// Send a close frame and wait (briefly) for the client's answer.
async fn close<S: AsyncRead + AsyncWrite + Unpin>(
    sink: &mut futures_util::stream::SplitSink<WebSocketStream<S>, Message>,
    source: &mut futures_util::stream::SplitStream<WebSocketStream<S>>,
    code: CloseCode,
    reason: &str,
) {
    let frame = CloseFrame { code, reason: reason.to_string().into() };
    if tokio::time::timeout(CLOSE_TIMEOUT, sink.send(Message::Close(Some(frame)))).await.is_err() {
        return;
    }
    let _ = tokio::time::timeout(CLOSE_TIMEOUT, async {
        while let Some(Ok(message)) = source.next().await {
            if message.is_close() {
                break;
            }
        }
    })
    .await;
}

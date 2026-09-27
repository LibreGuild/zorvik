//! TCP server: echo, rule-based replies, manual sends from the UI, or discard.
//! Incoming bytes are split into messages by the configured framing.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use zorvik_engine::framing::{Deframer, Framing, LineEnding, encode_message};
use zorvik_formats::{ReplyMode, SocketServerConfig};

use crate::connections::{ConnCommand, Connections};
use crate::report::{Reporter, TrafficDirection};
use crate::rules::{Replier, render};
use crate::{Control, Ctx, Live, message_bytes};

/// Time a client gets to finish the TLS handshake.
pub(crate) const TLS_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

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
                    // Ids follow accept order (assigned here, not in the spawned task).
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
                    // E.g. too many open files: report and back off instead of spinning.
                    ctx.reporter.error(None, None, format!("Accept failed: {e}"));
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            },
            Some(control) = ctx.control.recv() => match control {
                Control::Send { conn, message, reply } => {
                    let result = message_bytes(&message).and_then(|(bytes, is_text)| conns.send(conn, Outgoing { bytes, is_text }));
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

/// Rules that can never match (bad regex or hex) are reported when they appear
/// (not again on every edit while they stay broken). Returns the current problems.
fn report_rule_problems(live: &Live, reporter: &Reporter, already: &[String]) -> Vec<String> {
    let socket = &live.server.socket;
    if socket.mode != ReplyMode::Rules {
        return Vec::new();
    }
    let problems = Replier::new(&socket.rules, socket.encoding).problems;
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
    reporter.opened(conn, &peer);
    let reason = match acceptor {
        None => serve(stream, conn, peer, live, &reporter, &conns, &cancel).await,
        Some(acceptor) => match tokio::time::timeout(TLS_HANDSHAKE_TIMEOUT, acceptor.accept(stream)).await {
            Ok(Ok(tls)) => serve(tls, conn, peer, live, &reporter, &conns, &cancel).await,
            Ok(Err(e)) => format!("TLS handshake failed: {e}"),
            Err(_) => "TLS handshake timed out".to_string(),
        },
    };
    conns.remove(conn);
    reporter.closed(conn, &peer, &reason);
}

/// Serve one connection until it closes; returns why it ended.
async fn serve<S: AsyncRead + AsyncWrite + Unpin>(
    stream: S,
    conn: u64,
    peer: SocketAddr,
    mut live: watch::Receiver<Arc<Live>>,
    reporter: &Reporter,
    conns: &Connections<Outgoing>,
    cancel: &CancellationToken,
) -> String {
    let (mut reader, mut writer) = tokio::io::split(stream);
    let mut commands = conns.add(conn);
    let mut current = live.borrow_and_update().clone();
    let mut replier = Replier::new(&current.server.socket.rules, current.server.socket.encoding);
    // Framing is fixed per connection (changing it mid-stream would garble messages).
    let config = current.server.socket.clone();
    let mut deframer = Deframer::new(config.framing, config.length_bytes);

    macro_rules! write_out {
        ($bytes:expr, $is_text:expr) => {{
            let bytes: Vec<u8> = $bytes;
            match frame(&config, bytes.clone(), $is_text) {
                Ok(framed) => {
                    // A client that stops reading blocks the write: stopping the
                    // server or disconnecting the client still ends the connection.
                    tokio::select! {
                        // Flush too: over TLS the tail can otherwise wait in rustls until the next write.
                        written = async { writer.write_all(&framed).await?; writer.flush().await } => {
                            if let Err(e) = written {
                                return format!("Write failed: {e}");
                            }
                        }
                        _ = commands.closing() => return "closed by you".into(),
                        _ = cancel.cancelled() => return "server stopped".into(),
                    }
                    reporter.data(Some(conn), Some(&peer), TrafficDirection::Out, &shown(&config, bytes, $is_text), "");
                }
                Err(e) => reporter.error(Some(conn), Some(&peer), e),
            }
        }};
    }

    if !config.greeting.is_empty() {
        let greeting = render(&config.greeting, config.encoding, b"", &current.vars, 0);
        write_out!(greeting.bytes, greeting.is_text);
    }
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        tokio::select! {
            read = reader.read(&mut buf) => {
                let n = match read {
                    Ok(0) => {
                        if let Some(rest) = deframer.remainder() {
                            reporter.data(Some(conn), Some(&peer), TrafficDirection::In, &rest, "unfinished message");
                        }
                        return String::new();
                    }
                    Ok(n) => n,
                    Err(e) => return format!("Read failed: {e}"),
                };
                let messages = match deframer.push(&buf[..n]) {
                    Ok(m) => m,
                    Err(e) => return e,
                };
                if live.has_changed().unwrap_or(false) {
                    current = live.borrow_and_update().clone();
                    replier = Replier::new(&current.server.socket.rules, current.server.socket.encoding);
                }
                for message in messages {
                    reporter.data(Some(conn), Some(&peer), TrafficDirection::In, &message, "");
                    match current.server.socket.mode {
                        ReplyMode::Echo => write_out!(message, false),
                        ReplyMode::Rules => {
                            if let Some(reply) = replier.reply(&message, &current.vars) {
                                if reply.delay_ms > 0 {
                                    tokio::select! {
                                        _ = tokio::time::sleep(Duration::from_millis(reply.delay_ms)) => {}
                                        _ = commands.closing() => return "closed by you".into(),
                                        _ = cancel.cancelled() => return "server stopped".into(),
                                    }
                                }
                                write_out!(reply.bytes, reply.is_text);
                            }
                        }
                        ReplyMode::Manual | ReplyMode::Discard => {}
                    }
                }
            }
            command = commands.recv() => match command {
                Some(ConnCommand::Send(out)) => write_out!(out.bytes, out.is_text),
                Some(ConnCommand::Close) | None => {
                    let _ = writer.shutdown().await;
                    return "closed by you".into();
                }
            },
            _ = cancel.cancelled() => {
                let _ = writer.shutdown().await;
                return "server stopped".into();
            }
        }
    }
}

/// Line framing needs a line break even when none is configured.
fn line_ending(config: &SocketServerConfig) -> LineEnding {
    match (config.framing, config.line_ending) {
        (Framing::Line, LineEnding::None) => LineEnding::Lf,
        (_, ending) => ending,
    }
}

fn frame(config: &SocketServerConfig, bytes: Vec<u8>, is_text: bool) -> Result<Vec<u8>, String> {
    // Echoed/binary messages on a line protocol still end with a line break.
    let is_text = is_text || config.framing == Framing::Line;
    encode_message(config.framing, config.length_bytes, line_ending(config), bytes, is_text)
}

/// What the log shows for a sent message: the payload plus its line ending.
fn shown(config: &SocketServerConfig, mut bytes: Vec<u8>, is_text: bool) -> Vec<u8> {
    if is_text || config.framing == Framing::Line {
        bytes.extend_from_slice(line_ending(config).as_str().as_bytes());
    }
    bytes
}

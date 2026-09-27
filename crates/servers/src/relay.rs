//! TCP relay: each client connection is relayed to the target (optionally
//! over TLS) and the bytes in both directions show in the log. A side that
//! stops sending (EOF) is passed on as a half-close; the pair closes when both
//! sides are done, on an error, or when you disconnect it. Sends from the UI
//! go to the client, as if the target had sent them. The target is connected
//! directly (no proxy).

use std::io;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use zorvik_engine::socket::parse_socket_url;
use zorvik_formats::TcpProxyConfig;

use crate::connections::{Commands, ConnCommand, Connections, MAX_QUEUED};
use crate::report::{Reporter, TrafficDirection};
use crate::{Control, Ctx, StartOptions, message_bytes};

/// Time to reach the target (DNS, TCP and TLS).
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

pub(crate) struct Target {
    host: String,
    port: u16,
    tls: bool,
    /// `host:port` for messages.
    label: String,
}

pub(crate) fn parse_target(config: &TcpProxyConfig) -> Result<Target, String> {
    let raw = config.target.trim();
    if raw.is_empty() {
        return Err("Set the target (host:port) the relay connects clients to".into());
    }
    let (url, host, port) =
        parse_socket_url(raw, "tcp", &["tcp", "tls"]).map_err(|e| format!("Invalid target: {}", e.message))?;
    let label = if host.contains(':') { format!("[{host}]:{port}") } else { format!("{host}:{port}") };
    Ok(Target { tls: config.upstream_tls || url.scheme() == "tls", host, port, label })
}

/// Whether the target is this relay's own listener (every connection would
/// connect to itself, forever).
async fn is_self(target: &Target, listen: SocketAddr) -> bool {
    if target.port != listen.port() {
        return false;
    }
    let ips: Vec<IpAddr> = match target.host.parse::<IpAddr>() {
        Ok(ip) => vec![ip],
        Err(_) => {
            let lookup = tokio::net::lookup_host((target.host.as_str(), target.port));
            match tokio::time::timeout(Duration::from_secs(3), lookup).await {
                Ok(Ok(addrs)) => addrs.map(|a| a.ip()).collect(),
                _ => return false,
            }
        }
    };
    let local = |ip: &IpAddr| ip.is_loopback() || ip.is_unspecified();
    let us = listen.ip();
    ips.iter().any(|ip| {
        ip.is_ipv4() == us.is_ipv4()
            && (*ip == us || (us.is_unspecified() && local(ip)) || (ip.is_unspecified() && us.is_loopback()))
    })
}

fn self_error(target: &Target) -> String {
    format!("The target {} is this relay itself: point it at another server", target.label)
}

pub(crate) async fn run(listener: TcpListener, mut ctx: Ctx) -> Result<(), String> {
    let listen = listener.local_addr().map_err(|e| e.to_string())?;
    let target = parse_target(&ctx.live.borrow().server.proxy)?;
    if is_self(&target, listen).await {
        return Err(self_error(&target));
    }
    let conns: Arc<Connections<Vec<u8>>> = Arc::default();
    loop {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok((stream, peer)) => {
                    let _ = stream.set_nodelay(true);
                    // The target as configured now: edits apply to the next connection.
                    let config = ctx.live.borrow().server.proxy.clone();
                    tokio::spawn(connection(
                        stream,
                        ctx.reporter.next_conn(),
                        peer,
                        listen,
                        config,
                        ctx.options.clone(),
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
                    let result = message_bytes(&message).and_then(|(bytes, _)| conns.send(conn, bytes));
                    let _ = reply.send(result);
                }
                Control::Disconnect { conn } => conns.close(conn),
            },
            _ = ctx.cancel.cancelled() => return Ok(()),
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn connection(
    client: TcpStream,
    conn: u64,
    peer: SocketAddr,
    listen: SocketAddr,
    config: TcpProxyConfig,
    options: Arc<StartOptions>,
    reporter: Reporter,
    conns: Arc<Connections<Vec<u8>>>,
    cancel: CancellationToken,
) {
    reporter.opened(conn, &peer);
    let commands = conns.add(conn);
    let link = Link { conn, peer, reporter: &reporter };
    let reason = relay(client, &link, listen, &config, &options, commands, &cancel).await;
    conns.remove(conn);
    reporter.closed(conn, &peer, &reason);
}

/// One client connection, for reporting.
struct Link<'a> {
    conn: u64,
    peer: SocketAddr,
    reporter: &'a Reporter,
}

impl Link<'_> {
    fn data(&self, direction: TrafficDirection, bytes: &[u8]) {
        self.reporter.data(Some(self.conn), Some(&self.peer), direction, bytes, "");
    }

    fn error(&self, message: String) {
        self.reporter.error(Some(self.conn), Some(&self.peer), message);
    }
}

/// Relay one client until the pair closes; returns why it ended.
async fn relay(
    mut client: TcpStream,
    link: &Link<'_>,
    listen: SocketAddr,
    config: &TcpProxyConfig,
    options: &StartOptions,
    mut commands: Commands<Vec<u8>>,
    cancel: &CancellationToken,
) -> String {
    let target = match parse_target(config) {
        Ok(t) => t,
        Err(e) => {
            link.error(e.clone());
            return format!("target unreachable: {e}");
        }
    };
    if is_self(&target, listen).await {
        let e = self_error(&target);
        link.error(e.clone());
        return format!("target unreachable: {e}");
    }

    let connect = options.client.connect_raw(
        &target.host,
        target.port,
        target.tls,
        &options.request_options.tls,
        CONNECT_TIMEOUT,
    );
    tokio::pin!(connect);
    let upstream = loop {
        tokio::select! {
            result = &mut connect => break result,
            command = commands.recv() => match command {
                // The client is already there: sends reach it while the target connects.
                Some(ConnCommand::Send(bytes)) => tokio::select! {
                    written = async { client.write_all(&bytes).await?; client.flush().await } => match written {
                        Ok(()) => link.data(TrafficDirection::Out, &bytes),
                        Err(e) => return format!("client connection failed: {e}"),
                    },
                    _ = commands.closing() => return "closed by you".into(),
                    _ = cancel.cancelled() => return "server stopped".into(),
                },
                Some(ConnCommand::Close) | None => return "closed by you".into(),
            },
            _ = cancel.cancelled() => return "server stopped".into(),
        }
    };
    let upstream = match upstream {
        Ok(u) => u,
        Err(e) => {
            link.error(format!("Could not connect to the target {}: {}", target.label, e.message));
            return format!("target unreachable: {}", e.message);
        }
    };
    let tls = upstream.tls.as_ref().map(|t| format!(" over {}", t.version)).unwrap_or_default();
    link.reporter.info(format!("#{} relayed to {} ({}){tls}", link.conn, target.label, upstream.remote_addr));

    let (client_read, client_write) = client.into_split();
    let (target_read, target_write) = tokio::io::split(upstream.stream);
    // Sends from the UI go to the client, between chunks from the target.
    let (inject, injected) = mpsc::channel::<Vec<u8>>(MAX_QUEUED);
    let up = pump(client_read, target_write, None, TrafficDirection::ToTarget, ("client", "target"), link);
    let down =
        pump(target_read, client_write, Some(injected), TrafficDirection::FromTarget, ("target", "client"), link);
    tokio::pin!(up, down);
    let (mut up_done, mut down_done) = (false, false);
    let mut first_closed = None;
    loop {
        tokio::select! {
            result = &mut up, if !up_done => match result {
                Ok(()) => {
                    up_done = true;
                    let first = *first_closed.get_or_insert("client closed");
                    if down_done {
                        return first.into();
                    }
                }
                Err(e) => return e,
            },
            result = &mut down, if !down_done => match result {
                Ok(()) => {
                    down_done = true;
                    let first = *first_closed.get_or_insert("target closed");
                    if up_done {
                        return first.into();
                    }
                }
                Err(e) => return e,
            },
            command = commands.recv() => match command {
                Some(ConnCommand::Send(bytes)) => match inject.try_send(bytes) {
                    Ok(()) => {}
                    Err(mpsc::error::TrySendError::Full(_)) => {
                        link.error(format!("Not sent: the client is not reading ({MAX_QUEUED} sends are still waiting)"));
                    }
                    Err(mpsc::error::TrySendError::Closed(_)) => {
                        link.error("Not sent: the relay has already closed its side of this client connection".into());
                    }
                },
                Some(ConnCommand::Close) | None => return "closed by you".into(),
            },
            _ = cancel.cancelled() => return "server stopped".into(),
        }
    }
}

/// Copy `from` to `to` (plus `injected` bytes, reported as sent by the relay),
/// reporting each chunk. At EOF the write side of `to` is shut down (a
/// half-close) and `Ok` is returned; errors end the pair.
async fn pump<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    mut from: R,
    mut to: W,
    mut injected: Option<mpsc::Receiver<Vec<u8>>>,
    direction: TrafficDirection,
    (from_side, to_side): (&str, &str),
    link: &Link<'_>,
) -> Result<(), String> {
    let mut buf = vec![0u8; 64 * 1024];
    let write_failed = |e: io::Error| format!("{to_side} connection failed: {e}");
    loop {
        tokio::select! {
            read = from.read(&mut buf) => {
                let n = match read {
                    Ok(n) => n,
                    // TLS peers often close without a close_notify: that is still the end of the stream.
                    Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => 0,
                    Err(e) => return Err(format!("{from_side} connection failed: {e}")),
                };
                if n == 0 {
                    let _ = to.shutdown().await;
                    return Ok(());
                }
                // Flushed per chunk: TLS on either side must not hold data back.
                to.write_all(&buf[..n]).await.map_err(write_failed)?;
                to.flush().await.map_err(write_failed)?;
                link.data(direction, &buf[..n]);
            }
            Some(bytes) = recv(&mut injected) => {
                to.write_all(&bytes).await.map_err(write_failed)?;
                to.flush().await.map_err(write_failed)?;
                link.data(TrafficDirection::Out, &bytes);
            }
        }
    }
}

async fn recv(rx: &mut Option<mpsc::Receiver<Vec<u8>>>) -> Option<Vec<u8>> {
    match rx {
        Some(rx) => rx.recv().await,
        None => std::future::pending().await,
    }
}

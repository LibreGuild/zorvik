//! UDP client: each message is one datagram to the target, and every datagram
//! arriving on the socket is shown with its sender. The socket is not
//! connected, so answers from other addresses (e.g. to a broadcast) show up too.
//!
//! UDP never goes through a proxy (HTTP proxies only tunnel TCP): proxy
//! settings are ignored.

use std::io;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Instant;

use tokio::net::UdpSocket;
use tokio::sync::mpsc;

use crate::error::{EngineError, ErrorKind, Result};
use crate::http::{Client, RequestOptions, Timing};
use crate::net;
use crate::socket::{
    SocketConfig, SocketConnected, SocketEvent, SocketOpened, SocketOutgoing, SocketSession, ms, parse_socket_url,
};
use crate::ws::Direction;

/// Largest UDP payload over IPv4 (65,535 bytes minus the IP and UDP headers).
pub const MAX_DATAGRAM: usize = 65_507;

impl Client {
    /// Open a UDP session to `udp://host:port` (or `host:port`). Nothing is sent
    /// until the first message; when the host has IPv4 and IPv6 addresses, IPv4
    /// is used (local servers usually listen on 127.0.0.1 while `localhost` may
    /// resolve to `::1` first, and UDP has no handshake to find out which works).
    pub async fn udp(&self, address: &str, opts: &RequestOptions, config: SocketConfig) -> Result<SocketConnected> {
        let started = Instant::now();
        let (_, host, port) = parse_socket_url(address, "udp", &["udp"])?;
        let addrs = net::resolve(&host, port, opts.connect_timeout).await?;
        let dns = started.elapsed();
        let target = addrs.iter().copied().find(SocketAddr::is_ipv4).unwrap_or(addrs[0]);
        let bind: SocketAddr =
            if target.is_ipv4() { (Ipv4Addr::UNSPECIFIED, 0).into() } else { (Ipv6Addr::UNSPECIFIED, 0).into() };
        let io_err = |what: &str, e: io::Error| EngineError::new(ErrorKind::Io, format!("{what}: {e}"));
        let socket = UdpSocket::bind(bind).await.map_err(|e| io_err("Could not open a UDP socket", e))?;
        if config.broadcast {
            socket.set_broadcast(true).map_err(|e| io_err("Could not allow broadcast", e))?;
        }
        let opened = SocketOpened {
            protocol: "UDP".into(),
            remote_addr: Some(target.to_string()),
            local_addr: socket.local_addr().ok().map(|a| a.to_string()),
            tls: None,
            timing: Timing { dns_ms: ms(dns), total_ms: ms(started.elapsed()), ..Default::default() },
        };
        let (out_tx, out_rx) = mpsc::unbounded_channel();
        let (ev_tx, ev_rx) = mpsc::unbounded_channel();
        tokio::spawn(run_udp(socket, target, config, out_rx, ev_tx));
        Ok(SocketConnected { opened, session: SocketSession { tx: out_tx }, events: ev_rx })
    }
}

fn with_peer(mut event: SocketEvent, addr: SocketAddr) -> SocketEvent {
    if let SocketEvent::Message { peer, .. } = &mut event {
        *peer = Some(addr.to_string());
    }
    event
}

/// ICMP "port/host unreachable" surfaces as an error on the next receive
/// (Windows: WSAECONNRESET; Linux: ECONNREFUSED). The socket stays usable.
fn is_unreachable(e: &io::Error) -> bool {
    use io::ErrorKind as K;
    matches!(e.kind(), K::ConnectionReset | K::ConnectionRefused | K::HostUnreachable | K::NetworkUnreachable)
}

fn send_error(e: &io::Error, config: &SocketConfig, target: SocketAddr) -> String {
    if e.kind() == io::ErrorKind::PermissionDenied && !config.broadcast {
        return format!("Could not send to {target}: {e}. Sending to a broadcast address needs “Allow broadcast”.");
    }
    if is_unreachable(e) {
        return format!("Nothing is listening at {target} (port unreachable)");
    }
    format!("Could not send to {target}: {e}")
}

async fn run_udp(
    socket: UdpSocket,
    target: SocketAddr,
    config: SocketConfig,
    mut outgoing: mpsc::UnboundedReceiver<SocketOutgoing>,
    events: mpsc::UnboundedSender<SocketEvent>,
) {
    let mut buf = vec![0u8; 65_536];
    let error = |message: String| SocketEvent::Error { message };
    loop {
        tokio::select! {
            out = outgoing.recv() => {
                let Some(msg) = out else {
                    // Session handle dropped.
                    let _ = events.send(SocketEvent::Closed { reason: "Closed".into(), by_client: true });
                    return;
                };
                let (mut bytes, is_text) = match msg.payload() {
                    Ok(Some(p)) => p,
                    Ok(None) => {
                        let _ = events.send(error("Not supported on a UDP connection".into()));
                        continue;
                    }
                    Err(e) => {
                        let _ = events.send(error(e.message));
                        continue;
                    }
                };
                if is_text {
                    bytes.extend_from_slice(config.line_ending.as_str().as_bytes());
                }
                if bytes.len() > MAX_DATAGRAM {
                    let _ = events.send(error(format!(
                        "The message is {} bytes; a UDP datagram holds at most {MAX_DATAGRAM} bytes",
                        bytes.len()
                    )));
                    continue;
                }
                match socket.send_to(&bytes, target).await {
                    Ok(_) => {
                        let _ = events.send(with_peer(SocketEvent::message(Direction::Sent, &bytes), target));
                    }
                    // E.g. no route, broadcast not allowed, or more than the OS allows (macOS: 9216 bytes by default).
                    Err(e) => {
                        let _ = events.send(error(send_error(&e, &config, target)));
                    }
                }
            }
            received = socket.recv_from(&mut buf) => match received {
                Ok((n, peer)) => {
                    let _ = events.send(with_peer(SocketEvent::message(Direction::Received, &buf[..n]), peer));
                }
                Err(e) if is_unreachable(&e) => {
                    let _ = events.send(error(format!("Nothing is listening at {target} (port unreachable)")));
                }
                Err(e) => {
                    let reason = format!("Receive failed: {e}");
                    let _ = events.send(SocketEvent::Closed { reason, by_client: false });
                    return;
                }
            }
        }
    }
}

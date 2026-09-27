//! UDP server: echo, rule-based replies, manual sends from the UI, or discard.
//! UDP has no connections, so every sender address becomes a
//! pseudo-connection on its first datagram (an id in the log, a target for
//! sends) and is forgotten after 5 minutes without traffic. One datagram is
//! one message: the framing setting does not apply.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::net::UdpSocket;
use tokio::sync::Semaphore;
use zorvik_engine::framing::LineEnding;
use zorvik_engine::udp::MAX_DATAGRAM;
use zorvik_formats::{ReplyMode, SocketServerConfig};

use crate::report::{Reporter, TrafficDirection};
use crate::rules::Replier;
use crate::{Control, Ctx, message_bytes};

/// Senders without traffic for this long are forgotten.
const IDLE: Duration = Duration::from_secs(5 * 60);
/// Senders remembered at once; the least recently active is forgotten first.
const MAX_PEERS: usize = 10_000;
/// Senders forgotten at once when there are too many.
const EVICT_BATCH: usize = MAX_PEERS / 100;
/// Rule replies waiting for their delay; more are dropped (a flood cannot pile up tasks).
const MAX_DELAYED: usize = 1_000;

struct Peer {
    conn: u64,
    last_seen: Instant,
}

/// Known senders by address and by connection id. Dropping it (the server
/// stopped) reports them all closed.
struct Peers {
    by_addr: HashMap<SocketAddr, Peer>,
    by_conn: HashMap<u64, SocketAddr>,
    reporter: Reporter,
}

impl Peers {
    fn new(reporter: Reporter) -> Self {
        Self { by_addr: HashMap::new(), by_conn: HashMap::new(), reporter }
    }

    /// The connection id of `addr`, registering it on its first datagram.
    fn touch(&mut self, addr: SocketAddr) -> u64 {
        let now = Instant::now();
        if let Some(peer) = self.by_addr.get_mut(&addr) {
            peer.last_seen = now;
            return peer.conn;
        }
        if self.by_addr.len() >= MAX_PEERS {
            // The least active ones go in a batch: a flood from new senders must
            // not scan every known sender on each datagram.
            let mut all: Vec<(Instant, SocketAddr)> = self.by_addr.iter().map(|(a, p)| (p.last_seen, *a)).collect();
            let n = EVICT_BATCH.min(all.len());
            all.select_nth_unstable(n - 1);
            for (_, oldest) in all.drain(..n) {
                self.forget(oldest, "too many clients, forgot the least active ones");
            }
        }
        let conn = self.reporter.next_conn();
        self.by_addr.insert(addr, Peer { conn, last_seen: now });
        self.by_conn.insert(conn, addr);
        self.reporter.opened(conn, &addr);
        conn
    }

    fn forget(&mut self, addr: SocketAddr, reason: &str) {
        if let Some(peer) = self.by_addr.remove(&addr) {
            self.by_conn.remove(&peer.conn);
            self.reporter.closed(peer.conn, &addr, reason);
        }
    }

    fn forget_idle(&mut self) {
        let now = Instant::now();
        let idle: Vec<SocketAddr> =
            self.by_addr.iter().filter(|(_, p)| now.duration_since(p.last_seen) >= IDLE).map(|(a, _)| *a).collect();
        for addr in idle {
            self.forget(addr, "idle for 5 minutes");
        }
    }
}

impl Drop for Peers {
    fn drop(&mut self) {
        let mut all: Vec<(u64, SocketAddr)> = self.by_conn.drain().collect();
        all.sort_unstable();
        for (conn, addr) in all {
            self.reporter.closed(conn, &addr, "server stopped");
        }
    }
}

/// ICMP "port unreachable" for an earlier reply shows up as an error on the
/// next receive (Windows: WSAECONNRESET). It is about that reply, not the socket.
pub(crate) fn is_unreachable(e: &std::io::Error) -> bool {
    use std::io::ErrorKind as K;
    matches!(e.kind(), K::ConnectionReset | K::ConnectionRefused | K::HostUnreachable | K::NetworkUnreachable)
}

pub(crate) async fn run(socket: UdpSocket, mut ctx: Ctx) -> Result<(), String> {
    let socket = Arc::new(socket);
    let reporter = ctx.reporter.clone();
    let delayed = Arc::new(Semaphore::new(MAX_DELAYED));
    let mut peers = Peers::new(reporter.clone());
    let mut current = ctx.live.borrow_and_update().clone();
    let mut replier = compile_rules(&current.server.socket, &reporter);
    // Take the latest configuration; rules are compiled (and their problems reported) when they change.
    macro_rules! refresh {
        () => {{
            let latest = ctx.live.borrow_and_update().clone();
            let (old, new) = (&current.server.socket, &latest.server.socket);
            if new.rules != old.rules || new.encoding != old.encoding {
                replier = compile_rules(new, &reporter);
            }
            current = latest;
        }};
    }
    let mut buf = vec![0u8; 65_536];
    let mut sweep = tokio::time::interval(Duration::from_secs(15));
    sweep.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            received = socket.recv_from(&mut buf) => {
                let (n, peer) = match received {
                    Ok(r) => r,
                    Err(e) if is_unreachable(&e) => continue,
                    Err(e) => {
                        // Report and back off instead of spinning on a persistent error.
                        reporter.error(None, None, format!("Receive failed: {e}"));
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        continue;
                    }
                };
                if ctx.live.has_changed().unwrap_or(false) {
                    refresh!();
                }
                let conn = peers.touch(peer);
                let message = &buf[..n];
                reporter.data(Some(conn), Some(&peer), TrafficDirection::In, message, "");
                let config = &current.server.socket;
                match config.mode {
                    ReplyMode::Echo => {
                        let _ = send(&socket, &reporter, conn, peer, message).await;
                    }
                    ReplyMode::Rules => {
                        let Some(reply) = replier.reply(message, &current.vars) else { continue };
                        let bytes = with_line_ending(reply.bytes, reply.is_text, config.line_ending);
                        if reply.delay_ms == 0 {
                            let _ = send(&socket, &reporter, conn, peer, &bytes).await;
                            continue;
                        }
                        let Ok(permit) = delayed.clone().try_acquire_owned() else {
                            let why = "Too many delayed replies waiting: this one was dropped";
                            reporter.error(Some(conn), Some(&peer), why);
                            continue;
                        };
                        let (socket, reporter, cancel) = (socket.clone(), reporter.clone(), ctx.cancel.clone());
                        let delay = Duration::from_millis(reply.delay_ms);
                        tokio::spawn(async move {
                            let _permit = permit;
                            tokio::select! {
                                _ = tokio::time::sleep(delay) => {
                                    let _ = send(&socket, &reporter, conn, peer, &bytes).await;
                                }
                                _ = cancel.cancelled() => {}
                            }
                        });
                    }
                    ReplyMode::Manual | ReplyMode::Discard => {}
                }
            }
            Some(control) = ctx.control.recv() => match control {
                Control::Send { conn, message, reply } => {
                    let line_ending = ctx.live.borrow().server.socket.line_ending;
                    let result = match message_bytes(&message) {
                        Ok((bytes, is_text)) => {
                            let bytes = with_line_ending(bytes, is_text, line_ending);
                            send_to_peers(&socket, &reporter, &peers, conn, &bytes).await
                        }
                        Err(e) => Err(e),
                    };
                    let _ = reply.send(result);
                }
                Control::Disconnect { conn } => {
                    if let Some(addr) = peers.by_conn.get(&conn).copied() {
                        peers.forget(addr, "forgotten by you");
                    }
                }
            },
            Ok(()) = ctx.live.changed() => refresh!(),
            _ = sweep.tick() => peers.forget_idle(),
            _ = ctx.cancel.cancelled() => return Ok(()),
        }
    }
}

/// Compile the reply rules, reporting the ones that cannot be used.
fn compile_rules(config: &SocketServerConfig, reporter: &Reporter) -> Replier {
    let replier = Replier::new(&config.rules, config.encoding);
    for problem in &replier.problems {
        reporter.error(None, None, problem.clone());
    }
    replier
}

fn with_line_ending(mut bytes: Vec<u8>, is_text: bool, line_ending: LineEnding) -> Vec<u8> {
    if is_text {
        bytes.extend_from_slice(line_ending.as_str().as_bytes());
    }
    bytes
}

/// Send one datagram and report it (or why it could not be sent).
async fn send(
    socket: &UdpSocket,
    reporter: &Reporter,
    conn: u64,
    peer: SocketAddr,
    bytes: &[u8],
) -> Result<(), String> {
    let result = if bytes.len() > MAX_DATAGRAM {
        Err(format!("The message is {} bytes; a UDP datagram holds at most {MAX_DATAGRAM} bytes", bytes.len()))
    } else {
        socket.send_to(bytes, peer).await.map(|_| ()).map_err(|e| format!("Could not send to {peer}: {e}"))
    };
    match &result {
        Ok(()) => reporter.data(Some(conn), Some(&peer), TrafficDirection::Out, bytes, ""),
        Err(e) => reporter.error(Some(conn), Some(&peer), e.clone()),
    }
    result
}

/// Send from the UI to one sender or all known ones; returns how many got it.
async fn send_to_peers(
    socket: &UdpSocket,
    reporter: &Reporter,
    peers: &Peers,
    conn: Option<u64>,
    bytes: &[u8],
) -> Result<usize, String> {
    let targets: Vec<(u64, SocketAddr)> = match conn {
        Some(id) => {
            let addr = peers.by_conn.get(&id).ok_or_else(|| format!("Client #{id} is no longer known"))?;
            vec![(id, *addr)]
        }
        None => peers.by_conn.iter().map(|(c, a)| (*c, *a)).collect(),
    };
    if targets.is_empty() {
        return Err("No client has sent a datagram yet".into());
    }
    let mut sent = 0;
    let mut last_error = None;
    for (conn, addr) in targets {
        match send(socket, reporter, conn, addr, bytes).await {
            Ok(()) => sent += 1,
            Err(e) => last_error = Some(e),
        }
    }
    match (sent, last_error) {
        (0, Some(e)) => Err(e),
        _ => Ok(sent),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn too_many_senders_forget_the_least_active_in_a_batch() {
        let mut peers = Peers::new(Reporter::new(|_| {}));
        let addr = |i: usize| SocketAddr::from(([10, 0, (i >> 8) as u8, i as u8], 1000 + (i % 7) as u16));
        for i in 0..MAX_PEERS {
            peers.touch(addr(i));
        }
        // The first sender is the most active one now.
        peers.by_addr.get_mut(&addr(0)).unwrap().last_seen += Duration::from_secs(60);
        let newcomer = peers.touch(SocketAddr::from(([192, 0, 2, 1], 9)));
        assert_eq!(peers.by_addr.len(), MAX_PEERS - EVICT_BATCH + 1);
        assert_eq!(peers.by_conn.len(), peers.by_addr.len());
        assert!(peers.by_conn.contains_key(&newcomer) && peers.by_addr.contains_key(&addr(0)));
    }
}

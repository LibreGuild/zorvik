//! What running servers report: one [`TrafficEntry`] per connection, message,
//! HTTP exchange or DNS query, plus counters. The API keeps a bounded log of
//! the entries and forwards them to the UI.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use zorvik_engine::Header;
use zorvik_engine::framing::display_payload;

/// Payload bytes kept per log entry; the size still shows the full length.
pub const MAX_ENTRY_PAYLOAD: usize = 64 * 1024;
/// Messages, requests and errors about clients logged per second (connections
/// and the server's own notes always are).
/// Faster traffic is still counted, but only this much of it reaches the log,
/// so a flood cannot swamp the app with entries.
const MAX_ENTRIES_PER_SEC: u32 = 1000;
/// Payload bytes (message payloads, HTTP bodies, DNS answers) logged per second.
const MAX_PAYLOAD_PER_SEC: usize = 8 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum TrafficKind {
    /// A client connected.
    Open,
    /// A client disconnected.
    Close,
    /// Bytes or a message (TCP, UDP, WebSocket, SSE, relay).
    Data,
    /// An HTTP request and the mock's answer.
    Http,
    /// A DNS query and the answer.
    Dns,
    Info,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum TrafficDirection {
    /// From the client to the server.
    In,
    /// From the server to the client.
    Out,
    /// Relay: from the relay to the target.
    ToTarget,
    /// Relay: from the target back to the relay.
    FromTarget,
}

/// An HTTP request handled by a mock server.
#[derive(Debug, Clone, Default, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HttpExchange {
    pub method: String,
    /// Path and query as requested.
    pub path: String,
    pub http_version: String,
    pub request_headers: Vec<Header>,
    /// UTF-8 request body (lossy), capped.
    pub request_body: String,
    pub status: u16,
    pub response_headers: Vec<Header>,
    pub response_body: String,
    pub duration_ms: f64,
    /// Name or path of the route that answered; `None` for the fallback.
    pub route: Option<String>,
    /// Fault injected instead of the normal answer (`error`, `reset`, `hang`) or `proxy`.
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TrafficEntry {
    #[ts(type = "number")]
    pub id: u64,
    /// Unix epoch milliseconds.
    pub timestamp: f64,
    pub kind: TrafficKind,
    /// Connection id (per server), when the entry belongs to one.
    #[ts(type = "number | null")]
    pub conn: Option<u64>,
    /// Client address.
    pub peer: Option<String>,
    pub direction: Option<TrafficDirection>,
    /// One line for the log.
    pub summary: String,
    /// Payload as UTF-8 when it decodes (or details, e.g. a DNS answer).
    pub text: Option<String>,
    /// Payload as base64 when it is not UTF-8.
    pub base64: Option<String>,
    /// Payload size in bytes.
    #[ts(type = "number")]
    pub size: u64,
    /// Only the first 64 KB of the payload is kept.
    pub truncated: bool,
    pub http: Option<HttpExchange>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ServerStats {
    #[ts(type = "number")]
    pub connections_open: u64,
    #[ts(type = "number")]
    pub connections_total: u64,
    /// HTTP requests, DNS queries, UDP datagrams, WebSocket/SSE/TCP messages received.
    #[ts(type = "number")]
    pub requests: u64,
    #[ts(type = "number")]
    pub bytes_in: u64,
    #[ts(type = "number")]
    pub bytes_out: u64,
    #[ts(type = "number")]
    pub errors: u64,
}

/// Events of one running server.
#[allow(clippy::large_enum_variant)] // almost every event is traffic
#[derive(Debug, Clone, Serialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(export)]
pub enum ServerEvent {
    #[serde(rename_all = "camelCase")]
    Traffic { entry: TrafficEntry },
    #[serde(rename_all = "camelCase")]
    Stats { stats: ServerStats },
    /// The server stopped (`error` when it failed rather than being stopped).
    #[serde(rename_all = "camelCase")]
    Stopped { error: Option<String> },
}

/// Sent from the UI to a running server (manual mode, or broadcast).
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(export)]
pub enum OutgoingMessage {
    Text {
        text: String,
    },
    /// Binary payload as base64.
    Binary {
        base64: String,
    },
    /// Server-Sent Event.
    #[serde(rename_all = "camelCase")]
    Event {
        #[serde(default)]
        event: String,
        data: String,
        #[serde(default)]
        id: String,
    },
}

pub(crate) fn now_ms() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0)
}

type Sink = dyn Fn(ServerEvent) + Send + Sync;

/// What this second of the log has room for.
#[derive(Default)]
struct Budget {
    started: Option<Instant>,
    entries: u32,
    bytes: usize,
    /// Entries left out since the last note about them.
    skipped: u64,
}

impl Budget {
    /// Whether an entry with `payload` bytes fits (`None`: always logged). Also
    /// returns how many were left out before, once a new second starts.
    fn take(&mut self, now: Instant, payload: Option<usize>) -> (bool, u64) {
        let mut skipped = 0;
        if self.started.is_none_or(|s| now.duration_since(s) >= Duration::from_secs(1)) {
            skipped = self.skipped;
            *self = Budget { started: Some(now), ..Default::default() };
        }
        let Some(payload) = payload else { return (true, skipped) };
        if self.entries >= MAX_ENTRIES_PER_SEC || self.bytes + payload > MAX_PAYLOAD_PER_SEC {
            self.skipped += 1;
            return (false, skipped);
        }
        self.entries += 1;
        self.bytes += payload;
        (true, skipped)
    }
}

#[derive(Default)]
struct Counters {
    open: AtomicU64,
    total: AtomicU64,
    requests: AtomicU64,
    bytes_in: AtomicU64,
    bytes_out: AtomicU64,
    errors: AtomicU64,
}

/// Collects a server's traffic and counters and hands entries to the sink.
#[derive(Clone)]
pub struct Reporter {
    sink: Arc<Sink>,
    counters: Arc<Counters>,
    budget: Arc<Mutex<Budget>>,
    next_id: Arc<AtomicU64>,
    next_conn: Arc<AtomicU64>,
}

impl Reporter {
    pub fn new(sink: impl Fn(ServerEvent) + Send + Sync + 'static) -> Self {
        Self {
            sink: Arc::new(sink),
            counters: Arc::default(),
            budget: Arc::default(),
            next_id: Arc::new(AtomicU64::new(1)),
            next_conn: Arc::new(AtomicU64::new(1)),
        }
    }

    pub fn stats(&self) -> ServerStats {
        let c = &self.counters;
        ServerStats {
            connections_open: c.open.load(Ordering::Relaxed),
            connections_total: c.total.load(Ordering::Relaxed),
            requests: c.requests.load(Ordering::Relaxed),
            bytes_in: c.bytes_in.load(Ordering::Relaxed),
            bytes_out: c.bytes_out.load(Ordering::Relaxed),
            errors: c.errors.load(Ordering::Relaxed),
        }
    }

    /// A new connection id (unique within this server).
    pub fn next_conn(&self) -> u64 {
        self.next_conn.fetch_add(1, Ordering::Relaxed)
    }

    fn emit(&self, mut entry: TrafficEntry) {
        entry.id = self.next_id.fetch_add(1, Ordering::Relaxed);
        (self.sink)(ServerEvent::Traffic { entry });
    }

    /// Whether the log takes an entry with `payload` bytes now (`None`: one that
    /// is always logged, e.g. a connection). Notes what was left out before.
    fn admit(&self, payload: Option<usize>) -> bool {
        let (fits, skipped) = self.budget.lock().unwrap_or_else(|e| e.into_inner()).take(Instant::now(), payload);
        self.note_skipped(skipped);
        fits
    }

    fn note_skipped(&self, skipped: u64) {
        if skipped > 0 {
            self.emit(Self::entry(
                TrafficKind::Info,
                None,
                None,
                format!(
                    "{skipped} entries were left out of the log: the traffic was faster than the log keeps \
                     (at most {MAX_ENTRIES_PER_SEC} entries or {} MB a second). The counters include everything.",
                    MAX_PAYLOAD_PER_SEC >> 20
                ),
            ));
        }
    }

    fn entry(kind: TrafficKind, conn: Option<u64>, peer: Option<&SocketAddr>, summary: String) -> TrafficEntry {
        TrafficEntry {
            id: 0,
            timestamp: now_ms(),
            kind,
            conn,
            peer: peer.map(|p| p.to_string()),
            direction: None,
            summary,
            text: None,
            base64: None,
            size: 0,
            truncated: false,
            http: None,
        }
    }

    pub fn opened(&self, conn: u64, peer: &SocketAddr) {
        self.counters.open.fetch_add(1, Ordering::Relaxed);
        self.counters.total.fetch_add(1, Ordering::Relaxed);
        // Always logged: the UI lists open connections from these entries.
        self.admit(None);
        self.emit(Self::entry(TrafficKind::Open, Some(conn), Some(peer), format!("#{conn} connected from {peer}")));
    }

    pub fn closed(&self, conn: u64, peer: &SocketAddr, reason: &str) {
        let _ = self.counters.open.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_sub(1));
        let summary = if reason.is_empty() { "Disconnected".to_string() } else { format!("Disconnected: {reason}") };
        self.admit(None);
        self.emit(Self::entry(TrafficKind::Close, Some(conn), Some(peer), summary));
    }

    /// Bytes or one message. `In` counts as a request; `summary` is usually empty
    /// (the UI shows the payload), or a label such as an SSE event name.
    pub fn data(
        &self,
        conn: Option<u64>,
        peer: Option<&SocketAddr>,
        direction: TrafficDirection,
        bytes: &[u8],
        summary: impl Into<String>,
    ) {
        match direction {
            TrafficDirection::In | TrafficDirection::FromTarget => {
                self.counters.bytes_in.fetch_add(bytes.len() as u64, Ordering::Relaxed);
            }
            TrafficDirection::Out | TrafficDirection::ToTarget => {
                self.counters.bytes_out.fetch_add(bytes.len() as u64, Ordering::Relaxed);
            }
        }
        if direction == TrafficDirection::In {
            self.counters.requests.fetch_add(1, Ordering::Relaxed);
        }
        let shown = &bytes[..bytes.len().min(MAX_ENTRY_PAYLOAD)];
        if !self.admit(Some(shown.len())) {
            return;
        }
        let (text, base64) = display_payload(shown);
        let mut entry = Self::entry(TrafficKind::Data, conn, peer, summary.into());
        entry.direction = Some(direction);
        entry.text = text;
        entry.base64 = base64;
        entry.size = bytes.len() as u64;
        entry.truncated = bytes.len() > shown.len();
        self.emit(entry);
    }

    /// An HTTP exchange of a mock server.
    pub fn http(
        &self,
        conn: Option<u64>,
        peer: Option<&SocketAddr>,
        exchange: HttpExchange,
        bytes_in: u64,
        bytes_out: u64,
    ) {
        self.counters.requests.fetch_add(1, Ordering::Relaxed);
        self.counters.bytes_in.fetch_add(bytes_in, Ordering::Relaxed);
        self.counters.bytes_out.fetch_add(bytes_out, Ordering::Relaxed);
        if !self.admit(Some(exchange.request_body.len() + exchange.response_body.len())) {
            return;
        }
        let mut summary = format!("{} {} → ", exchange.method, exchange.path);
        match &exchange.note {
            Some(note) if exchange.status == 0 => summary.push_str(note),
            _ => summary.push_str(&exchange.status.to_string()),
        }
        summary.push_str(&format!(" · {:.0} ms", exchange.duration_ms));
        let mut entry = Self::entry(TrafficKind::Http, conn, peer, summary);
        entry.size = bytes_in;
        entry.http = Some(exchange);
        self.emit(entry);
    }

    /// Any other request/answer pair (e.g. a DNS query): `details` is shown when expanded.
    pub fn exchange(
        &self,
        kind: TrafficKind,
        peer: Option<&SocketAddr>,
        summary: String,
        details: String,
        bytes_in: u64,
        bytes_out: u64,
    ) {
        self.counters.requests.fetch_add(1, Ordering::Relaxed);
        self.counters.bytes_in.fetch_add(bytes_in, Ordering::Relaxed);
        self.counters.bytes_out.fetch_add(bytes_out, Ordering::Relaxed);
        if !self.admit(Some(details.len())) {
            return;
        }
        let mut entry = Self::entry(kind, None, peer, summary);
        entry.size = bytes_in;
        entry.text = Some(details);
        self.emit(entry);
    }

    pub fn info(&self, summary: impl Into<String>) {
        self.admit(None);
        self.emit(Self::entry(TrafficKind::Info, None, None, summary.into()));
    }

    pub fn error(&self, conn: Option<u64>, peer: Option<&SocketAddr>, summary: impl Into<String>) {
        self.counters.errors.fetch_add(1, Ordering::Relaxed);
        // Errors about a client are sampled like its traffic; the server's own always show.
        if !self.admit(peer.map(|_| 0)) {
            return;
        }
        self.emit(Self::entry(TrafficKind::Error, conn, peer, summary.into()));
    }

    pub(crate) fn stopped(&self, error: Option<String>) {
        let skipped = std::mem::take(&mut self.budget.lock().unwrap_or_else(|e| e.into_inner()).skipped);
        self.note_skipped(skipped);
        (self.sink)(ServerEvent::Stopped { error });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn counters_and_entries() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s = seen.clone();
        let r = Reporter::new(move |e| s.lock().unwrap().push(e));
        let peer: SocketAddr = "127.0.0.1:5000".parse().unwrap();
        let conn = r.next_conn();
        r.opened(conn, &peer);
        r.data(Some(conn), Some(&peer), TrafficDirection::In, b"hi", "");
        r.data(Some(conn), Some(&peer), TrafficDirection::Out, &[0xff; 70_000], "");
        r.closed(conn, &peer, "");
        r.closed(conn, &peer, ""); // never below zero
        let stats = r.stats();
        assert_eq!((stats.connections_open, stats.connections_total, stats.requests), (0, 1, 1));
        assert_eq!((stats.bytes_in, stats.bytes_out), (2, 70_000));
        let seen = seen.lock().unwrap();
        let ServerEvent::Traffic { entry } = &seen[2] else { panic!() };
        assert!(entry.truncated && entry.base64.is_some() && entry.size == 70_000);
        let ids: Vec<u64> = seen
            .iter()
            .filter_map(|e| match e {
                ServerEvent::Traffic { entry } => Some(entry.id),
                _ => None,
            })
            .collect();
        assert_eq!(ids, [1, 2, 3, 4, 5]);
    }

    #[test]
    fn floods_are_counted_but_sampled_in_the_log() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s = seen.clone();
        let r = Reporter::new(move |e| s.lock().unwrap().push(e));
        let peer: SocketAddr = "127.0.0.1:5000".parse().unwrap();
        r.opened(1, &peer);
        for _ in 0..1500 {
            r.data(Some(1), Some(&peer), TrafficDirection::In, b"x", "");
        }
        r.closed(1, &peer, "");
        r.stopped(None);
        assert_eq!(r.stats().requests, 1500);
        let seen = seen.lock().unwrap();
        let entries: Vec<&TrafficEntry> = seen
            .iter()
            .filter_map(|e| match e {
                ServerEvent::Traffic { entry } => Some(entry),
                _ => None,
            })
            .collect();
        let count = |kind| entries.iter().filter(|e| e.kind == kind).count();
        assert_eq!((count(TrafficKind::Data), count(TrafficKind::Open), count(TrafficKind::Close)), (1000, 1, 1));
        let note = entries.last().unwrap();
        assert!(note.kind == TrafficKind::Info && note.summary.starts_with("500 entries"), "{}", note.summary);
    }

    #[test]
    fn log_budget_per_second() {
        let t0 = Instant::now();
        let mut b = Budget::default();
        assert_eq!(b.take(t0, Some(MAX_PAYLOAD_PER_SEC)), (true, 0));
        assert_eq!(b.take(t0, Some(1)), (false, 0), "no payload budget left");
        assert_eq!(b.take(t0, None), (true, 0), "connections are always logged");
        let t1 = t0 + Duration::from_secs(1);
        assert_eq!(b.take(t1, Some(1)), (true, 1), "a new second notes what was left out");
        assert_eq!(b.take(t1, Some(1)), (true, 0));
    }
}

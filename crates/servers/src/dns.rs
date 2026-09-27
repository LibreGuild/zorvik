//! DNS server: answers from the configured records (A, AAAA, CNAME, TXT, MX,
//! NS, PTR, SRV, CAA, with `*.name` wildcards) and sends every other question
//! to the upstream: nowhere (NXDOMAIN), this computer's resolver, or another
//! DNS server. Serves UDP on the socket it is given and TCP on the same port.

use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use hickory_proto::op::{Edns, Message, Metadata, Query, ResponseCode};
use hickory_proto::rr::rdata::{A, AAAA, CNAME, MX, NS, PTR, SRV, TXT};
use hickory_proto::rr::{DNSClass, Name, RData, Record, RecordType};
use hickory_proto::serialize::binary::BinEncodable;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::sync::{Semaphore, watch};
use zorvik_formats::{DnsRecord, DnsServerConfig};

use crate::report::{Reporter, TrafficKind};
use crate::udp::is_unreachable;
use crate::{Control, Ctx, Live};

/// Time the upstream (server or this computer's resolver) gets to answer.
const UPSTREAM_TIMEOUT: Duration = Duration::from_secs(3);
/// Queries answered at once; more are dropped, as a busy server would.
const MAX_IN_FLIGHT: usize = 256;
/// Open DNS-over-TCP connections at once.
const MAX_TCP_CONNS: usize = 64;
/// A TCP client that sends nothing for this long is disconnected.
const TCP_IDLE: Duration = Duration::from_secs(10);
/// CNAMEs followed within the records.
const MAX_CNAME_HOPS: usize = 8;
/// Largest UDP answer, also when a client offers more (EDNS).
const MAX_UDP_ANSWER: u16 = 4096;
/// TTL of answers from this computer's resolver.
const SYSTEM_TTL: u32 = 30;
/// Record types the server answers from its records.
const RECORD_TYPES: [&str; 9] = ["A", "AAAA", "CNAME", "TXT", "MX", "NS", "PTR", "SRV", "CAA"];

// ---- records -------------------------------------------------------------------

#[derive(Clone)]
struct Rec {
    rtype: RecordType,
    ttl: u32,
    data: RData,
}

impl Rec {
    fn record(&self, owner: &Name) -> Record {
        Record::from_rdata(owner.clone(), self.ttl, self.data.clone())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Upstream {
    /// Answer "no such name".
    None,
    /// This computer's resolver (addresses only).
    System,
    Server(SocketAddr),
    /// Set but unusable (reported): answer SERVFAIL.
    Invalid,
}

/// The records of one configuration, ready for lookups.
struct Zone {
    config: DnsServerConfig,
    /// Records by name (lowercase ASCII, no trailing dot).
    names: HashMap<String, Vec<Rec>>,
    /// `*.suffix` records by `.suffix` (empty for a bare `*`), most specific first.
    wildcards: Vec<(String, Vec<Rec>)>,
    /// Names above records (`example.test` for `api.example.test`): they exist, without records.
    parents: HashSet<String>,
    upstream: Upstream,
}

/// What the records say about a question.
enum Local {
    /// Records answering it, possibly after CNAMEs. `chase` is set when the last
    /// CNAME points outside the records (the upstream may resolve it).
    Answer { records: Vec<Record>, chase: Option<Name> },
    /// The name exists here but has no records of that type.
    NoData,
    /// Not a name in the records.
    Unknown,
}

impl Zone {
    /// Compile the records; returns the problems (skipped records, bad upstream).
    fn compile(config: &DnsServerConfig, local: SocketAddr) -> (Zone, Vec<String>) {
        let mut problems = Vec::new();
        let mut names: HashMap<String, Vec<Rec>> = HashMap::new();
        let mut wildcards: HashMap<String, Vec<Rec>> = HashMap::new();
        let mut parents = HashSet::new();
        for (i, record) in config.records.iter().enumerate().filter(|(_, r)| r.enabled) {
            match parse_record(record) {
                Ok((name, rec)) => {
                    let base = name.strip_prefix('*').unwrap_or(&name);
                    // Every name above this one exists too (without records of its own).
                    let mut rest = base.trim_start_matches('.');
                    while let Some((_, parent)) = rest.split_once('.') {
                        parents.insert(parent.to_string());
                        rest = parent;
                    }
                    match name.strip_prefix('*') {
                        Some(suffix) => {
                            if !suffix.is_empty() {
                                parents.insert(suffix.trim_start_matches('.').to_string());
                            }
                            wildcards.entry(suffix.to_string()).or_default().push(rec)
                        }
                        None => names.entry(name).or_default().push(rec),
                    }
                }
                Err(why) => {
                    let label = format!("{} {}", record.record_type.trim(), record.name.trim());
                    problems.push(format!("DNS record {} ({}) skipped: {why}", i + 1, label.trim()));
                }
            }
        }
        let mut wildcards: Vec<(String, Vec<Rec>)> = wildcards.into_iter().collect();
        wildcards.sort_by_key(|(suffix, _)| std::cmp::Reverse(suffix.matches('.').count()));
        let upstream = match parse_upstream(&config.upstream, local) {
            Ok(u) => u,
            Err(e) => {
                problems.push(e);
                Upstream::Invalid
            }
        };
        (Zone { config: config.clone(), names, wildcards, parents, upstream }, problems)
    }

    /// Records owned by `name`: its own, or those of the closest wildcard.
    /// A name with records of its own never uses a wildcard (RFC 4592).
    fn lookup(&self, name: &str) -> Option<&[Rec]> {
        if let Some(recs) = self.names.get(name) {
            return Some(recs);
        }
        self.wildcards
            .iter()
            .find(|(suffix, _)| {
                if suffix.is_empty() { !name.is_empty() } else { name.len() > suffix.len() && name.ends_with(suffix) }
            })
            .map(|(_, recs)| recs.as_slice())
    }

    fn answer(&self, qname: &Name, qtype: RecordType) -> Local {
        let mut records = Vec::new();
        let mut owner = qname.clone();
        let mut seen = HashSet::new();
        for hop in 0..=MAX_CNAME_HOPS {
            let key = name_key(&owner);
            if !seen.insert(key.clone()) {
                break; // a CNAME loop: the client sees the chain once
            }
            let Some(recs) = self.lookup(&key) else {
                if hop > 0 {
                    return Local::Answer { records, chase: Some(owner) };
                }
                return if self.parents.contains(&key) { Local::NoData } else { Local::Unknown };
            };
            let matching: Vec<Record> = recs
                .iter()
                .filter(|r| qtype == RecordType::ANY || r.rtype == qtype)
                .map(|r| r.record(&owner))
                .collect();
            if !matching.is_empty() {
                records.extend(matching);
                return Local::Answer { records, chase: None };
            }
            let Some(cname) = recs.iter().find(|r| r.rtype == RecordType::CNAME) else {
                return if hop == 0 { Local::NoData } else { Local::Answer { records, chase: None } };
            };
            records.push(cname.record(&owner));
            let RData::CNAME(CNAME(target)) = &cname.data else { break };
            owner = target.clone();
        }
        Local::Answer { records, chase: None }
    }
}

/// Lowercase ASCII form of a name without the trailing dot (the lookup key).
fn name_key(name: &Name) -> String {
    name.to_ascii().trim_end_matches('.').to_ascii_lowercase()
}

/// A name as typed (international names become `xn--…`), checked for DNS limits.
fn parse_name(raw: &str) -> Result<Name, String> {
    let raw = raw.trim();
    if raw == "." {
        return Ok(Name::root());
    }
    let trimmed = raw.trim_end_matches('.');
    if trimmed.is_empty() {
        return Err("the name is empty".into());
    }
    if trimmed.contains(char::is_whitespace) {
        return Err(format!("'{trimmed}' is not a valid name (it contains spaces)"));
    }
    let mut name = Name::from_str_relaxed(trimmed).map_err(|e| format!("'{trimmed}' is not a valid name: {e}"))?;
    name.set_fqdn(true);
    Ok(name)
}

/// The lookup key of a record's name: `*.suffix` for wildcards, and for PTR
/// records an IP address becomes its `in-addr.arpa`/`ip6.arpa` name.
fn record_key(raw: &str, rtype: RecordType) -> Result<String, String> {
    let raw = raw.trim();
    if rtype == RecordType::PTR
        && let Ok(ip) = raw.parse::<IpAddr>()
    {
        return Ok(name_key(&Name::from(ip)));
    }
    if raw == "*" {
        return Ok("*".into());
    }
    if let Some(suffix) = raw.strip_prefix("*.") {
        return Ok(format!("*.{}", name_key(&parse_name(suffix)?)));
    }
    if raw.contains('*') {
        return Err("a wildcard must be the first label, e.g. *.example.test".into());
    }
    Ok(name_key(&parse_name(raw)?))
}

fn parse_record(record: &DnsRecord) -> Result<(String, Rec), String> {
    let type_name = record.record_type.trim().to_ascii_uppercase();
    if !RECORD_TYPES.contains(&type_name.as_str()) {
        return Err(format!("type '{}' is not supported (use {})", record.record_type.trim(), RECORD_TYPES.join(", ")));
    }
    let rtype: RecordType = type_name.parse().map_err(|_| format!("unknown type '{type_name}'"))?;
    let key = record_key(&record.name, rtype)?;
    let data = parse_value(rtype, record.value.trim())?;
    // Catch what only shows when encoding (a name or record too long).
    Record::from_rdata(Name::root(), record.ttl, data.clone())
        .to_bytes()
        .map_err(|e| format!("the value cannot be sent: {e}"))?;
    Ok((key, Rec { rtype, ttl: record.ttl, data }))
}

fn parse_value(rtype: RecordType, value: &str) -> Result<RData, String> {
    let fields: Vec<&str> = value.split_whitespace().collect();
    let number =
        |s: &str, what: &str| s.parse::<u16>().map_err(|_| format!("{what} '{s}' is not a number from 0 to 65535"));
    Ok(match rtype {
        RecordType::A => {
            RData::A(A(value.parse().map_err(|_| format!("'{value}' is not an IPv4 address (e.g. 127.0.0.1)"))?))
        }
        RecordType::AAAA => {
            RData::AAAA(AAAA(value.parse().map_err(|_| format!("'{value}' is not an IPv6 address (e.g. ::1)"))?))
        }
        RecordType::CNAME => RData::CNAME(CNAME(parse_name(value)?)),
        RecordType::NS => RData::NS(NS(parse_name(value)?)),
        RecordType::PTR => RData::PTR(PTR(parse_name(value)?)),
        RecordType::TXT => {
            let strings = txt_strings(value)?;
            RData::TXT(TXT::from_bytes(strings.iter().map(Vec::as_slice).collect()))
        }
        RecordType::MX => match fields.as_slice() {
            [preference, host] => RData::MX(MX::new(number(preference, "preference")?, parse_name(host)?)),
            _ => return Err("expected “preference host”, e.g. 10 mail.example.test".into()),
        },
        RecordType::SRV => match fields.as_slice() {
            [priority, weight, port, target] => RData::SRV(SRV::new(
                number(priority, "priority")?,
                number(weight, "weight")?,
                number(port, "port")?,
                parse_name(target)?,
            )),
            _ => return Err("expected “priority weight port target”, e.g. 10 5 5060 sip.example.test".into()),
        },
        RecordType::CAA => caa(value)?,
        other => return Err(format!("type {other} is not supported")),
    })
}

/// TXT data: one plain string, or quoted strings (`"v=spf1 -all" "second"`).
/// Strings longer than 255 bytes (the DNS limit) are split.
fn txt_strings(value: &str) -> Result<Vec<Vec<u8>>, String> {
    let strings = if value.starts_with('"') {
        let mut out = Vec::new();
        let mut chars = value.chars().peekable();
        loop {
            while chars.next_if(|c| c.is_whitespace()).is_some() {}
            match chars.next() {
                None => break,
                Some('"') => {}
                Some(c) => return Err(format!("unexpected '{c}' outside quotes (quote every string or none)")),
            }
            let mut s = String::new();
            loop {
                match chars.next() {
                    Some('"') => break,
                    Some('\\') => s.push(chars.next().ok_or("the value ends with a lone backslash")?),
                    Some(c) => s.push(c),
                    None => return Err("a quote is not closed".into()),
                }
            }
            out.push(s.into_bytes());
        }
        out
    } else {
        vec![value.as_bytes().to_vec()]
    };
    Ok(strings
        .into_iter()
        .flat_map(|s| if s.is_empty() { vec![s] } else { s.chunks(255).map(<[u8]>::to_vec).collect() })
        .collect())
}

/// CAA: `flags tag value`, e.g. `0 issue letsencrypt.org` (the value may be quoted).
fn caa(value: &str) -> Result<RData, String> {
    let usage = "expected “flags tag value”, e.g. 0 issue letsencrypt.org";
    let (flags, rest) = value.trim().split_once(char::is_whitespace).ok_or(usage)?;
    let (tag, rest) = rest.trim_start().split_once(char::is_whitespace).ok_or(usage)?;
    let flags: u8 = flags.parse().map_err(|_| format!("flags '{flags}' is not a number from 0 to 255 ({usage})"))?;
    if tag.is_empty() || tag.len() > 15 || !tag.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(format!("tag '{tag}' must be 1 to 15 letters or digits (issue, issuewild, iodef)"));
    }
    let rest = rest.trim();
    let inner = match rest.strip_prefix('"') {
        Some(quoted) => quoted.strip_suffix('"').ok_or("a quote is not closed")?,
        None => rest,
    };
    let escaped = inner.replace('\\', "\\\\").replace('"', "\\\"");
    RData::try_from_str(RecordType::CAA, &format!("{flags} {tag} \"{escaped}\""))
        .map_err(|e| format!("invalid CAA value: {e}"))
}

fn parse_upstream(raw: &str, local: SocketAddr) -> Result<Upstream, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(Upstream::None);
    }
    if raw.eq_ignore_ascii_case("system") {
        return Ok(Upstream::System);
    }
    let addr = match raw.parse::<SocketAddr>() {
        Ok(addr) => addr,
        Err(_) => match raw.trim_start_matches('[').trim_end_matches(']').parse::<IpAddr>() {
            Ok(ip) => SocketAddr::new(ip, 53),
            Err(_) => {
                return Err(format!(
                    "Upstream '{raw}' is not a DNS server address (e.g. 1.1.1.1, 1.1.1.1:53 or [2606:4700::1111]:53)"
                ));
            }
        },
    };
    let is_self = addr.port() == local.port()
        && (addr.ip() == local.ip()
            || (local.ip().is_unspecified() && (addr.ip().is_loopback() || addr.ip().is_unspecified())));
    if is_self {
        return Err(format!("Upstream {addr} is this DNS server itself: pick another server"));
    }
    Ok(Upstream::Server(addr))
}

/// The compiled records of the latest configuration. Recompiled (and its
/// problems reported) when the DNS settings change.
struct Zones {
    state: Mutex<(watch::Receiver<Arc<Live>>, Arc<Zone>)>,
    local: SocketAddr,
    reporter: Reporter,
}

impl Zones {
    fn new(mut live: watch::Receiver<Arc<Live>>, local: SocketAddr, reporter: Reporter) -> Self {
        let config = live.borrow_and_update().server.dns.clone();
        let zone = Arc::new(Self::build(&config, local, &reporter));
        Self { state: Mutex::new((live, zone)), local, reporter }
    }

    fn build(config: &DnsServerConfig, local: SocketAddr, reporter: &Reporter) -> Zone {
        let (zone, problems) = Zone::compile(config, local);
        for problem in problems {
            reporter.error(None, None, problem);
        }
        zone
    }

    fn current(&self) -> Arc<Zone> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let (live, zone) = &mut *state;
        if live.has_changed().unwrap_or(false) {
            let latest = live.borrow_and_update().clone();
            if latest.server.dns != zone.config {
                *zone = Arc::new(Self::build(&latest.server.dns, self.local, &self.reporter));
            }
        }
        zone.clone()
    }
}

// ---- serving ---------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Transport {
    Udp,
    Tcp,
}

struct Shared {
    zones: Zones,
    reporter: Reporter,
}

pub(crate) async fn run(socket: UdpSocket, mut ctx: Ctx) -> Result<(), String> {
    let local = socket.local_addr().map_err(|e| e.to_string())?;
    let socket = Arc::new(socket);
    let reporter = ctx.reporter.clone();
    let shared =
        Arc::new(Shared { zones: Zones::new(ctx.live.clone(), local, reporter.clone()), reporter: reporter.clone() });
    let tcp = match TcpListener::bind(local).await {
        Ok(listener) => Some(listener),
        Err(e) => {
            reporter
                .info(format!("DNS over TCP is off (could not listen on TCP port {}: {e}); UDP works", local.port()));
            None
        }
    };
    let in_flight = Arc::new(Semaphore::new(MAX_IN_FLIGHT));
    let tcp_conns = Arc::new(Semaphore::new(MAX_TCP_CONNS));
    let mut buf = vec![0u8; 65_536];
    loop {
        tokio::select! {
            received = socket.recv_from(&mut buf) => {
                let (n, peer) = match received {
                    Ok(r) => r,
                    Err(e) if is_unreachable(&e) => continue,
                    Err(e) => {
                        reporter.error(None, None, format!("Receive failed: {e}"));
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        continue;
                    }
                };
                let Ok(permit) = in_flight.clone().try_acquire_owned() else {
                    reporter.error(None, Some(&peer), "Too many queries at once: one was dropped");
                    continue;
                };
                let query = buf[..n].to_vec();
                let (shared, socket, cancel) = (shared.clone(), socket.clone(), ctx.cancel.clone());
                tokio::spawn(async move {
                    let _permit = permit;
                    tokio::select! {
                        _ = serve_datagram(&shared, &socket, &query, peer) => {}
                        _ = cancel.cancelled() => {}
                    }
                });
            }
            accepted = accept(&tcp), if tcp.is_some() => match accepted {
                Ok((stream, peer)) => {
                    let Ok(permit) = tcp_conns.clone().try_acquire_owned() else {
                        reporter.error(None, Some(&peer), "Too many DNS-over-TCP connections: one was refused");
                        continue;
                    };
                    let (shared, cancel) = (shared.clone(), ctx.cancel.clone());
                    tokio::spawn(async move {
                        let _permit = permit;
                        tokio::select! {
                            _ = serve_tcp(&shared, stream, peer) => {}
                            _ = cancel.cancelled() => {}
                        }
                    });
                }
                Err(e) => {
                    reporter.error(None, None, format!("Accept failed: {e}"));
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            },
            Some(control) = ctx.control.recv() => {
                if let Control::Send { reply, .. } = control {
                    let _ = reply.send(Err("A DNS server has no connections to send to".into()));
                }
            }
            // Report problems of changed records right away, not at the next query.
            Ok(()) = ctx.live.changed() => {
                shared.zones.current();
            }
            _ = ctx.cancel.cancelled() => return Ok(()),
        }
    }
}

async fn accept(listener: &Option<TcpListener>) -> std::io::Result<(TcpStream, SocketAddr)> {
    match listener {
        Some(listener) => listener.accept().await,
        None => std::future::pending().await,
    }
}

async fn serve_datagram(shared: &Shared, socket: &UdpSocket, query: &[u8], peer: SocketAddr) {
    let Some(handled) = handle(shared, query, peer, Transport::Udp).await else { return };
    if let Err(e) = socket.send_to(&handled.response, peer).await {
        shared.reporter.error(None, Some(&peer), format!("Could not send the answer to {peer}: {e}"));
    }
    handled.report(&shared.reporter, peer, query.len());
}

async fn serve_tcp(shared: &Shared, mut stream: TcpStream, peer: SocketAddr) {
    let _ = stream.set_nodelay(true);
    loop {
        // Each message is preceded by its length (2 bytes, big-endian).
        let len = match tokio::time::timeout(TCP_IDLE, stream.read_u16()).await {
            Ok(Ok(len)) => usize::from(len),
            _ => return,
        };
        let mut query = vec![0u8; len];
        match tokio::time::timeout(TCP_IDLE, stream.read_exact(&mut query)).await {
            Ok(Ok(_)) => {}
            _ => return,
        }
        let Some(handled) = handle(shared, &query, peer, Transport::Tcp).await else { return };
        let mut framed = Vec::with_capacity(handled.response.len() + 2);
        // `encode` keeps answers within 65,535 bytes.
        framed.extend_from_slice(&(handled.response.len() as u16).to_be_bytes());
        framed.extend_from_slice(&handled.response);
        // A client that does not read its answers must not keep one of the few TCP slots.
        let sent = match tokio::time::timeout(TCP_IDLE, stream.write_all(&framed)).await {
            Ok(result) => result.map_err(|e| e.to_string()),
            Err(_) => Err("the client did not read it".into()),
        };
        if let Err(e) = sent {
            shared.reporter.error(None, Some(&peer), format!("Could not send the answer to {peer}: {e}"));
            return;
        }
        handled.report(&shared.reporter, peer, query.len());
    }
}

/// An answer to send and what the log shows about it.
struct Handled {
    response: Vec<u8>,
    summary: String,
    details: String,
}

impl Handled {
    fn report(&self, reporter: &Reporter, peer: SocketAddr, bytes_in: usize) {
        reporter.exchange(
            TrafficKind::Dns,
            Some(&peer),
            self.summary.clone(),
            self.details.clone(),
            bytes_in as u64,
            self.response.len() as u64,
        );
    }
}

/// Header-only answer for queries that cannot be parsed.
fn error_header(query: &[u8], rcode: u8) -> Vec<u8> {
    let mut header = vec![0u8; 12];
    header[..2].copy_from_slice(&query[..2]);
    // QR, the query's opcode and RD.
    header[2] = 0x80 | (query[2] & 0x79);
    header[3] = rcode & 0x0f;
    header
}

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([bytes[at], bytes[at + 1]])
}

/// Answer one query. `None` when the packet is dropped (reported as an error).
async fn handle(shared: &Shared, query: &[u8], peer: SocketAddr, transport: Transport) -> Option<Handled> {
    let reporter = &shared.reporter;
    let via = if transport == Transport::Tcp { " · TCP" } else { "" };
    if query.len() < 12 {
        reporter.error(None, Some(&peer), format!("Dropped a {}-byte packet: too short for a DNS query", query.len()));
        return None;
    }
    if query[2] & 0x80 != 0 {
        reporter.error(None, Some(&peer), "Dropped a DNS response sent to the server (only queries are answered)");
        return None;
    }
    let opcode = (query[2] >> 3) & 0x0f;
    if opcode != 0 {
        return Some(Handled {
            response: error_header(query, 4),
            summary: format!("Opcode {opcode} → NOTIMP{via}"),
            details: format!(";; Only standard queries are answered (this one has opcode {opcode})."),
        });
    }
    // A standard query asks one question and carries few records (e.g. EDNS);
    // anything else is refused before parsing.
    let questions = u16_at(query, 4);
    let others = usize::from(u16_at(query, 6)) + usize::from(u16_at(query, 8)) + usize::from(u16_at(query, 10));
    let request = if questions != 1 || others > 32 {
        Err(format!("{questions} questions and {others} records (expected 1 question)"))
    } else {
        Message::from_vec(query).map_err(|e| e.to_string())
    };
    let request = match request {
        Ok(r) => r,
        Err(why) => {
            return Some(Handled {
                response: error_header(query, 1),
                summary: format!("Malformed query → FORMERR{via}"),
                details: format!(";; The query could not be read: {why}"),
            });
        }
    };
    let question = request.queries[0].clone();
    let mut response = Message::response(request.metadata.id, request.metadata.op_code);
    response.metadata = Metadata::response_from_request(&request.metadata);
    response.add_query(question.clone());
    if let Some(edns) = &request.edns {
        let mut ours = Edns::new();
        ours.set_max_payload(MAX_UDP_ANSWER);
        response.set_edns(ours);
        if edns.version() > 0 {
            response.metadata.response_code = ResponseCode::BADVERS;
            return Some(finish(response, &question, transport, &request, "BADVERS (EDNS version)".into(), via));
        }
    }
    let zone = shared.zones.current();
    response.metadata.recursion_available = matches!(zone.upstream, Upstream::System | Upstream::Server(_));
    let qtype = question.query_type();
    let local = match question.query_class() {
        DNSClass::IN | DNSClass::ANY => zone.answer(question.name(), qtype),
        _ => Local::Unknown,
    };
    let outcome = match local {
        Local::Answer { records, chase } => {
            response.metadata.authoritative = true;
            response.answers = records;
            if let Some(target) = chase {
                let extra = resolve_elsewhere(zone.upstream, &target, qtype).await;
                if !extra.is_empty() {
                    response.metadata.authoritative = false;
                    response.answers.extend(extra);
                }
            }
            answers_text(&response.answers, qtype)
        }
        Local::NoData | Local::Unknown => {
            let known = matches!(local, Local::NoData);
            match zone.upstream {
                Upstream::None => {
                    response.metadata.authoritative = true;
                    if known {
                        format!("NOERROR, no {qtype} records")
                    } else {
                        response.metadata.response_code = ResponseCode::NXDomain;
                        "NXDOMAIN".into()
                    }
                }
                Upstream::Invalid => {
                    response.metadata.response_code = ResponseCode::ServFail;
                    "SERVFAIL (the upstream setting is invalid)".into()
                }
                Upstream::System => system_answer(&mut response, &question, known).await,
                Upstream::Server(server) => {
                    return Some(forward_answer(query, &request, &question, server, transport, via).await);
                }
            }
        }
    };
    Some(finish(response, &question, transport, &request, outcome, via))
}

/// Encode an answer (truncated to fit UDP) and describe it.
fn finish(
    response: Message,
    question: &Query,
    transport: Transport,
    request: &Message,
    outcome: String,
    via: &str,
) -> Handled {
    let (response, bytes) = encode(response, size_limit(request, transport));
    let tc = if response.metadata.truncation { " (truncated: retry over TCP)" } else { "" };
    Handled {
        summary: format!("{} {} → {outcome}{tc}{via}", question.query_type(), display_name(question.name())),
        details: describe(&response, transport),
        response: bytes,
    }
}

/// Largest answer the client accepts: over UDP 512 bytes unless it offers more
/// (EDNS), over TCP what the 2-byte length allows.
fn size_limit(request: &Message, transport: Transport) -> usize {
    match transport {
        Transport::Tcp => usize::from(u16::MAX),
        Transport::Udp => usize::from(request.max_payload().clamp(512, MAX_UDP_ANSWER)),
    }
}

/// Encode `response`; when it is larger than `limit`, send only the question
/// with the TC flag so the client retries over TCP.
fn encode(mut response: Message, limit: usize) -> (Message, Vec<u8>) {
    match response.to_vec() {
        Ok(bytes) if bytes.len() <= limit => return (response, bytes),
        Ok(_) => {
            response.answers.clear();
            response.authorities.clear();
            response.additionals.clear();
            response.metadata.truncation = true;
        }
        Err(_) => {
            response.answers.clear();
            response.authorities.clear();
            response.additionals.clear();
            response.metadata.response_code = ResponseCode::ServFail;
        }
    }
    let bytes = response.to_vec().unwrap_or_else(|_| {
        let mut header = vec![0u8; 12];
        header[..2].copy_from_slice(&response.metadata.id.to_be_bytes());
        header[2] = 0x80;
        header[3] = 2;
        header
    });
    (response, bytes)
}

/// Addresses from this computer's resolver (it only resolves addresses).
async fn system_answer(response: &mut Message, question: &Query, known: bool) -> String {
    let qtype = question.query_type();
    if !matches!(qtype, RecordType::A | RecordType::AAAA | RecordType::ANY) {
        if known {
            response.metadata.authoritative = true;
            return format!("NOERROR, no {qtype} records");
        }
        response.metadata.response_code = ResponseCode::NotImp;
        return format!("NOTIMP (this computer's resolver only looks up addresses, not {qtype})");
    }
    match system_lookup(question.name()).await {
        Ok(ips) => {
            response.answers = address_records(question.name(), &ips, qtype);
            let answers = answers_text(&response.answers, qtype);
            format!("{answers} · this computer's resolver")
        }
        Err(code) => {
            response.metadata.response_code = code;
            format!("{} · this computer's resolver", rcode_name(code))
        }
    }
}

async fn system_lookup(name: &Name) -> Result<Vec<IpAddr>, ResponseCode> {
    let host = name.to_ascii().trim_end_matches('.').to_string();
    if host.is_empty() {
        return Err(ResponseCode::NXDomain);
    }
    match tokio::time::timeout(UPSTREAM_TIMEOUT, tokio::net::lookup_host((host.as_str(), 0))).await {
        Err(_) => Err(ResponseCode::ServFail),
        // "Temporary failure in name resolution" is not an answer about the name.
        Ok(Err(e)) if e.to_string().to_ascii_lowercase().contains("temporar") => Err(ResponseCode::ServFail),
        Ok(Err(_)) => Err(ResponseCode::NXDomain),
        Ok(Ok(addrs)) => {
            let mut ips: Vec<IpAddr> = Vec::new();
            for addr in addrs {
                if !ips.contains(&addr.ip()) {
                    ips.push(addr.ip());
                }
            }
            Ok(ips)
        }
    }
}

fn address_records(owner: &Name, ips: &[IpAddr], qtype: RecordType) -> Vec<Record> {
    ips.iter()
        .filter_map(|ip| match ip {
            IpAddr::V4(v4) if qtype != RecordType::AAAA => Some(RData::A(A(*v4))),
            IpAddr::V6(v6) if qtype != RecordType::A => Some(RData::AAAA(AAAA(*v6))),
            _ => None,
        })
        .map(|data| Record::from_rdata(owner.clone(), SYSTEM_TTL, data))
        .collect()
}

/// Records for a CNAME target outside the records, from the upstream.
async fn resolve_elsewhere(upstream: Upstream, target: &Name, qtype: RecordType) -> Vec<Record> {
    match upstream {
        Upstream::System if matches!(qtype, RecordType::A | RecordType::AAAA | RecordType::ANY) => {
            system_lookup(target).await.map(|ips| address_records(target, &ips, qtype)).unwrap_or_default()
        }
        Upstream::Server(server) => {
            let mut query = Message::query();
            query.metadata.recursion_desired = true;
            query.add_query(Query::query(target.clone(), qtype));
            let Ok(bytes) = query.to_vec() else { return Vec::new() };
            match forward_udp(&bytes, server).await.map(|b| Message::from_vec(&b)) {
                Ok(Ok(answer)) => answer.answers,
                _ => Vec::new(),
            }
        }
        _ => Vec::new(),
    }
}

/// Relay the query to the upstream server and its answer back to the client.
async fn forward_answer(
    query: &[u8],
    request: &Message,
    question: &Query,
    server: SocketAddr,
    transport: Transport,
    via: &str,
) -> Handled {
    let name = format!("{} {}", question.query_type(), display_name(question.name()));
    let upstream = if server.port() == 53 { server.ip().to_string() } else { server.to_string() };
    let mut answer = forward_udp(query, server).await;
    // A truncated answer is fetched again over TCP for TCP clients (UDP clients retry themselves).
    if transport == Transport::Tcp && answer.as_ref().is_ok_and(|a| a.len() > 2 && a[2] & 0x02 != 0) {
        answer = forward_tcp(query, server).await;
    }
    let parsed = answer.and_then(|bytes| match Message::from_vec(&bytes) {
        Ok(message) => Ok((message, bytes)),
        Err(e) => Err(format!("{server} sent an unreadable answer: {e}")),
    });
    match parsed {
        Ok((message, bytes)) => {
            let limit = size_limit(request, transport);
            let (message, bytes) = if bytes.len() <= limit { (message, bytes) } else { encode(message, limit) };
            let rcode = rcode_name(message.metadata.response_code);
            let answers = if message.answers.is_empty() {
                String::new()
            } else {
                format!(" · {}", answers_text(&message.answers, question.query_type()))
            };
            let tc = if message.metadata.truncation { " (truncated)" } else { "" };
            Handled {
                summary: format!("{name} → forwarded to {upstream} · {rcode}{answers}{tc}{via}"),
                details: describe(&message, transport),
                response: bytes,
            }
        }
        Err(why) => {
            let mut response = Message::response(request.metadata.id, request.metadata.op_code);
            response.metadata = Metadata::response_from_request(&request.metadata);
            response.metadata.response_code = ResponseCode::ServFail;
            response.metadata.recursion_available = true;
            response.add_query(question.clone());
            if request.edns.is_some() {
                let mut ours = Edns::new();
                ours.set_max_payload(MAX_UDP_ANSWER);
                response.set_edns(ours);
            }
            let (response, bytes) = encode(response, size_limit(request, transport));
            Handled {
                summary: format!("{name} → SERVFAIL · {why}{via}"),
                details: format!("{}\n;; Forwarding to {server} failed: {why}", describe(&response, transport)),
                response: bytes,
            }
        }
    }
}

/// Send the query to `server` over UDP and wait for the answer with the same id.
async fn forward_udp(query: &[u8], server: SocketAddr) -> Result<Vec<u8>, String> {
    // A loopback upstream from a loopback address: with a VPN up, macOS refuses to send from
    // the unspecified address to 127.0.0.1 ("Can't assign requested address").
    let bind: SocketAddr = match server.ip() {
        ip if ip.is_loopback() => (ip, 0).into(),
        std::net::IpAddr::V4(_) => (Ipv4Addr::UNSPECIFIED, 0).into(),
        std::net::IpAddr::V6(_) => (Ipv6Addr::UNSPECIFIED, 0).into(),
    };
    let socket = UdpSocket::bind(bind).await.map_err(|e| format!("could not open a socket: {e}"))?;
    // Connected: answers from other addresses are ignored.
    socket.connect(server).await.map_err(|e| format!("could not reach {server}: {e}"))?;
    socket.send(query).await.map_err(|e| format!("could not send to {server}: {e}"))?;
    let deadline = tokio::time::Instant::now() + UPSTREAM_TIMEOUT;
    let mut buf = vec![0u8; 65_536];
    loop {
        let n = match tokio::time::timeout_at(deadline, socket.recv(&mut buf)).await {
            Err(_) => return Err(format!("{server} did not answer within {} s", UPSTREAM_TIMEOUT.as_secs())),
            Ok(Err(e)) if is_unreachable(&e) => return Err(format!("nothing answers DNS at {server}")),
            Ok(Err(e)) => return Err(format!("{server}: {e}")),
            Ok(Ok(n)) => n,
        };
        // Only the answer to this query (same id, QR set) counts.
        if n >= 12 && buf[..2] == query[..2] && buf[2] & 0x80 != 0 {
            buf.truncate(n);
            return Ok(buf);
        }
    }
}

async fn forward_tcp(query: &[u8], server: SocketAddr) -> Result<Vec<u8>, String> {
    let exchange = async {
        let mut stream = TcpStream::connect(server).await?;
        let mut framed = Vec::with_capacity(query.len() + 2);
        framed.extend_from_slice(&(query.len() as u16).to_be_bytes());
        framed.extend_from_slice(query);
        stream.write_all(&framed).await?;
        loop {
            let len = usize::from(stream.read_u16().await?);
            let mut answer = vec![0u8; len];
            stream.read_exact(&mut answer).await?;
            if len >= 12 && answer[..2] == query[..2] {
                return Ok::<_, std::io::Error>(answer);
            }
        }
    };
    match tokio::time::timeout(UPSTREAM_TIMEOUT, exchange).await {
        Err(_) => Err(format!("{server} did not answer over TCP within {} s", UPSTREAM_TIMEOUT.as_secs())),
        Ok(Err(e)) => Err(format!("{server} (TCP): {e}")),
        Ok(Ok(answer)) => Ok(answer),
    }
}

// ---- log text ------------------------------------------------------------------------

fn display_name(name: &Name) -> String {
    let ascii = name.to_ascii();
    let trimmed = ascii.trim_end_matches('.');
    if trimmed.is_empty() { ".".into() } else { trimmed.to_string() }
}

fn rcode_name(code: ResponseCode) -> String {
    match code {
        ResponseCode::NoError => "NOERROR".into(),
        ResponseCode::FormErr => "FORMERR".into(),
        ResponseCode::ServFail => "SERVFAIL".into(),
        ResponseCode::NXDomain => "NXDOMAIN".into(),
        ResponseCode::NotImp => "NOTIMP".into(),
        ResponseCode::Refused => "REFUSED".into(),
        ResponseCode::BADVERS => "BADVERS".into(),
        other => format!("RCODE {}", u16::from(other)),
    }
}

/// Record data as in a zone file (TXT strings quoted).
fn rdata_text(data: &RData) -> String {
    match data {
        RData::TXT(txt) => txt
            .txt_data
            .iter()
            .map(|s| format!("\"{}\"", String::from_utf8_lossy(s).replace('\\', "\\\\").replace('"', "\\\"")))
            .collect::<Vec<_>>()
            .join(" "),
        other => other.to_string(),
    }
}

/// Answer records for the summary: `127.0.0.1, 127.0.0.2` (other types named, e.g. `CNAME api.example.test`).
fn answers_text(records: &[Record], qtype: RecordType) -> String {
    if records.is_empty() {
        return format!("NOERROR, no {qtype} records");
    }
    const SHOWN: usize = 4;
    const SHOWN_CHARS: usize = 60;
    let mut parts: Vec<String> = records
        .iter()
        .take(SHOWN)
        .map(|r| {
            let text = rdata_text(&r.data);
            let mut data: String = text.trim_end_matches('.').chars().take(SHOWN_CHARS).collect();
            if data.len() < text.trim_end_matches('.').len() {
                data.push('…');
            }
            if r.record_type() == qtype { data } else { format!("{} {data}", r.record_type()) }
        })
        .collect();
    if records.len() > SHOWN {
        parts.push(format!("+{} more", records.len() - SHOWN));
    }
    parts.join(", ")
}

/// The answer like `dig` shows it: status, question and records in zone-file format.
fn describe(message: &Message, transport: Transport) -> String {
    let m = &message.metadata;
    let mut flags = vec!["qr"];
    for (on, flag) in
        [(m.authoritative, "aa"), (m.truncation, "tc"), (m.recursion_desired, "rd"), (m.recursion_available, "ra")]
    {
        if on {
            flags.push(flag);
        }
    }
    let mut out = format!(
        ";; {} · id {} · flags: {} · {}\n",
        rcode_name(m.response_code),
        m.id,
        flags.join(" "),
        if transport == Transport::Tcp { "TCP" } else { "UDP" }
    );
    out.push_str(";; QUESTION\n");
    for q in &message.queries {
        out.push_str(&format!("{}\t{}\t{}\n", q.name(), q.query_class(), q.query_type()));
    }
    for (title, records) in
        [("ANSWER", &message.answers), ("AUTHORITY", &message.authorities), ("ADDITIONAL", &message.additionals)]
    {
        if records.is_empty() {
            continue;
        }
        out.push_str(&format!(";; {title}\n"));
        for r in records {
            out.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\n",
                r.name,
                r.ttl,
                r.dns_class,
                r.record_type(),
                rdata_text(&r.data)
            ));
        }
    }
    out.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(name: &str, rtype: &str, value: &str) -> DnsRecord {
        DnsRecord { name: name.into(), record_type: rtype.into(), value: value.into(), ttl: 60, enabled: true }
    }

    fn zone(records: Vec<DnsRecord>) -> (Zone, Vec<String>) {
        let config = DnsServerConfig { records, upstream: String::new() };
        Zone::compile(&config, "127.0.0.1:5353".parse().unwrap())
    }

    fn name(s: &str) -> Name {
        parse_name(s).unwrap()
    }

    #[test]
    fn values_parse_or_explain() {
        let (_, problems) = zone(vec![
            record("a.test", "A", "127.0.0.1"),
            record("a.test", "A", "not-an-ip"),
            record("a.test", "MX", "10"),
            record("a.test", "SRV", "10 5 99999 x.test"),
            record("a.test", "CAA", "0 is-sue x"),
            record("a.test", "HINFO", "x"),
            record("", "A", "127.0.0.1"),
            record("a.*.test", "A", "127.0.0.1"),
            record("a.test", "TXT", "\"unclosed"),
            record("a.test", "CAA", "0 issue \"letsencrypt.org\""),
            record("1.2.3.4", "PTR", "host.test"),
        ]);
        assert_eq!(problems.len(), 8, "{problems:#?}");
        assert!(
            problems[0].starts_with("DNS record 2 (A a.test) skipped: 'not-an-ip' is not an IPv4"),
            "{}",
            problems[0]
        );
    }

    #[test]
    fn txt_strings_are_split_and_unquoted() {
        assert_eq!(txt_strings("plain text").unwrap(), vec![b"plain text".to_vec()]);
        assert_eq!(txt_strings(r#""a \"q\"" "b""#).unwrap(), vec![br#"a "q""#.to_vec(), b"b".to_vec()]);
        let long = "x".repeat(600);
        let parts = txt_strings(&long).unwrap();
        assert_eq!(parts.iter().map(Vec::len).collect::<Vec<_>>(), [255, 255, 90]);
        assert!(txt_strings(r#""a" b"#).is_err());
    }

    #[test]
    fn wildcards_cnames_and_parents() {
        let (zone, problems) = zone(vec![
            record("API.Example.Test.", "A", "127.0.0.1"),
            record("*.example.test", "A", "10.0.0.1"),
            record("*.deep.example.test", "A", "10.0.0.2"),
            record("www.example.test", "CNAME", "api.example.test"),
            record("out.example.test", "CNAME", "elsewhere.test"),
            record("loop1.test", "CNAME", "loop2.test"),
            record("loop2.test", "CNAME", "loop1.test"),
        ]);
        assert!(problems.is_empty(), "{problems:?}");
        let a = |n: &str| match zone.answer(&name(n), RecordType::A) {
            Local::Answer { records, chase } => {
                (records.iter().map(|r| rdata_text(&r.data)).collect::<Vec<_>>(), chase.map(|c| display_name(&c)))
            }
            Local::NoData => (vec!["NODATA".into()], None),
            Local::Unknown => (vec!["UNKNOWN".into()], None),
        };
        assert_eq!(a("api.example.test").0, ["127.0.0.1"]);
        assert_eq!(a("x.example.test").0, ["10.0.0.1"]);
        assert_eq!(a("a.b.example.test").0, ["10.0.0.1"]);
        assert_eq!(a("x.deep.example.test").0, ["10.0.0.2"]);
        assert_eq!(a("www.example.test").0, ["api.example.test.", "127.0.0.1"]);
        assert_eq!(a("out.example.test"), (vec!["elsewhere.test.".into()], Some("elsewhere.test".into())));
        assert_eq!(a("loop1.test").0.len(), 2);
        // `example.test` exists (above the records) but has none; `nope.test` does not exist.
        assert_eq!(a("example.test").0, ["NODATA"]);
        assert_eq!(a("nope.test").0, ["UNKNOWN"]);
        assert!(matches!(zone.answer(&name("api.example.test"), RecordType::AAAA), Local::NoData));
    }

    #[test]
    fn upstream_forms() {
        let local: SocketAddr = "0.0.0.0:5353".parse().unwrap();
        assert_eq!(parse_upstream("", local), Ok(Upstream::None));
        assert_eq!(parse_upstream("System", local), Ok(Upstream::System));
        assert_eq!(parse_upstream("1.1.1.1", local), Ok(Upstream::Server("1.1.1.1:53".parse().unwrap())));
        assert_eq!(
            parse_upstream("[2606:4700::1111]:53", local),
            Ok(Upstream::Server("[2606:4700::1111]:53".parse().unwrap()))
        );
        assert_eq!(
            parse_upstream("2606:4700::1111", local),
            Ok(Upstream::Server("[2606:4700::1111]:53".parse().unwrap()))
        );
        assert!(parse_upstream("dns.google", local).is_err());
        assert!(parse_upstream("127.0.0.1:5353", local).unwrap_err().contains("itself"));
    }
}

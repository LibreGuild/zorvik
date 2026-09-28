//! DNS client: one query, answered by this computer's resolver or a server of
//! the user's choice over UDP (retried over TCP when truncated), TCP, DNS over
//! TLS or DNS over HTTPS. The whole response comes back the way `dig` shows it:
//! every section, the header flags and the response code (NXDOMAIN is a
//! result, not an error).
//!
//! hickory encodes and decodes messages and reads the OS resolver settings; the
//! transports are the engine's own, so DoT/DoH use the same TLS configuration
//! (OS trust store, extra CA, verification switch) as HTTP. DoH also uses the
//! HTTP proxy; UDP, TCP and DoT connect directly.

use std::fmt::Write as _;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::str::FromStr;
use std::time::{Duration, Instant};

use bytes::Bytes;
use hickory_proto::op::Query;
use hickory_proto::op::{Edns, Message, ResponseCode};
use hickory_proto::rr::{DNSClass, Name, RData, Record, RecordType};
use serde::Serialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UdpSocket;
use ts_rs::TS;
use url::Url;

use crate::error::{EngineError, ErrorKind, Result, human_duration};
use crate::http::{Client, Header, HttpRequest, RequestOptions};
use crate::net::{self, Target};
use crate::socket::ms;
use crate::tls::{Alpn, TlsInfo};

/// Used when a query does not set its own timeout.
pub const DEFAULT_DNS_TIMEOUT: Duration = Duration::from_secs(5);
/// EDNS UDP payload size (DNS Flag Day 2020): big enough for most answers,
/// small enough to avoid IP fragmentation.
const EDNS_PAYLOAD: u16 = 1232;
/// Largest DNS message (TCP length prefix / UDP datagram).
const MAX_MESSAGE: usize = 65_535;
/// System resolvers tried in turn when one does not answer.
const MAX_SYSTEM_SERVERS: usize = 3;

/// One lookup, with variables already substituted.
#[derive(Debug, Clone)]
pub struct DnsQuery {
    /// Name to look up (`example.com`); an IP address for PTR. A URL or a
    /// `host:port` is reduced to its host.
    pub name: String,
    /// A, AAAA, CNAME, MX, TXT, NS, SOA, SRV, PTR, CAA, ANY (any type name, or `TYPE65`).
    pub record_type: String,
    /// Resolver, see [`DnsResolver::parse`]. Empty: the system's.
    pub server: String,
    /// Ask for recursion (RD flag).
    pub recursion: bool,
    /// Whole query, including a TCP retry. Zero: [`DEFAULT_DNS_TIMEOUT`].
    pub timeout: Duration,
}

/// Where a query goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DnsResolver {
    /// The DNS servers in this computer's network settings.
    System,
    Udp {
        host: String,
        port: u16,
    },
    Tcp {
        host: String,
        port: u16,
    },
    /// DNS over TLS (RFC 7858).
    Tls {
        host: String,
        port: u16,
    },
    /// DNS over HTTPS (RFC 8484), `POST application/dns-message`.
    Https {
        url: String,
    },
}

impl DnsResolver {
    /// `""`/`system`, `1.1.1.1`, `8.8.8.8:53`, `[2606:4700::1111]`, `dns.example`
    /// (UDP), `udp://…`, `tcp://host[:53]`, `tls://host[:853]`, `https://host/dns-query`.
    pub fn parse(raw: &str) -> Result<Self> {
        let raw = raw.trim();
        if raw.is_empty() || raw.eq_ignore_ascii_case("system") {
            return Ok(Self::System);
        }
        let Some((scheme, rest)) = raw.split_once("://") else {
            let (host, port) = host_port(raw, 53)?;
            return Ok(Self::Udp { host, port });
        };
        match scheme.to_ascii_lowercase().as_str() {
            // Plain http is accepted for local DoH test servers.
            "https" | "http" => {
                let mut url = Url::parse(raw)
                    .map_err(|e| EngineError::invalid(format!("Invalid DNS over HTTPS URL '{raw}': {e}")))?;
                if url.host_str().is_none_or(str::is_empty) {
                    return Err(EngineError::invalid(format!("DNS over HTTPS URL '{raw}' has no host")));
                }
                // Most servers use the RFC 8484 example path.
                if url.path() == "/" && url.query().is_none() {
                    url.set_path("/dns-query");
                }
                url.set_fragment(None);
                Ok(Self::Https { url: url.to_string() })
            }
            "udp" | "dns" => host_port(rest, 53).map(|(host, port)| Self::Udp { host, port }),
            "tcp" => host_port(rest, 53).map(|(host, port)| Self::Tcp { host, port }),
            "tls" | "dot" => host_port(rest, 853).map(|(host, port)| Self::Tls { host, port }),
            other => Err(EngineError::invalid(format!(
                "Unsupported DNS server scheme '{other}' (use udp://, tcp://, tls:// or https://)"
            ))),
        }
    }
}

/// `host`, `host:port`, `1.2.3.4`, `::1`, `[::1]:53` (a trailing path is ignored).
fn host_port(raw: &str, default_port: u16) -> Result<(String, u16)> {
    let text = raw.split(['/', '?', '#']).next().unwrap_or_default().trim();
    let bad_port = |p: &str| EngineError::invalid(format!("'{p}' is not a valid port in DNS server '{raw}'"));
    let parse_port = |p: &str| p.parse::<u16>().ok().filter(|p| *p != 0).ok_or_else(|| bad_port(p));
    let (host, port) = if let Some(inner) = text.strip_prefix('[') {
        let (host, after) =
            inner.split_once(']').ok_or_else(|| EngineError::invalid(format!("DNS server '{raw}' is missing ']'")))?;
        let port = match after.strip_prefix(':') {
            Some(p) => parse_port(p)?,
            None if after.is_empty() => default_port,
            None => return Err(bad_port(after)),
        };
        (host, port)
    } else if text.parse::<IpAddr>().is_ok() {
        (text, default_port)
    } else if let Some((host, port)) = text.rsplit_once(':') {
        (host, parse_port(port)?)
    } else {
        (text, default_port)
    };
    if host.is_empty() {
        return Err(EngineError::invalid(format!("DNS server '{raw}' has no host")));
    }
    Ok((host.to_string(), port))
}

/// The name as typed without a URL scheme, user, port or path: `https://a.test:8443/x` -> `a.test`.
fn bare_name(raw: &str) -> &str {
    let mut s = raw.trim();
    if let Some((_, rest)) = s.split_once("://") {
        s = rest;
    }
    s = s.split(['/', '?', '#']).next().unwrap_or_default();
    if let Some((_, host)) = s.rsplit_once('@') {
        s = host;
    }
    if let Some(inner) = s.strip_prefix('[') {
        return inner.split(']').next().unwrap_or_default();
    }
    // One colon is a port; more is an IPv6 address.
    if let Some((host, port)) = s.split_once(':')
        && !port.contains(':')
        && port.chars().all(|c| c.is_ascii_digit())
    {
        return host;
    }
    s
}

/// The name to ask for. An IP address becomes its reverse name for PTR.
pub fn query_name(raw: &str, record_type: RecordType) -> Result<Name> {
    let text = bare_name(raw);
    if text.is_empty() {
        return Err(EngineError::invalid("Enter a name to look up, e.g. example.com"));
    }
    if let Ok(ip) = text.parse::<IpAddr>() {
        return if record_type == RecordType::PTR {
            Ok(Name::from(ip))
        } else {
            Err(EngineError::invalid("Use PTR to look up an IP address"))
        };
    }
    // Non-ASCII names go through IDNA (bücher.example -> xn--bcher-kva.example).
    let parsed = if text.is_ascii() { Name::from_ascii(text) } else { Name::from_utf8(text) };
    let mut name = parsed.map_err(|e| EngineError::invalid(format!("'{text}' is not a valid DNS name: {e}")))?;
    // Absolute: no search domains are appended.
    name.set_fqdn(true);
    Ok(name)
}

/// `A`, `mx`, `TYPE65`… Empty or `GET` (the default method of a request that
/// never got a record type) means A.
pub fn parse_record_type(raw: &str) -> Result<RecordType> {
    let text = raw.trim().to_ascii_uppercase();
    if text.is_empty() || text == "GET" {
        return Ok(RecordType::A);
    }
    let record_type = match text.strip_prefix("TYPE").and_then(|n| n.parse::<u16>().ok()) {
        Some(code) => RecordType::from(code),
        None => RecordType::from_str(&text)
            .map_err(|_| EngineError::invalid(format!("Unknown DNS record type '{}'", raw.trim())))?,
    };
    match record_type {
        RecordType::AXFR | RecordType::IXFR => {
            Err(EngineError::invalid("Zone transfers (AXFR, IXFR) are not supported"))
        }
        RecordType::OPT => Err(EngineError::invalid("OPT is not a record type that can be queried")),
        other => Ok(other),
    }
}

// ---- results ------------------------------------------------------------------

/// A DNS answer, section by section.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DnsResult {
    pub question: DnsQuestion,
    pub answers: Vec<DnsResultRecord>,
    pub authority: Vec<DnsResultRecord>,
    pub additional: Vec<DnsResultRecord>,
    /// NOERROR, NXDOMAIN, SERVFAIL, REFUSED, …
    pub rcode: String,
    pub rcode_value: u16,
    pub flags: DnsFlags,
    /// Server that answered: `1.1.1.1:53`, or the DoH URL.
    pub server: String,
    /// UDP, TCP, DoT, DoH, or `System` (the operating system's lookup, without DNS details).
    pub protocol: String,
    /// Asked the DNS servers from this computer's network settings.
    pub system: bool,
    /// DNS over TLS/HTTPS: the negotiated TLS session.
    pub tls: Option<TlsInfo>,
    pub duration_ms: f64,
    /// Size of the DNS response message in bytes.
    #[ts(type = "number")]
    pub size: u64,
    /// Things worth knowing, e.g. "retried over TCP".
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DnsQuestion {
    pub name: String,
    #[serde(rename = "type")]
    pub record_type: String,
    pub class: String,
}

/// One resource record, with its data as in a zone file (`10 mail.example.com.` for MX).
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DnsResultRecord {
    pub name: String,
    #[serde(rename = "type")]
    pub record_type: String,
    pub class: String,
    pub ttl: u32,
    pub data: String,
}

/// Header flags of the response.
#[derive(Debug, Clone, Copy, Default, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DnsFlags {
    /// Authoritative answer.
    pub aa: bool,
    /// Truncated.
    pub tc: bool,
    /// Recursion desired.
    pub rd: bool,
    /// Recursion available.
    pub ra: bool,
    /// Authentic data (DNSSEC validated by the resolver).
    pub ad: bool,
    /// Checking disabled.
    pub cd: bool,
}

/// The raw answer of one exchange and how it was obtained.
#[derive(Debug, Clone)]
pub struct DnsExchange {
    pub response: Vec<u8>,
    pub server: String,
    pub protocol: &'static str,
    pub tls: Option<TlsInfo>,
    pub notes: Vec<String>,
}

// ---- client -------------------------------------------------------------------

impl Client {
    /// Run one DNS query. `opts` supplies TLS settings, the connect timeout and
    /// (for DNS over HTTPS) the proxy; `query.timeout` bounds the whole lookup.
    pub async fn dns_query(&self, query: &DnsQuery, opts: &RequestOptions) -> Result<DnsResult> {
        let started = Instant::now();
        let record_type = parse_record_type(&query.record_type)?;
        let name = query_name(&query.name, record_type)?;
        let resolver = DnsResolver::parse(&query.server)?;
        if let Some(guard) = &opts.host_guard {
            // Resolvers pass the name on to its own servers, so the name counts as a host too.
            guard.check_host(query.name.trim().trim_end_matches('.'))?;
            match &resolver {
                DnsResolver::Udp { host, .. } | DnsResolver::Tcp { host, .. } | DnsResolver::Tls { host, .. } => {
                    guard.check_host(host)?
                }
                // The system's resolvers; DoH is checked like any HTTPS request.
                DnsResolver::System | DnsResolver::Https { .. } => {}
            }
        }
        let timeout = if query.timeout.is_zero() { DEFAULT_DNS_TIMEOUT } else { query.timeout };
        let deadline = tokio::time::Instant::now() + timeout;
        let work = async {
            let mut edns = true;
            let mut notes = Vec::new();
            loop {
                let mut message = Message::query();
                // DoH uses ID 0 so HTTP caches can share answers (RFC 8484 §4.1).
                if matches!(resolver, DnsResolver::Https { .. }) {
                    message.metadata.id = 0;
                }
                message.metadata.recursion_desired = query.recursion;
                // Ask for the AD bit: a validating resolver then says whether the data is authentic (RFC 6840 §5.7).
                message.metadata.authentic_data = true;
                message.add_query(Query::query(name.clone(), record_type));
                if edns {
                    let mut e = Edns::new();
                    e.set_max_payload(EDNS_PAYLOAD);
                    message.set_edns(e);
                }
                let bytes = message
                    .to_vec()
                    .map_err(|e| EngineError::invalid(format!("Could not build the DNS query: {e}")))?;

                let exchange = match self.dns_exchange(&resolver, &bytes, opts, deadline).await {
                    Ok(x) => x,
                    Err(e) if e.kind == ErrorKind::Dns && resolver == DnsResolver::System => {
                        // The DNS settings could not be read: the OS lookup still gives addresses.
                        if matches!(record_type, RecordType::A | RecordType::AAAA) {
                            return os_lookup_result(&name, record_type, &e.message, deadline, started).await;
                        }
                        return Err(EngineError::new(
                            ErrorKind::Dns,
                            format!(
                                "{} Choose a DNS server in the Resolver tab to look up {record_type} records.",
                                e.message
                            ),
                        ));
                    }
                    Err(e) => return Err(e),
                };
                let response = parse_response(&exchange.response)?;
                // Very old servers reject EDNS with FORMERR: ask again without it (like dig).
                if edns && response.metadata.response_code == ResponseCode::FormErr && response.edns.is_none() {
                    edns = false;
                    notes.push("The server rejected EDNS; asked again without it.".to_string());
                    continue;
                }
                notes.extend(exchange.notes.iter().cloned());
                let mut result = describe(&response, &exchange, &name, record_type, notes);
                // The DNS time: the OS lookup below is extra.
                result.duration_ms = ms(started.elapsed());
                if resolver == DnsResolver::System {
                    result.system = true;
                    if let Some(note) = os_resolution_hint(&result, &name, record_type, deadline).await {
                        result.notes.push(note);
                    }
                }
                return Ok(result);
            }
        };
        tokio::time::timeout_at(deadline, work).await.map_err(|_| EngineError::timeout("DNS query", timeout))?
    }

    /// Send an encoded query to `resolver` and return the raw answer. UDP answers
    /// that are truncated are fetched again over TCP.
    pub async fn dns_exchange(
        &self,
        resolver: &DnsResolver,
        query: &[u8],
        opts: &RequestOptions,
        deadline: tokio::time::Instant,
    ) -> Result<DnsExchange> {
        if query.len() < 12 {
            return Err(EngineError::invalid("DNS query is too short"));
        }
        match resolver {
            DnsResolver::System => {
                let servers = system_servers().await?;
                let count = servers.len().min(MAX_SYSTEM_SERVERS);
                let mut last_error = None;
                for (i, addr) in servers.into_iter().take(count).enumerate() {
                    // Share the remaining time among the servers still to try.
                    let left = deadline.saturating_duration_since(tokio::time::Instant::now());
                    let until = tokio::time::Instant::now() + left / (count - i) as u32;
                    match self.udp_then_tcp(addr, query, opts, until).await {
                        Ok(x) => return Ok(x),
                        Err(e) => last_error = Some(e),
                    }
                }
                Err(last_error.unwrap_or_else(|| EngineError::new(ErrorKind::Dns, "No DNS server answered")))
            }
            DnsResolver::Udp { host, port } => {
                let left = deadline.saturating_duration_since(tokio::time::Instant::now());
                let mut last_error = None;
                // A host name may have addresses this computer cannot reach (IPv6 without a route).
                for addr in net::resolve(host, *port, left).await?.into_iter().take(4) {
                    match self.udp_then_tcp(addr, query, opts, deadline).await {
                        Err(e) if e.kind == ErrorKind::Connect => last_error = Some(e),
                        other => return other,
                    }
                }
                Err(last_error
                    .unwrap_or_else(|| EngineError::new(ErrorKind::Dns, format!("'{host}' has no addresses"))))
            }
            DnsResolver::Tcp { host, port } | DnsResolver::Tls { host, port } => {
                let tls = matches!(resolver, DnsResolver::Tls { .. });
                let (response, remote, info) = self.stream_exchange(host, *port, tls, query, opts, deadline).await?;
                Ok(DnsExchange {
                    response,
                    server: remote.to_string(),
                    protocol: if tls { "DoT" } else { "TCP" },
                    tls: info,
                    notes: Vec::new(),
                })
            }
            DnsResolver::Https { url } => self.doh_exchange(url, query, opts, deadline).await,
        }
    }

    async fn udp_then_tcp(
        &self,
        addr: SocketAddr,
        query: &[u8],
        opts: &RequestOptions,
        deadline: tokio::time::Instant,
    ) -> Result<DnsExchange> {
        let response = udp_exchange(addr, query, deadline).await?;
        let truncated = response[2] & 0x02 != 0;
        let udp = DnsExchange { response, server: addr.to_string(), protocol: "UDP", tls: None, notes: Vec::new() };
        if !truncated {
            return Ok(udp);
        }
        match self.stream_exchange(&addr.ip().to_string(), addr.port(), false, query, opts, deadline).await {
            Ok((response, _, _)) => Ok(DnsExchange {
                response,
                protocol: "TCP",
                notes: vec!["The answer did not fit in a UDP packet; asked again over TCP.".into()],
                ..udp
            }),
            Err(e) => Ok(DnsExchange {
                notes: vec![format!("The answer was truncated and the TCP retry failed: {}", e.message)],
                ..udp
            }),
        }
    }

    /// DNS over TCP or TLS: a 2-byte length prefix before each message (RFC 1035 §4.2.2).
    async fn stream_exchange(
        &self,
        host: &str,
        port: u16,
        tls: bool,
        query: &[u8],
        opts: &RequestOptions,
        deadline: tokio::time::Instant,
    ) -> Result<(Vec<u8>, SocketAddr, Option<TlsInfo>)> {
        let left = deadline.saturating_duration_since(tokio::time::Instant::now()).max(Duration::from_millis(1));
        let conn = net::connect(
            &Target {
                host,
                port,
                tls,
                alpn: Alpn::None,
                tls_options: &opts.tls,
                proxy: None,
                force_tunnel: false,
                connect_timeout: opts.connect_timeout.min(left),
            },
            &self.tls,
        )
        .await?;
        let remote = conn.remote_addr;
        let mut stream = conn.stream;
        let io_error = |e: std::io::Error| {
            if e.kind() == std::io::ErrorKind::UnexpectedEof {
                EngineError::new(ErrorKind::Protocol, format!("{remote} closed the connection without answering"))
            } else {
                EngineError::new(
                    ErrorKind::Io,
                    format!("DNS over {} with {remote} failed: {e}", if tls { "TLS" } else { "TCP" }),
                )
            }
        };
        let len = u16::try_from(query.len()).map_err(|_| EngineError::invalid("DNS query is too large"))?;
        let mut framed = Vec::with_capacity(query.len() + 2);
        framed.extend_from_slice(&len.to_be_bytes());
        framed.extend_from_slice(query);
        let asked = tokio::time::Instant::now();
        // Bounded by `deadline` too: a TCP retry that never answers must still
        // leave the truncated UDP answer to show (see `udp_then_tcp`).
        let exchange = async {
            stream.write_all(&framed).await?;
            stream.flush().await?;
            let mut prefix = [0u8; 2];
            stream.read_exact(&mut prefix).await?;
            let mut response = vec![0u8; usize::from(u16::from_be_bytes(prefix))];
            stream.read_exact(&mut response).await?;
            Ok(response)
        };
        let response = tokio::time::timeout_at(deadline, exchange)
            .await
            .map_err(|_| {
                EngineError::new(
                    ErrorKind::Timeout,
                    format!("No answer from {remote} within {}", human_duration(asked.elapsed())),
                )
            })?
            .map_err(io_error)?;
        if !is_response_to(&response, query) {
            return Err(EngineError::new(
                ErrorKind::Protocol,
                format!("{remote} sent an answer that does not match the query"),
            ));
        }
        Ok((response, remote, conn.tls))
    }

    /// DNS over HTTPS through the HTTP client (so the proxy and TLS settings apply).
    async fn doh_exchange(
        &self,
        url: &str,
        query: &[u8],
        opts: &RequestOptions,
        deadline: tokio::time::Instant,
    ) -> Result<DnsExchange> {
        let mut http_opts = opts.clone();
        http_opts.timeout =
            Some(deadline.saturating_duration_since(tokio::time::Instant::now()).max(Duration::from_millis(1)));
        http_opts.max_body_bytes = MAX_MESSAGE;
        http_opts.decompress = true;
        let request = HttpRequest {
            method: "POST".into(),
            url: url.to_string(),
            headers: vec![
                Header::new("Content-Type", "application/dns-message"),
                Header::new("Accept", "application/dns-message"),
            ],
            body: Bytes::copy_from_slice(query),
        };
        let response = self.send(request, &http_opts, None).await?;
        let meta = &response.meta;
        if meta.status != 200 {
            return Err(EngineError::new(
                ErrorKind::Protocol,
                format!("The DNS over HTTPS server answered {} {}", meta.status, meta.status_text)
                    .trim_end()
                    .to_string(),
            ));
        }
        let content_type = meta
            .headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case("content-type"))
            .map(|h| h.value.to_ascii_lowercase())
            .unwrap_or_default();
        if !content_type.starts_with("application/dns-message") {
            let shown = if content_type.is_empty() { "no content type".to_string() } else { content_type };
            return Err(EngineError::new(
                ErrorKind::Protocol,
                format!(
                    "The server returned {shown} instead of a DNS message. Check the URL (it usually ends in /dns-query)."
                ),
            ));
        }
        if response.body_truncated {
            return Err(EngineError::new(ErrorKind::Protocol, "The DNS over HTTPS answer is larger than 64 KB"));
        }
        if !is_response_to(&response.body, query) {
            return Err(EngineError::new(ErrorKind::Protocol, "The DNS over HTTPS answer does not match the query"));
        }
        Ok(DnsExchange {
            server: meta.url.clone(),
            protocol: "DoH",
            tls: meta.tls.clone(),
            notes: Vec::new(),
            response: response.body,
        })
    }
}

/// DNS servers from the OS network settings (resolv.conf, the macOS dynamic
/// store, Windows adapters). A `Dns` error when they cannot be read.
async fn system_servers() -> Result<Vec<SocketAddr>> {
    let read = tokio::task::spawn_blocking(hickory_resolver::system_conf::read_system_conf)
        .await
        .map_err(|e| EngineError::new(ErrorKind::Dns, format!("Could not read this computer's DNS settings: {e}.")))?;
    let mut servers: Vec<SocketAddr> = Vec::new();
    match read {
        Ok((config, _)) => {
            for ns in config.name_servers() {
                let addr = SocketAddr::new(ns.ip, 53);
                if !servers.contains(&addr) {
                    servers.push(addr);
                }
            }
        }
        // hickory rejects the whole file for one entry it can't parse, e.g. the
        // `nameserver fe80::1%en0` macOS writes for IPv6 routers: read it ourselves.
        #[cfg(unix)]
        Err(e) => {
            let text = std::fs::read_to_string("/etc/resolv.conf").unwrap_or_default();
            servers = resolv_conf_servers(&text);
            if servers.is_empty() {
                return Err(EngineError::new(
                    ErrorKind::Dns,
                    format!("Could not read this computer's DNS settings: {e}."),
                ));
            }
        }
        #[cfg(not(unix))]
        Err(e) => {
            return Err(EngineError::new(ErrorKind::Dns, format!("Could not read this computer's DNS settings: {e}.")));
        }
    }
    if servers.is_empty() {
        return Err(EngineError::new(ErrorKind::Dns, "This computer has no DNS servers configured."));
    }
    Ok(servers)
}

/// `nameserver` lines of a resolv.conf. Scoped link-local addresses
/// (`fe80::1%en0`) are skipped: they can't be used without their interface.
#[cfg_attr(not(unix), allow(dead_code))]
fn resolv_conf_servers(text: &str) -> Vec<SocketAddr> {
    let mut servers = Vec::new();
    for line in text.lines() {
        let mut words = line.split_whitespace();
        if words.next() != Some("nameserver") {
            continue;
        }
        let Some(ip) = words.next().and_then(|w| w.parse::<std::net::IpAddr>().ok()) else { continue };
        let addr = SocketAddr::new(ip, 53);
        if !servers.contains(&addr) {
            servers.push(addr);
        }
    }
    servers
}

/// Send over UDP, resending after 1 s, 2 s, 4 s… until `deadline`. Only a
/// response with our ID from the server (the socket is connected) is accepted.
async fn udp_exchange(addr: SocketAddr, query: &[u8], deadline: tokio::time::Instant) -> Result<Vec<u8>> {
    // A loopback server from a loopback address: with a VPN up, macOS refuses to send from
    // the unspecified address to 127.0.0.1 ("Can't assign requested address").
    let local: SocketAddr = match addr.ip() {
        ip if ip.is_loopback() => (ip, 0).into(),
        std::net::IpAddr::V4(_) => (Ipv4Addr::UNSPECIFIED, 0).into(),
        std::net::IpAddr::V6(_) => (Ipv6Addr::UNSPECIFIED, 0).into(),
    };
    let socket = UdpSocket::bind(local).await.map_err(|e| udp_error(addr, e))?;
    socket.connect(addr).await.map_err(|e| udp_error(addr, e))?;
    let started = tokio::time::Instant::now();
    let mut buf = vec![0u8; MAX_MESSAGE];
    let mut wait = Duration::from_secs(1);
    loop {
        socket.send(query).await.map_err(|e| udp_error(addr, e))?;
        let resend_at = (tokio::time::Instant::now() + wait).min(deadline);
        loop {
            match tokio::time::timeout_at(resend_at, socket.recv(&mut buf)).await {
                Err(_) => break,
                Ok(Err(e)) => return Err(udp_error(addr, e)),
                Ok(Ok(n)) if is_response_to(&buf[..n], query) => return Ok(buf[..n].to_vec()),
                // A late answer to an earlier try has the same ID; anything else is ignored.
                Ok(Ok(_)) => {}
            }
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(EngineError::new(
                ErrorKind::Timeout,
                format!("No answer from {addr} within {}", human_duration(started.elapsed())),
            ));
        }
        wait *= 2;
    }
}

fn udp_error(addr: SocketAddr, err: std::io::Error) -> EngineError {
    match err.kind() {
        // An ICMP "port unreachable" surfaces as refused (Unix) or reset (Windows).
        std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::ConnectionReset => {
            EngineError::new(ErrorKind::Connect, format!("No DNS server is listening on {addr} (port unreachable)"))
        }
        _ => EngineError::new(ErrorKind::Connect, format!("Could not send the DNS query to {addr}: {err}")),
    }
}

/// A response (QR set) carrying the query's ID.
fn is_response_to(response: &[u8], query: &[u8]) -> bool {
    response.len() >= 12 && response[..2] == query[..2] && response[2] & 0x80 != 0
}

fn protocol_error(message: impl Into<String>) -> EngineError {
    EngineError::new(ErrorKind::Protocol, message)
}

/// Decode an answer from the network. Section counts the message cannot hold
/// are refused before decoding allocates room for them.
pub fn parse_response(bytes: &[u8]) -> Result<Message> {
    if bytes.len() < 12 {
        return Err(protocol_error("The DNS answer is too short"));
    }
    let count = |i: usize| usize::from(u16::from_be_bytes([bytes[i], bytes[i + 1]]));
    // A question takes at least 5 bytes, a record at least 11.
    let needed = count(4) * 5 + (count(6) + count(8) + count(10)) * 11;
    if needed > bytes.len() - 12 {
        return Err(protocol_error("The DNS answer is malformed (its section counts exceed its size)"));
    }
    Message::from_vec(bytes).map_err(|e| protocol_error(format!("The DNS answer could not be decoded: {e}")))
}

fn describe(
    response: &Message,
    exchange: &DnsExchange,
    name: &Name,
    record_type: RecordType,
    notes: Vec<String>,
) -> DnsResult {
    let m = &response.metadata;
    let question = match response.queries.first() {
        Some(q) => DnsQuestion {
            name: q.name().to_string(),
            record_type: type_name(q.query_type()),
            class: class_name(q.query_class()),
        },
        None => DnsQuestion { name: name.to_string(), record_type: type_name(record_type), class: "IN".into() },
    };
    let code = u16::from(m.response_code);
    DnsResult {
        question,
        answers: records(&response.answers),
        authority: records(&response.authorities),
        additional: records(&response.additionals),
        rcode: rcode_name(code),
        rcode_value: code,
        flags: DnsFlags {
            aa: m.authoritative,
            tc: m.truncation,
            rd: m.recursion_desired,
            ra: m.recursion_available,
            ad: m.authentic_data,
            cd: m.checking_disabled,
        },
        server: exchange.server.clone(),
        protocol: exchange.protocol.to_string(),
        system: false,
        tls: exchange.tls.clone(),
        duration_ms: 0.0,
        size: exchange.response.len() as u64,
        notes,
    }
}

fn records(list: &[Record]) -> Vec<DnsResultRecord> {
    list.iter()
        .map(|r| DnsResultRecord {
            name: r.name.to_string(),
            record_type: type_name(r.record_type()),
            class: class_name(r.dns_class),
            ttl: r.ttl,
            data: rdata_text(&r.data),
        })
        .collect()
}

fn type_name(t: RecordType) -> String {
    match t {
        RecordType::Unknown(code) => format!("TYPE{code}"),
        other => other.to_string(),
    }
}

fn class_name(c: DNSClass) -> String {
    match c {
        DNSClass::Unknown(code) => format!("CLASS{code}"),
        DNSClass::OPT(_) => "OPT".into(),
        other => other.to_string(),
    }
}

/// Record data as in a zone file. Never panics: some hickory types (OPT) refuse to display.
fn rdata_text(data: &RData) -> String {
    match data {
        RData::TXT(txt) => txt.txt_data.iter().map(|s| quote_txt(s)).collect::<Vec<_>>().join(" "),
        other => {
            let mut out = String::new();
            if write!(out, "{other}").is_err() {
                out = format!("({})", type_name(other.record_type()));
            }
            out
        }
    }
}

/// A TXT string in zone-file syntax: quoted, `"` and `\` escaped, other
/// unprintable bytes as `\DDD`.
fn quote_txt(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() + 2);
    out.push('"');
    for chunk in bytes.utf8_chunks() {
        for c in chunk.valid().chars() {
            match c {
                '"' | '\\' => {
                    out.push('\\');
                    out.push(c);
                }
                c if c.is_control() => {
                    let _ = write!(out, "\\{:03}", u32::from(c));
                }
                c => out.push(c),
            }
        }
        for b in chunk.invalid() {
            let _ = write!(out, "\\{b:03}");
        }
    }
    out.push('"');
    out
}

fn rcode_name(code: u16) -> String {
    let name = match code {
        0 => "NOERROR",
        1 => "FORMERR",
        2 => "SERVFAIL",
        3 => "NXDOMAIN",
        4 => "NOTIMP",
        5 => "REFUSED",
        6 => "YXDOMAIN",
        7 => "YXRRSET",
        8 => "NXRRSET",
        9 => "NOTAUTH",
        10 => "NOTZONE",
        16 => "BADVERS",
        17 => "BADKEY",
        18 => "BADTIME",
        19 => "BADMODE",
        20 => "BADNAME",
        21 => "BADALG",
        22 => "BADTRUNC",
        23 => "BADCOOKIE",
        other => return format!("RCODE{other}"),
    };
    name.to_string()
}

/// Addresses from the operating system's own lookup (hosts file, mDNS, VPN rules).
async fn os_addresses(name: &Name, record_type: RecordType, limit: Duration) -> std::io::Result<Vec<IpAddr>> {
    let host = name.to_ascii();
    let host = host.trim_end_matches('.').to_string();
    let lookup = tokio::net::lookup_host((host, 0));
    let addrs = tokio::time::timeout(limit, lookup).await.map_err(|_| std::io::ErrorKind::TimedOut)??;
    let mut out: Vec<IpAddr> = Vec::new();
    for ip in addrs.map(|a| a.ip()) {
        let wanted = if record_type == RecordType::AAAA { ip.is_ipv6() } else { ip.is_ipv4() };
        if wanted && !out.contains(&ip) {
            out.push(ip);
        }
    }
    Ok(out)
}

/// When the DNS settings cannot be read: A/AAAA from the OS lookup, without TTLs.
async fn os_lookup_result(
    name: &Name,
    record_type: RecordType,
    why: &str,
    deadline: tokio::time::Instant,
    started: Instant,
) -> Result<DnsResult> {
    let limit = deadline.saturating_duration_since(tokio::time::Instant::now());
    let host = name.to_string();
    let addrs = os_addresses(name, record_type, limit).await.map_err(|e| {
        let reason = if e.kind() == std::io::ErrorKind::TimedOut { "timed out".to_string() } else { e.to_string() };
        EngineError::new(ErrorKind::Dns, format!("Could not resolve '{}': {reason}", host.trim_end_matches('.')))
    })?;
    let answers = addrs
        .iter()
        .map(|ip| DnsResultRecord {
            name: host.clone(),
            record_type: type_name(record_type),
            class: "IN".into(),
            ttl: 0,
            data: ip.to_string(),
        })
        .collect();
    Ok(DnsResult {
        question: DnsQuestion { name: host.clone(), record_type: type_name(record_type), class: "IN".into() },
        answers,
        authority: Vec::new(),
        additional: Vec::new(),
        rcode: "NOERROR".into(),
        rcode_value: 0,
        flags: DnsFlags::default(),
        server: "Operating system".into(),
        protocol: "System".into(),
        system: true,
        tls: None,
        duration_ms: ms(started.elapsed()),
        size: 0,
        notes: vec![format!(
            "{why} The addresses come from the operating system's lookup, without TTLs or DNS details."
        )],
    })
}

/// A name missing from DNS may still resolve on this computer (hosts file, mDNS,
/// VPN split DNS). Say so, since that is what other apps will connect to.
async fn os_resolution_hint(
    result: &DnsResult,
    name: &Name,
    record_type: RecordType,
    deadline: tokio::time::Instant,
) -> Option<String> {
    if !matches!(record_type, RecordType::A | RecordType::AAAA) {
        return None;
    }
    let wanted = type_name(record_type);
    if result.rcode_value != 3 && result.answers.iter().any(|r| r.record_type == wanted) {
        return None;
    }
    let left = deadline.saturating_duration_since(tokio::time::Instant::now()).min(Duration::from_millis(1500));
    let addrs = os_addresses(name, record_type, left).await.ok().filter(|a| !a.is_empty())?;
    let list = addrs.iter().map(IpAddr::to_string).collect::<Vec<_>>().join(", ");
    Some(format!(
        "This computer still resolves the name to {list} (hosts file, mDNS or VPN settings), which is what other apps connect to."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolv_conf_skips_scoped_and_bad_entries() {
        let text = "# comment\nnameserver fe80::1%en0\nnameserver 192.168.1.1\nnameserver 2606:4700::1111\nnameserver nonsense\nsearch lan\nnameserver 192.168.1.1\n";
        let servers = resolv_conf_servers(text);
        assert_eq!(
            servers,
            vec!["192.168.1.1:53".parse::<SocketAddr>().unwrap(), "[2606:4700::1111]:53".parse().unwrap()]
        );
    }

    #[test]
    fn resolvers_parse() {
        let udp = |h: &str, p| DnsResolver::Udp { host: h.into(), port: p };
        assert_eq!(DnsResolver::parse("").unwrap(), DnsResolver::System);
        assert_eq!(DnsResolver::parse(" System ").unwrap(), DnsResolver::System);
        assert_eq!(DnsResolver::parse("1.1.1.1").unwrap(), udp("1.1.1.1", 53));
        assert_eq!(DnsResolver::parse("8.8.8.8:5353").unwrap(), udp("8.8.8.8", 5353));
        assert_eq!(DnsResolver::parse("2606:4700::1111").unwrap(), udp("2606:4700::1111", 53));
        assert_eq!(DnsResolver::parse("[2606:4700::1111]").unwrap(), udp("2606:4700::1111", 53));
        assert_eq!(DnsResolver::parse("[::1]:5300").unwrap(), udp("::1", 5300));
        assert_eq!(DnsResolver::parse("udp://dns.test").unwrap(), udp("dns.test", 53));
        assert_eq!(DnsResolver::parse("tcp://1.1.1.1").unwrap(), DnsResolver::Tcp { host: "1.1.1.1".into(), port: 53 });
        assert_eq!(
            DnsResolver::parse("tls://[2606:4700::1111]").unwrap(),
            DnsResolver::Tls { host: "2606:4700::1111".into(), port: 853 }
        );
        assert_eq!(
            DnsResolver::parse("https://cloudflare-dns.com").unwrap(),
            DnsResolver::Https { url: "https://cloudflare-dns.com/dns-query".into() }
        );
        assert_eq!(
            DnsResolver::parse("https://dns.google/resolve?x=1#f").unwrap(),
            DnsResolver::Https { url: "https://dns.google/resolve?x=1".into() }
        );
        assert!(DnsResolver::parse("ftp://x").is_err());
        assert!(DnsResolver::parse("1.1.1.1:99999").is_err());
        assert!(DnsResolver::parse("1.1.1.1:0").is_err());
        assert!(DnsResolver::parse("[::1").is_err());
        assert!(DnsResolver::parse("tcp://:53").is_err());
    }

    #[test]
    fn names_are_cleaned_and_reversed() {
        let a = |raw: &str| query_name(raw, RecordType::A).map(|n| n.to_string());
        assert_eq!(a("example.com").unwrap(), "example.com.");
        assert_eq!(a("https://user@api.example.com:8443/x?y#z").unwrap(), "api.example.com.");
        assert_eq!(a(" example.com. ").unwrap(), "example.com.");
        assert_eq!(a("_sip._tcp.example.com").unwrap(), "_sip._tcp.example.com.");
        // Sent as punycode, shown as typed.
        let idn = query_name("bücher.example", RecordType::A).unwrap();
        assert_eq!((idn.to_ascii().as_str(), idn.to_string().as_str()), ("xn--bcher-kva.example.", "bücher.example."));
        assert!(a("").unwrap_err().message.contains("Enter a name"));
        assert!(a("1.2.3.4").unwrap_err().message.contains("PTR"));
        assert!(a(&format!("{}.com", "x".repeat(64))).is_err());
        assert!(a("exa mple.com").is_err());
        assert!(a("a..b").is_err());
        let ptr = |raw: &str| query_name(raw, RecordType::PTR).unwrap().to_string();
        assert_eq!(ptr("1.2.3.4"), "4.3.2.1.in-addr.arpa.");
        assert_eq!(ptr("[::1]:53"), format!("1.{}ip6.arpa.", "0.".repeat(31)));
        assert_eq!(ptr("4.3.2.1.in-addr.arpa"), "4.3.2.1.in-addr.arpa.");
    }

    #[test]
    fn record_types() {
        assert_eq!(parse_record_type("mx").unwrap(), RecordType::MX);
        assert_eq!(parse_record_type("").unwrap(), RecordType::A);
        assert_eq!(parse_record_type("GET").unwrap(), RecordType::A);
        assert_eq!(parse_record_type("TYPE65").unwrap(), RecordType::HTTPS);
        assert_eq!(parse_record_type("TYPE65000").unwrap(), RecordType::Unknown(65000));
        assert!(parse_record_type("AXFR").is_err());
        assert!(parse_record_type("NOPE").is_err());
        assert_eq!(type_name(RecordType::Unknown(65000)), "TYPE65000");
    }

    #[test]
    fn txt_is_quoted_and_opt_does_not_panic() {
        assert_eq!(quote_txt(b"v=spf1 \"x\" \\"), r#""v=spf1 \"x\" \\""#);
        assert_eq!(quote_txt(b"a\x00\xff"), r#""a\000\255""#);
        let opt = RData::OPT(Default::default());
        assert_eq!(rdata_text(&opt), "(OPT)");
        assert_eq!(rcode_name(3), "NXDOMAIN");
        assert_eq!(rcode_name(4095), "RCODE4095");
    }

    #[test]
    fn malformed_answers_are_errors() {
        assert!(parse_response(&[0; 5]).is_err());
        // Header claims 65535 answers in a 12-byte message.
        let mut header = [0u8; 12];
        header[6] = 0xff;
        header[7] = 0xff;
        assert!(parse_response(&header).unwrap_err().message.contains("malformed"));
        // Random garbage never panics.
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        for len in 0..600 {
            let bytes: Vec<u8> = (0..len)
                .map(|_| {
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    seed as u8
                })
                .collect();
            if let Ok(m) = parse_response(&bytes) {
                let exchange =
                    DnsExchange { response: bytes, server: String::new(), protocol: "UDP", tls: None, notes: vec![] };
                describe(&m, &exchange, &Name::root(), RecordType::A, vec![]);
            }
        }
    }
}

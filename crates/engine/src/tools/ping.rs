//! Ping: ICMP echo, or TCP connect timing when ICMP is not available.
//!
//! * macOS / Linux: an unprivileged ICMP datagram socket (no root needed; on
//!   Linux the user's group must be in `net.ipv4.ping_group_range`).
//! * Windows: `IcmpSendEcho2` / `Icmp6SendEcho2` (no admin rights needed).
//! * `auto` falls back to TCP connect timing (port 443 by default) when the
//!   ICMP socket can't be opened. The first event says which mode is used.
//!
//! Echoes are sent one at a time: send, wait for the reply (or timeout), then
//! wait out the rest of the interval.

use std::net::SocketAddr;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use ts_rs::TS;

use super::{TcpProbe, ms, tcp_probe};
use crate::error::{EngineError, ErrorKind, Result};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum PingMode {
    /// ICMP when allowed, otherwise TCP.
    #[default]
    Auto,
    Icmp,
    Tcp,
}

#[derive(Debug, Clone, Copy)]
pub struct PingOptions {
    /// Echoes to send; 0 = until cancelled.
    pub count: u32,
    /// Time between the starts of two echoes (clamped to 200 ms – 60 s).
    pub interval: Duration,
    /// How long to wait for each reply (clamped to 100 ms – 30 s).
    pub timeout: Duration,
    pub mode: PingMode,
    /// TCP mode port.
    pub port: u16,
}

impl Default for PingOptions {
    fn default() -> Self {
        Self {
            count: 4,
            interval: Duration::from_secs(1),
            timeout: Duration::from_secs(2),
            mode: PingMode::Auto,
            port: 443,
        }
    }
}

/// How a ping run works; the first event of a run.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PingStarted {
    /// `icmp` or `tcp` (never `auto`).
    pub mode: PingMode,
    pub address: String,
    /// TCP mode: the port connected to.
    pub port: Option<u16>,
    /// Why TCP is used in auto mode, or other remarks.
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PingReply {
    /// 1-based sequence number.
    pub seq: u32,
    /// Round-trip time; `None` when there was no reply.
    pub ms: Option<f64>,
    /// IPv4 time-to-live of the reply, where the platform reports it.
    pub ttl: Option<u8>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PingSummary {
    pub sent: u32,
    pub received: u32,
    pub loss_percent: f64,
    pub min_ms: Option<f64>,
    pub avg_ms: Option<f64>,
    pub max_ms: Option<f64>,
    /// Mean absolute difference between consecutive round trips.
    pub jitter_ms: Option<f64>,
    pub cancelled: bool,
}

/// Running totals (no per-reply storage, so endless runs stay bounded).
#[derive(Default)]
struct Stats {
    sent: u32,
    received: u32,
    sum: f64,
    min: Option<f64>,
    max: Option<f64>,
    last: Option<f64>,
    jitter_sum: f64,
    jitter_n: u32,
}

impl Stats {
    fn add(&mut self, reply: &PingReply) {
        self.sent += 1;
        let Some(rtt) = reply.ms else { return };
        self.received += 1;
        self.sum += rtt;
        self.min = Some(self.min.map_or(rtt, |m| m.min(rtt)));
        self.max = Some(self.max.map_or(rtt, |m| m.max(rtt)));
        if let Some(last) = self.last {
            self.jitter_sum += (rtt - last).abs();
            self.jitter_n += 1;
        }
        self.last = Some(rtt);
    }

    fn summary(&self, cancelled: bool) -> PingSummary {
        let loss =
            if self.sent == 0 { 0.0 } else { f64::from(self.sent - self.received) * 100.0 / f64::from(self.sent) };
        PingSummary {
            sent: self.sent,
            received: self.received,
            loss_percent: loss,
            min_ms: self.min,
            avg_ms: (self.received > 0).then(|| self.sum / f64::from(self.received)),
            max_ms: self.max,
            jitter_ms: (self.jitter_n > 0).then(|| self.jitter_sum / f64::from(self.jitter_n)),
            cancelled,
        }
    }
}

enum Backend {
    Icmp(icmp::Socket),
    Tcp(u16),
}

/// A ping run ready to start (target resolved, mode chosen).
pub struct Pinger {
    addr: SocketAddr,
    backend: Backend,
    options: PingOptions,
    started: PingStarted,
}

impl Pinger {
    /// Resolve `host` and pick the mode. `host` may carry a port
    /// (`example.com:8443`), which is then used for TCP mode.
    pub async fn prepare(host: &str, options: PingOptions) -> Result<Self> {
        let (host, port) = super::parse_target(host)?;
        let port = port.or((options.port != 0).then_some(options.port)).unwrap_or(443);
        let addr = super::resolve_one(&host, port).await?;
        let options = PingOptions {
            interval: options.interval.clamp(Duration::from_millis(200), Duration::from_secs(60)),
            timeout: options.timeout.clamp(Duration::from_millis(100), Duration::from_secs(30)),
            port,
            ..options
        };
        let tcp = |note: Option<String>| {
            (
                Backend::Tcp(port),
                PingStarted { mode: PingMode::Tcp, address: addr.ip().to_string(), port: Some(port), note },
            )
        };
        let icmp_started = PingStarted { mode: PingMode::Icmp, address: addr.ip().to_string(), port: None, note: None };
        let (backend, started) = match options.mode {
            PingMode::Tcp => tcp(None),
            PingMode::Icmp => match icmp::Socket::open(addr) {
                Ok(socket) => (Backend::Icmp(socket), icmp_started),
                Err(e) => return Err(EngineError::new(ErrorKind::Io, icmp_unavailable(&e))),
            },
            PingMode::Auto => match icmp::Socket::open(addr) {
                Ok(socket) => (Backend::Icmp(socket), icmp_started),
                Err(e) => tcp(Some(format!("{} Using TCP connect time to port {port} instead.", icmp_unavailable(&e)))),
            },
        };
        Ok(Self { addr, backend, options, started })
    }

    /// Mode and address, to show before the first reply.
    pub fn started(&self) -> &PingStarted {
        &self.started
    }

    /// Send echoes until `count` is reached or `cancel` fires, reporting each
    /// reply (or loss) through `on_reply`.
    pub async fn run(mut self, cancel: &CancellationToken, mut on_reply: impl FnMut(PingReply)) -> PingSummary {
        let mut stats = Stats::default();
        let mut seq: u32 = 0;
        loop {
            if self.options.count != 0 && seq >= self.options.count {
                break;
            }
            seq += 1;
            let sent_at = tokio::time::Instant::now();
            let reply = tokio::select! {
                biased;
                _ = cancel.cancelled() => return stats.summary(true),
                r = self.echo(seq) => r,
            };
            stats.add(&reply);
            on_reply(reply);
            if self.options.count != 0 && seq >= self.options.count {
                break;
            }
            tokio::select! {
                biased;
                _ = cancel.cancelled() => return stats.summary(true),
                _ = tokio::time::sleep_until(sent_at + self.options.interval) => {}
            }
        }
        stats.summary(false)
    }

    async fn echo(&mut self, seq: u32) -> PingReply {
        let timeout = self.options.timeout;
        match &mut self.backend {
            Backend::Icmp(socket) => match socket.echo(self.addr, seq as u16, timeout).await {
                Ok(echo) => PingReply { seq, ms: Some(echo.ms), ttl: echo.ttl, error: None },
                Err(error) => PingReply { seq, ms: None, ttl: None, error: Some(error) },
            },
            Backend::Tcp(port) => {
                // Keeps the scope of a link-local IPv6 address (`fe80::1%en0`).
                let mut addr = self.addr;
                addr.set_port(*port);
                let (ms, error) = match tcp_probe(addr, timeout).await {
                    TcpProbe::Open(stream, took) => {
                        drop(stream);
                        (Some(ms(took)), None)
                    }
                    TcpProbe::Refused => (None, Some(format!("Connection refused: nothing listens on port {port}"))),
                    TcpProbe::TimedOut => (None, Some("Timed out".to_string())),
                    TcpProbe::Failed(e) => (None, Some(e.to_string())),
                };
                PingReply { seq, ms, ttl: None, error }
            }
        }
    }
}

fn icmp_unavailable(e: &std::io::Error) -> String {
    if e.kind() == std::io::ErrorKind::PermissionDenied {
        "ICMP is not permitted for this user.".to_string()
    } else {
        format!("ICMP is not available ({e}).")
    }
}

/// Reply to one echo request.
struct Echo {
    ms: f64,
    ttl: Option<u8>,
}

/// Payload carried by each echo request (Windows' `ping` sends 32 bytes too).
const PAYLOAD_LEN: usize = 32;

#[cfg(unix)]
mod icmp {
    //! ICMP datagram sockets. The kernel may rewrite the identifier (Linux uses
    //! the socket's port) and may deliver replies meant for other programs'
    //! sockets (macOS), so replies are matched by sequence number and a random
    //! token in the payload rather than by identifier.

    use std::io;
    use std::net::SocketAddr;
    use std::time::{Duration, Instant};

    use socket2::{Domain, Protocol, Type};

    use super::{Echo, PAYLOAD_LEN};

    const V4_ECHO_REQUEST: u8 = 8;
    const V4_ECHO_REPLY: u8 = 0;
    const V4_UNREACHABLE: u8 = 3;
    const V4_TIME_EXCEEDED: u8 = 11;
    const V6_ECHO_REQUEST: u8 = 128;
    const V6_ECHO_REPLY: u8 = 129;
    const V6_UNREACHABLE: u8 = 1;
    const V6_TIME_EXCEEDED: u8 = 3;

    pub(super) struct Socket {
        socket: tokio::net::UdpSocket,
        v6: bool,
        ident: u16,
        token: [u8; 8],
        buf: Vec<u8>,
    }

    impl Socket {
        pub(super) fn open(addr: SocketAddr) -> io::Result<Self> {
            let v6 = addr.is_ipv6();
            let socket = if v6 {
                socket2::Socket::new(Domain::IPV6, Type::DGRAM, Some(Protocol::ICMPV6))?
            } else {
                socket2::Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::ICMPV4))?
            };
            socket.set_nonblocking(true)?;
            let socket = tokio::net::UdpSocket::from_std(std::net::UdpSocket::from(socket))?;
            let token = random_token();
            Ok(Self { socket, v6, ident: u16::from_be_bytes([token[0], token[1]]), token, buf: vec![0u8; 2048] })
        }

        pub(super) async fn echo(&mut self, addr: SocketAddr, seq: u16, timeout: Duration) -> Result<Echo, String> {
            let packet = self.request(seq);
            let deadline = tokio::time::Instant::now() + timeout;
            let started = Instant::now();
            // The port is ignored for ICMP sockets; the scope of a link-local IPv6 address is not.
            let mut dest = addr;
            dest.set_port(0);
            if let Err(e) = self.socket.send_to(&packet, dest).await {
                return Err(send_error(&e));
            }
            loop {
                let received = match tokio::time::timeout_at(deadline, self.socket.recv_from(&mut self.buf)).await {
                    Err(_) => return Err("Request timed out".to_string()),
                    Ok(Err(e)) if e.kind() == io::ErrorKind::Interrupted => continue,
                    // A pending network error (e.g. host unreachable) is reported for this echo.
                    Ok(Err(e)) => return Err(send_error(&e)),
                    Ok(Ok((n, _))) => n,
                };
                let elapsed = started.elapsed();
                match parse_reply(&self.buf[..received], self.v6, seq, &self.token) {
                    Some(Ok(ttl)) => return Ok(Echo { ms: super::ms(elapsed), ttl }),
                    Some(Err(message)) => return Err(message),
                    // Someone else's packet, or a late reply to an earlier echo.
                    None => continue,
                }
            }
        }

        fn request(&self, seq: u16) -> Vec<u8> {
            let mut p = vec![0u8; 8 + PAYLOAD_LEN];
            p[0] = if self.v6 { V6_ECHO_REQUEST } else { V4_ECHO_REQUEST };
            p[4..6].copy_from_slice(&self.ident.to_be_bytes());
            p[6..8].copy_from_slice(&seq.to_be_bytes());
            p[8..16].copy_from_slice(&self.token);
            for (i, b) in p[16..].iter_mut().enumerate() {
                *b = b'a' + (i % 23) as u8;
            }
            // ICMPv6 checksums cover a pseudo-header the kernel fills in.
            if !self.v6 {
                let sum = checksum(&p);
                p[2..4].copy_from_slice(&sum.to_be_bytes());
            }
            p
        }
    }

    fn send_error(e: &io::Error) -> String {
        match e.raw_os_error() {
            Some(code) if is_unreachable(code) => format!("Destination unreachable ({e})"),
            _ => e.to_string(),
        }
    }

    fn is_unreachable(code: i32) -> bool {
        // ENETUNREACH / EHOSTUNREACH differ between macOS and Linux.
        #[cfg(target_os = "linux")]
        return code == 101 || code == 113;
        #[cfg(not(target_os = "linux"))]
        return code == 51 || code == 65;
    }

    /// Match a received datagram against the echo `seq` we sent. `None` = not ours.
    /// `Some(Ok(ttl))` = our reply; `Some(Err(..))` = an ICMP error about our request.
    pub(super) fn parse_reply(data: &[u8], v6: bool, seq: u16, token: &[u8; 8]) -> Option<Result<Option<u8>, String>> {
        // macOS includes the IPv4 header on ICMP datagram sockets; Linux does not.
        let (icmp, ttl) = if !v6 { strip_ipv4_header(data)? } else { (data, None) };
        if icmp.len() < 8 {
            return None;
        }
        let (kind, code) = (icmp[0], icmp[1]);
        let reply_type = if v6 { V6_ECHO_REPLY } else { V4_ECHO_REPLY };
        if kind == reply_type {
            let ours = u16::from_be_bytes([icmp[6], icmp[7]]) == seq && icmp.get(8..16) == Some(&token[..]);
            return ours.then_some(Ok(ttl));
        }
        // Errors quote the original packet: its IP header, then our ICMP header.
        let (quoted, message) = if v6 {
            let inner = icmp.get(8 + 40..)?;
            let message = match kind {
                V6_UNREACHABLE => unreachable_v6(code),
                V6_TIME_EXCEEDED => "Hop limit exceeded in transit".to_string(),
                _ => return None,
            };
            (inner, message)
        } else {
            let (inner, _) = strip_ipv4_header(icmp.get(8..)?)?;
            let message = match kind {
                V4_UNREACHABLE => unreachable_v4(code),
                V4_TIME_EXCEEDED => "TTL expired in transit".to_string(),
                _ => return None,
            };
            (inner, message)
        };
        let request_type = if v6 { V6_ECHO_REQUEST } else { V4_ECHO_REQUEST };
        (quoted.len() >= 8 && quoted[0] == request_type && u16::from_be_bytes([quoted[6], quoted[7]]) == seq)
            .then_some(Err(message))
    }

    /// Skip an IPv4 header if the data starts with one, returning the payload and TTL.
    fn strip_ipv4_header(data: &[u8]) -> Option<(&[u8], Option<u8>)> {
        let first = *data.first()?;
        if first >> 4 != 4 {
            return Some((data, None));
        }
        let header_len = usize::from(first & 0x0f) * 4;
        if header_len < 20 || data.len() < header_len {
            return None;
        }
        Some((&data[header_len..], Some(data[8])))
    }

    fn unreachable_v4(code: u8) -> String {
        match code {
            0 => "Destination network unreachable",
            1 => "Destination host unreachable",
            2 => "Destination protocol unreachable",
            3 => "Destination port unreachable",
            9 | 10 | 13 => "Communication administratively prohibited",
            _ => "Destination unreachable",
        }
        .to_string()
    }

    fn unreachable_v6(code: u8) -> String {
        match code {
            0 => "No route to destination",
            1 => "Communication administratively prohibited",
            3 => "Destination address unreachable",
            4 => "Destination port unreachable",
            _ => "Destination unreachable",
        }
        .to_string()
    }

    /// RFC 1071 Internet checksum.
    pub(super) fn checksum(data: &[u8]) -> u16 {
        let mut sum: u32 = 0;
        for chunk in data.chunks(2) {
            let word =
                if chunk.len() == 2 { u16::from_be_bytes([chunk[0], chunk[1]]) } else { u16::from(chunk[0]) << 8 };
            sum += u32::from(word);
        }
        while sum >> 16 != 0 {
            sum = (sum & 0xffff) + (sum >> 16);
        }
        !(sum as u16)
    }

    /// Unpredictable enough to tell our echoes from other programs' (not for security).
    fn random_token() -> [u8; 8] {
        use std::collections::hash_map::RandomState;
        use std::hash::{BuildHasher, Hasher};
        let mut h = RandomState::new().build_hasher();
        h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos()));
        h.write_u32(std::process::id());
        h.finish().to_be_bytes()
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn checksum_matches_rfc1071_example() {
            // Echo request, id 1, seq 1, no payload: 0xf7fd.
            let p = [8u8, 0, 0, 0, 0, 1, 0, 1];
            assert_eq!(checksum(&p), 0xf7fd);
            // Odd length is padded with zero.
            assert_eq!(checksum(&[0xff]), !0xff00);
        }

        #[test]
        fn matches_replies_with_or_without_ip_header() {
            let token = [1, 2, 3, 4, 5, 6, 7, 8];
            let mut reply = vec![V4_ECHO_REPLY, 0, 0, 0, 0x12, 0x34, 0, 7];
            reply.extend_from_slice(&token);
            reply.extend_from_slice(&[0u8; 24]);
            // Linux: ICMP only.
            assert_eq!(parse_reply(&reply, false, 7, &token), Some(Ok(None)));
            assert_eq!(parse_reply(&reply, false, 8, &token), None);
            assert_eq!(parse_reply(&reply, false, 7, &[0; 8]), None);
            // macOS: with a 20-byte IPv4 header (TTL 64).
            let mut with_ip = vec![0x45, 0, 0, 0, 0, 0, 0, 0, 64, 1, 0, 0, 127, 0, 0, 1, 127, 0, 0, 1];
            with_ip.extend_from_slice(&reply);
            assert_eq!(parse_reply(&with_ip, false, 7, &token), Some(Ok(Some(64))));
            // Our own request looped back is ignored.
            let mut request = reply.clone();
            request[0] = V4_ECHO_REQUEST;
            assert_eq!(parse_reply(&request, false, 7, &token), None);
            // IPv6 reply.
            let mut v6 = reply.clone();
            v6[0] = V6_ECHO_REPLY;
            assert_eq!(parse_reply(&v6, true, 7, &token), Some(Ok(None)));
        }

        #[test]
        fn reports_unreachable_errors_for_our_request() {
            let token = [0u8; 8];
            // Host unreachable quoting: IPv4 header + our echo request (seq 3).
            let mut error = vec![V4_UNREACHABLE, 1, 0, 0, 0, 0, 0, 0];
            error.extend_from_slice(&[0x45, 0, 0, 0, 0, 0, 0, 0, 64, 1, 0, 0, 10, 0, 0, 1, 10, 0, 0, 2]);
            error.extend_from_slice(&[V4_ECHO_REQUEST, 0, 0, 0, 0, 1, 0, 3]);
            assert_eq!(parse_reply(&error, false, 3, &token), Some(Err("Destination host unreachable".into())));
            assert_eq!(parse_reply(&error, false, 4, &token), None);
        }

        #[test]
        fn never_panics_on_garbage() {
            let token = [0u8; 8];
            for len in 0..80 {
                for fill in [0x00u8, 0x45, 0x4f, 0xff, 3, 11, 1] {
                    let data = vec![fill; len];
                    let _ = parse_reply(&data, false, 0, &token);
                    let _ = parse_reply(&data, true, 0, &token);
                }
            }
        }
    }
}

#[cfg(windows)]
mod icmp {
    //! ICMP through the IP Helper API, which needs no admin rights. The calls
    //! block, so each echo runs on the blocking thread pool (bounded by the timeout).

    use std::io;
    use std::net::SocketAddr;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use windows_sys::Win32::Foundation::{GetLastError, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        ICMP_ECHO_REPLY, ICMPV6_ECHO_REPLY_LH, IP_DEST_HOST_UNREACHABLE, IP_DEST_NET_UNREACHABLE,
        IP_DEST_PORT_UNREACHABLE, IP_DEST_PROHIBITED, IP_DEST_UNREACHABLE, IP_REQ_TIMED_OUT, IP_SUCCESS,
        IP_TIME_EXCEEDED, IP_TTL_EXPIRED_TRANSIT, Icmp6CreateFile, Icmp6ParseReplies, Icmp6SendEcho2, IcmpCloseHandle,
        IcmpCreateFile, IcmpSendEcho2,
    };
    use windows_sys::Win32::Networking::WinSock::{AF_INET6, IN6_ADDR, IN6_ADDR_0, SOCKADDR_IN6, SOCKADDR_IN6_0};

    use super::{Echo, PAYLOAD_LEN};

    struct Handle(HANDLE);

    // SAFETY: an ICMP handle may be used from any thread; echoes on it never overlap.
    unsafe impl Send for Handle {}
    unsafe impl Sync for Handle {}

    impl Drop for Handle {
        fn drop(&mut self) {
            // SAFETY: the handle came from Icmp(6)CreateFile and is closed once.
            unsafe { IcmpCloseHandle(self.0) };
        }
    }

    pub(super) struct Socket {
        handle: Arc<Handle>,
    }

    impl Socket {
        pub(super) fn open(addr: SocketAddr) -> io::Result<Self> {
            // SAFETY: plain constructors without arguments.
            let handle = unsafe { if addr.is_ipv6() { Icmp6CreateFile() } else { IcmpCreateFile() } };
            if handle == INVALID_HANDLE_VALUE || handle.is_null() {
                return Err(io::Error::last_os_error());
            }
            Ok(Self { handle: Arc::new(Handle(handle)) })
        }

        pub(super) async fn echo(&mut self, addr: SocketAddr, _seq: u16, timeout: Duration) -> Result<Echo, String> {
            let handle = self.handle.clone();
            tokio::task::spawn_blocking(move || echo_blocking(&handle, addr, timeout))
                .await
                .map_err(|e| format!("Ping failed: {e}"))?
        }
    }

    fn echo_blocking(handle: &Handle, addr: SocketAddr, timeout: Duration) -> Result<Echo, String> {
        let payload = [b'a'; PAYLOAD_LEN];
        let timeout_ms = timeout.as_millis().clamp(1, u128::from(u32::MAX)) as u32;
        // Room for the reply struct, the echoed data and an ICMP error; u64s keep it aligned.
        let reply_size = std::mem::size_of::<ICMP_ECHO_REPLY>() + PAYLOAD_LEN + 8 + 64;
        let mut buf = vec![0u64; reply_size.div_ceil(8)];
        let buf_len = (buf.len() * 8) as u32;
        let started = Instant::now();
        match addr {
            SocketAddr::V4(v4) => {
                // SAFETY: all pointers are valid for the duration of the call; the
                // reply buffer is writable, aligned and large enough (see above).
                let count = unsafe {
                    IcmpSendEcho2(
                        handle.0,
                        std::ptr::null_mut(),
                        None,
                        std::ptr::null(),
                        u32::from_ne_bytes(v4.ip().octets()),
                        payload.as_ptr().cast(),
                        PAYLOAD_LEN as u16,
                        std::ptr::null(),
                        buf.as_mut_ptr().cast(),
                        buf_len,
                        timeout_ms,
                    )
                };
                let elapsed = started.elapsed();
                // SAFETY: the buffer is aligned for and at least as large as ICMP_ECHO_REPLY.
                let reply = unsafe { &*(buf.as_ptr() as *const ICMP_ECHO_REPLY) };
                if count == 0 {
                    // SAFETY: reads the calling thread's last error.
                    let code = unsafe { GetLastError() };
                    return Err(status_message(if code != 0 { code } else { reply.Status }));
                }
                if reply.Status != IP_SUCCESS {
                    return Err(status_message(reply.Status));
                }
                Ok(Echo { ms: super::ms(elapsed), ttl: Some(reply.Options.Ttl) })
            }
            SocketAddr::V6(v6) => {
                let source = SOCKADDR_IN6 { sin6_family: AF_INET6, ..Default::default() };
                let dest = SOCKADDR_IN6 {
                    sin6_family: AF_INET6,
                    sin6_port: 0,
                    sin6_flowinfo: 0,
                    sin6_addr: IN6_ADDR { u: IN6_ADDR_0 { Byte: v6.ip().octets() } },
                    Anonymous: SOCKADDR_IN6_0 { sin6_scope_id: v6.scope_id() },
                };
                // SAFETY: as above; the socket addresses outlive the call.
                let count = unsafe {
                    Icmp6SendEcho2(
                        handle.0,
                        std::ptr::null_mut(),
                        None,
                        std::ptr::null(),
                        &source,
                        &dest,
                        payload.as_ptr().cast(),
                        PAYLOAD_LEN as u16,
                        std::ptr::null(),
                        buf.as_mut_ptr().cast(),
                        buf_len,
                        timeout_ms,
                    )
                };
                let elapsed = started.elapsed();
                if count == 0 {
                    // SAFETY: reads the calling thread's last error.
                    return Err(status_message(unsafe { GetLastError() }));
                }
                // SAFETY: the buffer holds the replies written by Icmp6SendEcho2.
                if unsafe { Icmp6ParseReplies(buf.as_mut_ptr().cast(), buf_len) } == 0 {
                    return Err(status_message(unsafe { GetLastError() }));
                }
                // SAFETY: the buffer is aligned for and larger than ICMPV6_ECHO_REPLY_LH.
                let reply = unsafe { &*(buf.as_ptr() as *const ICMPV6_ECHO_REPLY_LH) };
                if reply.Status != IP_SUCCESS {
                    return Err(status_message(reply.Status));
                }
                Ok(Echo { ms: super::ms(elapsed), ttl: None })
            }
        }
    }

    fn status_message(status: u32) -> String {
        match status {
            IP_REQ_TIMED_OUT => "Request timed out".to_string(),
            IP_DEST_HOST_UNREACHABLE => "Destination host unreachable".to_string(),
            IP_DEST_NET_UNREACHABLE => "Destination network unreachable".to_string(),
            IP_DEST_PORT_UNREACHABLE => "Destination port unreachable".to_string(),
            IP_DEST_PROHIBITED => "Communication administratively prohibited".to_string(),
            IP_DEST_UNREACHABLE => "Destination unreachable".to_string(),
            IP_TTL_EXPIRED_TRANSIT | IP_TIME_EXCEEDED => "TTL expired in transit".to_string(),
            other => format!("Ping failed ({})", io::Error::from_raw_os_error(other as i32)),
        }
    }
}

#[cfg(not(any(unix, windows)))]
mod icmp {
    use std::io;
    use std::net::SocketAddr;
    use std::time::Duration;

    use super::Echo;

    pub(super) struct Socket;

    impl Socket {
        pub(super) fn open(_: SocketAddr) -> io::Result<Self> {
            Err(io::Error::new(io::ErrorKind::Unsupported, "not supported on this platform"))
        }

        pub(super) async fn echo(&mut self, _: SocketAddr, _: u16, _: Duration) -> Result<Echo, String> {
            Err("ICMP is not supported on this platform".to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats_track_loss_and_jitter() {
        let mut s = Stats::default();
        for (seq, ms) in [(1, Some(10.0)), (2, None), (3, Some(14.0)), (4, Some(12.0))] {
            s.add(&PingReply { seq, ms, ttl: None, error: None });
        }
        let sum = s.summary(false);
        assert_eq!((sum.sent, sum.received), (4, 3));
        assert!((sum.loss_percent - 25.0).abs() < 1e-9);
        assert_eq!((sum.min_ms, sum.max_ms), (Some(10.0), Some(14.0)));
        assert!((sum.avg_ms.unwrap() - 12.0).abs() < 1e-9);
        assert!((sum.jitter_ms.unwrap() - 3.0).abs() < 1e-9);
        let empty = Stats::default().summary(true);
        assert_eq!(empty.loss_percent, 0.0);
        assert!(empty.avg_ms.is_none() && empty.cancelled);
    }
}

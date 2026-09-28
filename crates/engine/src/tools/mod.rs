//! Network tools below the request level: TLS inspection, TCP port checks,
//! ping (ICMP, or TCP connect timing), the local network interfaces and
//! which process holds a local port.
//!
//! Each tool validates and resolves its target first, so bad input fails
//! right away, then runs with bounded concurrency and per-step timeouts.
//! Streaming tools report through a callback and stop on a cancellation token.

pub mod interfaces;
pub mod ping;
pub mod port_owner;
pub mod ports;
pub mod tls_inspect;

use std::io;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use tokio::net::{TcpSocket, TcpStream};

use crate::error::{EngineError, ErrorKind, Result};

pub use interfaces::{IpFamily, NetAddress, NetInterface, list_interfaces};
pub use ping::{PingMode, PingOptions, PingReply, PingStarted, PingSummary, Pinger};
pub use port_owner::{PortOwner, Protocol, port_owner};
pub use ports::{MAX_PORTS, PortErrorKind, PortResult, PortScan, PortScanOptions, PortScanSummary, parse_ports};
pub use tls_inspect::{
    TlsCertificate, TlsCipherSupport, TlsInspectOptions, TlsReport, TlsVersionSupport, TlsWarning, TlsWarningLevel,
    inspect_tls,
};

/// How long resolving a tool's target may take.
const DNS_TIMEOUT: Duration = Duration::from_secs(10);

/// Split user input into a host and an optional port. Accepts `host`,
/// `host:port`, `[v6]:port`, bare IPv6 addresses and URLs
/// (`https://example.com/path` → `example.com`, port 443).
pub fn parse_target(input: &str) -> Result<(String, Option<u16>)> {
    let input = input.trim();
    if input.is_empty() {
        return Err(EngineError::invalid("Enter a host name or IP address"));
    }
    if input.contains("://") {
        let url =
            url::Url::parse(input).map_err(|e| EngineError::invalid(format!("Invalid address '{input}': {e}")))?;
        let host = url
            .host_str()
            .filter(|h| !h.is_empty())
            .ok_or_else(|| EngineError::invalid(format!("'{input}' has no host")))?;
        let host = host.trim_start_matches('[').trim_end_matches(']').to_string();
        return Ok((host, url.port_or_known_default()));
    }
    // Drop a path (`example.com/x`).
    let authority = input.split('/').next().unwrap_or_default();
    let (host, port) = if let Some(rest) = authority.strip_prefix('[') {
        let (host, after) =
            rest.split_once(']').ok_or_else(|| EngineError::invalid(format!("Missing ']' in '{input}'")))?;
        match after {
            "" => (host, None),
            p => match p.strip_prefix(':') {
                Some(p) => (host, Some(p)),
                None => return Err(EngineError::invalid(format!("Unexpected '{p}' after the address in '{input}'"))),
            },
        }
    } else if authority.matches(':').count() > 1 {
        // A bare IPv6 address (no port possible without brackets).
        (authority, None)
    } else {
        match authority.split_once(':') {
            Some((h, p)) => (h, Some(p)),
            None => (authority, None),
        }
    };
    validate_host(host)?;
    let port = port.map(parse_port).transpose()?;
    Ok((host.to_string(), port))
}

fn validate_host(host: &str) -> Result<()> {
    // Names are passed to the resolver as typed (IPv6 zones like `fe80::1%en0`
    // and internal names with `_` included); only reject what can't be a host.
    let invalid = host.is_empty()
        || host.len() > 300
        || host.starts_with(['-', '.'])
        || host.chars().any(|c| {
            c.is_whitespace()
                || c.is_control()
                || matches!(c, '@' | '/' | '\\' | '?' | '#' | '[' | ']' | '<' | '>' | '"')
        });
    if invalid {
        return Err(EngineError::invalid(format!("'{host}' is not a valid host name or IP address")));
    }
    Ok(())
}

fn parse_port(p: &str) -> Result<u16> {
    match p.trim().parse::<u32>() {
        Ok(n) if (1..=65535).contains(&n) => Ok(n as u16),
        Ok(_) => Err(EngineError::invalid(format!("Port {p} is out of range (1–65535)"))),
        Err(_) => Err(EngineError::invalid(format!("Invalid port '{p}'"))),
    }
}

/// Resolve `host` to one address. IPv4 is preferred when a name has both
/// (like nmap does): local servers often listen on 127.0.0.1 only while
/// `localhost` may resolve to ::1 first. An IPv6 literal is used as given.
pub(crate) async fn resolve_one(host: &str, port: u16) -> Result<SocketAddr> {
    let addrs = crate::net::resolve(host, port, DNS_TIMEOUT).await?;
    addrs
        .iter()
        .find(|a| a.is_ipv4())
        .or(addrs.first())
        .copied()
        .ok_or_else(|| EngineError::new(ErrorKind::Dns, format!("Host '{host}' has no addresses")))
}

/// Outcome of one TCP connection attempt.
pub(crate) enum TcpProbe {
    Open(TcpStream, Duration),
    Refused,
    TimedOut,
    Failed(io::Error),
}

/// One TCP connect with a hard timeout and no SYN retries on Windows (see
/// [`no_syn_retransmissions`]), so a closed port answers as fast as it does
/// on macOS and Linux.
pub(crate) async fn tcp_probe(addr: SocketAddr, timeout: Duration) -> TcpProbe {
    let started = Instant::now();
    let socket = match if addr.is_ipv4() { TcpSocket::new_v4() } else { TcpSocket::new_v6() } {
        Ok(s) => s,
        Err(e) => return TcpProbe::Failed(e),
    };
    #[cfg(windows)]
    no_syn_retransmissions(&socket);
    match tokio::time::timeout(timeout, socket.connect(addr)).await {
        Ok(Ok(stream)) => TcpProbe::Open(stream, started.elapsed()),
        Ok(Err(e)) if e.kind() == io::ErrorKind::ConnectionRefused => TcpProbe::Refused,
        Ok(Err(e)) if e.kind() == io::ErrorKind::TimedOut => TcpProbe::TimedOut,
        Ok(Err(e)) => TcpProbe::Failed(e),
        Err(_) => TcpProbe::TimedOut,
    }
}

/// Windows answers a RST to its SYN by retrying the SYN (about 2 s in total)
/// before reporting "refused". One probe should be one SYN, so turn the
/// retries off for this socket. Best effort: errors keep the default.
#[cfg(windows)]
fn no_syn_retransmissions(socket: &TcpSocket) {
    use std::os::windows::io::AsRawSocket;
    use windows_sys::Win32::Networking::WinSock::{
        SIO_TCP_INITIAL_RTO, TCP_INITIAL_RTO_NO_SYN_RETRANSMISSIONS, TCP_INITIAL_RTO_PARAMETERS, WSAIoctl,
    };
    let params = TCP_INITIAL_RTO_PARAMETERS {
        // TCP_INITIAL_RTO_UNSPECIFIED_RTT: keep the system's initial RTO.
        Rtt: u16::MAX,
        // The SDK defines this as `(UCHAR)-2` for a UCHAR field.
        MaxSynRetransmissions: TCP_INITIAL_RTO_NO_SYN_RETRANSMISSIONS as u8,
    };
    let mut returned = 0u32;
    // SAFETY: plain ioctl on a socket we own; the input buffer outlives the call
    // and no output buffer or overlapped structure is used.
    unsafe {
        WSAIoctl(
            socket.as_raw_socket() as usize,
            SIO_TCP_INITIAL_RTO,
            (&params as *const TCP_INITIAL_RTO_PARAMETERS).cast(),
            std::mem::size_of::<TCP_INITIAL_RTO_PARAMETERS>() as u32,
            std::ptr::null_mut(),
            0,
            &mut returned,
            std::ptr::null_mut(),
            None,
        );
    }
}

pub(crate) fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_targets() {
        let t = |s: &str| parse_target(s).unwrap();
        assert_eq!(t("example.com"), ("example.com".into(), None));
        assert_eq!(t("  example.com:8443 "), ("example.com".into(), Some(8443)));
        assert_eq!(t("https://example.com/path?q=1"), ("example.com".into(), Some(443)));
        assert_eq!(t("http://example.com:8080"), ("example.com".into(), Some(8080)));
        assert_eq!(t("[::1]:443"), ("::1".into(), Some(443)));
        assert_eq!(t("[::1]"), ("::1".into(), None));
        assert_eq!(t("fe80::1%en0"), ("fe80::1%en0".into(), None));
        assert_eq!(t("10.0.0.1/24"), ("10.0.0.1".into(), None));
        assert_eq!(t("my_host.internal"), ("my_host.internal".into(), None));
        assert!(parse_target("").is_err());
        assert!(parse_target("example.com:0").is_err());
        assert!(parse_target("example.com:99999").is_err());
        assert!(parse_target("example.com:http").is_err());
        assert!(parse_target("exa mple.com").is_err());
        assert!(parse_target("user@example.com").is_err());
        assert!(parse_target("[::1").is_err());
        assert!(parse_target("-bad").is_err());
    }
}

//! TCP port check: which ports on a host accept connections, and how fast.
//! The host is resolved once; ports are probed with bounded concurrency and
//! a per-port timeout. Results stream as they arrive.

use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use serde::Serialize;
use tokio_util::sync::CancellationToken;
use ts_rs::TS;

use super::{TcpProbe, ms, tcp_probe};
use crate::error::{EngineError, Result};

/// Most ports one check may probe.
pub const MAX_PORTS: usize = 1024;

/// Why a port did not accept the connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum PortErrorKind {
    /// The host answered with a reset: nothing listens there.
    Refused,
    /// No answer in time (often a firewall dropping packets).
    Timeout,
    Other,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PortResult {
    pub port: u16,
    pub open: bool,
    /// Time until the connection opened or failed.
    pub ms: f64,
    pub error: Option<PortErrorKind>,
    /// Details for [`PortErrorKind::Other`].
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PortScanSummary {
    /// The address that was checked.
    pub address: String,
    /// Open ports, ascending.
    pub open: Vec<u16>,
    pub refused: u32,
    pub timed_out: u32,
    pub errors: u32,
    /// Ports checked (less than `total` when cancelled).
    pub checked: u32,
    pub total: u32,
    pub duration_ms: f64,
    pub cancelled: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct PortScanOptions {
    /// Per-port connect timeout (clamped to 100 ms – 30 s).
    pub timeout: Duration,
    /// Connections in flight at once (clamped to 1 – 256).
    pub concurrency: usize,
}

impl Default for PortScanOptions {
    fn default() -> Self {
        Self { timeout: Duration::from_secs(2), concurrency: 64 }
    }
}

/// Parse a port list such as `80,443,8000-8010` (commas or spaces). Returns
/// unique ports in ascending order; at most [`MAX_PORTS`].
pub fn parse_ports(spec: &str) -> Result<Vec<u16>> {
    let mut ranges: Vec<(u16, u16)> = Vec::new();
    let mut count = 0usize;
    for token in spec.split(|c: char| c == ',' || c == ';' || c.is_whitespace()).filter(|t| !t.is_empty()) {
        let (a, b) = match token.split_once('-') {
            Some((a, b)) => (port_number(a, token)?, port_number(b, token)?),
            None => {
                let p = port_number(token, token)?;
                (p, p)
            }
        };
        if a > b {
            return Err(EngineError::invalid(format!("Invalid range '{token}': the start is after the end")));
        }
        count += usize::from(b - a) + 1;
        if count > MAX_PORTS * 64 {
            // Stop early on huge inputs; the exact total is reported below otherwise.
            return Err(too_many(count));
        }
        ranges.push((a, b));
    }
    if ranges.is_empty() {
        return Err(EngineError::invalid("Enter at least one port, e.g. 80,443 or 8000-8010"));
    }
    let mut ports: Vec<u16> = ranges.into_iter().flat_map(|(a, b)| a..=b).collect();
    ports.sort_unstable();
    ports.dedup();
    if ports.len() > MAX_PORTS {
        return Err(too_many(ports.len()));
    }
    Ok(ports)
}

fn port_number(s: &str, token: &str) -> Result<u16> {
    match s.trim().parse::<u32>() {
        Ok(n) if (1..=65535).contains(&n) => Ok(n as u16),
        Ok(_) => Err(EngineError::invalid(format!("Port '{token}' is out of range (1–65535)"))),
        Err(_) => Err(EngineError::invalid(format!("'{token}' is not a port or range (e.g. 443 or 8000-8010)"))),
    }
}

fn too_many(n: usize) -> EngineError {
    EngineError::invalid(format!("That is {n} ports; check at most {MAX_PORTS} at a time"))
}

/// A port check ready to run (target validated and resolved).
#[derive(Debug)]
pub struct PortScan {
    /// Port 0; kept as a socket address for the scope of link-local IPv6 addresses.
    addr: SocketAddr,
    ports: Vec<u16>,
    options: PortScanOptions,
}

impl PortScan {
    /// Resolve `host` once (the system's preferred address) and validate the ports.
    pub async fn prepare(host: &str, ports: Vec<u16>, options: PortScanOptions) -> Result<Self> {
        if ports.is_empty() {
            return Err(EngineError::invalid("Enter at least one port"));
        }
        if ports.len() > MAX_PORTS {
            return Err(too_many(ports.len()));
        }
        if ports.contains(&0) {
            return Err(EngineError::invalid("Port 0 is not a valid port"));
        }
        let (host, port) = super::parse_target(host)?;
        if port.is_some() {
            return Err(EngineError::invalid("Enter the host without a port, and list the ports to check below"));
        }
        let addr = super::resolve_one(&host, 0).await?;
        let options = PortScanOptions {
            timeout: options.timeout.clamp(Duration::from_millis(100), Duration::from_secs(30)),
            concurrency: options.concurrency.clamp(1, 256),
        };
        let mut ports = ports;
        ports.sort_unstable();
        ports.dedup();
        Ok(Self { addr, ports, options })
    }

    pub fn address(&self) -> IpAddr {
        self.addr.ip()
    }

    pub fn total(&self) -> usize {
        self.ports.len()
    }

    /// Probe every port, calling `on_result` as each finishes (in completion
    /// order). Stops early when `cancel` fires; connections in flight are dropped.
    pub async fn run(self, cancel: &CancellationToken, mut on_result: impl FnMut(PortResult)) -> PortScanSummary {
        let started = Instant::now();
        let addr = self.addr;
        let timeout = self.options.timeout;
        let mut summary = PortScanSummary {
            address: addr.ip().to_string(),
            open: Vec::new(),
            refused: 0,
            timed_out: 0,
            errors: 0,
            checked: 0,
            total: self.ports.len() as u32,
            duration_ms: 0.0,
            cancelled: false,
        };
        let mut probes = futures_util::stream::iter(self.ports)
            .map(|port| async move {
                let mut addr = addr;
                addr.set_port(port);
                check_port(addr, timeout).await
            })
            .buffer_unordered(self.options.concurrency);
        loop {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => {
                    summary.cancelled = true;
                    break;
                }
                next = probes.next() => match next {
                    Some(result) => {
                        summary.checked += 1;
                        match result.error {
                            None => summary.open.push(result.port),
                            Some(PortErrorKind::Refused) => summary.refused += 1,
                            Some(PortErrorKind::Timeout) => summary.timed_out += 1,
                            Some(PortErrorKind::Other) => summary.errors += 1,
                        }
                        on_result(result);
                    }
                    None => break,
                },
            }
        }
        summary.open.sort_unstable();
        summary.duration_ms = ms(started.elapsed());
        summary
    }
}

async fn check_port(addr: SocketAddr, timeout: Duration) -> PortResult {
    let port = addr.port();
    let started = Instant::now();
    let result = |open, error, message| PortResult { port, open, ms: ms(started.elapsed()), error, message };
    match tcp_probe(addr, timeout).await {
        TcpProbe::Open(stream, _) => {
            drop(stream);
            result(true, None, None)
        }
        TcpProbe::Refused => result(false, Some(PortErrorKind::Refused), None),
        TcpProbe::TimedOut => result(false, Some(PortErrorKind::Timeout), None),
        TcpProbe::Failed(e) => result(false, Some(PortErrorKind::Other), Some(e.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_port_lists() {
        assert_eq!(parse_ports("443, 80 8000-8002;22").unwrap(), vec![22, 80, 443, 8000, 8001, 8002]);
        assert_eq!(parse_ports("80,80,79-81").unwrap(), vec![79, 80, 81]);
        assert_eq!(parse_ports("65535").unwrap(), vec![65535]);
        assert_eq!(parse_ports("1-1024").unwrap().len(), 1024);
        assert!(parse_ports("1-1025").unwrap_err().message.contains("1025 ports"));
        assert!(parse_ports("1-65535").is_err());
        assert!(parse_ports("").is_err());
        assert!(parse_ports(" , ").is_err());
        assert!(parse_ports("0").is_err());
        assert!(parse_ports("65536").is_err());
        assert!(parse_ports("10-5").unwrap_err().message.contains("start is after the end"));
        assert!(parse_ports("http").is_err());
        assert!(parse_ports("1-2-3").is_err());
        assert!(parse_ports("-5").is_err());
        // Huge repeated ranges fail fast instead of allocating.
        assert!(parse_ports(&"1-65535,".repeat(10_000)).is_err());
    }
}

//! Connection establishment with per-phase timing:
//! DNS lookup, TCP connect (Happy Eyeballs), optional proxy tunnel, TLS.

use std::io;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use futures_util::stream::FuturesUnordered;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;

use crate::error::{EngineError, ErrorKind, Result};
use crate::proxy::ProxyEndpoint;
use crate::tls::{self, Alpn, TlsConfigCache, TlsInfo, TlsOptions};

/// A TCP stream, optionally wrapped in TLS.
pub(crate) enum Stream {
    Plain(TcpStream),
    Tls(Box<TlsStream<TcpStream>>),
}

impl AsyncRead for Stream {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Stream::Plain(s) => Pin::new(s).poll_read(cx, buf),
            Stream::Tls(s) => Pin::new(s.as_mut()).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for Stream {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            Stream::Plain(s) => Pin::new(s).poll_write(cx, buf),
            Stream::Tls(s) => Pin::new(s.as_mut()).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Stream::Plain(s) => Pin::new(s).poll_flush(cx),
            Stream::Tls(s) => Pin::new(s.as_mut()).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Stream::Plain(s) => Pin::new(s).poll_shutdown(cx),
            Stream::Tls(s) => Pin::new(s.as_mut()).poll_shutdown(cx),
        }
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            Stream::Plain(s) => Pin::new(s).poll_write_vectored(cx, bufs),
            Stream::Tls(s) => Pin::new(s.as_mut()).poll_write_vectored(cx, bufs),
        }
    }

    fn is_write_vectored(&self) -> bool {
        match self {
            Stream::Plain(s) => s.is_write_vectored(),
            Stream::Tls(s) => s.is_write_vectored(),
        }
    }
}

/// Where to connect and how.
pub(crate) struct Target<'a> {
    pub host: &'a str,
    pub port: u16,
    pub tls: bool,
    pub alpn: Alpn,
    pub tls_options: &'a TlsOptions,
    pub proxy: Option<&'a ProxyEndpoint>,
    /// Use a CONNECT tunnel through the proxy even without TLS (WebSocket).
    pub force_tunnel: bool,
    pub connect_timeout: Duration,
}

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct ConnectTiming {
    pub dns: Duration,
    pub connect: Duration,
    pub tls: Duration,
}

pub(crate) struct Connection {
    pub stream: Stream,
    pub remote_addr: SocketAddr,
    pub tls: Option<TlsInfo>,
    pub timing: ConnectTiming,
    /// True when talking to an HTTP proxy without a tunnel (plain http target):
    /// requests must use absolute-form URIs and carry Proxy-Authorization.
    pub via_forward_proxy: bool,
}

impl Connection {
    pub fn negotiated_h2(&self) -> bool {
        self.tls.as_ref().and_then(|t| t.alpn.as_deref()) == Some("h2")
    }
}

pub(crate) async fn connect(target: &Target<'_>, tls_cache: &TlsConfigCache) -> Result<Connection> {
    let mut timing = ConnectTiming::default();
    let (dial_host, dial_port) = match target.proxy {
        Some(p) => (p.host.as_str(), p.port),
        None => (target.host, target.port),
    };

    let started = Instant::now();
    let addrs = resolve(dial_host, dial_port, target.connect_timeout).await.map_err(|e| {
        if target.proxy.is_some() { EngineError::new(ErrorKind::Proxy, format!("Proxy {}", e.message)) } else { e }
    })?;
    timing.dns = started.elapsed();

    let remaining = target.connect_timeout.saturating_sub(timing.dns).max(Duration::from_millis(1));
    let connect_started = Instant::now();
    let (mut tcp, remote_addr) = connect_any(&addrs, remaining).await.map_err(|e| {
        if target.proxy.is_some() {
            EngineError::new(
                ErrorKind::Proxy,
                format!("Could not connect to proxy {dial_host}:{dial_port}: {}", e.message),
            )
        } else {
            e
        }
    })?;
    let _ = tcp.set_nodelay(true);

    let mut via_forward_proxy = false;
    if let Some(proxy) = target.proxy {
        if target.tls || target.force_tunnel {
            let left = target.connect_timeout.saturating_sub(started.elapsed()).max(Duration::from_millis(1));
            tokio::time::timeout(left, tunnel(&mut tcp, target.host, target.port, proxy))
                .await
                .map_err(|_| EngineError::timeout("Proxy CONNECT", target.connect_timeout))??;
        } else {
            via_forward_proxy = true;
        }
    }
    timing.connect = connect_started.elapsed();

    if !target.tls {
        return Ok(Connection { stream: Stream::Plain(tcp), remote_addr, tls: None, timing, via_forward_proxy });
    }

    let tls_started = Instant::now();
    let config = tls_cache.get(target.tls_options, target.alpn)?;
    let connector = tokio_rustls::TlsConnector::from(Arc::clone(&config));
    let name = tls::server_name(target.host)?;
    let left = target.connect_timeout.saturating_sub(started.elapsed()).max(Duration::from_millis(1));
    let stream = tokio::time::timeout(left, connector.connect(name, tcp))
        .await
        .map_err(|_| EngineError::timeout("TLS handshake", target.connect_timeout))?
        .map_err(|e| tls::handshake_error(e, target.host))?;
    timing.tls = tls_started.elapsed();
    let info = tls::connection_info(stream.get_ref().1);
    Ok(Connection {
        stream: Stream::Tls(Box::new(stream)),
        remote_addr,
        tls: Some(info),
        timing,
        via_forward_proxy: false,
    })
}

pub(crate) async fn resolve(host: &str, port: u16, timeout: Duration) -> Result<Vec<SocketAddr>> {
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = bare.parse::<IpAddr>() {
        return Ok(vec![SocketAddr::new(ip, port)]);
    }
    let lookup = tokio::net::lookup_host((bare, port));
    let addrs: Vec<SocketAddr> = tokio::time::timeout(timeout, lookup)
        .await
        .map_err(|_| EngineError::timeout(&format!("DNS lookup for '{bare}'"), timeout))?
        .map_err(|e| EngineError::new(ErrorKind::Dns, format!("Could not resolve host '{bare}': {e}")))?
        .collect();
    if addrs.is_empty() {
        return Err(EngineError::new(ErrorKind::Dns, format!("Host '{bare}' has no addresses")));
    }
    Ok(interleave_families(addrs))
}

/// Alternate IPv6/IPv4 addresses (RFC 8305) while keeping the OS preference first.
fn interleave_families(addrs: Vec<SocketAddr>) -> Vec<SocketAddr> {
    let first_v6 = addrs[0].is_ipv6();
    let (mut primary, mut secondary): (Vec<_>, Vec<_>) = addrs.into_iter().partition(|a| a.is_ipv6() == first_v6);
    let mut out = Vec::with_capacity(primary.len() + secondary.len());
    primary.reverse();
    secondary.reverse();
    while !primary.is_empty() || !secondary.is_empty() {
        if let Some(a) = primary.pop() {
            out.push(a);
        }
        if let Some(a) = secondary.pop() {
            out.push(a);
        }
    }
    out
}

/// Happy Eyeballs: start attempts 250 ms apart, first success wins.
async fn connect_any(addrs: &[SocketAddr], timeout: Duration) -> Result<(TcpStream, SocketAddr)> {
    const STAGGER: Duration = Duration::from_millis(250);
    let deadline = tokio::time::Instant::now() + timeout;
    let mut pending = FuturesUnordered::new();
    let mut next = 0;

    loop {
        if next < addrs.len() {
            let addr = addrs[next];
            next += 1;
            pending.push(async move { (addr, TcpStream::connect(addr).await) });
        }
        let stagger = tokio::time::sleep(STAGGER);
        tokio::select! {
            biased;
            Some((addr, result)) = pending.next() => match result {
                Ok(stream) => return Ok((stream, addr)),
                Err(e) => {
                    if pending.is_empty() && next >= addrs.len() {
                        return Err(connect_error(addr, e));
                    }
                    // A failure frees the slot: start the next address right away.
                    continue;
                }
            },
            _ = tokio::time::sleep_until(deadline) => {
                return Err(EngineError::timeout("Connection", timeout));
            }
            _ = stagger, if next < addrs.len() => continue,
        }
    }
}

fn connect_error(addr: SocketAddr, err: io::Error) -> EngineError {
    let reason = match err.kind() {
        io::ErrorKind::ConnectionRefused => "connection refused (is the server running?)".to_string(),
        io::ErrorKind::TimedOut => "connection timed out".to_string(),
        _ => err.to_string(),
    };
    EngineError::new(ErrorKind::Connect, format!("Could not connect to {addr}: {reason}"))
}

/// Open an HTTP CONNECT tunnel through a proxy.
async fn tunnel(tcp: &mut TcpStream, host: &str, port: u16, proxy: &ProxyEndpoint) -> Result<()> {
    let authority = if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    };
    let mut req = format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n");
    if let Some(auth) = proxy.authorization() {
        req.push_str(&format!("Proxy-Authorization: {auth}\r\n"));
    }
    req.push_str("\r\n");
    let io_err = |e: io::Error| EngineError::new(ErrorKind::Proxy, format!("Proxy tunnel failed: {e}"));
    tcp.write_all(req.as_bytes()).await.map_err(io_err)?;

    // Read the proxy response head byte by byte so no tunnel bytes are consumed.
    let mut head = Vec::with_capacity(256);
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        if head.len() > 16 * 1024 {
            return Err(EngineError::new(ErrorKind::Proxy, "Proxy response header too large"));
        }
        let n = tcp.read(&mut byte).await.map_err(io_err)?;
        if n == 0 {
            return Err(EngineError::new(ErrorKind::Proxy, "Proxy closed the connection during CONNECT"));
        }
        head.push(byte[0]);
    }
    let text = String::from_utf8_lossy(&head);
    let status_line = text.lines().next().unwrap_or_default();
    let code = status_line.split_whitespace().nth(1).and_then(|c| c.parse::<u16>().ok()).unwrap_or(0);
    match code {
        200..=299 => Ok(()),
        407 => Err(EngineError::new(
            ErrorKind::Proxy,
            "Proxy requires authentication (407). Add credentials to the proxy URL in Settings (only Basic auth is supported).",
        )),
        _ => Err(EngineError::new(ErrorKind::Proxy, format!("Proxy refused tunnel to {authority}: {status_line}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interleaves_address_families() {
        let a: Vec<SocketAddr> =
            ["[::1]:1", "[::2]:1", "1.1.1.1:1", "2.2.2.2:1"].iter().map(|s| s.parse().unwrap()).collect();
        let out = interleave_families(a);
        let s: Vec<String> = out.iter().map(|a| a.to_string()).collect();
        assert_eq!(s, ["[::1]:1", "1.1.1.1:1", "[::2]:1", "2.2.2.2:1"]);
    }

    #[tokio::test]
    async fn refused_connection_reports_address() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        // Windows retries refused SYNs for ~2 s, so leave headroom.
        let err = connect_any(&[addr], Duration::from_secs(10)).await.unwrap_err();
        assert_eq!(err.kind, ErrorKind::Connect);
        assert!(err.message.contains(&addr.to_string()));
    }
}

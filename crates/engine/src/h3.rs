//! HTTP/3 over QUIC (quinn + h3), used when a request's HTTP version is
//! "HTTP/3 (QUIC)". `http.rs` builds the request and handles redirects,
//! cookies and decoding; this module connects and moves the bytes.
//!
//! * Like HTTP/1.1 and HTTP/2, every request opens a fresh connection.
//! * QUIC merges the transport and TLS 1.3 handshakes into one round trip, so
//!   the whole handshake is reported as "connect" and TLS time is 0.
//! * Addresses are raced like Happy Eyeballs (250 ms apart, first handshake wins).
//! * No HTTP proxies (QUIC is UDP; a CONNECT tunnel can't carry it), no plain
//!   `http://`, no 0-RTT; Auto mode never upgrades to HTTP/3 via Alt-Svc.

use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use bytes::{Buf, Bytes};
use futures_util::StreamExt;
use futures_util::stream::FuturesUnordered;
use quinn_proto::crypto::{self, ExportKeyingMaterialError, HeaderKey, KeyPair, Keys, PacketKey};
use quinn_proto::transport_parameters::TransportParameters;
use rustls_pki_types::CertificateDer;

use crate::error::{EngineError, ErrorKind, Result, human_duration};
use crate::net::{self, ConnectTiming};
use crate::tls::{self, TlsConfigCache, TlsInfo, TlsOptions};

type SendRequest = ::h3::client::SendRequest<h3_quinn::OpenStreams, Bytes>;
type RequestStream = ::h3::client::RequestStream<h3_quinn::BidiStream<Bytes>, Bytes>;

/// Response header limit, the same as HTTP/1.1 and HTTP/2 (~400 KB).
const MAX_HEADER_SECTION: u64 = 400 * 1024;
/// Delay before racing the next address (RFC 8305).
const STAGGER: Duration = Duration::from_millis(250);
/// TLS alert `no_application_protocol` as a QUIC crypto error (0x100 + 120).
const NO_APPLICATION_PROTOCOL: u64 = 0x178;

/// An established HTTP/3 connection, ready for one request.
pub(crate) struct Connection {
    send: SendRequest,
    endpoint: quinn::Endpoint,
    /// Task polling the HTTP/3 control streams (SETTINGS, GOAWAY).
    driver: DriverGuard,
    pub remote_addr: SocketAddr,
    pub tls: TlsInfo,
    pub timing: ConnectTiming,
}

/// Resolve `host`, then complete a QUIC handshake (TLS 1.3, ALPN `h3`) within `connect_timeout`.
pub(crate) async fn connect(
    host: &str,
    port: u16,
    tls_options: &TlsOptions,
    connect_timeout: Duration,
    tls_cache: &TlsConfigCache,
) -> Result<Connection> {
    let config = tls_cache.quic(tls_options)?;
    let mut timing = ConnectTiming::default();
    let started = Instant::now();
    let addrs = net::resolve(host, port, connect_timeout).await?;
    timing.dns = started.elapsed();

    let handshake_started = Instant::now();
    let left = connect_timeout.saturating_sub(timing.dns).max(Duration::from_millis(1));
    let (endpoint, conn, suite) = handshake_any(&addrs, host, port, &config, left, connect_timeout).await?;
    timing.connect = handshake_started.elapsed();

    let tls = connection_info(&conn, suite);
    let remote_addr = conn.remote_address();
    let mut builder = ::h3::client::builder();
    builder.max_field_section_size(MAX_HEADER_SECTION);
    let left = connect_timeout.saturating_sub(started.elapsed()).max(Duration::from_millis(1));
    let (mut driver, send) = tokio::time::timeout(left, builder.build(h3_quinn::Connection::new(conn)))
        .await
        .map_err(|_| handshake_timeout(host, port, Some(connect_timeout)))?
        .map_err(|e| connection_error(&e))?;
    let driver = DriverGuard(
        tokio::spawn(async move {
            let _ = std::future::poll_fn(|cx| driver.poll_close(cx)).await;
        })
        .abort_handle(),
    );
    Ok(Connection { send, endpoint, driver, remote_addr, tls, timing })
}

impl Connection {
    /// Send the request and wait for the response head. The returned body
    /// owns the connection: dropping it closes the connection.
    pub(crate) async fn send(
        mut self,
        request: http::Request<()>,
        body: Bytes,
    ) -> Result<(http::response::Parts, Body)> {
        let mut stream = self.send.send_request(request).await.map_err(stream_error)?;
        // The body is sent before reading the response. A server that answers
        // early either reads and discards the rest or stops the upload
        // (STOP_SENDING); in both cases its response is still read.
        let upload = async {
            if !body.is_empty() {
                stream.send_data(body).await?;
            }
            stream.finish().await
        }
        .await;
        let response = match stream.recv_response().await {
            Ok(response) => response,
            Err(e) => return Err(stream_error(upload.err().unwrap_or(e))),
        };
        let (parts, ()) = response.into_parts();
        let body = Body { stream, _send: self.send, _endpoint: self.endpoint, _driver: self.driver, done: false };
        Ok((parts, body))
    }
}

/// Stops the connection driver task when dropped.
struct DriverGuard(tokio::task::AbortHandle);

impl Drop for DriverGuard {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// A response body being received. Dropping it closes the connection
/// (fields drop in order: stream, the last `SendRequest`, endpoint, driver).
pub(crate) struct Body {
    stream: RequestStream,
    _send: SendRequest,
    _endpoint: quinn::Endpoint,
    _driver: DriverGuard,
    done: bool,
}

impl Body {
    /// Next chunk of body data; `None` at end of stream.
    pub(crate) async fn next_chunk(&mut self) -> Option<Result<Bytes>> {
        while !self.done {
            match self.stream.recv_data().await {
                Ok(Some(mut data)) => {
                    let chunk = data.copy_to_bytes(data.remaining());
                    if !chunk.is_empty() {
                        return Some(Ok(chunk));
                    }
                }
                Ok(None) => self.done = true,
                // A graceful close right after the last byte is not an error.
                Err(e) if e.is_h3_no_error() => self.done = true,
                Err(e) => {
                    self.done = true;
                    return Some(Err(stream_error(e)));
                }
            }
        }
        None
    }
}

/// Start a handshake per address, 250 ms apart; the first to finish wins and
/// the others are dropped (which closes them).
async fn handshake_any(
    addrs: &[SocketAddr],
    host: &str,
    port: u16,
    config: &Arc<rustls::ClientConfig>,
    timeout: Duration,
    connect_timeout: Duration,
) -> Result<(quinn::Endpoint, quinn::Connection, Option<u16>)> {
    let deadline = tokio::time::Instant::now() + timeout;
    let mut pending = FuturesUnordered::new();
    let mut next = 0;
    let mut failure: Option<EngineError> = None;
    loop {
        if next < addrs.len() {
            let addr = addrs[next];
            next += 1;
            pending.push(handshake(addr, host, port, config.clone()));
        }
        let stagger = tokio::time::sleep(STAGGER);
        tokio::select! {
            biased;
            Some(result) = pending.next() => match result {
                Ok(done) => return Ok(done),
                Err(e) => {
                    failure = Some(most_useful(failure, e));
                    if pending.is_empty() && next >= addrs.len() {
                        return Err(failure.unwrap_or_else(|| handshake_timeout(host, port, Some(connect_timeout))));
                    }
                    // A failure frees the slot: start the next address right away.
                    continue;
                }
            },
            _ = tokio::time::sleep_until(deadline) => {
                return Err(most_useful(failure, handshake_timeout(host, port, Some(connect_timeout))));
            }
            _ = stagger, if next < addrs.len() => continue,
        }
    }
}

/// Keep the error that tells the user most: a certificate problem beats a
/// protocol problem, which beats "no answer".
fn most_useful(current: Option<EngineError>, new: EngineError) -> EngineError {
    let rank = |e: &EngineError| match e.kind {
        ErrorKind::Tls => 3,
        ErrorKind::Protocol => 2,
        ErrorKind::Connect if e.message.contains("timed out") => 1,
        _ => 0,
    };
    match current {
        Some(current) if rank(&current) >= rank(&new) => current,
        _ => new,
    }
}

async fn handshake(
    addr: SocketAddr,
    host: &str,
    port: u16,
    config: Arc<rustls::ClientConfig>,
) -> Result<(quinn::Endpoint, quinn::Connection, Option<u16>)> {
    let bind: SocketAddr =
        if addr.is_ipv6() { (Ipv6Addr::UNSPECIFIED, 0).into() } else { (Ipv4Addr::UNSPECIFIED, 0).into() };
    let endpoint = quinn::Endpoint::client(bind).map_err(|e| {
        EngineError::new(ErrorKind::Connect, format!("Could not open a UDP socket to reach {addr}: {e}"))
    })?;
    let quic = quinn::crypto::rustls::QuicClientConfig::try_from(config)
        .map_err(|e| EngineError::new(ErrorKind::Tls, format!("QUIC TLS setup failed: {e}")))?;
    let suite = Arc::new(OnceLock::new());
    let probe = CipherProbe { inner: Arc::new(quic), suite: suite.clone() };
    let mut client_config = quinn::ClientConfig::new(Arc::new(probe));
    let mut transport = quinn::TransportConfig::default();
    // Quiet responses (slow servers, event streams) must not hit the idle
    // timeout; a peer that vanished still does, as it stops acknowledging.
    transport.keep_alive_interval(Some(Duration::from_secs(10)));
    client_config.transport_config(Arc::new(transport));

    let connecting = endpoint.connect_with(client_config, addr, host).map_err(|e| match e {
        quinn::ConnectError::InvalidServerName(name) => {
            EngineError::invalid(format!("Invalid TLS server name '{name}'"))
        }
        other => EngineError::new(ErrorKind::Connect, format!("Could not start a QUIC connection to {addr}: {other}")),
    })?;
    let conn = connecting.await.map_err(|e| handshake_error(e, host, port, addr))?;
    Ok((endpoint, conn, suite.get().copied()))
}

fn handshake_timeout(host: &str, port: u16, after: Option<Duration>) -> EngineError {
    let target = if host.contains(':') { format!("[{host}]:{port}") } else { format!("{host}:{port}") };
    let after = after.map(|d| format!(" after {}", human_duration(d))).unwrap_or_default();
    EngineError::new(
        ErrorKind::Connect,
        format!(
            "QUIC handshake with {target} timed out{after}: no reply over UDP. Is UDP port {port} blocked by a firewall or VPN, or does the server not support HTTP/3?"
        ),
    )
}

/// Explain a failed QUIC handshake. Crypto errors (0x100–0x1ff) carry a TLS alert.
fn handshake_error(err: quinn::ConnectionError, host: &str, port: u16, addr: SocketAddr) -> EngineError {
    use quinn::ConnectionError as E;
    let is_crypto = |code: u64| (0x100..=0x1ff).contains(&code);
    // `example.com (93.184.215.14:443)`, or just the address for IP hosts.
    let peer = if host.parse::<std::net::IpAddr>().is_ok() { addr.to_string() } else { format!("{host} ({addr})") };
    match err {
        // Our side gave up, e.g. the certificate did not verify.
        E::TransportError(e) if is_crypto(u64::from(e.code)) => tls::handshake_failure(&e.reason, host),
        E::TransportError(e) => {
            EngineError::new(ErrorKind::Protocol, format!("QUIC handshake with {peer} failed: {e}"))
        }
        E::ConnectionClosed(close) if u64::from(close.error_code) == NO_APPLICATION_PROTOCOL => {
            EngineError::new(ErrorKind::Protocol, format!("{peer} speaks QUIC but not HTTP/3: it refused ALPN \"h3\""))
        }
        E::ConnectionClosed(close) if is_crypto(u64::from(close.error_code)) => {
            let alert = u64::from(close.error_code) - 0x100;
            // 42 bad_certificate, 116 certificate_required.
            let hint = if matches!(alert, 42 | 116) {
                " The server may require a client certificate (Settings > Certificates)."
            } else {
                ""
            };
            EngineError::new(
                ErrorKind::Tls,
                format!("TLS handshake with {host} failed: the server rejected it ({close}).{hint}"),
            )
        }
        E::ConnectionClosed(close) => EngineError::new(
            ErrorKind::Connect,
            format!("{peer} closed the QUIC connection during the handshake: {close}"),
        ),
        E::ApplicationClosed(close) => {
            EngineError::new(ErrorKind::Protocol, format!("{peer} closed the connection during the handshake: {close}"))
        }
        E::VersionMismatch => EngineError::new(ErrorKind::Protocol, format!("{peer} does not support QUIC version 1")),
        E::Reset => EngineError::new(ErrorKind::Connect, format!("{peer} reset the QUIC connection")),
        // quinn's idle timeout (connect timeouts above 30 s).
        E::TimedOut => handshake_timeout(host, port, None),
        other => EngineError::new(ErrorKind::Connect, format!("QUIC connection to {peer} failed: {other}")),
    }
}

fn connection_error(err: &::h3::error::ConnectionError) -> EngineError {
    use ::h3::error::ConnectionError as C;
    use ::h3::quic::ConnectionErrorIncoming as I;
    match err {
        C::Timeout { .. } | C::Remote { 0: I::Timeout, .. } => {
            EngineError::new(ErrorKind::Timeout, "The HTTP/3 connection timed out (the server stopped answering)")
        }
        C::Remote { 0: I::ApplicationClose { error_code }, .. } => EngineError::new(
            ErrorKind::Protocol,
            format!("The server closed the HTTP/3 connection ({})", ::h3::error::Code::from(*error_code)),
        ),
        C::Remote { 0: I::Undefined(e), .. } => EngineError::new(ErrorKind::Io, format!("HTTP/3 connection lost: {e}")),
        other => EngineError::new(ErrorKind::Protocol, format!("HTTP/3 connection error: {other}")),
    }
}

fn stream_error(err: ::h3::error::StreamError) -> EngineError {
    use ::h3::error::StreamError as S;
    match &err {
        S::ConnectionError { 0: e, .. } => connection_error(e),
        S::HeaderTooBig { actual_size, max_size, .. } => EngineError::new(
            ErrorKind::Protocol,
            format!("Response headers are too large ({actual_size} bytes; the limit is {max_size})"),
        ),
        S::RemoteTerminate { code, .. } => {
            EngineError::new(ErrorKind::Io, format!("The server reset the request stream ({code})"))
        }
        S::RemoteClosing { .. } => {
            EngineError::new(ErrorKind::Io, "The server is shutting the connection down and refused the request")
        }
        _ => EngineError::new(ErrorKind::Protocol, format!("HTTP/3 error: {err}")),
    }
}

fn connection_info(conn: &quinn::Connection, suite: Option<u16>) -> TlsInfo {
    let alpn = conn
        .handshake_data()
        .and_then(|d| d.downcast::<quinn::crypto::rustls::HandshakeData>().ok())
        .and_then(|d| d.protocol)
        .map(|p| String::from_utf8_lossy(&p).into_owned());
    let certificate = conn
        .peer_identity()
        .and_then(|p| p.downcast::<Vec<CertificateDer<'static>>>().ok())
        .and_then(|certs| certs.first().and_then(|c| tls::describe_certificate(c.as_ref())));
    // Same naming as TCP connections (e.g. `TLS13_AES_128_GCM_SHA256`).
    let cipher = suite.map(|s| format!("{:?}", rustls::CipherSuite::from(s))).unwrap_or_default();
    TlsInfo { version: "TLS 1.3".into(), cipher, alpn, certificate }
}

/// quinn does not expose the negotiated cipher suite, so this wrapper reads
/// it from the ServerHello as the handshake bytes pass through (nothing else
/// is inspected; everything is delegated to quinn's rustls session).
struct CipherProbe {
    inner: Arc<quinn::crypto::rustls::QuicClientConfig>,
    suite: Arc<OnceLock<u16>>,
}

impl crypto::ClientConfig for CipherProbe {
    fn start_session(
        self: Arc<Self>,
        version: u32,
        server_name: &str,
        params: &TransportParameters,
    ) -> std::result::Result<Box<dyn crypto::Session>, quinn_proto::ConnectError> {
        let inner = self.inner.clone().start_session(version, server_name, params)?;
        Ok(Box::new(ProbeSession { inner, suite: self.suite.clone(), hello: Some(Vec::new()) }))
    }
}

struct ProbeSession {
    inner: Box<dyn crypto::Session>,
    suite: Arc<OnceLock<u16>>,
    /// Start of the server's handshake data until the suite is known.
    hello: Option<Vec<u8>>,
}

/// More than enough for a ServerHello up to its cipher suite (at most 73 bytes).
const HELLO_PREFIX: usize = 128;

enum Hello {
    Incomplete,
    Suite(u16),
    Invalid,
}

/// The cipher suite of the TLS ServerHello (RFC 8446 §4.1.3) at the start of `data`.
fn server_hello_suite(data: &[u8]) -> Hello {
    // msg_type (1) + length (3), legacy_version (2), random (32).
    const SESSION_ID_AT: usize = 4 + 2 + 32;
    match data.first() {
        None => return Hello::Incomplete,
        Some(2) => {}
        Some(_) => return Hello::Invalid,
    }
    let Some(&id_len) = data.get(SESSION_ID_AT) else { return Hello::Incomplete };
    if id_len > 32 {
        return Hello::Invalid;
    }
    let at = SESSION_ID_AT + 1 + id_len as usize;
    match data.get(at..at + 2) {
        Some(suite) => Hello::Suite(u16::from_be_bytes([suite[0], suite[1]])),
        None => Hello::Incomplete,
    }
}

impl crypto::Session for ProbeSession {
    fn read_handshake(&mut self, buf: &[u8]) -> std::result::Result<bool, quinn_proto::TransportError> {
        if let Some(hello) = &mut self.hello {
            let room = HELLO_PREFIX.saturating_sub(hello.len());
            hello.extend_from_slice(&buf[..buf.len().min(room)]);
            match server_hello_suite(hello) {
                Hello::Suite(suite) => {
                    let _ = self.suite.set(suite);
                    self.hello = None;
                }
                Hello::Incomplete if hello.len() < HELLO_PREFIX => {}
                _ => self.hello = None,
            }
        }
        self.inner.read_handshake(buf)
    }

    fn initial_keys(&self, dst_cid: &quinn_proto::ConnectionId, side: quinn_proto::Side) -> Keys {
        self.inner.initial_keys(dst_cid, side)
    }

    fn handshake_data(&self) -> Option<Box<dyn std::any::Any>> {
        self.inner.handshake_data()
    }

    fn peer_identity(&self) -> Option<Box<dyn std::any::Any>> {
        self.inner.peer_identity()
    }

    fn early_crypto(&self) -> Option<(Box<dyn HeaderKey>, Box<dyn PacketKey>)> {
        self.inner.early_crypto()
    }

    fn early_data_accepted(&self) -> Option<bool> {
        self.inner.early_data_accepted()
    }

    fn is_handshaking(&self) -> bool {
        self.inner.is_handshaking()
    }

    fn transport_parameters(&self) -> std::result::Result<Option<TransportParameters>, quinn_proto::TransportError> {
        self.inner.transport_parameters()
    }

    fn write_handshake(&mut self, buf: &mut Vec<u8>) -> Option<Keys> {
        self.inner.write_handshake(buf)
    }

    fn next_1rtt_keys(&mut self) -> Option<KeyPair<Box<dyn PacketKey>>> {
        self.inner.next_1rtt_keys()
    }

    fn is_valid_retry(&self, orig_dst_cid: &quinn_proto::ConnectionId, header: &[u8], payload: &[u8]) -> bool {
        self.inner.is_valid_retry(orig_dst_cid, header, payload)
    }

    fn export_keying_material(
        &self,
        output: &mut [u8],
        label: &[u8],
        context: &[u8],
    ) -> std::result::Result<(), ExportKeyingMaterialError> {
        self.inner.export_keying_material(output, label, context)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server_hello(session_id: &[u8], suite: u16) -> Vec<u8> {
        let mut body = vec![0x03, 0x03];
        body.extend_from_slice(&[7u8; 32]);
        body.push(session_id.len() as u8);
        body.extend_from_slice(session_id);
        body.extend_from_slice(&suite.to_be_bytes());
        body.extend_from_slice(&[0, 0, 0]); // compression + empty extensions (abbreviated)
        let len = body.len() as u32;
        let mut msg = vec![2, (len >> 16) as u8, (len >> 8) as u8, len as u8];
        msg.extend_from_slice(&body);
        msg
    }

    #[test]
    fn reads_cipher_suite_from_server_hello() {
        let hello = server_hello(&[], 0x1301);
        assert!(matches!(server_hello_suite(&hello), Hello::Suite(0x1301)));
        let hello = server_hello(&[9; 32], 0x1303);
        assert!(matches!(server_hello_suite(&hello), Hello::Suite(0x1303)));
        // Split across CRYPTO frames: not enough bytes yet.
        assert!(matches!(server_hello_suite(&hello[..40]), Hello::Incomplete));
        assert!(matches!(server_hello_suite(&[]), Hello::Incomplete));
        // Not a ServerHello, or a corrupt session id length.
        assert!(matches!(server_hello_suite(&[1, 0, 0, 0]), Hello::Invalid));
        let mut bad = server_hello(&[], 0x1301);
        bad[38] = 200;
        assert!(matches!(server_hello_suite(&bad), Hello::Invalid));
        assert_eq!(format!("{:?}", rustls::CipherSuite::from(0x1302)), "TLS13_AES_256_GCM_SHA384");
    }

    #[test]
    fn prefers_certificate_errors_over_timeouts() {
        let tls = EngineError::new(ErrorKind::Tls, "cert");
        let timeout = handshake_timeout("h", 443, Some(Duration::from_secs(1)));
        assert_eq!(most_useful(Some(timeout.clone()), tls.clone()).kind, ErrorKind::Tls);
        assert_eq!(most_useful(Some(tls), timeout.clone()).kind, ErrorKind::Tls);
        let socket = EngineError::new(ErrorKind::Connect, "Could not open a UDP socket");
        assert!(most_useful(Some(socket), timeout).message.contains("timed out"));
    }
}

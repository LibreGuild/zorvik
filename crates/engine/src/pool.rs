//! Pooled HTTP/1.1 and HTTP/2 client for load generation (`zorvik-load`).
//!
//! [`Client`](crate::Client) opens a fresh connection per request so its timing
//! is complete. A load generator must reuse connections like real clients do
//! (and to not run out of ports), so this client keeps per origin:
//! * HTTP/1.1: idle keep-alive connections, one request at a time on each.
//! * HTTP/2: shared connections carrying up to [`MAX_STREAMS`] requests at once;
//!   another connection is opened when all of them are busy.
//!
//! Connections go through the engine's connect path (DNS, Happy Eyeballs, a
//! proxy CONNECT tunnel, TLS with the OS verifier and ALPN) and use the same
//! header rules as `Client` (default User-Agent/Accept, Content-Length, Host).
//! There is no cookie jar. Response bodies are read to the end and discarded:
//! only the status, sizes and timing are kept. Redirects are not followed (a
//! 3xx is an answer like any other) and bodies are not decoded, so sizes are
//! what went over the wire. HTTP/3 is not supported here.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use bytes::Bytes;
use http::{Method, Request, Uri, Version};
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::client::conn::{http1, http2};
use hyper_util::rt::{TokioExecutor, TokioIo};

use crate::error::{EngineError, ErrorKind, Result};
use crate::http::{self as h, Header, HttpRequest, HttpVersionPref, Prepared, RequestOptions};
use crate::net::{self, Target};
use crate::proxy::ProxyEndpoint;
use crate::tls::{Alpn, TlsConfigCache};

/// Most requests in flight on one HTTP/2 connection (RFC 9113 recommends
/// servers allow at least 100). A server allowing fewer makes the extra
/// requests wait inside the connection.
pub const MAX_STREAMS: usize = 100;

/// Idle HTTP/1.1 connections older than this are closed instead of reused.
const IDLE_TIMEOUT: Duration = Duration::from_secs(90);

/// What happened to one request.
#[derive(Debug, Clone)]
pub struct Exchange {
    /// Response status; `None` when the request failed.
    pub status: Option<u16>,
    pub error: Option<EngineError>,
    /// From the `started` instant given to [`PooledClient::send`] to the end of
    /// the response body (or the failure).
    pub latency: Duration,
    /// Request handed to a ready connection until the response head arrived.
    pub ttfb: Option<Duration>,
    /// Response head (as text, approximate for HTTP/2) plus body bytes as received.
    pub bytes_in: u64,
    /// Request head (as text, approximate for HTTP/2) plus body.
    pub bytes_out: u64,
    /// This request opened a new connection (DNS, TCP, TLS were paid).
    pub new_connection: bool,
}

/// A request checked and turned into ready-to-send heads, reusable for every send.
pub struct PooledRequest {
    origin: Arc<Origin>,
    method: Method,
    body: Bytes,
    h1: Result<Head>,
    h2: Result<Head>,
}

struct Head {
    uri: Uri,
    headers: http::HeaderMap,
    /// Size of the head as HTTP/1.1 text.
    size: u64,
}

impl Head {
    fn request(&self, method: &Method, body: &Bytes, version: Version) -> Request<Full<Bytes>> {
        let mut request = Request::new(Full::new(body.clone()));
        *request.method_mut() = method.clone();
        *request.uri_mut() = self.uri.clone();
        *request.version_mut() = version;
        *request.headers_mut() = self.headers.clone();
        request
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct OriginKey {
    https: bool,
    host: String,
    port: u16,
}

/// The protocol an origin speaks; learned from the first connection for https + auto.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Proto {
    Unknown = 0,
    Http1 = 1,
    Http2 = 2,
}

struct Origin {
    key: OriginKey,
    proxy: Option<ProxyEndpoint>,
    proto: AtomicU8,
    /// Idle HTTP/1.1 connections, most recently used last.
    idle: Mutex<Vec<H1Conn>>,
    h2: Mutex<Vec<Arc<H2Conn>>>,
    /// One HTTP/2 connection (or first connection, while the protocol is
    /// unknown) is opened at a time, so a burst of requests shares one
    /// handshake. Keeps the last failure: requests queued behind a failed
    /// attempt fail with the same error instead of retrying one by one.
    opening: tokio::sync::Mutex<Option<(Instant, EngineError)>>,
}

impl Origin {
    fn proto(&self) -> Proto {
        match self.proto.load(Ordering::Acquire) {
            1 => Proto::Http1,
            2 => Proto::Http2,
            _ => Proto::Unknown,
        }
    }

    fn set_proto(&self, proto: Proto) {
        self.proto.store(proto as u8, Ordering::Release);
    }

    /// A reusable idle HTTP/1.1 connection, if any.
    async fn take_idle(&self) -> Option<H1Conn> {
        loop {
            let mut conn = lock(&self.idle).pop()?;
            if conn.sender.is_closed() || conn.idle_since.elapsed() > IDLE_TIMEOUT {
                continue;
            }
            // Resolves once the connection finished the previous exchange; fails when it closed.
            if conn.sender.ready().await.is_ok() {
                return Some(conn);
            }
        }
    }

    /// A stream on the least busy HTTP/2 connection that has room.
    fn lease_h2(&self) -> Option<H2Lease> {
        let mut conns = lock(&self.h2);
        conns.retain(|c| !c.sender.is_closed() && !c.broken.load(Ordering::Relaxed));
        let conn = conns
            .iter()
            .filter(|c| c.streams.load(Ordering::Relaxed) < MAX_STREAMS)
            .min_by_key(|c| c.streams.load(Ordering::Relaxed))?;
        Some(H2Lease::new(conn.clone()))
    }
}

/// Aborts a connection's driver task when dropped, closing the socket.
struct Driver(tokio::task::AbortHandle);

impl Drop for Driver {
    fn drop(&mut self) {
        self.0.abort();
    }
}

struct H1Conn {
    sender: http1::SendRequest<Full<Bytes>>,
    idle_since: Instant,
    _driver: Driver,
}

struct H2Conn {
    sender: http2::SendRequest<Full<Bytes>>,
    streams: AtomicUsize,
    /// Refused a request (GOAWAY or closing): no new requests go to it.
    broken: AtomicBool,
    _driver: Driver,
}

/// One stream's share of an HTTP/2 connection; frees the slot when dropped.
struct H2Lease(Arc<H2Conn>);

impl H2Lease {
    fn new(conn: Arc<H2Conn>) -> Self {
        conn.streams.fetch_add(1, Ordering::Relaxed);
        Self(conn)
    }
}

impl Drop for H2Lease {
    fn drop(&mut self) {
        self.0.streams.fetch_sub(1, Ordering::Relaxed);
    }
}

enum Conn {
    H1(H1Conn),
    H2(H2Lease),
}

struct Lease {
    conn: Conn,
    fresh: bool,
}

/// A request that failed. `Unsent`: the connection closed before the server
/// could answer (it was not processed), so it may be retried on another connection.
enum Failure {
    Unsent(EngineError),
    Failed(EngineError),
}

#[derive(Default)]
struct Progress {
    new_connection: bool,
    ttfb: Option<Duration>,
    bytes_in: u64,
    bytes_out: u64,
}

/// The pooled client. Share it (`Arc`) between all tasks of a load test;
/// dropping it closes every connection.
pub struct PooledClient {
    opts: RequestOptions,
    keep_alive: bool,
    alpn: Alpn,
    tls: TlsConfigCache,
    origins: Mutex<HashMap<OriginKey, Arc<Origin>>>,
    opened: AtomicU64,
}

impl PooledClient {
    /// `keep_alive: false` opens a new connection for every request (and asks
    /// HTTP/1.1 servers to close it with `Connection: close`).
    pub fn new(opts: RequestOptions, keep_alive: bool) -> Result<Self> {
        let alpn = match opts.http_version {
            HttpVersionPref::Auto => Alpn::Auto,
            HttpVersionPref::Http1 => Alpn::Http1,
            HttpVersionPref::Http2 => Alpn::Http2,
            HttpVersionPref::Http3 => return Err(EngineError::invalid("HTTP/3 isn't supported for load tests yet")),
        };
        let tls = TlsConfigCache::default();
        let t = &opts.tls;
        if t.ca_cert_path.is_some() || t.client_cert_path.is_some() || t.client_key_path.is_some() {
            // Report an unreadable CA or client certificate now, not on every request.
            tls.get(t, alpn)?;
        }
        Ok(Self { opts, keep_alive, alpn, tls, origins: Mutex::default(), opened: AtomicU64::new(0) })
    }

    /// Connections opened so far.
    pub fn connections_opened(&self) -> u64 {
        self.opened.load(Ordering::Relaxed)
    }

    /// Check a request and build what is sent (once for a fixed request).
    pub fn prepare(&self, req: HttpRequest) -> Result<PooledRequest> {
        let prepared = h::prepare(req)?;
        let url = &prepared.url;
        let https = url.scheme() == "https";
        let host = url.host_str().ok_or_else(|| EngineError::invalid(format!("URL '{url}' has no host")))?;
        let host = host.trim_start_matches('[').trim_end_matches(']').to_string();
        let port = url.port_or_known_default().unwrap_or(if https { 443 } else { 80 });
        let origin = self.origin(OriginKey { https, host, port });
        // Build only the heads the origin can use (a dynamic request is
        // prepared on every iteration); auto over TLS may use either.
        let (use_h1, use_h2) = match (https, self.opts.http_version) {
            (true, HttpVersionPref::Auto) => (true, true),
            (_, HttpVersionPref::Http2) => (false, true),
            _ => (true, false),
        };
        let unused = || Err(EngineError::invalid("This HTTP version isn't used for this request"));
        let h1 = if use_h1 { self.head(&prepared, false) } else { unused() };
        let h2 = if use_h2 { self.head(&prepared, true) } else { unused() };
        // Report a head the origin can't avoid using now (auto over TLS may use either).
        match (https, self.opts.http_version) {
            (true, HttpVersionPref::Auto) => {
                if let (Err(e), Err(_)) = (&h1, &h2) {
                    return Err(e.clone());
                }
            }
            (_, HttpVersionPref::Http2) => {
                if let Err(e) = &h2 {
                    return Err(e.clone());
                }
            }
            _ => {
                if let Err(e) = &h1 {
                    return Err(e.clone());
                }
            }
        }
        Ok(PooledRequest { origin, method: prepared.method, body: prepared.body, h1, h2 })
    }

    /// Send a request and read the whole response. Latency is measured from
    /// `started`, which may lie before the call (the scheduled start of an
    /// open-model request), so time spent waiting to start is included.
    pub async fn send(&self, req: &PooledRequest, started: Instant) -> Exchange {
        let mut progress = Progress::default();
        let result = match self.opts.timeout {
            Some(limit) => tokio::time::timeout(limit, self.attempt(req, &mut progress))
                .await
                .unwrap_or_else(|_| Err(EngineError::timeout("Request", limit))),
            None => self.attempt(req, &mut progress).await,
        };
        let latency = Instant::now().saturating_duration_since(started);
        let (status, error) = match result {
            Ok(status) => (Some(status), None),
            Err(e) => (None, Some(e)),
        };
        Exchange {
            status,
            error,
            latency,
            ttfb: progress.ttfb,
            bytes_in: progress.bytes_in,
            bytes_out: progress.bytes_out,
            new_connection: progress.new_connection,
        }
    }

    fn origin(&self, key: OriginKey) -> Arc<Origin> {
        let mut origins = lock(&self.origins);
        if let Some(origin) = origins.get(&key) {
            return origin.clone();
        }
        // Load tests connect through a tunnel even for http:// so the pooled
        // connection speaks directly to the server (no absolute-form requests).
        let proxy = self.opts.proxy.for_target(&key.host, key.https).cloned();
        let proto = match (key.https, self.opts.http_version) {
            (_, HttpVersionPref::Http2) => Proto::Http2,
            (true, HttpVersionPref::Auto) => Proto::Unknown,
            _ => Proto::Http1,
        };
        let origin = Arc::new(Origin {
            key: key.clone(),
            proxy,
            proto: AtomicU8::new(proto as u8),
            idle: Mutex::default(),
            h2: Mutex::default(),
            opening: tokio::sync::Mutex::new(None),
        });
        origins.insert(key, origin.clone());
        origin
    }

    fn head(&self, req: &Prepared, multiplexed: bool) -> Result<Head> {
        let mut headers = h::build_headers(req, &self.opts, None, false, None, multiplexed, true);
        if !self.keep_alive && !multiplexed && !req.headers.iter().any(|h| h.name.eq_ignore_ascii_case("connection")) {
            headers.push(Header::new("Connection", "close"));
        }
        let authority = if multiplexed { h::host_override(&headers).map(str::to_string) } else { None };
        let uri = h::request_uri(&req.url, multiplexed, authority.as_deref())?;
        let version = if multiplexed { Version::HTTP_2 } else { Version::HTTP_11 };
        let (builder, sent) = h::request_head(&req.method, uri, version, headers)?;
        let (parts, ()) =
            builder.body(()).map_err(|e| EngineError::invalid(format!("Invalid request: {e}")))?.into_parts();
        // "GET /path HTTP/1.1\r\n", "Name: value\r\n" per header, "\r\n".
        let size = req.method.as_str().len()
            + parts.uri.to_string().len()
            + 12
            + sent.iter().map(|h| h.name.len() + h.value.len() + 4).sum::<usize>()
            + 2;
        Ok(Head { uri: parts.uri, headers: parts.headers, size: size as u64 })
    }

    async fn attempt(&self, req: &PooledRequest, progress: &mut Progress) -> Result<u16> {
        let mut retried = false;
        loop {
            let lease = self.checkout(&req.origin).await?;
            let fresh = lease.fresh;
            progress.new_connection |= fresh;
            match self.round_trip(lease, req, progress).await {
                Ok(status) => return Ok(status),
                // A reused connection the server closed meanwhile: once more on a new one.
                Err(Failure::Unsent(_)) if !fresh && !retried => retried = true,
                Err(Failure::Unsent(e) | Failure::Failed(e)) => return Err(e),
            }
        }
    }

    async fn checkout(&self, origin: &Origin) -> Result<Lease> {
        if !self.keep_alive {
            let conn = self.open(origin).await?;
            return Ok(Lease { conn, fresh: true });
        }
        loop {
            match origin.proto() {
                Proto::Http1 => {
                    if let Some(conn) = origin.take_idle().await {
                        return Ok(Lease { conn: Conn::H1(conn), fresh: false });
                    }
                    // HTTP/1.1 needs a connection per concurrent request: open without queueing.
                    let conn = self.open(origin).await?;
                    return Ok(Lease { conn, fresh: true });
                }
                Proto::Http2 => {
                    if let Some(lease) = origin.lease_h2() {
                        return Ok(Lease { conn: Conn::H2(lease), fresh: false });
                    }
                    let queued = Instant::now();
                    let mut opening = origin.opening.lock().await;
                    // Someone else may have opened one while this request waited.
                    if let Some(lease) = origin.lease_h2() {
                        return Ok(Lease { conn: Conn::H2(lease), fresh: false });
                    }
                    return self.open_serialized(origin, &mut opening, queued).await;
                }
                Proto::Unknown => {
                    let queued = Instant::now();
                    let mut opening = origin.opening.lock().await;
                    if origin.proto() != Proto::Unknown {
                        continue;
                    }
                    return self.open_serialized(origin, &mut opening, queued).await;
                }
            }
        }
    }

    /// Open a connection while holding the origin's `opening` lock.
    async fn open_serialized(
        &self,
        origin: &Origin,
        last_failure: &mut Option<(Instant, EngineError)>,
        queued: Instant,
    ) -> Result<Lease> {
        if let Some((at, err)) = last_failure.as_ref()
            && *at >= queued
        {
            return Err(err.clone());
        }
        match self.open(origin).await {
            Ok(conn) => {
                *last_failure = None;
                Ok(Lease { conn, fresh: true })
            }
            Err(e) => {
                *last_failure = Some((Instant::now(), e.clone()));
                Err(e)
            }
        }
    }

    /// Connect and handshake. A new HTTP/2 connection is shared through the
    /// origin (with keep-alive) and returned with one stream leased.
    async fn open(&self, origin: &Origin) -> Result<Conn> {
        let key = &origin.key;
        let conn = net::connect(
            &Target {
                host: &key.host,
                port: key.port,
                tls: key.https,
                alpn: self.alpn,
                tls_options: &self.opts.tls,
                proxy: origin.proxy.as_ref(),
                force_tunnel: true,
                connect_timeout: self.opts.connect_timeout,
            },
            &self.tls,
        )
        .await?;
        self.opened.fetch_add(1, Ordering::Relaxed);
        let use_h2 = if key.https { conn.negotiated_h2() } else { self.opts.http_version == HttpVersionPref::Http2 };
        if key.https && self.opts.http_version == HttpVersionPref::Http2 && !use_h2 {
            return Err(EngineError::new(ErrorKind::Protocol, "Server did not agree to HTTP/2 (ALPN)"));
        }
        origin.set_proto(if use_h2 { Proto::Http2 } else { Proto::Http1 });
        let io = TokioIo::new(conn.stream);
        if !use_h2 {
            let (sender, connection) = http1::Builder::new()
                .title_case_headers(true)
                .allow_spaces_after_header_name_in_responses(true)
                .allow_obsolete_multiline_headers_in_responses(true)
                .max_headers(1000)
                .handshake(io)
                .await
                .map_err(EngineError::from_hyper)?;
            let driver = Driver(
                tokio::spawn(async move {
                    let _ = connection.await;
                })
                .abort_handle(),
            );
            return Ok(Conn::H1(H1Conn { sender, idle_since: Instant::now(), _driver: driver }));
        }
        let (sender, connection) = http2::Builder::new(TokioExecutor::new())
            .adaptive_window(true)
            // hyper's default (16 KB) rejects response headers that HTTP/1.1 accepts.
            .max_header_list_size(400 * 1024)
            .handshake(io)
            .await
            .map_err(EngineError::from_hyper)?;
        let driver = Driver(
            tokio::spawn(async move {
                let _ = connection.await;
            })
            .abort_handle(),
        );
        let conn =
            Arc::new(H2Conn { sender, streams: AtomicUsize::new(0), broken: AtomicBool::new(false), _driver: driver });
        let lease = H2Lease::new(conn.clone());
        if self.keep_alive {
            lock(&origin.h2).push(conn);
        }
        Ok(Conn::H2(lease))
    }

    async fn round_trip(
        &self,
        lease: Lease,
        req: &PooledRequest,
        progress: &mut Progress,
    ) -> std::result::Result<u16, Failure> {
        match lease.conn {
            Conn::H1(mut conn) => {
                let head = req.h1.as_ref().map_err(|e| Failure::Failed(e.clone()))?;
                progress.bytes_out += head.size + req.body.len() as u64;
                let sent = Instant::now();
                let response =
                    match conn.sender.try_send_request(head.request(&req.method, &req.body, Version::HTTP_11)).await {
                        Ok(response) => response,
                        Err(mut e) => {
                            let unsent = e.take_message().is_some()
                                || e.error().is_incomplete_message()
                                || e.error().is_canceled();
                            let err = request_error(e.into_error());
                            return Err(if unsent { Failure::Unsent(err) } else { Failure::Failed(err) });
                        }
                    };
                progress.ttfb = Some(sent.elapsed());
                let status = response.status().as_u16();
                let (parts, body) = response.into_parts();
                progress.bytes_in += response_head_size(&parts);
                read_to_end(body, progress).await.map_err(Failure::Failed)?;
                if self.keep_alive {
                    conn.idle_since = Instant::now();
                    lock(&req.origin.idle).push(conn);
                }
                Ok(status)
            }
            Conn::H2(lease) => {
                let head = req.h2.as_ref().map_err(|e| Failure::Failed(e.clone()))?;
                progress.bytes_out += head.size + req.body.len() as u64;
                let mut sender = lease.0.sender.clone();
                let sent = Instant::now();
                let response =
                    match sender.try_send_request(head.request(&req.method, &req.body, Version::HTTP_2)).await {
                        Ok(response) => response,
                        Err(mut e) => {
                            let unsent = e.take_message().is_some();
                            if unsent || e.error().is_closed() {
                                lease.0.broken.store(true, Ordering::Relaxed);
                            }
                            let err = request_error(e.into_error());
                            return Err(if unsent { Failure::Unsent(err) } else { Failure::Failed(err) });
                        }
                    };
                progress.ttfb = Some(sent.elapsed());
                let status = response.status().as_u16();
                let (parts, body) = response.into_parts();
                progress.bytes_in += response_head_size(&parts);
                read_to_end(body, progress).await.map_err(Failure::Failed)?;
                drop(lease);
                Ok(status)
            }
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn request_error(err: hyper::Error) -> EngineError {
    let err = EngineError::from_hyper(err);
    if err.message.contains("connection closed before message completed") {
        EngineError::new(ErrorKind::Io, "Server closed the connection without sending a response")
    } else {
        err
    }
}

/// Read and discard the body, counting its bytes.
async fn read_to_end(mut body: Incoming, progress: &mut Progress) -> Result<()> {
    while let Some(frame) = body.frame().await {
        let frame = frame.map_err(EngineError::from_hyper)?;
        if let Some(data) = frame.data_ref() {
            progress.bytes_in += data.len() as u64;
        }
    }
    Ok(())
}

/// "HTTP/1.1 200 OK\r\n", "Name: value\r\n" per header, "\r\n".
fn response_head_size(parts: &http::response::Parts) -> u64 {
    let reason = parts
        .extensions
        .get::<hyper::ext::ReasonPhrase>()
        .map(|r| r.as_bytes().len())
        .or_else(|| parts.status.canonical_reason().map(str::len))
        .unwrap_or(0);
    let headers: usize = parts.headers.iter().map(|(n, v)| n.as_str().len() + v.len() + 4).sum();
    (15 + reason + headers + 2) as u64
}

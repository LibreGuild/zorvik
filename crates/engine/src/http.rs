//! HTTP/1.1, HTTP/2 and HTTP/3 client with per-phase timing, redirects,
//! cookies, decompression and body size limits (HTTP/3 transport: `h3.rs`).
//!
//! Every request opens a fresh connection. That keeps timing honest (DNS,
//! connect and TLS are always measured) and avoids stale pooled connections.

use std::time::{Duration, Instant};

use bytes::Bytes;
use http::header::{HeaderName, HeaderValue};
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper_util::rt::{TokioExecutor, TokioIo};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use url::{Position, Url};

use crate::cookies::{CookieInfo, CookieJar, parse_set_cookies};
use crate::decode::decode_content;
use crate::error::{EngineError, ErrorKind, Result};
use crate::net::{self, Connection, Target};
use crate::proxy::ProxySettings;
use crate::tls::{Alpn, TlsConfigCache, TlsInfo, TlsOptions};

pub const USER_AGENT: &str = concat!("Zorvik/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Header {
    pub name: String,
    pub value: String,
}

impl Header {
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self { name: name.into(), value: value.into() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum HttpVersionPref {
    /// HTTP/2 when the server offers it via ALPN, else HTTP/1.1.
    #[default]
    Auto,
    Http1,
    /// HTTP/2 only (ALPN for https, prior knowledge for http).
    Http2,
    /// HTTP/3 over QUIC (UDP), https only and never through a proxy. Chosen
    /// explicitly: Auto does not upgrade via Alt-Svc.
    Http3,
}

/// A fully resolved request: variables substituted, auth applied, body encoded.
#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<Header>,
    pub body: Bytes,
}

/// Decides which hosts requests may go to, redirects and OAuth token requests
/// included (AI agents: only hosts the user approved). Gets the host name.
#[derive(Clone)]
pub struct HostGuard(pub std::sync::Arc<dyn Fn(&str) -> bool + Send + Sync>);

impl std::fmt::Debug for HostGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("HostGuard")
    }
}

impl HostGuard {
    fn check(&self, url: &Url) -> Result<()> {
        self.check_host(url.host_str().unwrap_or_default())
    }

    /// Fails with `NotAllowed` for a host (name or address, brackets optional) not allowed.
    pub fn check_host(&self, host: &str) -> Result<()> {
        let host = host.trim_start_matches('[').trim_end_matches(']').to_ascii_lowercase();
        if (self.0)(&host) {
            return Ok(());
        }
        Err(EngineError::new(ErrorKind::NotAllowed, format!("Requests to {host} need the user's approval")))
    }
}

/// Authentication that answers the server's challenge (a 401 with `WWW-Authenticate`),
/// such as Digest and NTLM. The engine sends the request, and when the server challenges
/// it, sends it again with the answer on the same connection.
pub trait ChallengeAuth: Send + Sync {
    /// The Authorization header of the first attempt (NTLM's negotiate message), if any.
    fn initial(&self) -> Option<String>;
    /// The Authorization header answering a 401 whose `WWW-Authenticate` values are
    /// `challenges`. `target` is the request target as sent (path and query). `Ok(None)`:
    /// nothing this auth can answer, so the 401 is the response.
    fn answer(
        &self,
        challenges: &[String],
        method: &str,
        target: &str,
        body: &[u8],
    ) -> std::result::Result<Option<String>, String>;
    /// The answer must travel on the connection that got the challenge, over HTTP/1.1 (NTLM).
    fn connection_bound(&self) -> bool {
        false
    }
}

#[derive(Clone)]
pub struct ChallengeAuthRef(pub std::sync::Arc<dyn ChallengeAuth>);

impl std::fmt::Debug for ChallengeAuthRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ChallengeAuth")
    }
}

#[derive(Debug, Clone)]
pub struct RequestOptions {
    /// Whole request including redirects and body download. `None` = no limit.
    pub timeout: Option<Duration>,
    pub connect_timeout: Duration,
    pub follow_redirects: bool,
    pub max_redirects: u32,
    pub http_version: HttpVersionPref,
    pub tls: TlsOptions,
    pub proxy: ProxySettings,
    /// Undo Content-Encoding (gzip, deflate, br, zstd).
    pub decompress: bool,
    /// Response bodies beyond this size are truncated.
    pub max_body_bytes: usize,
    /// Add User-Agent, Accept and Accept-Encoding when the user did not set them.
    pub default_headers: bool,
    /// Hosts requests may go to; `None` = any.
    pub host_guard: Option<HostGuard>,
    /// Digest or NTLM: answer the server's 401 challenge.
    pub challenge_auth: Option<ChallengeAuthRef>,
}

impl Default for RequestOptions {
    fn default() -> Self {
        Self {
            timeout: Some(Duration::from_secs(60)),
            connect_timeout: Duration::from_secs(15),
            follow_redirects: true,
            max_redirects: 10,
            http_version: HttpVersionPref::Auto,
            tls: TlsOptions::default(),
            proxy: ProxySettings::default(),
            decompress: true,
            max_body_bytes: 100 * 1024 * 1024,
            default_headers: true,
            host_guard: None,
            challenge_auth: None,
        }
    }
}

/// Phase durations in milliseconds.
#[derive(Debug, Clone, Default, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Timing {
    /// Time spent on earlier redirect hops.
    pub redirect_ms: f64,
    pub dns_ms: f64,
    pub connect_ms: f64,
    pub tls_ms: f64,
    /// Request sent until the first response byte (server processing time).
    pub ttfb_ms: f64,
    pub download_ms: f64,
    pub total_ms: f64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RedirectHop {
    pub status: u16,
    pub method: String,
    pub url: String,
    pub location: String,
}

/// What actually went over the wire for the final hop.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SentRequest {
    pub method: String,
    pub url: String,
    pub http_version: String,
    pub headers: Vec<Header>,
    #[ts(type = "number")]
    pub body_size: u64,
    pub proxy: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ResponseMeta {
    pub status: u16,
    pub status_text: String,
    pub http_version: String,
    pub headers: Vec<Header>,
    /// Final URL after redirects.
    pub url: String,
    pub remote_addr: Option<String>,
    pub tls: Option<TlsInfo>,
    pub redirects: Vec<RedirectHop>,
    pub request: SentRequest,
    #[ts(type = "number")]
    pub headers_size: u64,
    pub cookies: Vec<CookieInfo>,
}

#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub meta: ResponseMeta,
    pub timing: Timing,
    /// Decoded body (after Content-Encoding), capped at `max_body_bytes`.
    pub body: Vec<u8>,
    pub body_truncated: bool,
    /// Bytes received on the wire for the body (before decoding).
    pub body_wire_size: u64,
    pub decode_warning: Option<String>,
}

/// Response head plus a live body, for streaming protocols such as SSE.
pub struct StreamingResponse {
    pub meta: ResponseMeta,
    pub timing: Timing,
    pub body: BodyStream,
}

pub struct BodyStream {
    body: RawBody,
    _guard: Option<ConnGuard>,
}

impl BodyStream {
    /// Next chunk of body data; `None` at end of stream.
    pub async fn next_chunk(&mut self) -> Option<Result<Bytes>> {
        self.body.next_chunk().await
    }
}

/// A response body as received, before decoding.
enum RawBody {
    Hyper(Incoming),
    H3(Box<crate::h3::Body>),
}

impl RawBody {
    async fn next_chunk(&mut self) -> Option<Result<Bytes>> {
        match self {
            RawBody::Hyper(body) => loop {
                match body.frame().await? {
                    Ok(frame) => {
                        if let Ok(data) = frame.into_data() {
                            return Some(Ok(data));
                        }
                    }
                    Err(e) => return Some(Err(EngineError::from_hyper(e))),
                }
            },
            RawBody::H3(body) => body.next_chunk().await,
        }
    }
}

/// Aborts the connection driver task when dropped, so no socket outlives its request.
pub(crate) struct ConnGuard(tokio::task::AbortHandle);

impl Drop for ConnGuard {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// The HTTP client. Cheap to share; holds cached TLS configurations.
#[derive(Default)]
pub struct Client {
    pub(crate) tls: TlsConfigCache,
}

struct Hop {
    parts: http::response::Parts,
    body: RawBody,
    /// HTTP/1.1 and HTTP/2 connection driver (an HTTP/3 body owns its connection).
    guard: Option<ConnGuard>,
    sent: SentRequest,
    remote_addr: String,
    tls: Option<TlsInfo>,
    timing: Timing,
}

/// The request side of an HTTP/1.1 or HTTP/2 connection, kept to send again (auth challenges).
enum Sender {
    H1(hyper::client::conn::http1::SendRequest<Full<Bytes>>),
    H2(hyper::client::conn::http2::SendRequest<Full<Bytes>>),
}

impl Sender {
    async fn send(
        &mut self,
        request: http::Request<Full<Bytes>>,
    ) -> std::result::Result<http::Response<Incoming>, hyper::Error> {
        match self {
            Self::H1(sender) => {
                sender.ready().await?;
                sender.send_request(request).await
            }
            Self::H2(sender) => {
                sender.ready().await?;
                sender.send_request(request).await
            }
        }
    }
}

/// The most of a 401's body read before answering its challenge on the same connection.
const MAX_CHALLENGE_BODY: usize = 1 << 20;

/// Read a body to its end, at most `limit` bytes: whether it ended within them.
async fn drain(mut body: hyper::body::Incoming, limit: usize) -> bool {
    let mut read = 0;
    while let Some(frame) = body.frame().await {
        let Ok(frame) = frame else { return false };
        read += frame.data_ref().map_or(0, Bytes::len);
        if read > limit {
            return false;
        }
    }
    true
}

pub(crate) struct Prepared {
    pub(crate) method: http::Method,
    pub(crate) url: Url,
    pub(crate) headers: Vec<Header>,
    pub(crate) body: Bytes,
}

impl Client {
    pub fn new() -> Self {
        Self::default()
    }

    /// Send a request and read the whole response body.
    pub async fn send(&self, req: HttpRequest, opts: &RequestOptions, jar: Option<&CookieJar>) -> Result<HttpResponse> {
        let started = Instant::now();
        let work = async {
            let (hop, redirects, redirect_time) = self.send_following(req, opts, jar, true).await?;
            let download_started = Instant::now();
            // Several Content-Encoding lines form one list (RFC 9110 §5.5).
            let content_encoding = header_values(&hop.parts.headers, "content-encoding").join(", ");
            let (raw, truncated, wire) = read_body(hop.body, opts.max_body_bytes).await?;
            drop(hop.guard);
            let download = download_started.elapsed();

            let (body, truncated, decode_warning) =
                if opts.decompress && !content_encoding.is_empty() && !raw.is_empty() {
                    // Decompressing up to `max_body_bytes` is CPU-bound: keep it off the async workers.
                    let limit = opts.max_body_bytes;
                    let decoded = tokio::task::spawn_blocking(move || decode_content(raw, &content_encoding, limit))
                        .await
                        .map_err(|e| EngineError::new(ErrorKind::Io, format!("Decoding the body failed: {e}")))?;
                    (decoded.body, truncated || decoded.truncated, decoded.error)
                } else {
                    (raw, truncated, None)
                };
            let mut timing = hop.timing;
            timing.redirect_ms = ms(redirect_time);
            timing.download_ms = ms(download);
            timing.total_ms = ms(started.elapsed());
            let meta = response_meta(&hop.parts, redirects, hop.sent, hop.remote_addr, hop.tls);
            Ok(HttpResponse { meta, timing, body, body_truncated: truncated, body_wire_size: wire, decode_warning })
        };
        with_timeout(opts.timeout, work).await
    }

    /// Send a request and return as soon as the response head arrives.
    /// The connect/redirect phases honour `opts.timeout`; the body stream does not.
    pub async fn open_stream(
        &self,
        req: HttpRequest,
        opts: &RequestOptions,
        jar: Option<&CookieJar>,
    ) -> Result<StreamingResponse> {
        let started = Instant::now();
        let (hop, redirects, redirect_time) =
            with_timeout(opts.timeout, self.send_following(req, opts, jar, false)).await?;
        let mut timing = hop.timing;
        timing.redirect_ms = ms(redirect_time);
        timing.total_ms = ms(started.elapsed());
        let meta = response_meta(&hop.parts, redirects, hop.sent, hop.remote_addr, hop.tls);
        Ok(StreamingResponse { meta, timing, body: BodyStream { body: hop.body, _guard: hop.guard } })
    }

    async fn send_following(
        &self,
        req: HttpRequest,
        opts: &RequestOptions,
        jar: Option<&CookieJar>,
        accept_compressed: bool,
    ) -> Result<(Hop, Vec<RedirectHop>, Duration)> {
        let mut current = prepare(req)?;
        let origin = current.url.origin();
        let mut redirects = Vec::new();
        let started = Instant::now();
        let mut redirect_time = Duration::ZERO;
        loop {
            if let Some(guard) = &opts.host_guard {
                guard.check(&current.url)?;
            }
            // Digest and NTLM answer challenges for the request's own origin only, never for a
            // site a redirect leads to (like the Authorization header).
            let challenge_ok = current.url.origin() == origin;
            let hop = self.exchange(&current, opts, jar, accept_compressed, challenge_ok).await?;
            if let Some(jar) = jar {
                let set_cookies = header_values(&hop.parts.headers, "set-cookie");
                jar.store(&current.url, &set_cookies);
            }
            let status = hop.parts.status.as_u16();
            let location = header_value(&hop.parts.headers, "location");
            match location {
                Some(location) if opts.follow_redirects && matches!(status, 301 | 302 | 303 | 307 | 308) => {
                    if redirects.len() as u32 >= opts.max_redirects {
                        return Err(EngineError::new(
                            ErrorKind::TooManyRedirects,
                            format!("Stopped after {} redirects (last: {})", redirects.len(), current.url),
                        ));
                    }
                    redirects.push(RedirectHop {
                        status,
                        method: current.method.to_string(),
                        url: current.url.to_string(),
                        location: location.clone(),
                    });
                    drop(hop);
                    current = redirect_request(current, status, &location)?;
                    redirect_time = started.elapsed();
                }
                _ => return Ok((hop, redirects, redirect_time)),
            }
        }
    }

    async fn exchange(
        &self,
        req: &Prepared,
        opts: &RequestOptions,
        jar: Option<&CookieJar>,
        accept_compressed: bool,
        challenge_ok: bool,
    ) -> Result<Hop> {
        let challenge = opts.challenge_auth.as_ref().filter(|_| challenge_ok).map(|c| c.0.clone());
        if opts.http_version == HttpVersionPref::Http3 {
            if challenge.is_some() {
                return Err(EngineError::invalid(
                    "Digest and NTLM need HTTP/1.1 or HTTP/2: choose another HTTP version in the request's settings",
                ));
            }
            return self.exchange_h3(req, opts, jar, accept_compressed).await;
        }
        let url = &req.url;
        let https = url.scheme() == "https";
        let host = url.host_str().ok_or_else(|| EngineError::invalid(format!("URL '{url}' has no host")))?;
        let bare_host = host.trim_start_matches('[').trim_end_matches(']');
        let port = url.port_or_known_default().unwrap_or(if https { 443 } else { 80 });
        let proxy = opts.proxy.for_target(bare_host, https);
        let bound = challenge.as_ref().is_some_and(|c| c.connection_bound());
        if bound && opts.http_version == HttpVersionPref::Http2 {
            return Err(EngineError::invalid("NTLM needs HTTP/1.1: choose HTTP/1.1 or Auto in the request's settings"));
        }
        let alpn = match opts.http_version {
            _ if bound => Alpn::Http1,
            HttpVersionPref::Auto | HttpVersionPref::Http3 => Alpn::Auto,
            HttpVersionPref::Http1 => Alpn::Http1,
            HttpVersionPref::Http2 => Alpn::Http2,
        };
        let conn = net::connect(
            &Target {
                host: bare_host,
                port,
                tls: https,
                alpn,
                tls_options: &opts.tls,
                proxy,
                force_tunnel: false,
                connect_timeout: opts.connect_timeout,
            },
            &self.tls,
        )
        .await?;

        let use_h2 = if https {
            conn.negotiated_h2()
        } else {
            opts.http_version == HttpVersionPref::Http2 && !conn.via_forward_proxy
        };
        if https && opts.http_version == HttpVersionPref::Http2 && !use_h2 {
            return Err(EngineError::new(ErrorKind::Protocol, "Server did not agree to HTTP/2 (ALPN)"));
        }

        let mut headers = build_headers(req, opts, jar, conn.via_forward_proxy, proxy, use_h2, accept_compressed);
        let user_authorization = has_header(&req.headers, "authorization");
        if !user_authorization && let Some(initial) = challenge.as_ref().and_then(|c| c.initial()) {
            headers.retain(|h| !h.name.eq_ignore_ascii_case("authorization"));
            headers.push(Header::new("Authorization", initial));
        }
        let authority_override = if use_h2 { host_override(&headers) } else { None };
        let uri = request_uri(url, use_h2 || conn.via_forward_proxy, authority_override)?;
        let target = uri.path_and_query().map_or_else(|| "/".to_string(), |p| p.to_string());
        let version = if use_h2 { http::Version::HTTP_2 } else { http::Version::HTTP_11 };
        let (builder, mut sent_headers) = request_head(&req.method, uri.clone(), version, headers.clone())?;
        let request = builder
            .body(Full::new(req.body.clone()))
            .map_err(|e| EngineError::invalid(format!("Invalid request: {e}")))?;

        let Connection { stream, remote_addr, tls, timing: conn_timing, .. } = conn;
        let io = TokioIo::new(stream);
        let send_started = Instant::now();
        let (response, guard, mut sender) = if use_h2 {
            let (mut sender, connection) = hyper::client::conn::http2::Builder::new(TokioExecutor::new())
                .adaptive_window(true)
                // hyper's default (16 KB) rejects response headers that HTTP/1.1 accepts (~400 KB).
                .max_header_list_size(400 * 1024)
                .handshake(io)
                .await
                .map_err(EngineError::from_hyper)?;
            let guard = ConnGuard(
                tokio::spawn(async move {
                    let _ = connection.await;
                })
                .abort_handle(),
            );
            (sender.send_request(request).await, guard, Sender::H2(sender))
        } else {
            let (mut sender, connection) = hyper::client::conn::http1::Builder::new()
                .title_case_headers(true)
                .allow_spaces_after_header_name_in_responses(true)
                .allow_obsolete_multiline_headers_in_responses(true)
                .max_headers(1000)
                .handshake(io)
                .await
                .map_err(EngineError::from_hyper)?;
            let guard = ConnGuard(
                tokio::spawn(async move {
                    let _ = connection.await;
                })
                .abort_handle(),
            );
            (sender.send_request(request).await, guard, Sender::H1(sender))
        };
        let closed = |e: hyper::Error| {
            let err = EngineError::from_hyper(e);
            if err.message.contains("connection closed before message completed") {
                EngineError::new(ErrorKind::Io, "Server closed the connection without sending a response")
            } else {
                err
            }
        };
        let mut response = response.map_err(closed)?;
        // Digest / NTLM: answer the challenge with the same request on this connection.
        if let Some(auth) = &challenge
            && !user_authorization
            && response.status() == http::StatusCode::UNAUTHORIZED
        {
            let challenges = header_values(response.headers(), "www-authenticate");
            let answer = auth
                .answer(&challenges, req.method.as_str(), &target, &req.body)
                .map_err(|message| EngineError::new(ErrorKind::Protocol, message))?;
            if let Some(answer) = answer {
                let closing = response.version() == http::Version::HTTP_10
                    || header_value(response.headers(), "connection")
                        .is_some_and(|v| v.to_ascii_lowercase().contains("close"));
                // The connection is reused only once the first response is read to its end
                // (a bounded read: the body of a 401 is thrown away).
                let drained = drain(response.into_body(), MAX_CHALLENGE_BODY).await;
                if (closing || !drained) && !auth.connection_bound() {
                    // Digest works on any connection: answer on a new one.
                    let mut retry = Prepared {
                        method: req.method.clone(),
                        url: req.url.clone(),
                        headers: req.headers.clone(),
                        body: req.body.clone(),
                    };
                    retry.headers.retain(|h| !h.name.eq_ignore_ascii_case("authorization"));
                    retry.headers.push(Header::new("Authorization", answer));
                    drop(guard);
                    return Box::pin(self.exchange(&retry, opts, jar, accept_compressed, false)).await;
                }
                headers.retain(|h| !h.name.eq_ignore_ascii_case("authorization"));
                headers.push(Header::new("Authorization", answer));
                let (builder, retry_headers) = request_head(&req.method, uri, version, headers)?;
                sent_headers = retry_headers;
                let retry = builder
                    .body(Full::new(req.body.clone()))
                    .map_err(|e| EngineError::invalid(format!("Invalid request: {e}")))?;
                response = sender.send(retry).await.map_err(|e| {
                    if auth.connection_bound() {
                        EngineError::new(
                            ErrorKind::Io,
                            format!("The server closed the connection during the NTLM handshake ({e})"),
                        )
                    } else {
                        closed(e)
                    }
                })?;
            }
        }
        let ttfb = send_started.elapsed();
        let (parts, body) = response.into_parts();

        let sent = SentRequest {
            method: req.method.to_string(),
            url: url.to_string(),
            http_version: if use_h2 { "HTTP/2" } else { "HTTP/1.1" }.to_string(),
            headers: sent_headers,
            body_size: req.body.len() as u64,
            proxy: proxy.map(|p| format!("{}:{}", p.host, p.port)),
        };
        Ok(Hop {
            parts,
            body: RawBody::Hyper(body),
            guard: Some(guard),
            sent,
            remote_addr: remote_addr.to_string(),
            tls,
            timing: Timing {
                dns_ms: ms(conn_timing.dns),
                connect_ms: ms(conn_timing.connect),
                tls_ms: ms(conn_timing.tls),
                ttfb_ms: ms(ttfb),
                ..Default::default()
            },
        })
    }

    /// One request over HTTP/3 (QUIC). The request is built and checked
    /// before connecting, so invalid headers fail without network traffic.
    async fn exchange_h3(
        &self,
        req: &Prepared,
        opts: &RequestOptions,
        jar: Option<&CookieJar>,
        accept_compressed: bool,
    ) -> Result<Hop> {
        let url = &req.url;
        if url.scheme() != "https" {
            return Err(EngineError::invalid(format!(
                "HTTP/3 needs an https:// URL (QUIC is always encrypted), got '{url}'. Use https:// or choose HTTP/1.1 or HTTP/2."
            )));
        }
        let host = url.host_str().ok_or_else(|| EngineError::invalid(format!("URL '{url}' has no host")))?;
        let bare_host = host.trim_start_matches('[').trim_end_matches(']');
        let port = url.port_or_known_default().unwrap_or(443);
        if opts.proxy.for_target(bare_host, true).is_some() {
            return Err(EngineError::new(
                ErrorKind::Proxy,
                format!(
                    "HTTP/3 can't go through an HTTP proxy; turn off the proxy for {bare_host} (add it to the proxy bypass list) or use HTTP/1.1/2"
                ),
            ));
        }

        let headers = build_headers(req, opts, jar, false, None, true, accept_compressed);
        let uri = request_uri(url, true, host_override(&headers))?;
        let (builder, sent_headers) = request_head(&req.method, uri, http::Version::HTTP_3, headers)?;
        let request = builder.body(()).map_err(|e| EngineError::invalid(format!("Invalid request: {e}")))?;

        let conn = crate::h3::connect(bare_host, port, &opts.tls, opts.connect_timeout, &self.tls).await?;
        let (remote_addr, tls, conn_timing) = (conn.remote_addr, conn.tls.clone(), conn.timing);
        let send_started = Instant::now();
        let (parts, body) = conn.send(request, req.body.clone()).await?;
        let ttfb = send_started.elapsed();

        let sent = SentRequest {
            method: req.method.to_string(),
            url: url.to_string(),
            http_version: "HTTP/3".to_string(),
            headers: sent_headers,
            body_size: req.body.len() as u64,
            proxy: None,
        };
        Ok(Hop {
            parts,
            body: RawBody::H3(Box::new(body)),
            guard: None,
            sent,
            remote_addr: remote_addr.to_string(),
            tls: Some(tls),
            timing: Timing {
                dns_ms: ms(conn_timing.dns),
                // QUIC's single handshake includes TLS 1.3; it is all reported as connect.
                connect_ms: ms(conn_timing.connect),
                tls_ms: 0.0,
                ttfb_ms: ms(ttfb),
                ..Default::default()
            },
        })
    }
}

async fn with_timeout<T>(limit: Option<Duration>, fut: impl std::future::Future<Output = Result<T>>) -> Result<T> {
    match limit {
        Some(limit) => tokio::time::timeout(limit, fut).await.map_err(|_| EngineError::timeout("Request", limit))?,
        None => fut.await,
    }
}

fn ms(d: Duration) -> f64 {
    (d.as_secs_f64() * 1_000_000.0).round() / 1000.0
}

/// Parse and validate the URL; default to http:// when no scheme is given.
pub fn normalize_url(raw: &str) -> Result<Url> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(EngineError::invalid("URL is empty"));
    }
    let with_scheme = if has_scheme(raw) { raw.to_string() } else { format!("http://{raw}") };
    let mut url = Url::parse(&with_scheme).map_err(|e| EngineError::invalid(format!("Invalid URL '{raw}': {e}")))?;
    match url.scheme() {
        "http" | "https" => {}
        other => return Err(EngineError::invalid(format!("Unsupported URL scheme '{other}' (use http or https)"))),
    }
    if url.host_str().is_none_or(str::is_empty) {
        return Err(EngineError::invalid(format!("URL '{raw}' has no host")));
    }
    url.set_fragment(None);
    Ok(url)
}

/// True when `raw` starts with `scheme:` (as opposed to `host:port`).
pub(crate) fn has_scheme(raw: &str) -> bool {
    if raw.contains("://") {
        return true;
    }
    let Some((scheme, rest)) = raw.split_once(':') else { return false };
    let valid_scheme = scheme.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    valid_scheme && !rest.starts_with(|c: char| c.is_ascii_digit()) && !scheme.contains('.')
}

pub(crate) fn prepare(req: HttpRequest) -> Result<Prepared> {
    let method_text = req.method.trim();
    if method_text.is_empty() {
        return Err(EngineError::invalid("HTTP method is empty"));
    }
    let method = http::Method::from_bytes(method_text.as_bytes())
        .map_err(|_| EngineError::invalid(format!("Invalid HTTP method '{method_text}'")))?;
    let url = normalize_url(&req.url)?;
    let headers = req
        .headers
        .into_iter()
        .filter(|h| !h.name.trim().is_empty())
        .map(|h| Header::new(h.name.trim(), h.value))
        .collect();
    Ok(Prepared { method, url, headers, body: req.body })
}

fn has_header(headers: &[Header], name: &str) -> bool {
    headers.iter().any(|h| h.name.eq_ignore_ascii_case(name))
}

/// `multiplexed` (HTTP/2 and HTTP/3): the host goes in `:authority`, not a Host header.
pub(crate) fn build_headers(
    req: &Prepared,
    opts: &RequestOptions,
    jar: Option<&CookieJar>,
    via_forward_proxy: bool,
    proxy: Option<&crate::proxy::ProxyEndpoint>,
    multiplexed: bool,
    accept_compressed: bool,
) -> Vec<Header> {
    let mut out = Vec::with_capacity(req.headers.len() + 6);
    let user = &req.headers;
    if !has_header(user, "host") && !multiplexed {
        let host = req.url.host_str().unwrap_or_default();
        let value = match req.url.port() {
            Some(port) => format!("{host}:{port}"),
            None => host.to_string(),
        };
        out.push(Header::new("Host", value));
    }
    for h in user {
        // Body framing is computed from the actual body.
        if h.name.eq_ignore_ascii_case("content-length") || h.name.eq_ignore_ascii_case("transfer-encoding") {
            continue;
        }
        out.push(h.clone());
    }
    // URL credentials are never sent in the URL itself; like curl they become Basic auth.
    if !has_header(user, "authorization")
        && let Some(auth) = crate::proxy::userinfo_authorization(&req.url)
    {
        out.push(Header::new("Authorization", auth));
    }
    if opts.default_headers {
        if !has_header(user, "user-agent") {
            out.push(Header::new("User-Agent", USER_AGENT));
        }
        if !has_header(user, "accept") {
            out.push(Header::new("Accept", "*/*"));
        }
        if opts.decompress && accept_compressed && !has_header(user, "accept-encoding") {
            out.push(Header::new("Accept-Encoding", "gzip, deflate, br, zstd"));
        }
    }
    if !req.body.is_empty() || matches!(req.method, http::Method::POST | http::Method::PUT | http::Method::PATCH) {
        out.push(Header::new("Content-Length", req.body.len().to_string()));
    }
    if let Some(jar_cookies) = jar.and_then(|j| j.header_for(&req.url)) {
        merge_cookie_header(&mut out, &jar_cookies);
    }
    if via_forward_proxy
        && !has_header(user, "proxy-authorization")
        && let Some(auth) = proxy.and_then(|p| p.authorization())
    {
        out.push(Header::new("Proxy-Authorization", auth));
    }
    out
}

/// A custom Host header, which becomes `:authority` on HTTP/2 and HTTP/3.
pub(crate) fn host_override(headers: &[Header]) -> Option<&str> {
    headers.iter().find(|h| h.name.eq_ignore_ascii_case("host")).map(|h| h.value.as_str())
}

/// Request line and validated headers; returns what will be sent. HTTP/2 and
/// HTTP/3 forbid connection-specific headers (RFC 9113 §8.2.2, RFC 9114 §4.2),
/// so those are left out there.
pub(crate) fn request_head(
    method: &http::Method,
    uri: http::Uri,
    version: http::Version,
    headers: Vec<Header>,
) -> Result<(http::request::Builder, Vec<Header>)> {
    let multiplexed = matches!(version, http::Version::HTTP_2 | http::Version::HTTP_3);
    let mut builder = http::Request::builder().method(method.clone()).uri(uri).version(version);
    let mut sent_headers = Vec::with_capacity(headers.len());
    for h in headers {
        if multiplexed && is_connection_specific(&h.name) {
            continue;
        }
        // HTTP/3 only allows `TE: trailers` (hyper's HTTP/2 client checks this itself).
        if version == http::Version::HTTP_3
            && h.name.eq_ignore_ascii_case("te")
            && !h.value.trim().eq_ignore_ascii_case("trailers")
        {
            continue;
        }
        let name = HeaderName::from_bytes(h.name.as_bytes())
            .map_err(|_| EngineError::invalid(format!("Invalid header name '{}'", h.name)))?;
        let value = HeaderValue::from_bytes(h.value.as_bytes()).map_err(|_| {
            EngineError::invalid(format!("Invalid value for header '{}' (line breaks are not allowed)", h.name))
        })?;
        builder = builder.header(name, value);
        sent_headers.push(h);
    }
    Ok((builder, sent_headers))
}

/// Add jar cookies to an existing Cookie header without overriding cookies the user set.
fn merge_cookie_header(headers: &mut Vec<Header>, jar_cookies: &str) {
    if let Some(existing) = headers.iter_mut().find(|h| h.name.eq_ignore_ascii_case("cookie")) {
        let present: Vec<String> =
            existing.value.split(';').filter_map(|p| p.split('=').next()).map(|n| n.trim().to_string()).collect();
        let extra: Vec<&str> = jar_cookies
            .split("; ")
            .filter(|p| !present.iter().any(|n| p.split('=').next() == Some(n.as_str())))
            .collect();
        if !extra.is_empty() {
            let base = existing.value.trim().trim_end_matches(';').to_string();
            existing.value = if base.is_empty() { extra.join("; ") } else { format!("{base}; {}", extra.join("; ")) };
        }
    } else {
        headers.push(Header::new("Cookie", jar_cookies));
    }
}

/// Request target: origin-form (`/path?q`) on a direct HTTP/1.1 connection,
/// absolute-form for HTTP/2 and forward proxies. `authority` (a custom Host
/// header on HTTP/2) replaces the URL's host. URL userinfo is never sent on
/// the wire (RFC 9110 §4.2.4).
pub(crate) fn request_uri(url: &Url, absolute: bool, authority: Option<&str>) -> Result<http::Uri> {
    let path = &url[Position::BeforePath..Position::AfterQuery];
    let path = if path.is_empty() { "/" } else { path };
    if !absolute {
        return http::Uri::try_from(path).map_err(|e| EngineError::invalid(format!("Invalid request path: {e}")));
    }
    let host = &url[Position::BeforeHost..Position::AfterPort];
    let uri = http::Uri::builder().scheme(url.scheme()).authority(authority.unwrap_or(host)).path_and_query(path);
    uri.build().map_err(|e| match authority {
        Some(a) => EngineError::invalid(format!("Invalid Host header '{a}': {e}")),
        None => EngineError::invalid(format!("Invalid URL '{url}': {e}")),
    })
}

fn is_connection_specific(name: &str) -> bool {
    ["connection", "keep-alive", "proxy-connection", "transfer-encoding", "upgrade", "host"]
        .iter()
        .any(|n| name.eq_ignore_ascii_case(n))
}

fn redirect_request(prev: Prepared, status: u16, location: &str) -> Result<Prepared> {
    let url = prev
        .url
        .join(location)
        .map_err(|e| EngineError::invalid(format!("Invalid redirect location '{location}': {e}")))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(EngineError::invalid(format!("Redirect to unsupported URL '{url}'")));
    }
    let to_get = status == 303 && prev.method != http::Method::HEAD
        || matches!(status, 301 | 302) && prev.method == http::Method::POST;
    let (method, body) = if to_get { (http::Method::GET, Bytes::new()) } else { (prev.method, prev.body) };
    let same_origin = url.origin() == prev.url.origin();
    let headers = prev
        .headers
        .into_iter()
        .filter(|h| {
            let n = h.name.to_ascii_lowercase();
            if to_get && matches!(n.as_str(), "content-type" | "content-encoding") {
                return false;
            }
            // Never leak credentials to another origin.
            same_origin || !matches!(n.as_str(), "authorization" | "cookie" | "host" | "proxy-authorization")
        })
        .collect();
    let mut url = url;
    url.set_fragment(None);
    Ok(Prepared { method, url, headers, body })
}

async fn read_body(mut body: RawBody, limit: usize) -> Result<(Vec<u8>, bool, u64)> {
    let mut out = Vec::new();
    let mut wire = 0u64;
    while let Some(data) = body.next_chunk().await {
        let data = data?;
        wire += data.len() as u64;
        let room = limit.saturating_sub(out.len());
        if data.len() > room {
            out.extend_from_slice(&data[..room]);
            return Ok((out, true, wire));
        }
        out.extend_from_slice(&data);
    }
    Ok((out, false, wire))
}

fn header_value(headers: &http::HeaderMap, name: &str) -> Option<String> {
    headers.get(name).map(|v| String::from_utf8_lossy(v.as_bytes()).into_owned())
}

fn header_values(headers: &http::HeaderMap, name: &str) -> Vec<String> {
    headers.get_all(name).iter().map(|v| String::from_utf8_lossy(v.as_bytes()).into_owned()).collect()
}

fn response_meta(
    parts: &http::response::Parts,
    redirects: Vec<RedirectHop>,
    sent: SentRequest,
    remote_addr: String,
    tls: Option<TlsInfo>,
) -> ResponseMeta {
    let status_text = parts
        .extensions
        .get::<hyper::ext::ReasonPhrase>()
        .map(|r| String::from_utf8_lossy(r.as_bytes()).into_owned())
        .or_else(|| parts.status.canonical_reason().map(str::to_string))
        .unwrap_or_default();
    let headers: Vec<Header> = parts
        .headers
        .iter()
        .map(|(n, v)| Header::new(canonical_name(n.as_str()), String::from_utf8_lossy(v.as_bytes())))
        .collect();
    let headers_size = headers.iter().map(|h| (h.name.len() + h.value.len() + 4) as u64).sum();
    let url = Url::parse(&sent.url).ok();
    let cookies = url.map(|u| parse_set_cookies(&header_values(&parts.headers, "set-cookie"), &u)).unwrap_or_default();
    ResponseMeta {
        status: parts.status.as_u16(),
        status_text,
        http_version: version_label(parts.version),
        headers,
        url: sent.url.clone(),
        remote_addr: Some(remote_addr),
        tls,
        redirects,
        request: sent,
        headers_size,
        cookies,
    }
}

fn version_label(v: http::Version) -> String {
    match v {
        http::Version::HTTP_09 => "HTTP/0.9",
        http::Version::HTTP_10 => "HTTP/1.0",
        http::Version::HTTP_11 => "HTTP/1.1",
        http::Version::HTTP_2 => "HTTP/2",
        http::Version::HTTP_3 => "HTTP/3",
        _ => "HTTP",
    }
    .to_string()
}

/// `content-type` -> `Content-Type` for display (hyper lowercases names).
pub fn canonical_name(name: &str) -> String {
    name.split('-')
        .map(|part| {
            let mut c = part.chars();
            match c.next() {
                Some(f) => f.to_ascii_uppercase().to_string() + &c.as_str().to_ascii_lowercase(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join("-")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prepared(method: &str, url: &str, headers: &[(&str, &str)]) -> Prepared {
        prepare(HttpRequest {
            method: method.into(),
            url: url.into(),
            headers: headers.iter().map(|(n, v)| Header::new(*n, *v)).collect(),
            body: Bytes::from_static(b"data"),
        })
        .unwrap()
    }

    #[test]
    fn url_normalization() {
        assert_eq!(normalize_url("localhost:3000/x#frag").unwrap().as_str(), "http://localhost:3000/x");
        assert!(normalize_url("ftp://x").is_err());
        assert!(normalize_url("mailto:x@y").is_err());
        assert_eq!(normalize_url("example.com:8080").unwrap().as_str(), "http://example.com:8080/");
        assert_eq!(normalize_url("localhost").unwrap().as_str(), "http://localhost/");
        assert!(normalize_url("  ").is_err());
        assert_eq!(normalize_url("https://h/a b?q=1 2").unwrap().as_str(), "https://h/a%20b?q=1%202");
    }

    #[test]
    fn redirect_303_switches_to_get_and_drops_body() {
        let p = prepared("POST", "http://a.test/x", &[("Content-Type", "json"), ("Authorization", "t")]);
        let r = redirect_request(p, 303, "/y").unwrap();
        assert_eq!(r.method, http::Method::GET);
        assert!(r.body.is_empty());
        assert_eq!(r.url.as_str(), "http://a.test/y");
        assert!(r.headers.iter().any(|h| h.name == "Authorization"));
        assert!(!r.headers.iter().any(|h| h.name == "Content-Type"));
    }

    #[test]
    fn redirect_307_keeps_method_and_strips_credentials_cross_origin() {
        let p = prepared("PUT", "http://a.test/x", &[("Authorization", "t"), ("Cookie", "c=1"), ("X-Keep", "1")]);
        let r = redirect_request(p, 307, "https://b.test/z").unwrap();
        assert_eq!(r.method, http::Method::PUT);
        assert_eq!(&r.body[..], b"data");
        let names: Vec<_> = r.headers.iter().map(|h| h.name.as_str()).collect();
        assert_eq!(names, vec!["X-Keep"]);
        assert!(redirect_request(prepared("GET", "http://a.test", &[]), 302, "javascript:alert(1)").is_err());
    }

    #[test]
    fn request_target_omits_userinfo_and_validates_host_override() {
        let url = normalize_url("http://user:pa%40ss@h.test:8080/a b?q=1").unwrap();
        assert_eq!(request_uri(&url, false, None).unwrap(), "/a%20b?q=1");
        // HTTP/2 :authority and proxy absolute-form must not carry credentials.
        assert_eq!(request_uri(&url, true, None).unwrap(), "http://h.test:8080/a%20b?q=1");
        assert_eq!(request_uri(&url, true, Some("api.test")).unwrap(), "http://api.test/a%20b?q=1");
        let v6 = normalize_url("https://[::1]:8443").unwrap();
        assert_eq!(request_uri(&v6, true, None).unwrap(), "https://[::1]:8443/");
        // A Host value with a path must not silently change the request path.
        assert!(request_uri(&url, true, Some("evil.test/x")).is_err());
    }

    #[test]
    fn cookie_merge_respects_user_values() {
        let mut h = vec![Header::new("Cookie", "a=user")];
        merge_cookie_header(&mut h, "a=jar; b=jar");
        assert_eq!(h[0].value, "a=user; b=jar");
    }

    #[test]
    fn canonical_names() {
        assert_eq!(canonical_name("content-type"), "Content-Type");
        assert_eq!(canonical_name("x-request-id"), "X-Request-Id");
    }
}

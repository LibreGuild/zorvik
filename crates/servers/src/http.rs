//! Mock HTTP servers: routes with canned (templated) responses, delays and
//! faults, CORS, and a fallback for unmatched requests (404, or forwarding to
//! the real backend). HTTP/1.1 and HTTP/2 (prior knowledge, or ALPN with TLS).
//!
//! The accept loop and connection handling here are shared with the SSE server.

use std::convert::Infallible;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use bytes::Bytes;
use http::header::{self, HeaderMap, HeaderName, HeaderValue};
use http::{Method, Request, Response, StatusCode, Version};
use http_body_util::{BodyExt, LengthLimitError, Limited};
use hyper::body::{Frame, Incoming, SizeHint};
use hyper_util::rt::{TokioExecutor, TokioIo, TokioTimer};
use hyper_util::server::conn::auto;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;
use zorvik_engine::http::canonical_name;
use zorvik_engine::{Header, HttpRequest};
use zorvik_formats::{HttpMockConfig, KeyValue, MockFallback, MockFault, MockRoute};
use zorvik_workspace::vars::VarContext;

use crate::report::{HttpExchange, MAX_ENTRY_PAYLOAD, Reporter};
use crate::tcp::TLS_HANDSHAKE_TIMEOUT;
use crate::template::{RequestValues, render};
use crate::{Control, Ctx, Live, StartOptions};

/// Larger request bodies are answered with 413.
const MAX_REQUEST_BODY: usize = 10 << 20;
/// Time a client gets to send its request body.
const BODY_TIMEOUT: Duration = Duration::from_secs(30);
/// Time a client gets to send the request head (slow clients hold connections),
/// also for its first request after connecting.
const HEADER_TIMEOUT: Duration = Duration::from_secs(30);
/// Longest route delay (a typo like 9999999999 still ends).
const MAX_DELAY: Duration = Duration::from_secs(24 * 3600);
/// Largest backend answer the proxy fallback passes on.
const MAX_PROXY_BODY: usize = 50 << 20;
/// Routes listed in the 404 answer.
const MAX_LISTED_ROUTES: usize = 100;
/// Marks requests the proxy fallback forwarded, to notice a backend URL pointing back at the mock.
const FORWARDED_MARK: &str = "x-zorvik-forwarded";

// ---- shared by the HTTP mock and SSE servers --------------------------------------------

/// Response body: one buffer, or chunks streamed from a channel (SSE).
pub(crate) enum Body {
    Full(Option<Bytes>),
    Stream(mpsc::Receiver<Bytes>),
}

impl Body {
    pub(crate) fn empty() -> Self {
        Body::Full(None)
    }

    pub(crate) fn full(bytes: impl Into<Bytes>) -> Self {
        let bytes = bytes.into();
        Body::Full((!bytes.is_empty()).then_some(bytes))
    }
}

impl hyper::body::Body for Body {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Result<Frame<Bytes>, Infallible>>> {
        match &mut *self {
            Body::Full(bytes) => Poll::Ready(bytes.take().map(|b| Ok(Frame::data(b)))),
            Body::Stream(rx) => rx.poll_recv(cx).map(|chunk| chunk.map(|b| Ok(Frame::data(b)))),
        }
    }

    fn is_end_stream(&self) -> bool {
        matches!(self, Body::Full(None))
    }

    fn size_hint(&self) -> SizeHint {
        match self {
            Body::Full(bytes) => SizeHint::with_exact(bytes.as_ref().map_or(0, |b| b.len() as u64)),
            Body::Stream(_) => SizeHint::default(),
        }
    }
}

/// Returned by a handler to drop the connection without answering.
#[derive(Debug)]
pub(crate) struct Abort;

impl std::fmt::Display for Abort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("connection dropped without an answer")
    }
}

impl std::error::Error for Abort {}

/// The client connection a request came in on.
#[derive(Clone)]
pub(crate) struct Peer {
    pub addr: SocketAddr,
    /// Connection id (HTTP mock) or `None` (SSE numbers its streams itself).
    pub conn: Option<u64>,
    /// Cancelled when the connection ends (client left or server stopped). HTTP/2
    /// requests run in their own tasks, so they watch this to end with it.
    pub closed: CancellationToken,
}

pub(crate) type HandlerFuture = Pin<Box<dyn Future<Output = Result<Response<Body>, Abort>> + Send>>;
pub(crate) type Handler = Arc<dyn Fn(Request<Incoming>, Peer) -> HandlerFuture + Send + Sync>;

/// Accept HTTP connections until the server stops; `on_control` answers the UI.
pub(crate) async fn serve(
    listener: TcpListener,
    mut ctx: Ctx,
    handler: Handler,
    number_connections: bool,
    mut on_control: impl FnMut(Control),
) -> Result<(), String> {
    let acceptor = ctx.tls.clone().map(tokio_rustls::TlsAcceptor::from);
    let slots = crate::connections_limit::Slots::new();
    loop {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok((stream, addr)) => {
                    let Some(slot) = slots.take(&ctx.reporter) else { continue };
                    let _ = stream.set_nodelay(true);
                    let peer = Peer {
                        addr,
                        conn: number_connections.then(|| ctx.reporter.next_conn()),
                        closed: ctx.cancel.child_token(),
                    };
                    let task = connection(stream, peer, acceptor.clone(), handler.clone(), ctx.reporter.clone());
                    tokio::spawn(async move {
                        task.await;
                        drop(slot);
                    });
                }
                Err(e) => {
                    ctx.reporter.error(None, None, format!("Accept failed: {e}"));
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            },
            Some(control) = ctx.control.recv() => on_control(control),
            _ = ctx.cancel.cancelled() => return Ok(()),
        }
    }
}

async fn connection(
    stream: TcpStream,
    peer: Peer,
    acceptor: Option<tokio_rustls::TlsAcceptor>,
    handler: Handler,
    reporter: Reporter,
) {
    let closed = peer.closed.clone();
    // Whatever ends this task (client, server stop) also ends the connection's requests.
    let _guard = closed.clone().drop_guard();
    let requested = Arc::new(AtomicBool::new(false));
    let serve = async {
        let requested = requested.clone();
        match acceptor {
            None => serve_connection(stream, peer, handler, requested).await,
            Some(acceptor) => match tokio::time::timeout(TLS_HANDSHAKE_TIMEOUT, acceptor.accept(stream)).await {
                Ok(Ok(tls)) => serve_connection(tls, peer, handler, requested).await,
                Ok(Err(e)) => reporter.error(peer.conn, Some(&peer.addr), format!("TLS handshake failed: {e}")),
                Err(_) => reporter.error(peer.conn, Some(&peer.addr), "TLS handshake timed out"),
            },
        }
    };
    // hyper's header timeout starts once it knows the protocol: a client that sends
    // nothing (or part of the HTTP/2 preface) and no request would stay until the stop.
    let silent = async {
        tokio::time::sleep(HEADER_TIMEOUT).await;
        if requested.load(Ordering::Relaxed) {
            std::future::pending::<()>().await;
        }
    };
    tokio::select! {
        _ = serve => {}
        _ = closed.cancelled() => {}
        _ = silent => {}
    }
}

async fn serve_connection<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
    io: S,
    peer: Peer,
    handler: Handler,
    requested: Arc<AtomicBool>,
) {
    let service = hyper::service::service_fn(move |req| {
        requested.store(true, Ordering::Relaxed);
        handler(req, peer.clone())
    });
    let mut builder = auto::Builder::new(TokioExecutor::new());
    builder.http1().timer(TokioTimer::new()).header_read_timeout(HEADER_TIMEOUT);
    builder.http2().timer(TokioTimer::new()).max_concurrent_streams(256u32).max_header_list_size(64 << 10);
    // Malformed requests and resets end the connection; there is nothing more to report.
    let _ = builder.serve_connection_with_upgrades(TokioIo::new(io), service).await;
}

pub(crate) fn version_label(v: Version) -> &'static str {
    match v {
        Version::HTTP_09 => "HTTP/0.9",
        Version::HTTP_10 => "HTTP/1.0",
        Version::HTTP_2 => "HTTP/2",
        Version::HTTP_3 => "HTTP/3",
        _ => "HTTP/1.1",
    }
}

/// Request headers as the log shows them.
pub(crate) fn header_list(headers: &HeaderMap) -> Vec<Header> {
    headers
        .iter()
        .map(|(n, v)| Header::new(canonical_name(n.as_str()), String::from_utf8_lossy(v.as_bytes())))
        .collect()
}

/// Decoded `?a=1&b=x+y` pairs, in order.
pub(crate) fn parse_query(query: &str) -> Vec<(String, String)> {
    query
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|pair| {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            (percent_decode(&k.replace('+', " ")), percent_decode(&v.replace('+', " ")))
        })
        .collect()
}

/// `%XX` decoding (invalid escapes are kept, invalid UTF-8 is replaced).
pub(crate) fn percent_decode(s: &str) -> String {
    if !s.contains('%') {
        return s.to_string();
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        match (bytes[i], bytes.get(i + 1).copied().and_then(hex), bytes.get(i + 2).copied().and_then(hex)) {
            (b'%', Some(hi), Some(lo)) => {
                out.push((hi * 16 + lo) as u8);
                i += 3;
            }
            (b, _, _) => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A body for the log: text up to 64 KB, or a note for binary data.
pub(crate) fn log_body(bytes: &[u8]) -> String {
    let shown = &bytes[..bytes.len().min(MAX_ENTRY_PAYLOAD)];
    let mut text = match std::str::from_utf8(shown) {
        Ok(s) => s.to_string(),
        // Cut in the middle of a character.
        Err(e) if e.error_len().is_none() => String::from_utf8_lossy(&shown[..e.valid_up_to()]).into_owned(),
        Err(_) => return format!("({} bytes of binary data)", bytes.len()),
    };
    if shown.len() < bytes.len() {
        text.push_str(&format!("\n… ({} bytes in total, only the first 64 KB are shown)", bytes.len()));
    }
    text
}

fn headers_size(headers: &HeaderMap) -> u64 {
    headers.iter().map(|(n, v)| (n.as_str().len() + v.len() + 4) as u64).sum()
}

// ---- mock server ------------------------------------------------------------------------

pub(crate) async fn run(listener: TcpListener, ctx: Ctx) -> Result<(), String> {
    let mock = Arc::new(Mock {
        live: ctx.live.clone(),
        reporter: ctx.reporter.clone(),
        options: ctx.options.clone(),
        stopped: ctx.cancel.clone(),
    });
    let handler: Handler = Arc::new(move |req: Request<Incoming>, peer: Peer| -> HandlerFuture {
        let mock = mock.clone();
        Box::pin(async move { mock.handle(req, peer).await })
    });
    serve(listener, ctx, handler, true, |control| match control {
        Control::Send { reply, .. } => {
            let _ = reply.send(Err("A mock API only answers requests: there is no connection to send to".into()));
        }
        Control::Disconnect { .. } => {}
    })
    .await
}

struct Mock {
    live: watch::Receiver<Arc<Live>>,
    reporter: Reporter,
    options: Arc<StartOptions>,
    stopped: CancellationToken,
}

enum BodyError {
    TooLarge,
    TimedOut,
    Failed(String),
}

async fn read_body(body: Incoming, headers: &HeaderMap) -> Result<Bytes, BodyError> {
    let declared = headers.get(header::CONTENT_LENGTH).and_then(|v| v.to_str().ok()?.trim().parse::<u64>().ok());
    if declared.is_some_and(|n| n > MAX_REQUEST_BODY as u64) {
        return Err(BodyError::TooLarge);
    }
    match tokio::time::timeout(BODY_TIMEOUT, Limited::new(body, MAX_REQUEST_BODY).collect()).await {
        Err(_) => Err(BodyError::TimedOut),
        Ok(Ok(collected)) => Ok(collected.to_bytes()),
        Ok(Err(e)) if e.downcast_ref::<LengthLimitError>().is_some() => Err(BodyError::TooLarge),
        Ok(Err(e)) => Err(BodyError::Failed(e.to_string())),
    }
}

/// One exchange for the traffic log. Reported once: when answered, or — dropped
/// unanswered — when the client left or the server stopped first.
struct Log {
    reporter: Reporter,
    peer: Peer,
    started: Instant,
    stopped: CancellationToken,
    exchange: Option<HttpExchange>,
    bytes_in: u64,
}

impl Log {
    fn duration_ms(&self) -> f64 {
        self.started.elapsed().as_secs_f64() * 1000.0
    }

    fn set_route(&mut self, route: Option<String>) {
        if let Some(x) = &mut self.exchange {
            x.route = route;
        }
    }

    /// Report an exchange that ends without an answer (`reset`, `hang`).
    fn unanswered(&mut self, note: &str) {
        if let Some(mut x) = self.exchange.take() {
            x.status = 0;
            x.note = Some(note.into());
            x.duration_ms = self.duration_ms();
            self.reporter.http(self.peer.conn, Some(&self.peer.addr), x, self.bytes_in, 0);
        }
    }

    /// Report the answer and build the response (headers only for HEAD).
    fn respond(
        &mut self,
        status: StatusCode,
        mut headers: HeaderMap,
        body: Bytes,
        head: bool,
        note: Option<String>,
    ) -> Response<Body> {
        let bodyless = status.is_informational() || matches!(status.as_u16(), 204 | 304);
        // HEAD answers tell the length of what GET would send (a proxied one keeps the
        // backend's); otherwise hyper sets it from the body.
        let keep_length = head && headers.contains_key(header::CONTENT_LENGTH);
        if !bodyless && !keep_length {
            headers.insert(header::CONTENT_LENGTH, HeaderValue::from(body.len()));
        }
        let body = if bodyless { Bytes::new() } else { body };
        let sent = if head { 0 } else { body.len() as u64 };
        if let Some(mut x) = self.exchange.take() {
            x.status = status.as_u16();
            x.response_headers = header_list(&headers);
            x.response_body = if head { String::new() } else { log_body(&body) };
            x.note = note;
            x.duration_ms = self.duration_ms();
            let out = headers_size(&headers) + 17 + sent;
            self.reporter.http(self.peer.conn, Some(&self.peer.addr), x, self.bytes_in, out);
        }
        if !head && !bodyless {
            // hyper computes the length from the body itself.
            headers.remove(header::CONTENT_LENGTH);
        }
        let mut response = Response::new(if head { Body::empty() } else { Body::full(body) });
        *response.status_mut() = status;
        *response.headers_mut() = headers;
        response
    }
}

impl Drop for Log {
    fn drop(&mut self) {
        if self.exchange.is_some() {
            let note = if self.stopped.is_cancelled() { "server stopped" } else { "client left" };
            self.unanswered(note);
        }
    }
}

fn json_error(message: &str) -> (HeaderMap, Bytes) {
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json"));
    let body = serde_json::to_vec(&serde_json::json!({ "error": message })).unwrap_or_default();
    (headers, Bytes::from(body))
}

impl Mock {
    async fn handle(&self, req: Request<Incoming>, peer: Peer) -> Result<Response<Body>, Abort> {
        let started = Instant::now();
        let (parts, body) = req.into_parts();
        let path = parts.uri.path().to_string();
        let query_string = parts.uri.query().unwrap_or_default().to_string();
        let target = if query_string.is_empty() { path.clone() } else { format!("{path}?{query_string}") };
        let request_headers = header_list(&parts.headers);
        let head_size = (parts.method.as_str().len() + target.len() + 12) as u64 + headers_size(&parts.headers);
        let mut log = Log {
            reporter: self.reporter.clone(),
            peer: peer.clone(),
            started,
            stopped: self.stopped.clone(),
            exchange: Some(HttpExchange {
                method: parts.method.to_string(),
                path: target,
                http_version: version_label(parts.version).into(),
                request_headers: request_headers.clone(),
                ..Default::default()
            }),
            bytes_in: head_size,
        };
        let is_head = parts.method == Method::HEAD;
        // The configuration as it is now: edits apply from the next request on.
        let live = self.live.borrow().clone();
        let config = &live.server.http;
        let origin = parts.headers.get(header::ORIGIN).cloned();
        let cors = |mut headers: HeaderMap| {
            if config.cors {
                add_cors(&mut headers, origin.as_ref());
            }
            headers
        };

        let body = match read_body(body, &parts.headers).await {
            Ok(body) => body,
            Err(BodyError::TooLarge) => {
                let (headers, body) = json_error("The request body is larger than 10 MB");
                return Ok(log.respond(StatusCode::PAYLOAD_TOO_LARGE, cors(headers), body, is_head, None));
            }
            Err(BodyError::TimedOut) => {
                let (headers, body) = json_error("The request body did not arrive in time");
                return Ok(log.respond(StatusCode::REQUEST_TIMEOUT, cors(headers), body, is_head, None));
            }
            Err(BodyError::Failed(e)) => {
                log.exchange = None;
                self.reporter.error(peer.conn, Some(&peer.addr), format!("Could not read the request body: {e}"));
                return Err(Abort);
            }
        };
        log.bytes_in += body.len() as u64;
        if let Some(x) = &mut log.exchange {
            x.request_body = log_body(&body);
        }
        let body_text = String::from_utf8_lossy(&body);

        if config.cors
            && parts.method == Method::OPTIONS
            && parts.headers.contains_key(header::ACCESS_CONTROL_REQUEST_METHOD)
        {
            let headers = preflight(&parts.headers);
            return Ok(log.respond(
                StatusCode::NO_CONTENT,
                headers,
                Bytes::new(),
                false,
                Some("CORS preflight".into()),
            ));
        }

        let query = parse_query(&query_string);
        let Some((route, params)) =
            find_route(config, &parts.method, &path, &query, &parts.headers, &body_text, &live.vars)
        else {
            return Ok(self.fallback(&mut log, &parts, body, config, &live.vars, cors).await);
        };
        log.set_route(Some(route_label(route)));

        let fault_hit = route.fault != MockFault::None
            && (route.fault_percent >= 100 || rand::random_range(0..100u8) < route.fault_percent);
        if fault_hit && route.fault == MockFault::Hang {
            // Never answer; the request ends when the client gives up or the server stops.
            log.unanswered("hang");
            peer.closed.cancelled().await;
            return Err(Abort);
        }
        if route.delay_ms > 0 {
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_millis(route.delay_ms).min(MAX_DELAY)) => {}
                _ = peer.closed.cancelled() => return Err(Abort),
            }
        }
        if fault_hit && route.fault == MockFault::Reset {
            log.unanswered("reset");
            return Err(Abort);
        }
        if fault_hit {
            let label = route_label(route);
            let (headers, body) = json_error(&format!("Injected fault on route {label}"));
            return Ok(log.respond(
                StatusCode::INTERNAL_SERVER_ERROR,
                cors(headers),
                body,
                is_head,
                Some("error".into()),
            ));
        }

        let values = RequestValues {
            method: parts.method.as_str(),
            path: &path,
            query_string: &query_string,
            params: &params,
            query: &query,
            headers: &request_headers,
            body: &body_text,
        };
        let (status, headers, body, problems) = route_response(route, &values, &live.vars);
        for problem in problems {
            self.reporter.error(peer.conn, Some(&peer.addr), format!("Route {}: {problem}", route_label(route)));
        }
        Ok(log.respond(status, cors(headers), body, is_head, None))
    }

    async fn fallback(
        &self,
        log: &mut Log,
        parts: &http::request::Parts,
        body: Bytes,
        config: &HttpMockConfig,
        vars: &VarContext,
        cors: impl Fn(HeaderMap) -> HeaderMap,
    ) -> Response<Body> {
        let is_head = parts.method == Method::HEAD;
        if config.fallback == MockFallback::Proxy {
            let base = render(&config.proxy_url, None, vars);
            let (status, headers, body) = match self.forward(parts, body, &base, &log.peer).await {
                Ok(answer) => answer,
                Err(message) => {
                    let (headers, body) = json_error(&message);
                    (StatusCode::BAD_GATEWAY, headers, body)
                }
            };
            return log.respond(status, cors(headers), body, is_head, Some("proxy".into()));
        }
        let routes: Vec<String> = config
            .routes
            .iter()
            .filter(|r| r.enabled)
            .take(MAX_LISTED_ROUTES)
            .map(|r| format!("{} {}", method_label(&r.method), r.path.trim()))
            .collect();
        let target = log.exchange.as_ref().map(|x| x.path.clone()).unwrap_or_default();
        let answer = serde_json::json!({
            "error": format!("No route matches {} {target}", parts.method),
            "routes": routes,
        });
        let mut headers = HeaderMap::new();
        headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json"));
        let body = serde_json::to_vec_pretty(&answer).unwrap_or_default();
        log.respond(StatusCode::NOT_FOUND, cors(headers), Bytes::from(body), is_head, None)
    }

    /// Forward to the backend and pass its answer back as it is (no redirects
    /// followed, no decompression).
    async fn forward(
        &self,
        parts: &http::request::Parts,
        body: Bytes,
        base: &str,
        peer: &Peer,
    ) -> Result<(StatusCode, HeaderMap, Bytes), String> {
        let base = base.trim().trim_end_matches('/');
        if base.is_empty() {
            return Err("No backend URL is set for requests that match no route".into());
        }
        // Not naming the URL: any client can send the mark, and the URL may hold variables' values.
        if parts.headers.contains_key(FORWARDED_MARK) {
            return Err("The request came back to the mock: the backend URL points at this server".into());
        }
        let target = parts.uri.path_and_query().map_or("/", |p| p.as_str());
        let dropped = connection_listed(&parts.headers);
        let mut headers: Vec<Header> = parts
            .headers
            .iter()
            .filter(|(n, _)| {
                let n = n.as_str();
                n != "host" && n != "content-length" && !is_hop_by_hop(n) && !dropped.iter().any(|d| d == n)
            })
            .map(|(n, v)| Header::new(canonical_name(n.as_str()), String::from_utf8_lossy(v.as_bytes())))
            .collect();
        headers.push(Header::new(canonical_name(FORWARDED_MARK), "1"));
        let mut opts = self.options.request_options.clone();
        opts.follow_redirects = false;
        opts.decompress = false;
        opts.default_headers = false;
        opts.max_body_bytes = MAX_PROXY_BODY;
        let request = HttpRequest { method: parts.method.to_string(), url: format!("{base}{target}"), headers, body };
        let send = self.options.client.send(request, &opts, None);
        let response = tokio::select! {
            r = send => r.map_err(|e| format!("Could not reach the backend: {}", e.message))?,
            _ = peer.closed.cancelled() => return Err("The client left".into()),
        };
        if response.body_truncated {
            return Err(format!("The backend's answer is larger than {} MB", MAX_PROXY_BODY >> 20));
        }
        let status = StatusCode::from_u16(response.meta.status).unwrap_or(StatusCode::BAD_GATEWAY);
        let mut out = HeaderMap::new();
        let dropped: Vec<String> = response
            .meta
            .headers
            .iter()
            .filter(|h| h.name.eq_ignore_ascii_case("connection"))
            .flat_map(|h| h.value.split(',').map(|v| v.trim().to_ascii_lowercase()).collect::<Vec<_>>())
            .collect();
        let head = parts.method == Method::HEAD;
        for h in &response.meta.headers {
            let lower = h.name.to_ascii_lowercase();
            // The length follows from the body passed on (a HEAD answer has none: keep the backend's).
            if (lower == "content-length" && !head) || is_hop_by_hop(&lower) || dropped.contains(&lower) {
                continue;
            }
            if let (Ok(name), Ok(value)) = (HeaderName::from_bytes(h.name.as_bytes()), HeaderValue::from_str(&h.value))
            {
                out.append(name, value);
            }
        }
        Ok((status, out, Bytes::from(response.body)))
    }
}

/// Headers that belong to one connection and are not forwarded (RFC 9110 §7.6.1).
fn is_hop_by_hop(lower: &str) -> bool {
    matches!(
        lower,
        "connection"
            | "keep-alive"
            | "proxy-connection"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "trailers"
            | "transfer-encoding"
            | "upgrade"
    )
}

/// Header names listed in `Connection` (also hop-by-hop).
fn connection_listed(headers: &HeaderMap) -> Vec<String> {
    headers
        .get_all(header::CONNECTION)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .map(|v| v.trim().to_ascii_lowercase())
        .filter(|v| !v.is_empty())
        .collect()
}

// ---- CORS -------------------------------------------------------------------------------

pub(crate) fn add_cors(headers: &mut HeaderMap, origin: Option<&HeaderValue>) {
    if !headers.contains_key(header::ACCESS_CONTROL_ALLOW_ORIGIN) {
        match origin {
            // Echo the origin (with credentials allowed) so cookies and auth headers work too.
            Some(origin) => {
                headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin.clone());
                headers.insert(header::ACCESS_CONTROL_ALLOW_CREDENTIALS, HeaderValue::from_static("true"));
                headers.append(header::VARY, HeaderValue::from_static("Origin"));
            }
            None => {
                headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));
            }
        }
    }
    if !headers.contains_key(header::ACCESS_CONTROL_EXPOSE_HEADERS) {
        // `*` is ignored for requests with credentials: name the headers instead.
        let mut names: Vec<&str> =
            headers.keys().map(HeaderName::as_str).filter(|n| !n.starts_with("access-control-")).collect();
        names.dedup();
        if let Ok(value) = HeaderValue::from_str(&names.join(", "))
            && !names.is_empty()
        {
            headers.insert(header::ACCESS_CONTROL_EXPOSE_HEADERS, value);
        }
    }
}

/// Answer to a CORS preflight: allow whatever the browser asks for.
pub(crate) fn preflight(request: &HeaderMap) -> HeaderMap {
    let mut headers = HeaderMap::new();
    add_cors(&mut headers, request.get(header::ORIGIN));
    headers.remove(header::ACCESS_CONTROL_EXPOSE_HEADERS);
    let mut methods = "GET, POST, PUT, PATCH, DELETE, HEAD, OPTIONS".to_string();
    if let Some(asked) = request.get(header::ACCESS_CONTROL_REQUEST_METHOD).and_then(|v| v.to_str().ok())
        && !methods.split(", ").any(|m| m.eq_ignore_ascii_case(asked.trim()))
    {
        methods = format!("{}, {methods}", asked.trim());
    }
    if let Ok(v) = HeaderValue::from_str(&methods) {
        headers.insert(header::ACCESS_CONTROL_ALLOW_METHODS, v);
    }
    if let Some(asked) = request.get(header::ACCESS_CONTROL_REQUEST_HEADERS) {
        headers.insert(header::ACCESS_CONTROL_ALLOW_HEADERS, asked.clone());
    }
    // Chrome asks before a public site may call a server on this computer.
    if request.get("access-control-request-private-network").is_some_and(|v| v == "true") {
        headers.insert("access-control-allow-private-network", HeaderValue::from_static("true"));
    }
    headers.insert(header::ACCESS_CONTROL_MAX_AGE, HeaderValue::from_static("600"));
    headers.append(
        header::VARY,
        HeaderValue::from_static("Access-Control-Request-Method, Access-Control-Request-Headers"),
    );
    headers
}

// ---- routes -----------------------------------------------------------------------------

fn method_label(method: &str) -> String {
    let m = method.trim();
    if m.is_empty() || m == "*" { "ANY".to_string() } else { m.to_ascii_uppercase() }
}

/// How the log names a route: its name, else method and path.
fn route_label(route: &MockRoute) -> String {
    let name = route.name.trim();
    if name.is_empty() { format!("{} {}", method_label(&route.method), route.path.trim()) } else { name.to_string() }
}

fn method_matches(route: &str, method: &Method) -> bool {
    let route = route.trim();
    route.is_empty() || route == "*" || route.eq_ignore_ascii_case("ANY") || route.eq_ignore_ascii_case(method.as_str())
}

/// The first enabled route answering this request, with its path parameters.
/// HEAD requests fall back to GET routes when no route takes HEAD itself.
fn find_route<'r>(
    config: &'r HttpMockConfig,
    method: &Method,
    path: &str,
    query: &[(String, String)],
    headers: &HeaderMap,
    body: &str,
    vars: &VarContext,
) -> Option<(&'r MockRoute, Vec<(String, String)>)> {
    let pass = |method: &Method| {
        config.routes.iter().filter(|r| r.enabled && method_matches(&r.method, method)).find_map(|route| {
            let params = match_path(&route.path, path)?;
            conditions_match(route, query, headers, body, vars).then_some((route, params))
        })
    };
    pass(method).or_else(|| if *method == Method::HEAD { pass(&Method::GET) } else { None })
}

fn param_name(segment: &str) -> Option<&str> {
    let name = segment
        .strip_prefix(':')
        .or_else(|| segment.strip_prefix('{').and_then(|s| s.strip_suffix('}')).filter(|s| !s.starts_with('{')))?;
    (!name.is_empty()).then_some(name)
}

/// Match a route path (`/users/:id`, `/files/*`) against a request path; returns
/// the parameters. Literal segments are case-sensitive; a trailing slash does not
/// matter; a trailing `*` takes the rest (also nothing), stored as `*`.
pub(crate) fn match_path(pattern: &str, path: &str) -> Option<Vec<(String, String)>> {
    let pattern = pattern.split(['?', '#']).next().unwrap_or_default();
    let wanted: Vec<&str> = pattern.split('/').filter(|s| !s.is_empty()).collect();
    let got: Vec<String> = path.split('/').filter(|s| !s.is_empty()).map(percent_decode).collect();
    let mut params = Vec::new();
    for (i, segment) in wanted.iter().enumerate() {
        if *segment == "*" && i + 1 == wanted.len() {
            params.push(("*".to_string(), got.get(i..).unwrap_or_default().join("/")));
            return Some(params);
        }
        let actual = got.get(i)?;
        if let Some(name) = param_name(segment) {
            params.push((name.to_string(), actual.clone()));
        } else if *segment != "*" && percent_decode(segment) != *actual {
            return None;
        }
    }
    (wanted.len() == got.len()).then_some(params)
}

/// `matchQuery`/`matchHeaders`/`matchBody` (plus a `?a=b` written in the route path).
/// Expected values may use variables; an empty value only requires presence.
fn conditions_match(
    route: &MockRoute,
    query: &[(String, String)],
    headers: &HeaderMap,
    body: &str,
    vars: &VarContext,
) -> bool {
    let expect = |value: &str| render(value, None, vars);
    let active = |kv: &&KeyValue| kv.enabled && !kv.key.trim().is_empty();
    let in_path = route.path.split_once('?').map(|(_, q)| parse_query(q)).unwrap_or_default();
    let query_ok = route
        .match_query
        .iter()
        .filter(active)
        .map(|kv| (kv.key.trim().to_string(), expect(&kv.value)))
        .chain(in_path)
        .all(|(key, want)| query.iter().any(|(k, v)| *k == key && (want.is_empty() || *v == want)));
    if !query_ok {
        return false;
    }
    let headers_ok = route.match_headers.iter().filter(active).all(|kv| {
        let want = expect(&kv.value);
        let Ok(name) = HeaderName::from_bytes(kv.key.trim().as_bytes()) else { return false };
        let mut values = headers.get_all(name).iter().peekable();
        values.peek().is_some()
            && (want.is_empty() || values.any(|v| String::from_utf8_lossy(v.as_bytes()).trim() == want.trim()))
    });
    headers_ok && (route.match_body.is_empty() || body.contains(expect(&route.match_body).as_str()))
}

/// Status, headers and body of a route (rendered), plus problems for the log.
fn route_response(
    route: &MockRoute,
    request: &RequestValues,
    vars: &VarContext,
) -> (StatusCode, HeaderMap, Bytes, Vec<String>) {
    let mut problems = Vec::new();
    let status = StatusCode::from_u16(route.status).unwrap_or_else(|_| {
        problems.push(format!("status {} is not valid, answered 500 instead", route.status));
        StatusCode::INTERNAL_SERVER_ERROR
    });
    let body = render(&route.body, Some(request), vars);
    let mut headers = HeaderMap::new();
    for kv in route.headers.iter().filter(|kv| kv.enabled && !kv.key.trim().is_empty()) {
        let name = render(kv.key.trim(), Some(request), vars);
        let value = render(&kv.value, Some(request), vars);
        match (HeaderName::from_bytes(name.trim().as_bytes()), HeaderValue::from_str(value.trim())) {
            // The length and framing follow from the body.
            (Ok(name), _) if name == header::CONTENT_LENGTH || name == header::TRANSFER_ENCODING => {}
            (Ok(name), Ok(value)) => {
                headers.append(name, value);
            }
            _ => problems.push(format!("header \"{}\" is not valid and was left out", name.trim())),
        }
    }
    if !body.is_empty() && !headers.contains_key(header::CONTENT_TYPE) {
        headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(guess_content_type(&body)));
    }
    (status, headers, Bytes::from(body), problems)
}

fn guess_content_type(body: &str) -> &'static str {
    let t = body.trim_start();
    if t.starts_with(['{', '[']) && serde_json::from_str::<serde::de::IgnoredAny>(t).is_ok() {
        "application/json"
    } else if t.starts_with("<?xml") {
        "application/xml"
    } else if t.starts_with('<') && t.get(..512).unwrap_or(t).to_ascii_lowercase().contains("<html") {
        "text/html; charset=utf-8"
    } else {
        "text/plain; charset=utf-8"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn path_patterns() {
        assert_eq!(match_path("/users/:id", "/users/42"), Some(params(&[("id", "42")])));
        assert_eq!(match_path("/users/:id/", "/users/42"), Some(params(&[("id", "42")])));
        assert_eq!(match_path("users/{id}", "/users/42/"), Some(params(&[("id", "42")])));
        assert_eq!(match_path("/users/:id", "/users"), None);
        assert_eq!(match_path("/users/:id", "/users/1/posts"), None);
        assert_eq!(match_path("/Users", "/users"), None, "case-sensitive");
        assert_eq!(match_path("/files/*", "/files/a/b%20c"), Some(params(&[("*", "a/b c")])));
        assert_eq!(match_path("/files/*", "/files"), Some(params(&[("*", "")])));
        assert_eq!(match_path("*", "/anything/at/all"), Some(params(&[("*", "anything/at/all")])));
        assert_eq!(match_path("/a/*/c", "/a/b/c"), Some(vec![]));
        assert_eq!(match_path("/", "/"), Some(vec![]));
        assert_eq!(match_path("/", "/x"), None);
        assert_eq!(match_path("/search?q=1", "/search"), Some(vec![]));
        assert_eq!(match_path("/name/:n", "/name/j%C3%BCrgen"), Some(params(&[("n", "jürgen")])));
    }

    #[test]
    fn query_and_decoding() {
        assert_eq!(parse_query("a=1&b=x+y&c&d=%2F%zz"), params(&[("a", "1"), ("b", "x y"), ("c", ""), ("d", "/%zz")]));
        assert_eq!(percent_decode("%E2%82"), "\u{fffd}");
    }

    #[test]
    fn route_selection_and_conditions() {
        let route =
            |method: &str, path: &str| MockRoute { method: method.into(), path: path.into(), ..Default::default() };
        let mut config = HttpMockConfig::default();
        let mut admin = route("GET", "/users/:id");
        admin.name = "admin".into();
        admin.match_query = vec![KeyValue::new("role", "admin")];
        admin.match_headers = vec![KeyValue::new("x-token", "")];
        let mut disabled = route("*", "/users/:id");
        disabled.enabled = false;
        let mut body = route("POST", "/login");
        body.match_body = "{{user}}".into();
        config.routes = vec![disabled, admin, route("GET", "/users/:id"), body, route("*", "/any")];
        let mut vars = VarContext::new();
        vars.push_layer(&[zorvik_formats::Variable {
            key: "user".into(),
            value: "ann".into(),
            enabled: true,
            secret: false,
        }]);
        let mut headers = HeaderMap::new();
        let find = |method: &Method, path: &str, query: &str, headers: &HeaderMap, body: &str| {
            find_route(&config, method, path, &parse_query(query), headers, body, &vars)
                .map(|(r, p)| (r.name.clone(), r.method.clone(), p))
        };
        let plain = find(&Method::GET, "/users/7", "role=admin", &headers, "").unwrap();
        assert_eq!((plain.0.as_str(), plain.2), ("", params(&[("id", "7")])), "needs the header");
        headers.insert("X-Token", HeaderValue::from_static("abc"));
        assert_eq!(find(&Method::GET, "/users/7", "role=admin", &headers, "").unwrap().0, "admin");
        assert_eq!(find(&Method::HEAD, "/users/7", "", &headers, "").unwrap().1, "GET", "HEAD uses GET routes");
        assert!(find(&Method::POST, "/login", "", &headers, "user=bob").is_none());
        assert!(find(&Method::POST, "/login", "", &headers, "user=ann").is_some());
        assert_eq!(find(&Method::DELETE, "/any", "", &headers, "").unwrap().1, "*");
        assert!(find(&Method::DELETE, "/users/7", "", &headers, "").is_none());
    }

    #[test]
    fn content_type_guess() {
        assert_eq!(guess_content_type(" {\"a\": 1}"), "application/json");
        assert_eq!(guess_content_type("{not json"), "text/plain; charset=utf-8");
        assert_eq!(guess_content_type("<?xml version=\"1.0\"?><a/>"), "application/xml");
        assert_eq!(guess_content_type("<!doctype html><html></html>"), "text/html; charset=utf-8");
    }

    #[test]
    fn log_bodies() {
        assert_eq!(log_body(b"hi"), "hi");
        assert_eq!(log_body(&[0xff, 0xfe, 0]), "(3 bytes of binary data)");
        let big = "é".repeat(40_000);
        let shown = log_body(big.as_bytes());
        assert!(shown.contains("80000 bytes in total"), "{}", &shown[shown.len() - 80..]);
    }
}

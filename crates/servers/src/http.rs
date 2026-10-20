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
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use zorvik_engine::http::canonical_name;
use zorvik_engine::{Header, HttpRequest};
use zorvik_formats::matching::MessageMatcher;
use zorvik_formats::{HttpMockConfig, KeyValue, MatchKind, MockFallback, MockFault, MockRoute, MockStep};
use zorvik_workspace::vars::VarContext;

use crate::report::{HttpExchange, MAX_ENTRY_PAYLOAD, Reporter};
use crate::tcp::TLS_HANDSHAKE_TIMEOUT;
use crate::template::{RequestValues, render};
use crate::{Control, Ctx, Live, PerConfig, StartOptions};

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
        live: PerConfig::new(ctx.live.clone(), Prepared::new),
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
    /// The configuration, and what its routes need besides it.
    live: PerConfig<Prepared>,
    reporter: Reporter,
    options: Arc<StartOptions>,
    stopped: CancellationToken,
}

/// What the routes need besides the configuration, worked out once per change.
struct Prepared {
    /// Per route: its JSON body condition.
    conditions: Vec<BodyCondition>,
    /// Per route: calls so far (for sequences). A route that stays the same keeps counting
    /// through other edits; a changed one starts again.
    calls: Vec<Arc<AtomicU64>>,
}

impl Prepared {
    fn new(live: &Live, previous: Option<(&Live, &Prepared)>) -> Self {
        let routes = &live.server.http.routes;
        let conditions = routes.iter().map(|r| BodyCondition::new(&r.match_json, &live.vars)).collect();
        let calls = routes
            .iter()
            .enumerate()
            .map(|(i, route)| {
                previous
                    .filter(|(old, _)| old.server.http.routes.get(i).is_some_and(|old| same_calls(old, route)))
                    .and_then(|(_, prepared)| prepared.calls.get(i).cloned())
                    .unwrap_or_default()
            })
            .collect();
        Self { conditions, calls }
    }
}

/// Whether two versions of a route count their calls as one: they take the same requests and
/// have the same sequence (the route's own answer may change).
fn same_calls(a: &MockRoute, b: &MockRoute) -> bool {
    (&a.method, &a.path, a.status, &a.sequence, a.sequence_repeat)
        == (&b.method, &b.path, b.status, &b.sequence, b.sequence_repeat)
        && (&a.match_query, &a.match_headers, &a.match_body, &a.match_json)
            == (&b.match_query, &b.match_headers, &b.match_body, &b.match_json)
}

/// A route's JSON body condition (`matchJson`).
enum BodyCondition {
    None,
    /// Filled in with the configuration's variables and compiled; `None` when it doesn't parse
    /// (the route never matches).
    Fixed(Option<MessageMatcher>),
    /// With dynamic values (`{{$randomInt}}`): filled in for each request.
    PerRequest,
}

impl BodyCondition {
    fn new(condition: &str, vars: &VarContext) -> Self {
        if condition.trim().is_empty() {
            Self::None
        } else if condition.contains("{{$") {
            Self::PerRequest
        } else {
            Self::Fixed(json_matcher(&render(condition, None, vars)))
        }
    }

    /// Whether the request's body is JSON meeting the condition (parsed once per request).
    fn matches(&self, route: &MockRoute, request: &RequestValues, vars: &VarContext) -> bool {
        let per_request;
        let matcher = match self {
            Self::None => return true,
            Self::Fixed(matcher) => matcher.as_ref(),
            Self::PerRequest => {
                per_request = json_matcher(&render(&route.match_json, None, vars));
                per_request.as_ref()
            }
        };
        matcher.is_some_and(|m| request.json().is_some_and(|root| m.matches_json(root) == Some(true)))
    }
}

fn json_matcher(condition: &str) -> Option<MessageMatcher> {
    MessageMatcher::new(MatchKind::Json, condition.as_bytes()).ok()
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
            let encoded = headers.get(header::CONTENT_ENCODING).is_some_and(|v| v != "identity");
            x.response_body_complete =
                head || (!encoded && body.len() <= MAX_ENTRY_PAYLOAD && std::str::from_utf8(&body).is_ok());
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
        let (live, prepared) = self.live.get();
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
        let mut values = RequestValues {
            method: parts.method.as_str(),
            path: &path,
            query_string: &query_string,
            params: &[],
            query: &query,
            headers: &request_headers,
            body: &body_text,
            json: Default::default(),
        };
        let Some(Found { index, route, params }) = find_route(config, &prepared, &parts.method, &values, &live.vars)
        else {
            return Ok(self.fallback(&mut log, &parts, body, config, &live.vars, cors).await);
        };
        values.params = &params;
        log.set_route(Some(route_label(route)));

        // Which answer of the route's sequence this call gets (none without a sequence).
        let call = (!route.sequence.is_empty()).then(|| prepared.calls[index].fetch_add(1, Ordering::Relaxed));
        let step = call.and_then(|n| route.sequence_step(n)).and_then(|i| route.sequence.get(i));
        let place = call.and_then(|n| sequence_place(route, n));
        let note = |what: &str| match &place {
            Some(place) if what.is_empty() => Some(place.clone()),
            Some(place) => Some(format!("{what} · {place}")),
            None => (!what.is_empty()).then(|| what.to_string()),
        };
        let fault = match step {
            Some(step) => step.fault,
            None if route.fault != MockFault::None
                && (route.fault_percent >= 100 || rand::random_range(0..100u8) < route.fault_percent) =>
            {
                route.fault
            }
            None => MockFault::None,
        };
        let delay_ms = step.and_then(|s| s.delay_ms).unwrap_or(route.delay_ms);
        if fault == MockFault::Hang {
            // Never answer; the request ends when the client gives up or the server stops.
            log.unanswered(&note("hang").unwrap_or_default());
            peer.closed.cancelled().await;
            return Err(Abort);
        }
        if delay_ms > 0 {
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_millis(delay_ms).min(MAX_DELAY)) => {}
                _ = peer.closed.cancelled() => return Err(Abort),
            }
        }
        if fault == MockFault::Reset {
            log.unanswered(&note("reset").unwrap_or_default());
            return Err(Abort);
        }
        if fault == MockFault::Error {
            let label = route_label(route);
            let (headers, body) = json_error(&format!("Injected fault on route {label}"));
            return Ok(log.respond(StatusCode::INTERNAL_SERVER_ERROR, cors(headers), body, is_head, note("error")));
        }

        let (status, headers, body, problems) = route_response(route, step, &values, &live.vars);
        for problem in problems {
            self.reporter.error(peer.conn, Some(&peer.addr), format!("Route {}: {problem}", route_label(route)));
        }
        Ok(log.respond(status, cors(headers), body, is_head, note("")))
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
            // Only the backend's own answers are noted "proxy": the traffic panel saves those as routes.
            let (status, headers, body, note) = match self.forward(parts, body, &base, &log.peer).await {
                Ok((status, headers, body)) => (status, headers, body, "proxy"),
                Err(message) => {
                    let (headers, body) = json_error(&message);
                    (StatusCode::BAD_GATEWAY, headers, body, "proxy failed")
                }
            };
            return log.respond(status, cors(headers), body, is_head, Some(note.into()));
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

/// The route answering a request.
struct Found<'r> {
    /// Its position in the configuration.
    index: usize,
    route: &'r MockRoute,
    /// Values of its `:name` segments.
    params: Vec<(String, String)>,
}

/// The first enabled route answering this request. HEAD requests fall back to GET routes when
/// no route takes HEAD itself. `prepared` belongs to the same configuration as `config`.
fn find_route<'r>(
    config: &'r HttpMockConfig,
    prepared: &Prepared,
    method: &Method,
    request: &RequestValues,
    vars: &VarContext,
) -> Option<Found<'r>> {
    let pass = |method: &Method| {
        config.routes.iter().enumerate().filter(|(_, r)| r.enabled && method_matches(&r.method, method)).find_map(
            |(index, route)| {
                let params = match_path(&route.path, request.path)?;
                let condition = &prepared.conditions[index];
                conditions_match(route, condition, request, vars).then_some(Found { index, route, params })
            },
        )
    };
    pass(method).or_else(|| if *method == Method::HEAD { pass(&Method::GET) } else { None })
}

/// A parameter segment (`:id`, `{id}`): its name, and the literal text after it, as in
/// Google's custom methods (`:name:generateContent`, `{name}:cancel`).
/// A segment holding more parameters (`:name.:ext`, `{a}-{b}`, as OpenAPI's `{name}.{ext}`
/// becomes) is one parameter named by all of it, which takes any value.
fn param_segment(segment: &str) -> Option<(&str, &str)> {
    let (whole, (name, suffix)) = match segment.strip_prefix(':') {
        Some(rest) => (rest, rest.find(':').map_or((rest, ""), |i| rest.split_at(i))),
        None => {
            let rest = segment.strip_prefix('{').filter(|s| !s.starts_with('{'))?;
            (rest.strip_suffix('}').unwrap_or(rest), rest.split_once('}')?)
        }
    };
    // A verb follows a name (`:name:verb`); `name.:ext` or `a::b` start another parameter.
    let another = suffix.get(1..).is_some_and(|s| s.contains([':', '{']))
        || (suffix.starts_with(':') && !name.ends_with(|c: char| c.is_alphanumeric() || c == '_'));
    let (name, suffix) = if another { (whole, "") } else { (name, suffix) };
    (!name.is_empty()).then_some((name, suffix))
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
        if let Some((name, suffix)) = param_segment(segment) {
            let value = actual.strip_suffix(suffix).filter(|v| !v.is_empty())?;
            params.push((name.to_string(), value.to_string()));
        } else if *segment != "*" && percent_decode(segment) != *actual {
            return None;
        }
    }
    (wanted.len() == got.len()).then_some(params)
}

/// `matchQuery`/`matchHeaders`/`matchBody` (plus a `?a=b` written in the route path) and
/// `matchJson`. Expected values may use variables; an empty value only requires presence.
fn conditions_match(route: &MockRoute, json: &BodyCondition, request: &RequestValues, vars: &VarContext) -> bool {
    let expect = |value: &str| render(value, None, vars);
    let active = |kv: &&KeyValue| kv.enabled && !kv.key.trim().is_empty();
    let in_path = route.path.split_once('?').map(|(_, q)| parse_query(q)).unwrap_or_default();
    let query_ok = route
        .match_query
        .iter()
        .filter(active)
        .map(|kv| (kv.key.trim().to_string(), expect(&kv.value)))
        .chain(in_path)
        .all(|(key, want)| request.query.iter().any(|(k, v)| *k == key && (want.is_empty() || *v == want)));
    if !query_ok {
        return false;
    }
    let headers_ok = route.match_headers.iter().filter(active).all(|kv| {
        let want = expect(&kv.value);
        let name = kv.key.trim();
        let mut values = request.headers.iter().filter(|h| h.name.eq_ignore_ascii_case(name)).peekable();
        values.peek().is_some() && (want.is_empty() || values.any(|h| h.value.trim() == want.trim()))
    });
    headers_ok
        && (route.match_body.is_empty() || request.body.contains(expect(&route.match_body).as_str()))
        && json.matches(route, request, vars)
}

/// `answer 2 of 3` for call `call` (0 for the first) of a route with a sequence; nothing once a
/// sequence that doesn't repeat is over.
fn sequence_place(route: &MockRoute, call: u64) -> Option<String> {
    let total = route.sequence.len() as u64 + 1;
    let at = if route.sequence_repeat { call % total } else { call };
    (total > 1 && at < total).then(|| format!("answer {} of {total}", at + 1))
}

/// Status, headers and body of a route, or of one answer of its sequence (rendered), plus
/// problems for the log.
fn route_response(
    route: &MockRoute,
    step: Option<&MockStep>,
    request: &RequestValues,
    vars: &VarContext,
) -> (StatusCode, HeaderMap, Bytes, Vec<String>) {
    let mut problems = Vec::new();
    let wanted = step.map_or(route.status, |s| s.status);
    let status = StatusCode::from_u16(wanted).unwrap_or_else(|_| {
        problems.push(format!("status {wanted} is not valid, answered 500 instead"));
        StatusCode::INTERNAL_SERVER_ERROR
    });
    let body = render(step.map_or(&route.body, |s| &s.body), Some(request), vars);
    let mut headers = HeaderMap::new();
    let mut add = |kv: &KeyValue, replace: bool, headers: &mut HeaderMap, replaced: &mut Vec<HeaderName>| {
        let name = render(kv.key.trim(), Some(request), vars);
        let value = render(&kv.value, Some(request), vars);
        match (HeaderName::from_bytes(name.trim().as_bytes()), HeaderValue::from_str(value.trim())) {
            // The length and framing follow from the body.
            (Ok(name), _) if name == header::CONTENT_LENGTH || name == header::TRANSFER_ENCODING => {}
            (Ok(name), Ok(value)) => {
                // A step's header replaces the route's of that name (several of its own stay).
                if replace && !replaced.contains(&name) {
                    headers.remove(&name);
                    replaced.push(name.clone());
                }
                headers.append(name, value);
            }
            _ => problems.push(format!("header \"{}\" is not valid and was left out", name.trim())),
        }
    };
    let active = |kv: &&KeyValue| kv.enabled && !kv.key.trim().is_empty();
    let mut replaced = Vec::new();
    for kv in route.headers.iter().filter(active) {
        add(kv, false, &mut headers, &mut replaced);
    }
    for kv in step.map_or(&[][..], |s| &s.headers[..]).iter().filter(active) {
        add(kv, true, &mut headers, &mut replaced);
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

    type Params = Vec<(String, String)>;

    fn params(list: &[(&str, &str)]) -> Params {
        list.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    fn live(routes: Vec<MockRoute>, vars: &VarContext) -> Live {
        let mut server = zorvik_formats::Server::new("Mock", zorvik_formats::ServerKind::Http);
        server.http.routes = routes;
        Live { server, vars: vars.clone() }
    }

    /// The route answering a request, as the server finds it: `(name, method, path parameters)`.
    fn find(
        live: &Live,
        method: &Method,
        path: &str,
        query: &str,
        headers: &[Header],
        body: &str,
    ) -> Option<(String, String, Params)> {
        let prepared = Prepared::new(live, None);
        let query = parse_query(query);
        let request =
            RequestValues { method: method.as_str(), path, query: &query, headers, body, ..Default::default() };
        find_route(&live.server.http, &prepared, method, &request, &live.vars)
            .map(|found| (found.route.name.clone(), found.route.method.clone(), found.params))
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
        // A custom method after a parameter (Google APIs): the verb must match too.
        let generate = "/v1/models/:model:generateContent";
        assert_eq!(match_path(generate, "/v1/models/gemini:generateContent"), Some(params(&[("model", "gemini")])));
        assert_eq!(match_path(generate, "/v1/models/gemini:countTokens"), None);
        assert_eq!(match_path(generate, "/v1/models/:generateContent"), None);
        assert_eq!(match_path("/v1/{name}:cancel", "/v1/op-1:cancel"), Some(params(&[("name", "op-1")])));
        assert_eq!(match_path("/v1/{name}:cancel", "/v1/op-1"), None);
        // Several parameters in one segment (OpenAPI's `{name}.{ext}` becomes `:name.:ext`): one
        // parameter takes the whole segment, as it always did.
        assert_eq!(match_path("/files/:name.:ext", "/files/a.json"), Some(params(&[("name.:ext", "a.json")])));
        assert_eq!(match_path("/r/:a-:b", "/r/1-2"), Some(params(&[("a-:b", "1-2")])));
        assert_eq!(match_path("/r/:a::b", "/r/1:2"), Some(params(&[("a::b", "1:2")])));
        assert_eq!(match_path("/files/{name}.{ext}", "/files/a.json"), Some(params(&[("name}.{ext", "a.json")])));
        assert_eq!(match_path("/users/:user-id", "/users/7"), Some(params(&[("user-id", "7")])));
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
        let mut admin = route("GET", "/users/:id");
        admin.name = "admin".into();
        admin.match_query = vec![KeyValue::new("role", "admin")];
        admin.match_headers = vec![KeyValue::new("x-token", "")];
        let mut disabled = route("*", "/users/:id");
        disabled.enabled = false;
        let mut body = route("POST", "/login");
        body.match_body = "{{user}}".into();
        let routes = vec![disabled, admin, route("GET", "/users/:id"), body, route("*", "/any")];
        let mut vars = VarContext::new();
        vars.push_layer(&[zorvik_formats::Variable {
            key: "user".into(),
            value: "ann".into(),
            enabled: true,
            secret: false,
        }]);
        let live = live(routes, &vars);
        let find = |method: &Method, path: &str, query: &str, headers: &[Header], body: &str| {
            find(&live, method, path, query, headers, body)
        };
        let plain = find(&Method::GET, "/users/7", "role=admin", &[], "").unwrap();
        assert_eq!((plain.0.as_str(), plain.2), ("", params(&[("id", "7")])), "needs the header");
        let headers = [Header::new("X-Token", "abc")];
        assert_eq!(find(&Method::GET, "/users/7", "role=admin", &headers, "").unwrap().0, "admin");
        assert_eq!(find(&Method::HEAD, "/users/7", "", &headers, "").unwrap().1, "GET", "HEAD uses GET routes");
        assert!(find(&Method::POST, "/login", "", &headers, "user=bob").is_none());
        assert!(find(&Method::POST, "/login", "", &headers, "user=ann").is_some());
        assert_eq!(find(&Method::DELETE, "/any", "", &headers, "").unwrap().1, "*");
        assert!(find(&Method::DELETE, "/users/7", "", &headers, "").is_none());
    }

    #[test]
    fn json_body_conditions() {
        let mut admin =
            MockRoute { name: "admin".into(), method: "POST".into(), path: "/login".into(), ..Default::default() };
        admin.match_json = "$.user.role == {{role}}".into();
        let mut broken =
            MockRoute { name: "broken".into(), method: "POST".into(), path: "/login".into(), ..Default::default() };
        broken.match_json = "$..x".into();
        let any = MockRoute { method: "POST".into(), path: "/login".into(), ..Default::default() };
        let mut dynamic =
            MockRoute { name: "dynamic".into(), method: "POST".into(), path: "/login".into(), ..Default::default() };
        dynamic.match_json = "$.n == {{$randomInt(4, 4)}}".into();
        let mut vars = VarContext::new();
        vars.push_layer(&[zorvik_formats::Variable {
            key: "role".into(),
            value: "\"admin\"".into(),
            enabled: true,
            secret: false,
        }]);
        let live = live(vec![broken, admin, dynamic, any], &vars);
        let find = |body: &str| find(&live, &Method::POST, "/login", "", &[], body).unwrap().0;
        assert_eq!(find(r#"{"user": {"role": "admin"}}"#), "admin");
        assert_eq!(find(r#"{"user": {"role": "guest"}}"#), "");
        assert_eq!(find("role=admin"), "", "not JSON");
        assert_eq!(find(r#"{"n": 4}"#), "dynamic");
        assert_eq!(sequence_place(&live.server.http.routes[3], 0), None, "no sequence");
        // Compiled once per configuration; dynamic values are filled in for each request.
        let prepared = Prepared::new(&live, None);
        assert!(matches!(prepared.conditions[0], BodyCondition::Fixed(None)), "doesn't parse");
        assert!(matches!(prepared.conditions[1], BodyCondition::Fixed(Some(_))));
        assert!(matches!(prepared.conditions[2], BodyCondition::PerRequest));
        assert!(matches!(prepared.conditions[3], BodyCondition::None));
    }

    #[test]
    fn call_counters_carry_over_unrelated_edits() {
        let step = MockStep { status: 503, ..Default::default() };
        let flaky = MockRoute { path: "/flaky".into(), sequence: vec![step.clone()], ..Default::default() };
        let other = MockRoute { path: "/other".into(), sequence: vec![step], ..Default::default() };
        let vars = VarContext::new();
        let first = live(vec![flaky.clone(), other.clone()], &vars);
        let prepared = Prepared::new(&first, None);
        prepared.calls[0].fetch_add(3, Ordering::Relaxed);
        prepared.calls[1].fetch_add(5, Ordering::Relaxed);
        // The answer's body, another route and the variables change: the count goes on.
        let mut edited = flaky.clone();
        edited.body = "new body".into();
        let mut other_changed = other.clone();
        other_changed.path = "/moved".into();
        let mut vars2 = VarContext::new();
        vars2.push_layer(&[zorvik_formats::Variable {
            key: "a".into(),
            value: "b".into(),
            enabled: true,
            secret: false,
        }]);
        let second = live(vec![edited.clone(), other_changed], &vars2);
        let carried = Prepared::new(&second, Some((&first, &prepared)));
        assert_eq!(carried.calls[0].load(Ordering::Relaxed), 3);
        assert!(Arc::ptr_eq(&carried.calls[0], &prepared.calls[0]), "one counter for both versions");
        assert_eq!(carried.calls[1].load(Ordering::Relaxed), 0, "a changed route starts again");
        // A changed sequence starts again too.
        let mut resequenced = edited;
        resequenced.sequence_repeat = true;
        let third = live(vec![resequenced], &vars2);
        let restarted = Prepared::new(&third, Some((&second, &carried)));
        assert_eq!(restarted.calls[0].load(Ordering::Relaxed), 0);
    }

    #[test]
    fn sequence_answers_and_headers() {
        let route = MockRoute {
            status: 200,
            body: "ok {{request.method}}".into(),
            headers: vec![KeyValue::new("Content-Type", "text/plain"), KeyValue::new("X-Kept", "1")],
            sequence: vec![MockStep {
                status: 503,
                body: "busy".into(),
                headers: vec![KeyValue::new("content-type", "application/json"), KeyValue::new("Retry-After", "1")],
                ..Default::default()
            }],
            ..Default::default()
        };
        let req = RequestValues { method: "GET", ..Default::default() };
        let vars = VarContext::new();
        let (status, headers, body, problems) = route_response(&route, Some(&route.sequence[0]), &req, &vars);
        assert!(problems.is_empty());
        assert_eq!((status.as_u16(), &body[..]), (503, &b"busy"[..]));
        assert_eq!(headers.get_all(header::CONTENT_TYPE).iter().collect::<Vec<_>>(), ["application/json"]);
        assert_eq!((headers["x-kept"].as_bytes(), headers["retry-after"].as_bytes()), (&b"1"[..], &b"1"[..]));
        let (status, headers, body, _) = route_response(&route, None, &req, &vars);
        assert_eq!((status.as_u16(), &body[..], headers.get("retry-after")), (200, &b"ok GET"[..], None));
        let places: Vec<Option<String>> = (0..3).map(|n| sequence_place(&route, n)).collect();
        assert_eq!(places, [Some("answer 1 of 2".into()), Some("answer 2 of 2".into()), None]);
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

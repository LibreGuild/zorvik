//! gRPC client: unary and streaming calls over HTTP/2 on the engine's connect
//! path (DNS, Happy Eyeballs, a proxy CONNECT tunnel, OS-trusted TLS with ALPN
//! `h2`; `grpc://` is plaintext HTTP/2 with prior knowledge), so gRPC behaves
//! like HTTP requests on the same network. Messages are JSON in and out,
//! converted with descriptors from server reflection (`reflection.rs`) or
//! `.proto` files (`schema.rs`). Framing and status handling: `codec.rs`.
//!
//! A fresh connection per call, like HTTP requests (honest timing). Unary
//! calls use the request timeout as their deadline (`grpc-timeout`); streams
//! have none and run until they end or are cancelled.

mod codec;
mod reflection;
mod schema;

use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use bytes::Bytes;
use http::{HeaderMap, HeaderName, HeaderValue};
use http_body_util::BodyExt;
use hyper::body::{Frame, Incoming};
use hyper::client::conn::http2;
use hyper_util::rt::{TokioExecutor, TokioIo};
use prost_reflect::{DescriptorPool, MessageDescriptor};
use serde::Serialize;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use ts_rs::TS;
use url::Url;

pub use codec::{MAX_MESSAGE_BYTES, status_name};
pub use prost_reflect::MethodDescriptor;
pub use schema::{GrpcDescriptors, GrpcMethod, GrpcService, example_json, message_from_json, method_path};

use crate::error::{EngineError, ErrorKind, Result, human_duration};
use crate::http::{Client, Header, RequestOptions, Timing, USER_AGENT};
use crate::net::{self, ConnectTiming, Target};
use crate::socket::{ms, now_ms};
use crate::tls::{Alpn, TlsInfo};
use crate::ws::Direction;
use codec::{CANCELLED, DEADLINE_EXCEEDED, INTERNAL, UNAVAILABLE, UNKNOWN};

/// Where to call: the resolved URL and metadata.
#[derive(Debug, Clone)]
pub struct GrpcTarget {
    /// `grpc://host:port` (plaintext HTTP/2) or `grpcs://host:port` (TLS);
    /// `http://` and `https://` work too. A path becomes a prefix of method paths.
    pub url: String,
    /// Custom metadata. `-bin` keys take base64 (other text is encoded for you);
    /// a `Host` entry sets `:authority`.
    pub metadata: Vec<Header>,
}

/// Final status of a call.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GrpcStatus {
    pub code: u32,
    /// `OK`, `NOT_FOUND`, …
    pub name: String,
    /// `grpc-message` (percent-decoded).
    pub message: String,
    /// `grpc-status-details-bin`: the `google.rpc.Status` details as JSON, or the raw value.
    pub details: Option<String>,
    /// Set by Zorvik (cancelled, deadline, connection lost, HTTP error) rather than sent by the server.
    pub local: bool,
}

impl GrpcStatus {
    pub(crate) fn new(code: u32, message: impl Into<String>, local: bool) -> Self {
        Self { code, name: status_name(code), message: message.into(), details: None, local }
    }

    fn with_details(mut self, details: Option<String>) -> Self {
        self.details = details;
        self
    }

    pub fn is_ok(&self) -> bool {
        self.code == 0
    }
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GrpcMessage {
    pub direction: Direction,
    /// The message as JSON (proto3 mapping, default values included).
    pub json: String,
    /// Encoded size in bytes.
    #[ts(type = "number")]
    pub size: u64,
    /// Unix epoch milliseconds.
    pub timestamp: f64,
}

/// Result of a unary call.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GrpcResponse {
    pub status: GrpcStatus,
    /// Received messages (one for a successful unary call).
    pub messages: Vec<GrpcMessage>,
    /// Response headers (initial metadata); empty for a trailers-only response.
    pub headers: Vec<Header>,
    pub trailers: Vec<Header>,
    pub timing: Timing,
    pub remote_addr: Option<String>,
    pub tls: Option<TlsInfo>,
    /// Request headers as sent: the gRPC ones, then your metadata.
    pub request_headers: Vec<Header>,
    /// Problems that did not end the call (e.g. a message that could not be decoded).
    pub warnings: Vec<String>,
}

/// What opening a streaming call established.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GrpcOpened {
    pub remote_addr: Option<String>,
    pub tls: Option<TlsInfo>,
    /// DNS, connect and TLS; the full timing comes with the `end` event.
    pub timing: Timing,
    pub request_headers: Vec<Header>,
}

/// Something that happened on a streaming call.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(export)]
pub enum GrpcEvent {
    /// Response headers (initial metadata).
    #[serde(rename_all = "camelCase")]
    Headers { headers: Vec<Header>, timestamp: f64 },
    /// A message sent (by [`GrpcSession::send`]) or received.
    #[serde(rename_all = "camelCase")]
    Message { message: GrpcMessage },
    /// A problem that does not end the call.
    #[serde(rename_all = "camelCase")]
    Error { message: String },
    /// The call is over; always the last event.
    #[serde(rename_all = "camelCase")]
    End { status: GrpcStatus, trailers: Vec<Header>, timing: Timing },
}

pub struct GrpcStream {
    pub opened: GrpcOpened,
    pub session: GrpcSession,
    pub events: mpsc::UnboundedReceiver<GrpcEvent>,
}

/// Handle to a streaming call. Dropping it cancels the call.
pub struct GrpcSession {
    input: MessageDescriptor,
    /// Request body; `None` once the client side is ended (half-closed).
    body: Mutex<Option<mpsc::UnboundedSender<Bytes>>>,
    events: mpsc::UnboundedSender<GrpcEvent>,
    ended: Arc<AtomicBool>,
    cancel: CancellationToken,
}

impl GrpcSession {
    /// Encode a JSON message of the method's input type and send it.
    pub fn send(&self, json: &str) -> Result<()> {
        if self.ended.load(Ordering::SeqCst) {
            return Err(EngineError::new(ErrorKind::Io, "The call has ended"));
        }
        let (payload, shown) = schema::encode_json(&self.input, json)?;
        let body = self.body.lock().unwrap_or_else(|e| e.into_inner());
        let tx = body
            .as_ref()
            .ok_or_else(|| EngineError::invalid("The stream was ended; start a new call to send more messages"))?;
        tx.send(codec::frame(&payload)).map_err(|_| EngineError::new(ErrorKind::Io, "The call has ended"))?;
        let message =
            GrpcMessage { direction: Direction::Sent, json: shown, size: payload.len() as u64, timestamp: now_ms() };
        let _ = self.events.send(GrpcEvent::Message { message });
        Ok(())
    }

    /// Tell the server no more messages follow (half-close). The server may still answer.
    pub fn end(&self) {
        self.body.lock().unwrap_or_else(|e| e.into_inner()).take();
    }

    pub fn is_ended(&self) -> bool {
        self.ended.load(Ordering::SeqCst)
    }

    /// Stop the call (RST_STREAM); an `end` event with CANCELLED follows.
    pub fn cancel(&self) {
        self.cancel.cancel();
    }
}

impl Drop for GrpcSession {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

/// Parsed `grpc://` / `grpcs://` URL.
#[derive(Debug)]
struct Endpoint {
    tls: bool,
    /// Without IPv6 brackets.
    host: String,
    port: u16,
    /// `:authority`: `host:port`, or a `Host` metadata entry.
    authority: String,
    /// URL path without the trailing `/`, put before `/package.Service/Method`.
    prefix: String,
}

fn endpoint(raw: &str) -> Result<Endpoint> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(EngineError::invalid("URL is empty (e.g. grpc://localhost:50051)"));
    }
    let with_scheme = if crate::http::has_scheme(raw) { raw.to_string() } else { format!("grpc://{raw}") };
    let url = Url::parse(&with_scheme).map_err(|e| EngineError::invalid(format!("Invalid URL '{raw}': {e}")))?;
    let tls = match url.scheme() {
        "grpc" | "http" => false,
        "grpcs" | "https" => true,
        other => {
            return Err(EngineError::invalid(format!(
                "Unsupported URL scheme '{other}' (use grpc://, or grpcs:// for TLS)"
            )));
        }
    };
    let host = url
        .host_str()
        .filter(|h| !h.is_empty())
        .ok_or_else(|| EngineError::invalid(format!("URL '{raw}' has no host")))?;
    let port = url.port().unwrap_or(if tls { 443 } else { 80 });
    Ok(Endpoint {
        tls,
        host: host.trim_start_matches('[').trim_end_matches(']').to_string(),
        port,
        authority: format!("{host}:{port}"),
        prefix: url.path().trim_end_matches('/').to_string(),
    })
}

/// Headers the protocol owns; metadata with these names is ignored.
const RESERVED: [&str; 11] = [
    "content-type",
    "te",
    "content-length",
    "grpc-timeout",
    "grpc-encoding",
    "grpc-accept-encoding",
    "connection",
    "keep-alive",
    "proxy-connection",
    "transfer-encoding",
    "upgrade",
];

/// Request headers of a call: the gRPC protocol headers, then the metadata.
/// Returns the header map, the list as sent (for display) and a `Host` override.
fn request_headers(
    metadata: &[Header],
    opts: &RequestOptions,
    timeout: Option<Duration>,
) -> Result<(HeaderMap, Vec<Header>, Option<String>)> {
    let mut list = vec![Header::new("content-type", "application/grpc"), Header::new("te", "trailers")];
    let has = |name: &str| metadata.iter().any(|h| h.name.trim().eq_ignore_ascii_case(name));
    if opts.default_headers && !has("user-agent") {
        list.push(Header::new("user-agent", USER_AGENT));
    }
    list.push(Header::new("grpc-accept-encoding", "gzip"));
    if let Some(timeout) = timeout {
        list.push(Header::new("grpc-timeout", codec::timeout_value(timeout)));
    }
    let mut authority = None;
    for h in metadata {
        let name = h.name.trim().to_ascii_lowercase();
        if name.is_empty() || RESERVED.contains(&name.as_str()) {
            continue;
        }
        if name == "host" || name == ":authority" {
            authority = Some(h.value.trim().to_string()).filter(|a| !a.is_empty());
            continue;
        }
        if name.starts_with(':') {
            return Err(EngineError::invalid(format!("'{}' is a pseudo-header and can't be set", h.name.trim())));
        }
        let value = if name.ends_with("-bin") {
            codec::binary_value(&h.value)
        } else if h.value.bytes().all(|b| (0x20..0x7f).contains(&b)) {
            h.value.clone()
        } else {
            // gRPC text metadata is printable ASCII (other clients refuse the rest too).
            return Err(EngineError::invalid(format!(
                "Metadata '{}' may only contain printable ASCII (no line breaks, tabs or accents); \
                 binary or other text goes in a key ending in -bin",
                h.name.trim()
            )));
        };
        list.push(Header::new(name, value));
    }
    let mut map = HeaderMap::with_capacity(list.len());
    for h in &list {
        let name = HeaderName::from_bytes(h.name.as_bytes())
            .map_err(|_| EngineError::invalid(format!("Invalid metadata key '{}'", h.name)))?;
        let value = HeaderValue::from_bytes(h.value.as_bytes()).map_err(|_| {
            EngineError::invalid(format!("Invalid value for metadata '{}' (line breaks are not allowed)", h.name))
        })?;
        map.append(name, value);
    }
    Ok((map, list, authority))
}

/// How long reflection may take when requests have no timeout.
const REFLECTION_LIMIT: Duration = Duration::from_secs(120);

/// Aborts the connection driver when dropped, so no socket outlives its call.
struct AbortOnDrop(tokio::task::AbortHandle);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// An HTTP/2 connection to a gRPC server.
pub(crate) struct Channel {
    sender: http2::SendRequest<RequestBody>,
    _driver: AbortOnDrop,
    tls: bool,
    authority: String,
    prefix: String,
    remote_addr: String,
    tls_info: Option<TlsInfo>,
    timing: ConnectTiming,
}

impl Channel {
    /// Start a call: the request headers go out now, messages with [`RawCall::send`].
    pub(crate) async fn call(&mut self, path: &str, headers: HeaderMap) -> Result<RawCall> {
        let uri = http::Uri::builder()
            .scheme(if self.tls { "https" } else { "http" })
            .authority(self.authority.as_str())
            .path_and_query(format!("{}{path}", self.prefix))
            .build()
            .map_err(|e| EngineError::invalid(format!("Invalid gRPC target '{}{path}': {e}", self.authority)))?;
        let (tx, rx) = mpsc::unbounded_channel();
        let mut request = http::Request::new(RequestBody(rx));
        *request.method_mut() = http::Method::POST;
        *request.uri_mut() = uri;
        *request.version_mut() = http::Version::HTTP_2;
        *request.headers_mut() = headers;
        self.sender.ready().await.map_err(EngineError::from_hyper)?;
        let sent_at = Instant::now();
        let response: ResponseFuture = Box::pin(self.sender.send_request(request));
        Ok(RawCall {
            body: Some(tx),
            state: CallState::Waiting(response),
            sent_at,
            ttfb: None,
            encoding: None,
            pool: None,
            tls: self.tls,
        })
    }
}

/// Request body fed by a channel, one framed message per item. Dropping the
/// sender ends the body: the client side of the call is closed.
struct RequestBody(mpsc::UnboundedReceiver<Bytes>);

impl hyper::body::Body for RequestBody {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<std::result::Result<Frame<Bytes>, Infallible>>> {
        self.0.poll_recv(cx).map(|m| m.map(|data| Ok(Frame::data(data))))
    }
}

type ResponseFuture = Pin<Box<dyn Future<Output = hyper::Result<http::Response<Incoming>>> + Send>>;

enum CallState {
    Waiting(ResponseFuture),
    Reading {
        body: Incoming,
        deframer: codec::Deframer,
    },
    /// The end is known (trailers arrived, or a non-gRPC answer after its
    /// headers); buffered messages go out first.
    Ending {
        deframer: codec::Deframer,
        status: GrpcStatus,
        trailers: Vec<Header>,
    },
    Done,
}

pub(crate) enum CallEvent {
    Headers(Vec<Header>),
    /// A received message (decompressed).
    Message(Bytes),
    End {
        status: GrpcStatus,
        trailers: Vec<Header>,
    },
}

/// One call on a [`Channel`], as bytes: messages in and out, then the status.
pub(crate) struct RawCall {
    body: Option<mpsc::UnboundedSender<Bytes>>,
    state: CallState,
    sent_at: Instant,
    /// Request sent until the response headers arrived.
    ttfb: Option<Duration>,
    /// `grpc-encoding` of the response.
    encoding: Option<String>,
    /// Decodes `grpc-status-details-bin` messages of known types.
    pool: Option<DescriptorPool>,
    tls: bool,
}

impl RawCall {
    pub(crate) fn send(&self, payload: &[u8]) -> Result<()> {
        let tx = self.body.as_ref().ok_or_else(|| EngineError::invalid("The stream was already ended"))?;
        tx.send(codec::frame(payload)).map_err(|_| EngineError::new(ErrorKind::Io, "The call has ended"))
    }

    /// Half-close: no more messages from this side.
    pub(crate) fn close_send(&mut self) {
        self.body = None;
    }

    /// The next thing the server sent; `None` after the end.
    pub(crate) async fn next(&mut self) -> Option<CallEvent> {
        loop {
            match &mut self.state {
                CallState::Done => return None,
                CallState::Waiting(response) => {
                    let result = response.await;
                    self.ttfb = Some(self.sent_at.elapsed());
                    match result {
                        Ok(response) => return Some(self.head(response).await),
                        Err(e) => return Some(self.finish(transport_status(e, self.tls, true), Vec::new())),
                    }
                }
                CallState::Reading { body, deframer } => {
                    match deframer.next() {
                        Ok(Some((compressed, payload))) => return Some(self.message(compressed, payload)),
                        Ok(None) => {}
                        Err(status) => return Some(self.finish(status, Vec::new())),
                    }
                    match body.frame().await {
                        Some(Ok(frame)) => match frame.into_data() {
                            Ok(data) => deframer.push(&data),
                            Err(frame) => {
                                if let Ok(trailers) = frame.into_trailers() {
                                    let status =
                                        codec::status_from(&trailers, self.pool.as_ref()).unwrap_or_else(|| {
                                            GrpcStatus::new(
                                                INTERNAL,
                                                "The server ended the call without a grpc-status",
                                                true,
                                            )
                                        });
                                    let deframer = std::mem::take(deframer);
                                    let trailers = codec::metadata(&trailers);
                                    self.state = CallState::Ending { deframer, status, trailers };
                                }
                            }
                        },
                        Some(Err(e)) => return Some(self.finish(transport_status(e, self.tls, false), Vec::new())),
                        None => {
                            let message = if deframer.has_partial() {
                                "The response ended in the middle of a message"
                            } else {
                                "The server closed the stream without a status (no grpc-status trailer)"
                            };
                            return Some(self.finish(GrpcStatus::new(INTERNAL, message, true), Vec::new()));
                        }
                    }
                }
                CallState::Ending { deframer, .. } => match deframer.next() {
                    Ok(Some((compressed, payload))) => return Some(self.message(compressed, payload)),
                    Err(status) => return Some(self.finish(status, Vec::new())),
                    Ok(None) => {
                        let partial = deframer.has_partial();
                        let CallState::Ending { status, trailers, .. } =
                            std::mem::replace(&mut self.state, CallState::Done)
                        else {
                            return None;
                        };
                        let status = if partial && status.is_ok() {
                            GrpcStatus::new(INTERNAL, "The response ended in the middle of a message", true)
                        } else {
                            status
                        };
                        return Some(CallEvent::End { status, trailers });
                    }
                },
            }
        }
    }

    /// Check the response head. Errors and empty answers come "trailers-only"
    /// (the status in the headers); anything that isn't gRPC ends the call with
    /// a status derived from the HTTP response, after its headers.
    async fn head(&mut self, response: http::Response<Incoming>) -> CallEvent {
        let (parts, body) = response.into_parts();
        if let Some(status) = codec::status_from(&parts.headers, self.pool.as_ref()) {
            return self.finish(status, codec::metadata(&parts.headers));
        }
        let headers = codec::metadata(&parts.headers);
        let content_type =
            parts.headers.get("content-type").map(|v| String::from_utf8_lossy(v.as_bytes()).into_owned());
        let failure = if parts.status != http::StatusCode::OK {
            let snippet = body_snippet(body).await;
            let reason = parts.status.canonical_reason().unwrap_or_default();
            let mut message = format!("HTTP {} {reason} from the server (not a gRPC answer)", parts.status.as_u16());
            if !snippet.is_empty() {
                message.push_str(&format!(": {snippet}"));
            }
            Some(GrpcStatus::new(codec::code_for_http_status(parts.status.as_u16()), message, true))
        } else if !codec::is_grpc_content_type(content_type.as_deref().unwrap_or_default()) {
            let shown = content_type.unwrap_or_else(|| "none".into());
            Some(GrpcStatus::new(UNKNOWN, format!("The response is not gRPC (Content-Type: {shown})"), true))
        } else {
            self.encoding = parts.headers.get("grpc-encoding").map(|v| String::from_utf8_lossy(v.as_bytes()).into());
            self.state = CallState::Reading { body, deframer: codec::Deframer::default() };
            None
        };
        if let Some(status) = failure {
            self.state = CallState::Ending { deframer: codec::Deframer::default(), status, trailers: Vec::new() };
        }
        CallEvent::Headers(headers)
    }

    fn message(&mut self, compressed: bool, payload: Bytes) -> CallEvent {
        if !compressed {
            return CallEvent::Message(payload);
        }
        match codec::decompress(&payload, self.encoding.as_deref()) {
            Ok(data) => CallEvent::Message(data),
            Err(e) => self.finish(GrpcStatus::new(INTERNAL, e, true), Vec::new()),
        }
    }

    fn finish(&mut self, status: GrpcStatus, trailers: Vec<Header>) -> CallEvent {
        self.state = CallState::Done;
        CallEvent::End { status, trailers }
    }
}

/// The start of a non-gRPC response body, for the error message.
async fn body_snippet(mut body: Incoming) -> String {
    let mut data = Vec::new();
    let read = async {
        while let Some(Ok(frame)) = body.frame().await {
            if let Ok(chunk) = frame.into_data() {
                data.extend_from_slice(&chunk);
                if data.len() >= 1024 {
                    break;
                }
            }
        }
    };
    let _ = tokio::time::timeout(Duration::from_secs(2), read).await;
    let text = String::from_utf8_lossy(&data).split_whitespace().collect::<Vec<_>>().join(" ");
    text.chars().take(300).collect()
}

/// A connection or stream failure as a status (UNAVAILABLE, like gRPC libraries).
fn transport_status(err: hyper::Error, tls: bool, before_headers: bool) -> GrpcStatus {
    let mut message = format!("Connection error: {}", EngineError::from_hyper(err).message);
    // A TLS server answers plaintext HTTP/2 with garbage, which fails right away.
    if !tls && before_headers {
        message.push_str(". grpc:// is plaintext HTTP/2; if the server uses TLS, use grpcs://");
    }
    GrpcStatus::new(UNAVAILABLE, message, true)
}

async fn with_timeout<T>(limit: Option<Duration>, fut: impl Future<Output = Result<T>>) -> Result<T> {
    match limit {
        Some(limit) => tokio::time::timeout(limit, fut).await.map_err(|_| EngineError::timeout("Request", limit))?,
        None => fut.await,
    }
}

fn call_timing(conn: ConnectTiming, call: &RawCall, started: Instant) -> Timing {
    let ttfb = call.ttfb.unwrap_or_default();
    Timing {
        dns_ms: ms(conn.dns),
        connect_ms: ms(conn.connect),
        tls_ms: ms(conn.tls),
        ttfb_ms: ms(ttfb),
        download_ms: ms(call.sent_at.elapsed().saturating_sub(ttfb)),
        total_ms: ms(started.elapsed()),
        ..Default::default()
    }
}

impl Client {
    /// Connect for gRPC: HTTP/2 over TLS (ALPN `h2`) or plaintext with prior knowledge.
    async fn grpc_channel(&self, endpoint: &Endpoint, opts: &RequestOptions) -> Result<Channel> {
        if let Some(guard) = &opts.host_guard {
            guard.check_host(&endpoint.host)?;
        }
        // Proxies can't forward plaintext HTTP/2, so grpc:// also uses a CONNECT tunnel (like WebSocket).
        let proxy = opts.proxy.for_target(&endpoint.host, true);
        let conn = net::connect(
            &Target {
                host: &endpoint.host,
                port: endpoint.port,
                tls: endpoint.tls,
                alpn: Alpn::Http2,
                tls_options: &opts.tls,
                proxy,
                force_tunnel: true,
                connect_timeout: opts.connect_timeout,
            },
            &self.tls,
        )
        .await
        .map_err(|mut e| {
            if e.kind == ErrorKind::Tls && (e.message.contains("InvalidContentType") || e.message.contains("corrupt")) {
                e.message.push_str(" The server may not use TLS: try grpc:// instead of grpcs://.");
            }
            e
        })?;
        let (sender, connection) = http2::Builder::new(TokioExecutor::new())
            .adaptive_window(true)
            .max_header_list_size(400 * 1024)
            .handshake(TokioIo::new(conn.stream))
            .await
            .map_err(EngineError::from_hyper)?;
        let driver = tokio::spawn(async move {
            let _ = connection.await;
        });
        Ok(Channel {
            sender,
            _driver: AbortOnDrop(driver.abort_handle()),
            tls: endpoint.tls,
            authority: endpoint.authority.clone(),
            prefix: endpoint.prefix.clone(),
            remote_addr: conn.remote_addr.to_string(),
            tls_info: conn.tls,
            timing: conn.timing,
        })
    }

    /// Load service definitions through server reflection (v1, then v1alpha).
    /// The metadata is sent too (servers may require auth for reflection).
    /// Limited by the request timeout, or [`REFLECTION_LIMIT`] without one (it
    /// can't be cancelled, so a silent server must not keep it open forever).
    pub async fn grpc_reflect(&self, target: &GrpcTarget, opts: &RequestOptions) -> Result<GrpcDescriptors> {
        let mut endpoint = endpoint(&target.url)?;
        let (headers, _, authority) = request_headers(&target.metadata, opts, None)?;
        if let Some(authority) = authority {
            endpoint.authority = authority;
        }
        with_timeout(Some(opts.timeout.unwrap_or(REFLECTION_LIMIT)), async {
            let mut channel = self.grpc_channel(&endpoint, opts).await?;
            reflection::load(&mut channel, &headers).await
        })
        .await
    }

    /// A unary call: one JSON message out, the answer with status, metadata and
    /// timing back. `opts.timeout` is the deadline. Connection failures are
    /// errors; everything after the request went out is a status.
    pub async fn grpc_unary(
        &self,
        target: &GrpcTarget,
        method: &MethodDescriptor,
        message: &str,
        opts: &RequestOptions,
    ) -> Result<GrpcResponse> {
        let started = Instant::now();
        let deadline = opts.timeout.map(|t| tokio::time::Instant::now() + t);
        // Checked before connecting, so mistakes fail without network traffic.
        let (payload, _) = schema::encode_json(&method.input(), message)?;
        let mut endpoint = endpoint(&target.url)?;
        let (_, _, authority) = request_headers(&target.metadata, opts, opts.timeout)?;
        if let Some(authority) = authority {
            endpoint.authority = authority;
        }
        let mut channel = with_timeout(opts.timeout, self.grpc_channel(&endpoint, opts)).await?;
        let remaining = deadline.map(|d| d.saturating_duration_since(tokio::time::Instant::now()));
        let (headers, request_headers, _) = request_headers(&target.metadata, opts, remaining)?;
        let mut call = channel.call(&format!("/{}", method_path(method)), headers).await?;
        call.pool = Some(method.parent_pool().clone());
        call.send(&payload)?;
        call.close_send();

        let output = method.output();
        let mut response = GrpcResponse {
            status: GrpcStatus::new(0, "", false),
            messages: Vec::new(),
            headers: Vec::new(),
            trailers: Vec::new(),
            timing: Timing::default(),
            remote_addr: Some(channel.remote_addr.clone()),
            tls: channel.tls_info.clone(),
            request_headers,
            warnings: Vec::new(),
        };
        response.status = loop {
            let next = match deadline {
                Some(deadline) => match tokio::time::timeout_at(deadline, call.next()).await {
                    Ok(next) => next,
                    Err(_) => {
                        let limit = human_duration(opts.timeout.unwrap_or_default());
                        break GrpcStatus::new(DEADLINE_EXCEEDED, format!("No answer within {limit}"), true);
                    }
                },
                None => call.next().await,
            };
            match next {
                Some(CallEvent::Headers(headers)) => response.headers = headers,
                Some(CallEvent::Message(bytes)) => match schema::message_json(&output, &bytes) {
                    Ok(json) => response.messages.push(GrpcMessage {
                        direction: Direction::Received,
                        json,
                        size: bytes.len() as u64,
                        timestamp: now_ms(),
                    }),
                    Err(e) => response.warnings.push(undecodable(&output, &e)),
                },
                Some(CallEvent::End { status, trailers }) => {
                    response.trailers = trailers;
                    break status;
                }
                None => break GrpcStatus::new(INTERNAL, "The call ended without a status", true),
            }
        };
        response.timing = call_timing(channel.timing, &call, started);
        Ok(response)
    }

    /// Open a call for streaming (any kind of method). Send messages with
    /// [`GrpcSession::send`], half-close with [`GrpcSession::end`]; everything
    /// that happens arrives as [`GrpcEvent`]s, the last one being `End`.
    pub async fn grpc_stream(
        &self,
        target: &GrpcTarget,
        method: &MethodDescriptor,
        opts: &RequestOptions,
    ) -> Result<GrpcStream> {
        let started = Instant::now();
        let mut endpoint = endpoint(&target.url)?;
        let (headers, request_headers, authority) = request_headers(&target.metadata, opts, None)?;
        if let Some(authority) = authority {
            endpoint.authority = authority;
        }
        let mut channel = with_timeout(opts.timeout, self.grpc_channel(&endpoint, opts)).await?;
        let mut call = channel.call(&format!("/{}", method_path(method)), headers).await?;
        call.pool = Some(method.parent_pool().clone());

        let (events_tx, events) = mpsc::unbounded_channel();
        let cancel = CancellationToken::new();
        let ended = Arc::new(AtomicBool::new(false));
        // The session holds the only body sender: dropping it half-closes the call.
        let session = GrpcSession {
            input: method.input(),
            body: Mutex::new(call.body.take()),
            events: events_tx.clone(),
            ended: ended.clone(),
            cancel: cancel.clone(),
        };
        let opened = GrpcOpened {
            remote_addr: Some(channel.remote_addr.clone()),
            tls: channel.tls_info.clone(),
            timing: Timing {
                dns_ms: ms(channel.timing.dns),
                connect_ms: ms(channel.timing.connect),
                tls_ms: ms(channel.timing.tls),
                total_ms: ms(started.elapsed()),
                ..Default::default()
            },
            request_headers,
        };
        tokio::spawn(run_stream(channel, call, method.output(), events_tx, cancel, ended, started));
        Ok(GrpcStream { opened, session, events })
    }
}

fn undecodable(desc: &MessageDescriptor, err: &str) -> String {
    format!("A received message could not be decoded as {}: {err}", desc.full_name())
}

async fn run_stream(
    channel: Channel,
    mut call: RawCall,
    output: MessageDescriptor,
    events: mpsc::UnboundedSender<GrpcEvent>,
    cancel: CancellationToken,
    ended: Arc<AtomicBool>,
    started: Instant,
) {
    let (status, trailers) = loop {
        tokio::select! {
            _ = cancel.cancelled() => break (GrpcStatus::new(CANCELLED, "Cancelled", true), Vec::new()),
            next = call.next() => match next {
                Some(CallEvent::Headers(headers)) => {
                    let _ = events.send(GrpcEvent::Headers { headers, timestamp: now_ms() });
                }
                Some(CallEvent::Message(bytes)) => {
                    let event = match schema::message_json(&output, &bytes) {
                        Ok(json) => GrpcEvent::Message {
                            message: GrpcMessage {
                                direction: Direction::Received,
                                json,
                                size: bytes.len() as u64,
                                timestamp: now_ms(),
                            },
                        },
                        Err(e) => GrpcEvent::Error { message: undecodable(&output, &e) },
                    };
                    let _ = events.send(event);
                }
                Some(CallEvent::End { status, trailers }) => break (status, trailers),
                None => break (GrpcStatus::new(INTERNAL, "The call ended without a status", true), Vec::new()),
            }
        }
    };
    ended.store(true, Ordering::SeqCst);
    let timing = call_timing(channel.timing, &call, started);
    // Dropping the call resets the stream if it is still open; the channel closes the connection.
    drop(call);
    drop(channel);
    let _ = events.send(GrpcEvent::End { status, trailers, timing });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grpc_urls() {
        let e = endpoint("localhost:50051").unwrap();
        assert_eq!(
            (e.tls, e.host.as_str(), e.port, e.authority.as_str()),
            (false, "localhost", 50051, "localhost:50051")
        );
        let e = endpoint("grpcs://api.test").unwrap();
        assert_eq!((e.tls, e.port, e.prefix.as_str()), (true, 443, ""));
        let e = endpoint("grpc://[::1]:9000/prefix/").unwrap();
        assert_eq!((e.host.as_str(), e.authority.as_str(), e.prefix.as_str()), ("::1", "[::1]:9000", "/prefix"));
        assert!(endpoint("https://h:1").unwrap().tls);
        assert!(endpoint("ws://h:1").unwrap_err().message.contains("grpcs://"));
        assert!(endpoint(" ").is_err());
    }

    #[test]
    fn metadata_headers() {
        let opts = RequestOptions::default();
        let metadata = vec![
            Header::new("X-Api-Key", "k"),
            Header::new("trace-bin", "hello"),
            Header::new("content-type", "text/plain"),
            Header::new("Host", "internal.test"),
            Header::new("", "ignored"),
        ];
        let (map, list, authority) = request_headers(&metadata, &opts, Some(Duration::from_secs(2))).unwrap();
        assert_eq!(map.get("content-type").unwrap(), "application/grpc");
        assert_eq!(map.get_all("content-type").iter().count(), 1);
        assert_eq!(map.get("te").unwrap(), "trailers");
        assert_eq!(map.get("grpc-timeout").unwrap(), "2000m");
        assert_eq!(map.get("x-api-key").unwrap(), "k");
        assert_eq!(map.get("trace-bin").unwrap(), "aGVsbG8");
        assert!(map.get("user-agent").unwrap().to_str().unwrap().starts_with("Zorvik/"));
        assert_eq!(authority.as_deref(), Some("internal.test"));
        assert!(list.iter().any(|h| h.name == "x-api-key"));
        assert!(request_headers(&[Header::new(":path", "/x")], &opts, None).is_err());
        assert!(request_headers(&[Header::new("bad key", "v")], &opts, None).is_err());
        assert!(request_headers(&[Header::new("k", "a\nb")], &opts, None).is_err());
        // Text metadata is printable ASCII; -bin keys take anything.
        let err = request_headers(&[Header::new("name", "Zoë")], &opts, None).unwrap_err();
        assert!(err.message.contains("-bin"), "{}", err.message);
        assert!(request_headers(&[Header::new("k", "a\tb")], &opts, None).is_err());
        let (map, _, _) = request_headers(&[Header::new("name-bin", "Zoë")], &opts, None).unwrap();
        assert_eq!(map.get("name-bin").unwrap(), "Wm/Dqw");
        // An empty Host keeps the URL's authority.
        let (_, _, authority) = request_headers(&[Header::new("Host", " ")], &opts, None).unwrap();
        assert_eq!(authority, None);
    }
}

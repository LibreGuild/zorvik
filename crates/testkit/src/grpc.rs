//! gRPC test server on hyper's HTTP/2 server with dynamic messages
//! (`prost-reflect`): the `zorvik.test.v1.Echo` service from
//! `proto/zorvik/test/v1/echo.proto` and server reflection (v1 and v1alpha).
//!
//! * `Unary`, `ServerStream`, `ClientStream`, `Bidi`: echo in every call style.
//! * `Fail`: a trailers-only error with `grpc-message` and `grpc-status-details-bin`.
//! * `Metadata`: returns the request metadata, sends a header and a trailer.
//! * Metadata `x-compress: gzip` makes the answers gzip-compressed.
//!
//! Reflection answers each file on its own (dependencies are asked for by name).

use std::collections::HashMap;
use std::convert::Infallible;
use std::io::Write as _;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use bytes::{Buf, BufMut, Bytes, BytesMut};
use http::{HeaderMap, HeaderValue, Request, Response};
use http_body_util::BodyExt;
use hyper::body::{Frame, Incoming};
use hyper_util::rt::{TokioExecutor, TokioIo};
use prost::Message as _;
use prost_reflect::{DescriptorPool, DynamicMessage, MapKey, Value};
use tokio::net::TcpListener;
use tokio::sync::mpsc;

use crate::TestCerts;

/// Folder with the test service's `.proto` files (import path for proto-file tests).
pub fn grpc_proto_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("proto")
}

/// The service's file, relative to [`grpc_proto_dir`].
pub const ECHO_PROTO: &str = "zorvik/test/v1/echo.proto";
const COMMON_PROTO: &str = "zorvik/test/v1/common.proto";
const ECHO_SOURCE: &str = include_str!("../proto/zorvik/test/v1/echo.proto");
const COMMON_SOURCE: &str = include_str!("../proto/zorvik/test/v1/common.proto");

/// Which reflection services the server offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Reflection {
    #[default]
    Both,
    /// Only the older `v1alpha` (clients must fall back).
    V1AlphaOnly,
    Off,
}

/// A running gRPC test server. Dropping it stops accepting connections.
pub struct GrpcTestServer {
    pub addr: SocketAddr,
    pub tls: bool,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for GrpcTestServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

struct State {
    pool: DescriptorPool,
    reflection: Reflection,
}

impl GrpcTestServer {
    /// Plaintext HTTP/2 (h2c) on 127.0.0.1, random port, both reflection versions.
    pub async fn start() -> Self {
        Self::bind("127.0.0.1:0".parse().unwrap(), None, Reflection::Both).await
    }

    pub async fn bind(addr: SocketAddr, certs: Option<&TestCerts>, reflection: Reflection) -> Self {
        let listener = TcpListener::bind(addr).await.expect("bind grpc test server");
        let addr = listener.local_addr().unwrap();
        let state = Arc::new(State { pool: echo_pool(), reflection });
        let acceptor = certs.map(|c| tokio_rustls::TlsAcceptor::from(Arc::new(c.server_config())));
        let tls = acceptor.is_some();
        let task = tokio::spawn(async move {
            loop {
                let Ok((tcp, _)) = listener.accept().await else { continue };
                let state = state.clone();
                let acceptor = acceptor.clone();
                tokio::spawn(async move {
                    let service = hyper::service::service_fn(move |req| {
                        let state = state.clone();
                        async move { Ok::<_, Infallible>(handle(req, state).await) }
                    });
                    let builder = hyper::server::conn::http2::Builder::new(TokioExecutor::new());
                    match acceptor {
                        Some(acceptor) => {
                            if let Ok(stream) = acceptor.accept(tcp).await {
                                let _ = builder.serve_connection(TokioIo::new(stream), service).await;
                            }
                        }
                        None => {
                            let _ = builder.serve_connection(TokioIo::new(tcp), service).await;
                        }
                    }
                });
            }
        });
        GrpcTestServer { addr, tls, task }
    }

    /// `grpc://127.0.0.1:PORT` (or `grpcs://`).
    pub fn url(&self) -> String {
        format!("{}://{}", if self.tls { "grpcs" } else { "grpc" }, self.addr)
    }
}

/// The compiled test service (from the embedded sources; well-known types built in).
pub fn echo_pool() -> DescriptorPool {
    use protox::file::{ChainFileResolver, File, FileResolver, GoogleFileResolver};
    struct Embedded;
    impl FileResolver for Embedded {
        fn open_file(&self, name: &str) -> Result<File, protox::Error> {
            match name {
                ECHO_PROTO => File::from_source(name, ECHO_SOURCE),
                COMMON_PROTO => File::from_source(name, COMMON_SOURCE),
                _ => Err(protox::Error::file_not_found(name)),
            }
        }
    }
    let mut resolver = ChainFileResolver::new();
    resolver.add(Embedded);
    resolver.add(GoogleFileResolver::new());
    let mut compiler = protox::Compiler::with_file_resolver(resolver);
    compiler.include_imports(true);
    compiler.open_file(ECHO_PROTO).expect("test proto compiles");
    compiler.descriptor_pool()
}

/// Response body fed by a channel (messages, then trailers). `None`: empty,
/// so a trailers-only answer ends with its headers.
struct ReplyBody(Option<mpsc::UnboundedReceiver<Frame<Bytes>>>);

impl hyper::body::Body for ReplyBody {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Result<Frame<Bytes>, Infallible>>> {
        match self.0.as_mut() {
            Some(rx) => rx.poll_recv(cx).map(|f| f.map(Ok)),
            None => Poll::Ready(None),
        }
    }

    fn is_end_stream(&self) -> bool {
        self.0.is_none()
    }
}

/// Writes answers of one call.
struct Reply {
    tx: mpsc::UnboundedSender<Frame<Bytes>>,
    gzip: bool,
}

impl Reply {
    fn message(&self, msg: &DynamicMessage) {
        let payload = msg.encode_to_vec();
        let (flag, payload) = if self.gzip {
            let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            gz.write_all(&payload).unwrap();
            (1, gz.finish().unwrap())
        } else {
            (0, payload)
        };
        let mut out = BytesMut::with_capacity(5 + payload.len());
        out.put_u8(flag);
        out.put_u32(payload.len() as u32);
        out.extend_from_slice(&payload);
        let _ = self.tx.send(Frame::data(out.freeze()));
    }

    fn finish(self, code: u32, message: &str, extra: &[(&'static str, &str)]) {
        let _ = self.tx.send(Frame::trailers(status_headers(code, message, extra)));
    }
}

fn status_headers(code: u32, message: &str, extra: &[(&'static str, &str)]) -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert("grpc-status", HeaderValue::from(code));
    if !message.is_empty() {
        let encoded = percent_encode(message);
        h.insert("grpc-message", HeaderValue::from_str(&encoded).unwrap());
    }
    for (name, value) in extra {
        h.insert(*name, HeaderValue::from_str(value).unwrap());
    }
    h
}

/// gRPC's percent-encoding: printable ASCII except `%` stays as is.
fn percent_encode(s: &str) -> String {
    s.bytes()
        .map(|b| if (0x20..0x7f).contains(&b) && b != b'%' { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect()
}

/// A trailers-only answer: the status in the headers, no body.
fn trailers_only(code: u32, message: &str, extra: &[(&'static str, &str)]) -> Response<ReplyBody> {
    let mut response = Response::new(ReplyBody(None));
    *response.headers_mut() = status_headers(code, message, extra);
    response.headers_mut().insert("content-type", HeaderValue::from_static("application/grpc"));
    response
}

async fn handle(req: Request<Incoming>, state: Arc<State>) -> Response<ReplyBody> {
    let content_type = req.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or_default();
    if req.method() != http::Method::POST || !content_type.starts_with("application/grpc") {
        let mut response = Response::new(ReplyBody(None));
        *response.status_mut() = http::StatusCode::UNSUPPORTED_MEDIA_TYPE;
        return response;
    }
    let path = req.uri().path().to_string();
    let reflection_version = match path.as_str() {
        "/grpc.reflection.v1.ServerReflection/ServerReflectionInfo" => Some("v1"),
        "/grpc.reflection.v1alpha.ServerReflection/ServerReflectionInfo" => Some("v1alpha"),
        _ => None,
    };
    let offered = match (reflection_version, state.reflection) {
        (Some(_), Reflection::Off) | (Some("v1"), Reflection::V1AlphaOnly) => false,
        (Some(_), _) => true,
        (None, _) => path.starts_with("/zorvik.test.v1.Echo/"),
    };
    let method = path.rsplit('/').next().unwrap_or_default().to_string();
    let known = reflection_version.is_some()
        || ["Unary", "ServerStream", "ClientStream", "Bidi", "Fail", "Metadata"].contains(&method.as_str());
    if !offered || !known {
        return trailers_only(12, &format!("unknown method {path}"), &[]);
    }
    let headers = req.headers().clone();
    let requests = read_messages(req.into_body());

    if method == "Fail" {
        let mut requests = requests;
        let fail = state.pool.get_message_by_name("zorvik.test.v1.FailRequest").unwrap();
        let (code, message, raw) = match requests.recv().await {
            Some(bytes) => {
                let msg =
                    DynamicMessage::decode(fail.clone(), bytes.clone()).unwrap_or_else(|_| DynamicMessage::new(fail));
                let code = msg.get_field_by_name("code").and_then(|v| v.as_i32()).filter(|c| *c > 0).unwrap_or(5);
                let text =
                    msg.get_field_by_name("message").and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
                (code as u32, text, bytes)
            }
            None => (5, String::new(), Bytes::new()),
        };
        let details = rpc_status(code, &message, "type.googleapis.com/zorvik.test.v1.FailRequest", &raw);
        return trailers_only(code, &message, &[("grpc-status-details-bin", &details)]);
    }

    let gzip = headers.get("x-compress").is_some_and(|v| v == "gzip");
    let (tx, rx) = mpsc::unbounded_channel();
    let reply = Reply { tx, gzip };
    let mut response = Response::new(ReplyBody(Some(rx)));
    response.headers_mut().insert("content-type", HeaderValue::from_static("application/grpc"));
    if gzip {
        response.headers_mut().insert("grpc-encoding", HeaderValue::from_static("gzip"));
    }
    if method == "Metadata" {
        response.headers_mut().insert("x-echo-header", HeaderValue::from_static("header-value"));
    }
    tokio::spawn(async move {
        match reflection_version {
            Some(version) => reflect(&state, version, requests, reply).await,
            None => echo(&state, &method, &headers, requests, reply).await,
        }
    });
    response
}

/// Deframe the request body into messages (a task, so answers can flow meanwhile).
fn read_messages(mut body: Incoming) -> mpsc::UnboundedReceiver<Bytes> {
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        let mut buf = BytesMut::new();
        while let Some(Ok(frame)) = body.frame().await {
            let Ok(data) = frame.into_data() else { continue };
            buf.extend_from_slice(&data);
            while buf.len() >= 5 {
                let len = u32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]) as usize;
                if buf.len() < 5 + len {
                    break;
                }
                buf.advance(5);
                if tx.send(buf.split_to(len).freeze()).is_err() {
                    return;
                }
            }
        }
    });
    rx
}

fn text(msg: &DynamicMessage, field: &str) -> String {
    msg.get_field_by_name(field).and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default()
}

async fn echo(
    state: &State,
    method: &str,
    headers: &HeaderMap,
    mut requests: mpsc::UnboundedReceiver<Bytes>,
    reply: Reply,
) {
    let req_desc = state.pool.get_message_by_name("zorvik.test.v1.EchoRequest").unwrap();
    let reply_desc = state.pool.get_message_by_name("zorvik.test.v1.EchoReply").unwrap();
    let decode = |bytes: Bytes| DynamicMessage::decode(req_desc.clone(), bytes);
    let answer = |message: String, index: i32, request: Option<&DynamicMessage>| {
        let mut out = DynamicMessage::new(reply_desc.clone());
        out.set_field_by_name("message", Value::String(message));
        out.set_field_by_name("index", Value::I32(index));
        if let Some(request) = request {
            out.set_field_by_name("request", Value::Message(request.clone()));
        }
        out
    };
    match method {
        "Unary" | "ServerStream" => {
            let Some(Ok(req)) = requests.recv().await.map(decode) else {
                return reply.finish(3, "expected one EchoRequest", &[]);
            };
            let delay =
                Duration::from_millis(req.get_field_by_name("delay_ms").and_then(|v| v.as_i64()).unwrap_or(0) as u64);
            let message = text(&req, "message");
            if method == "Unary" {
                tokio::time::sleep(delay).await;
                reply.message(&answer(message, 0, Some(&req)));
            } else {
                let count = req.get_field_by_name("count").and_then(|v| v.as_i32()).filter(|c| *c > 0).unwrap_or(3);
                for i in 0..count {
                    if i > 0 {
                        tokio::time::sleep(delay).await;
                    }
                    reply.message(&answer(format!("{message} #{}", i + 1), i, None));
                }
            }
            reply.finish(0, "", &[]);
        }
        "ClientStream" => {
            let mut messages = Vec::new();
            while let Some(bytes) = requests.recv().await {
                match decode(bytes) {
                    Ok(req) => messages.push(text(&req, "message")),
                    Err(e) => return reply.finish(3, &format!("bad message: {e}"), &[]),
                }
            }
            let mut out = answer(messages.join(","), messages.len() as i32, None);
            out.set_field_by_name("messages", Value::List(messages.into_iter().map(Value::String).collect()));
            reply.message(&out);
            reply.finish(0, "", &[]);
        }
        "Bidi" => {
            let mut index = 0;
            while let Some(bytes) = requests.recv().await {
                match decode(bytes) {
                    Ok(req) => reply.message(&answer(text(&req, "message"), index, None)),
                    Err(e) => return reply.finish(3, &format!("bad message: {e}"), &[]),
                }
                index += 1;
            }
            reply.finish(0, "", &[]);
        }
        "Metadata" => {
            let _ = requests.recv().await;
            let desc = state.pool.get_message_by_name("zorvik.test.v1.MetadataReply").unwrap();
            let mut entries = HashMap::new();
            for (name, value) in headers {
                let value = String::from_utf8_lossy(value.as_bytes()).into_owned();
                entries.insert(MapKey::String(name.as_str().to_string()), Value::String(value));
            }
            let mut out = DynamicMessage::new(desc);
            out.set_field_by_name("metadata", Value::Map(entries));
            reply.message(&out);
            reply.finish(0, "", &[("x-echo-trailer", "trailer-value")]);
        }
        _ => reply.finish(12, "unknown method", &[]),
    }
}

/// `google.rpc.Status` with one detail, base64 for `grpc-status-details-bin`.
fn rpc_status(code: u32, message: &str, type_url: &str, detail: &[u8]) -> String {
    use base64::Engine as _;
    let status = RpcStatus {
        code: code as i32,
        message: message.to_string(),
        details: vec![RpcAny { type_url: type_url.to_string(), value: detail.to_vec() }],
    };
    base64::engine::general_purpose::STANDARD_NO_PAD.encode(status.encode_to_vec())
}

#[derive(Clone, PartialEq, prost::Message)]
struct RpcStatus {
    #[prost(int32, tag = "1")]
    code: i32,
    #[prost(string, tag = "2")]
    message: String,
    #[prost(message, repeated, tag = "3")]
    details: Vec<RpcAny>,
}

#[derive(Clone, PartialEq, prost::Message)]
struct RpcAny {
    #[prost(string, tag = "1")]
    type_url: String,
    #[prost(bytes = "vec", tag = "2")]
    value: Vec<u8>,
}

// ---- server reflection ----------------------------------------------------------

#[derive(Clone, PartialEq, prost::Message)]
struct ReflectionRequest {
    #[prost(string, tag = "1")]
    host: String,
    #[prost(oneof = "Ask", tags = "3, 4, 7")]
    message_request: Option<Ask>,
}

#[derive(Clone, PartialEq, prost::Oneof)]
enum Ask {
    #[prost(string, tag = "3")]
    FileByFilename(String),
    #[prost(string, tag = "4")]
    FileContainingSymbol(String),
    #[prost(string, tag = "7")]
    ListServices(String),
}

#[derive(Clone, PartialEq, prost::Message)]
struct ReflectionResponse {
    #[prost(string, tag = "1")]
    valid_host: String,
    #[prost(message, optional, tag = "2")]
    original_request: Option<ReflectionRequest>,
    #[prost(oneof = "Answer", tags = "4, 6, 7")]
    message_response: Option<Answer>,
}

#[derive(Clone, PartialEq, prost::Oneof)]
enum Answer {
    #[prost(message, tag = "4")]
    Files(FileDescriptorResponse),
    #[prost(message, tag = "6")]
    Services(ListServiceResponse),
    #[prost(message, tag = "7")]
    Error(ErrorResponse),
}

#[derive(Clone, PartialEq, prost::Message)]
struct FileDescriptorResponse {
    #[prost(bytes = "vec", repeated, tag = "1")]
    file_descriptor_proto: Vec<Vec<u8>>,
}

#[derive(Clone, PartialEq, prost::Message)]
struct ListServiceResponse {
    #[prost(message, repeated, tag = "1")]
    service: Vec<ServiceResponse>,
}

#[derive(Clone, PartialEq, prost::Message)]
struct ServiceResponse {
    #[prost(string, tag = "1")]
    name: String,
}

#[derive(Clone, PartialEq, prost::Message)]
struct ErrorResponse {
    #[prost(int32, tag = "1")]
    error_code: i32,
    #[prost(string, tag = "2")]
    error_message: String,
}

async fn reflect(state: &State, version: &str, mut requests: mpsc::UnboundedReceiver<Bytes>, reply: Reply) {
    while let Some(bytes) = requests.recv().await {
        let Ok(request) = ReflectionRequest::decode(bytes) else {
            return reply.finish(3, "invalid reflection request", &[]);
        };
        let answer = match request.message_request.clone() {
            Some(Ask::ListServices(_)) => Answer::Services(ListServiceResponse {
                service: ["zorvik.test.v1.Echo".to_string(), format!("grpc.reflection.{version}.ServerReflection")]
                    .into_iter()
                    .map(|name| ServiceResponse { name })
                    .collect(),
            }),
            Some(Ask::FileContainingSymbol(symbol)) => file_answer(state, symbol_file(state, &symbol), &symbol),
            Some(Ask::FileByFilename(name)) => {
                file_answer(state, state.pool.get_file_by_name(&name).map(|f| f.name().to_string()), &name)
            }
            None => Answer::Error(ErrorResponse { error_code: 3, error_message: "empty request".into() }),
        };
        let response = ReflectionResponse {
            valid_host: request.host.clone(),
            original_request: Some(request),
            message_response: Some(answer),
        };
        send_raw(&reply, &response.encode_to_vec());
    }
    reply.finish(0, "", &[]);
}

fn send_raw(reply: &Reply, payload: &[u8]) {
    let mut out = BytesMut::with_capacity(5 + payload.len());
    out.put_u8(0);
    out.put_u32(payload.len() as u32);
    out.extend_from_slice(payload);
    let _ = reply.tx.send(Frame::data(out.freeze()));
}

/// The file defining a service, method, message or enum.
fn symbol_file(state: &State, symbol: &str) -> Option<String> {
    let pool = &state.pool;
    let parent = symbol.rsplit_once('.').map(|(p, _)| p).unwrap_or_default();
    pool.get_service_by_name(symbol)
        .map(|s| s.parent_file())
        .or_else(|| pool.get_message_by_name(symbol).map(|m| m.parent_file()))
        .or_else(|| pool.get_enum_by_name(symbol).map(|e| e.parent_file()))
        .or_else(|| pool.get_service_by_name(parent).map(|s| s.parent_file()))
        .map(|f| f.name().to_string())
}

fn file_answer(state: &State, file: Option<String>, asked: &str) -> Answer {
    match file.and_then(|name| state.pool.get_file_by_name(&name)) {
        Some(file) => Answer::Files(FileDescriptorResponse {
            file_descriptor_proto: vec![file.file_descriptor_proto().encode_to_vec()],
        }),
        None => Answer::Error(ErrorResponse { error_code: 5, error_message: format!("{asked} not found") }),
    }
}

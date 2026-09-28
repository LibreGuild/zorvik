//! Local servers for automated tests and manual QA.
//!
//! * [`TestServer`]: HTTP/1.1 + HTTP/2 (h2c or TLS+ALPN) with echo, redirect,
//!   compression, cookies, auth (Digest and NTLM in `auth.rs`), SSE, WebSocket, OAuth2 and
//!   GraphQL (`graphql.rs`) endpoints.
//! * [`TestProxy`]: minimal forward proxy (CONNECT + absolute-form) with optional Basic auth.
//! * [`H3TestServer`]: HTTP/3 over QUIC (see `h3.rs`).
//! * [`GrpcTestServer`]: gRPC echo service with server reflection (see `grpc.rs`).
//!
//! Endpoints are listed in `docs/testing.md`.

use std::collections::{HashMap, HashSet};
use std::convert::Infallible;
use std::io::Write as _;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, RawQuery, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, Uri, Version, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post};
use base64::Engine as _;
use futures_util::StreamExt;
use hyper::service::Service as _;
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto;
use hyper_util::service::TowerToHyperService;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

mod auth;
pub mod graphql;
pub mod grpc;
pub mod h3;
mod subscriptions;
pub use auth::ConnectionId;
pub use grpc::GrpcTestServer;
pub use h3::H3TestServer;

/// Certificates for the TLS server: a throwaway CA and a leaf for localhost/127.0.0.1.
pub struct TestCerts {
    pub ca_pem: String,
    pub cert_pem: String,
    pub key_pem: String,
}

impl TestCerts {
    pub fn generate() -> Self {
        use rcgen::{
            BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
            KeyUsagePurpose,
        };
        let today = time::OffsetDateTime::now_utc();
        let ca_key = KeyPair::generate().expect("ca key");
        let mut ca_params = CertificateParams::new(Vec::<String>::new()).expect("ca params");
        ca_params.distinguished_name.push(DnType::CommonName, "Zorvik Test CA");
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        ca_params.key_usages =
            vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign, KeyUsagePurpose::DigitalSignature];
        ca_params.not_before = today - Duration::from_secs(86400);
        ca_params.not_after = today + Duration::from_secs(86400 * 30);
        let ca_cert = ca_params.self_signed(&ca_key).expect("ca cert");
        let issuer = Issuer::new(ca_params, ca_key);

        let leaf_key = KeyPair::generate().expect("leaf key");
        let mut leaf =
            CertificateParams::new(vec!["localhost".to_string(), "127.0.0.1".to_string(), "::1".to_string()])
                .expect("leaf params");
        leaf.distinguished_name.push(DnType::CommonName, "localhost");
        leaf.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        leaf.key_usages = vec![KeyUsagePurpose::DigitalSignature, KeyUsagePurpose::KeyEncipherment];
        leaf.not_before = today - Duration::from_secs(86400);
        leaf.not_after = today + Duration::from_secs(86400 * 30);
        let leaf_cert = leaf.signed_by(&leaf_key, &issuer).expect("leaf cert");
        TestCerts { ca_pem: ca_cert.pem(), cert_pem: leaf_cert.pem(), key_pem: leaf_key.serialize_pem() }
    }

    /// Write the CA certificate to a file (for "custom CA" settings).
    pub fn write_ca(&self, dir: &std::path::Path) -> PathBuf {
        let path = dir.join("zorvik-test-ca.pem");
        std::fs::write(&path, &self.ca_pem).expect("write ca");
        path
    }

    fn server_config(&self) -> rustls::ServerConfig {
        use rustls_pki_types::pem::PemObject;
        use rustls_pki_types::{CertificateDer, PrivateKeyDer};
        let certs = CertificateDer::pem_slice_iter(self.cert_pem.as_bytes()).collect::<Result<Vec<_>, _>>().unwrap();
        let key = PrivateKeyDer::from_pem_slice(self.key_pem.as_bytes()).unwrap();
        let mut config =
            rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_safe_default_protocol_versions()
                .unwrap()
                .with_no_client_auth()
                .with_single_cert(certs, key)
                .unwrap();
        config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        config
    }
}

#[derive(Default)]
struct AppState {
    /// authorization code -> (code_challenge, redirect_uri)
    codes: Mutex<HashMap<String, (Option<String>, String)>>,
    tokens: Mutex<Vec<String>>,
    counter: AtomicU64,
    /// Digest nonces this server issued.
    digest_nonces: Mutex<HashSet<String>>,
    /// NTLM server challenge per connection ([`ConnectionId`]).
    ntlm_challenges: Mutex<HashMap<u64, [u8; 8]>>,
}

impl AppState {
    fn issue_token(&self, prefix: &str) -> String {
        let n = self.counter.fetch_add(1, Ordering::SeqCst);
        let token = format!("{prefix}-{n}");
        self.tokens.lock().unwrap().push(token.clone());
        token
    }
}

/// A running test server. Dropping it stops accepting connections.
pub struct TestServer {
    pub addr: SocketAddr,
    pub tls: bool,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl TestServer {
    /// Plain HTTP (HTTP/1.1 and h2c prior knowledge) on 127.0.0.1, random port.
    pub async fn start() -> Self {
        Self::bind("127.0.0.1:0".parse().unwrap(), None).await
    }

    /// HTTPS with ALPN h2/http1.1 using `certs`.
    pub async fn start_tls(certs: &TestCerts) -> Self {
        Self::bind("127.0.0.1:0".parse().unwrap(), Some(certs)).await
    }

    pub async fn bind(addr: SocketAddr, certs: Option<&TestCerts>) -> Self {
        let listener = TcpListener::bind(addr).await.expect("bind test server");
        let addr = listener.local_addr().unwrap();
        let app = router();
        let acceptor = certs.map(|c| tokio_rustls::TlsAcceptor::from(Arc::new(c.server_config())));
        let tls = acceptor.is_some();
        let task = tokio::spawn(async move {
            let mut connections = 0u64;
            loop {
                let Ok((tcp, _)) = listener.accept().await else { continue };
                let app = app.clone();
                let acceptor = acceptor.clone();
                connections += 1;
                let connection = ConnectionId(connections);
                tokio::spawn(async move {
                    // Every request carries its connection's id (NTLM authenticates connections).
                    let tower = TowerToHyperService::new(app);
                    let service = hyper::service::service_fn(move |mut req: http::Request<hyper::body::Incoming>| {
                        req.extensions_mut().insert(connection);
                        tower.call(req)
                    });
                    let builder = auto::Builder::new(TokioExecutor::new());
                    match acceptor {
                        Some(acceptor) => {
                            if let Ok(stream) = acceptor.accept(tcp).await {
                                let _ = builder.serve_connection_with_upgrades(TokioIo::new(stream), service).await;
                            }
                        }
                        None => {
                            let _ = builder.serve_connection_with_upgrades(TokioIo::new(tcp), service).await;
                        }
                    }
                });
            }
        });
        TestServer { addr, tls, task }
    }

    /// Base URL such as `http://127.0.0.1:1234`.
    pub fn url(&self, path: &str) -> String {
        let scheme = if self.tls { "https" } else { "http" };
        format!("{scheme}://{}{path}", self.addr)
    }

    pub fn ws_url(&self, path: &str) -> String {
        let scheme = if self.tls { "wss" } else { "ws" };
        format!("{scheme}://{}{path}", self.addr)
    }
}

pub fn router() -> Router {
    Router::new()
        .route("/", get(index))
        .route("/echo", any(echo))
        .route("/anything/{*rest}", any(echo))
        .route("/status/{code}", any(status))
        .route("/redirect/{n}", any(redirect_n))
        .route("/redirect-to", any(redirect_to))
        .route("/gzip", get(|| compressed("gzip")))
        .route("/deflate", get(|| compressed("deflate")))
        .route("/brotli", get(|| compressed("br")))
        .route("/delay/{ms}", any(delay))
        .route("/bytes/{n}", get(bytes_n))
        .route("/stream-bytes/{n}", get(stream_bytes))
        .route("/cookies", get(cookies))
        .route("/cookies/set", get(cookies_set))
        .route("/basic-auth/{user}/{pass}", get(basic_auth))
        .route("/bearer", get(bearer))
        .route("/digest-auth/{qop}/{user}/{passwd}", any(auth::digest))
        .route("/digest-auth/{qop}/{user}/{passwd}/{algorithm}", any(auth::digest))
        .route("/ntlm/{domain}/{user}/{passwd}", any(auth::ntlm))
        .route("/json", get(|| async { axum::Json(sample_json()) }))
        .route("/big-json", get(big_json))
        .route("/html", get(|| async { ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], SAMPLE_HTML) }))
        .route("/xml", get(|| async { ([(header::CONTENT_TYPE, "application/xml")], SAMPLE_XML) }))
        .route("/image.png", get(|| async { ([(header::CONTENT_TYPE, "image/png")], PNG_1X1.to_vec()) }))
        .route("/sse", get(sse))
        .route("/ws", get(ws))
        .route("/oauth/authorize", get(oauth_authorize))
        .route("/oauth/token", post(oauth_token))
        .route("/oauth/protected", get(oauth_protected))
        .route("/graphql", get(graphql::endpoint).post(graphql::endpoint))
        .route("/graphql-auth", get(graphql::endpoint_auth).post(graphql::endpoint_auth))
        .route("/graphql-ws", get(subscriptions::ws))
        .route("/graphql-sse", post(subscriptions::sse))
        .with_state(Arc::new(AppState::default()))
}

async fn index() -> impl IntoResponse {
    axum::Json(json!({
        "name": "Zorvik test server",
        "endpoints": ["/echo", "/anything/*", "/status/{code}", "/redirect/{n}", "/redirect-to?url=&status=",
            "/gzip", "/deflate", "/brotli", "/delay/{ms}", "/bytes/{n}", "/stream-bytes/{n}", "/cookies",
            "/cookies/set?k=v", "/basic-auth/{user}/{pass}", "/bearer",
            "/digest-auth/{qop}/{user}/{passwd}[/{algorithm}]", "/ntlm/{domain}/{user}/{passwd}",
            "/json", "/big-json?n=", "/html", "/xml",
            "/image.png", "/sse?count=&interval=", "/ws", "/oauth/authorize", "/oauth/token", "/oauth/protected",
            "/graphql?legacy=", "/graphql-auth", "/graphql-ws?auth=", "/graphql-sse"]
    }))
}

fn version_str(v: Version) -> &'static str {
    match v {
        Version::HTTP_2 => "HTTP/2",
        Version::HTTP_10 => "HTTP/1.0",
        _ => "HTTP/1.1",
    }
}

async fn echo(method: Method, uri: Uri, version: Version, headers: HeaderMap, body: Bytes) -> impl IntoResponse {
    let header_list: Vec<Value> =
        headers.iter().map(|(n, v)| json!([n.as_str(), String::from_utf8_lossy(v.as_bytes())])).collect();
    let args: HashMap<String, String> = url_query(uri.query().unwrap_or_default()).into_iter().collect();
    axum::Json(json!({
        "method": method.as_str(),
        "path": uri.path(),
        "query": uri.query().unwrap_or_default(),
        "args": args,
        "httpVersion": version_str(version),
        "headers": header_list,
        "body": String::from_utf8_lossy(&body),
        "bodyBase64": base64::engine::general_purpose::STANDARD.encode(&body),
        "bodyLength": body.len(),
    }))
}

fn url_query(q: &str) -> Vec<(String, String)> {
    q.split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (decode(k), decode(v))
        })
        .collect()
}

fn decode(s: &str) -> String {
    let s = s.replace('+', " ");
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        // Byte slice, not `&s[..]`: `%` followed by a multi-byte character must not panic.
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Some(b) = std::str::from_utf8(&bytes[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(b);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

async fn status(Path(code): Path<u16>) -> Response {
    let status = StatusCode::from_u16(code).unwrap_or(StatusCode::BAD_REQUEST);
    let mut resp = status.into_response();
    if status == StatusCode::UNAUTHORIZED {
        resp.headers_mut().insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Basic realm=\"test\""));
    }
    resp
}

async fn redirect_n(Path(n): Path<u32>) -> Response {
    if n == 0 {
        return "redirect chain done".into_response();
    }
    (StatusCode::FOUND, [(header::LOCATION, format!("/redirect/{}", n - 1))]).into_response()
}

async fn redirect_to(Query(q): Query<HashMap<String, String>>) -> Response {
    let url = q.get("url").cloned().unwrap_or_else(|| "/echo".into());
    let code = q.get("status").and_then(|s| s.parse().ok()).unwrap_or(302);
    (StatusCode::from_u16(code).unwrap_or(StatusCode::FOUND), [(header::LOCATION, url)]).into_response()
}

async fn compressed(encoding: &'static str) -> Response {
    let body = serde_json::to_vec(&json!({ "compressed": encoding, "ok": true })).unwrap();
    let data = match encoding {
        "gzip" => {
            let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            e.write_all(&body).unwrap();
            e.finish().unwrap()
        }
        "deflate" => {
            let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
            e.write_all(&body).unwrap();
            e.finish().unwrap()
        }
        _ => {
            let mut out = Vec::new();
            {
                let mut w = brotli::CompressorWriter::new(&mut out, 4096, 5, 22);
                w.write_all(&body).unwrap();
            }
            out
        }
    };
    ([(header::CONTENT_TYPE, "application/json"), (header::CONTENT_ENCODING, encoding)], data).into_response()
}

async fn delay(Path(ms): Path<u64>) -> impl IntoResponse {
    tokio::time::sleep(Duration::from_millis(ms.min(60_000))).await;
    axum::Json(json!({ "delayedMs": ms }))
}

async fn bytes_n(Path(n): Path<usize>) -> impl IntoResponse {
    let n = n.min(512 << 20);
    let data: Vec<u8> = (0..n).map(|i| b'a' + (i % 26) as u8).collect();
    ([(header::CONTENT_TYPE, "application/octet-stream")], data)
}

async fn stream_bytes(Path(n): Path<usize>) -> Response {
    let chunks = n.div_ceil(1024);
    let stream = futures_util::stream::iter((0..chunks).map(move |i| {
        let len = if i + 1 == chunks { n - i * 1024 } else { 1024 };
        Ok::<_, Infallible>(Bytes::from(vec![b'x'; len]))
    }));
    Response::builder()
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .body(Body::from_stream(stream))
        .unwrap()
}

async fn cookies(headers: HeaderMap) -> impl IntoResponse {
    let mut map = serde_json::Map::new();
    for v in headers.get_all(header::COOKIE) {
        for pair in String::from_utf8_lossy(v.as_bytes()).split(';') {
            if let Some((k, v)) = pair.trim().split_once('=') {
                map.insert(k.to_string(), Value::String(v.to_string()));
            }
        }
    }
    axum::Json(json!({ "cookies": map }))
}

async fn cookies_set(RawQuery(q): RawQuery) -> Response {
    let mut resp = axum::Json(json!({ "set": true })).into_response();
    for (k, v) in url_query(&q.unwrap_or_default()) {
        resp.headers_mut()
            .append(header::SET_COOKIE, HeaderValue::from_str(&format!("{k}={v}; Path=/; Max-Age=3600")).unwrap());
    }
    resp
}

fn basic_credentials(headers: &HeaderMap) -> Option<(String, String)> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let encoded = value.strip_prefix("Basic ")?;
    let decoded = base64::engine::general_purpose::STANDARD.decode(encoded.trim()).ok()?;
    let text = String::from_utf8(decoded).ok()?;
    let (u, p) = text.split_once(':')?;
    Some((u.to_string(), p.to_string()))
}

async fn basic_auth(Path((user, pass)): Path<(String, String)>, headers: HeaderMap) -> Response {
    match basic_credentials(&headers) {
        Some((u, p)) if u == user && p == pass => {
            axum::Json(json!({ "authenticated": true, "user": u })).into_response()
        }
        _ => (StatusCode::UNAUTHORIZED, [(header::WWW_AUTHENTICATE, "Basic realm=\"test\"")]).into_response(),
    }
}

async fn bearer(headers: HeaderMap) -> Response {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::to_string);
    match token {
        Some(t) => axum::Json(json!({ "authenticated": true, "token": t })).into_response(),
        None => (StatusCode::UNAUTHORIZED, [(header::WWW_AUTHENTICATE, "Bearer")]).into_response(),
    }
}

fn sample_json() -> Value {
    json!({
        "id": 42,
        "name": "Zorvik",
        "price": 19.90,
        "tags": ["api", "testing"],
        "nested": { "ok": true, "nothing": null },
        "unicode": "héllo ✓"
    })
}

async fn big_json(Query(q): Query<HashMap<String, String>>) -> impl IntoResponse {
    let n: usize = q.get("n").and_then(|v| v.parse().ok()).unwrap_or(1000).min(1_000_000);
    let items: Vec<Value> =
        (0..n).map(|i| json!({ "index": i, "name": format!("item-{i}"), "active": i % 2 == 0 })).collect();
    axum::Json(json!({ "count": n, "items": items }))
}

async fn sse(Query(q): Query<HashMap<String, String>>) -> Response {
    let count: usize = q.get("count").and_then(|v| v.parse().ok()).unwrap_or(5);
    let interval: u64 = q.get("interval").and_then(|v| v.parse().ok()).unwrap_or(100);
    let stream = futures_util::stream::unfold(0usize, move |i| async move {
        if i >= count {
            return None;
        }
        if i > 0 {
            tokio::time::sleep(Duration::from_millis(interval)).await;
        }
        let chunk = if i == 0 {
            format!(": welcome\nretry: 2000\nid: {i}\ndata: {{\"n\":{i}}}\n\n")
        } else if i % 2 == 1 {
            format!("event: tick\nid: {i}\ndata: line one {i}\ndata: line two\n\n")
        } else {
            format!("id: {i}\ndata: {{\"n\":{i}}}\n\n")
        };
        Some((Ok::<_, Infallible>(Bytes::from(chunk)), i + 1))
    });
    Response::builder()
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from_stream(stream))
        .unwrap()
}

/// `?delay=ms` holds the upgrade response (slow handshakes).
async fn ws(upgrade: WebSocketUpgrade, headers: HeaderMap, Query(q): Query<HashMap<String, String>>) -> Response {
    if let Some(ms) = q.get("delay").and_then(|v| v.parse::<u64>().ok()) {
        tokio::time::sleep(Duration::from_millis(ms.min(60_000))).await;
    }
    let protocol = headers
        .get("sec-websocket-protocol")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(|s| s.trim().to_string());
    let upgrade = match protocol {
        Some(p) => upgrade.protocols([p]),
        None => upgrade,
    };
    upgrade.on_upgrade(ws_echo)
}

async fn ws_echo(mut socket: WebSocket) {
    let _ = socket.send(Message::Text("welcome".into())).await;
    while let Some(Ok(msg)) = socket.next().await {
        match msg {
            Message::Text(t) if t.as_str() == "close" => {
                let _ = socket
                    .send(Message::Close(Some(axum::extract::ws::CloseFrame { code: 4000, reason: "bye".into() })))
                    .await;
                return;
            }
            Message::Text(t) => {
                if socket.send(Message::Text(t)).await.is_err() {
                    return;
                }
            }
            Message::Binary(b) => {
                if socket.send(Message::Binary(b)).await.is_err() {
                    return;
                }
            }
            Message::Close(_) => return,
            _ => {}
        }
    }
}

async fn oauth_authorize(State(state): State<Arc<AppState>>, Query(q): Query<HashMap<String, String>>) -> Response {
    let Some(redirect_uri) = q.get("redirect_uri") else {
        return (StatusCode::BAD_REQUEST, "missing redirect_uri").into_response();
    };
    if q.get("response_type").map(String::as_str) != Some("code") {
        return (StatusCode::BAD_REQUEST, "response_type must be code").into_response();
    }
    let code = format!("code-{}", state.counter.fetch_add(1, Ordering::SeqCst));
    state.codes.lock().unwrap().insert(code.clone(), (q.get("code_challenge").cloned(), redirect_uri.clone()));
    let sep = if redirect_uri.contains('?') { '&' } else { '?' };
    let mut location = format!("{redirect_uri}{sep}code={code}");
    if let Some(s) = q.get("state") {
        location.push_str(&format!("&state={}", encode(s)));
    }
    (StatusCode::FOUND, [(header::LOCATION, location)]).into_response()
}

fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

async fn oauth_token(State(state): State<Arc<AppState>>, headers: HeaderMap, body: Bytes) -> Response {
    let form: HashMap<String, String> = url_query(&String::from_utf8_lossy(&body)).into_iter().collect();
    let (client_id, client_secret) = basic_credentials(&headers)
        .or_else(|| Some((form.get("client_id")?.clone(), form.get("client_secret").cloned().unwrap_or_default())))
        .unwrap_or_default();
    let error = |code: StatusCode, err: &str| (code, axum::Json(json!({ "error": err }))).into_response();
    if client_id != "test-client" {
        return error(StatusCode::UNAUTHORIZED, "invalid_client");
    }
    let grant = form.get("grant_type").map(String::as_str).unwrap_or_default();
    match grant {
        "client_credentials" if client_secret == "test-secret" => {}
        "password"
            if form.get("username").map(String::as_str) == Some("alice")
                && form.get("password").map(String::as_str) == Some("wonderland") => {}
        "authorization_code" => {
            let code = form.get("code").cloned().unwrap_or_default();
            let Some((challenge, redirect)) = state.codes.lock().unwrap().remove(&code) else {
                return error(StatusCode::BAD_REQUEST, "invalid_grant");
            };
            if form.get("redirect_uri") != Some(&redirect) {
                return error(StatusCode::BAD_REQUEST, "invalid_grant");
            }
            if let Some(challenge) = challenge {
                let verifier = form.get("code_verifier").cloned().unwrap_or_default();
                if sha256_base64url(verifier.as_bytes()) != challenge {
                    return error(StatusCode::BAD_REQUEST, "invalid_grant");
                }
            }
        }
        "refresh_token" if form.get("refresh_token").is_some_and(|r| r.starts_with("refresh-")) => {}
        "client_credentials" | "password" => return error(StatusCode::UNAUTHORIZED, "invalid_client"),
        _ => return error(StatusCode::BAD_REQUEST, "unsupported_grant_type"),
    }
    let access = state.issue_token("access");
    axum::Json(json!({
        "access_token": access,
        "token_type": "Bearer",
        "expires_in": 3600,
        "refresh_token": state.issue_token("refresh"),
        "scope": form.get("scope").cloned().unwrap_or_default(),
    }))
    .into_response()
}

async fn oauth_protected(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or_default();
    if state.tokens.lock().unwrap().iter().any(|t| t == token && t.starts_with("access-")) {
        axum::Json(json!({ "ok": true })).into_response()
    } else {
        StatusCode::UNAUTHORIZED.into_response()
    }
}

/// A tiny forward proxy for testing proxy support.
pub struct TestProxy {
    pub addr: SocketAddr,
    /// Number of requests/tunnels served.
    pub hits: Arc<AtomicU64>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for TestProxy {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl TestProxy {
    /// `credentials`: required `user:pass` for Basic Proxy-Authorization.
    pub async fn start(credentials: Option<&str>) -> Self {
        Self::start_with_upstream(credentials, None).await
    }

    /// Like [`TestProxy::start`], but every request/tunnel goes to `upstream`
    /// regardless of the requested host (lets tests use fake host names).
    pub async fn start_with_upstream(credentials: Option<&str>, upstream: Option<SocketAddr>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let hits = Arc::new(AtomicU64::new(0));
        let expected = credentials.map(|c| format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(c)));
        let counter = hits.clone();
        let task = tokio::spawn(async move {
            loop {
                let Ok((client, _)) = listener.accept().await else { continue };
                let expected = expected.clone();
                let counter = counter.clone();
                tokio::spawn(async move {
                    let _ = proxy_connection(client, expected, counter, upstream).await;
                });
            }
        });
        TestProxy { addr, hits, task }
    }

    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }
}

async fn proxy_connection(
    mut client: TcpStream,
    expected: Option<String>,
    hits: Arc<AtomicU64>,
    upstream_override: Option<SocketAddr>,
) -> std::io::Result<()> {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        if client.read(&mut byte).await? == 0 {
            return Ok(());
        }
        head.push(byte[0]);
    }
    let text = String::from_utf8_lossy(&head).into_owned();
    let mut lines = text.split("\r\n");
    let request_line = lines.next().unwrap_or_default().to_string();
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect();
    if let Some(expected) = &expected {
        let given = headers.iter().find(|(k, _)| k == "proxy-authorization").map(|(_, v)| v.as_str());
        if given != Some(expected.as_str()) {
            client
                .write_all(b"HTTP/1.1 407 Proxy Authentication Required\r\nProxy-Authenticate: Basic realm=\"proxy\"\r\nContent-Length: 0\r\n\r\n")
                .await?;
            return Ok(());
        }
    }
    hits.fetch_add(1, Ordering::SeqCst);
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let target = parts.next().unwrap_or_default().to_string();
    if method == "CONNECT" {
        let mut upstream = match upstream_override {
            Some(addr) => TcpStream::connect(addr).await?,
            None => TcpStream::connect(&target).await?,
        };
        client.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n").await?;
        let _ = tokio::io::copy_bidirectional(&mut client, &mut upstream).await;
        return Ok(());
    }
    // Absolute-form: http://host:port/path
    let without_scheme = target.strip_prefix("http://").unwrap_or(&target);
    let (authority, path) = match without_scheme.find('/') {
        Some(i) => (&without_scheme[..i], &without_scheme[i..]),
        None => (without_scheme, "/"),
    };
    let authority = if authority.contains(':') { authority.to_string() } else { format!("{authority}:80") };
    let mut upstream = match upstream_override {
        Some(addr) => TcpStream::connect(addr).await?,
        None => TcpStream::connect(&authority).await?,
    };
    let mut forwarded = format!("{method} {path} HTTP/1.1\r\n");
    for (k, v) in &headers {
        if k != "proxy-authorization" && k != "proxy-connection" {
            forwarded.push_str(&format!("{k}: {v}\r\n"));
        }
    }
    forwarded.push_str("Via: 1.1 zorvik-test-proxy\r\n\r\n");
    upstream.write_all(forwarded.as_bytes()).await?;
    let _ = tokio::io::copy_bidirectional(&mut client, &mut upstream).await;
    Ok(())
}

fn sha256_base64url(data: &[u8]) -> String {
    use sha2::Digest as _;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sha2::Sha256::digest(data))
}

#[test]
fn percent_decoding() {
    assert_eq!(decode("a%20b+c%2F"), "a b c/");
    // `%` followed by a multi-byte character is kept as-is (used to panic).
    assert_eq!(decode("%aé"), "%aé");
}

#[test]
fn pkce_known_vector() {
    // RFC 7636 appendix B.
    assert_eq!(
        sha256_base64url(b"dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
        "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
    );
}

const SAMPLE_HTML: &str = "<!doctype html><html><head><title>Test page</title></head><body><h1>Hello from Zorvik</h1><p>Sample HTML response.</p><script>document.title='scripts must not run'</script></body></html>";
const SAMPLE_XML: &str = "<?xml version=\"1.0\"?><note><to>You</to><from>Zorvik</from><body>Sample XML</body></note>";
const PNG_1X1: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00,
    0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0D, 0x49,
    0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0xF8, 0xCF, 0xC0, 0xF0, 0x1F, 0x00, 0x05, 0x00, 0x01, 0xFF, 0x89, 0x99, 0x3D,
    0x1D, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
];

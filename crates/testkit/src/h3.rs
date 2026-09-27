//! HTTP/3 (QUIC over UDP) test server: quinn + h3 with the [`TestCerts`] leaf
//! certificate (trust `TestCerts::ca_pem`, or turn verification off).
//!
//! Endpoints: `/echo` and `/anything/*` (JSON like the TCP server, with
//! `httpVersion: "HTTP/3"` and the request `authority`), `/status/{code}`,
//! `/redirect/{n}`, `/redirect-to?url=&status=`, `/gzip`, `/bytes/{n}`,
//! `/delay/{ms}`, `/cookies`, `/cookies/set?k=v`, `/sse?count=&interval=`.

use std::collections::HashMap;
use std::io::Write as _;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use base64::Engine as _;
use bytes::{Buf, Bytes};
use http::{HeaderValue, Request, Response, StatusCode, header};
use serde_json::{Value, json};

use crate::TestCerts;

type Stream = ::h3::server::RequestStream<h3_quinn::BidiStream<Bytes>, Bytes>;
type StreamResult = Result<(), ::h3::error::StreamError>;

/// Request bodies are read up to this size (the rest is discarded).
const MAX_REQUEST_BODY: usize = 16 << 20;

/// A running HTTP/3 server on UDP. Dropping it closes every connection.
pub struct H3TestServer {
    pub addr: SocketAddr,
    endpoint: quinn::Endpoint,
    open: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for H3TestServer {
    fn drop(&mut self) {
        self.endpoint.close(0u32.into(), b"server stopped");
        self.task.abort();
    }
}

impl H3TestServer {
    /// HTTP/3 on 127.0.0.1, random UDP port.
    pub async fn start(certs: &TestCerts) -> Self {
        Self::bind("127.0.0.1:0".parse().unwrap(), certs).await
    }

    pub async fn bind(addr: SocketAddr, certs: &TestCerts) -> Self {
        Self::bind_with_alpn(addr, certs, &[b"h3"]).await
    }

    /// A QUIC server offering other ALPN protocols (e.g. to test a server without HTTP/3).
    pub async fn bind_with_alpn(addr: SocketAddr, certs: &TestCerts, alpn: &[&[u8]]) -> Self {
        let mut tls = certs.server_config();
        tls.alpn_protocols = alpn.iter().map(|p| p.to_vec()).collect();
        let crypto = quinn::crypto::rustls::QuicServerConfig::try_from(tls).expect("QUIC server config");
        let endpoint = quinn::Endpoint::server(quinn::ServerConfig::with_crypto(Arc::new(crypto)), addr)
            .expect("bind HTTP/3 test server");
        let addr = endpoint.local_addr().unwrap();
        let open = Arc::new(AtomicUsize::new(0));
        let (accepting, counter) = (endpoint.clone(), open.clone());
        let task = tokio::spawn(async move {
            while let Some(incoming) = accepting.accept().await {
                tokio::spawn(serve_connection(incoming, counter.clone()));
            }
        });
        H3TestServer { addr, endpoint, open, task }
    }

    /// URL such as `https://127.0.0.1:1234/echo`.
    pub fn url(&self, path: &str) -> String {
        format!("https://{}{path}", self.addr)
    }

    /// QUIC connections currently open (a client must close its connection when done).
    pub fn open_connections(&self) -> usize {
        self.open.load(Ordering::SeqCst)
    }
}

async fn serve_connection(incoming: quinn::Incoming, open: Arc<AtomicUsize>) {
    let Ok(conn) = incoming.await else { return };
    open.fetch_add(1, Ordering::SeqCst);
    let requests = tokio::spawn({
        let conn = conn.clone();
        async move {
            let Ok(mut h3) = ::h3::server::Connection::<_, Bytes>::new(h3_quinn::Connection::new(conn)).await else {
                return;
            };
            while let Ok(Some(resolver)) = h3.accept().await {
                tokio::spawn(async move {
                    if let Ok((req, stream)) = resolver.resolve_request().await {
                        let _ = handle(req, stream).await;
                    }
                });
            }
        }
    });
    conn.closed().await;
    requests.abort();
    open.fetch_sub(1, Ordering::SeqCst);
}

async fn handle(req: Request<()>, mut stream: Stream) -> StreamResult {
    let mut body = Vec::new();
    while let Some(mut chunk) = stream.recv_data().await? {
        let take = chunk.remaining().min(MAX_REQUEST_BODY.saturating_sub(body.len()));
        body.extend_from_slice(&chunk.copy_to_bytes(take));
        chunk.advance(chunk.remaining());
    }
    let path = req.uri().path().to_string();
    let args: HashMap<String, String> = crate::url_query(req.uri().query().unwrap_or_default()).into_iter().collect();
    let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    match segments.as_slice() {
        ["echo"] | ["anything", ..] => {
            let headers: Vec<Value> =
                req.headers().iter().map(|(n, v)| json!([n.as_str(), String::from_utf8_lossy(v.as_bytes())])).collect();
            let echo = json!({
                "method": req.method().as_str(),
                "path": path,
                "query": req.uri().query().unwrap_or_default(),
                "args": args,
                "httpVersion": "HTTP/3",
                "authority": req.uri().authority().map(|a| a.as_str()).unwrap_or_default(),
                "headers": headers,
                "body": String::from_utf8_lossy(&body),
                "bodyBase64": base64::engine::general_purpose::STANDARD.encode(&body),
                "bodyLength": body.len(),
            });
            json_response(&mut stream, StatusCode::OK, &echo).await
        }
        ["status", code] => {
            let status =
                code.parse().ok().and_then(|c| StatusCode::from_u16(c).ok()).unwrap_or(StatusCode::BAD_REQUEST);
            respond(&mut stream, status, &[], Bytes::new()).await
        }
        ["redirect", n] => match n.parse::<u32>().unwrap_or(0) {
            0 => respond(&mut stream, StatusCode::OK, &[], Bytes::from_static(b"redirect chain done")).await,
            n => {
                respond(&mut stream, StatusCode::FOUND, &[("location", format!("/redirect/{}", n - 1))], Bytes::new())
                    .await
            }
        },
        ["redirect-to"] => {
            let url = args.get("url").cloned().unwrap_or_else(|| "/echo".into());
            let status = args.get("status").and_then(|s| s.parse().ok()).and_then(|c| StatusCode::from_u16(c).ok());
            respond(&mut stream, status.unwrap_or(StatusCode::FOUND), &[("location", url)], Bytes::new()).await
        }
        ["gzip"] => {
            let json = serde_json::to_vec(&json!({ "compressed": "gzip", "ok": true })).unwrap();
            let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            encoder.write_all(&json).unwrap();
            let headers = [("content-type", "application/json".into()), ("content-encoding", "gzip".into())];
            respond(&mut stream, StatusCode::OK, &headers, encoder.finish().unwrap().into()).await
        }
        ["bytes", n] => {
            let n = n.parse::<usize>().unwrap_or(0).min(256 << 20);
            let headers = [("content-type", "application/octet-stream".into()), ("content-length", n.to_string())];
            stream.send_response(response(StatusCode::OK, &headers)).await?;
            // Sent in chunks, like a real download.
            let mut sent = 0;
            while sent < n {
                let len = (n - sent).min(16 * 1024);
                let chunk: Vec<u8> = (sent..sent + len).map(|i| b'a' + (i % 26) as u8).collect();
                stream.send_data(chunk.into()).await?;
                sent += len;
            }
            stream.finish().await
        }
        ["delay", ms] => {
            let ms = ms.parse::<u64>().unwrap_or(0).min(60_000);
            tokio::time::sleep(Duration::from_millis(ms)).await;
            json_response(&mut stream, StatusCode::OK, &json!({ "delayedMs": ms })).await
        }
        ["cookies"] => {
            let mut cookies = serde_json::Map::new();
            for value in req.headers().get_all(header::COOKIE) {
                for pair in String::from_utf8_lossy(value.as_bytes()).split(';') {
                    if let Some((k, v)) = pair.trim().split_once('=') {
                        cookies.insert(k.to_string(), Value::String(v.to_string()));
                    }
                }
            }
            json_response(&mut stream, StatusCode::OK, &json!({ "cookies": cookies })).await
        }
        ["cookies", "set"] => {
            let mut headers: Vec<(&str, String)> = vec![("content-type", "application/json".into())];
            for (k, v) in crate::url_query(req.uri().query().unwrap_or_default()) {
                headers.push(("set-cookie", format!("{k}={v}; Path=/; Max-Age=3600")));
            }
            respond(&mut stream, StatusCode::OK, &headers, Bytes::from_static(br#"{"set":true}"#)).await
        }
        ["sse"] => {
            let count: usize = args.get("count").and_then(|v| v.parse().ok()).unwrap_or(3);
            let interval: u64 = args.get("interval").and_then(|v| v.parse().ok()).unwrap_or(50);
            let headers = [("content-type", "text/event-stream".into()), ("cache-control", "no-cache".into())];
            stream.send_response(response(StatusCode::OK, &headers)).await?;
            for i in 0..count {
                if i > 0 {
                    tokio::time::sleep(Duration::from_millis(interval)).await;
                }
                stream.send_data(format!("id: {i}\ndata: {{\"n\":{i}}}\n\n").into()).await?;
            }
            stream.finish().await
        }
        _ => respond(&mut stream, StatusCode::NOT_FOUND, &[], Bytes::from_static(b"not found")).await,
    }
}

fn response(status: StatusCode, headers: &[(&str, String)]) -> Response<()> {
    let mut resp = Response::new(());
    *resp.status_mut() = status;
    for (name, value) in headers {
        if let Ok(value) = HeaderValue::from_str(value) {
            resp.headers_mut().append(header::HeaderName::from_bytes(name.as_bytes()).unwrap(), value);
        }
    }
    resp
}

async fn respond(stream: &mut Stream, status: StatusCode, headers: &[(&str, String)], body: Bytes) -> StreamResult {
    stream.send_response(response(status, headers)).await?;
    if !body.is_empty() {
        stream.send_data(body).await?;
    }
    stream.finish().await
}

async fn json_response(stream: &mut Stream, status: StatusCode, value: &Value) -> StreamResult {
    let body = Bytes::from(serde_json::to_vec(value).unwrap());
    respond(stream, status, &[("content-type", "application/json".into())], body).await
}

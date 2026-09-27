//! End-to-end tests against real local servers (see zorvik-testkit).

use std::time::Duration;

use bytes::Bytes;
use serde_json::Value;
use std::sync::atomic::Ordering;
use zorvik_engine::proxy::{ProxyMode, ProxySettings};
use zorvik_engine::{Client, CookieJar, ErrorKind, Header, HttpRequest, HttpVersionPref, RequestOptions, TlsOptions};
use zorvik_testkit::{TestCerts, TestProxy, TestServer};

fn get(url: impl Into<String>) -> HttpRequest {
    HttpRequest { method: "GET".into(), url: url.into(), headers: vec![], body: Bytes::new() }
}

fn json(body: &[u8]) -> Value {
    serde_json::from_slice(body).expect("json body")
}

fn header<'a>(echo: &'a Value, name: &str) -> Option<&'a str> {
    echo["headers"].as_array()?.iter().find(|h| h[0].as_str() == Some(name)).and_then(|h| h[1].as_str())
}

#[tokio::test]
async fn get_echo_with_default_headers_and_timing() {
    let server = TestServer::start().await;
    let client = Client::new();
    let resp = client.send(get(server.url("/echo?a=1&b=two")), &RequestOptions::default(), None).await.unwrap();
    assert_eq!(resp.meta.status, 200);
    assert_eq!(resp.meta.status_text, "OK");
    assert_eq!(resp.meta.http_version, "HTTP/1.1");
    let echo = json(&resp.body);
    assert_eq!(echo["args"]["b"], "two");
    assert!(header(&echo, "user-agent").unwrap().starts_with("Zorvik/"));
    assert_eq!(header(&echo, "accept"), Some("*/*"));
    assert!(resp.timing.total_ms > 0.0);
    assert!(resp.timing.ttfb_ms > 0.0);
    assert_eq!(resp.timing.tls_ms, 0.0);
    assert!(resp.meta.request.headers.iter().any(|h| h.name == "Host"));
    assert!(resp.meta.remote_addr.unwrap().starts_with("127.0.0.1"));
}

#[tokio::test]
async fn url_credentials_become_basic_auth() {
    let server = TestServer::start().await;
    let url = format!("http://us%20er:p%40ss@127.0.0.1:{}/echo", server.addr.port());
    let resp = Client::new().send(get(url), &RequestOptions::default(), None).await.unwrap();
    // base64("us er:p@ss")
    assert_eq!(header(&json(&resp.body), "authorization"), Some("Basic dXMgZXI6cEBzcw=="));

    // An explicit Authorization header wins.
    let mut req = get(format!("http://u:p@127.0.0.1:{}/echo", server.addr.port()));
    req.headers.push(Header::new("Authorization", "Bearer t"));
    let resp = Client::new().send(req, &RequestOptions::default(), None).await.unwrap();
    assert_eq!(header(&json(&resp.body), "authorization"), Some("Bearer t"));
}

#[tokio::test]
async fn localhost_falls_back_across_address_families() {
    let server = TestServer::start().await;
    let url = format!("http://localhost:{}/echo", server.addr.port());
    let resp = Client::new().send(get(url), &RequestOptions::default(), None).await.unwrap();
    assert_eq!(resp.meta.status, 200);
}

#[tokio::test]
async fn post_body_and_custom_headers() {
    let server = TestServer::start().await;
    let req = HttpRequest {
        method: "POST".into(),
        url: server.url("/echo"),
        headers: vec![
            Header::new("Content-Type", "application/json"),
            Header::new("X-Unicode", "café"),
            Header::new("Content-Length", "999"),
            Header::new("", "ignored"),
        ],
        body: Bytes::from_static(br#"{"hello":"world"}"#),
    };
    let resp = Client::new().send(req, &RequestOptions::default(), None).await.unwrap();
    let echo = json(&resp.body);
    assert_eq!(echo["method"], "POST");
    assert_eq!(echo["body"], r#"{"hello":"world"}"#);
    assert_eq!(header(&echo, "content-length"), Some("17"));
    assert_eq!(header(&echo, "x-unicode"), Some("café"));
}

#[tokio::test]
async fn empty_post_sends_zero_length_and_head_has_no_body() {
    let server = TestServer::start().await;
    let client = Client::new();
    let req = HttpRequest { method: "POST".into(), ..get(server.url("/echo")) };
    let echo = json(&client.send(req, &RequestOptions::default(), None).await.unwrap().body);
    assert_eq!(header(&echo, "content-length"), Some("0"));
    let head = HttpRequest { method: "HEAD".into(), ..get(server.url("/json")) };
    let resp = client.send(head, &RequestOptions::default(), None).await.unwrap();
    assert_eq!(resp.meta.status, 200);
    assert!(resp.body.is_empty());
}

#[tokio::test]
async fn h2c_prior_knowledge() {
    let server = TestServer::start().await;
    let opts = RequestOptions { http_version: HttpVersionPref::Http2, ..Default::default() };
    let resp = Client::new().send(get(server.url("/echo")), &opts, None).await.unwrap();
    assert_eq!(resp.meta.http_version, "HTTP/2");
    assert_eq!(json(&resp.body)["httpVersion"], "HTTP/2");
}

#[tokio::test]
async fn h2_accepts_large_response_headers() {
    // HTTP/1.1 accepts ~400 KB of response headers; HTTP/2 must not fail at 16 KB.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let service = hyper::service::service_fn(|_req| async {
            let resp = hyper::Response::builder()
                .header("x-big", "a".repeat(64 * 1024))
                .body(http_body_util::Full::new(Bytes::from_static(b"ok")))
                .unwrap();
            Ok::<_, std::convert::Infallible>(resp)
        });
        let _ = hyper::server::conn::http2::Builder::new(hyper_util::rt::TokioExecutor::new())
            .serve_connection(hyper_util::rt::TokioIo::new(tcp), service)
            .await;
    });
    let opts = RequestOptions { http_version: HttpVersionPref::Http2, ..Default::default() };
    let resp = Client::new().send(get(format!("http://{addr}/")), &opts, None).await.unwrap();
    assert_eq!(resp.meta.http_version, "HTTP/2");
    assert_eq!(resp.body, b"ok");
    assert!(resp.meta.headers.iter().any(|h| h.name == "X-Big" && h.value.len() == 64 * 1024));
}

#[tokio::test]
async fn tls_with_custom_ca_negotiates_h2() {
    let certs = TestCerts::generate();
    let dir = tempfile::tempdir().unwrap();
    let ca = certs.write_ca(dir.path());
    let server = TestServer::start_tls(&certs).await;
    let opts =
        RequestOptions { tls: TlsOptions { ca_cert_path: Some(ca), ..Default::default() }, ..Default::default() };
    let client = Client::new();
    let resp = client.send(get(server.url("/echo")), &opts, None).await.unwrap();
    assert_eq!(resp.meta.http_version, "HTTP/2");
    let tls = resp.meta.tls.expect("tls info");
    assert_eq!(tls.alpn.as_deref(), Some("h2"));
    assert!(tls.version.starts_with("TLS 1."));
    let cert = tls.certificate.expect("certificate");
    assert!(cert.subject.contains("localhost"));
    assert!(cert.subject_alt_names.contains(&"127.0.0.1".to_string()));
    assert!(resp.timing.tls_ms > 0.0);

    let http1 = RequestOptions { http_version: HttpVersionPref::Http1, ..opts.clone() };
    let resp = client.send(get(server.url("/echo")), &http1, None).await.unwrap();
    assert_eq!(resp.meta.http_version, "HTTP/1.1");
}

#[tokio::test]
async fn tls_untrusted_certificate_errors_with_hint_and_can_be_skipped() {
    let certs = TestCerts::generate();
    let server = TestServer::start_tls(&certs).await;
    let client = Client::new();
    let err = client.send(get(server.url("/echo")), &RequestOptions::default(), None).await.unwrap_err();
    assert_eq!(err.kind, ErrorKind::Tls, "{err:?}");
    let insecure = RequestOptions { tls: TlsOptions { verify: false, ..Default::default() }, ..Default::default() };
    let resp = client.send(get(server.url("/echo")), &insecure, None).await.unwrap();
    assert_eq!(resp.meta.status, 200);
}

#[tokio::test]
async fn redirects_follow_limit_and_disable() {
    let server = TestServer::start().await;
    let client = Client::new();
    let resp = client.send(get(server.url("/redirect/3")), &RequestOptions::default(), None).await.unwrap();
    assert_eq!(resp.meta.status, 200);
    assert_eq!(resp.meta.redirects.len(), 3);
    assert!(resp.meta.url.ends_with("/redirect/0"));
    assert!(resp.timing.redirect_ms > 0.0);

    let no_follow = RequestOptions { follow_redirects: false, ..Default::default() };
    let resp = client.send(get(server.url("/redirect/3")), &no_follow, None).await.unwrap();
    assert_eq!(resp.meta.status, 302);

    let limited = RequestOptions { max_redirects: 2, ..Default::default() };
    let err = client.send(get(server.url("/redirect/3")), &limited, None).await.unwrap_err();
    assert_eq!(err.kind, ErrorKind::TooManyRedirects);
}

#[tokio::test]
async fn see_other_turns_post_into_get() {
    let server = TestServer::start().await;
    let req = HttpRequest {
        method: "POST".into(),
        url: server.url("/redirect-to?url=/echo&status=303"),
        headers: vec![Header::new("Content-Type", "text/plain")],
        body: Bytes::from_static(b"payload"),
    };
    let resp = Client::new().send(req, &RequestOptions::default(), None).await.unwrap();
    let echo = json(&resp.body);
    assert_eq!(echo["method"], "GET");
    assert_eq!(echo["body"], "");
    assert!(header(&echo, "content-type").is_none());
}

#[tokio::test]
async fn decodes_compressed_bodies() {
    let server = TestServer::start().await;
    let client = Client::new();
    for (path, enc) in [("/gzip", "gzip"), ("/deflate", "deflate"), ("/brotli", "br")] {
        let resp = client.send(get(server.url(path)), &RequestOptions::default(), None).await.unwrap();
        assert_eq!(json(&resp.body)["compressed"], enc, "{path}");
        assert!(resp.decode_warning.is_none());
    }
    let raw = RequestOptions { decompress: false, ..Default::default() };
    let resp = client.send(get(server.url("/gzip")), &raw, None).await.unwrap();
    assert!(serde_json::from_slice::<Value>(&resp.body).is_err());
}

#[tokio::test]
async fn timeouts_and_connection_errors() {
    let server = TestServer::start().await;
    let client = Client::new();
    let opts = RequestOptions { timeout: Some(Duration::from_millis(300)), ..Default::default() };
    let err = client.send(get(server.url("/delay/3000")), &opts, None).await.unwrap_err();
    assert_eq!(err.kind, ErrorKind::Timeout);
    assert!(err.message.contains("300ms"));

    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let err =
        client.send(get(format!("http://127.0.0.1:{port}/")), &RequestOptions::default(), None).await.unwrap_err();
    assert_eq!(err.kind, ErrorKind::Connect);

    let err = client.send(get("http://does-not-exist.invalid/"), &RequestOptions::default(), None).await.unwrap_err();
    assert_eq!(err.kind, ErrorKind::Dns, "{err:?}");

    let err = client.send(get("mailto:x@y"), &RequestOptions::default(), None).await.unwrap_err();
    assert_eq!(err.kind, ErrorKind::InvalidRequest);

    let bad_header = HttpRequest { headers: vec![Header::new("X-Bad", "a\r\nb")], ..get(server.url("/echo")) };
    let err = client.send(bad_header, &RequestOptions::default(), None).await.unwrap_err();
    assert_eq!(err.kind, ErrorKind::InvalidRequest);
}

#[tokio::test]
async fn body_limit_truncates() {
    let server = TestServer::start().await;
    let opts = RequestOptions { max_body_bytes: 1000, ..Default::default() };
    let resp = Client::new().send(get(server.url("/bytes/5000")), &opts, None).await.unwrap();
    assert_eq!(resp.body.len(), 1000);
    assert!(resp.body_truncated);
    let resp =
        Client::new().send(get(server.url("/stream-bytes/3000")), &RequestOptions::default(), None).await.unwrap();
    assert_eq!(resp.body.len(), 3000);
    assert!(!resp.body_truncated);
}

#[tokio::test]
async fn cookie_jar_persists_across_requests_and_redirects() {
    let server = TestServer::start().await;
    let client = Client::new();
    let jar = CookieJar::new();
    let resp = client
        .send(get(server.url("/cookies/set?session=abc&theme=dark")), &RequestOptions::default(), Some(&jar))
        .await
        .unwrap();
    assert_eq!(resp.meta.cookies.len(), 2);
    let resp = client.send(get(server.url("/cookies")), &RequestOptions::default(), Some(&jar)).await.unwrap();
    assert_eq!(json(&resp.body)["cookies"]["session"], "abc");

    // Cookie set on a redirect hop is sent on the next hop.
    let jar2 = CookieJar::new();
    let url = server.url("/redirect-to?url=%2Fcookies&status=302");
    let _ = client.send(get(server.url("/cookies/set?x=1")), &RequestOptions::default(), Some(&jar2)).await.unwrap();
    let resp = client.send(get(url), &RequestOptions::default(), Some(&jar2)).await.unwrap();
    assert_eq!(json(&resp.body)["cookies"]["x"], "1");

    // Without a jar nothing is sent.
    let resp = client.send(get(server.url("/cookies")), &RequestOptions::default(), None).await.unwrap();
    assert!(json(&resp.body)["cookies"].as_object().unwrap().is_empty());
}

fn proxied(url: &str, bypass: &str) -> RequestOptions {
    RequestOptions {
        proxy: ProxySettings::from_mode(&ProxyMode::Manual { url: url.into(), bypass: bypass.into() }).unwrap(),
        ..Default::default()
    }
}

#[tokio::test]
async fn loopback_targets_bypass_proxy() {
    let proxy = TestProxy::start(None).await;
    let server = TestServer::start().await;
    let opts = proxied(&proxy.url(), "");
    let resp = Client::new().send(get(server.url("/echo")), &opts, None).await.unwrap();
    assert_eq!(resp.meta.status, 200);
    assert_eq!(proxy.hits.load(Ordering::SeqCst), 0);
    assert!(resp.meta.request.proxy.is_none());
}

#[tokio::test]
async fn http_via_forward_proxy_with_auth() {
    let server = TestServer::start().await;
    let proxy = TestProxy::start_with_upstream(Some("user:s3cret"), Some(server.addr)).await;
    let client = Client::new();
    let opts = proxied(&format!("http://user:s3cret@{}", proxy.addr), "");
    let resp = client.send(get("http://api.example.test/echo?x=1"), &opts, None).await.unwrap();
    assert_eq!(resp.meta.status, 200, "{}", String::from_utf8_lossy(&resp.body));
    let echo = json(&resp.body);
    assert_eq!(echo["path"], "/echo");
    assert_eq!(header(&echo, "host"), Some("api.example.test"));
    assert!(header(&echo, "proxy-authorization").is_none(), "proxy must strip its own auth");
    assert_eq!(proxy.hits.load(Ordering::SeqCst), 1);
    assert!(resp.meta.request.proxy.is_some());

    // Wrong credentials: the proxy answers 407 for a forward request.
    let bad = proxied(&format!("http://user:wrong@{}", proxy.addr), "");
    let resp = client.send(get("http://api.example.test/echo"), &bad, None).await.unwrap();
    assert_eq!(resp.meta.status, 407);

    // Bypass list skips the proxy (and then DNS fails for the fake host).
    let bypassed = proxied(&proxy.url(), "*.example.test");
    let err = client.send(get("http://api.example.test/echo"), &bypassed, None).await.unwrap_err();
    assert_eq!(err.kind, ErrorKind::Dns);
}

#[tokio::test]
async fn https_via_connect_tunnel() {
    let certs = TestCerts::generate();
    let server = TestServer::start_tls(&certs).await;
    let proxy = TestProxy::start_with_upstream(Some("u:p"), Some(server.addr)).await;
    let client = Client::new();
    let mut opts = proxied(&format!("http://u:p@{}", proxy.addr), "");
    opts.tls.verify = false; // certificate is for localhost, not the fake host
    let resp = client.send(get("https://secure.example.test/echo"), &opts, None).await.unwrap();
    assert_eq!(resp.meta.status, 200);
    assert_eq!(resp.meta.http_version, "HTTP/2");
    assert_eq!(proxy.hits.load(Ordering::SeqCst), 1);

    let mut no_auth = proxied(&proxy.url(), "");
    no_auth.tls.verify = false;
    let err = client.send(get("https://secure.example.test/echo"), &no_auth, None).await.unwrap_err();
    assert_eq!(err.kind, ErrorKind::Proxy);
    assert!(err.message.contains("407"));
}

#[tokio::test]
async fn unreachable_proxy_is_reported_as_proxy_error() {
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let opts = proxied(&format!("http://127.0.0.1:{port}"), "");
    let err = Client::new().send(get("http://api.example.test/"), &opts, None).await.unwrap_err();
    assert_eq!(err.kind, ErrorKind::Proxy, "{err:?}");
}

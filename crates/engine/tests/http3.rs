//! HTTP/3 (QUIC) end-to-end tests against the local testkit HTTP/3 server (UDP, 127.0.0.1).

use std::time::{Duration, Instant};

use bytes::Bytes;
use serde_json::Value;
use zorvik_engine::proxy::{ProxyMode, ProxySettings};
use zorvik_engine::{Client, CookieJar, ErrorKind, Header, HttpRequest, HttpVersionPref, RequestOptions, TlsOptions};
use zorvik_testkit::{H3TestServer, TestCerts};

fn get(url: impl Into<String>) -> HttpRequest {
    HttpRequest { method: "GET".into(), url: url.into(), headers: vec![], body: Bytes::new() }
}

fn json(body: &[u8]) -> Value {
    serde_json::from_slice(body).expect("json body")
}

fn header<'a>(echo: &'a Value, name: &str) -> Option<&'a str> {
    echo["headers"].as_array()?.iter().find(|h| h[0].as_str() == Some(name)).and_then(|h| h[1].as_str())
}

/// An HTTP/3 server plus options that trust its CA and force HTTP/3.
struct Setup {
    server: H3TestServer,
    opts: RequestOptions,
    _dir: tempfile::TempDir,
}

async fn setup() -> Setup {
    let certs = TestCerts::generate();
    let dir = tempfile::tempdir().unwrap();
    let ca = certs.write_ca(dir.path());
    let server = H3TestServer::start(&certs).await;
    let opts = RequestOptions {
        http_version: HttpVersionPref::Http3,
        tls: TlsOptions { ca_cert_path: Some(ca), ..Default::default() },
        ..Default::default()
    };
    Setup { server, opts, _dir: dir }
}

/// The client closes its QUIC connection when a request ends (or is dropped).
async fn assert_connections_close(server: &H3TestServer) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while server.open_connections() > 0 {
        assert!(Instant::now() < deadline, "QUIC connection left open");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn get_echo_with_timing_and_tls_info() {
    let Setup { server, opts, _dir } = setup().await;
    let resp = Client::new().send(get(server.url("/echo?a=1&b=two")), &opts, None).await.unwrap();
    assert_eq!(resp.meta.status, 200);
    assert_eq!(resp.meta.status_text, "OK");
    assert_eq!(resp.meta.http_version, "HTTP/3");
    assert_eq!(resp.meta.request.http_version, "HTTP/3");
    assert_eq!(resp.meta.remote_addr.as_deref(), Some(server.addr.to_string().as_str()));
    let echo = json(&resp.body);
    assert_eq!(echo["httpVersion"], "HTTP/3");
    assert_eq!(echo["args"]["b"], "two");
    assert_eq!(echo["authority"], server.addr.to_string());
    assert!(header(&echo, "user-agent").unwrap().starts_with("Zorvik/"));
    assert_eq!(header(&echo, "accept"), Some("*/*"));
    assert!(header(&echo, "accept-encoding").is_some());
    // The host travels as :authority; no Host header on the wire or in the record.
    assert!(header(&echo, "host").is_none());
    assert!(!resp.meta.request.headers.iter().any(|h| h.name.eq_ignore_ascii_case("host")));

    let tls = resp.meta.tls.expect("tls info");
    assert_eq!(tls.version, "TLS 1.3");
    assert_eq!(tls.alpn.as_deref(), Some("h3"));
    assert!(tls.cipher.starts_with("TLS13_"), "{}", tls.cipher);
    let cert = tls.certificate.expect("certificate");
    assert!(cert.subject.contains("localhost"));
    assert!(cert.subject_alt_names.contains(&"127.0.0.1".to_string()));

    // QUIC's combined handshake is reported as connect time.
    assert!(resp.timing.connect_ms > 0.0);
    assert_eq!(resp.timing.tls_ms, 0.0);
    assert!(resp.timing.ttfb_ms > 0.0);
    assert!(resp.timing.total_ms >= resp.timing.connect_ms);
    assert_connections_close(&server).await;
}

#[tokio::test]
async fn post_body_and_headers() {
    let Setup { server, opts, _dir } = setup().await;
    let req = HttpRequest {
        method: "POST".into(),
        url: server.url("/echo"),
        headers: vec![
            Header::new("Content-Type", "application/json"),
            Header::new("X-Unicode", "café"),
            Header::new("Content-Length", "999"),
            // Connection-specific headers are not allowed in HTTP/3.
            Header::new("Connection", "keep-alive"),
            Header::new("TE", "gzip"),
            // A custom Host becomes :authority.
            Header::new("Host", "api.example.test"),
        ],
        body: Bytes::from_static(br#"{"hello":"world"}"#),
    };
    let resp = Client::new().send(req, &opts, None).await.unwrap();
    let echo = json(&resp.body);
    assert_eq!(echo["method"], "POST");
    assert_eq!(echo["body"], r#"{"hello":"world"}"#);
    assert_eq!(header(&echo, "content-length"), Some("17"));
    assert_eq!(header(&echo, "x-unicode"), Some("café"));
    assert!(header(&echo, "connection").is_none());
    assert!(header(&echo, "te").is_none());
    assert_eq!(echo["authority"], "api.example.test");
    let sent: Vec<_> = resp.meta.request.headers.iter().map(|h| h.name.to_ascii_lowercase()).collect();
    assert!(!sent.contains(&"connection".to_string()) && !sent.contains(&"host".to_string()));
    assert_eq!(resp.meta.request.body_size, 17);

    // A large upload arrives intact.
    let big = vec![b'z'; 300_000];
    let req = HttpRequest { method: "PUT".into(), body: Bytes::from(big), ..get(server.url("/echo")) };
    let echo = json(&Client::new().send(req, &opts, None).await.unwrap().body);
    assert_eq!(echo["bodyLength"], 300_000);
}

#[tokio::test]
async fn status_codes() {
    let Setup { server, opts, _dir } = setup().await;
    let client = Client::new();
    for (code, text) in [(404, "Not Found"), (500, "Internal Server Error"), (204, "No Content"), (418, "I'm a teapot")]
    {
        let resp = client.send(get(server.url(&format!("/status/{code}"))), &opts, None).await.unwrap();
        assert_eq!(resp.meta.status, code);
        assert_eq!(resp.meta.status_text, text);
        assert!(resp.body.is_empty());
    }
}

#[tokio::test]
async fn redirects_stay_on_http3() {
    let Setup { server, opts, _dir } = setup().await;
    let client = Client::new();
    let resp = client.send(get(server.url("/redirect/3")), &opts, None).await.unwrap();
    assert_eq!(resp.meta.status, 200);
    assert_eq!(resp.body, b"redirect chain done");
    assert_eq!(resp.meta.redirects.len(), 3);
    assert!(resp.meta.url.ends_with("/redirect/0"));
    assert_eq!(resp.meta.http_version, "HTTP/3");
    assert!(resp.timing.redirect_ms > 0.0);

    // 303 turns POST into GET without a body.
    let req = HttpRequest {
        method: "POST".into(),
        url: server.url("/redirect-to?url=/echo&status=303"),
        headers: vec![Header::new("Content-Type", "text/plain")],
        body: Bytes::from_static(b"payload"),
    };
    let echo = json(&client.send(req, &opts, None).await.unwrap().body);
    assert_eq!(echo["method"], "GET");
    assert_eq!(echo["body"], "");
    assert_eq!(echo["httpVersion"], "HTTP/3");

    let no_follow = RequestOptions { follow_redirects: false, ..opts.clone() };
    let resp = client.send(get(server.url("/redirect/3")), &no_follow, None).await.unwrap();
    assert_eq!(resp.meta.status, 302);
    let limited = RequestOptions { max_redirects: 2, ..opts.clone() };
    let err = client.send(get(server.url("/redirect/3")), &limited, None).await.unwrap_err();
    assert_eq!(err.kind, ErrorKind::TooManyRedirects);
    assert_connections_close(&server).await;
}

#[tokio::test]
async fn decodes_gzip_and_caps_body() {
    let Setup { server, opts, _dir } = setup().await;
    let client = Client::new();
    let resp = client.send(get(server.url("/gzip")), &opts, None).await.unwrap();
    assert_eq!(json(&resp.body)["compressed"], "gzip");
    assert!(resp.decode_warning.is_none());
    let raw = RequestOptions { decompress: false, ..opts.clone() };
    let resp = client.send(get(server.url("/gzip")), &raw, None).await.unwrap();
    assert!(serde_json::from_slice::<Value>(&resp.body).is_err());

    let capped = RequestOptions { max_body_bytes: 1000, ..opts.clone() };
    let resp = client.send(get(server.url("/bytes/50000")), &capped, None).await.unwrap();
    assert_eq!(resp.body.len(), 1000);
    assert!(resp.body_truncated);

    let resp = client.send(get(server.url("/bytes/1000000")), &opts, None).await.unwrap();
    assert_eq!(resp.body.len(), 1_000_000);
    assert!(!resp.body_truncated);
    assert_eq!(resp.body_wire_size, 1_000_000);
    assert!(resp.body.iter().enumerate().all(|(i, b)| *b == b'a' + (i % 26) as u8));
    assert_connections_close(&server).await;
}

#[tokio::test]
async fn cookies_are_stored_and_sent() {
    let Setup { server, opts, _dir } = setup().await;
    let client = Client::new();
    let jar = CookieJar::new();
    let resp = client.send(get(server.url("/cookies/set?session=abc&theme=dark")), &opts, Some(&jar)).await.unwrap();
    assert_eq!(resp.meta.cookies.len(), 2);
    let resp = client.send(get(server.url("/cookies")), &opts, Some(&jar)).await.unwrap();
    assert_eq!(json(&resp.body)["cookies"]["session"], "abc");
}

#[tokio::test]
async fn timeout_and_cancellation_close_the_connection() {
    let Setup { server, opts, _dir } = setup().await;
    let client = Client::new();
    let short = RequestOptions { timeout: Some(Duration::from_millis(300)), ..opts.clone() };
    let err = client.send(get(server.url("/delay/3000")), &short, None).await.unwrap_err();
    assert_eq!(err.kind, ErrorKind::Timeout);
    assert!(err.message.contains("300ms"));
    assert_connections_close(&server).await;

    // Dropping the request future (what "Cancel" does) closes the connection too.
    let unlimited = RequestOptions { timeout: None, ..opts.clone() };
    let dropped =
        tokio::time::timeout(Duration::from_millis(300), client.send(get(server.url("/delay/5000")), &unlimited, None))
            .await;
    assert!(dropped.is_err());
    assert_connections_close(&server).await;
}

#[tokio::test]
async fn untrusted_certificate_errors_with_hint_and_can_be_skipped() {
    let Setup { server, opts, _dir } = setup().await;
    let client = Client::new();
    let no_ca = RequestOptions { tls: TlsOptions::default(), ..opts.clone() };
    let err = client.send(get(server.url("/echo")), &no_ca, None).await.unwrap_err();
    assert_eq!(err.kind, ErrorKind::Tls, "{err:?}");
    assert!(err.message.contains("not trusted"), "{}", err.message);

    let insecure = RequestOptions { tls: TlsOptions { verify: false, ..Default::default() }, ..opts.clone() };
    let resp = client.send(get(server.url("/echo")), &insecure, None).await.unwrap();
    assert_eq!(resp.meta.status, 200);
    assert!(resp.meta.tls.unwrap().certificate.is_some());
}

#[tokio::test]
async fn server_without_http3_alpn() {
    let certs = TestCerts::generate();
    let server = H3TestServer::bind_with_alpn("127.0.0.1:0".parse().unwrap(), &certs, &[b"hq-interop"]).await;
    let opts = RequestOptions {
        http_version: HttpVersionPref::Http3,
        tls: TlsOptions { verify: false, ..Default::default() },
        ..Default::default()
    };
    let err = Client::new().send(get(server.url("/")), &opts, None).await.unwrap_err();
    assert_eq!(err.kind, ErrorKind::Protocol, "{err:?}");
    assert!(err.message.contains("not HTTP/3"), "{}", err.message);
}

#[tokio::test]
async fn silent_udp_port_times_out_with_hint() {
    // Bound but never answering: packets are swallowed, like a firewall dropping UDP.
    let silent = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let url = format!("https://{}/", silent.local_addr().unwrap());
    let opts = RequestOptions {
        http_version: HttpVersionPref::Http3,
        connect_timeout: Duration::from_millis(400),
        ..Default::default()
    };
    let started = Instant::now();
    let err = Client::new().send(get(url), &opts, None).await.unwrap_err();
    assert_eq!(err.kind, ErrorKind::Connect, "{err:?}");
    assert!(err.message.contains("UDP port"), "{}", err.message);
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[tokio::test]
async fn localhost_races_address_families() {
    // `localhost` usually resolves to ::1 first; the server only listens on 127.0.0.1.
    let Setup { server, opts, _dir } = setup().await;
    let url = format!("https://localhost:{}/echo", server.addr.port());
    let started = Instant::now();
    let resp = Client::new().send(get(url), &opts, None).await.unwrap();
    assert_eq!(resp.meta.status, 200);
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[tokio::test]
async fn proxy_plain_http_and_invalid_requests_fail_clearly() {
    let Setup { server, opts, _dir } = setup().await;
    let client = Client::new();
    let proxied = RequestOptions {
        proxy: ProxySettings::from_mode(&ProxyMode::Manual { url: "http://127.0.0.1:9".into(), bypass: String::new() })
            .unwrap(),
        ..opts.clone()
    };
    let err = client.send(get("https://h3.example.test/"), &proxied, None).await.unwrap_err();
    assert_eq!(err.kind, ErrorKind::Proxy, "{err:?}");
    assert!(err.message.contains("HTTP/3 can't go through an HTTP proxy"));
    // Loopback is always direct, so the proxy does not get in the way there.
    assert_eq!(client.send(get(server.url("/echo")), &proxied, None).await.unwrap().meta.status, 200);

    let err = client.send(get(format!("http://{}/echo", server.addr)), &opts, None).await.unwrap_err();
    assert_eq!(err.kind, ErrorKind::InvalidRequest);
    assert!(err.message.contains("https://"));

    // Bad headers are rejected before any packet is sent (nothing listens on port 9).
    let bad = HttpRequest { headers: vec![Header::new("X-Bad", "a\r\nb")], ..get("https://127.0.0.1:9/") };
    let err = client.send(bad, &opts, None).await.unwrap_err();
    assert_eq!(err.kind, ErrorKind::InvalidRequest);

    let err = client.send(get("https://does-not-exist.invalid/"), &opts, None).await.unwrap_err();
    assert_eq!(err.kind, ErrorKind::Dns, "{err:?}");
}

#[tokio::test]
async fn event_stream_over_http3() {
    let Setup { server, opts, _dir } = setup().await;
    let mut stream = Client::new().open_stream(get(server.url("/sse?count=3&interval=20")), &opts, None).await.unwrap();
    assert_eq!(stream.meta.http_version, "HTTP/3");
    let mut text = String::new();
    while let Some(chunk) = stream.body.next_chunk().await {
        text.push_str(&String::from_utf8_lossy(&chunk.unwrap()));
    }
    assert!(text.contains("id: 0") && text.contains("id: 2"), "{text}");
    drop(stream);
    assert_connections_close(&server).await;
}

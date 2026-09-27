//! Pooled client for load tests: connection reuse (HTTP/1.1 keep-alive and
//! HTTP/2 multiplexing), new connection per request, error kinds, byte counts.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use bytes::Bytes;
use futures_util::future::join_all;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use zorvik_engine::pool::{Exchange, MAX_STREAMS, PooledClient};
use zorvik_engine::proxy::{ProxyMode, ProxySettings};
use zorvik_engine::{ErrorKind, HttpRequest, HttpVersionPref, RequestOptions, TlsOptions};
use zorvik_testkit::{TestCerts, TestProxy, TestServer};

fn get(url: impl Into<String>) -> HttpRequest {
    HttpRequest { method: "GET".into(), url: url.into(), headers: vec![], body: Bytes::new() }
}

/// Forwards TCP connections to `upstream` and counts them.
async fn counting_relay(upstream: SocketAddr) -> (SocketAddr, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let count = Arc::new(AtomicUsize::new(0));
    let accepted = count.clone();
    tokio::spawn(async move {
        while let Ok((mut client, _)) = listener.accept().await {
            accepted.fetch_add(1, Ordering::SeqCst);
            tokio::spawn(async move {
                if let Ok(mut server) = TcpStream::connect(upstream).await {
                    let _ = tokio::io::copy_bidirectional(&mut client, &mut server).await;
                }
            });
        }
    });
    (addr, count)
}

async fn send_many(client: &PooledClient, url: &str, n: usize) -> Vec<Exchange> {
    let req = client.prepare(get(url)).unwrap();
    join_all((0..n).map(|_| client.send(&req, Instant::now()))).await
}

fn opts(version: HttpVersionPref) -> RequestOptions {
    RequestOptions { http_version: version, ..Default::default() }
}

#[tokio::test(flavor = "multi_thread")]
async fn http1_reuses_keep_alive_connections() {
    let server = TestServer::start().await;
    let (relay, accepted) = counting_relay(server.addr).await;
    let client = PooledClient::new(opts(HttpVersionPref::Auto), true).unwrap();
    let req = client.prepare(get(format!("http://{relay}/status/200"))).unwrap();
    for i in 0..20 {
        let e = client.send(&req, Instant::now()).await;
        assert_eq!(e.status, Some(200), "{:?}", e.error);
        assert_eq!(e.new_connection, i == 0);
    }
    assert_eq!(accepted.load(Ordering::SeqCst), 1);
    assert_eq!(client.connections_opened(), 1);

    // Concurrent requests need a connection each, and then reuse them.
    for _ in 0..3 {
        let results = send_many(&client, &format!("http://{relay}/delay/20"), 8).await;
        assert!(results.iter().all(|e| e.status == Some(200)));
    }
    let opened = accepted.load(Ordering::SeqCst);
    assert!((2..=8).contains(&opened), "{opened} connections");
    assert_eq!(client.connections_opened() as usize, opened);
}

#[tokio::test(flavor = "multi_thread")]
async fn http2_multiplexes_requests_on_one_connection() {
    let server = TestServer::start().await;
    let (relay, accepted) = counting_relay(server.addr).await;
    // h2c (prior knowledge) on plain http.
    let client = PooledClient::new(opts(HttpVersionPref::Http2), true).unwrap();
    let started = Instant::now();
    let results = send_many(&client, &format!("http://{relay}/delay/200"), 50).await;
    assert!(results.iter().all(|e| e.status == Some(200)), "{:?}", results[0].error);
    assert_eq!(results.iter().filter(|e| e.new_connection).count(), 1);
    assert_eq!(accepted.load(Ordering::SeqCst), 1);
    // All 50 ran at once on that connection.
    assert!(started.elapsed() < Duration::from_millis(1500), "{:?}", started.elapsed());

    // More requests than one connection's streams: a second connection.
    let results = send_many(&client, &format!("http://{relay}/delay/300"), MAX_STREAMS + 50).await;
    assert!(results.iter().all(|e| e.status == Some(200)));
    assert_eq!(accepted.load(Ordering::SeqCst), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn https_auto_negotiates_http2_and_shares_the_connection() {
    let certs = TestCerts::generate();
    let dir = tempfile::tempdir().unwrap();
    let server = TestServer::start_tls(&certs).await;
    let (relay, accepted) = counting_relay(server.addr).await;
    let tls = TlsOptions { ca_cert_path: Some(certs.write_ca(dir.path())), ..Default::default() };
    let client = PooledClient::new(RequestOptions { tls, ..Default::default() }, true).unwrap();
    // A burst before the protocol is known still makes a single connection.
    let results = send_many(&client, &format!("https://localhost:{}/delay/100", relay.port()), 30).await;
    assert!(results.iter().all(|e| e.status == Some(200)), "{:?}", results[0].error);
    assert_eq!(accepted.load(Ordering::SeqCst), 1);

    // HTTP/1.1 only: one connection per concurrent request.
    let client = PooledClient::new(
        RequestOptions {
            http_version: HttpVersionPref::Http1,
            tls: client_tls(&certs, dir.path()),
            ..Default::default()
        },
        true,
    )
    .unwrap();
    let results = send_many(&client, &format!("https://localhost:{}/delay/100", server.addr.port()), 5).await;
    assert!(results.iter().all(|e| e.status == Some(200)));
    assert_eq!(client.connections_opened(), 5);
}

fn client_tls(certs: &TestCerts, dir: &std::path::Path) -> TlsOptions {
    TlsOptions { ca_cert_path: Some(certs.write_ca(dir)), ..Default::default() }
}

#[tokio::test(flavor = "multi_thread")]
async fn keep_alive_off_opens_a_connection_per_request() {
    let server = TestServer::start().await;
    let (relay, accepted) = counting_relay(server.addr).await;
    for version in [HttpVersionPref::Http1, HttpVersionPref::Http2] {
        accepted.store(0, Ordering::SeqCst);
        let client = PooledClient::new(opts(version), false).unwrap();
        let req = client.prepare(get(format!("http://{relay}/status/204"))).unwrap();
        for _ in 0..10 {
            let e = client.send(&req, Instant::now()).await;
            assert_eq!(e.status, Some(204), "{:?}", e.error);
            assert!(e.new_connection);
        }
        assert_eq!(accepted.load(Ordering::SeqCst), 10);
        assert_eq!(client.connections_opened(), 10);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn failures_have_a_kind() {
    let server = TestServer::start().await;
    let fast = RequestOptions { timeout: Some(Duration::from_millis(200)), ..Default::default() };
    let client = PooledClient::new(fast, true).unwrap();
    let e = client.send(&client.prepare(get(server.url("/delay/3000"))).unwrap(), Instant::now()).await;
    assert_eq!(e.error.as_ref().map(|e| e.kind), Some(ErrorKind::Timeout));
    assert!(e.status.is_none());
    assert!(e.latency >= Duration::from_millis(200) && e.latency < Duration::from_secs(2), "{:?}", e.latency);

    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap();
    let client = PooledClient::new(RequestOptions::default(), true).unwrap();
    let e = client.send(&client.prepare(get(format!("http://{closed}/"))).unwrap(), Instant::now()).await;
    assert_eq!(e.error.as_ref().map(|e| e.kind), Some(ErrorKind::Connect));
    assert!(!e.new_connection);

    // A certificate the OS does not trust.
    let tls_server = TestServer::start_tls(&TestCerts::generate()).await;
    let e = client.send(&client.prepare(get(tls_server.url("/"))).unwrap(), Instant::now()).await;
    assert_eq!(e.error.as_ref().map(|e| e.kind), Some(ErrorKind::Tls));

    assert!(client.prepare(get("ftp://x/")).is_err());
    let err = PooledClient::new(opts(HttpVersionPref::Http3), true).err().unwrap();
    assert!(err.message.contains("HTTP/3"), "{}", err.message);
}

#[tokio::test(flavor = "multi_thread")]
async fn counts_bytes_measures_from_start_and_does_not_follow_redirects() {
    let server = TestServer::start().await;
    let client = PooledClient::new(RequestOptions::default(), true).unwrap();
    let e = client.send(&client.prepare(get(server.url("/bytes/5000"))).unwrap(), Instant::now()).await;
    assert_eq!(e.status, Some(200));
    assert!(e.bytes_in > 5000 && e.bytes_in < 5400, "{}", e.bytes_in);
    assert!(e.bytes_out > 30, "{}", e.bytes_out);

    let post = HttpRequest {
        method: "POST".into(),
        url: server.url("/echo"),
        headers: vec![],
        body: Bytes::from(vec![b'x'; 1000]),
    };
    let e = client.send(&client.prepare(post).unwrap(), Instant::now()).await;
    assert_eq!(e.status, Some(200));
    assert!(e.bytes_out > 1000);

    let e = client.send(&client.prepare(get(server.url("/redirect/1"))).unwrap(), Instant::now()).await;
    assert_eq!(e.status, Some(302));

    // Latency from a scheduled start in the past includes the wait; TTFB does not.
    let e = client
        .send(&client.prepare(get(server.url("/status/200"))).unwrap(), Instant::now() - Duration::from_millis(150))
        .await;
    assert!(e.latency >= Duration::from_millis(150), "{:?}", e.latency);
    assert!(e.ttfb.unwrap() < Duration::from_millis(150));
}

/// Answers the first request on each connection and closes the connection
/// when the next one arrives, like a server whose keep-alive timeout just ran out.
async fn closes_reused_connections() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 1024];
                while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    match socket.read(&mut chunk).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => buf.extend_from_slice(&chunk[..n]),
                    }
                }
                let _ = socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok").await;
                // Wait for a second request, then close without answering.
                let _ = socket.read(&mut chunk).await;
            });
        }
    });
    addr
}

#[tokio::test(flavor = "multi_thread")]
async fn retries_on_a_new_connection_when_a_reused_one_was_closed() {
    let addr = closes_reused_connections().await;
    let client = PooledClient::new(RequestOptions::default(), true).unwrap();
    let req = client.prepare(get(format!("http://{addr}/"))).unwrap();
    for _ in 0..5 {
        let e = client.send(&req, Instant::now()).await;
        assert_eq!(e.status, Some(200), "{:?}", e.error);
        assert!(e.new_connection);
    }
    assert_eq!(client.connections_opened(), 5);
}

#[tokio::test(flavor = "multi_thread")]
async fn tunnels_through_a_proxy_once_per_connection() {
    let server = TestServer::start().await;
    let proxy = TestProxy::start_with_upstream(None, Some(server.addr)).await;
    let proxy_settings =
        ProxySettings::from_mode(&ProxyMode::Manual { url: proxy.url(), bypass: String::new() }).unwrap();
    let client = PooledClient::new(RequestOptions { proxy: proxy_settings, ..Default::default() }, true).unwrap();
    let req = client.prepare(get("http://load.example.test/status/200")).unwrap();
    for _ in 0..5 {
        let e = client.send(&req, Instant::now()).await;
        assert_eq!(e.status, Some(200), "{:?}", e.error);
    }
    assert_eq!(proxy.hits.load(Ordering::SeqCst), 1);
}

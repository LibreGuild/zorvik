//! TCP relay: both directions, half-close, injected sends, disconnect,
//! unreachable and TLS targets, bad targets.

mod common;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use common::{Events, start};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use zorvik_engine::{Client, RequestOptions, TlsOptions};
use zorvik_formats::{Server, ServerKind, ServerTls, TcpProxyConfig};
use zorvik_servers::{OutgoingMessage, Reporter, RunningServer, StartOptions, TrafficDirection, TrafficKind};

fn relay(target: &str, upstream_tls: bool) -> Server {
    let mut server = Server::new("Relay", ServerKind::TcpProxy);
    server.port = 0;
    server.proxy = TcpProxyConfig { target: target.into(), upstream_tls };
    server
}

/// A target that echoes until the client stops sending, then says "bye" and closes.
async fn echo_target() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut sock, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buf = [0u8; 1024];
                loop {
                    match sock.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if sock.write_all(&buf[..n]).await.is_err() {
                                return;
                            }
                        }
                    }
                }
                let _ = sock.write_all(b"bye").await;
            });
        }
    });
    addr
}

async fn read_some(stream: &mut TcpStream) -> Vec<u8> {
    let mut buf = vec![0u8; 1024];
    let n = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buf)).await.expect("data in time").unwrap();
    buf.truncate(n);
    buf
}

async fn read_to_end(stream: &mut TcpStream) -> Vec<u8> {
    let mut all = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut all)).await.expect("end in time").unwrap();
    all
}

#[tokio::test]
async fn relays_both_ways_with_half_close() {
    let target = echo_target().await;
    let (running, events) = start(relay(&target.to_string(), false)).await;
    assert!(running.url.starts_with("tcp://127.0.0.1:"), "{}", running.url);

    let mut client = TcpStream::connect(running.addr).await.unwrap();
    client.write_all(b"hello").await.unwrap();
    assert_eq!(read_some(&mut client).await, b"hello");
    // The client stops sending; the target still answers, then closes.
    client.shutdown().await.unwrap();
    assert_eq!(read_to_end(&mut client).await, b"bye");

    let close = events.wait_kind(TrafficKind::Close).await;
    assert_eq!(close.conn, Some(1));
    assert!(close.summary.contains("client closed"), "{}", close.summary);
    let traffic = events.traffic();
    let to: Vec<_> = traffic.iter().filter(|e| e.direction == Some(TrafficDirection::ToTarget)).collect();
    let from: Vec<_> = traffic.iter().filter(|e| e.direction == Some(TrafficDirection::FromTarget)).collect();
    assert_eq!(to.iter().map(|e| e.text.clone().unwrap()).collect::<String>(), "hello");
    assert_eq!(from.iter().map(|e| e.text.clone().unwrap()).collect::<String>(), "hellobye");
    assert!(traffic.iter().any(|e| e.kind == TrafficKind::Info && e.summary.contains(&target.to_string())));
}

#[tokio::test]
async fn target_closing_first_and_injected_sends() {
    // Greets, then closes its side.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let target = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut sock, _)) = listener.accept().await {
            tokio::spawn(async move {
                let _ = sock.write_all(b"220 ready\r\n").await;
                let _ = sock.shutdown().await;
                // Keep reading until the client is gone.
                let mut buf = [0u8; 64];
                while matches!(sock.read(&mut buf).await, Ok(n) if n > 0) {}
            });
        }
    });
    let (running, events) = start(relay(&target.to_string(), false)).await;

    let mut client = TcpStream::connect(running.addr).await.unwrap();
    assert_eq!(read_to_end(&mut client).await, b"220 ready\r\n");
    client.write_all(b"QUIT\r\n").await.unwrap();
    client.shutdown().await.unwrap();
    let close = events.wait_kind(TrafficKind::Close).await;
    assert!(close.summary.contains("target closed"), "{}", close.summary);

    // Once the target has closed its side, nothing more can be sent to the client.
    let mut second = TcpStream::connect(running.addr).await.unwrap();
    assert_eq!(read_to_end(&mut second).await, b"220 ready\r\n");
    running.send(Some(2), OutgoingMessage::Text { text: "late".into() }).await.unwrap();
    events.wait("not sent", |e| e.kind == TrafficKind::Error && e.conn == Some(2)).await;
    // Disconnect closes the pair.
    running.disconnect(2);
    let closed = events.wait("second close", |e| e.kind == TrafficKind::Close && e.conn == Some(2)).await;
    assert!(closed.summary.contains("closed by you"), "{}", closed.summary);
}

#[tokio::test]
async fn injected_sends_reach_the_client() {
    let target = echo_target().await;
    let (running, events) = start(relay(&target.to_string(), false)).await;
    let mut client = TcpStream::connect(running.addr).await.unwrap();
    client.write_all(b"ping").await.unwrap();
    assert_eq!(read_some(&mut client).await, b"ping");
    assert_eq!(running.send(None, OutgoingMessage::Binary { base64: "AQI=".into() }).await.unwrap(), 1);
    assert_eq!(read_some(&mut client).await, [1, 2]);
    let out = events.wait("out", |e| e.direction == Some(TrafficDirection::Out)).await;
    assert_eq!((out.conn, out.size), (Some(1), 2));
}

#[tokio::test]
async fn unreachable_and_self_targets_close_the_client() {
    let closed_port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap();
    let mut server = relay(&closed_port.to_string(), false);
    let (running, events) = start(server.clone()).await;
    let mut client = TcpStream::connect(running.addr).await.unwrap();
    assert_eq!(read_to_end(&mut client).await, b"");
    let error = events.wait_kind(TrafficKind::Error).await;
    assert!(error.summary.contains("Could not connect to the target"), "{}", error.summary);
    let close = events.wait_kind(TrafficKind::Close).await;
    assert!(close.summary.contains("target unreachable"), "{}", close.summary);

    // Edits apply to the next connection: pointing the relay at itself is refused.
    server.proxy.target = format!("localhost:{}", running.addr.port());
    assert!(running.update(server, Default::default()));
    let mut client = TcpStream::connect(running.addr).await.unwrap();
    assert_eq!(read_to_end(&mut client).await, b"");
    events.wait("self", |e| e.kind == TrafficKind::Error && e.summary.contains("this relay itself")).await;
    assert_eq!(events.traffic().iter().filter(|e| e.kind == TrafficKind::Open).count(), 2);
}

#[tokio::test]
async fn missing_or_invalid_target_is_refused_at_start() {
    for (target, expected) in
        [("", "Set the target"), ("http://x:1", "Invalid target"), ("host-without-port", "no port")]
    {
        let mut server = relay(target, false);
        server.port = 0;
        let reporter = zorvik_servers::Reporter::new(|_| {});
        let error = zorvik_servers::start(server, Default::default(), common::options(), reporter).await.err().unwrap();
        assert!(error.message.contains(expected), "{target}: {}", error.message);
    }
}

/// Start `server` with the given TLS verification setting for the relay's own connections.
async fn start_verifying(mut server: Server, verify: bool) -> (RunningServer, Events) {
    server.port = 0;
    let events = Events::default();
    let sink = events.clone();
    let reporter = Reporter::new(move |e| sink.0.lock().unwrap().push(e));
    let options = StartOptions {
        base_dir: std::env::temp_dir(),
        client: Arc::new(Client::new()),
        request_options: RequestOptions { tls: TlsOptions { verify, ..Default::default() }, ..Default::default() },
    };
    let running = zorvik_servers::start(server, Default::default(), options, reporter).await.expect("server starts");
    (running, events)
}

#[tokio::test]
async fn tls_to_the_target() {
    // A TLS echo server with a self-signed certificate.
    let mut target = Server::new("Secure echo", ServerKind::Tcp);
    target.tls = ServerTls { enabled: true, ..Default::default() };
    let (target, _) = start(target).await;
    let target_addr = format!("localhost:{}", target.addr.port());

    let (running, events) = start_verifying(relay(&target_addr, true), false).await;
    let mut client = TcpStream::connect(running.addr).await.unwrap();
    client.write_all(b"secret").await.unwrap();
    assert_eq!(read_some(&mut client).await, b"secret");
    let info = events.wait_kind(TrafficKind::Info).await;
    assert!(info.summary.contains("over TLS"), "{}", info.summary);
    let to = events.wait("to target", |e| e.direction == Some(TrafficDirection::ToTarget)).await;
    assert_eq!(to.text.as_deref(), Some("secret"));

    // With verification on, the self-signed certificate is refused.
    let (running, events) = start_verifying(relay(&target_addr, true), true).await;
    let mut client = TcpStream::connect(running.addr).await.unwrap();
    assert_eq!(read_to_end(&mut client).await, b"");
    let close = events.wait_kind(TrafficKind::Close).await;
    assert!(close.summary.contains("target unreachable") && close.summary.contains("TLS"), "{}", close.summary);
}

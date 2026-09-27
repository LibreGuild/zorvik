//! TCP server: echo, rules, greeting, manual sends, disconnect, TLS, stop.

mod common;

use std::time::Duration;

use common::start;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use zorvik_engine::framing::{Framing, LineEnding};
use zorvik_engine::{Client, Direction, RequestOptions, SocketConfig, SocketEvent, SocketOutgoing, TlsOptions};
use zorvik_formats::{MatchKind, ReplyMode, ReplyRule, Server, ServerKind, ServerTls};
use zorvik_servers::{OutgoingMessage, TrafficDirection, TrafficKind};

async fn read_line(r: &mut BufReader<TcpStream>) -> String {
    let mut line = String::new();
    tokio::time::timeout(Duration::from_secs(5), r.read_line(&mut line)).await.expect("line in time").unwrap();
    line
}

#[tokio::test]
async fn echo_greeting_and_rules_with_line_framing() {
    let mut server = Server::new("Echo", ServerKind::Tcp);
    server.port = 0;
    server.socket.framing = Framing::Line;
    server.socket.greeting = "hello {{$uuid}}".into();
    let (running, events) = start(server.clone()).await;
    assert!(running.url.starts_with("tcp://127.0.0.1:"), "{}", running.url);

    let mut c = BufReader::new(TcpStream::connect(running.addr).await.unwrap());
    assert!(read_line(&mut c).await.starts_with("hello "));
    c.get_mut().write_all(b"one\r\ntwo\n").await.unwrap();
    assert_eq!(read_line(&mut c).await, "one\n");
    assert_eq!(read_line(&mut c).await, "two\n");

    // Switch to rules without a restart.
    server.socket.mode = ReplyMode::Rules;
    server.socket.rules = vec![
        ReplyRule { matcher: MatchKind::Exact, pattern: "PING".into(), reply: "PONG".into(), ..Default::default() },
        ReplyRule {
            matcher: MatchKind::Regex,
            pattern: "^echo ".into(),
            reply: "you said {{message}}".into(),
            ..Default::default()
        },
        ReplyRule { matcher: MatchKind::Regex, pattern: "(".into(), reply: "never".into(), ..Default::default() },
    ];
    assert!(running.update(server.clone(), Default::default()));
    c.get_mut().write_all(b"PING\nunknown\necho hi\n").await.unwrap();
    assert_eq!(read_line(&mut c).await, "PONG\n");
    assert_eq!(read_line(&mut c).await, "you said echo hi\n");
    // The unusable rule is reported (once).
    let bad = events.wait("rule problem", |e| e.kind == TrafficKind::Error && e.summary.contains("regex")).await;
    assert!(bad.summary.starts_with("Rule 3"), "{}", bad.summary);

    // Port changes need a restart.
    assert!(!running.update(Server { port: 1, ..server }, Default::default()));

    let input =
        events.wait("PING", |e| e.direction == Some(TrafficDirection::In) && e.text.as_deref() == Some("PING")).await;
    assert_eq!(input.conn, Some(1));
    let stats = events.traffic();
    assert!(stats.iter().any(|e| e.kind == TrafficKind::Open));
}

#[tokio::test]
async fn manual_send_broadcast_and_disconnect() {
    let mut server = Server::new("Manual", ServerKind::Tcp);
    server.socket.mode = ReplyMode::Manual;
    let (running, events) = start(server).await;
    assert_eq!(
        running.send(None, OutgoingMessage::Text { text: "x".into() }).await.unwrap_err().message,
        "No client is connected"
    );

    let mut a = TcpStream::connect(running.addr).await.unwrap();
    let mut b = TcpStream::connect(running.addr).await.unwrap();
    for _ in 0..100 {
        if events.traffic().iter().filter(|e| e.kind == TrafficKind::Open).count() == 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(running.send(None, OutgoingMessage::Text { text: "all".into() }).await.unwrap(), 2);
    assert_eq!(running.send(Some(2), OutgoingMessage::Binary { base64: "AQI=".into() }).await.unwrap(), 1);
    let mut buf = [0u8; 16];
    let n = tokio::time::timeout(Duration::from_secs(5), a.read(&mut buf)).await.expect("data in time").unwrap();
    assert_eq!(&buf[..n], b"all");
    let mut got = Vec::new();
    while got.len() < 5 {
        let n = tokio::time::timeout(Duration::from_secs(5), b.read(&mut buf)).await.expect("data in time").unwrap();
        got.extend_from_slice(&buf[..n]);
    }
    assert_eq!(got, [b'a', b'l', b'l', 1, 2]);

    running.disconnect(1);
    assert_eq!(a.read(&mut buf).await.unwrap(), 0);
    let close = events.wait_kind(TrafficKind::Close).await;
    assert!(close.summary.contains("closed by you"), "{}", close.summary);
    // Manual mode does not answer.
    b.write_all(b"hi").await.unwrap();
    events.wait("hi", |e| e.text.as_deref() == Some("hi")).await;
    assert!(tokio::time::timeout(Duration::from_millis(200), b.read(&mut buf)).await.is_err());
}

#[tokio::test]
async fn tls_listener_and_stop_frees_the_port() {
    let mut server = Server::new("Secure", ServerKind::Tcp);
    server.tls = ServerTls { enabled: true, ..Default::default() };
    let (running, events) = start(server).await;
    assert!(running.url.starts_with("tls://"));
    let opts = RequestOptions { tls: TlsOptions { verify: false, ..Default::default() }, ..Default::default() };
    let conn = Client::new()
        .tcp(
            &format!("tls://localhost:{}", running.addr.port()),
            &opts,
            SocketConfig { framing: Framing::Line, line_ending: LineEnding::Lf, ..Default::default() },
        )
        .await
        .unwrap();
    assert!(conn.opened.protocol.starts_with("TCP + TLS"), "{}", conn.opened.protocol);
    let mut ev = conn.events;
    conn.session.send(SocketOutgoing::Text { text: "secret".into() }).unwrap();
    let mut echoed = false;
    while let Ok(Some(e)) = tokio::time::timeout(Duration::from_secs(5), ev.recv()).await {
        if let SocketEvent::Message { direction: Direction::Received, text, .. } = e {
            assert_eq!(text.as_deref(), Some("secret"));
            echoed = true;
            break;
        }
    }
    assert!(echoed);

    let addr = running.addr;
    running.stop();
    for _ in 0..100 {
        if events.stopped().is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(events.stopped(), Some(None));
    // The port can be used again right away.
    tokio::net::TcpListener::bind(addr).await.expect("port is free");
}

#[tokio::test]
async fn port_in_use_is_explained() {
    let taken = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut server = Server::new("Busy", ServerKind::Tcp);
    server.port = taken.local_addr().unwrap().port();
    let reporter = zorvik_servers::Reporter::new(|_| {});
    let err = zorvik_servers::start(server, Default::default(), common::options(), reporter).await.err().unwrap();
    assert!(err.message.contains("already in use"), "{}", err.message);
}

/// A client with a tiny receive window that never reads.
async fn stalled_client(addr: std::net::SocketAddr) -> TcpStream {
    let socket = tokio::net::TcpSocket::new_v4().unwrap();
    socket.set_recv_buffer_size(4096).unwrap();
    socket.connect(addr).await.unwrap()
}

#[tokio::test]
async fn clients_that_do_not_read_still_close_on_disconnect_and_stop() {
    let mut server = Server::new("Manual", ServerKind::Tcp);
    server.socket.mode = ReplyMode::Manual;
    let (running, events) = start(server).await;
    let _a = stalled_client(running.addr).await;
    let _b = stalled_client(running.addr).await;
    for _ in 0..100 {
        if events.traffic().iter().filter(|e| e.kind == TrafficKind::Open).count() == 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    // More than the socket buffers hold: both writes block.
    let big = "x".repeat(16 << 20);
    for conn in [1, 2] {
        assert_eq!(running.send(Some(conn), OutgoingMessage::Text { text: big.clone() }).await.unwrap(), 1);
    }
    tokio::time::sleep(Duration::from_millis(200)).await;

    running.disconnect(1);
    let first = events.wait("#1 closed", |e| e.kind == TrafficKind::Close && e.conn == Some(1)).await;
    assert!(first.summary.contains("closed by you"), "{}", first.summary);
    running.stop();
    let second = events.wait("#2 closed", |e| e.kind == TrafficKind::Close && e.conn == Some(2)).await;
    assert!(second.summary.contains("server stopped"), "{}", second.summary);
}

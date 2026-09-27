//! UDP server: echo, rules (with delay), manual sends, forgetting peers, stop.

mod common;

use std::net::SocketAddr;
use std::time::Duration;

use common::start;
use tokio::net::UdpSocket;
use zorvik_engine::framing::{LineEnding, PayloadEncoding};
use zorvik_formats::{MatchKind, ReplyMode, ReplyRule, Server, ServerKind};
use zorvik_servers::{OutgoingMessage, TrafficDirection, TrafficKind};

async fn recv(socket: &UdpSocket) -> (Vec<u8>, SocketAddr) {
    let mut buf = vec![0u8; 70_000];
    let (n, from) = tokio::time::timeout(Duration::from_secs(5), socket.recv_from(&mut buf))
        .await
        .expect("datagram in time")
        .unwrap();
    (buf[..n].to_vec(), from)
}

async fn nothing(socket: &UdpSocket) {
    let mut buf = [0u8; 64];
    assert!(tokio::time::timeout(Duration::from_millis(200), socket.recv_from(&mut buf)).await.is_err());
}

async fn client() -> UdpSocket {
    UdpSocket::bind("127.0.0.1:0").await.unwrap()
}

#[tokio::test]
async fn echo_then_rules_without_restart() {
    let mut server = Server::new("Echo", ServerKind::Udp);
    server.port = 0;
    // Framing does not apply to UDP: one datagram is one message.
    server.socket.framing = zorvik_engine::framing::Framing::Line;
    let (running, events) = start(server.clone()).await;
    assert!(running.url.starts_with("udp://127.0.0.1:"), "{}", running.url);

    let a = client().await;
    a.send_to(b"one\ntwo", running.addr).await.unwrap();
    let (echo, from) = recv(&a).await;
    assert_eq!((echo.as_slice(), from), (&b"one\ntwo"[..], running.addr));
    let open = events.wait_kind(TrafficKind::Open).await;
    assert_eq!((open.conn, open.peer.clone()), (Some(1), Some(a.local_addr().unwrap().to_string())));

    server.socket.mode = ReplyMode::Rules;
    server.socket.line_ending = LineEnding::CrLf;
    server.socket.rules = vec![
        ReplyRule { matcher: MatchKind::Exact, pattern: "PING".into(), reply: "PONG".into(), ..Default::default() },
        ReplyRule {
            matcher: MatchKind::Contains,
            pattern: "slow".into(),
            reply: "late {{message}}".into(),
            delay_ms: 150,
            ..Default::default()
        },
        ReplyRule { matcher: MatchKind::Regex, pattern: "(".into(), reply: "bad".into(), ..Default::default() },
    ];
    assert!(running.update(server.clone(), Default::default()));
    a.send_to(b"PING", running.addr).await.unwrap();
    assert_eq!(recv(&a).await.0, b"PONG\r\n");
    a.send_to(b"unknown", running.addr).await.unwrap();
    nothing(&a).await;
    a.send_to(b"slow one", running.addr).await.unwrap();
    assert_eq!(recv(&a).await.0, b"late slow one\r\n");
    // The unusable rule is reported once.
    let bad = events.wait("regex problem", |e| e.kind == TrafficKind::Error && e.summary.contains("regex")).await;
    assert!(bad.summary.contains("Rule 3"), "{}", bad.summary);

    // The same sender stays one pseudo-connection.
    let traffic = events.traffic();
    assert_eq!(traffic.iter().filter(|e| e.kind == TrafficKind::Open).count(), 1);
    assert!(traffic.iter().filter(|e| e.direction == Some(TrafficDirection::In)).all(|e| e.conn == Some(1)));
    assert!(!running.update(Server { port: 1, ..server.clone() }, Default::default()));

    // Hex replies.
    server.socket.encoding = PayloadEncoding::Hex;
    server.socket.rules = vec![ReplyRule {
        matcher: MatchKind::Contains,
        pattern: "01 02".into(),
        reply: "ff 00".into(),
        ..Default::default()
    }];
    assert!(running.update(server, Default::default()));
    a.send_to(&[0, 1, 2, 3], running.addr).await.unwrap();
    assert_eq!(recv(&a).await.0, [0xff, 0]);
}

#[tokio::test]
async fn manual_sends_and_forgetting_peers() {
    let mut server = Server::new("Manual", ServerKind::Udp);
    server.socket.mode = ReplyMode::Manual;
    server.socket.line_ending = LineEnding::Lf;
    let (running, events) = start(server).await;
    let err = running.send(None, OutgoingMessage::Text { text: "x".into() }).await.unwrap_err();
    assert!(err.message.contains("No client"), "{}", err.message);

    let a = client().await;
    let b = client().await;
    a.send_to(b"hi from a", running.addr).await.unwrap();
    events.wait("a", |e| e.text.as_deref() == Some("hi from a")).await;
    b.send_to(b"hi from b", running.addr).await.unwrap();
    events.wait("b", |e| e.text.as_deref() == Some("hi from b")).await;
    // Manual mode does not answer.
    nothing(&a).await;

    assert_eq!(running.send(None, OutgoingMessage::Text { text: "all".into() }).await.unwrap(), 2);
    assert_eq!(recv(&a).await.0, b"all\n");
    assert_eq!(recv(&b).await.0, b"all\n");
    assert_eq!(running.send(Some(2), OutgoingMessage::Binary { base64: "AQI=".into() }).await.unwrap(), 1);
    assert_eq!(recv(&b).await.0, [1, 2]);
    let out = events.wait("sent", |e| e.direction == Some(TrafficDirection::Out) && e.conn == Some(2)).await;
    assert_eq!(out.peer, Some(b.local_addr().unwrap().to_string()));

    // Too big for one datagram: an error, and the server keeps going.
    let huge = OutgoingMessage::Text { text: "x".repeat(70_000) };
    let err = running.send(Some(1), huge).await.unwrap_err();
    assert!(err.message.contains("65507"), "{}", err.message);

    running.disconnect(1);
    let close = events.wait_kind(TrafficKind::Close).await;
    assert_eq!(close.conn, Some(1));
    assert!(close.summary.contains("forgotten by you"), "{}", close.summary);
    assert!(running.send(Some(1), OutgoingMessage::Text { text: "gone".into() }).await.is_err());
    // Its next datagram makes it a new client.
    a.send_to(b"back", running.addr).await.unwrap();
    let again = events.wait("reopened", |e| e.kind == TrafficKind::Open && e.conn == Some(3)).await;
    assert_eq!(again.peer, Some(a.local_addr().unwrap().to_string()));
}

#[tokio::test]
async fn replies_to_closed_ports_do_not_stop_the_server() {
    let (running, events) = start(Server::new("Echo", ServerKind::Udp)).await;
    // A sender that is gone before the echo arrives (Windows then reports "connection reset" on receive).
    for _ in 0..3 {
        let gone = client().await;
        gone.send_to(b"bye", running.addr).await.unwrap();
        drop(gone);
    }
    tokio::time::sleep(Duration::from_millis(100)).await;
    let a = client().await;
    a.send_to(b"still there?", running.addr).await.unwrap();
    assert_eq!(recv(&a).await.0, b"still there?");
    assert!(events.stopped().is_none());

    let addr = running.addr;
    running.stop();
    for _ in 0..100 {
        if events.stopped().is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(events.stopped(), Some(None));
    // Every known sender is closed and the port is free again.
    let traffic = events.traffic();
    let opened = traffic.iter().filter(|e| e.kind == TrafficKind::Open).count();
    let closed =
        traffic.iter().filter(|e| e.kind == TrafficKind::Close && e.summary.contains("server stopped")).count();
    assert_eq!((opened, closed), (4, 4));
    UdpSocket::bind(addr).await.expect("port is free");
}

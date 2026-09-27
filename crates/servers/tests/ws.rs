//! WebSocket server: greeting, echo, rules, manual sends, disconnect, client
//! close codes, handshake errors, message limit, TLS and stop.

mod common;

use std::time::Duration;

use base64::Engine as _;
use common::{Events, start};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use zorvik_engine::{
    Client, Direction, HttpRequest, RequestOptions, TlsOptions, WsConnected, WsEvent, WsMessageKind, WsOutgoing,
};
use zorvik_formats::{MatchKind, ReplyMode, ReplyRule, Server, ServerKind, ServerTls};
use zorvik_servers::{OutgoingMessage, TrafficDirection, TrafficKind};
use zorvik_workspace::vars::VarContext;

async fn connect(url: &str, opts: &RequestOptions) -> WsConnected {
    let request = HttpRequest { method: "GET".into(), url: url.into(), headers: Vec::new(), body: Default::default() };
    Client::new().websocket(request, opts, None).await.expect("connects")
}

/// Next received message (text, or base64 for binary) or the close.
async fn next(ws: &mut WsConnected) -> WsEvent {
    loop {
        let event =
            tokio::time::timeout(Duration::from_secs(5), ws.events.recv()).await.expect("event in time").expect("open");
        match &event {
            WsEvent::Message { direction: Direction::Sent, .. } => continue,
            WsEvent::Message { kind: WsMessageKind::Ping | WsMessageKind::Pong, .. } => continue,
            _ => return event,
        }
    }
}

async fn next_text(ws: &mut WsConnected) -> String {
    match next(ws).await {
        WsEvent::Message { text: Some(text), .. } => text,
        other => panic!("expected a text message, got {other:?}"),
    }
}

async fn wait_open(events: &Events, count: usize) {
    for _ in 0..200 {
        if events.traffic().iter().filter(|e| e.kind == TrafficKind::Open).count() >= count {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("{count} clients did not connect: {:#?}", events.traffic());
}

#[tokio::test]
async fn greeting_echo_and_rules() {
    let mut server = Server::new("WS", ServerKind::Websocket);
    server.port = 0; // like `start` does, so live updates apply
    server.websocket.greeting = "welcome {{$uuid}}".into();
    let (running, events) = start(server.clone()).await;
    assert!(running.url.starts_with("ws://127.0.0.1:"), "{}", running.url);
    let mut ws = connect(&format!("{}/any/path?x=1", running.url), &RequestOptions::default()).await;
    let greeting = next_text(&mut ws).await;
    assert!(greeting.starts_with("welcome ") && greeting.len() == 44, "{greeting}");

    ws.session.send(WsOutgoing::Text { text: "hello".into() }).unwrap();
    assert_eq!(next_text(&mut ws).await, "hello");
    ws.session.send(WsOutgoing::Binary { base64: "AAEC".into() }).unwrap();
    match next(&mut ws).await {
        WsEvent::Message { kind: WsMessageKind::Binary, base64: Some(b), .. } => assert_eq!(b, "AAEC"),
        other => panic!("expected binary echo, got {other:?}"),
    }

    // Switch to rules without a restart.
    server.websocket.mode = ReplyMode::Rules;
    server.websocket.rules = vec![
        ReplyRule { matcher: MatchKind::Exact, pattern: "ping".into(), reply: "pong".into(), ..Default::default() },
        ReplyRule {
            matcher: MatchKind::Regex,
            pattern: "^say ".into(),
            reply: "you said: {{message}}".into(),
            delay_ms: 50,
            ..Default::default()
        },
        ReplyRule { matcher: MatchKind::Regex, pattern: "(".into(), reply: "never".into(), ..Default::default() },
    ];
    assert!(running.update(server, VarContext::new()));
    ws.session.send(WsOutgoing::Text { text: "unknown".into() }).unwrap();
    ws.session.send(WsOutgoing::Text { text: "ping".into() }).unwrap();
    assert_eq!(next_text(&mut ws).await, "pong", "no reply to 'unknown'");
    ws.session.send(WsOutgoing::Text { text: "say hi".into() }).unwrap();
    assert_eq!(next_text(&mut ws).await, "you said: say hi");

    // The broken rule is reported once.
    events.wait("rule problem", |e| e.kind == TrafficKind::Error && e.summary.contains("invalid regex")).await;
    let open = events.wait_kind(TrafficKind::Open).await;
    assert_eq!(open.conn, Some(1));
    let binary = events.wait("binary in", |e| e.direction == Some(TrafficDirection::In) && e.summary == "binary").await;
    assert_eq!(binary.size, 3);
    events.wait("pong out", |e| e.direction == Some(TrafficDirection::Out) && e.text.as_deref() == Some("pong")).await;

    ws.session.send(WsOutgoing::Close { code: Some(4000), reason: Some("bye".into()) }).unwrap();
    let close = events.wait_kind(TrafficKind::Close).await;
    assert!(close.summary.contains("closed by the client (4000 bye)"), "{}", close.summary);
}

#[tokio::test]
async fn manual_sends_broadcast_and_disconnect() {
    let mut server = Server::new("WS", ServerKind::Websocket);
    server.websocket.mode = ReplyMode::Manual;
    let (running, events) = start(server).await;
    let mut a = connect(&running.url, &RequestOptions::default()).await;
    let mut b = connect(&running.url, &RequestOptions::default()).await;
    wait_open(&events, 2).await;

    assert_eq!(running.send(None, OutgoingMessage::Text { text: "all".into() }).await.unwrap(), 2);
    assert_eq!(next_text(&mut a).await, "all");
    assert_eq!(next_text(&mut b).await, "all");
    let b64 = base64::engine::general_purpose::STANDARD.encode([9u8, 8]);
    // Ids follow accept order: `a` is #1, `b` is #2.
    assert_eq!(running.send(Some(2), OutgoingMessage::Binary { base64: b64.clone() }).await.unwrap(), 1);
    match next(&mut b).await {
        WsEvent::Message { kind: WsMessageKind::Binary, base64: Some(got), .. } => assert_eq!(got, b64),
        other => panic!("{other:?}"),
    }
    let err = running
        .send(None, OutgoingMessage::Event { event: "x".into(), data: "y".into(), id: String::new() })
        .await
        .unwrap_err();
    assert!(err.message.contains("not Server-Sent Events"), "{}", err.message);

    // Manual mode does not answer.
    a.session.send(WsOutgoing::Text { text: "anyone?".into() }).unwrap();
    events.wait("message in", |e| e.text.as_deref() == Some("anyone?")).await;

    let a_conn = 1;
    running.disconnect(a_conn);
    match next(&mut a).await {
        WsEvent::Closed { code, .. } => assert_eq!(code, Some(1000)),
        other => panic!("expected a close, got {other:?}"),
    }
    let close = events.wait_kind(TrafficKind::Close).await;
    assert_eq!(close.conn, Some(a_conn));
    assert!(close.summary.contains("closed by you"), "{}", close.summary);

    // Stopping closes the others with 1001 (going away).
    running.stop();
    match next(&mut b).await {
        WsEvent::Closed { code, .. } => assert_eq!(code, Some(1001)),
        other => panic!("expected a close, got {other:?}"),
    }
}

#[tokio::test]
async fn handshake_errors_limits_and_tls() {
    let (running, events) = start(Server::new("WS", ServerKind::Websocket)).await;
    let mut c = TcpStream::connect(running.addr).await.unwrap();
    c.write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n").await.unwrap();
    let mut buf = [0u8; 64];
    let _ = tokio::time::timeout(Duration::from_secs(5), c.read(&mut buf)).await.expect("closed in time");
    let err = events.wait("handshake error", |e| e.kind == TrafficKind::Error).await;
    assert!(err.summary.contains("no WebSocket upgrade headers"), "{}", err.summary);
    assert!(!events.traffic().iter().any(|e| e.kind == TrafficKind::Open), "not counted as a client");

    // Messages over 16 MB end the connection.
    let ws = connect(&running.url, &RequestOptions::default()).await;
    let big = base64::engine::general_purpose::STANDARD.encode(vec![0u8; 17 << 20]);
    ws.session.send(WsOutgoing::Binary { base64: big }).unwrap();
    let close = events.wait_kind(TrafficKind::Close).await;
    assert!(close.summary.contains("larger than the limit"), "{}", close.summary);

    let mut server = Server::new("Secure", ServerKind::Websocket);
    server.tls = ServerTls { enabled: true, ..Default::default() };
    let (secure, _events) = start(server).await;
    assert!(secure.url.starts_with("wss://"));
    let opts = RequestOptions { tls: TlsOptions { verify: false, ..Default::default() }, ..Default::default() };
    let mut ws = connect(&format!("wss://localhost:{}/", secure.addr.port()), &opts).await;
    ws.session.send(WsOutgoing::Text { text: "over tls".into() }).unwrap();
    assert_eq!(next_text(&mut ws).await, "over tls");
}

#[tokio::test]
async fn a_client_that_does_not_read_is_closed_on_stop() {
    let mut server = Server::new("WS", ServerKind::Websocket);
    server.websocket.mode = ReplyMode::Manual;
    let (running, events) = start(server).await;
    // A client with a tiny receive window that never reads after the handshake.
    let socket = tokio::net::TcpSocket::new_v4().unwrap();
    socket.set_recv_buffer_size(4096).unwrap();
    let stream = socket.connect(running.addr).await.unwrap();
    let (_ws, _) = tokio_tungstenite::client_async(format!("ws://{}/", running.addr), stream).await.unwrap();
    wait_open(&events, 1).await;
    let big = "x".repeat(15 << 20);
    assert_eq!(running.send(Some(1), OutgoingMessage::Text { text: big }).await.unwrap(), 1);
    tokio::time::sleep(Duration::from_millis(200)).await;

    running.stop();
    let close = events.wait_kind(TrafficKind::Close).await;
    assert!(close.summary.contains("server stopped"), "{}", close.summary);
}

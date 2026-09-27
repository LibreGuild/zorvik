//! UDP client against local sockets.

use std::net::SocketAddr;
use std::time::Duration;

use tokio::net::UdpSocket;
use tokio::sync::mpsc::UnboundedReceiver;
use zorvik_engine::framing::LineEnding;
use zorvik_engine::{Client, Direction, RequestOptions, SocketConfig, SocketEvent, SocketOutgoing};

async fn next(events: &mut UnboundedReceiver<SocketEvent>) -> SocketEvent {
    tokio::time::timeout(Duration::from_secs(5), events.recv()).await.expect("event in time").expect("event")
}

/// Next message (skipping errors such as a Windows "port unreachable").
async fn next_message(events: &mut UnboundedReceiver<SocketEvent>) -> (Direction, String, String) {
    loop {
        match next(events).await {
            SocketEvent::Message { direction, text, base64, peer, .. } => {
                return (direction, text.or(base64).unwrap_or_default(), peer.unwrap_or_default());
            }
            SocketEvent::Error { .. } | SocketEvent::Info { .. } => continue,
            other => panic!("not a message: {other:?}"),
        }
    }
}

async fn echo_server() -> SocketAddr {
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let addr = socket.local_addr().unwrap();
    tokio::spawn(async move {
        let mut buf = [0u8; 2048];
        while let Ok((n, peer)) = socket.recv_from(&mut buf).await {
            let _ = socket.send_to(&buf[..n], peer).await;
        }
    });
    addr
}

fn local_port(opened: &zorvik_engine::SocketOpened) -> u16 {
    opened.local_addr.as_deref().unwrap().parse::<SocketAddr>().unwrap().port()
}

#[tokio::test]
async fn datagrams_both_ways_with_peers() {
    let server = echo_server().await;
    let config = SocketConfig { line_ending: LineEnding::Lf, ..Default::default() };
    let conn = Client::new()
        .udp(&format!("udp://127.0.0.1:{}", server.port()), &RequestOptions::default(), config)
        .await
        .unwrap();
    assert_eq!(conn.opened.protocol, "UDP");
    assert_eq!(conn.opened.remote_addr.as_deref(), Some(server.to_string().as_str()));
    let mut events = conn.events;

    conn.session.send(SocketOutgoing::Text { text: "ping".into() }).unwrap();
    assert_eq!(next_message(&mut events).await, (Direction::Sent, "ping\n".into(), server.to_string()));
    assert_eq!(next_message(&mut events).await, (Direction::Received, "ping\n".into(), server.to_string()));

    // Binary payloads get no line ending.
    conn.session.send(SocketOutgoing::Binary { base64: "AAH/".into() }).unwrap();
    assert_eq!(next_message(&mut events).await.0, Direction::Sent);
    let (direction, payload, _) = next_message(&mut events).await;
    assert_eq!((direction, payload.as_str()), (Direction::Received, "AAH/"));

    // Datagrams from other addresses are shown with their sender.
    let stranger = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    stranger.send_to(b"hello from elsewhere", ("127.0.0.1", local_port(&conn.opened))).await.unwrap();
    assert_eq!(
        next_message(&mut events).await,
        (Direction::Received, "hello from elsewhere".into(), stranger.local_addr().unwrap().to_string())
    );

    drop(conn.session);
    assert!(matches!(next(&mut events).await, SocketEvent::Closed { by_client: true, .. }));
}

#[tokio::test]
async fn oversized_and_unreachable_keep_the_session_open() {
    // Nothing listens on this port.
    let closed = std::net::UdpSocket::bind("127.0.0.1:0").unwrap().local_addr().unwrap();
    let conn =
        Client::new().udp(&closed.to_string(), &RequestOptions::default(), SocketConfig::default()).await.unwrap();
    let mut events = conn.events;

    conn.session.send(SocketOutgoing::Text { text: "x".repeat(70_000) }).unwrap();
    match next(&mut events).await {
        SocketEvent::Error { message } => assert!(message.contains("65507"), "{message}"),
        other => panic!("expected an error: {other:?}"),
    }

    // Windows reports the ICMP "port unreachable" as an error on the next receive; either way the session lives on.
    conn.session.send(SocketOutgoing::Text { text: "anyone?".into() }).unwrap();
    assert_eq!(next_message(&mut events).await.0, Direction::Sent);
    let stranger = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    stranger.send_to(b"still here", ("127.0.0.1", local_port(&conn.opened))).await.unwrap();
    assert_eq!(next_message(&mut events).await.1, "still here");
}

#[tokio::test]
async fn localhost_prefers_ipv4_and_bad_addresses_fail() {
    let server = echo_server().await;
    let conn = Client::new()
        .udp(&format!("localhost:{}", server.port()), &RequestOptions::default(), SocketConfig::default())
        .await
        .unwrap();
    assert_eq!(conn.opened.remote_addr.as_deref(), Some(server.to_string().as_str()));
    assert!(conn.opened.local_addr.as_deref().unwrap().starts_with("0.0.0.0:"));

    let client = Client::new();
    let opts = RequestOptions::default();
    let err = client.udp("udp://127.0.0.1", &opts, SocketConfig::default()).await.err().unwrap();
    assert!(err.message.contains("no port"), "{}", err.message);
    let err = client.udp("tcp://127.0.0.1:9", &opts, SocketConfig::default()).await.err().unwrap();
    assert!(err.message.contains("Unsupported scheme"), "{}", err.message);
}

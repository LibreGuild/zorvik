//! Raw TCP client against a local listener.

use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use zorvik_engine::framing::{Framing, LineEnding};
use zorvik_engine::{Client, Direction, RequestOptions, SocketConfig, SocketEvent, SocketOutgoing};

async fn next(events: &mut tokio::sync::mpsc::UnboundedReceiver<SocketEvent>) -> SocketEvent {
    tokio::time::timeout(Duration::from_secs(5), events.recv()).await.expect("event in time").expect("event")
}

fn text(event: &SocketEvent) -> (Direction, String) {
    match event {
        SocketEvent::Message { direction, text, .. } => (*direction, text.clone().unwrap_or_default()),
        other => panic!("not a message: {other:?}"),
    }
}

#[tokio::test]
async fn tcp_lines_both_ways_and_server_close() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let (mut sock, _) = listener.accept().await.unwrap();
        sock.write_all(b"hello\r\nwor").await.unwrap();
        let mut buf = [0u8; 64];
        let n = sock.read(&mut buf).await.unwrap();
        assert_eq!(&buf[..n], b"ping\n");
        sock.write_all(b"ld\npartial").await.unwrap();
    });
    let config = SocketConfig { framing: Framing::Line, line_ending: LineEnding::Lf, ..Default::default() };
    let conn = Client::new().tcp(&format!("127.0.0.1:{port}"), &RequestOptions::default(), config).await.unwrap();
    assert_eq!(conn.opened.protocol, "TCP");
    assert!(conn.opened.remote_addr.unwrap().ends_with(&port.to_string()));
    let mut events = conn.events;
    assert_eq!(text(&next(&mut events).await), (Direction::Received, "hello".into()));
    conn.session.send(SocketOutgoing::Text { text: "ping".into() }).unwrap();
    assert_eq!(text(&next(&mut events).await), (Direction::Sent, "ping\n".into()));
    assert_eq!(text(&next(&mut events).await), (Direction::Received, "world".into()));
    // The unfinished line is shown when the server closes.
    assert_eq!(text(&next(&mut events).await), (Direction::Received, "partial".into()));
    assert!(matches!(next(&mut events).await, SocketEvent::Closed { by_client: false, .. }));
}

#[tokio::test]
async fn tcp_connect_errors_are_clear() {
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let err = Client::new()
        .tcp(&format!("tcp://127.0.0.1:{port}"), &RequestOptions::default(), SocketConfig::default())
        .await;
    assert!(err.is_err());
    let err = Client::new().tcp("127.0.0.1", &RequestOptions::default(), SocketConfig::default()).await.err().unwrap();
    assert!(err.message.contains("no port"), "{}", err.message);
}

#[tokio::test]
async fn large_message_to_an_echo_server_does_not_deadlock() {
    use base64::Engine as _;
    // Echoes while the client is still sending: the client must keep reading
    // while it writes, or both sides stall once the socket buffers are full.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let (sock, _) = listener.accept().await.unwrap();
        let (mut rd, mut wr) = tokio::io::split(sock);
        let _ = tokio::io::copy(&mut rd, &mut wr).await;
    });
    let conn = Client::new()
        .tcp(&format!("127.0.0.1:{port}"), &RequestOptions::default(), SocketConfig::default())
        .await
        .unwrap();
    let mut events = conn.events;
    const SIZE: usize = 32 << 20;
    let payload = base64::engine::general_purpose::STANDARD.encode(vec![7u8; SIZE]);
    conn.session.send(SocketOutgoing::Binary { base64: payload }).unwrap();
    let (mut received, mut sent) = (0u64, false);
    while received < SIZE as u64 || !sent {
        match tokio::time::timeout(Duration::from_secs(10), events.recv()).await.expect("no deadlock").unwrap() {
            SocketEvent::Message { direction: Direction::Received, size, .. } => received += size,
            SocketEvent::Message { direction: Direction::Sent, size, .. } => {
                assert_eq!(size, SIZE as u64);
                sent = true;
            }
            other => panic!("unexpected {other:?}"),
        }
    }
    assert_eq!(received, SIZE as u64);
}

#[tokio::test]
async fn closing_sends_what_is_queued_but_does_not_wait_forever() {
    use base64::Engine as _;
    // Sent right before the session is dropped: still delivered, then EOF.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        let (mut sock, _) = listener.accept().await.unwrap();
        let mut all = Vec::new();
        sock.read_to_end(&mut all).await.unwrap();
        all
    });
    let conn = Client::new()
        .tcp(&format!("127.0.0.1:{port}"), &RequestOptions::default(), SocketConfig::default())
        .await
        .unwrap();
    let mut events = conn.events;
    conn.session.send(SocketOutgoing::Text { text: "one".into() }).unwrap();
    conn.session.send(SocketOutgoing::Text { text: "two".into() }).unwrap();
    drop(conn.session);
    assert_eq!(tokio::time::timeout(Duration::from_secs(5), server).await.unwrap().unwrap(), b"onetwo");
    let mut closed = false;
    while let Some(event) = events.recv().await {
        closed |= matches!(event, SocketEvent::Closed { by_client: true, .. });
    }
    assert!(closed);

    // A peer that never reads: the stalled write gives up a few seconds after the session is dropped.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let _server = tokio::spawn(async move {
        let (sock, _) = listener.accept().await.unwrap();
        tokio::time::sleep(Duration::from_secs(60)).await;
        drop(sock);
    });
    let conn = Client::new()
        .tcp(&format!("127.0.0.1:{port}"), &RequestOptions::default(), SocketConfig::default())
        .await
        .unwrap();
    let mut events = conn.events;
    let payload = base64::engine::general_purpose::STANDARD.encode(vec![1u8; 64 << 20]);
    conn.session.send(SocketOutgoing::Binary { base64: payload }).unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    drop(conn.session);
    // Windows may take the whole write into its loopback buffers (then a Sent message comes
    // first); either way the session must end within the grace period.
    let closed = tokio::time::timeout(Duration::from_secs(10), async {
        while let Some(event) = events.recv().await {
            if let SocketEvent::Closed { by_client, .. } = event {
                return by_client;
            }
        }
        false
    })
    .await
    .expect("closed in time");
    assert!(closed);
}

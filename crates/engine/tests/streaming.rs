//! SSE and WebSocket against local servers.

use std::time::Duration;

use base64::Engine as _;
use bytes::Bytes;
use zorvik_engine::{
    Client, Direction, Header, HttpRequest, RequestOptions, SseParser, TlsOptions, WsEvent, WsMessageKind, WsOutgoing,
};
use zorvik_testkit::{TestCerts, TestServer};

fn get(url: impl Into<String>) -> HttpRequest {
    HttpRequest { method: "GET".into(), url: url.into(), headers: vec![], body: Bytes::new() }
}

async fn next_event(rx: &mut tokio::sync::mpsc::UnboundedReceiver<WsEvent>) -> WsEvent {
    tokio::time::timeout(Duration::from_secs(5), rx.recv()).await.expect("event in time").expect("channel open")
}

fn received_text(ev: &WsEvent) -> Option<&str> {
    match ev {
        WsEvent::Message { direction: Direction::Received, kind: WsMessageKind::Text, text, .. } => text.as_deref(),
        _ => None,
    }
}

#[tokio::test]
async fn sse_stream_parses_events() {
    let server = TestServer::start().await;
    let mut stream = Client::new()
        .open_stream(get(server.url("/sse?count=4&interval=20")), &RequestOptions::default(), None)
        .await
        .unwrap();
    assert_eq!(stream.meta.status, 200);
    assert!(stream.meta.headers.iter().any(|h| h.name == "Content-Type" && h.value == "text/event-stream"));
    // SSE responses must not ask for compression.
    assert!(!stream.meta.request.headers.iter().any(|h| h.name == "Accept-Encoding"));
    let mut parser = SseParser::new();
    let mut events = Vec::new();
    while let Some(chunk) = stream.body.next_chunk().await {
        events.extend(parser.feed(&chunk.unwrap()));
    }
    assert_eq!(events.len(), 4);
    assert_eq!(events[0].data, r#"{"n":0}"#);
    assert_eq!(events[0].retry, Some(2000));
    assert_eq!(events[1].event, "tick");
    assert_eq!(events[1].data, "line one 1\nline two");
    assert_eq!(events[3].id.as_deref(), Some("3"));
}

#[tokio::test]
async fn websocket_echo_text_binary_and_server_close() {
    let server = TestServer::start().await;
    let mut req = get(server.ws_url("/ws"));
    req.headers.push(Header::new("Sec-WebSocket-Protocol", "chat"));
    req.headers.push(Header::new("X-Test", "1"));
    let mut conn = Client::new().websocket(req, &RequestOptions::default(), None).await.unwrap();
    assert_eq!(conn.meta.status, 101);
    assert!(conn.meta.headers.iter().any(|h| h.name == "Sec-Websocket-Protocol" && h.value == "chat"));
    assert_eq!(received_text(&next_event(&mut conn.events).await), Some("welcome"));

    conn.session.send(WsOutgoing::Text { text: "hello".into() }).unwrap();
    let sent = next_event(&mut conn.events).await;
    assert!(matches!(sent, WsEvent::Message { direction: Direction::Sent, .. }));
    assert_eq!(received_text(&next_event(&mut conn.events).await), Some("hello"));

    let payload = base64::engine::general_purpose::STANDARD.encode([0u8, 1, 2, 255]);
    conn.session.send(WsOutgoing::Binary { base64: payload.clone() }).unwrap();
    let _sent = next_event(&mut conn.events).await;
    match next_event(&mut conn.events).await {
        WsEvent::Message { kind: WsMessageKind::Binary, base64, size, .. } => {
            assert_eq!(base64.unwrap(), payload);
            assert_eq!(size, 4);
        }
        other => panic!("unexpected {other:?}"),
    }
    assert!(conn.session.send(WsOutgoing::Binary { base64: "%%%".into() }).is_err());

    conn.session.send(WsOutgoing::Text { text: "close".into() }).unwrap();
    let _sent = next_event(&mut conn.events).await;
    match next_event(&mut conn.events).await {
        WsEvent::Closed { code, reason, by_client } => {
            assert_eq!(code, Some(4000));
            assert_eq!(reason, "bye");
            assert!(!by_client);
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn websocket_client_close_and_wss() {
    let certs = TestCerts::generate();
    let dir = tempfile::tempdir().unwrap();
    let ca = certs.write_ca(dir.path());
    let server = TestServer::start_tls(&certs).await;
    let opts =
        RequestOptions { tls: TlsOptions { ca_cert_path: Some(ca), ..Default::default() }, ..Default::default() };
    let mut conn = Client::new().websocket(get(server.ws_url("/ws")), &opts, None).await.unwrap();
    assert!(conn.meta.tls.is_some());
    assert_eq!(conn.meta.tls.as_ref().unwrap().alpn.as_deref(), Some("http/1.1"));
    assert_eq!(received_text(&next_event(&mut conn.events).await), Some("welcome"));
    conn.session.send(WsOutgoing::Close { code: Some(1000), reason: Some("done".into()) }).unwrap();
    loop {
        match next_event(&mut conn.events).await {
            WsEvent::Closed { by_client, .. } => {
                assert!(by_client);
                break;
            }
            WsEvent::Message { .. } => continue,
            other => panic!("unexpected {other:?}"),
        }
    }
}

#[tokio::test]
async fn websocket_rejected_upgrade_reports_status() {
    let server = TestServer::start().await;
    let err = Client::new()
        .websocket(get(server.ws_url("/status/401")), &RequestOptions::default(), None)
        .await
        .err()
        .expect("upgrade rejected");
    assert!(err.message.contains("401"), "{err:?}");
}

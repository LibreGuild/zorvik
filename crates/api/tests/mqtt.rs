//! MQTT through `socket.connect`: inherited Basic auth and variables reach the broker.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use zorvik_api::{Api, EventSink, StreamEvent};
use zorvik_engine::SocketEvent;

#[derive(Default)]
struct Collect(Mutex<Vec<StreamEvent>>);

impl EventSink for Collect {
    fn emit(&self, event: StreamEvent) {
        self.0.lock().unwrap().push(event);
    }
}

/// One MQTT 3.1.1 packet: (first byte, body).
async fn read_packet(stream: &mut TcpStream) -> Option<(u8, Vec<u8>)> {
    let first = stream.read_u8().await.ok()?;
    let (mut len, mut shift) = (0usize, 0);
    loop {
        let b = stream.read_u8().await.ok()?;
        len |= usize::from(b & 0x7f) << shift;
        if b & 0x80 == 0 {
            break;
        }
        shift += 7;
    }
    let mut body = vec![0u8; len];
    stream.read_exact(&mut body).await.ok()?;
    Some((first, body))
}

fn string_at(body: &[u8], at: &mut usize) -> String {
    let len = usize::from(u16::from_be_bytes([body[*at], body[*at + 1]]));
    let s = String::from_utf8_lossy(&body[*at + 2..*at + 2 + len]).into_owned();
    *at += 2 + len;
    s
}

async fn next(rx: &mut mpsc::UnboundedReceiver<String>) -> String {
    tokio::time::timeout(Duration::from_secs(5), rx.recv()).await.unwrap().unwrap()
}

/// A hand-rolled broker that reports what it received as text lines.
async fn broker() -> (SocketAddr, mpsc::UnboundedReceiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        let Ok((mut stream, _)) = listener.accept().await else { return };
        while let Some((first, body)) = read_packet(&mut stream).await {
            match first >> 4 {
                1 => {
                    let flags = body[7];
                    let mut at = 10;
                    let id = string_at(&body, &mut at);
                    let user = if flags & 0x80 != 0 { string_at(&body, &mut at) } else { String::new() };
                    let pass = if flags & 0x40 != 0 { string_at(&body, &mut at) } else { String::new() };
                    let _ = tx.send(format!("connect id={id} user={user} pass={pass} keepalive={}", body[9]));
                    stream.write_all(&[0x20, 2, 0, 0]).await.unwrap();
                }
                8 => {
                    let mut at = 2;
                    let mut granted = Vec::new();
                    while at < body.len() {
                        let topic = string_at(&body, &mut at);
                        granted.push(body[at]);
                        at += 1;
                        let _ = tx.send(format!("subscribe {topic}"));
                    }
                    let mut suback = vec![0x90, 2 + granted.len() as u8, body[0], body[1]];
                    suback.extend(granted);
                    stream.write_all(&suback).await.unwrap();
                }
                14 => {
                    let _ = tx.send("disconnect".into());
                    return;
                }
                _ => {}
            }
        }
    });
    (addr, rx)
}

#[tokio::test]
async fn inherited_basic_auth_and_variables_reach_the_broker() {
    let (addr, mut seen) = broker().await;
    let data = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    let events = Arc::new(Collect::default());
    let api = Api::new(data.path().to_path_buf(), events.clone());
    let info = api.call("workspace.create", json!({ "path": ws.path(), "name": "Test" })).await.unwrap();
    let env = api
        .call(
            "env.create",
            json!({ "environment": { "name": "Dev", "variables": [
                { "key": "user", "value": "alice" },
                { "key": "room", "value": "kitchen" },
                { "key": "broker", "value": format!("mqtt://{addr}") }
            ] } }),
        )
        .await
        .unwrap();
    api.call("env.setActive", json!({ "id": env })).await.unwrap();
    // Basic auth set on the workspace; the request inherits it.
    let mut meta = info["meta"].clone();
    meta["auth"] = json!({ "type": "basic", "username": "{{user}}", "password": "p:w" });
    api.call("workspace.saveMeta", json!({ "meta": meta })).await.unwrap();

    let request = json!({
        "name": "m", "kind": "mqtt", "seq": 0, "method": "GET", "url": "{{broker}}",
        "mqtt": { "clientId": "dev-{{room}}", "keepAliveSecs": 45, "qos": 0,
                  "subscriptions": [{ "topic": "sensors/{{room}}", "qos": 1 }, { "topic": "off", "qos": 0, "enabled": false }] }
    });
    let opened = api.call("socket.connect", json!({ "connId": "m1", "request": request, "path": null })).await.unwrap();
    assert_eq!(opened["opened"]["protocol"], "MQTT 3.1.1");
    assert_eq!(next(&mut seen).await, "connect id=dev-kitchen user=alice pass=p:w keepalive=45");
    assert_eq!(next(&mut seen).await, "subscribe sensors/kitchen");

    let subscribed = || {
        events.0.lock().unwrap().iter().any(|e| {
            matches!(e, StreamEvent::Socket { event: SocketEvent::Info { text, .. }, .. } if text == "Subscribed to sensors/kitchen (QoS 1)")
        })
    };
    for _ in 0..200 {
        if subscribed() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(subscribed(), "{:?}", events.0.lock().unwrap());

    api.call("socket.close", json!({ "connId": "m1" })).await.unwrap();
    assert_eq!(next(&mut seen).await, "disconnect");
}

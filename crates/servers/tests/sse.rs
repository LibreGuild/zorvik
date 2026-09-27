//! SSE server: configured events (templates, interval, repeat, resume), sends
//! from the UI, disconnects both ways, wrong methods and stop.

mod common;

use std::time::{Duration, Instant};

use common::{start, start_with};
use zorvik_engine::{BodyStream, Client, HttpRequest, RequestOptions, SseEvent, SseParser};
use zorvik_formats::{Server, ServerKind, SseEventTemplate, Variable};
use zorvik_servers::{OutgoingMessage, TrafficDirection, TrafficKind};
use zorvik_workspace::vars::VarContext;

fn event(name: &str, data: &str, id: &str) -> SseEventTemplate {
    SseEventTemplate { event: name.into(), data: data.into(), id: id.into() }
}

fn sse(events: Vec<SseEventTemplate>, interval_ms: u64, repeat: bool) -> Server {
    let mut server = Server::new("Events", ServerKind::Sse);
    server.port = 0; // like `start` does, so live updates apply
    server.sse.events = events;
    server.sse.interval_ms = interval_ms;
    server.sse.repeat = repeat;
    server
}

struct Stream {
    body: BodyStream,
    parser: SseParser,
    ready: Vec<SseEvent>,
    headers: Vec<(String, String)>,
}

async fn open(url: &str, headers: &[(&str, &str)]) -> Stream {
    let request = HttpRequest {
        method: "GET".into(),
        url: url.into(),
        headers: headers.iter().map(|(k, v)| zorvik_engine::Header::new(*k, *v)).collect(),
        body: Default::default(),
    };
    let response = Client::new().open_stream(request, &RequestOptions::default(), None).await.expect("stream opens");
    assert_eq!(response.meta.status, 200);
    let headers = response.meta.headers.iter().map(|h| (h.name.to_ascii_lowercase(), h.value.clone())).collect();
    Stream { body: response.body, parser: SseParser::new(), ready: Vec::new(), headers }
}

impl Stream {
    async fn next(&mut self) -> SseEvent {
        loop {
            if !self.ready.is_empty() {
                return self.ready.remove(0);
            }
            let chunk = tokio::time::timeout(Duration::from_secs(5), self.body.next_chunk())
                .await
                .expect("event in time")
                .expect("stream still open")
                .expect("no stream error");
            self.ready.extend(self.parser.feed(&chunk));
        }
    }

    /// Whether the server ended the stream (within 5 s).
    async fn ended(&mut self) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            match tokio::time::timeout(Duration::from_secs(5), self.body.next_chunk()).await {
                Ok(None) | Ok(Some(Err(_))) => return true,
                Ok(Some(Ok(_))) => continue,
                Err(_) => return false,
            }
        }
        false
    }
}

#[tokio::test]
async fn plays_configured_events_with_templates() {
    let mut vars = VarContext::new();
    vars.push_layer(&[Variable { key: "env".into(), value: "dev".into(), enabled: true, secret: false }]);
    let server = sse(
        vec![
            event("hello", "hi {{request.query.user}} on {{env}}", "1"),
            event("", "line one\nline two", ""),
            event("tick", "{{$uuid}}", "3"),
        ],
        40,
        false,
    );
    let (running, events) = start_with(server, vars).await;
    let started = Instant::now();
    let mut s = open(&format!("{}/stream?user=ann", running.url), &[]).await;
    let headers = &s.headers;
    assert!(headers.contains(&("content-type".into(), "text/event-stream".into())), "{headers:?}");
    assert!(headers.contains(&("cache-control".into(), "no-cache".into())));
    assert!(headers.contains(&("access-control-allow-origin".into(), "*".into())));

    let first = s.next().await;
    assert_eq!((first.event.as_str(), first.data.as_str(), first.id.as_deref()), ("hello", "hi ann on dev", Some("1")));
    let second = s.next().await;
    assert_eq!((second.event.as_str(), second.data.as_str()), ("message", "line one\nline two"));
    let third = s.next().await;
    assert_eq!((third.event.as_str(), third.data.len(), third.id.as_deref()), ("tick", 36, Some("3")));
    assert!(started.elapsed() >= Duration::from_millis(80), "events are spaced by the interval");

    let open_entry = events.wait_kind(TrafficKind::Open).await;
    assert_eq!(open_entry.conn, Some(1));
    let out = events.wait("hello out", |e| e.direction == Some(TrafficDirection::Out) && e.summary == "hello").await;
    assert_eq!(out.text.as_deref(), Some("hi ann on dev"));
    events.wait("message out", |e| e.summary == "message" && e.text.as_deref() == Some("line one\nline two")).await;

    // A reconnecting client continues after the last id it saw.
    let mut resumed = open(&running.url, &[("Last-Event-ID", "1")]).await;
    assert_eq!(resumed.next().await.data, "line one\nline two");
}

#[tokio::test]
async fn repeat_sends_from_the_ui_and_disconnects() {
    let (running, events) = start(sse(vec![event("a", "1", ""), event("b", "2", "")], 20, true)).await;
    let mut s = open(&running.url, &[]).await;
    let mut got = Vec::new();
    for _ in 0..5 {
        got.push(s.next().await.event);
    }
    assert_eq!(got, ["a", "b", "a", "b", "a"]);

    // Stop the loop, then send from the UI: an event to everyone, text to one stream.
    let mut quiet = sse(Vec::new(), 0, false);
    quiet.name = "Quiet".into();
    assert!(running.update(quiet, VarContext::new()));
    let mut other = open(&running.url, &[]).await;
    for _ in 0..100 {
        if events.traffic().iter().filter(|e| e.kind == TrafficKind::Open).count() == 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let sent = running
        .send(
            None,
            OutgoingMessage::Event { event: "news".into(), data: "hello {{$randomInt}}".into(), id: "n1".into() },
        )
        .await
        .unwrap();
    assert_eq!(sent, 2);
    let news = other.next().await;
    assert_eq!((news.event.as_str(), news.id.as_deref()), ("news", Some("n1")));
    assert!(news.data.starts_with("hello ") && !news.data.contains("{{"), "{}", news.data);
    assert_eq!(running.send(Some(2), OutgoingMessage::Text { text: "just data".into() }).await.unwrap(), 1);
    let plain = other.next().await;
    assert_eq!((plain.event.as_str(), plain.data.as_str()), ("message", "just data"));
    let err = running.send(None, OutgoingMessage::Binary { base64: "AA==".into() }).await.unwrap_err();
    assert!(err.message.contains("carry text"), "{}", err.message);

    // Disconnect from the UI ends that stream.
    running.disconnect(2);
    assert!(other.ended().await, "the stream ends");
    let closed = events.wait_kind(TrafficKind::Close).await;
    assert_eq!(closed.conn, Some(2));
    assert!(closed.summary.contains("closed by you"), "{}", closed.summary);

    // A client that goes away is noticed right away (not at the next keep-alive).
    let started = Instant::now();
    drop(s);
    let gone = events.wait("client gone", |e| e.kind == TrafficKind::Close && e.conn == Some(1)).await;
    assert!(gone.summary.contains("client disconnected"), "{}", gone.summary);
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[tokio::test]
async fn wrong_method_and_stop() {
    let (running, events) = start(sse(vec![event("", "x", "")], 0, false)).await;
    let request =
        HttpRequest { method: "POST".into(), url: running.url.clone(), headers: Vec::new(), body: "b".into() };
    let response = Client::new().send(request, &RequestOptions::default(), None).await.unwrap();
    assert_eq!(response.meta.status, 405);
    assert!(response.meta.headers.iter().any(|h| h.name.eq_ignore_ascii_case("allow") && h.value == "GET"));
    events.wait("405", |e| e.kind == TrafficKind::Info && e.summary.contains("405")).await;

    let mut s = open(&running.url, &[]).await;
    assert_eq!(s.next().await.data, "x");
    running.stop();
    assert!(s.ended().await, "stopping the server ends open streams");
    let closed = events.wait_kind(TrafficKind::Close).await;
    assert!(closed.summary.contains("server stopped"), "{}", closed.summary);
}

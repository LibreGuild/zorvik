//! GraphQL subscriptions through the API against the test server: live sessions
//! (`socket.connect`) over both WebSocket protocols and SSE, and reads to an end (`http.send`,
//! as the collection runner, CLI and agents do).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};
use zorvik_api::{Api, EventSink, StreamEvent};
use zorvik_engine::{Direction, SocketEvent};
use zorvik_testkit::TestServer;

#[derive(Default)]
struct Collect(Mutex<Vec<StreamEvent>>);

impl EventSink for Collect {
    fn emit(&self, event: StreamEvent) {
        self.0.lock().unwrap().push(event);
    }
}

struct Harness {
    api: Api,
    events: Arc<Collect>,
    _data: tempfile::TempDir,
    _ws: tempfile::TempDir,
}

impl Harness {
    async fn new() -> Self {
        let data = tempfile::tempdir().unwrap();
        let ws = tempfile::tempdir().unwrap();
        let events = Arc::new(Collect::default());
        let api = Api::new(data.path().to_path_buf(), events.clone());
        api.call("workspace.create", json!({ "path": ws.path(), "name": "Subs" })).await.unwrap();
        Self { api, events, _data: data, _ws: ws }
    }

    async fn call(&self, method: &str, params: Value) -> Value {
        match self.api.call(method, params).await {
            Ok(v) => v,
            Err(e) => panic!("{method} failed: {} ({})", e.message, e.code),
        }
    }

    /// The socket events of `conn` once it closed (or after 5 s).
    async fn session(&self, conn: &str) -> Vec<SocketEvent> {
        let mine = || -> Vec<SocketEvent> {
            self.events
                .0
                .lock()
                .unwrap()
                .iter()
                .filter_map(|e| match e {
                    StreamEvent::Socket { conn_id, event } if conn_id == conn => Some(event.clone()),
                    _ => None,
                })
                .collect()
        };
        for _ in 0..250 {
            if mine().iter().any(|e| matches!(e, SocketEvent::Closed { .. })) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        mine()
    }
}

fn subscription(url: &str, graphql: Value) -> Value {
    let mut body = json!({ "type": "graphql", "graphql": graphql });
    if body["graphql"]["query"].is_null() {
        body["graphql"]["query"] = json!("subscription Ticks($count: Int) { tick }");
    }
    json!({ "name": "s", "seq": 0, "method": "POST", "url": url, "body": body })
}

/// The received results' `data.tick` values.
fn ticks(events: &[SocketEvent]) -> Vec<u64> {
    events
        .iter()
        .filter_map(|e| match e {
            SocketEvent::Message { direction: Direction::Received, text: Some(t), .. } => {
                serde_json::from_str::<Value>(t).ok()?["data"]["tick"].as_u64()
            }
            _ => None,
        })
        .collect()
}

fn closed_reason(events: &[SocketEvent]) -> String {
    events
        .iter()
        .find_map(|e| match e {
            SocketEvent::Closed { reason, .. } => Some(reason.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

#[tokio::test]
async fn websocket_subscriptions_with_both_protocols() {
    let server = TestServer::start().await;
    let h = Harness::new().await;
    for (transport, label) in [("websocket", "graphql-transport-ws"), ("websocketLegacy", "subscriptions-transport-ws")]
    {
        let conn = format!("ws-{transport}");
        // The subscription URL is the request URL as ws://, variables filled in.
        let request = subscription(
            &server.url("/graphql-ws"),
            json!({ "variables": "{\"count\": 3, \"who\": \"{{$randomUUID}}\"}", "transport": transport }),
        );
        let opened = h.call("socket.connect", json!({ "connId": conn, "request": request, "path": null })).await;
        assert_eq!(opened["opened"]["protocol"], format!("GraphQL over WebSocket ({label})"));
        let events = h.session(&conn).await;
        assert_eq!(ticks(&events), [1, 2, 3], "{transport}: {events:?}");
        assert_eq!(closed_reason(&events), "The server completed the subscription");
        // The operation went out first, as sent; the server received it with its variables.
        match &events[0] {
            SocketEvent::Message { direction: Direction::Sent, text: Some(text), detail, .. } => {
                assert!(text.contains("\"query\":\"subscription Ticks"), "{text}");
                assert_eq!(detail.as_deref(), Some("subscribe"));
            }
            other => panic!("{other:?}"),
        }
        let first = events.iter().find_map(|e| match e {
            SocketEvent::Message { direction: Direction::Received, text: Some(t), .. } => Some(t.clone()),
            _ => None,
        });
        let first: Value = serde_json::from_str(&first.unwrap()).unwrap();
        assert_eq!(first["extensions"]["received"]["variables"]["count"], 3);
        assert_eq!(first["extensions"]["received"]["operationName"], Value::Null);
    }
}

#[tokio::test]
async fn connection_params_unsubscribe_and_errors() {
    let server = TestServer::start().await;
    let h = Harness::new().await;
    let env = h
        .call(
            "env.create",
            json!({ "environment": { "name": "Dev", "variables": [{ "key": "token", "value": "graphql-token" }] } }),
        )
        .await;
    h.call("env.setActive", json!({ "id": env })).await;

    // The endpoint wants a token in connection_init; the subscription runs until closed.
    let url = server.url("/graphql-ws?auth=1");
    let endless = subscription(
        "http://unused.invalid/graphql",
        json!({ "variables": "{\"count\": 0}", "subscriptionUrl": url, "connectionParams": "{\"token\": \"{{token}}\"}" }),
    );
    h.call("socket.connect", json!({ "connId": "endless", "request": endless, "path": null })).await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    h.call("socket.close", json!({ "connId": "endless" })).await;
    let events = h.session("endless").await;
    assert!(ticks(&events).len() >= 2, "{events:?}");
    assert_eq!(closed_reason(&events), "Unsubscribed");

    // A wrong token: the server closes with 4403 before accepting.
    let refused = subscription(&url, json!({ "connectionParams": "{\"token\": \"nope\"}" }));
    let e = h
        .api
        .call("socket.connect", json!({ "connId": "refused", "request": refused, "path": null }))
        .await
        .unwrap_err();
    assert!(e.message.contains("4403") && e.message.contains("forbidden"), "{}", e.message);

    // A rejected operation ends the subscription with the server's errors.
    let bad = subscription(&server.url("/graphql-ws"), json!({ "query": "subscription { boom }" }));
    h.call("socket.connect", json!({ "connId": "bad", "request": bad, "path": null })).await;
    let events = h.session("bad").await;
    let error = events.iter().find_map(|e| match e {
        SocketEvent::Error { message } => Some(message.clone()),
        _ => None,
    });
    assert!(error.unwrap_or_default().contains("Cannot query field \\\"boom\\\""), "{events:?}");
    assert_eq!(closed_reason(&events), "The subscription failed");

    // Invalid connection params are reported before connecting.
    let invalid = subscription(&server.url("/graphql-ws"), json!({ "connectionParams": "{oops" }));
    let e = h.api.call("socket.connect", json!({ "connId": "x", "request": invalid, "path": null })).await.unwrap_err();
    assert!(e.message.starts_with("Connection params are not valid JSON"), "{}", e.message);
}

#[tokio::test]
async fn sse_subscriptions() {
    let server = TestServer::start().await;
    let h = Harness::new().await;
    let request =
        subscription(&server.url("/graphql-sse"), json!({ "variables": "{\"count\": 2}", "transport": "sse" }));
    let opened = h.call("socket.connect", json!({ "connId": "sse", "request": request, "path": null })).await;
    assert!(opened["opened"]["protocol"].as_str().unwrap().starts_with("GraphQL over SSE"), "{opened}");
    let events = h.session("sse").await;
    assert_eq!(ticks(&events), [1, 2], "{events:?}");
    assert_eq!(closed_reason(&events), "The server completed the subscription");

    let bad =
        subscription(&server.url("/graphql-sse"), json!({ "query": "subscription { boom }", "transport": "sse" }));
    let e = h.api.call("socket.connect", json!({ "connId": "bad", "request": bad, "path": null })).await.unwrap_err();
    assert!(e.message.contains("400") && e.message.contains("boom"), "{}", e.message);
}

#[tokio::test]
async fn sending_reads_results_until_the_stream_settings_say_to_stop() {
    let server = TestServer::start().await;
    let h = Harness::new().await;
    for transport in ["websocket", "sse"] {
        let url = server.url(if transport == "sse" { "/graphql-sse" } else { "/graphql-ws" });
        let mut request = subscription(&url, json!({ "variables": "{\"count\": 0}", "transport": transport }));
        request["settings"] = json!({ "stream": { "maxEvents": 2, "timeoutMs": 5000 } });
        request["scripts"] = json!({ "postResponse":
            "pm.test('two results', () => pm.expect(pm.response.json().map(r => r.data.tick)).to.eql([1, 2]));\n\
             pm.test('as events', () => pm.expect(pm.response.events.length).to.equal(2));" });
        let result = h.call("http.send", json!({ "requestId": transport, "request": request, "path": null })).await;
        let tests = result["scripts"]["tests"].as_array().unwrap();
        assert!(tests.iter().all(|t| t["passed"] == true), "{transport}: {tests:?}");
        assert_eq!(tests.len(), 2);
    }

    // A completed subscription ends the read early.
    let mut request = subscription(&server.url("/graphql-ws"), json!({ "variables": "{\"count\": 1}" }));
    request["settings"] = json!({ "stream": { "maxEvents": 10, "timeoutMs": 5000 } });
    let started = std::time::Instant::now();
    let result = h.call("http.send", json!({ "requestId": "one", "request": request, "path": null })).await;
    assert!(started.elapsed() < Duration::from_secs(3));
    assert_eq!(result["meta"]["status"], 101);
    let body: Value = serde_json::from_str(result["body"]["text"].as_str().unwrap()).unwrap();
    assert_eq!(body[0]["data"]["tick"], 1);
}

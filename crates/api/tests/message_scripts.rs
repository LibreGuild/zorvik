//! Scripts of WebSocket and Socket.IO requests: pre-request scripts before connecting, and
//! message scripts on the messages that match (tests, variables, replies), on live connections
//! and in collection runs.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};
use zorvik_api::{Api, EventSink, StreamEvent};
use zorvik_engine::{Direction, SocketEvent, WsEvent};
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
    _dir: tempfile::TempDir,
}

impl Harness {
    async fn new() -> Self {
        let data = tempfile::tempdir().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let events = Arc::new(Collect::default());
        let api = Api::new(data.path().to_path_buf(), events.clone());
        let h = Self { api, events, _data: data, _dir: dir };
        h.ok("workspace.create", json!({ "path": h._dir.path(), "name": "Scripts" })).await;
        let env = h.ok("env.create", json!({ "environment": { "name": "Dev", "variables": [] } })).await;
        h.ok("env.setActive", json!({ "id": env })).await;
        h
    }

    async fn ok(&self, method: &str, params: Value) -> Value {
        match self.api.call(method, params).await {
            Ok(v) => v,
            Err(e) => panic!("{method} failed: {} ({})", e.message, e.code),
        }
    }

    async fn wait_for(&self, what: &str, ok: impl Fn(&[StreamEvent]) -> bool) {
        for _ in 0..250 {
            if ok(&self.events.0.lock().unwrap()) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("{what}: {:#?}", self.events.0.lock().unwrap());
    }

    async fn variable(&self, key: &str) -> Option<String> {
        let list = self.ok("vars.list", json!({})).await;
        list.as_array()?.iter().find(|v| v["key"] == key).and_then(|v| v["value"].as_str().map(str::to_string))
    }

    async fn run(&self, request: Value) -> String {
        let folder = self.ok("folder.create", json!({ "parent": "", "name": "Live" })).await;
        self.ok("request.create", json!({ "parent": folder, "request": request })).await;
        let started = self.ok("runner.start", json!({ "folder": folder })).await;
        let run_id = started["runId"].as_str().unwrap().to_string();
        let mut text = String::new();
        for _ in 0..250 {
            text = self
                .events
                .0
                .lock()
                .unwrap()
                .iter()
                .filter(|e| matches!(e, StreamEvent::Runner { run_id: r, .. } if *r == run_id))
                .map(|e| serde_json::to_string(e).unwrap())
                .collect::<Vec<_>>()
                .join("\n");
            if text.contains("\"finished\"") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(40)).await;
        }
        text
    }
}

fn ws_notes(events: &[StreamEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e {
            StreamEvent::Ws { event: WsEvent::Info { text, .. }, .. } => Some(text.clone()),
            StreamEvent::Ws { event: WsEvent::Error { message }, .. } => Some(message.clone()),
            _ => None,
        })
        .collect()
}

fn ws_sent(events: &[StreamEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e {
            StreamEvent::Ws { event: WsEvent::Message { direction: Direction::Sent, text: Some(t), .. }, .. } => {
                Some(t.clone())
            }
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn websocket_scripts_before_connecting_and_on_messages() {
    let server = TestServer::start().await;
    let h = Harness::new().await;
    let request = json!({
        "name": "Echo", "kind": "websocket", "url": server.ws_url("/{{path}}"),
        "scripts": { "preRequest": "pm.variables.set('path', 'ws'); pm.environment.set('who', 'ada'); console.log('ready');" },
        "websocket": {
            "onMessage": [
                // The server's greeting: a test, a variable, and a reply.
                { "match": "exact", "pattern": "welcome",
                  "script": "pm.test('greeted', () => pm.expect(pm.message.text()).to.eql('welcome'));\npm.environment.set('greeting', pm.message.text());\npm.connection.send({ hello: pm.environment.get('who') });" },
                // Every sent message: the script's own sends don't run it again.
                { "direction": "sent", "script": "pm.connection.send('seen ' + pm.message.text());" },
                { "enabled": false, "script": "pm.connection.send('never');" },
            ],
        },
    });
    h.ok("ws.connect", json!({ "connId": "w", "request": request, "path": null })).await;
    h.wait_for("the reply to the greeting", |evs| ws_sent(evs).iter().any(|t| t == r#"{"hello":"ada"}"#)).await;
    // A message the composer sends runs the "sent" script; what the scripts send doesn't.
    h.ok("ws.send", json!({ "connId": "w", "message": { "type": "text", "text": "hi" } })).await;
    h.wait_for("the sent script's reply", |evs| ws_sent(evs).iter().any(|t| t == "seen hi")).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let sent = ws_sent(&h.events.0.lock().unwrap());
    assert_eq!(sent, [r#"{"hello":"ada"}"#, "hi", "seen hi"], "no loop, nothing from the disabled one");
    let notes = ws_notes(&h.events.0.lock().unwrap());
    assert!(notes.contains(&"Pre-request script: ready".to_string()), "{notes:?}");
    assert!(notes.contains(&"On the message \"welcome\": ✓ greeted".to_string()), "{notes:?}");
    assert_eq!(h.variable("greeting").await.as_deref(), Some("welcome"));
    assert!(h.events.0.lock().unwrap().iter().any(|e| matches!(e, StreamEvent::VariablesChanged {})));

    // A failing pre-request script stops the connection.
    let mut broken = request.clone();
    broken["scripts"]["preRequest"] = json!("throw new Error('no token')");
    let e = h.api.call("ws.connect", json!({ "connId": "x", "request": broken, "path": null })).await.unwrap_err();
    assert!(e.message.contains("no token"), "{}", e.message);
    h.ok("ws.close", json!({ "connId": "w" })).await;

    // In a collection run, message scripts' tests are the request's tests.
    let mut run = request.clone();
    run["websocket"]["onMessage"][0]["name"] = json!("Greeting");
    run["websocket"]["onMessage"][1] = json!({ "script": "pm.environment.set('fromRun', pm.message.text());" });
    run["websocket"]["onConnect"] = json!([{ "type": "wait", "match": "contains", "pattern": "hello" }]);
    let text = h.run(run).await;
    assert!(text.contains("Greeting: greeted"), "{text}");
    assert!(text.contains(r#""testsPassed":2,"testsFailed":0"#), "{text}");
    // What message scripts set in a run is kept like any script's.
    assert_eq!(h.variable("fromRun").await.as_deref(), Some(r#"{"hello":"ada"}"#), "the last message received");
}

#[tokio::test]
async fn socketio_scripts_answer_events() {
    let h = Harness::new().await;
    let server = json!({ "name": "Rooms", "kind": "socketio", "port": 0,
        "socketio": { "mode": "rules", "rules": [
            { "event": "join", "replyEvent": "joined", "replyArgs": "{\"room\": {{event.arg0}}}" },
        ] } });
    let id: String = serde_json::from_value(h.ok("server.create", json!({ "server": server })).await).unwrap();
    let server = h.ok("server.read", json!({ "id": id })).await;
    let info = h.ok("server.start", json!({ "id": id, "server": server })).await;
    let url = info["url"].as_str().unwrap().to_string();

    let request = json!({ "name": "Rooms", "kind": "socketio", "url": format!("{url}/chat"),
        "socketio": {
            "messages": [{ "name": "Join", "event": "join", "text": "lobby" }],
            "onConnect": [{ "type": "send", "message": "Join" }, { "type": "wait", "event": "joined" }],
            "onMessage": [
                { "event": "joined", "match": "json", "pattern": "$[0].room == lobby",
                  "script": "pm.test('room', () => pm.expect(pm.message.args[0].room).to.eql('lobby'));\npm.connection.emit('hello', pm.message.args[0].room, 1);" },
            ],
        } });
    h.ok("socket.connect", json!({ "connId": "io", "request": request, "path": null })).await;
    let hello = |evs: &[StreamEvent]| {
        evs.iter().any(|e| matches!(e, StreamEvent::Socket { event: SocketEvent::Message { direction: Direction::Sent, topic: Some(t), text: Some(x), .. }, .. } if t == "hello" && x == r#"["lobby",1]"#))
    };
    h.wait_for("the emit from the script", hello).await;
    h.ok("socket.close", json!({ "connId": "io" })).await;

    let text = h.run(request).await;
    assert!(text.contains(r#"On the event \"joined\" with arguments where $[0].room == lobby: room"#), "{text}");
}

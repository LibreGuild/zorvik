//! Socket.IO through the API: a Socket.IO server started from the workspace and a
//! `socketio` request talking to it (auth payload with variables, emits with
//! acknowledgements, emits from the server's side, a refused namespace URL).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};
use zorvik_api::{Api, EventSink, StreamEvent};
use zorvik_engine::{Direction, SocketEvent};

#[derive(Default)]
struct Collect(Mutex<Vec<StreamEvent>>);

impl EventSink for Collect {
    fn emit(&self, event: StreamEvent) {
        self.0.lock().unwrap().push(event);
    }
}

fn received(events: &Collect, conn: &str) -> Vec<(Option<String>, Value, Option<String>)> {
    events
        .0
        .lock()
        .unwrap()
        .iter()
        .filter_map(|e| match e {
            StreamEvent::Socket {
                conn_id,
                event: SocketEvent::Message { direction: Direction::Received, text, topic, detail, .. },
            } if conn_id == conn => Some((topic.clone(), serde_json::from_str(text.as_deref()?).ok()?, detail.clone())),
            _ => None,
        })
        .collect()
}

async fn wait_for(events: &Collect, conn: &str, count: usize) -> Vec<(Option<String>, Value, Option<String>)> {
    for _ in 0..250 {
        let list = received(events, conn);
        if list.len() >= count {
            return list;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("{count} messages did not arrive: {:?}", events.0.lock().unwrap());
}

#[tokio::test]
async fn a_socketio_request_talks_to_a_socketio_server() {
    let data = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(Collect::default());
    let api = Api::new(data.path().to_path_buf(), events.clone());
    let call = |method: &'static str, params: Value| {
        let api = &api;
        async move {
            match api.call(method, params).await {
                Ok(v) => v,
                Err(e) => panic!("{method} failed: {} ({})", e.message, e.code),
            }
        }
    };
    call("workspace.create", json!({ "path": dir.path(), "name": "IO" })).await;
    let env = call(
        "env.create",
        json!({ "environment": { "name": "Dev", "variables": [{ "key": "token", "value": "t-1" }] } }),
    )
    .await;
    call("env.setActive", json!({ "id": env })).await;

    let server = json!({ "name": "IO", "kind": "socketio", "port": 0,
        "socketio": { "mode": "rules", "rules": [{ "event": "save", "ack": "{\"saved\": {{event.arg0}}}" }] } });
    let id: String = serde_json::from_value(call("server.create", json!({ "server": server })).await).unwrap();
    let server = call("server.read", json!({ "id": id })).await;
    let info = call("server.start", json!({ "id": id, "server": server })).await;
    let url = info["url"].as_str().unwrap().to_string();
    let run_id = info["runId"].as_str().unwrap().to_string();
    assert!(url.starts_with("http://127.0.0.1:"), "{url}");

    let request = json!({ "name": "io", "kind": "socketio", "url": format!("{url}/orders"),
        "socketio": { "auth": "{\"token\": \"{{token}}\"}", "event": "save" } });
    let opened = call("socket.connect", json!({ "connId": "io", "request": request, "path": null })).await;
    assert_eq!(opened["opened"]["protocol"], "Socket.IO over WebSocket");

    call(
        "socket.send",
        json!({ "connId": "io", "message": { "type": "emit", "event": "save", "args": "{\"id\": 7}", "ack": true } }),
    )
    .await;
    let list = wait_for(&events, "io", 1).await;
    assert_eq!(list[0], (None, json!([{ "saved": { "id": 7 } }]), Some("acknowledgement of #1".into())));

    // The server's log has the auth payload with the variable filled in.
    let log = call("server.log", json!({ "runId": run_id })).await;
    let joined = log.as_array().unwrap().iter().find(|e| e["summary"] == "joined /orders").cloned().expect("joined");
    assert_eq!(joined["text"], r#"{"token":"t-1"}"#);

    // An emit from the server's side.
    let sent = call("server.send", json!({ "runId": run_id, "message": { "type": "emit", "event": "news", "args": "[1, 2]", "namespace": "/orders" } })).await;
    assert_eq!(sent, 1);
    let list = wait_for(&events, "io", 2).await;
    assert_eq!(list[1], (Some("news".into()), json!([1, 2]), None));

    // Emitting needs an event name and JSON arguments.
    let e = api
        .call("socket.send", json!({ "connId": "io", "message": { "type": "emit", "event": " ", "args": "" } }))
        .await
        .unwrap_err();
    assert_eq!(e.message, "Enter the name of the event to emit");
    let e = api
        .call("socket.send", json!({ "connId": "io", "message": { "type": "emit", "event": "x", "args": "{oops" } }))
        .await
        .unwrap_err();
    assert!(e.message.starts_with("The arguments are not valid JSON"), "{}", e.message);

    call("socket.close", json!({ "connId": "io" })).await;
    call("server.stop", json!({ "runId": run_id })).await;

    // Invalid auth JSON is reported before connecting.
    let bad = json!({ "name": "io", "kind": "socketio", "url": url, "socketio": { "auth": "{oops" } });
    let e = api.call("socket.connect", json!({ "connId": "bad", "request": bad, "path": null })).await.unwrap_err();
    assert!(e.message.starts_with("The auth payload is not valid JSON"), "{}", e.message);
}

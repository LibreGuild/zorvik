//! gRPC through the RPC API: describe (reflection and `.proto` files), unary
//! calls with inherited metadata, auth and variables, history, cancelling,
//! and streaming sessions with their events.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};
use zorvik_api::{Api, EventSink, StreamEvent};
use zorvik_engine::GrpcEvent;
use zorvik_engine::grpc::GrpcStatus;
use zorvik_testkit::GrpcTestServer;
use zorvik_testkit::grpc::{ECHO_PROTO, Reflection, grpc_proto_dir};

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
    ws_dir: tempfile::TempDir,
}

impl Harness {
    async fn new() -> Self {
        let data = tempfile::tempdir().unwrap();
        let ws_dir = tempfile::tempdir().unwrap();
        let events = Arc::new(Collect::default());
        let api = Api::new(data.path().to_path_buf(), events.clone());
        let h = Self { api, events, _data: data, ws_dir };
        h.ok("workspace.create", json!({ "path": h.ws_dir.path(), "name": "Test" })).await;
        h
    }

    async fn ok(&self, method: &str, params: Value) -> Value {
        match self.api.call(method, params).await {
            Ok(v) => v,
            Err(e) => panic!("{method} failed: {} ({})", e.message, e.code),
        }
    }

    async fn err(&self, method: &str, params: Value) -> zorvik_api::ApiError {
        self.api.call(method, params).await.expect_err("expected an error")
    }

    /// Events of a session, once its `end` event arrived.
    async fn session_events(&self, session_id: &str) -> Vec<GrpcEvent> {
        for _ in 0..250 {
            let events = self.grpc_events(session_id);
            if events.iter().any(|e| matches!(e, GrpcEvent::End { .. })) {
                return events;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("no end event for {session_id}: {:?}", self.grpc_events(session_id));
    }

    fn grpc_events(&self, session_id: &str) -> Vec<GrpcEvent> {
        let mut out = Vec::new();
        let events = self.events.0.lock().unwrap();
        for event in events.iter() {
            if let StreamEvent::Grpc { session_id: id, event } = event
                && id == session_id
            {
                out.push(event.clone());
            }
        }
        out
    }
}

fn grpc_request(url: &str, method: &str, message: &str) -> Value {
    json!({ "name": "g", "kind": "grpc", "seq": 0, "method": method, "url": url, "body": { "type": "json", "text": message } })
}

fn end_status(events: &[GrpcEvent]) -> GrpcStatus {
    events
        .iter()
        .find_map(|e| match e {
            GrpcEvent::End { status, .. } => Some(status.clone()),
            _ => None,
        })
        .unwrap()
}

fn received(events: &[GrpcEvent]) -> Vec<Value> {
    events
        .iter()
        .filter_map(|e| match e {
            GrpcEvent::Message { message } if message.direction == zorvik_engine::Direction::Received => {
                Some(serde_json::from_str(&message.json).unwrap())
            }
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn describe_with_reflection_and_proto_files() {
    let server = GrpcTestServer::start().await;
    let h = Harness::new().await;
    let env = h
        .ok("env.create", json!({ "environment": { "name": "Dev", "variables": [{ "key": "grpcHost", "value": server.addr.to_string() }] } }))
        .await;
    h.ok("env.setActive", json!({ "id": env })).await;

    let request = grpc_request("grpc://{{grpcHost}}", "", "{}");
    let described = h.ok("grpc.describe", json!({ "request": request })).await;
    assert_eq!(described["source"], "reflection v1");
    assert_eq!(described["services"][0]["name"], "zorvik.test.v1.Echo");
    let methods = described["services"][0]["methods"].as_array().unwrap();
    assert_eq!(methods.len(), 6);
    assert_eq!(methods[1]["path"], "zorvik.test.v1.Echo/ServerStream");
    assert_eq!(methods[1]["serverStreaming"], true);
    assert!(methods[0]["example"].as_str().unwrap().contains("\"message\": \"\""));

    // Undefined host variables fail before any network traffic.
    let err = h.err("grpc.describe", json!({ "request": grpc_request("grpc://{{nope}}", "", "{}") })).await;
    assert_eq!(err.code, "undefinedVariable");

    // .proto files: relative to the workspace, imports found through an import folder.
    let protos = h.ws_dir.path().join("protos");
    for file in [ECHO_PROTO, "zorvik/test/v1/common.proto"] {
        let to = protos.join(file);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::copy(grpc_proto_dir().join(file), to).unwrap();
    }
    let mut request = grpc_request("", "", "{}");
    request["grpc"] = json!({ "protoFiles": [format!("protos/{ECHO_PROTO}")], "importPaths": ["protos"] });
    let described = h.ok("grpc.describe", json!({ "request": request })).await;
    assert_eq!(described["source"], "proto files");
    assert_eq!(described["services"][0]["methods"].as_array().unwrap().len(), 6);

    // Files outside the workspace need the setting, like body files.
    let outside = grpc_proto_dir().join(ECHO_PROTO);
    request["grpc"] = json!({ "protoFiles": [outside], "importPaths": [grpc_proto_dir()] });
    let err = h.err("grpc.describe", json!({ "request": request })).await;
    assert!(err.message.contains("outside the workspace"), "{}", err.message);
    let mut settings = h.ok("settings.get", json!({})).await;
    settings["filesOutsideWorkspace"] = json!(true);
    h.ok("settings.save", json!({ "settings": settings })).await;
    let described = h.ok("grpc.describe", json!({ "request": request })).await;
    assert_eq!(described["source"], "proto files");
}

#[tokio::test]
async fn unary_calls_carry_metadata_auth_and_variables_and_land_in_history() {
    let server = GrpcTestServer::start().await;
    let h = Harness::new().await;
    let info = h.ok("workspace.current", json!({})).await;
    let mut meta = info["meta"].clone();
    meta["headers"] = json!([{ "key": "X-Team", "value": "core" }]);
    meta["auth"] = json!({ "type": "bearer", "token": "{{token}}" });
    meta["variables"] = json!([{ "key": "token", "value": "t0k" }, { "key": "who", "value": "world" }]);
    h.ok("workspace.saveMeta", json!({ "meta": meta })).await;

    let url = server.url();
    let mut request = grpc_request(&url, "zorvik.test.v1.Echo/Metadata", "{}");
    request["headers"] = json!([{ "key": "x-api-key", "value": "k-{{who}}" }]);
    let result = h.ok("grpc.invoke", json!({ "requestId": "r1", "request": request })).await;
    assert_eq!(result["status"]["name"], "OK");
    let seen: Value = serde_json::from_str(result["messages"][0]["json"].as_str().unwrap()).unwrap();
    assert_eq!(seen["metadata"]["x-team"], "core");
    assert_eq!(seen["metadata"]["x-api-key"], "k-world");
    assert_eq!(seen["metadata"]["authorization"], "Bearer t0k");
    assert!(result["trailers"].as_array().unwrap().iter().any(|t| t["name"] == "x-echo-trailer"));

    // Variables in the message; undefined ones are reported.
    let request = grpc_request(&url, "zorvik.test.v1.Echo/Unary", r#"{"message": "hello {{who}} {{missing}}"}"#);
    let result = h.ok("grpc.invoke", json!({ "requestId": "r2", "request": request, "path": null })).await;
    let reply: Value = serde_json::from_str(result["messages"][0]["json"].as_str().unwrap()).unwrap();
    assert_eq!(reply["message"], "hello world {{missing}}");
    assert_eq!(result["unresolved"], json!(["missing"]));

    // A failing call is a result (not an RPC error), recorded in history as an error.
    let request = grpc_request(&url, "zorvik.test.v1.Echo/Fail", r#"{"message": "gone"}"#);
    let result = h.ok("grpc.invoke", json!({ "requestId": "r3", "request": request })).await;
    assert_eq!(result["status"]["name"], "NOT_FOUND");
    assert_eq!(result["status"]["message"], "gone");

    let history = h.ok("history.list", json!({})).await;
    let history = history.as_array().unwrap();
    assert_eq!(history.len(), 3);
    assert_eq!(history[0]["error"], "NOT_FOUND: gone");
    assert!(history[0]["status"].is_null());
    assert_eq!(history[1]["status"], 200);
    assert_eq!(history[1]["method"], "zorvik.test.v1.Echo/Unary");
    assert_eq!(history[1]["request"]["kind"], "grpc");

    // Streaming methods need grpc.start; unknown methods are named.
    let request = grpc_request(&url, "zorvik.test.v1.Echo/Bidi", "{}");
    let err = h.err("grpc.invoke", json!({ "requestId": "r4", "request": request })).await;
    assert!(err.message.contains("streaming"), "{}", err.message);
    let request = grpc_request(&url, "zorvik.test.v1.Echo/Nope", "{}");
    let err = h.err("grpc.invoke", json!({ "requestId": "r5", "request": request })).await;
    assert!(err.message.contains("no method 'Nope'"), "{}", err.message);
}

#[tokio::test]
async fn cancelling_a_unary_call() {
    let server = GrpcTestServer::bind("127.0.0.1:0".parse().unwrap(), None, Reflection::Both).await;
    let h = Harness::new().await;
    let request = grpc_request(&server.url(), "zorvik.test.v1.Echo/Unary", r#"{"delayMs": "5000"}"#);
    let api = h.api.clone();
    let call =
        tokio::spawn(async move { api.call("grpc.invoke", json!({ "requestId": "slow", "request": request })).await });
    tokio::time::sleep(Duration::from_millis(300)).await;
    h.ok("http.cancel", json!({ "requestId": "slow" })).await;
    let err = tokio::time::timeout(Duration::from_secs(3), call).await.unwrap().unwrap().unwrap_err();
    assert_eq!(err.network_kind, Some(zorvik_engine::ErrorKind::Cancelled));
    // Cancelled calls are not history.
    assert_eq!(h.ok("history.list", json!({})).await.as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn streaming_sessions() {
    let server = GrpcTestServer::start().await;
    let h = Harness::new().await;
    let url = server.url();

    // Server streaming: the message is sent by start.
    let request = grpc_request(&url, "zorvik.test.v1.Echo/ServerStream", r#"{"message": "n", "count": 2}"#);
    let started = h.ok("grpc.start", json!({ "sessionId": "s1", "request": request })).await;
    assert_eq!(started["method"]["serverStreaming"], true);
    assert_eq!(started["method"]["clientStreaming"], false);
    let events = h.session_events("s1").await;
    assert!(end_status(&events).is_ok());
    let texts: Vec<_> = received(&events).iter().map(|m| m["message"].clone()).collect();
    assert_eq!(texts, [json!("n #1"), json!("n #2")]);
    assert!(matches!(events[0], GrpcEvent::Message { .. }), "the sent message comes first");
    let err = h.err("grpc.send", json!({ "sessionId": "s1", "message": "{}" })).await;
    assert_eq!(err.code, "notFound");

    // Bidi: send (with variables), end, and the answers arrive as events.
    let request = grpc_request(&url, "zorvik.test.v1.Echo/Bidi", "");
    h.ok("grpc.start", json!({ "sessionId": "s2", "request": request })).await;
    h.ok("grpc.send", json!({ "sessionId": "s2", "message": r#"{"message": "one {{$randomInt}}"}"# })).await;
    let err = h.err("grpc.send", json!({ "sessionId": "s2", "message": r#"{"bad": 1}"# })).await;
    assert!(err.message.contains("bad"), "{}", err.message);
    h.ok("grpc.send", json!({ "sessionId": "s2", "message": r#"{"message": "two"}"# })).await;
    h.ok("grpc.end", json!({ "sessionId": "s2" })).await;
    let events = h.session_events("s2").await;
    assert!(end_status(&events).is_ok());
    let answers = received(&events);
    assert_eq!(answers.len(), 2);
    assert!(
        answers[0]["message"].as_str().unwrap().starts_with("one ")
            && !answers[0]["message"].as_str().unwrap().contains("{{")
    );
    assert_eq!(answers[1]["message"], "two");

    // Cancel: grpc.cancel, or http.cancel with the start's requestId (the UI's Cancel button).
    for (id, cancel) in [("s3", "grpc.cancel"), ("s4", "http.cancel")] {
        let request = grpc_request(&url, "zorvik.test.v1.Echo/Bidi", "");
        h.ok("grpc.start", json!({ "sessionId": id, "requestId": "tab-1", "request": request })).await;
        let params = if cancel == "grpc.cancel" { json!({ "sessionId": id }) } else { json!({ "requestId": "tab-1" }) };
        h.ok(cancel, params).await;
        let events = h.session_events(id).await;
        assert_eq!(end_status(&events).name, "CANCELLED", "{cancel}");
    }

    // Start errors are RPC errors: an invalid first message fails before connecting.
    let request = grpc_request(&url, "zorvik.test.v1.Echo/ServerStream", r#"{"message": 5}"#);
    let err = h.err("grpc.start", json!({ "sessionId": "s5", "request": request })).await;
    assert!(err.message.contains("EchoRequest"), "{}", err.message);
    assert!(h.grpc_events("s5").is_empty());
}

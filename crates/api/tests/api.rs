//! Drive the RPC API exactly like the UI does, against local test servers.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};
use zorvik_api::{Api, EventSink, StreamEvent};
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

    async fn wait_for(&self, pred: impl Fn(&[StreamEvent]) -> bool) {
        for _ in 0..200 {
            if pred(&self.events.0.lock().unwrap()) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("event not received: {:?}", self.events.0.lock().unwrap());
    }
}

fn http_request(url: &str) -> Value {
    json!({ "name": "r", "seq": 0, "method": "GET", "url": url })
}

#[tokio::test]
async fn send_with_environment_secrets_and_history() {
    let server = TestServer::start().await;
    let h = Harness::new().await;

    let env_id = h
        .ok(
            "env.create",
            json!({ "environment": { "name": "Dev", "variables": [
                { "key": "base", "value": server.url("") },
                { "key": "token", "value": "s3cret", "secret": true }
            ] } }),
        )
        .await;
    h.ok("env.setActive", json!({ "id": env_id })).await;

    // Secret value is not written to the workspace file.
    let env_file = std::fs::read_to_string(h.ws_dir.path().join("environments/Dev.yaml")).unwrap();
    assert!(!env_file.contains("s3cret"), "{env_file}");
    let envs = h.ok("env.list", json!({})).await;
    assert_eq!(envs[0]["environment"]["variables"][1]["value"], "s3cret");
    let vars = h.ok("vars.list", json!({})).await;
    assert_eq!(vars.as_array().unwrap().iter().find(|v| v["key"] == "token").unwrap()["value"], "••••••");

    let mut req = http_request("{{base}}/bearer?missing={{nope}}");
    req["auth"] = json!({ "type": "bearer", "token": "{{token}}", "prefix": "Bearer" });
    let result = h.ok("http.send", json!({ "requestId": "t1", "request": req })).await;
    assert_eq!(result["meta"]["status"], 200);
    assert!(result["body"]["pretty"].as_str().unwrap().contains("s3cret"));
    assert_eq!(result["unresolved"], json!(["nope"]));

    let history = h.ok("history.list", json!({})).await;
    let entry = &history[0];
    assert_eq!(entry["status"], 200);
    // History keeps the unresolved request (no secret values).
    assert_eq!(entry["request"]["auth"]["token"], "{{token}}");

    // A secret in the URL (e.g. an API key in the query) is not stored in history either.
    let mut req = http_request("{{base}}/echo?key={{token}}");
    req["auth"] = json!({ "type": "apiKey", "key": "k2", "value": "{{token}}", "location": "query" });
    h.ok("http.send", json!({ "requestId": "t2", "request": req })).await;
    let history = h.ok("history.list", json!({})).await;
    let url = history[0]["url"].as_str().unwrap();
    assert!(url.ends_with("/echo?key={{token}}&k2={{token}}"), "{url}");
}

#[tokio::test]
async fn secrets_stay_with_the_workspace_folder() {
    let h = Harness::new().await;
    let secret = json!({ "environment": { "name": "Dev", "variables": [
        { "key": "token", "value": "s3cret", "secret": true }
    ] } });
    h.ok("env.create", secret).await;

    // A copy with the same id (e.g. a hostile repo that copied zorvik.yaml) gets no secrets.
    let copy = tempfile::tempdir().unwrap();
    for file in ["zorvik.yaml", "environments/Dev.yaml"] {
        std::fs::create_dir_all(copy.path().join("environments")).unwrap();
        std::fs::copy(h.ws_dir.path().join(file), copy.path().join(file)).unwrap();
    }
    h.ok("workspace.open", json!({ "path": copy.path() })).await;
    let envs = h.ok("env.list", json!({})).await;
    assert_eq!(envs[0]["environment"]["variables"][0]["value"], "");

    h.ok("workspace.open", json!({ "path": h.ws_dir.path() })).await;
    let envs = h.ok("env.list", json!({})).await;
    assert_eq!(envs[0]["environment"]["variables"][0]["value"], "s3cret");
}

#[tokio::test]
async fn collection_crud_and_inherited_folder_auth() {
    let server = TestServer::start().await;
    let h = Harness::new().await;
    let folder = h.ok("folder.create", json!({ "parent": "", "name": "Secured" })).await;
    let folder = folder.as_str().unwrap();
    h.ok(
        "folder.save",
        json!({ "path": folder, "meta": { "name": "Secured", "seq": 1, "auth": { "type": "basic", "username": "u", "password": "p" } } }),
    )
    .await;
    let path = h
        .ok("request.create", json!({ "parent": folder, "request": http_request(&server.url("/basic-auth/u/p")) }))
        .await;
    let path = path.as_str().unwrap().to_string();
    let saved = h.ok("request.read", json!({ "path": path })).await;
    let result = h.ok("http.send", json!({ "requestId": "x", "request": saved, "path": path })).await;
    assert_eq!(result["meta"]["status"], 200, "folder auth should be inherited");

    let renamed = h.ok("item.rename", json!({ "path": path, "name": "Login check" })).await;
    assert_eq!(renamed, "Secured/Login check.yaml");
    let tree = h.ok("workspace.tree", json!({})).await;
    assert_eq!(tree[0]["children"][0]["name"], "Login check");
    let err = h.err("request.read", json!({ "path": "../zorvik.yaml" })).await;
    assert_eq!(err.code, "invalidInput");
}

#[tokio::test]
async fn cancel_in_flight_request() {
    let server = TestServer::start().await;
    let h = Harness::new().await;
    let api = h.api.clone();
    let url = server.url("/delay/5000");
    let pending = tokio::spawn(async move {
        api.call("http.send", json!({ "requestId": "slow", "request": http_request(&url) })).await
    });
    tokio::time::sleep(Duration::from_millis(200)).await;
    h.ok("http.cancel", json!({ "requestId": "slow" })).await;
    let err = tokio::time::timeout(Duration::from_secs(2), pending).await.unwrap().unwrap().unwrap_err();
    assert_eq!(err.network_kind, Some(zorvik_engine::ErrorKind::Cancelled));
}

#[tokio::test]
async fn cookies_are_kept_per_workspace() {
    let server = TestServer::start().await;
    let h = Harness::new().await;
    h.ok("http.send", json!({ "requestId": "a", "request": http_request(&server.url("/cookies/set?flavor=oat")) }))
        .await;
    let cookies = h.ok("cookies.list", json!({})).await;
    assert_eq!(cookies[0]["name"], "flavor");
    let result = h.ok("http.send", json!({ "requestId": "b", "request": http_request(&server.url("/cookies")) })).await;
    assert!(result["body"]["text"].as_str().unwrap().contains("oat"));
    h.ok("cookies.clear", json!({})).await;
    assert_eq!(h.ok("cookies.list", json!({})).await, json!([]));
}

#[tokio::test]
async fn websocket_and_sse_stream_events() {
    let server = TestServer::start().await;
    let h = Harness::new().await;
    let mut ws_req = http_request(&server.ws_url("/ws"));
    ws_req["kind"] = json!("websocket");
    let opened = h.ok("ws.connect", json!({ "connId": "c1", "request": ws_req })).await;
    assert_eq!(opened["meta"]["status"], 101);
    h.ok("ws.send", json!({ "connId": "c1", "message": { "type": "text", "text": "ping-me" } })).await;
    h.wait_for(|evs| {
        evs.iter()
            .filter(|e| matches!(e, StreamEvent::Ws { event: zorvik_engine::WsEvent::Message { text: Some(t), .. }, .. } if t == "ping-me"))
            .count()
            == 2
    })
    .await;
    h.ok("ws.close", json!({ "connId": "c1" })).await;
    h.wait_for(|evs| {
        evs.iter().any(|e| matches!(e, StreamEvent::Ws { event: zorvik_engine::WsEvent::Closed { .. }, .. }))
    })
    .await;

    let mut sse_req = http_request(&server.url("/sse?count=3&interval=5"));
    sse_req["kind"] = json!("sse");
    h.ok("sse.connect", json!({ "connId": "s1", "request": sse_req })).await;
    h.wait_for(|evs| {
        evs.iter().any(|e| matches!(e, StreamEvent::Sse { event: zorvik_api::SseStreamEvent::Closed { .. }, .. }))
    })
    .await;
    let count = h
        .events
        .0
        .lock()
        .unwrap()
        .iter()
        .filter(|e| matches!(e, StreamEvent::Sse { event: zorvik_api::SseStreamEvent::Event { .. }, .. }))
        .count();
    assert_eq!(count, 3);
}

#[tokio::test]
async fn closing_a_stream_while_connecting_cancels_it() {
    // The UI closes a "connecting" stream when the user disconnects, closes the tab or
    // switches workspace. The close used to be ignored and the stream opened anyway (leaked).
    let server = TestServer::start().await;
    let h = Harness::new().await;
    for (kind, connect, close) in [("sse", "sse.connect", "sse.close"), ("websocket", "ws.connect", "ws.close")] {
        let mut req = http_request(&server.url("/delay/5000"));
        req["kind"] = json!(kind);
        let api = h.api.clone();
        let pending = tokio::spawn(async move { api.call(connect, json!({ "connId": "slow", "request": req })).await });
        tokio::time::sleep(Duration::from_millis(200)).await;
        h.ok(close, json!({ "connId": "slow" })).await;
        let result = tokio::time::timeout(Duration::from_secs(2), pending).await.expect(kind).unwrap();
        assert_eq!(result.unwrap_err().network_kind, Some(zorvik_engine::ErrorKind::Cancelled), "{kind}");
    }
    let events = h.events.0.lock().unwrap();
    assert!(
        !events.iter().any(|e| matches!(e, StreamEvent::Ws { .. } | StreamEvent::Sse { .. })),
        "no events from cancelled streams: {events:?}"
    );
}

#[tokio::test]
async fn reconnecting_with_the_same_id_keeps_the_newest_connection() {
    let server = TestServer::start().await;
    let h = Harness::new().await;
    let mut slow = http_request(&server.ws_url("/ws?delay=400"));
    slow["kind"] = json!("websocket");
    let api = h.api.clone();
    let first = tokio::spawn(async move { api.call("ws.connect", json!({ "connId": "c", "request": slow })).await });
    tokio::time::sleep(Duration::from_millis(100)).await;
    let mut fast = http_request(&server.ws_url("/ws"));
    fast["kind"] = json!("websocket");
    h.ok("ws.connect", json!({ "connId": "c", "request": fast })).await;
    // The replaced attempt is cancelled instead of taking over (and then tearing down) the new one.
    let err = tokio::time::timeout(Duration::from_secs(2), first).await.unwrap().unwrap().unwrap_err();
    assert_eq!(err.network_kind, Some(zorvik_engine::ErrorKind::Cancelled));
    tokio::time::sleep(Duration::from_millis(500)).await;
    h.ok("ws.send", json!({ "connId": "c", "message": { "type": "text", "text": "still-here" } })).await;
    h.wait_for(|evs| {
        evs.iter().any(|e| matches!(e, StreamEvent::Ws { event: zorvik_engine::WsEvent::Message { direction: zorvik_engine::Direction::Received, text: Some(t), .. }, .. } if t == "still-here"))
    })
    .await;
}

#[tokio::test]
async fn oauth_sign_in_only_opens_web_urls() {
    // authUrl comes from workspace files (shared via Git); never hand file:// or custom schemes to the OS.
    let h = Harness::new().await;
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let auth = json!({
        "type": "oauth2", "grantType": "authorizationCode", "authUrl": "file:///etc/passwd",
        "tokenUrl": "http://127.0.0.1:1/token", "clientId": "c",
        "redirectUri": format!("http://127.0.0.1:{port}/callback"),
    });
    let err = h.err("oauth2.getToken", json!({ "auth": auth })).await;
    assert!(err.message.contains("http:// or https://"), "{}", err.message);
    assert!(!h.events.0.lock().unwrap().iter().any(|e| matches!(e, StreamEvent::OpenUrl { .. })));
}

#[tokio::test]
async fn import_postman_and_export_curl() {
    let h = Harness::new().await;
    let collection = json!({
        "info": { "name": "Demo API", "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json" },
        "item": [
            { "name": "Users", "item": [
                { "name": "List users", "request": { "method": "GET", "url": "{{host}}/users", "header": [{ "key": "Accept", "value": "application/json" }] } }
            ] }
        ],
        "variable": [{ "key": "host", "value": "https://api.example.com" }, { "key": "key", "value": "s3cret", "type": "secret" }]
    });
    let summary = h.ok("import.file", json!({ "text": collection.to_string(), "parent": "" })).await;
    assert_eq!(summary["requests"], 1);
    // Collection variables are workspace variables (what pm.collectionVariables uses), not an environment.
    assert_eq!((summary["environments"].clone(), summary["workspaceVariables"].clone()), (json!(0), json!(2)));
    let meta = h.ok("workspace.current", json!({})).await["meta"].clone();
    assert!(
        meta["variables"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["key"] == "host" && v["value"] == "https://api.example.com")
    );
    let file = std::fs::read_to_string(h.ws_dir.path().join("zorvik.yaml")).unwrap();
    assert!(file.contains("host") && !file.contains("s3cret"), "secret values stay out of the file: {file}");
    // A second import keeps what the workspace already has, and says so.
    let mut changed = collection.clone();
    changed["variable"][0]["value"] = json!("https://other.example.com");
    let again = h.ok("import.file", json!({ "text": changed.to_string(), "parent": "" })).await;
    assert_eq!(again["workspaceVariables"], 0);
    assert!(again["warnings"].to_string().contains("host"), "{again}");
    let tree = h.ok("workspace.tree", json!({})).await;
    assert_eq!(tree[0]["name"], "Demo API");
    let path = tree[0]["children"][0]["children"][0]["path"].as_str().unwrap().to_string();
    let req = h.ok("request.read", json!({ "path": path })).await;
    let curl =
        h.ok("export.curl", json!({ "request": req, "path": path, "flavor": "bash", "resolveVariables": false })).await;
    let curl = curl.as_str().unwrap();
    // Braces are escaped so curl's URL globbing leaves them alone.
    assert!(curl.contains(r"\{\{host\}\}/users"), "{curl}");
    assert!(curl.contains("Accept: application/json"), "{curl}");

    let parsed = h.ok("import.curl", json!({ "text": "curl -X POST https://x.test/a -H 'X-A: 1' -d '{\"k\":1}' -H 'Content-Type: application/json'" })).await;
    assert_eq!(parsed["request"]["method"], "POST");
    assert_eq!(parsed["request"]["body"]["type"], "json");

    let err = h.err("import.file", json!({ "text": "hello world", "parent": "" })).await;
    assert!(err.message.contains("Unrecognized"));
}

#[tokio::test]
async fn settings_round_trip_and_validation() {
    let h = Harness::new().await;
    let mut settings = h.ok("settings.get", json!({})).await;
    settings["historyLimit"] = json!(42);
    h.ok("settings.save", json!({ "settings": settings })).await;
    assert_eq!(h.ok("settings.get", json!({})).await["historyLimit"], 42);
    settings["proxy"] = json!({ "mode": "manual", "url": "socks5://x:1", "bypass": "" });
    let err = h.err("settings.save", json!({ "settings": settings })).await;
    assert_eq!(err.code, "invalidInput");
    assert!(h.err("nope.method", json!({})).await.message.contains("Unknown method"));
}

#[tokio::test]
async fn tcp_server_and_client_through_the_api() {
    let h = Harness::new().await;
    let id: String = serde_json::from_value(
        h.ok(
            "server.create",
            json!({ "server": { "name": "Echo", "kind": "tcp", "port": 0, "socket": { "framing": "line" } } }),
        )
        .await,
    )
    .unwrap();
    let list = h.ok("server.list", json!({})).await;
    assert_eq!(list[0]["id"], id);
    let server = h.ok("server.read", json!({ "id": id })).await;
    let info = h.ok("server.start", json!({ "id": id, "server": server })).await;
    let run_id = info["runId"].as_str().unwrap().to_string();
    let port = info["port"].as_u64().unwrap();
    assert!(info["url"].as_str().unwrap().starts_with("tcp://127.0.0.1:"));
    // Starting it twice is refused.
    let err = h.err("server.start", json!({ "id": id, "server": server })).await;
    assert!(err.message.contains("already running"), "{}", err.message);

    // A TCP client request talks to it.
    let request = json!({ "name": "c", "kind": "tcp", "url": format!("tcp://127.0.0.1:{port}"),
        "socket": { "framing": "line", "lineEnding": "lf" } });
    let opened = h.ok("socket.connect", json!({ "connId": "c1", "request": request })).await;
    assert_eq!(opened["opened"]["protocol"], "TCP");
    h.ok("socket.send", json!({ "connId": "c1", "message": { "type": "text", "text": "hello" } })).await;
    h.wait_for(|events| {
        events.iter().any(|e| matches!(e, StreamEvent::Socket { event: zorvik_engine::SocketEvent::Message { direction: zorvik_engine::Direction::Received, text: Some(t), .. }, .. } if t == "hello"))
    })
    .await;
    let log = h.ok("server.log", json!({ "runId": run_id })).await;
    assert!(log.as_array().unwrap().iter().any(|e| e["text"] == "hello" && e["direction"] == "in"), "{log}");
    let running = h.ok("server.running", json!({})).await;
    assert_eq!(running[0]["name"], "Echo");

    h.ok("socket.close", json!({ "connId": "c1" })).await;
    h.ok("server.stop", json!({ "runId": run_id })).await;
    h.wait_for(|events| {
        events.iter().any(|e| {
            matches!(e, StreamEvent::Server { event: zorvik_servers::ServerEvent::Stopped { error: None }, .. })
        })
    })
    .await;
    assert_eq!(h.ok("server.running", json!({})).await, json!([]));
    assert_eq!(h.api.running_server_count(), 0);
}

/// "Start with workspace" only starts configurations this computer started or
/// saved: a server file that arrives through Git (clone, pull) waits for the
/// user to start it once.
#[tokio::test]
async fn auto_start_needs_a_configuration_started_here() {
    let h = Harness::new().await;
    let saved = json!({ "name": "Saved", "kind": "tcp", "port": 0, "autoStart": true });
    let saved_id = h.ok("server.create", json!({ "server": saved })).await;
    // Written by someone else (e.g. a Git pull), also listening on all addresses.
    std::fs::write(
        h.ws_dir.path().join("servers/Cloned.yaml"),
        "name: Cloned\nkind: tcp\nhost: 0.0.0.0\nport: 0\nautoStart: true\n",
    )
    .unwrap();

    let result = h.ok("server.autoStart", json!({})).await;
    let started: Vec<&str> =
        result["started"].as_array().unwrap().iter().map(|s| s["name"].as_str().unwrap()).collect();
    assert_eq!(started, ["Saved"]);
    assert_eq!(result["errors"].as_array().unwrap().len(), 1, "{result}");
    assert!(result["errors"][0].as_str().unwrap().starts_with("Cloned: not started automatically"), "{result}");
    h.ok("server.stopAll", json!({})).await;

    // Started once by the user: from now on it starts with the workspace.
    let cloned = h.ok("server.read", json!({ "id": "Cloned" })).await;
    let info = h.ok("server.start", json!({ "id": "Cloned", "server": cloned })).await;
    h.ok("server.stop", json!({ "runId": info["runId"] })).await;
    let result = h.ok("server.autoStart", json!({})).await;
    assert_eq!(result["started"].as_array().unwrap().len(), 2, "{result}");
    h.ok("server.stopAll", json!({})).await;

    // Changed outside the app: not trusted any more (and the choice survives a restart).
    let mut saved: Value = h.ok("server.read", json!({ "id": saved_id })).await;
    saved["socket"] = json!({ "greeting": "changed" });
    let yaml = "name: Saved\nkind: tcp\nport: 0\nautoStart: true\nsocket:\n  greeting: changed\n";
    std::fs::write(h.ws_dir.path().join("servers/Saved.yaml"), yaml).unwrap();
    let api = Api::new(h._data.path().to_path_buf(), h.events.clone());
    api.call("workspace.open", json!({ "path": h.ws_dir.path() })).await.unwrap();
    let result = api.call("server.autoStart", json!({})).await.unwrap();
    let started: Vec<&str> =
        result["started"].as_array().unwrap().iter().map(|s| s["name"].as_str().unwrap()).collect();
    assert_eq!(started, ["Cloned"], "{result}");
    api.stop_all_servers();

    // Saving it in the app trusts the saved configuration.
    api.call("server.save", json!({ "id": saved_id, "server": saved })).await.unwrap();
    let result = api.call("server.autoStart", json!({})).await.unwrap();
    assert_eq!(result["started"].as_array().unwrap().len(), 2, "{result}");
    api.stop_all_servers();
}

/// Server templates see environment variables, but not secrets: a server
/// answers whoever connects to it.
#[tokio::test]
async fn server_templates_leave_secrets_out() {
    let h = Harness::new().await;
    let env_id = h
        .ok(
            "env.create",
            json!({ "environment": { "name": "Dev", "variables": [
                { "key": "who", "value": "dev" },
                { "key": "token", "value": "s3cret", "secret": true }
            ] } }),
        )
        .await;
    h.ok("env.setActive", json!({ "id": env_id })).await;
    let server = json!({ "name": "Greeter", "kind": "tcp", "port": 0,
        "socket": { "mode": "manual", "greeting": "hi {{who}} {{token}}" } });
    let id = h.ok("server.create", json!({ "server": server })).await;
    let info = h.ok("server.start", json!({ "id": id, "server": server })).await;
    let port = info["port"].as_u64().unwrap();
    let mut client = tokio::net::TcpStream::connect(("127.0.0.1", port as u16)).await.unwrap();
    let mut buf = vec![0u8; 64];
    let n = tokio::time::timeout(Duration::from_secs(5), tokio::io::AsyncReadExt::read(&mut client, &mut buf))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&buf[..n]), "hi dev {{token}}");
    h.ok("server.stop", json!({ "runId": info["runId"] })).await;
}

#[tokio::test]
async fn standalone_sends_ignore_workspace_headers_and_history() {
    let server = TestServer::start().await;
    let h = Harness::new().await;
    let meta = h.ok("workspace.current", json!({})).await["meta"].clone();
    let mut meta = meta;
    meta["headers"] = json!([{ "key": "X-Secret", "value": "s3" }]);
    h.ok("workspace.saveMeta", json!({ "meta": meta })).await;

    let req = http_request(&server.url("/echo"));
    let normal = h.ok("http.send", json!({ "requestId": "a", "request": req })).await;
    assert!(normal["body"]["pretty"].as_str().unwrap().to_lowercase().contains("x-secret"));
    let alone = h.ok("http.send", json!({ "requestId": "b", "request": req, "standalone": true })).await;
    assert!(!alone["body"]["pretty"].as_str().unwrap().to_lowercase().contains("x-secret"));
    let history = h.ok("history.list", json!({})).await;
    assert_eq!(history.as_array().unwrap().len(), 1, "standalone sends are not recorded");
}

#[tokio::test]
async fn openapi_without_a_server_asks_for_the_base_url() {
    let h = Harness::new().await;
    let api = &h.api;
    let doc = r#"{"openapi": "3.0.0", "info": {"title": "Sessions", "version": "1"}, "servers": [{"url": "/v1"}],
        "paths": {"/sessions/{session_id}": {"get": {"parameters": [{"name": "session_id", "in": "path", "required": true,
        "schema": {"type": "string"}, "example": "s-42"}], "responses": {"200": {"description": "ok"}}}}}}"#;
    let err = api.call("import.file", serde_json::json!({ "text": doc })).await.unwrap_err();
    assert_eq!(err.code, "needsBaseUrl", "{}", err.message);
    assert!(err.message.contains("/v1"), "{}", err.message);
    // Nothing was written by the refused import.
    assert!(api.call("env.list", serde_json::Value::Null).await.unwrap().as_array().unwrap().is_empty());

    let summary =
        api.call("import.file", serde_json::json!({ "text": doc, "baseUrl": "http://localhost:8080/" })).await.unwrap();
    assert_eq!(summary["requests"], 1);
    let envs = api.call("env.list", serde_json::Value::Null).await.unwrap();
    let vars = &envs[0]["environment"]["variables"];
    let get = |k: &str| vars.as_array().unwrap().iter().find(|v| v["key"] == k).map(|v| v["value"].clone());
    assert_eq!(get("baseUrl"), Some(serde_json::json!("http://localhost:8080/v1")));
    assert_eq!(get("sessionId"), Some(serde_json::json!("s-42")));
}

/// A small OpenAPI document for the test server's `/json` (`{"id": 42, "name": "Zorvik", …}`).
fn slides_spec(version: u32) -> String {
    let (extra_path, required) = match version {
        1 => ("", r#"["id", "name"]"#),
        _ => (
            r#", "/status/{code}": {"get": {"summary": "Status", "parameters": [{"name": "code", "in": "path", "required": true, "schema": {"type": "integer"}, "example": 204}], "responses": {"204": {"description": "empty"}}}}"#,
            r#"["id", "name", "missing"]"#,
        ),
    };
    let gone = if version == 1 {
        r#", "/old": {"get": {"summary": "Old", "responses": {"200": {"description": "ok"}}}}"#
    } else {
        ""
    };
    format!(
        r#"{{"openapi": "3.0.0", "info": {{"title": "Slides", "version": "{version}"}}, "servers": [{{"url": "http://localhost:1"}}],
          "paths": {{
            "/json": {{"get": {{"summary": "Get slides", "description": "Docs v{version}", "responses": {{"200": {{"content": {{"application/json": {{"schema": {{
              "type": "object", "required": {required}, "properties": {{"id": {{"type": "integer"}}, "name": {{"type": "string"}}}}}}}}}}}}}}}}}}{extra_path}{gone}
          }}}}"#
    )
}

#[tokio::test]
async fn imported_requests_are_checked_and_updated_from_the_spec() {
    let server = TestServer::start().await;
    let h = Harness::new().await;
    let summary = h.ok("import.file", json!({ "text": slides_spec(1) })).await;
    let folder = summary["folderPath"].as_str().unwrap().to_string();
    // The document is kept, and the folder knows it.
    let meta = h.ok("folder.read", json!({ "path": folder })).await;
    let spec = meta["openapi"]["spec"].as_str().unwrap().to_string();
    assert!(spec.starts_with("specs/") && h.ws_dir.path().join(&spec).is_file(), "{meta}");

    // Point the requests at the test server and send: the response matches the spec.
    let env = h.ok("env.list", Value::Null).await;
    let env_id = env[0]["id"].as_str().unwrap().to_string();
    let mut environment = env[0]["environment"].clone();
    environment["variables"][0]["value"] = json!(server.url(""));
    h.ok("env.save", json!({ "id": env_id, "environment": environment })).await;
    h.ok("env.setActive", json!({ "id": env_id })).await;
    let tree = h.ok("workspace.tree", Value::Null).await;
    let get = find_request(&tree, "Get slides").expect("imported request");
    let request = h.ok("request.read", json!({ "path": get })).await;
    assert_eq!(request["openapi"]["operation"], "GET /json");
    let sent = h.ok("http.send", json!({ "requestId": "s1", "request": request, "path": get })).await;
    let test = &sent["scripts"]["tests"][0];
    assert_eq!(test["name"], "Matches the API spec (GET /json → 200)");
    assert_eq!(test["passed"], true, "{test}");

    // The user edits the docs; then the document changes the docs, adds an operation, drops one,
    // and now requires a field the response doesn't have.
    let mut edited = request.clone();
    edited["docs"] = json!("My notes");
    h.ok("request.save", json!({ "path": get, "request": edited })).await;
    let preview = h.ok("import.updatePreview", json!({ "folder": folder, "text": slides_spec(2) })).await;
    assert_eq!(preview["applied"], false);
    assert_eq!(preview["added"][0]["operation"], "GET /status/{code}");
    assert_eq!(preview["removed"][0]["operation"], "GET /old");
    assert_eq!(preview["changed"][0]["kept"], json!(["docs"]), "{preview}");
    // `{code}` is generic: named after the segment before it.
    assert_eq!(preview["variables"], json!(["statusCode"]));
    // A preview writes nothing.
    assert!(find_request(&h.ok("workspace.tree", Value::Null).await, "Status").is_none());

    let applied = h.ok("import.update", json!({ "folder": folder, "text": slides_spec(2) })).await;
    assert_eq!(applied["applied"], true);
    let tree = h.ok("workspace.tree", Value::Null).await;
    assert!(find_request(&tree, "Status").is_some());
    let old = find_request(&tree, "Old").expect("kept");
    assert_eq!(h.ok("request.read", json!({ "path": old })).await["openapi"]["removed"], true);
    assert_eq!(node(&tree, &old).unwrap()["removedFromSpec"], true);
    let request = h.ok("request.read", json!({ "path": get })).await;
    assert_eq!(request["docs"], "My notes", "the user's docs stay");
    // The kept document is the new one: the response no longer matches.
    let sent = h.ok("http.send", json!({ "requestId": "s2", "request": request, "path": get })).await;
    let test = &sent["scripts"]["tests"][0];
    assert_eq!(test["passed"], false);
    assert!(test["error"].as_str().unwrap().contains("required field `missing`"), "{test}");
    // The new path variable is in the environment, with the spec's example.
    let env = h.ok("env.list", Value::Null).await;
    let code =
        env[0]["environment"]["variables"].as_array().unwrap().iter().find(|v| v["key"] == "statusCode").cloned();
    assert_eq!(code.map(|v| v["value"].clone()), Some(json!("204")));
    // Switching the check off in the folder stops it.
    let mut meta = h.ok("folder.read", json!({ "path": folder })).await;
    meta["openapi"]["validate"] = json!(false);
    h.ok("folder.save", json!({ "path": folder, "meta": meta })).await;
    let sent = h.ok("http.send", json!({ "requestId": "s3", "request": request, "path": get })).await;
    assert!(sent["scripts"].is_null() || sent["scripts"]["tests"].as_array().unwrap().is_empty(), "{sent}");
}

fn node<'a>(nodes: &'a Value, path: &str) -> Option<&'a Value> {
    nodes.as_array()?.iter().find_map(|n| if n["path"] == path { Some(n) } else { node(&n["children"], path) })
}

fn find_request(nodes: &Value, name: &str) -> Option<String> {
    nodes.as_array()?.iter().find_map(|n| {
        if n["kind"] == "request" && n["name"] == name {
            n["path"].as_str().map(str::to_string)
        } else {
            find_request(&n["children"], name)
        }
    })
}

//! An agent end to end: MCP over the bridge (in memory) → the app listener →
//! the tools, with the user's answers given like the UI gives them.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, Lines, ReadHalf, WriteHalf};
use tokio::sync::mpsc;
use zorvik_api::agents::{AgentEvent, ConfirmKind};
use zorvik_api::{Api, EventSink, StreamEvent};
use zorvik_mcp::bridge::{BridgeOptions, serve};
use zorvik_testkit::TestServer;

/// Forwards the app's questions to the test.
struct Questions(mpsc::UnboundedSender<(String, ConfirmKind, Vec<String>)>);

impl EventSink for Questions {
    fn emit(&self, event: StreamEvent) {
        if let StreamEvent::Agent { event: AgentEvent::Confirm { request } } = event {
            let _ = self.0.send((request.id, request.kind, request.items));
        }
    }
}

/// What the user was asked: the kind and the items listed.
type Asked = Arc<Mutex<Vec<(ConfirmKind, Vec<String>)>>>;
type Incoming = mpsc::UnboundedReceiver<(String, ConfirmKind, Vec<String>)>;

/// The user: allows everything except the kinds in `deny`, and remembers what was asked.
fn user(api: Api, mut rx: Incoming, deny: Arc<Mutex<HashSet<String>>>) -> Asked {
    let asked: Asked = Default::default();
    let log = asked.clone();
    tokio::spawn(async move {
        while let Some((id, kind, items)) = rx.recv().await {
            let name = serde_json::to_value(kind).unwrap().as_str().unwrap().to_string();
            let allow = !deny.lock().unwrap().contains(&name);
            log.lock().unwrap().push((kind, items));
            api.call("agent.answer", json!({ "id": id, "allow": allow })).await.unwrap();
        }
    });
    asked
}

struct Agent {
    lines: Lines<BufReader<ReadHalf<DuplexStream>>>,
    write: WriteHalf<DuplexStream>,
    next: u64,
}

impl Agent {
    fn start(options: BridgeOptions) -> Self {
        let (ours, theirs) = tokio::io::duplex(8 * 1024 * 1024);
        let (their_read, their_write) = tokio::io::split(theirs);
        tokio::spawn(async move { serve(BufReader::new(their_read), their_write, options).await.unwrap() });
        let (read, write) = tokio::io::split(ours);
        Self { lines: BufReader::new(read).lines(), write, next: 1 }
    }

    async fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next;
        self.next += 1;
        let msg = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        self.write.write_all(format!("{msg}\n").as_bytes()).await.unwrap();
        loop {
            let line = tokio::time::timeout(Duration::from_secs(60), self.lines.next_line())
                .await
                .expect("answer in time")
                .unwrap()
                .expect("open");
            let v: Value = serde_json::from_str(&line).unwrap();
            if v["id"] == id {
                return v;
            }
        }
    }

    /// A tool call's result (`isError` and the text).
    async fn call(&mut self, name: &str, arguments: Value) -> (bool, Value, String) {
        let v = self.request("tools/call", json!({ "name": name, "arguments": arguments })).await;
        let result = &v["result"];
        let text = result["content"][0]["text"].as_str().unwrap_or_default().to_string();
        let data = serde_json::from_str(&text).unwrap_or(Value::Null);
        (result["isError"] == true, data, text)
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn agent_edits_sends_runs_and_is_asked() {
    let server = TestServer::start().await;
    let data = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    let (tx, rx) = mpsc::unbounded_channel();
    let api = Api::new(data.path().to_path_buf(), Arc::new(Questions(tx)));
    api.call("workspace.create", json!({ "path": ws.path(), "name": "Shop" })).await.unwrap();
    let deny: Arc<Mutex<HashSet<String>>> = Default::default();
    let asked = user(api.clone(), rx, deny.clone());
    let _listener = zorvik_mcp::listener::start(api.clone(), data.path()).await.unwrap();
    // A missing app: the bridge must find the running one through agent.json.
    let mut agent =
        Agent::start(BridgeOptions { data_dir: data.path().to_path_buf(), app: Some("/nonexistent".into()) });

    let init = agent
        .request("initialize", json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "claude-code", "version": "1" } }))
        .await;
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
    let tools = agent.request("tools/list", json!({})).await;
    assert!(tools["result"]["tools"].as_array().unwrap().iter().any(|t| t["name"] == "send_request"));

    // Agent access is off: the first call asks, and allowing turns it on.
    let (err, info, text) = agent.call("get_workspace", json!({})).await;
    assert!(!err, "{text}");
    assert_eq!(info["name"], "Shop");
    assert_eq!(asked.lock().unwrap()[0].0, ConfirmKind::Enable);
    assert_eq!(api.call("settings.get", Value::Null).await.unwrap()["agents"]["enabled"], true);
    let state = api.call("agent.state", Value::Null).await.unwrap();
    assert_eq!(state["sessions"][0]["client"], "Claude Code");

    let (err, _, text) = agent
        .call(
            "save_environment",
            json!({ "name": "Local", "activate": true, "variables": [{ "key": "base", "value": server.url("") }, { "key": "token", "value": "s3cret-token-value", "secret": true }] }),
        )
        .await;
    assert!(!err, "{text}");
    let (err, saved, text) = agent
        .call(
            "save_requests",
            json!({ "requests": [
                { "folder": "Users", "name": "Echo", "url": "{{base}}/echo?q=1", "headers": { "Authorization": "Bearer {{token}}" },
                  "scripts": { "postResponse": "pm.test('ok', () => pm.response.to.have.status(200));" } },
                { "folder": "Users", "name": "Status", "method": "GET", "url": "{{base}}/status/204" },
            ] }),
        )
        .await;
    assert!(!err, "{text}");
    assert_eq!(saved["saved"][0]["path"], "Users/Echo.yaml");
    // Saving again updates instead of adding a copy.
    let (_, again, _) = agent
        .call(
            "save_requests",
            json!({ "requests": [{ "folder": "Users", "name": "Echo", "url": "{{base}}/echo?q=2" }] }),
        )
        .await;
    assert_eq!(again["saved"][0], json!({ "path": "Users/Echo.yaml", "created": false }));
    // By path: replaced as given.
    let echo = json!({ "path": "Users/Echo.yaml", "url": "{{base}}/echo?q=1", "headers": { "Authorization": "Bearer {{token}}" },
        "scripts": { "postResponse": "pm.test('ok', () => pm.response.to.have.status(200));" } });
    agent.call("save_requests", json!({ "requests": [echo] })).await;
    let (_, list, _) = agent.call("list_requests", json!({})).await;
    let urls: Vec<&str> = list["items"].as_array().unwrap().iter().filter_map(|i| i["url"].as_str()).collect();
    assert_eq!(urls, ["{{base}}/echo?q=1", "{{base}}/status/204"]);

    // A local host needs no approval; the secret never comes back.
    let (err, res, text) = agent.call("send_request", json!({ "path": "Users/Echo.yaml" })).await;
    assert!(!err, "{text}");
    assert_eq!(res["status"], 200);
    assert!(!text.contains("s3cret-token-value"), "{text}");
    assert!(text.contains("••••••"), "{text}");

    // A redirect to a host nobody approved is stopped before it connects…
    let redirect = json!({ "request": { "url": "{{base}}/redirect-to?url=http://blocked.example/x" } });
    let (err, _, text) = agent.call("send_request", redirect.clone()).await;
    assert!(err && text.contains("blocked.example"), "{text}");
    // …and the next try asks the user about it (who says no here).
    deny.lock().unwrap().insert("traffic".into());
    let (err, _, text) = agent.call("send_request", redirect).await;
    assert!(err && text.contains("declined"), "{text}");
    let traffic = asked.lock().unwrap().iter().find(|(k, _)| *k == ConfirmKind::Traffic).cloned().unwrap();
    assert_eq!(traffic.1, vec!["blocked.example".to_string()]);
    deny.lock().unwrap().clear();

    let (err, run, text) = agent.call("run_collection", json!({ "folder": "Users", "waitSeconds": 30 })).await;
    assert!(!err, "{text}");
    assert_eq!(run["status"], "finished");
    assert_eq!(run["summary"]["requests"], 2);
    assert_eq!(run["summary"]["testsPassed"], 1);

    // Deletes always ask; declined, nothing is deleted.
    deny.lock().unwrap().insert("delete".into());
    let (err, _, text) = agent.call("delete_items", json!({ "paths": ["Users"] })).await;
    assert!(err && text.contains("declined"), "{text}");
    assert!(ws.path().join("requests/Users/Echo.yaml").exists());

    let (err, _, text) = agent.call("read_request", json!({ "path": "Nope.yaml" })).await;
    assert!(err && text.contains("not found"), "{text}");
    let (err, _, _) = agent.call("no_such_tool", json!({})).await;
    assert!(err);

    // A partial update keeps the rest of the request.
    let (err, _, text) = agent
        .call("save_requests", json!({ "requests": [{ "path": "Users/Echo.yaml", "docs": "echo handler" }] }))
        .await;
    assert!(!err, "{text}");
    let (_, read, _) = agent.call("read_request", json!({ "path": "Users/Echo.yaml" })).await;
    assert_eq!(read["request"]["url"], "{{base}}/echo?q=1");
    assert_eq!(read["request"]["docs"], "echo handler");

    let state = api.call("agent.state", Value::Null).await.unwrap();
    let statuses: Vec<&str> =
        state["activity"].as_array().unwrap().iter().map(|a| a["status"].as_str().unwrap()).collect();
    assert!(statuses.contains(&"done") && statuses.contains(&"denied") && statuses.contains(&"failed"), "{statuses:?}");

    // Disconnected in the app: the agent is told, and gets no more calls (no hang, no quiet reconnect).
    api.call("agent.disconnect", json!({})).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    for _ in 0..2 {
        let (err, _, text) = agent.call("get_workspace", json!({})).await;
        assert!(err && text.contains("disconnected"), "{text}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn headless_runs_without_the_app_but_asks_nothing() {
    let data = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    {
        let api = Api::new(data.path().to_path_buf(), Arc::new(Questions(mpsc::unbounded_channel().0)));
        api.call("workspace.create", json!({ "path": ws.path(), "name": "Offline" })).await.unwrap();
        let mut settings = api.call("settings.get", Value::Null).await.unwrap();
        settings["agents"]["enabled"] = json!(true);
        settings["agents"]["headless"] = json!(true);
        api.call("settings.save", json!({ "settings": settings })).await.unwrap();
    }
    let mut agent =
        Agent::start(BridgeOptions { data_dir: data.path().to_path_buf(), app: Some("/nonexistent".into()) });
    agent.request("initialize", json!({ "protocolVersion": "2025-03-26", "clientInfo": { "name": "codex" } })).await;
    let (err, info, text) = agent.call("get_workspace", json!({})).await;
    assert!(!err, "{text}");
    assert_eq!(info["name"], "Offline");
    let (err, _, text) =
        agent.call("save_requests", json!({ "requests": [{ "name": "A", "url": "http://localhost:1/a" }] })).await;
    assert!(!err, "{text}");
    let (err, _, text) = agent.call("delete_items", json!({ "paths": ["A.yaml"] })).await;
    assert!(err && text.contains("not open"), "{text}");
}

#[tokio::test(flavor = "multi_thread")]
async fn no_app_and_no_headless_means_starting_the_app() {
    let data = tempfile::tempdir().unwrap();
    let mut agent =
        Agent::start(BridgeOptions { data_dir: data.path().to_path_buf(), app: Some("/nonexistent/zorvik".into()) });
    agent
        .request("initialize", json!({ "protocolVersion": "2025-06-18", "clientInfo": { "name": "gemini-cli" } }))
        .await;
    // Lists work without the app.
    let prompts = agent.request("prompts/list", json!({})).await;
    assert!(prompts["result"]["prompts"].as_array().unwrap().iter().any(|p| p["name"] == "map_apis"));
    let (err, _, text) = agent.call("get_workspace", json!({})).await;
    assert!(err && text.contains("could not be started"), "{text}");
}

/// A connected agent on a fresh workspace, with the user allowing everything.
async fn connected(
    name: &str,
) -> (Api, Agent, Asked, tempfile::TempDir, tempfile::TempDir, zorvik_mcp::listener::AgentListener) {
    let data = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    let (tx, rx) = mpsc::unbounded_channel();
    let api = Api::new(data.path().to_path_buf(), Arc::new(Questions(tx)));
    api.call("workspace.create", json!({ "path": ws.path(), "name": name })).await.unwrap();
    let asked = user(api.clone(), rx, Default::default());
    let listener = zorvik_mcp::listener::start(api.clone(), data.path()).await.unwrap();
    let mut agent =
        Agent::start(BridgeOptions { data_dir: data.path().to_path_buf(), app: Some("/nonexistent".into()) });
    agent
        .request("initialize", json!({ "protocolVersion": "2025-06-18", "clientInfo": { "name": "claude-code" } }))
        .await;
    let (err, _, text) = agent.call("get_workspace", json!({})).await;
    assert!(!err, "{text}");
    (api, agent, asked, data, ws, listener)
}

#[tokio::test(flavor = "multi_thread")]
async fn agent_builds_runs_and_inspects_a_mock() {
    let (_api, mut agent, asked, _data, ws, _listener) = connected("Mocks").await;

    // A misspelled field is refused, with the field it probably meant.
    let (err, _, text) = agent
        .call(
            "save_server",
            json!({ "name": "Orders", "server": { "http": { "routes": [{ "path": "/o", "stauts": 201 }] } } }),
        )
        .await;
    assert!(err && text.contains("did you mean `status`"), "{text}");
    let (err, _, text) = agent
        .call("save_server", json!({ "name": "Orders", "server": { "http": { "routes": [{ "path": "o" }] } } }))
        .await;
    assert!(err && text.contains("must start with /"), "{text}");

    let (err, saved, text) = agent
        .call(
            "save_server",
            json!({ "name": "Orders", "server": { "port": 0, "http": { "routes": [{
                "method": "GET", "path": "/orders/:id", "status": 200,
                "headers": [{ "key": "Content-Type", "value": "application/json" }],
                "body": "{\"id\": \"{{request.params.id}}\", \"q\": \"{{request.query.expand}}\"}"
            }] } } }),
        )
        .await;
    assert!(!err, "{text}");
    assert_eq!(saved["created"], true);
    let (_, read, _) = agent.call("read_server", json!({ "name": "orders" })).await;
    assert_eq!(read["server"]["http"]["routes"][0]["path"], "/orders/:id");

    // Any free port for this run; the saved port stays 0.
    let (err, started, text) = agent.call("start_server", json!({ "name": "Orders", "port": 0 })).await;
    assert!(!err, "{text}");
    let url = started["url"].as_str().unwrap().to_string();
    assert!(asked.lock().unwrap().iter().any(|(k, _)| *k == ConfirmKind::Server));

    let (err, response, text) =
        agent.call("send_request", json!({ "request": { "url": format!("{url}/orders/42?expand=items") } })).await;
    assert!(!err, "{text}");
    assert_eq!(response["status"], 200);
    assert!(response["body"].as_str().unwrap().contains("\"42\""), "{response}");

    // What the mock received, and the route that answered.
    let (err, traffic, text) = agent.call("get_server_traffic", json!({ "name": "Orders" })).await;
    assert!(!err, "{text}");
    let http = traffic["entries"].as_array().unwrap().iter().find(|e| e["kind"] == "http").expect("an http entry");
    assert_eq!(http["http"]["path"], "/orders/42?expand=items");
    assert_eq!(http["http"]["status"], 200);
    assert!(http["http"]["route"].as_str().unwrap().contains("/orders/:id"), "{http}");
    let last = traffic["lastId"].as_u64().unwrap();
    let (_, newer, _) = agent.call("get_server_traffic", json!({ "name": "Orders", "sinceId": last })).await;
    assert_eq!(newer["entries"].as_array().unwrap().len(), 0);

    // A change reaches the running server at once.
    let (err, updated, text) = agent
        .call("save_server", json!({ "name": "Orders", "server": { "http": { "routes": [{ "method": "GET", "path": "/health", "body": "ok" }] } } }))
        .await;
    assert!(!err, "{text}");
    assert_eq!(updated["running"]["applied"], true);
    let (_, health, _) = agent.call("send_request", json!({ "request": { "url": format!("{url}/health") } })).await;
    assert_eq!(health["body"], "ok");

    // A second server on the same port says it is taken.
    let port = url.rsplit(':').next().unwrap().to_string();
    let (err, _, text) = agent
        .call(
            "save_server",
            json!({ "name": "Clash", "server": { "port": port.parse::<u16>().unwrap(), "http": { "routes": [] } } }),
        )
        .await;
    assert!(!err, "{text}");
    let (err, _, text) = agent.call("start_server", json!({ "name": "Clash" })).await;
    assert!(err && text.contains("already in use") && text.contains("port: 0"), "{text}");

    // A mock from the collection.
    let (err, _, text) = agent
        .call("save_requests", json!({ "requests": [{ "name": "Get user", "folder": "Users", "url": "{{base}}/users/:id", "pathParams": [{ "key": "id", "value": "7" }] }] }))
        .await;
    assert!(!err, "{text}");
    let (err, mock, text) =
        agent.call("create_mock", json!({ "name": "Users mock", "folder": "Users", "port": 4999 })).await;
    assert!(!err, "{text}");
    assert_eq!(mock["port"], 4999);
    assert!(mock["routes"].as_array().unwrap().iter().any(|r| r.as_str().unwrap().contains("/users/:id")), "{mock}");
    let (_, list, _) = agent.call("list_servers", json!({})).await;
    assert_eq!(list["servers"].as_array().unwrap().len(), 3);

    let (err, _, _) = agent.call("stop_server", json!({ "name": "Orders" })).await;
    assert!(!err);
    let (err, _, text) = agent.call("get_server_traffic", json!({ "name": "Orders" })).await;
    assert!(err && text.contains("not running"), "{text}");
    drop(ws);
}

#[tokio::test(flavor = "multi_thread")]
async fn agent_reads_state_writes_files_and_exports() {
    let server = TestServer::start().await;
    let (_api, mut agent, _asked, _data, ws, _listener) = connected("State").await;

    // Query rows become the URL (enabled) and switched-off params, descriptions kept.
    let (err, _, text) = agent
        .call(
            "save_requests",
            json!({ "requests": [{
                "name": "Search", "url": "{{base}}/json",
                "query": [
                    { "key": "q", "value": "a b", "description": "Search text" },
                    { "key": "page", "value": "{{page}}" },
                    { "key": "debug", "value": "1", "enabled": false, "description": "Verbose output" }
                ]
            }] }),
        )
        .await;
    assert!(!err, "{text}");
    let (_, read, _) = agent.call("read_request", json!({ "paths": ["Search.yaml", "Missing.yaml"] })).await;
    let search = &read["requests"][0]["request"];
    assert_eq!(search["url"], "{{base}}/json?q=a%20b&page={{page}}");
    assert_eq!(search["disabledParams"][0]["key"], "debug");
    assert_eq!(search["paramDescriptions"][0]["description"], "Search text");
    assert!(read["requests"][1]["error"].is_string());
    let (err, _, text) =
        agent.call("save_requests", json!({ "requests": [{ "name": "Bad", "url": "x", "descripton": "?" }] })).await;
    assert!(err && text.contains("descripton"), "{text}");

    // Variables, with the values scripts save.
    let (err, env, text) = agent
        .call("save_environment", json!({ "name": "Local", "activate": true, "variables": [{ "key": "base", "value": server.url("") }, { "key": "page", "value": "2" }] }))
        .await;
    assert!(!err, "{text}");
    assert_eq!(env["active"], true);
    let (err, _, text) = agent
        .call(
            "save_requests",
            json!({ "requests": [{ "name": "Login", "url": "{{base}}/json", "scripts": { "postResponse": "pm.environment.set('orderId', 'ord_77')" } }] }),
        )
        .await;
    assert!(!err, "{text}");
    let (err, sent, text) = agent.call("send_request", json!({ "path": "Login.yaml" })).await;
    assert!(!err, "{text}");
    assert_eq!(sent["status"], 200);
    let (_, vars, _) = agent.call("get_variables", json!({})).await;
    let order = vars["variables"].as_array().unwrap().iter().find(|v| v["key"] == "orderId").expect("orderId");
    assert_eq!((order["value"].as_str(), order["setByScript"].as_bool()), (Some("ord_77"), Some(true)));
    let (_, envs, _) = agent.call("list_environments", json!({})).await;
    assert!(
        envs["environments"][0]["variables"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["key"] == "orderId" && v["setByScript"] == true)
    );

    // History: the send, with its response.
    let (err, history, text) = agent.call("read_history", json!({ "path": "Login.yaml" })).await;
    assert!(!err, "{text}");
    let entry = &history["entries"][0];
    assert_eq!(entry["status"], 200);
    assert_eq!(entry["response"]["status"], 200);
    assert!(entry["response"]["body"].as_str().is_some_and(|b| !b.is_empty()), "{entry}");

    // Server-Sent Events: read until a number of events.
    let (err, events, text) = agent
        .call("send_request", json!({ "request": { "kind": "sse", "url": server.url("/sse?count=5&interval=20") }, "stream": { "maxEvents": 2 } }))
        .await;
    assert!(!err, "{text}");
    assert_eq!((events["ended"].as_str(), events["events"].as_array().map(Vec::len)), (Some("count"), Some(2)));

    // A file for an upload, but not into Zorvik's own folders.
    let (err, file, text) =
        agent.call("write_file", json!({ "path": "fixtures/pixel.png", "base64": "iVBORw0KGgo=" })).await;
    assert!(!err, "{text}");
    assert_eq!(file["size"], 8);
    assert!(ws.path().join("fixtures").join("pixel.png").is_file());
    let (err, _, text) = agent.call("write_file", json!({ "path": "fixtures/pixel.png", "text": "x" })).await;
    assert!(err && text.contains("overwrite"), "{text}");
    let (err, _, text) = agent.call("write_file", json!({ "path": "requests/evil.yaml", "text": "x" })).await;
    assert!(err && text.contains("belongs to Zorvik"), "{text}");
    let (err, _, text) = agent.call("write_file", json!({ "path": "../outside.txt", "text": "x" })).await;
    assert!(err, "{text}");

    // Export as code for another team.
    let (err, code, text) = agent.call("export_request", json!({ "path": "Search.yaml", "format": "kotlin" })).await;
    assert!(!err, "{text}");
    let code = code["code"].as_str().unwrap();
    assert!(code.contains("OkHttpClient") && code.contains("/json?q=a%20b&page=2"), "{code}");
    let (err, code, _) =
        agent.call("export_request", json!({ "path": "Search.yaml", "resolveVariables": false })).await;
    // Not filled in: the variable stays (cURL escapes its braces so they aren't a URL pattern).
    let curl = code["code"].as_str().unwrap();
    assert!(!err && curl.contains(r"\{\{base\}\}") && !curl.contains("127.0.0.1"), "{code}");

    // Load test thresholds: units are checked, misspellings refused.
    let (err, _, text) = agent
        .call("save_load_test", json!({ "name": "Smoke", "test": { "targets": [{ "request": "Login.yaml" }], "stages": [{ "durationSecs": 1, "target": 1 }], "thresholds": [{ "metric": "errorRate", "op": "<", "value": 150 }] } }))
        .await;
    assert!(err && text.contains("percent"), "{text}");
    let (err, _, text) = agent
        .call("save_load_test", json!({ "name": "Smoke", "test": { "targets": [{ "request": "Login.yaml" }], "stages": [{ "durationSecs": 1, "target": 1 }], "thersholds": [] } }))
        .await;
    assert!(err && text.contains("thresholds"), "{text}");
}

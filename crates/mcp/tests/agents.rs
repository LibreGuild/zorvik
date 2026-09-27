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

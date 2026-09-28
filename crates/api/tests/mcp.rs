//! MCP through the API: an MCP server started from the workspace, and `mcp` requests talking
//! to it (a session with its catalog; tool calls with variables, resources and prompts sent
//! like any request, on the tab's session or in one go; the message log), the same calls in a
//! collection run with tests, and the trust a program needs before a workspace may start it.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};
use zorvik_api::{Api, EventSink, StreamEvent};

#[derive(Default)]
struct Events(Mutex<Vec<Value>>);

impl EventSink for Events {
    fn emit(&self, event: StreamEvent) {
        if matches!(event, StreamEvent::Mcp { .. } | StreamEvent::Runner { .. }) {
            self.0.lock().unwrap().push(serde_json::to_value(&event).unwrap());
        }
    }
}

struct Harness {
    api: Api,
    events: Arc<Events>,
    _data: tempfile::TempDir,
    ws_dir: tempfile::TempDir,
}

impl Harness {
    async fn new() -> Self {
        let data = tempfile::tempdir().unwrap();
        let ws_dir = tempfile::tempdir().unwrap();
        let events = Arc::new(Events::default());
        let api = Api::new(data.path().to_path_buf(), events.clone());
        let h = Self { api, events, _data: data, ws_dir };
        h.ok("workspace.create", json!({ "path": h.ws_dir.path(), "name": "Agents" })).await;
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

    /// Send like the UI does (`requestId` is the tab, whose session the call reuses): the
    /// status and the body as JSON.
    async fn send(&self, tab: &str, request: &Value) -> (u64, Value) {
        let r = self.ok("http.send", json!({ "requestId": tab, "request": request })).await;
        let body = serde_json::from_str(r["body"]["text"].as_str().unwrap()).unwrap();
        (r["meta"]["status"].as_u64().unwrap(), body)
    }

    fn events_where(&self, f: impl Fn(&Value) -> bool) -> Vec<Value> {
        self.events.0.lock().unwrap().iter().filter(|e| f(e)).cloned().collect()
    }

    async fn wait_for(&self, what: &str, f: impl Fn(&Value) -> bool) -> Value {
        for _ in 0..400 {
            if let Some(e) = self.events_where(&f).into_iter().next() {
                return e;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("{what} did not happen: {:?}", self.events.0.lock().unwrap());
    }

    /// An MCP mock server, started: its URL.
    async fn start_server(&self) -> String {
        let server = json!({ "name": "Weather MCP", "kind": "mcp", "port": 0, "mcp": {
            "instructions": "Ask for the weather.",
            "tools": [{
                "name": "get_weather", "description": "The weather in a city",
                "inputSchema": r#"{"type":"object","properties":{"city":{"type":"string"}},"required":["city"]}"#,
                "result": "Sunny in {{args.city}}",
            }],
            "resources": [
                { "uri": "docs://readme", "name": "readme", "mimeType": "text/markdown", "text": "# Readme" },
                { "uri": "users://{id}", "name": "user", "text": "User {{params.id}}" },
            ],
            "prompts": [{
                "name": "review", "arguments": [{ "name": "code", "required": true }],
                "messages": [{ "role": "user", "text": "Review this: {{args.code}}" }],
            }],
        } });
        let id = self.ok("server.create", json!({ "server": server })).await;
        let server = self.ok("server.read", json!({ "id": id })).await;
        let info = self.ok("server.start", json!({ "id": id, "server": server })).await;
        info["url"].as_str().unwrap().to_string()
    }
}

fn mcp_request(url: &str, call: &str, name: &str, arguments: &str) -> Value {
    json!({ "name": "call", "kind": "mcp", "url": url,
            "mcp": { "call": call, "name": name, "arguments": arguments } })
}

#[tokio::test]
async fn a_session_lists_and_calls_what_the_server_offers() {
    let h = Harness::new().await;
    let env = h
        .ok(
            "env.create",
            json!({ "environment": { "name": "Dev", "variables": [{ "key": "city", "value": "Paris" }] } }),
        )
        .await;
    h.ok("env.setActive", json!({ "id": env })).await;
    let url = h.start_server().await;
    assert!(url.starts_with("http://127.0.0.1:") && url.ends_with("/mcp"), "{url}");

    let request = mcp_request(&url, "tool", "get_weather", r#"{"city": "{{city}}"}"#);
    let opened = h.ok("mcp.connect", json!({ "connId": "c1", "request": request })).await;
    assert_eq!(opened["info"]["name"], "Weather MCP");
    assert_eq!(opened["info"]["transport"], "Streamable HTTP");
    assert_eq!(opened["info"]["instructions"], "Ask for the weather.");
    assert!(opened["info"]["sessionId"].as_str().is_some_and(|s| !s.is_empty()));

    let catalog = h.ok("mcp.catalog", json!({ "connId": "c1" })).await;
    assert_eq!(catalog["tools"][0]["name"], "get_weather");
    assert_eq!(catalog["tools"][0]["inputSchema"]["required"][0], "city");
    assert_eq!(catalog["resources"].as_array().unwrap().len(), 1);
    assert_eq!(catalog["resourceTemplates"][0]["uriTemplate"], "users://{id}");
    assert_eq!(catalog["prompts"][0]["name"], "review");
    assert_eq!(catalog["problems"], json!([]));

    // A tool, with a variable in its arguments, on the session.
    let (status, result) = h.send("c1", &request).await;
    assert_eq!(status, 200);
    assert_eq!(result["content"][0]["text"], "Sunny in Paris");
    assert_ne!(result["isError"], true);
    let on_session = h.events_where(|e| e["connId"] == "c1" && e["event"]["method"] == "tools/call");
    assert_eq!(on_session.len(), 1, "the call went over the session");

    // A missing required argument: the tool reports the failure.
    let missing = mcp_request(&url, "tool", "get_weather", "{}");
    let (status, result) = h.send("c1", &missing).await;
    assert_eq!((status, &result["isError"]), (200, &json!(true)));

    // An unknown tool: a JSON-RPC error, as status 500.
    let unknown = mcp_request(&url, "tool", "nope", "{}");
    let (status, answer) = h.send("c1", &unknown).await;
    assert_eq!(status, 500);
    assert!(answer["error"]["code"].is_i64(), "{answer}");

    // A resource template, filled in from the arguments.
    let read = mcp_request(&url, "resource", "users://{id}", r#"{"id": "42"}"#);
    let (_, result) = h.send("c1", &read).await;
    assert_eq!(result["contents"][0]["uri"], "users://42");
    assert_eq!(result["contents"][0]["text"], "User 42");

    // A prompt: its arguments go as text.
    let prompt = mcp_request(&url, "prompt", "review", r#"{"code": 1}"#);
    let (_, result) = h.send("c1", &prompt).await;
    assert_eq!(result["messages"][0]["content"]["text"], "Review this: 1");

    // Without a session (another tab), the call connects, calls and disconnects.
    let (status, result) = h.send("other-tab", &request).await;
    assert_eq!((status, &result["content"][0]["text"]), (200, &json!("Sunny in Paris")));
    assert!(h.events_where(|e| e["connId"] == "other-tab").is_empty());

    // Arguments must be a JSON object; a call needs a name.
    let bad = mcp_request(&url, "tool", "get_weather", "[1]");
    assert_eq!(h.err("http.send", json!({ "requestId": "c1", "request": bad })).await.code, "invalidInput");
    let unnamed = mcp_request(&url, "tool", "", "{}");
    let e = h.err("http.send", json!({ "requestId": "c1", "request": unnamed })).await;
    assert!(e.message.contains("Choose the tool"), "{}", e.message);

    // Every message is in the log, both ways.
    let sent = h.events_where(|e| {
        e["type"] == "mcp" && e["event"]["type"] == "message" && e["event"]["method"] == "tools/call"
    });
    assert!(!sent.is_empty());
    let received = h.events_where(|e| e["event"]["type"] == "message" && e["event"]["direction"] == "received");
    assert!(received.len() >= 6, "{received:?}");

    h.ok("mcp.close", json!({ "connId": "c1" })).await;
    h.wait_for("the session to close", |e| e["connId"] == "c1" && e["event"]["type"] == "closed").await;
    assert_eq!(h.err("mcp.catalog", json!({ "connId": "c1" })).await.code, "notFound");
}

#[tokio::test]
async fn collection_runs_make_the_call_and_test_it() {
    let h = Harness::new().await;
    let url = h.start_server().await;
    let mut request = mcp_request(&url, "tool", "get_weather", r#"{"city": "Oslo"}"#);
    request["name"] = json!("Weather");
    request["scripts"] = json!({ "postResponse": r#"
        const answer = pm.response.json();
        pm.test("says the city", () => pm.expect(answer.content[0].text).to.eql("Sunny in Oslo"));
        pm.test("status", () => pm.response.to.have.status(200));
    "# });
    h.ok("request.create", json!({ "parent": "", "request": request })).await;
    let mut failing = mcp_request(&url, "tool", "nope", "{}");
    failing["name"] = json!("Unknown tool");
    failing["scripts"] = json!({ "postResponse": "pm.test('an error', () => pm.response.to.have.status(500));" });
    h.ok("request.create", json!({ "parent": "", "request": failing })).await;

    let started = h.ok("runner.start", json!({ "folder": "" })).await;
    let run_id = started["runId"].as_str().unwrap().to_string();
    let finished =
        h.wait_for("the run to finish", |e| e["runId"] == run_id.as_str() && e["event"]["type"] == "finished").await;
    let summary = &finished["event"]["summary"];
    assert_eq!(summary["testsPassed"], 3, "{summary}");
    assert_eq!(summary["testsFailed"], 0, "{summary}");
    assert_eq!(summary["skipped"], 0, "{summary}");
}

#[tokio::test]
async fn a_program_runs_only_once_trusted() {
    let h = Harness::new().await;
    let request = json!({ "name": "local", "kind": "mcp", "url": "zorvik-test-no-such-program --stdio",
                          "mcp": { "call": "tool", "name": "x", "env": [{ "key": "MODE", "value": "a" }] } });
    let e = h.err("mcp.connect", json!({ "connId": "p", "request": request })).await;
    assert_eq!(e.code, "untrustedProgram");
    assert!(e.message.contains("zorvik-test-no-such-program"), "{}", e.message);

    // Runs are refused the same way (the app hasn't been told to trust it).
    let mut saved = request.clone();
    saved["name"] = json!("Local program");
    h.ok("request.create", json!({ "parent": "", "request": saved })).await;
    let started = h.ok("runner.start", json!({ "folder": "" })).await;
    let run_id = started["runId"].as_str().unwrap().to_string();
    let result = h.wait_for("the result", |e| e["runId"] == run_id.as_str() && e["event"]["type"] == "result").await;
    assert!(result["event"]["result"]["error"].as_str().unwrap().contains("starts a program"), "{result}");

    // What the user is asked to trust.
    let program = h.ok("mcp.program", json!({ "request": request })).await;
    assert_eq!(program["command"], "zorvik-test-no-such-program --stdio");
    assert_eq!(program["env"][0], json!({ "key": "MODE", "value": "a" }));
    assert_eq!(program["trusted"], false);

    // Once trusted, it is started (here it doesn't exist, so starting fails).
    h.ok("mcp.trust", json!({ "request": request })).await;
    assert_eq!(h.ok("mcp.program", json!({ "request": request })).await["trusted"], true);
    let e = h.err("mcp.connect", json!({ "connId": "p", "request": request })).await;
    assert_ne!(e.code, "untrustedProgram", "{}", e.message);

    // Another environment is another program.
    let mut changed = request.clone();
    changed["mcp"]["env"][0]["value"] = json!("b");
    assert_eq!(h.err("mcp.connect", json!({ "connId": "p", "request": changed })).await.code, "untrustedProgram");
}

#[tokio::test]
async fn saving_a_running_server_applies_it() {
    let h = Harness::new().await;
    let url = h.start_server().await;
    let id = "Weather MCP";
    let mut server = h.ok("server.read", json!({ "id": id })).await;
    server["mcp"]["tools"][0]["result"] = json!("Rainy in {{args.city}}");
    h.ok("server.save", json!({ "id": id, "server": server })).await;
    let (_, result) = h.send("t", &mcp_request(&url, "tool", "get_weather", r#"{"city": "Bergen"}"#)).await;
    assert_eq!(result["content"][0]["text"], "Rainy in Bergen");
}

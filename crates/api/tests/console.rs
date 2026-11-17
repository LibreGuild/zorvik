//! The console: what `http.send`, connections, collection runs and scripts leave in it.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};
use zorvik_api::{Api, EventSink, StreamEvent};
use zorvik_testkit::TestServer;

#[derive(Default)]
struct Events(Mutex<Vec<Value>>);

impl EventSink for Events {
    fn emit(&self, event: StreamEvent) {
        self.0.lock().unwrap().push(serde_json::to_value(&event).unwrap());
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
        h.ok("workspace.create", json!({ "path": h.ws_dir.path(), "name": "Test" })).await;
        h
    }

    async fn ok(&self, method: &str, params: Value) -> Value {
        match self.api.call(method, params).await {
            Ok(v) => v,
            Err(e) => panic!("{method} failed: {} ({})", e.message, e.code),
        }
    }

    async fn lines(&self) -> Vec<Value> {
        self.ok("console.list", json!({})).await.as_array().unwrap().clone()
    }

    async fn details(&self, line: &Value) -> Value {
        self.ok("console.details", json!({ "id": line["id"] })).await
    }

    async fn env(&self, variables: Value) {
        let id = self.ok("env.create", json!({ "environment": { "name": "Dev", "variables": variables } })).await;
        self.ok("env.setActive", json!({ "id": id })).await;
    }

    /// Wait until the console has a line that `found` accepts.
    async fn wait_for(&self, found: impl Fn(&Value) -> bool) -> Value {
        for _ in 0..200 {
            if let Some(line) = self.lines().await.into_iter().find(|l| found(l)) {
                return line;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("no such console line: {:#?}", self.lines().await);
    }
}

fn header<'a>(headers: &'a Value, name: &str) -> Option<&'a str> {
    headers.as_array().unwrap().iter().find(|h| h["name"].as_str().unwrap().eq_ignore_ascii_case(name))?["value"]
        .as_str()
}

/// What a line is, in a word or two, for comparing the order of lines.
fn brief(line: &Value) -> String {
    let item = &line["item"];
    match item["type"].as_str().unwrap() {
        "log" => format!("{}: {}", item["level"].as_str().unwrap(), item["message"].as_str().unwrap()),
        _ => {
            let origin = item["origin"].as_str().map(|o| format!(" [{o}]")).unwrap_or_default();
            let path = item["url"].as_str().unwrap().split('/').skip(3).collect::<Vec<_>>().join("/");
            format!("{} /{path} {}{origin}", item["method"].as_str().unwrap(), item["status"])
        }
    }
}

#[tokio::test]
async fn a_send_shows_what_went_out_and_what_came_back() {
    let server = TestServer::start().await;
    let h = Harness::new().await;
    h.env(
        json!([{ "key": "base", "value": server.url("") }, { "key": "key", "value": "s3cr3t-value", "secret": true }]),
    )
    .await;
    let request = json!({
        "name": "Create user", "seq": 0, "method": "POST", "url": "{{base}}/echo?a=1",
        "headers": [{ "key": "X-Api-Key", "value": "{{key}}" }],
        "body": { "type": "json", "text": "{\"name\": \"Ada\", \"key\": \"{{key}}\"}" },
    });
    let sent = h.ok("http.send", json!({ "requestId": "r1", "request": request })).await;
    assert_eq!(sent["meta"]["status"], 200);

    let lines = h.lines().await;
    assert_eq!(lines.len(), 1, "{lines:#?}");
    let line = &lines[0];
    assert_eq!((line["source"].as_str(), line["request"].as_str()), (Some("user"), Some("Create user")));
    let item = &line["item"];
    assert_eq!(item["type"], "exchange");
    assert_eq!((item["protocol"].as_str(), item["method"].as_str()), (Some("HTTP"), Some("POST")));
    assert_eq!(item["url"], server.url("/echo?a=1"));
    assert_eq!((item["status"].as_u64(), item["error"].is_null()), (Some(200), true));
    assert!(item["durationMs"].as_f64().is_some() && item["size"].as_u64().is_some_and(|s| s > 0), "{item}");

    // Everything as it was sent, secret values included: the console is for debugging, and it
    // is never written to a file.
    let details = h.details(line).await;
    assert_eq!(details["prepared"], false);
    assert_eq!(header(&details["requestHeaders"], "x-api-key"), Some("s3cr3t-value"));
    assert!(header(&details["requestHeaders"], "user-agent").is_some(), "the headers added while sending");
    assert_eq!(details["requestBody"]["text"], "{\"name\": \"Ada\", \"key\": \"s3cr3t-value\"}");
    assert_eq!(details["response"]["status"], 200);
    assert!(header(&details["response"]["headers"], "content-type").is_some());
    let body = details["response"]["body"]["text"].as_str().unwrap();
    assert!(body.contains("Ada") && body.contains("bodyBase64"), "{body}");
    assert!(details["remoteAddr"].as_str().is_some() && details["timing"]["totalMs"].as_f64().is_some());

    // The UI was told as it happened.
    let events = h.events.0.lock().unwrap().clone();
    let told: Vec<&Value> = events.iter().filter(|e| e["type"] == "console").collect();
    assert_eq!(told.len(), 1);
    assert_eq!(told[0]["event"]["line"]["id"], line["id"]);

    // What came back is in no file: not in the app's data folder, not in the workspace.
    for dir in [h._data.path(), h.ws_dir.path()] {
        for entry in walk(dir) {
            let text = String::from_utf8_lossy(&std::fs::read(&entry).unwrap()).into_owned();
            assert!(!text.contains("bodyBase64"), "{} has the response body", entry.display());
        }
    }
}

fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(walk(&path));
        } else {
            out.push(path);
        }
    }
    out
}

#[tokio::test]
async fn a_send_that_fails_keeps_the_request_it_prepared() {
    let h = Harness::new().await;
    // A port nothing listens on.
    let free = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    h.env(json!([{ "key": "token", "value": "tok-123456" }])).await;
    let request = json!({
        "name": "Down", "seq": 0, "method": "PUT", "url": format!("http://127.0.0.1:{free}/orders/7"),
        "auth": { "type": "bearer", "token": "{{token}}" },
        "body": { "type": "json", "text": "{\"paid\": true}" },
    });
    let err = h.api.call("http.send", json!({ "requestId": "r1", "request": request })).await.expect_err("refused");
    assert_eq!(err.code, "network");

    let lines = h.lines().await;
    assert_eq!(lines.len(), 1, "{lines:#?}");
    let item = &lines[0]["item"];
    assert_eq!((item["method"].as_str(), item["status"].is_null()), (Some("PUT"), true));
    assert_eq!(item["error"], err.message);
    let details = h.details(&lines[0]).await;
    assert_eq!(details["prepared"], true);
    assert_eq!(header(&details["requestHeaders"], "authorization"), Some("Bearer tok-123456"));
    assert_eq!(details["requestBody"]["text"], "{\"paid\": true}");
    assert!(details["response"].is_null());

    // A URL that can't be resolved never gets that far: the request as it is written.
    let request = json!({ "name": "Undefined", "seq": 0, "method": "GET", "url": "{{nowhere}}/x",
        "headers": [{ "key": "X-Token", "value": "{{token}}" }] });
    let err = h.api.call("http.send", json!({ "requestId": "r2", "request": request })).await.expect_err("undefined");
    let lines = h.lines().await;
    assert_eq!(lines.len(), 2, "{lines:#?}");
    assert_eq!(lines[1]["item"]["error"], err.message);
    assert_eq!(lines[1]["item"]["url"], "{{nowhere}}/x");
    let details = h.details(&lines[1]).await;
    assert_eq!(header(&details["requestHeaders"], "x-token"), Some("tok-123456"));
}

#[tokio::test]
async fn script_lines_and_their_requests_come_in_the_order_they_happened() {
    let server = TestServer::start().await;
    let h = Harness::new().await;
    h.env(json!([{ "key": "base", "value": server.url("") }])).await;
    let pre = r#"
        console.log('before');
        const answer = await pm.sendRequest(pm.variables.replaceIn('{{base}}/status/418'));
        console.warn('got', answer.code);
    "#;
    let post = "console.log('after', pm.response.code); throw new Error('boom');";
    let request = json!({ "name": "Scripted", "seq": 0, "method": "GET", "url": "{{base}}/json",
        "scripts": { "preRequest": pre, "postResponse": post } });
    let sent = h.ok("http.send", json!({ "requestId": "r1", "request": request })).await;
    // The response's own Console tab is as before.
    let own: Vec<&str> =
        sent["scripts"]["console"].as_array().unwrap().iter().map(|c| c["message"].as_str().unwrap()).collect();
    assert_eq!(own.len(), 4, "{own:?}");

    let lines = h.lines().await;
    let order: Vec<String> = lines.iter().map(brief).collect();
    assert_eq!(
        order,
        [
            "log: before",
            "GET /status/418 418 [pm.sendRequest]",
            "warn: got 418",
            "GET /json 200",
            "log: after 200",
            "error: Post-response script of request 'Scripted' failed at line 1: Error: boom",
        ],
        "{lines:#?}"
    );
    assert!(lines.iter().all(|l| l["request"] == "Scripted"));
    let ids: Vec<u64> = lines.iter().map(|l| l["id"].as_u64().unwrap()).collect();
    assert!(ids.windows(2).all(|w| w[0] < w[1]), "{ids:?}");

    // A pre-request script that fails sends nothing: only its error.
    h.ok("console.clear", json!({})).await;
    let request = json!({ "name": "Broken", "seq": 0, "method": "GET", "url": "{{base}}/json",
        "scripts": { "preRequest": "nope()" } });
    h.api.call("http.send", json!({ "requestId": "r2", "request": request })).await.expect_err("script error");
    let lines = h.lines().await;
    assert_eq!(lines.len(), 1, "{lines:#?}");
    assert!(brief(&lines[0]).starts_with("error: Pre-request script of request 'Broken' failed"), "{lines:#?}");
    assert!(h.events.0.lock().unwrap().iter().any(|e| e["type"] == "console" && e["event"]["type"] == "cleared"));
}

#[tokio::test]
async fn token_requests_redirects_and_binary_bodies() {
    let server = TestServer::start().await;
    let h = Harness::new().await;
    let request = json!({ "name": "Protected", "seq": 0, "method": "GET", "url": server.url("/oauth/protected"),
        "auth": { "type": "oauth2", "grantType": "clientCredentials", "tokenUrl": server.url("/oauth/token"),
                  "clientId": "test-client", "clientSecret": "test-secret" } });
    let sent = h.ok("http.send", json!({ "requestId": "r1", "request": request.clone() })).await;
    assert_eq!(sent["meta"]["status"], 200);
    let lines = h.lines().await;
    let order: Vec<String> = lines.iter().map(brief).collect();
    assert_eq!(order, ["POST /oauth/token 200 [OAuth 2.0 token]", "GET /oauth/protected 200"], "{lines:#?}");
    let token = h.details(&lines[0]).await;
    assert!(token["response"]["body"]["text"].as_str().unwrap().contains("access_token"));
    // The cached token is used the next time: no second token request.
    h.ok("http.send", json!({ "requestId": "r2", "request": request })).await;
    assert_eq!(h.lines().await.len(), 3);

    h.ok("console.clear", json!({})).await;
    let request = json!({ "name": "Around", "seq": 0, "method": "GET", "url": server.url("/redirect/2") });
    h.ok("http.send", json!({ "requestId": "r3", "request": request })).await;
    let lines = h.lines().await;
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0]["item"]["url"], server.url("/redirect/2"));
    let details = h.details(&lines[0]).await;
    assert_eq!(details["redirects"].as_array().unwrap().len(), 2);
    assert!(details["finalUrl"].as_str().is_some_and(|u| u != server.url("/redirect/2")), "{details}");

    let request = json!({ "name": "Picture", "seq": 0, "method": "GET", "url": server.url("/image.png") });
    h.ok("http.send", json!({ "requestId": "r4", "request": request })).await;
    let lines = h.lines().await;
    let details = h.details(&lines[1]).await;
    let body = &details["response"]["body"];
    assert!(body["text"].is_null() && body["size"].as_u64().is_some_and(|s| s > 0), "{body}");
    // Details of a line that is gone, or that never was.
    assert!(h.ok("console.details", json!({ "id": 1 })).await.is_null());
    assert!(h.ok("console.details", json!({ "id": 999_999 })).await.is_null());
}

#[tokio::test]
async fn connections_runs_and_another_workspace() {
    let server = TestServer::start().await;
    let h = Harness::new().await;
    let socket =
        json!({ "name": "Echo socket", "seq": 0, "kind": "websocket", "method": "GET", "url": server.ws_url("/ws") });
    let opened = h.ok("ws.connect", json!({ "connId": "c1", "request": socket })).await;
    assert_eq!(opened["meta"]["status"], 101);
    h.ok("ws.close", json!({ "connId": "c1" })).await;
    let closed =
        h.wait_for(|l| l["item"]["message"].as_str().is_some_and(|m| m.starts_with("WebSocket: Closed"))).await;
    assert_eq!(closed["request"], "Echo socket");
    let lines = h.lines().await;
    let item = &lines[0]["item"];
    assert_eq!((item["protocol"].as_str(), item["status"].as_u64()), (Some("WebSocket"), Some(101)));
    let details = h.details(&lines[0]).await;
    assert!(header(&details["requestHeaders"], "sec-websocket-key").is_some(), "{details}");
    assert!(header(&details["response"]["headers"], "sec-websocket-accept").is_some(), "{details}");

    // A connection that can't open says why.
    let free = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let down = json!({ "name": "Down socket", "seq": 0, "kind": "websocket", "method": "GET", "url": format!("ws://127.0.0.1:{free}/ws") });
    let err = h.api.call("ws.connect", json!({ "connId": "c2", "request": down })).await.expect_err("refused");
    let failed = h.wait_for(|l| l["request"] == "Down socket").await;
    assert_eq!(failed["item"]["error"], err.message);

    // Sent in one go (as a run does): the handshake's line, then what its on-connect steps did.
    h.ok("console.clear", json!({})).await;
    let steps = json!({ "name": "Steps", "seq": 0, "kind": "websocket", "method": "GET", "url": server.ws_url("/ws"),
        "websocket": { "messages": [{ "name": "Hello", "text": "hello" }],
            "onConnect": [{ "type": "send", "message": "Hello" }, { "type": "wait", "match": "contains", "pattern": "hello" }] } });
    let sent = h.ok("http.send", json!({ "requestId": "w1", "request": steps })).await;
    assert_eq!(sent["meta"]["status"], 101);
    let lines = h.lines().await;
    assert_eq!(lines[0]["item"]["status"], 101, "{lines:#?}");
    assert!(lines.len() > 1 && lines[1..].iter().all(|l| l["item"]["type"] == "log"), "{lines:#?}");

    // A collection run's requests are the runner's.
    h.ok("console.clear", json!({})).await;
    let saved = json!({ "name": "Listed", "seq": 0, "method": "GET", "url": server.url("/json"),
        "scripts": { "postResponse": "console.log('ran', pm.info.iteration)" } });
    let path = h.ok("request.create", json!({ "parent": "", "request": saved })).await;
    h.ok("runner.start", json!({ "requests": [path], "iterations": 2 })).await;
    h.wait_for(|l| l["item"]["message"] == "ran 1").await;
    let lines = h.lines().await;
    let order: Vec<String> = lines.iter().map(brief).collect();
    assert_eq!(order, ["GET /json 200", "log: ran 0", "GET /json 200", "log: ran 1"], "{lines:#?}");
    assert!(lines.iter().all(|l| l["source"] == "runner" && l["request"] == "Listed"));

    // The same workspace again keeps the console; another one starts empty.
    h.ok("workspace.open", json!({ "path": h.ws_dir.path() })).await;
    assert_eq!(h.lines().await.len(), 4);
    let other = tempfile::tempdir().unwrap();
    h.ok("workspace.create", json!({ "path": other.path(), "name": "Other" })).await;
    assert!(h.lines().await.is_empty());
}

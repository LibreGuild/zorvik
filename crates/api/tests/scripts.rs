//! Pre-request and post-response scripts through `http.send`, like the UI calls it.

use std::sync::Arc;

use serde_json::{Value, json};
use zorvik_api::{Api, EventSink, StreamEvent};
use zorvik_testkit::TestServer;

struct NoEvents;

impl EventSink for NoEvents {
    fn emit(&self, _event: StreamEvent) {}
}

struct Harness {
    api: Api,
    _data: tempfile::TempDir,
    ws_dir: tempfile::TempDir,
}

impl Harness {
    async fn new() -> Self {
        let data = tempfile::tempdir().unwrap();
        let ws_dir = tempfile::tempdir().unwrap();
        let api = Api::new(data.path().to_path_buf(), Arc::new(NoEvents));
        let h = Self { api, _data: data, ws_dir };
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

    async fn workspace_scripts(&self, pre: &str, post: &str) {
        let mut meta = self.ok("workspace.current", json!({})).await["meta"].clone();
        meta["scripts"] = json!({ "preRequest": pre, "postResponse": post });
        self.ok("workspace.saveMeta", json!({ "meta": meta })).await;
    }

    async fn env(&self, variables: Value) -> String {
        let id = self.ok("env.create", json!({ "environment": { "name": "Dev", "variables": variables } })).await;
        self.ok("env.setActive", json!({ "id": id })).await;
        id.as_str().unwrap().to_string()
    }

    /// Every file of the workspace, concatenated.
    fn files(&self) -> String {
        fn walk(dir: &std::path::Path, out: &mut String) {
            for entry in std::fs::read_dir(dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, out);
                } else {
                    out.push_str(&std::fs::read_to_string(&path).unwrap_or_default());
                }
            }
        }
        let mut out = String::new();
        walk(self.ws_dir.path(), &mut out);
        out
    }
}

fn request(name: &str, url: &str, pre: &str, post: &str) -> Value {
    json!({ "name": name, "seq": 0, "method": "GET", "url": url,
            "scripts": { "preRequest": pre, "postResponse": post } })
}

fn tests_of(result: &Value) -> Vec<(String, bool, Option<String>)> {
    result["scripts"]["tests"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| {
            (t["name"].as_str().unwrap().into(), t["passed"].as_bool().unwrap(), t["error"].as_str().map(Into::into))
        })
        .collect()
}

fn var<'a>(list: &'a Value, key: &str) -> &'a Value {
    list.as_array().unwrap().iter().find(|v| v["key"] == key).unwrap_or_else(|| panic!("{key} missing: {list}"))
}

#[tokio::test]
async fn scripts_set_values_that_the_request_uses() {
    let server = TestServer::start().await;
    let h = Harness::new().await;
    let env_id = h
        .env(json!([
            { "key": "base", "value": server.url("") },
            { "key": "token", "value": "file-token" }
        ]))
        .await;
    h.workspace_scripts(
        // Values computed, so they don't appear in the script text saved in the workspace.
        "pm.environment.set('token', ['from', 'script'].join('_'));\npm.collectionVariables.set('wsv', 'wsv' + 42);\npm.globals.set('g', 'G'.repeat(3));",
        "pm.test('workspace sees the response', () => pm.expect(pm.response.code).to.equal(200));",
    )
    .await;
    let folder = h.ok("folder.create", json!({ "parent": "", "name": "Users" })).await;
    let mut meta = h.ok("folder.read", json!({ "path": folder })).await;
    meta["scripts"] = json!({ "preRequest": "pm.variables.set('q', pm.environment.get('token') + '-q');\npm.request.headers.add({ key: 'X-Folder', value: '1' });" });
    h.ok("folder.save", json!({ "path": folder, "meta": meta })).await;
    let req = request(
        "Echo",
        "{{base}}/echo?t={{token}}&q={{q}}&w={{wsv}}&g={{g}}",
        "console.log('pre', pm.info.requestName, pm.request.url.toString());",
        r#"
        const body = pm.response.json();
        pm.test('status', () => pm.response.to.have.status(200));
        const token = pm.environment.get('token');
        pm.test('args', () => pm.expect(body.args).to.eql({ t: token, q: token + '-q', w: pm.collectionVariables.get('wsv'), g: pm.globals.get('g') }));
        pm.test('header', () => pm.expect(body.headers.some(h => h[0] === 'x-folder')).to.be.true);
        pm.test('sent url', () => pm.expect(pm.request.url.toString()).to.include('t=' + token));
        pm.environment.set('fromPost', body.args.t);
        "#,
    );
    let path = h.ok("request.create", json!({ "parent": folder, "request": req })).await;
    let saved = h.ok("request.read", json!({ "path": path })).await;

    let result = h.ok("http.send", json!({ "requestId": "s1", "request": saved, "path": path })).await;
    assert_eq!(result["meta"]["status"], 200);
    let tests = tests_of(&result);
    assert_eq!(tests.len(), 5, "{tests:?}");
    assert!(tests.iter().all(|t| t.1), "{tests:?}");
    assert_eq!(tests[0].0, "workspace sees the response");
    assert_eq!(
        result["scripts"]["console"][0]["message"],
        "pre Echo {{base}}/echo?t={{token}}&q={{q}}&w={{wsv}}&g={{g}}"
    );
    assert_eq!(result["scripts"]["errors"], json!([]));

    // Values set by scripts are local current values: never in the workspace files.
    let files = h.files();
    assert!(files.contains("file-token"));
    for value in ["from_script", "wsv42", "GGG"] {
        assert!(!files.contains(value), "{value} was written to the workspace:\n{files}");
    }
    let local = h.ok("vars.local", json!({})).await;
    assert_eq!(var(&local, "token")["value"], "from_script");
    assert_eq!(var(&local, "token")["scope"], "environment");
    assert_eq!(var(&local, "token")["environmentId"], env_id);
    assert_eq!(var(&local, "fromPost")["value"], "from_script");
    assert_eq!(var(&local, "wsv")["scope"], "workspace");
    assert_eq!(var(&local, "g")["scope"], "globals");
    assert!(local.as_array().unwrap().iter().all(|v| v["key"] != "q"), "pm.variables are for this send only");

    // They win over file values everywhere, until cleared.
    let vars = h.ok("vars.list", json!({})).await;
    assert_eq!(var(&vars, "token")["value"], "from_script");
    assert_eq!(var(&vars, "token")["local"], true);
    assert_eq!(var(&vars, "base")["local"], false);
    assert_eq!(var(&vars, "g")["source"], "globals");
    assert_eq!(h.ok("vars.render", json!({ "text": "{{token}}/{{wsv}}" })).await, "from_script/wsv42");
    h.ok("vars.clearLocal", json!({ "scope": "environment", "environmentId": env_id, "key": "token" })).await;
    assert_eq!(h.ok("vars.render", json!({ "text": "{{token}}/{{wsv}}" })).await, "file-token/wsv42");
    h.ok("vars.clearLocal", json!({ "scope": "workspace" })).await;
    h.ok("vars.clearLocal", json!({ "scope": "globals", "key": "g" })).await;
    let local = h.ok("vars.local", json!({})).await;
    let keys: Vec<&str> = local.as_array().unwrap().iter().map(|v| v["key"].as_str().unwrap()).collect();
    assert_eq!(keys, ["fromPost"]);
    assert_eq!(h.err("vars.clearLocal", json!({ "scope": "environment" })).await.code, "invalidInput");
}

#[tokio::test]
async fn test_results_and_post_response_errors_are_reported() {
    let server = TestServer::start().await;
    let h = Harness::new().await;
    h.workspace_scripts("", "throw new Error('workspace broke');").await;
    let req = request(
        "r",
        &server.url("/status/404"),
        "",
        "pm.test('ok', () => pm.response.to.be.ok);\npm.test('not found', () => pm.expect(pm.response.code).to.equal(404));\nconsole.warn('done');",
    );
    let result = h.ok("http.send", json!({ "requestId": "s2", "request": req })).await;
    assert_eq!(result["meta"]["status"], 404, "a post-response error does not fail the request");
    assert_eq!(
        tests_of(&result),
        [
            ("ok".into(), false, Some("expected response code to be 200 but found 404".into())),
            ("not found".into(), true, None),
        ]
    );
    assert_eq!(result["scripts"]["console"], json!([{ "level": "warn", "message": "done" }]));
    assert_eq!(
        result["scripts"]["errors"],
        json!([{ "script": "Post-response script of workspace", "message": "Error: workspace broke", "line": 1 }])
    );

    // No scripts: no report.
    h.workspace_scripts("", "").await;
    let result =
        h.ok("http.send", json!({ "requestId": "s3", "request": request("r", &server.url("/echo"), "", "") })).await;
    assert!(result.get("scripts").is_none(), "{result}");
}

#[tokio::test]
async fn pre_request_errors_stop_the_send() {
    let server = TestServer::start().await;
    let h = Harness::new().await;
    let req = request("Get user", &server.url("/echo"), "pm.environment.set('a', 1);\n\nnope();", "");
    let err = h.err("http.send", json!({ "requestId": "s4", "request": req })).await;
    assert_eq!(err.code, "script");
    assert_eq!(
        err.message,
        "Pre-request script of request 'Get user' failed at line 3: ReferenceError: nope is not defined"
    );
    // Nothing was sent: no history entry.
    assert_eq!(h.ok("history.list", json!({})).await, json!([]));

    // Unsupported APIs fail clearly; tool (standalone) sends run no scripts.
    h.workspace_scripts("pm.sendRequest('https://example.com', () => {});", "").await;
    let err =
        h.err("http.send", json!({ "requestId": "s5", "request": request("r", &server.url("/echo"), "", "") })).await;
    assert!(err.message.ends_with("Error: pm.sendRequest is not supported in Zorvik"), "{}", err.message);
    let result = h
        .ok(
            "http.send",
            json!({ "requestId": "s6", "request": request("r", &server.url("/echo"), "", ""), "standalone": true }),
        )
        .await;
    assert_eq!(result["meta"]["status"], 200);
}

#[tokio::test]
async fn script_limits_and_missing_environment() {
    let server = TestServer::start().await;
    let h = Harness::new().await;
    let mut settings = h.ok("settings.get", json!({})).await;
    assert_eq!(settings["scriptTimeoutMs"], 5000);
    settings["scriptTimeoutMs"] = json!(200);
    h.ok("settings.save", json!({ "settings": settings })).await;
    let err = h
        .err(
            "http.send",
            json!({ "requestId": "s7", "request": request("r", &server.url("/echo"), "while (true) {}", "") }),
        )
        .await;
    assert!(err.message.contains("took longer than 0.2 s"), "{}", err.message);

    // No active environment: pm.environment values are not kept (and say so).
    let req =
        request("r", &server.url("/echo?x={{x}}"), "pm.environment.set('x', 'y'); pm.variables.set('x', 'z');", "");
    let result = h.ok("http.send", json!({ "requestId": "s8", "request": req })).await;
    assert_eq!(result["meta"]["url"].as_str().unwrap(), server.url("/echo?x=z"));
    assert!(result["scripts"]["console"][0]["message"].as_str().unwrap().contains("No environment is active"));
    assert_eq!(h.ok("vars.local", json!({})).await, json!([]));
}

#[tokio::test]
async fn secrets_changed_by_scripts_stay_out_of_history() {
    let server = TestServer::start().await;
    let h = Harness::new().await;
    h.env(json!([
        { "key": "key", "value": "old-secret-key", "secret": true },
        { "key": "token", "value": "", "secret": true }
    ]))
    .await;
    // A post-response script rotates the key after it went out in the URL; a pre-request
    // script sets another secret that the URL uses.
    let req = request(
        "r",
        &server.url("/echo?k={{key}}&t={{token}}"),
        "pm.environment.set('token', 'tok-' + 'computed');",
        "pm.environment.set('key', 'new-' + 'secret-key');",
    );
    let result = h.ok("http.send", json!({ "requestId": "s9", "request": req })).await;
    assert_eq!(result["meta"]["status"], 200);
    let history = h.ok("history.list", json!({})).await;
    let url = history[0]["url"].as_str().unwrap();
    assert!(url.ends_with("/echo?k={{key}}&t={{token}}"), "{url}");
}

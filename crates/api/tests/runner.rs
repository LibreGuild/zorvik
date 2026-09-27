//! Collection runs through `runner.*`, like the UI calls them: order, scripts and
//! tests, data files, skips, stop on failure, setNextRequest, stop and exports.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};
use zorvik_api::{Api, EventSink, StreamEvent};
use zorvik_testkit::TestServer;

#[derive(Default)]
struct Events(Mutex<Vec<Value>>);

impl EventSink for Events {
    fn emit(&self, event: StreamEvent) {
        if let StreamEvent::Runner { .. } = event {
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
        h.ok("workspace.create", json!({ "path": h.ws_dir.path(), "name": "Shop" })).await;
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

    async fn request(&self, parent: &str, name: &str, url: &str, pre: &str, post: &str) -> String {
        let request = json!({ "name": name, "method": "GET", "url": url,
                              "scripts": { "preRequest": pre, "postResponse": post } });
        self.ok("request.create", json!({ "parent": parent, "request": request })).await.as_str().unwrap().to_string()
    }

    async fn env(&self, variables: Value) -> String {
        let id = self.ok("env.create", json!({ "environment": { "name": "Local", "variables": variables } })).await;
        self.ok("env.setActive", json!({ "id": id })).await;
        id.as_str().unwrap().to_string()
    }

    /// Start a run and wait for it to finish: (run id, results, summary).
    async fn run(&self, params: Value) -> (String, Vec<Value>, Value) {
        let started = self.ok("runner.start", params).await;
        let run_id = started["runId"].as_str().unwrap().to_string();
        let summary = self.finished(&run_id).await;
        (run_id.clone(), self.results(&run_id), summary)
    }

    async fn finished(&self, run_id: &str) -> Value {
        for _ in 0..400 {
            if let Some(summary) = self.event(run_id, "finished").map(|e| e["summary"].clone()) {
                return summary;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("the run did not finish: {:?}", self.events.0.lock().unwrap());
    }

    fn event(&self, run_id: &str, kind: &str) -> Option<Value> {
        self.events
            .0
            .lock()
            .unwrap()
            .iter()
            .find(|e| e["runId"] == run_id && e["event"]["type"] == kind)
            .map(|e| e["event"].clone())
    }

    fn results(&self, run_id: &str) -> Vec<Value> {
        self.events
            .0
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e["runId"] == run_id && e["event"]["type"] == "result")
            .map(|e| e["event"]["result"].clone())
            .collect()
    }
}

/// `(iteration, name, status, passed)` of each result.
fn rows(results: &[Value]) -> Vec<(u64, String, Option<u64>, bool)> {
    results
        .iter()
        .map(|r| {
            (
                r["iteration"].as_u64().unwrap(),
                r["name"].as_str().unwrap().to_string(),
                r["status"].as_u64(),
                r["passed"].as_bool().unwrap(),
            )
        })
        .collect()
}

fn names(results: &[Value]) -> Vec<String> {
    results.iter().map(|r| r["name"].as_str().unwrap().to_string()).collect()
}

#[tokio::test]
async fn runs_a_folder_in_order_with_scripts_data_and_skips() {
    let server = TestServer::start().await;
    let h = Harness::new().await;
    h.env(json!([{ "key": "base", "value": server.url("") }, { "key": "key", "value": "top-secret", "secret": true }]))
        .await;
    let folder = h.ok("folder.create", json!({ "parent": "", "name": "Users" })).await;
    let folder = folder.as_str().unwrap();
    h.request(
        folder,
        "Login",
        "{{base}}/echo?user={{user}}&k={{key}}",
        "pm.variables.set('seen', (pm.variables.get('seen') || '') + pm.iterationData.get('user'));",
        r#"pm.test('status', () => pm.response.to.have.status(200));
           pm.test('user from data', () => pm.expect(pm.response.json().args.user).to.equal(pm.iterationData.get('user')));
           pm.environment.set('token', 'tok-' + pm.info.iteration);
           console.log('iteration', pm.info.iteration, 'of', pm.info.iterationCount);"#,
    )
    .await;
    let socket = json!({ "name": "Socket", "kind": "websocket", "url": "ws://127.0.0.1:1/ws" });
    h.ok("request.create", json!({ "parent": folder, "request": socket })).await;
    h.request(
        folder,
        "Profile",
        "{{base}}/echo?t={{token}}&seen={{seen}}",
        "",
        "pm.test('token', () => pm.expect(pm.response.json().args.t).to.equal('tok-' + pm.info.iteration));",
    )
    .await;
    // Outside the folder: not part of the run.
    h.request("", "Other", "{{base}}/status/500", "", "").await;
    std::fs::write(h.ws_dir.path().join("users.csv"), "user,extra\r\nada,1\r\n\"gr,ace\",2\r\n").unwrap();

    let preview = h.ok("runner.preview", json!({ "dataFile": "users.csv" })).await;
    assert_eq!(
        preview,
        json!({ "format": "csv", "columns": ["user", "extra"], "rows": [["ada", "1"], ["gr,ace", "2"]], "count": 2 })
    );

    let started = h.ok("runner.start", json!({ "folder": folder, "dataFile": "users.csv" })).await;
    assert_eq!(started["name"], "Users");
    assert_eq!(started["iterations"], 2, "one iteration per data row");
    assert_eq!(started["total"], 6);
    assert_eq!(started["environment"], "Local");
    let run_id = started["runId"].as_str().unwrap().to_string();
    let summary = h.finished(&run_id).await;
    assert_eq!(
        h.event(&run_id, "started").unwrap(),
        json!({ "type": "started", "name": "Users", "total": 6, "iterations": 2 })
    );
    let results = h.results(&run_id);
    assert_eq!(
        rows(&results),
        [
            (0, "Login".into(), Some(200), true),
            (0, "Socket".into(), None, true),
            (0, "Profile".into(), Some(200), true),
            (1, "Login".into(), Some(200), true),
            (1, "Socket".into(), None, true),
            (1, "Profile".into(), Some(200), true),
        ]
    );
    assert_eq!(results[1]["skipped"], true);
    assert_eq!(results[1]["kind"], "websocket");
    // Secret values never show in reported URLs; pm.variables carry through the run.
    let url = results[3]["url"].as_str().unwrap();
    assert!(url.ends_with("/echo?user=gr,ace&k={{key}}"), "{url}");
    assert!(results[5]["url"].as_str().unwrap().ends_with("t=tok-1&seen=adagr,ace"), "{}", results[5]["url"]);
    assert_eq!(results[3]["console"], json!([{ "level": "log", "message": "iteration 1 of 2" }]));
    assert_eq!(results[3]["tests"].as_array().unwrap().len(), 2);
    assert_eq!(summary["passed"], true);
    assert_eq!(
        (summary["requests"].clone(), summary["failed"].clone(), summary["skipped"].clone()),
        (json!(4), json!(0), json!(2))
    );
    assert_eq!((summary["testsPassed"].clone(), summary["testsFailed"].clone()), (json!(6), json!(0)));
    assert_eq!(summary["perIteration"].as_array().unwrap().len(), 2);
    // Environment values set by scripts are kept like single sends keep them.
    let local = h.ok("vars.local", json!({})).await;
    assert!(local.as_array().unwrap().iter().any(|v| v["key"] == "token" && v["value"] == "tok-1"), "{local}");

    // Exports: the JSON report and JUnit XML.
    let json_path = h.ws_dir.path().join("out/report.json");
    std::fs::create_dir_all(json_path.parent().unwrap()).unwrap();
    h.ok("runner.export", json!({ "runId": run_id, "path": json_path, "format": "json" })).await;
    let report: Value = serde_json::from_slice(&std::fs::read(&json_path).unwrap()).unwrap();
    assert_eq!(report["summary"]["requests"], 4);
    assert_eq!(report["results"].as_array().unwrap().len(), 6);
    let junit_path = h.ws_dir.path().join("out/report.xml");
    h.ok("runner.export", json!({ "runId": run_id, "path": junit_path, "format": "junit" })).await;
    let xml = std::fs::read_to_string(&junit_path).unwrap();
    assert!(xml.contains(r#"<testsuite name="Users/Login" tests="4" failures="0""#), "{xml}");
    assert!(xml.contains(r#"<testcase name="user from data (iteration 2)""#), "{xml}");
    assert_eq!(
        h.err("runner.export", json!({ "runId": "nope", "path": junit_path, "format": "json" })).await.code,
        "notFound"
    );
    let err = h.err("runner.export", json!({ "runId": run_id, "path": junit_path, "format": "html" })).await;
    assert!(err.message.contains("Unknown report format"), "{}", err.message);
}

#[tokio::test]
async fn explicit_order_iterations_and_http_errors() {
    let server = TestServer::start().await;
    let h = Harness::new().await;
    let a = h.request("", "A", &server.url("/status/200"), "", "").await;
    let b = h.request("", "B", &server.url("/status/404"), "", "").await;
    let c = h
        .request(
            "",
            "C",
            &server.url("/status/404"),
            "",
            "pm.test('not found', () => pm.response.to.have.status(404));",
        )
        .await;

    // Only the chosen requests, in the chosen order; an HTTP error fails a request without tests.
    let (_, results, summary) = h.run(json!({ "requests": [c, a, b], "iterations": 2, "delayMs": 20 })).await;
    assert_eq!(names(&results), ["C", "A", "B", "C", "A", "B"]);
    assert_eq!(
        rows(&results)[..3],
        [(0, "C".into(), Some(404), true), (0, "A".into(), Some(200), true), (0, "B".into(), Some(404), false)]
    );
    assert_eq!(summary["name"], "Shop", "the whole collection is named after the workspace");
    assert_eq!((summary["failed"].clone(), summary["passed"].clone()), (json!(2), json!(false)));
    assert!(summary["durationMs"].as_f64().unwrap() >= 100.0, "5 delays of 20 ms: {summary}");

    let (_, results, summary) = h.run(json!({ "requests": [b], "allowHttpErrors": true })).await;
    assert_eq!(rows(&results), [(0, "B".into(), Some(404), true)]);
    assert_eq!(summary["passed"], true);

    // Stop on the first failure.
    let (_, results, summary) = h.run(json!({ "requests": [a, b, c], "iterations": 3, "stopOnFailure": true })).await;
    assert_eq!(names(&results), ["A", "B"]);
    assert_eq!((summary["bailed"].clone(), summary["passed"].clone()), (json!(true), json!(false)));

    // A missing request is reported as a failed result, not a refused run.
    let (_, results, _) = h.run(json!({ "requests": ["gone.yaml", a] })).await;
    assert_eq!(rows(&results), [(0, "gone".into(), None, false), (0, "A".into(), Some(200), true)]);
    assert!(results[0]["error"].as_str().unwrap().starts_with("The request can't be read"), "{}", results[0]["error"]);
}

#[tokio::test]
async fn set_next_request_jumps_and_ends_iterations() {
    let server = TestServer::start().await;
    let h = Harness::new().await;
    let url = server.url("/echo");
    // Login → Poll (three times, counted with pm.variables) → Done; Skipped is jumped over.
    h.request("", "Login", &url, "", "pm.execution.setNextRequest('Poll');").await;
    h.request("", "Skipped", &url, "", "").await;
    h.request(
        "",
        "Poll",
        &url,
        "",
        r#"const n = Number(pm.variables.get('polls') || 0) + 1;
           pm.variables.set('polls', n);
           postman.setNextRequest(n < 3 ? 'Poll' : 'Done');"#,
    )
    .await;
    h.request("", "Done", &url, "", "pm.execution.setNextRequest(null);").await;
    h.request("", "After", &url, "", "").await;
    let (_, results, summary) = h.run(json!({})).await;
    assert_eq!(names(&results), ["Login", "Poll", "Poll", "Poll", "Done"]);
    assert_eq!(summary["passed"], true);

    // An unknown name ends the iteration with a warning.
    h.request("", "Lost", &url, "", "pm.execution.setNextRequest('Nowhere');").await;
    let lost = h.ok("workspace.tree", json!({})).await;
    let lost_path = lost.as_array().unwrap().iter().find(|n| n["name"] == "Lost").unwrap()["path"].clone();
    let after_path = lost.as_array().unwrap().iter().find(|n| n["name"] == "After").unwrap()["path"].clone();
    let (_, results, _) = h.run(json!({ "requests": [lost_path, after_path], "iterations": 2 })).await;
    assert_eq!(names(&results), ["Lost", "Lost"]);
    assert!(results[0]["console"][0]["message"].as_str().unwrap().contains("no request named 'Nowhere'"));
}

#[tokio::test]
async fn stop_one_run_at_a_time_and_checks() {
    let server = TestServer::start().await;
    let h = Harness::new().await;
    let slow = h.request("", "Slow", &server.url("/delay/3000"), "", "").await;
    let started = h.ok("runner.start", json!({ "requests": [slow], "iterations": 5 })).await;
    let run_id = started["runId"].as_str().unwrap().to_string();
    let err = h.err("runner.start", json!({ "requests": [slow] })).await;
    assert!(err.message.contains("in progress"), "{}", err.message);
    // Normal sends are not blocked by a run.
    let ok = json!({ "name": "Ok", "method": "GET", "url": server.url("/status/200") });
    assert_eq!(h.ok("http.send", json!({ "requestId": "x", "request": ok })).await["meta"]["status"], 200);
    tokio::time::sleep(Duration::from_millis(100)).await;
    h.ok("runner.stop", json!({ "runId": run_id })).await;
    let summary = h.finished(&run_id).await;
    assert_eq!(summary["stopped"], true);
    assert!(h.results(&run_id).is_empty(), "the request in flight is not reported");
    assert!(summary["durationMs"].as_f64().unwrap() < 2500.0, "{summary}");
    // The next run can start.
    let fast = h.request("", "Fast", &server.url("/status/200"), "", "").await;
    let (_, results, _) = h.run(json!({ "requests": [fast] })).await;
    assert_eq!(results.len(), 1);

    // Checks before a run.
    let err = h.err("runner.start", json!({ "folder": "missing" })).await;
    assert_eq!(err.message, "Folder 'missing' not found");
    let empty = h.ok("folder.create", json!({ "parent": "", "name": "Empty" })).await;
    let err = h.err("runner.start", json!({ "folder": empty })).await;
    assert_eq!(err.message, "'Empty' has no requests to run");
    let err = h.err("runner.start", json!({ "iterations": 0 })).await;
    assert!(err.message.contains("Iterations"), "{}", err.message);
    let outside = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(outside.path(), "a\n1\n").unwrap();
    let err = h.err("runner.start", json!({ "dataFile": outside.path() })).await;
    assert!(err.message.contains("outside the workspace folder"), "{}", err.message);
    let err = h.err("runner.preview", json!({ "dataFile": outside.path() })).await;
    assert!(err.message.contains("outside the workspace folder"), "{}", err.message);
    std::fs::write(h.ws_dir.path().join("bad.json"), "{\"a\": 1}").unwrap();
    let err = h.err("runner.preview", json!({ "dataFile": "bad.json" })).await;
    assert_eq!(err.message, "JSON data must be an array of objects, one per iteration");
}

#[tokio::test]
async fn stop_during_the_delay_and_secrets_set_by_scripts_stay_hidden() {
    let server = TestServer::start().await;
    let h = Harness::new().await;
    // `token` is declared secret but has no value until Login's script sets one.
    h.env(json!([{ "key": "token", "value": "", "secret": true }])).await;
    let login =
        h.request("", "Login", &server.url("/echo"), "", "pm.environment.set('token', 'tok-from-login');").await;
    let profile = h.request("", "Profile", &format!("{}?t={{{{token}}}}", server.url("/echo")), "", "").await;
    let (_, results, summary) = h.run(json!({ "requests": [login, profile.clone()] })).await;
    let url = results[1]["url"].as_str().unwrap();
    assert!(url.ends_with("/echo?t={{token}}"), "{url}");
    assert_eq!(summary["omitted"], 0);

    // Stop while waiting between two requests: at once, and nothing more is reported.
    let started = h.ok("runner.start", json!({ "requests": [profile], "iterations": 3, "delayMs": 5000 })).await;
    let run_id = started["runId"].as_str().unwrap().to_string();
    for _ in 0..400 {
        if !h.results(&run_id).is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    h.ok("runner.stop", json!({ "runId": run_id })).await;
    let summary = h.finished(&run_id).await;
    assert_eq!(summary["stopped"], true);
    assert_eq!(h.results(&run_id).len(), 1);
    assert!(summary["durationMs"].as_f64().unwrap() < 4000.0, "{summary}");
}

//! Training Bootcamp: every lab of the course can be finished, one step at a time. For each
//! step: it has not passed before its solution runs, and passes after it (as "Do it for me"
//! would do it). Run one lesson with `ACADEMY_LESSON=<id> cargo test -p zorvik-api --test academy`.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};
use zorvik_api::{Api, EventSink, StreamEvent};

#[derive(Default)]
struct Collect(Mutex<Vec<StreamEvent>>);

impl EventSink for Collect {
    fn emit(&self, event: StreamEvent) {
        self.0.lock().unwrap().push(event);
    }
}

async fn ok(api: &Api, method: &str, params: Value) -> Value {
    match api.call(method, params).await {
        Ok(v) => v,
        Err(e) => panic!("{method} failed: {} ({})", e.message, e.code),
    }
}

async fn bootcamp() -> (Api, Arc<Collect>, tempfile::TempDir) {
    let data = tempfile::tempdir().unwrap();
    let events = Arc::new(Collect::default());
    let api = Api::new(data.path().to_path_buf(), events.clone());
    let path = ok(&api, "academy.workspace", json!({})).await["path"].clone();
    ok(&api, "workspace.open", json!({ "path": path })).await;
    (api, events, data)
}

fn done(lab: &Value, step: usize) -> bool {
    lab["steps"][step]["done"] == json!(true)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_lab_can_be_finished() {
    let course = zorvik_academy::course().unwrap_or_else(|e| panic!("{e}"));
    let only = std::env::var("ACADEMY_LESSON").ok();
    let (api, _events, _data) = bootcamp().await;
    let mut failures = Vec::new();
    for lesson in course.lessons().filter(|l| l.lab.is_some()) {
        if only.as_deref().is_some_and(|o| o != lesson.id) {
            continue;
        }
        let steps = lesson.lab.as_ref().unwrap().steps.len();
        let lab = match api.call("academy.startLab", json!({ "id": lesson.id })).await {
            Ok(lab) => lab,
            Err(e) => {
                failures.push(format!("{}: could not start: {}", lesson.id, e.message));
                continue;
            }
        };
        assert_eq!(lab["steps"].as_array().map(Vec::len), Some(steps));
        for step in 0..steps {
            let lab = ok(&api, "academy.check", json!({})).await;
            if done(&lab, step) {
                failures.push(format!("{} step {}: passes before its solution runs", lesson.id, step + 1));
                break;
            }
            let lab = match api.call("academy.doStep", json!({ "step": step })).await {
                Ok(lab) => lab,
                Err(e) => {
                    failures.push(format!("{} step {}: solution failed: {}", lesson.id, step + 1, e.message));
                    break;
                }
            };
            if !done(&lab, step) {
                failures.push(format!("{} step {}: does not pass after its solution", lesson.id, step + 1));
                break;
            }
        }
        ok(&api, "academy.stopLab", json!({})).await;
    }
    assert!(failures.is_empty(), "labs that can't be finished:\n{}", failures.join("\n"));
}

#[tokio::test]
async fn progress_rewards_and_workspace() {
    let (api, events, data) = bootcamp().await;
    let course = ok(&api, "academy.course", json!({})).await;
    assert!(course["units"].as_array().is_some_and(|u| !u.is_empty()));

    // Labs only run in the Bootcamp workspace.
    let other = tempfile::tempdir().unwrap();
    ok(&api, "workspace.create", json!({ "path": other.path(), "name": "Other" })).await;
    let e = api.call("academy.startLab", json!({ "id": "first-request" })).await.unwrap_err();
    assert_eq!(e.code, "notBootcamp");
    let path = ok(&api, "academy.workspace", json!({})).await["path"].clone();
    ok(&api, "workspace.open", json!({ "path": path })).await;

    // A lab: servers saved as "Lab · …", the Lab environment active, a step done by hand.
    let lab = ok(&api, "academy.startLab", json!({ "id": "first-request" })).await;
    let url = lab["vars"].as_array().unwrap().iter().find(|v| v["key"] == "api").unwrap()["value"].clone();
    let servers = ok(&api, "server.list", json!({})).await;
    assert!(servers.as_array().unwrap().iter().any(|s| s["name"].as_str().unwrap().starts_with("Lab · ")));
    let request = json!({ "name": "r", "seq": 0, "method": "GET", "url": "{{api}}/hello" });
    let sent = ok(&api, "http.send", json!({ "requestId": "1", "request": request, "path": null })).await;
    assert_eq!(sent["meta"]["status"], 200);
    assert!(sent["meta"]["url"].as_str().unwrap().starts_with(url.as_str().unwrap()));
    let mut passed = false;
    for _ in 0..50 {
        if done(&ok(&api, "academy.lab", json!({})).await, 0) {
            passed = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(passed, "the step passes after the request");

    // A wrong answer doesn't pass; the secret word does.
    let word = sent["body"]["text"].as_str().and_then(|t| serde_json::from_str::<Value>(t).ok()).unwrap()["secretWord"]
        .clone();
    let r = ok(&api, "academy.answer", json!({ "step": 1, "value": "nope" })).await;
    assert_eq!(r["correct"], false);
    let r = ok(&api, "academy.answer", json!({ "step": 1, "value": word })).await;
    assert_eq!(r["correct"], true);
    assert_eq!(r["lab"]["finished"], true);

    let progress = ok(&api, "academy.progress", json!({})).await;
    assert_eq!(progress["xp"], 20, "two steps done by hand");
    let rewarded = events
        .0
        .lock()
        .unwrap()
        .iter()
        .any(|e| matches!(e, StreamEvent::Academy { update } if update.rewards.is_some()));
    assert!(rewarded);

    // The quiz completes the lesson.
    let quiz = ok(&api, "academy.quiz", json!({ "id": "first-request", "answers": [0, 1, 1] })).await;
    assert_eq!((quiz["right"].clone(), quiz["passed"].clone()), (json!(3), json!(true)));
    let progress = ok(&api, "academy.progress", json!({})).await;
    let state = progress["lessons"].as_array().unwrap().iter().find(|l| l["id"] == "first-request").unwrap().clone();
    assert_eq!(state["completed"], true);
    assert!(progress["badges"].as_array().unwrap().iter().any(|b| b["id"] == "perfect-score"));

    // Test-out: one answer per question counts (sending every option doesn't pass), and a
    // failed attempt doesn't give the answers away.
    let questions = ok(&api, "academy.testOut", json!({ "id": "welcome" })).await;
    let all: Vec<Value> = questions
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|q| (0..q["options"].as_array().unwrap().len()).map(move |i| json!([q["id"], i])))
        .collect();
    let r = ok(&api, "academy.testOutSubmit", json!({ "id": "welcome", "answers": all })).await;
    assert_eq!(r["passed"], false);
    assert_eq!(r["results"], json!([]));

    // Progress survives a restart; reset keeps it but empties the workspace.
    ok(&api, "academy.stopLab", json!({})).await;
    ok(
        &api,
        "request.create",
        json!({ "parent": "", "request": { "name": "Mine", "seq": 0, "method": "GET", "url": "x" } }),
    )
    .await;
    ok(&api, "academy.reset", json!({})).await;
    let tree = ok(&api, "workspace.tree", json!({})).await;
    assert_eq!(tree, json!([]));
    let again = Api::new(data.path().to_path_buf(), Arc::new(Collect::default()));
    assert_eq!(ok(&again, "academy.progress", json!({})).await["xp"], progress["xp"]);
}

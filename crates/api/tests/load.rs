//! Load test files and the checks before a run (the generator has its own tests).

use std::sync::Arc;

use serde_json::{Value, json};
use zorvik_api::{Api, EventSink, StreamEvent};

struct Quiet;
impl EventSink for Quiet {
    fn emit(&self, _: StreamEvent) {}
}

async fn api() -> (Api, tempfile::TempDir, tempfile::TempDir) {
    let data = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    let api = Api::new(data.path().to_path_buf(), Arc::new(Quiet));
    api.call("workspace.create", json!({ "path": ws.path(), "name": "Load" })).await.unwrap();
    (api, data, ws)
}

async fn ok(api: &Api, method: &str, params: Value) -> Value {
    api.call(method, params).await.unwrap_or_else(|e| panic!("{method}: {}", e.message))
}

fn test_json(targets: &[&str]) -> Value {
    json!({ "name": "Smoke", "targets": targets.iter().map(|t| json!({ "request": t })).collect::<Vec<_>>(),
            "stages": [{ "durationSecs": 1, "target": 1 }] })
}

#[tokio::test]
async fn files_round_trip_and_rename_keeps_history_folder() {
    let (api, _data, _ws) = api().await;
    let id: String = serde_json::from_value(ok(&api, "load.create", json!({ "test": test_json(&[]) })).await).unwrap();
    assert_eq!(ok(&api, "load.list", json!({})).await[0]["name"], "Smoke");
    let mut test = ok(&api, "load.read", json!({ "id": id })).await;
    test["name"] = json!("Smoke 2");
    let new_id: String =
        serde_json::from_value(ok(&api, "load.save", json!({ "id": id, "test": test })).await).unwrap();
    assert_eq!(new_id, "Smoke 2");
    assert_eq!(ok(&api, "load.runs", json!({ "id": new_id })).await, json!([]));
    assert_eq!(ok(&api, "load.active", json!({})).await, Value::Null);
}

#[tokio::test]
async fn start_checks_targets_and_asks_before_outside_hosts() {
    let (api, _data, _ws) = api().await;
    // No targets.
    let err = api.call("load.start", json!({ "id": "x", "test": test_json(&[]) })).await.unwrap_err();
    assert!(err.message.contains("at least one request"), "{}", err.message);
    // A missing request.
    let err = api.call("load.start", json!({ "id": "x", "test": test_json(&["missing.yaml"]) })).await.unwrap_err();
    assert!(err.message.contains("missing.yaml"), "{}", err.message);
    // A public host needs confirmation; a local one doesn't get that far.
    let path: String = serde_json::from_value(
        ok(
            &api,
            "request.create",
            json!({ "parent": "", "request": { "name": "Ext", "method": "GET", "url": "https://example.com/" } }),
        )
        .await,
    )
    .unwrap();
    let err = api.call("load.start", json!({ "id": "x", "test": test_json(&[&path]) })).await.unwrap_err();
    assert_eq!(err.code, "confirmTarget");
    assert!(err.message.contains("example.com"), "{}", err.message);
    // WebSocket requests can't be load tested.
    let ws_path: String = serde_json::from_value(
        ok(
            &api,
            "request.create",
            json!({ "parent": "", "request": { "name": "Socket", "kind": "websocket", "url": "ws://127.0.0.1:1" } }),
        )
        .await,
    )
    .unwrap();
    let err = api.call("load.start", json!({ "id": "x", "test": test_json(&[&ws_path]) })).await.unwrap_err();
    assert!(err.message.contains("only HTTP requests"), "{}", err.message);
}

#[tokio::test]
async fn renaming_or_moving_a_request_updates_load_tests() {
    let (api, _data, _ws) = api().await;
    let path: String = serde_json::from_value(
        ok(
            &api,
            "request.create",
            json!({ "parent": "", "request": { "name": "Get", "method": "GET", "url": "http://127.0.0.1:1/" } }),
        )
        .await,
    )
    .unwrap();
    let mut test = test_json(&[&path]);
    test["thresholds"] = json!([{ "metric": "p95", "op": "<", "value": 100.0, "target": path }]);
    let id: String = serde_json::from_value(ok(&api, "load.create", json!({ "test": test })).await).unwrap();
    let renamed: String =
        serde_json::from_value(ok(&api, "item.rename", json!({ "path": path, "name": "Get users" })).await).unwrap();
    let folder: String =
        serde_json::from_value(ok(&api, "folder.create", json!({ "parent": "", "name": "Users" })).await).unwrap();
    let moved: String = serde_json::from_value(
        ok(&api, "item.move", json!({ "path": renamed, "parent": folder, "index": null })).await,
    )
    .unwrap();
    let test = ok(&api, "load.read", json!({ "id": id })).await;
    assert_eq!(test["targets"][0]["request"], json!(moved));
    assert_eq!(test["thresholds"][0]["target"], json!(moved));
}

#[tokio::test]
async fn following_a_request_keeps_the_load_test_file_name() {
    let (api, _data, ws) = api().await;
    let path: String = serde_json::from_value(
        ok(
            &api,
            "request.create",
            json!({ "parent": "", "request": { "name": "Get", "method": "GET", "url": "http://127.0.0.1:1/" } }),
        )
        .await,
    )
    .unwrap();
    let id: String =
        serde_json::from_value(ok(&api, "load.create", json!({ "test": test_json(&[&path]) })).await).unwrap();
    // A file whose name doesn't follow the test's name (renamed outside the app, from git, …).
    let dir = ws.path().join("loadtests");
    std::fs::rename(dir.join(format!("{id}.yaml")), dir.join("legacy.yaml")).unwrap();
    let renamed: String =
        serde_json::from_value(ok(&api, "item.rename", json!({ "path": path, "name": "Get users" })).await).unwrap();
    let test = ok(&api, "load.read", json!({ "id": "legacy" })).await;
    assert_eq!(test["targets"][0]["request"], json!(renamed));
    assert!(!dir.join(format!("{id}.yaml")).exists());
}

#[tokio::test]
async fn ids_that_name_no_history_folder_are_refused() {
    let (api, _data, _ws) = api().await;
    for id in ["", ".", "..", " . "] {
        let err = api.call("load.runs", json!({ "id": id })).await.unwrap_err();
        assert!(err.message.contains("Invalid load test id"), "{id:?}: {}", err.message);
        let err = api.call("load.deleteRun", json!({ "id": id, "runId": "abc" })).await.unwrap_err();
        assert!(err.message.contains("Invalid load test id"), "{id:?}: {}", err.message);
        let err = api.call("load.start", json!({ "id": id, "test": test_json(&[]) })).await.unwrap_err();
        assert!(err.message.contains("Invalid load test id"), "{id:?}: {}", err.message);
    }
    let err = api.call("load.run", json!({ "id": "Smoke", "runId": "../x" })).await.unwrap_err();
    assert!(err.message.contains("Invalid run id"), "{}", err.message);
}

async fn wait_until_idle(api: &Api) {
    for _ in 0..300 {
        if ok(api, "load.active", json!({})).await.is_null() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("the load test did not end");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_run_saves_its_result_where_its_test_is_when_it_ends() {
    let server = zorvik_testkit::TestServer::start().await;
    let (api, _data, _ws) = api().await;
    let path: String = serde_json::from_value(
        ok(
            &api,
            "request.create",
            json!({ "parent": "", "request": { "name": "Ok", "method": "GET", "url": server.url("/status/200") } }),
        )
        .await,
    )
    .unwrap();
    let mut test = test_json(&[&path]);
    test["stages"] = json!([{ "durationSecs": 0, "target": 1 }, { "durationSecs": 60, "target": 1 }]);
    test["thinkTimeMs"] = json!(20);
    let id: String = serde_json::from_value(ok(&api, "load.create", json!({ "test": test })).await).unwrap();

    // Renamed while it runs: the result goes to the new name's history.
    let run = ok(&api, "load.start", json!({ "id": id, "test": test })).await;
    let run_id = run["runId"].as_str().unwrap().to_string();
    test["name"] = json!("Renamed");
    let new_id: String =
        serde_json::from_value(ok(&api, "load.save", json!({ "id": id, "test": test })).await).unwrap();
    assert_eq!(new_id, "Renamed");
    assert_eq!(ok(&api, "load.active", json!({})).await["testId"], json!(new_id));
    ok(&api, "load.stop", json!({ "runId": run_id })).await;
    wait_until_idle(&api).await;
    let runs = ok(&api, "load.runs", json!({ "id": new_id })).await;
    assert_eq!(runs.as_array().unwrap().len(), 1, "{runs}");
    assert_eq!(runs[0]["runId"], json!(run_id));
    assert_eq!(runs[0]["stoppedEarly"], json!(true));
    assert_eq!(ok(&api, "load.runs", json!({ "id": id })).await, json!([]));
    let summary = ok(&api, "load.run", json!({ "id": new_id, "runId": run_id })).await;
    assert_eq!(summary["targets"][0]["request"], json!(path));

    // Deleted while it runs: its result doesn't bring the history back.
    let run = ok(&api, "load.start", json!({ "id": new_id, "test": test })).await;
    ok(&api, "load.delete", json!({ "id": new_id })).await;
    ok(&api, "load.stop", json!({ "runId": run["runId"] })).await;
    wait_until_idle(&api).await;
    assert_eq!(ok(&api, "load.runs", json!({ "id": new_id })).await, json!([]));
}

//! `mock.*`: mock servers from a folder, an OpenAPI document (text, file, URL)
//! and single responses, driven through the RPC API like the UI does.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use zorvik_api::{Api, EventSink, StreamEvent};
use zorvik_engine::{Client, HttpRequest, RequestOptions};

struct Quiet;

impl EventSink for Quiet {
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
        let api = Api::new(data.path().to_path_buf(), Arc::new(Quiet));
        let h = Self { api, _data: data, ws_dir };
        h.ok("workspace.create", json!({ "path": h.ws_dir.path(), "name": "Mocks" })).await;
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

    /// Start a saved server on a free port (saved that way, so changes to the file
    /// apply to the running copy); returns its run id and URL.
    async fn start(&self, id: &str) -> (String, String) {
        let mut server = self.ok("server.read", json!({ "id": id })).await;
        server["port"] = json!(0);
        assert_eq!(self.ok("server.save", json!({ "id": id, "server": server })).await, json!(id));
        let info = self.ok("server.start", json!({ "id": id, "server": server })).await;
        (info["runId"].as_str().unwrap().to_string(), info["url"].as_str().unwrap().to_string())
    }
}

fn request(name: &str, method: &str, url: &str) -> Value {
    json!({ "name": name, "seq": 0, "method": method, "url": url })
}

async fn get(url: &str) -> (u16, String) {
    let request = HttpRequest { method: "GET".into(), url: url.into(), headers: Vec::new(), body: Default::default() };
    let opts = RequestOptions { timeout: Some(Duration::from_secs(10)), ..Default::default() };
    let r = Client::new().send(request, &opts, None).await.expect("request succeeds");
    (r.meta.status, String::from_utf8_lossy(&r.body).into_owned())
}

#[tokio::test]
async fn mock_from_a_folder_and_the_whole_collection() {
    let h = Harness::new().await;
    let folder = h.ok("folder.create", json!({ "parent": "", "name": "Users" })).await;
    let folder = folder.as_str().unwrap();
    h.ok("request.create", json!({ "parent": folder, "request": request("List", "GET", "{{baseUrl}}/users?page=1") }))
        .await;
    h.ok("request.create", json!({ "parent": folder, "request": request("One", "GET", "{{baseUrl}}/users/{{id}}") }))
        .await;
    let mut socket = request("Live", "GET", "ws://localhost/live");
    socket["kind"] = json!("websocket");
    h.ok("request.create", json!({ "parent": folder, "request": socket })).await;
    h.ok("request.create", json!({ "parent": "", "request": request("Health", "HEAD", "https://api.test/health") }))
        .await;

    let created = h.ok("mock.fromFolder", json!({ "folder": folder, "name": "Users mock" })).await;
    assert_eq!(created["routes"], 2, "{created}");
    let id = created["id"].as_str().unwrap();
    let server = h.ok("server.read", json!({ "id": id })).await;
    assert_eq!(
        (server["name"].as_str(), server["kind"].as_str(), server["port"].as_u64()),
        (Some("Users mock"), Some("http"), Some(3000))
    );
    let paths: Vec<&str> =
        server["http"]["routes"].as_array().unwrap().iter().map(|r| r["path"].as_str().unwrap()).collect();
    assert_eq!(paths, ["/users", "/users/:id"]);

    // The whole collection; the next free port.
    let all = h.ok("mock.fromFolder", json!({ "folder": "", "name": "" })).await;
    assert_eq!(all["routes"], 3);
    let server = h.ok("server.read", json!({ "id": all["id"] })).await;
    assert_eq!((server["name"].as_str(), server["port"].as_u64()), (Some("Mock API"), Some(3001)));

    let (run_id, url) = h.start(id).await;
    assert_eq!(get(&format!("{url}/users/5")).await, (200, "{}".to_string()));
    h.ok("server.stop", json!({ "runId": run_id })).await;

    let err = h.err("mock.fromFolder", json!({ "folder": "Nope", "name": "x" })).await;
    assert_eq!(err.code, "notFound");
    let empty = h.ok("folder.create", json!({ "parent": "", "name": "Empty" })).await;
    let err = h.err("mock.fromFolder", json!({ "folder": empty, "name": "x" })).await;
    assert!(err.message.contains("no HTTP requests"), "{}", err.message);
}

const SPEC: &str = r#"openapi: 3.0.0
info: {title: Pets, version: "1"}
paths:
  /pets/{id}:
    get:
      summary: Get pet
      responses:
        "200":
          description: ok
          content:
            application/json:
              example: {id: 1, name: Rex}
  /pets:
    post:
      responses:
        "201": {description: created}
"#;

#[tokio::test]
async fn mock_from_openapi_text_file_and_url() {
    let h = Harness::new().await;
    let created = h.ok("mock.fromOpenApi", json!({ "text": SPEC, "name": "Pets" })).await;
    assert_eq!((created["routes"].as_u64(), created["warnings"].as_array().map(Vec::len)), (Some(2), Some(0)));
    let id = created["id"].as_str().unwrap().to_string();
    let (run_id, url) = h.start(&id).await;
    let (status, body) = get(&format!("{url}/pets/9")).await;
    assert_eq!(status, 200);
    assert_eq!(serde_json::from_str::<Value>(&body).unwrap(), json!({"id": 1, "name": "Rex"}));

    // From a file.
    let file = h.ws_dir.path().join("spec.yaml");
    std::fs::write(&file, SPEC).unwrap();
    let from_file = h.ok("mock.fromOpenApi", json!({ "path": file, "name": "From file" })).await;
    assert_eq!(from_file["routes"], 2);

    // From a URL: the first mock serves the document itself.
    let route = json!({ "method": "GET", "path": "/openapi.yaml", "body": SPEC, "headers": [{ "key": "Content-Type", "value": "application/yaml" }] });
    h.ok("mock.addRoute", json!({ "serverId": id, "route": route })).await;
    let from_url = h.ok("mock.fromOpenApi", json!({ "url": format!("{url}/openapi.yaml"), "name": "From URL" })).await;
    assert_eq!(from_url["routes"], 2);
    let err = h.err("mock.fromOpenApi", json!({ "url": format!("{url}/missing.yaml"), "name": "x" })).await;
    assert!(err.message.contains("HTTP 404"), "{}", err.message);
    h.ok("server.stop", json!({ "runId": run_id })).await;

    let err = h.err("mock.fromOpenApi", json!({ "text": "not: openapi", "name": "x" })).await;
    assert_eq!(err.code, "parse");
    let err = h.err("mock.fromOpenApi", json!({ "name": "x" })).await;
    assert_eq!(err.code, "invalidInput");
    let err = h.err("mock.fromOpenApi", json!({ "path": h.ws_dir.path().join("nope.yaml"), "name": "x" })).await;
    assert_eq!(err.code, "io");
}

#[tokio::test]
async fn add_route_to_saved_and_running_servers() {
    let h = Harness::new().await;
    let id = h
        .ok(
            "server.create",
            json!({ "server": { "name": "Api", "kind": "http", "seq": 0, "host": "127.0.0.1", "port": 0 } }),
        )
        .await;
    let id = id.as_str().unwrap();
    let (run_id, url) = h.start(id).await;
    assert_eq!(get(&format!("{url}/users/1")).await.0, 404);

    let route = json!({
        "name": "Get user", "method": "GET", "path": "/users/:id", "status": 200,
        "headers": [{ "key": "Content-Type", "value": "application/json" }],
        "body": "{\"id\": \"{{request.params.id}}\"}"
    });
    assert_eq!(h.ok("mock.addRoute", json!({ "serverId": id, "route": route })).await, json!(id));
    let server = h.ok("server.read", json!({ "id": id })).await;
    assert_eq!(server["http"]["routes"][0]["name"], "Get user");
    // The running server answers with it right away.
    assert_eq!(get(&format!("{url}/users/1")).await, (200, "{\"id\": \"1\"}".to_string()));
    h.ok("server.stop", json!({ "runId": run_id })).await;

    let tcp = h
        .ok(
            "server.create",
            json!({ "server": { "name": "Raw", "kind": "tcp", "seq": 0, "host": "127.0.0.1", "port": 0 } }),
        )
        .await;
    let err = h.err("mock.addRoute", json!({ "serverId": tcp, "route": route })).await;
    assert!(err.message.contains("not a mock API"), "{}", err.message);
    let err = h.err("mock.addRoute", json!({ "serverId": "missing", "route": route })).await;
    assert_eq!(err.code, "notFound");
}

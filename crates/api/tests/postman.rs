//! Postman collections imported and run like the app does it: `import.file`, then
//! `runner.start` on the imported folder.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};
use zorvik_api::{Api, EventSink, StreamEvent};

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
    /// A fresh app with a workspace; `globals` are what scripts would have set before.
    async fn new(globals: &[(String, String)]) -> Self {
        let data = tempfile::tempdir().unwrap();
        if !globals.is_empty() {
            let values: serde_json::Map<String, Value> =
                globals.iter().map(|(k, v)| (k.clone(), Value::String(v.clone()))).collect();
            std::fs::write(data.path().join("local-values.json"), json!({ "globals": values }).to_string()).unwrap();
        }
        let ws_dir = tempfile::tempdir().unwrap();
        let events = Arc::new(Events::default());
        let api = Api::new(data.path().to_path_buf(), events.clone());
        let h = Self { api, events, _data: data, ws_dir };
        h.ok("workspace.create", json!({ "path": h.ws_dir.path(), "name": "Postman" })).await;
        h
    }

    async fn ok(&self, method: &str, params: Value) -> Value {
        match self.api.call(method, params).await {
            Ok(v) => v,
            Err(e) => panic!("{method} failed: {} ({})", e.message, e.code),
        }
    }

    async fn import(&self, text: &str) -> Value {
        self.ok("import.file", json!({ "text": text, "parent": "" })).await
    }

    /// Every file of the workspace, concatenated.
    fn files(&self) -> String {
        fn walk(dir: &Path, out: &mut String) {
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

    /// Run a folder and wait for it: (results, summary).
    async fn run(&self, params: Value) -> (Vec<Value>, Value) {
        self.try_run(params).await.unwrap_or_else(|e| panic!("runner.start failed: {e}"))
    }

    async fn try_run(&self, params: Value) -> Result<(Vec<Value>, Value), String> {
        let started = self.api.call("runner.start", params).await.map_err(|e| e.message)?;
        let run_id = started["runId"].as_str().unwrap().to_string();
        for _ in 0..2400 {
            let events = self.events.0.lock().unwrap().clone();
            let mine = |kind: &str| -> Vec<Value> {
                events.iter().filter(|e| e["runId"] == run_id.as_str() && e["event"]["type"] == kind).cloned().collect()
            };
            if let Some(finished) = mine("finished").into_iter().next() {
                let results = mine("result").into_iter().map(|e| e["event"]["result"].clone()).collect();
                return Ok((results, finished["event"]["summary"].clone()));
            }
            drop(events);
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("the run did not finish");
    }
}

/// A collection like the ones Postman exports, with what Postman does at run time:
/// collection and folder auth and scripts, the legacy API, SugarJS, values that keep their
/// type from one request to the next, form bodies changed by scripts, path variables, bodies
/// Postman doesn't send and stale auth headers it replaces.
const SHOP: &str = r#"{
  "info": {"name": "Shop", "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json"},
  "auth": {"type": "basic", "basic": [{"key": "username", "value": "{{user}}"}, {"key": "password", "value": "pw"}]},
  "variable": [{"key": "user", "value": "ann"}, {"key": "signingKey", "value": "s3cret-collection", "type": "secret"}],
  "event": [{"listen": "prerequest", "script": {"exec": ["pm.variables.set('order', 'collection');"]}}],
  "item": [
    {"name": "Accounts",
     "event": [{"listen": "prerequest", "script": {"exec": "pm.variables.set('order', pm.variables.get('order') + ' > folder');"}}],
     "item": [
      {"name": "Login",
       "request": {"method": "GET", "url": "{{base}}/basic-auth/ann/pw",
                   "header": [{"key": "Authorization", "value": "Basic c3RhbGU6c3RhbGU="}]},
       "event": [{"listen": "test", "script": {"exec": [
         "tests['authenticated'] = JSON.parse(responseBody).authenticated === true;",
         "tests['sugar'] = responseBody.has('authenticated');",
         "pm.collectionVariables.set('ids', [3, 1, 2]);",
         "pm.test('script order', () => pm.expect(pm.variables.get('order')).to.equal('collection > folder'));",
         "pm.test('location', () => pm.expect(pm.execution.location).to.eql(['Shop', 'Accounts', 'Login']));"
       ]}}]},
      {"name": "Profile",
       "request": {"method": "POST", "url": "{{base}}/echo", "auth": {"type": "noauth"},
                   "body": {"mode": "urlencoded", "urlencoded": [
                     {"key": "name", "value": "{{user}}"}, {"key": "off", "value": "x", "disabled": true}]}},
       "event": [
         {"listen": "prerequest", "script": {"exec": ["pm.request.body.urlencoded.add({ key: 'sig', value: CryptoJS.MD5('ann').toString() });"]}},
         {"listen": "test", "script": {"exec": [
           "const ids = pm.collectionVariables.get('ids');",
           "pm.test('typed', () => pm.expect(_.sortBy(ids)).to.eql([1, 2, 3]));",
           "pm.test('form sent', () => pm.expect(pm.response.json().body).to.equal('name=ann&sig=' + CryptoJS.MD5('ann')));",
           "pm.test('request', () => {",
           "  pm.request.to.have.header('Host');",
           "  pm.expect(pm.request.body.urlencoded.get('name')).to.equal('ann');",
           "});"
         ]}}]}
    ]},
    {"name": "Item",
     "request": {"method": "GET", "auth": {"type": "noauth"},
                 "url": {"raw": "{{base}}/anything/items/:id", "host": ["{{base}}"], "path": ["anything", "items", ":id"],
                         "variable": [{"key": "id", "value": "{{itemId}}"}]},
                 "body": {"mode": "raw", "raw": "{\"ignored\": true}", "options": {"raw": {"language": "json"}}}},
     "event": [{"listen": "test", "script": {"exec": [
       "pm.test('path', () => pm.expect(pm.response.json().path).to.equal('/anything/items/42'));",
       "pm.test('no body', () => pm.response.to.have.jsonBody('bodyLength', 0));",
       "pm.test('url variables', () => pm.expect(pm.request.url.variables.get('id')).to.equal('42'));"
     ]}}]},
    {"name": "Never",
     "request": {"method": "GET", "url": "{{base}}/status/500", "auth": {"type": "noauth"}},
     "event": [{"listen": "prerequest", "script": {"exec": ["pm.execution.skipRequest();"]}}]}
  ]
}"#;

#[tokio::test(flavor = "multi_thread")]
async fn a_postman_collection_runs_as_in_postman() {
    let server = zorvik_testkit::TestServer::start().await;
    let h = Harness::new(&[]).await;
    let summary = h.import(SHOP).await;
    assert_eq!(summary["requests"], 4);
    let warnings: Vec<&str> = summary["warnings"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    assert_eq!(warnings.len(), 2, "{warnings:?}");
    assert!(warnings[0].starts_with("1 request had its own auth header as well as an auth setting"), "{}", warnings[0]);
    assert!(warnings[1].starts_with("1 GET, HEAD, COPY, PURGE or UNLOCK request had a body"), "{}", warnings[1]);

    let environment = json!({
        "name": "Local",
        "values": [
            {"key": "base", "value": server.url(""), "enabled": true},
            {"key": "itemId", "value": "42"},
            {"key": "apiKey", "value": "s3cret-environment", "type": "secret"},
            {"key": "old", "value": "1", "disabled": true}
        ],
        "_postman_variable_scope": "environment"
    });
    h.import(&environment.to_string()).await;
    let envs = h.ok("env.list", json!({})).await;
    h.ok("env.setActive", json!({ "id": envs[0]["id"] })).await;
    // Secret values are kept on this computer only, never in the workspace's files.
    let files = h.files();
    assert!(!files.contains("s3cret-environment") && !files.contains("s3cret-collection"), "{files}");

    let (results, run) = h.run(json!({ "folder": summary["folderPath"] })).await;
    let report: Vec<(String, bool, bool)> = results
        .iter()
        .map(|r| (r["name"].as_str().unwrap().to_string(), r["passed"] == true, r["skipped"] == true))
        .collect();
    let failures: Vec<&Value> =
        results.iter().flat_map(|r| r["tests"].as_array().unwrap()).filter(|t| t["passed"] != true).collect();
    assert!(failures.is_empty(), "{failures:#?}\n{results:#?}");
    assert_eq!(
        report,
        [
            ("Login".to_string(), true, false),
            ("Profile".to_string(), true, false),
            ("Item".to_string(), true, false),
            ("Never".to_string(), true, true),
        ]
    );
    assert_eq!(run["testsPassed"], 10);
}

#[tokio::test(flavor = "multi_thread")]
async fn postman_files_of_every_kind_are_recognized() {
    let h = Harness::new(&[]).await;
    // Collection v1, as Postman exported it before 2017.
    let v1 = json!({
        "id": "c1", "name": "Old API", "order": ["r1"], "folders": [],
        "requests": [{"id": "r1", "name": "Ping", "url": "https://x.test/ping", "method": "GET",
                      "tests": "tests['ok'] = responseCode.code === 200;"}]
    });
    let summary = h.import(&v1.to_string()).await;
    assert_eq!((summary["name"].as_str(), summary["requests"].as_u64()), (Some("Old API"), Some(1)));
    // The Postman API's wrapper, and a collection without `info` (newman runs those).
    let wrapped =
        json!({"collection": {"info": {"name": "From the API"}, "item": [{"name": "A", "request": "https://x.test"}]}});
    assert_eq!(h.import(&wrapped.to_string()).await["requests"], 1);
    let bare = json!({"name": "Bare", "item": [{"name": "B", "request": {"url": "https://x.test", "method": "GET"}}]});
    assert_eq!(h.import(&bare.to_string()).await["requests"], 1);
    // A globals export without a name, and an environment that is only values.
    let globals =
        json!({"values": [{"key": "g", "value": "1", "enabled": true}], "_postman_variable_scope": "globals"});
    assert_eq!(h.import(&globals.to_string()).await["environments"], 1);
    let values = json!({"values": [{"key": "e", "value": "2"}]});
    assert_eq!(h.import(&values.to_string()).await["environments"], 1);
    let names: Vec<String> = h
        .ok("env.list", json!({}))
        .await
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["environment"]["name"].as_str().or(e["name"].as_str()).unwrap_or_default().to_string())
        .collect();
    assert!(names.contains(&"Globals".to_string()), "{names:?}");
}

// ---- Postman's own collections (not in the repository) ----------------------------------------

/// Files of one newman-style suite: `name.postman_collection.json`, `…_environment.json`, …
#[derive(Default, Debug)]
struct Suite {
    collection: Option<PathBuf>,
    environment: Option<PathBuf>,
    globals: Option<PathBuf>,
    data: Option<PathBuf>,
}

fn suites(dir: &Path) -> Vec<(String, Suite)> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else {
                out.push(path);
            }
        }
    }
    let mut files = Vec::new();
    walk(dir, &mut files);
    files.sort();
    let mut suites: std::collections::BTreeMap<String, Suite> = Default::default();
    for file in files {
        let name = file.file_name().unwrap().to_string_lossy().to_string();
        let Some((base, rest)) = name.split_once(".postman_") else {
            if name.ends_with(".json") {
                let key = file.with_extension("").to_string_lossy().to_string();
                suites.entry(key).or_default().collection = Some(file.clone());
            }
            continue;
        };
        let key = file.with_file_name(base).to_string_lossy().to_string();
        let suite = suites.entry(key).or_default();
        match rest {
            "collection.json" => suite.collection = Some(file.clone()),
            "environment.json" => suite.environment = Some(file.clone()),
            "globals.json" => suite.globals = Some(file.clone()),
            "data.json" | "data.csv" => suite.data = Some(file.clone()),
            _ => {}
        }
    }
    suites.into_iter().filter(|(_, s)| s.collection.is_some()).collect()
}

/// Imports and runs every Postman collection under `ZORVIK_POSTMAN_DIR` (for example
/// newman's `test/integration`) and prints each failed request and test, so they can be
/// compared with what the collections expect. Not part of the normal test run: the
/// collections are not in the repository and most of them call postman-echo.com.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn postman_collections_from_a_folder() {
    let Some(dir) = std::env::var_os("ZORVIK_POSTMAN_DIR") else {
        panic!("set ZORVIK_POSTMAN_DIR to a folder of Postman collections");
    };
    let only = std::env::var("ZORVIK_POSTMAN_ONLY").ok();
    let (mut total, mut clean) = (0, 0);
    for (name, suite) in suites(Path::new(&dir)) {
        if only.as_deref().is_some_and(|o| !name.contains(o)) {
            continue;
        }
        total += 1;
        let read = |p: &Path| String::from_utf8_lossy(&std::fs::read(p).unwrap()).into_owned();
        let globals: Vec<(String, String)> = suite
            .globals
            .as_deref()
            .map(|p| serde_json::from_str::<Value>(read(p).trim_start_matches('\u{feff}')).unwrap())
            .and_then(|g| g["values"].as_array().cloned())
            .unwrap_or_default()
            .iter()
            .filter(|v| v["enabled"] != false)
            .map(|v| {
                (
                    v["key"].as_str().unwrap_or_default().to_string(),
                    v["value"].to_string().trim_matches('"').to_string(),
                )
            })
            .collect();
        let h = Harness::new(&globals).await;
        let summary = match h
            .api
            .call("import.file", json!({ "text": read(suite.collection.as_ref().unwrap()), "parent": "" }))
            .await
        {
            Ok(s) => s,
            Err(e) => {
                println!("\n== {name}\n   IMPORT FAILED: {}", e.message);
                continue;
            }
        };
        if let Some(env) = &suite.environment {
            match h.api.call("import.file", json!({ "text": read(env), "parent": "" })).await {
                Ok(_) => {
                    let envs = h.ok("env.list", json!({})).await;
                    let id = envs[0]["id"].clone();
                    h.ok("env.setActive", json!({ "id": id })).await;
                }
                Err(e) => println!("\n== {name}\n   ENVIRONMENT IMPORT FAILED: {}", e.message),
            }
        }
        let mut params = json!({ "folder": summary["folderPath"].as_str().unwrap_or_default() });
        if let Some(data) = &suite.data {
            let target = h.ws_dir.path().join(data.file_name().unwrap());
            std::fs::copy(data, &target).unwrap();
            params["dataFile"] = json!(data.file_name().unwrap().to_string_lossy());
        }
        let (results, run) = match h.try_run(params).await {
            Ok(done) => done,
            Err(e) => {
                println!("\n== {name}\n   RUN FAILED: {e}");
                continue;
            }
        };
        let warnings = summary["warnings"].as_array().cloned().unwrap_or_default();
        let failed: Vec<&Value> = results.iter().filter(|r| r["passed"] != true).collect();
        if failed.is_empty() && warnings.is_empty() && run["error"].is_null() {
            clean += 1;
            println!("ok {name} ({} requests, {} tests)", results.len(), run["testsPassed"]);
            continue;
        }
        println!("\n== {name}: {} of {} requests failed", failed.len(), results.len());
        for w in &warnings {
            println!("   import warning: {}", w.as_str().unwrap_or_default());
        }
        if !run["error"].is_null() {
            println!("   run error: {}", run["error"]);
        }
        for r in failed {
            println!(
                "   [{}] {} -> {} {}",
                r["iteration"],
                r["name"].as_str().unwrap_or_default(),
                r["status"],
                r["error"].as_str().unwrap_or_default()
            );
            for e in r["scriptErrors"].as_array().into_iter().flatten() {
                println!("      script error: {}", e.as_str().unwrap_or_default());
            }
            for t in r["tests"].as_array().into_iter().flatten().filter(|t| t["passed"] != true && t["skipped"] != true)
            {
                println!(
                    "      FAIL {}: {}",
                    t["name"].as_str().unwrap_or_default(),
                    t["error"].as_str().unwrap_or_default()
                );
            }
            for c in
                r["console"].as_array().into_iter().flatten().filter(|c| c["level"] == "warn" || c["level"] == "error")
            {
                println!(
                    "      console {}: {}",
                    c["level"].as_str().unwrap_or_default(),
                    c["message"].as_str().unwrap_or_default()
                );
            }
        }
    }
    println!("\n{clean} of {total} collections imported and ran without warnings or failures");
}

/// Imports every JSON file under `ZORVIK_POSTMAN_DIR` with the Postman importer only (no
/// run) and prints errors, warnings and what each request became.
#[test]
#[ignore]
fn postman_imports_from_a_folder() {
    use zorvik_workspace::formats::ImportedItem;
    use zorvik_workspace::formats::postman::{import_postman_collection, import_postman_environment};
    let Some(dir) = std::env::var_os("ZORVIK_POSTMAN_DIR") else {
        panic!("set ZORVIK_POSTMAN_DIR to a folder of Postman collections");
    };
    fn walk(items: &[ImportedItem], depth: usize) {
        for item in items {
            match item {
                ImportedItem::Folder { meta, children } => {
                    println!("{}[{}] auth={:?}", "  ".repeat(depth), meta.name, std::mem::discriminant(&meta.auth));
                    walk(children, depth + 1);
                }
                ImportedItem::Request(r) => println!(
                    "{}{} {} {} body={:?} auth={} pre={} post={}",
                    "  ".repeat(depth),
                    r.method,
                    r.name,
                    r.url,
                    r.body.body_type,
                    serde_json::to_value(&r.auth).map(|a| a["type"].to_string()).unwrap_or_default(),
                    r.scripts.pre_request.len(),
                    r.scripts.post_response.len()
                ),
            }
        }
    }
    for (name, suite) in suites(Path::new(&dir)) {
        let text = String::from_utf8_lossy(&std::fs::read(suite.collection.as_ref().unwrap()).unwrap()).into_owned();
        println!("\n== {name}");
        match import_postman_collection(&text) {
            Ok(c) => {
                for w in &c.warnings {
                    println!("   warning: {w}");
                }
                println!("   {} requests, {} workspace variables", c.request_count(), c.workspace_variables.len());
                if std::env::var_os("ZORVIK_POSTMAN_TREE").is_some() {
                    walk(&c.items, 2);
                }
            }
            Err(e) => match import_postman_environment(&text) {
                Ok(env) => println!("   environment '{}' with {} variables", env.name, env.variables.len()),
                Err(_) => println!("   ERROR: {e}"),
            },
        }
    }
}

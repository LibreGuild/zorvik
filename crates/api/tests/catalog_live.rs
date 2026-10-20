//! Download every API in the catalog from its publisher and import it, the way a user does.
//! Needs the internet, so it only runs when asked (and weekly in CI, see docs/ci-release.md):
//!
//! ```text
//! cargo test -p zorvik-api --test catalog_live -- --ignored --nocapture
//! CATALOG_ONLY=stripe,openai cargo test -p zorvik-api --test catalog_live -- --ignored --nocapture
//! ```
//!
//! `CATALOG_WORKSPACE=<empty folder>` imports into that folder and keeps it, to look at the
//! requests afterwards.

use std::sync::Arc;

use serde_json::{Value, json};
use zorvik_api::{Api, EventSink, StreamEvent};

struct NoEvents;

impl EventSink for NoEvents {
    fn emit(&self, _: StreamEvent) {}
}

/// Above this many operations, an API without preset groups is imported in part (its first
/// groups), to keep the run short.
const MOST_OPERATIONS: u64 = 1500;

#[tokio::test(flavor = "multi_thread")]
#[ignore = "downloads every API in the catalog"]
async fn every_catalog_api_imports() {
    let dir = tempfile::tempdir().unwrap();
    let api = Api::new(dir.path().join("data"), Arc::new(NoEvents));
    let ws = std::env::var_os("CATALOG_WORKSPACE").map_or_else(|| dir.path().join("ws"), Into::into);
    api.call("workspace.create", json!({ "path": ws, "name": "Catalog" })).await.unwrap();
    let only: Vec<String> = std::env::var("CATALOG_ONLY")
        .map(|v| v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
        .unwrap_or_default();

    let entries = api.call("catalog.list", json!({})).await.unwrap();
    let mut failures = Vec::new();
    let mut checked = 0;
    for entry in entries.as_array().unwrap() {
        let id = entry["id"].as_str().unwrap();
        if entry["source"]["type"] == "link" || (!only.is_empty() && !only.iter().any(|o| o == id)) {
            continue;
        }
        checked += 1;
        let preview = match api.call("catalog.preview", json!({ "id": id })).await {
            Ok(p) => p,
            Err(e) => {
                println!("FAIL {id}: preview: {}", e.message);
                failures.push(format!("{id}: {}", e.message));
                continue;
            }
        };
        let groups = preview["groups"].as_array().cloned().unwrap_or_default();
        let selected = preview["selected"].as_array().cloned().unwrap_or_default();
        // Preset groups the description no longer has are left out (all of it, when none are
        // left): the publisher renamed its tags, so the catalog needs the new names.
        let preset = entry["source"]["groups"].as_array().cloned().unwrap_or_default();
        let gone: Vec<&Value> = preset.iter().filter(|g| !selected.contains(g)).collect();
        if !gone.is_empty() {
            println!("FAIL {id}: preset groups no longer in the description: {gone:?}");
            failures.push(format!("{id}: preset groups no longer in the description: {gone:?}"));
        }
        let picked: Option<Vec<Value>> =
            if selected.is_empty() && preview["operations"].as_u64().unwrap_or(0) > MOST_OPERATIONS && groups.len() > 1
            {
                Some(groups.iter().take(10).map(|g| g["tag"].clone()).collect())
            } else {
                None
            };
        let params = match &picked {
            Some(groups) => json!({ "id": id, "groups": groups }),
            None => json!({ "id": id }),
        };
        match api.call("catalog.import", params).await {
            Ok(imported) => {
                let summary = &imported["summary"];
                let requests = summary["requests"].as_u64().unwrap_or(0);
                let warnings = summary["warnings"].as_array().map_or(0, Vec::len);
                let mut problems = Vec::new();
                if requests == 0 {
                    problems.push("no requests".to_string());
                }
                if entry.get("first").is_some_and(|f| f.is_string()) && imported.get("open").is_none() {
                    problems.push(format!("no request {}", entry["first"]));
                }
                println!(
                    "{} {id}: {requests} requests of {} operations, {warnings} warnings{}",
                    if problems.is_empty() { "ok  " } else { "FAIL" },
                    preview["operations"],
                    if picked.is_some() { " (part)" } else { "" }
                );
                if !problems.is_empty() {
                    failures.push(format!("{id}: {}", problems.join(", ")));
                }
            }
            Err(e) => {
                println!("FAIL {id}: {}", e.message);
                failures.push(format!("{id}: {}", e.message));
            }
        }
    }
    println!("{checked} APIs checked, {} failed", failures.len());
    assert!(failures.is_empty(), "catalog APIs that didn't import:\n{}", failures.join("\n"));
}

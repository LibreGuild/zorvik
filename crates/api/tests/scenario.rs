//! Scenarios through `scenario.*`, like the UI calls them: values from step to step,
//! checks, flow (if, loops, at the same time, other scenarios), failures and clean-up,
//! running a part, asking for a value, and that nothing leaves the scenario.

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
    server: TestServer,
}

impl Harness {
    /// A workspace with an environment (`base`: the test server; `key`: a secret) and the
    /// requests the scenarios use.
    async fn new() -> Self {
        let server = TestServer::start().await;
        let data = tempfile::tempdir().unwrap();
        let ws_dir = tempfile::tempdir().unwrap();
        let events = Arc::new(Events::default());
        let api = Api::new(data.path().to_path_buf(), events.clone());
        let h = Self { api, events, _data: data, ws_dir, server };
        h.ok("workspace.create", json!({ "path": h.ws_dir.path(), "name": "Shop" })).await;
        let variables = json!([{ "key": "base", "value": h.server.url("") }, { "key": "key", "value": "top-secret", "secret": true }]);
        let id = h.ok("env.create", json!({ "environment": { "name": "Local", "variables": variables } })).await;
        h.ok("env.setActive", json!({ "id": id })).await;
        h.request("Create", "{{base}}/echo?id=7&name=Ada", "").await;
        h.request("Use", "{{base}}/echo?cart={{cartId}}&ref={{ref}}", "").await;
        h.request("Item", "{{base}}/echo?item={{item}}&at={{index}}", "").await;
        h.request("List", "{{base}}/big-json?n=3", "").await;
        h.request("Broken", "{{base}}/status/500", "").await;
        h
    }

    async fn ok(&self, method: &str, params: Value) -> Value {
        match self.api.call(method, params).await {
            Ok(v) => v,
            Err(e) => panic!("{method} failed: {} ({})", e.message, e.code),
        }
    }

    async fn request(&self, name: &str, url: &str, post: &str) -> String {
        let request = json!({ "name": name, "method": "GET", "url": url, "scripts": { "postResponse": post } });
        self.ok("request.create", json!({ "parent": "", "request": request })).await.as_str().unwrap().to_string()
    }

    async fn scenario(&self, scenario: Value) -> String {
        self.ok("scenario.create", json!({ "scenario": scenario })).await.as_str().unwrap().to_string()
    }

    /// Run and wait for the end: (results, summary).
    async fn run(&self, params: Value) -> (Vec<Value>, Value) {
        let started = self.ok("scenario.run", params).await;
        let run_id = started["runId"].as_str().unwrap().to_string();
        let summary = self.finished(&run_id).await;
        (self.results(&run_id), summary)
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
        let events = self.events.0.lock().unwrap();
        events.iter().find(|e| e["runId"] == run_id && e["event"]["type"] == kind).map(|e| e["event"].clone())
    }

    fn results(&self, run_id: &str) -> Vec<Value> {
        let events = self.events.0.lock().unwrap();
        events
            .iter()
            .filter(|e| e["runId"] == run_id && e["event"]["type"] == "result")
            .map(|e| e["event"]["result"].clone())
            .collect()
    }

    /// The values the last run left, as `name=value`.
    async fn values(&self, id: &str) -> Vec<String> {
        let values = self.ok("scenario.values", json!({ "id": id })).await;
        values
            .as_array()
            .unwrap()
            .iter()
            .map(|v| format!("{}={}", v["key"].as_str().unwrap(), v["value"].as_str().unwrap()))
            .collect()
    }
}

/// `name: note` (or just the name) of each result, indented by its depth, failed ones with `!`.
fn lines(results: &[Value]) -> Vec<String> {
    results
        .iter()
        .map(|r| {
            let step = &r["step"];
            let indent = "  ".repeat(step["depth"].as_u64().unwrap_or(0) as usize);
            let failed = if r["passed"] == true { "" } else { "!" };
            match step["note"].as_str() {
                Some(note) => format!("{indent}{failed}{}: {note}", r["name"].as_str().unwrap()),
                None => format!("{indent}{failed}{}", r["name"].as_str().unwrap()),
            }
        })
        .collect()
}

fn args(result: &Value) -> String {
    result["url"].as_str().unwrap().split_once('?').map(|(_, q)| q.to_string()).unwrap_or_default()
}

#[tokio::test]
async fn values_go_from_step_to_step_and_stay_in_the_scenario() {
    let h = Harness::new().await;
    // A script that sets an environment value: it must stay inside the scenario too.
    h.request("Login", "{{base}}/echo?login=1", "pm.environment.set('token', 'tok-1'); pm.globals.set('g', 5);").await;
    h.request("Me", "{{base}}/echo?t={{token}}&g={{g}}", "").await;
    let id = h
        .scenario(json!({
            "name": "Checkout",
            "docs": "Creates a cart and uses it.",
            "variables": [{ "key": "productId", "value": "42" }, { "key": "label", "value": "p-{{productId}}" }],
            "steps": [
                { "type": "request", "request": "Create.yaml",
                  "outputs": [
                      { "variable": "cartId", "path": "$.args.id" },
                      { "variable": "whole", "path": "$.args" },
                      { "variable": "ct", "from": "header", "path": "content-type" },
                      { "variable": "code", "from": "status" },
                      { "variable": "missing", "path": "$.nope", "optional": true },
                  ],
                  "checks": [
                      { "from": "status", "value": "200" },
                      { "path": "$.args.name", "value": "Ada" },
                      { "path": "$.args.name", "op": "matches", "value": "^A" },
                      { "from": "header", "path": "Content-Type", "op": "contains", "value": "json" },
                      { "from": "body", "op": "contains", "value": "Ada" },
                      { "from": "time", "op": "less", "value": "60000" },
                      { "path": "$.nope", "op": "notExists" },
                  ] },
                { "type": "request", "name": "Use the cart", "request": "Use.yaml",
                  "inputs": [{ "key": "ref", "value": "{{cartId}}-{{productId}}-{{whole.name}}" }],
                  "checks": [{ "path": "$.args.ref", "value": "7-42-Ada" }, { "path": "$.args.cart", "value": "{{cartId}}" }] },
                { "type": "set", "values": [{ "key": "total", "value": "= Number(cartId) * 20 + productId.length" }, { "key": "list", "value": "= [1, 2].concat(vars['productId'])" }] },
                { "type": "if",
                  "when": { "rules": [{ "value": "{{total}}", "op": "greater", "to": "1000" }] },
                  "steps": [{ "type": "stop", "what": "fail", "message": "too much" }],
                  "elseIf": [{ "when": { "any": true, "rules": [{ "value": "{{total}}", "op": "equals", "to": "0" }, { "value": "= total === 142", "op": "isTrue" }] },
                               "steps": [{ "type": "request", "request": "Use.yaml", "inputs": [{ "key": "ref", "value": "= 'sum-' + total" }] }] }],
                  "else": [{ "type": "stop", "what": "fail", "message": "not reached" }] },
                { "type": "request", "request": "Login.yaml" },
                { "type": "request", "request": "Me.yaml", "checks": [{ "path": "$.args.t", "value": "tok-1" }, { "path": "$.args.g", "value": "5" }] },
            ],
        }))
        .await;
    assert_eq!(
        h.ok("scenario.list", json!({})).await,
        json!([{ "id": "Checkout", "name": "Checkout", "steps": 9, "seq": 0 }])
    );
    assert_eq!(h.ok("scenario.read", json!({ "id": id })).await["docs"], "Creates a cart and uses it.");

    let (results, summary) = h.run(json!({ "id": id })).await;
    assert_eq!(
        lines(&results),
        [
            "Create",
            "Use the cart",
            "Set total, list",
            "If {{total}} is greater than 1000: else if {{total}} equals 0 or = total === 142 is true",
            "  Use",
            "Login",
            "Me",
        ],
        "{results:#?}"
    );
    assert_eq!(summary["passed"], true, "{results:#?}");
    // Six steps did something; the `if` only directs.
    assert_eq!((summary["requests"].clone(), summary["failed"].clone()), (json!(6), json!(0)));
    assert_eq!((summary["testsPassed"].clone(), summary["testsFailed"].clone()), (json!(11), json!(0)));
    let saved = &results[0]["step"]["saved"];
    assert_eq!(saved[0], json!({ "key": "cartId", "value": "7" }));
    assert_eq!(saved[3], json!({ "key": "code", "value": "200" }));
    assert_eq!(results[0]["step"]["path"], "steps/0");
    assert_eq!(results[4]["step"]["path"], "steps/3/elseIf/0/0");
    assert_eq!(
        results[0]["tests"][0],
        json!({ "name": "Status equals 200", "passed": true, "skipped": false, "error": null })
    );
    assert!(args(&results[1]).ends_with("ref=7-42-Ada"), "{}", results[1]["url"]);
    assert_eq!(args(&results[4]), "cart=7&ref=sum-142");
    assert_eq!(
        results[2]["step"]["saved"],
        json!([{ "key": "total", "value": "142" }, { "key": "list", "value": "[1,2,\"42\"]" }])
    );

    // The scenario keeps its values; the step input `ref` was only for its step.
    let values = h.values(&id).await;
    for expected in
        ["cartId=7", "code=200", "g=5", "label=p-42", "list=[1,2,\"42\"]", "productId=42", "token=tok-1", "total=142"]
    {
        assert!(values.contains(&expected.to_string()), "{expected} not in {values:?}");
    }
    assert!(!values.iter().any(|v| v.starts_with("ref=") || v.starts_with("missing=")), "{values:?}");
    let kinds = h.ok("scenario.values", json!({ "id": id })).await;
    let kind = |key: &str| kinds.as_array().unwrap().iter().find(|v| v["key"] == key).unwrap()["kind"].clone();
    assert_eq!(
        (kind("total"), kind("list"), kind("whole"), kind("cartId")),
        (json!("number"), json!("list"), json!("object"), json!("text"))
    );

    // Nothing left the scenario: no value was kept for the environment or globals.
    assert_eq!(h.ok("vars.local", json!({})).await, json!([]));
    let me = h.ok("http.send", json!({ "requestId": "x", "path": "Me.yaml", "request": h.ok("request.read", json!({ "path": "Me.yaml" })).await })).await;
    assert_eq!(me["unresolved"], json!(["g", "token"]));

    h.ok("scenario.clearValues", json!({ "id": id, "key": "total" })).await;
    assert!(!h.values(&id).await.iter().any(|v| v.starts_with("total=")));
    h.ok("scenario.clearValues", json!({ "id": id })).await;
    assert!(h.values(&id).await.is_empty());
}

#[tokio::test]
async fn loops_collect_values_and_run_at_the_same_time() {
    let h = Harness::new().await;
    let id = h
        .scenario(json!({
            "name": "Loops",
            "variables": [{ "key": "item", "value": "outside" }],
            "steps": [
                { "type": "request", "request": "List.yaml", "outputs": [{ "variable": "names", "path": "$.items[*].name" }, { "variable": "items", "path": "$.items" }] },
                { "type": "forEach", "list": "{{names}}", "steps": [
                    { "type": "request", "request": "Item.yaml", "outputs": [{ "variable": "seen", "path": "$.args.item", "collect": true }] },
                    { "type": "if", "when": { "rules": [{ "value": "{{index}}", "op": "equals", "to": "1" }] }, "steps": [{ "type": "stop", "what": "loop", "message": "enough" }] },
                ] },
                { "type": "forEach", "list": "{{items}}", "as": "row", "steps": [
                    { "type": "set", "values": [{ "key": "last", "value": "{{row.name}}/{{row.index}}/{{items.length}}" }] },
                ] },
                { "type": "repeat", "times": 5, "until": { "rules": [{ "value": "{{count}}", "op": "greaterOrEqual", "to": "3" }] }, "steps": [
                    { "type": "set", "values": [{ "key": "count", "value": "= (Number(vars.count) || 0) + 1" }] },
                ] },
                { "type": "parallel", "steps": [
                    { "type": "request", "name": "First", "request": "Create.yaml", "outputs": [{ "variable": "a", "path": "$.args.id" }, { "variable": "both", "path": "$.args.name" }] },
                    { "type": "request", "name": "Second", "request": "Create.yaml", "outputs": [{ "variable": "b", "path": "$.args.name" }, { "variable": "both", "path": "$.args.id" }] },
                ] },
                { "type": "forEach", "list": "a, b", "steps": [] },
            ],
        }))
        .await;
    let (results, summary) = h.run(json!({ "id": id })).await;
    assert_eq!(
        lines(&results),
        [
            "List",
            "For each item in {{names}}: 3 items",
            "  item = item-0: 1 of 3",
            "  Item",
            "  If {{index}} equals 1: no",
            "  item = item-1: 2 of 3",
            "  Item",
            "  If {{index}} equals 1: yes",
            "    Leave the loop: enough",
            "For each row in {{items}}: 3 items",
            "  row = {\"index\":0,\"name\":\"item-0\",\"active\":true}: 1 of 3",
            "  Set last",
            "  row = {\"index\":1,\"name\":\"item-1\",\"active\":false}: 2 of 3",
            "  Set last",
            "  row = {\"index\":2,\"name\":\"item-2\",\"active\":true}: 3 of 3",
            "  Set last",
            "Repeat until {{count}} is at least 3: up to 5 rounds",
            "  Round 1: 1 of 5",
            "  Set count",
            "  Round 2: 2 of 5",
            "  Set count",
            "  Round 3: 3 of 5",
            "  Set count",
            "At the same time: 2 steps",
            "  First",
            "  Second",
            "For each item in a, b: 2 items",
            "  item = a: 1 of 2",
            "  item = b: 2 of 2",
        ],
        "{results:#?}"
    );
    assert_eq!(summary["passed"], true);
    assert_eq!(args(&results[3]), "item=item-0&at=0");
    assert_eq!(args(&results[6]), "item=item-1&at=1");
    let values = h.values(&id).await;
    // The loop's names are the scenario's again after it; what steps saved stays.
    for expected in
        ["item=outside", "seen=[\"item-0\",\"item-1\"]", "last=item-2/2/3", "count=3", "a=7", "b=Ada", "both=7"]
    {
        assert!(values.contains(&expected.to_string()), "{expected} not in {values:?}");
    }
    assert!(!values.iter().any(|v| v.starts_with("index=") || v.starts_with("row=")), "{values:?}");

    // A repeat whose condition never holds fails; a loop that never ends is stopped.
    let never = h
        .scenario(json!({ "name": "Never", "steps": [
            { "type": "repeat", "times": 2, "until": { "rules": [{ "value": "{{x}}", "op": "exists" }] }, "steps": [{ "type": "wait", "ms": 1 }] },
            { "type": "request", "request": "Create.yaml" },
        ] }))
        .await;
    let (results, summary) = h.run(json!({ "id": never })).await;
    assert_eq!(lines(&results).last().unwrap(), "!Repeat until {{x}} exists");
    assert_eq!(results.last().unwrap()["error"], "{{x}} exists didn't hold after 2 rounds");
    assert_eq!(
        (summary["passed"].clone(), summary["failed"].clone(), summary["bailed"].clone()),
        (json!(false), json!(1), json!(true))
    );

    let endless = h
        .scenario(json!({ "name": "Endless", "steps": [{ "type": "repeat", "times": 9999, "steps": [{ "type": "repeat", "times": 9999, "steps": [] }] }] }))
        .await;
    let (_, summary) = h.run(json!({ "id": endless })).await;
    assert!(summary["error"].as_str().unwrap().contains("more than 10000 steps"), "{summary}");
    assert_eq!(summary["passed"], false);

    // Steps at the same time are requests in flight together: there is a most.
    let wait = json!({ "type": "wait", "ms": 1 });
    let many = h.scenario(json!({ "name": "Many", "steps": [{ "type": "parallel", "steps": vec![wait; 101] }] })).await;
    let (results, summary) = h.run(json!({ "id": many })).await;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["error"], "At most 100 steps can run at the same time (this has 101)");
    assert_eq!(summary["passed"], false);
}

#[tokio::test]
async fn failures_end_the_run_unless_told_to_go_on_and_clean_up_always_runs() {
    let h = Harness::new().await;
    let steps = json!([
        { "type": "request", "request": "Create.yaml", "checks": [{ "path": "$.args.id", "value": "8" }], "continueOnFailure": true },
        { "type": "request", "request": "Broken.yaml", "retry": { "times": 2 } },
        { "type": "request", "name": "Never", "request": "Create.yaml" },
    ]);
    let always = json!([
        { "type": "request", "request": "Broken.yaml" },
        { "type": "script", "code": "pm.variables.set('cleaned', true); pm.test('cleaned up', () => pm.expect(1).to.equal(1)); console.log('bye');" },
    ]);
    let id = h.scenario(json!({ "name": "Fails", "steps": steps, "always": always })).await;
    let (results, summary) = h.run(json!({ "id": id })).await;
    assert_eq!(lines(&results), ["!Create", "!Broken", "!Broken", "Script"], "{results:#?}");
    assert_eq!(results[0]["tests"][0]["error"], "It is \"7\"");
    assert_eq!(results[1]["attempts"], 3);
    assert_eq!(results[3]["step"]["path"], "always/1");
    assert_eq!(results[3]["console"], json!([{ "level": "log", "message": "bye" }]));
    assert_eq!(results[3]["step"]["saved"], json!([{ "key": "cleaned", "value": "true" }]));
    assert_eq!(
        (summary["passed"].clone(), summary["failed"].clone(), summary["bailed"].clone()),
        (json!(false), json!(3), json!(true))
    );

    // The whole scenario may go on after failures; a stop step still ends it.
    let lenient = h
        .scenario(json!({ "name": "Lenient", "continueOnFailure": true, "steps": [
            { "type": "request", "request": "Broken.yaml" },
            { "type": "request", "request": "Missing.yaml" },
            { "type": "script", "code": "throw new Error('boom')" },
            { "type": "set", "values": [{ "key": "x", "value": "= nope(" }] },
            { "type": "if", "when": { "rules": [{ "value": "abc", "op": "greater", "to": "1" }] }, "steps": [] },
            { "type": "stop", "what": "scenario", "message": "done" },
            { "type": "request", "name": "Never", "request": "Create.yaml" },
        ] }))
        .await;
    let (results, summary) = h.run(json!({ "id": lenient })).await;
    assert_eq!(
        lines(&results),
        ["!Broken", "!Missing", "!Script", "!Set x", "!If abc is greater than 1", "End the scenario: done"]
    );
    assert!(
        results[1]["error"].as_str().unwrap().starts_with("The request can't be read: "),
        "{}",
        results[1]["error"]
    );
    assert_eq!(results[2]["scriptErrors"][0], "The script failed at line 1: Error: boom");
    assert!(results[3]["error"].as_str().unwrap().starts_with("x: `nope(`: "), "{}", results[3]["error"]);
    assert_eq!(results[4]["error"], "abc is greater than 1: 'abc' is not a number");
    assert_eq!((summary["failed"].clone(), summary["bailed"].clone()), (json!(5), json!(false)));

    // An output that must be there fails the step when it is missing.
    let output = h
        .scenario(json!({ "name": "Output", "steps": [{ "type": "request", "request": "Create.yaml", "outputs": [{ "variable": "x", "path": "$.args.nope" }, { "variable": "y", "from": "regex", "path": "(" }] }] }))
        .await;
    let (results, _) = h.run(json!({ "id": output })).await;
    assert_eq!(
        results[0]["tests"][0],
        json!({ "name": "Save x from $.args.nope", "passed": false, "skipped": false, "error": "Nothing was found" })
    );
    assert!(results[0]["tests"][1]["error"].as_str().unwrap().contains("regular expression"));
    assert_eq!(results[0]["passed"], false);

    // What can't run is refused before it starts.
    let empty = h.scenario(json!({ "name": "Empty", "steps": [] })).await;
    let err = h.api.call("scenario.run", json!({ "id": empty })).await.unwrap_err();
    assert_eq!(err.message, "'Empty' has no steps to run");
    let err = h.api.call("scenario.run", json!({ "id": id, "to": "steps/9" })).await.unwrap_err();
    assert_eq!(err.message, "That step is no longer in the scenario");
}

#[tokio::test]
async fn a_part_of_a_scenario_runs_with_the_values_of_the_last_run() {
    let h = Harness::new().await;
    let id = h
        .scenario(json!({ "name": "Parts", "steps": [
            { "type": "request", "request": "Create.yaml", "outputs": [{ "variable": "cartId", "path": "$.args.id" }] },
            { "type": "request", "request": "Use.yaml", "inputs": [{ "key": "ref", "value": "r" }] },
            { "type": "request", "name": "Later", "request": "Create.yaml" },
        ], "always": [{ "type": "request", "name": "Clean up", "request": "Create.yaml" }] }))
        .await;
    // Up to a step: the clean-up steps wait for a whole run.
    let (results, summary) = h.run(json!({ "id": id, "to": "steps/0" })).await;
    assert_eq!(lines(&results), ["Create"]);
    assert_eq!(summary["passed"], true);
    assert_eq!(h.values(&id).await, ["cartId=7"]);
    // One step, with what the last run left.
    let (results, _) = h.run(json!({ "id": id, "only": "steps/1" })).await;
    assert_eq!(lines(&results), ["Use"]);
    assert_eq!(args(&results[0]), "cart=7&ref=r");
    // The draft as edited runs, not the saved file; values given for the run win.
    let mut draft = h.ok("scenario.read", json!({ "id": id })).await;
    draft["steps"][1]["inputs"][0]["value"] = json!("edited");
    let (results, _) =
        h.run(json!({ "id": id, "scenario": draft, "only": "steps/1", "variables": { "cartId": "99" } })).await;
    assert_eq!(args(&results[0]), "cart=99&ref=edited");
    let (results, _) = h.run(json!({ "id": id })).await;
    assert_eq!(lines(&results), ["Create", "Use", "Later", "Clean up"]);

    // A step the run never gets to (a branch not taken): still no clean-up. A clean-up step
    // itself can be run to.
    let branch = h
        .scenario(json!({ "name": "Branch", "steps": [
            { "type": "if", "when": { "rules": [{ "value": "1", "to": "2" }] }, "steps": [{ "type": "request", "request": "Create.yaml" }] },
        ], "always": [{ "type": "wait", "ms": 1 }, { "type": "request", "name": "Clean up", "request": "Create.yaml" }] }))
        .await;
    let (results, _) = h.run(json!({ "id": branch, "to": "steps/0/steps/0" })).await;
    assert_eq!(lines(&results), ["If 1 equals 2: no"]);
    let (results, _) = h.run(json!({ "id": branch, "to": "always/0" })).await;
    assert_eq!(lines(&results), ["If 1 equals 2: no", "Wait 1 ms"]);

    // Rows not filled in yet are left out: a scenario being written saves and runs.
    let blank = h
        .scenario(json!({ "name": "Blank", "variables": [{ "key": "", "value": "" }, { "key": "", "value": "x" }], "steps": [
            { "type": "request", "request": "Create.yaml", "inputs": [{ "key": " ", "value": "1" }], "outputs": [{ "variable": "", "path": "$.args.id" }] },
            { "type": "set", "values": [{ "key": "", "value": "" }] },
            { "type": "ask", "variable": "", "continueOnFailure": true },
        ] }))
        .await;
    let (results, _) = h.run(json!({ "id": blank })).await;
    assert_eq!(lines(&results), ["Create", "Set values", "!Ask for a value"]);
    assert_eq!(results[2]["error"], "Give the variable the answer goes in");
    assert!(h.values(&blank).await.is_empty());
}

#[tokio::test]
async fn scenarios_run_each_other_follow_renames_and_ask_for_values() {
    let h = Harness::new().await;
    let login = h
        .scenario(json!({ "name": "Log in", "variables": [{ "key": "user", "value": "guest" }], "steps": [
            { "type": "request", "request": "Create.yaml", "outputs": [{ "variable": "token", "path": "$.args.name", "secret": true }] },
            { "type": "set", "values": [{ "key": "who", "value": "{{user}}/{{outer}}" }] },
        ] }))
        .await;
    let main = h
        .scenario(json!({ "name": "Main", "variables": [{ "key": "outer", "value": "mine" }], "steps": [
            { "type": "scenario", "scenario": login, "inputs": [{ "key": "user", "value": "ada" }], "outputs": [{ "key": "t", "value": "{{token}}" }, { "key": "who", "value": "{{who}}" }] },
            { "type": "ask", "variable": "otp", "message": "Code for {{who}}" },
            { "type": "request", "request": "Use.yaml", "inputs": [{ "key": "cartId", "value": "{{otp}}" }, { "key": "ref", "value": "{{t}}" }] },
        ] }))
        .await;
    let started = h.ok("scenario.run", json!({ "id": main })).await;
    let run_id = started["runId"].as_str().unwrap().to_string();
    let mut asked = None;
    for _ in 0..400 {
        asked = h.event(&run_id, "ask");
        if asked.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    // The other scenario has its own values: it doesn't see this one's `outer`.
    assert_eq!(
        asked.unwrap(),
        json!({ "type": "ask", "variable": "otp", "message": "Code for ada/{{outer}}", "secret": false })
    );
    h.ok("scenario.answer", json!({ "runId": run_id, "value": "123456" })).await;
    let summary = h.finished(&run_id).await;
    let results = h.results(&run_id);
    assert_eq!(lines(&results), ["Run scenario Log in", "  Create", "  Set who", "Ask for otp", "Use"], "{results:#?}");
    assert_eq!(summary["passed"], true);
    assert_eq!(results[1]["step"]["path"], "steps/0>steps/0");
    // A secret value is hidden where it was saved and in what the request reports.
    assert_eq!(results[1]["step"]["saved"], json!([{ "key": "token", "value": "••••••", "secret": true }]));
    assert_eq!(args(&results[4]), "cart=123456&ref={{t}}");
    let values = h.values(&main).await;
    assert_eq!(values, ["otp=123456", "outer=mine", "t=••••••", "who=ada/{{outer}}"]);

    // No answer fails the step; a run nobody watches needs the value up front.
    let started = h.ok("scenario.run", json!({ "id": main })).await;
    let run_id = started["runId"].as_str().unwrap().to_string();
    for _ in 0..400 {
        if h.event(&run_id, "ask").is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    h.ok("scenario.answer", json!({ "runId": run_id, "value": null })).await;
    h.finished(&run_id).await;
    assert_eq!(h.results(&run_id).last().unwrap()["error"], "No value was given for 'otp'");
    let (results, summary) = h.run(json!({ "id": main, "variables": { "otp": "42" } })).await;
    assert_eq!(lines(&results)[3], "Ask for otp: given for this run");
    assert_eq!((summary["passed"].clone(), args(&results[4])), (json!(true), "cart=42&ref={{t}}".to_string()));

    // Renamed requests and scenarios are followed.
    h.ok("item.rename", json!({ "path": "Create.yaml", "name": "Create cart" })).await;
    let folder = h.ok("folder.create", json!({ "parent": "", "name": "Carts" })).await;
    h.ok("item.move", json!({ "path": "Use.yaml", "parent": folder })).await;
    let mut renamed = h.ok("scenario.read", json!({ "id": login })).await;
    assert_eq!(renamed["steps"][0]["request"], "Create cart.yaml");
    renamed["name"] = json!("Sign in");
    let new_login = h.ok("scenario.save", json!({ "id": login, "scenario": renamed })).await;
    assert_eq!(new_login, "Sign in");
    let main_now = h.ok("scenario.read", json!({ "id": main })).await;
    assert_eq!(
        (main_now["steps"][0]["scenario"].clone(), main_now["steps"][2]["request"].clone()),
        (json!("Sign in"), json!("Carts/Use.yaml"))
    );
    let (_, summary) = h.run(json!({ "id": main, "variables": { "otp": "1" } })).await;
    assert_eq!(summary["passed"], true);

    // A scenario that runs itself ends with a clear error.
    let itself = h.scenario(json!({ "name": "Loop", "steps": [] })).await;
    let scenario = json!({ "name": "Loop", "steps": [{ "type": "scenario", "scenario": itself }] });
    h.ok("scenario.save", json!({ "id": itself, "scenario": scenario })).await;
    let (results, summary) = h.run(json!({ "id": itself })).await;
    assert!(results.last().unwrap()["error"].as_str().unwrap().contains("more than 8 deep"), "{results:#?}");
    assert_eq!(summary["passed"], false);

    h.ok("scenario.duplicate", json!({ "id": main })).await;
    h.ok("scenario.move", json!({ "id": "Main copy", "index": 0 })).await;
    h.ok("scenario.move", json!({ "id": "Main", "index": 1 })).await;
    let names: Vec<Value> =
        h.ok("scenario.list", json!({})).await.as_array().unwrap().iter().map(|n| n["name"].clone()).collect();
    assert_eq!(names, ["Main copy", "Main", "Sign in", "Loop"]);
}

#[tokio::test]
async fn data_rows_drive_iterations_and_what_a_server_sends_is_never_expanded() {
    let h = Harness::new().await;
    std::fs::write(h.ws_dir.path().join("users.csv"), "user,productId\nada,1\ngrace,2\n").unwrap();
    // The server answers with text that looks like a variable: it must go on as that text,
    // not as the secret's value.
    h.request("Trap", "{{base}}/echo?x=%7B%7Bkey%7D%7D", "").await;
    h.request("After", "{{base}}/echo?v={{fromServer}}&u={{user}}&p={{productId}}", "").await;
    let id = h
        .scenario(json!({ "name": "Rows", "dataFile": "users.csv", "variables": [{ "key": "productId", "value": "default" }], "steps": [
            { "type": "request", "request": "Trap.yaml", "outputs": [{ "variable": "fromServer", "path": "$.args.x" }] },
            { "type": "request", "request": "After.yaml", "checks": [{ "path": "$.args.v", "value": "= '{{' + 'key}}'" }] },
        ] }))
        .await;
    let (results, summary) = h.run(json!({ "id": id })).await;
    assert_eq!(summary["iterations"], 2);
    assert_eq!(summary["passed"], true, "{results:#?}");
    let iterations: Vec<u64> = results.iter().map(|r| r["iteration"].as_u64().unwrap()).collect();
    assert_eq!(iterations, [0, 0, 1, 1]);
    // The row wins over the scenario's own starting value.
    assert!(args(&results[1]).ends_with("&u=ada&p=1"), "{}", results[1]["url"]);
    assert!(args(&results[3]).ends_with("&u=grace&p=2"), "{}", results[3]["url"]);
    assert!(!results[1]["url"].as_str().unwrap().contains("top-secret"));
    // Iterations as asked, each starting from the scenario's values again.
    let mut scenario = h.ok("scenario.read", json!({ "id": id })).await;
    scenario["iterations"] = json!(1);
    let (results, _) = h.run(json!({ "id": id, "scenario": scenario })).await;
    assert_eq!(results.len(), 2);

    // The reports of a scenario run: flow steps aren't cases of their own.
    let flow = h
        .scenario(json!({ "name": "Report", "steps": [
            { "type": "forEach", "list": "a,b", "steps": [{ "type": "request", "request": "Create.yaml", "checks": [{ "from": "status", "value": "200" }] }] },
            { "type": "wait", "ms": 1 },
        ] }))
        .await;
    let started = h.ok("scenario.run", json!({ "id": flow })).await;
    let run_id = started["runId"].as_str().unwrap();
    h.finished(run_id).await;
    let path = h.ws_dir.path().join("report.xml");
    h.ok("runner.export", json!({ "runId": run_id, "path": path, "format": "junit" })).await;
    let xml = std::fs::read_to_string(&path).unwrap();
    assert!(xml.contains("<testsuites name=\"Report\" tests=\"3\" failures=\"0\""), "{xml}");
    assert!(
        xml.contains("<testsuite name=\"Create\" tests=\"2\"")
            && xml.contains("<testsuite name=\"Wait 1 ms\" tests=\"1\""),
        "{xml}"
    );
    let details = h.ok("runner.details", json!({ "runId": run_id, "seq": 3 })).await;
    assert_eq!(details["response"]["status"], 200);
}

#[tokio::test]
async fn secrets_stay_hidden_and_failed_tries_and_other_scenarios_leave_nothing_behind() {
    let h = Harness::new().await;
    // Passes on its third try; every try answers the same.
    let third = "const n = Number(pm.variables.get('tries') || 0) + 1; pm.variables.set('tries', n); \
                 pm.test('third try', () => pm.expect(n).to.equal(3));";
    h.request("Flaky", "{{base}}/echo?n=x", third).await;
    let id = h
        .scenario(json!({ "name": "Tries", "steps": [
            { "type": "request", "request": "Flaky.yaml", "retry": { "times": 3 },
              "outputs": [{ "variable": "seen", "path": "$.args.n", "collect": true }] },
            // A value built from a secret, and a list spread into a call.
            { "type": "set", "values": [
                { "key": "auth", "value": "Bearer {{key}}" },
                { "key": "most", "value": "= Math.max(...[1, 5, 3], ...seen.map((s) => s.length))" },
            ] },
            { "type": "ask", "variable": "pin", "secret": true },
            { "type": "set", "values": [{ "key": "code", "value": "pin-{{pin}}" }] },
            { "type": "request", "request": "Use.yaml", "inputs": [{ "key": "cartId", "value": "{{pin}}" }, { "key": "ref", "value": "{{most}}" }] },
        ] }))
        .await;
    let (results, summary) = h.run(json!({ "id": id, "variables": { "pin": "987654" } })).await;
    assert_eq!(summary["passed"], true, "{results:#?}");
    // Only the try that passed saved its output.
    assert_eq!(results[0]["attempts"], 3);
    assert_eq!(results[0]["step"]["saved"], json!([{ "key": "seen", "value": "[\"x\"]" }]));
    assert_eq!(
        results[1]["step"]["saved"],
        json!([{ "key": "auth", "value": "Bearer {{key}}" }, { "key": "most", "value": "5" }])
    );
    // A secret answer given with the run is hidden like one that was typed.
    assert_eq!(lines(&results)[2], "Ask for pin: given for this run");
    assert_eq!(results[3]["step"]["saved"], json!([{ "key": "code", "value": "pin-{{pin}}" }]));
    assert_eq!(args(&results[4]), "cart={{pin}}&ref=5");
    let values = h.values(&id).await;
    assert!(values.contains(&"auth=Bearer {{key}}".to_string()), "{values:?}");
    assert!(values.contains(&"seen=[\"x\"]".to_string()), "{values:?}");

    // Another scenario's values end with it, also what its scripts set; a failed step of it
    // fails the step that ran it, even one it went on after.
    h.request("Leak", "{{base}}/echo?leak=1", "pm.environment.set('leak', '{{key}}');").await;
    let inner = h
        .scenario(json!({ "name": "Inner", "continueOnFailure": true, "steps": [
            { "type": "request", "request": "Leak.yaml" },
            { "type": "request", "request": "Broken.yaml" },
            { "type": "request", "request": "Create.yaml" },
        ] }))
        .await;
    let call = |go_on: bool| json!({ "type": "scenario", "scenario": inner, "continueOnFailure": go_on });
    let outer = |go_on: bool| {
        json!({ "name": "Outer", "steps": [
            call(go_on),
            { "type": "request", "request": "Use.yaml", "inputs": [{ "key": "cartId", "value": "x{{leak}}" }, { "key": "ref", "value": "r" }] },
        ] })
    };
    let outer_id = h.scenario(outer(false)).await;
    let (results, summary) = h.run(json!({ "id": outer_id })).await;
    assert_eq!(lines(&results), ["Run scenario Inner", "  Leak", "  !Broken", "  Create"], "{results:#?}");
    assert_eq!(summary["passed"], false);
    let (results, summary) = h.run(json!({ "id": outer_id, "scenario": outer(true) })).await;
    assert_eq!(lines(&results), ["Run scenario Inner", "  Leak", "  !Broken", "  Create", "Use"], "{results:#?}");
    assert_eq!((summary["passed"].clone(), summary["failed"].clone()), (json!(false), json!(1)));
    assert_eq!(args(&results[4]), "cart=x{{leak}}&ref=r");
    assert!(!h.values(&outer_id).await.iter().any(|v| v.starts_with("leak=")));
    // A scenario failed by a stop step fails the step too.
    let failing = json!({ "name": "Inner", "steps": [{ "type": "stop", "what": "fail", "message": "no stock" }] });
    h.ok("scenario.save", json!({ "id": inner, "scenario": failing })).await;
    let (results, summary) = h.run(json!({ "id": outer_id })).await;
    assert_eq!(lines(&results), ["Run scenario Inner", "  !Fail the scenario"], "{results:#?}");
    assert_eq!(summary["passed"], false);
}

#[tokio::test]
async fn without_an_environment_what_scripts_set_on_it_is_the_scenarios_too() {
    let h = Harness::new().await;
    h.ok("env.setActive", json!({ "id": null })).await;
    let url = h.server.url("");
    h.request("Login", &format!("{url}/echo?login=1"), "pm.environment.set('token', 'tok-9');").await;
    h.request("Me", &format!("{url}/echo?t={{{{token}}}}"), "").await;
    let id = h
        .scenario(json!({ "name": "No environment", "steps": [
            { "type": "request", "request": "Login.yaml" },
            { "type": "request", "request": "Me.yaml", "checks": [{ "path": "$.args.t", "value": "tok-9" }] },
        ] }))
        .await;
    let (results, summary) = h.run(json!({ "id": id })).await;
    assert_eq!(summary["passed"], true, "{results:#?}");
    assert_eq!(results[0]["console"], json!([]), "nothing is lost, so nothing to warn about");
    assert_eq!(h.values(&id).await, ["token=tok-9"]);
    assert_eq!(h.ok("vars.local", json!({})).await, json!([]));
}

#[tokio::test]
async fn folders_keep_scenarios_in_order_and_steps_follow_them() {
    let h = Harness::new().await;
    let shop = h.ok("scenario.createFolder", json!({ "parent": "", "name": "Shop" })).await;
    let auth = h.ok("scenario.createFolder", json!({ "parent": shop, "name": "Auth" })).await;
    assert_eq!((shop.clone(), auth.clone()), (json!("Shop"), json!("Shop/Auth")));
    let login = h
        .ok(
            "scenario.create",
            json!({ "folder": auth, "scenario": { "name": "Log in", "steps": [
                { "type": "request", "request": "Create.yaml", "outputs": [{ "variable": "token", "path": "$.args.name" }] },
            ] } }),
        )
        .await;
    assert_eq!(login, "Shop/Auth/Log in");
    let main = h
        .scenario(json!({ "name": "Main", "steps": [
            { "type": "scenario", "scenario": login, "outputs": [{ "key": "t", "value": "{{token}}" }] },
        ] }))
        .await;
    let (_, summary) = h.run(json!({ "id": main })).await;
    assert_eq!(summary["passed"], true);
    let (_, summary) = h.run(json!({ "id": login })).await;
    assert_eq!(summary["passed"], true);
    assert_eq!(h.values("Shop/Auth/Log in").await, ["token=Ada"]);

    // The tree: folders with what is in them.
    let tree = h.ok("scenario.list", json!({})).await;
    assert_eq!((tree[0]["id"].clone(), tree[0]["folder"].clone()), (json!("Shop"), json!(true)));
    assert_eq!(tree[0]["children"][0]["children"][0]["id"], "Shop/Auth/Log in");
    assert_eq!(tree[1]["id"], "Main");
    assert_eq!(tree[1].get("folder"), None, "a scenario says nothing about folders");

    // A folder renamed, then moved: the step that runs a scenario in it follows, and so do the
    // values its last run left.
    let auth = h.ok("scenario.renameFolder", json!({ "id": auth, "name": "Sign in" })).await;
    assert_eq!(auth, "Shop/Sign in");
    let auth = h.ok("scenario.move", json!({ "id": auth, "folder": true, "parent": "", "index": 0 })).await;
    assert_eq!(auth, "Sign in");
    assert_eq!(h.ok("scenario.read", json!({ "id": main })).await["steps"][0]["scenario"], "Sign in/Log in");
    assert_eq!(h.values("Sign in/Log in").await, ["token=Ada"]);
    // A scenario moved into a folder, and renamed there.
    let moved = h.ok("scenario.move", json!({ "id": "Sign in/Log in", "parent": "Shop" })).await;
    assert_eq!(moved, "Shop/Log in");
    let mut renamed = h.ok("scenario.read", json!({ "id": moved })).await;
    renamed["name"] = json!("Log on");
    let renamed = h.ok("scenario.save", json!({ "id": moved, "scenario": renamed })).await;
    assert_eq!(renamed, "Shop/Log on");
    assert_eq!(h.ok("scenario.read", json!({ "id": main })).await["steps"][0]["scenario"], "Shop/Log on");
    let (results, summary) = h.run(json!({ "id": main })).await;
    assert_eq!(
        (summary["passed"].clone(), lines(&results)[0].clone()),
        (json!(true), "Run scenario Log on".to_string())
    );
    let copy = h.ok("scenario.duplicate", json!({ "id": renamed })).await;
    assert_eq!(copy, "Shop/Log on copy");

    let order: Vec<Value> =
        h.ok("scenario.list", json!({})).await.as_array().unwrap().iter().map(|n| n["id"].clone()).collect();
    assert_eq!(order, ["Sign in", "Shop", "Main"]);
    let err = h.api.call("scenario.move", json!({ "id": "Shop", "folder": true, "parent": "Shop" })).await.unwrap_err();
    assert!(err.message.contains("into itself"), "{}", err.message);
    let err = h
        .api
        .call("scenario.create", json!({ "folder": "../requests", "scenario": { "name": "X" } }))
        .await
        .unwrap_err();
    assert!(err.message.contains("Invalid scenario id"), "{}", err.message);
    if std::env::var_os("ZORVIK_TEST_TRASH").is_some() {
        h.ok("scenario.deleteFolder", json!({ "id": "Shop" })).await;
        assert!(h.values("Shop/Log on").await.is_empty());
        assert!(h.api.call("scenario.read", json!({ "id": "Shop/Log on" })).await.is_err());
    }
}

//! Tools for scenarios: list, read, create or change, and run them (the whole scenario, up
//! to a step, or one step).

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::{Value, json};
use zorvik_workspace::Workspace;
use zorvik_workspace::formats::Scenario;

use super::{Call, Def, Done, Fail, Outcome, obj, plural, string, wait_schema};
use crate::Api;
use crate::agents::{AgentEvent, AgentTarget};
use crate::runner::RunReport;

/// Step results returned for a finished run at most (the summary counts all of them).
const MAX_STEPS: usize = 200;

fn values_schema(description: &str) -> Value {
    json!({
        "type": "array",
        "description": description,
        "items": obj(json!({ "key": { "type": "string" }, "value": { "type": "string" }, "enabled": { "type": "boolean" } }), &["key", "value"]),
    })
}

fn condition_schema(description: &str) -> Value {
    json!({
        "type": "object",
        "description": description,
        "properties": {
            "any": { "type": "boolean", "description": "true: one rule that holds is enough (default: all must hold)." },
            "rules": {
                "type": "array",
                "items": obj(json!({
                    "value": string("The left side: text with {{variables}}, or JavaScript after =."),
                    "op": { "type": "string", "enum": OPS, "description": "Default equals. Numbers compare as numbers (201 equals \"201\"). exists: defined and not empty. in / notIn: `to` is a JSON array or values separated by commas. isTrue / isFalse, exists / notExists take no `to`." },
                    "to": string("The right side: text with {{variables}}, or JavaScript after =."),
                }), &["value"]),
            },
        },
        "required": ["rules"],
    })
}

const OPS: [&str; 15] = [
    "equals",
    "notEquals",
    "contains",
    "notContains",
    "matches",
    "greater",
    "greaterOrEqual",
    "less",
    "lessOrEqual",
    "exists",
    "notExists",
    "in",
    "notIn",
    "isTrue",
    "isFalse",
];

const SOURCES: [&str; 7] = ["json", "header", "cookie", "regex", "status", "body", "time"];

/// A scenario as agents give it.
fn scenario_schema() -> Value {
    let source = "json (default): a JSONPath into the body, e.g. $.id, $.items[0].id; a path that can match several values ($.items[*].id, a filter, ..) gives a list. header: a response header by name. cookie: a cookie the response set. regex: a regular expression over the body (its first group). status: the status code. body: the whole body. time: the response time in ms.";
    let step = obj(
        json!({
            "type": { "type": "string", "enum": ["request", "if", "forEach", "repeat", "parallel", "set", "wait", "stop", "scenario", "script", "ask"], "description": "request: send a saved request. if: run `steps` when `when` holds, else the first of `elseIf` that holds, else `else`. forEach: run `steps` once per item of `list` ({{item}}, {{item.field}}, {{index}} from 0). repeat: run `steps` `times` times, or until `until` holds (it fails when it never does). parallel: run `steps` at the same time (each with its own copy of the values, put together at the end). set: set `values`. wait: pause `ms`. stop: leave the loop, or end the scenario. scenario: run another scenario with its own values. script: JavaScript with the pm API. ask: ask the user for a value (a run you start must give it in `variables`)." },
            "name": string("Shown in the list and the results (default: the request's name, or what the step does)."),
            "enabled": { "type": "boolean", "description": "false: left out of runs." },
            "continueOnFailure": { "type": "boolean", "description": "Go on with the next step when this one fails (the run still fails)." },
            "request": string("request: the request's path as list_requests shows it, e.g. \"Cart/Create cart.yaml\"."),
            "inputs": values_schema("request, scenario: values for this step only; they win over every other variable, so the request's own {{name}} takes them. In a scenario step: the values the other scenario starts with."),
            "outputs": {
                "type": "array",
                "description": "request: values saved from the response for the steps after it, used as {{variable}}. An output that finds nothing fails the step unless optional. scenario: [{key, value}] values to bring back, each value read with the other scenario's values, e.g. {key: \"token\", value: \"{{token}}\"}.",
                "items": obj(json!({
                    "variable": string("Variable name (no spaces or braces)."),
                    "from": { "type": "string", "enum": SOURCES, "description": source },
                    "path": string("The JSONPath, header or cookie name, or regular expression."),
                    "secret": { "type": "boolean", "description": "Hide the value in results (a token)." },
                    "optional": { "type": "boolean", "description": "Finding nothing is fine: the variable is then unset." },
                    "collect": { "type": "boolean", "description": "Add the value to a list under this name (one per round of a loop) instead of replacing it." },
                    "key": { "type": "string" },
                    "value": { "type": "string" },
                    "enabled": { "type": "boolean" },
                }), &[]),
            },
            "checks": {
                "type": "array",
                "description": "request: checks on the response; each is a test in the results. A request with checks or tests passes when all of them pass; without any, when the status is below 400.",
                "items": obj(json!({
                    "from": { "type": "string", "enum": SOURCES, "description": source },
                    "path": { "type": "string" },
                    "op": { "type": "string", "enum": OPS },
                    "value": string("What it is compared with: text with {{variables}}, or JavaScript after =."),
                    "enabled": { "type": "boolean" },
                }), &[]),
            },
            "retry": obj(json!({ "times": { "type": "integer", "description": "More tries after the first one when the step fails." }, "delayMs": { "type": "integer" } }), &["times"]),
            "when": condition_schema("if: the condition."),
            "steps": { "type": "array", "description": "if, forEach, repeat, parallel: the steps inside (the same shape as these steps).", "items": { "type": "object" } },
            "elseIf": { "type": "array", "description": "if: more branches, [{when, steps}].", "items": { "type": "object" } },
            "else": { "type": "array", "description": "if: the steps when no condition holds.", "items": { "type": "object" } },
            "list": string("forEach: {{variable}} holding a list, a JSON array, values separated by commas, or JavaScript after =."),
            "as": string("forEach: the variable each item is in (default item)."),
            "times": { "type": "integer", "description": "repeat: how many rounds (with `until`: at most)." },
            "until": condition_schema("repeat: stop when this holds after a round."),
            "delayMs": { "type": "integer", "description": "repeat: pause between two rounds." },
            "values": values_schema("set: [{key, value}]. A value is text with {{variables}}, or JavaScript after = (e.g. \"= Number(total) + 1\"; variables by name, all of them in `vars`)."),
            "ms": { "type": "integer", "description": "wait: milliseconds." },
            "what": { "type": "string", "enum": ["loop", "scenario", "fail"], "description": "stop: loop = leave the loop it is in; scenario (default) = end the scenario as passed; fail = end it as failed." },
            "message": string("stop: why. ask: the question."),
            "scenario": string("scenario: the name of the scenario to run."),
            "code": string("script: JavaScript. pm.variables.set(name, value) saves a value of the scenario; pm.test, pm.expect and pm.sendRequest work as in request scripts."),
            "variable": string("ask: the variable the answer goes in."),
            "secret": { "type": "boolean", "description": "ask: hide the answer in results." },
        }),
        &["type"],
    );
    obj(
        json!({
            "variables": values_schema("The values a run starts with, [{key, value}]. Everything a scenario sets (these, outputs, what scripts set) stays in the scenario: environments and collection variables are read, never changed. Values are data: {{ }} inside a saved value is not expanded again."),
            "steps": { "type": "array", "description": "What runs, in order.", "items": step.clone() },
            "always": { "type": "array", "description": "Steps that run at the end whatever happened before (clean up).", "items": step },
            "dataFile": string("CSV or JSON data file inside the workspace folder: the scenario runs once per row, the columns as variables."),
            "iterations": { "type": "integer", "description": "How many times it runs (default: once per data row, or once)." },
            "continueOnFailure": { "type": "boolean", "description": "Go on after a failed step (default: the first failed step ends the run; `always` still runs)." },
            "docs": string("Markdown notes: what the scenario checks and what it needs."),
        }),
        &["steps"],
    )
}

/// The fields a step of each type takes (besides the ones every step has).
fn step_fields(kind: &str) -> Option<&'static [&'static str]> {
    Some(match kind {
        "request" => &["request", "inputs", "outputs", "checks", "retry"],
        "if" => &["when", "steps", "elseIf", "else"],
        "forEach" => &["list", "as", "steps"],
        "repeat" => &["times", "until", "delayMs", "steps"],
        "parallel" => &["steps"],
        "set" => &["values"],
        "wait" => &["ms"],
        "stop" => &["what", "message"],
        "scenario" => &["scenario", "inputs", "outputs"],
        "script" => &["code"],
        "ask" => &["variable", "message", "secret"],
        _ => return None,
    })
}

/// Fields the scenario format doesn't have, as `steps[2].outputs[0].varable` (serde would
/// drop them silently, so a misspelled field would look saved).
fn unknown_fields(scenario: &Value) -> Vec<String> {
    fn object(value: &Value, at: &str, known: &[&str], out: &mut Vec<String>) {
        for key in value.as_object().into_iter().flat_map(|map| map.keys()) {
            if !known.contains(&key.as_str()) {
                out.push(format!("{at}.{key}"));
            }
        }
    }
    fn list(value: Option<&Value>, at: &str, known: &[&str], out: &mut Vec<String>) {
        for (i, item) in value.and_then(Value::as_array).into_iter().flatten().enumerate() {
            object(item, &format!("{at}[{i}]"), known, out);
        }
    }
    fn condition(value: Option<&Value>, at: &str, out: &mut Vec<String>) {
        let Some(value) = value else { return };
        object(value, at, &["any", "rules"], out);
        list(value.get("rules"), &format!("{at}.rules"), &["value", "op", "to"], out);
    }
    fn steps(value: Option<&Value>, at: &str, out: &mut Vec<String>) {
        for (i, step) in value.and_then(Value::as_array).into_iter().flatten().enumerate() {
            let at = format!("{at}[{i}]");
            let kind = step.get("type").and_then(Value::as_str).unwrap_or_default();
            // An unknown type is refused when the scenario is read.
            let Some(own) = step_fields(kind) else { continue };
            let mut known = vec!["type", "name", "enabled", "continueOnFailure"];
            known.extend(own);
            object(step, &at, &known, out);
            let pairs = ["key", "value", "enabled"];
            match kind {
                "request" => {
                    list(step.get("inputs"), &format!("{at}.inputs"), &pairs, out);
                    let output = ["variable", "from", "path", "secret", "optional", "collect"];
                    list(step.get("outputs"), &format!("{at}.outputs"), &output, out);
                    list(step.get("checks"), &format!("{at}.checks"), &["from", "path", "op", "value", "enabled"], out);
                    if let Some(retry) = step.get("retry") {
                        object(retry, &format!("{at}.retry"), &["times", "delayMs"], out);
                    }
                }
                "if" => {
                    condition(step.get("when"), &format!("{at}.when"), out);
                    for (b, branch) in step.get("elseIf").and_then(Value::as_array).into_iter().flatten().enumerate() {
                        let at = format!("{at}.elseIf[{b}]");
                        object(branch, &at, &["when", "steps"], out);
                        condition(branch.get("when"), &format!("{at}.when"), out);
                        steps(branch.get("steps"), &format!("{at}.steps"), out);
                    }
                    steps(step.get("else"), &format!("{at}.else"), out);
                }
                "repeat" => condition(step.get("until"), &format!("{at}.until"), out),
                "set" => list(step.get("values"), &format!("{at}.values"), &pairs, out),
                "scenario" => {
                    list(step.get("inputs"), &format!("{at}.inputs"), &pairs, out);
                    list(step.get("outputs"), &format!("{at}.outputs"), &pairs, out);
                }
                _ => {}
            }
            steps(step.get("steps"), &format!("{at}.steps"), out);
        }
    }
    let mut out = Vec::new();
    let known = ["name", "seq", "variables", "steps", "always", "dataFile", "iterations", "continueOnFailure", "docs"];
    object(scenario, "scenario", &known, &mut out);
    list(scenario.get("variables"), "scenario.variables", &["key", "value", "enabled"], &mut out);
    steps(scenario.get("steps"), "scenario.steps", &mut out);
    steps(scenario.get("always"), "scenario.always", &mut out);
    out
}

pub(super) fn defs() -> Vec<Def> {
    vec![
        Def {
            name: "list_scenarios",
            title: "List scenarios",
            description: "Saved scenarios: saved requests run as steps, with values passed from one step to the next, checks, and flow (if, loops, steps at the same time).",
            schema: obj(json!({}), &[]),
            read_only: true,
            destructive: false,
            open_world: false,
        },
        Def {
            name: "read_scenario",
            title: "Read a scenario",
            description: "A scenario's variables, steps and docs, in the shape save_scenario takes, and the values its last run left (secret ones hidden).",
            schema: obj(json!({ "name": { "type": "string" } }), &["name"]),
            read_only: true,
            destructive: false,
            open_world: false,
        },
        Def {
            name: "save_scenario",
            title: "Save a scenario",
            description: "Create or replace a scenario. Its steps send requests that are saved in the collection (save them first with save_requests). Pass a value from one step to the next with an output on the first (e.g. {variable: \"cartId\", path: \"$.id\"}) and {{cartId}} in the later request, or as an input of the later step. Unknown or misspelled fields are refused.",
            schema: obj(json!({ "name": { "type": "string" }, "scenario": scenario_schema() }), &["name", "scenario"]),
            read_only: false,
            destructive: false,
            open_world: false,
        },
        Def {
            name: "run_scenario",
            title: "Run a scenario",
            description: "Run a saved scenario and return its summary, what each step did (the values it saved too) and the failures. The scenario's tab shows it live. Still running after waitSeconds: poll with get_run_status; stop it with stop_collection_run.",
            schema: obj(
                json!({
                    "name": { "type": "string" },
                    "variables": { "type": "object", "description": "Values for this run, {name: value}: they win over the scenario's own values and every other variable, and answer its ask steps.", "additionalProperties": { "type": "string" } },
                    "toStep": string("Run from the start up to and including this step, e.g. \"steps/2\" (the third step) or \"steps/2/steps/0\" (the first step inside it). The clean-up steps don't run."),
                    "onlyStep": string("Run only this step, with the values the last run left."),
                    "waitSeconds": wait_schema(),
                }),
                &["name"],
            ),
            read_only: false,
            destructive: false,
            open_world: true,
        },
    ]
}

impl Api {
    pub(super) fn find_scenario(&self, ws: &Workspace, name: &str) -> Result<(String, Scenario), Fail> {
        let node = ws
            .list_scenarios()?
            .into_iter()
            .find(|s| s.name.eq_ignore_ascii_case(name.trim()) || s.id == name)
            .ok_or_else(|| Fail::Invalid(format!("Scenario '{name}' not found (list_scenarios)")))?;
        Ok((node.id.clone(), ws.read_scenario(&node.id)?))
    }

    pub(super) fn tool_list_scenarios(&self) -> Outcome {
        let ws = self.agent_ws()?;
        let scenarios = ws.list_scenarios()?;
        let n = scenarios.len();
        Ok(Done::new(json!({ "scenarios": scenarios }), plural(n, "scenario")))
    }

    pub(super) fn tool_read_scenario(&self, c: &Call<'_>) -> Outcome {
        #[derive(Deserialize)]
        struct A {
            name: String,
        }
        let A { name } = c.args()?;
        let (id, scenario) = self.find_scenario(&self.agent_ws()?, &name)?;
        let values = self.scenario_values(&id);
        Ok(Done::new(json!({ "id": id, "scenario": scenario, "values": values }), name)
            .at(AgentTarget::Scenario { id }))
    }

    pub(super) async fn tool_save_scenario(&self, c: &Call<'_>) -> Outcome {
        #[derive(Deserialize)]
        struct A {
            name: String,
            scenario: Value,
        }
        let A { name, mut scenario } = c.args()?;
        if let Some(s) = scenario.as_object_mut() {
            s.insert("name".into(), json!(name.trim()));
        }
        let unknown = unknown_fields(&scenario);
        if !unknown.is_empty() {
            return Err(Fail::Invalid(format!(
                "scenario: unknown field{} {}. Nothing was saved; the tool's input schema lists every field of each step type.",
                if unknown.len() == 1 { "" } else { "s" },
                unknown.iter().map(|f| format!("`{f}`")).collect::<Vec<_>>().join(", ")
            )));
        }
        let mut scenario: Scenario =
            serde_json::from_value(scenario).map_err(|e| Fail::Invalid(format!("scenario: {e}")))?;
        if let Some(problem) = scenario.problem() {
            return Err(Fail::Invalid(problem));
        }
        let ws = self.agent_ws()?;
        let existing = self.find_scenario(&ws, &name).ok();
        // The scenarios it runs are given by name; the file keeps their ids.
        let listed = ws.list_scenarios()?;
        let called = |call: &str| listed.iter().find(|s| s.name.eq_ignore_ascii_case(call.trim()) || s.id == call);
        if let Some(call) = scenario.calls().into_iter().find(|call| called(call).is_none()) {
            return Err(Fail::Invalid(format!("Scenario '{call}' not found (list_scenarios): save it first")));
        }
        scenario.retarget(&|_| None, &|call| called(call).filter(|s| s.id != call).map(|s| s.id.clone()));
        let missing: Vec<&str> =
            scenario.requests().into_iter().filter(|path| ws.read_request(path).is_err()).collect();
        if !missing.is_empty() {
            return Err(Fail::Invalid(format!(
                "Not in the collection: {}. Steps send saved requests (paths as list_requests shows them): save them first with save_requests.",
                missing.join(", ")
            )));
        }
        if let Some(seq) = existing.as_ref().map(|(_, old)| old.seq) {
            scenario.seq = seq;
        }
        let summary =
            vec![plural(scenario.step_count() as usize, "step"), plural(scenario.requests().len(), "request")];
        self.gate_change(
            c,
            format!("{} scenario “{name}”?", if existing.is_some() { "Update" } else { "Create" }),
            summary,
        )
        .await?;
        let id: String = match existing {
            Some((id, _)) => {
                serde_json::from_value(self.call("scenario.save", json!({ "id": id, "scenario": scenario })).await?)
            }
            None => serde_json::from_value(self.call("scenario.create", json!({ "scenario": scenario })).await?),
        }
        .map_err(|e| Fail::Error(e.to_string()))?;
        let target = AgentTarget::Scenario { id: id.clone() };
        self.show(target.clone());
        Ok(Done::new(json!({ "id": id, "name": name }), format!("Saved {name}")).at(target))
    }

    pub(super) async fn tool_run_scenario(&self, c: &Call<'_>) -> Outcome {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct A {
            name: String,
            #[serde(default)]
            variables: BTreeMap<String, String>,
            to_step: Option<String>,
            only_step: Option<String>,
            wait_seconds: Option<u64>,
        }
        let a: A = c.args()?;
        let ws = self.agent_ws()?;
        let (id, scenario) = self.find_scenario(&ws, &a.name)?;
        // Every request it can send, those of the scenarios it runs too.
        let (requests, _) = crate::scenario::collect(&ws, &scenario);
        let mut paths: Vec<&String> = requests.keys().collect();
        paths.sort();
        let mut hosts = Vec::new();
        for path in paths {
            if let Ok(request) = &requests[path].request {
                for host in self.request_hosts(&ws, request, Some(path)) {
                    if !hosts.contains(&host) {
                        hosts.push(host);
                    }
                }
            }
        }
        let what = format!("Run scenario “{}” ({})", scenario.name, plural(scenario.step_count() as usize, "step"));
        let guard = self.gate_traffic(c, &what, hosts).await?;
        // The run takes the call's scope (approved hosts, no files outside the workspace) along.
        let started = crate::agents::with_guard(
            guard,
            self.scenario_start(crate::scenario::StartParams {
                id: id.clone(),
                scenario: None,
                to: a.to_step,
                only: a.only_step,
                variables: a.variables,
            }),
        )
        .await?;
        self.emit_agent(AgentEvent::ScenarioRun { id: id.clone(), run: started.clone() });
        let target = AgentTarget::Scenario { id: id.clone() };
        let mut done = self.wait_run(c, &started.run_id, a.wait_seconds).await?;
        if let Some(report) = self.finished_run(&started.run_id) {
            let redactor = self.redactor(&ws);
            done.value["steps"] = super::redact_value(&step_results(&report), &redactor);
            done.value["values"] = super::redact_value(&json!(self.scenario_values(&id)), &redactor);
        }
        Ok(done.at(target))
    }
}

/// What each step of a finished scenario run did, in order.
fn step_results(report: &RunReport) -> Value {
    let steps: Vec<Value> = report
        .results
        .iter()
        .take(MAX_STEPS)
        .map(|r| {
            let mut step = json!({ "name": r.name, "passed": r.passed });
            if let Some(info) = &r.step {
                step["step"] = json!(info.path);
                step["type"] = json!(info.kind);
                if let Some(note) = &info.note {
                    step["note"] = json!(note);
                }
                if !info.saved.is_empty() {
                    step["saved"] = info
                        .saved
                        .iter()
                        .map(|s| (s.key.clone(), json!(s.value)))
                        .collect::<serde_json::Map<_, _>>()
                        .into();
                }
            }
            if report.summary.iterations > 1 {
                step["iteration"] = json!(r.iteration + 1);
            }
            if let Some(status) = r.status {
                step["status"] = json!(status);
            }
            if r.skipped {
                step["skipped"] = json!(r.skip_reason);
            }
            if let Some(error) = &r.error {
                step["error"] = json!(error);
            }
            step
        })
        .collect();
    let mut value = json!(steps);
    if report.results.len() > MAX_STEPS {
        value = json!({ "first": steps, "notShown": report.results.len() - MAX_STEPS });
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn misspelled_fields_are_found_at_every_level() {
        let scenario = json!({
            "name": "S", "variables": [{ "key": "a", "value": "1", "secret": true }], "stepz": [],
            "steps": [
                { "type": "request", "request": "a.yaml", "output": [], "outputs": [{ "variable": "x", "pathh": "$.id" }], "checks": [{ "from": "status", "equals": 200 }], "retry": { "times": 1, "wait": 5 } },
                { "type": "if", "when": { "rules": [{ "value": "1", "op": "equals", "too": "1" }], "all": true },
                  "steps": [{ "type": "wait", "ms": 1, "seconds": 2 }],
                  "elseIf": [{ "when": { "rules": [] }, "steps": [{ "type": "set", "values": [{ "key": "k", "value": "v", "x": 1 }] }], "then": [] }],
                  "else": [{ "type": "stop", "reason": "x" }] },
                { "type": "forEach", "list": "a", "steps": [{ "type": "script", "code": "", "source": "" }] },
                { "type": "repeat", "times": 2, "until": { "rules": [], "match": "any" }, "steps": [] },
                { "type": "scenario", "scenario": "Other", "inputs": [{ "key": "a", "value": "b" }], "returns": [] },
                { "type": "ask", "variable": "otp", "prompt": "Code?" },
                { "type": "nonsense", "anything": 1 },
            ],
            "always": [{ "type": "parallel", "steps": [], "limit": 2 }],
        });
        assert_eq!(
            unknown_fields(&scenario),
            [
                "scenario.stepz",
                "scenario.variables[0].secret",
                "scenario.steps[0].output",
                "scenario.steps[0].outputs[0].pathh",
                "scenario.steps[0].checks[0].equals",
                "scenario.steps[0].retry.wait",
                "scenario.steps[1].when.all",
                "scenario.steps[1].when.rules[0].too",
                "scenario.steps[1].elseIf[0].then",
                "scenario.steps[1].elseIf[0].steps[0].values[0].x",
                "scenario.steps[1].else[0].reason",
                "scenario.steps[1].steps[0].seconds",
                "scenario.steps[2].steps[0].source",
                "scenario.steps[3].until.match",
                "scenario.steps[4].returns",
                "scenario.steps[5].prompt",
                "scenario.always[0].limit",
            ]
        );
        let fine = json!({ "steps": [{ "type": "request", "name": "A", "enabled": false, "continueOnFailure": true, "request": "a.yaml",
            "outputs": [{ "variable": "x", "from": "header", "path": "ETag", "secret": true, "optional": true, "collect": true }] }] });
        assert!(unknown_fields(&fine).is_empty());
    }

    #[test]
    fn the_schema_names_every_field_a_step_takes() {
        let schema = scenario_schema();
        let properties = schema["properties"]["steps"]["items"]["properties"].as_object().unwrap();
        for kind in schema["properties"]["steps"]["items"]["properties"]["type"]["enum"].as_array().unwrap() {
            for field in step_fields(kind.as_str().unwrap()).unwrap() {
                assert!(properties.contains_key(*field), "{field} of {kind} is not in the schema");
            }
        }
    }
}

//! Scenarios (`scenarios/<name>.yaml`, also in folders): saved requests run as steps, one after the
//! other, with values passed from one step to the next, checks on what came back,
//! and flow (if, for each, repeat, at the same time). This is the file format and
//! the parts that need no network: comparing values, reading a value out of JSON,
//! and walking the steps.

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use ts_rs::TS;

fn yes() -> bool {
    true
}
fn is_true(v: &bool) -> bool {
    *v
}
fn is_false(v: &bool) -> bool {
    !*v
}
fn is_zero(v: &u64) -> bool {
    *v == 0
}
fn item() -> String {
    "item".into()
}
fn is_item(v: &String) -> bool {
    v == "item"
}
fn once() -> u32 {
    1
}
fn is_json(v: &ValueFrom) -> bool {
    *v == ValueFrom::Json
}
fn is_equals(v: &CompareOp) -> bool {
    *v == CompareOp::Equals
}

/// Text written as text, a number or `true`/`false`: a hand-written `value: 201` is the text `201`.
fn text<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Any {
        Text(String),
        Bool(bool),
        Unsigned(u64),
        Signed(i64),
        Float(f64),
        Nothing(()),
    }
    Ok(match Any::deserialize(d)? {
        Any::Text(s) => s,
        Any::Bool(b) => b.to_string(),
        Any::Unsigned(n) => n.to_string(),
        Any::Signed(n) => n.to_string(),
        Any::Float(n) => n.to_string(),
        Any::Nothing(()) => String::new(),
    })
}

/// A name and its value. The value is text with `{{variables}}`, or JavaScript when it
/// starts with `=` (`= Number(total) + 1`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct StepValue {
    #[serde(default)]
    pub key: String,
    #[serde(default, deserialize_with = "text")]
    pub value: String,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    #[ts(optional, as = "Option<bool>")]
    pub enabled: bool,
}

impl StepValue {
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self { key: key.into(), value: value.into(), enabled: true }
    }
}

/// Where an output or a check finds its value in a response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ValueFrom {
    /// A JSONPath into the body: `$.id`, `$.items[0].id`, `$.items[*].id` (a list).
    #[default]
    Json,
    /// A response header, by name (any case).
    Header,
    /// A cookie the response set, by name.
    Cookie,
    /// A regular expression over the body; its first group is the value.
    Regex,
    /// The status code.
    Status,
    /// The whole body as text.
    Body,
    /// The response time in milliseconds.
    Time,
}

impl ValueFrom {
    /// Whether `path` says where to look (the others take the whole thing).
    pub fn needs_path(self) -> bool {
        matches!(self, ValueFrom::Json | ValueFrom::Header | ValueFrom::Cookie | ValueFrom::Regex)
    }
}

/// Save a value from a step's response for the steps after it (`{{variable}}`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct StepOutput {
    pub variable: String,
    #[serde(default, skip_serializing_if = "is_json")]
    #[ts(optional, as = "Option<ValueFrom>")]
    pub from: ValueFrom,
    /// The JSONPath, the header or cookie name, or the regular expression.
    #[serde(default, deserialize_with = "text", skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub path: String,
    /// Shown as `••••••` in results, the console and to AI agents.
    #[serde(default, skip_serializing_if = "is_false")]
    #[ts(optional, as = "Option<bool>")]
    pub secret: bool,
    /// Finding nothing is fine: the variable is then unset (an output that must be there
    /// fails the step when it is missing).
    #[serde(default, skip_serializing_if = "is_false")]
    #[ts(optional, as = "Option<bool>")]
    pub optional: bool,
    /// Add the value to a list under this name instead of replacing it (a step in a loop
    /// then collects one value per round).
    #[serde(default, skip_serializing_if = "is_false")]
    #[ts(optional, as = "Option<bool>")]
    pub collect: bool,
}

/// How two values are compared (in checks and conditions).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum CompareOp {
    #[default]
    Equals,
    NotEquals,
    Contains,
    NotContains,
    /// A regular expression.
    Matches,
    Greater,
    GreaterOrEqual,
    Less,
    LessOrEqual,
    /// There is a value (not missing, `null` or empty text).
    Exists,
    NotExists,
    /// One of a list: a JSON array, or values separated by commas.
    In,
    NotIn,
    IsTrue,
    IsFalse,
}

impl CompareOp {
    /// Whether the comparison has a right-hand side.
    pub fn takes_value(self) -> bool {
        !matches!(self, CompareOp::Exists | CompareOp::NotExists | CompareOp::IsTrue | CompareOp::IsFalse)
    }

    /// For messages: "equals", "is greater than".
    pub fn words(self) -> &'static str {
        match self {
            CompareOp::Equals => "equals",
            CompareOp::NotEquals => "does not equal",
            CompareOp::Contains => "contains",
            CompareOp::NotContains => "does not contain",
            CompareOp::Matches => "matches",
            CompareOp::Greater => "is greater than",
            CompareOp::GreaterOrEqual => "is at least",
            CompareOp::Less => "is less than",
            CompareOp::LessOrEqual => "is at most",
            CompareOp::Exists => "exists",
            CompareOp::NotExists => "does not exist",
            CompareOp::In => "is one of",
            CompareOp::NotIn => "is none of",
            CompareOp::IsTrue => "is true",
            CompareOp::IsFalse => "is false",
        }
    }
}

/// A check on a step's response; each one is a test in the results.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct StepCheck {
    #[serde(default, skip_serializing_if = "is_json")]
    #[ts(optional, as = "Option<ValueFrom>")]
    pub from: ValueFrom,
    #[serde(default, deserialize_with = "text", skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub path: String,
    #[serde(default, skip_serializing_if = "is_equals")]
    #[ts(optional, as = "Option<CompareOp>")]
    pub op: CompareOp,
    /// What it is compared with: text with `{{variables}}`, or JavaScript after `=`.
    #[serde(default, deserialize_with = "text", skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub value: String,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    #[ts(optional, as = "Option<bool>")]
    pub enabled: bool,
}

/// One comparison of a condition: `value` `op` `to`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Rule {
    /// Text with `{{variables}}`, or JavaScript after `=`.
    #[serde(default, deserialize_with = "text")]
    pub value: String,
    #[serde(default, skip_serializing_if = "is_equals")]
    #[ts(optional, as = "Option<CompareOp>")]
    pub op: CompareOp,
    #[serde(default, deserialize_with = "text", skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub to: String,
}

/// Rules that must all hold (or any one of them, with `any`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Condition {
    #[serde(default, skip_serializing_if = "is_false")]
    #[ts(optional, as = "Option<bool>")]
    pub any: bool,
    #[serde(default)]
    pub rules: Vec<Rule>,
}

/// Send again when the step fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Retry {
    /// More tries after the first one.
    pub times: u32,
    /// Pause before each of them.
    #[serde(default, skip_serializing_if = "is_zero")]
    #[ts(optional, as = "Option<u32>")]
    pub delay_ms: u64,
}

/// Send a saved request.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct StepRequest {
    /// Request path relative to `requests/`.
    pub request: String,
    /// Values for this step only (they win over every other variable).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, as = "Option<Vec<StepValue>>")]
    pub inputs: Vec<StepValue>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, as = "Option<Vec<StepOutput>>")]
    pub outputs: Vec<StepOutput>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, as = "Option<Vec<StepCheck>>")]
    pub checks: Vec<StepCheck>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub retry: Option<Retry>,
}

/// Another branch of an `if`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Branch {
    pub when: Condition,
    #[serde(default)]
    pub steps: Vec<Step>,
}

/// Run `steps` when the condition holds; else the first `else_if` whose condition holds; else `otherwise`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct StepIf {
    pub when: Condition,
    #[serde(default)]
    pub steps: Vec<Step>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, as = "Option<Vec<Branch>>")]
    pub else_if: Vec<Branch>,
    #[serde(default, rename = "else", skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, as = "Option<Vec<Step>>")]
    pub otherwise: Vec<Step>,
}

/// Run `steps` once per item of a list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct StepForEach {
    /// The list: `{{variable}}` holding one, a JSON array, values separated by commas, or JavaScript after `=`.
    #[serde(default, deserialize_with = "text")]
    pub list: String,
    /// The variable each item is in (`{{item}}`, `{{item.id}}`); `{{index}}` counts from 0.
    #[serde(default = "item", rename = "as", skip_serializing_if = "is_item")]
    #[ts(optional, as = "Option<String>")]
    pub item: String,
    #[serde(default)]
    pub steps: Vec<Step>,
}

impl Default for StepForEach {
    fn default() -> Self {
        Self { list: String::new(), item: item(), steps: Vec::new() }
    }
}

/// Run `steps` several times: `times` rounds, or until the condition holds (checked after
/// each round; the step fails when it never does within `times` rounds).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct StepRepeat {
    #[serde(default = "once")]
    pub times: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub until: Option<Condition>,
    /// Pause between two rounds.
    #[serde(default, skip_serializing_if = "is_zero")]
    #[ts(optional, as = "Option<u32>")]
    pub delay_ms: u64,
    #[serde(default)]
    pub steps: Vec<Step>,
}

impl Default for StepRepeat {
    fn default() -> Self {
        Self { times: 1, until: None, delay_ms: 0, steps: Vec::new() }
    }
}

/// Run `steps` at the same time. Each gets its own copy of the values; what they saved is
/// put together when all have finished (the later step wins a name both saved).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct StepParallel {
    #[serde(default)]
    pub steps: Vec<Step>,
}

/// Set values without sending anything.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct StepSet {
    #[serde(default)]
    pub values: Vec<StepValue>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct StepWait {
    #[ts(type = "number")]
    pub ms: u64,
}

/// What a stop step ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum StopWhat {
    /// The loop it is in (for each, repeat); the steps after the loop go on.
    Loop,
    /// The scenario, as passed.
    #[default]
    Scenario,
    /// The scenario, as failed.
    Fail,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct StepStop {
    #[serde(default)]
    pub what: StopWhat,
    /// Why (shown in the results); may use `{{variables}}`.
    #[serde(default, deserialize_with = "text", skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub message: String,
}

/// Run another scenario with its own values.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct StepCall {
    /// The scenario's id: its path under `scenarios/` without `.yaml` (`Shop/Log in`).
    pub scenario: String,
    /// Values it starts with (over its own variables).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, as = "Option<Vec<StepValue>>")]
    pub inputs: Vec<StepValue>,
    /// Values to bring back: each `value` is read with the other scenario's values
    /// (`token` = `{{token}}`) and saved here under `key`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, as = "Option<Vec<StepValue>>")]
    pub outputs: Vec<StepValue>,
}

/// JavaScript with the `pm` API (`pm.variables`, `pm.test`, `pm.sendRequest`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct StepScript {
    #[serde(default)]
    pub code: String,
}

/// Pause and ask the person running the scenario for a value (a one-time code). Runs
/// nobody watches (the command line, AI agents) must give it up front.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct StepAsk {
    pub variable: String,
    #[serde(default, deserialize_with = "text", skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub message: String,
    #[serde(default, skip_serializing_if = "is_false")]
    #[ts(optional, as = "Option<bool>")]
    pub secret: bool,
}

/// What a step does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(export)]
pub enum StepAction {
    Request(StepRequest),
    If(StepIf),
    ForEach(StepForEach),
    Repeat(StepRepeat),
    Parallel(StepParallel),
    Set(StepSet),
    Wait(StepWait),
    Stop(StepStop),
    Scenario(StepCall),
    Script(StepScript),
    Ask(StepAsk),
}

/// The kinds of step, by name (results say which one they are).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum StepKind {
    #[default]
    Request,
    If,
    ForEach,
    Repeat,
    Parallel,
    Set,
    Wait,
    Stop,
    Scenario,
    Script,
    Ask,
    /// One round of a loop (results only).
    Round,
}

impl StepKind {
    /// Steps that only hold or direct other steps: they aren't counted as steps of their own.
    pub fn is_flow(self) -> bool {
        matches!(
            self,
            StepKind::If
                | StepKind::ForEach
                | StepKind::Repeat
                | StepKind::Parallel
                | StepKind::Scenario
                | StepKind::Round
        )
    }
}

/// One step of a scenario.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Step {
    /// Shown in the list and the results (the request's name, or the kind's, when empty).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub name: String,
    /// Switched-off steps are left out of runs.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    #[ts(optional, as = "Option<bool>")]
    pub enabled: bool,
    /// Go on with the next step when this one fails (the scenario still fails).
    #[serde(default, skip_serializing_if = "is_false")]
    #[ts(optional, as = "Option<bool>")]
    pub continue_on_failure: bool,
    #[serde(flatten)]
    #[ts(flatten)]
    pub action: StepAction,
}

impl Step {
    pub fn new(action: StepAction) -> Self {
        Self { name: String::new(), enabled: true, continue_on_failure: false, action }
    }

    pub fn request(path: impl Into<String>) -> Self {
        Self::new(StepAction::Request(StepRequest { request: path.into(), ..Default::default() }))
    }

    pub fn kind(&self) -> StepKind {
        match &self.action {
            StepAction::Request(_) => StepKind::Request,
            StepAction::If(_) => StepKind::If,
            StepAction::ForEach(_) => StepKind::ForEach,
            StepAction::Repeat(_) => StepKind::Repeat,
            StepAction::Parallel(_) => StepKind::Parallel,
            StepAction::Set(_) => StepKind::Set,
            StepAction::Wait(_) => StepKind::Wait,
            StepAction::Stop(_) => StepKind::Stop,
            StepAction::Scenario(_) => StepKind::Scenario,
            StepAction::Script(_) => StepKind::Script,
            StepAction::Ask(_) => StepKind::Ask,
        }
    }

    /// The lists of steps inside this one, each with its part of a step path
    /// (`steps`, `elseIf/0`, `else`).
    pub fn children(&self) -> Vec<(String, &[Step])> {
        match &self.action {
            StepAction::If(s) => {
                let mut out = vec![("steps".to_string(), s.steps.as_slice())];
                out.extend(s.else_if.iter().enumerate().map(|(i, b)| (format!("elseIf/{i}"), b.steps.as_slice())));
                out.push(("else".to_string(), s.otherwise.as_slice()));
                out
            }
            StepAction::ForEach(s) => vec![("steps".to_string(), s.steps.as_slice())],
            StepAction::Repeat(s) => vec![("steps".to_string(), s.steps.as_slice())],
            StepAction::Parallel(s) => vec![("steps".to_string(), s.steps.as_slice())],
            _ => Vec::new(),
        }
    }

    fn children_mut(&mut self) -> Vec<&mut Vec<Step>> {
        match &mut self.action {
            StepAction::If(s) => {
                let mut out = vec![&mut s.steps];
                out.extend(s.else_if.iter_mut().map(|b| &mut b.steps));
                out.push(&mut s.otherwise);
                out
            }
            StepAction::ForEach(s) => vec![&mut s.steps],
            StepAction::Repeat(s) => vec![&mut s.steps],
            StepAction::Parallel(s) => vec![&mut s.steps],
            _ => Vec::new(),
        }
    }
}

/// Steps nested deeper than this are refused (a file from Git could nest without end).
pub const MAX_STEP_DEPTH: usize = 16;

/// A saved scenario.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Scenario {
    pub name: String,
    /// Sort position in the sidebar.
    #[serde(default)]
    pub seq: u32,
    /// The values a run starts with. They, and everything steps save, belong to the
    /// scenario: environments and workspace variables are read, never changed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, as = "Option<Vec<StepValue>>")]
    pub variables: Vec<StepValue>,
    #[serde(default)]
    pub steps: Vec<Step>,
    /// Steps that run at the end whatever happened before (clean up).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, as = "Option<Vec<Step>>")]
    pub always: Vec<Step>,
    /// CSV or JSON data file (relative to the workspace folder, or absolute): the
    /// scenario runs once per row, the columns as variables.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub data_file: Option<String>,
    /// How many times it runs (default: once per data row, or once).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub iterations: Option<u32>,
    /// Go on after a failed step (default: a failed step ends the run; `always` still runs).
    #[serde(default, skip_serializing_if = "is_false")]
    #[ts(optional, as = "Option<bool>")]
    pub continue_on_failure: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub docs: String,
}

impl Scenario {
    /// A scenario that sends `requests` in order.
    pub fn new(name: impl Into<String>, requests: Vec<String>) -> Self {
        Self { name: name.into(), steps: requests.into_iter().map(Step::request).collect(), ..Default::default() }
    }

    /// Every step with its path (`steps/2`, `steps/2/else/0`, `always/0`), outer steps first.
    pub fn walk<'a>(&'a self, visit: &mut dyn FnMut(&str, &'a Step)) {
        fn go<'a>(prefix: &str, steps: &'a [Step], visit: &mut dyn FnMut(&str, &'a Step)) {
            for (i, step) in steps.iter().enumerate() {
                let path = format!("{prefix}/{i}");
                visit(&path, step);
                for (part, children) in step.children() {
                    go(&format!("{path}/{part}"), children, visit);
                }
            }
        }
        go("steps", &self.steps, visit);
        go("always", &self.always, visit);
    }

    /// The step at a path [`Self::walk`] gives.
    pub fn step_at(&self, path: &str) -> Option<&Step> {
        let mut found = None;
        self.walk(&mut |p, step| {
            if p == path {
                found = Some(step);
            }
        });
        found
    }

    /// How many steps it has, at every level.
    pub fn step_count(&self) -> u32 {
        let mut n = 0;
        self.walk(&mut |_, _| n += 1);
        n
    }

    /// The request paths its steps send, each once, in order.
    pub fn requests(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        self.walk(&mut |_, step| {
            if let StepAction::Request(r) = &step.action
                && !out.contains(&r.request.as_str())
            {
                out.push(&r.request);
            }
        });
        out
    }

    /// The scenarios its steps run, each once.
    pub fn calls(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        self.walk(&mut |_, step| {
            if let StepAction::Scenario(c) = &step.action
                && !out.contains(&c.scenario.as_str())
            {
                out.push(&c.scenario);
            }
        });
        out
    }

    /// Change the request paths (`rewrite` gives the new one, or `None` to keep it) and
    /// scenario ids (`rewrite_call`). Returns whether anything changed.
    pub fn retarget(
        &mut self,
        rewrite: &dyn Fn(&str) -> Option<String>,
        rewrite_call: &dyn Fn(&str) -> Option<String>,
    ) -> bool {
        fn go(
            steps: &mut [Step],
            rewrite: &dyn Fn(&str) -> Option<String>,
            rewrite_call: &dyn Fn(&str) -> Option<String>,
        ) -> bool {
            let mut changed = false;
            for step in steps {
                match &mut step.action {
                    StepAction::Request(r) => {
                        if let Some(path) = rewrite(&r.request) {
                            r.request = path;
                            changed = true;
                        }
                    }
                    StepAction::Scenario(c) => {
                        if let Some(id) = rewrite_call(&c.scenario) {
                            c.scenario = id;
                            changed = true;
                        }
                    }
                    _ => {}
                }
                for children in step.children_mut() {
                    changed |= go(children, rewrite, rewrite_call);
                }
            }
            changed
        }
        let a = go(&mut self.steps, rewrite, rewrite_call);
        let b = go(&mut self.always, rewrite, rewrite_call);
        a || b
    }

    /// What makes the scenario unusable, if anything: steps nested too deep, a name that
    /// can't be a variable. (What a run needs, such as its requests, is checked when it starts.)
    pub fn problem(&self) -> Option<String> {
        fn depth(steps: &[Step]) -> usize {
            steps
                .iter()
                .map(|s| 1 + s.children().into_iter().map(|(_, c)| depth(c)).max().unwrap_or(0))
                .max()
                .unwrap_or(0)
        }
        if depth(&self.steps).max(depth(&self.always)) > MAX_STEP_DEPTH {
            return Some(format!("Steps are nested more than {MAX_STEP_DEPTH} levels deep"));
        }
        let mut problem = None;
        // A row without a name is left out when the scenario runs: only a name that can't be
        // one is refused (a scenario being written is saved with rows not filled in yet).
        let mut name = |what: &str, name: &str| {
            if problem.is_none()
                && !name.trim().is_empty()
                && let Err(e) = variable_name(name)
            {
                problem = Some(format!("{what}: {e}"));
            }
        };
        for v in self.variables.iter().filter(|v| v.enabled) {
            name("Variables", &v.key);
        }
        self.walk(&mut |_, step| match &step.action {
            StepAction::Request(r) => {
                for o in &r.outputs {
                    name("An output", &o.variable);
                }
                for i in r.inputs.iter().filter(|i| i.enabled) {
                    name("An input", &i.key);
                }
            }
            StepAction::Set(s) => {
                for v in s.values.iter().filter(|v| v.enabled) {
                    name("Set values", &v.key);
                }
            }
            StepAction::ForEach(f) => name("For each", &f.item),
            StepAction::Ask(a) => name("Ask for a value", &a.variable),
            StepAction::Scenario(c) => {
                for v in c.inputs.iter().chain(&c.outputs).filter(|v| v.enabled) {
                    name("Run scenario", &v.key);
                }
            }
            _ => {}
        });
        problem
    }
}

/// Whether `name` can be a variable (`{{name}}`): not empty, no spaces or braces.
pub fn variable_name(name: &str) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("a value has no name".into());
    }
    if name.contains(['{', '}']) || name.chars().any(char::is_whitespace) {
        return Err(format!("'{name}' can't be a variable name (no spaces or braces)"));
    }
    Ok(())
}

/// A saved scenario, or a folder of them, in the sidebar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ScenarioNode {
    /// Its path under `scenarios/`, without `.yaml` for a scenario: `Checkout`,
    /// `Shop/Checkout` (`scenarios/Shop/Checkout.yaml`), or the folder `Shop`.
    pub id: String,
    pub name: String,
    /// A folder: what is in it is in `children`.
    #[serde(default, skip_serializing_if = "is_false")]
    #[ts(optional, as = "Option<bool>")]
    pub folder: bool,
    /// Steps at every level (a scenario's).
    pub steps: u32,
    pub seq: u32,
    /// Set when the file (a folder's `_folder.yaml`) could not be parsed.
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// A folder's scenarios and folders, in sidebar order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, as = "Option<Vec<ScenarioNode>>")]
    pub children: Vec<ScenarioNode>,
}

// ---- values ---------------------------------------------------------------------------------

/// A value as text: strings as they are, `null` as nothing, numbers as JavaScript writes
/// them (`7.0` is `7`), lists and objects as JSON.
pub fn value_text(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Number(n) if !n.is_i64() && !n.is_u64() => n.as_f64().map(|f| f.to_string()).unwrap_or_default(),
        other => other.to_string(),
    }
}

/// The number a value is, or its text reads as.
fn number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok().filter(|n| n.is_finite()),
        _ => None,
    }
}

/// Equal as numbers when both are, else as text (`201` equals `"201"`, `true` equals `"true"`).
fn same(a: &Value, b: &Value) -> bool {
    match (number(a), number(b)) {
        (Some(x), Some(y)) => x == y,
        _ => value_text(a) == value_text(b),
    }
}

/// Whether a value counts as true: not `false`, `0`, `null` or empty (as text too).
pub fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => {
            !matches!(s.trim().to_ascii_lowercase().as_str(), "" | "false" | "0" | "null" | "undefined")
        }
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// The items of a list given as a value: a JSON array (also as text), or text with commas.
pub fn list_items(value: &Value) -> Vec<Value> {
    match value {
        Value::Array(items) => items.clone(),
        Value::Null => Vec::new(),
        Value::String(s) => {
            let s = s.trim();
            if s.is_empty() {
                return Vec::new();
            }
            if s.starts_with('[')
                && let Ok(Value::Array(items)) = serde_json::from_str(s)
            {
                return items;
            }
            s.split(',').map(|part| Value::String(part.trim().to_string())).filter(|v| v != "").collect()
        }
        other => vec![other.clone()],
    }
}

/// Whether `left` `op` `right` holds. `left` is `None` when there is no value at all (a
/// path that found nothing, an undefined variable). Errors say what can't be compared.
pub fn compare(op: CompareOp, left: Option<&Value>, right: &Value) -> Result<bool, String> {
    let exists = left.is_some_and(|v| !v.is_null() && !matches!(v, Value::String(s) if s.is_empty()));
    let null = Value::Null;
    let value = left.unwrap_or(&null);
    let numbers = || match (number(value), number(right)) {
        (Some(a), Some(b)) => Ok((a, b)),
        (None, _) => Err(format!("'{}' is not a number", short(&value_text(value)))),
        (_, None) => Err(format!("'{}' is not a number", short(&value_text(right)))),
    };
    let contains = || match value {
        Value::Array(items) => items.iter().any(|i| same(i, right)),
        Value::Object(map) => map.contains_key(&value_text(right)),
        other => value_text(other).contains(&value_text(right)),
    };
    Ok(match op {
        CompareOp::Equals => same(value, right),
        CompareOp::NotEquals => !same(value, right),
        CompareOp::Contains => contains(),
        CompareOp::NotContains => !contains(),
        CompareOp::Matches => {
            let pattern = value_text(right);
            let re = regex::RegexBuilder::new(&pattern)
                .size_limit(1 << 20)
                .build()
                .map_err(|e| format!("'{}' is not a regular expression: {e}", short(&pattern)))?;
            re.is_match(&value_text(value))
        }
        CompareOp::Greater => numbers().map(|(a, b)| a > b)?,
        CompareOp::GreaterOrEqual => numbers().map(|(a, b)| a >= b)?,
        CompareOp::Less => numbers().map(|(a, b)| a < b)?,
        CompareOp::LessOrEqual => numbers().map(|(a, b)| a <= b)?,
        CompareOp::Exists => exists,
        CompareOp::NotExists => !exists,
        CompareOp::In => list_items(right).iter().any(|i| same(value, i)),
        CompareOp::NotIn => !list_items(right).iter().any(|i| same(value, i)),
        CompareOp::IsTrue => truthy(value),
        CompareOp::IsFalse => !truthy(value),
    })
}

/// The start of a long text, for messages.
pub fn short(text: &str) -> String {
    const MAX: usize = 80;
    match text.char_indices().nth(MAX) {
        Some((at, _)) => format!("{}…", &text[..at]),
        None => text.to_string(),
    }
}

/// A JSONPath (RFC 9535) ready to read values with. A path that can match several values
/// (`*`, `..`, a filter, a slice or several names) gives them as a list; one that names a
/// single place gives that value.
pub struct JsonPick {
    path: serde_json_path::JsonPath,
    many: bool,
}

impl JsonPick {
    /// `path` may leave out the leading `$` (`items[0].id`, `.id`).
    pub fn parse(path: &str) -> Result<Self, String> {
        let path = path.trim();
        if path.is_empty() {
            return Err("give a JSON path, e.g. $.id".into());
        }
        let full = match path {
            p if p.starts_with('$') => p.to_string(),
            p if p.starts_with(['.', '[']) => format!("${p}"),
            p => format!("$.{p}"),
        };
        let parsed = serde_json_path::JsonPath::parse(&full)
            .map_err(|e| format!("'{path}' is not a JSON path ({e}); use one like $.items[0].id"))?;
        Ok(Self { path: parsed, many: matches_many(&full) })
    }

    /// The value at the path: `None` when nothing is there (an empty list for a path that gives lists).
    pub fn get(&self, root: &Value) -> Option<Value> {
        let found = self.path.query(root).all();
        if self.many {
            return Some(Value::Array(found.into_iter().cloned().collect()));
        }
        found.first().map(|v| (*v).clone())
    }
}

/// Whether a JSONPath can match more than one value: it has a wildcard, `..`, a filter, a
/// slice or a list of names outside of quoted names.
fn matches_many(path: &str) -> bool {
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut in_brackets = 0usize;
    let mut previous = ' ';
    for c in path.chars() {
        match quote {
            Some(_) if escaped => escaped = false,
            Some(_) if c == '\\' => escaped = true,
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None => match c {
                '\'' | '"' => quote = Some(c),
                '[' => in_brackets += 1,
                ']' => in_brackets = in_brackets.saturating_sub(1),
                '*' | '?' => return true,
                '.' if previous == '.' => return true,
                ':' | ',' if in_brackets > 0 => return true,
                _ => {}
            },
        }
        previous = c;
    }
    false
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn yaml_round_trip_and_defaults() {
        let yaml = r#"
name: Checkout
variables:
  - { key: productId, value: 42 }
steps:
  - type: request
    request: Cart/Create cart.yaml
    outputs:
      - { variable: cartId, path: $.id }
      - { variable: token, from: header, path: X-Token, secret: true }
    checks:
      - { from: status, value: 201 }
  - type: forEach
    list: "{{products}}"
    as: product
    steps:
      - type: request
        name: Add
        request: Cart/Add item.yaml
        inputs:
          - { key: qty, value: 2 }
        retry: { times: 2, delayMs: 100 }
  - type: if
    when: { rules: [{ value: "{{total}}", op: greater, to: 100 }] }
    steps:
      - { type: wait, ms: 10 }
    elseIf:
      - when: { any: true, rules: [{ value: "= total < 0", op: isTrue }] }
        steps:
          - { type: stop, what: fail, message: negative }
    else:
      - { type: set, values: [{ key: cheap, value: yes }] }
always:
  - { type: script, code: "pm.test('done', () => {})", continueOnFailure: true }
"#;
        let s: Scenario = serde_yaml_ng::from_str(yaml).unwrap();
        assert_eq!(s.variables, [StepValue::new("productId", "42")]);
        assert_eq!(s.step_count(), 8);
        assert_eq!(s.requests(), ["Cart/Create cart.yaml", "Cart/Add item.yaml"]);
        let StepAction::Request(create) = &s.steps[0].action else { panic!() };
        assert_eq!((create.outputs[0].from, create.outputs[1].from), (ValueFrom::Json, ValueFrom::Header));
        assert!(create.outputs[1].secret && !create.outputs[0].optional);
        assert_eq!(
            (create.checks[0].from, create.checks[0].op, create.checks[0].value.as_str()),
            (ValueFrom::Status, CompareOp::Equals, "201")
        );
        let StepAction::ForEach(each) = &s.steps[1].action else { panic!() };
        assert_eq!(each.item, "product");
        let StepAction::Request(add) = &each.steps[0].action else { panic!() };
        assert_eq!((add.inputs[0].value.as_str(), add.retry), ("2", Some(Retry { times: 2, delay_ms: 100 })));
        let StepAction::If(branch) = &s.steps[2].action else { panic!() };
        assert_eq!(branch.when.rules[0].to, "100");
        assert!(branch.else_if[0].when.any && branch.otherwise.len() == 1);
        assert!(s.always[0].continue_on_failure);
        assert_eq!(s.problem(), None);

        let again = serde_yaml_ng::to_string(&s).unwrap();
        assert_eq!(serde_yaml_ng::from_str::<Scenario>(&again).unwrap(), s);
        // Defaults stay out of the file.
        for absent in ["enabled", "optional", "collect", "dataFile", "docs", "seq: 1", "as: item", "from: json"] {
            assert!(!again.contains(absent), "{absent} in\n{again}");
        }
        // And through JSON, as the app and agents send it.
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["steps"][1]["type"], "forEach");
        assert_eq!(serde_json::from_value::<Scenario>(json).unwrap(), s);
    }

    #[test]
    fn paths_find_steps() {
        let mut s = Scenario::new("S", vec!["a.yaml".into()]);
        s.steps.push(Step::new(StepAction::If(StepIf {
            when: Condition::default(),
            steps: vec![Step::request("b.yaml")],
            else_if: vec![Branch { when: Condition::default(), steps: vec![Step::request("c.yaml")] }],
            otherwise: vec![Step::request("d.yaml")],
        })));
        s.always.push(Step::request("a.yaml"));
        let mut paths = Vec::new();
        s.walk(&mut |p, _| paths.push(p.to_string()));
        assert_eq!(
            paths,
            ["steps/0", "steps/1", "steps/1/steps/0", "steps/1/elseIf/0/0", "steps/1/else/0", "always/0"]
        );
        fn at(s: &Scenario, p: &str) -> String {
            match &s.step_at(p).unwrap().action {
                StepAction::Request(r) => r.request.clone(),
                _ => "if".into(),
            }
        }
        assert_eq!(
            (at(&s, "steps/1/elseIf/0/0"), at(&s, "steps/1/else/0"), at(&s, "always/0")),
            ("c.yaml".into(), "d.yaml".into(), "a.yaml".into())
        );
        assert!(s.step_at("steps/9").is_none());
        assert_eq!(s.requests(), ["a.yaml", "b.yaml", "c.yaml", "d.yaml"]);

        let moved = |p: &str| (p == "c.yaml").then(|| "x/c.yaml".to_string());
        assert!(s.retarget(&moved, &|_| None));
        assert!(!s.retarget(&moved, &|_| None));
        assert_eq!(at(&s, "steps/1/elseIf/0/0"), "x/c.yaml");
    }

    #[test]
    fn problems() {
        let mut s = Scenario::new("S", vec![]);
        s.variables.push(StepValue::new("my var", "1"));
        assert_eq!(s.problem().unwrap(), "Variables: 'my var' can't be a variable name (no spaces or braces)");
        s.variables[0].enabled = false;
        assert_eq!(s.problem(), None);
        // A row without a name is left out of runs, so it is no problem.
        s.variables.push(StepValue::new(" ", "not named yet"));
        s.steps.push(Step::new(StepAction::Request(StepRequest {
            request: "a.yaml".into(),
            outputs: vec![StepOutput::default()],
            ..Default::default()
        })));
        assert_eq!(s.problem(), None);
        let mut step = Step::request("a.yaml");
        for _ in 0..MAX_STEP_DEPTH {
            step = Step::new(StepAction::Parallel(StepParallel { steps: vec![step] }));
        }
        s.steps.push(step);
        assert!(s.problem().unwrap().contains("nested"));
    }

    #[test]
    fn comparisons() {
        let ok = |op, left: Value, right: Value| compare(op, Some(&left), &right).unwrap();
        assert!(ok(CompareOp::Equals, json!(201), json!("201")));
        assert!(ok(CompareOp::Equals, json!("2.0"), json!(2)));
        assert!(ok(CompareOp::Equals, json!(true), json!("true")));
        assert!(ok(CompareOp::NotEquals, json!("a"), json!("b")));
        assert!(ok(CompareOp::Contains, json!("hello world"), json!("lo w")));
        assert!(ok(CompareOp::Contains, json!([1, 2, 3]), json!("2")));
        assert!(ok(CompareOp::Contains, json!({"a": 1}), json!("a")));
        assert!(ok(CompareOp::NotContains, json!([1, 2]), json!(5)));
        assert!(ok(CompareOp::Matches, json!("order-123"), json!(r"^order-\d+$")));
        assert!(ok(CompareOp::Greater, json!("10"), json!(9)));
        assert!(!ok(CompareOp::Greater, json!(9), json!(9)));
        assert!(ok(CompareOp::GreaterOrEqual, json!(9), json!(9)));
        assert!(ok(CompareOp::Less, json!(1.5), json!("2")));
        assert!(ok(CompareOp::LessOrEqual, json!(2), json!(2)));
        assert!(ok(CompareOp::In, json!("b"), json!("a, b, c")));
        assert!(ok(CompareOp::In, json!(2), json!([1, 2])));
        assert!(ok(CompareOp::In, json!("x"), json!("[\"x\", \"y\"]")));
        assert!(ok(CompareOp::NotIn, json!("z"), json!("a,b")));
        assert!(ok(CompareOp::IsTrue, json!("yes"), Value::Null));
        assert!(ok(CompareOp::IsFalse, json!("false"), Value::Null));
        assert!(ok(CompareOp::IsFalse, json!(0), Value::Null));
        assert!(ok(CompareOp::Exists, json!(0), Value::Null));
        assert!(ok(CompareOp::Exists, json!([]), Value::Null));
        assert!(ok(CompareOp::NotExists, json!(""), Value::Null));
        assert!(ok(CompareOp::NotExists, Value::Null, Value::Null));
        // Nothing there at all.
        assert!(compare(CompareOp::NotExists, None, &Value::Null).unwrap());
        assert!(!compare(CompareOp::Equals, None, &json!("x")).unwrap());
        assert!(compare(CompareOp::IsFalse, None, &Value::Null).unwrap());
        // What can't be compared says so.
        assert_eq!(compare(CompareOp::Greater, Some(&json!("abc")), &json!(1)).unwrap_err(), "'abc' is not a number");
        assert!(
            compare(CompareOp::Matches, Some(&json!("a")), &json!("(")).unwrap_err().contains("regular expression")
        );
    }

    #[test]
    fn json_paths_give_one_value_or_a_list() {
        let body = json!({"id": 7, "items": [{"id": 1, "tags": ["a"]}, {"id": 2, "tags": []}], "odd key": {"a.b": true}, "none": null});
        let get = |path: &str| JsonPick::parse(path).unwrap().get(&body);
        assert_eq!(get("$.id"), Some(json!(7)));
        assert_eq!(get("id"), Some(json!(7)));
        assert_eq!(get(".items[1].id"), Some(json!(2)));
        assert_eq!(get("$.items[-1].id"), Some(json!(2)));
        assert_eq!(get("$['odd key']['a.b']"), Some(json!(true)));
        assert_eq!(get("$.missing"), None);
        assert_eq!(get("$.none"), Some(Value::Null));
        assert_eq!(get("$.items"), Some(body["items"].clone()));
        // Paths that can match several values always give a list.
        assert_eq!(get("$.items[*].id"), Some(json!([1, 2])));
        assert_eq!(get("$..id"), Some(json!([7, 1, 2])));
        assert_eq!(get("$.items[?@.id > 1].id"), Some(json!([2])));
        assert_eq!(get("$.items[?@.id > 5].id"), Some(json!([])));
        assert_eq!(get("$.items[0:1]"), Some(json!([{"id": 1, "tags": ["a"]}])));
        // A name with a star or dots in quotes is still one place.
        assert!(!matches_many("$['a*b']['c..d']"));
        assert!(JsonPick::parse("$.items[").is_err());
        assert!(JsonPick::parse(" ").is_err());
    }

    #[test]
    fn values_as_text_and_lists() {
        assert_eq!(value_text(&json!(7.0)), "7");
        assert_eq!(value_text(&json!(1.5)), "1.5");
        assert_eq!(value_text(&json!({"a": [1]})), "{\"a\":[1]}");
        assert_eq!(value_text(&Value::Null), "");
        assert_eq!(list_items(&json!("a, b ,,c")), [json!("a"), json!("b"), json!("c")]);
        assert_eq!(list_items(&json!("[1, {\"a\": 2}]")), [json!(1), json!({"a": 2})]);
        assert_eq!(list_items(&json!("")), Vec::<Value>::new());
        assert_eq!(list_items(&json!(5)), [json!(5)]);
        assert_eq!(short(&"x".repeat(100)).chars().count(), 81);
    }
}

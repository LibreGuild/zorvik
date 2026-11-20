//! Scenarios (docs/architecture.md, "Scenarios"): the steps of a saved scenario run in
//! order. A request step goes through the collection runner's send ([`send_item`]), so
//! scripts, tests and reports work the same; what is new here are the values that go
//! from one step to the next, the checks, and the flow (if, for each, repeat, at the
//! same time, another scenario). [`run`] is shared by the app (`scenario.*`), `zorvik run
//! --scenario` and AI agents.
//!
//! A scenario has its own values: its variables, what steps save, what scripts set.
//! They are data (never expanded as templates) and sit above the environment, the
//! workspace variables and globals, which a scenario reads but never changes.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;
use ts_rs::TS;
use zorvik_engine::HttpResponse;
use zorvik_script::{ConsoleEntry, ConsoleLevel, Event, Info, Limits, ScriptOutput, ScriptRequest, TestResult};
use zorvik_workspace::formats::scenario::{JsonPick, compare, list_items, short, value_text, variable_name};
use zorvik_workspace::formats::{
    Condition, Rule, Scenario, Step, StepAction, StepAsk, StepCall, StepCheck, StepForEach, StepIf, StepKind,
    StepParallel, StepRepeat, StepRequest, StepScript, StepSet, StepStop, StepWait, StopWhat, ValueFrom,
};
use zorvik_workspace::{ScenarioItem, Workspace};

use crate::runner::{
    AgentRun, DataRow, IterationSummary, MAX_DELAY_MS, MAX_ITERATIONS, MAX_KEPT_RESULTS, MAX_STEPS, RunEvent, RunItem,
    RunReport, RunResult, RunSlot, RunStarted, RunSummary, SavedValue, Sending, StepInfo, bounded_console, count,
    iteration_count, keep, mask_secrets, plural, read_data_file, secret_values, send_item, sendable, verdict,
};
use crate::scripting::{script_host, script_input};
use crate::{Api, ApiError, ApiResult, Iteration, ScriptVars, SendContext, StreamEvent, lock, ok, params};

/// Scenarios that may run each other this deep (one that runs itself ends here).
const MAX_CALL_DEPTH: usize = 8;
/// Scenarios one run may read (those it runs, and theirs).
const MAX_SCENARIOS: usize = 200;
/// Steps that may run at the same time (each is a request in flight).
const MAX_PARALLEL: usize = 100;
/// Tries a request step gets with "try again" at most.
const MAX_TRIES: u32 = 100;
/// The longest a value a step saves may be (bytes of text): values are rendered into requests.
const MAX_VALUE: usize = 1024 * 1024;
/// Characters of a saved value shown in the results.
const SHOWN_VALUE: usize = 300;
/// What stands for a secret value in results.
const HIDDEN: &str = "••••••";
/// The variable an expression's result comes back in.
const RESULT: &str = "\u{0}scenario value";

/// Whether the scenario `id` is in the folder `folder` (at any depth).
fn in_folder(id: &str, folder: &str) -> bool {
    id.strip_prefix(folder).is_some_and(|rest| rest.starts_with('/'))
}

/// How much of the scenario a run goes through.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum RunMode {
    /// Everything.
    #[default]
    All,
    /// From the start up to and including the step at this path ("run to here"); the
    /// clean-up steps don't run.
    To(String),
    /// Only the step at this path, with the values the last run left.
    Only(String),
}

/// What a scenario's values hold after a run (kept in memory, for the next "run this step"
/// and for the user to look at).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Kept {
    pub values: BTreeMap<String, Value>,
    /// Names whose values are hidden.
    pub secret: BTreeSet<String>,
}

/// A value an "ask" step wants from the person running the scenario.
#[derive(Debug, Clone)]
pub struct Question {
    pub variable: String,
    pub message: String,
    pub secret: bool,
}

/// Asks the user; the answer (`None`: no value) comes through the receiver.
pub type Ask = Arc<dyn Fn(Question) -> oneshot::Receiver<Option<String>> + Send + Sync>;

/// What to run and how.
#[derive(Clone, Default)]
pub struct ScenarioPlan {
    pub scenario: Scenario,
    /// Every request its steps send (and those of the scenarios it runs), by path, read
    /// when the run starts.
    pub requests: HashMap<String, RunItem>,
    /// The scenarios its steps run (and theirs), by id.
    pub scenarios: HashMap<String, Result<Scenario, String>>,
    pub iterations: u32,
    /// Data file rows: iteration `i` gets row `i`, the last row once they run out.
    pub data: Vec<DataRow>,
    /// HTTP 4xx/5xx responses of requests without tests or checks don't fail them.
    pub allow_http_errors: bool,
    /// Secret variables `(key, value)`: reported URLs show `{{key}}` instead of the value.
    pub secrets: Vec<(String, String)>,
    pub mode: RunMode,
    /// The values a single step ([`RunMode::Only`]) starts from.
    pub start: Option<Kept>,
    /// How to ask the user for a value; `None` when nobody is there to answer.
    pub ask: Option<Ask>,
}

/// Read what a scenario's steps need: the requests they send and the scenarios they
/// run, and those of the scenarios in turn.
pub fn collect(
    ws: &Workspace,
    scenario: &Scenario,
) -> (HashMap<String, RunItem>, HashMap<String, Result<Scenario, String>>) {
    let mut requests: HashMap<String, RunItem> = HashMap::new();
    let mut scenarios: HashMap<String, Result<Scenario, String>> = HashMap::new();
    let mut add_requests = |scenario: &Scenario| {
        for path in scenario.requests() {
            if requests.contains_key(path) {
                continue;
            }
            let request = ws.read_request(path).map_err(|e| e.message);
            let name = match &request {
                Ok(r) if !r.name.trim().is_empty() => r.name.clone(),
                _ => path.rsplit('/').next().unwrap_or(path).trim_end_matches(".yaml").to_string(),
            };
            requests.insert(path.to_string(), RunItem { path: path.to_string(), name, request });
        }
    };
    add_requests(scenario);
    let mut pending: Vec<String> = scenario.calls().into_iter().map(str::to_string).collect();
    while let Some(id) = pending.pop() {
        if scenarios.contains_key(&id) || scenarios.len() >= MAX_SCENARIOS {
            continue;
        }
        let read = ws.read_scenario(&id).map_err(|e| e.message);
        if let Ok(s) = &read {
            add_requests(s);
            pending.extend(s.calls().into_iter().map(str::to_string));
        }
        scenarios.insert(id, read);
    }
    (requests, scenarios)
}

/// A run's report and the values it left.
pub struct ScenarioRun {
    pub report: RunReport,
    pub kept: Kept,
}

/// Run the plan. `base` has the environment, workspace variables and globals a scenario
/// reads, and the values given for this run (`overrides`: they win over everything).
/// `on_result` sees every result as it comes. Cancelling stops at once (the step in
/// progress is not reported, the clean-up steps don't run).
pub async fn run(
    cx: &SendContext<'_>,
    plan: &ScenarioPlan,
    base: &ScriptVars,
    cancel: &CancellationToken,
    mut on_result: impl FnMut(&RunResult) + Send,
) -> ScenarioRun {
    let started = Instant::now();
    let started_at = crate::now_ms() as f64;
    let mut summary = RunSummary::default();
    let mut results: Vec<RunResult> = Vec::new();
    let mut iteration_times = Vec::new();
    let (mut stopped, mut bailed, mut error) = (false, false, None);
    let mut seq = 0;
    let mut kept = plan.start.clone().unwrap_or_default();
    for index in 0..plan.iterations {
        let iteration_started = Instant::now();
        summary.per_iteration.push(IterationSummary { iteration: index, ..Default::default() });
        let mut vars = ScriptVars { scenario: true, changes: Vec::new(), ..base.clone() };
        vars.values.local.clear();
        vars.values.typed.clear();
        vars.values.data = plan.data.get(index as usize).or(plan.data.last()).cloned().unwrap_or_default();
        let walker = Walker {
            cx,
            plan,
            cancel,
            iteration: Iteration { index, count: plan.iterations },
            steps: AtomicUsize::new(0),
        };
        let (tx, mut rx) = mpsc::unbounded_channel::<RunResult>();
        let mut scope = Scope {
            vars,
            secret: BTreeSet::new(),
            out: Out::Live(tx),
            depth: 0,
            prefix: String::new(),
            calls: 0,
            lenient: plan.scenario.continue_on_failure,
            parallel: false,
            failures: 0,
        };
        let flow = {
            let walking = walker.iteration(&mut scope);
            tokio::pin!(walking);
            loop {
                let result = tokio::select! {
                    biased;
                    Some(result) = rx.recv() => result,
                    flow = &mut walking => break flow,
                };
                report(result, &mut seq, &mut summary, &mut results, &mut on_result);
            }
        };
        while let Ok(result) = rx.try_recv() {
            report(result, &mut seq, &mut summary, &mut results, &mut on_result);
        }
        kept = scope.kept();
        iteration_times.push(iteration_started.elapsed());
        match flow {
            Flow::Stopped => {
                stopped = true;
                break;
            }
            Flow::Overrun => {
                error = Some(format!(
                    "Stopped: iteration {} ran more than {MAX_STEPS} steps (a loop that never ends?)",
                    index + 1
                ));
                break;
            }
            Flow::Reached => break,
            Flow::Failed => bailed = true,
            Flow::Go | Flow::LeaveLoop | Flow::End => {}
        }
        if cancel.is_cancelled() {
            stopped = index + 1 < plan.iterations;
            break;
        }
    }
    for (it, time) in summary.per_iteration.iter_mut().zip(&iteration_times) {
        it.duration_ms = time.as_secs_f64() * 1000.0;
    }
    summary.name = plan.scenario.name.clone();
    summary.environment = base.environment.as_ref().map(|(_, name)| name.clone());
    summary.started_at = started_at;
    summary.duration_ms = started.elapsed().as_secs_f64() * 1000.0;
    summary.iterations = plan.iterations;
    summary.stopped = stopped;
    summary.bailed = bailed;
    summary.passed = summary.failed == 0 && error.is_none();
    summary.error = error;
    ScenarioRun { report: RunReport { summary, results }, kept }
}

/// Number a result, hand it on, count it and keep it for the report.
fn report(
    mut result: RunResult,
    seq: &mut u32,
    summary: &mut RunSummary,
    results: &mut Vec<RunResult>,
    on_result: &mut (impl FnMut(&RunResult) + Send),
) {
    *seq += 1;
    result.seq = *seq;
    on_result(&result);
    // The details stay with the app (`on_result`), not in the report.
    result.details = None;
    count(summary, &result);
    keep(results, &mut summary.omitted, result, MAX_KEPT_RESULTS);
}

/// Where a list of steps goes next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flow {
    /// On to the next step.
    Go,
    /// A stop step ended the loop it is in.
    LeaveLoop,
    /// A stop step ended the scenario.
    End,
    /// A step failed and the scenario doesn't go on after failures.
    Failed,
    /// The step a "run to here" asked for has run.
    Reached,
    /// The user stopped the run.
    Stopped,
    /// More steps than an iteration may run.
    Overrun,
}

impl Flow {
    /// Nothing more runs, not even the clean-up steps.
    fn ends_run(self) -> bool {
        matches!(self, Flow::Reached | Flow::Stopped | Flow::Overrun)
    }
}

/// Where results go: straight to the run, or held back (steps that run at the same time
/// report together, in their order).
enum Out {
    Live(mpsc::UnboundedSender<RunResult>),
    Held(Vec<RunResult>),
}

/// The values a list of steps runs with, and where it is.
struct Scope {
    vars: ScriptVars,
    /// Names whose values are hidden in results.
    secret: BTreeSet<String>,
    out: Out,
    /// How deep inside other steps.
    depth: u32,
    /// Before the paths of steps of a scenario this one runs (`steps/2>`).
    prefix: String,
    /// How many scenarios deep (a scenario that runs another one).
    calls: usize,
    /// Go on after failed steps.
    lenient: bool,
    /// Inside steps that run at the same time.
    parallel: bool,
    /// Failed results so far.
    failures: usize,
}

impl Scope {
    fn emit(&mut self, result: RunResult) {
        if !result.passed {
            self.failures += 1;
        }
        match &mut self.out {
            Out::Live(tx) => drop(tx.send(result)),
            Out::Held(list) => list.push(result),
        }
    }

    /// Set a value of the scenario: text as it is, anything else also as what it is (so
    /// scripts and loops get the number, list or object back).
    fn set(&mut self, key: &str, value: Value) {
        let id = format!("local:{key}");
        self.vars.values.local.insert(key.to_string(), value_text(&value));
        match value {
            Value::String(_) | Value::Null => self.vars.values.typed.remove(&id),
            other => self.vars.values.typed.insert(id, other),
        };
    }

    fn unset(&mut self, key: &str) {
        self.vars.values.local.remove(key);
        self.vars.values.typed.remove(&format!("local:{key}"));
    }

    /// The scenario's value under `key`, as what it is.
    fn get(&self, key: &str) -> Option<Value> {
        let text = self.vars.values.local.get(key)?;
        match self.vars.values.typed.get(&format!("local:{key}")) {
            Some(typed) if value_text(typed) == *text => Some(typed.clone()),
            _ => Some(Value::String(text.clone())),
        }
    }

    /// What scripts set on the environment, the workspace or globals stays in the
    /// scenario: it becomes a value of the scenario (those win over the others).
    fn absorb(&mut self) {
        for change in std::mem::take(&mut self.vars.changes) {
            match (&change.value, change.json) {
                (Some(_), Some(json)) => self.set(&change.key, json),
                (Some(text), None) => self.set(&change.key, Value::String(text.clone())),
                (None, _) => self.unset(&change.key),
            }
        }
    }

    fn kept(&self) -> Kept {
        let values = self.vars.values.local.keys().filter_map(|k| Some((k.clone(), self.get(k)?))).collect();
        Kept { values, secret: self.secret.clone() }
    }

    /// Secret values to hide in what a request reports: the run's, and the scenario's own.
    fn secrets(&self, run: &[(String, String)]) -> Vec<(String, String)> {
        let own = self.secret.iter().filter_map(|k| Some((k.clone(), self.vars.values.get(k)?)));
        run.iter().cloned().chain(own).collect()
    }
}

/// The first `max` characters of `text`.
fn cut(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((at, _)) => format!("{}…", &text[..at]),
        None => text.to_string(),
    }
}

/// Values of the scenario put aside while a loop uses their names, to put back after it.
struct Shadow(Vec<(String, Option<Value>)>);

impl Shadow {
    fn take(scope: &Scope, keys: &[&str]) -> Self {
        Self(keys.iter().map(|k| (k.to_string(), scope.get(k))).collect())
    }

    fn restore(self, scope: &mut Scope) {
        for (key, value) in self.0 {
            match value {
                Some(value) => scope.set(&key, value),
                None => scope.unset(&key),
            }
        }
    }
}

type Boxed<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

struct Walker<'a> {
    cx: &'a SendContext<'a>,
    plan: &'a ScenarioPlan,
    cancel: &'a CancellationToken,
    iteration: Iteration,
    /// Steps run in this iteration (every branch counts).
    steps: AtomicUsize,
}

impl Walker<'_> {
    /// One pass through the scenario: its values, its steps, then the clean-up steps.
    async fn iteration(&self, scope: &mut Scope) -> Flow {
        let scenario = &self.plan.scenario;
        if let RunMode::Only(path) = &self.plan.mode {
            match &self.plan.start {
                Some(kept) => {
                    for (key, value) in &kept.values {
                        scope.set(key, value.clone());
                        if kept.secret.contains(key) {
                            self.secret(scope, key, value);
                        }
                    }
                    scope.secret.extend(kept.secret.iter().cloned());
                }
                None => {
                    if let Err(flow) = self.start_values(scope, scenario).await {
                        return flow;
                    }
                }
            }
            return match scenario.step_at(path) {
                Some(step) => self.run_step(scope, step, path.clone()).await,
                None => Flow::Go,
            };
        }
        if let Err(flow) = self.start_values(scope, scenario).await {
            return flow;
        }
        // A data file's row wins over the scenario's own starting values.
        for (key, value) in scope.vars.values.data.clone() {
            scope.set(&key, value);
        }
        let flow = self.run_steps(scope, &scenario.steps, "steps".into()).await;
        // "Run to here" cleans up only when the step it runs to is one of the clean-up steps
        // (also when it never got there: a branch that wasn't taken).
        let part = matches!(&self.plan.mode, RunMode::To(to) if !to.starts_with("always/"));
        if flow.ends_run() || scenario.always.is_empty() || part {
            return flow;
        }
        // Clean up whatever happened, and go on after a clean-up step that fails.
        let lenient = std::mem::replace(&mut scope.lenient, true);
        let after = self.run_steps(scope, &scenario.always, "always".into()).await;
        scope.lenient = lenient;
        if after.ends_run() { after } else { flow }
    }

    /// Give the scenario its starting values, in order (a later one may use an earlier one).
    async fn start_values(&self, scope: &mut Scope, scenario: &Scenario) -> Result<(), Flow> {
        for v in scenario.variables.iter().filter(|v| v.enabled && !v.key.trim().is_empty()) {
            match self.eval(scope, &v.value).await {
                Ok(Some(value)) => scope.set(v.key.trim(), value),
                Ok(None) => scope.unset(v.key.trim()),
                Err(Problem::Stopped) => return Err(Flow::Stopped),
                Err(Problem::Message(e)) => {
                    let mut result = self.row(scope, StepKind::Set, "variables", "Variables".into());
                    result.error = Some(format!("{}: {e}", v.key.trim()));
                    result.passed = false;
                    scope.emit(result);
                    return Err(Flow::Failed);
                }
            }
        }
        Ok(())
    }

    fn run_steps<'s>(&'s self, scope: &'s mut Scope, steps: &'s [Step], at: String) -> Boxed<'s, Flow> {
        Box::pin(async move {
            for (i, step) in steps.iter().enumerate() {
                if !step.enabled {
                    continue;
                }
                let flow = self.run_step(scope, step, format!("{at}/{i}")).await;
                if flow != Flow::Go {
                    return flow;
                }
            }
            Flow::Go
        })
    }

    fn run_step<'s>(&'s self, scope: &'s mut Scope, step: &'s Step, path: String) -> Boxed<'s, Flow> {
        Box::pin(async move {
            if self.cancel.is_cancelled() {
                return Flow::Stopped;
            }
            if self.steps.fetch_add(1, Ordering::Relaxed) >= MAX_STEPS {
                return Flow::Overrun;
            }
            let flow = match &step.action {
                StepAction::Request(r) => self.request(scope, step, r, &path).await,
                StepAction::If(s) => self.branch(scope, step, s, &path).await,
                StepAction::ForEach(s) => self.for_each(scope, step, s, &path).await,
                StepAction::Repeat(s) => self.repeat(scope, step, s, &path).await,
                StepAction::Parallel(s) => self.parallel(scope, step, s, &path).await,
                StepAction::Set(s) => self.set(scope, step, s, &path).await,
                StepAction::Wait(s) => self.wait(scope, step, s, &path).await,
                StepAction::Stop(s) => self.stop(scope, step, s, &path).await,
                StepAction::Scenario(s) => self.call(scope, step, s, &path).await,
                StepAction::Script(s) => self.script_step(scope, step, s, &path).await,
                StepAction::Ask(s) => self.ask(scope, step, s, &path).await,
            };
            let flow = match flow {
                Flow::Failed if step.continue_on_failure || scope.lenient => Flow::Go,
                other => other,
            };
            let target = matches!(&self.plan.mode, RunMode::To(to) if scope.prefix.is_empty() && *to == path);
            if target && !matches!(flow, Flow::Stopped | Flow::Overrun) { Flow::Reached } else { flow }
        })
    }

    /// A result for a step that isn't a request.
    fn row(&self, scope: &Scope, kind: StepKind, path: &str, name: String) -> RunResult {
        RunResult {
            iteration: self.iteration.index,
            name,
            passed: true,
            step: Some(Box::new(StepInfo {
                path: format!("{}{path}", scope.prefix),
                kind,
                depth: scope.depth,
                note: None,
                saved: Vec::new(),
            })),
            ..Default::default()
        }
    }

    /// `key` holds a secret from here on: hidden in results, and in what agents read of the
    /// run's console lines (also the ones before, like the response it came in).
    fn secret(&self, scope: &mut Scope, key: &str, value: &Value) {
        scope.secret.insert(key.to_string());
        if let Some(console) = &self.cx.console {
            console.hide(key, &value_text(value));
        }
    }

    /// A value as the results show it: hidden when it is secret, and with the secret
    /// values inside it (a header built from a secret key) as `{{name}}`.
    fn shown(&self, scope: &Scope, key: &str, value: &Value) -> SavedValue {
        let secret = scope.secret.contains(key);
        let value = if secret {
            HIDDEN.to_string()
        } else {
            let secrets = secret_values(&scope.secrets(&self.plan.secrets), &scope.vars);
            cut(&mask_secrets(&value_text(value), &secrets), SHOWN_VALUE)
        };
        SavedValue { key: key.to_string(), value, secret }
    }

    /// Report a step that failed before it did anything.
    fn fail(&self, scope: &mut Scope, step: &Step, path: &str, error: String) -> Flow {
        let mut result = self.row(scope, step.kind(), path, self.title(step));
        result.error = Some(error);
        result.passed = false;
        scope.emit(result);
        Flow::Failed
    }

    /// What a step is called in the results: its name, or what it does.
    fn title(&self, step: &Step) -> String {
        if !step.name.trim().is_empty() {
            return step.name.trim().to_string();
        }
        match &step.action {
            StepAction::Request(r) => match self.plan.requests.get(&r.request) {
                Some(item) => item.name.clone(),
                None => r.request.clone(),
            },
            StepAction::If(s) => format!("If {}", describe(&s.when)),
            StepAction::ForEach(s) => format!("For each {} in {}", item_name(s), short(s.list.trim())),
            StepAction::Repeat(s) => match &s.until {
                Some(until) => format!("Repeat until {}", describe(until)),
                None => format!("Repeat {}", plural(s.times as usize, "time")),
            },
            StepAction::Parallel(_) => "At the same time".into(),
            StepAction::Set(s) => {
                let keys: Vec<&str> =
                    s.values.iter().filter(|v| v.enabled && !v.key.trim().is_empty()).map(|v| v.key.trim()).collect();
                if keys.is_empty() { "Set values".into() } else { format!("Set {}", keys.join(", ")) }
            }
            StepAction::Wait(s) => format!("Wait {} ms", s.ms),
            StepAction::Stop(s) => match s.what {
                StopWhat::Loop => "Leave the loop".into(),
                StopWhat::Scenario => "End the scenario".into(),
                StopWhat::Fail => "Fail the scenario".into(),
            },
            StepAction::Scenario(s) => match self.plan.scenarios.get(&s.scenario) {
                Some(Ok(scenario)) => format!("Run scenario {}", scenario.name),
                _ => format!("Run scenario {}", s.scenario),
            },
            StepAction::Script(_) => "Script".into(),
            StepAction::Ask(s) => match s.variable.trim() {
                "" => "Ask for a value".into(),
                name => format!("Ask for {name}"),
            },
        }
    }

    // ---- values ------------------------------------------------------------------------------

    /// What a value field gives: text with its `{{variables}}` filled in, or the result of
    /// the JavaScript after `=`. `None`: a single `{{variable}}` that is not defined.
    async fn eval(&self, scope: &Scope, text: &str) -> Result<Option<Value>, Problem> {
        if let Some(code) = text.trim_start().strip_prefix('=') {
            return self.expression(scope, code).await.map(Some);
        }
        let single = single_variable(text);
        // A value that is a list, a number or an object comes as what it is.
        if let Some(name) = single
            && !scope.vars.values.overrides.contains_key(name)
            && let Some(value) = scope.get(name)
        {
            return Ok(Some(value));
        }
        let mut unresolved = BTreeSet::new();
        let rendered = scope.vars.var_context().render(text, &mut unresolved);
        if single.is_some() && !unresolved.is_empty() {
            return Ok(None);
        }
        Ok(Some(Value::String(rendered)))
    }

    /// [`Self::eval`] as text (nothing for an undefined variable).
    async fn eval_text(&self, scope: &Scope, text: &str) -> Result<String, Problem> {
        Ok(self.eval(scope, text).await?.map(|v| value_text(&v)).unwrap_or_default())
    }

    /// Run JavaScript that gives a value. Variables whose names can be written in
    /// JavaScript are there by name (`total + 1`), all of them in `vars` (`vars["my-id"]`).
    async fn expression(&self, scope: &Scope, code: &str) -> Result<Value, Problem> {
        let names: Vec<&str> = identifiers(code)
            .into_iter()
            .filter(|n| !RESERVED.contains(n) && scope.vars.values.get(n).is_some())
            .collect();
        let quoted = |s: &str| serde_json::to_string(s).unwrap_or_default();
        let params: String = names.iter().map(|n| format!(", {n}")).collect();
        let args: String = names.iter().map(|n| format!(", pm.variables.get({})", quoted(n))).collect();
        // The expression stays on line 2, on a line of its own (a `//` comment in it ends there).
        let script = format!(
            "pm.variables.set({}, [(function (vars{params}) {{ return (\n{code}\n); }})(pm.variables.toObject(){args})]);",
            quoted(RESULT)
        );
        let said = |e: String| Problem::Message(format!("`{}`: {e}", short(code.trim())));
        let out = self.script(scope, script, "", false).await?;
        if let Some(e) = out.error {
            return Err(said(e.message));
        }
        let change = out.variables.into_iter().rev().find(|c| c.key == RESULT);
        let list = match change {
            Some(c) => c.json.or_else(|| c.value.and_then(|text| serde_json::from_str(&text).ok())),
            None => None,
        };
        match list {
            Some(Value::Array(mut items)) if items.len() == 1 => Ok(items.remove(0)),
            _ => Err(said("it gave no value".into())),
        }
    }

    /// Run a script with the scenario's values. `name` names it in the console and
    /// `pm.info`; its console lines go to the app's console only when `console`.
    async fn script(&self, scope: &Scope, code: String, name: &str, console: bool) -> Result<ScriptOutput, Problem> {
        let info = Info {
            request_name: name.to_string(),
            request_id: String::new(),
            iteration: self.iteration.index,
            iteration_count: self.iteration.count,
            location: vec![self.plan.scenario.name.clone(), name.to_string()],
            current: name.to_string(),
        };
        let input = script_input(Event::PreRequest, &info, &scope.vars, ScriptRequest::default(), None);
        let limits = Limits { timeout: self.cx.settings.script_timeout(), ..Default::default() };
        let console = if console { self.cx.console.as_ref().map(|c| c.of(name)) } else { None };
        let host = script_host(self.cx, "", console).map_err(|e| Problem::Message(e.message))?;
        let running = tokio::task::spawn_blocking(move || zorvik_script::run_with(&code, &input, &limits, host));
        tokio::select! {
            out = running => out.map_err(|e| Problem::Message(crate::join_err(e).message)),
            _ = self.cancel.cancelled() => Err(Problem::Stopped),
        }
    }

    /// Whether a condition holds: all of its rules, or any one of them.
    async fn holds(&self, scope: &Scope, condition: &Condition) -> Result<bool, Problem> {
        for rule in &condition.rules {
            let left = self.eval(scope, &rule.value).await?;
            let right = match rule.op.takes_value() {
                true => self.eval(scope, &rule.to).await?.unwrap_or(Value::Null),
                false => Value::Null,
            };
            let holds = compare(rule.op, left.as_ref(), &right)
                .map_err(|e| Problem::Message(format!("{}: {e}", describe_rule(rule))))?;
            if holds == condition.any {
                return Ok(holds);
            }
        }
        // No rule decided: every one held (all), or none did (any). No rules at all: it holds.
        Ok(!condition.any || condition.rules.is_empty())
    }

    // ---- request steps -----------------------------------------------------------------------

    async fn request(&self, scope: &mut Scope, step: &Step, r: &StepRequest, path: &str) -> Flow {
        let Some(item) = self.plan.requests.get(&r.request) else {
            return self.fail(scope, step, path, format!("The request '{}' was not found", r.request));
        };
        let name = self.title(step);
        let info = |scope: &Scope, saved: Vec<SavedValue>| {
            Some(Box::new(StepInfo {
                path: format!("{}{path}", scope.prefix),
                kind: StepKind::Request,
                depth: scope.depth,
                note: None,
                saved,
            }))
        };
        let request = match sendable(item, self.iteration.index) {
            Ok(request) => request,
            Err(result) => {
                let mut result = *result;
                result.name = name;
                result.step = info(scope, Vec::new());
                result.passed = verdict(&result, self.plan.allow_http_errors);
                let passed = result.passed;
                scope.emit(result);
                return if passed { Flow::Go } else { Flow::Failed };
            }
        };
        let tries = 1 + r.retry.map_or(0, |retry| retry.times.min(MAX_TRIES - 1));
        let outputs: Vec<&str> = r.outputs.iter().map(|o| o.variable.trim()).filter(|key| !key.is_empty()).collect();
        let mut tried = 0;
        loop {
            tried += 1;
            // This step's inputs win over every other value while it runs.
            let mut notes = Vec::new();
            let mut inputs: Vec<(String, String)> = Vec::new();
            for input in r.inputs.iter().filter(|i| i.enabled && !i.key.trim().is_empty()) {
                let key = input.key.trim();
                match self.eval(scope, &input.value).await {
                    Ok(Some(value)) => inputs.push((key.to_string(), value_text(&value))),
                    Ok(None) => notes.push(format!("Input {key}: {} is not defined", input.value.trim())),
                    Err(Problem::Stopped) => return Flow::Stopped,
                    Err(Problem::Message(e)) => return self.fail(scope, step, path, format!("Input {key}: {e}")),
                }
            }
            // Values given for the whole run (`--var`) stay on top.
            inputs.retain(|(key, _)| !scope.vars.values.overrides.contains_key(key));
            for (key, value) in &inputs {
                scope.vars.values.overrides.insert(key.clone(), value.clone());
            }
            let secrets = scope.secrets(&self.plan.secrets);
            // Boxed: the whole pipeline is a big future (debug builds would overflow the stack).
            let sending =
                Box::pin(send_item(self.cx, item, request, &mut scope.vars, self.iteration, &secrets, self.cancel))
                    .await;
            for (key, _) in &inputs {
                scope.vars.values.overrides.remove(key);
            }
            let Some(Sending { mut result, next, response }) = sending else { return Flow::Stopped };
            scope.absorb();
            // A try that is tried again leaves no outputs: they are put back as they were.
            let before = (tried < tries).then(|| Shadow::take(scope, &outputs));
            let mut saved = Vec::new();
            if let Some(response) = &response {
                match self.read_response(scope, r, response, &mut result.tests, &mut saved).await {
                    Ok(()) => {}
                    Err(Problem::Stopped) => return Flow::Stopped,
                    Err(Problem::Message(e)) => result.error = Some(result.error.take().unwrap_or(e)),
                }
            }
            result.name = name.clone();
            result.step = info(scope, saved);
            let warn = |message: String| ConsoleEntry { level: ConsoleLevel::Warn, message };
            result.console.extend(notes.into_iter().map(warn));
            if next.is_some() {
                result.console.push(warn(
                    "setNextRequest does nothing in a scenario: use If and Stop steps to change what runs next.".into(),
                ));
            }
            result.passed = verdict(&result, self.plan.allow_http_errors);
            if !result.passed
                && let Some(before) = before
            {
                before.restore(scope);
                let pause = Duration::from_millis(r.retry.map_or(0, |retry| retry.delay_ms).min(MAX_DELAY_MS));
                tokio::select! {
                    _ = tokio::time::sleep(pause) => {}
                    _ = self.cancel.cancelled() => return Flow::Stopped,
                }
                continue;
            }
            if tried > 1 {
                result.attempts = Some(result.attempts.unwrap_or(0).max(tried));
            }
            let passed = result.passed;
            scope.emit(result);
            return if passed { Flow::Go } else { Flow::Failed };
        }
    }

    /// A request step's checks (each one a test) and outputs (saved into the scenario).
    async fn read_response(
        &self,
        scope: &mut Scope,
        r: &StepRequest,
        response: &HttpResponse,
        tests: &mut Vec<TestResult>,
        saved: &mut Vec<SavedValue>,
    ) -> Result<(), Problem> {
        let mut reader = Reader { response, json: None };
        for check in r.checks.iter().filter(|c| c.enabled) {
            let name = describe_check(check);
            let outcome = match reader.read(check.from, &check.path) {
                Err(e) => Err(e),
                Ok(found) => {
                    let expected = match check.op.takes_value() {
                        true => self.eval(scope, &check.value).await?.unwrap_or(Value::Null),
                        false => Value::Null,
                    };
                    match compare(check.op, found.as_ref(), &expected) {
                        Ok(true) => Ok(()),
                        Ok(false) => Err(match &found {
                            Some(value) => format!("It is {}", quote(value)),
                            None => "Nothing was found".to_string(),
                        }),
                        Err(e) => Err(e),
                    }
                }
            };
            tests.push(TestResult { name, passed: outcome.is_ok(), skipped: false, error: outcome.err() });
        }
        for output in r.outputs.iter().filter(|o| !o.variable.trim().is_empty()) {
            let key = output.variable.trim();
            let found = reader.read(output.from, &output.path).and_then(|found| match found {
                Some(value) if value_text(&value).len() > MAX_VALUE => {
                    Err(format!("The value is larger than {} MB", MAX_VALUE >> 20))
                }
                found => Ok(found.filter(|value| !value.is_null())),
            });
            let missing = |why: String| TestResult {
                name: format!("Save {key} from {}", describe_source(output.from, &output.path)),
                passed: false,
                skipped: false,
                error: Some(why),
            };
            match found {
                Ok(Some(value)) => {
                    if output.secret {
                        self.secret(scope, key, &value);
                    }
                    let value = match (output.collect, scope.get(key)) {
                        (false, _) => value,
                        (true, Some(Value::Array(mut list))) => {
                            list.push(value);
                            Value::Array(list)
                        }
                        (true, _) => Value::Array(vec![value]),
                    };
                    saved.push(self.shown(scope, key, &value));
                    scope.set(key, value);
                }
                Ok(None) if output.optional => {
                    if !output.collect {
                        scope.unset(key);
                    }
                }
                Ok(None) => tests.push(missing("Nothing was found".into())),
                Err(e) => tests.push(missing(e)),
            }
        }
        Ok(())
    }

    // ---- flow --------------------------------------------------------------------------------

    async fn branch(&self, scope: &mut Scope, step: &Step, s: &StepIf, path: &str) -> Flow {
        let mut chosen: Option<(String, &[Step], String)> = None;
        let branches = std::iter::once((&s.when, s.steps.as_slice(), "steps".to_string(), "yes".to_string())).chain(
            s.else_if.iter().enumerate().map(|(i, b)| {
                (&b.when, b.steps.as_slice(), format!("elseIf/{i}"), format!("else if {}", describe(&b.when)))
            }),
        );
        for (when, steps, part, note) in branches {
            match self.holds(scope, when).await {
                Ok(true) => {
                    chosen = Some((note, steps, part));
                    break;
                }
                Ok(false) => {}
                Err(Problem::Stopped) => return Flow::Stopped,
                Err(Problem::Message(e)) => return self.fail(scope, step, path, e),
            }
        }
        let (note, steps, part) = chosen.unwrap_or_else(|| {
            (if s.otherwise.is_empty() { "no" } else { "else" }.to_string(), &s.otherwise, "else".into())
        });
        let mut result = self.row(scope, StepKind::If, path, self.title(step));
        result.step.as_mut().expect("a step row").note = Some(note);
        scope.emit(result);
        scope.depth += 1;
        let flow = self.run_steps(scope, steps, format!("{path}/{part}")).await;
        scope.depth -= 1;
        flow
    }

    async fn for_each(&self, scope: &mut Scope, step: &Step, s: &StepForEach, path: &str) -> Flow {
        let items = match self.eval(scope, &s.list).await {
            Ok(value) => list_items(&value.unwrap_or(Value::Null)),
            Err(Problem::Stopped) => return Flow::Stopped,
            Err(Problem::Message(e)) => return self.fail(scope, step, path, e),
        };
        let name = item_name(s);
        let mut result = self.row(scope, StepKind::ForEach, path, self.title(step));
        result.step.as_mut().expect("a step row").note = Some(plural(items.len(), "item"));
        scope.emit(result);
        let shadow = Shadow::take(scope, &[name, "index"]);
        let count = items.len();
        let mut flow = Flow::Go;
        for (i, item) in items.into_iter().enumerate() {
            let label = format!("{name} = {}", short(&value_text(&item)));
            scope.set(name, item);
            match self.round(scope, &s.steps, path, i, count, label).await {
                Flow::Go => {}
                Flow::LeaveLoop => break,
                other => {
                    flow = other;
                    break;
                }
            }
        }
        shadow.restore(scope);
        flow
    }

    async fn repeat(&self, scope: &mut Scope, step: &Step, s: &StepRepeat, path: &str) -> Flow {
        let rounds = s.times.clamp(1, MAX_STEPS as u32) as usize;
        let mut result = self.row(scope, StepKind::Repeat, path, self.title(step));
        result.step.as_mut().expect("a step row").note = Some(match s.until {
            Some(_) => format!("up to {}", plural(rounds, "round")),
            None => plural(rounds, "round"),
        });
        scope.emit(result);
        let shadow = Shadow::take(scope, &["index"]);
        let mut flow = Flow::Go;
        let mut held = s.until.is_none();
        let mut ran = 0;
        for i in 0..rounds {
            if i > 0 && s.delay_ms > 0 {
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_millis(s.delay_ms.min(MAX_DELAY_MS))) => {}
                    _ = self.cancel.cancelled() => {
                        flow = Flow::Stopped;
                        break;
                    }
                }
            }
            ran = i + 1;
            match self.round(scope, &s.steps, path, i, rounds, format!("Round {}", i + 1)).await {
                Flow::Go => {}
                Flow::LeaveLoop => {
                    held = true;
                    break;
                }
                other => {
                    flow = other;
                    break;
                }
            }
            let Some(until) = &s.until else { continue };
            match self.holds(scope, until).await {
                Ok(true) => {
                    held = true;
                    break;
                }
                Ok(false) => {}
                Err(Problem::Stopped) => {
                    flow = Flow::Stopped;
                    break;
                }
                Err(Problem::Message(e)) => {
                    flow = self.fail(scope, step, path, e);
                    held = true;
                    break;
                }
            }
        }
        shadow.restore(scope);
        if flow == Flow::Go
            && !held
            && let Some(until) = &s.until
        {
            let error = format!("{} didn't hold after {}", describe(until), plural(ran, "round"));
            return self.fail(scope, step, path, error);
        }
        flow
    }

    /// One round of a loop: its row, `{{index}}`, then the steps.
    async fn round(&self, scope: &mut Scope, steps: &[Step], path: &str, i: usize, of: usize, label: String) -> Flow {
        if self.cancel.is_cancelled() {
            return Flow::Stopped;
        }
        // A round counts as a step, so a loop without steps ends too.
        if self.steps.fetch_add(1, Ordering::Relaxed) >= MAX_STEPS {
            return Flow::Overrun;
        }
        scope.set("index", Value::from(i));
        scope.depth += 1;
        let mut result = self.row(scope, StepKind::Round, path, label);
        result.step.as_mut().expect("a step row").note = Some(format!("{} of {of}", i + 1));
        scope.emit(result);
        let flow = self.run_steps(scope, steps, format!("{path}/steps")).await;
        scope.depth -= 1;
        flow
    }

    async fn parallel(&self, scope: &mut Scope, step: &Step, s: &StepParallel, path: &str) -> Flow {
        let children: Vec<(usize, &Step)> = s.steps.iter().enumerate().filter(|(_, child)| child.enabled).collect();
        if children.len() > MAX_PARALLEL {
            let error = format!("At most {MAX_PARALLEL} steps can run at the same time (this has {})", children.len());
            return self.fail(scope, step, path, error);
        }
        let mut result = self.row(scope, StepKind::Parallel, path, self.title(step));
        result.step.as_mut().expect("a step row").note = Some(plural(children.len(), "step"));
        scope.emit(result);
        let before = scope.vars.clone();
        let mut branches: Vec<Scope> = children
            .iter()
            .map(|_| Scope {
                vars: before.clone(),
                secret: scope.secret.clone(),
                out: Out::Held(Vec::new()),
                depth: scope.depth + 1,
                prefix: scope.prefix.clone(),
                calls: scope.calls,
                lenient: scope.lenient,
                parallel: true,
                failures: 0,
            })
            .collect();
        let running = children
            .iter()
            .zip(branches.iter_mut())
            .map(|((i, child), branch)| self.run_step(branch, child, format!("{path}/steps/{i}")));
        let flows = futures_util::future::join_all(running).await;
        // Their results in the steps' order, and what they saved put together.
        for branch in branches {
            if let Out::Held(results) = branch.out {
                for result in results {
                    scope.emit(result);
                }
            }
            merge(&mut scope.vars, &before, &branch.vars);
            scope.secret.extend(branch.secret);
        }
        [Flow::Stopped, Flow::Overrun, Flow::Reached, Flow::Failed, Flow::End, Flow::LeaveLoop]
            .into_iter()
            .find(|flow| flows.contains(flow))
            .unwrap_or(Flow::Go)
    }

    async fn call(&self, scope: &mut Scope, step: &Step, s: &StepCall, path: &str) -> Flow {
        let scenario = match self.plan.scenarios.get(&s.scenario) {
            Some(Ok(scenario)) => scenario,
            Some(Err(e)) => {
                return self.fail(scope, step, path, format!("The scenario '{}' can't be read: {e}", s.scenario));
            }
            None => return self.fail(scope, step, path, format!("The scenario '{}' was not found", s.scenario)),
        };
        if scope.calls >= MAX_CALL_DEPTH {
            let error = format!("Scenarios run each other more than {MAX_CALL_DEPTH} deep (does one run itself?)");
            return self.fail(scope, step, path, error);
        }
        // What it is given, read with this scenario's values.
        let mut given = Vec::new();
        for input in s.inputs.iter().filter(|i| i.enabled && !i.key.trim().is_empty()) {
            match self.eval(scope, &input.value).await {
                Ok(value) => given.push((input.key.trim().to_string(), value)),
                Err(Problem::Stopped) => return Flow::Stopped,
                Err(Problem::Message(e)) => {
                    return self.fail(scope, step, path, format!("Input {}: {e}", input.key.trim()));
                }
            }
        }
        scope.emit(self.row(scope, StepKind::Scenario, path, self.title(step)));
        let failures = scope.failures;
        // The other scenario's own values, in this scope's place while it runs.
        let mut inner = scope.vars.clone();
        inner.values.local.clear();
        inner.values.typed.retain(|id, _| !id.starts_with("local:"));
        let outer = std::mem::replace(&mut scope.vars, inner);
        let outer_secret = std::mem::take(&mut scope.secret);
        let prefix = format!("{}{path}>", scope.prefix);
        let outer_prefix = std::mem::replace(&mut scope.prefix, prefix);
        let outer_lenient = std::mem::replace(&mut scope.lenient, scenario.continue_on_failure);
        scope.depth += 1;
        scope.calls += 1;
        let mut flow = match self.start_values(scope, scenario).await {
            Ok(()) => {
                for (key, value) in given {
                    match value {
                        Some(value) => scope.set(&key, value),
                        None => scope.unset(&key),
                    }
                }
                self.run_steps(scope, &scenario.steps, "steps".into()).await
            }
            Err(flow) => flow,
        };
        if !flow.ends_run() && !scenario.always.is_empty() {
            scope.lenient = true;
            let after = self.run_steps(scope, &scenario.always, "always".into()).await;
            if after.ends_run() {
                flow = after;
            }
        }
        // What it gives back, read with its values.
        let mut back = Vec::new();
        let mut problem = None;
        if !flow.ends_run() {
            for output in s.outputs.iter().filter(|o| o.enabled && !o.key.trim().is_empty()) {
                match self.eval(scope, &output.value).await {
                    Ok(value) => {
                        let secret = value.as_ref().is_some_and(|v| {
                            let text = value_text(v);
                            scope.secret.iter().any(|k| scope.vars.values.local.get(k) == Some(&text))
                        });
                        back.push((output.key.trim().to_string(), value, secret));
                    }
                    Err(Problem::Stopped) => flow = Flow::Stopped,
                    Err(Problem::Message(e)) => problem = Some(format!("Output {}: {e}", output.key.trim())),
                }
            }
        }
        scope.calls -= 1;
        scope.depth -= 1;
        scope.lenient = outer_lenient;
        scope.prefix = outer_prefix;
        scope.secret = outer_secret;
        // Its values end with it, what its scripts set too: only what is brought back stays.
        scope.vars = outer;
        for (key, value, secret) in back {
            if secret {
                scope.secret.insert(key.clone());
            }
            match value {
                Some(value) => scope.set(&key, value),
                None => scope.unset(&key),
            }
        }
        if let Some(error) = problem {
            return self.fail(scope, step, path, error);
        }
        match flow {
            // A step of it failed, also one it went on after: this step failed.
            Flow::Go | Flow::End | Flow::LeaveLoop if scope.failures > failures => Flow::Failed,
            // Its own end, or a "leave the loop" outside a loop: this scenario goes on.
            Flow::End | Flow::LeaveLoop => Flow::Go,
            other => other,
        }
    }

    // ---- small steps ---------------------------------------------------------------------------

    async fn set(&self, scope: &mut Scope, step: &Step, s: &StepSet, path: &str) -> Flow {
        let mut saved = Vec::new();
        for v in s.values.iter().filter(|v| v.enabled && !v.key.trim().is_empty()) {
            let key = v.key.trim();
            match self.eval(scope, &v.value).await {
                Ok(Some(value)) if value_text(&value).len() > MAX_VALUE => {
                    let error = format!("{key}: the value is larger than {} MB", MAX_VALUE >> 20);
                    return self.fail(scope, step, path, error);
                }
                Ok(Some(value)) => {
                    saved.push(self.shown(scope, key, &value));
                    scope.set(key, value);
                }
                Ok(None) => scope.unset(key),
                Err(Problem::Stopped) => return Flow::Stopped,
                Err(Problem::Message(e)) => return self.fail(scope, step, path, format!("{key}: {e}")),
            }
        }
        let mut result = self.row(scope, StepKind::Set, path, self.title(step));
        result.step.as_mut().expect("a step row").saved = saved;
        scope.emit(result);
        Flow::Go
    }

    async fn wait(&self, scope: &mut Scope, step: &Step, s: &StepWait, path: &str) -> Flow {
        let started = Instant::now();
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(s.ms.min(MAX_DELAY_MS))) => {}
            _ = self.cancel.cancelled() => return Flow::Stopped,
        }
        let mut result = self.row(scope, StepKind::Wait, path, self.title(step));
        result.duration_ms = Some(started.elapsed().as_secs_f64() * 1000.0);
        scope.emit(result);
        Flow::Go
    }

    async fn stop(&self, scope: &mut Scope, step: &Step, s: &StepStop, path: &str) -> Flow {
        let message = match self.eval_text(scope, &s.message).await {
            Ok(message) => message,
            Err(Problem::Stopped) => return Flow::Stopped,
            Err(Problem::Message(e)) => return self.fail(scope, step, path, e),
        };
        let mut result = self.row(scope, StepKind::Stop, path, self.title(step));
        let note = (!message.trim().is_empty()).then(|| message.trim().to_string());
        match s.what {
            StopWhat::Loop => {
                result.step.as_mut().expect("a step row").note = note;
                scope.emit(result);
                Flow::LeaveLoop
            }
            StopWhat::Scenario => {
                result.step.as_mut().expect("a step row").note = note;
                scope.emit(result);
                Flow::End
            }
            // Ends the scenario whether or not it goes on after failed steps.
            StopWhat::Fail => {
                result.error = Some(note.unwrap_or_else(|| "The scenario was failed by a stop step".into()));
                result.passed = false;
                scope.emit(result);
                Flow::End
            }
        }
    }

    async fn script_step(&self, scope: &mut Scope, step: &Step, s: &StepScript, path: &str) -> Flow {
        let name = self.title(step);
        let started = Instant::now();
        let out = match self.script(scope, s.code.clone(), &name, true).await {
            Ok(out) => out,
            Err(Problem::Stopped) => return Flow::Stopped,
            Err(Problem::Message(e)) => return self.fail(scope, step, path, e),
        };
        let mut saved = Vec::new();
        for change in &out.variables {
            scope.vars.values.apply(change);
            if change.scope != zorvik_script::Scope::Local {
                scope.vars.changes.push(change.clone());
            }
        }
        scope.absorb();
        for change in out.variables.iter().filter(|c| c.value.is_some()) {
            if let Some(value) = scope.get(&change.key) {
                saved.retain(|s: &SavedValue| s.key != change.key);
                saved.push(self.shown(scope, &change.key, &value));
            }
        }
        let mut result = self.row(scope, StepKind::Script, path, name);
        result.duration_ms = Some(started.elapsed().as_secs_f64() * 1000.0);
        result.step.as_mut().expect("a step row").saved = saved;
        result.tests = out.tests;
        result.console = bounded_console(out.console);
        if let Some(e) = out.error {
            let message = match e.line {
                Some(line) => format!("The script failed at line {line}: {}", e.message),
                None => format!("The script failed: {}", e.message),
            };
            if let Some(console) = &self.cx.console {
                console.of(&result.name).log(ConsoleLevel::Error, &message);
            }
            result.script_errors.push(message);
        }
        result.passed = verdict(&result, true);
        let passed = result.passed;
        scope.emit(result);
        if passed { Flow::Go } else { Flow::Failed }
    }

    async fn ask(&self, scope: &mut Scope, step: &Step, s: &StepAsk, path: &str) -> Flow {
        let key = s.variable.trim();
        if key.is_empty() {
            return self.fail(scope, step, path, "Give the variable the answer goes in".into());
        }
        let mut result = self.row(scope, StepKind::Ask, path, self.title(step));
        // A value given for the run answers it.
        if let Some(given) = scope.vars.values.overrides.get(key).cloned() {
            if s.secret {
                self.secret(scope, key, &Value::String(given));
            }
            result.step.as_mut().expect("a step row").note = Some("given for this run".into());
            scope.emit(result);
            return Flow::Go;
        }
        let Some(ask) = &self.plan.ask else {
            if let Some(kept) = scope.get(key) {
                if s.secret {
                    self.secret(scope, key, &kept);
                }
                result.step.as_mut().expect("a step row").note = Some("kept its value".into());
                scope.emit(result);
                return Flow::Go;
            }
            let error = format!("Nobody can be asked for '{key}' in this run: give it a value when starting the run");
            return self.fail(scope, step, path, error);
        };
        if scope.parallel {
            let error = "Steps that run at the same time can't ask for a value: ask before them".to_string();
            return self.fail(scope, step, path, error);
        }
        let message = match self.eval_text(scope, &s.message).await {
            Ok(message) => message,
            Err(Problem::Stopped) => return Flow::Stopped,
            Err(Problem::Message(e)) => return self.fail(scope, step, path, e),
        };
        let asked = ask(Question { variable: key.to_string(), message, secret: s.secret });
        let answer = tokio::select! {
            answer = asked => answer.ok().flatten(),
            _ = self.cancel.cancelled() => return Flow::Stopped,
        };
        let Some(answer) = answer else {
            return self.fail(scope, step, path, format!("No value was given for '{key}'"));
        };
        let value = Value::String(answer);
        if s.secret {
            self.secret(scope, key, &value);
        }
        result.step.as_mut().expect("a step row").saved = vec![self.shown(scope, key, &value)];
        scope.set(key, value);
        scope.emit(result);
        Flow::Go
    }
}

/// Why a value could not be worked out.
enum Problem {
    Message(String),
    /// The run was stopped meanwhile.
    Stopped,
}

/// The name in a text that is one `{{name}}` and nothing else.
fn single_variable(text: &str) -> Option<&str> {
    let inner = text.trim().strip_prefix("{{")?.strip_suffix("}}")?;
    (!inner.contains("{{") && !inner.contains("}}") && !inner.trim().is_empty()).then(|| inner.trim())
}

fn item_name(s: &StepForEach) -> &str {
    match s.item.trim() {
        "" => "item",
        name => name,
    }
}

/// What steps that ran at the same time changed (compared with `before`), put into `into`.
fn merge(into: &mut ScriptVars, before: &ScriptVars, branch: &ScriptVars) {
    fn map<V: Clone + PartialEq>(
        into: &mut BTreeMap<String, V>,
        before: &BTreeMap<String, V>,
        branch: &BTreeMap<String, V>,
    ) {
        for (key, value) in branch {
            if before.get(key) != Some(value) {
                into.insert(key.clone(), value.clone());
            }
        }
        for key in before.keys().filter(|key| !branch.contains_key(*key)) {
            into.remove(key);
        }
    }
    let (into, before, branch) = (&mut into.values, &before.values, &branch.values);
    map(&mut into.local, &before.local, &branch.local);
    map(&mut into.environment, &before.environment, &branch.environment);
    map(&mut into.collection, &before.collection, &branch.collection);
    map(&mut into.globals, &before.globals, &branch.globals);
    map(&mut into.typed, &before.typed, &branch.typed);
}

/// The words in `code` that could be variable names (not after a `.`, but after `...`).
fn identifiers(code: &str) -> Vec<&str> {
    let mut out: Vec<&str> = Vec::new();
    let bytes = code.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c.is_ascii_alphabetic() || c == b'_' || c == b'$' {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'$') {
                i += 1;
            }
            let before = code[..start].trim_end();
            let member = before.ends_with('.') && !before.ends_with("...");
            let word = &code[start..i];
            if !member && !out.contains(&word) {
                out.push(word);
            }
        } else if c.is_ascii_digit() {
            // A number (`1e5`, `0x1F`) is not a name.
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'.') {
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    out
}

/// Words an expression can't take a variable as: JavaScript's own, and what scripts have.
const RESERVED: &[&str] = &[
    "vars",
    "pm",
    "postman",
    "console",
    "require",
    "arguments",
    "eval",
    "undefined",
    "NaN",
    "Infinity",
    "globalThis",
    "Number",
    "String",
    "Boolean",
    "Array",
    "Object",
    "JSON",
    "Math",
    "Date",
    "RegExp",
    "Error",
    "Map",
    "Set",
    "Promise",
    "Symbol",
    "BigInt",
    "Buffer",
    "parseInt",
    "parseFloat",
    "isNaN",
    "isFinite",
    "encodeURIComponent",
    "decodeURIComponent",
    "encodeURI",
    "decodeURI",
    "atob",
    "btoa",
    "setTimeout",
    "setInterval",
    "clearTimeout",
    "clearInterval",
    "_",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "else",
    "enum",
    "export",
    "extends",
    "false",
    "finally",
    "for",
    "function",
    "if",
    "import",
    "in",
    "instanceof",
    "let",
    "new",
    "null",
    "return",
    "static",
    "super",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "typeof",
    "var",
    "void",
    "while",
    "with",
    "yield",
    "await",
    "async",
    "of",
    "implements",
    "interface",
    "package",
    "private",
    "protected",
    "public",
];

/// "{{total}} is greater than 100".
fn describe_rule(rule: &Rule) -> String {
    match rule.op.takes_value() {
        true => format!("{} {} {}", short(rule.value.trim()), rule.op.words(), short(rule.to.trim())),
        false => format!("{} {}", short(rule.value.trim()), rule.op.words()),
    }
}

fn describe(condition: &Condition) -> String {
    let rules: Vec<String> = condition.rules.iter().map(describe_rule).collect();
    match rules.is_empty() {
        true => "always".into(),
        false => rules.join(if condition.any { " or " } else { " and " }),
    }
}

/// "Status", "$.items[0].id", "Header ETag".
fn describe_source(from: ValueFrom, path: &str) -> String {
    let path = path.trim();
    match from {
        ValueFrom::Json => path.to_string(),
        ValueFrom::Header => format!("header {path}"),
        ValueFrom::Cookie => format!("cookie {path}"),
        ValueFrom::Regex => format!("/{path}/"),
        ValueFrom::Status => "status".into(),
        ValueFrom::Body => "body".into(),
        ValueFrom::Time => "time (ms)".into(),
    }
}

/// The name of a check's test: "Status equals 201".
fn describe_check(check: &StepCheck) -> String {
    let source = describe_source(check.from, &check.path);
    let mut source = source.chars();
    let source: String = source.next().map(|c| c.to_uppercase().chain(source).collect()).unwrap_or_default();
    match check.op.takes_value() {
        true => format!("{source} {} {}", check.op.words(), short(check.value.trim())),
        false => format!("{source} {}", check.op.words()),
    }
}

/// A value in a message: text in quotes, the rest as JSON, long ones cut.
fn quote(value: &Value) -> String {
    match value {
        Value::String(s) => format!("\"{}\"", short(s)),
        other => short(&other.to_string()),
    }
}

/// Reads values out of a response; the body is parsed as JSON once, when first needed.
struct Reader<'a> {
    response: &'a HttpResponse,
    json: Option<Option<Value>>,
}

impl Reader<'_> {
    /// The value at `path` of `from`; `None` when there is nothing there.
    fn read(&mut self, from: ValueFrom, path: &str) -> Result<Option<Value>, String> {
        let path = path.trim();
        let response = self.response;
        Ok(match from {
            ValueFrom::Json => {
                let pick = JsonPick::parse(path)?;
                let json = self.json.get_or_insert_with(|| serde_json::from_slice(&response.body).ok());
                match json {
                    Some(root) => pick.get(root),
                    None => return Err("The response body is not JSON".into()),
                }
            }
            ValueFrom::Header if path.is_empty() => return Err("Give the header's name".into()),
            ValueFrom::Header => response
                .meta
                .headers
                .iter()
                .find(|h| h.name.eq_ignore_ascii_case(path))
                .map(|h| Value::String(h.value.clone())),
            ValueFrom::Cookie if path.is_empty() => return Err("Give the cookie's name".into()),
            ValueFrom::Cookie => {
                response.meta.cookies.iter().find(|c| c.name == path).map(|c| Value::String(c.value.clone()))
            }
            ValueFrom::Regex if path.is_empty() => return Err("Give a regular expression".into()),
            ValueFrom::Regex => {
                let re = regex::bytes::RegexBuilder::new(path)
                    .size_limit(1 << 20)
                    .build()
                    .map_err(|e| format!("'{}' is not a regular expression: {e}", short(path)))?;
                re.captures(&response.body).and_then(|caps| {
                    let found = if re.captures_len() > 1 { caps.get(1)? } else { caps.get(0)? };
                    Some(Value::String(String::from_utf8_lossy(found.as_bytes()).into_owned()))
                })
            }
            ValueFrom::Status => Some(Value::from(response.meta.status)),
            ValueFrom::Body => Some(Value::String(String::from_utf8_lossy(&response.body).into_owned())),
            ValueFrom::Time => Some(Value::from(response.timing.total_ms.round() as u64)),
        })
    }
}

// ---- app API --------------------------------------------------------------------------------

/// A value of a scenario as the app shows it (`scenario.values`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ScenarioValue {
    pub key: String,
    /// As text (lists and objects as JSON); hidden for secret values.
    pub value: String,
    /// `text`, `number`, `boolean`, `list` or `object`.
    pub kind: String,
    pub secret: bool,
}

#[derive(Default)]
pub(crate) struct ScenarioState {
    /// What the last run of each scenario left in its values, by scenario id. In memory
    /// only: they can hold tokens.
    kept: Mutex<HashMap<String, Kept>>,
    /// The question a running scenario waits on: its run, and where the answer goes.
    asking: Mutex<Option<(String, oneshot::Sender<Option<String>>)>>,
}

impl ScenarioState {
    /// Another workspace: the values of the last one's scenarios go.
    pub(crate) fn clear(&self) {
        lock(&self.kept).clear();
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StartParams {
    pub id: String,
    /// The scenario as edited (default: as saved).
    pub scenario: Option<Scenario>,
    /// Run up to and including the step at this path.
    pub to: Option<String>,
    /// Run only the step at this path, with the values the last run left.
    pub only: Option<String>,
    /// Values for this run: they win over the scenario's own and every other variable.
    #[serde(default)]
    pub variables: BTreeMap<String, String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct IdParam {
    id: String,
}

impl Api {
    /// `scenario.*` methods.
    pub(crate) async fn call_scenario(&self, method: &str, p: Value) -> ApiResult<Value> {
        match method {
            // The scenarios and their folders, as a tree.
            "scenario.list" => ok(self.ws()?.scenario_tree()?),
            "scenario.read" => {
                let IdParam { id } = params(p)?;
                ok(self.ws()?.read_scenario(&id)?)
            }
            "scenario.create" => {
                #[derive(Deserialize)]
                struct P {
                    scenario: Scenario,
                    /// The folder it goes in ("" or missing: the top level).
                    #[serde(default)]
                    folder: String,
                }
                let P { scenario, folder } = params(p)?;
                ok(self.ws()?.create_scenario(&folder, &scenario)?)
            }
            "scenario.save" => {
                #[derive(Deserialize)]
                struct P {
                    id: String,
                    scenario: Scenario,
                }
                let P { id, scenario } = params(p)?;
                let ws = self.ws()?;
                let new_id = ws.save_scenario(&id, &scenario)?;
                self.scenarios_moved(&ws, &id, &new_id, ScenarioItem::Scenario);
                ok(new_id)
            }
            "scenario.duplicate" => {
                let IdParam { id } = params(p)?;
                ok(self.ws()?.duplicate_scenario(&id)?)
            }
            "scenario.delete" => {
                let IdParam { id } = params(p)?;
                let ws = self.ws()?;
                let deleted = id.clone();
                tokio::task::spawn_blocking(move || ws.delete_scenario(&deleted)).await.map_err(crate::join_err)??;
                lock(&self.inner.scenarios.kept).remove(&id);
                ok(())
            }
            "scenario.createFolder" => {
                #[derive(Deserialize)]
                struct P {
                    #[serde(default)]
                    parent: String,
                    name: String,
                }
                let P { parent, name } = params(p)?;
                ok(self.ws()?.create_scenario_folder(&parent, &name)?)
            }
            "scenario.renameFolder" => {
                #[derive(Deserialize)]
                struct P {
                    id: String,
                    name: String,
                }
                let P { id, name } = params(p)?;
                let ws = self.ws()?;
                let new_id = ws.rename_scenario_folder(&id, &name)?;
                self.scenarios_moved(&ws, &id, &new_id, ScenarioItem::Folder);
                ok(new_id)
            }
            "scenario.deleteFolder" => {
                let IdParam { id } = params(p)?;
                let ws = self.ws()?;
                let deleted = id.clone();
                tokio::task::spawn_blocking(move || ws.delete_scenario_folder(&deleted))
                    .await
                    .map_err(crate::join_err)??;
                lock(&self.inner.scenarios.kept).retain(|kept, _| !in_folder(kept, &id));
                ok(())
            }
            // A scenario or a folder to another folder, or to another place in its own.
            "scenario.move" => {
                #[derive(Deserialize)]
                struct P {
                    id: String,
                    #[serde(default)]
                    folder: bool,
                    #[serde(default)]
                    parent: String,
                    index: Option<usize>,
                }
                let P { id, folder, parent, index } = params(p)?;
                let item = if folder { ScenarioItem::Folder } else { ScenarioItem::Scenario };
                let ws = self.ws()?;
                let new_id = ws.move_scenario(&id, item, &parent, index)?;
                self.scenarios_moved(&ws, &id, &new_id, item);
                ok(new_id)
            }
            "scenario.run" => ok(self.scenario_start(params(p)?).await?),
            // The value an "ask" step waits for (`null`: none, the step fails).
            "scenario.answer" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct P {
                    run_id: String,
                    value: Option<String>,
                }
                let P { run_id, value } = params(p)?;
                let mut asking = lock(&self.inner.scenarios.asking);
                if asking.as_ref().is_some_and(|(id, _)| *id == run_id)
                    && let Some((_, answer)) = asking.take()
                {
                    let _ = answer.send(value);
                }
                ok(())
            }
            "scenario.values" => {
                let IdParam { id } = params(p)?;
                ok(self.scenario_values(&id))
            }
            // Forget the values the last run left (all of them, or one).
            "scenario.clearValues" => {
                #[derive(Deserialize)]
                struct P {
                    id: String,
                    key: Option<String>,
                }
                let P { id, key } = params(p)?;
                let mut kept = lock(&self.inner.scenarios.kept);
                match key {
                    Some(key) => {
                        if let Some(values) = kept.get_mut(&id) {
                            values.values.remove(&key);
                            values.secret.remove(&key);
                        }
                    }
                    None => drop(kept.remove(&id)),
                }
                ok(())
            }
            other => Err(ApiError::new("notFound", format!("Unknown method '{other}'"))),
        }
    }

    /// The values the last run of a scenario left, secret ones hidden.
    pub(crate) fn scenario_values(&self, id: &str) -> Vec<ScenarioValue> {
        // Secret values inside other values (a header built from one) show as `{{name}}`.
        let mut secrets: Vec<(String, String)> = match self.try_ws() {
            Some(ws) => self
                .active_env_vars(&ws)
                .into_iter()
                .chain(self.workspace_vars(&ws))
                .filter(|v| v.secret && v.enabled)
                .map(|v| (v.key, v.value))
                .collect(),
            None => Vec::new(),
        };
        let kept = lock(&self.inner.scenarios.kept);
        let Some(kept) = kept.get(id) else { return Vec::new() };
        secrets.extend(kept.secret.iter().filter_map(|key| Some((key.clone(), value_text(kept.values.get(key)?)))));
        kept.values
            .iter()
            .map(|(key, value)| {
                let secret = kept.secret.contains(key);
                let kind = match value {
                    Value::Number(_) => "number",
                    Value::Bool(_) => "boolean",
                    Value::Array(_) => "list",
                    Value::Object(_) => "object",
                    Value::String(_) | Value::Null => "text",
                };
                ScenarioValue {
                    key: key.clone(),
                    value: if secret { HIDDEN.to_string() } else { mask_secrets(&value_text(value), &secrets) },
                    kind: kind.to_string(),
                    secret,
                }
            })
            .collect()
    }

    /// A scenario or a folder of them has a new id: the values their last runs left and the
    /// steps that run them follow.
    fn scenarios_moved(&self, ws: &Workspace, old: &str, new: &str, item: ScenarioItem) {
        if old == new {
            return;
        }
        let moved = |id: &str| -> Option<String> {
            match item {
                ScenarioItem::Scenario => (id == old).then(|| new.to_string()),
                ScenarioItem::Folder => {
                    id.strip_prefix(old).filter(|rest| rest.starts_with('/')).map(|rest| format!("{new}{rest}"))
                }
            }
        };
        let mut kept = lock(&self.inner.scenarios.kept);
        let ids: Vec<(String, String)> = kept.keys().filter_map(|id| Some((id.clone(), moved(id)?))).collect();
        for (id, to) in ids {
            if let Some(values) = kept.remove(&id) {
                kept.insert(to, values);
            }
        }
        drop(kept);
        self.rewrite_scenarios(ws, &|_| None, &moved);
    }

    /// Keep scenarios pointing at a request or folder that was renamed or moved.
    pub(crate) fn retarget_scenarios(&self, ws: &Workspace, old: &str, new: &str) {
        if old == new {
            return;
        }
        let rewrite = |path: &str| -> Option<String> {
            if path == old {
                Some(new.to_string())
            } else {
                path.strip_prefix(old).filter(|rest| rest.starts_with('/')).map(|rest| format!("{new}{rest}"))
            }
        };
        self.rewrite_scenarios(ws, &rewrite, &|_| None);
    }

    fn rewrite_scenarios(
        &self,
        ws: &Workspace,
        request: &dyn Fn(&str) -> Option<String>,
        call: &dyn Fn(&str) -> Option<String>,
    ) {
        for node in ws.list_scenarios().unwrap_or_default().into_iter().filter(|n| n.error.is_none()) {
            let Ok(mut scenario) = ws.read_scenario(&node.id) else { continue };
            // In place: a rename here would change the id under the open tab.
            if scenario.retarget(request, call)
                && let Err(e) = ws.update_scenario(&node.id, &scenario)
            {
                tracing::warn!("could not update scenario '{}': {}", node.id, e.message);
            }
        }
    }

    /// Read what a run of `scenario` needs and check it can start.
    pub(crate) async fn scenario_plan(
        &self,
        ws: &Workspace,
        scenario: Scenario,
        mode: RunMode,
    ) -> ApiResult<ScenarioPlan> {
        if let Some(problem) = scenario.problem() {
            return Err(ApiError::invalid(problem));
        }
        if let RunMode::To(path) | RunMode::Only(path) = &mode
            && scenario.step_at(path).is_none()
        {
            return Err(ApiError::invalid("That step is no longer in the scenario"));
        }
        if scenario.steps.iter().chain(&scenario.always).all(|s| !s.enabled) {
            return Err(ApiError::invalid(format!("'{}' has no steps to run", scenario.name)));
        }
        let settings = self.settings();
        let data = match scenario.data_file.clone().filter(|f| !f.trim().is_empty()) {
            // Up to 50 MB to read and parse: not on the async workers.
            Some(file) => {
                let (root, outside) = (ws.root().to_path_buf(), settings.files_outside_workspace);
                tokio::task::spawn_blocking(move || read_data_file(&file, &root, outside))
                    .await
                    .map_err(crate::join_err)?
                    .map_err(ApiError::invalid)?
                    .0
                    .rows
            }
            None => Vec::new(),
        };
        // A part of the scenario runs once, with the first row.
        let iterations = match mode {
            RunMode::All => iteration_count(scenario.iterations, data.len()),
            RunMode::To(_) | RunMode::Only(_) => 1,
        };
        if !(1..=MAX_ITERATIONS).contains(&iterations) {
            return Err(ApiError::invalid(format!("Iterations must be between 1 and {MAX_ITERATIONS}")));
        }
        let secrets = self
            .active_env_vars(ws)
            .into_iter()
            .chain(self.workspace_vars(ws))
            .filter(|v| v.secret && v.enabled)
            .map(|v| (v.key, v.value))
            .collect();
        let (requests, scenarios) = {
            let (ws, scenario) = (ws.clone(), scenario.clone());
            tokio::task::spawn_blocking(move || collect(&ws, &scenario)).await.map_err(crate::join_err)?
        };
        Ok(ScenarioPlan {
            scenario,
            requests,
            scenarios,
            iterations,
            data,
            allow_http_errors: false,
            secrets,
            mode,
            start: None,
            ask: None,
        })
    }

    pub(crate) async fn scenario_start(&self, p: StartParams) -> ApiResult<RunStarted> {
        let ws = self.ws()?;
        let settings = self.settings();
        let scenario = match p.scenario {
            Some(scenario) => scenario,
            None => ws.read_scenario(&p.id)?,
        };
        let mode = match (p.to, p.only) {
            (_, Some(path)) => RunMode::Only(path),
            (Some(path), None) => RunMode::To(path),
            (None, None) => RunMode::All,
        };
        for key in p.variables.keys() {
            variable_name(key).map_err(ApiError::invalid)?;
        }
        let mut plan = self.scenario_plan(&ws, scenario, mode).await?;
        if matches!(plan.mode, RunMode::Only(_)) {
            plan.start = lock(&self.inner.scenarios.kept).get(&p.id).cloned();
        }
        let mut vars = self.script_vars(&ws);
        vars.values.overrides = p.variables;
        let cancel = CancellationToken::new();
        let run_id = uuid::Uuid::new_v4().to_string();
        let agent = crate::agents::in_agent_call();
        // Someone watches a run started in the app: they can be asked. An agent's run gets
        // its values up front.
        if !agent {
            let (api, id) = (self.clone(), run_id.clone());
            plan.ask = Some(Arc::new(move |question: Question| {
                let (answer, asked) = oneshot::channel();
                *lock(&api.inner.scenarios.asking) = Some((id.clone(), answer));
                let Question { variable, message, secret } = question;
                let event = RunEvent::Ask { variable, message, secret };
                api.inner.sink.emit(StreamEvent::Runner { run_id: id.clone(), event, agent: false });
                asked
            }));
        }
        self.claim_run_slot(&run_id, &cancel)?;
        let started = RunStarted {
            run_id: run_id.clone(),
            name: plan.scenario.name.clone(),
            total: 0,
            iterations: plan.iterations,
            environment: vars.environment.as_ref().map(|(_, name)| name.clone()),
        };
        self.inner.sink.emit(StreamEvent::Runner {
            run_id: run_id.clone(),
            event: RunEvent::Started { name: started.name.clone(), total: 0, iterations: plan.iterations },
            agent,
        });
        let api = self.clone();
        // The run outlives the agent's call: it takes the call's host limits along.
        let agent = agent.then(|| AgentRun { guard: crate::agents::scope_guard() });
        let id = p.id;
        tokio::spawn(
            async move { api.scenario_in_background(run_id, id, ws, settings, plan, vars, cancel, agent).await },
        );
        Ok(started)
    }

    #[allow(clippy::too_many_arguments)]
    async fn scenario_in_background(
        &self,
        run_id: String,
        id: String,
        ws: Workspace,
        settings: zorvik_workspace::settings::Settings,
        plan: ScenarioPlan,
        vars: ScriptVars,
        cancel: CancellationToken,
        agent: Option<AgentRun>,
    ) {
        let by_agent = agent.is_some();
        let _slot = RunSlot { api: self, run_id: &run_id, agent: by_agent };
        let meta = self.revealed_meta(&ws);
        let jar = settings.cookie_jar.then(|| self.jar(&ws));
        let cx = SendContext {
            ws: &ws,
            meta: &meta,
            client: &self.inner.client,
            settings: &settings,
            tokens: &self.inner.tokens,
            jar: jar.as_ref(),
            guard: agent.and_then(|a| a.guard),
            specs: Some(&self.inner.specs),
            // Runs an agent starts never start programs: nobody is asked about each one.
            programs: match by_agent {
                true => crate::mcp::Programs::Approved(None),
                false => crate::mcp::Programs::Trusted(&self.inner.programs),
            },
            mcp_session: None,
            console: Some(crate::console::ConsoleScope::new(
                self.inner.console.clone(),
                if by_agent { crate::ConsoleSource::Agent } else { crate::ConsoleSource::Runner },
                Some(self.redactor(&ws)),
            )),
        };
        let sink = self.inner.sink.clone();
        // Nothing a scenario's scripts set is kept outside of it: its values stay its own.
        let ran = Box::pin(run(&cx, &plan, &vars, &cancel, |result| {
            let mut result = result.clone();
            if let Some(details) = result.details.take() {
                self.keep_run_details(&run_id, result.seq, details);
            }
            let event = RunEvent::Result { result: Box::new(result) };
            sink.emit(StreamEvent::Runner { run_id: run_id.clone(), event, agent: by_agent });
        }))
        .await;
        if let Some(jar) = &jar {
            self.save_jar(&ws, jar);
        }
        lock(&self.inner.scenarios.asking).take_if(|(asking, _)| *asking == run_id);
        // Still the same workspace: its scenario keeps what the run left.
        if self.try_ws().is_some_and(|open| open.root() == ws.root()) {
            lock(&self.inner.scenarios.kept).insert(id, ran.kept);
        }
        self.finish_run(&run_id, ran.report, by_agent);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_in_expressions() {
        assert_eq!(identifiers("total + user.id * 2 - vars.count + $x"), ["total", "user", "vars", "$x"]);
        assert_eq!(identifiers("1e5 + 0x1F + a1"), ["a1"]);
        assert_eq!(identifiers("items.filter(i => i.ok).length"), ["items", "i"]);
        assert_eq!(identifiers("Math.max(...prices, ... more) + a?.b"), ["Math", "prices", "more", "a"]);
        assert_eq!(single_variable(" {{ cartId }} "), Some("cartId"));
        assert_eq!(single_variable("{{a}}-{{b}}"), None);
        assert_eq!(single_variable("x{{a}}"), None);
    }

    #[test]
    fn descriptions() {
        let rule = |value: &str, op, to: &str| Rule { value: value.into(), op, to: to.into() };
        use zorvik_workspace::formats::CompareOp::*;
        let condition =
            Condition { any: false, rules: vec![rule("{{total}}", Greater, "100"), rule("{{ok}}", IsTrue, "")] };
        assert_eq!(describe(&condition), "{{total}} is greater than 100 and {{ok}} is true");
        assert_eq!(describe(&Condition { any: true, ..condition }), "{{total}} is greater than 100 or {{ok}} is true");
        assert_eq!(describe(&Condition::default()), "always");
        let check = |from, path: &str, op, value: &str| StepCheck {
            from,
            path: path.into(),
            op,
            value: value.into(),
            enabled: true,
        };
        assert_eq!(describe_check(&check(ValueFrom::Status, "", Equals, "201")), "Status equals 201");
        assert_eq!(describe_check(&check(ValueFrom::Json, "$.id", Exists, "")), "$.id exists");
        assert_eq!(describe_check(&check(ValueFrom::Header, "ETag", Contains, "W/")), "Header ETag contains W/");
        assert_eq!(describe_check(&check(ValueFrom::Time, "", Less, "500")), "Time (ms) is less than 500");
    }

    #[test]
    fn parallel_changes_are_put_together() {
        let mut before = ScriptVars::default();
        before.values.local.insert("kept".into(), "1".into());
        before.values.local.insert("gone".into(), "1".into());
        let mut a = before.clone();
        a.values.local.insert("a".into(), "from a".into());
        a.values.local.insert("both".into(), "a".into());
        a.values.local.remove("gone");
        let mut b = before.clone();
        b.values.local.insert("both".into(), "b".into());
        b.values.environment.insert("env".into(), "set".into());
        let mut into = before.clone();
        merge(&mut into, &before, &a);
        merge(&mut into, &before, &b);
        let local: Vec<(&str, &str)> = into.values.local.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        assert_eq!(local, [("a", "from a"), ("both", "b"), ("kept", "1")]);
        assert_eq!(into.values.environment.get("env").map(String::as_str), Some("set"));
    }
}

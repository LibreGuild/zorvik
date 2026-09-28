//! Collection runner (docs/architecture.md, "Collection runner"): the HTTP requests of a folder
//! (or a chosen list) in order, once per iteration, each through
//! [`send_scripted`] so scripts, tests and `pm.variables` work as in a single
//! send. [`run`] is shared by the app (`runner.*`) and `zorvik run`; so are the
//! data files (CSV/JSON) and the reports (JSON, JUnit XML).
//!
//! A request fails when it can't be sent, a script fails or a test fails; an HTTP
//! status of 400 or more fails it only when it has no tests (and HTTP errors are
//! not allowed). Other request kinds (WebSocket, gRPC, …) are reported as skipped.
//! `pm.execution.setNextRequest(name)` jumps to that request, `null` ends the iteration.

mod data;
mod junit;

use std::collections::VecDeque;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_util::sync::CancellationToken;
use ts_rs::TS;
use zorvik_script::{ConsoleEntry, ConsoleLevel, NextRequest, TestResult};
use zorvik_workspace::Workspace;
use zorvik_workspace::formats::{NodeKind, Request, RequestKind, TreeNode};

pub use data::{DataFile, DataPreview, DataRow, MAX_DATA_FILE, data_format, parse_data};
pub use junit::junit_xml;

use crate::scripting::send_scripted_with;
use crate::{Api, ApiError, ApiResult, Iteration, ScriptVars, SendContext, StreamEvent, lock, ok, params};

/// Iterations per run at most.
pub const MAX_ITERATIONS: u32 = 100_000;
/// Longest pause between two requests (10 minutes).
pub const MAX_DELAY_MS: u64 = 600_000;
/// Requests one iteration may send (a `setNextRequest` loop ends here).
const MAX_STEPS: usize = 10_000;
/// Console lines kept per result, and characters per line.
const MAX_CONSOLE: usize = 200;
const MAX_CONSOLE_LINE: usize = 4096;
/// Finished runs kept for `runner.export`.
const KEEP_FINISHED: usize = 5;
/// Results a report keeps: every one up to this many, then failed ones only, up to
/// twice as many. The summary counts all of them (`omitted`: those left out), so a
/// long run × many iterations can't fill the memory.
pub const MAX_KEPT_RESULTS: usize = 50_000;

/// A request of the run, read when the run starts.
#[derive(Debug, Clone)]
pub struct RunItem {
    /// Path relative to `requests/`.
    pub path: String,
    pub name: String,
    /// The saved request, or why it can't be read (reported in every iteration).
    pub request: Result<Request, String>,
}

/// What to run and how.
#[derive(Debug, Clone, Default)]
pub struct RunPlan {
    /// The folder's name, or the workspace's for the whole collection.
    pub name: String,
    pub items: Vec<RunItem>,
    pub iterations: u32,
    /// Pause between two requests.
    pub delay: Duration,
    /// Data file rows: iteration `i` gets row `i`, the last row once they run out.
    pub data: Vec<DataRow>,
    pub stop_on_failure: bool,
    /// HTTP 4xx/5xx responses of requests without tests don't fail them.
    pub allow_http_errors: bool,
    /// Secret variables `(key, value)`: reported URLs show `{{key}}` instead of the value.
    pub secrets: Vec<(String, String)>,
}

impl RunPlan {
    /// Requests the run sends if every iteration goes through the list once.
    pub fn total(&self) -> u32 {
        self.iterations.saturating_mul(self.items.len() as u32)
    }
}

/// Iterations: as asked, else one per data row (at least one).
pub fn iteration_count(requested: Option<u32>, rows: usize) -> u32 {
    requested.unwrap_or_else(|| rows.clamp(1, MAX_ITERATIONS as usize) as u32)
}

/// One request of one iteration.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RunResult {
    /// 0-based.
    pub iteration: u32,
    /// Path relative to `requests/`.
    pub path: String,
    pub name: String,
    pub kind: RequestKind,
    pub method: String,
    /// The URL as sent, with secret values shown as `{{name}}`; the saved URL when nothing was sent.
    pub url: String,
    pub status: Option<u16>,
    pub duration_ms: Option<f64>,
    /// Response body bytes.
    pub size: Option<u32>,
    pub tests: Vec<TestResult>,
    /// Console output of the scripts (the first lines).
    pub console: Vec<ConsoleEntry>,
    /// Why there is no response: network or resolve error, pre-request script error.
    pub error: Option<String>,
    /// Post-response scripts that failed.
    pub script_errors: Vec<String>,
    /// Variables that were referenced but not defined.
    pub unresolved: Vec<String>,
    pub passed: bool,
    /// Not a kind the runner sends: not sent.
    pub skipped: bool,
    /// Why it was skipped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub skip_reason: Option<String>,
    /// Sends made for "repeat until" (the result is the last one's).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub attempts: Option<u32>,
}

/// Counts of one iteration.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct IterationSummary {
    /// 0-based.
    pub iteration: u32,
    pub requests: u32,
    pub failed: u32,
    pub skipped: u32,
    pub tests_passed: u32,
    pub tests_failed: u32,
    pub duration_ms: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RunSummary {
    /// The folder's name, or the workspace's for the whole collection.
    pub name: String,
    pub environment: Option<String>,
    /// Unix epoch milliseconds.
    pub started_at: f64,
    pub duration_ms: f64,
    /// Iterations planned.
    pub iterations: u32,
    /// Requests sent or tried (skipped ones not counted).
    pub requests: u32,
    /// Requests that failed.
    pub failed: u32,
    pub skipped: u32,
    pub tests_passed: u32,
    pub tests_failed: u32,
    pub tests_skipped: u32,
    /// Stopped by the user.
    pub stopped: bool,
    /// Stopped at the first failure (`stopOnFailure`).
    pub bailed: bool,
    /// Why the run ended early otherwise (e.g. a `setNextRequest` loop).
    pub error: Option<String>,
    /// No request failed and the run was not cut short by an error.
    pub passed: bool,
    pub per_iteration: Vec<IterationSummary>,
    /// Results left out of `results`: a report keeps the first 50,000, then failed ones only (counted above).
    pub omitted: u32,
}

/// The JSON report: summary and every result.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RunReport {
    pub summary: RunSummary,
    pub results: Vec<RunResult>,
}

/// Events of a run (`runner {runId, event}`).
#[derive(Debug, Clone, Serialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(export)]
pub enum RunEvent {
    #[serde(rename_all = "camelCase")]
    Started { name: String, total: u32, iterations: u32 },
    #[serde(rename_all = "camelCase")]
    Result { result: Box<RunResult> },
    #[serde(rename_all = "camelCase")]
    Finished { summary: RunSummary },
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RunStarted {
    pub run_id: String,
    pub name: String,
    /// Requests if every iteration goes through the list once.
    pub total: u32,
    pub iterations: u32,
    pub environment: Option<String>,
}

// ---- planning -------------------------------------------------------------------------------

/// The requests to run: every request under `folder` ("" = the whole collection)
/// in sidebar order, or `only` (paths, in that order). Returns the run's name too.
pub fn collect(ws: &Workspace, folder: &str, only: Option<&[String]>) -> Result<(String, Vec<RunItem>), String> {
    let tree = ws.tree().map_err(|e| e.message)?;
    let folder = folder.trim_matches('/');
    let (name, nodes) = if folder.is_empty() {
        (ws.meta().name.clone(), tree.as_slice())
    } else {
        let node = find_folder(&tree, folder).ok_or_else(|| format!("Folder '{folder}' not found"))?;
        (node.name.clone(), node.children.as_slice())
    };
    let item = |path: &str, node: Option<&TreeNode>| {
        let name = node.map(|n| n.name.clone()).unwrap_or_else(|| file_stem(path));
        let request = match node.and_then(|n| n.error.clone()) {
            Some(error) => Err(error),
            None => ws.read_request(path).map_err(|e| e.message),
        };
        RunItem { path: path.to_string(), name, request }
    };
    let items = match only {
        Some(paths) => paths.iter().map(|p| item(p, find_request(&tree, p))).collect(),
        None => {
            let mut nodes_in_order = Vec::new();
            requests_under(nodes, &mut nodes_in_order);
            nodes_in_order.into_iter().map(|n| item(&n.path, Some(n))).collect()
        }
    };
    Ok((name, items))
}

fn requests_under<'a>(nodes: &'a [TreeNode], out: &mut Vec<&'a TreeNode>) {
    for n in nodes {
        match n.kind {
            NodeKind::Folder => requests_under(&n.children, out),
            NodeKind::Request => out.push(n),
        }
    }
}

fn find_folder<'a>(nodes: &'a [TreeNode], path: &str) -> Option<&'a TreeNode> {
    nodes.iter().find_map(|n| match n.kind {
        NodeKind::Folder if n.path == path => Some(n),
        NodeKind::Folder => find_folder(&n.children, path),
        NodeKind::Request => None,
    })
}

fn find_request<'a>(nodes: &'a [TreeNode], path: &str) -> Option<&'a TreeNode> {
    nodes.iter().find_map(|n| match n.kind {
        NodeKind::Request if n.path == path => Some(n),
        NodeKind::Request => None,
        NodeKind::Folder => find_request(&n.children, path),
    })
}

fn file_stem(path: &str) -> String {
    Path::new(path).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| path.to_string())
}

/// Read a data file for the app: relative to the workspace folder or absolute, and
/// (like body files) inside the workspace unless files outside it are allowed.
pub fn read_data_file(path: &str, base_dir: &Path, outside_files: bool) -> Result<(DataFile, &'static str), String> {
    let p = Path::new(path.trim());
    let full = if p.is_absolute() { p.to_path_buf() } else { base_dir.join(p) };
    let meta = std::fs::metadata(&full).map_err(|e| format!("Data file '{}': {e}", full.display()))?;
    // Canonical paths: `..` and links that lead out of the workspace count as outside.
    let inside = || Some(std::fs::canonicalize(&full).ok()?.starts_with(std::fs::canonicalize(base_dir).ok()?));
    if !outside_files && inside() != Some(true) {
        return Err(format!(
            "Data file '{}' is outside the workspace folder. Move it into the workspace, \
             or allow files outside the workspace in Settings → Data & privacy.",
            full.display()
        ));
    }
    if !meta.is_file() {
        return Err(format!("Data file '{}' is not a regular file", full.display()));
    }
    if meta.len() > MAX_DATA_FILE {
        return Err(format!("Data file '{}' is larger than 50 MB", full.display()));
    }
    let bytes = std::fs::read(&full).map_err(|e| format!("Could not read '{}': {e}", full.display()))?;
    let name = full.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    Ok((parse_data(&name, &bytes)?, data_format(&name, &bytes)))
}

/// `url` with secret values put back as `{{name}}` (plain and URL-encoded), like history.
/// Longer values go first, so a secret that contains another one is hidden whole.
pub fn mask_secrets(url: &str, secrets: &[(String, String)]) -> String {
    let mut secrets: Vec<&(String, String)> = secrets.iter().filter(|(_, v)| v.len() >= 3).collect();
    secrets.sort_by_key(|(_, v)| std::cmp::Reverse(v.len()));
    let mut url = url.to_string();
    for (key, value) in secrets {
        let encoded: String = url::form_urlencoded::byte_serialize(value.as_bytes()).collect();
        // Spaces as `+` (forms) and as `%20` (paths, most clients' queries).
        let spaces = encoded.replace('+', "%20");
        for form in [value, &encoded, &spaces] {
            url = url.replace(form.as_str(), &format!("{{{{{key}}}}}"));
        }
    }
    url
}

/// Secret values to hide in reported URLs: the values when the run started, and
/// what scripts have set those variables to since (e.g. a token from a login).
fn secret_values(plan: &RunPlan, vars: &ScriptVars) -> Vec<(String, String)> {
    let v = &vars.values;
    let mut out = plan.secrets.clone();
    for (key, _) in &plan.secrets {
        for scope in [&v.overrides, &v.local, &v.environment, &v.collection, &v.globals] {
            if let Some(value) = scope.get(key) {
                out.push((key.clone(), value.clone()));
            }
        }
    }
    out
}

// ---- running --------------------------------------------------------------------------------

/// Run the plan. `vars` carries `pm.variables` and the other scopes through the
/// run; `on_result` sees every result as it comes, with the variables after it
/// (the app keeps what scripts set, then clears `vars.changes`). Cancelling
/// stops at once (the request in flight is not reported).
pub async fn run(
    cx: &SendContext<'_>,
    plan: &RunPlan,
    vars: &mut ScriptVars,
    cancel: &CancellationToken,
    mut on_result: impl FnMut(&RunResult, &mut ScriptVars) + Send,
) -> RunReport {
    let started = Instant::now();
    let started_at = crate::now_ms() as f64;
    let mut summary = RunSummary::default();
    let mut results: Vec<RunResult> = Vec::new();
    let mut iteration_times = Vec::new();
    let (mut stopped, mut bailed, mut error) = (false, false, None);
    let mut sent_any = false;
    'run: for index in 0..plan.iterations {
        let iteration_started = Instant::now();
        summary.per_iteration.push(IterationSummary { iteration: index, ..Default::default() });
        vars.values.data = plan.data.get(index as usize).or(plan.data.last()).cloned().unwrap_or_default();
        let iteration = Iteration { index, count: plan.iterations };
        let mut at = Some(0);
        let mut steps = 0;
        while let Some(item) = at.and_then(|i| plan.items.get(i)) {
            // Stopped between two requests (skipped ones don't wait): nothing more is reported.
            if cancel.is_cancelled() {
                stopped = true;
                iteration_times.push(iteration_started.elapsed());
                break 'run;
            }
            if steps == MAX_STEPS {
                error = Some(format!(
                    "Stopped: iteration {} sent more than {MAX_STEPS} requests (a setNextRequest loop?)",
                    index + 1
                ));
                iteration_times.push(iteration_started.elapsed());
                break 'run;
            }
            steps += 1;
            let request = match &item.request {
                Ok(r) if matches!(r.kind, RequestKind::Http | RequestKind::Sse) => r,
                other => {
                    let mut result = base_result(item, index);
                    match other {
                        Ok(r) => {
                            result.skipped = true;
                            result.skip_reason = Some(skip_reason(r.kind));
                        }
                        Err(e) => result.error = Some(format!("The request can't be read: {e}")),
                    }
                    result.passed = verdict(&result, plan.allow_http_errors);
                    on_result(&result, vars);
                    let failed = !result.passed;
                    count(&mut summary, &result);
                    keep(&mut results, &mut summary.omitted, result, MAX_KEPT_RESULTS);
                    if failed && plan.stop_on_failure {
                        bailed = true;
                        iteration_times.push(iteration_started.elapsed());
                        break 'run;
                    }
                    at = at.map(|i| i + 1);
                    continue;
                }
            };
            if sent_any && !plan.delay.is_zero() {
                tokio::select! {
                    _ = tokio::time::sleep(plan.delay) => {}
                    _ = cancel.cancelled() => {
                        stopped = true;
                        iteration_times.push(iteration_started.elapsed());
                        break 'run;
                    }
                }
            }
            sent_any = true;
            // "Repeat until": send again after a pause while the condition doesn't hold.
            let repeat = request.settings.repeat.as_ref();
            let condition = repeat.and_then(|r| condition_script(&r.condition));
            let repeat_started = Instant::now();
            let mut attempts = 0u32;
            let (mut t0, mut scripted, repeat_failure) = loop {
                attempts += 1;
                let t0 = Instant::now();
                let mut scripted = tokio::select! {
                    // Boxed: the whole pipeline is a big future (debug builds would overflow the stack).
                    r = Box::pin(send_scripted_with(cx, request.clone(), Some(&item.path), vars, iteration, condition.clone())) => r,
                    _ = cancel.cancelled() => {
                        stopped = true;
                        iteration_times.push(iteration_started.elapsed());
                        break 'run;
                    }
                };
                let Some(repeat) = repeat else { break (t0, scripted, None) };
                let check = take_condition(&mut scripted, &repeat.condition);
                let waited = repeat_started.elapsed();
                let interval = Duration::from_millis(repeat.interval_ms);
                match check {
                    Check::Met => break (t0, scripted, None),
                    Check::Broken => break (t0, scripted, Some(String::new())),
                    Check::NotYet(why) if waited + interval > Duration::from_millis(repeat.timeout_ms) => {
                        let what = if repeat.condition.trim().is_empty() {
                            "its tests didn't pass".to_string()
                        } else {
                            format!("`{}` didn't hold", repeat.condition.trim())
                        };
                        let why = why.map(|w| format!(" (last: {w})")).unwrap_or_default();
                        let message = format!(
                            "Repeat until: {what} after {} in {:.1} s{why}",
                            plural(attempts as usize, "send"),
                            waited.as_secs_f64()
                        );
                        break (t0, scripted, Some(message));
                    }
                    Check::NotYet(_) => {}
                }
                tokio::select! {
                    _ = tokio::time::sleep(interval) => {}
                    _ = cancel.cancelled() => {
                        stopped = true;
                        iteration_times.push(iteration_started.elapsed());
                        break 'run;
                    }
                }
            };
            if repeat.is_some() {
                t0 = repeat_started.min(t0);
            }
            let mut result = base_result(item, index);
            if repeat.is_some() {
                result.attempts = Some(attempts);
            }
            let stream = scripted.result.as_ref().ok().and_then(|s| s.stream);
            let report = scripted.report.take().unwrap_or_default();
            let described: Vec<String> = report.errors.iter().map(|e| e.describe()).collect();
            match scripted.result {
                Ok(sent) => {
                    result.method = sent.request.method.clone();
                    result.url = mask_secrets(&sent.url, &secret_values(plan, vars));
                    result.status = Some(sent.response.meta.status);
                    result.duration_ms = Some(sent.response.timing.total_ms);
                    result.size = Some(sent.response.body.len().min(u32::MAX as usize) as u32);
                    result.unresolved = sent.unresolved;
                    result.script_errors = described;
                }
                // `pm.execution.skipRequest()`: not sent, and not a failure.
                Err(e) if e.code == crate::scripting::SKIPPED => {
                    result.skipped = true;
                    result.skip_reason = Some(e.message);
                }
                Err(e) => {
                    result.duration_ms = Some(t0.elapsed().as_secs_f64() * 1000.0);
                    result.script_errors = described.into_iter().filter(|d| *d != e.message).collect();
                    result.error = Some(e.message);
                }
            }
            result.tests = report.tests;
            result.console = bounded_console(report.console);
            if let Some((events, end)) = stream {
                result.console.push(ConsoleEntry { level: ConsoleLevel::Info, message: stream_note(events, end) });
            }
            if let Some(message) = repeat_failure.filter(|m| !m.is_empty())
                && result.error.is_none()
            {
                result.error = Some(message);
            }
            at = match next_step(&plan.items, at.unwrap_or_default(), scripted.next_request.as_ref()) {
                Ok(next) => next,
                Err(name) => {
                    result.console.push(ConsoleEntry {
                        level: ConsoleLevel::Warn,
                        message: format!(
                            "setNextRequest: no request named '{name}' in this run; the iteration ends here."
                        ),
                    });
                    None
                }
            };
            result.passed = verdict(&result, plan.allow_http_errors);
            on_result(&result, vars);
            let failed = !result.passed;
            count(&mut summary, &result);
            keep(&mut results, &mut summary.omitted, result, MAX_KEPT_RESULTS);
            if failed && plan.stop_on_failure {
                bailed = true;
                iteration_times.push(iteration_started.elapsed());
                break 'run;
            }
        }
        iteration_times.push(iteration_started.elapsed());
        if cancel.is_cancelled() {
            stopped = index + 1 < plan.iterations;
            break;
        }
    }
    let environment = vars.environment.as_ref().map(|(_, name)| name.clone());
    for (it, time) in summary.per_iteration.iter_mut().zip(&iteration_times) {
        it.duration_ms = time.as_secs_f64() * 1000.0;
    }
    summary.name = plan.name.clone();
    summary.environment = environment;
    summary.started_at = started_at;
    summary.duration_ms = started.elapsed().as_secs_f64() * 1000.0;
    summary.iterations = plan.iterations;
    summary.stopped = stopped;
    summary.bailed = bailed;
    summary.passed = summary.failed == 0 && error.is_none();
    summary.error = error;
    RunReport { summary, results }
}

/// The name of the test that carries a "repeat until" condition (never reported).
const REPEAT_TEST: &str = "\u{0}repeat until";

/// The condition as a test run after the post-response scripts; `None` when it is empty
/// (then the request's own tests decide).
fn condition_script(condition: &str) -> Option<(String, String)> {
    let condition = condition.trim();
    (!condition.is_empty()).then(|| {
        let name = serde_json::to_string(REPEAT_TEST).unwrap_or_default();
        let code =
            format!("pm.test({name}, function () {{\n  if (!(\n{condition}\n  )) throw new Error('not yet');\n}});");
        ("“repeat until” condition".to_string(), code)
    })
}

enum Check {
    Met,
    /// Not yet, with why (a thrown error other than "not yet").
    NotYet(Option<String>),
    /// The condition itself failed to run (a syntax error): repeating can't help.
    Broken,
}

/// Whether a "repeat until" condition holds after a send, taking its test out of the report.
fn take_condition(scripted: &mut crate::ScriptedSend, condition: &str) -> Check {
    let report = scripted.report.get_or_insert_with(Default::default);
    let marker = report.tests.iter().position(|t| t.name == REPEAT_TEST).map(|i| report.tests.remove(i));
    if report.errors.iter().any(|e| e.script.contains("repeat until")) {
        return Check::Broken;
    }
    let sent = match &scripted.result {
        Ok(sent) => sent,
        Err(e) => return Check::NotYet(Some(e.message.clone())),
    };
    if !condition.trim().is_empty() {
        return match marker {
            Some(t) if t.passed => Check::Met,
            Some(t) => Check::NotYet(t.error.filter(|e| !e.ends_with("not yet"))),
            None => Check::NotYet(None),
        };
    }
    let mut counted = report.tests.iter().filter(|t| !t.skipped).peekable();
    let met = if counted.peek().is_some() { counted.all(|t| t.passed) } else { sent.response.meta.status < 400 };
    if met { Check::Met } else { Check::NotYet(Some(format!("status {}", sent.response.meta.status))) }
}

/// What reading an event stream got, for the result's console.
fn stream_note(events: usize, end: crate::sse_read::SseEnd) -> String {
    use crate::sse_read::SseEnd;
    let why = match end {
        SseEnd::Event => "the awaited event arrived",
        SseEnd::Count => "enough events arrived",
        SseEnd::Timeout => "the time limit",
        SseEnd::Closed => "the server ended the stream",
        SseEnd::NotAStream => "the answer is not an event stream",
    };
    format!("Read {} (stopped: {why})", plural(events, "event"))
}

fn plural(n: usize, what: &str) -> String {
    format!("{n} {what}{}", if n == 1 { "" } else { "s" })
}

/// Why the runner doesn't send a request of `kind`.
fn skip_reason(kind: RequestKind) -> String {
    let what = match kind {
        RequestKind::Websocket => "WebSocket connections are live sessions",
        RequestKind::Tcp => "TCP connections are live sessions",
        RequestKind::Udp => "UDP sockets are live sessions",
        RequestKind::Mqtt => "MQTT clients are live sessions",
        RequestKind::Grpc => "gRPC calls don't run in the collection runner",
        RequestKind::Dns => "DNS queries don't run in the collection runner",
        RequestKind::Http | RequestKind::Sse => "not sent",
    };
    format!("{what}; the runner sends HTTP, GraphQL and SSE requests")
}

fn base_result(item: &RunItem, iteration: u32) -> RunResult {
    let request = item.request.as_ref().ok();
    RunResult {
        iteration,
        path: item.path.clone(),
        name: item.name.clone(),
        kind: request.map(|r| r.kind).unwrap_or_default(),
        method: request.map(|r| r.method.clone()).unwrap_or_default(),
        url: request.map(|r| r.url.clone()).unwrap_or_default(),
        passed: true,
        ..Default::default()
    }
}

/// Whether a result counts as passed (see the module docs).
fn verdict(r: &RunResult, allow_http_errors: bool) -> bool {
    if r.skipped {
        return true;
    }
    if r.error.is_some() || !r.script_errors.is_empty() {
        return false;
    }
    let mut counted = r.tests.iter().filter(|t| !t.skipped).peekable();
    if counted.peek().is_some() {
        return counted.all(|t| t.passed);
    }
    allow_http_errors || r.status.is_none_or(|s| s < 400)
}

/// Where the iteration goes after item `at`: `Ok(Some(i))` item `i`, `Ok(None)` it
/// ends; `Err(name)` names no request of the run (it ends too). A name matches a
/// request's name, or its path (`pm.info.requestId`).
fn next_step(items: &[RunItem], at: usize, next: Option<&NextRequest>) -> Result<Option<usize>, String> {
    match next {
        None => Ok(Some(at + 1)),
        Some(NextRequest { name: None }) => Ok(None),
        Some(NextRequest { name: Some(name) }) => items
            .iter()
            .position(|i| i.name == *name)
            .or_else(|| items.iter().position(|i| i.path == *name))
            .map(Some)
            .ok_or_else(|| name.clone()),
    }
}

fn bounded_console(mut console: Vec<ConsoleEntry>) -> Vec<ConsoleEntry> {
    let dropped = console.len().saturating_sub(MAX_CONSOLE);
    console.truncate(MAX_CONSOLE);
    for entry in &mut console {
        if let Some((cut, _)) = entry.message.char_indices().nth(MAX_CONSOLE_LINE) {
            entry.message.truncate(cut);
            entry.message.push_str("… (truncated)");
        }
    }
    if dropped > 0 {
        console.push(ConsoleEntry {
            level: ConsoleLevel::Warn,
            message: format!("{dropped} more console lines not kept."),
        });
    }
    console
}

/// Add a result to the run's counts and to its iteration's (`per_iteration[r.iteration]`).
fn count(s: &mut RunSummary, r: &RunResult) {
    let tests = |passed: bool| r.tests.iter().filter(|t| !t.skipped && t.passed == passed).count() as u32;
    let (passed_tests, failed_tests) = (tests(true), tests(false));
    s.tests_passed += passed_tests;
    s.tests_failed += failed_tests;
    s.tests_skipped += r.tests.iter().filter(|t| t.skipped).count() as u32;
    let failed = u32::from(!r.passed);
    if r.skipped {
        s.skipped += 1;
    } else {
        s.requests += 1;
        s.failed += failed;
    }
    if let Some(it) = s.per_iteration.get_mut(r.iteration as usize) {
        if r.skipped {
            it.skipped += 1;
        } else {
            it.requests += 1;
            it.failed += failed;
        }
        it.tests_passed += passed_tests;
        it.tests_failed += failed_tests;
    }
}

/// Keep `result` in the report: any while it holds fewer than `limit`, then failed
/// ones only up to twice that; the others are counted in `omitted`.
fn keep(results: &mut Vec<RunResult>, omitted: &mut u32, result: RunResult, limit: usize) {
    if results.len() < limit || (!result.passed && results.len() < 2 * limit) {
        results.push(result);
    } else {
        *omitted += 1;
    }
}

// ---- app API --------------------------------------------------------------------------------

#[derive(Default)]
pub(crate) struct RunnerState {
    /// The run in progress (one at a time): id and its stop handle.
    active: Mutex<Option<(String, CancellationToken)>>,
    /// The last finished runs, oldest first.
    finished: Mutex<VecDeque<(String, Arc<RunReport>)>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StartParams {
    /// Folder to run ("" = the whole collection); names the run.
    #[serde(default)]
    pub folder: String,
    /// Request paths to run, in this order (default: every request under `folder`).
    pub requests: Option<Vec<String>>,
    /// Default: one per data row, or 1.
    pub iterations: Option<u32>,
    #[serde(default)]
    pub delay_ms: u64,
    /// CSV or JSON, relative to the workspace folder or absolute.
    pub data_file: Option<String>,
    #[serde(default)]
    pub stop_on_failure: bool,
    #[serde(default)]
    pub allow_http_errors: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RunIdParam {
    run_id: String,
}

impl Api {
    /// `runner.*` methods.
    pub(crate) async fn call_runner(&self, method: &str, p: Value) -> ApiResult<Value> {
        match method {
            "runner.start" => ok(self.runner_start(params(p)?).await?),
            "runner.stop" => {
                let RunIdParam { run_id } = params(p)?;
                if let Some((_, cancel)) = lock(&self.inner.runner.active).as_ref().filter(|(id, _)| *id == run_id) {
                    cancel.cancel();
                }
                ok(())
            }
            "runner.preview" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct P {
                    data_file: String,
                }
                let P { data_file } = params(p)?;
                let ws = self.ws()?;
                let outside = self.settings().files_outside_workspace;
                let root = ws.root().to_path_buf();
                let (data, format) = tokio::task::spawn_blocking(move || read_data_file(&data_file, &root, outside))
                    .await
                    .map_err(crate::join_err)?
                    .map_err(ApiError::invalid)?;
                ok(data.preview(format))
            }
            "runner.export" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct P {
                    run_id: String,
                    path: String,
                    /// `json` or `junit`.
                    format: String,
                }
                let P { run_id, path, format } = params(p)?;
                let report = lock(&self.inner.runner.finished)
                    .iter()
                    .find(|(id, _)| *id == run_id)
                    .map(|(_, r)| r.clone())
                    .ok_or_else(|| ApiError::new("notFound", "That run is no longer available; run it again"))?;
                let data = match format.as_str() {
                    "json" => {
                        serde_json::to_vec_pretty(&*report).map_err(|e| ApiError::new("internal", e.to_string()))?
                    }
                    "junit" => junit_xml(&report).into_bytes(),
                    other => return Err(ApiError::invalid(format!("Unknown report format '{other}'"))),
                };
                tokio::task::spawn_blocking(move || std::fs::write(&path, data).map_err(|e| (path, e)))
                    .await
                    .map_err(crate::join_err)?
                    .map_err(|(path, e)| ApiError::new("io", format!("Could not save to {path}: {e}")))?;
                ok(())
            }
            other => Err(ApiError::new("notFound", format!("Unknown method '{other}'"))),
        }
    }

    /// Whether `run_id` is the collection run in progress.
    pub(crate) fn run_active(&self, run_id: &str) -> bool {
        lock(&self.inner.runner.active).as_ref().is_some_and(|(id, _)| id == run_id)
    }

    /// The finished runs still kept, oldest first.
    pub(crate) fn finished_runs(&self) -> Vec<Arc<RunReport>> {
        lock(&self.inner.runner.finished).iter().map(|(_, r)| r.clone()).collect()
    }

    /// A finished run, while it is still kept.
    pub(crate) fn finished_run(&self, run_id: &str) -> Option<Arc<RunReport>> {
        lock(&self.inner.runner.finished).iter().find(|(id, _)| id == run_id).map(|(_, r)| r.clone())
    }

    pub(crate) async fn runner_start(&self, p: StartParams) -> ApiResult<RunStarted> {
        let ws = self.ws()?;
        let settings = self.settings();
        let (name, items) = collect(&ws, &p.folder, p.requests.as_deref()).map_err(ApiError::invalid)?;
        if items.is_empty() {
            return Err(ApiError::invalid(format!("'{name}' has no requests to run")));
        }
        let data = match p.data_file.filter(|f| !f.trim().is_empty()) {
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
        let iterations = iteration_count(p.iterations, data.len());
        if !(1..=MAX_ITERATIONS).contains(&iterations) {
            return Err(ApiError::invalid(format!("Iterations must be between 1 and {MAX_ITERATIONS}")));
        }
        if p.delay_ms > MAX_DELAY_MS {
            return Err(ApiError::invalid("The delay can be 10 minutes at most"));
        }
        let secrets = self
            .active_env_vars(&ws)
            .into_iter()
            .chain(self.workspace_vars(&ws))
            .filter(|v| v.secret && v.enabled)
            .map(|v| (v.key, v.value))
            .collect();
        let plan = RunPlan {
            name,
            items,
            iterations,
            delay: Duration::from_millis(p.delay_ms),
            data,
            stop_on_failure: p.stop_on_failure,
            allow_http_errors: p.allow_http_errors,
            secrets,
        };
        let vars = self.script_vars(&ws);
        let cancel = CancellationToken::new();
        let run_id = uuid::Uuid::new_v4().to_string();
        {
            let mut active = lock(&self.inner.runner.active);
            if active.is_some() {
                return Err(ApiError::invalid("A collection run is in progress. Stop it first."));
            }
            *active = Some((run_id.clone(), cancel.clone()));
        }
        let started = RunStarted {
            run_id: run_id.clone(),
            name: plan.name.clone(),
            total: plan.total(),
            iterations,
            environment: vars.environment.as_ref().map(|(_, name)| name.clone()),
        };
        let agent = crate::agents::in_agent_call();
        self.inner.sink.emit(StreamEvent::Runner {
            run_id: run_id.clone(),
            event: RunEvent::Started { name: plan.name.clone(), total: started.total, iterations },
            agent,
        });
        let api = self.clone();
        // The run outlives the agent's call: it takes the call's host limits along.
        let agent = agent.then(|| AgentRun { guard: crate::agents::scope_guard() });
        tokio::spawn(async move { api.run_in_background(run_id, ws, settings, plan, vars, cancel, agent).await });
        Ok(started)
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_in_background(
        &self,
        run_id: String,
        ws: Workspace,
        settings: zorvik_workspace::settings::Settings,
        plan: RunPlan,
        mut vars: ScriptVars,
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
        };
        let sink = self.inner.sink.clone();
        let report = Box::pin(run(&cx, &plan, &mut vars, &cancel, |result, vars| {
            // What scripts set is kept as it happens (like single sends), not only at the end.
            let warning = self.keep_local_values(&ws, vars);
            vars.changes.clear();
            let mut result = result.clone();
            if let Some(message) = warning {
                result.console.push(zorvik_script::ConsoleEntry { level: zorvik_script::ConsoleLevel::Warn, message });
            }
            let event = RunEvent::Result { result: Box::new(result) };
            sink.emit(StreamEvent::Runner { run_id: run_id.clone(), event, agent: by_agent });
        }))
        .await;
        if let Some(jar) = &jar {
            self.save_jar(&ws, jar);
        }
        let summary = report.summary.clone();
        {
            let mut finished = lock(&self.inner.runner.finished);
            finished.push_back((run_id.clone(), Arc::new(report)));
            while finished.len() > KEEP_FINISHED {
                finished.pop_front();
            }
        }
        self.free_run_slot(&run_id);
        sink.emit(StreamEvent::Runner {
            run_id: run_id.clone(),
            event: RunEvent::Finished { summary },
            agent: by_agent,
        });
    }

    fn free_run_slot(&self, run_id: &str) {
        let mut active = lock(&self.inner.runner.active);
        if active.as_ref().is_some_and(|(id, _)| id == run_id) {
            *active = None;
        }
    }
}

/// Frees the run slot if a run panics, so later runs are not refused as "in progress",
/// and tells the UI it ended.
struct RunSlot<'a> {
    api: &'a Api,
    run_id: &'a str,
    agent: bool,
}

/// A run an AI agent started, and the hosts its requests may reach.
struct AgentRun {
    guard: Option<zorvik_engine::HostGuard>,
}

impl Drop for RunSlot<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            self.api.free_run_slot(self.run_id);
            let event = RunEvent::Finished { summary: RunSummary::default() };
            self.api.inner.sink.emit(StreamEvent::Runner { run_id: self.run_id.to_string(), event, agent: self.agent });
        }
    }
}

/// A passed result of an HTTP GET, for tests.
#[cfg(test)]
pub(crate) fn test_result(path: &str, name: &str, iteration: u32) -> RunResult {
    RunResult {
        iteration,
        path: path.into(),
        name: name.into(),
        method: "GET".into(),
        passed: true,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(name: &str, path: &str) -> RunItem {
        RunItem { path: path.into(), name: name.into(), request: Ok(Request::new(name, RequestKind::Http)) }
    }

    fn test(name: &str, passed: bool, skipped: bool) -> TestResult {
        TestResult { name: name.into(), passed, skipped, error: None }
    }

    #[test]
    fn next_request_by_name_or_path() {
        let items = [item("Login", "auth/login.yaml"), item("Get", "get.yaml"), item("Get", "other/get.yaml")];
        let next = |name: Option<&str>| NextRequest { name: name.map(str::to_string) };
        assert_eq!(next_step(&items, 0, None), Ok(Some(1)));
        assert_eq!(next_step(&items, 2, None), Ok(Some(3)), "past the end: the iteration is over");
        assert_eq!(next_step(&items, 2, Some(&next(Some("Login")))), Ok(Some(0)));
        assert_eq!(next_step(&items, 0, Some(&next(Some("Get")))), Ok(Some(1)), "the first with that name");
        assert_eq!(next_step(&items, 0, Some(&next(Some("other/get.yaml")))), Ok(Some(2)));
        assert_eq!(next_step(&items, 0, Some(&next(None))), Ok(None));
        assert_eq!(next_step(&items, 0, Some(&next(Some("Nope")))), Err("Nope".to_string()));
    }

    #[test]
    fn verdicts() {
        let mut r = test_result("a.yaml", "A", 0);
        r.status = Some(500);
        assert!(!verdict(&r, false), "an HTTP error without tests fails");
        assert!(verdict(&r, true), "unless HTTP errors are allowed");
        r.tests = vec![test("server error expected", true, false)];
        assert!(verdict(&r, false), "tests decide when there are some");
        r.tests.push(test("skipped", false, true));
        assert!(verdict(&r, false));
        r.tests.push(test("broken", false, false));
        assert!(!verdict(&r, true));
        let mut r = test_result("a.yaml", "A", 0);
        r.status = Some(200);
        r.tests = vec![test("only skipped", false, true)];
        assert!(verdict(&r, false));
        r.script_errors = vec!["Post-response script of request 'A' failed: x".into()];
        assert!(!verdict(&r, false));
        let mut r = test_result("a.yaml", "A", 0);
        r.error = Some("Connection refused".into());
        assert!(!verdict(&r, true));
        r.skipped = true;
        assert!(verdict(&r, false));
    }

    #[test]
    fn iterations_follow_data_rows() {
        assert_eq!(iteration_count(None, 0), 1);
        assert_eq!(iteration_count(None, 7), 7);
        assert_eq!(iteration_count(Some(2), 7), 2);
    }

    #[test]
    fn secrets_are_masked_in_urls() {
        let secrets = vec![("key".to_string(), "s3cr3t/+".to_string()), ("short".to_string(), "ab".to_string())];
        assert_eq!(
            mask_secrets("https://x.test/a?k=s3cr3t%2F%2B&raw=s3cr3t/+&ab=ab", &secrets),
            "https://x.test/a?k={{key}}&raw={{key}}&ab=ab"
        );
        // Spaces encoded either way; a secret inside a longer one doesn't leave the rest.
        let secrets = vec![("inner".to_string(), "abc".to_string()), ("outer".to_string(), "abc def".to_string())];
        assert_eq!(
            mask_secrets("https://x.test/abc%20def?q=abc+def&r=abc", &secrets),
            "https://x.test/{{outer}}?q={{outer}}&r={{inner}}"
        );
    }

    #[test]
    fn secret_values_include_what_scripts_set() {
        let plan = RunPlan { secrets: vec![("token".into(), String::new())], ..Default::default() };
        let mut vars = ScriptVars::default();
        vars.values.environment.insert("token".into(), "from-login".into());
        vars.values.environment.insert("public".into(), "visible".into());
        let secrets = secret_values(&plan, &vars);
        assert_eq!(
            mask_secrets("https://x.test/?t=from-login&p=visible", &secrets),
            "https://x.test/?t={{token}}&p=visible"
        );
    }

    #[test]
    fn long_runs_keep_the_first_results_then_failures() {
        let (mut results, mut omitted) = (Vec::new(), 0);
        for i in 0..6 {
            let mut r = test_result("a.yaml", "A", i);
            r.passed = i != 3 && i != 5;
            keep(&mut results, &mut omitted, r, 2);
        }
        // 0 and 1 (the limit), then failed ones only (3, 5) up to twice the limit.
        assert_eq!(results.iter().map(|r| r.iteration).collect::<Vec<_>>(), [0, 1, 3, 5]);
        assert_eq!(omitted, 2);
        let mut failed = test_result("a.yaml", "A", 6);
        failed.passed = false;
        keep(&mut results, &mut omitted, failed, 2);
        assert_eq!((results.len(), omitted), (4, 3));
    }

    #[test]
    fn console_is_bounded() {
        let entry = |message: String| ConsoleEntry { level: ConsoleLevel::Log, message };
        let console = bounded_console((0..250).map(|i| entry(i.to_string())).collect());
        assert_eq!(console.len(), MAX_CONSOLE + 1);
        assert_eq!(console.last().unwrap().message, "50 more console lines not kept.");
        let long = bounded_console(vec![entry("é".repeat(MAX_CONSOLE_LINE + 10))]);
        assert_eq!(long[0].message.chars().count(), MAX_CONSOLE_LINE + "… (truncated)".chars().count());
    }

    #[test]
    fn summary_counts() {
        let mut a = test_result("a.yaml", "A", 0);
        a.tests = vec![test("t1", true, false), test("t2", false, false), test("t3", false, true)];
        a.passed = false;
        let mut b = test_result("b.yaml", "B", 0);
        b.skipped = true;
        let c = test_result("a.yaml", "A", 1);
        let mut s = RunSummary {
            per_iteration: (0..2).map(|iteration| IterationSummary { iteration, ..Default::default() }).collect(),
            ..Default::default()
        };
        for r in [a, b, c] {
            count(&mut s, &r);
        }
        assert_eq!((s.requests, s.failed, s.skipped), (2, 1, 1));
        assert_eq!((s.tests_passed, s.tests_failed, s.tests_skipped), (1, 1, 1));
        assert_eq!(s.per_iteration.len(), 2);
        let first = &s.per_iteration[0];
        assert_eq!(
            (first.requests, first.failed, first.skipped, first.tests_passed, first.tests_failed),
            (1, 1, 1, 1, 1)
        );
        assert_eq!(s.per_iteration[1].requests, 1);
    }
}

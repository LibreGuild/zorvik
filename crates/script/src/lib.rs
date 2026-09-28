//! Sandboxed JavaScript for pre-request and post-response scripts.
//!
//! Every run gets a fresh QuickJS runtime and context: no file, network, process
//! or timer access (`require` gives only the built-in libraries, `libs.rs`), a
//! memory limit and a time limit. The Postman-compatible `pm` API, the
//! chai-style `pm.expect` and `console` are a JS prelude (`prelude.js`); the
//! input goes in and the results come out as JSON, so a script can do nothing
//! but compute. Design: docs/architecture.md ("Scripts").

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use std::sync::Arc;

use rquickjs::context::EvalOptions;
use rquickjs::promise::PromiseState;
use rquickjs::{
    CatchResultExt, CaughtError, Context, Ctx, Function, Module, Object, Promise, Runtime, Type, Value, WriteOptions,
    qjs,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub mod host;
mod libs;

pub use host::{Host, HostRequest, HostResponse, ScriptCookie};

const PRELUDE: &str = include_str!("prelude.js");
/// Module name of the prelude (in stack traces).
const PRELUDE_FILE: &str = "prelude.js";
/// File name of the user's script in stack traces, used to find the failing line.
const SCRIPT_FILE: &str = "script";
/// Postman runs scripts inside a function (top-level `return` works). The
/// wrapper stays on line 1 so line numbers match the editor.
const WRAP_START: &str = "(function () {";
/// Hidden property marking a rejected promise nobody handled yet.
const UNHANDLED_TAG: &str = "__zvUnhandled";
/// For scripts that `await` (`await pm.sendRequest(…)`): an async function.
const WRAP_ASYNC: &str = "(async function () {";
/// QuickJS stack limit; well below the 2 MB of the threads scripts run on.
const MAX_STACK: usize = 768 * 1024;
/// Response bodies longer than this reach scripts cut (the rest of the memory is for the script).
const MAX_BODY_DIVISOR: usize = 4;
/// Time limit for collecting a script's results after it ran.
const FINISH_TIMEOUT: Duration = Duration::from_secs(1);

/// Resource limits of one script run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub timeout: Duration,
    /// JavaScript heap limit in bytes.
    pub memory: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self { timeout: Duration::from_secs(5), memory: 64 * 1024 * 1024 }
    }
}

/// Which script runs (`pm.info.eventName`, Postman's names).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Event {
    #[serde(rename = "prerequest")]
    PreRequest,
    #[serde(rename = "test")]
    PostResponse,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ScriptHeader {
    pub key: String,
    pub value: String,
}

impl ScriptHeader {
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self { key: key.into(), value: value.into() }
    }
}

/// The request as a script sees it (`pm.request`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ScriptRequest {
    pub url: String,
    pub method: String,
    pub headers: Vec<ScriptHeader>,
    /// Raw body text (`pm.request.body.raw`).
    pub body: String,
}

/// The response (`pm.response`), for post-response scripts.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptResponse {
    pub code: u16,
    /// Reason phrase (`pm.response.status`).
    pub status: String,
    pub headers: Vec<ScriptHeader>,
    /// Handed to the engine as its own string (not inside the input JSON).
    #[serde(skip)]
    pub body: String,
    /// Milliseconds.
    pub response_time: f64,
    /// Body size in bytes.
    pub response_size: u64,
    /// Server-Sent Events read from the response (`pm.response.events`), for event streams.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub events: Option<Vec<ScriptEvent>>,
    /// Cookies the response set (`pm.response.cookies`).
    pub cookies: Vec<ScriptCookie>,
}

/// One Server-Sent Event as scripts see it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScriptEvent {
    pub event: String,
    pub data: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

/// Variable scopes, highest precedence first: `pm.variables.get` looks through
/// all of them, `pm.environment.get` & co. through their own.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct Variables {
    /// Values that win over everything and scripts can't change (`zorvik run --var`).
    pub overrides: BTreeMap<String, String>,
    /// `pm.variables`: this send, or the whole run.
    pub local: BTreeMap<String, String>,
    /// The runner's current data row (`pm.iterationData`).
    pub data: BTreeMap<String, serde_json::Value>,
    pub environment: BTreeMap<String, String>,
    /// Workspace variables (`pm.collectionVariables`).
    pub collection: BTreeMap<String, String>,
    pub globals: BTreeMap<String, String>,
}

impl Variables {
    /// Apply a script's change, so later scripts and the request see it.
    pub fn apply(&mut self, change: &VariableChange) {
        let map = match change.scope {
            Scope::Local => &mut self.local,
            Scope::Environment => &mut self.environment,
            Scope::Collection => &mut self.collection,
            Scope::Globals => &mut self.globals,
        };
        match &change.value {
            Some(value) => map.insert(change.key.clone(), value.clone()),
            None => map.remove(&change.key),
        };
    }

    /// Value of `name` as `{{name}}` would resolve it (data values as text).
    pub fn get(&self, name: &str) -> Option<String> {
        [&self.overrides, &self.local]
            .into_iter()
            .find_map(|m| m.get(name).cloned())
            .or_else(|| self.data.get(name).map(value_text))
            .or_else(|| {
                [&self.environment, &self.collection, &self.globals].into_iter().find_map(|m| m.get(name).cloned())
            })
    }
}

/// A data file value as a variable: strings as-is, anything else as JSON.
pub fn value_text(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// `pm.info`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    pub request_name: String,
    /// The request's path in the workspace.
    pub request_id: String,
    /// 0-based.
    pub iteration: u32,
    pub iteration_count: u32,
}

/// Everything a script can read.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptInput {
    pub event: Event,
    pub info: Info,
    /// Name of the active environment (`pm.environment.name`), if any.
    pub environment_name: Option<String>,
    pub request: ScriptRequest,
    pub response: Option<ScriptResponse>,
    pub variables: Variables,
    /// The cookie jar's cookies for the request's URL (`pm.cookies`).
    pub cookies: Vec<ScriptCookie>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Scope {
    /// `pm.variables`
    Local,
    Environment,
    /// `pm.collectionVariables` (workspace variables)
    Collection,
    Globals,
}

/// A `set` or `unset` by a script (the last one per scope and key).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VariableChange {
    pub scope: Scope,
    pub key: String,
    /// `None`: unset.
    pub value: Option<String>,
}

/// One `pm.test`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TestResult {
    pub name: String,
    pub passed: bool,
    /// `pm.test.skip` (neither passed nor failed).
    pub skipped: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ConsoleLevel {
    Log,
    Info,
    Warn,
    Error,
    Debug,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ConsoleEntry {
    pub level: ConsoleLevel,
    pub message: String,
}

/// An error that stopped the script.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptError {
    /// `ReferenceError: x is not defined`
    pub message: String,
    /// 1-based line in the script, when known.
    pub line: Option<u32>,
}

/// `pm.execution.setNextRequest(name)` (or `postman.setNextRequest`): which request
/// a collection run goes on with. A single send ignores it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct NextRequest {
    /// Name (or path) of a request in the run; `None` (`null`) ends the iteration.
    pub name: Option<String>,
}

/// What a script did. Tests, console output and variable changes made before an
/// error are kept.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScriptOutput {
    /// The request after the script, when the script changed it.
    pub request: Option<ScriptRequest>,
    pub variables: Vec<VariableChange>,
    pub tests: Vec<TestResult>,
    pub console: Vec<ConsoleEntry>,
    /// The last `setNextRequest` call, if any.
    pub next_request: Option<NextRequest>,
    /// `pm.execution.skipRequest()` in a pre-request script: don't send.
    pub skip_request: bool,
    /// What `pm.visualizer.set` rendered (HTML).
    pub visualization: Option<String>,
    pub error: Option<ScriptError>,
}

/// The prelude's report.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Report {
    request: ScriptRequest,
    variables: Vec<VariableChange>,
    tests: Vec<TestResult>,
    console: Vec<ConsoleEntry>,
    #[serde(default)]
    next_request: Option<NextRequest>,
    #[serde(default)]
    skip_request: bool,
    #[serde(default)]
    visualization: Option<String>,
}

/// Run one script. Blocking and CPU-bound (up to `limits.timeout`): call it
/// from a blocking thread.
pub fn run(source: &str, input: &ScriptInput, limits: &Limits) -> ScriptOutput {
    run_with(source, input, limits, None)
}

/// [`run`], with the app's side (`pm.sendRequest`, the cookie jar, dynamic variables).
/// `host` calls block this thread.
pub fn run_with(source: &str, input: &ScriptInput, limits: &Limits, host: Option<Arc<dyn Host>>) -> ScriptOutput {
    let failed =
        |message: String| ScriptOutput { error: Some(ScriptError { message, line: None }), ..Default::default() };
    let input_json = match serde_json::to_string(input) {
        Ok(json) => json,
        Err(e) => return failed(format!("Could not prepare the script: {e}")),
    };
    let body = input.response.as_ref().map(|r| cut(&r.body, limits.memory / MAX_BODY_DIVISOR)).unwrap_or("");
    let mut out = match execute(source, &input_json, body, limits, host) {
        Ok(out) => out,
        Err(message) => return failed(message),
    };
    if input.response.as_ref().is_some_and(|r| body.len() < r.body.len()) {
        out.console.push(ConsoleEntry {
            level: ConsoleLevel::Warn,
            message: format!("The response body is larger than {} MB; scripts see only its start.", mb(body.len())),
        });
    }
    if out.request.as_ref() == Some(&input.request) {
        out.request = None;
    }
    out
}

/// Runs the next pending promise job. An exception in it (a rejected callback) comes back
/// as the error: `Ctx::execute_pending_job` would drop it.
fn run_job<'js>(ctx: &Ctx<'js>) -> Result<bool, CaughtError<'js>> {
    let mut job_ctx = std::ptr::null_mut();
    // SAFETY: the runtime pointer comes from this live context; QuickJS runs one job and
    // reports the context it ran in (ours: a script has one), leaving any exception there.
    let status = unsafe { qjs::JS_ExecutePendingJob(qjs::JS_GetRuntime(ctx.as_raw().as_ptr()), &mut job_ctx) };
    if status < 0 {
        return Err::<bool, _>(rquickjs::Error::Exception).catch(ctx);
    }
    Ok(status > 0)
}

/// `await` as a word outside comments and strings is enough of a hint: such scripts get an
/// async wrapper (a stray `await` in a string only makes the script async, which is harmless).
fn uses_await(source: &str) -> bool {
    source.match_indices("await").any(|(i, _)| {
        let before = source[..i].chars().next_back();
        let after = source[i + 5..].chars().next();
        let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '$');
        !word(before) && !word(after)
    })
}

fn execute(
    source: &str,
    input_json: &str,
    body: &str,
    limits: &Limits,
    app: Option<Arc<dyn Host>>,
) -> Result<ScriptOutput, String> {
    let internal = |e: rquickjs::Error| format!("Could not start the script engine: {e}");
    let runtime = Runtime::new().map_err(internal)?;
    runtime.set_memory_limit(limits.memory);
    runtime.set_max_stack_size(MAX_STACK);
    // The engine calls this regularly while running; `true` stops the script.
    let deadline: Rc<Cell<Option<Instant>>> = Rc::new(Cell::new(None));
    let check = deadline.clone();
    runtime.set_interrupt_handler(Some(Box::new(move || check.get().is_some_and(|d| Instant::now() >= d))));
    // Promises rejected with nobody handling them (a `.then` callback that threw, a failed
    // `pm.sendRequest` nobody awaited): the first one still unhandled at the end fails the script.
    let unhandled: Rc<RefCell<Vec<(u32, ScriptError)>>> = Rc::default();
    let seen = unhandled.clone();
    let next_id = Cell::new(0u32);
    runtime.set_host_promise_rejection_tracker(Some(Box::new(move |ctx, promise, reason, handled| {
        let Some(promise) = promise.as_object() else { return };
        if handled {
            if let Ok(id) = promise.get::<_, u32>(UNHANDLED_TAG) {
                seen.borrow_mut().retain(|(i, _)| *i != id);
            }
            return;
        }
        next_id.set(next_id.get() + 1);
        let _ = promise.set(UNHANDLED_TAG, next_id.get());
        let caught = match reason.clone().into_exception() {
            Some(exception) => CaughtError::Exception(exception),
            None => CaughtError::Value(reason),
        };
        seen.borrow_mut().push((next_id.get(), script_error(&ctx, caught)));
    })));
    let context = Context::full(&runtime).map_err(internal)?;

    let bytecode = prelude_bytecode()?;

    context.with(|ctx| {
        let failed = |e: CaughtError| format!("The script API failed to load: {e}");
        // SAFETY: the bytecode was written by this same engine from our own prelude.
        let prelude = unsafe { Module::load(ctx.clone(), bytecode) }.catch(&ctx).map_err(failed)?;
        let (prelude, done) = prelude.eval().catch(&ctx).map_err(failed)?;
        done.finish::<()>().catch(&ctx).map_err(failed)?;
        let setup: Function = prelude.get("default").catch(&ctx).map_err(failed)?;
        let host: Object = libs::host(&ctx).catch(&ctx).map_err(failed)?;
        host::install(&ctx, &host, app, deadline.clone()).catch(&ctx).map_err(failed)?;
        let finish: Function = setup.call((input_json, body, host)).catch(&ctx).map_err(failed)?;
        let tick: Function = finish.get("tick").catch(&ctx).map_err(failed)?;

        let started = Instant::now();
        deadline.set(Some(started + limits.timeout));
        let out_of_time = || deadline.get().is_some_and(|d| Instant::now() >= d);
        let mut opts = EvalOptions::default();
        opts.strict = false;
        opts.filename = Some(SCRIPT_FILE.into());
        let start = if uses_await(source) { WRAP_ASYNC } else { WRAP_START };
        let wrapped = format!("{start}{source}\n}}).call(undefined);");
        let result = ctx.eval_with_options::<Value, _>(wrapped, opts).catch(&ctx);
        let (mut error, completion) = match result {
            Ok(value) => (None, value.into_promise()),
            Err(e) => (Some(script_error(&ctx, e)), None),
        };
        // Promise callbacks (async `pm.test` functions, `pm.sendRequest` callbacks), then
        // the timers (`setTimeout`), each followed by the callbacks it queued.
        'events: while error.is_none() && !out_of_time() {
            loop {
                match run_job(&ctx) {
                    Ok(true) if !out_of_time() => {}
                    Ok(_) => break,
                    Err(e) => {
                        error = Some(script_error(&ctx, e));
                        break 'events;
                    }
                }
            }
            match tick.call::<_, bool>(()).catch(&ctx) {
                Ok(true) => {}
                Ok(false) => break,
                Err(e) => error = Some(script_error(&ctx, e)),
            }
        }
        // An async script that threw, else a rejection nobody handled.
        if error.is_none()
            && let Some(promise) = completion.filter(|p: &Promise| p.state() == PromiseState::Rejected)
        {
            let reason = promise.result::<Value>().and_then(|r| r.err());
            error = reason.map(|e| script_error(&ctx, CaughtError::from_error(&ctx, e)));
        }
        if error.is_none() {
            error = unhandled.borrow_mut().drain(..).next().map(|(_, e)| e);
        }
        if started.elapsed() >= limits.timeout {
            error = Some(ScriptError {
                message: format!("The script took longer than {} and was stopped", seconds(limits.timeout)),
                line: error.and_then(|e| e.line),
            });
        } else if let Some(e) = &mut error
            // A bare InternalError: out of memory before even its message was allocated.
            && (e.message.contains("out of memory") || e.message == "InternalError")
        {
            let limit = mb(limits.memory);
            e.message = if e.message.starts_with("Uncaught null") {
                format!("Uncaught null: the script threw null or ran out of memory (limit {limit} MB)")
            } else {
                format!("The script ran out of memory (limit {limit} MB)")
            };
        }
        // The report can run the script's code too (built-ins it replaced, a `tests`
        // getter or proxy), so it gets its own time limit.
        let finish_by = Instant::now() + FINISH_TIMEOUT;
        deadline.set(Some(finish_by));
        let report = finish
            .call::<_, String>(())
            .catch(&ctx)
            .map_err(|e| {
                if Instant::now() >= finish_by {
                    "Collecting the script's results took too long (did it replace built-in functions?)".into()
                } else {
                    e.to_string()
                }
            })
            .and_then(|json| {
                serde_json::from_str::<Report>(&json).map_err(|e| format!("Could not read the script's results: {e}"))
            });
        deadline.set(None);
        Ok(match report {
            Ok(r) => ScriptOutput {
                request: Some(r.request),
                variables: r.variables,
                tests: r.tests,
                console: r.console,
                next_request: r.next_request,
                skip_request: r.skip_request,
                visualization: r.visualization,
                error,
            },
            // Out of memory, or the script broke the globals the report needs.
            Err(message) => {
                ScriptOutput { error: error.or(Some(ScriptError { message, line: None })), ..Default::default() }
            }
        })
    })
}

/// The prelude as bytecode, compiled once per process: loading it is several
/// times faster than parsing the source for every script.
fn prelude_bytecode() -> Result<&'static [u8], String> {
    static BYTECODE: OnceLock<Result<Vec<u8>, String>> = OnceLock::new();
    BYTECODE
        .get_or_init(|| {
            let runtime = Runtime::new().map_err(|e| e.to_string())?;
            let context = Context::full(&runtime).map_err(|e| e.to_string())?;
            context.with(|ctx| {
                // The prelude is one expression (an IIFE returning `setup`).
                Module::declare(ctx.clone(), PRELUDE_FILE, format!("export default {PRELUDE}"))
                    .and_then(|module| module.write(WriteOptions::default()))
                    .catch(&ctx)
                    .map_err(|e| e.to_string())
            })
        })
        .as_deref()
        .map_err(|e| format!("The script API failed to load: {e}"))
}

/// Message and line of an exception thrown by the script.
fn script_error<'js>(ctx: &Ctx<'js>, error: CaughtError<'js>) -> ScriptError {
    match error {
        CaughtError::Exception(ex) => {
            let name: Option<String> = ex.get("name").ok();
            let message = ex.message().unwrap_or_default();
            let stack = ex.stack().unwrap_or_default();
            let text = match name.filter(|n| !n.is_empty()) {
                Some(name) if !message.is_empty() => format!("{name}: {message}"),
                Some(name) => name,
                None => message,
            };
            ScriptError { message: text, line: script_line(&stack) }
        }
        // QuickJS throws `null` when it can't even allocate the error.
        CaughtError::Value(value) if matches!(value.type_of(), Type::Null | Type::Uninitialized) => {
            ScriptError { message: "Uncaught null (out of memory?)".into(), line: None }
        }
        CaughtError::Value(value) => {
            let text = ctx
                .json_stringify(value.clone())
                .ok()
                .flatten()
                .and_then(|s| s.to_string().ok())
                .unwrap_or_else(|| format!("{value:?}"));
            ScriptError { message: format!("Uncaught {}", text.trim_matches('"')), line: None }
        }
        CaughtError::Error(e) => ScriptError { message: e.to_string(), line: None },
    }
}

/// Line of the innermost stack frame in the user's script (`… (script:3:5)`).
fn script_line(stack: &str) -> Option<u32> {
    stack.lines().find_map(|frame| {
        let at = frame.find(&format!("({SCRIPT_FILE}:")).map(|i| i + 1).or_else(|| {
            let trimmed = frame.trim_start();
            trimmed.starts_with(&format!("at {SCRIPT_FILE}:")).then(|| frame.len() - trimmed.len() + 3)
        })?;
        let rest = &frame[at + SCRIPT_FILE.len() + 1..];
        rest.split([':', ')']).next()?.parse().ok()
    })
}

/// The longest prefix of `s` of at most `max` bytes, on a character boundary.
fn cut(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

fn mb(bytes: usize) -> usize {
    bytes / (1024 * 1024)
}

fn seconds(d: Duration) -> String {
    let ms = d.as_millis();
    if ms.is_multiple_of(1000) { format!("{} s", ms / 1000) } else { format!("{:.1} s", ms as f64 / 1000.0) }
}

#[cfg(test)]
mod tests;

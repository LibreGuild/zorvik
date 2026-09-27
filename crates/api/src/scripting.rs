//! Pre-request and post-response scripts around HTTP sends (docs/architecture.md, "Scripts").
//!
//! Scripts run workspace → folders (outer to inner) → request, each in a fresh
//! sandbox (`zorvik_script`). Pre-request scripts see the request before its
//! `{{variables}}` are resolved and may change it; post-response scripts see the
//! response and add tests. [`send_scripted`] is the whole pipeline: `http.send`
//! uses it for one request, the collection runner for every request of a run.
//! What scripts set on the environment, the workspace or globals is kept as local
//! current values in the app data dir (`vars.local`), never in workspace files.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use zorvik_engine::{Client, CookieJar, HttpRequest, HttpResponse};
use zorvik_script::{
    ConsoleEntry, ConsoleLevel, Event, Info, Limits, NextRequest, Scope, ScriptHeader, ScriptInput, ScriptRequest,
    ScriptResponse, TestResult, VariableChange, Variables,
};
use zorvik_workspace::Workspace;
use zorvik_workspace::formats::{BodyType, FolderMeta, KeyValue, Request, Scripts, Variable, WorkspaceMeta};
use zorvik_workspace::localvalues::{GLOBALS, overlay};
use zorvik_workspace::oauth2::{self, TokenCache};
use zorvik_workspace::resolve::{Inheritance, Resolved, apply_token, check_url_variables, resolve};
use zorvik_workspace::secrets::{env_scope, workspace_scope};
use zorvik_workspace::settings::Settings;
use zorvik_workspace::vars::VarContext;

use crate::{Api, ApiError, ApiResult, join_err};

/// Console lines kept per send (each script keeps up to 1000 itself).
const MAX_CONSOLE: usize = 2000;
/// Sent bodies reach post-response scripts (`pm.request.body`) cut to this.
const MAX_SENT_BODY: usize = 1024 * 1024;

/// What the scripts of one send produced.
#[derive(Debug, Clone, Default, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ScriptReport {
    pub tests: Vec<TestResult>,
    pub console: Vec<ConsoleEntry>,
    /// Scripts that stopped with an error (a pre-request one also stops the send).
    pub errors: Vec<ScriptFailure>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ScriptFailure {
    /// Which script: "Pre-request script of folder 'Users'".
    pub script: String,
    pub message: String,
    pub line: Option<u32>,
}

impl ScriptFailure {
    /// "Pre-request script of request 'Get' failed at line 3: …"
    pub fn describe(&self) -> String {
        match self.line {
            Some(line) => format!("{} failed at line {line}: {}", self.script, self.message),
            None => format!("{} failed: {}", self.script, self.message),
        }
    }
}

/// A value a script set, kept on this computer (`vars.local`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LocalValue {
    /// `environment`, `workspace` or `globals`.
    pub scope: String,
    /// The environment's id, for scope `environment`.
    pub environment_id: Option<String>,
    pub key: String,
    pub value: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ClearLocalParams {
    scope: String,
    environment_id: Option<String>,
    /// One value; all of the scope when missing.
    key: Option<String>,
}

/// Variables during one send or one run.
#[derive(Debug, Clone, Default)]
pub struct ScriptVars {
    pub values: Variables,
    /// Active environment (id, name). Without one, `pm.environment.set` values are not kept.
    pub environment: Option<(String, String)>,
    /// Environment, workspace and global changes by scripts, in order, to keep as local values.
    pub changes: Vec<VariableChange>,
}

impl ScriptVars {
    /// `{{variables}}` for the request: overrides > `pm.variables` > data row >
    /// environment > workspace > globals.
    pub fn var_context(&self) -> VarContext {
        let v = &self.values;
        let data: BTreeMap<String, String> =
            v.data.iter().map(|(k, x)| (k.clone(), zorvik_script::value_text(x))).collect();
        let mut ctx = VarContext::new();
        for layer in [&v.overrides, &v.local, &data, &v.environment, &v.collection, &v.globals] {
            let vars: Vec<Variable> = layer
                .iter()
                .map(|(key, value)| Variable { key: key.clone(), value: value.clone(), enabled: true, secret: false })
                .collect();
            ctx.push_layer(&vars);
        }
        ctx
    }
}

/// Enabled variables as a map (a later duplicate wins, as in `VarContext`).
pub fn values_of(vars: &[Variable]) -> BTreeMap<String, String> {
    vars.iter()
        .filter(|v| v.enabled && !v.key.trim().is_empty())
        .map(|v| (v.key.trim().to_string(), v.value.clone()))
        .collect()
}

/// Position in a run (`pm.info.iteration`, 0-based); a single send is 0 of 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Iteration {
    pub index: u32,
    pub count: u32,
}

impl Default for Iteration {
    fn default() -> Self {
        Self { index: 0, count: 1 }
    }
}

/// What a scripted send needs besides the request. The app fills it from its
/// state; a command-line run can use an in-memory token cache and no cookie jar.
pub struct SendContext<'a> {
    pub ws: &'a Workspace,
    /// Workspace meta with secret values revealed (auth and headers are inherited from it).
    pub meta: &'a WorkspaceMeta,
    pub client: &'a Client,
    pub settings: &'a Settings,
    pub tokens: &'a TokenCache,
    pub jar: Option<&'a CookieJar>,
    /// Hosts requests may go to (AI agents); `None` = any.
    pub guard: Option<zorvik_engine::HostGuard>,
}

/// A request that was sent.
pub struct Sent {
    /// The request after pre-request scripts (variables not resolved).
    pub request: Request,
    /// The resolved URL.
    pub url: String,
    pub response: HttpResponse,
    /// Variables that were referenced but not defined.
    pub unresolved: Vec<String>,
}

/// Outcome of [`send_scripted`]. The report is kept when the send fails.
pub struct ScriptedSend {
    /// `None` when no script ran.
    pub report: Option<ScriptReport>,
    pub result: ApiResult<Sent>,
    /// The last `setNextRequest` call of the scripts (the collection runner acts on it).
    pub next_request: Option<NextRequest>,
}

/// Pre-request scripts → resolve → send → post-response scripts. `vars` carries
/// `pm.variables` and the other scopes from one request of a run to the next; the
/// environment, workspace and global changes collect in `vars.changes`.
pub async fn send_scripted(
    cx: &SendContext<'_>,
    mut request: Request,
    path: Option<&str>,
    vars: &mut ScriptVars,
    iteration: Iteration,
) -> ScriptedSend {
    let folders = path.map(|p| cx.ws.ancestors(p)).unwrap_or_default();
    let limits = Limits { timeout: cx.settings.script_timeout(), ..Default::default() };
    let info = Info {
        request_name: request.name.clone(),
        request_id: path.unwrap_or_default().to_string(),
        iteration: iteration.index,
        iteration_count: iteration.count,
    };
    let mut report: Option<ScriptReport> = None;
    let mut next_request = None;

    let pre = chain(cx.meta, &folders, &request, |s| &s.pre_request);
    if !pre.is_empty() {
        let before = request_snapshot(&request);
        let input = script_input(Event::PreRequest, &info, vars, before.clone(), None);
        let outcome = match run_chain(Event::PreRequest, pre, input, vars.environment.is_some(), limits).await {
            Ok(outcome) => outcome,
            Err(e) => return ScriptedSend { report, result: Err(e), next_request },
        };
        vars.values = outcome.values;
        vars.changes.extend(outcome.changes);
        apply_request(&mut request, &before, outcome.request);
        next_request = outcome.next_request;
        let failure = outcome.report.errors.first().map(ScriptFailure::describe);
        report = Some(outcome.report);
        if let Some(message) = failure {
            return ScriptedSend { report, result: Err(ApiError::new("script", message)), next_request };
        }
    }

    let (resolved, response) = match send(cx, &request, &folders, vars).await {
        Ok(sent) => sent,
        Err(e) => return ScriptedSend { report, result: Err(e), next_request },
    };

    let post = chain(cx.meta, &folders, &request, |s| &s.post_response);
    if !post.is_empty() {
        let res = response_snapshot(&response, limits.memory);
        let input = script_input(Event::PostResponse, &info, vars, sent_snapshot(&resolved.request), Some(res));
        let merged = report.get_or_insert_with(Default::default);
        match run_chain(Event::PostResponse, post, input, vars.environment.is_some(), limits).await {
            Ok(outcome) => {
                vars.values = outcome.values;
                vars.changes.extend(outcome.changes);
                next_request = outcome.next_request.or(next_request);
                merge(merged, outcome.report);
            }
            Err(e) => merged.errors.push(ScriptFailure {
                script: "Post-response scripts".into(),
                message: e.message,
                line: None,
            }),
        }
    }
    let url = resolved.request.url.clone();
    ScriptedSend { report, result: Ok(Sent { request, url, response, unresolved: resolved.unresolved }), next_request }
}

/// Resolve against the workspace and `vars`, authorize, send.
async fn send(
    cx: &SendContext<'_>,
    request: &Request,
    folders: &[FolderMeta],
    vars: &ScriptVars,
) -> ApiResult<(Resolved, HttpResponse)> {
    let outside_files = cx.settings.files_outside_workspace;
    let inherit = Inheritance { workspace: cx.meta, folders, base_dir: cx.ws.root(), outside_files };
    let mut resolved = resolve(request, &inherit, &vars.var_context())?;
    check_url_variables(&resolved)?;
    let mut opts = cx.settings.request_options(&request.settings)?;
    opts.host_guard = cx.guard.clone();
    if let Some(config) = resolved.oauth2.clone() {
        let token = oauth2::ensure_token(cx.client, &opts, &config, cx.tokens, &cx.ws.local_key()).await?;
        apply_token(&mut resolved, &token.access_token);
    }
    let response = cx.client.send(resolved.request.clone(), &opts, cx.jar).await?;
    Ok((resolved, response))
}

/// The non-empty scripts for `request`, outermost first, with names for messages.
fn chain(
    meta: &WorkspaceMeta,
    folders: &[FolderMeta],
    request: &Request,
    pick: fn(&Scripts) -> &String,
) -> Vec<(String, String)> {
    let named =
        |name: &str, what: &str| if name.trim().is_empty() { what.to_string() } else { format!("{what} '{name}'") };
    std::iter::once(("workspace".to_string(), pick(&meta.scripts)))
        .chain(folders.iter().map(|f| (named(&f.name, "folder"), pick(&f.scripts))))
        .chain(std::iter::once((named(&request.name, "request"), pick(&request.scripts))))
        .filter(|(_, code)| !code.trim().is_empty())
        .map(|(label, code)| (label, code.clone()))
        .collect()
}

fn script_input(
    event: Event,
    info: &Info,
    vars: &ScriptVars,
    request: ScriptRequest,
    response: Option<ScriptResponse>,
) -> ScriptInput {
    ScriptInput {
        event,
        info: info.clone(),
        environment_name: vars.environment.as_ref().map(|(_, name)| name.clone()),
        request,
        response,
        variables: vars.values.clone(),
    }
}

struct ChainOutcome {
    request: ScriptRequest,
    values: Variables,
    /// Changes to keep (not `pm.variables`, and not the environment when none is active).
    changes: Vec<VariableChange>,
    report: ScriptReport,
    /// The last `setNextRequest` of the chain.
    next_request: Option<NextRequest>,
}

/// Run scripts one after the other on a blocking thread; each sees what the
/// previous ones did. A pre-request error stops the rest, a post-response one doesn't.
async fn run_chain(
    event: Event,
    scripts: Vec<(String, String)>,
    input: ScriptInput,
    has_environment: bool,
    limits: Limits,
) -> ApiResult<ChainOutcome> {
    tokio::task::spawn_blocking(move || {
        let mut input = input;
        let mut report = ScriptReport::default();
        let mut changes = Vec::new();
        let mut warned = false;
        let mut next_request = None;
        let kind = if event == Event::PreRequest { "Pre-request" } else { "Post-response" };
        for (label, code) in scripts {
            let out = zorvik_script::run(&code, &input, &limits);
            if let Some(request) = out.request {
                input.request = request;
            }
            for change in out.variables {
                input.variables.apply(&change);
                if change.scope == Scope::Environment && !has_environment {
                    if !warned {
                        warned = true;
                        report.console.push(ConsoleEntry {
                            level: ConsoleLevel::Warn,
                            message: "No environment is active: values set with pm.environment are not kept.".into(),
                        });
                    }
                } else if change.scope != Scope::Local {
                    changes.push(change);
                }
            }
            next_request = out.next_request.or(next_request);
            report.tests.extend(out.tests);
            let room = MAX_CONSOLE.saturating_sub(report.console.len());
            report.console.extend(out.console.into_iter().take(room));
            if let Some(error) = out.error {
                report.errors.push(ScriptFailure {
                    script: format!("{kind} script of {label}"),
                    message: error.message,
                    line: error.line,
                });
                if event == Event::PreRequest {
                    break;
                }
            }
        }
        ChainOutcome { request: input.request, values: input.variables, changes, report, next_request }
    })
    .await
    .map_err(join_err)
}

fn merge(into: &mut ScriptReport, from: ScriptReport) {
    into.tests.extend(from.tests);
    let room = MAX_CONSOLE.saturating_sub(into.console.len());
    into.console.extend(from.console.into_iter().take(room));
    into.errors.extend(from.errors);
}

/// Body types whose text is `pm.request.body.raw`.
fn raw_body(t: BodyType) -> bool {
    matches!(t, BodyType::Json | BodyType::Text | BodyType::Xml)
}

/// `pm.request` for pre-request scripts: the request as saved (enabled headers only).
fn request_snapshot(r: &Request) -> ScriptRequest {
    ScriptRequest {
        url: r.url.clone(),
        method: r.method.clone(),
        headers: r.headers.iter().filter(|h| h.enabled).map(|h| ScriptHeader::new(&h.key, &h.value)).collect(),
        body: if raw_body(r.body.body_type) { r.body.text.clone() } else { String::new() },
    }
}

/// Put a pre-request script's changes into the request.
fn apply_request(r: &mut Request, before: &ScriptRequest, after: ScriptRequest) {
    if after.url != before.url {
        r.url = after.url;
    }
    if after.method != before.method {
        r.method = after.method;
    }
    if after.headers != before.headers {
        let disabled: Vec<KeyValue> = r.headers.iter().filter(|h| !h.enabled).cloned().collect();
        r.headers = after.headers.into_iter().map(|h| KeyValue::new(h.key, h.value)).chain(disabled).collect();
    }
    if after.body != before.body {
        if !raw_body(r.body.body_type) {
            let json = after.body.trim_start().starts_with(['{', '[']);
            r.body.body_type = if json { BodyType::Json } else { BodyType::Text };
        }
        r.body.text = after.body;
    }
}

/// `pm.request` for post-response scripts: what was sent.
fn sent_snapshot(r: &HttpRequest) -> ScriptRequest {
    ScriptRequest {
        url: r.url.clone(),
        method: r.method.clone(),
        headers: r.headers.iter().map(|h| ScriptHeader::new(&h.name, &h.value)).collect(),
        body: String::from_utf8_lossy(&r.body[..r.body.len().min(MAX_SENT_BODY)]).into_owned(),
    }
}

fn response_snapshot(res: &HttpResponse, max_body: usize) -> ScriptResponse {
    ScriptResponse {
        code: res.meta.status,
        status: res.meta.status_text.clone(),
        headers: res.meta.headers.iter().map(|h| ScriptHeader::new(&h.name, &h.value)).collect(),
        body: String::from_utf8_lossy(&res.body[..res.body.len().min(max_body)]).into_owned(),
        response_time: res.timing.total_ms,
        response_size: res.body.len() as u64,
    }
}

impl Api {
    /// Variables for a send: file values with local values applied, plus globals.
    pub(crate) fn script_vars(&self, ws: &Workspace) -> ScriptVars {
        let environment = self.active_env_id(ws).and_then(|id| ws.read_environment(&id).ok().map(|e| (id, e.name)));
        ScriptVars {
            values: Variables {
                environment: values_of(&self.active_env_vars(ws)),
                collection: values_of(&self.workspace_vars(ws)),
                globals: self.inner.local_values.get(GLOBALS),
                ..Default::default()
            },
            environment,
            changes: Vec::new(),
        }
    }

    /// Keep what scripts set on the environment, the workspace and globals. Returns
    /// why some values could not be kept (for the script console), if so.
    pub(crate) fn keep_local_values(&self, ws: &Workspace, vars: &ScriptVars) -> Option<String> {
        let key = ws.local_key();
        let mut by_scope: BTreeMap<String, Vec<(String, Option<String>)>> = BTreeMap::new();
        for c in &vars.changes {
            let scope = match (c.scope, &vars.environment) {
                (Scope::Environment, Some((id, _))) => env_scope(&key, id),
                (Scope::Collection, _) => workspace_scope(&key),
                (Scope::Globals, _) => GLOBALS.to_string(),
                _ => continue,
            };
            by_scope.entry(scope).or_default().push((c.key.clone(), c.value.clone()));
        }
        let mut failure = None;
        for (scope, changes) in by_scope {
            if let Err(e) = self.inner.local_values.update(&scope, &changes) {
                tracing::warn!("could not keep script values: {}", e.message);
                failure.get_or_insert(format!("Values set by scripts were not kept: {}", e.message));
            }
        }
        failure
    }

    /// Workspace variables with secrets revealed and local values applied.
    pub(crate) fn workspace_vars(&self, ws: &Workspace) -> Vec<Variable> {
        let local = self.inner.local_values.get(&workspace_scope(&ws.local_key()));
        overlay(&self.revealed_meta(ws).variables, &local)
    }

    /// Global variables (only ever set by scripts).
    pub(crate) fn global_vars(&self) -> Vec<Variable> {
        overlay(&[], &self.inner.local_values.get(GLOBALS))
    }

    /// `vars.local`: values scripts set for the open workspace (its environments and
    /// the workspace itself) and globals.
    pub(crate) fn local_values(&self) -> Vec<LocalValue> {
        let mut out = Vec::new();
        let mut push = |scope: &str, environment_id: Option<&str>, values: BTreeMap<String, String>| {
            out.extend(values.into_iter().map(|(key, value)| LocalValue {
                scope: scope.into(),
                environment_id: environment_id.map(str::to_string),
                key,
                value,
            }));
        };
        if let Some(ws) = self.try_ws() {
            let prefix = format!("{}/", ws.local_key());
            for (scope, values) in self.inner.local_values.list(&prefix) {
                match &scope[prefix.len()..] {
                    "workspace" => push("workspace", None, values),
                    rest => {
                        if let Some(id) = rest.strip_prefix("env/") {
                            push("environment", Some(id), values);
                        }
                    }
                }
            }
        }
        push("globals", None, self.inner.local_values.get(GLOBALS));
        out
    }

    /// `vars.clearLocal`: forget one value or a whole scope.
    pub(crate) fn clear_local_values(&self, p: ClearLocalParams) -> ApiResult<()> {
        let scope = match (p.scope.as_str(), &p.environment_id) {
            ("globals", _) => GLOBALS.to_string(),
            ("workspace", _) => workspace_scope(&self.ws()?.local_key()),
            ("environment", Some(id)) => env_scope(&self.ws()?.local_key(), id),
            ("environment", None) => return Err(ApiError::invalid("environmentId is required")),
            (other, _) => return Err(ApiError::invalid(format!("Unknown scope '{other}'"))),
        };
        Ok(self.inner.local_values.clear(&scope, p.key.as_deref())?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kv(k: &str, v: &str) -> KeyValue {
        KeyValue::new(k, v)
    }

    #[test]
    fn chain_order_and_labels() {
        let mut meta: WorkspaceMeta =
            serde_json::from_value(serde_json::json!({"version": 1, "id": "w", "name": "W"})).unwrap();
        meta.scripts.pre_request = "a()".into();
        let folders = [
            FolderMeta {
                name: "Users".into(),
                scripts: Scripts { pre_request: "b()".into(), ..Default::default() },
                ..Default::default()
            },
            FolderMeta { name: "Empty".into(), ..Default::default() },
        ];
        let mut request = Request::new("Get", Default::default());
        request.scripts.pre_request = "c()".into();
        request.scripts.post_response = "  ".into();
        let pre = chain(&meta, &folders, &request, |s| &s.pre_request);
        assert_eq!(
            pre,
            [
                ("workspace".to_string(), "a()".to_string()),
                ("folder 'Users'".to_string(), "b()".to_string()),
                ("request 'Get'".to_string(), "c()".to_string()),
            ]
        );
        assert!(chain(&meta, &folders, &request, |s| &s.post_response).is_empty());
    }

    #[test]
    fn script_changes_go_into_the_request() {
        let mut r = Request::new("R", Default::default());
        r.url = "{{base}}/a".into();
        r.headers = vec![kv("Accept", "*/*"), KeyValue { enabled: false, ..kv("X-Off", "1") }];
        let before = request_snapshot(&r);
        assert_eq!(before.headers, [ScriptHeader::new("Accept", "*/*")]);
        assert_eq!(before.body, "");

        let unchanged = before.clone();
        let mut same = r.clone();
        apply_request(&mut same, &before, unchanged);
        assert_eq!(same, r);

        let after = ScriptRequest {
            url: "{{base}}/b".into(),
            method: "POST".into(),
            headers: vec![ScriptHeader::new("Accept", "*/*"), ScriptHeader::new("X-Sig", "1")],
            body: "{\"a\":1}".into(),
        };
        apply_request(&mut r, &before, after);
        assert_eq!(r.url, "{{base}}/b");
        assert_eq!(r.method, "POST");
        assert_eq!(r.headers, [kv("Accept", "*/*"), kv("X-Sig", "1"), KeyValue { enabled: false, ..kv("X-Off", "1") }]);
        assert_eq!((r.body.body_type, r.body.text.as_str()), (BodyType::Json, "{\"a\":1}"));
    }

    #[test]
    fn var_context_precedence() {
        let mut vars = ScriptVars::default();
        let v = &mut vars.values;
        v.globals.insert("a".into(), "global".into());
        v.collection.insert("a".into(), "workspace".into());
        v.environment.insert("a".into(), "env".into());
        v.data.insert("a".into(), serde_json::json!(3));
        v.local.insert("b".into(), "local".into());
        v.overrides.insert("b".into(), "override".into());
        let ctx = vars.var_context();
        assert_eq!(ctx.get("a"), Some("3"));
        assert_eq!(ctx.get("b"), Some("override"));
        assert_eq!(
            values_of(&[
                Variable { key: " k ".into(), value: "1".into(), enabled: true, secret: false },
                Variable { key: "off".into(), value: "1".into(), enabled: false, secret: false },
            ]),
            BTreeMap::from([("k".to_string(), "1".to_string())])
        );
    }
}

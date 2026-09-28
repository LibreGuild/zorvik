//! Load tests (`load.*`): saved files, one run at a time, run history and
//! reports. The generator itself is `zorvik_load`; this module turns a saved
//! load test into a plan (resolving each request once, with variables,
//! inheritance and OAuth2; the data file's rows) and keeps the results.

use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;
use zorvik_load::{
    DataRow, LoadEvent, LoadRun, Plan, PlanTarget, Render, RequestSource, Snapshot, Summary, TimePoint, UserVars,
};
use zorvik_workspace::Workspace;
use zorvik_workspace::formats::{LoadTest, RequestKind, RequestSettings, Variable};
use zorvik_workspace::resolve::{Inheritance, apply_token, check_url_variables, resolve};
use zorvik_workspace::vars::VarContext;

use crate::{Api, ApiError, ApiResult, StreamEvent, lock, ok, params};

/// Runs kept per load test (oldest deleted first).
const HISTORY_PER_TEST: usize = 30;

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LoadStarted {
    pub run_id: String,
    #[ts(type = "number")]
    pub planned_ms: u64,
    /// `[name, request path]` of the targets, in plan order.
    pub targets: Vec<(String, String)>,
}

/// The run in progress (the UI restores its view with it).
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ActiveLoadRun {
    pub run_id: String,
    pub test_id: String,
    pub workspace_path: String,
    pub name: String,
    /// Unix epoch milliseconds.
    pub started_at: f64,
    #[ts(type = "number")]
    pub planned_ms: u64,
    /// The latest snapshot (its `points` empty) and every chart point so far,
    /// so a reloaded window shows the whole run.
    pub snapshot: Option<Snapshot>,
    pub points: Vec<TimePoint>,
}

/// One finished run in the history list.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LoadRunRecord {
    pub run_id: String,
    pub started_at: f64,
    #[ts(type = "number")]
    pub duration_ms: u64,
    pub passed: bool,
    pub stopped_early: bool,
    #[ts(type = "number")]
    pub requests: u64,
    pub rps: f64,
    pub p95: f64,
    pub error_rate: f64,
    pub error: Option<String>,
}

pub(crate) struct ActiveEntry {
    info: ActiveLoadRun,
    run: LoadRun,
    /// Where the result is saved: follows a rename of the test, `None` once it is deleted.
    history: Option<PathBuf>,
}

#[derive(Default)]
pub(crate) struct LoadState {
    active: Mutex<Option<ActiveEntry>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct IdParam {
    id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RunParam {
    id: String,
    run_id: String,
}

impl Api {
    /// `load.*` methods.
    pub(crate) async fn call_load(&self, method: &str, p: Value) -> ApiResult<Value> {
        match method {
            "load.list" => ok(self.ws()?.list_load_tests()?),
            "load.read" => {
                let IdParam { id } = params(p)?;
                ok(self.ws()?.read_load_test(&id)?)
            }
            "load.create" => {
                #[derive(Deserialize)]
                struct P {
                    test: LoadTest,
                }
                let P { test } = params(p)?;
                ok(self.ws()?.create_load_test(&test)?)
            }
            "load.save" => {
                #[derive(Deserialize)]
                struct P {
                    id: String,
                    test: LoadTest,
                }
                let P { id, test } = params(p)?;
                let ws = self.ws()?;
                let new_id = ws.save_load_test(&id, &test)?;
                if new_id != id
                    && let (Ok(from), Ok(to)) = (self.history_dir(&ws, &id), self.history_dir(&ws, &new_id))
                {
                    // History follows the rename, and so does a run of this test that ends later
                    // (under the run lock, so its result can't land in the old folder).
                    let mut active = lock(&self.inner.load.active);
                    if let Some(entry) = active.as_mut().filter(|a| a.history.as_ref() == Some(&from)) {
                        entry.info.test_id = new_id.clone();
                        entry.history = Some(to.clone());
                    }
                    if from.exists() && !to.exists() {
                        let _ = std::fs::rename(from, to);
                    }
                }
                ok(new_id)
            }
            "load.duplicate" => {
                let IdParam { id } = params(p)?;
                ok(self.ws()?.duplicate_load_test(&id)?)
            }
            "load.delete" => {
                let IdParam { id } = params(p)?;
                let ws = self.ws()?;
                let history = self.history_dir(&ws, &id)?;
                tokio::task::spawn_blocking(move || ws.delete_load_test(&id)).await.map_err(crate::join_err)??;
                // A run of this test that ends later must not bring the history back.
                if let Some(entry) =
                    lock(&self.inner.load.active).as_mut().filter(|a| a.history.as_ref() == Some(&history))
                {
                    entry.history = None;
                }
                let _ = std::fs::remove_dir_all(history);
                ok(())
            }
            "load.reorder" => {
                #[derive(Deserialize)]
                struct P {
                    ids: Vec<String>,
                }
                let P { ids } = params(p)?;
                ok(self.ws()?.reorder_load_tests(&ids)?)
            }

            "load.start" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct P {
                    id: String,
                    /// The configuration to run (may have unsaved edits).
                    test: LoadTest,
                    /// The user confirmed sending load to hosts outside this computer/network.
                    #[serde(default)]
                    confirmed: bool,
                }
                let P { id, test, confirmed } = params(p)?;
                ok(self.load_start(id, test, confirmed, None).await?)
            }
            "load.stop" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct P {
                    run_id: String,
                }
                let P { run_id } = params(p)?;
                if let Some(active) = lock(&self.inner.load.active).as_ref().filter(|a| a.info.run_id == run_id) {
                    active.run.stop();
                }
                ok(())
            }
            "load.active" => ok(lock(&self.inner.load.active).as_ref().map(|a| a.info.clone())),
            "load.runs" => {
                let IdParam { id } = params(p)?;
                ok(self.load_history(&self.ws()?, &id)?)
            }
            "load.run" => {
                let RunParam { id, run_id } = params(p)?;
                ok(self.read_summary(&self.ws()?, &id, &run_id)?)
            }
            "load.deleteRun" => {
                let RunParam { id, run_id } = params(p)?;
                let file = self.run_file(&self.ws()?, &id, &run_id)?;
                let _ = std::fs::remove_file(file);
                ok(())
            }
            "load.export" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct P {
                    id: String,
                    run_id: String,
                    path: String,
                    /// `json` or `html`.
                    format: String,
                }
                let P { id, run_id, path, format } = params(p)?;
                let ws = self.ws()?;
                let summary = self.read_summary(&ws, &id, &run_id)?;
                let name = ws.read_load_test(&id).map(|t| t.name).unwrap_or(id);
                let data = match format.as_str() {
                    "json" => {
                        serde_json::to_vec_pretty(&summary).map_err(|e| ApiError::new("internal", e.to_string()))?
                    }
                    "html" => zorvik_load::html_report(&name, &summary).into_bytes(),
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

    /// The load test in progress.
    pub(crate) fn active_load_run(&self) -> Option<ActiveLoadRun> {
        lock(&self.inner.load.active).as_ref().map(|a| a.info.clone())
    }

    /// `approved` (AI agents): the hosts the user approved; the run is refused if the
    /// resolved plan goes anywhere else (the files may have changed while they decided).
    pub(crate) async fn load_start(
        &self,
        test_id: String,
        test: LoadTest,
        confirmed: bool,
        approved: Option<&[String]>,
    ) -> ApiResult<LoadStarted> {
        if lock(&self.inner.load.active).is_some() {
            return Err(ApiError::invalid("A load test is already running. Stop it first."));
        }
        let ws = self.ws()?;
        let history = self.history_dir(&ws, &test_id)?;
        let plan = self.build_plan(&ws, &test).await?;
        if let Some(approved) = approved
            && let Some(host) = plan_hosts(&plan).into_iter().find(|h| !approved.contains(h))
        {
            return Err(ApiError::invalid(format!(
                "The load test now sends to {host}, which the user did not approve; nothing was started"
            )));
        }
        if !confirmed {
            let outside = outside_hosts(&plan);
            if !outside.is_empty() {
                return Err(ApiError::new(
                    "confirmTarget",
                    format!(
                        "This sends load to {}. Only load test systems you own or are allowed to test.",
                        outside.join(", ")
                    ),
                ));
            }
        }
        let run_id = uuid::Uuid::new_v4().to_string();
        let info = ActiveLoadRun {
            run_id: run_id.clone(),
            test_id: test_id.clone(),
            workspace_path: ws.root().to_string_lossy().into_owned(),
            name: test.name.clone(),
            started_at: crate::now_ms() as f64,
            planned_ms: test.duration_secs() * 1000,
            snapshot: None,
            points: Vec::new(),
        };
        let targets = plan.targets.iter().map(|t| (t.name.clone(), t.request.clone())).collect();
        let planned_ms = test.duration_secs() * 1000;
        let inner = Arc::downgrade(&self.inner);
        let id = run_id.clone();
        let on_event: zorvik_load::EventFn = Arc::new(move |event: LoadEvent| {
            let Some(inner) = inner.upgrade() else { return };
            if let LoadEvent::Snapshot { snapshot } = &event
                && let Some(active) = lock(&inner.load.active).as_mut().filter(|a| a.info.run_id == id)
            {
                active.info.points.extend(snapshot.points.iter().cloned());
                active.info.snapshot = Some(Snapshot { points: Vec::new(), ..snapshot.clone() });
            }
            if let LoadEvent::Finished { summary } = &event {
                // Saved under the lock: a rename or delete of the test meanwhile moves or drops it.
                let mut active = lock(&inner.load.active);
                if let Some(entry) = active.take_if(|a| a.info.run_id == id)
                    && let Some(history) = &entry.history
                {
                    save_run(history, &id, summary);
                }
            }
            inner.sink.emit(StreamEvent::Load { run_id: id.clone(), event });
        });
        // Hold the slot while starting so two starts can't both run.
        let mut active = lock(&self.inner.load.active);
        if active.is_some() {
            return Err(ApiError::invalid("A load test is already running. Stop it first."));
        }
        let run = zorvik_load::start(plan, on_event).map_err(ApiError::invalid)?;
        *active = Some(ActiveEntry { info, run, history: Some(history) });
        Ok(LoadStarted { run_id, planned_ms, targets })
    }

    /// Resolve every enabled target once (variables, inherited headers/auth,
    /// OAuth2 token) and read the data file. Requests that use dynamic
    /// variables, data file columns or captured values are rendered again for
    /// every request; the others are sent as resolved here.
    async fn build_plan(&self, ws: &Workspace, test: &LoadTest) -> ApiResult<Plan> {
        let settings = self.settings();
        let overrides =
            RequestSettings { timeout_ms: test.timeout_ms, http_version: test.http_version, ..Default::default() };
        let options = crate::request_options(&settings, &overrides)?;
        let meta = Arc::new(self.revealed_meta(ws));
        let outside_files = settings.files_outside_workspace;
        let (rows, columns) = match test.data_file.as_deref().map(str::trim).filter(|f| !f.is_empty()) {
            // Up to 50 MB to read and parse: not on the async workers.
            Some(file) => {
                let (file, root) = (file.to_string(), ws.root().to_path_buf());
                let (data, _) =
                    tokio::task::spawn_blocking(move || crate::runner::read_data_file(&file, &root, outside_files))
                        .await
                        .map_err(crate::join_err)?
                        .map_err(ApiError::invalid)?;
                (data_rows(&data), data.columns)
            }
            None => (Vec::new(), Vec::new()),
        };
        // What a request may take from its user: data file columns and captured values.
        let captured = test.targets.iter().flat_map(|t| &t.captures).map(|c| c.variable.trim().to_string());
        let names: Vec<String> = columns.into_iter().chain(captured).filter(|n| !n.is_empty()).collect();
        // Below the user's variables: the environment, workspace variables, globals.
        let base = Arc::new(flatten(&[self.active_env_vars(ws), self.workspace_vars(ws), self.global_vars()]));
        let mut targets = Vec::new();
        for target in test.targets.iter().filter(|t| t.enabled && t.weight > 0) {
            let request = ws.read_request(&target.request).map_err(|e| {
                ApiError::invalid(format!("Request '{}' can't be loaded: {}", target.request, e.message))
            })?;
            if request.kind != RequestKind::Http {
                return Err(ApiError::invalid(format!(
                    "'{}' is not an HTTP request: only HTTP requests can be load tested",
                    request.name
                )));
            }
            let folders = ws.ancestors(&target.request);
            // Checked with every user variable defined: a URL may use them.
            let inherit = Inheritance { workspace: &meta, folders: &folders, base_dir: ws.root(), outside_files };
            let mut resolved = resolve(&request, &inherit, &user_context(&UserVars::probe(&names), &base))?;
            check_url_variables(&resolved)?;
            let token = match resolved.oauth2.clone() {
                Some(config) => {
                    let token = zorvik_workspace::oauth2::ensure_token(
                        &self.inner.client,
                        &options,
                        &config,
                        &self.inner.tokens,
                        &ws.local_key(),
                    )
                    .await?
                    .access_token;
                    apply_token(&mut resolved, &token);
                    Some(token)
                }
                None => None,
            };
            let dynamic = serde_json::to_string(&request).is_ok_and(|json| json.contains("{{$"));
            let name = request.name.clone();
            let (meta, base, base_dir) = (meta.clone(), base.clone(), ws.root().to_path_buf());
            // Without user variables (no data file, nothing captured) the context is built once.
            let plain = user_context(&UserVars::default(), &base);
            let render: Render = Arc::new(move |user: &UserVars| {
                let inherit = Inheritance { workspace: &meta, folders: &folders, base_dir: &base_dir, outside_files };
                let vars = if user.is_empty() { None } else { Some(user_context(user, &base)) };
                let mut resolved =
                    resolve(&request, &inherit, vars.as_ref().unwrap_or(&plain)).map_err(|e| e.message)?;
                if let Some(token) = &token {
                    apply_token(&mut resolved, token);
                }
                Ok(resolved.request)
            });
            let source = RequestSource::from_render(render, &names, dynamic)
                .map_err(|e| ApiError::invalid(format!("{name}: {e}")))?;
            targets.push(PlanTarget {
                name,
                request: target.request.clone(),
                source,
                weight: target.weight,
                captures: target.captures.clone(),
            });
        }
        let plan = Plan {
            targets,
            model: test.model,
            stages: test.stages.clone(),
            think_time: std::time::Duration::from_millis(test.think_time_ms),
            max_in_flight: test.max_in_flight,
            keep_alive: test.keep_alive,
            options,
            thresholds: test.thresholds.clone(),
            rows,
        };
        zorvik_load::validate(&plan).map_err(ApiError::invalid)?;
        Ok(plan)
    }

    /// Keep load tests pointing at a request or folder that was renamed or moved.
    pub(crate) fn retarget_load_tests(&self, ws: &Workspace, old: &str, new: &str) {
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
        for node in ws.list_load_tests().unwrap_or_default().into_iter().filter(|n| n.error.is_none()) {
            let Ok(mut test) = ws.read_load_test(&node.id) else { continue };
            let mut changed = false;
            for target in &mut test.targets {
                if let Some(path) = rewrite(&target.request) {
                    target.request = path;
                    changed = true;
                }
            }
            for threshold in &mut test.thresholds {
                if let Some(path) = threshold.target.as_deref().and_then(rewrite) {
                    threshold.target = Some(path);
                    changed = true;
                }
            }
            // In place: a rename here would change the id under the open tab and its history.
            if changed && let Err(e) = ws.update_load_test(&node.id, &test) {
                tracing::warn!("could not update load test '{}': {}", node.id, e.message);
            }
        }
    }

    /// Whether a load test is running (quitting asks first).
    pub fn load_test_running(&self) -> bool {
        lock(&self.inner.load.active).is_some()
    }

    /// Stop the running load test, if any.
    pub fn stop_load_test(&self) {
        if let Some(active) = lock(&self.inner.load.active).as_ref() {
            active.run.stop();
        }
    }

    /// Stop the running load test and wait (at most `limit`) for it to end, so
    /// its result is saved to the history (quit). Requests in flight get a few
    /// seconds' grace after a stop.
    pub async fn stop_load_test_and_wait(&self, limit: std::time::Duration) {
        self.stop_load_test();
        let deadline = tokio::time::Instant::now() + limit;
        while self.load_test_running() && tokio::time::Instant::now() < deadline {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }

    /// `load-runs/<workspace>/<test id>/` in the app data dir. The id comes from
    /// the UI: one that names no folder of its own (`..`, empty) is refused.
    fn history_dir(&self, ws: &Workspace, test_id: &str) -> ApiResult<PathBuf> {
        let name = sanitize(test_id);
        if name.is_empty() {
            return Err(ApiError::invalid(format!("Invalid load test id '{test_id}'")));
        }
        Ok(self.inner.data_dir.join("load-runs").join(ws.local_key()).join(name))
    }

    fn run_file(&self, ws: &Workspace, test_id: &str, run_id: &str) -> ApiResult<PathBuf> {
        if run_id.is_empty() || !run_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return Err(ApiError::invalid("Invalid run id"));
        }
        Ok(self.history_dir(ws, test_id)?.join(format!("{run_id}.json")))
    }

    pub(crate) fn read_summary(&self, ws: &Workspace, test_id: &str, run_id: &str) -> ApiResult<Summary> {
        let file = self.run_file(ws, test_id, run_id)?;
        let data = std::fs::read(&file).map_err(|_| ApiError::new("notFound", "That run is no longer available"))?;
        let stored: StoredRun = serde_json::from_slice(&data)
            .map_err(|e| ApiError::new("parse", format!("The saved run can't be read: {e}")))?;
        Ok(stored.summary)
    }

    pub(crate) fn load_history(&self, ws: &Workspace, test_id: &str) -> ApiResult<Vec<LoadRunRecord>> {
        let Ok(entries) = std::fs::read_dir(self.history_dir(ws, test_id)?) else { return Ok(Vec::new()) };
        let mut runs: Vec<LoadRunRecord> = entries
            .flatten()
            .filter_map(|e| {
                let data = std::fs::read(e.path()).ok()?;
                let stored: StoredRecord = serde_json::from_slice(&data).ok()?;
                Some(stored.record)
            })
            .collect();
        runs.sort_by(|a, b| b.started_at.total_cmp(&a.started_at));
        Ok(runs)
    }
}

/// What a history file holds: the list entry and the full summary.
#[derive(Serialize, Deserialize)]
struct StoredRun {
    record: LoadRunRecord,
    summary: Summary,
}

/// Only the list entry of a history file (a long run's summary holds a chart
/// point per second: listing doesn't build them).
#[derive(Deserialize)]
struct StoredRecord {
    record: LoadRunRecord,
}

fn save_run(dir: &std::path::Path, run_id: &str, summary: &Summary) {
    let record = LoadRunRecord {
        run_id: run_id.to_string(),
        started_at: summary.started_at,
        duration_ms: summary.duration_ms,
        passed: summary.passed,
        stopped_early: summary.stopped_early,
        requests: summary.totals.requests,
        rps: summary.totals.rps,
        p95: summary.totals.latency.p95,
        error_rate: summary.totals.error_rate,
        error: summary.error.clone(),
    };
    let Ok(data) = serde_json::to_vec(&StoredRun { record, summary: summary.clone() }) else { return };
    if let Err(e) = std::fs::create_dir_all(dir) {
        tracing::warn!("could not save load test run in {}: {e}", dir.display());
        return;
    }
    if let Err(e) = zorvik_workspace::fsutil::atomic_write(&dir.join(format!("{run_id}.json")), &data) {
        tracing::warn!("could not save load test run: {}", e.message);
    }
    // Keep the newest runs only.
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut files: Vec<(std::time::SystemTime, PathBuf)> =
        entries.flatten().filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path()))).collect();
    if files.len() > HISTORY_PER_TEST {
        files.sort();
        for (_, path) in files.iter().take(files.len() - HISTORY_PER_TEST) {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// A file-name-safe version of an id (test ids are file stems already; this is belt and braces).
/// Leading and trailing dots and spaces go (Windows drops trailing ones): `..` becomes empty.
fn sanitize(id: &str) -> String {
    id.chars()
        .map(|c| if c.is_alphanumeric() || matches!(c, '-' | '_' | ' ' | '.') { c } else { '_' })
        .collect::<String>()
        .trim_matches(['.', ' '])
        .to_string()
}

/// Hosts the plan targets that are not this computer or a private network.
fn outside_hosts(plan: &Plan) -> Vec<String> {
    plan_hosts(plan).into_iter().filter(|h| !is_local(h)).collect()
}

/// Every host the plan targets, also the ones that come from the data file.
fn plan_hosts(plan: &Plan) -> Vec<String> {
    zorvik_load::hosts(plan)
}

/// Data file rows as the generator takes them: `(column, value)` with values
/// as variables see them (like the collection runner's data rows).
fn data_rows(data: &crate::runner::DataFile) -> Vec<DataRow> {
    data.rows
        .iter()
        .map(|row| row.iter().map(|(k, v)| (k.clone(), zorvik_script::value_text(v))).collect::<Vec<_>>().into())
        .collect()
}

/// Several layers of variables as one (the first definition of a name wins, as in `VarContext`).
fn flatten(layers: &[Vec<Variable>]) -> Vec<Variable> {
    let mut seen = HashSet::new();
    layers
        .iter()
        .flatten()
        .filter(|v| v.enabled && !v.key.trim().is_empty() && seen.insert(v.key.trim().to_string()))
        .cloned()
        .collect()
}

/// Variables for one user's request: the user's (captured values, then its
/// data row) above `base` (environment, workspace, globals), like the
/// collection runner's data row.
fn user_context(user: &UserVars, base: &[Variable]) -> VarContext {
    let mut ctx = VarContext::new();
    if !user.is_empty() {
        let vars: Vec<Variable> = user
            .iter()
            .map(|(key, value)| Variable { key: key.into(), value: value.into(), enabled: true, secret: false })
            .collect();
        ctx.push_layer(&vars);
    }
    ctx.push_layer(base);
    ctx
}

/// This computer or a private network: loopback, private and link-local
/// addresses (also as IPv4-mapped IPv6), `localhost` and names reserved for
/// local networks (`.local` mDNS, `.home.arpa`, `.internal`).
pub(crate) fn is_local(host: &str) -> bool {
    let host = host.strip_suffix('.').unwrap_or(host);
    if host == "localhost" || [".localhost", ".local", ".home.arpa", ".internal"].iter().any(|s| host.ends_with(s)) {
        return true;
    }
    let v4 = |ip: Ipv4Addr| ip.is_loopback() || ip.is_private() || ip.is_link_local() || ip.is_unspecified();
    match host.parse::<IpAddr>() {
        Ok(IpAddr::V4(ip)) => v4(ip),
        Ok(IpAddr::V6(ip)) => match ip.to_ipv4_mapped() {
            Some(ip) => v4(ip),
            None => {
                ip.is_loopback()
                    || ip.is_unspecified()
                    || (ip.segments()[0] & 0xfe00) == 0xfc00
                    || (ip.segments()[0] & 0xffc0) == 0xfe80
            }
        },
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_hosts() {
        for h in [
            "localhost",
            "localhost.",
            "api.localhost",
            "printer.local",
            "nas.home.arpa",
            "db.internal",
            "127.0.0.1",
            "0.0.0.0",
            "10.1.2.3",
            "172.16.0.1",
            "172.31.255.255",
            "192.168.1.9",
            "169.254.1.1",
            "::1",
            "::",
            "fd00::1",
            "fe80::1",
            "::ffff:7f00:1",
            "::ffff:a00:1",
        ] {
            assert!(is_local(h), "{h}");
        }
        for h in [
            "example.com",
            "example.com.",
            "localhost.example.com",
            "notlocal",
            "8.8.8.8",
            "172.32.0.1",
            "2606:4700::1111",
            "::ffff:808:808",
            "intranet",
        ] {
            assert!(!is_local(h), "{h}");
        }
    }

    #[test]
    fn history_folder_names_stay_inside() {
        assert_eq!(sanitize("../x"), "_x");
        assert_eq!(sanitize("a/b\\c:d"), "a_b_c_d");
        assert_eq!(sanitize("Smoke test"), "Smoke test");
        for id in ["", ".", "..", "...", " . ", ". ."] {
            assert_eq!(sanitize(id), "", "{id:?}");
        }
    }

    #[test]
    fn hosts_from_urls() {
        let target = |url: &str| {
            let request = zorvik_engine::HttpRequest {
                method: "GET".into(),
                url: url.into(),
                headers: Vec::new(),
                body: Default::default(),
            };
            PlanTarget {
                name: url.into(),
                request: String::new(),
                source: RequestSource::Fixed(request),
                weight: 1,
                captures: Vec::new(),
            }
        };
        let plan = Plan {
            targets: vec![
                target("http://[::ffff:127.0.0.1]:8080/"),
                target("http://2130706433/"),
                target("http://LOCALHOST.:3000/x"),
                target("https://API.example.com/a"),
                target("https://api.example.com/b"),
                target("http://[2606:4700::1111]/"),
            ],
            model: Default::default(),
            stages: Vec::new(),
            think_time: std::time::Duration::ZERO,
            max_in_flight: 1,
            keep_alive: true,
            options: Default::default(),
            thresholds: Vec::new(),
            rows: Vec::new(),
        };
        assert_eq!(outside_hosts(&plan), ["api.example.com", "2606:4700::1111"]);
    }
}

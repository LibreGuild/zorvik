//! Checking a lab's steps: what its servers received, what the learner sent and saved,
//! runs that finished, answers typed, and calls to servers the learner runs. Steps pass in
//! order; each check looks at everything since the lab started.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::Ordering;
use std::time::Duration;

use serde_json::{Map, Value, json};
use tokio_util::sync::CancellationToken;
use zorvik_academy::matcher::{self, matches};
use zorvik_academy::progress::Rewards;
use zorvik_academy::{Probe, ProbeKind};
use zorvik_workspace::Workspace;
use zorvik_workspace::formats::{NodeKind, ServerKind, TreeNode};

use crate::{Api, ApiResult, lock};

/// Calls after which a step that calls the learner's server is checked (it may answer now).
pub(super) const PROBE_TRIGGERS: &[&str] = &[
    "server.start",
    "server.update",
    "server.save",
    "server.create",
    "mock.addRoute",
    "mock.fromFolder",
    "mock.fromOpenApi",
];
/// Calls whose effect on a lab server's log may land just after they return.
pub(super) const SETTLE_TRIGGERS: &[&str] = &[
    "http.send",
    "ws.connect",
    "ws.send",
    "sse.connect",
    "socket.connect",
    "socket.send",
    "dns.query",
    "grpc.invoke",
    "grpc.start",
    "grpc.send",
    "runner.start",
];
/// Body text kept of a response for checks.
const BODY_CHARS: usize = 64 * 1024;
/// Calls whose results are big and never checked (noted without them).
const BIG_RESULTS: &[&str] = &[
    "graphql.schema",
    "grpc.describe",
    "workspace.open",
    "workspace.create",
    "workspace.reload",
    "load.run",
    "response.save",
];
/// Log entries of a lab server looked at per check, newest first.
const TRAFFIC_SCAN: usize = 2000;

/// What a round of checks may do beyond reading what the app noted.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Scope {
    /// Call the learner's servers.
    pub probes: bool,
    /// Read the workspace's files.
    pub files: bool,
}

impl Scope {
    pub(crate) const ALL: Scope = Scope { probes: true, files: true };
    /// Once a second: only what can change without a call (server traffic, finished runs).
    pub(crate) const TICK: Scope = Scope { probes: false, files: false };
}

type Checking<'a> = Pin<Box<dyn Future<Output = bool> + Send + 'a>>;

/// Check the lab once a second, for what happens in the background (runs, traffic from
/// other programs), until the lab stops.
pub(super) async fn tick(api: Api, stop: CancellationToken) {
    loop {
        tokio::select! {
            _ = stop.cancelled() => break,
            _ = tokio::time::sleep(Duration::from_secs(1)) => api.evaluate(Scope::TICK).await,
        }
    }
}

pub(super) fn has_probe(check: &zorvik_academy::Check) -> bool {
    match check {
        zorvik_academy::Check::Probe(_) => true,
        zorvik_academy::Check::All(list) | zorvik_academy::Check::Any(list) => list.iter().any(has_probe),
        _ => false,
    }
}

/// What the journal keeps of a call: `{method, params, ok, result, error}`.
pub(super) fn fact_value(method: &str, params: Value, result: &ApiResult<Value>) -> Value {
    if method == "http.send" {
        let request = params.get("request").cloned().unwrap_or(Value::Null);
        let summary = send_summary(&request, result);
        return json!({ "method": method, "params": params, "ok": result.is_ok(), "result": summary });
    }
    let (ok, result, error) = match result {
        Ok(v) => (true, if BIG_RESULTS.contains(&method) { Value::Null } else { v.clone() }, Value::Null),
        Err(e) => (false, Value::Null, json!({ "code": e.code, "message": e.message })),
    };
    json!({ "method": method, "params": params, "ok": ok, "result": result, "error": error })
}

/// Headers as an object of lower-case names (repeated ones joined with ", ").
fn header_map(list: &Value) -> Value {
    let mut out = Map::new();
    for h in list.as_array().into_iter().flatten() {
        let (Some(name), Some(value)) = (h["name"].as_str(), h["value"].as_str()) else { continue };
        let name = name.to_ascii_lowercase();
        match out.get_mut(&name) {
            Some(Value::String(v)) => {
                v.push_str(", ");
                v.push_str(value);
            }
            _ => {
                out.insert(name, Value::String(value.to_string()));
            }
        }
    }
    Value::Object(out)
}

fn cut(text: &str) -> String {
    match text.char_indices().nth(BODY_CHARS) {
        Some((i, _)) => text[..i].to_string(),
        None => text.to_string(),
    }
}

/// An HTTP send as checks see it (see `docs/academy.md`).
fn send_summary(request: &Value, result: &ApiResult<Value>) -> Value {
    let kind = request.get("kind").and_then(Value::as_str).unwrap_or("http");
    let auth = request.pointer("/auth/type").cloned().unwrap_or(json!("inherit"));
    match result {
        Ok(r) => {
            let meta = &r["meta"];
            let sent = &meta["request"];
            let text = r.pointer("/body/text").and_then(Value::as_str).map(cut);
            let json = text.as_deref().and_then(|t| serde_json::from_str::<Value>(t).ok());
            let tests: Vec<Value> = r
                .pointer("/scripts/tests")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|t| json!({ "name": t["name"], "passed": t["passed"] }))
                .collect();
            let passed = tests.iter().filter(|t| t["passed"] == json!(true)).count();
            json!({
                "kind": kind,
                "method": sent["method"],
                "url": sent["url"],
                "finalUrl": meta["url"],
                "status": meta["status"],
                "statusText": meta["statusText"],
                "httpVersion": meta["httpVersion"],
                "headers": header_map(&sent["headers"]),
                "responseHeaders": header_map(&meta["headers"]),
                "body": text,
                "json": json,
                "tests": tests,
                "testsPassed": passed,
                "testsFailed": tests.len() - passed,
                // `pm.visualizer.set` made a Visualize tab.
                "visualized": r.pointer("/scripts/visualization").is_some_and(Value::is_string),
                "redirects": meta["redirects"].as_array().map_or(0, Vec::len),
                "tls": meta["tls"],
                "auth": auth,
                "request": request,
                "error": null,
            })
        }
        Err(e) => json!({
            "kind": kind,
            "method": request["method"],
            "url": request["url"],
            "status": null,
            "auth": auth,
            "request": request,
            "error": e.message,
            "errorCode": e.code,
            "errorKind": e.network_kind,
        }),
    }
}

/// An HTTP exchange a lab server logged.
fn http_fact(h: &zorvik_servers::HttpExchange) -> Value {
    let (path, query) = h.path.split_once('?').unwrap_or((&h.path, ""));
    let mut q = Map::new();
    for (k, v) in url::form_urlencoded::parse(query.as_bytes()) {
        q.entry(k.into_owned()).or_insert(Value::String(v.into_owned()));
    }
    let headers: Vec<Value> = h.request_headers.iter().map(|x| json!({ "name": x.name, "value": x.value })).collect();
    json!({
        "method": h.method,
        "path": path,
        "query": q,
        "headers": header_map(&Value::Array(headers)),
        "body": h.request_body,
        "json": serde_json::from_str::<Value>(&h.request_body).ok(),
        "status": h.status,
        "route": h.route,
        "httpVersion": h.http_version,
    })
}

/// A lab's state for one round of checks, taken so no lock is held while checking.
struct Round<'a> {
    api: &'a Api,
    ws: &'a Workspace,
    since: i64,
    vars: Vec<(String, String)>,
    secrets: HashMap<String, String>,
    /// Lab server id → run id.
    servers: HashMap<String, String>,
    /// Saved ids of the lab's own servers (not the learner's).
    lab_files: Vec<String>,
    answers: Vec<Option<String>>,
}

impl Round<'_> {
    fn lookup(&self, name: &str) -> Option<String> {
        match name.strip_prefix("secret.") {
            Some(s) => self.secrets.get(s).cloned(),
            None => self.vars.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone()),
        }
    }

    fn pattern(&self, v: &Value) -> Value {
        matcher::substitute(v, &|n| self.lookup(n))
    }

    fn check<'s>(&'s self, check: &'s zorvik_academy::Check, step: usize, scope: Scope) -> Checking<'s> {
        Box::pin(async move {
            match check {
                zorvik_academy::Check::Request(t) | zorvik_academy::Check::Message(t) => {
                    let http = matches!(check, zorvik_academy::Check::Request(_));
                    self.traffic(&t.server, &t.pattern, http)
                }
                zorvik_academy::Check::Send(p) => {
                    let p = self.pattern(p);
                    self.any_fact(|f| f["method"] == "http.send" && matches(&p, Some(&f["result"])))
                }
                zorvik_academy::Check::Call(p) => {
                    let p = self.pattern(p);
                    self.any_fact(|f| matches(&p, Some(f)))
                }
                zorvik_academy::Check::Saved(p) => scope.files && self.saved(&self.pattern(p)),
                zorvik_academy::Check::Run(p) => {
                    let p = self.pattern(p);
                    self.api.finished_runs().iter().any(|r| {
                        r.summary.started_at >= self.since as f64
                            && serde_json::to_value(&r.summary).is_ok_and(|v| matches(&p, Some(&v)))
                    })
                }
                zorvik_academy::Check::Load(p) => self.load(&self.pattern(p)),
                zorvik_academy::Check::Probe(probe) => scope.probes && self.probe(probe).await,
                zorvik_academy::Check::Answer(a) => {
                    let Some(Some(answer)) = self.answers.get(step) else { return false };
                    let answer = Value::String(answer.clone());
                    a.patterns()
                        .iter()
                        .any(|p| matcher::text_matches(&matcher::substitute_text(p, &|n| self.lookup(n)), &answer))
                }
                zorvik_academy::Check::All(list) => {
                    for c in list {
                        if !self.check(c, step, scope).await {
                            return false;
                        }
                    }
                    true
                }
                zorvik_academy::Check::Any(list) => {
                    for c in list {
                        if self.check(c, step, scope).await {
                            return true;
                        }
                    }
                    false
                }
            }
        })
    }

    /// Whether a journal entry since the lab started passes `f` (checked in place, newest first).
    fn any_fact(&self, f: impl Fn(&Value) -> bool) -> bool {
        lock(&self.api.inner.academy.journal).iter().rev().take_while(|x| x.at >= self.since).any(|x| f(&x.value))
    }

    /// `count` (at least that many matches, default 1) and the pattern for each entry.
    fn traffic(&self, server: &str, pattern: &Map<String, Value>, http: bool) -> bool {
        let Some(run_id) = self.servers.get(server) else { return false };
        let mut pattern = pattern.clone();
        let count = pattern.remove("count").and_then(|c| c.as_u64().or_else(|| c.as_str()?.parse().ok())).unwrap_or(1);
        let pattern = self.pattern(&Value::Object(pattern));
        let since = self.since as f64;
        // The newest entries, copied out so the server isn't held up while they're matched
        // (a lab server under load keeps thousands).
        let Some(entries) = self.api.with_server_log(run_id, |entries| {
            entries
                .iter()
                .rev()
                .take_while(|e| e.timestamp >= since)
                .filter(|e| e.http.is_some() == http)
                .take(TRAFFIC_SCAN)
                .cloned()
                .collect::<Vec<_>>()
        }) else {
            return false;
        };
        let found = entries
            .iter()
            .filter_map(|e| match (&e.http, http) {
                (Some(h), true) => Some(http_fact(h)),
                (None, false) => {
                    let mut v = serde_json::to_value(e).ok()?;
                    if let Some(o) = v.as_object_mut() {
                        o.remove("http");
                        o.remove("base64");
                    }
                    Some(v)
                }
                _ => None,
            })
            .filter(|fact| matches(&pattern, Some(fact)))
            .take(count as usize)
            .count();
        found as u64 >= count
    }

    /// Items saved in the workspace, by kind (`request`, `folder`, `environment`, `server`, `loadTest`, `workspace`).
    fn saved(&self, pattern: &Value) -> bool {
        let Some(kinds) = pattern.as_object() else { return false };
        kinds.iter().all(|(kind, p)| self.saved_items(kind).iter().any(|item| matches(p, Some(item))))
    }

    fn saved_items(&self, kind: &str) -> Vec<Value> {
        let ws = self.ws;
        let with = |mut v: Value, key: &str, value: &str| {
            if let Some(o) = v.as_object_mut() {
                o.insert(key.into(), Value::String(value.into()));
            }
            v
        };
        match kind {
            "request" | "folder" => {
                let mut nodes = Vec::new();
                fn walk<'n>(list: &'n [TreeNode], out: &mut Vec<&'n TreeNode>) {
                    for n in list {
                        out.push(n);
                        walk(&n.children, out);
                    }
                }
                let tree = ws.tree().unwrap_or_default();
                walk(&tree, &mut nodes);
                nodes
                    .into_iter()
                    .filter_map(|n| match (n.kind, kind) {
                        (NodeKind::Request, "request") => {
                            Some(with(serde_json::to_value(ws.read_request(&n.path).ok()?).ok()?, "path", &n.path))
                        }
                        (NodeKind::Folder, "folder") => {
                            let v = with(serde_json::to_value(ws.read_folder(&n.path).ok()?).ok()?, "path", &n.path);
                            Some(with(v, "name", &n.name))
                        }
                        _ => None,
                    })
                    .collect()
            }
            "environment" => ws
                .list_environments()
                .unwrap_or_default()
                .into_iter()
                .filter_map(|e| Some(with(serde_json::to_value(&e.environment).ok()?, "id", &e.id)))
                .collect(),
            "server" => {
                let running = self.api.inner.servers.running();
                let root = ws.root().to_string_lossy();
                ws.list_servers()
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|n| !self.lab_files.contains(&n.id))
                    .filter_map(|n| {
                        let mut v = with(serde_json::to_value(ws.read_server(&n.id).ok()?).ok()?, "id", &n.id);
                        let run = running.iter().find(|r| r.workspace_path == root && r.server_id == n.id);
                        v["running"] = json!(run.is_some());
                        Some(v)
                    })
                    .collect()
            }
            "loadTest" => ws
                .list_load_tests()
                .unwrap_or_default()
                .into_iter()
                .filter_map(|n| Some(with(serde_json::to_value(ws.read_load_test(&n.id).ok()?).ok()?, "id", &n.id)))
                .collect(),
            "workspace" => serde_json::to_value(self.api.revealed_meta(ws)).into_iter().collect(),
            _ => Vec::new(),
        }
    }

    /// Load test runs that finished during the lab: their record plus the test's `name` and `id`.
    fn load(&self, pattern: &Value) -> bool {
        let tests = self.ws.list_load_tests().unwrap_or_default();
        tests.iter().any(|t| {
            self.api.load_history(self.ws, &t.id).unwrap_or_default().into_iter().any(|run| {
                if run.started_at < self.since as f64 {
                    return false;
                }
                let Ok(mut v) = serde_json::to_value(&run) else { return false };
                v["name"] = json!(t.name);
                v["id"] = json!(t.id);
                matches(pattern, Some(&v))
            })
        })
    }

    /// Call the learner's running servers of the probe's kind; passes when one answers as expected.
    async fn probe(&self, probe: &Probe) -> bool {
        let kind = match probe.kind {
            ProbeKind::Http => ServerKind::Http,
            ProbeKind::Tcp => ServerKind::Tcp,
            ProbeKind::Udp => ServerKind::Udp,
            ProbeKind::Mcp => ServerKind::Mcp,
        };
        let root = self.ws.root().to_string_lossy().into_owned();
        let targets: Vec<(String, u16)> = self
            .api
            .inner
            .servers
            .running()
            .into_iter()
            .filter(|r| r.workspace_path == root && r.kind == kind && !r.tls && !self.lab_files.contains(&r.server_id))
            .filter(|r| probe.name.as_deref().is_none_or(|p| matcher::text_matches(p, &Value::String(r.name.clone()))))
            .map(|r| (r.url, r.port))
            .collect();
        let expect = self.pattern(&probe.expect);
        let body = probe.body.as_deref().map(|b| matcher::substitute_text(b, &|n| self.lookup(n)));
        for (url, port) in targets {
            let answer = match probe.kind {
                ProbeKind::Http => self.probe_http(probe, &url, body.as_deref()).await,
                ProbeKind::Tcp => probe_tcp(port, body.as_deref().unwrap_or("")).await,
                ProbeKind::Udp => probe_udp(port, body.as_deref().unwrap_or("")).await,
                ProbeKind::Mcp => self.probe_mcp(probe, &url, body.as_deref()).await,
            };
            if answer.is_some_and(|a| matches(&expect, Some(&a))) {
                return true;
            }
        }
        false
    }

    async fn probe_http(&self, probe: &Probe, base: &str, body: Option<&str>) -> Option<Value> {
        let path = probe.path.as_deref().map_or("/".to_string(), |p| matcher::substitute_text(p, &|n| self.lookup(n)));
        let request = zorvik_engine::HttpRequest {
            method: probe.method.clone().unwrap_or_else(|| "GET".into()),
            url: format!(
                "{}{}",
                base.trim_end_matches('/'),
                if path.starts_with('/') { path } else { format!("/{path}") }
            ),
            headers: probe
                .headers
                .iter()
                .map(|(k, v)| zorvik_engine::Header { name: k.clone(), value: v.clone() })
                .collect(),
            body: body.unwrap_or_default().as_bytes().to_vec().into(),
        };
        let opts = zorvik_engine::RequestOptions { timeout: Some(Duration::from_secs(3)), ..Default::default() };
        let response = self.api.inner.client.send(request, &opts, None).await.ok()?;
        let text = String::from_utf8_lossy(&response.body).into_owned();
        let headers: Vec<Value> =
            response.meta.headers.iter().map(|h| json!({ "name": h.name, "value": h.value })).collect();
        Some(json!({
            "status": response.meta.status,
            "headers": header_map(&Value::Array(headers)),
            "json": serde_json::from_str::<Value>(&text).ok(),
            "body": text,
        }))
    }
}

impl Round<'_> {
    /// Connect to the learner's MCP server (its URL ends with the endpoint path) and ask it the
    /// probe's method, with the body as the parameters: `{result}` or `{error}`.
    async fn probe_mcp(&self, probe: &Probe, url: &str, body: Option<&str>) -> Option<Value> {
        let params = match body.map(str::trim).filter(|b| !b.is_empty()) {
            Some(b) => serde_json::from_str::<Value>(b).ok()?,
            None => Value::Null,
        };
        let method = probe.method.as_deref().unwrap_or("tools/list");
        let target = zorvik_engine::mcp::McpTarget {
            address: url.to_string(),
            transport: zorvik_engine::mcp::McpTransport::StreamableHttp,
            headers: Vec::new(),
            env: Vec::new(),
            cwd: None,
        };
        let opts = zorvik_engine::RequestOptions { timeout: Some(Duration::from_secs(3)), ..Default::default() };
        let run = async {
            let connected = Box::pin(self.api.inner.client.mcp(target, &opts, None)).await.ok()?;
            Some(match connected.client.request(method, params, Duration::from_secs(3)).await {
                Ok(result) => json!({ "result": result }),
                Err(e) => json!({ "error": { "code": e.code, "message": e.message } }),
            })
        };
        tokio::time::timeout(Duration::from_secs(5), run).await.ok().flatten()
    }
}

async fn probe_tcp(port: u16, send: &str) -> Option<Value> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let run = async {
        let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.ok()?;
        if !send.is_empty() {
            stream.write_all(send.as_bytes()).await.ok()?;
        }
        let mut buf = vec![0u8; 64 * 1024];
        let n = stream.read(&mut buf).await.ok()?;
        Some(json!({ "text": String::from_utf8_lossy(&buf[..n]) }))
    };
    tokio::time::timeout(Duration::from_secs(2), run).await.ok().flatten()
}

async fn probe_udp(port: u16, send: &str) -> Option<Value> {
    let run = async {
        let socket = tokio::net::UdpSocket::bind(("127.0.0.1", 0)).await.ok()?;
        socket.send_to(send.as_bytes(), ("127.0.0.1", port)).await.ok()?;
        let mut buf = vec![0u8; 64 * 1024];
        let (n, _) = socket.recv_from(&mut buf).await.ok()?;
        Some(json!({ "text": String::from_utf8_lossy(&buf[..n]) }))
    };
    tokio::time::timeout(Duration::from_secs(2), run).await.ok().flatten()
}

impl Api {
    /// Check the running lab's steps in order from the first one not done, within `scope`;
    /// a step that becomes current in this round is always checked in full once.
    pub(crate) async fn evaluate(&self, scope: Scope) {
        let academy = &self.inner.academy;
        if !academy.active.load(Ordering::Relaxed) {
            return;
        }
        let _one = academy.evaluating.lock().await;
        let Ok(course) = zorvik_academy::course() else { return };
        let Some(ws) = self.try_ws().filter(|ws| self.is_bootcamp(ws)) else { return };
        let Some((generation, lesson_id, round, first)) = self.with_lab(|lab| {
            let round = Round {
                api: self,
                ws: &ws,
                since: lab.started_at,
                vars: lab.vars.clone(),
                secrets: lab.secrets.clone(),
                servers: lab.servers.iter().map(|s| (s.key.clone(), s.run_id.clone())).collect(),
                lab_files: lab.servers.iter().map(|s| s.file_id.clone()).collect(),
                answers: lab.steps.iter().map(|s| s.answer.clone()).collect(),
            };
            (lab.generation, lab.lesson.clone(), round, lab.current())
        }) else {
            return;
        };
        let Some(first) = first else { return };
        let Some(lesson) = course.lesson(&lesson_id) else { return };
        let Some(spec) = lesson.lab.as_ref() else { return };
        let clock = self.clock();
        let mut rewards = Rewards::default();
        let mut passed_any = false;
        let mut i = first;
        while let Some(step) = spec.steps.get(i) {
            if !round.check(&step.check, i, if i == first { scope } else { Scope::ALL }).await {
                break;
            }
            // The lab may have been stopped or restarted while checking.
            let assisted = {
                let mut lab = lock(&academy.lab);
                let Some(lab) = lab.as_mut().filter(|l| l.generation == generation) else { return };
                let Some(run) = lab.steps.get_mut(i) else { return };
                run.done = true;
                run.assisted
            };
            self.with_progress(|p| p.step_done(&lesson.id, i as u32, assisted, &clock, &mut rewards));
            passed_any = true;
            i += 1;
        }
        if !passed_any {
            return;
        }
        if i == spec.steps.len() {
            let finished = {
                let mut lab = lock(&academy.lab);
                lab.as_mut().filter(|l| l.generation == generation && !l.finished).map(|lab| {
                    lab.finished = true;
                    // Nothing left to check: the journal and the ticker stop until the next lab.
                    academy.active.store(false, Ordering::Relaxed);
                    lab.ticker.cancel();
                    let hints = lab.steps.iter().map(|s| s.hints).sum::<u32>();
                    let assisted = lab.steps.iter().any(|s| s.assisted);
                    (hints, assisted, (clock.now - lab.started_at) / 1000)
                })
            };
            if let Some((hints, assisted, secs)) = finished {
                self.with_progress(|p| p.lab_done(course, lesson, hints, assisted, secs, &clock, &mut rewards));
            }
        }
        self.emit_academy(Some(rewards));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_send_says_whether_scripts_made_a_visualization() {
        let request = json!({ "name": "r", "seq": 0, "method": "GET", "url": "{{api}}/x" });
        let answer = |scripts: Value| {
            let r = json!({
                "meta": { "status": 200, "request": { "method": "GET", "url": "http://127.0.0.1/x", "headers": [] }, "headers": [] },
                "body": { "text": "{}" },
                "scripts": scripts,
            });
            send_summary(&request, &Ok(r))
        };
        let shown = answer(json!({ "tests": [], "console": [], "errors": [], "visualization": "<table></table>" }));
        assert_eq!(shown["visualized"], json!(true));
        assert_eq!(answer(json!({ "tests": [], "console": [], "errors": [] }))["visualized"], json!(false));
        assert_eq!(answer(Value::Null)["visualized"], json!(false));
    }
}

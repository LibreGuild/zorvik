//! Load generator. Runs a [`Plan`] on its own Tokio runtime (so the app stays
//! responsive), with a pooled HTTP client, and reports [`Snapshot`]s about 4
//! times a second and a final [`Summary`]. See docs/architecture.md ("Load testing").
//!
//! Each virtual user has its own variables ([`UserVars`]): a data file row and
//! the values its captures took from earlier responses. In the arrival-rate
//! model every request is its own iteration: it takes the next row, and its
//! captures are checked (misses are counted) but no later request uses them.

mod capture;
mod html;
mod metrics;
pub mod report;
mod runner;
mod schedule;

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use zorvik_engine::{HttpRequest, HttpVersionPref, RequestOptions};
use zorvik_formats::{LoadCapture, LoadModel, LoadStage, Threshold};

pub use capture::{DataRow, UserVars, server_timing_ms};
pub use report::*;

/// Most virtual users / requests per second a plan may ask for.
pub const MAX_USERS: u32 = 5_000;
pub const MAX_RATE: u32 = 50_000;
/// Most requests in flight (open model); bounds the tasks and connections held.
pub const MAX_IN_FLIGHT: u32 = 100_000;

/// Renders a target's request with one user's variables.
pub type Render = Arc<dyn Fn(&UserVars) -> Result<HttpRequest, String> + Send + Sync>;

/// What one target sends on each iteration.
#[derive(Clone)]
pub enum RequestSource {
    /// Resolved once before the run.
    Fixed(HttpRequest),
    /// Rendered again for every request: it uses dynamic variables such as
    /// `{{$uuid}}`, or the user's variables (data file columns, captured values).
    Dynamic(Render),
}

impl RequestSource {
    /// `Fixed` when the request needs no rendering per iteration: it has no
    /// dynamic values (`dynamic`: it uses `{{$uuid}}` and the like) and is the
    /// same with and without the user's variables `names`. Else `Dynamic`.
    /// Requests that use neither keep the fast path.
    pub fn from_render(render: Render, names: &[String], dynamic: bool) -> Result<Self, String> {
        let plain = render(&UserVars::default())?;
        if dynamic {
            return Ok(Self::Dynamic(render));
        }
        if !names.is_empty() {
            let probed = render(&UserVars::probe(names))?;
            let same = plain.method == probed.method
                && plain.url == probed.url
                && plain.headers == probed.headers
                && plain.body == probed.body;
            if !same {
                return Ok(Self::Dynamic(render));
            }
        }
        Ok(Self::Fixed(plain))
    }
}

#[derive(Clone)]
pub struct PlanTarget {
    pub name: String,
    /// Request path (relative to `requests/`), used for per-target thresholds.
    pub request: String,
    pub source: RequestSource,
    pub weight: u32,
    /// Values to take from this target's responses for the user's later requests.
    pub captures: Vec<LoadCapture>,
}

#[derive(Clone)]
pub struct Plan {
    pub targets: Vec<PlanTarget>,
    pub model: LoadModel,
    pub stages: Vec<LoadStage>,
    pub think_time: Duration,
    pub max_in_flight: u32,
    pub keep_alive: bool,
    /// Timeout, TLS, proxy, HTTP version, redirects, decompression.
    pub options: RequestOptions,
    pub thresholds: Vec<Threshold>,
    /// Data file rows: virtual user N takes row N % rows (in start order); in
    /// the arrival-rate model each request takes the next row. Empty: none.
    pub rows: Vec<DataRow>,
}

impl Plan {
    /// Names a request may take from a user: data file columns and captured variables.
    pub fn user_variables(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        let captured = self.targets.iter().flat_map(|t| &t.captures).map(|c| c.variable.trim());
        let columns = self.rows.iter().flat_map(|row| row.iter()).map(|(k, _)| k.as_str());
        for name in captured.chain(columns) {
            if !name.is_empty() && !names.iter().any(|n| n == name) {
                names.push(name.to_string());
            }
        }
        names
    }

    /// Captures change what later requests send: each user then goes through
    /// the requests in their weighted order, so a create comes before its get.
    pub(crate) fn has_captures(&self) -> bool {
        self.targets.iter().any(|t| t.weight > 0 && !t.captures.is_empty())
    }
}

/// Every host the plan sends to (lowercase, IPv6 without brackets), for the
/// checks before a run. A request whose host comes from the data file is
/// rendered with every row. A host that would come from a captured value can't
/// be known before the run: the run refuses requests that don't go where the
/// plan's requests go (the same scheme, host, port and `Host` header).
pub fn hosts(plan: &Plan) -> Vec<String> {
    let mut hosts: Vec<String> = Vec::new();
    each_request(plan, |req| {
        if let Some(to) = Destination::of(req)
            && !hosts.contains(&to.host)
        {
            hosts.push(to.host);
        }
    });
    hosts
}

/// Where a request goes: the scheme, host and port it connects to, and the
/// `Host` header it sends instead of the URL's host (the virtual host, and
/// `:authority` on HTTP/2).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct Destination {
    https: bool,
    /// Lowercase, IPv6 without brackets.
    host: String,
    port: u16,
    /// Every `Host` header the request sets (lowercase), in order.
    virtual_hosts: Vec<String>,
}

impl Destination {
    /// `None` when the URL is not an http(s) URL with a host (it can't be sent).
    pub(crate) fn of(req: &HttpRequest) -> Option<Self> {
        let url = zorvik_engine::http::normalize_url(&req.url).ok()?;
        let host = url.host_str()?.trim_start_matches('[').trim_end_matches(']').to_ascii_lowercase();
        let virtual_hosts = req
            .headers
            .iter()
            .filter(|h| h.name.trim().eq_ignore_ascii_case("host"))
            .map(|h| h.value.trim().to_ascii_lowercase())
            .collect();
        Some(Self { https: url.scheme() == "https", host, port: url.port_or_known_default()?, virtual_hosts })
    }

    fn mentions(&self, text: &str) -> bool {
        self.host.contains(text) || self.virtual_hosts.iter().any(|h| h.contains(text))
    }
}

impl std::fmt::Display for Destination {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let scheme = if self.https { "https" } else { "http" };
        if self.host.contains(':') {
            write!(f, "{scheme}://[{}]:{}", self.host, self.port)?;
        } else {
            write!(f, "{scheme}://{}:{}", self.host, self.port)?;
        }
        if !self.virtual_hosts.is_empty() {
            write!(f, " with Host {}", self.virtual_hosts.join(", "))?;
        }
        Ok(())
    }
}

/// Everywhere the plan sends to, known before the run: a rendered request
/// must go to one of these, so a captured value can't move the load to another
/// host, port, scheme or virtual host.
pub(crate) fn destinations(plan: &Plan) -> HashSet<Destination> {
    let mut all = HashSet::new();
    each_request(plan, |req| {
        all.extend(Destination::of(req));
    });
    all
}

/// Every request of the plan as the checks before a run see it. A request
/// whose destination (scheme, host, port or `Host` header) comes from the data
/// file is rendered with every row; others once, with the first row.
fn each_request(plan: &Plan, mut visit: impl FnMut(&HttpRequest)) {
    let names = plan.user_variables();
    let first = UserVars::new(plan.rows.first().cloned());
    for target in plan.targets.iter().filter(|t| t.weight > 0) {
        match &target.source {
            RequestSource::Fixed(r) => visit(r),
            RequestSource::Dynamic(render) => {
                // A probe value in a port or scheme leaves no valid URL.
                let varies = !plan.rows.is_empty()
                    && render(&UserVars::probe(&names))
                        .is_ok_and(|r| Destination::of(&r).is_none_or(|to| to.mentions(capture::PROBE)));
                if varies {
                    for row in &plan.rows {
                        if let Ok(r) = render(&UserVars::new(Some(row.clone()))) {
                            visit(&r);
                        }
                    }
                } else if let Ok(r) = render(&first) {
                    visit(&r);
                }
            }
        }
    }
}

/// Check a plan before running it; the message is shown to the user.
pub fn validate(plan: &Plan) -> Result<(), String> {
    if !plan.targets.iter().any(|t| t.weight > 0) {
        return Err("Add at least one request to send".into());
    }
    if plan.stages.is_empty() || plan.stages.iter().all(|s| s.duration_secs == 0) {
        return Err("Add a stage with a duration".into());
    }
    let (limit, what) = match plan.model {
        LoadModel::VirtualUsers => (MAX_USERS, "virtual users"),
        LoadModel::ArrivalRate => (MAX_RATE, "requests per second"),
    };
    if let Some(stage) = plan.stages.iter().find(|s| s.target > limit) {
        return Err(format!("{} {what} is more than the limit of {limit}", stage.target));
    }
    if plan.model == LoadModel::ArrivalRate && plan.max_in_flight == 0 {
        return Err("Allow at least one request in flight".into());
    }
    if plan.model == LoadModel::ArrivalRate && plan.max_in_flight > MAX_IN_FLIGHT {
        return Err(format!("{} requests in flight is more than the limit of {MAX_IN_FLIGHT}", plan.max_in_flight));
    }
    if plan.options.http_version == HttpVersionPref::Http3 {
        return Err("HTTP/3 isn't supported for load tests yet".into());
    }
    for target in plan.targets.iter().filter(|t| t.weight > 0) {
        for c in &target.captures {
            capture::Capture::new(c).map_err(|e| format!("{}: {e}", target.name))?;
        }
    }
    Ok(())
}

/// A running load test. Dropping it does not stop the run; call [`LoadRun::stop`].
pub struct LoadRun {
    stop: tokio_util::sync::CancellationToken,
}

impl LoadRun {
    /// Stop early: no new requests start; requests in flight get a short grace
    /// period; then [`LoadEvent::Finished`] is sent with `stopped_early`.
    pub fn stop(&self) {
        self.stop.cancel();
    }
}

/// Where snapshots and the final summary go (called from the load runtime's threads).
pub type EventFn = Arc<dyn Fn(LoadEvent) + Send + Sync>;

/// Start a plan on its own runtime. Returns once the run has started;
/// [`LoadEvent::Finished`] is always sent exactly once at the end.
///
/// Requests are sent over pooled connections (see `zorvik_engine::pool`):
/// redirects are not followed (a 3xx counts as an answer) and bodies are read
/// and counted, not decoded or kept.
pub fn start(plan: Plan, on_event: EventFn) -> Result<LoadRun, String> {
    validate(&plan)?;
    let prepared = runner::prepare(plan)?;
    let stop = tokio_util::sync::CancellationToken::new();
    runner::spawn(prepared, on_event, stop.clone())?;
    Ok(LoadRun { stop })
}

/// A self-contained HTML report (inline styles, SVG charts and a small script
/// for their tooltips; no external resources).
pub fn html_report(title: &str, summary: &Summary) -> String {
    html::report(title, summary)
}

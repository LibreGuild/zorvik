//! Load generator. Runs a [`Plan`] on its own Tokio runtime (so the app stays
//! responsive), with a pooled HTTP client, and reports [`Snapshot`]s about 4
//! times a second and a final [`Summary`]. See docs/architecture.md ("Load testing").

mod html;
mod metrics;
pub mod report;
mod runner;
mod schedule;

use std::sync::Arc;
use std::time::Duration;

use zorvik_engine::{HttpRequest, HttpVersionPref, RequestOptions};
use zorvik_formats::{LoadModel, LoadStage, Threshold};

pub use report::*;

/// Most virtual users / requests per second a plan may ask for.
pub const MAX_USERS: u32 = 5_000;
pub const MAX_RATE: u32 = 50_000;
/// Most requests in flight (open model); bounds the tasks and connections held.
pub const MAX_IN_FLIGHT: u32 = 100_000;

/// What one target sends on each iteration.
#[derive(Clone)]
pub enum RequestSource {
    /// Resolved once before the run.
    Fixed(HttpRequest),
    /// Rendered again for every request (it uses dynamic variables such as `{{$uuid}}`).
    Dynamic(Arc<dyn Fn() -> Result<HttpRequest, String> + Send + Sync>),
}

#[derive(Clone)]
pub struct PlanTarget {
    pub name: String,
    /// Request path (relative to `requests/`), used for per-target thresholds.
    pub request: String,
    pub source: RequestSource,
    pub weight: u32,
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

/// A self-contained HTML report (inline styles and SVG charts, no scripts).
pub fn html_report(title: &str, summary: &Summary) -> String {
    html::report(title, summary)
}

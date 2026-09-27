//! What a load test reports: live snapshots (4× per second) and the final
//! summary. Latencies are milliseconds.

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use zorvik_formats::{ThresholdMetric, ThresholdOp};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LatencySummary {
    pub min: f64,
    pub avg: f64,
    pub p50: f64,
    pub p90: f64,
    pub p95: f64,
    pub p99: f64,
    pub p999: f64,
    pub max: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MetricsSummary {
    /// Completed requests (with an answer or an error).
    #[ts(type = "number")]
    pub requests: u64,
    /// Network errors plus HTTP status ≥ 400.
    #[ts(type = "number")]
    pub errors: u64,
    /// Percent of requests that failed.
    pub error_rate: f64,
    /// Completed requests per second over the elapsed time.
    pub rps: f64,
    #[ts(type = "number")]
    pub bytes_in: u64,
    #[ts(type = "number")]
    pub bytes_out: u64,
    pub latency: LatencySummary,
    /// `[status, count]`, most frequent first.
    #[ts(type = "Array<[number, number]>")]
    pub status_codes: Vec<(u16, u64)>,
    /// `[error kind, count]` for network errors (`timeout`, `connect`, …).
    #[ts(type = "Array<[string, number]>")]
    pub error_kinds: Vec<(String, u64)>,
    /// Open model: iterations not started because `maxInFlight` was reached.
    #[ts(type = "number")]
    pub dropped: u64,
    /// New connections opened (each new connection pays DNS/TCP/TLS).
    #[ts(type = "number")]
    pub connections: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TargetSummary {
    /// Request name.
    pub name: String,
    /// Request path (relative to `requests/`).
    pub request: String,
    pub metrics: MetricsSummary,
}

/// One second of the run, for the charts.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TimePoint {
    /// Seconds since the start.
    pub second: u32,
    /// Requests completed in this second.
    pub rps: f64,
    pub errors: u32,
    pub p50: f64,
    pub p95: f64,
    pub p99: f64,
    /// Users (closed model) or requests in flight (open model) at the end of the second.
    pub active: u32,
    /// Stage target: users at the end of the second, or the average request
    /// rate over the second.
    pub target: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ThresholdResult {
    /// E.g. `p95 < 300 ms` or `errorRate < 1 % · Get users`.
    pub label: String,
    pub metric: ThresholdMetric,
    pub op: ThresholdOp,
    pub value: f64,
    pub target: Option<String>,
    /// `None` until there is data.
    pub actual: Option<f64>,
    pub passed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum RunPhase {
    Starting,
    Running,
    /// Stop requested: waiting for requests in flight (bounded).
    Stopping,
    Finished,
}

/// Sent about 4 times a second while a test runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Snapshot {
    pub phase: RunPhase,
    #[ts(type = "number")]
    pub elapsed_ms: u64,
    #[ts(type = "number")]
    pub planned_ms: u64,
    /// Users (closed) or requests in flight (open) right now.
    pub active: u32,
    /// Current stage target (users or requests per second).
    pub target: f64,
    pub totals: MetricsSummary,
    pub targets: Vec<TargetSummary>,
    /// Seconds completed since the previous snapshot (append to the charts).
    pub points: Vec<TimePoint>,
    pub thresholds: Vec<ThresholdResult>,
    /// CPU used by this app (percent of one core × cores, 0–100 per core summed), when known.
    pub cpu_percent: Option<f32>,
}

/// The result of a run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Summary {
    /// Unix epoch milliseconds.
    pub started_at: f64,
    #[ts(type = "number")]
    pub duration_ms: u64,
    pub totals: MetricsSummary,
    pub targets: Vec<TargetSummary>,
    pub points: Vec<TimePoint>,
    pub thresholds: Vec<ThresholdResult>,
    /// Every threshold passed (and the run was not aborted by an error).
    pub passed: bool,
    /// Stopped before the planned end.
    pub stopped_early: bool,
    /// Why the run could not continue, if it failed.
    pub error: Option<String>,
    /// Highest CPU seen, when known.
    pub peak_cpu_percent: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(export)]
pub enum LoadEvent {
    #[serde(rename_all = "camelCase")]
    Snapshot { snapshot: Snapshot },
    #[serde(rename_all = "camelCase")]
    Finished { summary: Summary },
}

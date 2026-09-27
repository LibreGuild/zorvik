//! Load tests (`loadtests/<name>.yaml`): which saved requests to send, how
//! hard, for how long, and the thresholds that decide pass or fail.

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use zorvik_engine::HttpVersionPref;

fn yes() -> bool {
    true
}
fn is_true(v: &bool) -> bool {
    *v
}
fn one() -> u32 {
    1
}
fn is_one(v: &u32) -> bool {
    *v == 1
}
fn is_zero(v: &u64) -> bool {
    *v == 0
}
fn default_in_flight() -> u32 {
    1000
}
fn is_default_in_flight(v: &u32) -> bool {
    *v == 1000
}

/// How load is generated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum LoadModel {
    /// Closed model: each user sends, waits for the answer, thinks, repeats.
    /// Stage targets are numbers of users.
    #[default]
    VirtualUsers,
    /// Open model: requests start at a fixed rate however slow the server is.
    /// Stage targets are requests per second.
    ArrivalRate,
}

/// Ramp to `target` over `duration_secs` (linear), then the next stage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LoadStage {
    pub duration_secs: u32,
    pub target: u32,
}

/// A saved request to send, and how often relative to the others.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LoadTarget {
    /// Request path relative to `requests/`.
    pub request: String,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    #[ts(optional, as = "Option<u32>")]
    pub weight: u32,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    #[ts(optional, as = "Option<bool>")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ThresholdMetric {
    /// Latency percentiles and summary values, in milliseconds.
    P50,
    P90,
    P95,
    P99,
    P999,
    Avg,
    Max,
    /// Failed requests (network errors and HTTP status ≥ 400), in percent.
    ErrorRate,
    /// Completed requests per second over the whole run.
    Rps,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum ThresholdOp {
    #[serde(rename = "<")]
    Lt,
    #[serde(rename = "<=")]
    Lte,
    #[serde(rename = ">")]
    Gt,
    #[serde(rename = ">=")]
    Gte,
}

impl ThresholdOp {
    pub fn holds(self, actual: f64, limit: f64) -> bool {
        match self {
            ThresholdOp::Lt => actual < limit,
            ThresholdOp::Lte => actual <= limit,
            ThresholdOp::Gt => actual > limit,
            ThresholdOp::Gte => actual >= limit,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            ThresholdOp::Lt => "<",
            ThresholdOp::Lte => "<=",
            ThresholdOp::Gt => ">",
            ThresholdOp::Gte => ">=",
        }
    }
}

/// "p95 < 300" — fails the run when it does not hold at the end.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Threshold {
    pub metric: ThresholdMetric,
    pub op: ThresholdOp,
    pub value: f64,
    /// Only this target (request path); the whole test when missing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub target: Option<String>,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    #[ts(optional, as = "Option<bool>")]
    pub enabled: bool,
}

/// A saved load test.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LoadTest {
    pub name: String,
    /// Sort position in the sidebar.
    #[serde(default)]
    pub seq: u32,
    #[serde(default)]
    pub targets: Vec<LoadTarget>,
    #[serde(default)]
    pub model: LoadModel,
    #[serde(default)]
    pub stages: Vec<LoadStage>,
    /// Closed model: pause after each answer, per user.
    #[serde(default, skip_serializing_if = "is_zero")]
    #[ts(optional, as = "Option<u32>")]
    pub think_time_ms: u64,
    /// Open model: most requests in flight at once; more are counted as dropped.
    #[serde(default = "default_in_flight", skip_serializing_if = "is_default_in_flight")]
    #[ts(optional, as = "Option<u32>")]
    pub max_in_flight: u32,
    /// Reuse connections (HTTP/1.1 keep-alive, HTTP/2 multiplexing). Off: a new
    /// connection per request (measures connection setup, uses many ports).
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    #[ts(optional, as = "Option<bool>")]
    pub keep_alive: bool,
    /// Per-request timeout; the app setting when missing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub timeout_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub http_version: Option<HttpVersionPref>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, as = "Option<Vec<Threshold>>")]
    pub thresholds: Vec<Threshold>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub docs: String,
}

impl LoadTest {
    pub fn new(name: impl Into<String>, targets: Vec<String>) -> Self {
        Self {
            name: name.into(),
            seq: 0,
            targets: targets.into_iter().map(|request| LoadTarget { request, weight: 1, enabled: true }).collect(),
            model: LoadModel::VirtualUsers,
            stages: vec![
                LoadStage { duration_secs: 10, target: 10 },
                LoadStage { duration_secs: 40, target: 10 },
                LoadStage { duration_secs: 10, target: 0 },
            ],
            think_time_ms: 0,
            max_in_flight: default_in_flight(),
            keep_alive: true,
            timeout_ms: None,
            http_version: None,
            thresholds: vec![
                Threshold {
                    metric: ThresholdMetric::P95,
                    op: ThresholdOp::Lt,
                    value: 500.0,
                    target: None,
                    enabled: true,
                },
                Threshold {
                    metric: ThresholdMetric::ErrorRate,
                    op: ThresholdOp::Lt,
                    value: 1.0,
                    target: None,
                    enabled: true,
                },
            ],
            docs: String::new(),
        }
    }

    /// Total planned duration in seconds.
    pub fn duration_secs(&self) -> u64 {
        self.stages.iter().map(|s| u64::from(s.duration_secs)).sum()
    }
}

/// A saved load test in the sidebar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LoadTestNode {
    /// File stem (`loadtests/<id>.yaml`).
    pub id: String,
    pub name: String,
    pub model: LoadModel,
    pub targets: u32,
    #[ts(type = "number")]
    pub duration_secs: u64,
    pub seq: u32,
    /// Set when the file could not be parsed.
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaml_round_trip_and_defaults() {
        let t = LoadTest::new("Smoke", vec!["Users/List users.yaml".into()]);
        let yaml = serde_yaml_ng::to_string(&t).unwrap();
        assert!(yaml.contains("metric: p95") && yaml.contains("op: <"), "{yaml}");
        assert!(!yaml.contains("keepAlive") && !yaml.contains("weight"), "{yaml}");
        assert_eq!(serde_yaml_ng::from_str::<LoadTest>(&yaml).unwrap(), t);
        assert_eq!(t.duration_secs(), 60);

        let minimal: LoadTest = serde_yaml_ng::from_str("name: X\ntargets: [{request: a.yaml}]\n").unwrap();
        assert!(minimal.keep_alive && minimal.max_in_flight == 1000 && minimal.targets[0].weight == 1);
        assert!(ThresholdOp::Lte.holds(1.0, 1.0) && !ThresholdOp::Lt.holds(1.0, 1.0));
    }
}

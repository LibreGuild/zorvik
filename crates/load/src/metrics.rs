//! Aggregation: every finished request becomes a [`Sample`]; the [`Recorder`]
//! keeps HdrHistograms (µs, 1 µs – 1 h, 3 significant digits) per target and
//! in total, counters, and one-second buckets for the charts. Memory is fixed
//! except for the chart points (one per second: 86,400 for a day). The phases
//! (connect, first byte, transfer, server-reported) have histograms too, made
//! when their first value arrives.
//!
//! Latency includes every request that got an answer (any status), plus
//! timed-out requests and requests cut off by a stop, as the time waited (a
//! lower bound; leaving them out would hide exactly the slow ones). Requests
//! that could not connect count as errors but have no latency.

use std::collections::{HashMap, VecDeque};
use std::time::Duration;

use hdrhistogram::Histogram;
use zorvik_engine::ErrorKind;
use zorvik_formats::{Threshold, ThresholdMetric};

use crate::report::{
    LatencySummary, MetricsSummary, PhaseSummary, RunPhase, Snapshot, Summary, TargetSummary, ThresholdResult,
    TimePoint, TimingSummary,
};
use crate::schedule::Profile;

/// Highest latency the histograms track (1 hour); longer ones count as 1 hour.
const MAX_LATENCY_US: u64 = 3_600_000_000;

/// A second's chart point is written this long after the second ended, so
/// samples still on their way are counted in the right second.
const LATE_US: u64 = 500_000;

/// Network error kinds, in [`kind_index`] order.
const KINDS: [ErrorKind; 11] = [
    ErrorKind::Timeout,
    ErrorKind::Connect,
    ErrorKind::Dns,
    ErrorKind::Tls,
    ErrorKind::Proxy,
    ErrorKind::Protocol,
    ErrorKind::Io,
    ErrorKind::Cancelled,
    ErrorKind::InvalidRequest,
    ErrorKind::TooManyRedirects,
    ErrorKind::NotAllowed,
];

fn kind_index(kind: ErrorKind) -> usize {
    KINDS.iter().position(|k| *k == kind).unwrap_or(0)
}

/// Same names as `ErrorKind` has in JSON.
pub(crate) fn kind_name(kind: ErrorKind) -> &'static str {
    match kind {
        ErrorKind::InvalidRequest => "invalidRequest",
        ErrorKind::Dns => "dns",
        ErrorKind::Connect => "connect",
        ErrorKind::Tls => "tls",
        ErrorKind::Proxy => "proxy",
        ErrorKind::Timeout => "timeout",
        ErrorKind::Protocol => "protocol",
        ErrorKind::Io => "io",
        ErrorKind::Cancelled => "cancelled",
        ErrorKind::TooManyRedirects => "tooManyRedirects",
        ErrorKind::NotAllowed => "notAllowed",
    }
}

/// One finished (or dropped) iteration.
#[derive(Debug, Clone)]
pub(crate) struct Sample {
    pub target: usize,
    /// When it finished, in µs since the run started.
    pub at_us: u64,
    pub outcome: Outcome,
}

#[derive(Debug, Clone)]
pub(crate) enum Outcome {
    Done {
        /// `None` for connection failures (no meaningful latency).
        latency_us: Option<u64>,
        status: Option<u16>,
        error: Option<ErrorKind>,
        bytes_in: u64,
        bytes_out: u64,
        new_connection: bool,
        phases: Phases,
        /// Captures that found nothing in this response.
        capture_misses: u32,
    },
    /// Open model: not started because `maxInFlight` requests were running.
    Dropped,
}

/// Parts of one request's time (µs); `None` when not measured.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Phases {
    /// Only when it opened a new connection.
    pub connect_us: Option<u64>,
    pub ttfb_us: Option<u64>,
    pub transfer_us: Option<u64>,
    /// From a `Server-Timing` header.
    pub server_us: Option<u64>,
}

fn histogram() -> Histogram<u64> {
    Histogram::new_with_bounds(1, MAX_LATENCY_US, 3).expect("valid histogram bounds")
}

/// One phase's numbers; the histogram is made when the first value arrives.
#[derive(Default)]
struct Phase {
    hist: Option<Histogram<u64>>,
    sum_us: u128,
    max_us: u64,
}

impl Phase {
    fn record(&mut self, us: Option<u64>) {
        let Some(us) = us else { return };
        let us = us.min(MAX_LATENCY_US);
        self.hist.get_or_insert_with(histogram).saturating_record(us);
        self.sum_us += u128::from(us);
        self.max_us = self.max_us.max(us);
    }

    fn summary(&self) -> PhaseSummary {
        let Some(hist) = self.hist.as_ref().filter(|h| !h.is_empty()) else { return PhaseSummary::default() };
        let n = hist.len();
        let q: Vec<f64> = hist.value_at_quantiles([0.5, 0.95, 0.99]).map(|v| ms(v.min(self.max_us))).collect();
        PhaseSummary {
            count: n,
            avg: round3(self.sum_us as f64 / n as f64 / 1000.0),
            p50: q[0],
            p95: q[1],
            p99: q[2],
            max: ms(self.max_us),
        }
    }
}

struct Stats {
    latency: Histogram<u64>,
    connect: Phase,
    ttfb: Phase,
    transfer: Phase,
    server: Phase,
    capture_misses: u64,
    sum_us: u128,
    min_us: u64,
    max_us: u64,
    requests: u64,
    errors: u64,
    bytes_in: u64,
    bytes_out: u64,
    dropped: u64,
    connections: u64,
    statuses: HashMap<u16, u64>,
    error_kinds: [u64; KINDS.len()],
}

impl Stats {
    fn new() -> Self {
        Self {
            latency: histogram(),
            connect: Phase::default(),
            ttfb: Phase::default(),
            transfer: Phase::default(),
            server: Phase::default(),
            capture_misses: 0,
            sum_us: 0,
            min_us: u64::MAX,
            max_us: 0,
            requests: 0,
            errors: 0,
            bytes_in: 0,
            bytes_out: 0,
            dropped: 0,
            connections: 0,
            statuses: HashMap::new(),
            error_kinds: [0; KINDS.len()],
        }
    }

    fn record(&mut self, outcome: &Outcome) {
        let Outcome::Done { latency_us, status, error, bytes_in, bytes_out, new_connection, phases, capture_misses } =
            outcome
        else {
            self.dropped += 1;
            return;
        };
        self.requests += 1;
        if error.is_some() || status.is_some_and(|s| s >= 400) {
            self.errors += 1;
        }
        if let Some(kind) = error {
            self.error_kinds[kind_index(*kind)] += 1;
        }
        if let Some(status) = status {
            *self.statuses.entry(*status).or_default() += 1;
        }
        if let Some(us) = latency_us {
            let us = (*us).min(MAX_LATENCY_US);
            self.latency.saturating_record(us);
            self.sum_us += u128::from(us);
            self.min_us = self.min_us.min(us);
            self.max_us = self.max_us.max(us);
        }
        self.bytes_in += bytes_in;
        self.bytes_out += bytes_out;
        self.connections += u64::from(*new_connection);
        self.connect.record(phases.connect_us);
        self.ttfb.record(phases.ttfb_us);
        self.transfer.record(phases.transfer_us);
        self.server.record(phases.server_us);
        self.capture_misses += u64::from(*capture_misses);
    }

    fn summary(&self, elapsed_secs: f64) -> MetricsSummary {
        let mut status_codes: Vec<(u16, u64)> = self.statuses.iter().map(|(s, n)| (*s, *n)).collect();
        status_codes.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let mut error_kinds: Vec<(String, u64)> = KINDS
            .iter()
            .zip(self.error_kinds)
            .filter(|(_, n)| *n > 0)
            .map(|(k, n)| (kind_name(*k).to_string(), n))
            .collect();
        error_kinds.sort_by_key(|k| std::cmp::Reverse(k.1));
        MetricsSummary {
            requests: self.requests,
            errors: self.errors,
            error_rate: if self.requests == 0 { 0.0 } else { self.errors as f64 * 100.0 / self.requests as f64 },
            rps: if elapsed_secs > 0.0 { self.requests as f64 / elapsed_secs } else { 0.0 },
            bytes_in: self.bytes_in,
            bytes_out: self.bytes_out,
            latency: self.latency_summary(),
            status_codes,
            error_kinds,
            dropped: self.dropped,
            connections: self.connections,
            timing: TimingSummary {
                connect: self.connect.summary(),
                ttfb: self.ttfb.summary(),
                transfer: self.transfer.summary(),
                server: self.server.summary(),
            },
            capture_misses: self.capture_misses,
        }
    }

    fn latency_summary(&self) -> LatencySummary {
        let n = self.latency.len();
        if n == 0 {
            return LatencySummary::default();
        }
        let q: Vec<u64> = self
            .latency
            .value_at_quantiles([0.5, 0.9, 0.95, 0.99, 0.999])
            // A bucket's highest value can lie above the largest sample.
            .map(|v| v.clamp(self.min_us, self.max_us))
            .collect();
        let at = |i: usize| ms(q.get(i).copied().unwrap_or(self.max_us));
        LatencySummary {
            min: ms(self.min_us),
            avg: round3(self.sum_us as f64 / n as f64 / 1000.0),
            p50: at(0),
            p90: at(1),
            p95: at(2),
            p99: at(3),
            p999: at(4),
            max: ms(self.max_us),
        }
    }
}

fn ms(us: u64) -> f64 {
    round3(us as f64 / 1000.0)
}

fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

struct Second {
    requests: u64,
    errors: u32,
    latency: Histogram<u64>,
    /// Users or requests in flight when the second ended.
    active: Option<u32>,
}

/// All numbers of a run. Shared behind a mutex: the aggregator task records
/// batches from the channel; a sender records directly when the channel is full.
pub(crate) struct Recorder {
    names: Vec<(String, String)>,
    total: Stats,
    targets: Vec<Stats>,
    /// Open seconds, starting at `first_open`.
    seconds: VecDeque<Second>,
    first_open: u32,
    /// Chart points of closed seconds; `points[sent..]` go in the next snapshot.
    points: Vec<TimePoint>,
    sent: usize,
    last_active: u32,
    spare: Vec<Histogram<u64>>,
    thresholds: Vec<Threshold>,
    profile: Profile,
    /// Open model: chart the average target of each second, like the request
    /// count it is compared with; users are charted at the end of the second.
    rate: bool,
}

impl Recorder {
    /// `names`: `(name, request path)` per target, in plan order.
    pub(crate) fn new(names: Vec<(String, String)>, thresholds: Vec<Threshold>, profile: Profile, rate: bool) -> Self {
        let targets = names.iter().map(|_| Stats::new()).collect();
        Self {
            names,
            total: Stats::new(),
            targets,
            seconds: VecDeque::new(),
            first_open: 0,
            points: Vec::new(),
            sent: 0,
            last_active: 0,
            spare: Vec::new(),
            thresholds,
            profile,
            rate,
        }
    }

    pub(crate) fn record(&mut self, sample: &Sample) {
        self.total.record(&sample.outcome);
        if let Some(stats) = self.targets.get_mut(sample.target) {
            stats.record(&sample.outcome);
        }
        let Outcome::Done { latency_us, status, error, .. } = &sample.outcome else { return };
        // A sample for a second already charted (very late) goes into the oldest open one.
        let second = ((sample.at_us / 1_000_000) as u32).max(self.first_open);
        let bucket = self.second(second);
        bucket.requests += 1;
        if error.is_some() || status.is_some_and(|s| s >= 400) {
            bucket.errors += 1;
        }
        if let Some(us) = latency_us {
            bucket.latency.saturating_record((*us).min(MAX_LATENCY_US));
        }
    }

    fn second(&mut self, second: u32) -> &mut Second {
        while self.first_open as usize + self.seconds.len() <= second as usize {
            let mut latency = self.spare.pop().unwrap_or_else(histogram);
            latency.reset();
            self.seconds.push_back(Second { requests: 0, errors: 0, latency, active: None });
        }
        &mut self.seconds[(second - self.first_open) as usize]
    }

    /// Note how many users/requests were active for the seconds that ended.
    pub(crate) fn note_active(&mut self, elapsed: Duration, active: u32) {
        let ended = (elapsed.as_secs() as u32).max(self.first_open);
        if ended > self.first_open {
            self.second(ended - 1);
        }
        for s in self.seconds.iter_mut().take((ended - self.first_open) as usize) {
            s.active.get_or_insert(active);
        }
        self.last_active = active;
    }

    /// Turn seconds before `until` into chart points.
    fn close_seconds(&mut self, until: u32) {
        while self.first_open < until {
            let second = self.first_open;
            let bucket = match self.seconds.pop_front() {
                Some(b) => b,
                None => {
                    Second { requests: 0, errors: 0, latency: self.spare.pop().unwrap_or_else(histogram), active: None }
                }
            };
            let mut q = [0.0; 3];
            if !bucket.latency.is_empty() {
                for (slot, v) in q.iter_mut().zip(bucket.latency.value_at_quantiles([0.5, 0.95, 0.99])) {
                    *slot = ms(v);
                }
            }
            self.points.push(TimePoint {
                second,
                rps: bucket.requests as f64,
                errors: bucket.errors,
                p50: q[0],
                p95: q[1],
                p99: q[2],
                active: bucket.active.unwrap_or(self.last_active),
                target: if self.rate {
                    self.profile.mean(f64::from(second), f64::from(second) + 1.0)
                } else {
                    self.profile.target_at(f64::from(second) + 1.0)
                },
            });
            if self.spare.len() < 4 {
                self.spare.push(bucket.latency);
            }
            self.first_open += 1;
        }
    }

    fn metrics(&self, elapsed: Duration) -> (MetricsSummary, Vec<TargetSummary>) {
        let secs = elapsed.as_secs_f64();
        let targets = self
            .names
            .iter()
            .zip(&self.targets)
            .map(|((name, request), stats)| TargetSummary {
                name: name.clone(),
                request: request.clone(),
                metrics: stats.summary(secs),
            })
            .collect();
        (self.total.summary(secs), targets)
    }

    pub(crate) fn snapshot(
        &mut self,
        phase: RunPhase,
        elapsed: Duration,
        active: u32,
        cpu_percent: Option<f32>,
    ) -> Snapshot {
        self.note_active(elapsed, active);
        let elapsed_us = elapsed.as_micros() as u64;
        self.close_seconds((elapsed_us.saturating_sub(LATE_US) / 1_000_000) as u32);
        let points = self.points[self.sent..].to_vec();
        self.sent = self.points.len();
        let (totals, targets) = self.metrics(elapsed);
        let thresholds = self.evaluate(&totals, &targets);
        Snapshot {
            phase,
            elapsed_ms: elapsed.as_millis() as u64,
            planned_ms: self.profile.duration().as_millis() as u64,
            active,
            target: self.profile.target_at(elapsed.as_secs_f64()),
            totals,
            targets,
            points,
            thresholds,
            cpu_percent,
        }
    }

    /// The final numbers. Every whole second is charted; the last, partial
    /// second (a stop, or answers arriving just after the planned end) is in
    /// the totals only, so the chart doesn't show a false drop.
    pub(crate) fn summary(&mut self, run: SummaryInfo) -> Summary {
        self.note_active(run.elapsed, self.last_active);
        self.close_seconds(run.elapsed.as_secs() as u32);
        let (totals, targets) = self.metrics(run.elapsed);
        let thresholds = self.evaluate(&totals, &targets);
        let passed = run.error.is_none() && thresholds.iter().all(|t| t.passed);
        Summary {
            started_at: run.started_at,
            duration_ms: run.elapsed.as_millis() as u64,
            totals,
            targets,
            points: self.points.clone(),
            thresholds,
            passed,
            stopped_early: run.stopped_early,
            error: run.error,
            peak_cpu_percent: run.peak_cpu_percent,
        }
    }

    fn evaluate(&self, totals: &MetricsSummary, targets: &[TargetSummary]) -> Vec<ThresholdResult> {
        self.thresholds
            .iter()
            .filter(|t| t.enabled)
            .map(|t| {
                let scope = match &t.target {
                    None => Some((totals, &self.total, None)),
                    Some(path) => targets
                        .iter()
                        .zip(&self.targets)
                        .find(|(summary, _)| &summary.request == path)
                        .map(|(summary, stats)| (&summary.metrics, stats, Some(summary.name.as_str()))),
                };
                let actual = scope.and_then(|(m, stats, _)| actual(t.metric, m, stats));
                let name = match (&t.target, scope) {
                    (Some(_), Some((_, _, Some(name)))) => Some(name),
                    (Some(path), _) => Some(path.as_str()),
                    (None, _) => None,
                };
                ThresholdResult {
                    label: threshold_label(t, name),
                    metric: t.metric,
                    op: t.op,
                    value: t.value,
                    target: t.target.clone(),
                    actual,
                    passed: actual.is_some_and(|a| t.op.holds(a, t.value)),
                }
            })
            .collect()
    }
}

pub(crate) struct SummaryInfo {
    pub started_at: f64,
    pub elapsed: Duration,
    pub stopped_early: bool,
    pub error: Option<String>,
    pub peak_cpu_percent: Option<f32>,
}

/// The measured value; `None` until there is data for it.
fn actual(metric: ThresholdMetric, m: &MetricsSummary, stats: &Stats) -> Option<f64> {
    if m.requests == 0 {
        return None;
    }
    let l = &m.latency;
    let latency = |v: f64| (!stats.latency.is_empty()).then_some(v);
    match metric {
        ThresholdMetric::P50 => latency(l.p50),
        ThresholdMetric::P90 => latency(l.p90),
        ThresholdMetric::P95 => latency(l.p95),
        ThresholdMetric::P99 => latency(l.p99),
        ThresholdMetric::P999 => latency(l.p999),
        ThresholdMetric::Avg => latency(l.avg),
        ThresholdMetric::Max => latency(l.max),
        ThresholdMetric::ErrorRate => Some(m.error_rate),
        ThresholdMetric::Rps => Some(m.rps),
    }
}

/// `p95 < 300 ms`, `errorRate < 1 % · Get users`, `rps >= 100`.
pub(crate) fn threshold_label(t: &Threshold, target: Option<&str>) -> String {
    let (metric, unit) = match t.metric {
        ThresholdMetric::P50 => ("p50", " ms"),
        ThresholdMetric::P90 => ("p90", " ms"),
        ThresholdMetric::P95 => ("p95", " ms"),
        ThresholdMetric::P99 => ("p99", " ms"),
        ThresholdMetric::P999 => ("p99.9", " ms"),
        ThresholdMetric::Avg => ("avg", " ms"),
        ThresholdMetric::Max => ("max", " ms"),
        ThresholdMetric::ErrorRate => ("errorRate", " %"),
        ThresholdMetric::Rps => ("rps", ""),
    };
    let mut label = format!("{metric} {} {}{unit}", t.op.as_str(), number(t.value));
    if let Some(target) = target {
        label.push_str(" · ");
        label.push_str(target);
    }
    label
}

/// A number without needless decimals: `300`, `0.5`, `1.25`.
pub(crate) fn number(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        return format!("{v:.0}");
    }
    let s = format!("{v:.3}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use zorvik_formats::{LoadStage, ThresholdOp};

    fn done(target: usize, at_ms: u64, latency_ms: u64, status: u16) -> Sample {
        Sample {
            target,
            at_us: at_ms * 1000,
            outcome: Outcome::Done {
                latency_us: Some(latency_ms * 1000),
                status: Some(status),
                error: None,
                bytes_in: 100,
                bytes_out: 10,
                new_connection: false,
                phases: Phases {
                    connect_us: None,
                    ttfb_us: Some(latency_ms * 800),
                    transfer_us: Some(latency_ms * 200),
                    server_us: None,
                },
                capture_misses: 0,
            },
        }
    }

    fn threshold(metric: ThresholdMetric, op: ThresholdOp, value: f64, target: Option<&str>) -> Threshold {
        Threshold { metric, op, value, target: target.map(str::to_string), enabled: true }
    }

    #[test]
    fn percentiles_seconds_and_thresholds() {
        let profile =
            Profile::new(&[LoadStage { duration_secs: 0, target: 5 }, LoadStage { duration_secs: 3, target: 5 }]);
        let names = vec![("List".to_string(), "list.yaml".to_string()), ("Fail".to_string(), "fail.yaml".to_string())];
        let thresholds = vec![
            threshold(ThresholdMetric::P95, ThresholdOp::Lt, 300.0, None),
            threshold(ThresholdMetric::ErrorRate, ThresholdOp::Lt, 1.0, Some("fail.yaml")),
            threshold(ThresholdMetric::Rps, ThresholdOp::Gte, 1.0, Some("missing.yaml")),
            Threshold { enabled: false, ..threshold(ThresholdMetric::Max, ThresholdOp::Lt, 1.0, None) },
        ];
        let mut rec = Recorder::new(names, thresholds, profile, false);
        for i in 0..100 {
            rec.record(&done(0, 100 + i * 10, i + 1, 200));
        }
        rec.record(&done(1, 1500, 5, 500));
        rec.record(&Sample {
            target: 0,
            at_us: 1_600_000,
            outcome: Outcome::Done {
                latency_us: None,
                status: None,
                error: Some(ErrorKind::Connect),
                bytes_in: 0,
                bytes_out: 0,
                new_connection: false,
                phases: Phases::default(),
                capture_misses: 0,
            },
        });
        rec.record(&Sample { target: 1, at_us: 1_700_000, outcome: Outcome::Dropped });

        let snap = rec.snapshot(RunPhase::Running, Duration::from_millis(1600), 5, None);
        // Second 0 is charted 0.5 s after it ended; second 1 not yet.
        assert_eq!(snap.points.len(), 1);
        assert_eq!(snap.points[0].rps, 90.0);
        assert_eq!(snap.points[0].active, 5);
        assert_eq!(snap.totals.requests, 102);
        assert_eq!(snap.totals.errors, 2);
        assert_eq!(snap.totals.dropped, 1);
        // 1..=100 ms plus one 5 ms: the 51st of 101 values is 50 ms (3 significant digits).
        assert!((snap.totals.latency.p50 - 50.0).abs() < 0.1, "{}", snap.totals.latency.p50);
        assert_eq!((snap.totals.latency.min, snap.totals.latency.max), (1.0, 100.0));
        assert_eq!(snap.totals.status_codes, vec![(200, 100), (500, 1)]);
        assert_eq!(snap.totals.error_kinds, vec![("connect".to_string(), 1)]);
        assert_eq!(snap.targets[1].metrics.error_rate, 100.0);
        // Phases: first byte is 80 % of each latency here; no connects, no Server-Timing.
        let timing = &snap.totals.timing;
        assert_eq!(
            (timing.ttfb.count, timing.transfer.count, timing.connect.count, timing.server.count),
            (101, 101, 0, 0)
        );
        assert!((timing.ttfb.p50 - 40.0).abs() < 0.1 && timing.ttfb.max == 80.0, "{timing:?}");
        assert_eq!(timing.connect, PhaseSummary::default());
        assert_eq!(snap.thresholds.len(), 3);
        assert_eq!(snap.thresholds[0].label, "p95 < 300 ms");
        assert!(snap.thresholds[0].passed);
        assert_eq!(snap.thresholds[1].label, "errorRate < 1 % · Fail");
        assert!(!snap.thresholds[1].passed && snap.thresholds[1].actual == Some(100.0));
        assert_eq!(snap.thresholds[2].label, "rps >= 1 · missing.yaml");
        assert!(snap.thresholds[2].actual.is_none() && !snap.thresholds[2].passed);

        // Finished just after the end: counted, not charted.
        rec.record(&done(0, 2050, 1, 200));
        let mut fresh = done(1, 2060, 3, 200);
        if let Outcome::Done { phases, capture_misses, new_connection, .. } = &mut fresh.outcome {
            phases.connect_us = Some(1500);
            phases.server_us = Some(700);
            *capture_misses = 2;
            *new_connection = true;
        }
        rec.record(&fresh);
        let summary = rec.summary(SummaryInfo {
            started_at: 0.0,
            elapsed: Duration::from_millis(2100),
            stopped_early: false,
            error: None,
            peak_cpu_percent: None,
        });
        // Whole seconds only: 0 and 1.
        assert_eq!(summary.points.iter().map(|p| p.second).collect::<Vec<_>>(), [0, 1]);
        assert_eq!(summary.points[1].rps, 12.0);
        assert_eq!(summary.totals.requests, 104);
        assert_eq!(summary.points[1].errors, 2);
        let timing = &summary.totals.timing;
        assert_eq!(
            (timing.connect.count, timing.connect.p95, timing.server.count, timing.server.max),
            (1, 1.5, 1, 0.7)
        );
        assert_eq!((summary.totals.capture_misses, summary.targets[1].metrics.capture_misses), (2, 2));
        assert_eq!(summary.targets[0].metrics.capture_misses, 0);
        assert!(!summary.passed);
        assert_eq!(number(0.5), "0.5");
        assert_eq!(number(300.0), "300");
    }
}

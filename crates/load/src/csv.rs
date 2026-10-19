//! CSV exports of a run for spreadsheets and scripts: one row per second (the
//! charts' data) and one row per request after a row for all requests. Text
//! from workspace files (request names and paths) is quoted when needed, and a
//! cell a spreadsheet would run as a formula gets a leading `'`.

use std::fmt::Write as _;

use crate::metrics::number;
use crate::{MetricsSummary, PhaseSummary, Summary};

/// `second,requests,errors,p50_ms,p95_ms,p99_ms,active,target,warm_up`: one
/// row per whole second of the run (`second` 0 is the first second).
pub fn seconds_csv(summary: &Summary) -> String {
    let mut out = String::with_capacity(64 + summary.points.len() * 48);
    out.push_str("second,requests,errors,p50_ms,p95_ms,p99_ms,active,target,warm_up\n");
    for p in &summary.points {
        let warm_up = u64::from(p.second) * 1000 < summary.warm_up_ms;
        let _ = writeln!(
            out,
            "{},{},{},{},{},{},{},{},{warm_up}",
            p.second,
            number(p.rps),
            p.errors,
            number(p.p50),
            number(p.p95),
            number(p.p99),
            p.active,
            number(p.target),
        );
    }
    out
}

/// One row for all requests, then one per request: counts, throughput,
/// latency percentiles (ms), first byte and connect p95, data, status codes
/// (`200:950 503:12`) and network errors (`timeout:3`).
pub fn requests_csv(summary: &Summary) -> String {
    let mut out = String::new();
    out.push_str(
        "request,path,requests,errors,error_rate_pct,dropped,rps,min_ms,avg_ms,p50_ms,p90_ms,p95_ms,p99_ms,p999_ms,max_ms,\
         first_byte_p95_ms,connect_p95_ms,bytes_in,bytes_out,connections,capture_misses,status_codes,network_errors\n",
    );
    row(&mut out, "All requests", "", &summary.totals);
    for target in &summary.targets {
        row(&mut out, &target.name, &target.request, &target.metrics);
    }
    out
}

fn row(out: &mut String, name: &str, path: &str, m: &MetricsSummary) {
    let l = &m.latency;
    let has_latency = m.requests > 0 && l.max > 0.0;
    let lat = |v: f64| if has_latency { number(v) } else { String::new() };
    let phase = |p: &PhaseSummary| if p.count > 0 { number(p.p95) } else { String::new() };
    let codes: Vec<String> = m.status_codes.iter().map(|(code, n)| format!("{code}:{n}")).collect();
    let kinds: Vec<String> = m.error_kinds.iter().map(|(kind, n)| format!("{kind}:{n}")).collect();
    let _ = writeln!(
        out,
        "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
        cell(name),
        cell(path),
        m.requests,
        m.errors,
        number(m.error_rate),
        m.dropped,
        number(m.rps),
        lat(l.min),
        lat(l.avg),
        lat(l.p50),
        lat(l.p90),
        lat(l.p95),
        lat(l.p99),
        lat(l.p999),
        lat(l.max),
        phase(&m.timing.ttfb),
        phase(&m.timing.connect),
        m.bytes_in,
        m.bytes_out,
        m.connections,
        m.capture_misses,
        cell(&codes.join(" ")),
        cell(&kinds.join(" ")),
    );
}

/// A text cell: quoted when it holds a comma, quote or line break; a leading
/// `=`, `+`, `-`, `@`, tab or carriage return is defused with `'`.
fn cell(text: &str) -> String {
    let text = if text.starts_with(['=', '+', '-', '@', '\t', '\r']) { format!("'{text}") } else { text.to_string() };
    if text.contains([',', '"', '\n', '\r']) { format!("\"{}\"", text.replace('"', "\"\"")) } else { text }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LatencySummary, TargetSummary, TimePoint, TimingSummary};

    fn run() -> Summary {
        let latency =
            LatencySummary { min: 1.0, avg: 4.5, p50: 4.0, p90: 8.0, p95: 9.5, p99: 12.25, p999: 20.0, max: 31.0 };
        let metrics = MetricsSummary {
            requests: 600,
            errors: 3,
            error_rate: 0.5,
            rps: 199.9876,
            latency,
            status_codes: vec![(200, 597), (503, 3)],
            error_kinds: vec![("timeout".into(), 1)],
            timing: TimingSummary {
                ttfb: PhaseSummary { count: 600, p95: 7.5, ..Default::default() },
                ..Default::default()
            },
            bytes_in: 12_000,
            connections: 4,
            ..Default::default()
        };
        Summary {
            started_at: 0.0,
            duration_ms: 3000,
            totals: metrics.clone(),
            targets: vec![
                TargetSummary { name: "=HYPERLINK(\"x\"), \"quoted\"".into(), request: "a,b.yaml".into(), metrics },
                TargetSummary { name: "Idle".into(), request: "idle.yaml".into(), metrics: MetricsSummary::default() },
            ],
            points: (0..3)
                .map(|s| TimePoint {
                    second: s,
                    rps: 200.0,
                    errors: s,
                    p50: 4.256,
                    p95: 12.0,
                    p99: 120.0,
                    active: 10,
                    target: 10.5,
                })
                .collect(),
            thresholds: Vec::new(),
            passed: true,
            stopped_early: false,
            error: None,
            peak_cpu_percent: None,
            warm_up_ms: 1000,
            stopped_by_threshold: None,
        }
    }

    #[test]
    fn one_row_per_second() {
        let csv = seconds_csv(&run());
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines[0], "second,requests,errors,p50_ms,p95_ms,p99_ms,active,target,warm_up");
        assert_eq!(lines[1], "0,200,0,4.256,12,120,10,10.5,true");
        assert_eq!(lines[2], "1,200,1,4.256,12,120,10,10.5,false");
        assert_eq!(lines.len(), 4);
    }

    #[test]
    fn one_row_per_request_with_text_defused() {
        let csv = requests_csv(&run());
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines.len(), 4, "{csv}");
        let columns = lines[0].split(',').count();
        assert_eq!(columns, 23);
        assert_eq!(
            lines[1],
            "All requests,,600,3,0.5,0,199.988,1,4.5,4,8,9.5,12.25,20,31,7.5,,12000,0,4,0,200:597 503:3,timeout:1"
        );
        // A formula is defused and quoted with its own quotes doubled; the path's comma is quoted.
        assert!(lines[2].starts_with("\"'=HYPERLINK(\"\"x\"\"), \"\"quoted\"\"\",\"a,b.yaml\",600,"), "{}", lines[2]);
        // No requests: no latency numbers.
        assert_eq!(lines[3], "Idle,idle.yaml,0,0,0,0,0,,,,,,,,,,,0,0,0,0,,");
        assert_eq!(cell("-1"), "'-1");
        assert_eq!(cell("line\nbreak"), "\"line\nbreak\"");
        assert_eq!(cell("plain"), "plain");
    }
}

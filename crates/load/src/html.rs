//! The HTML report: one self-contained page (inline CSS and SVG, no scripts,
//! no external resources) with the verdict, key numbers, thresholds, charts
//! over time, and per-target, status and error tables. Every text that comes
//! from files (title, request names) is escaped. Light and dark follow the
//! viewer's system setting.

use std::fmt::Write as _;

use crate::metrics::number;
use crate::{LatencySummary, MetricsSummary, PhaseSummary, Summary, TimePoint};

/// Most columns a chart draws; longer runs are averaged into buckets
/// (percentile lines keep each bucket's worst value).
const MAX_COLUMNS: usize = 600;

const STYLE: &str = r#"<style>
:root{color-scheme:light;--page:#f9f9f7;--surface:#fcfcfb;--ink:#0b0b0b;--ink-2:#52514e;--muted:#898781;--grid:#e1e0d9;--axis:#c3c2b7;--ring:rgba(11,11,11,.10);--hover:rgba(11,11,11,.05);--s1:#2a78d6;--s2:#eb6834;--s3:#1baf7a;--good:#006300;--good-bg:rgba(12,163,12,.10);--bad:#d03b3b;--bad-bg:rgba(208,59,59,.10)}
@media (prefers-color-scheme:dark){:root{color-scheme:dark;--page:#0d0d0d;--surface:#1a1a19;--ink:#fff;--ink-2:#c3c2b7;--muted:#898781;--grid:#2c2c2a;--axis:#383835;--ring:rgba(255,255,255,.10);--hover:rgba(255,255,255,.06);--s1:#3987e5;--s2:#d95926;--s3:#199e70;--good:#0ca30c;--good-bg:rgba(12,163,12,.16);--bad:#e66767;--bad-bg:rgba(230,103,103,.16)}}
*{box-sizing:border-box}
body{margin:0;background:var(--page);color:var(--ink);font:14px/1.5 system-ui,-apple-system,"Segoe UI",sans-serif}
main{max-width:1080px;margin:0 auto;padding:32px 16px 48px}
header{display:flex;flex-wrap:wrap;align-items:flex-start;justify-content:space-between;gap:12px;margin-bottom:24px}
h1{font-size:24px;line-height:1.25;margin:0 0 4px;overflow-wrap:anywhere}
h2{font-size:16px;margin:0 0 12px}
.meta{color:var(--ink-2)}
.verdict{display:inline-flex;align-items:center;gap:6px;padding:6px 12px;border-radius:999px;font-weight:600;white-space:nowrap}
.verdict.pass{color:var(--good);background:var(--good-bg)}.verdict.fail{color:var(--bad);background:var(--bad-bg)}
.error{color:var(--bad);background:var(--bad-bg);border-radius:8px;padding:10px 14px;margin:0 0 24px;overflow-wrap:anywhere}
section{background:var(--surface);border:1px solid var(--ring);border-radius:12px;padding:20px;margin-bottom:16px}
.tiles{display:grid;grid-template-columns:repeat(auto-fill,minmax(150px,1fr));gap:12px;background:none;border:0;padding:0}
.tile{background:var(--surface);border:1px solid var(--ring);border-radius:12px;padding:14px 16px}
.tile .label{color:var(--ink-2);font-size:13px}.tile .value{font-size:22px;font-weight:600;margin-top:2px}.tile .sub{color:var(--muted);font-size:12px}
.chart{overflow-x:auto}.chart svg{display:block;width:100%;height:auto;min-width:560px}
.chart+.chart{margin-top:24px}
.legend{display:flex;flex-wrap:wrap;gap:16px;color:var(--ink-2);font-size:13px;margin:-4px 0 8px}
.key{display:inline-block;width:14px;height:2px;border-radius:1px;vertical-align:middle;margin-right:6px}
svg text{fill:var(--muted);font-size:11px;font-variant-numeric:tabular-nums}
svg .grid{stroke:var(--grid);stroke-width:1}svg .base{stroke:var(--axis);stroke-width:1}
svg .line{fill:none;stroke-width:2;stroke-linejoin:round;stroke-linecap:round}
svg .hit{fill:transparent}svg .hit:hover{fill:var(--hover)}
.table{overflow-x:auto}
table{border-collapse:collapse;width:100%;font-variant-numeric:tabular-nums}
th,td{text-align:right;padding:7px 10px;border-bottom:1px solid var(--grid);white-space:nowrap}
th{color:var(--ink-2);font-weight:600;font-size:13px}
th:first-child,td:first-child{text-align:left;white-space:normal;overflow-wrap:anywhere}
tr:last-child td{border-bottom:0}
.muted{color:var(--muted)}.pass{color:var(--good)}.fail{color:var(--bad)}
.split{display:grid;grid-template-columns:repeat(auto-fit,minmax(280px,1fr));gap:16px}
.split section{margin:0}
footer{color:var(--muted);font-size:12px;text-align:center;margin-top:24px}
</style>
"#;

/// Escape text for HTML content and attribute values.
fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

pub(crate) fn report(title: &str, s: &Summary) -> String {
    let mut out = String::with_capacity(64 * 1024);
    let title = esc(title);
    out.push_str("<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
    out.push_str("<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\n");
    let _ = writeln!(out, "<title>{title} · Load test report</title>");
    out.push_str(STYLE);
    out.push_str("</head>\n<body>\n<main>\n");

    // Verdict.
    let failed_thresholds = s.thresholds.iter().filter(|t| !t.passed).count();
    let verdict = match (&s.error, s.passed) {
        (Some(_), _) => "✗ Failed (error)".to_string(),
        (None, true) if s.thresholds.is_empty() => "✓ Passed (no thresholds)".to_string(),
        (None, true) => "✓ Passed".to_string(),
        (None, false) => format!("✗ Failed ({failed_thresholds} of {} thresholds)", s.thresholds.len()),
    };
    let mut meta = format!("Started {} · ran {}", utc(s.started_at), duration(s.duration_ms));
    if s.stopped_early {
        meta.push_str(" · stopped early");
    }
    let _ = writeln!(
        out,
        "<header><div><h1>{title}</h1><div class=\"meta\">Load test · {meta}</div></div><div class=\"verdict {}\">{verdict}</div></header>",
        if s.passed { "pass" } else { "fail" }
    );
    if let Some(error) = &s.error {
        let _ = writeln!(out, "<p class=\"error\">{}</p>", esc(error));
    }

    // Key numbers.
    let t = &s.totals;
    out.push_str("<section class=\"tiles\">\n");
    tile(&mut out, "Requests", &thousands(t.requests), &format!("{} req/s", rate(t.rps)));
    tile(&mut out, "Errors", &format!("{} %", number2(t.error_rate)), &format!("{} failed", thousands(t.errors)));
    tile(&mut out, "p95 latency", &ms(t.latency.p95), &format!("p50 {}", ms(t.latency.p50)));
    tile(&mut out, "p99 latency", &ms(t.latency.p99), &format!("max {}", ms(t.latency.max)));
    if t.timing.ttfb.count > 0 {
        tile(&mut out, "p95 first byte", &ms(t.timing.ttfb.p95), "server + network");
    }
    tile(&mut out, "Data", &bytes(t.bytes_in), &format!("{} sent", bytes(t.bytes_out)));
    tile(
        &mut out,
        "Connections",
        &thousands(t.connections),
        &format!("opened by {} % of requests", number2(share(t.connections, t.requests))),
    );
    if t.capture_misses > 0 {
        tile(&mut out, "Capture misses", &thousands(t.capture_misses), "found nothing");
    }
    if t.dropped > 0 {
        tile(&mut out, "Dropped", &thousands(t.dropped), "not started: in-flight limit");
    }
    if let Some(cpu) = s.peak_cpu_percent {
        tile(&mut out, "Generator CPU", &format!("{cpu:.0} %"), "peak, 100 % = one core");
    }
    out.push_str("</section>\n");

    if !s.thresholds.is_empty() {
        out.push_str("<section><h2>Thresholds</h2><div class=\"table\"><table>\n");
        out.push_str("<tr><th>Threshold</th><th>Actual</th><th>Result</th></tr>\n");
        for th in &s.thresholds {
            let actual = th.actual.map_or_else(|| "no data".to_string(), number2);
            let (class, result) = if th.passed { ("pass", "✓ Passed") } else { ("fail", "✗ Failed") };
            let _ = writeln!(
                out,
                "<tr><td>{}</td><td>{}</td><td class=\"{class}\">{result}</td></tr>",
                esc(&th.label),
                esc(&actual)
            );
        }
        out.push_str("</table></div></section>\n");
    }

    charts(&mut out, &s.points);

    out.push_str("<section><h2>Latency</h2><div class=\"table\"><table>\n");
    out.push_str("<tr><th>All requests</th><th>min</th><th>avg</th><th>p50</th><th>p90</th><th>p95</th><th>p99</th><th>p99.9</th><th>max</th></tr>\n");
    let l = &t.latency;
    let _ = writeln!(out, "<tr><td>milliseconds</td>{}</tr>", latency_cells(l));
    out.push_str("</table></div></section>\n");

    timing_table(&mut out, t);

    if !s.targets.is_empty() {
        let ttfb = s.targets.iter().any(|t| t.metrics.timing.ttfb.count > 0);
        let misses = s.targets.iter().any(|t| t.metrics.capture_misses > 0);
        out.push_str("<section><h2>Requests</h2><div class=\"table\"><table>\n");
        out.push_str("<tr><th>Request</th><th>Requests</th><th>req/s</th><th>Errors</th><th>p50</th><th>p95</th><th>p99</th><th>max</th>");
        if ttfb {
            out.push_str("<th>p95 first byte</th>");
        }
        if misses {
            out.push_str("<th>Capture misses</th>");
        }
        out.push_str("<th>Data in</th></tr>\n");
        for target in &s.targets {
            let m = &target.metrics;
            let _ = write!(
                out,
                "<tr><td>{}<br><span class=\"muted\">{}</span></td><td>{}</td><td>{}</td><td>{} %</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td>",
                esc(&target.name),
                esc(&target.request),
                thousands(m.requests),
                rate(m.rps),
                number2(m.error_rate),
                ms(m.latency.p50),
                ms(m.latency.p95),
                ms(m.latency.p99),
                ms(m.latency.max),
            );
            if ttfb {
                let _ = write!(out, "<td>{}</td>", phase_ms(&m.timing.ttfb, m.timing.ttfb.p95));
            }
            if misses {
                let class = if m.capture_misses > 0 { " class=\"fail\"" } else { "" };
                let _ = write!(out, "<td{class}>{}</td>", thousands(m.capture_misses));
            }
            let _ = writeln!(out, "<td>{}</td></tr>", bytes(m.bytes_in));
        }
        out.push_str("</table></div></section>\n");
    }

    out.push_str("<div class=\"split\">\n");
    status_table(&mut out, t);
    error_table(&mut out, t);
    out.push_str("</div>\n");

    out.push_str("<footer>Made by Zorvik. Latency counts from each request's scheduled start (open model) or send (virtual users) to the end of the response body.</footer>\n");
    out.push_str("</main>\n</body>\n</html>\n");
    out
}

fn tile(out: &mut String, label: &str, value: &str, sub: &str) {
    let _ = writeln!(
        out,
        "<div class=\"tile\"><div class=\"label\">{label}</div><div class=\"value\">{}</div><div class=\"sub\">{}</div></div>",
        esc(value),
        esc(sub)
    );
}

/// Where the time went: connect, first byte, transfer and (when servers sent
/// `Server-Timing`) what they reported. Nothing for runs saved without it.
fn timing_table(out: &mut String, t: &MetricsSummary) {
    let tm = &t.timing;
    if tm.ttfb.count == 0 && tm.connect.count == 0 {
        return;
    }
    out.push_str("<section><h2>Timing</h2><div class=\"table\"><table>\n");
    out.push_str(
        "<tr><th>Phase</th><th>Requests</th><th>avg</th><th>p50</th><th>p95</th><th>p99</th><th>max</th></tr>\n",
    );
    let new_share = number2(share(t.connections, t.requests));
    let rows = [
        ("Connect", format!("DNS, TCP and TLS of a new connection: {new_share} % of requests opened one"), &tm.connect),
        (
            "Time to first byte",
            "request sent to first byte: server time plus one network round trip".to_string(),
            &tm.ttfb,
        ),
        ("Transfer", "first byte to last byte".to_string(), &tm.transfer),
        (
            "Server-reported",
            "from the Server-Timing header (total, or the sum of its durations)".to_string(),
            &tm.server,
        ),
    ];
    for (name, note, p) in rows {
        if p.count == 0 && name == "Server-reported" {
            continue;
        }
        let _ = writeln!(
            out,
            "<tr><td>{name}<br><span class=\"muted\">{}</span></td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
            esc(&note),
            thousands(p.count),
            phase_ms(p, p.avg),
            phase_ms(p, p.p50),
            phase_ms(p, p.p95),
            phase_ms(p, p.p99),
            phase_ms(p, p.max),
        );
    }
    out.push_str("</table></div>");
    if tm.server.count == 0 {
        out.push_str("<p class=\"muted\">No response had a Server-Timing header, so the server's own time is not known: the time to first byte includes the network.</p>");
    }
    out.push_str("</section>\n");
}

/// A phase value, or a dash when the phase has no data.
fn phase_ms(p: &PhaseSummary, v: f64) -> String {
    if p.count == 0 { "–".to_string() } else { ms(v) }
}

/// `part` of `whole` in percent.
fn share(part: u64, whole: u64) -> f64 {
    if whole == 0 { 0.0 } else { part as f64 * 100.0 / whole as f64 }
}

fn latency_cells(l: &LatencySummary) -> String {
    [l.min, l.avg, l.p50, l.p90, l.p95, l.p99, l.p999, l.max].iter().map(|v| format!("<td>{}</td>", ms(*v))).collect()
}

fn status_table(out: &mut String, t: &MetricsSummary) {
    out.push_str("<section><h2>Status codes</h2>");
    if t.status_codes.is_empty() {
        out.push_str("<p class=\"muted\">No responses.</p></section>\n");
        return;
    }
    out.push_str("<div class=\"table\"><table>\n<tr><th>Status</th><th>Responses</th><th>Share</th></tr>\n");
    for (status, count) in &t.status_codes {
        let share = if t.requests == 0 { 0.0 } else { *count as f64 * 100.0 / t.requests as f64 };
        let class = if *status >= 400 { " class=\"fail\"" } else { "" };
        let _ = writeln!(
            out,
            "<tr><td{class}>{status}</td><td>{}</td><td>{} %</td></tr>",
            thousands(*count),
            number2(share)
        );
    }
    out.push_str("</table></div></section>\n");
}

fn error_table(out: &mut String, t: &MetricsSummary) {
    out.push_str("<section><h2>Network errors</h2>");
    if t.error_kinds.is_empty() {
        out.push_str("<p class=\"muted\">None.</p></section>\n");
        return;
    }
    out.push_str("<div class=\"table\"><table>\n<tr><th>Error</th><th>Requests</th></tr>\n");
    for (kind, count) in &t.error_kinds {
        let label = match kind.as_str() {
            "timeout" => "Timed out",
            "connect" => "Could not connect",
            "dns" => "Host name not found",
            "tls" => "TLS handshake failed",
            "proxy" => "Proxy error",
            "protocol" => "Protocol error",
            "io" => "Connection broke",
            "cancelled" => "Cut off by the stop",
            "invalidRequest" => "Request could not be built",
            _ => kind.as_str(),
        };
        let _ = writeln!(
            out,
            "<tr><td>{} <span class=\"muted\">{}</span></td><td>{}</td></tr>",
            esc(label),
            esc(kind),
            thousands(*count)
        );
    }
    out.push_str("</table></div></section>\n");
}

/// One chart column: seconds `from..to` (averaged when the run is long).
struct Column {
    from: u32,
    to: u32,
    rps: f64,
    errors: f64,
    p50: f64,
    p95: f64,
    p99: f64,
    active: f64,
}

fn columns(points: &[TimePoint]) -> Vec<Column> {
    let per = points.len().div_ceil(MAX_COLUMNS).max(1);
    points
        .chunks(per)
        .map(|c| {
            let n = c.len() as f64;
            let max = |f: fn(&TimePoint) -> f64| c.iter().map(f).fold(0.0, f64::max);
            Column {
                from: c[0].second,
                to: c[c.len() - 1].second + 1,
                rps: c.iter().map(|p| p.rps).sum::<f64>() / n,
                errors: c.iter().map(|p| f64::from(p.errors)).sum::<f64>() / n,
                p50: c.iter().map(|p| p.p50).sum::<f64>() / n,
                p95: max(|p| p.p95),
                p99: max(|p| p.p99),
                active: c.iter().map(|p| f64::from(p.active)).sum::<f64>() / n,
            }
        })
        .collect()
}

struct Series<'a> {
    name: &'a str,
    color: &'a str,
    values: Vec<f64>,
}

fn charts(out: &mut String, points: &[TimePoint]) {
    out.push_str("<section><h2>Over time</h2>\n");
    if points.is_empty() {
        out.push_str("<p class=\"muted\">No data yet.</p></section>\n");
        return;
    }
    let cols = columns(points);
    let span = |c: &Column| {
        if c.to - c.from <= 1 { clock(c.from) } else { format!("{}–{}", clock(c.from), clock(c.to)) }
    };
    let throughput = [
        Series { name: "Completed", color: "var(--s1)", values: cols.iter().map(|c| c.rps).collect() },
        Series { name: "Failed", color: "var(--s2)", values: cols.iter().map(|c| c.errors).collect() },
    ];
    let tips: Vec<String> =
        cols.iter().map(|c| format!("{} · {} req/s · {} failed/s", span(c), rate(c.rps), rate(c.errors))).collect();
    chart(out, "Requests per second", &cols, &throughput, &tips, true, false);

    let latency = [
        Series { name: "p50", color: "var(--s1)", values: cols.iter().map(|c| c.p50).collect() },
        Series { name: "p95", color: "var(--s2)", values: cols.iter().map(|c| c.p95).collect() },
        Series { name: "p99", color: "var(--s3)", values: cols.iter().map(|c| c.p99).collect() },
    ];
    let tips: Vec<String> = cols
        .iter()
        .map(|c| format!("{} · p50 {} · p95 {} · p99 {}", span(c), ms(c.p50), ms(c.p95), ms(c.p99)))
        .collect();
    chart(out, "Latency (ms)", &cols, &latency, &tips, false, false);

    let active = [Series { name: "Active", color: "var(--s1)", values: cols.iter().map(|c| c.active).collect() }];
    let tips: Vec<String> = cols.iter().map(|c| format!("{} · {} active", span(c), number(c.active.round()))).collect();
    chart(out, "Users or requests in flight", &cols, &active, &tips, true, true);
    out.push_str("</section>\n");
}

/// A line chart over time with one y axis starting at 0 (`whole`: counts, no
/// fractional ticks). Hovering a column shows its values (SVG titles, no script).
fn chart(out: &mut String, title: &str, cols: &[Column], series: &[Series], tips: &[String], area: bool, whole: bool) {
    const W: f64 = 800.0;
    const H: f64 = 220.0;
    const LEFT: f64 = 56.0;
    const RIGHT: f64 = 16.0;
    const TOP: f64 = 10.0;
    const BOTTOM: f64 = 26.0;
    let (pw, ph) = (W - LEFT - RIGHT, H - TOP - BOTTOM);
    let start = cols.first().map_or(0, |c| c.from);
    let end = cols.last().map_or(1, |c| c.to).max(start + 1);
    let x = |t: f64| LEFT + (t - f64::from(start)) / f64::from(end - start) * pw;
    let mid = |c: &Column| x((f64::from(c.from) + f64::from(c.to)) / 2.0);
    let max = series.iter().flat_map(|s| s.values.iter().copied()).fold(0.0, f64::max);
    let (step, top) = nice_scale(max, whole);
    let y = |v: f64| TOP + ph - v / top * ph;

    let _ = writeln!(
        out,
        "<div class=\"chart\"><h2 class=\"muted\" style=\"font-size:13px;font-weight:600\">{}</h2>",
        esc(title)
    );
    if series.len() > 1 {
        out.push_str("<div class=\"legend\">");
        for s in series {
            let _ =
                write!(out, "<span><span class=\"key\" style=\"background:{}\"></span>{}</span>", s.color, esc(s.name));
        }
        out.push_str("</div>");
    }
    let _ = writeln!(out, "<svg viewBox=\"0 0 {W} {H}\" role=\"img\" aria-label=\"{}\">", esc(title));
    // Horizontal grid and y labels.
    let mut v = 0.0;
    while v <= top + step / 2.0 {
        let yy = y(v);
        let class = if v == 0.0 { "base" } else { "grid" };
        let _ = writeln!(
            out,
            "<line class=\"{class}\" x1=\"{LEFT}\" x2=\"{}\" y1=\"{yy:.1}\" y2=\"{yy:.1}\"/><text x=\"{}\" y=\"{:.1}\" text-anchor=\"end\">{}</text>",
            W - RIGHT,
            LEFT - 8.0,
            yy + 4.0,
            axis_number(v)
        );
        v += step;
    }
    // Time labels.
    let tick = time_step(end - start);
    let mut t = start.div_ceil(tick) * tick;
    while t <= end {
        let _ = writeln!(
            out,
            "<text x=\"{:.1}\" y=\"{}\" text-anchor=\"middle\">{}</text>",
            x(f64::from(t)),
            H - 8.0,
            clock(t)
        );
        t += tick;
    }
    for (i, s) in series.iter().enumerate() {
        let pts: Vec<String> = cols.iter().zip(&s.values).map(|(c, v)| format!("{:.1},{:.1}", mid(c), y(*v))).collect();
        if cols.len() == 1 {
            let _ = writeln!(
                out,
                "<circle cx=\"{:.1}\" cy=\"{:.1}\" r=\"4\" fill=\"{}\" stroke=\"var(--surface)\" stroke-width=\"2\"/>",
                mid(&cols[0]),
                y(s.values[0]),
                s.color
            );
            continue;
        }
        if area && i == 0 {
            let _ = writeln!(
                out,
                "<polygon points=\"{:.1},{:.1} {} {:.1},{:.1}\" fill=\"{}\" fill-opacity=\"0.1\"/>",
                mid(&cols[0]),
                y(0.0),
                pts.join(" "),
                mid(&cols[cols.len() - 1]),
                y(0.0),
                s.color
            );
        }
        let _ = writeln!(out, "<polyline class=\"line\" stroke=\"{}\" points=\"{}\"/>", s.color, pts.join(" "));
    }
    // Hover bands.
    for (c, tip) in cols.iter().zip(tips) {
        let (x0, x1) = (x(f64::from(c.from)), x(f64::from(c.to)));
        let _ = writeln!(
            out,
            "<rect class=\"hit\" x=\"{x0:.1}\" y=\"{TOP}\" width=\"{:.1}\" height=\"{ph}\"><title>{}</title></rect>",
            (x1 - x0).max(0.5),
            esc(tip)
        );
    }
    out.push_str("</svg></div>\n");
}

/// A round step and the axis top (a multiple of the step, ≥ `max`), about 4
/// steps; `whole`: counts, so no fractional steps.
fn nice_scale(max: f64, whole: bool) -> (f64, f64) {
    if max.is_nan() || max <= 0.0 {
        return (1.0, if whole { 1.0 } else { 4.0 });
    }
    let raw = max / 4.0;
    let magnitude = 10f64.powf(raw.log10().floor());
    let mut step = [1.0, 2.0, 2.5, 5.0, 10.0]
        .iter()
        .map(|m| m * magnitude)
        .find(|s| *s >= raw && !(whole && s.fract() != 0.0))
        .unwrap_or(10.0 * magnitude);
    if whole {
        step = step.max(1.0);
    }
    (step, (max / step).ceil().max(1.0) * step)
}

/// Seconds between time labels: at most ~8 labels.
fn time_step(span: u32) -> u32 {
    [1, 2, 5, 10, 15, 30, 60, 120, 300, 600, 900, 1800, 3600, 7200, 10800, 21600]
        .into_iter()
        .find(|s| span / s <= 8)
        .unwrap_or(43200)
}

fn clock(secs: u32) -> String {
    let (h, m, s) = (secs / 3600, secs % 3600 / 60, secs % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}

fn duration(ms: u64) -> String {
    let secs = ms as f64 / 1000.0;
    if secs < 60.0 { format!("{} s", number(round1(secs))) } else { clock(secs.round() as u32) }
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

fn thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Requests per second: one decimal below 100, whole numbers above.
fn rate(v: f64) -> String {
    if v >= 100.0 { thousands(v.round() as u64) } else { number(round1(v)) }
}

fn number2(v: f64) -> String {
    number((v * 100.0).round() / 100.0)
}

fn ms(v: f64) -> String {
    let text = if v >= 100.0 {
        thousands(v.round() as u64)
    } else if v >= 10.0 {
        number(round1(v))
    } else {
        number2(v)
    };
    format!("{text} ms")
}

fn axis_number(v: f64) -> String {
    if v >= 10_000.0 {
        format!("{}k", number(v / 1000.0))
    } else if v >= 1000.0 {
        thousands(v as u64)
    } else {
        number(v)
    }
}

fn bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut unit = 0;
    while v >= 1000.0 && unit < UNITS.len() - 1 {
        v /= 1000.0;
        unit += 1;
    }
    if unit == 0 { format!("{n} B") } else { format!("{} {}", number(round1(v)), UNITS[unit]) }
}

/// `2026-09-27 14:03:05 UTC` from Unix milliseconds.
fn utc(ms: f64) -> String {
    let secs = (ms / 1000.0).floor() as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Days to civil date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02} UTC", rem / 3600, rem % 3600 / 60, rem % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        assert_eq!(utc(0.0), "1970-01-01 00:00:00 UTC");
        assert_eq!(utc(1_790_512_385_000.0), "2026-09-27 12:33:05 UTC");
        assert_eq!(thousands(1_234_567), "1,234,567");
        assert_eq!(bytes(1_500_000), "1.5 MB");
        assert_eq!(ms(4.256), "4.26 ms");
        assert_eq!(ms(1234.5), "1,235 ms");
        assert_eq!(clock(3725), "1:02:05");
        assert_eq!(nice_scale(730.0, false), (200.0, 800.0));
        assert_eq!(nice_scale(0.0, false), (1.0, 4.0));
        assert_eq!(nice_scale(1.0, true), (1.0, 1.0));
        assert_eq!(nice_scale(9.0, true), (5.0, 10.0));
        assert_eq!(time_step(60), 10);
        assert_eq!(esc("<a href='x'>&\"</a>"), "&lt;a href=&#39;x&#39;&gt;&amp;&quot;&lt;/a&gt;");
    }
}

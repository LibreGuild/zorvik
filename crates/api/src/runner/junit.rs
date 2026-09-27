//! JUnit XML for CI (Jenkins, GitLab, GitHub test reporters): one `<testsuite>`
//! per request, one `<testcase>` per `pm.test` (per iteration), and one for the
//! request itself when it has no tests or failed without them (network error,
//! script error, HTTP status).

use std::fmt::Write as _;

use super::{RunReport, RunResult};

/// The report as JUnit XML.
pub fn junit_xml(report: &RunReport) -> String {
    let s = &report.summary;
    // Suites in the order their requests first ran.
    let mut suites: Vec<(&str, Vec<&RunResult>)> = Vec::new();
    for r in &report.results {
        match suites.iter_mut().find(|(path, _)| *path == r.path) {
            Some((_, list)) => list.push(r),
            None => suites.push((&r.path, vec![r])),
        }
    }
    let repeat = s.iterations > 1;
    let mut body = String::new();
    let mut totals = Counts::default();
    for (path, results) in &suites {
        let suite = path.strip_suffix(".yaml").unwrap_or(path);
        let mut counts = Counts::default();
        let mut cases = String::new();
        for r in results {
            counts.time += r.duration_ms.unwrap_or_default() / 1000.0;
            for case in cases_of(r, repeat) {
                counts.tests += 1;
                let _ = write!(
                    cases,
                    "    <testcase name=\"{}\" classname=\"{}\" time=\"{:.3}\"",
                    escape(&case.name),
                    escape(suite),
                    case.time
                );
                match case.outcome {
                    Outcome::Passed => cases.push_str("/>\n"),
                    Outcome::Skipped(reason) => {
                        counts.skipped += 1;
                        match reason {
                            Some(r) => cases.push_str(&format!(
                                ">\n      <skipped message=\"{}\"/>\n    </testcase>\n",
                                escape(&r)
                            )),
                            None => cases.push_str(">\n      <skipped/>\n    </testcase>\n"),
                        }
                    }
                    Outcome::Failure(message) => {
                        counts.failures += 1;
                        let _ = write!(
                            cases,
                            ">\n      <failure message=\"{0}\" type=\"AssertionError\">{0}</failure>\n    </testcase>\n",
                            escape(&message)
                        );
                    }
                    Outcome::Error(message) => {
                        counts.errors += 1;
                        let _ = write!(
                            cases,
                            ">\n      <error message=\"{0}\" type=\"Error\">{0}</error>\n    </testcase>\n",
                            escape(&message)
                        );
                    }
                }
            }
        }
        let _ = writeln!(
            body,
            "  <testsuite name=\"{}\" tests=\"{}\" failures=\"{}\" errors=\"{}\" skipped=\"{}\" time=\"{:.3}\">",
            escape(suite),
            counts.tests,
            counts.failures,
            counts.errors,
            counts.skipped,
            counts.time
        );
        body.push_str(&cases);
        body.push_str("  </testsuite>\n");
        totals.add(&counts);
    }
    let mut xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    let _ = writeln!(
        xml,
        "<testsuites name=\"{}\" tests=\"{}\" failures=\"{}\" errors=\"{}\" skipped=\"{}\" time=\"{:.3}\" timestamp=\"{}\">",
        escape(&s.name),
        totals.tests,
        totals.failures,
        totals.errors,
        totals.skipped,
        s.duration_ms / 1000.0,
        timestamp(s.started_at)
    );
    xml.push_str(&body);
    xml.push_str("</testsuites>\n");
    xml
}

#[derive(Default)]
struct Counts {
    tests: u32,
    failures: u32,
    errors: u32,
    skipped: u32,
    time: f64,
}

impl Counts {
    fn add(&mut self, other: &Counts) {
        self.tests += other.tests;
        self.failures += other.failures;
        self.errors += other.errors;
        self.skipped += other.skipped;
    }
}

enum Outcome {
    Passed,
    Skipped(Option<String>),
    /// A test or the HTTP status failed.
    Failure(String),
    /// The request could not be sent, or a script broke.
    Error(String),
}

struct Case {
    name: String,
    /// Seconds: the request's time on its first case, 0 on the others (so sums stay right).
    time: f64,
    outcome: Outcome,
}

fn cases_of(r: &RunResult, repeat: bool) -> Vec<Case> {
    let suffix = if repeat { format!(" (iteration {})", r.iteration + 1) } else { String::new() };
    let mut cases = Vec::new();
    if r.skipped {
        let reason = r.skip_reason.clone();
        cases.push(Case {
            name: format!("{}{suffix}", request_label(r)),
            time: 0.0,
            outcome: Outcome::Skipped(reason),
        });
        return cases;
    }
    let counted = r.tests.iter().any(|t| !t.skipped);
    let problem = r.error.iter().chain(&r.script_errors).cloned().collect::<Vec<_>>().join("\n");
    if !problem.is_empty() {
        cases.push(Case { name: format!("{}{suffix}", request_label(r)), time: 0.0, outcome: Outcome::Error(problem) });
    } else if !counted {
        let outcome = if r.passed {
            Outcome::Passed
        } else {
            Outcome::Failure(format!("HTTP {}", r.status.map(|s| s.to_string()).unwrap_or_default()))
        };
        cases.push(Case { name: format!("{}{suffix}", request_label(r)), time: 0.0, outcome });
    }
    for t in &r.tests {
        let outcome = match (&t.error, t.passed, t.skipped) {
            (_, _, true) => Outcome::Skipped(None),
            (_, true, _) => Outcome::Passed,
            (Some(e), false, _) if !e.is_empty() => Outcome::Failure(e.clone()),
            _ => Outcome::Failure("failed".into()),
        };
        cases.push(Case { name: format!("{}{suffix}", t.name), time: 0.0, outcome });
    }
    if let Some(first) = cases.first_mut() {
        first.time = r.duration_ms.unwrap_or_default() / 1000.0;
    }
    cases
}

/// "GET Get user".
fn request_label(r: &RunResult) -> String {
    if r.method.is_empty() { r.name.clone() } else { format!("{} {}", r.method, r.name) }
}

/// Text for an XML attribute or element. Characters XML 1.0 can't carry at all
/// (control characters) are written as `\u{…}`.
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\n' => out.push_str("&#10;"),
            '\t' => out.push_str("&#9;"),
            '\r' => out.push_str("&#13;"),
            c if (c as u32) < 0x20 || c == '\u{fffe}' || c == '\u{ffff}' => {
                let _ = write!(out, "\\u{{{:x}}}", c as u32);
            }
            c => out.push(c),
        }
    }
    out
}

/// ISO 8601 (UTC, seconds) of a Unix time in milliseconds.
fn timestamp(ms: f64) -> String {
    let format = time::macros::format_description!("[year]-[month]-[day]T[hour]:[minute]:[second]");
    time::OffsetDateTime::from_unix_timestamp((ms / 1000.0) as i64)
        .ok()
        .and_then(|t| t.format(format).ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::super::{RunSummary, test_result};
    use super::*;

    #[test]
    fn suites_per_request_and_cases_per_test() {
        let mut ok = test_result("Users/Get user.yaml", "Get user", 0);
        ok.status = Some(200);
        ok.duration_ms = Some(12.0);
        ok.tests = vec![
            zorvik_script::TestResult { name: "status <ok>".into(), passed: true, skipped: false, error: None },
            zorvik_script::TestResult {
                name: "body".into(),
                passed: false,
                skipped: false,
                error: Some("expected 'a' to equal \"b\" & more".into()),
            },
            zorvik_script::TestResult { name: "later".into(), passed: false, skipped: true, error: None },
        ];
        let mut down = test_result("Health.yaml", "Health", 0);
        down.error = Some("Connection refused\u{1b}".into());
        down.passed = false;
        let mut again = test_result("Users/Get user.yaml", "Get user", 1);
        again.status = Some(200);
        again.duration_ms = Some(8.0);
        let mut missing = test_result("Users/Missing.yaml", "Missing", 1);
        missing.status = Some(404);
        missing.passed = false;
        let mut socket = test_result("Socket.yaml", "Socket", 1);
        socket.skipped = true;
        socket.method = String::new();
        let report = RunReport {
            summary: RunSummary {
                name: "Demo <api>".into(),
                started_at: 1_790_000_000_000.0,
                duration_ms: 1500.0,
                iterations: 2,
                ..Default::default()
            },
            results: vec![ok, down, again, missing, socket],
        };
        let xml = junit_xml(&report);
        assert_eq!(
            xml,
            r#"<?xml version="1.0" encoding="UTF-8"?>
<testsuites name="Demo &lt;api&gt;" tests="7" failures="2" errors="1" skipped="2" time="1.500" timestamp="2026-09-21T14:13:20">
  <testsuite name="Users/Get user" tests="4" failures="1" errors="0" skipped="1" time="0.020">
    <testcase name="status &lt;ok&gt; (iteration 1)" classname="Users/Get user" time="0.012"/>
    <testcase name="body (iteration 1)" classname="Users/Get user" time="0.000">
      <failure message="expected &apos;a&apos; to equal &quot;b&quot; &amp; more" type="AssertionError">expected &apos;a&apos; to equal &quot;b&quot; &amp; more</failure>
    </testcase>
    <testcase name="later (iteration 1)" classname="Users/Get user" time="0.000">
      <skipped/>
    </testcase>
    <testcase name="GET Get user (iteration 2)" classname="Users/Get user" time="0.008"/>
  </testsuite>
  <testsuite name="Health" tests="1" failures="0" errors="1" skipped="0" time="0.000">
    <testcase name="GET Health (iteration 1)" classname="Health" time="0.000">
      <error message="Connection refused\u{1b}" type="Error">Connection refused\u{1b}</error>
    </testcase>
  </testsuite>
  <testsuite name="Users/Missing" tests="1" failures="1" errors="0" skipped="0" time="0.000">
    <testcase name="GET Missing (iteration 2)" classname="Users/Missing" time="0.000">
      <failure message="HTTP 404" type="AssertionError">HTTP 404</failure>
    </testcase>
  </testsuite>
  <testsuite name="Socket" tests="1" failures="0" errors="0" skipped="1" time="0.000">
    <testcase name="Socket (iteration 2)" classname="Socket" time="0.000">
      <skipped/>
    </testcase>
  </testsuite>
</testsuites>
"#
        );
    }

    #[test]
    fn single_iteration_names_have_no_suffix() {
        let mut r = test_result("A.yaml", "A", 0);
        r.status = Some(204);
        let report = RunReport { summary: RunSummary { iterations: 1, ..Default::default() }, results: vec![r] };
        assert!(junit_xml(&report).contains(r#"<testcase name="GET A" classname="A" time="0.000"/>"#));
    }
}

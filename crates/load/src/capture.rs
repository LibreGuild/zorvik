//! Per-user variables (a data file row and captured values), captures that
//! take values from responses (JSON path, header, regex), and the
//! `Server-Timing` header.

use std::sync::Arc;

use serde_json::Value;
use zorvik_engine::Header;
use zorvik_engine::pool::KeptResponse;
use zorvik_formats::matching::JsonPath;
use zorvik_formats::{CaptureFrom, LoadCapture};

/// Body bytes a capture looks at (the rest of the body is read, not kept).
pub(crate) const CAPTURE_BODY: usize = 1024 * 1024;

/// Longest value a capture keeps (bytes). Each user holds its captured values
/// for its whole life; a longer value counts as a miss.
pub(crate) const MAX_CAPTURED: usize = 64 * 1024;

/// What [`UserVars::probe`] sets every name to: plain letters, so it stays a
/// valid URL, header and host name.
pub(crate) const PROBE: &str = "zvprobe7c1e";

/// A data file row as `(column, value)` text.
pub type DataRow = Arc<[(String, String)]>;

/// The variables of one virtual user (or one request in the arrival-rate
/// model): its data file row and the values captured from its responses.
/// Captured values win over the row.
#[derive(Debug, Clone, Default)]
pub struct UserVars {
    row: Option<DataRow>,
    captured: Vec<(String, String)>,
}

impl UserVars {
    pub fn new(row: Option<DataRow>) -> Self {
        Self { row, captured: Vec::new() }
    }

    /// Every name set to the same probe value: a render with these differs from
    /// one without them exactly when the request uses one of the names.
    pub fn probe(names: &[String]) -> Self {
        Self { row: None, captured: names.iter().map(|n| (n.clone(), PROBE.to_string())).collect() }
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.captured
            .iter()
            .find(|(k, _)| k == name)
            .or_else(|| self.row.as_deref()?.iter().find(|(k, _)| k == name))
            .map(|(_, v)| v.as_str())
    }

    /// Every variable once: captured values, then the row's other columns.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        let row = self.row.as_deref().unwrap_or_default();
        self.captured
            .iter()
            .chain(row.iter().filter(|(k, _)| !self.captured.iter().any(|(c, _)| c == k)))
            .map(|(k, v)| (k.as_str(), v.as_str()))
    }

    pub fn is_empty(&self) -> bool {
        self.captured.is_empty() && self.row.as_deref().is_none_or(<[_]>::is_empty)
    }

    pub(crate) fn set(&mut self, name: &str, value: String) {
        match self.captured.iter_mut().find(|(k, _)| k == name) {
            Some(slot) => slot.1 = value,
            None => self.captured.push((name.to_string(), value)),
        }
    }
}

/// A capture checked and ready to run.
#[derive(Debug)]
pub(crate) struct Capture {
    variable: String,
    how: How,
}

#[derive(Debug)]
enum How {
    Json(JsonPath),
    /// Lowercase header name.
    Header(String),
    Regex(regex::bytes::Regex),
}

impl Capture {
    /// Check a capture; the message names what is wrong.
    pub(crate) fn new(c: &LoadCapture) -> Result<Self, String> {
        let variable = c.variable.trim();
        if variable.is_empty() {
            return Err("a capture has no variable name".into());
        }
        if variable.contains(['{', '}']) || variable.chars().any(char::is_whitespace) {
            return Err(format!("'{variable}' can't be a variable name (no spaces or braces)"));
        }
        let path = c.path.trim();
        let problem = |message: String| format!("capture '{variable}': {message}");
        let how = match c.from {
            CaptureFrom::Json => How::Json(JsonPath::parse(path).map_err(problem)?),
            CaptureFrom::Header if path.is_empty() => return Err(problem("give the header name".into())),
            CaptureFrom::Header => How::Header(path.to_ascii_lowercase()),
            CaptureFrom::Regex if path.is_empty() => return Err(problem("give a regular expression".into())),
            CaptureFrom::Regex => How::Regex(
                regex::bytes::RegexBuilder::new(path)
                    .size_limit(1 << 20)
                    .build()
                    .map_err(|e| problem(format!("invalid regular expression: {e}")))?,
            ),
        };
        Ok(Self { variable: variable.to_string(), how })
    }
}

/// Run a target's captures on its response: found values go into `vars`.
/// Returns how many found nothing (or a value over [`MAX_CAPTURED`]).
pub(crate) fn apply(captures: &[Capture], response: &KeptResponse, vars: &mut UserVars) -> u32 {
    // Parsed once, and only when a JSON capture needs it.
    let mut json: Option<Option<Value>> = None;
    let mut misses = 0;
    for capture in captures {
        let found = match &capture.how {
            How::Json(steps) => json
                .get_or_insert_with(|| serde_json::from_slice(&response.body).ok())
                .as_ref()
                .and_then(|root| steps.text(root)),
            How::Header(name) => header(&response.headers, name),
            How::Regex(re) => regex_value(re, &response.body),
        };
        match found {
            Some(value) if value.len() <= MAX_CAPTURED => vars.set(&capture.variable, value),
            Some(value) => {
                tracing::debug!("capture '{}' found {} bytes, more than it keeps", capture.variable, value.len());
                misses += 1;
            }
            None => misses += 1,
        }
    }
    misses
}

fn header(headers: &[Header], name: &str) -> Option<String> {
    headers.iter().find(|h| h.name.eq_ignore_ascii_case(name)).map(|h| h.value.clone())
}

/// The first match's first group (the whole match for a pattern without groups).
fn regex_value(re: &regex::bytes::Regex, body: &[u8]) -> Option<String> {
    let caps = re.captures(body)?;
    let m = if re.captures_len() > 1 { caps.get(1)? } else { caps.get(0)? };
    Some(String::from_utf8_lossy(m.as_bytes()).into_owned())
}

// ---- Server-Timing ----------------------------------------------------------

/// The time a server reports in `Server-Timing` (milliseconds): the `total`
/// metric's `dur` when there is one, else the sum of every `dur`. `None` when
/// no metric has a usable duration.
///
/// `db;dur=53, app;dur=47.2` → 100.2; `cache;desc="a, b";dur=2, total;dur=12.3` → 12.3.
pub fn server_timing_ms(header: &str) -> Option<f64> {
    let mut sum = 0.0;
    let mut any = false;
    for metric in split_outside_quotes(header, ',') {
        let mut parts = split_outside_quotes(metric, ';').into_iter();
        let name = parts.next().unwrap_or_default().trim();
        if name.is_empty() {
            continue;
        }
        let dur = parts.find_map(|p| {
            let (key, value) = p.split_once('=')?;
            if !key.trim().eq_ignore_ascii_case("dur") {
                return None;
            }
            let value = value.trim().trim_matches('"');
            value.parse::<f64>().ok().filter(|v| v.is_finite() && *v >= 0.0)
        });
        let Some(dur) = dur else { continue };
        if name.eq_ignore_ascii_case("total") {
            return Some(dur);
        }
        sum += dur;
        any = true;
    }
    any.then_some(sum)
}

/// Split at `sep` where it is not inside a "quoted string" (with `\"` escapes).
fn split_outside_quotes(s: &str, sep: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let (mut start, mut quoted, mut escaped) = (0, false, false);
    for (i, c) in s.char_indices() {
        if quoted {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                quoted = false;
            }
        } else if c == '"' {
            quoted = true;
        } else if c == sep {
            parts.push(&s[start..i]);
            start = i + c.len_utf8();
        }
    }
    parts.push(&s[start..]);
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capture(from: CaptureFrom, variable: &str, path: &str) -> Capture {
        Capture::new(&LoadCapture { variable: variable.into(), from, path: path.into() }).unwrap()
    }

    fn response(headers: &[(&str, &str)], body: &str) -> KeptResponse {
        KeptResponse {
            headers: headers.iter().map(|(n, v)| Header::new(*n, *v)).collect(),
            body: body.as_bytes().to_vec().into(),
        }
    }

    #[test]
    fn captures_take_values_and_count_misses() {
        let captures = [
            capture(CaptureFrom::Json, "orderId", "$.order.id"),
            capture(CaptureFrom::Header, "etag", "ETag"),
            capture(CaptureFrom::Regex, "token", r#"token=(\w+)"#),
            capture(CaptureFrom::Regex, "whole", r"[A-Z]{3}"),
            capture(CaptureFrom::Json, "missing", "$.nope"),
            capture(CaptureFrom::Header, "absent", "X-Absent"),
            capture(CaptureFrom::Regex, "nomatch", "zzz(\\d)"),
        ];
        let mut vars = UserVars::new(Some(vec![("missing".to_string(), "from row".to_string())].into()));
        let r = response(
            &[("etag", "\"v1\""), ("content-type", "application/json")],
            r#"{"order": {"id": 42}, "note": "token=abc123 ABC"}"#,
        );
        assert_eq!(apply(&captures, &r, &mut vars), 3);
        assert_eq!(vars.get("orderId"), Some("42"));
        assert_eq!(vars.get("etag"), Some("\"v1\""));
        assert_eq!(vars.get("token"), Some("abc123"));
        assert_eq!(vars.get("whole"), Some("ABC"));
        // A miss leaves the variable as it was.
        assert_eq!(vars.get("missing"), Some("from row"));
        assert_eq!(vars.get("absent"), None);

        // A body that is not JSON: JSON captures miss, the others still work.
        let r = response(&[("ETag", "v2")], "token=zzz");
        assert_eq!(apply(&captures, &r, &mut vars), 5);
        assert_eq!((vars.get("orderId"), vars.get("etag"), vars.get("token")), (Some("42"), Some("v2"), Some("zzz")));

        let err = |from, variable: &str, path: &str| {
            Capture::new(&LoadCapture { variable: variable.into(), from, path: path.into() }).unwrap_err()
        };
        assert_eq!(err(CaptureFrom::Json, " ", "$.id"), "a capture has no variable name");
        assert!(err(CaptureFrom::Json, "a b", "$.id").contains("no spaces"));
        assert!(err(CaptureFrom::Regex, "x", "(").contains("invalid regular expression"));
        assert!(err(CaptureFrom::Header, "x", " ").contains("header name"));
        assert!(err(CaptureFrom::Json, "x", "$..x").starts_with("capture 'x': '$..x' is not a JSON path"));
    }

    #[test]
    fn values_over_the_limit_are_misses() {
        let captures = [
            capture(CaptureFrom::Json, "id", "$.id"),
            capture(CaptureFrom::Header, "token", "X-Token"),
            capture(CaptureFrom::Regex, "blob", "blob=(x+)"),
        ];
        let mut vars = UserVars::new(None);
        let fits = "x".repeat(MAX_CAPTURED);
        let body = format!(r#"{{"id": "{fits}", "note": "blob={fits}"}}"#);
        assert_eq!(apply(&captures, &response(&[("x-token", &fits)], &body), &mut vars), 0);
        assert!(["id", "token", "blob"].iter().all(|v| vars.get(v) == Some(fits.as_str())));

        // One byte more: each capture misses and the user keeps the value it had.
        let over = "y".repeat(MAX_CAPTURED + 1);
        let body = format!(r#"{{"id": "{over}", "note": "blob={}"}}"#, "x".repeat(MAX_CAPTURED + 1));
        assert_eq!(apply(&captures, &response(&[("x-token", &over)], &body), &mut vars), 3);
        assert!(["id", "token", "blob"].iter().all(|v| vars.get(v) == Some(fits.as_str())));
    }

    #[test]
    fn user_vars_prefer_captured_values() {
        let row: DataRow = vec![("user".to_string(), "ada".to_string()), ("id".to_string(), "1".to_string())].into();
        let mut vars = UserVars::new(Some(row));
        assert_eq!(vars.get("user"), Some("ada"));
        vars.set("id", "99".into());
        vars.set("token", "t".into());
        assert_eq!(vars.get("id"), Some("99"));
        let all: Vec<(&str, &str)> = vars.iter().collect();
        assert_eq!(all, [("id", "99"), ("token", "t"), ("user", "ada")]);
        assert!(UserVars::default().is_empty() && !vars.is_empty());
        assert_eq!(UserVars::probe(&["a".into()]).get("a"), Some(PROBE));
    }

    #[test]
    fn server_timing_header() {
        assert_eq!(server_timing_ms("total;dur=12.3"), Some(12.3));
        assert!((server_timing_ms("db;dur=5, app;dur=7.1").unwrap() - 12.1).abs() < 1e-9);
        assert_eq!(server_timing_ms("db;dur=5, total;dur=20, app;dur=7"), Some(20.0));
        assert_eq!(server_timing_ms("TOTAL;DUR=\"3\""), Some(3.0));
        assert_eq!(server_timing_ms(r#"cache;desc="Hit, fast; really";dur=2.5, miss"#), Some(2.5));
        assert_eq!(server_timing_ms("db;desc=\"no duration\""), None);
        for garbage in ["", "garbage", ";;;", "db;dur=abc", "db;dur=-4", "db;dur=NaN", "=,=;", "db;dur"] {
            assert_eq!(server_timing_ms(garbage), None, "{garbage}");
        }
    }
}

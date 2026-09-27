//! Patterns that check what the learner did, written in lab files as plain YAML values.
//!
//! * An object matches when every key's pattern matches the value under that key
//!   (keys compare without case). A missing key only matches `null` or a `!` pattern.
//! * An array matches when every pattern in it matches some element of the array.
//! * A string pattern compares with the text of the value (numbers and booleans too), without case:
//!   - `*` anything that is there; `abc*`, `*abc*` wildcards;
//!   - `re:<regex>` a regular expression (case-sensitive unless it says `(?i)`);
//!   - `>=10`, `<200`, `>0`, `<=5`, `!=3` numbers; `2xx`, `4xx` HTTP status classes;
//!   - `!<pattern>` anything the pattern does not match, including a missing value.
//! * A number or boolean matches the same number or boolean (or its text).

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use regex::Regex;
use serde_json::Value;

/// Whether `actual` (possibly missing) matches `pattern`.
pub fn matches(pattern: &Value, actual: Option<&Value>) -> bool {
    match pattern {
        Value::Null => actual.is_none_or(Value::is_null),
        Value::String(p) => match p.strip_prefix('!') {
            // `!=3` is a comparison, not "not =3".
            Some(rest) if !rest.is_empty() && !rest.starts_with('=') => {
                !matches(&Value::String(rest.to_string()), actual)
            }
            _ => actual.is_some_and(|a| text_matches(p, a)),
        },
        Value::Bool(b) => actual.is_some_and(|a| match a {
            Value::Bool(x) => x == b,
            Value::String(s) => s.eq_ignore_ascii_case(&b.to_string()),
            _ => false,
        }),
        Value::Number(n) => actual.is_some_and(|a| match (a, n.as_f64()) {
            (Value::Number(x), Some(n)) => x.as_f64() == Some(n),
            (Value::String(s), Some(n)) => s.trim().parse::<f64>().ok() == Some(n),
            _ => false,
        }),
        Value::Array(patterns) => match actual {
            Some(Value::Array(items)) => patterns.iter().all(|p| items.iter().any(|i| matches(p, Some(i)))),
            _ => false,
        },
        Value::Object(fields) => match actual {
            Some(Value::Object(obj)) => fields.iter().all(|(k, p)| matches(p, get_key(obj, k))),
            _ => false,
        },
    }
}

/// A key's value, compared without case.
fn get_key<'a>(obj: &'a serde_json::Map<String, Value>, key: &str) -> Option<&'a Value> {
    obj.get(key).or_else(|| obj.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v))
}

/// The text of a scalar (objects and arrays as JSON).
pub fn text_of(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// Whether a string pattern matches a value.
pub fn text_matches(pattern: &str, actual: &Value) -> bool {
    if actual.is_null() {
        return false;
    }
    let text = text_of(actual);
    if pattern == "*" {
        return true;
    }
    if let Some(re) = pattern.strip_prefix("re:") {
        return regex(re).is_some_and(|r| r.is_match(&text));
    }
    if let Some(ok) = compare(pattern, &text) {
        return ok;
    }
    if let Some(class) = status_class(pattern) {
        return text.trim().parse::<u16>().is_ok_and(|s| s / 100 == class);
    }
    if pattern.contains('*') {
        return glob(&pattern.to_lowercase(), &text.to_lowercase());
    }
    pattern.trim().eq_ignore_ascii_case(text.trim())
}

/// A compiled `re:` pattern (compiled once: checks run every second while a lab runs).
fn regex(source: &str) -> Option<Regex> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<Regex>>>> = OnceLock::new();
    let mut cache = CACHE.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    if cache.len() > 256 {
        cache.clear();
    }
    cache.entry(source.to_string()).or_insert_with(|| Regex::new(source).ok()).clone()
}

/// `>=10`-style comparisons; `None` when `pattern` is not one.
fn compare(pattern: &str, text: &str) -> Option<bool> {
    let (op, rest) = ["<=", ">=", "!=", "<", ">"].iter().find_map(|op| pattern.strip_prefix(op).map(|r| (*op, r)))?;
    let bound: f64 = rest.trim().parse().ok()?;
    let Ok(value) = text.trim().parse::<f64>() else { return Some(false) };
    Some(match op {
        "<=" => value <= bound,
        ">=" => value >= bound,
        "!=" => value != bound,
        "<" => value < bound,
        _ => value > bound,
    })
}

/// `4xx` → 4.
fn status_class(pattern: &str) -> Option<u16> {
    let b = pattern.as_bytes();
    (b.len() == 3 && b[0].is_ascii_digit() && b[1..].eq_ignore_ascii_case(b"xx")).then(|| u16::from(b[0] - b'0'))
}

/// `*` matches any run of characters.
fn glob(pattern: &str, text: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    let (first, last) = (parts[0], parts[parts.len() - 1]);
    if !text.starts_with(first) || text.len() < first.len() + last.len() || !text.ends_with(last) {
        return false;
    }
    let mut at = first.len();
    let end = text.len() - last.len();
    for part in &parts[1..parts.len() - 1] {
        match text[at..end].find(part) {
            Some(i) => at += i + part.len(),
            None => return false,
        }
    }
    true
}

/// Replace `{{name}}` placeholders in every string of `value` (unknown names stay).
pub fn substitute(value: &Value, lookup: &dyn Fn(&str) -> Option<String>) -> Value {
    match value {
        Value::String(s) => Value::String(substitute_text(s, lookup)),
        Value::Array(items) => Value::Array(items.iter().map(|v| substitute(v, lookup)).collect()),
        Value::Object(obj) => Value::Object(obj.iter().map(|(k, v)| (k.clone(), substitute(v, lookup))).collect()),
        other => other.clone(),
    }
}

pub fn substitute_text(text: &str, lookup: &dyn Fn(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        match after.find("}}") {
            Some(end) => {
                let name = after[..end].trim();
                match lookup(name) {
                    Some(v) => out.push_str(&v),
                    None => out.push_str(&rest[start..start + 2 + end + 2]),
                }
                rest = &after[end + 2..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn m(p: Value, a: Value) -> bool {
        matches(&p, Some(&a))
    }

    #[test]
    fn scalars() {
        assert!(m(json!("GET"), json!("get")));
        assert!(!m(json!("GET"), json!("POST")));
        assert!(m(json!(200), json!(200)));
        assert!(m(json!(200), json!("200")));
        assert!(m(json!("200"), json!(200)));
        assert!(m(json!(true), json!(true)));
        assert!(m(json!("4xx"), json!(404)));
        assert!(!m(json!("4xx"), json!(500)));
        assert!(m(json!(">=2"), json!(3)));
        assert!(!m(json!("<100"), json!(150)));
        assert!(!m(json!(">1"), json!("abc")));
        assert!(m(json!("Bearer *"), json!("bearer abc")));
        assert!(m(json!("*hello*"), json!("say Hello there")));
        assert!(!m(json!("a*b*c"), json!("acb")));
        assert!(m(json!("a*b*c"), json!("axxbyyc")));
        assert!(!m(json!("ab*ba"), json!("aba")));
        assert!(m(json!("re:^/users/\\d+$"), json!("/users/42")));
        assert!(m(json!("*"), json!(0)));
        assert!(!m(json!("*"), Value::Null));
    }

    #[test]
    fn missing_and_negation() {
        assert!(matches(&json!(null), None));
        assert!(!matches(&json!("*"), None));
        assert!(matches(&json!("!*"), None));
        assert!(!m(json!("!*"), json!("x")));
        assert!(m(json!("!GET"), json!("POST")));
        assert!(!m(json!("!=0"), json!(0)));
        assert!(m(json!("!=0"), json!(2)));
        assert!(!m(json!("!=3"), json!("3")));
        assert!(m(json!({"auth": "!*"}), json!({"x": 1})));
    }

    #[test]
    fn objects_and_arrays() {
        let actual =
            json!({"method": "POST", "Headers": {"content-type": "application/json"}, "tags": ["a", "b"], "n": 3});
        assert!(m(json!({"method": "post", "headers": {"Content-Type": "application/json*"}}), actual.clone()));
        assert!(m(json!({"tags": ["b"]}), actual.clone()));
        assert!(!m(json!({"tags": ["c"]}), actual.clone()));
        assert!(!m(json!({"missing": "*"}), actual.clone()));
        assert!(m(json!({"items": [{"id": 2}]}), json!({"items": [{"id": 1}, {"id": 2, "x": 0}]})));
    }

    #[test]
    fn substitutes() {
        let lookup = |n: &str| (n == "api").then(|| "http://127.0.0.1:1".to_string());
        assert_eq!(
            substitute_text("{{api}}/x {{ api }} {{other}} {{", &lookup),
            "http://127.0.0.1:1/x http://127.0.0.1:1 {{other}} {{"
        );
        assert_eq!(substitute(&json!({"u": ["{{api}}"]}), &lookup), json!({"u": ["http://127.0.0.1:1"]}));
    }
}

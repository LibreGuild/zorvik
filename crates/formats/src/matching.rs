//! Matching messages: the JSON paths that pick one value (`$.items[0].id`),
//! and [`MessageMatcher`], which decides whether a message matches a pattern
//! (any, contains, exact, regex, or a JSON condition such as
//! `$.type == "welcome"`). Server reply rules, WebSocket wait steps and the
//! agents' waits all match with it.

use serde_json::Value;

use crate::server::MatchKind;

/// One step of a JSON path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathStep {
    Key(String),
    /// From the end when negative (`-1` is the last item).
    Index(i64),
}

/// The subset of JSONPath that picks one value: `$`, `.name`, `['name']` or
/// `["name"]`, and `[n]` (`[-1]` from the end). The leading `$` is optional.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonPath(pub Vec<PathStep>);

impl JsonPath {
    pub fn parse(path: &str) -> Result<Self, String> {
        let bad = |why: &str| format!("'{path}' is not a JSON path ({why}); use one like $.items[0].id");
        let mut rest = path.trim();
        if rest.is_empty() {
            return Err("give a JSON path, e.g. $.id".into());
        }
        rest = rest.strip_prefix('$').unwrap_or(rest);
        let mut steps = Vec::new();
        // Without `$`, the path may start with a name: `data.id`.
        let mut first = !path.trim().starts_with('$');
        while !rest.is_empty() {
            if let Some(after) = rest.strip_prefix('[') {
                let end = bracket_end(after).ok_or_else(|| bad("a [ is never closed"))?;
                let inner = after[..end].trim();
                rest = &after[end + 1..];
                if let Some(quote) = inner.chars().next().filter(|c| *c == '\'' || *c == '"') {
                    let key = inner
                        .strip_prefix(quote)
                        .and_then(|k| k.strip_suffix(quote))
                        .ok_or_else(|| bad("a quoted name is not closed"))?;
                    steps.push(PathStep::Key(key.replace(&format!("\\{quote}"), &quote.to_string())));
                } else {
                    let index = inner.parse::<i64>().map_err(|_| bad("[…] needs a number or a quoted name"))?;
                    steps.push(PathStep::Index(index));
                }
            } else {
                let after = match rest.strip_prefix('.') {
                    Some(after) => after,
                    None if first => rest,
                    None => return Err(bad("expected . or [")),
                };
                if after.starts_with('.') {
                    return Err(bad("wildcards and .. are not supported"));
                }
                let end = after.find(['.', '[']).unwrap_or(after.len());
                let key = after[..end].trim();
                if key.is_empty() {
                    return Err(bad("a name is empty"));
                }
                if key == "*" {
                    return Err(bad("wildcards and .. are not supported"));
                }
                steps.push(PathStep::Key(key.to_string()));
                rest = &after[end..];
            }
            first = false;
        }
        Ok(Self(steps))
    }

    /// The value at the path, if there is one.
    pub fn get<'a>(&self, root: &'a Value) -> Option<&'a Value> {
        let mut at = root;
        for step in &self.0 {
            at = match (step, at) {
                (PathStep::Key(k), Value::Object(map)) => map.get(k)?,
                (PathStep::Index(i), Value::Array(items)) => {
                    let i = if *i < 0 { items.len().checked_sub(i.unsigned_abs() as usize)? } else { *i as usize };
                    items.get(i)?
                }
                _ => return None,
            };
        }
        Some(at)
    }

    /// The value at the path as text: strings as they are, numbers and
    /// booleans as text, objects and arrays as JSON. `null` or nothing: `None`.
    pub fn text(&self, root: &Value) -> Option<String> {
        self.get(root).and_then(value_text)
    }
}

/// A JSON value as text (see [`JsonPath::text`]).
pub fn value_text(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        // Like JavaScript writes numbers (the app's filters compare the same way): 7.0 is "7".
        Value::Number(n) if !n.is_i64() && !n.is_u64() => n.as_f64().map(|f| f.to_string()),
        other => Some(other.to_string()),
    }
}

/// Position of the `]` that closes a bracket, skipping quoted text.
fn bracket_end(s: &str) -> Option<usize> {
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for (i, c) in s.char_indices() {
        match quote {
            Some(_) if escaped => escaped = false,
            Some(_) if c == '\\' => escaped = true,
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '\'' || c == '"' => quote = Some(c),
            None if c == ']' => return Some(i),
            None => {}
        }
    }
    None
}

/// A JSON condition: `$.path` (the value is there and not null), `$.path == value`
/// or `$.path != value`. The value is JSON (`"text"`, `7`, `true`, `null`) or bare text.
#[derive(Debug, Clone)]
struct JsonCondition {
    path: JsonPath,
    /// `(equal, expected text)`; `None` for "is there". Expected `None` is `null`.
    compare: Option<(bool, Option<String>)>,
}

impl JsonCondition {
    fn parse(pattern: &str) -> Result<Self, String> {
        let (path, compare) = match split_operator(pattern) {
            Some((path, equal, value)) => {
                let value = value.trim();
                let expected = match serde_json::from_str::<Value>(value) {
                    Ok(v) => value_text(&v),
                    // 'text' in single quotes, like the paths take.
                    Err(_) => match value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')) {
                        Some(inner) => Some(inner.to_string()),
                        None => Some(value.to_string()),
                    },
                };
                (path, Some((equal, expected)))
            }
            None => (pattern, None),
        };
        Ok(Self { path: JsonPath::parse(path)?, compare })
    }

    fn matches(&self, root: &Value) -> bool {
        let actual = self.path.text(root);
        match &self.compare {
            None => actual.is_some(),
            Some((equal, expected)) => (&actual == expected) == *equal,
        }
    }
}

/// `path == value` or `path != value`, outside quoted names in the path.
fn split_operator(pattern: &str) -> Option<(&str, bool, &str)> {
    let mut quote: Option<char> = None;
    let bytes = pattern.as_bytes();
    for (i, c) in pattern.char_indices() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '\'' || c == '"' => quote = Some(c),
            None if (c == '=' || c == '!') && bytes.get(i + 1) == Some(&b'=') => {
                return Some((&pattern[..i], c == '=', &pattern[i + 2..]));
            }
            None => {}
        }
    }
    None
}

/// Whether a message matches a pattern, compiled once.
#[derive(Debug, Clone)]
pub struct MessageMatcher {
    how: How,
}

#[derive(Debug, Clone)]
enum How {
    Any,
    Contains(Vec<u8>),
    Exact(Vec<u8>),
    Regex(regex::bytes::Regex),
    Json(JsonCondition),
}

impl MessageMatcher {
    /// `pattern` is text (or raw bytes, for hex rules). The error says what's wrong with it.
    pub fn new(kind: MatchKind, pattern: &[u8]) -> Result<Self, String> {
        let text = || String::from_utf8_lossy(pattern).into_owned();
        let how = match kind {
            MatchKind::Any => How::Any,
            MatchKind::Contains => How::Contains(pattern.to_vec()),
            MatchKind::Exact => How::Exact(trim_eol(pattern).to_vec()),
            MatchKind::Regex => {
                How::Regex(regex::bytes::Regex::new(&text()).map_err(|e| format!("invalid regex: {e}"))?)
            }
            MatchKind::Json => How::Json(JsonCondition::parse(&text())?),
        };
        Ok(Self { how })
    }

    pub fn matches(&self, message: &[u8]) -> bool {
        match &self.how {
            How::Any => true,
            How::Contains(p) => p.is_empty() || message.windows(p.len()).any(|w| w == p.as_slice()),
            How::Exact(p) => trim_eol(message) == p.as_slice(),
            How::Regex(re) => re.is_match(message),
            How::Json(condition) => {
                serde_json::from_slice::<Value>(trim_eol(message)).is_ok_and(|root| condition.matches(&root))
            }
        }
    }

    /// Whether an already parsed JSON message matches a JSON condition (parse the message
    /// once, then check several conditions). `None` for the other kinds, which match text.
    pub fn matches_json(&self, root: &Value) -> Option<bool> {
        match &self.how {
            How::Json(condition) => Some(condition.matches(root)),
            _ => None,
        }
    }
}

/// The message without a trailing line ending (lines from TCP clients end with one).
pub fn trim_eol(bytes: &[u8]) -> &[u8] {
    let end = bytes.iter().rposition(|b| !matches!(b, b'\n' | b'\r')).map_or(0, |i| i + 1);
    &bytes[..end]
}

/// A short description of a pattern for logs and test names: `contains "hello"`,
/// `$.type == "welcome"`, `any message`.
pub fn describe(kind: MatchKind, pattern: &str) -> String {
    match kind {
        MatchKind::Any => "any message".into(),
        MatchKind::Contains => format!("a message containing \"{pattern}\""),
        MatchKind::Exact => format!("the message \"{pattern}\""),
        MatchKind::Regex => format!("a message matching /{pattern}/"),
        MatchKind::Json => format!("a JSON message where {}", pattern.trim()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matches(kind: MatchKind, pattern: &str, message: &str) -> bool {
        MessageMatcher::new(kind, pattern.as_bytes()).unwrap().matches(message.as_bytes())
    }

    #[test]
    fn json_paths() {
        let doc: Value = serde_json::from_str(
            r#"{"id": 7, "name": "Ada", "ok": true, "none": null,
                "items": [{"id": "a"}, {"id": "b", "tags": ["x", "y"]}],
                "odd key": {"a.b": 1}, "nested": {"list": [1, 2]}}"#,
        )
        .unwrap();
        let get = |path: &str| JsonPath::parse(path).unwrap().text(&doc);
        assert_eq!(get("$.id").as_deref(), Some("7"));
        assert_eq!(get("$.name").as_deref(), Some("Ada"));
        assert_eq!(get("name").as_deref(), Some("Ada"));
        assert_eq!(get("$.ok").as_deref(), Some("true"));
        assert_eq!(get("$.items[0].id").as_deref(), Some("a"));
        assert_eq!(get("$.items[-1].tags[1]").as_deref(), Some("y"));
        assert_eq!(get("items[1].id").as_deref(), Some("b"));
        assert_eq!(get("$['odd key'][\"a.b\"]").as_deref(), Some("1"));
        assert_eq!(get("$.nested.list").as_deref(), Some("[1,2]"));
        assert_eq!(get("$.items[1].tags").as_deref(), Some(r#"["x","y"]"#));
        // Nothing there, or null.
        for path in ["$.missing", "$.none", "$.items[5]", "$.items[-3]", "$.id.x", "$.name[0]", "$.items.id"] {
            assert_eq!(get(path), None, "{path}");
        }
        // The whole document.
        assert!(get("$").unwrap().starts_with('{'));
        for bad in ["", "$.", "$..id", "$.a[", "$.a[x]", "$.*", "$['a]", "$a"] {
            assert!(JsonPath::parse(bad).is_err(), "{bad}");
        }
        assert!(JsonPath::parse("$..id").unwrap_err().contains("wildcards and .. are not supported"));
        assert_eq!(
            JsonPath::parse("$.a[0]['b c']").unwrap().0,
            [PathStep::Key("a".into()), PathStep::Index(0), PathStep::Key("b c".into())]
        );
    }

    #[test]
    fn text_kinds() {
        assert!(matches(MatchKind::Any, "", "anything"));
        assert!(matches(MatchKind::Contains, "llo", "hello"));
        assert!(matches(MatchKind::Contains, "", "hello"));
        assert!(!matches(MatchKind::Contains, "bye", "hello"));
        assert!(matches(MatchKind::Exact, "ping", "ping\r\n"));
        assert!(matches(MatchKind::Exact, "ping", "ping\n\n"));
        assert!(!matches(MatchKind::Exact, "ping", "ping pong"));
        assert!(matches(MatchKind::Regex, "(?i)^HEL+o$", "hello"));
        assert!(MessageMatcher::new(MatchKind::Regex, b"(").is_err());
    }

    #[test]
    fn json_conditions() {
        let msg = r#"{"type":"welcome","id":7,"user":{"name":"Ada"},"tags":["a"],"gone":null}"#;
        assert!(matches(MatchKind::Json, "$.type == \"welcome\"", msg));
        assert!(matches(MatchKind::Json, "$.type == welcome", msg));
        assert!(matches(MatchKind::Json, "$.type == 'welcome'", msg));
        assert!(matches(MatchKind::Json, "$.id == 7", msg));
        assert!(matches(MatchKind::Json, "$.n == 7", r#"{"n": 7.0}"#));
        assert!(matches(MatchKind::Json, "$.n == 1.5", r#"{"n": 1.50}"#));
        assert!(matches(MatchKind::Json, "$.id==\"7\"", msg));
        assert!(matches(MatchKind::Json, "$.user.name != Bob", msg));
        assert!(matches(MatchKind::Json, "$.tags[0]", msg));
        assert!(matches(MatchKind::Json, "$.gone == null", msg));
        assert!(matches(MatchKind::Json, "$.missing == null", msg));
        assert!(!matches(MatchKind::Json, "$.gone", msg));
        assert!(!matches(MatchKind::Json, "$.type == bye", msg));
        assert!(!matches(MatchKind::Json, "$.type", "not json"));
        assert!(matches(MatchKind::Json, "$['a == b'] == 1", r#"{"a == b":1}"#));
        assert!(MessageMatcher::new(MatchKind::Json, b"$..x == 1").is_err());
    }

    #[test]
    fn json_conditions_on_parsed_values() {
        let root: Value = serde_json::from_str(r#"{"type":"welcome","id":7}"#).unwrap();
        let json = |pattern: &str| MessageMatcher::new(MatchKind::Json, pattern.as_bytes()).unwrap();
        assert_eq!(json("$.type == welcome").matches_json(&root), Some(true));
        assert_eq!(json("$.id != 7").matches_json(&root), Some(false));
        assert_eq!(json("$.missing").matches_json(&root), Some(false));
        // Text kinds don't look at parsed values.
        assert_eq!(MessageMatcher::new(MatchKind::Any, b"").unwrap().matches_json(&root), None);
    }
}

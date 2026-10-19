//! JSON compare: the differences between two JSON documents, by path. Key order never
//! matters; array items are aligned like lines in a text diff (an item inserted at the
//! start is one addition, not every item changed), or matched in any order on request.
//! Numbers compare by value (`1` equals `1.0`).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Number, Value};
use ts_rs::TS;

/// Differences listed; the counts cover all of them.
pub const MAX_DIFFERENCES: usize = 1000;
/// Size of each document.
pub const MAX_INPUT: usize = 20 * 1024 * 1024;
/// Characters of a value shown next to a difference.
const MAX_PREVIEW: usize = 200;
/// Cells of the table that aligns two arrays (after common items at both ends are set
/// aside); longer arrays are compared item by item at the same index.
const MAX_ALIGN_CELLS: usize = 4_000_000;

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JsonDiffOptions {
    /// Match array items in any order: an item counts as unchanged when the other array
    /// has an equal one anywhere.
    #[serde(default)]
    pub ignore_array_order: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum JsonDiffKind {
    /// Only in the right document.
    Added,
    /// Only in the left document.
    Removed,
    /// A different value (or type) on each side.
    Changed,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct JsonDifference {
    /// JSONPath of the value, e.g. `$.items[2].name` or `$['odd key']`. Array indexes are
    /// positions in the left document, except for added items (positions in the right one).
    pub path: String,
    pub kind: JsonDiffKind,
    /// The left value as compact JSON (shortened), for removed and changed values.
    pub left: Option<String>,
    /// The right value as compact JSON (shortened), for added and changed values.
    pub right: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct JsonDiff {
    /// In document order, at most [`MAX_DIFFERENCES`].
    pub differences: Vec<JsonDifference>,
    pub added: u32,
    pub removed: u32,
    pub changed: u32,
    /// There are more differences than listed.
    pub cut: bool,
}

/// Compare two JSON documents. Errors are plain sentences naming the side.
pub fn diff_json(left: &str, right: &str, options: JsonDiffOptions) -> Result<JsonDiff, String> {
    let a = parse("left", left)?;
    let b = parse("right", right)?;
    let mut out = Collector { options, ..Default::default() };
    let mut path = String::from("$");
    out.compare(&a, &b, &mut path);
    Ok(JsonDiff {
        cut: out.total() > out.differences.len(),
        differences: out.differences,
        added: out.added,
        removed: out.removed,
        changed: out.changed,
    })
}

fn parse(side: &str, text: &str) -> Result<Value, String> {
    if text.len() > MAX_INPUT {
        return Err(format!("The {side} document is larger than {} MB", MAX_INPUT / (1024 * 1024)));
    }
    let text = text.trim_start_matches('\u{feff}');
    if text.trim().is_empty() {
        return Err(format!("The {side} document is empty"));
    }
    serde_json::from_str(text).map_err(|e| format!("The {side} document isn't valid JSON: {e}"))
}

#[derive(Default)]
struct Collector {
    options: JsonDiffOptions,
    differences: Vec<JsonDifference>,
    added: u32,
    removed: u32,
    changed: u32,
}

impl Collector {
    fn total(&self) -> usize {
        (self.added + self.removed + self.changed) as usize
    }

    fn push(&mut self, kind: JsonDiffKind, path: &str, left: Option<&Value>, right: Option<&Value>) {
        match kind {
            JsonDiffKind::Added => self.added += 1,
            JsonDiffKind::Removed => self.removed += 1,
            JsonDiffKind::Changed => self.changed += 1,
        }
        if self.differences.len() < MAX_DIFFERENCES {
            self.differences.push(JsonDifference {
                path: path.to_string(),
                kind,
                left: left.map(preview),
                right: right.map(preview),
            });
        }
    }

    fn compare(&mut self, a: &Value, b: &Value, path: &mut String) {
        match (a, b) {
            (Value::Object(x), Value::Object(y)) => self.objects(x, y, path),
            (Value::Array(x), Value::Array(y)) => {
                if self.options.ignore_array_order {
                    self.arrays_any_order(x, y, path)
                } else {
                    self.arrays(x, y, path)
                }
            }
            _ if same(a, b) => {}
            _ => self.push(JsonDiffKind::Changed, path, Some(a), Some(b)),
        }
    }

    fn objects(&mut self, x: &Map<String, Value>, y: &Map<String, Value>, path: &mut String) {
        // Left keys in order (removed and changed ones), then keys only on the right.
        for (key, a) in x {
            let len = path.len();
            push_key(path, key);
            match y.get(key) {
                Some(b) => self.compare(a, b, path),
                None => self.push(JsonDiffKind::Removed, path, Some(a), None),
            }
            path.truncate(len);
        }
        for (key, b) in y {
            if !x.contains_key(key) {
                let len = path.len();
                push_key(path, key);
                self.push(JsonDiffKind::Added, path, None, Some(b));
                path.truncate(len);
            }
        }
    }

    fn arrays(&mut self, x: &[Value], y: &[Value], path: &mut String) {
        // Equal items at both ends need no alignment.
        let head = x.iter().zip(y).take_while(|(a, b)| same(a, b)).count();
        let tail = x[head..].iter().rev().zip(y[head..].iter().rev()).take_while(|(a, b)| same(a, b)).count();
        let (xs, ys) = (&x[head..x.len() - tail], &y[head..y.len() - tail]);
        if xs.is_empty() && ys.is_empty() {
            return;
        }
        let ops = if !xs.is_empty() && !ys.is_empty() && xs.len().saturating_mul(ys.len()) <= MAX_ALIGN_CELLS {
            align(xs, ys)
        } else {
            // Only additions or only removals, or too long to align: item by item.
            let n = xs.len().max(ys.len());
            let mut ops = Vec::with_capacity(n * 2);
            for i in 0..n {
                if i < xs.len() {
                    ops.push(Op::Remove(i));
                }
                if i < ys.len() {
                    ops.push(Op::Add(i));
                }
            }
            ops
        };
        // Between items that stayed the same, removed and added items are paired up and
        // compared (a changed item); the rest are removed or added.
        let mut removed = Vec::new();
        let mut added = Vec::new();
        for op in ops.into_iter().chain([Op::Keep]) {
            match op {
                Op::Remove(i) => removed.push(i),
                Op::Add(j) => added.push(j),
                Op::Keep => {
                    let pairs = removed.len().min(added.len());
                    for k in 0..pairs {
                        let (i, j) = (removed[k], added[k]);
                        let len = path.len();
                        push_index(path, head + i);
                        self.compare(&xs[i], &ys[j], path);
                        path.truncate(len);
                    }
                    for &i in &removed[pairs..] {
                        let len = path.len();
                        push_index(path, head + i);
                        self.push(JsonDiffKind::Removed, path, Some(&xs[i]), None);
                        path.truncate(len);
                    }
                    for &j in &added[pairs..] {
                        let len = path.len();
                        push_index(path, head + j);
                        self.push(JsonDiffKind::Added, path, None, Some(&ys[j]));
                        path.truncate(len);
                    }
                    removed.clear();
                    added.clear();
                }
            }
        }
    }

    fn arrays_any_order(&mut self, x: &[Value], y: &[Value], path: &mut String) {
        let mut pool: HashMap<String, Vec<usize>> = HashMap::new();
        for (j, b) in y.iter().enumerate().rev() {
            pool.entry(canonical(b)).or_default().push(j);
        }
        let mut matched = vec![false; y.len()];
        for (i, a) in x.iter().enumerate() {
            match pool.get_mut(&canonical(a)).and_then(Vec::pop) {
                Some(j) => matched[j] = true,
                None => {
                    let len = path.len();
                    push_index(path, i);
                    self.push(JsonDiffKind::Removed, path, Some(a), None);
                    path.truncate(len);
                }
            }
        }
        for (j, b) in y.iter().enumerate().filter(|(j, _)| !matched[*j]) {
            let len = path.len();
            push_index(path, j);
            self.push(JsonDiffKind::Added, path, None, Some(b));
            path.truncate(len);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Keep,
    Remove(usize),
    Add(usize),
}

/// The longest common subsequence of two arrays, as keep / remove / add steps in order.
fn align(xs: &[Value], ys: &[Value]) -> Vec<Op> {
    // Items compare as small ids: equal items (by value) get the same one.
    let mut ids: HashMap<String, u32> = HashMap::new();
    let mut id = |v: &Value| {
        let next = ids.len() as u32;
        *ids.entry(canonical(v)).or_insert(next)
    };
    let a: Vec<u32> = xs.iter().map(&mut id).collect();
    let b: Vec<u32> = ys.iter().map(&mut id).collect();
    let (n, m) = (a.len(), b.len());
    // lcs[i][j]: length of the common subsequence of a[i..] and b[j..]. At most
    // MAX_ALIGN_CELLS cells, so the shorter side (and every value) fits in u16.
    let width = m + 1;
    let mut lcs = vec![0u16; (n + 1) * width];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i * width + j] = if a[i] == b[j] {
                lcs[(i + 1) * width + j + 1] + 1
            } else {
                lcs[(i + 1) * width + j].max(lcs[i * width + j + 1])
            };
        }
    }
    let mut ops = Vec::with_capacity(n + m);
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if a[i] == b[j] {
            ops.push(Op::Keep);
            i += 1;
            j += 1;
        } else if lcs[(i + 1) * width + j] >= lcs[i * width + j + 1] {
            ops.push(Op::Remove(i));
            i += 1;
        } else {
            ops.push(Op::Add(j));
            j += 1;
        }
    }
    ops.extend((i..n).map(Op::Remove));
    ops.extend((j..m).map(Op::Add));
    ops
}

/// Equal JSON values: objects in any key order, numbers by value.
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => same_number(x, y),
        (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(a, b)| same(a, b)),
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len() && x.iter().all(|(k, a)| y.get(k).is_some_and(|b| same(a, b)))
        }
        _ => a == b,
    }
}

fn same_number(x: &Number, y: &Number) -> bool {
    if x.is_f64() || y.is_f64() { x.as_f64() == y.as_f64() } else { x == y }
}

/// A text that is the same for values [`same`] calls equal: keys sorted, whole numbers
/// written without a fraction.
fn canonical(v: &Value) -> String {
    let mut out = String::new();
    write_canonical(v, &mut out);
    out
}

fn write_canonical(v: &Value, out: &mut String) {
    match v {
        Value::Number(n) => match n.as_f64() {
            Some(f) if n.is_f64() && f.fract() == 0.0 && f.abs() < 9_007_199_254_740_992.0 => {
                out.push_str(&(f as i64).to_string())
            }
            _ => out.push_str(&n.to_string()),
        },
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_canonical(item, out);
            }
            out.push(']');
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (i, key) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&Value::String(key.clone()).to_string());
                out.push(':');
                write_canonical(&map[key], out);
            }
            out.push('}');
        }
        other => out.push_str(&other.to_string()),
    }
}

/// `.name` when the key is a plain name, else `['odd key']` (RFC 9535 JSONPath).
fn push_key(path: &mut String, key: &str) {
    let plain = key.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if plain {
        path.push('.');
        path.push_str(key);
    } else {
        path.push_str("['");
        for c in key.chars() {
            match c {
                '\'' => path.push_str("\\'"),
                '\\' => path.push_str("\\\\"),
                '\n' => path.push_str("\\n"),
                '\r' => path.push_str("\\r"),
                '\t' => path.push_str("\\t"),
                c if (c as u32) < 0x20 => path.push_str(&format!("\\u{:04x}", c as u32)),
                c => path.push(c),
            }
        }
        path.push_str("']");
    }
}

fn push_index(path: &mut String, index: usize) {
    path.push('[');
    path.push_str(&index.to_string());
    path.push(']');
}

/// Compact JSON, cut to [`MAX_PREVIEW`] characters.
fn preview(v: &Value) -> String {
    let text = v.to_string();
    match text.char_indices().nth(MAX_PREVIEW) {
        Some((at, _)) => format!("{}…", &text[..at]),
        None => text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn diff(a: &str, b: &str) -> JsonDiff {
        diff_json(a, b, JsonDiffOptions::default()).unwrap()
    }

    fn lines(d: &JsonDiff) -> Vec<String> {
        d.differences
            .iter()
            .map(|x| {
                let kind = match x.kind {
                    JsonDiffKind::Added => "+",
                    JsonDiffKind::Removed => "-",
                    JsonDiffKind::Changed => "~",
                };
                format!("{kind} {} {} {}", x.path, x.left.as_deref().unwrap_or("_"), x.right.as_deref().unwrap_or("_"))
            })
            .collect()
    }

    #[test]
    fn equal_documents_ignore_key_order_whitespace_and_number_form() {
        let d = diff(r#"{"a": 1, "b": [1, 2.0, {"x": null}], "c": "s"}"#, r#"{"c":"s","b":[1.0,2,{"x":null}],"a":1}"#);
        assert_eq!(d, JsonDiff { differences: vec![], added: 0, removed: 0, changed: 0, cut: false });
        assert!(diff("1e2", "100").differences.is_empty());
        // Whole numbers too large for a double stay exact.
        assert_eq!(diff("9007199254740993", "9007199254740992").changed, 1);
    }

    #[test]
    fn lists_added_removed_and_changed_paths() {
        let d = diff(
            r#"{"id": 7, "name": "Ada", "tags": ["a"], "odd key": {"it's": 1}, "gone": true}"#,
            r#"{"id": "7", "name": "Ada L", "tags": ["a"], "odd key": {"it's": 2}, "new": [1]}"#,
        );
        assert_eq!(
            lines(&d),
            [
                r#"~ $.id 7 "7""#,
                r#"~ $.name "Ada" "Ada L""#,
                r#"~ $['odd key']['it\'s'] 1 2"#,
                "- $.gone true _",
                "+ $.new _ [1]",
            ]
        );
        assert_eq!((d.added, d.removed, d.changed, d.cut), (1, 1, 3, false));
    }

    #[test]
    fn aligns_array_items_like_a_text_diff() {
        // An item inserted at the start is one addition, not every item changed.
        let d = diff(r#"[{"id": 1}, {"id": 2}, {"id": 3}]"#, r#"[{"id": 0}, {"id": 1}, {"id": 2}, {"id": 3}]"#);
        assert_eq!(lines(&d), [r#"+ $[0] _ {"id":0}"#]);
        // A changed item in the middle is compared field by field.
        let d = diff(r#"[1, {"id": 2, "n": "b"}, 3]"#, r#"[1, {"id": 2, "n": "B"}, 3]"#);
        assert_eq!(lines(&d), [r#"~ $[1].n "b" "B""#]);
        // Removed from the middle, added at the end.
        let d = diff("[1, 2, 3, 4]", "[1, 3, 4, 5]");
        assert_eq!(lines(&d), ["- $[1] 2 _", "+ $[3] _ 5"]);
        // Different types at the root.
        assert_eq!(lines(&diff("[1]", r#"{"a": 1}"#)), [r#"~ $ [1] {"a":1}"#]);
    }

    #[test]
    fn can_ignore_array_order() {
        let options = JsonDiffOptions { ignore_array_order: true };
        let d = diff_json(r#"{"ids": [3, 1, 2, 2]}"#, r#"{"ids": [1, 2, 3, 4]}"#, options).unwrap();
        assert_eq!(lines(&d), ["- $.ids[3] 2 _", "+ $.ids[3] _ 4"]);
        let d = diff_json(r#"[{"a": 1, "b": 2}, {"c": 3}]"#, r#"[{"c": 3}, {"b": 2, "a": 1.0}]"#, options).unwrap();
        assert!(d.differences.is_empty());
        // In order, the same arrays differ.
        assert!(!diff("[1, 2]", "[2, 1]").differences.is_empty());
    }

    #[test]
    fn caps_the_list_but_counts_everything() {
        let a: Vec<u32> = (0..3000).collect();
        let b: Vec<String> = (0..3000).map(|i| i.to_string()).collect();
        let d = diff(&serde_json::to_string(&a).unwrap(), &serde_json::to_string(&b).unwrap());
        assert_eq!(d.differences.len(), MAX_DIFFERENCES);
        assert_eq!(d.changed, 3000);
        assert!(d.cut);
    }

    #[test]
    fn long_arrays_are_compared_by_index_without_a_huge_table() {
        let a: Vec<u32> = (0..5000).collect();
        let mut b = a.clone();
        b.reverse();
        let d = diff(&serde_json::to_string(&a).unwrap(), &serde_json::to_string(&b).unwrap());
        assert_eq!(d.changed, 5000);
        assert_eq!((d.added, d.removed), (0, 0));
    }

    #[test]
    fn explains_bad_input_by_side() {
        let e = diff_json("{", "{}", JsonDiffOptions::default()).unwrap_err();
        assert!(e.starts_with("The left document isn't valid JSON"), "{e}");
        let e = diff_json("{}", "  ", JsonDiffOptions::default()).unwrap_err();
        assert_eq!(e, "The right document is empty");
        assert!(diff_json("\u{feff}{}", "{}", JsonDiffOptions::default()).unwrap().differences.is_empty());
    }

    #[test]
    fn shortens_long_values_and_quotes_odd_keys() {
        let long = "x".repeat(500);
        let d = diff(&format!(r#"{{"a\nb": "{long}"}}"#), r#"{"a\nb": 1}"#);
        let x = &d.differences[0];
        assert_eq!(x.path, r"$['a\nb']");
        assert_eq!(x.left.as_ref().unwrap().chars().count(), MAX_PREVIEW + 1);
        assert!(x.left.as_ref().unwrap().ends_with('…'));
    }
}

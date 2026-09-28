//! Checking a response against the OpenAPI operation its request was imported from: is the
//! status documented, and does a JSON body match the documented schema? A small validator
//! for the schema keywords that describe types (OpenAPI 3.0 and 3.1, Swagger 2), not a full
//! JSON Schema implementation: formats and patterns are not checked.

use serde_json::Value;

use crate::openapi::percent_decode;

/// Problems reported at most per response.
const MAX_PROBLEMS: usize = 20;
/// Array items checked at most (the rest are assumed alike).
const MAX_ITEMS: usize = 1000;
/// Nesting followed at most (recursive schemas).
const MAX_DEPTH: usize = 64;
/// Schemas checked at most per response. `allOf`, `anyOf` and `oneOf` that refer back to their
/// own schema branch at every level; past this the rest of the response is assumed to match.
const MAX_VISITS: usize = 200_000;

/// The outcome of one check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecCheck {
    /// E.g. `GET /pets/{petId} → 200`.
    pub label: String,
    /// Empty when the response matches.
    pub problems: Vec<String>,
}

impl SpecCheck {
    pub fn passed(&self) -> bool {
        self.problems.is_empty()
    }
}

/// Check a response to `operation` (`METHOD /path` as imported) against `doc`. `None` when
/// the document doesn't have the operation or documents no responses for it (nothing to check).
pub fn check_response(
    doc: &Value,
    operation: &str,
    status: u16,
    content_type: Option<&str>,
    body: &[u8],
) -> Option<SpecCheck> {
    let (method, path) = operation.split_once(' ')?;
    let item = resolve(doc, doc.get("paths")?.get(path)?)?;
    let op = resolve(doc, item.get(method.to_ascii_lowercase())?)?;
    let responses = resolve(doc, op.get("responses")?)?.as_object()?;
    if responses.is_empty() {
        return None;
    }
    let code = status.to_string();
    let range = format!("{}XX", status / 100);
    let found = responses
        .iter()
        .find(|(k, _)| **k == code)
        .or_else(|| responses.iter().find(|(k, _)| k.eq_ignore_ascii_case(&range)))
        .or_else(|| responses.get_key_value("default"));
    let label = format!("{operation} → {status}");
    let Some((_, response)) = found else {
        let documented: Vec<&str> = responses.keys().map(String::as_str).filter(|k| !k.starts_with("x-")).collect();
        return Some(SpecCheck {
            label,
            problems: vec![format!("Status {status} is not documented (documented: {})", documented.join(", "))],
        });
    };
    let response = resolve(doc, response)?;
    let Some(schema) = response_schema(doc, response, content_type) else {
        // No body documented, or not a JSON body: the status is all there is to check.
        return Some(SpecCheck { label, problems: Vec::new() });
    };
    if body.is_empty() {
        return Some(SpecCheck { label, problems: vec!["The body is empty, but the spec documents one".into()] });
    }
    let value: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(e) => return Some(SpecCheck { label, problems: vec![format!("The body is not valid JSON ({e})")] }),
    };
    let mut problems = Vec::new();
    let mut checker = Checker { doc, problems: &mut problems, visits: 0 };
    checker.check(schema, &value, "$", 0);
    if problems.len() > MAX_PROBLEMS {
        let more = problems.len() - MAX_PROBLEMS;
        problems.truncate(MAX_PROBLEMS);
        problems.push(format!("… and {more} more"));
    }
    Some(SpecCheck { label, problems })
}

/// The schema of a JSON body the response documents for `content_type`.
fn response_schema<'a>(doc: &'a Value, response: &'a Value, content_type: Option<&str>) -> Option<&'a Value> {
    // Swagger 2: the schema sits on the response.
    if let Some(schema) = response.get("schema") {
        return Some(schema);
    }
    let content = response.get("content")?.as_object()?;
    let mime = content_type.unwrap_or_default().split(';').next().unwrap_or_default().trim().to_ascii_lowercase();
    let is_json = |m: &str| m == "application/json" || m.ends_with("+json") || m == "*/*" || m == "application/*";
    let (_, media) = content
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(&mime))
        .or_else(|| content.iter().find(|(k, _)| is_json(&k.to_ascii_lowercase())))?;
    // A non-JSON answer to an operation that documents JSON: only JSON bodies are checked.
    if !mime.is_empty() && !is_json(&mime) && !mime.contains("json") {
        return None;
    }
    let media = resolve(doc, media)?;
    media.get("schema")
}

/// `value`, following a local `$ref` (`#/components/schemas/Pet`), up to a few hops.
fn resolve<'a>(doc: &'a Value, mut value: &'a Value) -> Option<&'a Value> {
    for _ in 0..16 {
        match value.get("$ref").and_then(Value::as_str) {
            Some(r) => value = pointer(doc, r)?,
            None => return Some(value),
        }
    }
    None
}

/// A local JSON pointer reference (`#/a/b~1c`, or percent-encoded as in a URI: `#/paths/~1pets~1%7Bid%7D`).
fn pointer<'a>(doc: &'a Value, reference: &str) -> Option<&'a Value> {
    let path = reference.strip_prefix('#')?;
    doc.pointer(path).or_else(|| percent_decode(path).and_then(|p| doc.pointer(&p)))
}

struct Checker<'a, 'p> {
    doc: &'a Value,
    problems: &'p mut Vec<String>,
    /// Schemas checked so far, up to [`MAX_VISITS`].
    visits: usize,
}

impl Checker<'_, '_> {
    fn report(&mut self, message: String) {
        if self.problems.len() <= MAX_PROBLEMS {
            self.problems.push(message);
        } else {
            // Counted, for the "and N more" line.
            self.problems.push(String::new());
        }
    }

    fn check(&mut self, schema: &Value, value: &Value, at: &str, depth: usize) {
        if depth > MAX_DEPTH || self.visits >= MAX_VISITS {
            return;
        }
        self.visits += 1;
        // A `$ref` the document doesn't have (or an external one): nothing to check against.
        let Some(schema) = resolve(self.doc, schema) else { return };
        let Some(s) = schema.as_object() else { return };

        if let Some(all) = s.get("allOf").and_then(Value::as_array) {
            for sub in all {
                self.check(sub, value, at, depth + 1);
            }
        }
        // Only "matches none" is reported, so `oneOf` stops at its first match too: options
        // that overlap are common in real documents.
        for (key, need_one) in [("anyOf", false), ("oneOf", true)] {
            if let Some(options) = s.get(key).and_then(Value::as_array)
                && !options.is_empty()
                && !value.is_null()
                && !options.iter().any(|o| self.matches(o, value, depth + 1))
            {
                let which = if need_one { "one of the oneOf schemas" } else { "any of the anyOf schemas" };
                self.report(format!("`{at}` matches none of {which}"));
                return;
            }
        }

        let nullable = s.get("nullable").and_then(Value::as_bool).unwrap_or(false);
        let types: Vec<&str> = match s.get("type") {
            Some(Value::String(t)) => vec![t.as_str()],
            Some(Value::Array(ts)) => ts.iter().filter_map(Value::as_str).collect(),
            _ => Vec::new(),
        };
        if value.is_null() {
            let allowed = nullable || types.contains(&"null") || (types.is_empty() && !s.contains_key("enum"));
            if !allowed {
                self.report(format!("`{at}` is null, but the spec doesn't allow null"));
            }
            return;
        }
        if !types.is_empty() && !types.iter().any(|t| type_matches(t, value)) {
            self.report(format!("`{at}`: expected {}, got {}", types.join(" or "), describe(value)));
            return;
        }
        if let Some(options) = s.get("enum").and_then(Value::as_array)
            && !options.iter().any(|o| same(o, value))
        {
            self.report(format!("`{at}`: {} is not one of the documented values", short(value)));
        }
        if let Some(c) = s.get("const")
            && !same(c, value)
        {
            self.report(format!("`{at}`: expected {}, got {}", short(c), short(value)));
        }

        match value {
            Value::Object(map) => {
                let props = s.get("properties").and_then(Value::as_object);
                if let Some(required) = s.get("required").and_then(Value::as_array) {
                    for name in required.iter().filter_map(Value::as_str) {
                        // A write-only property never comes back in a response.
                        let write_only = props
                            .and_then(|p| p.get(name))
                            .and_then(|p| resolve(self.doc, p))
                            .and_then(|p| p.get("writeOnly"))
                            .and_then(Value::as_bool)
                            .unwrap_or(false);
                        if !map.contains_key(name) && !write_only {
                            self.report(format!("`{at}` is missing the required field `{name}`"));
                        }
                    }
                }
                for (name, v) in map {
                    let path = format!("{at}.{name}");
                    match props.and_then(|p| p.get(name)) {
                        Some(sub) => self.check(sub, v, &path, depth + 1),
                        None => match s.get("additionalProperties") {
                            Some(Value::Bool(false)) => self.report(format!("`{path}` is not in the spec")),
                            Some(extra @ Value::Object(_)) => self.check(extra, v, &path, depth + 1),
                            _ => {}
                        },
                    }
                }
            }
            Value::Array(items) => {
                let (min, max) = (s.get("minItems").and_then(Value::as_u64), s.get("maxItems").and_then(Value::as_u64));
                if min.is_some_and(|m| (items.len() as u64) < m) {
                    self.report(format!(
                        "`{at}` has {} items, fewer than the {} required",
                        items.len(),
                        min.unwrap_or(0)
                    ));
                }
                if max.is_some_and(|m| (items.len() as u64) > m) {
                    self.report(format!(
                        "`{at}` has {} items, more than the {} allowed",
                        items.len(),
                        max.unwrap_or(0)
                    ));
                }
                if let Some(item) = s.get("items").filter(|i| i.is_object()) {
                    for (i, v) in items.iter().take(MAX_ITEMS).enumerate() {
                        self.check(item, v, &format!("{at}[{i}]"), depth + 1);
                    }
                }
            }
            Value::String(text) => {
                let len = text.chars().count() as u64;
                if s.get("minLength").and_then(Value::as_u64).is_some_and(|m| len < m) {
                    self.report(format!("`{at}` is shorter than the documented minimum length"));
                }
                if s.get("maxLength").and_then(Value::as_u64).is_some_and(|m| len > m) {
                    self.report(format!("`{at}` is longer than the documented maximum length"));
                }
            }
            Value::Number(n) => {
                let Some(x) = n.as_f64() else { return };
                let bound = |key: &str| s.get(key).and_then(Value::as_f64);
                let exclusive = |key: &str| s.get(key).and_then(Value::as_bool).unwrap_or(false);
                if let Some(min) = bound("minimum")
                    && (x < min || (x == min && exclusive("exclusiveMinimum")))
                {
                    self.report(format!("`{at}`: {x} is below the documented minimum {min}"));
                }
                if let Some(max) = bound("maximum")
                    && (x > max || (x == max && exclusive("exclusiveMaximum")))
                {
                    self.report(format!("`{at}`: {x} is above the documented maximum {max}"));
                }
                // OpenAPI 3.1: the exclusive bounds are numbers.
                if let Some(min) = bound("exclusiveMinimum")
                    && x <= min
                {
                    self.report(format!("`{at}`: {x} must be above {min}"));
                }
                if let Some(max) = bound("exclusiveMaximum")
                    && x >= max
                {
                    self.report(format!("`{at}`: {x} must be below {max}"));
                }
            }
            _ => {}
        }
    }

    /// Whether `value` matches `schema` (without reporting). Counts toward [`MAX_VISITS`].
    fn matches(&mut self, schema: &Value, value: &Value, depth: usize) -> bool {
        let mut problems = Vec::new();
        let mut checker = Checker { doc: self.doc, problems: &mut problems, visits: self.visits };
        checker.check(schema, value, "$", depth);
        self.visits = checker.visits;
        problems.is_empty()
    }
}

fn type_matches(kind: &str, value: &Value) -> bool {
    match kind {
        "string" => value.is_string(),
        "boolean" => value.is_boolean(),
        "object" => value.is_object(),
        "array" => value.is_array(),
        "null" => value.is_null(),
        "number" => value.is_number(),
        "integer" => {
            value.as_i64().is_some() || value.as_u64().is_some() || value.as_f64().is_some_and(|f| f.fract() == 0.0)
        }
        // A type the validator doesn't know: don't fail on it.
        _ => true,
    }
}

fn same(a: &Value, b: &Value) -> bool {
    match (a.as_f64(), b.as_f64()) {
        (Some(x), Some(y)) => x == y,
        _ => a == b,
    }
}

fn describe(value: &Value) -> String {
    match value {
        Value::Null => "null".into(),
        Value::Bool(_) => "a boolean".into(),
        Value::Number(_) => format!("a number ({value})"),
        Value::String(_) => format!("a string ({})", short(value)),
        Value::Array(_) => "an array".into(),
        Value::Object(_) => "an object".into(),
    }
}

fn short(value: &Value) -> String {
    let text = value.to_string();
    match text.char_indices().nth(60) {
        Some((at, _)) => format!("{}…", &text[..at]),
        None => text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn doc() -> Value {
        json!({
            "openapi": "3.0.3",
            "paths": {
                "/pets/{petId}": {
                    "get": {
                        "responses": {
                            "200": { "content": { "application/json": { "schema": { "$ref": "#/components/schemas/Pet" } } } },
                            "4XX": { "content": { "application/problem+json": { "schema": { "$ref": "#/components/schemas/Problem" } } } }
                        }
                    },
                    "delete": { "responses": { "204": { "description": "gone" } } }
                },
                "/pets": { "get": { "responses": { "200": { "content": { "application/json": { "schema": {
                    "type": "array", "items": { "$ref": "#/components/schemas/Pet" }, "maxItems": 3 } } } } } } }
            },
            "components": { "schemas": {
                "Pet": {
                    "type": "object",
                    "required": ["id", "name", "password"],
                    "properties": {
                        "id": { "type": "integer", "minimum": 1 },
                        "name": { "type": "string", "maxLength": 10 },
                        "status": { "type": "string", "enum": ["available", "sold"] },
                        "tag": { "type": "string", "nullable": true },
                        "owner": { "oneOf": [{ "type": "string" }, { "$ref": "#/components/schemas/Owner" }] },
                        "password": { "type": "string", "writeOnly": true }
                    },
                    "additionalProperties": false
                },
                "Owner": { "type": "object", "required": ["id"], "properties": { "id": { "type": "integer" } } },
                "Problem": { "type": "object", "required": ["title"], "properties": { "title": { "type": "string" } } }
            } }
        })
    }

    fn run(op: &str, status: u16, ct: &str, body: Value) -> SpecCheck {
        check_response(&doc(), op, status, Some(ct), body.to_string().as_bytes()).expect("checked")
    }

    #[test]
    fn a_matching_response_passes() {
        let ok = run(
            "GET /pets/{petId}",
            200,
            "application/json",
            json!({ "id": 7, "name": "Rex", "status": "sold", "tag": null, "owner": { "id": 1 } }),
        );
        assert!(ok.passed(), "{:?}", ok.problems);
        assert_eq!(ok.label, "GET /pets/{petId} → 200");
        assert!(run("GET /pets", 200, "application/json; charset=utf-8", json!([{ "id": 1, "name": "a" }])).passed());
    }

    #[test]
    fn mismatches_are_named_by_path() {
        let bad = run(
            "GET /pets/{petId}",
            200,
            "application/json",
            json!({ "id": "7", "status": "lost", "extra": 1, "owner": true }),
        );
        let text = bad.problems.join("\n");
        assert!(text.contains("`$.id`: expected integer, got a string"), "{text}");
        assert!(text.contains("missing the required field `name`"), "{text}");
        assert!(!text.contains("`password`"), "write-only fields are not expected back: {text}");
        assert!(text.contains("`$.status`: \"lost\" is not one of"), "{text}");
        assert!(text.contains("`$.extra` is not in the spec"), "{text}");
        assert!(text.contains("`$.owner` matches none of one of the oneOf"), "{text}");
        let list = run("GET /pets", 200, "application/json", json!([{ "id": 0, "name": "a" }, {}, {}, {}]));
        let text = list.problems.join("\n");
        assert!(text.contains("more than the 3 allowed") && text.contains("`$[0].id`: 0 is below"), "{text}");
    }

    #[test]
    fn statuses_ranges_and_bodies() {
        let problem = run("GET /pets/{petId}", 404, "application/problem+json", json!({ "title": "Not found" }));
        assert!(problem.passed(), "{:?}", problem.problems);
        let undocumented = run("GET /pets/{petId}", 500, "application/json", json!({}));
        assert!(
            undocumented.problems[0].contains("Status 500 is not documented (documented: 200, 4XX)"),
            "{:?}",
            undocumented.problems
        );
        // No body documented: only the status counts.
        assert!(check_response(&doc(), "DELETE /pets/{petId}", 204, None, b"").unwrap().passed());
        // A non-JSON answer isn't checked against a JSON schema.
        assert!(check_response(&doc(), "GET /pets/{petId}", 200, Some("text/html"), b"<html>").unwrap().passed());
        let broken = check_response(&doc(), "GET /pets/{petId}", 200, Some("application/json"), b"{oops").unwrap();
        assert!(broken.problems[0].contains("not valid JSON"));
        assert!(check_response(&doc(), "GET /nowhere", 200, None, b"").is_none());
    }

    #[test]
    fn openapi_31_and_swagger_2() {
        let v31 = json!({ "openapi": "3.1.0", "paths": { "/a": { "get": { "responses": { "200": { "content": { "application/json": {
            "schema": { "type": "object", "properties": { "n": { "type": ["integer", "null"], "exclusiveMinimum": 0 } } } } } } } } } } });
        assert!(check_response(&v31, "GET /a", 200, Some("application/json"), br#"{"n": null}"#).unwrap().passed());
        let low = check_response(&v31, "GET /a", 200, Some("application/json"), br#"{"n": 0}"#).unwrap();
        assert!(low.problems[0].contains("must be above 0"), "{:?}", low.problems);
        let v2 = json!({ "swagger": "2.0", "paths": { "/b": { "get": { "responses": { "200": { "schema": { "$ref": "#/definitions/B" } } } } } },
                         "definitions": { "B": { "type": "object", "required": ["x"] } } });
        let missing = check_response(&v2, "GET /b", 200, Some("application/json"), b"{}").unwrap();
        assert!(missing.problems[0].contains("required field `x`"), "{:?}", missing.problems);
    }

    #[test]
    fn recursive_schemas_end() {
        let d = json!({ "openapi": "3.0.0", "paths": { "/t": { "get": { "responses": { "200": { "content": { "application/json": {
            "schema": { "$ref": "#/components/schemas/Node" } } } } } } } },
            "components": { "schemas": { "Node": { "type": "object", "properties": { "child": { "$ref": "#/components/schemas/Node" } } } } } });
        let mut body = json!({});
        // Deeper than the checker follows (serde_json itself parses at most 128 levels).
        for _ in 0..100 {
            body = json!({ "child": body });
        }
        assert!(
            check_response(&d, "GET /t", 200, Some("application/json"), body.to_string().as_bytes()).unwrap().passed()
        );
    }

    #[test]
    fn schemas_that_branch_into_themselves_end() {
        // Every level checks the schema again for each of its three branches: without a limit on
        // the work, these take 3^64 steps and hang the check.
        for (keyword, value, passes) in [
            ("allOf", json!({ "a": 1 }), true),
            ("anyOf", json!({ "a": 1 }), true),
            ("oneOf", json!({ "a": 1 }), true),
            // Nothing matches, so every option is tried at every level.
            ("anyOf", json!("text"), false),
            ("oneOf", json!("text"), false),
        ] {
            let a = json!({ "$ref": "#/components/schemas/A" });
            let d = json!({ "openapi": "3.0.0", "paths": { "/t": { "get": { "responses": { "200": { "content": {
                "application/json": { "schema": a } } } } } } },
                "components": { "schemas": { "A": { "type": "object", keyword: [a, a, a] } } } });
            let started = std::time::Instant::now();
            let check = check_response(&d, "GET /t", 200, Some("application/json"), value.to_string().as_bytes());
            assert!(started.elapsed() < std::time::Duration::from_secs(10), "{keyword} took {:?}", started.elapsed());
            assert_eq!(check.unwrap().passed(), passes, "{keyword} {value}");
        }
    }

    #[test]
    fn refs_are_json_pointers() {
        let d = json!({ "openapi": "3.0.0",
            "paths": {
                "/pets": { "get": { "responses": { "200": { "content": { "application/json": {
                    "schema": { "$ref": "#/components/schemas/a~1b~0c" } } } } } } },
                "/pets/{id}": { "get": { "responses": { "200": { "$ref": "#/paths/~1pets/get/responses/200" } } } },
                "/copy": { "get": { "$ref": "#/paths/~1pets~1%7Bid%7D/get" } }
            },
            "components": { "schemas": { "a/b~c": { "type": "object", "required": ["id"] } } } });
        for op in ["GET /pets", "GET /pets/{id}", "GET /copy"] {
            let missing = check_response(&d, op, 200, Some("application/json"), b"{}").unwrap();
            assert_eq!(missing.problems, ["`$` is missing the required field `id`"], "{op}");
        }
        assert_eq!(pointer(&d, "#"), Some(&d));
        assert_eq!(pointer(&d, "#/components/schemas/a~1b~0c/required/0"), Some(&json!("id")));
        assert_eq!(pointer(&d, "#/components/schemas/a~01b"), None);
    }
}

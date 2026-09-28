//! Response filters: JSONPath (RFC 9535) and jq expressions over a JSON body. The
//! app's response filter, and AI agents, use them to pick parts of large responses.

use jaq_core::load::{Arena, File, Loader};
use jaq_core::{Compiler, Ctx, Vars, data, unwrap_valr};
use jaq_json::Val;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum FilterLanguage {
    /// `$.items[?@.price > 10].name`
    JsonPath,
    /// `.items[] | select(.price > 10) | .name`
    Jq,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FilterResult {
    /// The matches: JSONPath as a pretty JSON array, jq as each output value pretty-printed
    /// on its own (like the jq command).
    pub text: String,
    /// How many values matched (or jq produced).
    pub count: u32,
    /// Stopped after [`MAX_RESULTS`] values or [`MAX_TEXT`] bytes.
    pub cut: bool,
}

pub const MAX_RESULTS: usize = 10_000;
pub const MAX_TEXT: usize = 20 * 1024 * 1024;

/// Runs `expression` over the JSON document `body`. Errors are plain sentences.
pub fn filter(body: &[u8], language: FilterLanguage, expression: &str) -> Result<FilterResult, String> {
    let expression = expression.trim();
    if expression.is_empty() {
        return Err("Type an expression".into());
    }
    match language {
        FilterLanguage::JsonPath => json_path(body, expression),
        FilterLanguage::Jq => jq(body, expression),
    }
}

fn not_json(e: impl std::fmt::Display) -> String {
    format!("The response isn't valid JSON ({e}), so it can't be filtered")
}

fn json_path(body: &[u8], expression: &str) -> Result<FilterResult, String> {
    let path =
        serde_json_path::JsonPath::parse(expression).map_err(|e| format!("Not a valid JSONPath expression: {e}"))?;
    let value: serde_json::Value = serde_json::from_slice(body).map_err(not_json)?;
    let found = path.query(&value).all();
    let count = found.len();
    let cut = count > MAX_RESULTS;
    let shown: Vec<&serde_json::Value> = found.into_iter().take(MAX_RESULTS).collect();
    let text = serde_json::to_string_pretty(&shown).unwrap_or_default();
    Ok(FilterResult { text, count: count as u32, cut })
}

fn jq(body: &[u8], expression: &str) -> Result<FilterResult, String> {
    let input = jaq_json::read::parse_single(body).map_err(not_json)?;
    let defs = jaq_core::defs().chain(jaq_std::defs()).chain(jaq_json::defs());
    let funs = jaq_core::funs().chain(jaq_std::funs()).chain(jaq_json::funs());
    let loader = Loader::new(defs);
    let arena = Arena::default();
    let modules = loader.load(&arena, File { code: expression, path: () }).map_err(|errors| {
        let detail = errors.into_iter().flat_map(|(_, e)| load_error(e)).next().unwrap_or_default();
        format!("Not a valid jq expression{detail}")
    })?;
    let filter = Compiler::default().with_funs(funs).compile(modules).map_err(|errors| {
        let names: Vec<String> = errors
            .into_iter()
            .flat_map(|(_, list)| list)
            .map(|(name, undefined)| match undefined {
                jaq_core::compile::Undefined::Filter(arity) => format!("{name}/{arity}"),
                _ => name.to_string(),
            })
            .collect();
        format!("Unknown in jq: {}", names.join(", "))
    })?;
    let ctx = Ctx::<data::JustLut<Val>>::new(&filter.lut, Vars::new([]));
    let mut outputs = Vec::new();
    let mut size = 0;
    let mut cut = false;
    let mut count = 0usize;
    for value in filter.id.run((ctx, input)).map(unwrap_valr) {
        let value = value.map_err(|e| format!("jq: {e}"))?;
        count += 1;
        if count > MAX_RESULTS || size > MAX_TEXT {
            cut = true;
            break;
        }
        let text = pretty(&value);
        size += text.len();
        outputs.push(text);
    }
    Ok(FilterResult { text: outputs.join("\n"), count: count.min(MAX_RESULTS) as u32, cut })
}

/// A jq value pretty-printed like the jq command (strings, numbers and scalars as JSON).
fn pretty(value: &Val) -> String {
    let compact = value.to_string();
    match serde_json::from_str::<serde_json::Value>(&compact) {
        Ok(json) if json.is_object() || json.is_array() => serde_json::to_string_pretty(&json).unwrap_or(compact),
        _ => compact,
    }
}

fn load_error(e: jaq_core::load::Error<&str>) -> Option<String> {
    match e {
        jaq_core::load::Error::Lex(errors) => {
            errors.first().map(|(expect, at)| format!(": expected a {} {}", expect.as_str(), near(at)))
        }
        jaq_core::load::Error::Parse(errors) => {
            errors.first().map(|(expect, at)| format!(": expected {} {}", expect.as_str(), near(at)))
        }
        jaq_core::load::Error::Io(_) => None,
    }
}

fn near(at: &str) -> String {
    let at: String = at.chars().take(20).collect();
    if at.trim().is_empty() { "at the end".into() } else { format!("near \"{at}\"") }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = r#"{"store": {"books": [
        {"title": "Dune", "price": 9.5, "tags": ["sf"]},
        {"title": "Emma", "price": 12, "tags": []},
        {"title": "Élan", "price": 30, "tags": ["fr", "new"]}
    ]}}"#;

    #[test]
    fn json_path_queries() {
        let r = filter(DOC.as_bytes(), FilterLanguage::JsonPath, "$.store.books[?@.price > 10].title").unwrap();
        assert_eq!(r.count, 2);
        assert_eq!(serde_json::from_str::<serde_json::Value>(&r.text).unwrap(), serde_json::json!(["Emma", "Élan"]));
        let r = filter(DOC.as_bytes(), FilterLanguage::JsonPath, "$..tags[*]").unwrap();
        assert_eq!(r.count, 3);
        let none = filter(DOC.as_bytes(), FilterLanguage::JsonPath, "$.nothing").unwrap();
        assert_eq!((none.count, none.text.as_str()), (0, "[]"));
        let e = filter(DOC.as_bytes(), FilterLanguage::JsonPath, "$.store[").unwrap_err();
        assert!(e.starts_with("Not a valid JSONPath expression"), "{e}");
    }

    #[test]
    fn jq_expressions() {
        let r = filter(DOC.as_bytes(), FilterLanguage::Jq, ".store.books[] | select(.price > 10) | .title").unwrap();
        assert_eq!((r.count, r.text.as_str()), (2, "\"Emma\"\n\"Élan\""));
        let r = filter(DOC.as_bytes(), FilterLanguage::Jq, "[.store.books[].price] | add").unwrap();
        assert_eq!(r.text, "51.5");
        let r = filter(DOC.as_bytes(), FilterLanguage::Jq, ".store.books | map({title}) | .[0]").unwrap();
        assert_eq!(r.text, "{\n  \"title\": \"Dune\"\n}");
        let r = filter(DOC.as_bytes(), FilterLanguage::Jq, "[.store.books[] | .tags | length] | max").unwrap();
        assert_eq!(r.text, "2");
    }

    #[test]
    fn errors_are_sentences() {
        let e = filter(DOC.as_bytes(), FilterLanguage::Jq, ".store[").unwrap_err();
        assert!(e.starts_with("Not a valid jq expression: expected a closing bracket"), "{e}");
        let e = filter(DOC.as_bytes(), FilterLanguage::Jq, "frobnicate(1)").unwrap_err();
        assert_eq!(e, "Unknown in jq: frobnicate/1");
        let e = filter(DOC.as_bytes(), FilterLanguage::Jq, ".store.books[0].title.x").unwrap_err();
        assert!(e.starts_with("jq: cannot index"), "{e}");
        let e = filter(b"<html>", FilterLanguage::Jq, ".").unwrap_err();
        assert!(e.starts_with("The response isn't valid JSON"), "{e}");
        assert_eq!(filter(DOC.as_bytes(), FilterLanguage::Jq, "  ").unwrap_err(), "Type an expression");
    }

    #[test]
    fn endless_output_is_cut() {
        let r = filter(b"0", FilterLanguage::Jq, "repeat(1)").unwrap();
        assert!(r.cut);
        assert_eq!(r.count as usize, MAX_RESULTS);
    }
}

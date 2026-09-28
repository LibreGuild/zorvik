//! Response filters: JSONPath (RFC 9535) and jq expressions over a JSON body. The
//! app's response filter, and AI agents, use them to pick parts of large responses.

use std::cell::Cell;

use jaq_core::data::{DataT, HasLut};
use jaq_core::load::{Arena, File, Loader};
use jaq_core::{Compiler, Ctx, Lut, Vars, unwrap_valr};
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

/// Arrays and objects a document may nest for jq, as serde_json allows for JSONPath (jaq's
/// parser recurses once per level, with no limit of its own).
const MAX_NESTING: usize = 128;
/// Stack of the thread jq runs on: its evaluator recurses as deeply as the expression does.
/// Only the pages it touches are used.
const JQ_STACK: usize = 256 * 1024 * 1024;
/// Stack a jq expression may use before it is stopped. The rest is headroom for work that
/// recurses between checks, such as comparing or printing values (those from the response
/// nest at most [`MAX_NESTING`] levels).
const JQ_STACK_USE: usize = 192 * 1024 * 1024;
/// Steps a jq expression may take, so an endless loop that outputs nothing ends: a fixed
/// allowance plus some per byte of the response (going through every value takes about two).
const JQ_STEPS: u64 = 100_000_000;
const JQ_STEPS_PER_BYTE: u64 = 100;

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
    if too_deep(body) {
        return Err(format!(
            "The response nests arrays and objects more than {MAX_NESTING} levels deep, so it can't be filtered"
        ));
    }
    let max_steps = JQ_STEPS.saturating_add(JQ_STEPS_PER_BYTE.saturating_mul(body.len() as u64));
    jq_limited(body, expression, max_steps)
}

/// Runs jq on a thread of its own, with a stack big enough for deep recursion. [`Limits`]
/// stops an expression before it overflows that stack, which would abort the whole process.
/// Not covered: a loop inside one builtin (`last(range(1e12))` takes no steps), and values or
/// expressions nested deeper than the stack allows (`reduce range(1e6) as $x (null; [.])`).
fn jq_limited(body: &[u8], expression: &str, max_steps: u64) -> Result<FilterResult, String> {
    std::thread::scope(|scope| {
        let run = std::thread::Builder::new()
            .name("jq".into())
            .stack_size(JQ_STACK)
            .spawn_scoped(scope, || run_jq(body, expression, max_steps))
            .map_err(|e| format!("Could not start jq ({e})"))?;
        run.join().unwrap_or_else(|panic| {
            Err(match panic.downcast::<Stop>() {
                Ok(stop) => stop.message().to_string(),
                Err(_) => "jq stopped unexpectedly".to_string(),
            })
        })
    })
}

/// Whether `body` nests arrays and objects more than [`MAX_NESTING`] levels deep.
fn too_deep(body: &[u8]) -> bool {
    let mut depth = 0usize;
    let mut bytes = body.iter();
    while let Some(&b) = bytes.next() {
        match b {
            b'"' => {
                while let Some(&c) = bytes.next() {
                    match c {
                        b'\\' => {
                            bytes.next();
                        }
                        b'"' => break,
                        _ => {}
                    }
                }
            }
            // jaq reads `#` comments too.
            b'#' => {
                bytes.by_ref().find(|&&c| c == b'\n');
            }
            b'[' | b'{' => {
                depth += 1;
                if depth > MAX_NESTING {
                    return true;
                }
            }
            b']' | b'}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    false
}

/// Why a jq run was stopped: the payload its thread unwinds with.
enum Stop {
    TooDeep,
    TooLong,
}

impl Stop {
    fn message(&self) -> &'static str {
        match self {
            Stop::TooDeep => "jq: the expression recursed too deeply and was stopped; check that its recursion ends",
            Stop::TooLong => {
                "jq: the expression took too many steps and was stopped; check that it doesn't loop forever"
            }
        }
    }
}

/// jaq data that stops an expression that recurses too deeply or runs too long. Every step of
/// a jq filter looks up its program through [`HasLut::lut`], so that's where the limits are checked.
struct Guarded;

impl DataT for Guarded {
    type V<'a> = Val;
    type Data<'a> = Limits<'a>;
}

#[derive(Clone)]
struct Limits<'a> {
    lut: &'a Lut<Guarded>,
    steps: &'a Cell<u64>,
    max_steps: u64,
    /// Where the stack was when jq started.
    stack: usize,
}

impl<'a> HasLut<'a, Guarded> for Limits<'a> {
    fn lut(&self) -> &'a Lut<Guarded> {
        let steps = self.steps.get() + 1;
        self.steps.set(steps);
        let why = if steps > self.max_steps {
            Some(Stop::TooLong)
        } else if stack_position().abs_diff(self.stack) > JQ_STACK_USE {
            Some(Stop::TooDeep)
        } else {
            None
        };
        if let Some(why) = why {
            // Unwinds the jq thread without a panic message; `jq` turns it into an error.
            std::panic::resume_unwind(Box::new(why));
        }
        self.lut
    }
}

/// An address on the current thread's stack.
fn stack_position() -> usize {
    let marker = 0u8;
    std::hint::black_box(std::ptr::addr_of!(marker)) as usize
}

fn run_jq(body: &[u8], expression: &str, max_steps: u64) -> Result<FilterResult, String> {
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
    let steps = Cell::new(0);
    let limits = Limits { lut: &filter.lut, steps: &steps, max_steps, stack: stack_position() };
    let ctx = Ctx::<Guarded>::new(limits, Vars::new([]));
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

    #[test]
    fn deeply_nested_responses_are_refused() {
        // jaq's parser would overflow the stack (and abort the process) on these.
        for body in ["[".repeat(20_000), format!("{}1{}", "[".repeat(129), "]".repeat(129)), "{\"a\":".repeat(20_000)] {
            let e = filter(body.as_bytes(), FilterLanguage::Jq, ".").unwrap_err();
            assert_eq!(e, "The response nests arrays and objects more than 128 levels deep, so it can't be filtered");
        }
        // Brackets in strings and comments don't count; 128 levels are fine.
        let body = format!("{}\"{}\"{} # {}", "[".repeat(128), "[".repeat(200), "]".repeat(128), "{".repeat(200));
        assert_eq!(filter(body.as_bytes(), FilterLanguage::Jq, "flatten | length").unwrap().text, "1");
        assert!(!too_deep(br#"{"a": "\"[[[", "b": [[[]]]}"#));
    }

    #[test]
    fn endless_recursion_is_stopped() {
        for expression in ["def f: 1 + f; f", "def f: [f]; f", "def f: f | f; f"] {
            let e = filter(DOC.as_bytes(), FilterLanguage::Jq, expression).unwrap_err();
            assert!(e.starts_with("jq: the expression recursed too deeply"), "{expression}: {e}");
        }
        // Recursion that ends still works.
        let r = filter(b"500", FilterLanguage::Jq, "def f: if . == 0 then 0 else 1 + (. - 1 | f) end; f").unwrap();
        assert_eq!(r.text, "500");
    }

    #[test]
    fn endless_loops_are_stopped() {
        for expression in ["last(repeat(1))", "until(false; .)", "def f: f; f"] {
            let e = jq_limited(b"0", expression, 100_000).unwrap_err();
            assert!(e.starts_with("jq: the expression took too many steps"), "{expression}: {e}");
        }
        assert_eq!(jq_limited(DOC.as_bytes(), "[.. | numbers] | add", 100_000).unwrap().text, "51.5");
    }
}

//! `{{variable}}` templating with Postman-compatible dynamic variables.

use std::borrow::Cow;
use std::collections::{BTreeSet, HashMap};

use rand::RngExt as _;
use zorvik_formats::Variable;

/// Most bytes that variable values may add to one rendered string. Values come
/// from files shared via Git, so chains like `a: "{{b}}{{b}}…"`, `b: "{{c}}{{c}}…"`
/// must not expand exponentially.
const MAX_EXPANSION: usize = 16 * 1024 * 1024;

/// Variables in precedence order: earlier layers win.
#[derive(Debug, Default, Clone)]
pub struct VarContext {
    layers: Vec<HashMap<String, String>>,
}

impl VarContext {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a lower-precedence layer (only enabled variables with a key).
    pub fn push_layer(&mut self, vars: &[Variable]) -> &mut Self {
        self.layers.push(
            vars.iter()
                .filter(|v| v.enabled && !v.key.trim().is_empty())
                .map(|v| (v.key.trim().to_string(), v.value.clone()))
                .collect(),
        );
        self
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.layers.iter().find_map(|l| l.get(name)).map(String::as_str)
    }

    /// Render `template`, recording names that could not be resolved.
    pub fn render(&self, template: &str, unresolved: &mut BTreeSet<String>) -> String {
        let mut budget = MAX_EXPANSION;
        self.render_depth(template, unresolved, 0, &mut budget)
    }

    /// `budget` is shared by the whole render: every substitution uses up the
    /// length of its value (at least 1), which bounds both output and work.
    fn render_depth(
        &self,
        template: &str,
        unresolved: &mut BTreeSet<String>,
        depth: usize,
        budget: &mut usize,
    ) -> String {
        if !template.contains("{{") {
            return template.to_string();
        }
        let mut out = String::with_capacity(template.len());
        let mut rest = template;
        while let Some(start) = rest.find("{{") {
            out.push_str(&rest[..start]);
            let after = &rest[start + 2..];
            let Some(end) = after.find("}}") else {
                out.push_str(&rest[start..]);
                return out;
            };
            let raw_name = &after[..end];
            // `{{` inside the name means the first braces were literal text. Jump
            // straight to the last candidate before these `}}`, so long runs of
            // `{{` are not rescanned once per pair (quadratic time).
            if let Some(last) = last_open(raw_name) {
                out.push_str("{{");
                out.push_str(&after[..last]);
                rest = &after[last..];
                continue;
            }
            let name = raw_name.trim();
            if name.is_empty() || raw_name.contains('\n') {
                out.push_str("{{");
                rest = after;
                continue;
            }
            let value = self.lookup(name).filter(|v| v.len() < *budget);
            if let Some(value) = &value {
                *budget -= value.len() + 1;
            }
            match value {
                Some(value) if depth < 10 => out.push_str(&self.render_depth(&value, unresolved, depth + 1, budget)),
                Some(value) => {
                    // Too deep: probably a cycle. Leave the rest raw.
                    unresolved.insert(name.to_string());
                    out.push_str(&value);
                }
                // Undefined, or expanding it would exceed the budget.
                None => {
                    unresolved.insert(name.to_string());
                    out.push_str(&rest[start..start + 2 + end + 2]);
                }
            }
            rest = &after[end + 2..];
        }
        out.push_str(rest);
        out
    }

    /// Defined variables first, then dynamic ones (docs/architecture.md, "Variables").
    fn lookup(&self, name: &str) -> Option<Cow<'_, str>> {
        self.get(name).map(Cow::Borrowed).or_else(|| dynamic_value(name.strip_prefix('$')?).map(Cow::Owned))
    }
}

/// Start of the `{{` that the left-to-right scan in `render_depth` would try
/// last within `s` (each match skips its two braces), if any.
fn last_open(s: &str) -> Option<usize> {
    let mut last = None;
    let mut from = 0;
    while let Some(i) = s[from..].find("{{") {
        last = Some(from + i);
        from += i + 2;
    }
    last
}

/// Postman-style dynamic variables.
pub fn dynamic_value(name: &str) -> Option<String> {
    let mut rng = rand::rng();
    Some(match name {
        "guid" | "uuid" | "randomUUID" => uuid::Uuid::new_v4().to_string(),
        "timestamp" => time::OffsetDateTime::now_utc().unix_timestamp().to_string(),
        "timestampMs" => (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000).to_string(),
        "isoTimestamp" => {
            time::OffsetDateTime::now_utc().format(&time::format_description::well_known::Rfc3339).unwrap_or_default()
        }
        "randomInt" => rng.random_range(0..=1000).to_string(),
        "randomBoolean" => rng.random_bool(0.5).to_string(),
        "randomAlphaNumeric" => {
            const CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
            (CHARS[rng.random_range(0..CHARS.len())] as char).to_string()
        }
        "randomEmail" => format!("user{}@example.com", rng.random_range(1000..10000)),
        _ => return None,
    })
}

/// Names of the dynamic variables, for UI autocomplete.
pub const DYNAMIC_VARIABLES: &[&str] = &[
    "$uuid",
    "$guid",
    "$randomUUID",
    "$timestamp",
    "$timestampMs",
    "$isoTimestamp",
    "$randomInt",
    "$randomBoolean",
    "$randomAlphaNumeric",
    "$randomEmail",
];

#[cfg(test)]
mod tests {
    use super::*;

    fn var(k: &str, v: &str) -> Variable {
        Variable { key: k.into(), value: v.into(), enabled: true, secret: false }
    }

    #[test]
    fn precedence_and_nesting() {
        let mut ctx = VarContext::new();
        ctx.push_layer(&[var("host", "env.example.com"), var("url", "https://{{host}}/{{ver}}")]);
        ctx.push_layer(&[var("host", "ws.example.com"), var("ver", "v2")]);
        let mut missing = BTreeSet::new();
        assert_eq!(
            ctx.render("{{url}}/x?q={{ missing }}", &mut missing),
            "https://env.example.com/v2/x?q={{ missing }}"
        );
        assert_eq!(missing.into_iter().collect::<Vec<_>>(), vec!["missing"]);
    }

    #[test]
    fn disabled_and_cycles() {
        let mut ctx = VarContext::new();
        ctx.push_layer(&[
            Variable { key: "off".into(), value: "x".into(), enabled: false, secret: false },
            var("a", "{{b}}"),
            var("b", "{{a}}"),
        ]);
        let mut missing = BTreeSet::new();
        assert_eq!(ctx.render("{{off}}", &mut missing), "{{off}}");
        let out = ctx.render("{{a}}", &mut missing);
        assert!(out.contains("{{"));
        assert!(missing.contains("a") || missing.contains("b"));
    }

    #[test]
    fn literal_braces_and_dynamic() {
        let ctx = VarContext::new();
        let mut missing = BTreeSet::new();
        assert_eq!(ctx.render("a {{ b", &mut missing), "a {{ b");
        assert_eq!(ctx.render("{{}} {{{{x}}", &mut missing), "{{}} {{{{x}}");
        assert_eq!(ctx.render("{\"a\":{\"b\":1}}", &mut missing), "{\"a\":{\"b\":1}}");
        let id = ctx.render("{{$uuid}}", &mut missing);
        assert_eq!(id.len(), 36);
        assert!(ctx.render("{{$timestamp}}", &mut missing).parse::<i64>().is_ok());
        assert!(missing.contains("x"));
        assert!(!missing.contains("$uuid"));
        assert_eq!(ctx.render("{{a{{b}} {{{{{x}}", &mut missing), "{{a{{b}} {{{{{x}}");
        assert!(missing.contains("b") && missing.contains("{x"));
    }

    #[test]
    fn defined_variables_win_over_dynamic() {
        let mut ctx = VarContext::new();
        ctx.push_layer(&[var("$timestamp", "fixed")]);
        let mut missing = BTreeSet::new();
        assert_eq!(ctx.render("{{$timestamp}} {{$nope}}", &mut missing), "fixed {{$nope}}");
        assert!(missing.contains("$nope"));
    }

    #[test]
    fn exponential_expansion_is_bounded() {
        // v0 = 40 x {{v1}}, v1 = 40 x {{v2}}, ... : 40^9 copies without a budget.
        let vars: Vec<Variable> = (0..10)
            .map(|i| {
                let value = if i == 9 { "payload".to_string() } else { format!("{{{{v{}}}}}", i + 1).repeat(40) };
                var(&format!("v{i}"), &value)
            })
            .collect();
        let mut ctx = VarContext::new();
        ctx.push_layer(&vars);
        let mut missing = BTreeSet::new();
        let out = ctx.render("{{v0}}", &mut missing);
        assert!(out.len() <= MAX_EXPANSION + 16, "{}", out.len());
        assert!(!missing.is_empty());
        // Many cheap references are still fine.
        let mut missing = BTreeSet::new();
        assert_eq!(ctx.render(&"{{v9}}".repeat(1000), &mut missing), "payload".repeat(1000));
        assert!(missing.is_empty());
    }

    #[test]
    fn runs_of_open_braces_render_in_linear_time() {
        let ctx = VarContext::new();
        let mut missing = BTreeSet::new();
        let text = format!("{}x}}}}", "{{".repeat(300_000));
        assert_eq!(ctx.render(&text, &mut missing), text);
        let text = format!("{}}}}}", "{{\n".repeat(300_000));
        assert_eq!(ctx.render(&text, &mut missing), text);
    }
}

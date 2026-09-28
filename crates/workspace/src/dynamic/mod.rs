//! Dynamic variables: `{{$uuid}}`, `{{$randomEmail}}`, `{{$randomInt(1, 10)}}` and the rest.
//!
//! The catalog is the single source of truth: templates, scripts and the UI's
//! autocomplete all use it. It has every Postman dynamic variable (same names,
//! compatible output) plus values that API tests often need: modern IDs, values
//! with valid check digits, dates relative to now, and edge-case text.
//!
//! Syntax inside the braces: `$name` or `$name(arg, …)`. Arguments are literal
//! text (variables inside them are not expanded) separated by commas; quote one
//! (`"…"` or `'…'`) to keep commas, parentheses or spaces in it. An empty
//! argument takes its default, and `$randomFrom` treats it as an empty value.
//! An unknown name or invalid arguments give `None`: the placeholder stays as
//! written and is reported like an undefined variable.

#[rustfmt::skip]
mod catalog;
#[rustfmt::skip]
mod data;
mod generators;
#[cfg(test)]
mod tests;

use rand::RngExt as _;
use rand::rngs::ThreadRng;
use serde::Serialize;
use time::OffsetDateTime;
use ts_rs::TS;

/// Most characters, bytes or items one parameterized variable may produce.
const MAX_LEN: usize = 100_000;
/// Most words, sentences, paragraphs or lines of generated text.
const MAX_WORDS: usize = 10_000;
/// Most days a `(days)` argument may span (100 years).
const MAX_DAYS: usize = 36_525;

/// One dynamic variable.
#[derive(Debug, Clone, Copy)]
pub struct DynamicVar {
    /// With the `$`, e.g. `$randomInt`.
    pub name: &'static str,
    pub group: &'static str,
    pub description: &'static str,
    /// A realistic sample output.
    pub example: &'static str,
    /// `""`, or the optional parameters, e.g. `(min, max)`; `…` means any number.
    pub args: &'static str,
    generator: Generator,
}

type Generator = fn(&mut Ctx) -> Option<String>;

impl DynamicVar {
    /// Most arguments the variable takes, from its `args` signature.
    fn max_args(&self) -> usize {
        match self.args {
            "" => 0,
            a if a.contains('…') => usize::MAX,
            a => a.matches(',').count() + 1,
        }
    }
}

/// A catalog entry for the UI (autocomplete and docs).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DynamicVarInfo {
    pub name: String,
    pub group: String,
    pub description: String,
    pub example: String,
    pub args: String,
}

/// Every dynamic variable, grouped and in display order.
pub fn catalog() -> &'static [DynamicVar] {
    catalog::CATALOG
}

/// The catalog in a serializable form.
pub fn catalog_info() -> Vec<DynamicVarInfo> {
    catalog()
        .iter()
        .map(|v| DynamicVarInfo {
            name: v.name.into(),
            group: v.group.into(),
            description: v.description.into(),
            example: v.example.into(),
            args: v.args.into(),
        })
        .collect()
}

/// The variable called `name` (with the `$`). Exact names win; otherwise case is
/// ignored, so `$randomIpv6` finds `$randomIPV6`.
pub fn find(name: &str) -> Option<&'static DynamicVar> {
    let all = catalog();
    all.iter().find(|v| v.name == name).or_else(|| all.iter().find(|v| v.name.eq_ignore_ascii_case(name)))
}

/// Generate a value. `expr` is the text inside the braces, e.g. `$randomInt` or
/// `$randomInt(1, 10)`. `None` means an unknown name or invalid arguments.
pub fn generate(expr: &str) -> Option<String> {
    let (name, args) = parse(expr)?;
    let var = find(name)?;
    if args.len() > var.max_args() {
        return None;
    }
    let mut ctx = Ctx { args, rng: rand::rng(), now: OffsetDateTime::now_utc() };
    (var.generator)(&mut ctx)
}

/// Split `$name(a, "b, c")` into the name and its unquoted arguments.
fn parse(expr: &str) -> Option<(&str, Vec<String>)> {
    let expr = expr.trim();
    let Some(open) = expr.find('(') else {
        return Some((expr, Vec::new()));
    };
    let inner = expr[open + 1..].strip_suffix(')')?;
    Some((expr[..open].trim_end(), split_args(inner)?))
}

fn split_args(inner: &str) -> Option<Vec<String>> {
    let mut args = Vec::new();
    if inner.trim().is_empty() {
        return Some(args);
    }
    let mut rest = inner;
    loop {
        let item = rest.trim_start();
        let (value, after) = match item.chars().next() {
            Some(quote @ ('"' | '\'')) => {
                let body = &item[1..];
                let end = body.find(quote)?;
                (&body[..end], body[end + 1..].trim_start())
            }
            _ => {
                let end = item.find(',').unwrap_or(item.len());
                (item[..end].trim_end(), &item[end..])
            }
        };
        args.push(value.to_string());
        if after.is_empty() {
            return Some(args);
        }
        // Anything but a comma after a closing quote is a mistake.
        rest = after.strip_prefix(',')?;
    }
}

/// What a generator gets: its arguments, a random source and one fixed "now".
struct Ctx {
    args: Vec<String>,
    rng: ThreadRng,
    now: OffsetDateTime,
}

impl Ctx {
    /// Argument `i` when given and not empty.
    fn arg(&self, i: usize) -> Option<&str> {
        self.args.get(i).map(String::as_str).filter(|a| !a.is_empty())
    }

    /// Integer argument `i`, or `default`; `None` when it is not an integer.
    fn int(&self, i: usize, default: i64) -> Option<i64> {
        self.arg(i).map_or(Some(default), |a| a.parse().ok())
    }

    /// Integer argument `i` (or `default`) that must be in `1..=max`.
    fn count(&self, i: usize, default: usize, max: usize) -> Option<usize> {
        let n = usize::try_from(self.int(i, default as i64)?).ok()?;
        (1..=max).contains(&n).then_some(n)
    }

    /// Integer argument `i` (or `default`) that must be in `min..=max`.
    fn int_in(&self, i: usize, default: i64, min: i64, max: i64) -> Option<i64> {
        self.int(i, default).filter(|n| (min..=max).contains(n))
    }

    /// Finite number argument `i`, or `default`.
    fn float(&self, i: usize, default: f64) -> Option<f64> {
        self.arg(i).map_or(Some(default), |a| a.parse::<f64>().ok().filter(|f| f.is_finite()))
    }

    /// Time offset argument `i` in seconds (`+1h`, `-7d`, `1w2d`, `0`, `now`), or `default`.
    fn offset(&self, i: usize, default: i64) -> Option<i64> {
        self.arg(i).map_or(Some(default), parse_offset)
    }

    /// A random index below `n` (`n` > 0).
    fn below(&mut self, n: usize) -> usize {
        self.rng.random_range(0..n)
    }

    /// A random integer in `lo..=hi`.
    fn between(&mut self, lo: i64, hi: i64) -> i64 {
        self.rng.random_range(lo..=hi)
    }

    fn pick<T: Copy>(&mut self, list: &[T]) -> T {
        list[self.below(list.len())]
    }

    /// A random entry of `list` as the value.
    fn one(&mut self, list: &[&str]) -> Option<String> {
        Some(self.pick(list).to_string())
    }

    /// `n` random characters from `alphabet` (ASCII).
    fn chars(&mut self, alphabet: &[u8], n: usize) -> String {
        (0..n).map(|_| alphabet[self.below(alphabet.len())] as char).collect()
    }

    /// Fill a pattern: `#` any digit, `N` 2-9, `M` 1-9; other characters stay.
    fn pattern(&mut self, pattern: &str) -> String {
        pattern
            .chars()
            .map(|p| match p {
                '#' => char::from(b'0' + self.rng.random_range(0..10u8)),
                'N' => char::from(b'0' + self.rng.random_range(2..10u8)),
                'M' => char::from(b'0' + self.rng.random_range(1..10u8)),
                other => other,
            })
            .collect()
    }
}

/// `+1h`, `-7d`, `1w2d`, `90s`, `0` or `now` as seconds. Units: s, m, h, d, w.
fn parse_offset(text: &str) -> Option<i64> {
    if text == "0" || text.eq_ignore_ascii_case("now") {
        return Some(0);
    }
    let (sign, mut rest) = match text.as_bytes().first()? {
        b'+' => (1, &text[1..]),
        b'-' => (-1, &text[1..]),
        _ => (1, text),
    };
    if rest.is_empty() {
        return None;
    }
    let mut total: i64 = 0;
    while !rest.is_empty() {
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 || digits > 12 {
            return None;
        }
        let n: i64 = rest[..digits].parse().ok()?;
        let unit = match rest.as_bytes().get(digits)? {
            b's' => 1,
            b'm' => 60,
            b'h' => 3_600,
            b'd' => 86_400,
            b'w' => 604_800,
            _ => return None,
        };
        total = total.checked_add(n.checked_mul(unit)?)?;
        rest = &rest[digits + 1..];
    }
    Some(sign * total)
}

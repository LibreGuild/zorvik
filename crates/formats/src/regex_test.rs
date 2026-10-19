//! Regex tester: run a pattern over a text the way the app uses regular expressions
//! (mock server and stream rules, load test captures), with Rust's regex syntax. Positions
//! are given in UTF-16 code units so the UI can highlight them in its own strings.

use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Matches listed; the count goes on to [`MAX_COUNT`].
pub const MAX_MATCHES: usize = 1000;
/// Matches counted.
pub const MAX_COUNT: usize = 100_000;
/// Size of the text.
pub const MAX_TEXT: usize = 10 * 1024 * 1024;
/// Characters of a match or group shown.
const MAX_SHOWN: usize = 500;
/// Compiled size a pattern may have, as for load test captures.
const SIZE_LIMIT: usize = 1 << 20;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RegexFlags {
    /// `i`: letters match upper and lower case.
    #[serde(default)]
    pub ignore_case: bool,
    /// `m`: `^` and `$` match at the start and end of each line.
    #[serde(default)]
    pub multi_line: bool,
    /// `s`: `.` matches line breaks too.
    #[serde(default)]
    pub dot_all: bool,
    /// `x`: whitespace and `# comments` in the pattern are ignored.
    #[serde(default)]
    pub ignore_whitespace: bool,
}

impl RegexFlags {
    /// The flags as an inline group, e.g. `(?im)`; empty without flags.
    pub fn inline(&self) -> String {
        let flags: String =
            [(self.ignore_case, 'i'), (self.multi_line, 'm'), (self.dot_all, 's'), (self.ignore_whitespace, 'x')]
                .into_iter()
                .filter_map(|(on, c)| on.then_some(c))
                .collect();
        if flags.is_empty() { flags } else { format!("(?{flags})") }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RegexGroup {
    /// UTF-16 offsets into the text.
    pub start: u32,
    pub end: u32,
    /// The matched text (shortened).
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RegexMatch {
    /// UTF-16 offsets into the text.
    pub start: u32,
    pub end: u32,
    /// The matched text (shortened).
    pub text: String,
    /// Groups 1…n; `None` when a group took no part in this match.
    pub groups: Vec<Option<RegexGroup>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RegexTestResult {
    /// The pattern with the flags written in it, as rules and captures take it: `(?i)abc`.
    pub pattern: String,
    /// Names of groups 1…n (`None` for unnamed groups).
    pub group_names: Vec<Option<String>>,
    /// The first [`MAX_MATCHES`] matches.
    pub matches: Vec<RegexMatch>,
    /// Matches found, up to [`MAX_COUNT`].
    pub count: u32,
    /// Counting stopped at [`MAX_COUNT`].
    pub count_capped: bool,
    /// What a load test capture with this pattern takes: the first match's first group, or
    /// the whole match when the pattern has no group. `None` when nothing is taken.
    pub capture: Option<String>,
}

/// Compile `pattern` with `flags` (plus the size limit the app uses).
pub fn compile(pattern: &str, flags: RegexFlags) -> Result<Regex, String> {
    RegexBuilder::new(&format!("{}{pattern}", flags.inline())).size_limit(SIZE_LIMIT).build().map_err(|e| e.to_string())
}

/// Run `pattern` over `text`. The error is the regex crate's description of what is wrong.
pub fn test_regex(pattern: &str, flags: RegexFlags, text: &str) -> Result<RegexTestResult, String> {
    if text.len() > MAX_TEXT {
        return Err(format!("The text is larger than {} MB", MAX_TEXT / (1024 * 1024)));
    }
    let re = compile(pattern, flags)?;
    let group_names: Vec<Option<String>> = re.capture_names().skip(1).map(|n| n.map(str::to_string)).collect();
    let mut matches = Vec::new();
    let mut count = 0usize;
    // UTF-16 position of a byte offset, walking forward from the last one asked.
    let mut cursor = (0usize, 0u32);
    let mut utf16_at = |byte: usize| -> u32 {
        let (from, units) = if byte >= cursor.0 { cursor } else { (0, 0) };
        let at = units + utf16_len(&text[from..byte]);
        cursor = (byte, at);
        at
    };
    let mut capture = None;
    for caps in re.captures_iter(text) {
        count += 1;
        let whole = caps.get(0).expect("group 0 always matches");
        if count == 1 {
            let taken = if re.captures_len() > 1 { caps.get(1) } else { Some(whole) };
            capture = taken.map(|m| m.as_str().to_string());
        }
        if matches.len() < MAX_MATCHES {
            let start = utf16_at(whole.start());
            let end = start + utf16_len(whole.as_str());
            let groups = caps
                .iter()
                .skip(1)
                .map(|g| {
                    g.map(|g| {
                        let at = start + utf16_len(&text[whole.start()..g.start()]);
                        RegexGroup { start: at, end: at + utf16_len(g.as_str()), text: shorten(g.as_str()) }
                    })
                })
                .collect();
            matches.push(RegexMatch { start, end, text: shorten(whole.as_str()), groups });
        }
        if count >= MAX_COUNT {
            break;
        }
    }
    Ok(RegexTestResult {
        pattern: format!("{}{pattern}", flags.inline()),
        group_names,
        matches,
        count: count as u32,
        count_capped: count >= MAX_COUNT,
        capture,
    })
}

fn utf16_len(s: &str) -> u32 {
    s.chars().map(|c| c.len_utf16() as u32).sum()
}

fn shorten(s: &str) -> String {
    match s.char_indices().nth(MAX_SHOWN) {
        Some((at, _)) => format!("{}…", &s[..at]),
        None => s.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(pattern: &str, text: &str) -> RegexTestResult {
        test_regex(pattern, RegexFlags::default(), text).unwrap()
    }

    #[test]
    fn finds_matches_and_groups() {
        let r = run(r"(?P<key>\w+)=(\d+)?", "a=1 b= c=33");
        assert_eq!(r.group_names, [Some("key".to_string()), None]);
        assert_eq!(r.count, 3);
        assert_eq!(
            r.matches[0],
            RegexMatch {
                start: 0,
                end: 3,
                text: "a=1".into(),
                groups: vec![
                    Some(RegexGroup { start: 0, end: 1, text: "a".into() }),
                    Some(RegexGroup { start: 2, end: 3, text: "1".into() })
                ],
            }
        );
        // An optional group that took no part.
        assert_eq!(r.matches[1].groups[1], None);
        assert_eq!((r.matches[2].start, r.matches[2].end), (7, 11));
        // A capture takes the first group of the first match.
        assert_eq!(r.capture.as_deref(), Some("a"));
        assert_eq!(run("[a-z]=", "x a=").capture.as_deref(), Some("a="));
        assert_eq!(run("zzz", "abc").capture, None);
        assert_eq!(run("zzz", "abc").count, 0);
    }

    #[test]
    fn positions_are_utf16_units() {
        // é is 2 bytes (1 unit), 😀 is 4 bytes (2 units).
        let r = run("b+", "é😀bb é😀b");
        let at: Vec<(u32, u32)> = r.matches.iter().map(|m| (m.start, m.end)).collect();
        assert_eq!(at, [(3, 5), (9, 10)]);
        let r = run("(😀)(b)", "é😀b");
        assert_eq!(r.matches[0].groups[1].as_ref().map(|g| (g.start, g.end)), Some((3, 4)));
    }

    #[test]
    fn flags_are_written_into_the_pattern() {
        let flags = RegexFlags { ignore_case: true, multi_line: true, ..Default::default() };
        let r = test_regex("^ok$", flags, "fine\nOK\nok").unwrap();
        assert_eq!(r.pattern, "(?im)^ok$");
        assert_eq!(r.count, 2);
        assert_eq!(run("^ok$", "fine\nOK\nok").count, 0);
        let dot = RegexFlags { dot_all: true, ignore_whitespace: true, ..Default::default() };
        assert_eq!(test_regex("a . b # comment", dot, "a\nb").unwrap().count, 1);
        assert_eq!(RegexFlags::default().inline(), "");
    }

    #[test]
    fn explains_bad_patterns_and_limits() {
        let e = test_regex("(", RegexFlags::default(), "x").unwrap_err();
        assert!(e.contains("unclosed group"), "{e}");
        // Backreferences and lookaround are not part of Rust's syntax.
        assert!(test_regex(r"(a)\1", RegexFlags::default(), "aa").is_err());
        assert!(test_regex("(?=a)", RegexFlags::default(), "a").is_err());
        let r = run("", "abc");
        assert_eq!(r.count, 4);
        let many = "a".repeat(MAX_COUNT + 10);
        let r = run("a", &many);
        assert_eq!((r.matches.len(), r.count as usize, r.count_capped), (MAX_MATCHES, MAX_COUNT, true));
    }
}

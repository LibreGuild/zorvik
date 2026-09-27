//! Reply rules of WebSocket/TCP/UDP servers: "when a message matches, reply
//! with …". Rules are compiled once per configuration.

use std::collections::BTreeSet;

use zorvik_engine::framing::{PayloadEncoding, parse_hex};
use zorvik_formats::{MatchKind, ReplyRule};
use zorvik_workspace::vars::VarContext;

struct Compiled {
    matcher: MatchKind,
    pattern: Vec<u8>,
    regex: Option<regex::bytes::Regex>,
    reply: String,
    delay_ms: u64,
}

/// A reply to send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    pub bytes: Vec<u8>,
    pub is_text: bool,
    pub delay_ms: u64,
}

pub struct Replier {
    rules: Vec<Compiled>,
    encoding: PayloadEncoding,
    /// Rules that could not be used (bad regex or hex), for the log.
    pub problems: Vec<String>,
}

impl Replier {
    pub fn new(rules: &[ReplyRule], encoding: PayloadEncoding) -> Self {
        let mut problems = Vec::new();
        let compiled = rules
            .iter()
            .enumerate()
            .filter(|(_, r)| r.enabled)
            .filter_map(|(i, r)| {
                let pattern = match (r.matcher, encoding) {
                    (MatchKind::Any | MatchKind::Regex, _) | (_, PayloadEncoding::Text) => {
                        r.pattern.as_bytes().to_vec()
                    }
                    (_, PayloadEncoding::Hex) => match parse_hex(&r.pattern) {
                        Ok(p) => p,
                        Err(e) => {
                            problems.push(format!("Rule {}: pattern {e}", i + 1));
                            return None;
                        }
                    },
                };
                let regex = if r.matcher == MatchKind::Regex {
                    match regex::bytes::Regex::new(&r.pattern) {
                        Ok(re) => Some(re),
                        Err(e) => {
                            problems.push(format!("Rule {}: invalid regex: {e}", i + 1));
                            return None;
                        }
                    }
                } else {
                    None
                };
                // Otherwise the rule would answer with nothing, without saying why.
                if encoding == PayloadEncoding::Hex
                    && let Err(e) = parse_hex(&r.reply)
                {
                    problems.push(format!("Rule {}: reply {e}", i + 1));
                    return None;
                }
                Some(Compiled { matcher: r.matcher, pattern, regex, reply: r.reply.clone(), delay_ms: r.delay_ms })
            })
            .collect();
        Self { rules: compiled, encoding, problems }
    }

    /// The reply of the first rule matching `message`. Text replies may use
    /// `{{message}}`, dynamic variables (`{{$uuid}}`) and the environment's variables.
    pub fn reply(&self, message: &[u8], vars: &VarContext) -> Option<Reply> {
        let rule = self.rules.iter().find(|r| matches(r, message))?;
        Some(render(&rule.reply, self.encoding, message, vars, rule.delay_ms))
    }
}

fn matches(rule: &Compiled, message: &[u8]) -> bool {
    match rule.matcher {
        MatchKind::Any => true,
        MatchKind::Contains => {
            rule.pattern.is_empty() || message.windows(rule.pattern.len()).any(|w| w == rule.pattern)
        }
        // Line-based protocols end messages with a line break; it is not part of what one types.
        MatchKind::Exact => trim_eol(message) == trim_eol(&rule.pattern),
        MatchKind::Regex => rule.regex.as_ref().is_some_and(|re| re.is_match(message)),
    }
}

fn trim_eol(b: &[u8]) -> &[u8] {
    let mut end = b.len();
    while end > 0 && matches!(b[end - 1], b'\n' | b'\r') {
        end -= 1;
    }
    &b[..end]
}

/// Render a greeting or reply (text with variables, or hex bytes).
pub fn render(template: &str, encoding: PayloadEncoding, message: &[u8], vars: &VarContext, delay_ms: u64) -> Reply {
    match encoding {
        PayloadEncoding::Hex => match parse_hex(template) {
            Ok(bytes) => Reply { bytes, is_text: false, delay_ms },
            Err(_) => Reply { bytes: Vec::new(), is_text: false, delay_ms },
        },
        PayloadEncoding::Text => {
            // The message is inserted after the variables are rendered: a client
            // sending `{{token}}` gets that text back, never the variable's value.
            let message = String::from_utf8_lossy(trim_eol(message));
            let parts: Vec<String> =
                template.split("{{message}}").map(|part| vars.render(part, &mut BTreeSet::new())).collect();
            Reply { bytes: parts.join(message.as_ref()).into_bytes(), is_text: true, delay_ms }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(matcher: MatchKind, pattern: &str, reply: &str) -> ReplyRule {
        ReplyRule { matcher, pattern: pattern.into(), reply: reply.into(), ..Default::default() }
    }

    #[test]
    fn text_rules_in_order() {
        let r = Replier::new(
            &[
                rule(MatchKind::Exact, "PING", "PONG"),
                rule(MatchKind::Regex, r"^GET (\w+)", "value of {{message}}"),
                rule(MatchKind::Contains, "id", "{{$uuid}}"),
                ReplyRule { enabled: false, ..rule(MatchKind::Any, "", "never") },
            ],
            PayloadEncoding::Text,
        );
        let vars = VarContext::new();
        assert_eq!(r.reply(b"PING\r\n", &vars).unwrap().bytes, b"PONG");
        assert_eq!(r.reply(b"GET key", &vars).unwrap().bytes, b"value of GET key");
        assert_eq!(r.reply(b"my id", &vars).unwrap().bytes.len(), 36);
        assert!(r.reply(b"nothing", &vars).is_none());
    }

    #[test]
    fn client_text_is_never_expanded() {
        let mut vars = VarContext::new();
        vars.push_layer(&[zorvik_formats::Variable {
            key: "token".into(),
            value: "s3cret".into(),
            enabled: true,
            secret: true,
        }]);
        let r = Replier::new(&[rule(MatchKind::Any, "", "[{{message}}] {{token}}")], PayloadEncoding::Text);
        let reply = r.reply(b"{{token}} {{message}}\n", &vars).unwrap();
        assert_eq!(String::from_utf8(reply.bytes).unwrap(), "[{{token}} {{message}}] s3cret");
    }

    #[test]
    fn hex_rules_and_problems() {
        let r = Replier::new(
            &[
                rule(MatchKind::Contains, "01 02", "ff 00"),
                rule(MatchKind::Exact, "zz", "00"),
                rule(MatchKind::Regex, "(", ""),
                rule(MatchKind::Any, "", "zz"),
            ],
            PayloadEncoding::Hex,
        );
        assert_eq!(r.problems.len(), 3, "{:?}", r.problems);
        assert!(r.problems[2].starts_with("Rule 4: reply"), "{:?}", r.problems);
        let reply = r.reply(&[0, 1, 2, 3], &VarContext::new()).unwrap();
        assert_eq!((reply.bytes, reply.is_text), (vec![0xff, 0], false));
    }
}

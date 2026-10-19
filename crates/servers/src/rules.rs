//! Reply rules of WebSocket/TCP/UDP servers: "when a message matches, reply
//! with …". Rules are compiled once per configuration.

use std::borrow::Cow;
use std::collections::BTreeSet;

use serde_json::Value;
use zorvik_engine::framing::{PayloadEncoding, parse_hex};
use zorvik_formats::matching::{JsonPath, MessageMatcher, trim_eol, value_text};
use zorvik_formats::{MatchKind, ReplyRule};
use zorvik_workspace::vars::VarContext;

use crate::template::MAX_OUTPUT;

struct Compiled {
    matcher: MessageMatcher,
    reply: String,
    delay_ms: u64,
    close: Option<u16>,
}

/// A reply to send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    pub bytes: Vec<u8>,
    pub is_text: bool,
    pub delay_ms: u64,
    /// WebSocket: close the connection with this code after the reply (if any).
    pub close: Option<u16>,
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
                    (MatchKind::Any | MatchKind::Regex | MatchKind::Json, _) | (_, PayloadEncoding::Text) => {
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
                if let Some(code) = r.close
                    && !(code == 1000 || (3000..=4999).contains(&code))
                {
                    problems.push(format!("Rule {}: close code {code} can't be sent; use 1000 or 3000-4999", i + 1));
                    return None;
                }
                let matcher = match MessageMatcher::new(r.matcher, &pattern) {
                    Ok(m) => m,
                    Err(e) => {
                        problems.push(format!("Rule {}: {e}", i + 1));
                        return None;
                    }
                };
                // Otherwise the rule would answer with nothing, without saying why.
                if encoding == PayloadEncoding::Hex
                    && let Err(e) = parse_hex(&r.reply)
                {
                    problems.push(format!("Rule {}: reply {e}", i + 1));
                    return None;
                }
                Some(Compiled { matcher, reply: r.reply.clone(), delay_ms: r.delay_ms, close: r.close })
            })
            .collect();
        Self { rules: compiled, encoding, problems }
    }

    /// The reply of the first rule matching `message`. Text replies may use
    /// `{{message}}`, `{{message.path}}`, dynamic variables (`{{$uuid}}`) and the
    /// environment's variables.
    pub fn reply(&self, message: &[u8], vars: &VarContext) -> Option<Reply> {
        let rule = self.rules.iter().find(|r| r.matcher.matches(message))?;
        let mut reply = render(&rule.reply, self.encoding, message, vars, rule.delay_ms);
        reply.close = rule.close;
        Some(reply)
    }
}

/// Render a greeting or reply (text with variables, or hex bytes). `{{message}}` is the
/// incoming message; when it is JSON, `{{message.id}}` or `{{message.items[0].name}}` is one
/// value of it (text as it is, other values as JSON, nothing when it isn't there).
pub fn render(template: &str, encoding: PayloadEncoding, message: &[u8], vars: &VarContext, delay_ms: u64) -> Reply {
    match encoding {
        PayloadEncoding::Hex => match parse_hex(template) {
            Ok(bytes) => Reply { bytes, is_text: false, delay_ms, close: None },
            Err(_) => Reply { bytes: Vec::new(), is_text: false, delay_ms, close: None },
        },
        PayloadEncoding::Text => {
            // The message's values are inserted after the variables are rendered: a client
            // sending `{{token}}` gets that text back, never the variable's value.
            let message = trim_eol(message);
            let text = String::from_utf8_lossy(message);
            let mut json: Option<Option<Value>> = None;
            let fill = |text: &str| vars.render(text, &mut BTreeSet::new());
            let mut out = String::new();
            let mut rest = template;
            while let Some(start) = rest.find("{{message") {
                let tail = &rest[start + "{{message".len()..];
                let Some(len) = tail.find("}}") else { break };
                let key = &tail[..len];
                let value = if key.is_empty() {
                    Some(Cow::Borrowed(text.as_ref()))
                } else if key.starts_with(['.', '[']) {
                    let root = json.get_or_insert_with(|| serde_json::from_slice(message).ok());
                    Some(Cow::Owned(match (JsonPath::parse(key), root.as_ref()) {
                        (Ok(path), Some(root)) => path.get(root).and_then(value_text).unwrap_or_default(),
                        _ => String::new(),
                    }))
                } else {
                    // `{{messages}}` and the like are other variables.
                    None
                };
                let end = start + "{{message".len() + len + 2;
                match value {
                    Some(value) => {
                        out.push_str(&fill(&rest[..start]));
                        // A large message in many placeholders must not grow the reply without
                        // end: past the limit they stay as written.
                        if out.len() + value.len() > MAX_OUTPUT {
                            out.push_str(&rest[start..end]);
                        } else {
                            out.push_str(&value);
                        }
                    }
                    None => out.push_str(&fill(&rest[..end])),
                }
                rest = &rest[end..];
            }
            out.push_str(&fill(rest));
            Reply { bytes: out.into_bytes(), is_text: true, delay_ms, close: None }
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
        // A JSON value of the message is inserted as it is too.
        let r = Replier::new(&[rule(MatchKind::Any, "", "{{message.q}} {{token}}")], PayloadEncoding::Text);
        let reply = r.reply(br#"{"q": "{{token}}"}"#, &vars).unwrap();
        assert_eq!(String::from_utf8(reply.bytes).unwrap(), "{{token}} s3cret");
    }

    #[test]
    fn json_values_of_the_message_and_json_rules() {
        let r = Replier::new(
            &[
                ReplyRule { close: Some(4001), ..rule(MatchKind::Json, "$.op == \"bye\"", "") },
                rule(
                    MatchKind::Json,
                    "$.id",
                    r#"{"id": {{message.id}}, "echo": "{{message.params.text}}", "first": {{message[ 'list' ][0]}}, "none": "{{message.nope}}"}"#,
                ),
            ],
            PayloadEncoding::Text,
        );
        let vars = VarContext::new();
        let reply = r.reply(br#"{"id": 7, "params": {"text": "hi"}, "list": [{"a": 1}]}"#, &vars).unwrap();
        assert_eq!(String::from_utf8(reply.bytes).unwrap(), r#"{"id": 7, "echo": "hi", "first": {"a":1}, "none": ""}"#);
        assert_eq!(reply.close, None);
        let bye = r.reply(br#"{"op": "bye"}"#, &vars).unwrap();
        assert_eq!((bye.bytes.len(), bye.close), (0, Some(4001)));
        assert!(r.reply(b"not json", &vars).is_none());
        // Codes a server may not send are refused, like a bad regex.
        let bad =
            Replier::new(&[ReplyRule { close: Some(1006), ..rule(MatchKind::Any, "", "") }], PayloadEncoding::Text);
        assert!(bad.problems[0].contains("close code 1006"), "{:?}", bad.problems);
        // Other names that start with "message" are variables.
        let r = Replier::new(&[rule(MatchKind::Any, "", "{{messages}}/{{message}}")], PayloadEncoding::Text);
        assert_eq!(r.reply(b"x\n", &vars).unwrap().bytes, b"{{messages}}/x");
    }

    #[test]
    fn large_messages_in_many_placeholders_stay_bounded() {
        let placeholders = "{{message}}{{message.a}}".repeat(40);
        let r = Replier::new(&[rule(MatchKind::Any, "", &placeholders)], PayloadEncoding::Text);
        let message = format!(r#"{{"a": "{}"}}"#, "x".repeat(1 << 20));
        let reply = r.reply(message.as_bytes(), &VarContext::new()).unwrap().bytes;
        assert!(reply.len() <= MAX_OUTPUT + placeholders.len(), "{}", reply.len());
        // Placeholders past the limit stay as written.
        assert!(reply.ends_with(b"{{message}}{{message.a}}"));
        assert!(reply.starts_with(message.as_bytes()));
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

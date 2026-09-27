//! Keep credentials out of what agents get back: secret variable values become
//! `{{name}}`, credentials sent in headers become `••••••` wherever they show up.

use zorvik_engine::Header;
use zorvik_workspace::formats::Variable;

use crate::MASK;

/// Headers whose values are credentials.
const SENSITIVE_HEADERS: &[&str] = &[
    "authorization",
    "proxy-authorization",
    "cookie",
    "set-cookie",
    "x-api-key",
    "api-key",
    "apikey",
    "x-auth-token",
    "x-access-token",
    "x-csrf-token",
    "x-xsrf-token",
    "x-amz-security-token",
];

/// Shorter values are left alone (masking "a" everywhere would ruin the text).
const MIN_SECRET_LEN: usize = 4;
/// Parts of credential headers (`Bearer <token>`, cookie values) are hidden from this
/// length: tokens are long, words like "Bearer" or a cookie's "dark" are not.
const MIN_HEADER_PART_LEN: usize = 8;

pub(crate) fn is_sensitive_header(name: &str) -> bool {
    SENSITIVE_HEADERS.iter().any(|h| name.eq_ignore_ascii_case(h))
}

#[derive(Default)]
pub(crate) struct Redactor {
    /// (value, replacement), longest value first.
    rules: Vec<(String, String)>,
}

impl Redactor {
    /// Secret variables (enabled ones) as `{{name}}`.
    pub fn new(secrets: &[Variable]) -> Self {
        let mut r = Self::default();
        for v in secrets.iter().filter(|v| v.secret && v.enabled) {
            r.add(&v.value, &format!("{{{{{}}}}}", v.key.trim()), MIN_SECRET_LEN);
        }
        r
    }

    /// Also hide what these headers carried (e.g. the token of `Authorization: Bearer …`).
    pub fn with_headers(mut self, headers: &[Header]) -> Self {
        for h in headers.iter().filter(|h| is_sensitive_header(&h.name)) {
            self.add(&h.value, MASK, MIN_SECRET_LEN);
            for part in h.value.split([' ', ';', ',']) {
                // `Cookie: a=b; c=d`: the values.
                let value = part.split_once('=').map_or(part, |(_, v)| v);
                self.add(value, MASK, MIN_HEADER_PART_LEN);
            }
        }
        self
    }

    fn add(&mut self, value: &str, replacement: &str, min_len: usize) {
        let value = value.trim();
        if value.chars().count() < min_len || self.rules.iter().any(|(v, _)| v == value) {
            return;
        }
        let encoded: String = url::form_urlencoded::byte_serialize(value.as_bytes()).collect();
        if encoded != value {
            self.rules.push((encoded, replacement.to_string()));
        }
        self.rules.push((value.to_string(), replacement.to_string()));
        self.rules.sort_by_key(|(v, _)| std::cmp::Reverse(v.len()));
    }

    pub fn text(&self, text: &str) -> String {
        let mut out = text.to_string();
        for (value, replacement) in &self.rules {
            if out.contains(value.as_str()) {
                out = out.replace(value.as_str(), replacement);
            }
        }
        out
    }

    /// Header lines as `name: value`; credential headers masked whole.
    pub fn headers(&self, headers: &[Header]) -> Vec<String> {
        headers
            .iter()
            .map(|h| {
                let value = if is_sensitive_header(&h.name) { MASK.to_string() } else { self.text(&h.value) };
                format!("{}: {value}", h.name)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn var(key: &str, value: &str, secret: bool) -> Variable {
        Variable { key: key.into(), value: value.into(), enabled: true, secret }
    }

    #[test]
    fn secrets_and_sent_credentials_are_hidden() {
        let r = Redactor::new(&[
            var("apiKey", "k3y-v@lue", true),
            var("host", "example.com", false),
            var("pin", "12", true),
        ])
        .with_headers(&[
            Header::new("Authorization", "Bearer abc.def.ghi"),
            Header::new("Cookie", "session=s3ss10n-value; theme=dark"),
            Header::new("Accept", "application/json"),
        ]);
        let body = r#"{"key":"k3y-v@lue","q":"k3y-v%40lue","auth":"Bearer abc.def.ghi","s":"s3ss10n-value","host":"example.com","n":12,"t":"dark"}"#;
        let out = r.text(body);
        assert!(!out.contains("k3y") && !out.contains("abc.def") && !out.contains("s3ss10n"), "{out}");
        assert!(out.contains("{{apiKey}}") && out.contains("example.com") && out.contains("\"n\":12"), "{out}");
        assert!(out.contains("\"t\":\"dark\"") && !out.contains("Bearer"), "{out}");
        let lines =
            r.headers(&[Header::new("authorization", "Bearer abc.def.ghi"), Header::new("x-trace", "k3y-v@lue")]);
        assert_eq!(lines, vec![format!("authorization: {MASK}"), "x-trace: {{apiKey}}".to_string()]);
    }
}

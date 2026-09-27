//! Templates in mock responses and SSE events. `{{request.…}}` placeholders
//! take values from the request being answered; every other `{{name}}` is an
//! environment/workspace variable or a dynamic one (`{{$uuid}}`).
//!
//! Request values are inserted as they are: a client sending `{{token}}` in a
//! body echoed with `{{request.body}}` gets that text back, never the value of
//! the `token` variable.

use std::borrow::Cow;
use std::collections::BTreeSet;

use zorvik_engine::Header;
use zorvik_workspace::vars::VarContext;

/// Most bytes one rendered template may grow to; placeholders past it stay as written.
const MAX_OUTPUT: usize = 32 << 20;

/// The request a template is rendered for.
#[derive(Default)]
pub(crate) struct RequestValues<'a> {
    pub method: &'a str,
    /// Path without the query.
    pub path: &'a str,
    /// Raw query string (without `?`).
    pub query_string: &'a str,
    /// Values of the route's `:name` segments (`*` for a trailing wildcard).
    pub params: &'a [(String, String)],
    /// Decoded query parameters in order.
    pub query: &'a [(String, String)],
    pub headers: &'a [Header],
    /// Body as text (lossy UTF-8).
    pub body: &'a str,
}

impl RequestValues<'_> {
    /// Value of `request.<key>`; unknown keys and missing values are empty.
    fn get(&self, key: &str) -> Cow<'_, str> {
        let pick = |list: &'_ [(String, String)], name: &str| -> String {
            list.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone()).unwrap_or_default()
        };
        match key {
            "method" => Cow::Borrowed(self.method),
            "path" => Cow::Borrowed(self.path),
            "url" if self.query_string.is_empty() => Cow::Borrowed(self.path),
            "url" => Cow::Owned(format!("{}?{}", self.path, self.query_string)),
            "body" => Cow::Borrowed(self.body),
            _ => {
                if let Some(name) = key.strip_prefix("params.") {
                    Cow::Owned(pick(self.params, name))
                } else if let Some(name) = key.strip_prefix("query.") {
                    Cow::Owned(pick(self.query, name))
                } else if let Some(name) = key.strip_prefix("headers.") {
                    let found = self.headers.iter().find(|h| h.name.eq_ignore_ascii_case(name));
                    Cow::Borrowed(found.map_or("", |h| h.value.as_str()))
                } else {
                    Cow::Borrowed("")
                }
            }
        }
    }
}

/// Render `template` for `request` (or without request values, e.g. a greeting).
pub(crate) fn render(template: &str, request: Option<&RequestValues>, vars: &VarContext) -> String {
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
        let raw = &after[..end];
        // `{{` inside the name: the first braces were literal text.
        if let Some(inner) = raw.rfind("{{") {
            out.push_str(&rest[start..start + 2 + inner]);
            rest = &after[inner..];
            continue;
        }
        let whole = &rest[start..start + 2 + end + 2];
        let name = raw.trim();
        if name.is_empty() || raw.contains('\n') {
            out.push_str("{{");
            rest = after;
            continue;
        }
        let value = match (name.strip_prefix("request."), request) {
            (Some(key), Some(request)) => request.get(key),
            // Variables (unknown names stay as written, like everywhere else).
            _ => Cow::Owned(vars.render(whole, &mut BTreeSet::new())),
        };
        if out.len() + value.len() > MAX_OUTPUT {
            out.push_str(whole);
        } else {
            out.push_str(&value);
        }
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use zorvik_formats::Variable;

    fn vars() -> VarContext {
        let mut v = VarContext::new();
        v.push_layer(&[
            Variable { key: "host".into(), value: "example.test".into(), enabled: true, secret: false },
            Variable { key: "token".into(), value: "s3cret".into(), enabled: true, secret: false },
        ]);
        v
    }

    #[test]
    fn request_values_and_variables() {
        let params = [("id".to_string(), "42".to_string())];
        let query = [("q".to_string(), "a b".to_string())];
        let headers = [Header::new("X-Trace", "t-1")];
        let req = RequestValues {
            method: "POST",
            path: "/users/42",
            query_string: "q=a%20b",
            params: &params,
            query: &query,
            headers: &headers,
            body: "{\"x\":1}",
        };
        let out = render(
            "{{request.method}} {{request.url}} id={{ request.params.id }} q={{request.query.q}} \
             trace={{request.headers.x-trace}} missing=[{{request.query.nope}}] body={{request.body}} \
             host={{host}} {{unknown}} {{$uuid}}",
            Some(&req),
            &vars(),
        );
        assert!(
            out.starts_with(
                "POST /users/42?q=a%20b id=42 q=a b trace=t-1 missing=[] body={\"x\":1} host=example.test {{unknown}} "
            ),
            "{out}"
        );
        assert_eq!(out.rsplit(' ').next().unwrap().len(), 36, "{out}");
    }

    #[test]
    fn request_text_is_never_expanded() {
        let req = RequestValues { body: "{{token}} {{request.method}}", method: "GET", ..Default::default() };
        assert_eq!(render("echo: {{request.body}}", Some(&req), &vars()), "echo: {{token}} {{request.method}}");
        // Without a request, `request.*` is just an unknown variable.
        assert_eq!(render("{{request.body}}", None, &vars()), "{{request.body}}");
    }

    #[test]
    fn literal_braces_and_limits() {
        let v = vars();
        assert_eq!(render("a {{ b {{host}} }} {{", None, &v), "a {{ b example.test }} {{");
        assert_eq!(render("{{}} {{\n}}", None, &v), "{{}} {{\n}}");
        let body = "x".repeat(1 << 20);
        let req = RequestValues { body: &body, ..Default::default() };
        let out = render(&"{{request.body}}".repeat(64), Some(&req), &v);
        assert!(out.len() <= MAX_OUTPUT + 64 * 16 && out.ends_with("{{request.body}}"), "{}", out.len());
    }
}

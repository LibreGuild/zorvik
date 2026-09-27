//! Turn a saved [`Request`] into an engine [`HttpRequest`]: substitute
//! variables, apply inherited headers/auth, encode the body.

use std::collections::BTreeSet;
use std::io::Read as _;
use std::path::Path;

use base64::Engine as _;
use bytes::Bytes;
use zorvik_engine::{Header, HttpRequest};
use zorvik_formats::{
    ApiKeyLocation, Auth, BodyType, FolderMeta, KeyValue, OAuth2Config, Request, RequestKind, WorkspaceMeta,
};

use crate::error::{Error, ErrorCode, Result};
use crate::vars::VarContext;

/// Largest file accepted as a request body or multipart part.
pub const MAX_FILE_BODY: u64 = 100 * 1024 * 1024;

/// Everything a request inherits from its location in the workspace.
pub struct Inheritance<'a> {
    pub workspace: &'a WorkspaceMeta,
    /// Folders from the top level down to the request's parent.
    pub folders: &'a [FolderMeta],
    /// Base directory for relative file paths (the workspace root).
    pub base_dir: &'a Path,
    /// Allow body files outside `base_dir`. Off unless the user opts in: a shared
    /// workspace could otherwise upload any file on this machine (`~/.ssh/id_rsa`).
    pub outside_files: bool,
}

#[derive(Debug)]
pub struct Resolved {
    pub request: HttpRequest,
    /// OAuth2 config (variables substituted) when a token must be obtained and
    /// applied with [`apply_token`].
    pub oauth2: Option<OAuth2Config>,
    /// Variables referenced but not defined.
    pub unresolved: Vec<String>,
}

/// The auth that applies: the request's own, else the nearest folder's, else the workspace's.
pub fn effective_auth<'a>(request_auth: &'a Auth, inherit: &'a Inheritance<'_>) -> &'a Auth {
    if !matches!(request_auth, Auth::Inherit) {
        return request_auth;
    }
    inherit
        .folders
        .iter()
        .rev()
        .map(|f| &f.auth)
        .find(|a| !matches!(a, Auth::Inherit))
        .unwrap_or(&inherit.workspace.auth)
}

pub fn resolve(request: &Request, inherit: &Inheritance<'_>, vars: &VarContext) -> Result<Resolved> {
    let mut missing = BTreeSet::new();
    let mut render = |s: &str| vars.render(s, &mut missing);

    let mut url = render(request.url.trim());
    let mut empty_path_params = Vec::new();
    url = substitute_path_params(&url, &request.path_params, &mut render, &mut empty_path_params);

    // Headers: workspace, then folders, then the request; later definitions of
    // the same name replace earlier inherited ones.
    let mut headers: Vec<Header> = Vec::new();
    let layers = std::iter::once(&inherit.workspace.headers)
        .chain(inherit.folders.iter().map(|f| &f.headers))
        .chain(std::iter::once(&request.headers));
    for layer in layers {
        let rendered: Vec<Header> =
            enabled(layer).map(|h| Header::new(render(&h.key).trim(), render(&h.value))).collect();
        headers.retain(|existing| !rendered.iter().any(|h| h.name.eq_ignore_ascii_case(&existing.name)));
        headers.extend(rendered);
    }

    let (body, content_type) = if request.kind == RequestKind::Http {
        encode_body(request, inherit, &mut render)?
    } else {
        (Bytes::new(), None)
    };
    if let Some(ct) = content_type
        && !has_header(&headers, "content-type")
    {
        headers.push(Header::new("Content-Type", ct));
    }

    let mut oauth2 = None;
    match effective_auth(&request.auth, inherit) {
        Auth::Inherit | Auth::None => {}
        Auth::Basic { username, password } => {
            if !has_header(&headers, "authorization") {
                let token = base64::engine::general_purpose::STANDARD.encode(format!(
                    "{}:{}",
                    render(username),
                    render(password)
                ));
                headers.push(Header::new("Authorization", format!("Basic {token}")));
            }
        }
        Auth::Bearer { token, prefix } => {
            if !has_header(&headers, "authorization") {
                headers.push(Header::new("Authorization", with_prefix(&render(prefix), &render(token))));
            }
        }
        Auth::ApiKey { key, value, location } => {
            let (key, value) = (render(key).trim().to_string(), render(value));
            if !key.is_empty() {
                match location {
                    ApiKeyLocation::Header => {
                        if !has_header(&headers, &key) {
                            headers.push(Header::new(key, value));
                        }
                    }
                    ApiKeyLocation::Query => url = append_query(&url, &key, &value),
                }
            }
        }
        Auth::OAuth2(config) => {
            if !has_header(&headers, "authorization") {
                oauth2 = Some(render_oauth2(config, &mut render));
            }
        }
    }

    Ok(Resolved {
        request: HttpRequest { method: request.method.trim().to_string(), url, headers, body },
        oauth2,
        unresolved: missing.into_iter().chain(empty_path_params).collect(),
    })
}

/// `text` with each `{{…}}` left by rendering (an undefined variable) as `null`.
fn undefined_as_null(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        let Some(len) = rest[start..].find("}}") else { break };
        out.push_str(&rest[..start]);
        out.push_str("null");
        rest = &rest[start + len + 2..];
    }
    out.push_str(rest);
    out
}

/// Fail early when the URL's host still contains `{{variables}}`: sending would
/// only produce a confusing DNS error. Undefined variables elsewhere are sent
/// as-is and reported as a warning.
pub fn check_url_variables(resolved: &Resolved) -> Result<()> {
    let url = &resolved.request.url;
    let after_scheme = url.split_once("://").map_or(url.as_str(), |(_, rest)| rest);
    let authority = after_scheme.split(['/', '?', '#']).next().unwrap_or_default();
    if !authority.contains("{{") && !url.trim_start().starts_with("{{") {
        return Ok(());
    }
    let names: Vec<&String> = resolved
        .unresolved
        .iter()
        .filter(|n| authority.contains(n.as_str()) || url.trim_start().starts_with("{{"))
        .collect();
    let list = if names.is_empty() {
        "A variable".to_string()
    } else {
        names.iter().map(|n| format!("{{{{{n}}}}}")).collect::<Vec<_>>().join(", ")
    };
    Err(Error::new(
        ErrorCode::UndefinedVariable,
        format!(
            "{list} in the URL is not defined. Select an environment that defines it, or add it under Environments. Names are case-sensitive."
        ),
    ))
}

/// Add `Authorization: <prefix> <token>` for an OAuth2 access token.
pub fn apply_token(resolved: &mut Resolved, access_token: &str) {
    let prefix = resolved.oauth2.as_ref().map(|c| c.header_prefix.clone()).unwrap_or_else(|| "Bearer".into());
    resolved.request.headers.push(Header::new("Authorization", with_prefix(&prefix, access_token)));
}

fn with_prefix(prefix: &str, token: &str) -> String {
    let prefix = prefix.trim();
    if prefix.is_empty() { token.to_string() } else { format!("{prefix} {token}") }
}

pub fn render_oauth2(config: &OAuth2Config, render: &mut impl FnMut(&str) -> String) -> OAuth2Config {
    OAuth2Config {
        grant_type: config.grant_type,
        token_url: render(&config.token_url).trim().to_string(),
        auth_url: render(&config.auth_url).trim().to_string(),
        redirect_uri: render(&config.redirect_uri).trim().to_string(),
        client_id: render(&config.client_id).trim().to_string(),
        client_secret: render(&config.client_secret),
        scope: render(&config.scope),
        audience: render(&config.audience),
        username: render(&config.username),
        password: render(&config.password),
        client_auth: config.client_auth,
        pkce: config.pkce,
        header_prefix: render(&config.header_prefix),
    }
}

fn enabled(list: &[KeyValue]) -> impl Iterator<Item = &KeyValue> {
    list.iter().filter(|kv| kv.enabled && !kv.key.trim().is_empty())
}

fn has_header(headers: &[Header], name: &str) -> bool {
    headers.iter().any(|h| h.name.eq_ignore_ascii_case(name))
}

/// Replace `:name` path segments (not in the host/port or query) with values.
fn substitute_path_params(
    url: &str,
    params: &[KeyValue],
    render: &mut impl FnMut(&str) -> String,
    missing: &mut Vec<String>,
) -> String {
    if !url.contains("/:") {
        return url.to_string();
    }
    let (before_query, query) = match url.find(['?', '#']) {
        Some(i) => url.split_at(i),
        None => (url, ""),
    };
    let path_start = match before_query.find("://") {
        Some(i) => before_query[i + 3..].find('/').map(|p| p + i + 3),
        None => before_query.find('/'),
    };
    let Some(path_start) = path_start else { return url.to_string() };
    let (origin, path) = before_query.split_at(path_start);
    let path = path
        .split('/')
        .map(|segment| match segment.strip_prefix(':') {
            Some(name) => match params.iter().find(|p| p.enabled && p.key == name) {
                Some(p) if !p.value.is_empty() => encode_path_segment(&render(&p.value)),
                _ => {
                    missing.push(segment.to_string());
                    segment.to_string()
                }
            },
            None => segment.to_string(),
        })
        .collect::<Vec<_>>()
        .join("/");
    format!("{origin}{path}{query}")
}

fn encode_path_segment(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect::<String>().replace('+', "%20")
}

fn append_query(url: &str, key: &str, value: &str) -> String {
    let (base, fragment) = match url.find('#') {
        Some(i) => url.split_at(i),
        None => (url, ""),
    };
    let pair = url::form_urlencoded::Serializer::new(String::new()).append_pair(key, value).finish();
    let sep = if base.contains('?') { if base.ends_with('?') || base.ends_with('&') { "" } else { "&" } } else { "?" };
    format!("{base}{sep}{pair}{fragment}")
}

fn read_body_file(path: &str, inherit: &Inheritance) -> Result<(Vec<u8>, String)> {
    if path.trim().is_empty() {
        return Err(Error::invalid("No file selected for the request body"));
    }
    let p = Path::new(path.trim());
    let full = if p.is_absolute() { p.to_path_buf() } else { inherit.base_dir.join(p) };
    let meta = std::fs::metadata(&full).map_err(|e| Error::io(format!("Body file '{}'", full.display()), e))?;
    // Canonical paths: `..` and links that lead out of the workspace count as outside.
    let inside = || {
        let base = std::fs::canonicalize(inherit.base_dir).ok()?;
        Some(std::fs::canonicalize(&full).ok()?.starts_with(base))
    };
    if !inherit.outside_files && inside() != Some(true) {
        return Err(Error::invalid(format!(
            "Body file '{}' is outside the workspace folder. Move it into the workspace, \
             or allow files outside the workspace in Settings → Data & privacy.",
            full.display()
        )));
    }
    // Devices such as /dev/zero report size 0 and never end.
    if !meta.is_file() {
        return Err(Error::invalid(format!("Body file '{}' is not a regular file", full.display())));
    }
    let too_big = || Error::invalid(format!("Body file '{}' is larger than 100 MB", full.display()));
    if meta.len() > MAX_FILE_BODY {
        return Err(too_big());
    }
    let mut data = Vec::new();
    std::fs::File::open(&full)
        .and_then(|f| f.take(MAX_FILE_BODY + 1).read_to_end(&mut data))
        .map_err(|e| Error::io(format!("Could not read '{}'", full.display()), e))?;
    if data.len() as u64 > MAX_FILE_BODY {
        return Err(too_big());
    }
    let name = full.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into());
    Ok((data, name))
}

fn encode_body(
    request: &Request,
    inherit: &Inheritance,
    render: &mut impl FnMut(&str) -> String,
) -> Result<(Bytes, Option<String>)> {
    let body = &request.body;
    Ok(match body.body_type {
        BodyType::None => (Bytes::new(), None),
        BodyType::Json => (Bytes::from(render(&body.text)), Some("application/json".into())),
        BodyType::Xml => (Bytes::from(render(&body.text)), Some("application/xml".into())),
        BodyType::Text => (
            Bytes::from(render(&body.text)),
            Some(body.content_type.clone().filter(|c| !c.trim().is_empty()).unwrap_or_else(|| "text/plain".into())),
        ),
        BodyType::FormUrlencoded => {
            let mut s = url::form_urlencoded::Serializer::new(String::new());
            for kv in enabled(&body.form) {
                s.append_pair(&render(&kv.key), &render(&kv.value));
            }
            (Bytes::from(s.finish()), Some("application/x-www-form-urlencoded".into()))
        }
        BodyType::Graphql => {
            // `{"query", "variables", "operationName"}`. The variables go in as typed once they
            // are known to be JSON, so numbers keep their precision.
            let g = &body.graphql;
            let string = |s: String| serde_json::Value::String(s).to_string();
            let mut op = format!("{{\"query\":{}", string(render(&g.query)));
            let variables = render(&g.variables);
            let variables = variables.trim();
            if !variables.is_empty() {
                let parse = |text: &str| serde_json::from_str::<serde_json::Value>(text);
                // Undefined `{{variables}}` are sent as they are (and reported), as in other bodies.
                if let Err(e) = parse(variables)
                    && parse(&undefined_as_null(variables)).is_err()
                {
                    return Err(Error::invalid(format!("GraphQL variables are not valid JSON: {e}")));
                }
                op.push_str(",\"variables\":");
                op.push_str(variables);
            }
            if let Some(name) = g.operation_name.as_deref().map(&mut *render).filter(|n| !n.trim().is_empty()) {
                op.push_str(",\"operationName\":");
                op.push_str(&string(name));
            }
            op.push('}');
            (Bytes::from(op), Some("application/json".into()))
        }
        BodyType::Binary => {
            let (data, name) = read_body_file(&render(&body.file), inherit)?;
            let mime = mime_guess::from_path(&name).first_or_octet_stream().to_string();
            (Bytes::from(data), Some(mime))
        }
        BodyType::Multipart => {
            let boundary = format!("----ZorvikBoundary{}", uuid::Uuid::new_v4().simple());
            let mut out = Vec::new();
            for field in body.multipart.iter().filter(|f| f.enabled && !f.key.trim().is_empty()) {
                let name = escape_quoted(&render(&field.key));
                out.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
                if field.file {
                    let (data, file_name) = read_body_file(&render(&field.value), inherit)?;
                    let mime = field
                        .content_type
                        .clone()
                        .filter(|c| !c.trim().is_empty())
                        .unwrap_or_else(|| mime_guess::from_path(&file_name).first_or_octet_stream().to_string());
                    out.extend_from_slice(
                        format!(
                            "Content-Disposition: form-data; name=\"{name}\"; filename=\"{}\"\r\nContent-Type: {mime}\r\n\r\n",
                            escape_quoted(&file_name)
                        )
                        .as_bytes(),
                    );
                    out.extend_from_slice(&data);
                } else {
                    out.extend_from_slice(format!("Content-Disposition: form-data; name=\"{name}\"\r\n").as_bytes());
                    if let Some(ct) = field.content_type.as_ref().filter(|c| !c.trim().is_empty()) {
                        out.extend_from_slice(format!("Content-Type: {ct}\r\n").as_bytes());
                    }
                    out.extend_from_slice(b"\r\n");
                    out.extend_from_slice(render(&field.value).as_bytes());
                }
                out.extend_from_slice(b"\r\n");
            }
            out.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
            (Bytes::from(out), Some(format!("multipart/form-data; boundary={boundary}")))
        }
    })
}

fn escape_quoted(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"").replace(['\r', '\n'], " ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use zorvik_formats::{Body, MultipartField, Variable};

    fn meta() -> WorkspaceMeta {
        WorkspaceMeta {
            version: 1,
            id: "id".into(),
            name: "ws".into(),
            variables: vec![],
            auth: Auth::None,
            headers: vec![KeyValue::new("X-Workspace", "1"), KeyValue::new("X-Override", "workspace")],
            scripts: Default::default(),
            docs: String::new(),
        }
    }

    fn ctx() -> VarContext {
        let mut c = VarContext::new();
        c.push_layer(&[
            Variable { key: "base".into(), value: "https://api.test".into(), enabled: true, secret: false },
            Variable { key: "token".into(), value: "t0k".into(), enabled: true, secret: true },
            Variable { key: "id".into(), value: "a b/c".into(), enabled: true, secret: false },
        ]);
        c
    }

    fn resolve_simple(req: &Request, folders: &[FolderMeta]) -> Resolved {
        let m = meta();
        let inherit = Inheritance { workspace: &m, folders, base_dir: Path::new("."), outside_files: false };
        resolve(req, &inherit, &ctx()).unwrap()
    }

    #[test]
    fn variables_path_params_and_headers() {
        let mut req = Request::new("r", RequestKind::Http);
        req.url = "{{base}}/users/:id/posts/:missing?x={{nope}}".into();
        req.path_params = vec![KeyValue::new("id", "{{id}}")];
        req.headers =
            vec![KeyValue::new("X-Override", "request"), KeyValue { enabled: false, ..KeyValue::new("X-Off", "1") }];
        let folders = [FolderMeta { headers: vec![KeyValue::new("X-Folder", "f")], ..Default::default() }];
        let r = resolve_simple(&req, &folders);
        assert_eq!(r.request.url, "https://api.test/users/a%20b%2Fc/posts/:missing?x={{nope}}");
        assert_eq!(r.unresolved, vec!["nope", ":missing"]);
        let names: Vec<_> = r.request.headers.iter().map(|h| format!("{}={}", h.name, h.value)).collect();
        assert_eq!(names, vec!["X-Workspace=1", "X-Folder=f", "X-Override=request"]);
    }

    #[test]
    fn auth_inheritance_and_explicit_header_wins() {
        let mut req = Request::new("r", RequestKind::Http);
        req.url = "http://h/".into();
        let folders = [
            FolderMeta { auth: Auth::Basic { username: "u".into(), password: "p".into() }, ..Default::default() },
            FolderMeta { auth: Auth::Inherit, ..Default::default() },
        ];
        let r = resolve_simple(&req, &folders);
        assert!(r.request.headers.iter().any(|h| h.name == "Authorization" && h.value == "Basic dTpw"));

        req.auth = Auth::Bearer { token: "{{token}}".into(), prefix: "Bearer".into() };
        let r = resolve_simple(&req, &folders);
        assert!(r.request.headers.iter().any(|h| h.value == "Bearer t0k"));

        req.headers = vec![KeyValue::new("authorization", "Custom x")];
        let r = resolve_simple(&req, &folders);
        assert_eq!(r.request.headers.iter().filter(|h| h.name.eq_ignore_ascii_case("authorization")).count(), 1);

        req.headers.clear();
        req.auth = Auth::ApiKey { key: "api_key".into(), value: "{{token}}".into(), location: ApiKeyLocation::Query };
        req.url = "http://h/x?a=1#frag".into();
        assert_eq!(resolve_simple(&req, &folders).request.url, "http://h/x?a=1&api_key=t0k#frag");

        req.auth = Auth::None;
        assert!(!resolve_simple(&req, &folders).request.headers.iter().any(|h| h.name == "Authorization"));
    }

    #[test]
    fn bodies() {
        let mut req = Request::new("r", RequestKind::Http);
        req.method = "POST".into();
        req.url = "http://h/".into();
        req.body = Body { body_type: BodyType::Json, text: "{\"t\":\"{{token}}\"}".into(), ..Default::default() };
        let r = resolve_simple(&req, &[]);
        assert_eq!(&r.request.body[..], b"{\"t\":\"t0k\"}");
        assert!(r.request.headers.iter().any(|h| h.name == "Content-Type" && h.value == "application/json"));

        req.body = Body {
            body_type: BodyType::FormUrlencoded,
            form: vec![KeyValue::new("a b", "c&d"), KeyValue { enabled: false, ..KeyValue::new("x", "y") }],
            ..Default::default()
        };
        assert_eq!(&resolve_simple(&req, &[]).request.body[..], b"a+b=c%26d");

        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("f.txt"), "file-data").unwrap();
        req.body = Body {
            body_type: BodyType::Multipart,
            multipart: vec![
                MultipartField {
                    key: "field".into(),
                    value: "v".into(),
                    file: false,
                    content_type: None,
                    enabled: true,
                },
                MultipartField {
                    key: "up".into(),
                    value: "f.txt".into(),
                    file: true,
                    content_type: None,
                    enabled: true,
                },
            ],
            ..Default::default()
        };
        let m = meta();
        let mut inherit = Inheritance { workspace: &m, folders: &[], base_dir: dir.path(), outside_files: false };
        let r = resolve(&req, &inherit, &ctx()).unwrap();
        let text = String::from_utf8_lossy(&r.request.body);
        assert!(text.contains("name=\"field\"\r\n\r\nv\r\n"));
        assert!(text.contains("filename=\"f.txt\"\r\nContent-Type: text/plain\r\n\r\nfile-data"));
        let ct = r.request.headers.iter().find(|h| h.name == "Content-Type").unwrap();
        assert!(ct.value.starts_with("multipart/form-data; boundary="));

        req.body = Body { body_type: BodyType::Binary, file: "missing.bin".into(), ..Default::default() };
        assert!(resolve(&req, &inherit, &ctx()).is_err());

        // A shared request must not upload files from outside the workspace unless allowed.
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret.txt"), "s3cret").unwrap();
        for path in [
            outside.path().join("secret.txt"),
            dir.path().join("..").join(outside.path().file_name().unwrap()).join("secret.txt"),
        ] {
            req.body =
                Body { body_type: BodyType::Binary, file: path.to_string_lossy().into_owned(), ..Default::default() };
            let err = resolve(&req, &inherit, &ctx()).unwrap_err();
            assert!(err.message.contains("outside the workspace"), "{}", err.message);
        }
        req.body.file = dir.path().join("f.txt").to_string_lossy().into_owned(); // absolute but inside
        assert!(resolve(&req, &inherit, &ctx()).is_ok());
        inherit.outside_files = true;
        req.body.file = outside.path().join("secret.txt").to_string_lossy().into_owned();
        assert_eq!(&resolve(&req, &inherit, &ctx()).unwrap().request.body[..], b"s3cret");

        // Nor point the body at a device (endless read).
        #[cfg(unix)]
        {
            req.body = Body { body_type: BodyType::Binary, file: "/dev/zero".into(), ..Default::default() };
            let err = resolve(&req, &inherit, &ctx()).unwrap_err();
            assert!(err.message.contains("not a regular file"), "{}", err.message);
        }
    }

    #[test]
    fn graphql_body() {
        let mut req = Request::new("r", RequestKind::Http);
        req.method = "POST".into();
        req.url = "http://h/graphql".into();
        req.body = Body {
            body_type: BodyType::Graphql,
            graphql: zorvik_formats::GraphqlBody {
                query: "query User($id: ID!) { user(id: $id) { name } } # {{token}}".into(),
                variables: "{\"id\": \"{{id}}\", \"n\": 2}".into(),
                operation_name: Some("User".into()),
            },
            ..Default::default()
        };
        let r = resolve_simple(&req, &[]);
        let sent: serde_json::Value = serde_json::from_slice(&r.request.body).unwrap();
        assert_eq!(
            sent,
            serde_json::json!({
                "query": "query User($id: ID!) { user(id: $id) { name } } # t0k",
                "variables": {"id": "a b/c", "n": 2},
                "operationName": "User",
            })
        );
        assert!(r.request.headers.iter().any(|h| h.name == "Content-Type" && h.value == "application/json"));
        // Keys keep the documented order.
        assert!(String::from_utf8_lossy(&r.request.body).starts_with("{\"query\":"));

        // Empty variables and operation name are left out; a user Content-Type wins.
        req.body.graphql = zorvik_formats::GraphqlBody { query: "{ ping }".into(), ..Default::default() };
        req.body.graphql.operation_name = Some("  ".into());
        req.headers = vec![KeyValue::new("Content-Type", "application/graphql+json")];
        let r = resolve_simple(&req, &[]);
        assert_eq!(&r.request.body[..], b"{\"query\":\"{ ping }\"}");
        let types: Vec<_> = r.request.headers.iter().filter(|h| h.name.eq_ignore_ascii_case("content-type")).collect();
        assert_eq!(types.len(), 1);
        assert_eq!(types[0].value, "application/graphql+json");

        // Variables must be JSON once rendered.
        req.body.graphql.variables = "{\"id\": {{id}}}".into();
        let m = meta();
        let inherit = Inheritance { workspace: &m, folders: &[], base_dir: Path::new("."), outside_files: false };
        let err = resolve(&req, &inherit, &ctx()).unwrap_err();
        assert!(err.message.contains("GraphQL variables are not valid JSON"), "{}", err.message);
        req.body.graphql.variables = "{\"n\": {{n}}}".into();
        let mut vars = ctx();
        vars.push_layer(&[Variable { key: "n".into(), value: "5".into(), enabled: true, secret: false }]);
        let r = resolve(&req, &inherit, &vars).unwrap();
        assert_eq!(&r.request.body[..], b"{\"query\":\"{ ping }\",\"variables\":{\"n\": 5}}");

        // Variables are sent as typed: numbers keep their precision.
        req.body.graphql.variables =
            " {\"big\": 123456789012345678901234567890, \"d\": 0.10000000000000000001}\n".into();
        let r = resolve(&req, &inherit, &vars).unwrap();
        assert_eq!(
            &r.request.body[..],
            b"{\"query\":\"{ ping }\",\"variables\":{\"big\": 123456789012345678901234567890, \"d\": 0.10000000000000000001}}"
        );

        // Undefined variables are sent as they are and reported (as in other bodies, and so
        // cURL export without resolving variables works); anything else must still be JSON.
        req.body.graphql.variables = "{\"n\": {{n}}, \"f\": {{filter}}, \"s\": \"{{nope}}\"}".into();
        let r = resolve(&req, &inherit, &vars).unwrap();
        assert_eq!(
            &r.request.body[..],
            b"{\"query\":\"{ ping }\",\"variables\":{\"n\": 5, \"f\": {{filter}}, \"s\": \"{{nope}}\"}}"
        );
        assert_eq!(r.unresolved, ["filter", "nope"]);
        req.body.graphql.variables = "{\"f\": {{filter}}".into();
        let err = resolve(&req, &inherit, &vars).unwrap_err();
        assert!(err.message.contains("GraphQL variables are not valid JSON"), "{}", err.message);
        assert_eq!(undefined_as_null("{{a}} {{b}}}} {{c"), "null null}} {{c");

        // Only HTTP requests carry a body.
        req.kind = RequestKind::Websocket;
        assert!(resolve(&req, &inherit, &vars).unwrap().request.body.is_empty());
    }

    #[test]
    fn undefined_host_variable_fails_early() {
        let mut req = Request::new("r", RequestKind::Http);
        req.url = "{{baseurl}}/users".into();
        let err = check_url_variables(&resolve_simple(&req, &[])).unwrap_err();
        assert_eq!(err.code, ErrorCode::UndefinedVariable);
        assert!(err.message.contains("{{baseurl}}"), "{}", err.message);
        req.url = "https://{{tenant}}.example.com/x".into();
        assert!(check_url_variables(&resolve_simple(&req, &[])).is_err());
        req.url = "{{base}}/users?q={{nope}}".into();
        assert!(check_url_variables(&resolve_simple(&req, &[])).is_ok());
    }

    #[test]
    fn oauth2_is_deferred() {
        let mut req = Request::new("r", RequestKind::Http);
        req.url = "http://h/".into();
        req.auth = Auth::OAuth2(OAuth2Config { client_id: "{{token}}".into(), ..Default::default() });
        let mut r = resolve_simple(&req, &[]);
        assert_eq!(r.oauth2.as_ref().unwrap().client_id, "t0k");
        apply_token(&mut r, "abc");
        assert!(r.request.headers.iter().any(|h| h.value == "Bearer abc"));
    }
}

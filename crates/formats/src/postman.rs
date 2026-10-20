//! Postman import: collections (schema v2.0 and v2.1, and v1 exports, converted
//! first: `v1.rs`), environment and globals exports.
//!
//! Postman files are loosely typed, so they are read as `serde_json::Value` and anything
//! odd becomes a warning instead of failing the whole import.

mod v1;

use std::collections::{HashMap, VecDeque};

use serde_json::{Map, Value};

use crate::import::{ImportError, ImportedCollection, ImportedItem};
use crate::model::{
    ApiKeyLocation, AsapConfig, Auth, AwsSigV4Config, Body, BodyType, ClientAuthMethod, DEFAULT_REDIRECT_URI,
    EdgeGridConfig, Environment, Example, FolderMeta, GrantType, GraphqlBody, HawkAlgorithm, HawkConfig, JwtAlgorithm,
    JwtConfig, KeyValue, MAX_EXAMPLE_BODY, MultipartField, OAuth1Config, OAuth1Method, OAuth2Config, Request,
    RequestKind, RequestSettings, Scripts, Variable,
};

/// Whether `json` is a Postman collection: v2.0, v2.1 or v1, or the Postman API's
/// `{"collection": …}` wrapper around one.
pub fn is_collection(json: &Value) -> bool {
    let Some(obj) = unwrap_api_export(json, "collection").as_object() else { return false };
    let schema = obj.get("info").and_then(|i| i.get("schema")).and_then(Value::as_str).unwrap_or_default();
    if schema.contains("getpostman.com") || schema.contains("postman.com/") {
        return true;
    }
    if obj.contains_key("openapi") || obj.contains_key("swagger") {
        return false;
    }
    if let Some(item) = obj.get("item") {
        // `info` is optional for Postman's own tools; without it the items must look like requests or folders.
        let item_like = |i: &Value| ["request", "item", "name"].iter().any(|k| i.get(*k).is_some());
        let items = item.as_array().map(Vec::as_slice).unwrap_or_default();
        return obj.contains_key("info") || (!items.is_empty() && items.iter().all(item_like));
    }
    v1::is_v1(obj) && ["name", "id", "order", "folders"].iter().any(|k| obj.contains_key(*k))
}

/// Whether `json` is a Postman environment or globals export (or the Postman API's
/// `{"environment": …}` wrapper around one).
pub fn is_environment(json: &Value) -> bool {
    let Some(obj) = unwrap_api_export(json, "environment").as_object() else { return false };
    let Some(values) = obj.get("values").and_then(Value::as_array) else { return false };
    // Without a name or scope, the values must look like variables.
    let variable_like = |v: &Value| v.get("key").is_some_and(Value::is_string);
    obj.contains_key("name")
        || obj.contains_key("_postman_variable_scope")
        || (!values.is_empty() && values.iter().all(variable_like))
}

/// Import a Postman collection (schema v2.0 or v2.1, or a v1 export).
pub fn import_postman_collection(json: &str) -> Result<ImportedCollection, ImportError> {
    let root = parse_json(json)?;
    let obj = unwrap_api_export(&root, "collection")
        .as_object()
        .ok_or_else(|| ImportError::new("Not a Postman collection: expected a JSON object."))?;
    if v1::is_v1(obj) {
        let converted = v1::to_v2(obj);
        return import_collection(converted.as_object().unwrap_or(&Map::new()));
    }
    check_collection_shape(obj)?;
    import_collection(obj)
}

fn import_collection(obj: &Map<String, Value>) -> Result<ImportedCollection, ImportError> {
    let mut imp = Importer::default();
    let info = obj.get("info");
    if let Some(schema) = info.and_then(|i| i.get("schema")).and_then(Value::as_str)
        && !schema.contains("v2.0.0")
        && !schema.contains("v2.1.0")
    {
        imp.warn(format!("Unrecognized collection schema '{schema}'; imported as v2.1."));
    }
    let name = trimmed(info.and_then(|i| i.get("name"))).unwrap_or_else(|| "Imported collection".into());
    let scripts = imp.scripts(obj, "Collection");
    let auth = match imp.auth(obj.get("auth"), "Collection") {
        Auth::Inherit => Auth::None,
        auth => auth,
    };
    let inherited = Inherited {
        auth_header: auth_header(&auth, None),
        ..imp.inherit(obj.get("protocolProfileBehavior"), &Inherited::default(), "Collection")
    };
    let items = imp.items(obj.get("item"), "", &inherited);

    let mut warnings = imp.warnings;
    for (note, places) in imp.notes {
        let n = places.len();
        let shown: Vec<&str> = places.iter().take(3).map(String::as_str).collect();
        let more = if n > 3 { ", …" } else { "" };
        warnings.push(format!("{}: {}{more}.", note.message(n), shown.join(", ")));
    }
    Ok(ImportedCollection {
        name,
        variables: Vec::new(),
        workspace_variables: variables(obj.get("variable")),
        auth,
        headers: Vec::new(),
        scripts,
        items,
        warnings,
        docs: description(info.and_then(|i| i.get("description"))),
    })
}

/// Import a Postman environment export.
pub fn import_postman_environment(json: &str) -> Result<Environment, ImportError> {
    let root = parse_json(json)?;
    let obj = unwrap_api_export(&root, "environment")
        .as_object()
        .ok_or_else(|| ImportError::new("Not a Postman environment: expected a JSON object."))?;
    let Some(values) = obj.get("values").and_then(Value::as_array) else {
        let looks_like_collection = ["info", "item", "collection"].iter().any(|k| obj.contains_key(*k));
        return Err(ImportError::new(if looks_like_collection {
            "This looks like a Postman collection, not an environment. Import it as a collection instead."
        } else {
            "Not a Postman environment export: the \"values\" list is missing."
        }));
    };
    let fallback = if obj.get("_postman_variable_scope").and_then(Value::as_str) == Some("globals") {
        "Globals"
    } else {
        "Imported environment"
    };
    let variables = values
        .iter()
        .filter_map(Value::as_object)
        .filter_map(|o| {
            Some(Variable {
                key: key_of(o)?,
                value: o.get("value").map(text).unwrap_or_default(),
                // Postman writes `enabled: false`; some tools write `disabled: true`.
                enabled: o.get("enabled").and_then(Value::as_bool).unwrap_or(true) && !flag(o.get("disabled")),
                secret: is_secret(o),
            })
        })
        .collect();
    Ok(Environment { name: trimmed(obj.get("name")).unwrap_or_else(|| fallback.into()), variables })
}

fn parse_json(json: &str) -> Result<Value, ImportError> {
    // Windows tools like to prepend a BOM, which serde_json rejects.
    serde_json::from_str(json.trim_start_matches('\u{feff}'))
        .map_err(|e| ImportError::new(format!("Invalid JSON: {e}")))
}

/// Postman API responses wrap the export: `{"collection": {...}}` / `{"environment": {...}}`.
fn unwrap_api_export<'a>(root: &'a Value, key: &str) -> &'a Value {
    match root.get(key) {
        Some(inner @ Value::Object(_)) if ["info", "item", "values"].iter().all(|k| root.get(*k).is_none()) => inner,
        _ => root,
    }
}

fn check_collection_shape(obj: &Map<String, Value>) -> Result<(), ImportError> {
    if obj.contains_key("item") {
        return Ok(());
    }
    if obj.get("values").is_some_and(Value::is_array) {
        return Err(ImportError::new(
            "This looks like a Postman environment, not a collection. Import it as an environment instead.",
        ));
    }
    if !obj.contains_key("info") {
        return Err(ImportError::new("Not a Postman collection: \"info\" and \"item\" are missing."));
    }
    Ok(())
}

/// Script APIs Zorvik doesn't have: text to look for, name for the warning.
const UNSUPPORTED_APIS: &[(&str, &str)] =
    &[("pm.vault", "pm.vault"), ("pm.execution.runRequest", "pm.execution.runRequest")];

/// Postman request settings (`protocolProfileBehavior`) Zorvik doesn't have, with the
/// value that is Postman's default (anything else gets a warning).
const UNSUPPORTED_BEHAVIORS: &[(&str, &str)] = &[
    ("followOriginalHttpMethod", "Zorvik follows redirects like browsers do (a 301, 302 or 303 changes POST to GET)"),
    ("followAuthorizationHeader", "Zorvik drops the Authorization header when a redirect goes to another site"),
    ("disableUrlEncoding", "Zorvik always encodes URLs"),
    ("disableCookies", "cookies follow the cookie jar setting (Settings → Requests)"),
    ("insecureHTTPParser", "Zorvik always parses responses strictly"),
    ("tlsPreferServerCiphers", "Zorvik uses its own TLS cipher order"),
    ("tlsDisabledProtocols", "Zorvik uses TLS 1.2 and 1.3"),
    ("tlsCipherSelection", "Zorvik uses its own TLS cipher order"),
    ("disabledSystemHeaders", "Zorvik always sends its default headers unless the request sets them"),
];

/// Things the import changed or can't do, reported as one warning for all the places.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Note {
    /// A script uses an API Zorvik doesn't have.
    UnsupportedApi(&'static str),
    /// A request had an explicit header that its auth replaces in Postman.
    ReplacedAuthHeader,
    /// A GET (…) request had a body, which Postman doesn't send.
    PrunedBody,
    /// A `protocolProfileBehavior` setting Zorvik doesn't have (index in UNSUPPORTED_BEHAVIORS).
    Behavior(usize),
}

impl Note {
    fn message(self, n: usize) -> String {
        let (s, verb) = if n == 1 { ("", "uses") } else { ("s", "use") };
        match self {
            Note::UnsupportedApi(api) => {
                format!("{n} script{s} {verb} {api}, which Zorvik does not support (it fails when run)")
            }
            Note::ReplacedAuthHeader => {
                let (whose, those) = if n == 1 { ("its", "header was") } else { ("their", "headers were") };
                format!(
                    "{n} request{s} had {whose} own auth header as well as an auth setting; Postman sends the \
                     auth setting's header, so the request's {those} turned off"
                )
            }
            Note::PrunedBody => format!(
                "{n} GET, HEAD, COPY, PURGE or UNLOCK request{s} had a body, which Postman doesn't send \
                 (\"disableBodyPruning\" was off); the body was kept but set to None"
            ),
            Note::Behavior(i) => {
                let (name, why) = UNSUPPORTED_BEHAVIORS[i];
                format!("{n} place{s} {verb} the Postman setting \"{name}\", which was not imported ({why})")
            }
        }
    }
}

/// What requests inherit from their folders and the collection.
#[derive(Debug, Clone, Default)]
struct Inherited {
    settings: RequestSettings,
    /// Postman's `disableBodyPruning`: GET (…) requests send their body.
    keep_body: bool,
    /// The header the nearest auth sets: Postman replaces a request's own header of that name.
    auth_header: Option<String>,
}

#[derive(Default)]
struct Importer {
    warnings: Vec<String>,
    /// Reported as one warning each, with the places.
    notes: Vec<(Note, Vec<String>)>,
}

impl Importer {
    fn warn(&mut self, message: String) {
        self.warnings.push(message);
    }

    fn note(&mut self, note: Note, label: &str) {
        match self.notes.iter_mut().find(|(n, _)| *n == note) {
            Some((_, places)) if places.last().map(String::as_str) != Some(label) => places.push(label.into()),
            Some(_) => {}
            None => self.notes.push((note, vec![label.into()])),
        }
    }

    /// Overlay Postman's `protocolProfileBehavior` on what the parents set.
    fn inherit(&mut self, v: Option<&Value>, parent: &Inherited, label: &str) -> Inherited {
        let mut out = parent.clone();
        let Some(p) = v.and_then(Value::as_object) else { return out };
        let s = &mut out.settings;
        if let Some(b) = p.get("followRedirects").and_then(Value::as_bool) {
            s.follow_redirects = Some(b);
        }
        if let Some(b) = p.get("strictSSL").and_then(Value::as_bool) {
            s.verify_tls = Some(b);
        }
        if let Some(n) = p.get("maxRedirects").and_then(Value::as_u64) {
            s.max_redirects = Some(u32::try_from(n).unwrap_or(u32::MAX));
        }
        if let Some(b) = p.get("disableBodyPruning").and_then(Value::as_bool) {
            out.keep_body = b;
        }
        for (i, (name, _)) in UNSUPPORTED_BEHAVIORS.iter().enumerate() {
            let set = match p.get(*name) {
                Some(Value::Bool(b)) => *b,
                Some(Value::Array(a)) => !a.is_empty(),
                Some(Value::Object(o)) => o.values().any(|v| v.as_bool() == Some(true)),
                Some(Value::String(s)) => !s.trim().is_empty(),
                _ => false,
            };
            if set {
                self.note(Note::Behavior(i), label);
            }
        }
        out
    }

    /// Convert an `item` list. `parent` is the " / "-joined folder path, used in warnings.
    fn items(&mut self, v: Option<&Value>, parent: &str, inherited: &Inherited) -> Vec<ImportedItem> {
        let entries = match v {
            Some(Value::Array(list)) => list.as_slice(),
            Some(single @ Value::Object(_)) => std::slice::from_ref(single),
            _ => &[],
        };
        let mut out = Vec::with_capacity(entries.len());
        for entry in entries {
            let seq = out.len() as u32 + 1;
            let Some(obj) = entry.as_object() else {
                self.warn(format!("{}: skipped an entry that is not a request or folder.", place(parent)));
                continue;
            };
            let item = if obj.get("item").is_some_and(Value::is_array) {
                let name = trimmed(obj.get("name")).unwrap_or_else(|| "Untitled folder".into());
                self.folder(obj, seq, name, parent, inherited)
            } else if obj.get("request").is_some_and(|r| !r.is_null()) {
                let name = trimmed(obj.get("name")).unwrap_or_else(|| "Untitled request".into());
                self.request(obj, seq, name, parent, inherited)
            } else {
                let name = trimmed(obj.get("name")).unwrap_or_else(|| "unnamed".into());
                self.warn(format!("{}: skipped '{name}', which is neither a request nor a folder.", place(parent)));
                continue;
            };
            out.push(item);
        }
        out
    }

    fn folder(
        &mut self,
        obj: &Map<String, Value>,
        seq: u32,
        name: String,
        parent: &str,
        inherited: &Inherited,
    ) -> ImportedItem {
        let path = join_path(parent, &name);
        let label = format!("Folder '{path}'");
        let scripts = self.scripts(obj, &label);
        let auth = self.auth(obj.get("auth"), &label);
        if obj.get("variable").and_then(Value::as_array).is_some_and(|v| !v.is_empty()) {
            self.warn(format!("{label}: folder variables are not supported and were skipped."));
        }
        let inherited = Inherited {
            auth_header: auth_header(&auth, inherited.auth_header.clone()),
            ..self.inherit(obj.get("protocolProfileBehavior"), inherited, &label)
        };
        let children = self.items(obj.get("item"), &path, &inherited);
        let docs = description(obj.get("description"));
        ImportedItem::Folder {
            meta: FolderMeta { name, seq, auth, headers: Vec::new(), scripts, docs, openapi: None },
            children,
        }
    }

    fn request(
        &mut self,
        obj: &Map<String, Value>,
        seq: u32,
        name: String,
        parent: &str,
        inherited: &Inherited,
    ) -> ImportedItem {
        let label = format!("Request '{}'", join_path(parent, &name));
        let scripts = self.scripts(obj, &label);
        let mut req = Request::new(name, RequestKind::Http);
        req.scripts = scripts;
        req.seq = seq;
        let inherited = self.inherit(obj.get("protocolProfileBehavior"), inherited, &label);
        req.settings = inherited.settings.clone();
        match obj.get("request") {
            Some(Value::String(url)) => req.url = url.trim().to_string(),
            Some(Value::Object(r)) => {
                if let Some(method) = trimmed(r.get("method")) {
                    req.method = method.to_uppercase();
                }
                apply_url(r.get("url"), &mut req);
                req.headers = headers(r.get("header"));
                req.body = self.body(r.get("body"), &req.headers, &label);
                req.auth = self.auth(r.get("auth"), &label);
                req.docs = description(r.get("description"));
            }
            _ => {}
        }
        // Postman's auth replaces a header of the same name the request sets itself.
        if let Some(name) = auth_header(&req.auth, inherited.auth_header.clone()) {
            let mut replaced = false;
            for h in req.headers.iter_mut().filter(|h| h.enabled && h.key.trim().eq_ignore_ascii_case(&name)) {
                h.enabled = false;
                replaced = true;
            }
            if replaced {
                self.note(Note::ReplacedAuthHeader, &label);
            }
        }
        // Postman doesn't send a body with these methods (unless `disableBodyPruning`).
        let prunes = ["GET", "HEAD", "COPY", "PURGE", "UNLOCK"].contains(&req.method.as_str());
        if prunes && !inherited.keep_body && req.body.body_type != BodyType::None {
            req.body.body_type = BodyType::None;
            self.note(Note::PrunedBody, &label);
        }
        if req.docs.is_empty() {
            req.docs = description(obj.get("description"));
        }
        req.examples = self.examples(obj.get("response"), &req.url, &label);
        ImportedItem::Request(req)
    }

    /// Saved responses (`response`): Postman's examples.
    fn examples(&mut self, v: Option<&Value>, url: &str, label: &str) -> Vec<Example> {
        let mut examples = Vec::new();
        for (i, r) in list(v).filter_map(Value::as_object).enumerate() {
            let mut body = r.get("body").map(text).unwrap_or_default();
            if body.len() > MAX_EXAMPLE_BODY {
                self.warn(format!("{label}: example {} is larger than 1 MB; its body was left out.", i + 1));
                body.clear();
            }
            let mut sent = Request::new("", RequestKind::Http);
            if let Some(Value::Object(original)) = r.get("originalRequest") {
                apply_url(original.get("url"), &mut sent);
            }
            examples.push(Example {
                name: trimmed(r.get("name")).unwrap_or_else(|| format!("Example {}", i + 1)),
                status: r.get("code").and_then(Value::as_u64).and_then(|c| u16::try_from(c).ok()).unwrap_or(200),
                headers: headers(r.get("header")),
                body,
                url: if sent.url == url { String::new() } else { sent.url },
            });
        }
        examples
    }

    /// `event` scripts: `prerequest` → pre-request, `test` → post-response.
    fn scripts(&mut self, obj: &Map<String, Value>, label: &str) -> Scripts {
        let mut scripts = Scripts::default();
        for event in list(obj.get("event")).filter(|e| !flag(e.get("disabled"))) {
            let code = script_text(event);
            if code.trim().is_empty() {
                continue;
            }
            let target = match event.get("listen").and_then(Value::as_str) {
                Some("prerequest") => &mut scripts.pre_request,
                Some("test") => &mut scripts.post_response,
                other => {
                    let what = other.unwrap_or("unnamed");
                    self.warn(format!("{label}: skipped a '{what}' script (only pre-request and test scripts run)."));
                    continue;
                }
            };
            if !target.is_empty() {
                target.push('\n');
            }
            target.push_str(&code);
            for (needle, api) in UNSUPPORTED_APIS {
                if code.contains(needle) {
                    self.note(Note::UnsupportedApi(api), label);
                }
            }
        }
        scripts
    }

    fn body(&mut self, v: Option<&Value>, headers: &[KeyValue], label: &str) -> Body {
        let Some(b) = v.and_then(Value::as_object) else { return Body::default() };
        let mut body = Body::default();
        match b.get("mode").and_then(Value::as_str).unwrap_or("") {
            "raw" => {
                body.text = match b.get("raw") {
                    Some(Value::String(s)) => s.clone(),
                    Some(v) if !v.is_null() => serde_json::to_string_pretty(v).unwrap_or_default(),
                    _ => String::new(),
                };
                if body.text.is_empty() {
                    return Body::default();
                }
                let language = b.get("options").and_then(|o| o.pointer("/raw/language")).and_then(Value::as_str);
                let (body_type, content_type) = match language.map(str::to_ascii_lowercase).as_deref() {
                    Some("json") => (BodyType::Json, None),
                    Some("xml") => (BodyType::Xml, None),
                    Some("html") => (BodyType::Text, Some("text/html")),
                    Some("javascript") => (BodyType::Text, Some("application/javascript")),
                    Some("text") => (BodyType::Text, Some("text/plain")),
                    _ => (type_from_headers(headers), None),
                };
                body.body_type = body_type;
                body.content_type = content_type.map(String::from);
            }
            "urlencoded" => {
                body.form = list(b.get("urlencoded")).filter_map(Value::as_object).filter_map(key_value).collect();
                if !body.form.is_empty() {
                    body.body_type = BodyType::FormUrlencoded;
                }
            }
            "formdata" => {
                body.multipart = self.multipart(b.get("formdata"), label);
                if !body.multipart.is_empty() {
                    body.body_type = BodyType::Multipart;
                }
            }
            "file" => {
                body.body_type = BodyType::Binary;
                body.file = trimmed(b.get("file").and_then(|f| f.get("src"))).unwrap_or_default();
                if body.file.is_empty() {
                    self.warn(format!("{label}: no file was selected for the binary body; choose it after import."));
                }
            }
            "graphql" => {
                let g = b.get("graphql");
                let field = |key: &str| g.and_then(|g| g.get(key));
                // Postman keeps variables as JSON text; `{{variables}}` in it may only become JSON once rendered.
                let variables = match field("variables") {
                    Some(Value::String(s)) if !s.trim().is_empty() => s.clone(),
                    Some(v @ Value::Object(o)) if !o.is_empty() => serde_json::to_string_pretty(v).unwrap_or_default(),
                    _ => String::new(),
                };
                if !variables.is_empty()
                    && !variables.contains("{{")
                    && serde_json::from_str::<Value>(&variables).is_err()
                {
                    self.warn(format!("{label}: GraphQL variables are not valid JSON; fix them before sending."));
                }
                body.body_type = BodyType::Graphql;
                body.graphql = GraphqlBody {
                    query: field("query").map(text).unwrap_or_default(),
                    variables,
                    operation_name: trimmed(field("operationName")),
                    ..Default::default()
                };
            }
            "" | "none" => {}
            other => self.warn(format!("{label}: body mode '{other}' is not supported; imported without a body.")),
        }
        // A body Postman doesn't send: kept, but not sent either.
        if flag(b.get("disabled")) {
            body.body_type = BodyType::None;
        }
        body
    }

    fn multipart(&mut self, v: Option<&Value>, label: &str) -> Vec<MultipartField> {
        let mut fields = Vec::new();
        for o in list(v).filter_map(Value::as_object) {
            let key = o.get("key").map(text).unwrap_or_default();
            let enabled = !flag(o.get("disabled"));
            let file = o.get("type").and_then(Value::as_str) == Some("file");
            let value = if file {
                let (src, extra) = match o.get("src") {
                    Some(Value::Array(srcs)) => {
                        (srcs.first().map(text).unwrap_or_default(), srcs.len().saturating_sub(1))
                    }
                    Some(src) => (text(src), 0),
                    None => (String::new(), 0),
                };
                if src.is_empty() && enabled {
                    self.warn(format!("{label}: no file was selected for form field '{key}'; choose it after import."));
                }
                if extra > 0 {
                    self.warn(format!("{label}: form field '{key}' had several files; only the first was imported."));
                }
                src
            } else {
                o.get("value").map(text).unwrap_or_default()
            };
            if key.is_empty() && value.is_empty() && !file {
                continue;
            }
            let content_type = trimmed(o.get("contentType"));
            fields.push(MultipartField { key, value, file, content_type, enabled });
        }
        fields
    }

    /// Absent or null auth means "inherit from the parent".
    fn auth(&mut self, v: Option<&Value>, label: &str) -> Auth {
        let Some(a) = v.and_then(Value::as_object) else { return Auth::Inherit };
        let kind = a.get("type").and_then(Value::as_str).unwrap_or("").trim();
        let p = auth_params(a.get(kind));
        let get = |k: &str| p.get(k).map(text).unwrap_or_default();
        match kind {
            "" | "inherit" => Auth::Inherit,
            "noauth" => Auth::None,
            "basic" => Auth::Basic { username: get("username"), password: get("password") },
            "bearer" => Auth::Bearer { token: get("token"), prefix: "Bearer".into() },
            "apikey" => Auth::ApiKey {
                key: get("key"),
                value: get("value"),
                location: if get("in") == "query" { ApiKeyLocation::Query } else { ApiKeyLocation::Header },
            },
            "oauth2" => self.oauth2(&p, label),
            "digest" => Auth::Digest { username: get("username"), password: get("password") },
            "ntlm" => Auth::Ntlm {
                username: get("username"),
                password: get("password"),
                domain: get("domain"),
                workstation: get("workstation"),
            },
            "awsv4" => Auth::AwsSigV4(AwsSigV4Config {
                access_key: get("accessKey"),
                secret_key: get("secretKey"),
                session_token: get("sessionToken"),
                region: get("region"),
                service: get("service"),
                location: if flag(p.get("addAuthDataToQuery")) {
                    ApiKeyLocation::Query
                } else {
                    ApiKeyLocation::Header
                },
            }),
            "oauth1" => self.oauth1(&p, label),
            "jwt" => {
                let algorithm = jwt_algorithm(&get("algorithm")).unwrap_or_else(|| {
                    self.warn(format!("{label}: JWT algorithm '{}' is not supported; using HS256.", get("algorithm")));
                    JwtAlgorithm::HS256
                });
                let secret = if algorithm.uses_secret() { get("secret") } else { get("privateKey") };
                Auth::Jwt(JwtConfig {
                    algorithm,
                    secret,
                    secret_base64: flag(p.get("isSecretBase64Encoded")),
                    payload: match get("payload") {
                        s if s.trim().is_empty() => "{}".into(),
                        s => s,
                    },
                    header: get("header"),
                    prefix: p.get("headerPrefix").map(text).unwrap_or_else(|| "Bearer".into()),
                    location: if get("addTokenTo") == "queryParam" {
                        ApiKeyLocation::Query
                    } else {
                        ApiKeyLocation::Header
                    },
                    query_param: trimmed(p.get("queryParamKey")).unwrap_or_else(|| "token".into()),
                })
            }
            "hawk" => Auth::Hawk(HawkConfig {
                id: get("authId"),
                key: get("authKey"),
                algorithm: if get("algorithm").eq_ignore_ascii_case("sha1") {
                    HawkAlgorithm::Sha1
                } else {
                    HawkAlgorithm::Sha256
                },
                ext: get("extraData"),
                app: get("app"),
                dlg: get("delegation"),
                include_payload_hash: flag(p.get("includePayloadHash")),
            }),
            "edgegrid" => Auth::EdgeGrid(EdgeGridConfig {
                client_token: get("clientToken"),
                client_secret: get("clientSecret"),
                access_token: get("accessToken"),
                headers_to_sign: get("headersToSign"),
                ..EdgeGridConfig::default()
            }),
            "asap" => Auth::Asap(AsapConfig {
                issuer: get("iss"),
                subject: get("sub"),
                audience: get("aud"),
                key_id: get("kid"),
                private_key: get("privateKey"),
                algorithm: jwt_algorithm(&get("alg")).unwrap_or(JwtAlgorithm::RS256),
                expires_in: expiry_seconds(&get("exp")).unwrap_or(3600),
                claims: get("claims"),
            }),
            other => {
                self.warn(format!("{label}: '{other}' auth is not supported; imported with no auth."));
                Auth::None
            }
        }
    }

    fn oauth1(&mut self, p: &Map<String, Value>, label: &str) -> Auth {
        let get = |k: &str| p.get(k).map(text).unwrap_or_default();
        let signature_method = match get("signatureMethod").trim().to_ascii_uppercase().as_str() {
            "" | "HMAC-SHA1" => OAuth1Method::HmacSha1,
            "HMAC-SHA256" => OAuth1Method::HmacSha256,
            "HMAC-SHA512" => OAuth1Method::HmacSha512,
            "RSA-SHA1" => OAuth1Method::RsaSha1,
            "RSA-SHA256" => OAuth1Method::RsaSha256,
            "RSA-SHA512" => OAuth1Method::RsaSha512,
            "PLAINTEXT" => OAuth1Method::Plaintext,
            other => {
                self.warn(format!("{label}: OAuth 1.0 signature method '{other}' is not supported; using HMAC-SHA1."));
                OAuth1Method::HmacSha1
            }
        };
        // Postman's default is the header; `addParamsToHeader: false` means the query or body.
        let in_header = p.get("addParamsToHeader").is_none_or(|v| flag(Some(v)));
        Auth::OAuth1(OAuth1Config {
            consumer_key: get("consumerKey"),
            consumer_secret: get("consumerSecret"),
            token: get("token"),
            token_secret: get("tokenSecret"),
            signature_method,
            private_key: get("privateKey"),
            callback: get("callback"),
            verifier: get("verifier"),
            realm: get("realm"),
            include_version: p.get("version").is_none_or(|v| !text(v).trim().is_empty()),
            include_body_hash: flag(p.get("includeBodyHash")),
            location: if in_header { ApiKeyLocation::Header } else { ApiKeyLocation::Query },
        })
    }

    fn oauth2(&mut self, p: &Map<String, Value>, label: &str) -> Auth {
        let get = |k: &str| p.get(k).map(text).unwrap_or_default();
        let get_trimmed = |k: &str| get(k).trim().to_string();
        // Postman sends an empty-string prefix as a bare token; only a missing one means "Bearer".
        let header_prefix = p.get("headerPrefix").and_then(Value::as_str).unwrap_or("Bearer").to_string();
        let (token_url, auth_url) = (get_trimmed("accessTokenUrl"), get_trimmed("authUrl"));

        // Old exports carry just a saved token and no flow configuration.
        let saved_token = get_trimmed("accessToken");
        if token_url.is_empty() && auth_url.is_empty() && !saved_token.is_empty() {
            self.warn(format!(
                "{label}: OAuth 2.0 settings have no token URL; imported the saved access token as a Bearer token."
            ));
            return Auth::Bearer { token: saved_token, prefix: header_prefix };
        }
        // A token from a variable (that a script sets): Postman sends just that, so it stays a token.
        if saved_token.contains("{{") {
            self.warn(format!(
                "{label}: OAuth 2.0 sends the access token {saved_token}, as in Postman; it was imported as a Bearer \
                 token, without the other OAuth 2.0 settings."
            ));
            return Auth::Bearer { token: saved_token, prefix: header_prefix };
        }

        let (grant_type, pkce) = match get("grant_type").trim() {
            "client_credentials" => (GrantType::ClientCredentials, true),
            "password_credentials" | "password" => (GrantType::Password, true),
            "authorization_code_with_pkce" => (GrantType::AuthorizationCode, true),
            "authorization_code" | "" => (GrantType::AuthorizationCode, false),
            "implicit" => (GrantType::Implicit, false),
            other => {
                self.warn(format!(
                    "{label}: OAuth 2.0 grant type '{other}' is not supported; imported as authorization code."
                ));
                (GrantType::AuthorizationCode, false)
            }
        };

        let mut redirect_uri = get_trimmed("redirect_uri");
        if redirect_uri.is_empty() || redirect_uri.starts_with("https://oauth.pstmn.io/") {
            redirect_uri = DEFAULT_REDIRECT_URI.to_string();
            if matches!(grant_type, GrantType::AuthorizationCode | GrantType::Implicit) {
                self.warn(format!(
                    "{label}: OAuth 2.0 redirect URI set to {DEFAULT_REDIRECT_URI}; register this URI with the provider."
                ));
            }
        }
        if get("addTokenTo") == "queryParams" {
            self.warn(format!(
                "{label}: Postman added the OAuth 2.0 token to the query string; it will be sent in the Authorization header."
            ));
        }

        let extra = |list: &str| -> Vec<(String, String)> {
            let Some(entries) = p.get(list).and_then(Value::as_array) else { return Vec::new() };
            entries
                .iter()
                .filter_map(Value::as_object)
                .filter(|o| o.get("enabled").and_then(Value::as_bool) != Some(false))
                .filter_map(|o| Some((trimmed(o.get("key"))?, o.get("value").map(text).unwrap_or_default())))
                .collect()
        };
        let mut params = extra("tokenRequestParams");
        params.extend(extra("authRequestParams"));
        let audience = params.iter().find(|(k, _)| k == "audience").map(|(_, v)| v.clone()).unwrap_or_default();
        let mut dropped: Vec<&str> = params.iter().map(|(k, _)| k.as_str()).filter(|k| *k != "audience").collect();
        dropped.sort_unstable();
        dropped.dedup();
        if !dropped.is_empty() {
            self.warn(format!("{label}: extra OAuth 2.0 parameters were not imported: {}.", dropped.join(", ")));
        }

        Auth::OAuth2(OAuth2Config {
            grant_type,
            token_url,
            auth_url,
            redirect_uri,
            client_id: get("clientId"),
            client_secret: get("clientSecret"),
            scope: get("scope"),
            audience,
            username: get("username"),
            password: get("password"),
            client_auth: if get("client_authentication") == "body" {
                ClientAuthMethod::Body
            } else {
                ClientAuthMethod::BasicHeader
            },
            pkce,
            header_prefix,
        })
    }
}

fn jwt_algorithm(name: &str) -> Option<JwtAlgorithm> {
    serde_json::from_value(Value::String(name.trim().to_ascii_uppercase())).ok()
}

/// `3600`, `60s`, `15m` or `1h` (Postman's ASAP expiry) in seconds.
fn expiry_seconds(text: &str) -> Option<u32> {
    let t = text.trim();
    let (number, unit) = t.find(|c: char| !c.is_ascii_digit()).map_or((t, ""), |i| t.split_at(i));
    let n: u32 = number.parse().ok()?;
    let factor = match unit.trim() {
        "" | "s" => 1,
        "m" => 60,
        "h" => 3600,
        _ => return None,
    };
    n.checked_mul(factor)
}

/// Where a warning about a child of `parent` points.
fn place(parent: &str) -> String {
    if parent.is_empty() { "Collection".to_string() } else { format!("Folder '{parent}'") }
}

fn join_path(parent: &str, name: &str) -> String {
    if parent.is_empty() { name.to_string() } else { format!("{parent} / {name}") }
}

/// The header `auth` sets (`inherited` for `Inherit`); Postman's auth replaces a request
/// header of that name.
fn auth_header(auth: &Auth, inherited: Option<String>) -> Option<String> {
    match auth {
        Auth::Inherit => inherited,
        Auth::None => None,
        // Without a name, Postman adds no header.
        Auth::ApiKey { key, location: ApiKeyLocation::Header, .. } => {
            Some(key.trim().to_string()).filter(|k| !k.is_empty())
        }
        Auth::ApiKey { .. } => None,
        Auth::OAuth1(c) if c.location == ApiKeyLocation::Query => None,
        Auth::AwsSigV4(c) if c.location == ApiKeyLocation::Query => None,
        Auth::Jwt(c) if c.location == ApiKeyLocation::Query => None,
        _ => Some("Authorization".into()),
    }
}

/// A `url.query` entry as written in the file (not decoded).
struct QueryParam {
    key: String,
    /// `None` for a bare `?flag` without `=`.
    value: Option<String>,
    disabled: bool,
    description: String,
}

impl QueryParam {
    fn parse(v: &Value) -> Option<Self> {
        let o = v.as_object()?;
        let key = o.get("key").map(text).unwrap_or_default();
        let value = o.get("value").filter(|v| !v.is_null()).map(text);
        if key.is_empty() && value.is_none() {
            return None;
        }
        Some(Self { key, value, disabled: flag(o.get("disabled")), description: description(o.get("description")) })
    }

    /// Key and value as they appear in a raw `key=value` query string segment.
    fn pair(&self) -> (&str, &str) {
        (&self.key, self.value.as_deref().unwrap_or(""))
    }
}

fn apply_url(v: Option<&Value>, req: &mut Request) {
    let url = match v {
        Some(Value::String(s)) => {
            req.url = s.trim().to_string();
            return;
        }
        Some(Value::Object(url)) => url,
        _ => return,
    };
    let query: Vec<QueryParam> = list(url.get("query")).filter_map(QueryParam::parse).collect();
    req.url = match trimmed(url.get("raw")) {
        Some(raw) => strip_disabled(&raw, &query),
        None => build_url(url, &query),
    };
    req.disabled_params = query
        .into_iter()
        .filter(|q| q.disabled)
        .map(|q| KeyValue {
            key: q.key,
            value: q.value.unwrap_or_default(),
            enabled: false,
            description: q.description,
        })
        .collect();
    req.path_params = list(url.get("variable"))
        .filter_map(Value::as_object)
        .filter_map(|o| {
            Some(KeyValue {
                key: key_of(o)?,
                value: o.get("value").map(text).unwrap_or_default(),
                enabled: true,
                description: description(o.get("description")),
            })
        })
        .collect();
}

/// Remove disabled params from the query string of `raw`, leaving everything else byte-for-byte.
/// Postman usually leaves disabled params out of `raw` already; a disabled param is only removed
/// when `raw` holds more copies of it than there are enabled entries with the same key and value.
fn strip_disabled(raw: &str, query: &[QueryParam]) -> String {
    if !query.iter().any(|q| q.disabled) {
        return raw.to_string();
    }
    let (before_hash, hash) = raw.split_at(raw.find('#').unwrap_or(raw.len()));
    let Some((base, qs)) = before_hash.split_once('?') else { return raw.to_string() };
    let mut segments: Vec<Option<&str>> = qs.split('&').map(Some).collect();
    // Maps, not scans: a collection can list thousands of params for one URL.
    let mut hits: HashMap<(&str, &str), VecDeque<usize>> = HashMap::new();
    for (i, segment) in qs.split('&').enumerate() {
        hits.entry(segment.split_once('=').unwrap_or((segment, ""))).or_default().push_back(i);
    }
    let mut enabled_twins: HashMap<(&str, &str), usize> = HashMap::new();
    for q in query.iter().filter(|q| !q.disabled) {
        *enabled_twins.entry(q.pair()).or_default() += 1;
    }
    let mut removed = false;
    for d in query.iter().filter(|q| q.disabled) {
        let twins = enabled_twins.get(&d.pair()).copied().unwrap_or(0);
        if let Some(hits) = hits.get_mut(&d.pair())
            && hits.len() > twins
            && let Some(i) = hits.pop_front()
        {
            segments[i] = None;
            removed = true;
        }
    }
    if !removed {
        return raw.to_string();
    }
    let kept: Vec<&str> = segments.into_iter().flatten().collect();
    let mut out = base.to_string();
    if kept.iter().any(|s| !s.is_empty()) {
        out.push('?');
        out.push_str(&kept.join("&"));
    }
    out.push_str(hash);
    out
}

/// Rebuild a URL from its parts when `raw` is missing.
fn build_url(url: &Map<String, Value>, query: &[QueryParam]) -> String {
    let mut out = String::new();
    if let Some(protocol) = trimmed(url.get("protocol")) {
        out.push_str(protocol.trim_end_matches("://"));
        out.push_str("://");
    }
    match url.get("host") {
        Some(Value::Array(parts)) => out.push_str(&parts.iter().map(segment).collect::<Vec<_>>().join(".")),
        Some(host) => out.push_str(&text(host)),
        None => {}
    }
    if let Some(port) = trimmed(url.get("port")) {
        out.push(':');
        out.push_str(&port);
    }
    match url.get("path") {
        Some(Value::Array(parts)) => {
            for part in parts {
                out.push('/');
                out.push_str(&segment(part));
            }
        }
        Some(path) => {
            let path = text(path);
            if !path.is_empty() && !path.starts_with('/') {
                out.push('/');
            }
            out.push_str(&path);
        }
        None => {}
    }
    let enabled: Vec<String> = query
        .iter()
        .filter(|q| !q.disabled)
        .map(|q| match &q.value {
            Some(v) => format!("{}={v}", q.key),
            None => q.key.clone(),
        })
        .collect();
    if !enabled.is_empty() {
        out.push('?');
        out.push_str(&enabled.join("&"));
    }
    if let Some(hash) = trimmed(url.get("hash")) {
        out.push('#');
        out.push_str(&hash);
    }
    out
}

/// A host or path segment: a string, or `{"type": "string", "value": ...}` in some v2.0 exports.
fn segment(v: &Value) -> String {
    match v {
        Value::Object(o) => o.get("value").map(text).unwrap_or_default(),
        v => text(v),
    }
}

/// Headers as a list of objects or `Name: value` strings, or one multi-line string (old exports).
fn headers(v: Option<&Value>) -> Vec<KeyValue> {
    match v {
        Some(Value::Array(entries)) => entries
            .iter()
            .filter_map(|h| match h {
                Value::Object(o) => key_value(o),
                Value::String(line) => header_line(line),
                _ => None,
            })
            .collect(),
        Some(Value::String(block)) => block.lines().filter_map(header_line).collect(),
        _ => Vec::new(),
    }
}

/// Parse `Name: value`; a leading `//` marks a disabled header (Postman's bulk-edit syntax).
fn header_line(line: &str) -> Option<KeyValue> {
    let line = line.trim();
    let (line, enabled) = match line.strip_prefix("//") {
        Some(rest) => (rest.trim_start(), false),
        None => (line, true),
    };
    let (key, value) = line.split_once(':').unwrap_or((line, ""));
    let key = key.trim();
    if key.is_empty() {
        return None;
    }
    Some(KeyValue { key: key.to_string(), value: value.trim().to_string(), enabled, description: String::new() })
}

/// A `{key, value, disabled, description}` entry; `None` when both key and value are empty.
fn key_value(o: &Map<String, Value>) -> Option<KeyValue> {
    let key = o.get("key").map(text).unwrap_or_default();
    let value = o.get("value").map(text).unwrap_or_default();
    if key.is_empty() && value.is_empty() {
        return None;
    }
    Some(KeyValue { key, value, enabled: !flag(o.get("disabled")), description: description(o.get("description")) })
}

/// Body type for a raw body without a language, guessed from the Content-Type header.
fn type_from_headers(headers: &[KeyValue]) -> BodyType {
    let content_type = headers
        .iter()
        .find(|h| h.enabled && h.key.eq_ignore_ascii_case("content-type"))
        .map(|h| h.value.to_ascii_lowercase())
        .unwrap_or_default();
    if content_type.contains("json") {
        BodyType::Json
    } else if content_type.contains("xml") {
        BodyType::Xml
    } else {
        BodyType::Text
    }
}

/// Auth parameters: v2.1 stores `[{key, value, type}]`, v2.0 a plain object.
fn auth_params(v: Option<&Value>) -> Map<String, Value> {
    match v {
        Some(Value::Array(entries)) => entries
            .iter()
            .filter_map(|e| {
                let key = e.get("key")?.as_str()?;
                Some((key.to_string(), e.get("value").cloned().unwrap_or(Value::Null)))
            })
            .collect(),
        Some(Value::Object(o)) => o.clone(),
        _ => Map::new(),
    }
}

fn variables(v: Option<&Value>) -> Vec<Variable> {
    list(v)
        .filter_map(Value::as_object)
        .filter_map(|o| {
            Some(Variable {
                key: key_of(o)?,
                value: o.get("value").map(text).unwrap_or_default(),
                enabled: !flag(o.get("disabled")),
                secret: is_secret(o),
            })
        })
        .collect()
}

/// A script's source: `script.exec` is a list of lines or one string.
fn script_text(event: &Value) -> String {
    match event.pointer("/script/exec") {
        Some(Value::Array(lines)) => {
            lines.iter().map(|l| l.as_str().unwrap_or_default()).collect::<Vec<_>>().join("\n")
        }
        Some(Value::String(s)) => s.clone(),
        _ => String::new(),
    }
}

fn list(v: Option<&Value>) -> impl Iterator<Item = &Value> {
    v.and_then(Value::as_array).into_iter().flatten()
}

/// Variable name: `key`, or `id` in some v2.0 exports.
fn key_of(o: &Map<String, Value>) -> Option<String> {
    ["key", "id"].iter().find_map(|k| trimmed(o.get(*k)))
}

fn is_secret(o: &Map<String, Value>) -> bool {
    o.get("type").and_then(Value::as_str) == Some("secret")
}

fn flag(v: Option<&Value>) -> bool {
    match v {
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => s.eq_ignore_ascii_case("true"),
        _ => false,
    }
}

/// A description: a string, or `{content, type}`.
fn description(v: Option<&Value>) -> String {
    match v {
        Some(Value::Object(o)) => o.get("content").and_then(Value::as_str).unwrap_or("").trim().to_string(),
        Some(Value::String(s)) => s.trim().to_string(),
        _ => String::new(),
    }
}

/// Non-empty trimmed text of a scalar.
fn trimmed(v: Option<&Value>) -> Option<String> {
    let s = text(v?);
    let t = s.trim();
    (!t.is_empty()).then(|| t.to_string())
}

/// Scalars as text (numbers and booleans stringified, null empty); arrays and objects as JSON.
fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const V21: &str = "https://schema.getpostman.com/json/collection/v2.1.0/collection.json";

    /// Wrap `items` (a JSON array body) in a v2.1 collection.
    fn collection(items: &str) -> String {
        format!(r#"{{"info": {{"name": "Test", "schema": "{V21}"}}, "item": [{items}]}}"#)
    }

    fn import(json: &str) -> ImportedCollection {
        import_postman_collection(json).expect("import")
    }

    fn all_requests(items: &[ImportedItem]) -> Vec<&Request> {
        let mut out = Vec::new();
        for item in items {
            match item {
                ImportedItem::Folder { children, .. } => out.extend(all_requests(children)),
                ImportedItem::Request(r) => out.push(r),
            }
        }
        out
    }

    fn request<'a>(c: &'a ImportedCollection, name: &str) -> &'a Request {
        all_requests(&c.items).into_iter().find(|r| r.name == name).unwrap_or_else(|| panic!("no {name}"))
    }

    fn folder<'a>(items: &'a [ImportedItem], name: &str) -> (&'a FolderMeta, &'a [ImportedItem]) {
        items
            .iter()
            .find_map(|i| match i {
                ImportedItem::Folder { meta, children } if meta.name == name => Some((meta, children.as_slice())),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no folder {name}"))
    }

    /// `"<seq> <name>"`, with a trailing `/` for folders.
    fn names(items: &[ImportedItem]) -> Vec<String> {
        items
            .iter()
            .map(|i| match i {
                ImportedItem::Folder { meta, .. } => format!("{} {}/", meta.seq, meta.name),
                ImportedItem::Request(r) => format!("{} {}", r.seq, r.name),
            })
            .collect()
    }

    fn one(json_item: &str) -> Request {
        let c = import(&collection(json_item));
        all_requests(&c.items).into_iter().next().expect("one request").clone()
    }

    #[test]
    fn nested_folders_keep_file_order_and_seq() {
        let c = import(&collection(
            r#"
            {"name": "Users", "description": "User endpoints", "item": [
                {"name": "List users", "request": {"method": "GET", "url": "https://api.example.com/users"}},
                {"name": "  Admin  ", "item": [
                    {"name": "Ban user", "request": {"method": "post", "url": "https://api.example.com/users/1/ban"}},
                    {"name": "", "request": {"method": "DELETE", "url": "https://api.example.com/users/1"}}
                ]},
                {"name": "Create user", "request": {"method": "POST", "url": "https://api.example.com/users",
                    "description": {"content": "Creates a user.", "type": "text/markdown"}},
                 "response": [{"name": "201", "code": 201, "body": "{}"}]}
            ]},
            {"name": "Health", "request": {"method": "GET", "url": "https://api.example.com/health"}},
            {"name": "", "item": []}
            "#,
        ));
        assert_eq!(c.name, "Test");
        assert_eq!(c.request_count(), 5);
        assert!(c.warnings.is_empty(), "{:?}", c.warnings);
        assert_eq!(names(&c.items), ["1 Users/", "2 Health", "3 Untitled folder/"]);
        let (users, children) = folder(&c.items, "Users");
        assert_eq!(users.docs, "User endpoints");
        assert_eq!(users.auth, Auth::Inherit);
        assert_eq!(names(children), ["1 List users", "2 Admin/", "3 Create user"]);
        let (_, admin) = folder(children, "Admin");
        assert_eq!(names(admin), ["1 Ban user", "2 Untitled request"]);
        assert_eq!(request(&c, "Ban user").method, "POST");
        assert_eq!(request(&c, "Create user").docs, "Creates a user.");
    }

    #[test]
    fn request_as_plain_string_is_a_get() {
        let r = one(r#"{"name": "Ping", "request": "https://example.com/ping?x=1"}"#);
        assert_eq!(r.method, "GET");
        assert_eq!(r.url, "https://example.com/ping?x=1");
        assert_eq!(r.body, Body::default());
        assert_eq!(r.auth, Auth::Inherit);
    }

    #[test]
    fn disabled_query_params_are_stripped_from_raw_url() {
        let r = one(r#"{"name": "Search", "request": {"method": "GET", "url": {
                "raw": "{{baseUrl}}/search?q=caf%C3%A9&debug=true&sort=name+asc&flag#top",
                "host": ["{{baseUrl}}"], "path": ["search"],
                "query": [
                    {"key": "q", "value": "caf%C3%A9"},
                    {"key": "debug", "value": "true", "disabled": true, "description": "Verbose output"},
                    {"key": "sort", "value": "name+asc"},
                    {"key": "flag", "value": null},
                    {"key": "page", "value": "2", "disabled": true}
                ]}}}"#);
        assert_eq!(r.url, "{{baseUrl}}/search?q=caf%C3%A9&sort=name+asc&flag#top");
        assert_eq!(
            r.disabled_params,
            [
                KeyValue {
                    key: "debug".into(),
                    value: "true".into(),
                    enabled: false,
                    description: "Verbose output".into()
                },
                KeyValue { key: "page".into(), value: "2".into(), enabled: false, description: String::new() },
            ]
        );

        // Postman normally leaves disabled params out of `raw`; an enabled twin must survive.
        let r = one(r#"{"name": "Twins", "request": {"url": {"raw": "https://x.test/a?id=1&only=1",
                "query": [{"key": "id", "value": "1"}, {"key": "id", "value": "1", "disabled": true},
                          {"key": "only", "value": "1", "disabled": true}]}}}"#);
        assert_eq!(r.url, "https://x.test/a?id=1");
        assert_eq!(r.disabled_params.len(), 2);

        let r = one(r#"{"name": "All off", "request": {"url": {"raw": "https://x.test/a?a=1",
                "query": [{"key": "a", "value": "1", "disabled": true}]}}}"#);
        assert_eq!(r.url, "https://x.test/a");
    }

    #[test]
    fn url_is_built_from_parts_when_raw_is_missing() {
        let r = one(r#"{"name": "Parts", "request": {"method": "GET", "url": {
                "protocol": "https", "host": ["api", "example", "com"], "port": "8443",
                "path": ["v1", "users", ":id"],
                "query": [{"key": "expand", "value": "orgs"}, {"key": "trace", "value": "1", "disabled": true}],
                "variable": [{"key": "id", "value": "42", "description": "User id"}],
                "hash": "details"}}}"#);
        assert_eq!(r.url, "https://api.example.com:8443/v1/users/:id?expand=orgs#details");
        assert_eq!(r.disabled_params.len(), 1);
        assert_eq!(
            r.path_params,
            [KeyValue { key: "id".into(), value: "42".into(), enabled: true, description: "User id".into() }]
        );

        let r = one(r#"{"name": "Str parts", "request": {"url": {"host": "{{host}}", "path": "a/b"}}}"#);
        assert_eq!(r.url, "{{host}}/a/b");
    }

    #[test]
    fn path_variables_become_path_params() {
        let r = one(r#"{"name": "Get order", "request": {"method": "GET", "url": {
                "raw": "{{baseUrl}}/users/:userId/orders/:orderId",
                "host": ["{{baseUrl}}"], "path": ["users", ":userId", "orders", ":orderId"],
                "variable": [
                    {"key": "userId", "value": "{{userId}}"},
                    {"key": "orderId", "value": 7, "type": "number"},
                    {"value": "no key"}
                ]}}}"#);
        assert_eq!(r.url, "{{baseUrl}}/users/:userId/orders/:orderId");
        assert_eq!(r.path_params, [KeyValue::new("userId", "{{userId}}"), KeyValue::new("orderId", "7")]);
    }

    #[test]
    fn headers_in_object_and_string_forms() {
        let r = one(r#"{"name": "Headers", "request": {"method": "GET", "url": "https://x.test", "header": [
                {"key": "Accept", "value": "application/json", "type": "text"},
                {"key": "X-Debug", "value": "1", "disabled": true, "description": "Server-side tracing"},
                "X-Legacy: yes: really",
                {"key": "", "value": ""},
                42
            ]}}"#);
        assert_eq!(
            r.headers,
            [
                KeyValue::new("Accept", "application/json"),
                KeyValue {
                    key: "X-Debug".into(),
                    value: "1".into(),
                    enabled: false,
                    description: "Server-side tracing".into()
                },
                KeyValue::new("X-Legacy", "yes: really"),
            ]
        );

        let r = one(r#"{"name": "Block", "request": {"url": "https://x.test",
                "header": "Content-Type: application/json\n//X-Off: 1\n\n"}}"#);
        let mut off = KeyValue::new("X-Off", "1");
        off.enabled = false;
        assert_eq!(r.headers, [KeyValue::new("Content-Type", "application/json"), off]);
    }

    #[test]
    fn raw_body_languages() {
        let raw = |lang: &str| {
            one(&format!(
                r#"{{"name": "R", "request": {{"method": "POST", "url": "https://x.test",
                    "body": {{"mode": "raw", "raw": "payload", "options": {{"raw": {{"language": "{lang}"}}}}}}}}}}"#
            ))
            .body
        };
        let expect = |body_type, content_type: Option<&str>| Body {
            body_type,
            text: "payload".into(),
            content_type: content_type.map(String::from),
            ..Body::default()
        };
        assert_eq!(raw("json"), expect(BodyType::Json, None));
        assert_eq!(raw("xml"), expect(BodyType::Xml, None));
        assert_eq!(raw("html"), expect(BodyType::Text, Some("text/html")));
        assert_eq!(raw("text"), expect(BodyType::Text, Some("text/plain")));
        assert_eq!(raw("javascript"), expect(BodyType::Text, Some("application/javascript")));

        // No language: fall back to the Content-Type header.
        let r = one(r#"{"name": "Old", "request": {"method": "PUT", "url": "https://x.test",
                "header": [{"key": "content-type", "value": "application/vnd.api+json"}],
                "body": {"mode": "raw", "raw": "{\"a\": 1}"}}}"#);
        assert_eq!(r.body.body_type, BodyType::Json);
        assert_eq!(r.body.text, r#"{"a": 1}"#);
    }

    #[test]
    fn form_bodies() {
        let c = import(&collection(
            r#"
            {"name": "Login", "request": {"method": "POST", "url": "https://x.test/login", "body": {
                "mode": "urlencoded", "urlencoded": [
                    {"key": "user", "value": "ada", "type": "text"},
                    {"key": "remember", "value": "true", "disabled": true}
                ]}}},
            {"name": "Upload", "request": {"method": "POST", "url": "https://x.test/upload", "body": {
                "mode": "formdata", "formdata": [
                    {"key": "title", "value": "Report", "type": "text"},
                    {"key": "doc", "type": "file", "src": "/Users/ada/report.pdf", "contentType": "application/pdf"},
                    {"key": "pics", "type": "file", "src": ["/tmp/a.png", "/tmp/b.png"]},
                    {"key": "missing", "type": "file", "src": []},
                    {"key": "old", "value": "x", "disabled": true}
                ]}}}
            "#,
        ));
        let login = request(&c, "Login");
        assert_eq!(login.body.body_type, BodyType::FormUrlencoded);
        let mut remember = KeyValue::new("remember", "true");
        remember.enabled = false;
        assert_eq!(login.body.form, [KeyValue::new("user", "ada"), remember]);

        let upload = request(&c, "Upload");
        assert_eq!(upload.body.body_type, BodyType::Multipart);
        let field = |key: &str, value: &str, file, content_type: Option<&str>, enabled| MultipartField {
            key: key.into(),
            value: value.into(),
            file,
            content_type: content_type.map(String::from),
            enabled,
        };
        assert_eq!(
            upload.body.multipart,
            [
                field("title", "Report", false, None, true),
                field("doc", "/Users/ada/report.pdf", true, Some("application/pdf"), true),
                field("pics", "/tmp/a.png", true, None, true),
                field("missing", "", true, None, true),
                field("old", "x", false, None, false),
            ]
        );
        assert_eq!(c.warnings.len(), 2, "{:?}", c.warnings);
        assert!(c.warnings.iter().any(|w| w.contains("'Upload'") && w.contains("'pics'")));
        assert!(c.warnings.iter().any(|w| w.contains("'Upload'") && w.contains("'missing'")));
    }

    #[test]
    fn file_body_is_binary() {
        let c = import(&collection(
            r#"
            {"name": "Put blob", "request": {"method": "PUT", "url": "https://x.test/blob",
                "body": {"mode": "file", "file": {"src": "/data/blob.bin"}}}},
            {"name": "No file", "request": {"method": "PUT", "url": "https://x.test/blob",
                "body": {"mode": "file", "file": {}}}}
            "#,
        ));
        let blob = request(&c, "Put blob");
        assert_eq!(blob.body.body_type, BodyType::Binary);
        assert_eq!(blob.body.file, "/data/blob.bin");
        assert_eq!(request(&c, "No file").body.body_type, BodyType::Binary);
        assert_eq!(c.warnings.len(), 1);
        assert!(c.warnings[0].contains("'No file'"));
    }

    #[test]
    fn graphql_body_is_imported_as_graphql() {
        let c = import(&collection(
            r#"{"name": "Viewer", "request": {"method": "POST", "url": "https://x.test/graphql", "body": {
                "mode": "graphql",
                "graphql": {"query": "query Repos($n: Int) { viewer { repos(first: $n) { name } } }",
                            "variables": "{\n  \"n\": 5\n}", "operationName": "Repos"}}}}"#,
        ));
        let r = request(&c, "Viewer");
        assert_eq!(r.body.body_type, BodyType::Graphql);
        assert_eq!(
            r.body.graphql,
            GraphqlBody {
                query: "query Repos($n: Int) { viewer { repos(first: $n) { name } } }".into(),
                variables: "{\n  \"n\": 5\n}".into(),
                operation_name: Some("Repos".into()),
                ..Default::default()
            }
        );
        assert!(c.warnings.is_empty(), "{:?}", c.warnings);

        let r = one(r#"{"name": "No vars", "request": {"method": "POST", "url": "https://x.test/graphql",
                "body": {"mode": "graphql", "graphql": {"query": "{ ping }", "variables": ""}}}}"#);
        assert_eq!(r.body.graphql, GraphqlBody { query: "{ ping }".into(), ..Default::default() });

        // Variables as an object; `{{variables}}` that only become JSON when rendered are kept as typed.
        let r = one(r#"{"name": "Object", "request": {"method": "POST", "url": "https://x.test/graphql",
                "body": {"mode": "graphql", "graphql": {"query": "{ a }", "variables": {"n": 1}}}}}"#);
        assert_eq!(r.body.graphql.variables, "{\n  \"n\": 1\n}");
        let c = import(&collection(
            r#"{"name": "Placeholder", "request": {"method": "POST", "url": "https://x.test/graphql",
                "body": {"mode": "graphql", "graphql": {"query": "{ a }", "variables": "{\"n\": {{n}}}"}}}},
               {"name": "Broken", "request": {"method": "POST", "url": "https://x.test/graphql",
                "body": {"mode": "graphql", "graphql": {"query": "{ a }", "variables": "{nope"}}}}"#,
        ));
        assert_eq!(request(&c, "Placeholder").body.graphql.variables, "{\"n\": {{n}}}");
        assert_eq!(request(&c, "Broken").body.graphql.variables, "{nope");
        assert_eq!(c.warnings, ["Request 'Broken': GraphQL variables are not valid JSON; fix them before sending."]);
    }

    #[test]
    fn empty_disabled_and_unknown_bodies_are_none() {
        let c = import(&collection(
            r#"
            {"name": "Empty raw", "request": {"method": "GET", "url": "https://x.test",
                "body": {"mode": "raw", "raw": "", "options": {"raw": {"language": "json"}}}}},
            {"name": "Disabled", "request": {"method": "POST", "url": "https://x.test",
                "body": {"mode": "raw", "raw": "{}", "disabled": true}}},
            {"name": "None", "request": {"method": "POST", "url": "https://x.test", "body": {"mode": "none"}}},
            {"name": "Null", "request": {"method": "POST", "url": "https://x.test", "body": null}},
            {"name": "Empty form", "request": {"method": "POST", "url": "https://x.test",
                "body": {"mode": "urlencoded", "urlencoded": []}}},
            {"name": "Weird", "request": {"method": "POST", "url": "https://x.test", "body": {"mode": "carrier-pigeon"}}}
            "#,
        ));
        for r in all_requests(&c.items).into_iter().filter(|r| r.name != "Disabled") {
            assert_eq!(r.body, Body::default(), "{}", r.name);
        }
        // Postman doesn't send a disabled body; it is kept for when it's needed again.
        let disabled = &request(&c, "Disabled").body;
        assert_eq!((disabled.body_type, disabled.text.as_str()), (BodyType::None, "{}"));
        assert_eq!(c.warnings.len(), 1);
        assert!(c.warnings[0].contains("'Weird'") && c.warnings[0].contains("carrier-pigeon"));
    }

    #[test]
    fn v21_auth_shapes() {
        let c = import(&collection(
            r#"
            {"name": "Basic", "request": {"url": "https://x.test", "auth": {"type": "basic", "basic": [
                {"key": "password", "value": "s3cret", "type": "string"},
                {"key": "username", "value": "ada", "type": "string"}]}}},
            {"name": "Bearer", "request": {"url": "https://x.test", "auth": {"type": "bearer", "bearer": [
                {"key": "token", "value": "{{token}}", "type": "string"}]}}},
            {"name": "Key", "request": {"url": "https://x.test", "auth": {"type": "apikey", "apikey": [
                {"key": "value", "value": "abc", "type": "string"},
                {"key": "key", "value": "api_key", "type": "string"},
                {"key": "in", "value": "query", "type": "string"}]}}},
            {"name": "Key header", "request": {"url": "https://x.test", "auth": {"type": "apikey", "apikey": [
                {"key": "key", "value": "X-Api-Key"}, {"key": "value", "value": "abc"}]}}},
            {"name": "None", "request": {"url": "https://x.test", "auth": {"type": "noauth"}}},
            {"name": "Inherit", "request": {"url": "https://x.test", "auth": null}}
            "#,
        ));
        assert_eq!(request(&c, "Basic").auth, Auth::Basic { username: "ada".into(), password: "s3cret".into() });
        assert_eq!(request(&c, "Bearer").auth, Auth::Bearer { token: "{{token}}".into(), prefix: "Bearer".into() });
        assert_eq!(
            request(&c, "Key").auth,
            Auth::ApiKey { key: "api_key".into(), value: "abc".into(), location: ApiKeyLocation::Query }
        );
        assert_eq!(
            request(&c, "Key header").auth,
            Auth::ApiKey { key: "X-Api-Key".into(), value: "abc".into(), location: ApiKeyLocation::Header }
        );
        assert_eq!(request(&c, "None").auth, Auth::None);
        assert_eq!(request(&c, "Inherit").auth, Auth::Inherit);
        assert_eq!(c.auth, Auth::None);
        assert!(c.warnings.is_empty());
    }

    #[test]
    fn v20_auth_shapes() {
        let c = import(
            r#"{"info": {"name": "Old", "schema": "https://schema.getpostman.com/json/collection/v2.0.0/collection.json"},
                "auth": {"type": "bearer", "bearer": {"token": "root-token"}},
                "item": [
                    {"name": "Basic", "request": {"url": "https://x.test", "method": "GET",
                        "auth": {"type": "basic", "basic": {"username": "ada", "password": "pw", "saveHelperData": true}}}},
                    {"name": "Key", "request": {"url": "https://x.test", "method": "GET",
                        "auth": {"type": "apikey", "apikey": {"key": "k", "value": "v", "in": "header"}}}}
                ]}"#,
        );
        assert_eq!(c.auth, Auth::Bearer { token: "root-token".into(), prefix: "Bearer".into() });
        assert_eq!(request(&c, "Basic").auth, Auth::Basic { username: "ada".into(), password: "pw".into() });
        assert_eq!(
            request(&c, "Key").auth,
            Auth::ApiKey { key: "k".into(), value: "v".into(), location: ApiKeyLocation::Header }
        );
        assert!(c.warnings.is_empty(), "{:?}", c.warnings);
    }

    #[test]
    fn folder_and_collection_auth() {
        let c = import(
            r#"{"info": {"name": "Auth", "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json"},
                "auth": {"type": "apikey", "apikey": [{"key": "key", "value": "X-Key"}, {"key": "value", "value": "{{key}}"}]},
                "item": [
                    {"name": "Admin", "auth": {"type": "basic", "basic": [{"key": "username", "value": "root"}]}, "item": [
                        {"name": "Inherits", "request": {"url": "https://x.test"}}
                    ]},
                    {"name": "Public", "auth": {"type": "noauth"}, "item": []},
                    {"name": "Plain", "item": []}
                ]}"#,
        );
        assert_eq!(
            c.auth,
            Auth::ApiKey { key: "X-Key".into(), value: "{{key}}".into(), location: ApiKeyLocation::Header }
        );
        assert_eq!(folder(&c.items, "Admin").0.auth, Auth::Basic { username: "root".into(), password: "".into() });
        assert_eq!(folder(&c.items, "Public").0.auth, Auth::None);
        assert_eq!(folder(&c.items, "Plain").0.auth, Auth::Inherit);
        assert_eq!(request(&c, "Inherits").auth, Auth::Inherit);
    }

    #[test]
    fn unsupported_auth_becomes_none_with_warning() {
        let c = import(
            r#"{"info": {"name": "Legacy", "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json"},
                "auth": {"type": "kerberos", "kerberos": []},
                "item": [{"name": "Plain", "request": {"url": "https://x.test"}}]}"#,
        );
        assert_eq!(c.auth, Auth::None);
        assert_eq!(c.warnings, ["Collection: 'kerberos' auth is not supported; imported with no auth."]);
    }

    #[test]
    fn signing_auth_types() {
        let c = import(&collection(
            r#"
            {"name": "Digest", "request": {"url": "https://x.test", "auth": {"type": "digest", "digest": [
                {"key": "username", "value": "u"}, {"key": "password", "value": "p"}, {"key": "algorithm", "value": "MD5"}]}}},
            {"name": "NTLM", "request": {"url": "https://x.test", "auth": {"type": "ntlm", "ntlm": [
                {"key": "username", "value": "u"}, {"key": "password", "value": "p"}, {"key": "domain", "value": "CORP"}]}}},
            {"name": "AWS", "request": {"url": "https://x.test", "auth": {"type": "awsv4", "awsv4": [
                {"key": "accessKey", "value": "AKID"}, {"key": "secretKey", "value": "{{aws_secret}}"},
                {"key": "region", "value": "eu-west-1"}, {"key": "service", "value": "execute-api"},
                {"key": "addAuthDataToQuery", "value": true}]}}},
            {"name": "OAuth1", "request": {"url": "https://x.test", "auth": {"type": "oauth1", "oauth1": [
                {"key": "consumerKey", "value": "ck"}, {"key": "consumerSecret", "value": "cs"},
                {"key": "signatureMethod", "value": "HMAC-SHA256"}, {"key": "addParamsToHeader", "value": false},
                {"key": "version", "value": "1.0"}, {"key": "includeBodyHash", "value": true}]}}},
            {"name": "JWT", "request": {"url": "https://x.test", "auth": {"type": "jwt", "jwt": [
                {"key": "algorithm", "value": "RS256"}, {"key": "privateKey", "value": "-----BEGIN PRIVATE KEY-----"},
                {"key": "payload", "value": "{\"sub\": \"1\"}"}, {"key": "addTokenTo", "value": "queryParam"},
                {"key": "queryParamKey", "value": "jwt"}]}}},
            {"name": "Hawk", "request": {"url": "https://x.test", "auth": {"type": "hawk", "hawk": [
                {"key": "authId", "value": "id"}, {"key": "authKey", "value": "k"}, {"key": "algorithm", "value": "sha1"},
                {"key": "extraData", "value": "e"}, {"key": "includePayloadHash", "value": true}]}}},
            {"name": "EdgeGrid", "request": {"url": "https://x.test", "auth": {"type": "edgegrid", "edgegrid": [
                {"key": "accessToken", "value": "at"}, {"key": "clientToken", "value": "ct"},
                {"key": "clientSecret", "value": "sec"}]}}},
            {"name": "ASAP", "request": {"url": "https://x.test", "auth": {"type": "asap", "asap": [
                {"key": "iss", "value": "svc"}, {"key": "aud", "value": "api"}, {"key": "kid", "value": "svc/1"},
                {"key": "privateKey", "value": "pem"}, {"key": "exp", "value": "15m"}]}}}
            "#,
        ));
        assert!(c.warnings.is_empty(), "{:?}", c.warnings);
        assert_eq!(request(&c, "Digest").auth, Auth::Digest { username: "u".into(), password: "p".into() });
        assert_eq!(
            request(&c, "NTLM").auth,
            Auth::Ntlm { username: "u".into(), password: "p".into(), domain: "CORP".into(), workstation: "".into() }
        );
        let Auth::AwsSigV4(aws) = &request(&c, "AWS").auth else { panic!("aws") };
        assert_eq!(
            (aws.access_key.as_str(), aws.secret_key.as_str(), aws.region.as_str(), aws.service.as_str(), aws.location),
            ("AKID", "{{aws_secret}}", "eu-west-1", "execute-api", ApiKeyLocation::Query)
        );
        let Auth::OAuth1(o1) = &request(&c, "OAuth1").auth else { panic!("oauth1") };
        assert_eq!(
            (o1.signature_method, o1.location, o1.include_version, o1.include_body_hash),
            (OAuth1Method::HmacSha256, ApiKeyLocation::Query, true, true)
        );
        let Auth::Jwt(jwt) = &request(&c, "JWT").auth else { panic!("jwt") };
        assert_eq!(
            (jwt.algorithm, jwt.secret.as_str(), jwt.location, jwt.query_param.as_str()),
            (JwtAlgorithm::RS256, "-----BEGIN PRIVATE KEY-----", ApiKeyLocation::Query, "jwt")
        );
        let Auth::Hawk(hawk) = &request(&c, "Hawk").auth else { panic!("hawk") };
        assert_eq!((hawk.algorithm, hawk.ext.as_str(), hawk.include_payload_hash), (HawkAlgorithm::Sha1, "e", true));
        let Auth::EdgeGrid(eg) = &request(&c, "EdgeGrid").auth else { panic!("edgegrid") };
        assert_eq!((eg.client_secret.as_str(), eg.max_body), ("sec", 131_072));
        let Auth::Asap(asap) = &request(&c, "ASAP").auth else { panic!("asap") };
        assert_eq!((asap.key_id.as_str(), asap.expires_in, asap.algorithm), ("svc/1", 900, JwtAlgorithm::RS256));
    }

    #[test]
    fn examples_are_imported() {
        let r = one(r#"{"name": "Get user", "request": {"method": "GET", "url": "https://api.test/users/1"},
                "response": [
                    {"name": "Found", "code": 200, "header": [{"key": "Content-Type", "value": "application/json"}],
                     "body": "{\"id\": 1}", "originalRequest": {"method": "GET", "url": "https://api.test/users/1"}},
                    {"code": 404, "body": "", "originalRequest": {"method": "GET", "url": {
                        "raw": "https://api.test/users/9?full=1", "query": [{"key": "full", "value": "1"}]}}}
                ]}"#);
        assert_eq!(r.examples.len(), 2);
        assert_eq!((r.examples[0].name.as_str(), r.examples[0].status), ("Found", 200));
        assert_eq!(r.examples[0].headers, vec![KeyValue::new("Content-Type", "application/json")]);
        assert_eq!((r.examples[0].body.as_str(), r.examples[0].url.as_str()), (r#"{"id": 1}"#, ""));
        assert_eq!((r.examples[1].name.as_str(), r.examples[1].status), ("Example 2", 404));
        assert_eq!(r.examples[1].url, "https://api.test/users/9?full=1");
    }

    #[test]
    fn oauth2_mapping() {
        let c = import(&collection(
            r#"
            {"name": "PKCE", "request": {"url": "https://x.test", "auth": {"type": "oauth2", "oauth2": [
                {"key": "grant_type", "value": "authorization_code_with_pkce"},
                {"key": "authUrl", "value": "https://id.example.com/authorize"},
                {"key": "accessTokenUrl", "value": "https://id.example.com/oauth/token"},
                {"key": "clientId", "value": "{{clientId}}"},
                {"key": "clientSecret", "value": "{{clientSecret}}"},
                {"key": "scope", "value": "openid profile"},
                {"key": "redirect_uri", "value": "https://oauth.pstmn.io/v1/callback"},
                {"key": "useBrowser", "value": true},
                {"key": "client_authentication", "value": "body"},
                {"key": "headerPrefix", "value": "Token"},
                {"key": "tokenRequestParams", "value": [
                    {"key": "audience", "value": "https://api.example.com", "enabled": true, "send_as": "request_body"},
                    {"key": "resource", "value": "r", "enabled": true, "send_as": "request_body"},
                    {"key": "ignored", "value": "x", "enabled": false}
                ]},
                {"key": "addTokenTo", "value": "header"}]}}},
            {"name": "Machine", "request": {"url": "https://x.test", "auth": {"type": "oauth2", "oauth2": {
                "grant_type": "client_credentials", "accessTokenUrl": "https://id.example.com/token",
                "clientId": "svc", "clientSecret": "shh", "client_authentication": "header"}}}},
            {"name": "Password", "request": {"url": "https://x.test", "auth": {"type": "oauth2", "oauth2": [
                {"key": "grant_type", "value": "password_credentials"},
                {"key": "accessTokenUrl", "value": "https://id.example.com/token"},
                {"key": "username", "value": "ada"}, {"key": "password", "value": "pw"},
                {"key": "headerPrefix", "value": ""}]}}},
            {"name": "Implicit", "request": {"url": "https://x.test", "auth": {"type": "oauth2", "oauth2": [
                {"key": "grant_type", "value": "implicit"},
                {"key": "authUrl", "value": "https://id.example.com/authorize"},
                {"key": "redirect_uri", "value": "http://localhost:8080/cb"}]}}},
            {"name": "Saved token", "request": {"url": "https://x.test", "auth": {"type": "oauth2", "oauth2": [
                {"key": "accessToken", "value": "eyJhbGciOi"}, {"key": "tokenType", "value": "Bearer"}]}}},
            {"name": "Script token", "request": {"url": "https://x.test", "auth": {"type": "oauth2", "oauth2": [
                {"key": "accessToken", "value": "{{access_token}}"},
                {"key": "accessTokenUrl", "value": "https://id.example.com/token"},
                {"key": "grant_type", "value": "client_credentials"}]}}}
            "#,
        ));
        let Auth::OAuth2(pkce) = &request(&c, "PKCE").auth else { panic!("oauth2") };
        assert_eq!(
            *pkce,
            OAuth2Config {
                grant_type: GrantType::AuthorizationCode,
                token_url: "https://id.example.com/oauth/token".into(),
                auth_url: "https://id.example.com/authorize".into(),
                redirect_uri: DEFAULT_REDIRECT_URI.into(),
                client_id: "{{clientId}}".into(),
                client_secret: "{{clientSecret}}".into(),
                scope: "openid profile".into(),
                audience: "https://api.example.com".into(),
                username: String::new(),
                password: String::new(),
                client_auth: ClientAuthMethod::Body,
                pkce: true,
                header_prefix: "Token".into(),
            }
        );

        let Auth::OAuth2(machine) = &request(&c, "Machine").auth else { panic!("oauth2") };
        assert_eq!(machine.grant_type, GrantType::ClientCredentials);
        assert_eq!(machine.token_url, "https://id.example.com/token");
        assert_eq!((machine.client_id.as_str(), machine.client_secret.as_str()), ("svc", "shh"));
        assert_eq!(machine.client_auth, ClientAuthMethod::BasicHeader);
        assert_eq!(machine.header_prefix, "Bearer");
        assert_eq!(machine.redirect_uri, DEFAULT_REDIRECT_URI);

        let Auth::OAuth2(password) = &request(&c, "Password").auth else { panic!("oauth2") };
        assert_eq!(password.grant_type, GrantType::Password);
        assert_eq!((password.username.as_str(), password.password.as_str()), ("ada", "pw"));
        assert_eq!(password.header_prefix, "");

        let Auth::OAuth2(implicit) = &request(&c, "Implicit").auth else { panic!("oauth2") };
        assert_eq!(implicit.grant_type, GrantType::Implicit);
        assert_eq!(implicit.redirect_uri, "http://localhost:8080/cb");

        assert_eq!(
            request(&c, "Saved token").auth,
            Auth::Bearer { token: "eyJhbGciOi".into(), prefix: "Bearer".into() }
        );

        let warned = |name: &str, needle: &str| c.warnings.iter().any(|w| w.contains(name) && w.contains(needle));
        assert!(warned("'PKCE'", "register this URI"), "{:?}", c.warnings);
        assert!(warned("'PKCE'", "resource"));
        assert!(!c.warnings.iter().any(|w| w.contains("ignored")));
        assert!(warned("'Saved token'", "Bearer"));
        // A token a script keeps in a variable: Postman sends just that.
        assert_eq!(
            request(&c, "Script token").auth,
            Auth::Bearer { token: "{{access_token}}".into(), prefix: "Bearer".into() }
        );
        assert!(warned("'Script token'", "{{access_token}}"));
        assert_eq!(c.warnings.len(), 4, "{:?}", c.warnings);
    }

    #[test]
    fn scripts_are_imported() {
        let c = import(
            r#"{"info": {"name": "Scripts", "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json"},
                "event": [
                    {"listen": "prerequest", "script": {"type": "text/javascript", "exec": ["pm.variables.set('t', Date.now());"]}},
                    {"listen": "test", "script": {"type": "text/javascript", "exec": [""]}}
                ],
                "item": [{"name": "F", "event": [{"listen": "test", "script": {"exec": "pm.test('ok', () => {});"}}], "item": [
                    {"name": "R", "event": [
                        {"listen": "prerequest", "script": {"exec": ["console.log(1)", "", "console.log(2)"]}},
                        {"listen": "test", "script": {"exec": ["pm.response.to.have.status(200);"]}},
                        {"listen": "test", "disabled": true, "script": {"exec": ["off()"]}}
                    ], "request": {"url": "https://x.test"}}
                ]}]}"#,
        );
        assert!(c.warnings.is_empty(), "{:?}", c.warnings);
        assert_eq!(
            c.scripts,
            Scripts { pre_request: "pm.variables.set('t', Date.now());".into(), ..Default::default() }
        );
        let (f, _) = folder(&c.items, "F");
        assert_eq!(f.scripts, Scripts { post_response: "pm.test('ok', () => {});".into(), ..Default::default() });
        let r = request(&c, "R");
        assert_eq!(r.scripts.pre_request, "console.log(1)\n\nconsole.log(2)");
        assert_eq!(r.scripts.post_response, "pm.response.to.have.status(200);");
    }

    #[test]
    fn unsupported_script_apis_are_reported() {
        let item = |name: &str, code: &str| json!({"name": name, "event": [{"listen": "prerequest", "script": {"exec": [code]}}], "request": {"url": "https://x.test"}});
        let c = import(
            &json!({"info": {"name": "U"}, "item": [
                item("A", "pm.vault.get('k'); pm.execution.runRequest('x');"),
                item("B", "const _ = require('lodash'); pm.sendRequest('x', () => {}); setTimeout(() => {}, 1);"),
                item("C", "pm.vault.get('a');"),
                item("D", "pm.vault.get('b');"),
                item("E", "pm.vault.get('c'); pm.vault.get('d');"),
                {"name": "F", "event": [{"listen": "test", "script": {"exec": "pm.execution.runRequest('')"}}], "item": []},
                {"name": "G", "event": [{"listen": "shutdown", "script": {"exec": "x()"}}], "request": {"url": "https://x.test"}}
            ]})
            .to_string(),
        );
        assert_eq!(
            c.warnings,
            [
                "Request 'G': skipped a 'shutdown' script (only pre-request and test scripts run).",
                "4 scripts use pm.vault, which Zorvik does not support (it fails when run): Request 'A', Request 'C', Request 'D', ….",
                "2 scripts use pm.execution.runRequest, which Zorvik does not support (it fails when run): Request 'A', Folder 'F'.",
            ]
        );
        assert!(request(&c, "C").scripts.pre_request.contains("pm.vault"), "imported anyway");
    }

    #[test]
    fn collection_variables() {
        let c = import(
            r#"{"info": {"name": "Vars", "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json"},
                "item": [],
                "variable": [
                    {"key": "baseUrl", "value": "https://api.example.com", "type": "string"},
                    {"key": "apiKey", "value": "sk_test_123", "type": "secret"},
                    {"key": "retries", "value": 3, "type": "number"},
                    {"key": "verbose", "value": true, "disabled": true},
                    {"id": "legacyId", "value": "v2.0 style"},
                    {"key": "", "value": "no key"},
                    {"value": "no key either"}
                ]}"#,
        );
        let var = |key: &str, value: &str, enabled, secret| Variable {
            key: key.into(),
            value: value.into(),
            enabled,
            secret,
        };
        assert!(c.variables.is_empty(), "collection variables are workspace variables, not an environment");
        assert_eq!(
            c.workspace_variables,
            [
                var("baseUrl", "https://api.example.com", true, false),
                var("apiKey", "sk_test_123", true, true),
                var("retries", "3", true, false),
                var("verbose", "true", false, false),
                var("legacyId", "v2.0 style", true, false),
            ]
        );
        assert_eq!(c.request_count(), 0);
    }

    #[test]
    fn protocol_profile_behavior_maps_to_settings() {
        let c = import(&collection(
            r#"
            {"name": "Lax", "protocolProfileBehavior": {"strictSSL": false, "maxRedirects": 3}, "item": [
                {"name": "No follow", "protocolProfileBehavior": {"followRedirects": false, "disableBodyPruning": true},
                 "request": {"url": "https://self-signed.test"}},
                {"name": "Strict", "protocolProfileBehavior": {"strictSSL": true},
                 "request": {"url": "https://self-signed.test"}}
            ]},
            {"name": "Default", "request": {"url": "https://x.test"}}
            "#,
        ));
        let no_follow = &request(&c, "No follow").settings;
        assert_eq!(no_follow.follow_redirects, Some(false));
        assert_eq!(no_follow.verify_tls, Some(false));
        assert_eq!(no_follow.max_redirects, Some(3));
        assert_eq!(request(&c, "Strict").settings.verify_tls, Some(true));
        assert_eq!(request(&c, "Default").settings, RequestSettings::default());
    }

    #[test]
    fn auth_replaces_the_requests_own_auth_header() {
        let c = import(&format!(
            r#"{{"info": {{"name": "T", "schema": "{V21}"}},
                "auth": {{"type": "bearer", "bearer": [{{"key": "token", "value": "t"}}]}},
                "item": [
                {{"name": "Inherits", "request": {{"url": "https://x.test",
                    "header": [{{"key": "authorization", "value": "Bearer stale"}}, {{"key": "Accept", "value": "*/*"}}]}}}},
                {{"name": "No auth", "request": {{"url": "https://x.test", "auth": {{"type": "noauth"}},
                    "header": [{{"key": "Authorization", "value": "Custom x"}}]}}}},
                {{"name": "Hawk", "request": {{"url": "https://x.test",
                    "auth": {{"type": "hawk", "hawk": [{{"key": "authId", "value": "a"}}]}},
                    "header": [{{"key": "Authorization", "value": "Hawk id=\"old\""}}]}}}},
                {{"name": "Key", "request": {{"url": "https://x.test",
                    "auth": {{"type": "apikey", "apikey": [{{"key": "key", "value": "X-Key"}}, {{"key": "value", "value": "k"}}]}},
                    "header": [{{"key": "X-Key", "value": "old"}}, {{"key": "Authorization", "value": "Mine"}}]}}}},
                {{"name": "Signed URL", "request": {{"url": "https://x.test",
                    "auth": {{"type": "awsv4", "awsv4": [{{"key": "addAuthDataToQuery", "value": true}}]}},
                    "header": [{{"key": "Authorization", "value": "Mine"}}]}}}}
            ]}}"#
        ));
        let enabled = |name: &str| -> Vec<String> {
            request(&c, name).headers.iter().filter(|h| h.enabled).map(|h| h.key.clone()).collect()
        };
        assert_eq!(enabled("Inherits"), ["Accept"]);
        assert_eq!(request(&c, "Inherits").headers[0].value, "Bearer stale");
        assert_eq!(enabled("No auth"), ["Authorization"]);
        assert!(enabled("Hawk").is_empty());
        assert_eq!(enabled("Key"), ["Authorization"]);
        assert_eq!(enabled("Signed URL"), ["Authorization"]);
        assert_eq!(c.warnings.len(), 1);
        assert!(
            c.warnings[0].starts_with("3 requests had their own auth header as well as an auth setting"),
            "{}",
            c.warnings[0]
        );
        assert!(c.warnings[0].ends_with("Request 'Inherits', Request 'Hawk', Request 'Key'."), "{}", c.warnings[0]);
    }

    #[test]
    fn bodies_postman_does_not_send_are_kept_but_off() {
        let c = import(&collection(
            r#"
            {"name": "Get", "request": {"method": "GET", "url": "https://x.test",
                "body": {"mode": "raw", "raw": "{\"a\": 1}", "options": {"raw": {"language": "json"}}}}},
            {"name": "Head", "request": {"method": "head", "url": "https://x.test",
                "body": {"mode": "urlencoded", "urlencoded": [{"key": "a", "value": "1"}]}}},
            {"name": "Delete", "request": {"method": "DELETE", "url": "https://x.test", "body": {"mode": "raw", "raw": "x"}}},
            {"name": "Kept", "protocolProfileBehavior": {"disableBodyPruning": true},
             "request": {"method": "GET", "url": "https://x.test", "body": {"mode": "raw", "raw": "x"}}},
            {"name": "Folder", "protocolProfileBehavior": {"disableBodyPruning": true}, "item": [
                {"name": "Inner", "request": {"method": "GET", "url": "https://x.test", "body": {"mode": "raw", "raw": "x"}}}
            ]}
            "#,
        ));
        let get = &request(&c, "Get").body;
        assert_eq!((get.body_type, get.text.as_str()), (BodyType::None, "{\"a\": 1}"));
        let head = &request(&c, "Head").body;
        assert_eq!((head.body_type, head.form[0].key.as_str()), (BodyType::None, "a"));
        for sent in ["Delete", "Kept", "Inner"] {
            assert_eq!(request(&c, sent).body.body_type, BodyType::Text, "{sent}");
        }
        assert_eq!(c.warnings.len(), 1);
        assert!(
            c.warnings[0].starts_with("2 GET, HEAD, COPY, PURGE or UNLOCK requests had a body"),
            "{}",
            c.warnings[0]
        );
    }

    #[test]
    fn unsupported_request_settings_are_reported() {
        let c = import(&format!(
            r#"{{"info": {{"name": "T", "schema": "{V21}"}},
                "protocolProfileBehavior": {{"disableUrlEncoding": true, "followRedirects": true}},
                "item": [
                {{"name": "A", "protocolProfileBehavior": {{"followOriginalHttpMethod": true, "tlsDisabledProtocols": []}},
                  "request": {{"url": "https://x.test"}}}},
                {{"name": "B", "protocolProfileBehavior": {{"followOriginalHttpMethod": false, "disableCookies": true}},
                  "request": {{"url": "https://x.test"}}}}
            ]}}"#
        ));
        assert_eq!(request(&c, "A").settings.follow_redirects, Some(true));
        let mut warnings = c.warnings.clone();
        warnings.sort();
        assert_eq!(warnings.len(), 3, "{warnings:?}");
        assert!(warnings[0].starts_with("1 place uses the Postman setting \"disableCookies\""), "{}", warnings[0]);
        assert!(warnings[1].starts_with("1 place uses the Postman setting \"disableUrlEncoding\""), "{}", warnings[1]);
        assert!(warnings[1].ends_with(": Collection."), "{}", warnings[1]);
        assert!(warnings[2].contains("\"followOriginalHttpMethod\"") && warnings[2].ends_with(": Request 'A'."));
    }

    #[test]
    fn odd_entries_are_skipped_with_warnings() {
        let c = import(&collection(
            r#"
            "not an item",
            {"name": "Orphan"},
            {"name": "Null request", "request": null},
            {"name": "Box", "variable": [{"key": "folderVar", "value": "1"}], "item": [
                {"name": "Weird url", "request": {"method": "", "url": 12, "header": {"oops": true},
                    "body": "raw text?", "auth": "basic", "description": 5}}
            ]}
            "#,
        ));
        assert_eq!(c.request_count(), 1);
        let r = request(&c, "Weird url");
        assert_eq!((r.method.as_str(), r.url.as_str()), ("GET", ""));
        assert_eq!(r.seq, 1);
        assert_eq!(folder(&c.items, "Box").0.seq, 1);
        assert_eq!(
            c.warnings,
            [
                "Collection: skipped an entry that is not a request or folder.",
                "Collection: skipped 'Orphan', which is neither a request nor a folder.",
                "Collection: skipped 'Null request', which is neither a request nor a folder.",
                "Folder 'Box': folder variables are not supported and were skipped.",
            ]
        );
    }

    #[test]
    fn invalid_json_is_an_error() {
        let err = import_postman_collection(r#"{"info": {"name": "x"}, "item": ["#).unwrap_err();
        assert!(err.0.starts_with("Invalid JSON: "), "{err}");
        assert!(import_postman_collection("[1, 2]").is_err());
        assert!(import_postman_collection(r#"{"hello": "world"}"#).unwrap_err().0.contains("Not a Postman collection"));
    }

    #[test]
    fn v1_collections_are_converted() {
        // Folders nest through `folders_order`; root requests follow the folders.
        let v1 = r#"{"id": "c1", "name": "Old", "description": "From 2016",
            "order": ["r1"], "folders_order": ["f1"],
            "folders": [
                {"id": "f1", "name": "Users", "order": ["r2"], "folders_order": ["f2"],
                 "auth": {"type": "bearer", "bearer": [{"key": "token", "value": "{{token}}"}]}},
                {"id": "f2", "name": "Admin", "order": ["r3"], "folders_order": [], "description": "Admins only"}
            ],
            "variables": [{"key": "base", "value": "https://x.test"}],
            "requests": [
                {"id": "r1", "name": "Ping", "url": "{{base}}/ping?a=1", "method": "GET",
                 "headers": "Accept: */*\n// X-Off: 1\n", "dataMode": "params", "data": [],
                 "queryParams": [{"key": "a", "value": "1", "equals": true},
                                 {"key": "b", "value": "2", "equals": true, "enabled": false}],
                 "tests": "tests['ok'] = responseCode.code === 200;", "preRequestScript": "",
                 "responses": [{"id": "x1", "name": "Pong", "responseCode": {"code": 200, "name": "OK"},
                                "headers": [{"key": "Content-Type", "value": "text/plain"}], "text": "pong"}]},
                {"id": "r2", "name": "Create user", "url": "{{base}}/users/:id", "method": "POST",
                 "pathVariables": {"id": "7"},
                 "headerData": [{"key": "Content-Type", "value": "application/json"},
                                {"key": "X-Debug", "value": "1", "enabled": false}],
                 "dataMode": "raw", "rawModeData": "{\"name\": \"Ann\"}", "dataOptions": {"raw": {"language": "json"}},
                 "currentHelper": "basicAuth", "helperAttributes": {"id": "basic", "username": "u", "password": "p"},
                 "events": [{"listen": "prerequest", "script": {"exec": ["pm.variables.set('n', 1);"]}}]},
                {"id": "r3", "name": "Upload", "url": "{{base}}/files", "method": "POST",
                 "dataMode": "params", "data": [{"key": "file", "value": "a.txt", "type": "file", "enabled": true},
                                                {"key": "note", "value": "hi", "type": "text", "enabled": false}],
                 "currentHelper": "hawkAuth", "helperAttributes": "{\"hawk_id\": \"id1\", \"hawk_key\": \"k\", \"algorithm\": \"sha1\"}"},
                {"id": "r4", "name": "Orphan", "url": "{{base}}/form", "method": "PUT",
                 "dataMode": "urlencoded", "data": [{"key": "q", "value": "1", "type": "text"}]},
                {"id": "r5", "name": "Query", "url": "{{base}}/graphql", "method": "POST",
                 "dataMode": "graphql", "graphqlModeData": {"query": "{ me { id } }", "variables": "{}"}}
            ]}"#;
        assert!(is_collection(&serde_json::from_str(v1).unwrap()));
        let c = import(v1);
        assert_eq!(c.name, "Old");
        assert!(c.warnings.is_empty(), "{:?}", c.warnings);
        assert_eq!(names(&c.items), ["1 Users/", "2 Ping", "3 Orphan", "4 Query"]);
        let (users, children) = folder(&c.items, "Users");
        assert_eq!(names(children), ["1 Admin/", "2 Create user"]);
        assert!(matches!(&users.auth, Auth::Bearer { token, .. } if token == "{{token}}"));
        assert_eq!(folder(children, "Admin").0.docs, "Admins only");
        assert_eq!(c.workspace_variables[0].key, "base");

        let ping = request(&c, "Ping");
        assert_eq!(ping.url, "{{base}}/ping?a=1");
        assert_eq!(ping.disabled_params[0].key, "b");
        assert_eq!(
            ping.headers,
            [KeyValue::new("Accept", "*/*"), KeyValue { enabled: false, ..KeyValue::new("X-Off", "1") }]
        );
        assert_eq!(ping.scripts.post_response, "tests['ok'] = responseCode.code === 200;");
        assert_eq!(
            (ping.examples[0].name.as_str(), ping.examples[0].status, ping.examples[0].body.as_str()),
            ("Pong", 200, "pong")
        );

        let create = request(&c, "Create user");
        assert_eq!(create.path_params[0].key, "id");
        assert_eq!(create.path_params[0].value, "7");
        assert!(!create.headers[1].enabled);
        assert_eq!((create.body.body_type, create.body.text.as_str()), (BodyType::Json, "{\"name\": \"Ann\"}"));
        assert!(matches!(&create.auth, Auth::Basic { username, password } if username == "u" && password == "p"));
        assert_eq!(create.scripts.pre_request, "pm.variables.set('n', 1);");

        let upload = request(&c, "Upload");
        assert_eq!(upload.body.multipart[0].value, "a.txt");
        assert!(upload.body.multipart[0].file);
        assert!(!upload.body.multipart[1].enabled);
        assert!(matches!(&upload.auth, Auth::Hawk(h) if h.id == "id1" && h.algorithm == HawkAlgorithm::Sha1));
        assert_eq!(request(&c, "Orphan").body.form[0].key, "q");
        assert_eq!(request(&c, "Query").body.graphql.query, "{ me { id } }");
    }

    #[test]
    fn v1_folders_listing_themselves_end() {
        let v1 = r#"{"name": "Loop", "folders": [{"id": "f", "name": "F", "order": [], "folders_order": ["f"]}],
            "order": [], "requests": []}"#;
        assert_eq!(names(&import(v1).items), ["1 F/"]);
    }

    #[test]
    fn v1_responses_listed_again_are_taken_once() {
        // A 1 MB example named 2000 times must not become 2 GB of examples.
        let mut order: Vec<Value> = (0..2000).map(|_| json!("x1")).collect();
        order.extend([json!("x2"), json!("nope")]);
        let big = "x".repeat(1 << 20);
        let v1 = json!({"name": "Old", "order": ["r1"], "requests": [
            {"id": "r1", "name": "Big", "url": "https://x.test", "method": "GET",
             "responses": [{"id": "x1", "name": "One", "text": big}, {"id": "x2", "name": "Two"}],
             "responses_order": order},
            {"name": "No id", "url": "https://x.test/2", "method": "GET"}
        ]});
        let c = import(&v1.to_string());
        let examples: Vec<&str> = request(&c, "Big").examples.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(examples, ["One", "Two"]);
        // A request without an id to list it by is kept too.
        assert_eq!(names(&c.items), ["1 Big", "2 No id"]);
    }

    #[test]
    fn detects_postman_files() {
        let yes = [
            format!(r#"{{"info": {{"name": "A", "schema": "{V21}"}}, "item": []}}"#),
            r#"{"info": {"name": "No schema"}, "item": []}"#.to_string(),
            r#"{"name": "No info", "item": [{"name": "R", "request": "https://x.test"}]}"#.to_string(),
            r#"{"collection": {"info": {"name": "API"}, "item": []}}"#.to_string(),
            r#"{"name": "Old", "order": [], "requests": []}"#.to_string(),
        ];
        for text in &yes {
            assert!(is_collection(&serde_json::from_str(text).unwrap()), "{text}");
        }
        let envs = [
            r#"{"name": "Dev", "values": []}"#,
            r#"{"values": [{"key": "a", "value": "1"}], "_postman_variable_scope": "globals"}"#,
            r#"{"environment": {"name": "Dev", "values": []}}"#,
            r#"{"values": [{"key": "a", "value": "1"}]}"#,
        ];
        for text in envs {
            let json = serde_json::from_str(text).unwrap();
            assert!(is_environment(&json) && !is_collection(&json), "{text}");
        }
        let no = [
            r#"{"openapi": "3.0.0", "item": []}"#,
            r#"{"item": [1, 2]}"#,
            r#"{"item": []}"#,
            r#"{"values": [1]}"#,
            r#"[]"#,
        ];
        for text in no {
            let json = serde_json::from_str(text).unwrap();
            assert!(!is_collection(&json) && !is_environment(&json), "{text}");
        }
    }

    #[test]
    fn environment_passed_as_collection_is_rejected() {
        let env = r#"{"name": "Dev", "values": [{"key": "a", "value": "1", "enabled": true}]}"#;
        assert!(import_postman_collection(env).unwrap_err().0.contains("environment, not a collection"));
    }

    #[test]
    fn bom_api_wrapper_and_unknown_schema_are_tolerated() {
        let json = "\u{feff}{\"collection\": {\"info\": {\"name\": \" Wrapped \", \"schema\": \"https://example.com/v9\"}, \
                    \"item\": [{\"name\": \"A\", \"request\": \"https://x.test\"}]}}";
        let c = import(json);
        assert_eq!(c.name, "Wrapped");
        assert_eq!(c.request_count(), 1);
        assert_eq!(c.warnings, ["Unrecognized collection schema 'https://example.com/v9'; imported as v2.1."]);

        let unnamed = import(r#"{"info": {"schema": "v2.1.0"}, "item": []}"#);
        assert_eq!(unnamed.name, "Imported collection");
    }

    #[test]
    fn environment_import() {
        let env = import_postman_environment(
            r#"{
                "id": "5e7a1f2c-0000-4000-8000-000000000000",
                "name": " Staging ",
                "values": [
                    {"key": "baseUrl", "value": "https://staging.example.com", "type": "default", "enabled": true},
                    {"key": "token", "value": "abc", "type": "secret", "enabled": true},
                    {"key": "old", "value": "x", "enabled": false},
                    {"key": "also old", "value": "y", "disabled": true},
                    {"key": "port", "value": 8080},
                    {"key": "", "value": "dropped"},
                    "garbage"
                ],
                "_postman_variable_scope": "environment",
                "_postman_exported_at": "2026-01-01T00:00:00.000Z"
            }"#,
        )
        .unwrap();
        assert_eq!(env.name, "Staging");
        assert_eq!(
            env.variables,
            [
                Variable {
                    key: "baseUrl".into(),
                    value: "https://staging.example.com".into(),
                    enabled: true,
                    secret: false
                },
                Variable { key: "token".into(), value: "abc".into(), enabled: true, secret: true },
                Variable { key: "old".into(), value: "x".into(), enabled: false, secret: false },
                Variable { key: "also old".into(), value: "y".into(), enabled: false, secret: false },
                Variable { key: "port".into(), value: "8080".into(), enabled: true, secret: false },
            ]
        );

        let globals = import_postman_environment(r#"{"values": [], "_postman_variable_scope": "globals"}"#).unwrap();
        assert_eq!(globals, Environment { name: "Globals".into(), variables: vec![] });
    }

    #[test]
    fn environment_rejects_other_files() {
        let err = import_postman_environment(&collection("")).unwrap_err();
        assert!(err.0.contains("collection, not an environment"), "{err}");
        let err = import_postman_environment(r#"{"name": "x"}"#).unwrap_err();
        assert!(err.0.contains("\"values\""), "{err}");
        assert!(import_postman_environment("{").unwrap_err().0.starts_with("Invalid JSON: "));
        assert!(import_postman_environment("\"str\"").is_err());
    }

    #[test]
    fn large_collection_imports_quickly() {
        let folders: Vec<Value> = (0..40)
            .map(|f| {
                let items: Vec<Value> = (0..50)
                    .map(|r| {
                        json!({
                            "name": format!("Request {f}-{r}"),
                            "request": {
                                "method": if r % 2 == 0 { "PUT" } else { "POST" },
                                "header": [{"key": "Accept", "value": "application/json"}],
                                "url": {
                                    "raw": format!("{{{{baseUrl}}}}/items/{r}?page=1&debug=1"),
                                    "host": ["{{baseUrl}}"],
                                    "path": ["items", r.to_string()],
                                    "query": [{"key": "page", "value": "1"}, {"key": "debug", "value": "1", "disabled": true}]
                                },
                                "body": {"mode": "raw", "raw": "{\"n\": 1}", "options": {"raw": {"language": "json"}}}
                            },
                            "event": [{"listen": "test", "script": {"exec": ["pm.test('ok', () => {});"]}}]
                        })
                    })
                    .collect();
                json!({"name": format!("Folder {f}"), "item": items})
            })
            .collect();
        let json = json!({"info": {"name": "Big", "schema": V21}, "item": folders}).to_string();

        let start = std::time::Instant::now();
        let c = import(&json);
        let elapsed = start.elapsed();
        assert_eq!(c.request_count(), 2000);
        assert_eq!(request(&c, "Request 39-49").url, "{{baseUrl}}/items/49?page=1");
        assert_eq!(request(&c, "Request 39-49").seq, 50);
        assert!(c.warnings.is_empty(), "{:?}", c.warnings);
        assert_eq!(request(&c, "Request 39-49").scripts.post_response, "pm.test('ok', () => {});");
        assert!(elapsed.as_secs_f64() < 2.0, "took {elapsed:?}");
    }

    #[test]
    fn many_disabled_query_params_import_quickly() {
        let n = 20_000;
        let mut query: Vec<Value> =
            (0..n).map(|i| json!({"key": format!("k{i}"), "value": "v", "disabled": true})).collect();
        query.push(json!({"key": "on", "value": "1"}));
        let raw = format!("https://x.test/?{}&on=1", (0..n).map(|i| format!("k{i}=v")).collect::<Vec<_>>().join("&"));
        let json =
            json!({"info": {"name": "Q"}, "item": [{"name": "R", "request": {"url": {"raw": raw, "query": query}}}]});

        let start = std::time::Instant::now();
        let c = import(&json.to_string());
        assert!(start.elapsed().as_secs_f64() < 5.0, "took {:?}", start.elapsed());
        let r = request(&c, "R");
        assert_eq!(r.url, "https://x.test/?on=1");
        assert_eq!(r.disabled_params.len(), n);
    }
}

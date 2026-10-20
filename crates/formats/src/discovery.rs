//! Google API Discovery documents (`discovery#restDescription`, the format Google publishes
//! for Gmail, Drive, Gemini, Cloud and its other REST APIs), converted to OpenAPI 3 so the
//! OpenAPI importer, response checks, mock servers and updates work for them unchanged.
//! Works on an untyped JSON tree like the importer: parts that cannot be mapped are left out.

use std::collections::HashMap;

use indexmap::{IndexMap, IndexSet};
use serde_json::{Map, Number, Value, json};

use crate::import::ImportError;

/// Discovery documents name only the scopes; these are Google's OAuth 2.0 endpoints.
const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const METHODS: [&str; 8] = ["get", "put", "post", "delete", "options", "head", "patch", "trace"];
/// Deepest resource or inline schema nesting converted; deeper parts are left out. `$ref`s
/// are not followed here, so recursive schemas cost nothing until the importer bounds them.
const MAX_DEPTH: usize = 32;

/// Whether `doc` is a Google API Discovery document (and not an OpenAPI one).
pub fn is_discovery(doc: &Value) -> bool {
    if doc.get("openapi").is_some() || doc.get("swagger").is_some() {
        return false;
    }
    let object = |key| doc.get(key).is_some_and(Value::is_object);
    doc.get("kind").and_then(Value::as_str) == Some("discovery#restDescription")
        || (doc.get("discoveryVersion").is_some() && (object("resources") || object("methods")))
}

/// A Discovery document as an OpenAPI 3.0 document: one operation per method, one tag per
/// resource (`users.messages`), schemas in `components`, and Google's OAuth 2.0 and API key.
pub fn to_openapi(doc: &Value) -> Result<Value, ImportError> {
    if !is_discovery(doc) {
        return Err(ImportError::new("Not a Google API Discovery document"));
    }
    Ok(Converter::new(doc).run())
}

struct Converter<'a> {
    doc: &'a Value,
    /// The document has OAuth 2.0 scopes (an `oauth2` scheme).
    oauth2: bool,
    /// The document takes the `key` parameter (an `apiKey` scheme).
    api_key: bool,
    paths: Map<String, Value>,
    tags: IndexSet<String>,
    /// How many methods accept each scope, in order of first appearance.
    scope_counts: IndexMap<&'a str, usize>,
    /// The last number given to each repeated `path#fragment` (with its HTTP method), so many
    /// methods repeating one URL don't each try all the numbers before theirs.
    repeats: HashMap<(String, String), usize>,
}

impl<'a> Converter<'a> {
    fn new(doc: &'a Value) -> Self {
        Self {
            doc,
            oauth2: doc.pointer("/auth/oauth2/scopes").is_some_and(Value::is_object),
            api_key: doc.pointer("/parameters/key").is_some_and(Value::is_object),
            paths: Map::new(),
            tags: IndexSet::new(),
            scope_counts: IndexMap::new(),
            repeats: HashMap::new(),
        }
    }

    fn run(mut self) -> Value {
        let doc = self.doc;
        self.resource(doc, &mut Vec::new(), 0);

        let name = str_of(doc, "name").trim();
        let version = str_of(doc, "version").trim();
        let title = match str_of(doc, "title").trim() {
            "" => format!("{name} {version}").trim().to_string(),
            title => title.to_string(),
        };
        let mut info = Map::new();
        info.insert("title".into(), if title.is_empty() { "Google API".into() } else { title.into() });
        info.insert("version".into(), if version.is_empty() { "1".into() } else { version.into() });
        if let Some(description) = text(doc, "description") {
            info.insert("description".into(), description.into());
        }

        let mut out = Map::new();
        out.insert("openapi".into(), "3.0.3".into());
        out.insert("info".into(), info.into());
        if let Some(link) = text(doc, "documentationLink") {
            out.insert("externalDocs".into(), json!({ "url": link }));
        }
        // Method paths are relative to `rootUrl` + `servicePath`; `baseUrl` is the older name.
        let root = str_of(doc, "rootUrl").trim();
        let server = match root {
            "" => str_of(doc, "baseUrl").trim().to_string(),
            root => {
                format!("{}/{}", root.trim_end_matches('/'), str_of(doc, "servicePath").trim().trim_start_matches('/'))
            }
        };
        let server = server.trim_end_matches('/');
        // Braces would make the address reference variables (`{{secret}}`); Google's never has any.
        if !server.is_empty() && !server.contains(['{', '}']) {
            out.insert("servers".into(), json!([{ "url": server }]));
        }
        if !self.tags.is_empty() {
            out.insert("tags".into(), self.tags.iter().map(|t| json!({ "name": t })).collect());
        }
        if let Some(security) = self.root_security() {
            out.insert("security".into(), security);
        }
        out.insert("paths".into(), std::mem::take(&mut self.paths).into());
        out.insert("components".into(), self.components());
        Value::Object(out)
    }

    /// The methods of a resource (the document itself at the top), then its sub-resources,
    /// in document order. `path` holds the names of the resources it is nested in.
    fn resource(&mut self, node: &'a Value, path: &mut Vec<&'a str>, depth: usize) {
        if depth > MAX_DEPTH {
            return;
        }
        if let Some(methods) = node.get("methods").and_then(Value::as_object) {
            // Top-level methods get no tag, so they stay outside the folders.
            let tag = (!path.is_empty()).then(|| path.join("."));
            if let Some(tag) = &tag
                && !methods.is_empty()
            {
                self.tags.insert(tag.clone());
            }
            for (name, method) in methods {
                self.method(tag.as_deref(), path.last().copied(), name, method);
            }
        }
        for (name, child) in node.get("resources").and_then(Value::as_object).into_iter().flatten() {
            path.push(name);
            self.resource(child, path, depth + 1);
            path.pop();
        }
    }

    fn method(&mut self, tag: Option<&str>, resource: Option<&str>, name: &str, m: &'a Value) {
        let http = str_of(m, "httpMethod").trim().to_ascii_lowercase();
        if !METHODS.contains(&http.as_str()) {
            return;
        }
        let id = match str_of(m, "id").trim() {
            "" => {
                [tag.unwrap_or_default(), name].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(".")
            }
            id => id.to_string(),
        };
        let Some(path) = self.pick_path(m, &http, &id) else { return };

        let mut op = Map::new();
        op.insert("operationId".into(), id.into());
        // Short, and unique in its folder: method names are unique within a resource.
        let summary = match resource {
            Some(resource) => format!("{resource}.{name}"),
            None => name.to_string(),
        };
        op.insert("summary".into(), summary.into());
        if let Some(description) = text(m, "description") {
            op.insert("description".into(), description.into());
        }
        if let Some(tag) = tag {
            op.insert("tags".into(), json!([tag]));
        }
        if m.get("deprecated") == Some(&Value::Bool(true)) {
            op.insert("deprecated".into(), true.into());
        }
        let parameters = parameters(&path, m.get("parameters").and_then(Value::as_object));
        if !parameters.is_empty() {
            op.insert("parameters".into(), parameters.into());
        }
        if let Some(schema) = body_schema(m.get("request")) {
            op.insert(
                "requestBody".into(),
                json!({ "required": true, "content": { "application/json": { "schema": schema } } }),
            );
        }
        let mut ok = json!({ "description": "Successful response" });
        if let Some(schema) = body_schema(m.get("response")) {
            ok["content"] = json!({ "application/json": { "schema": schema } });
        }
        // Google APIs answer errors in one format (https://google.aip.dev/193).
        op.insert("responses".into(), json!({ "200": ok, "default": { "$ref": "#/components/responses/Error" } }));
        if let Some(security) = self.operation_security(m) {
            op.insert("security".into(), security);
        }
        // `pick_path` made sure the path has no operation for this method yet.
        if let Some(item) = self.paths.entry(path).or_insert_with(|| json!({})).as_object_mut() {
            item.insert(http, op.into());
        }
    }

    /// The method's `flatPath` (one variable per segment, so methods sharing a `{+name}` path,
    /// such as `models` and `tunedModels` `generateContent`, stay apart), else its `path`,
    /// whichever is still free for its HTTP method. When both are taken the method repeats
    /// another one's URL: a `#fragment` naming it keeps both (fragments are never sent).
    fn pick_path(&mut self, m: &Value, http: &str, id: &str) -> Option<String> {
        let candidates: Vec<String> =
            ["flatPath", "path"].into_iter().filter_map(|key| m.get(key)?.as_str()).map(template).collect();
        let paths = &self.paths;
        let free = |path: &str| paths.get(path).and_then(|item| item.get(http)).is_none();
        if let Some(path) = candidates.iter().find(|p| free(p)) {
            return Some(path.clone());
        }
        let fragment: String = id.chars().filter(|c| c.is_ascii_alphanumeric() || "._-".contains(*c)).collect();
        let base = format!("{}#{fragment}", candidates.first()?);
        let last = self.repeats.entry((base.clone(), http.to_string())).or_insert(0);
        let (n, path) = (*last + 1..)
            .map(|n| (n, if n == 1 { base.clone() } else { format!("{base}-{n}") }))
            .find(|(_, p)| free(p))?;
        *last = n;
        Some(path)
    }

    /// A method's scopes are alternatives (any one of them will do), so each is a requirement
    /// of its own, then the API key. A method without scopes takes only the API key.
    fn operation_security(&mut self, m: &'a Value) -> Option<Value> {
        if !self.oauth2 {
            // Covered by the root requirement, if any.
            return None;
        }
        let scopes: Vec<&'a str> =
            m.get("scopes").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect();
        for scope in &scopes {
            *self.scope_counts.entry(scope).or_default() += 1;
        }
        let mut reqs: Vec<Value> = scopes.iter().map(|s| json!({ "oauth2": [s] })).collect();
        if self.api_key {
            reqs.push(json!({ "apiKey": [] }));
        }
        (!reqs.is_empty()).then(|| reqs.into())
    }

    /// OAuth 2.0 first (with the scope most methods accept), then the API key.
    fn root_security(&self) -> Option<Value> {
        let mut reqs = Vec::new();
        if self.oauth2 {
            let mut best: Option<(&str, usize)> = None;
            for (scope, count) in &self.scope_counts {
                if best.is_none_or(|(_, most)| *count > most) {
                    best = Some((scope, *count));
                }
            }
            let scopes: Vec<&str> = best.map(|(scope, _)| scope).into_iter().collect();
            reqs.push(json!({ "oauth2": scopes }));
        }
        if self.api_key {
            reqs.push(json!({ "apiKey": [] }));
        }
        (!reqs.is_empty()).then(|| reqs.into())
    }

    fn components(&self) -> Value {
        let doc = self.doc;
        let mut components = Map::new();
        let schemas: Map<String, Value> = doc
            .get("schemas")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
            .filter(|(_, s)| s.is_object())
            .map(|(name, s)| (name.clone(), schema(s, 0)))
            .collect();
        components.insert("schemas".into(), schemas.into());
        components.insert(
            "responses".into(),
            json!({ "Error": {
                "description": "Error",
                "content": { "application/json": { "schema": {
                    "type": "object",
                    "properties": { "error": {
                        "type": "object",
                        "properties": {
                            "code": { "type": "integer" },
                            "message": { "type": "string" },
                            "status": { "type": "string" },
                            "errors": { "type": "array", "items": { "type": "object" } },
                            "details": { "type": "array", "items": { "type": "object" } }
                        }
                    } }
                } } }
            } }),
        );
        let mut schemes = Map::new();
        if let Some(scopes) = doc.pointer("/auth/oauth2/scopes").and_then(Value::as_object) {
            let scopes: Map<String, Value> =
                scopes.iter().map(|(scope, s)| (scope.clone(), str_of(s, "description").trim().into())).collect();
            schemes.insert(
                "oauth2".into(),
                json!({
                    "type": "oauth2",
                    "description": "Google OAuth 2.0",
                    "flows": { "authorizationCode": {
                        "authorizationUrl": AUTH_URL,
                        "tokenUrl": TOKEN_URL,
                        "scopes": scopes
                    } }
                }),
            );
        }
        if let Some(key) = doc.pointer("/parameters/key").filter(|_| self.api_key) {
            let mut scheme = json!({ "type": "apiKey", "in": "query", "name": "key" });
            if let Some(description) = text(key, "description") {
                scheme["description"] = description.into();
            }
            schemes.insert("apiKey".into(), scheme);
        }
        if !schemes.is_empty() {
            components.insert("securitySchemes".into(), schemes.into());
        }
        components.into()
    }
}

/// The method's parameters: every `{variable}` of the path (required; described by the method's
/// parameter of that name when there is one, else a string), then its query parameters.
/// The document-wide parameters (`key`, `alt`, `fields`, ...) are left out.
fn parameters(path: &str, defs: Option<&Map<String, Value>>) -> Vec<Value> {
    let variables = placeholders(path);
    let mut out: Vec<Value> =
        variables.iter().map(|name| parameter(name, "path", defs.and_then(|d| d.get(*name)))).collect();
    for (name, p) in defs.into_iter().flatten() {
        // A name with braces would reference variables (`{{secret}}`) in the query.
        if str_of(p, "location") == "query" && !variables.contains(&name.as_str()) && !name.contains(['{', '}']) {
            out.push(parameter(name, "query", Some(p)));
        }
    }
    out
}

fn parameter(name: &str, location: &str, p: Option<&Value>) -> Value {
    let mut s = match p {
        Some(p) => schema(p, 0),
        None => json!({ "type": "string" }),
    };
    let mut out = Map::new();
    out.insert("name".into(), name.into());
    out.insert("in".into(), location.into());
    let required = location == "path" || p.is_some_and(|p| p.get("required") == Some(&Value::Bool(true)));
    out.insert("required".into(), required.into());
    if let Some(fields) = s.as_object_mut() {
        // Parameters carry these themselves; enum descriptions go into the description.
        fields.remove("description");
        fields.remove("x-enumDescriptions");
        if fields.remove("deprecated").is_some() {
            out.insert("deprecated".into(), true.into());
        }
        fields.remove("readOnly");
    }
    let description = p.map(param_description).unwrap_or_default();
    if !description.is_empty() {
        out.insert("description".into(), description.into());
    }
    let repeated = location == "query" && p.is_some_and(|p| p.get("repeated") == Some(&Value::Bool(true)));
    out.insert("schema".into(), if repeated { json!({ "type": "array", "items": s }) } else { s });
    out.into()
}

/// A parameter's description, followed by what each of its values means.
fn param_description(p: &Value) -> String {
    let mut out = str_of(p, "description").trim().to_string();
    let values = p.get("enum").and_then(Value::as_array).into_iter().flatten();
    let notes = p.get("enumDescriptions").and_then(Value::as_array).into_iter().flatten();
    let lines: Vec<String> = values
        .zip(notes)
        .filter_map(|(v, note)| Some((v.as_str()?, note.as_str()?.trim())))
        .filter(|(_, note)| !note.is_empty())
        .map(|(v, note)| format!("- `{v}`: {note}"))
        .collect();
    if !lines.is_empty() {
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str(&lines.join("\n"));
    }
    out
}

/// A request or response body schema: `{"$ref": "Name"}` (or, rarely, an inline schema).
fn body_schema(body: Option<&Value>) -> Option<Value> {
    let body = body.filter(|b| b.get("$ref").is_some_and(Value::is_string) || b.get("type").is_some())?;
    Some(schema(body, 0))
}

/// A Discovery schema as an OpenAPI one. `$ref`s name a schema in `components` and are not
/// followed, so cycles cost nothing here; inline nesting deeper than `MAX_DEPTH` is left out.
fn schema(s: &Value, depth: usize) -> Value {
    let mut out = Map::new();
    let Some(s) = s.as_object() else { return out.into() };
    if let Some(name) = s.get("$ref").and_then(Value::as_str) {
        // `~` and `/` are escaped in JSON pointers.
        let name = name.replace('~', "~0").replace('/', "~1");
        out.insert("$ref".into(), format!("#/components/schemas/{name}").into());
    } else if depth < MAX_DEPTH {
        let kind = s.get("type").and_then(Value::as_str).unwrap_or_default();
        // `any` (and anything unknown) is any value: no type.
        if ["string", "integer", "number", "boolean", "object", "array"].contains(&kind) {
            out.insert("type".into(), kind.into());
        }
        // 64-bit integers are JSON strings in Google APIs (`type: string, format: int64`).
        match s.get("format").and_then(Value::as_str).map(str::trim) {
            Some("google-datetime") => {
                out.insert("format".into(), "date-time".into());
            }
            Some(format) if !format.is_empty() => {
                out.insert("format".into(), format.into());
            }
            _ => {}
        }
        if let Some(pattern) = s.get("pattern").filter(|p| p.is_string()) {
            out.insert("pattern".into(), pattern.clone());
        }
        // Discovery writes bounds and defaults as strings.
        for key in ["minimum", "maximum"] {
            if let Some(n) = s.get(key).filter(|_| matches!(kind, "integer" | "number")).and_then(number) {
                out.insert(key.into(), n);
            }
        }
        if let Some(values) = s.get("enum").filter(|v| v.is_array()) {
            out.insert("enum".into(), values.clone());
            if let Some(notes) = s.get("enumDescriptions").filter(|v| v.is_array()) {
                out.insert("x-enumDescriptions".into(), notes.clone());
            }
        }
        if let Some(default) = s.get("default").and_then(|d| typed(d, kind)) {
            out.insert("default".into(), default);
        }
        if let Some(props) = s.get("properties").and_then(Value::as_object) {
            let props: Map<String, Value> = props.iter().map(|(k, v)| (k.clone(), schema(v, depth + 1))).collect();
            out.insert("properties".into(), props.into());
        }
        let extra = match s.get("additionalProperties") {
            Some(extra @ Value::Object(_)) => Some(schema(extra, depth + 1)),
            Some(extra @ Value::Bool(_)) => Some(extra.clone()),
            _ => None,
        };
        if let Some(extra) = extra {
            out.insert("additionalProperties".into(), extra);
        }
        if let Some(items) = s.get("items").filter(|i| i.is_object()) {
            out.insert("items".into(), schema(items, depth + 1));
        }
    }
    // Kept beside a `$ref` too (as OpenAPI 3.1 allows): the importer honours `readOnly` there.
    let description = s.get("description").and_then(Value::as_str).unwrap_or_default();
    if !description.trim().is_empty() {
        out.insert("description".into(), description.into());
    }
    if s.get("deprecated") == Some(&Value::Bool(true)) {
        out.insert("deprecated".into(), true.into());
    }
    if s.get("readOnly") == Some(&Value::Bool(true)) || output_only(description) {
        out.insert("readOnly".into(), true.into());
    }
    out.into()
}

/// Google marks fields the server sets as "Output only." (older APIs: "[Output Only]").
fn output_only(description: &str) -> bool {
    let start: String = description.trim_start().chars().take(13).collect::<String>().to_ascii_lowercase();
    start.starts_with("output only") || start.starts_with("output-only") || start.starts_with("[output only]")
}

/// A Discovery default (always text) as a value of the schema's type; `None` when it isn't one.
fn typed(v: &Value, kind: &str) -> Option<Value> {
    let Value::String(s) = v else { return Some(v.clone()) };
    match kind {
        "integer" | "number" => number(v),
        "boolean" => match s.trim() {
            "true" => Some(true.into()),
            "false" => Some(false.into()),
            _ => None,
        },
        _ => Some(v.clone()),
    }
}

/// A number given as a number or as text.
fn number(v: &Value) -> Option<Value> {
    let s = match v {
        Value::Number(_) => return Some(v.clone()),
        Value::String(s) => s.trim(),
        _ => return None,
    };
    if let Ok(i) = s.parse::<i64>() {
        return Some(i.into());
    }
    if let Ok(u) = s.parse::<u64>() {
        return Some(u.into());
    }
    s.parse::<f64>().ok().and_then(Number::from_f64).map(Value::Number)
}

/// A Discovery path as an OpenAPI path: a leading `/`, and RFC 6570 reserved expansion
/// (`{+name}`, whose value may hold slashes) written as a plain `{name}`.
fn template(path: &str) -> String {
    let mut out = String::with_capacity(path.len() + 1);
    out.push('/');
    let mut rest = path.trim().trim_start_matches('/');
    while let Some(start) = rest.find('{') {
        let Some(len) = rest[start..].find('}') else { break };
        out.push_str(&rest[..start]);
        let name = rest[start + 1..start + len].trim_start_matches(['+', '#']).trim_end_matches('*');
        out.push('{');
        out.push_str(name);
        out.push('}');
        rest = &rest[start + len + 1..];
    }
    out.push_str(rest);
    out
}

/// The `{variable}` names of a path, in order, each once. Scanned like the importer does, so
/// both see the same names.
fn placeholders(path: &str) -> Vec<&str> {
    let mut names = Vec::new();
    let mut rest = path;
    while let Some(start) = rest.find('{') {
        let Some(len) = rest[start..].find('}') else { break };
        let name = &rest[start + 1..start + len];
        if !name.is_empty() && !names.contains(&name) {
            names.push(name);
        }
        rest = &rest[start + len + 1..];
    }
    names
}

fn str_of<'v>(v: &'v Value, key: &str) -> &'v str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

/// A non-blank text field, trimmed.
fn text<'v>(v: &'v Value, key: &str) -> Option<&'v str> {
    Some(str_of(v, key).trim()).filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::{ImportedCollection, ImportedItem};
    use crate::model::{ApiKeyLocation, Auth, GrantType, Request};
    use crate::openapi::{import_openapi, parse_document};

    const DEMO: &str = "https://www.googleapis.com/auth/demo";
    const READONLY: &str = "https://www.googleapis.com/auth/demo.readonly";

    /// A small Discovery document shaped like Gmail's and Gemini's.
    fn sample() -> Value {
        json!({
            "kind": "discovery#restDescription",
            "discoveryVersion": "v1",
            "name": "demo",
            "version": "v1",
            "title": "Demo API",
            "description": "Manages demo data.",
            "documentationLink": "https://developers.google.com/demo",
            "rootUrl": "https://demo.googleapis.com/",
            "servicePath": "demo/v1/",
            "parameters": {
                "key": {"type": "string", "location": "query", "description": "API key for your project."},
                "alt": {"type": "string", "location": "query", "enum": ["json", "media"], "default": "json"},
                "fields": {"type": "string", "location": "query"},
                "$.xgafv": {"type": "string", "location": "query", "enum": ["1", "2"]}
            },
            "auth": {"oauth2": {"scopes": {
                READONLY: {"description": "See your demo data"},
                DEMO: {"description": "See and edit your demo data"}
            }}},
            "methods": {
                "ping": {"id": "demo.ping", "httpMethod": "GET", "path": "ping", "description": "Checks the service.",
                         "scopes": [DEMO]}
            },
            "resources": {
                "users": {
                    "methods": {
                        "getProfile": {
                            "id": "demo.users.getProfile", "httpMethod": "GET",
                            "path": "users/{userId}/profile", "flatPath": "users/{userId}/profile",
                            "parameters": {"userId": {"type": "string", "location": "path", "required": true,
                                                      "default": "me", "description": "The user."}},
                            "response": {"$ref": "Profile"},
                            "scopes": [READONLY]
                        }
                    },
                    "resources": {"messages": {"methods": {
                        "list": {
                            "id": "demo.users.messages.list", "httpMethod": "GET",
                            "path": "users/{userId}/messages", "flatPath": "users/{userId}/messages",
                            "description": "Lists the messages.",
                            "parameters": {
                                "userId": {"type": "string", "location": "path", "required": true, "default": "me"},
                                "maxResults": {"type": "integer", "format": "uint32", "location": "query",
                                               "default": "100", "minimum": "1", "maximum": "500"},
                                "labelIds": {"type": "string", "location": "query", "repeated": true},
                                "format": {"type": "string", "location": "query", "description": "How much to return.",
                                           "enum": ["minimal", "full"],
                                           "enumDescriptions": ["Only IDs.", "The whole message."]},
                                "includeSpamTrash": {"type": "boolean", "location": "query", "default": "false"},
                                "view": {"type": "string", "location": "query", "required": true, "default": "basic"},
                                "oldFilter": {"type": "string", "location": "query", "deprecated": true}
                            },
                            "parameterOrder": ["userId"],
                            "response": {"$ref": "ListMessagesResponse"},
                            "scopes": [READONLY, DEMO]
                        },
                        "send": {
                            "id": "demo.users.messages.send", "httpMethod": "POST",
                            "path": "users/{userId}/messages/send", "flatPath": "users/{userId}/messages/send",
                            "parameters": {"userId": {"type": "string", "location": "path", "required": true, "default": "me"}},
                            "request": {"$ref": "Message"}, "response": {"$ref": "Message"},
                            "supportsMediaUpload": true,
                            "mediaUpload": {"accept": ["message/*"], "protocols": {
                                "simple": {"multipart": true, "path": "/upload/demo/v1/users/{userId}/messages/send"}
                            }},
                            "scopes": [DEMO]
                        },
                        "delete": {
                            "id": "demo.users.messages.delete", "httpMethod": "DELETE",
                            "path": "users/{userId}/messages/{id}", "flatPath": "users/{userId}/messages/{id}",
                            "parameters": {
                                "userId": {"type": "string", "location": "path", "required": true, "default": "me"},
                                "id": {"type": "string", "location": "path", "required": true}
                            },
                            "deprecated": true,
                            "scopes": [DEMO]
                        }
                    }}}
                },
                "models": {"methods": {"generateContent": {
                    "id": "demo.models.generateContent", "httpMethod": "POST",
                    "path": "v1/{+model}:generateContent", "flatPath": "v1/models/{modelsId}:generateContent",
                    "parameters": {"model": {"type": "string", "location": "path", "required": true,
                                             "pattern": "^models/[^/]+$"}},
                    "request": {"$ref": "GenerateRequest"}, "response": {"$ref": "GenerateResponse"}
                }}},
                "tunedModels": {"methods": {"generateContent": {
                    "id": "demo.tunedModels.generateContent", "httpMethod": "POST",
                    "path": "v1/{+model}:generateContent", "flatPath": "v1/tunedModels/{tunedModelsId}:generateContent",
                    "parameters": {"model": {"type": "string", "location": "path", "required": true,
                                             "pattern": "^tunedModels/[^/]+$"}},
                    "request": {"$ref": "GenerateRequest"}, "response": {"$ref": "GenerateResponse"}
                }}},
                "operations": {"methods": {"get": {
                    "id": "demo.operations.get", "httpMethod": "GET", "path": "v1/{+name}",
                    "parameters": {"name": {"type": "string", "location": "path", "required": true,
                                            "pattern": "^operations/.*$", "description": "The operation."}},
                    "scopes": [DEMO]
                }}}
            },
            "schemas": {
                "Profile": {"id": "Profile", "type": "object", "properties": {
                    "emailAddress": {"type": "string"},
                    "messagesTotal": {"type": "integer", "format": "int32"},
                    "historyId": {"type": "string", "format": "uint64"},
                    "createTime": {"type": "string", "format": "google-datetime", "description": "Output only. When it was made."}
                }},
                "ListMessagesResponse": {"id": "ListMessagesResponse", "type": "object", "properties": {
                    "messages": {"type": "array", "items": {"$ref": "Message"}},
                    "nextPageToken": {"type": "string"}
                }},
                "Message": {"id": "Message", "type": "object", "properties": {
                    "id": {"type": "string", "description": "Output only. The immutable ID."},
                    "raw": {"type": "string", "format": "byte"},
                    "labelIds": {"type": "array", "items": {"type": "string"}},
                    "payload": {"$ref": "MessagePart", "description": "The parsed email structure."},
                    "internalDate": {"type": "string", "format": "int64"}
                }},
                "MessagePart": {"id": "MessagePart", "type": "object", "properties": {
                    "mimeType": {"type": "string"},
                    "parts": {"type": "array", "items": {"$ref": "MessagePart"}},
                    "headers": {"type": "array", "items": {"type": "object", "properties": {
                        "name": {"type": "string"}, "value": {"type": "string"}
                    }}}
                }},
                "GenerateRequest": {"id": "GenerateRequest", "type": "object", "properties": {
                    "contents": {"type": "array", "items": {"$ref": "Content"}},
                    "generationConfig": {"type": "object", "properties": {
                        "temperature": {"type": "number", "format": "float", "minimum": "0", "maximum": "2"}
                    }},
                    "labels": {"type": "object", "additionalProperties": {"type": "string"}}
                }},
                "Content": {"id": "Content", "type": "object", "properties": {
                    "role": {"type": "string", "enum": ["user", "model"], "enumDescriptions": ["The user.", "The model."]},
                    "parts": {"type": "array", "items": {"$ref": "Part"}}
                }},
                "Part": {"id": "Part", "type": "object", "properties": {
                    "text": {"type": "string"},
                    "data": {"type": "any", "description": "Any JSON value."}
                }},
                "GenerateResponse": {"id": "GenerateResponse", "type": "object", "properties": {
                    "candidates": {"type": "array", "items": {"$ref": "Content"}}
                }}
            }
        })
    }

    fn operation<'d>(doc: &'d Value, path: &str, method: &str) -> &'d Value {
        doc["paths"].get(path).and_then(|i| i.get(method)).unwrap_or_else(|| panic!("no {method} {path}"))
    }

    fn param<'d>(op: &'d Value, name: &str) -> &'d Value {
        op["parameters"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|p| p["name"] == name)
            .unwrap_or_else(|| panic!("no {name}"))
    }

    fn requests(items: &[ImportedItem]) -> Vec<&Request> {
        items
            .iter()
            .flat_map(|i| match i {
                ImportedItem::Folder { children, .. } => requests(children),
                ImportedItem::Request(r) => vec![r],
            })
            .collect()
    }

    fn find<'c>(c: &'c ImportedCollection, name: &str) -> &'c Request {
        requests(&c.items).into_iter().find(|r| r.name == name).unwrap_or_else(|| panic!("no request {name}"))
    }

    #[test]
    fn detects_discovery_documents() {
        assert!(is_discovery(&sample()));
        assert!(is_discovery(&json!({"discoveryVersion": "v1", "resources": {}})));
        assert!(is_discovery(&json!({"discoveryVersion": "v1", "methods": {}})));
        assert!(!is_discovery(&json!({"discoveryVersion": "v1"})));
        assert!(!is_discovery(&json!({"kind": "discovery#directoryList", "items": []})));
        assert!(!is_discovery(&json!({"openapi": "3.0.0", "kind": "discovery#restDescription"})));
        assert!(!is_discovery(&json!([1, 2])));
        assert!(to_openapi(&json!({"openapi": "3.0.0"})).is_err());
    }

    #[test]
    fn info_servers_and_tags() {
        let doc = to_openapi(&sample()).unwrap();
        assert_eq!(doc["openapi"], "3.0.3");
        assert_eq!(doc["info"], json!({"title": "Demo API", "version": "v1", "description": "Manages demo data."}));
        assert_eq!(doc["externalDocs"]["url"], "https://developers.google.com/demo");
        assert_eq!(doc["servers"], json!([{"url": "https://demo.googleapis.com/demo/v1"}]));
        // One tag per resource with methods, nested ones joined with dots, in document order.
        assert_eq!(
            doc["tags"],
            json!([{"name": "users"}, {"name": "users.messages"}, {"name": "models"}, {"name": "tunedModels"}, {"name": "operations"}])
        );
        let list = operation(&doc, "/users/{userId}/messages", "get");
        assert_eq!(list["operationId"], "demo.users.messages.list");
        assert_eq!(list["summary"], "messages.list");
        assert_eq!(list["tags"], json!(["users.messages"]));
        assert_eq!(list["description"], "Lists the messages.");
        let ping = operation(&doc, "/ping", "get");
        assert_eq!(ping["summary"], "ping");
        assert!(ping.get("tags").is_none());
        assert_eq!(operation(&doc, "/users/{userId}/messages/{id}", "delete")["deprecated"], true);

        // Without `rootUrl`, `baseUrl` (the older name) is the server; with neither, there is none.
        let mut old = sample();
        old.as_object_mut().unwrap().remove("rootUrl");
        old["baseUrl"] = json!("https://www.googleapis.com/demo/v1/");
        assert_eq!(to_openapi(&old).unwrap()["servers"], json!([{"url": "https://www.googleapis.com/demo/v1"}]));
        old.as_object_mut().unwrap().remove("baseUrl");
        assert!(to_openapi(&old).unwrap().get("servers").is_none());
        // No title: name and version.
        old.as_object_mut().unwrap().remove("title");
        assert_eq!(to_openapi(&old).unwrap()["info"]["title"], "demo v1");
    }

    #[test]
    fn flat_paths_and_reserved_expansion() {
        let doc = to_openapi(&sample()).unwrap();
        // `flatPath` gives every segment its own variable, so the two `{+model}` methods stay apart.
        let generate = operation(&doc, "/v1/models/{modelsId}:generateContent", "post");
        assert_eq!(
            generate["parameters"],
            json!([{"name": "modelsId", "in": "path", "required": true, "schema": {"type": "string"}}])
        );
        operation(&doc, "/v1/tunedModels/{tunedModelsId}:generateContent", "post");
        // Without one, `{+name}` becomes a plain `{name}` described by the method's parameter.
        let get = operation(&doc, "/v1/{name}", "get");
        assert_eq!(
            get["parameters"],
            json!([{"name": "name", "in": "path", "required": true, "description": "The operation.",
                    "schema": {"type": "string", "pattern": "^operations/.*$"}}])
        );
        assert_eq!(template("v1/{+name}:cancel"), "/v1/{name}:cancel");
        assert_eq!(template("/a/{b}/{+c}/{d*}"), "/a/{b}/{c}/{d}");
        assert_eq!(template("a/{unclosed"), "/a/{unclosed");
        assert_eq!(placeholders("/a/{b}/{c}:x/{b}/{}"), vec!["b", "c"]);
        // Upload endpoints (`mediaUpload`) are left out; the method itself is kept.
        operation(&doc, "/users/{userId}/messages/send", "post");
        assert!(!doc.to_string().contains("/upload/"));
    }

    #[test]
    fn query_parameters() {
        let doc = to_openapi(&sample()).unwrap();
        let list = operation(&doc, "/users/{userId}/messages", "get");
        let names: Vec<&str> =
            list["parameters"].as_array().unwrap().iter().filter_map(|p| p["name"].as_str()).collect();
        // Path first; the document-wide parameters (`key`, `alt`, `fields`, ...) are left out.
        assert_eq!(names, ["userId", "maxResults", "labelIds", "format", "includeSpamTrash", "view", "oldFilter"]);
        assert_eq!(
            param(list, "userId"),
            &json!({"name": "userId", "in": "path", "required": true, "schema": {"type": "string", "default": "me"}})
        );
        // Defaults and bounds are typed; `repeated` is an array.
        assert_eq!(
            param(list, "maxResults")["schema"],
            json!({"type": "integer", "format": "uint32", "minimum": 1, "maximum": 500, "default": 100})
        );
        assert_eq!(param(list, "maxResults")["required"], false);
        assert_eq!(param(list, "labelIds")["schema"], json!({"type": "array", "items": {"type": "string"}}));
        assert_eq!(param(list, "includeSpamTrash")["schema"], json!({"type": "boolean", "default": false}));
        assert_eq!(param(list, "view")["required"], true);
        assert_eq!(param(list, "oldFilter")["deprecated"], true);
        // Enum values keep their meaning in the description.
        let format = param(list, "format");
        assert_eq!(format["schema"], json!({"type": "string", "enum": ["minimal", "full"]}));
        assert_eq!(
            format["description"],
            "How much to return.\n\n- `minimal`: Only IDs.\n- `full`: The whole message."
        );
    }

    #[test]
    fn bodies_responses_and_schemas() {
        let doc = to_openapi(&sample()).unwrap();
        let send = operation(&doc, "/users/{userId}/messages/send", "post");
        assert_eq!(
            send["requestBody"],
            json!({"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/Message"}}}})
        );
        assert_eq!(
            send["responses"]["200"]["content"]["application/json"]["schema"]["$ref"],
            "#/components/schemas/Message"
        );
        // Errors share Google's error format; a method without a response answers 200 without a body.
        assert_eq!(send["responses"]["default"], json!({"$ref": "#/components/responses/Error"}));
        let delete = operation(&doc, "/users/{userId}/messages/{id}", "delete");
        assert_eq!(delete["responses"]["200"], json!({"description": "Successful response"}));
        assert!(delete.get("requestBody").is_none());
        assert!(doc.pointer("/components/responses/Error/content/application~1json/schema").is_some());

        let schemas = &doc["components"]["schemas"];
        assert_eq!(
            schemas["Profile"],
            json!({"type": "object", "properties": {
                "emailAddress": {"type": "string"},
                "messagesTotal": {"type": "integer", "format": "int32"},
                // 64-bit integers are strings in Google's JSON.
                "historyId": {"type": "string", "format": "uint64"},
                "createTime": {"type": "string", "format": "date-time", "description": "Output only. When it was made.", "readOnly": true}
            }})
        );
        // `$ref`s point into `components`, keeping their description; recursion stays a reference.
        assert_eq!(
            schemas["Message"]["properties"]["payload"],
            json!({"$ref": "#/components/schemas/MessagePart", "description": "The parsed email structure."})
        );
        assert_eq!(
            schemas["MessagePart"]["properties"]["parts"]["items"],
            json!({"$ref": "#/components/schemas/MessagePart"})
        );
        assert_eq!(
            schemas["Content"]["properties"]["role"],
            json!({"type": "string", "enum": ["user", "model"], "x-enumDescriptions": ["The user.", "The model."]})
        );
        assert_eq!(
            schemas["GenerateRequest"]["properties"]["generationConfig"]["properties"]["temperature"],
            json!({"type": "number", "format": "float", "minimum": 0, "maximum": 2})
        );
        assert_eq!(
            schemas["GenerateRequest"]["properties"]["labels"]["additionalProperties"],
            json!({"type": "string"})
        );
        // `any` is any value.
        assert_eq!(schemas["Part"]["properties"]["data"], json!({"description": "Any JSON value."}));
        assert!(output_only("[Output Only] Creation timestamp."));
        assert!(output_only("  Output only. The name."));
        assert!(!output_only("The output only appears later."));
    }

    #[test]
    fn security_schemes_and_requirements() {
        let doc = to_openapi(&sample()).unwrap();
        assert_eq!(
            doc["components"]["securitySchemes"],
            json!({
                "oauth2": {"type": "oauth2", "description": "Google OAuth 2.0", "flows": {"authorizationCode": {
                    "authorizationUrl": "https://accounts.google.com/o/oauth2/v2/auth",
                    "tokenUrl": "https://oauth2.googleapis.com/token",
                    "scopes": {READONLY: "See your demo data", DEMO: "See and edit your demo data"}
                }}},
                "apiKey": {"type": "apiKey", "in": "query", "name": "key", "description": "API key for your project."}
            })
        );
        // OAuth first, with the scope most methods accept (not just the first one seen).
        assert_eq!(doc["security"], json!([{"oauth2": [DEMO]}, {"apiKey": []}]));
        // A method's scopes are alternatives; one without scopes takes the API key.
        let list = operation(&doc, "/users/{userId}/messages", "get");
        assert_eq!(list["security"], json!([{"oauth2": [READONLY]}, {"oauth2": [DEMO]}, {"apiKey": []}]));
        let generate = operation(&doc, "/v1/models/{modelsId}:generateContent", "post");
        assert_eq!(generate["security"], json!([{"apiKey": []}]));

        // Only an API key.
        let mut keyed = sample();
        keyed.as_object_mut().unwrap().remove("auth");
        let doc = to_openapi(&keyed).unwrap();
        assert_eq!(doc["security"], json!([{"apiKey": []}]));
        assert!(doc["components"]["securitySchemes"].get("oauth2").is_none());
        assert!(operation(&doc, "/users/{userId}/messages", "get").get("security").is_none());
        // Neither.
        keyed["parameters"].as_object_mut().unwrap().remove("key");
        let doc = to_openapi(&keyed).unwrap();
        assert!(doc.get("security").is_none());
        assert!(doc["components"].get("securitySchemes").is_none());
    }

    #[test]
    fn imports_end_to_end() {
        let c = import_openapi(&sample().to_string()).unwrap();
        assert_eq!(c.name, "Demo API");
        assert!(c.warnings.is_empty(), "{:?}", c.warnings);
        let folders: Vec<&str> = c
            .items
            .iter()
            .filter_map(|i| match i {
                ImportedItem::Folder { meta, .. } => Some(meta.name.as_str()),
                ImportedItem::Request(_) => None,
            })
            .collect();
        assert_eq!(folders, ["users", "users.messages", "models", "tunedModels", "operations"]);
        assert!(matches!(c.items.last(), Some(ImportedItem::Request(r)) if r.name == "ping"));
        assert_eq!(c.request_count(), 8);

        // Google's OAuth 2.0 authorization code flow with the scope most methods accept;
        // requests inherit it.
        let Auth::OAuth2(oauth) = &c.auth else { panic!("{:?}", c.auth) };
        assert_eq!(oauth.grant_type, GrantType::AuthorizationCode);
        assert_eq!(oauth.auth_url, AUTH_URL);
        assert_eq!(oauth.token_url, TOKEN_URL);
        assert_eq!(oauth.scope, DEMO);
        assert_eq!(oauth.client_id, "{{clientId}}");

        let list = find(&c, "messages.list");
        assert_eq!(list.method, "GET");
        assert_eq!(list.url, "{{baseUrl}}/users/{{userId}}/messages?view=basic");
        assert_eq!(list.auth, Auth::Inherit);
        assert_eq!(list.docs, "Lists the messages.");
        // A method that doesn't accept the collection's scope asks for one it accepts.
        let Auth::OAuth2(profile) = &find(&c, "users.getProfile").auth else { panic!("own OAuth 2.0") };
        assert_eq!(profile.scope, READONLY);
        let disabled: Vec<(&str, &str)> =
            list.disabled_params.iter().map(|p| (p.key.as_str(), p.value.as_str())).collect();
        assert_eq!(
            disabled,
            [
                ("maxResults", "100"),
                ("labelIds", ""),
                ("format", "minimal"),
                ("includeSpamTrash", "false"),
                ("oldFilter", "")
            ]
        );

        // The method without scopes takes the API key; its body leaves out output-only fields.
        let generate = find(&c, "models.generateContent");
        assert_eq!(generate.url, "{{baseUrl}}/v1/models/{{modelsId}}:generateContent");
        assert_eq!(
            generate.auth,
            Auth::ApiKey { key: "key".into(), value: "{{apiKey}}".into(), location: ApiKeyLocation::Query }
        );
        let body: Value = serde_json::from_str(&generate.body.text).unwrap();
        assert!(body["contents"][0]["parts"].is_array(), "{body}");
        let send: Value = serde_json::from_str(&find(&c, "messages.send").body.text).unwrap();
        assert!(send.get("id").is_none() && send.get("raw").is_some(), "{send}");
        assert!(find(&c, "messages.delete").docs.starts_with("Deprecated."));
        assert!(find(&c, "operations.get").url.starts_with("{{baseUrl}}/v1/{{"));

        let var = |key: &str| c.variables.iter().find(|v| v.key == key).map(|v| (v.value.as_str(), v.secret));
        assert_eq!(var("baseUrl"), Some(("https://demo.googleapis.com/demo/v1", false)));
        assert_eq!(var("userId"), Some(("me", false)));
        assert!(var("modelsId").is_some() && var("tunedModelsId").is_some());
        assert_eq!(var("apiKey"), Some(("", true)));
        assert_eq!(var("clientSecret"), Some(("", true)));
    }

    #[test]
    fn preferring_the_api_key_makes_every_request_use_it() {
        let mut doc = to_openapi(&sample()).unwrap();
        assert!(crate::openapi::prefer_scheme(&mut doc, "apiKey"));
        let c = import_openapi(&doc.to_string()).unwrap();
        assert_eq!(
            c.auth,
            Auth::ApiKey { key: "key".into(), value: "{{apiKey}}".into(), location: ApiKeyLocation::Query }
        );
        assert!(requests(&c.items).iter().all(|r| r.auth == Auth::Inherit));

        let mut keyless = sample();
        keyless["parameters"].as_object_mut().unwrap().remove("key");
        let mut doc = to_openapi(&keyless).unwrap();
        let before = doc.clone();
        assert!(!crate::openapi::prefer_scheme(&mut doc, "apiKey"));
        assert_eq!(doc, before);
    }

    #[test]
    fn mocks_and_spec_checks_work_on_discovery_documents() {
        let text = sample().to_string();
        let (routes, warnings) = crate::mock::routes_from_openapi(&text).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(routes.len(), 8);
        let list = routes.iter().find(|r| r.name == "messages.list").unwrap();
        assert_eq!((list.method.as_str(), list.path.as_str(), list.status), ("GET", "/users/:userId/messages", 200));
        let body: Value = serde_json::from_str(&list.body).unwrap();
        assert!(body["messages"][0]["payload"].is_object(), "{body}");

        // Responses are checked against the converted document.
        let doc = parse_document(&text).unwrap();
        assert_eq!(doc["openapi"], "3.0.3");
        let check = |status, body: &str| {
            crate::spec_check::check_response(
                &doc,
                "GET /users/{userId}/messages",
                status,
                Some("application/json"),
                body.as_bytes(),
            )
            .unwrap()
        };
        assert!(check(200, &list.body).passed());
        assert!(check(200, r#"{"messages": [{"id": "1", "internalDate": "1704110400000"}]}"#).passed());
        assert!(!check(200, r#"{"messages": {"id": 1}}"#).passed());
        assert!(check(403, r#"{"error": {"code": 403, "message": "No.", "status": "PERMISSION_DENIED"}}"#).passed());
        assert!(!check(403, r#"{"error": {"code": "403"}}"#).passed());
    }

    #[test]
    fn colliding_methods_stay_apart() {
        let method = |id: &str, http: &str| {
            json!({"id": id, "httpMethod": http, "path": "v1/{+name}", "flatPath": "v1/things/{thingsId}",
                   "parameters": {"name": {"type": "string", "location": "path", "required": true}}})
        };
        let doc = json!({
            "kind": "discovery#restDescription",
            "rootUrl": "https://x.googleapis.com/",
            "resources": {
                "a": {"methods": {"get": method("x.a.get", "GET"), "patch": method("x.a.patch", "PATCH")}},
                "b": {"methods": {"get": method("x.b.get", "GET")}},
                "c": {"methods": {"get": method("x.c.get", "GET"), "again": method("x.c.get", "GET")}}
            }
        });
        let openapi = to_openapi(&doc).unwrap();
        // The first takes `flatPath`, the next `path`, and the rest a fragment naming them.
        let at = |path: &str| openapi["paths"][path]["get"]["operationId"].as_str().unwrap_or_default().to_string();
        assert_eq!(at("/v1/things/{thingsId}"), "x.a.get");
        assert_eq!(at("/v1/{name}"), "x.b.get");
        assert_eq!(at("/v1/things/{thingsId}#x.c.get"), "x.c.get");
        assert_eq!(at("/v1/things/{thingsId}#x.c.get-2"), "x.c.get");
        assert_eq!(openapi["paths"]["/v1/things/{thingsId}"]["patch"]["operationId"], "x.a.patch");
        let c = import_openapi(&doc.to_string()).unwrap();
        assert_eq!(c.request_count(), 5);
        assert!(requests(&c.items).iter().any(|r| r.url == "{{baseUrl}}/v1/things/{{thingsId}}#x.c.get"));
    }

    #[test]
    fn many_resources_and_repeated_urls_convert_in_linear_time() {
        // Every method repeats one URL (each takes the next number) in a resource of its own.
        let n = 20_000;
        let resources: Map<String, Value> = (0..n)
            .map(|i| {
                let method =
                    json!({"id": "x.same", "httpMethod": "GET", "path": "v1/{+name}", "flatPath": "v1/things"});
                (format!("r{i}"), json!({"methods": {"get": method}}))
            })
            .collect();
        let doc = json!({"kind": "discovery#restDescription", "rootUrl": "https://x.googleapis.com/", "resources": resources});
        let start = std::time::Instant::now();
        let openapi = to_openapi(&doc).unwrap();
        assert!(start.elapsed().as_secs() < 10, "took {:?}", start.elapsed());
        let paths = openapi["paths"].as_object().unwrap();
        assert_eq!(paths.len(), n);
        assert!(paths.contains_key("/v1/things#x.same") && paths.contains_key(&format!("/v1/things#x.same-{}", n - 2)));
        assert_eq!(openapi["tags"].as_array().unwrap().len(), n);
    }

    #[test]
    fn document_text_cannot_reference_variables() {
        let mut doc = sample();
        doc["rootUrl"] = json!("https://{{githubToken}}.example.com/");
        doc["resources"]["users"]["resources"]["messages"]["methods"]["list"]["parameters"]["{{githubToken}}"] =
            json!({"type": "string", "location": "query", "required": true, "default": "x"});
        let openapi = to_openapi(&doc).unwrap();
        // No server (the importer asks for `baseUrl`), and no query parameter with braces.
        assert!(openapi.get("servers").is_none());
        let list = operation(&openapi, "/users/{userId}/messages", "get");
        assert!(list["parameters"].as_array().unwrap().iter().all(|p| !p["name"].as_str().unwrap().contains('{')));
        let c = import_openapi(&doc.to_string()).unwrap();
        for r in requests(&c.items) {
            assert!(!r.url.contains("githubToken"), "{}", r.url);
        }
        assert!(c.variables.iter().all(|v| !v.value.contains("{{")), "{:?}", c.variables);
    }

    #[test]
    fn recursive_and_looping_schemas_terminate() {
        let doc = json!({
            "kind": "discovery#restDescription",
            "rootUrl": "https://x.googleapis.com/",
            "methods": {
                "tree": {"id": "x.tree", "httpMethod": "POST", "path": "tree", "request": {"$ref": "Node"}, "response": {"$ref": "Node"}},
                "loop": {"id": "x.loop", "httpMethod": "GET", "path": "loop", "response": {"$ref": "Loop"}},
                "missing": {"id": "x.missing", "httpMethod": "GET", "path": "missing", "response": {"$ref": "Nowhere"}}
            },
            "schemas": {
                "Node": {"type": "object", "properties": {
                    "name": {"type": "string"},
                    "children": {"type": "array", "items": {"$ref": "Node"}},
                    "parent": {"$ref": "Node"},
                    "other": {"$ref": "Other"}
                }},
                "Other": {"type": "object", "properties": {"node": {"$ref": "Node"}}},
                "Loop": {"$ref": "Loop"},
                "Odd~/Name": {"type": "string"}
            }
        });
        let openapi = to_openapi(&doc).unwrap();
        assert_eq!(openapi["components"]["schemas"]["Loop"], json!({"$ref": "#/components/schemas/Loop"}));
        assert_eq!(schema(&json!({"$ref": "Odd~/Name"}), 0), json!({"$ref": "#/components/schemas/Odd~0~1Name"}));
        assert!(openapi.pointer("/components/schemas/Odd~0~1Name").is_some());
        let c = import_openapi(&doc.to_string()).unwrap();
        assert_eq!(c.request_count(), 3);
        let tree: Value = serde_json::from_str(&find(&c, "tree").body.text).unwrap();
        assert_eq!(tree["name"], "Jane Doe");
        let (routes, warnings) = crate::mock::routes_from_openapi(&doc.to_string()).unwrap();
        assert_eq!(routes.len(), 3);
        assert!(warnings.iter().any(|w| w.contains("loops")), "{warnings:?}");
    }

    #[test]
    fn malformed_and_deep_documents_do_not_panic() {
        // Resources and inline schemas nested past the limit are cut off (JSON text can't nest this
        // deep, but a converted value can come from anywhere).
        let mut resource = json!({"methods": {"get": {"httpMethod": "GET", "path": "deep"}}});
        let mut inline = json!({"type": "string"});
        for _ in 0..200 {
            resource = json!({"resources": {"r": resource}});
            inline = json!({"type": "object", "properties": {"p": inline}});
        }
        let doc = json!({
            "kind": "discovery#restDescription",
            "parameters": "nope",
            "auth": {"oauth2": {"scopes": {"s": "not an object"}}},
            "methods": {
                "noHttp": {"path": "a"},
                "noPath": {"httpMethod": "GET"},
                "odd": {"httpMethod": "FETCH", "path": "b"},
                "weird": {"httpMethod": "get", "path": 7, "flatPath": "c/{}/{x", "parameters": "p", "scopes": {"a": 1},
                          "request": "Body", "response": {"$ref": 5}},
                "notAnObject": 5
            },
            "resources": {"deep": resource, "list": [1, 2]},
            "schemas": {"Inline": inline, "Bad": 5, "Typed": {"type": ["string"], "properties": [], "items": 3,
                        "enum": "x", "default": {"a": 1}, "minimum": "low"}}
        });
        let openapi = to_openapi(&doc).unwrap();
        assert_eq!(openapi["paths"].as_object().unwrap().len(), 1);
        assert_eq!(openapi["paths"]["/c/{}/{x"]["get"]["parameters"], Value::Null);
        assert!(openapi["components"]["schemas"].get("Bad").is_none());
        // Unknown types are any value; a default that isn't text is kept as given.
        assert_eq!(openapi["components"]["schemas"]["Typed"], json!({"default": {"a": 1}}));
        let mut depth = 0;
        let mut at = &openapi["components"]["schemas"]["Inline"];
        while let Some(next) = at.get("properties").and_then(|p| p.get("p")) {
            at = next;
            depth += 1;
        }
        assert_eq!(depth, MAX_DEPTH);
        let c = import_openapi(&serde_json::to_string(&doc).unwrap());
        // Too deep for the JSON parser, which refuses it instead of overflowing.
        assert!(c.is_err());
    }

    /// Imports every `*.json` Discovery document in `ZORVIK_DISCOVERY_DIR` and prints what came
    /// out: `ZORVIK_DISCOVERY_DIR=/path cargo test -p zorvik-formats discovery -- --ignored --nocapture`.
    #[test]
    #[ignore = "needs downloaded Discovery documents"]
    fn real_documents_import() {
        fn methods(node: &Value) -> usize {
            let own = node.get("methods").and_then(Value::as_object).map_or(0, |m| m.len());
            own + node
                .get("resources")
                .and_then(Value::as_object)
                .into_iter()
                .flatten()
                .map(|(_, r)| methods(r))
                .sum::<usize>()
        }
        let dir = std::env::var("ZORVIK_DISCOVERY_DIR").expect("set ZORVIK_DISCOVERY_DIR");
        let mut files: Vec<_> = std::fs::read_dir(dir).unwrap().filter_map(|e| e.ok().map(|e| e.path())).collect();
        files.retain(|p| p.extension().is_some_and(|e| e == "json"));
        files.sort();
        assert!(!files.is_empty(), "no .json files");
        for file in files {
            let text = std::fs::read_to_string(&file).unwrap();
            let source: Value = serde_json::from_str(&text).unwrap();
            let started = std::time::Instant::now();
            let c = import_openapi(&text).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
            let took = started.elapsed();
            let (routes, mock_warnings) = crate::mock::routes_from_openapi(&text).unwrap();
            let folders = c.items.iter().filter(|i| matches!(i, ImportedItem::Folder { .. })).count();
            let expected = methods(&source);
            println!(
                "{} | {} | methods {expected} | requests {} | folders {folders} | routes {} | {took:.1?} | auth {} | warnings {:?} | mock warnings {}",
                file.file_name().unwrap().to_string_lossy(),
                c.name,
                c.request_count(),
                routes.len(),
                match &c.auth {
                    Auth::OAuth2(o) => format!("oauth2 ({} scopes)", o.scope.split(' ').count()),
                    other => format!("{other:?}"),
                },
                c.warnings,
                mock_warnings.len(),
            );
            assert_eq!(c.request_count(), expected, "{}", file.display());
            assert_eq!(routes.len(), expected, "{}", file.display());
            for name in ["messages.list", "models.generateContent", "files.list", "objects.list", "instances.list"] {
                if let Some(r) = requests(&c.items).into_iter().find(|r| r.name == name) {
                    println!("    {name}: {} {} auth={:?}", r.method, r.url, r.auth);
                }
            }
        }
    }
}

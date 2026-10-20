//! Postman Collection v1 (`requests`, `folders`, `order`), the format Postman exported
//! before 2017. It is converted to v2.1 JSON the way Postman's own converter
//! (postman-collection-transformer) does it, then imported like any v2.1 collection.

use std::collections::{HashMap, HashSet};

use serde_json::{Map, Value, json};

/// Folders nest at most this deep (a v1 file can list a folder inside itself).
const MAX_DEPTH: usize = 64;

/// A v1 collection: `requests` and `order` instead of `item`.
pub(super) fn is_v1(obj: &Map<String, Value>) -> bool {
    !obj.contains_key("item") && ["requests", "order"].iter().any(|k| obj.get(*k).is_some_and(Value::is_array))
}

/// The v1 collection as v2.1 JSON.
pub(super) fn to_v2(c: &Map<String, Value>) -> Value {
    let requests: Vec<&Map<String, Value>> = list(c.get("requests")).filter_map(Value::as_object).collect();
    let folders: Vec<&Map<String, Value>> = list(c.get("folders")).filter_map(Value::as_object).collect();
    let mut cx = Converter {
        requests: requests.iter().filter_map(|r| Some((id(r)?, *r))).collect(),
        folders: folders.iter().filter_map(|f| Some((id(f)?, *f))).collect(),
        used_requests: HashSet::new(),
        used_folders: HashSet::new(),
    };
    // Folders listed in another folder's `folders_order` are subfolders, not top-level ones.
    let nested: HashSet<String> = folders
        .iter()
        .flat_map(|f| ids(f.get("folders_order")).into_iter().filter(move |sub| id(f).as_ref() != Some(sub)))
        .collect();
    let top: Vec<String> = match c.get("folders_order").and_then(Value::as_array) {
        Some(order) => order.iter().filter_map(Value::as_str).map(String::from).collect(),
        None => folders.iter().filter_map(|f| id(f)).collect(),
    };
    let mut items: Vec<Value> = Vec::new();
    for f in top.iter().filter(|f| !nested.contains(*f)) {
        items.extend(cx.folder(f, 0));
    }
    // Folders nothing reaches (unlisted, or only in a loop of folders) are kept at the top level.
    for f in folders.iter().filter_map(|f| id(f)) {
        items.extend(cx.folder(&f, 0));
    }
    for request in ids(c.get("order")) {
        if let Some(item) = cx.request(&request) {
            items.push(item);
        }
    }
    // Requests no folder or order lists (or without an id to list them by): kept at the end
    // rather than lost.
    for r in &requests {
        if id(r).is_none_or(|rid| cx.used_requests.insert(rid)) {
            items.push(item(r));
        }
    }

    let mut info = json!({
        "name": c.get("name").cloned().unwrap_or(Value::Null),
        "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json",
    });
    if let Some(description) = c.get("description").filter(|d| !is_empty(d)) {
        info["description"] = description.clone();
    }
    let mut out = json!({ "info": info, "item": items });
    copy_common(c, &mut out);
    out
}

struct Converter<'a> {
    requests: HashMap<String, &'a Map<String, Value>>,
    folders: HashMap<String, &'a Map<String, Value>>,
    used_requests: HashSet<String>,
    used_folders: HashSet<String>,
}

impl Converter<'_> {
    /// A folder with its subfolders first, then its requests (Postman's order).
    fn folder(&mut self, folder_id: &str, depth: usize) -> Option<Value> {
        if depth > MAX_DEPTH || !self.used_folders.insert(folder_id.to_string()) {
            return None;
        }
        let f = *self.folders.get(folder_id)?;
        let mut children: Vec<Value> =
            ids(f.get("folders_order")).iter().filter_map(|sub| self.folder(sub, depth + 1)).collect();
        for request in ids(f.get("order")) {
            if let Some(item) = self.request(&request) {
                children.push(item);
            }
        }
        let mut out = json!({ "name": f.get("name").cloned().unwrap_or(Value::Null), "item": children });
        if let Some(description) = f.get("description").filter(|d| !is_empty(d)) {
            out["description"] = description.clone();
        }
        copy_common(f, &mut out);
        Some(out)
    }

    fn request(&mut self, request_id: &str) -> Option<Value> {
        let r = *self.requests.get(request_id)?;
        self.used_requests.insert(request_id.to_string()).then(|| item(r))
    }
}

/// Auth, scripts, variables and protocol settings of a collection or folder.
fn copy_common(from: &Map<String, Value>, to: &mut Value) {
    if let Some(auth) = auth(from) {
        to["auth"] = auth;
    }
    if let Some(events) = events(from) {
        to["event"] = events;
    }
    let variables = from.get("variables").or_else(|| from.get("variable"));
    if let Some(vars) = variables.and_then(Value::as_array).filter(|v| !v.is_empty()) {
        to["variable"] = Value::Array(vars.clone());
    }
    if let Some(behavior) = from.get("protocolProfileBehavior") {
        to["protocolProfileBehavior"] = behavior.clone();
    }
}

/// A v1 request as a v2.1 item.
fn item(r: &Map<String, Value>) -> Value {
    let mut request = json!({
        "method": r.get("method").cloned().unwrap_or_else(|| "GET".into()),
        "url": url(r),
        "header": header(r),
    });
    if let Some(body) = body(r) {
        request["body"] = body;
    }
    if let Some(auth) = auth(r) {
        request["auth"] = auth;
    }
    if let Some(description) = r.get("description").filter(|d| !is_empty(d)) {
        request["description"] = description.clone();
    }
    let mut out = json!({ "name": r.get("name").cloned().unwrap_or(Value::Null), "request": request });
    if let Some(events) = events(r) {
        out["event"] = events;
    }
    let responses = responses(r);
    if !responses.is_empty() {
        out["response"] = Value::Array(responses);
    }
    if let Some(behavior) = r.get("protocolProfileBehavior") {
        out["protocolProfileBehavior"] = behavior.clone();
    }
    out
}

/// The URL string, as an object when there are query parameters (disabled ones are only
/// there) or path variables.
fn url(r: &Map<String, Value>) -> Value {
    let raw = r.get("url").map(text).unwrap_or_default();
    let query: Vec<Value> = list(r.get("queryParams"))
        .filter_map(Value::as_object)
        .map(|q| {
            let mut q = q.clone();
            if q.remove("enabled") == Some(Value::Bool(false)) {
                q.insert("disabled".into(), Value::Bool(true));
            }
            // `equals: false` is `?key` without a value.
            if q.remove("equals") == Some(Value::Bool(false)) && q.get("value").is_none_or(is_empty) {
                q.insert("value".into(), Value::Null);
            }
            Value::Object(q)
        })
        .collect();
    let mut variables: Vec<Value> = list(r.get("pathVariableData")).cloned().collect();
    if let Some(Value::Object(vars)) = r.get("pathVariables") {
        for (key, value) in vars {
            if !variables.iter().any(|v| v.get("key").and_then(Value::as_str) == Some(key)) {
                variables.push(json!({ "key": key, "value": value }));
            }
        }
    }
    if query.is_empty() && variables.is_empty() {
        return Value::String(raw);
    }
    json!({ "raw": raw, "query": query, "variable": variables })
}

/// `headerData` (a list) or `headers` (the `Name: value` lines v2 also accepts).
fn header(r: &Map<String, Value>) -> Value {
    if let Some(data) = r.get("headerData").and_then(Value::as_array).filter(|d| !d.is_empty()) {
        let headers = data
            .iter()
            .filter_map(Value::as_object)
            .map(|h| {
                let mut h = h.clone();
                let key = h.get("key").map(text).unwrap_or_default();
                let commented = key.trim_start().starts_with("//");
                if h.remove("enabled") == Some(Value::Bool(false)) || commented {
                    h.insert("disabled".into(), Value::Bool(true));
                }
                if commented {
                    h.insert("key".into(), Value::String(key.trim_start()[2..].trim().to_string()));
                }
                Value::Object(h)
            })
            .collect();
        return Value::Array(headers);
    }
    r.get("headers").cloned().unwrap_or(Value::Null)
}

fn body(r: &Map<String, Value>) -> Option<Value> {
    let raw_data = match r.get("rawModeData") {
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Array(a)) if a.len() == 1 && a[0].is_string() => a[0].as_str().map(String::from),
        _ => None,
    };
    let graphql = r.get("graphqlModeData").filter(|g| !is_empty(g)).cloned();
    let data = r.get("data");
    let mode = match r.get("dataMode") {
        Some(Value::Null) => return None,
        Some(Value::String(m)) => match m.as_str() {
            "params" => "formdata",
            "urlencoded" => "urlencoded",
            "raw" => "raw",
            "binary" => "file",
            "graphql" => "graphql",
            _ => "raw",
        },
        _ if data.is_none_or(is_empty) && raw_data.as_deref().is_none_or(str::is_empty) && graphql.is_none() => {
            return None;
        }
        _ if raw_data.as_deref().is_none_or(str::is_empty) && data.is_some_and(Value::is_array) => "formdata",
        _ if raw_data.as_deref().is_none_or(str::is_empty) && graphql.is_some() => "graphql",
        _ => "raw",
    };
    let mut body = json!({ "mode": mode });
    match mode {
        "raw" => {
            let text = raw_data.filter(|s| !s.is_empty()).or_else(|| data.and_then(Value::as_str).map(String::from));
            body["raw"] = Value::String(text.unwrap_or_default());
        }
        "graphql" => body["graphql"] = graphql.or_else(|| data.cloned()).unwrap_or(Value::Null),
        "file" => body["file"] = json!({ "src": raw_data.unwrap_or_default() }),
        _ => body[mode] = Value::Array(form_fields(data)),
    }
    if let Some(Value::Object(options)) = r.get("dataOptions") {
        let renamed: Map<String, Value> = options
            .iter()
            .filter(|(_, v)| !is_empty(v))
            .map(|(k, v)| {
                let key = match k.as_str() {
                    "params" => "formdata",
                    "binary" => "file",
                    other => other,
                };
                (key.to_string(), v.clone())
            })
            .collect();
        if !renamed.is_empty() {
            body["options"] = Value::Object(renamed);
        }
    }
    if r.get("dataDisabled") == Some(&Value::Bool(true)) {
        body["disabled"] = Value::Bool(true);
    }
    Some(body)
}

/// Form fields: `enabled: false` becomes `disabled`, a file field's path is its `src`.
fn form_fields(data: Option<&Value>) -> Vec<Value> {
    list(data)
        .filter_map(Value::as_object)
        .filter(|f| f.get("key").is_some_and(|k| !k.is_null()))
        .map(|f| {
            let mut f = f.clone();
            if f.get("type").and_then(Value::as_str) == Some("file")
                && f.get("src").is_none()
                && let Some(value) = f.remove("value")
            {
                f.insert("src".into(), value);
            }
            if f.remove("enabled") == Some(Value::Bool(false)) {
                f.insert("disabled".into(), Value::Bool(true));
            }
            Value::Object(f)
        })
        .collect()
}

/// v2-style `auth`, or the old `currentHelper` + `helperAttributes`.
fn auth(e: &Map<String, Value>) -> Option<Value> {
    let legacy = e.contains_key("currentHelper") || e.get("helperAttributes").is_some_and(|h| !h.is_null());
    if !legacy {
        return e.get("auth").filter(|a| a.is_object()).cloned();
    }
    let helper = e.get("currentHelper").and_then(Value::as_str).unwrap_or("normal");
    let kind = match helper {
        "apikeyAuth" => "apikey",
        "basicAuth" => "basic",
        "bearerAuth" => "bearer",
        "digestAuth" => "digest",
        "hawkAuth" => "hawk",
        "oAuth1" => "oauth1",
        "oAuth2" => "oauth2",
        "ntlmAuth" => "ntlm",
        "awsSigV4" => "awsv4",
        "jwtAuth" => "jwt",
        "asapAuth" => "asap",
        "edgegridAuth" => "edgegrid",
        "normal" | "" => return None,
        other => other,
    };
    let attributes = match e.get("helperAttributes") {
        Some(Value::String(s)) => serde_json::from_str(s).ok()?,
        Some(Value::Object(o)) => o.clone(),
        _ => Map::new(),
    };
    // The few helpers whose attributes have other names than the v2 parameters.
    let renames: &[(&str, &str)] = match kind {
        "basic" => &[("saveToRequest", "saveHelperData")],
        "oauth1" => {
            &[("header", "addParamsToHeader"), ("auto", "autoAddParam"), ("includeEmpty", "addEmptyParamsToSign")]
        }
        "hawk" => &[
            ("hawk_id", "authId"),
            ("hawk_key", "authKey"),
            ("ext", "extraData"),
            ("app", "appId"),
            ("dlg", "delegation"),
            ("saveToRequest", "saveHelperData"),
        ],
        "oauth2" => {
            &[("grantType", "grant_type"), ("redirectUri", "redirect_uri"), ("clientAuth", "client_authentication")]
        }
        _ => &[],
    };
    let params: Map<String, Value> = attributes
        .into_iter()
        .filter(|(k, _)| k != "id")
        .map(|(k, v)| (renames.iter().find(|(old, _)| *old == k).map_or(k, |(_, new)| new.to_string()), v))
        .collect();
    Some(json!({ "type": kind, kind: params }))
}

/// `events`, or the old `preRequestScript` and `tests` text.
fn events(e: &Map<String, Value>) -> Option<Value> {
    let script = |key: &str| e.get(key).filter(|s| !is_empty(s));
    let (pre, tests) = (script("preRequestScript"), script("tests"));
    if pre.is_none() && tests.is_none() {
        return e.get("events").and_then(Value::as_array).filter(|v| !v.is_empty()).cloned().map(Value::Array);
    }
    let mut events = Vec::new();
    if let Some(code) = tests {
        events.push(json!({ "listen": "test", "script": { "type": "text/javascript", "exec": code } }));
    }
    if let Some(code) = pre {
        events.push(json!({ "listen": "prerequest", "script": { "type": "text/javascript", "exec": code } }));
    }
    Some(Value::Array(events))
}

/// Saved responses (examples), in `responses_order` when given. Each is taken once: an order
/// naming one large response over and over must not copy it each time.
fn responses(r: &Map<String, Value>) -> Vec<Value> {
    let all: Vec<&Map<String, Value>> = list(r.get("responses")).filter_map(Value::as_object).collect();
    let order = ids(r.get("responses_order"));
    let ordered: Vec<&Map<String, Value>> = if order.is_empty() {
        all
    } else {
        let mut by_id: HashMap<String, &Map<String, Value>> = HashMap::new();
        for x in &all {
            if let Some(xid) = id(x) {
                by_id.entry(xid).or_insert(x);
            }
        }
        order.iter().filter_map(|o| by_id.remove(o)).collect()
    };
    ordered
        .into_iter()
        .map(|x| {
            let code = x.get("responseCode").and_then(|c| c.get("code"));
            json!({
                "name": x.get("name").cloned().unwrap_or_else(|| "response".into()),
                "code": code.and_then(|c| c.as_u64().or_else(|| text(c).parse().ok())),
                "status": x.get("responseCode").and_then(|c| c.get("name")).cloned(),
                "header": x.get("headers").cloned().unwrap_or(Value::Null),
                "body": x.get("text").cloned().unwrap_or(Value::Null),
            })
        })
        .collect()
}

fn id(o: &Map<String, Value>) -> Option<String> {
    o.get("id").map(text).filter(|s| !s.is_empty())
}

fn ids(v: Option<&Value>) -> Vec<String> {
    list(v).map(text).filter(|s| !s.is_empty()).collect()
}

fn list(v: Option<&Value>) -> impl Iterator<Item = &Value> {
    v.and_then(Value::as_array).into_iter().flatten()
}

fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// Null, `""`, `[]` or `{}`.
fn is_empty(v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::String(s) => s.trim().is_empty(),
        Value::Array(a) => a.is_empty(),
        Value::Object(o) => o.is_empty(),
        _ => false,
    }
}

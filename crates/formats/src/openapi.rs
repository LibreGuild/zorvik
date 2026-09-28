//! OpenAPI 3.0/3.1 and Swagger 2.0 importer. Works on an untyped JSON tree so partially
//! invalid documents still import; parts that cannot be mapped are skipped with a warning.

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::fmt::{self, Write as _};

use indexmap::IndexMap;
use serde::de::{self, DeserializeSeed, Deserializer, EnumAccess, MapAccess, SeqAccess, VariantAccess, Visitor};
use serde_json::{Map, Value};

use crate::import::{ImportError, ImportedCollection, ImportedItem};
use crate::model::{
    ApiKeyLocation, Auth, Body, BodyType, FolderMeta, GrantType, KeyValue, MultipartField, OAuth2Config,
    OpenApiOperation, Request, RequestKind, Variable,
};

const METHODS: [&str; 8] = ["get", "put", "post", "delete", "options", "head", "patch", "trace"];
/// Nesting limit for generated examples.
const MAX_DEPTH: usize = 6;
/// Upper bound on generated object properties per example.
const MAX_PROPS: usize = 500;
/// Longest `$ref` chain followed before assuming a loop.
const MAX_REF_HOPS: usize = 32;
/// Deepest schema nesting walked for one example, counting `allOf`/`oneOf` levels too.
const MAX_NESTING: usize = 64;
/// Most schemas visited for one example: `allOf`/`oneOf` can fan out exponentially.
const MAX_VISITS: usize = 10_000;
/// Rough limit, in bytes, on everything one import copies or generates. `$ref`s let a small
/// document repeat one large schema, example or description in every operation.
const MAX_OUTPUT: usize = 256 << 20;
/// What each schema visit costs against `MAX_OUTPUT`, so visits that produce nothing still count.
const VISIT_COST: usize = 16;
/// Values YAML aliases may add on top of the document's own.
const MAX_YAML_ALIAS_VALUES: usize = 1_000_000;
/// Most items generated for one array; a larger `minItems` is not honoured.
const MAX_ITEMS: u64 = 100;
/// Longest string padded to reach a schema's `minLength`.
const MAX_PADDING: u64 = 1024;
/// Example values shared by formats and names.
const EXAMPLE_UUID: &str = "3fa85f64-5717-4562-b3fc-2c963f66afa6";
const EXAMPLE_TIMESTAMP: &str = "2024-01-01T12:00:00Z";
const EXAMPLE_IP: &str = "203.0.113.10";
const EXAMPLE_SECRET: &str = "secret-value";

/// Import an OpenAPI 3.0/3.1 or Swagger 2.0 document (JSON or YAML).
pub fn import_openapi(text: &str) -> Result<ImportedCollection, ImportError> {
    let doc = parse(text)?;
    let swagger2 = is_swagger2(&doc)?;
    Ok(Importer::new(&doc, swagger2).run())
}

/// An OpenAPI 3 / Swagger 2 document (JSON or YAML) as JSON, checked to be one.
pub fn parse_document(text: &str) -> Result<Value, ImportError> {
    let doc = parse(text)?;
    is_swagger2(&doc)?;
    Ok(doc)
}

/// `false` for OpenAPI 3.x, `true` for Swagger 2.0.
fn is_swagger2(doc: &Value) -> Result<bool, ImportError> {
    // Unquoted YAML versions (`swagger: 2.0`) parse as numbers.
    let version = |key| match doc.get(key) {
        Some(Value::String(s)) => s.trim().to_string(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    };
    if version("openapi").starts_with('3') {
        Ok(false)
    } else if version("swagger").starts_with('2') {
        Ok(true)
    } else {
        Err(ImportError::new("Not an OpenAPI/Swagger document"))
    }
}

/// The canned answer of one operation, for mock servers (see `mock.rs`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MockResponse {
    pub method: String,
    /// Path with `:name` parameters.
    pub path: String,
    pub name: String,
    pub status: u16,
    pub content_type: Option<String>,
    pub body: String,
}

/// Every operation's first 2xx response (example, or one generated from its
/// schema), plus warnings. Bounded like the importer.
pub(crate) fn mock_responses(text: &str) -> Result<(Vec<MockResponse>, Vec<String>), ImportError> {
    let doc = parse(text)?;
    let swagger2 = is_swagger2(&doc)?;
    Ok(Importer::new(&doc, swagger2).mock_responses())
}

fn parse(text: &str) -> Result<Value, ImportError> {
    let text = text.trim_start_matches('\u{feff}');
    if text.trim().is_empty() {
        return Err(ImportError::new("The document is empty"));
    }
    let json_err = match serde_json::from_str(text) {
        Ok(v) => return Ok(v),
        Err(e) => e,
    };
    // Aliases are expanded while parsing, so a few kilobytes of YAML can stand for billions of
    // values. Alias-free YAML never has more values than bytes.
    let left = Cell::new(text.len() + MAX_YAML_ALIAS_VALUES);
    match YamlSeed(&left).deserialize(serde_yaml_ng::Deserializer::from_str(text)) {
        Ok(mut yaml) => {
            // Expand `<<` merge keys; a malformed merge is left as written.
            let _ = yaml.apply_merge();
            Ok(yaml_to_json(yaml))
        }
        Err(_) if text.trim_start().starts_with(['{', '[']) => {
            Err(ImportError::new(format!("Invalid JSON: {json_err}")))
        }
        Err(e) => Err(ImportError::new(format!("Invalid YAML: {e}"))),
    }
}

fn yaml_to_json(v: serde_yaml_ng::Value) -> Value {
    use serde_yaml_ng::Value as Y;
    match v {
        Y::Null => Value::Null,
        Y::Bool(b) => Value::Bool(b),
        Y::Number(n) => match (n.as_i64(), n.as_u64()) {
            (Some(i), _) => i.into(),
            (_, Some(u)) => u.into(),
            _ => n.as_f64().and_then(serde_json::Number::from_f64).map_or(Value::Null, Value::Number),
        },
        Y::String(s) => Value::String(s),
        Y::Sequence(items) => Value::Array(items.into_iter().map(yaml_to_json).collect()),
        // Non-string keys (e.g. `200:` response codes) become their text form.
        Y::Mapping(m) => Value::Object(
            m.into_iter()
                .map(|(k, v)| {
                    let key = match yaml_to_json(k) {
                        Value::String(s) => s,
                        other => other.to_string(),
                    };
                    (key, yaml_to_json(v))
                })
                .collect(),
        ),
        Y::Tagged(t) => yaml_to_json(t.value),
    }
}

/// Deserializes a `serde_yaml_ng::Value`, failing once the shared counter of values runs out.
#[derive(Clone, Copy)]
struct YamlSeed<'c>(&'c Cell<usize>);

impl YamlSeed<'_> {
    fn count<E: de::Error>(self) -> Result<(), E> {
        let left = self.0.get().checked_sub(1).ok_or_else(|| E::custom("aliases expand to too many values"))?;
        self.0.set(left);
        Ok(())
    }

    fn leaf<E: de::Error>(self, v: serde_yaml_ng::Value) -> Result<serde_yaml_ng::Value, E> {
        self.count()?;
        Ok(v)
    }
}

impl<'de> DeserializeSeed<'de> for YamlSeed<'_> {
    type Value = serde_yaml_ng::Value;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for YamlSeed<'_> {
    type Value = serde_yaml_ng::Value;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("any YAML value")
    }

    fn visit_bool<E: de::Error>(self, v: bool) -> Result<Self::Value, E> {
        self.leaf(v.into())
    }

    fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
        self.leaf(v.into())
    }

    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
        self.leaf(v.into())
    }

    // Integers beyond 64 bits (which `serde_yaml_ng::Value` rejects) become floats, as in JSON.
    fn visit_i128<E: de::Error>(self, v: i128) -> Result<Self::Value, E> {
        self.leaf((v as f64).into())
    }

    fn visit_u128<E: de::Error>(self, v: u128) -> Result<Self::Value, E> {
        self.leaf((v as f64).into())
    }

    fn visit_f64<E: de::Error>(self, v: f64) -> Result<Self::Value, E> {
        self.leaf(v.into())
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
        self.leaf(v.into())
    }

    fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
        self.leaf(v.into())
    }

    fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
        self.leaf(serde_yaml_ng::Value::Null)
    }

    fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
        self.leaf(serde_yaml_ng::Value::Null)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        self.count()?;
        let mut items = Vec::new();
        while let Some(item) = seq.next_element_seed(self)? {
            items.push(item);
        }
        Ok(serde_yaml_ng::Value::Sequence(items))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        self.count()?;
        let mut mapping = serde_yaml_ng::Mapping::new();
        while let Some(key) = map.next_key_seed(self)? {
            if mapping.contains_key(&key) {
                return Err(de::Error::custom(match key.as_str() {
                    Some(key) => format!("duplicate entry with key {key:?}"),
                    None => "duplicate entry in YAML map".to_string(),
                }));
            }
            let value = map.next_value_seed(self)?;
            mapping.insert(key, value);
        }
        Ok(serde_yaml_ng::Value::Mapping(mapping))
    }

    /// A tagged value (`!Tag value`); tags mean nothing to OpenAPI, so only the value is kept.
    fn visit_enum<A: EnumAccess<'de>>(self, data: A) -> Result<Self::Value, A::Error> {
        let (_, value) = data.variant::<de::IgnoredAny>()?;
        value.newtype_variant_seed(self)
    }
}

/// Parameters by `name` and `in`, in declaration order.
type Params<'a> = IndexMap<(&'a str, &'a str), &'a Value>;

/// A form or multipart field derived from a schema or Swagger 2 `formData` parameters.
struct Field {
    name: String,
    value: String,
    description: String,
    file: bool,
    content_type: Option<String>,
}

struct Importer<'a> {
    doc: &'a Value,
    swagger2: bool,
    base_url: String,
    warnings: Vec<String>,
    warned: HashSet<String>,
    /// Placeholder variables referenced by imported auth (name -> secret).
    vars: IndexMap<&'static str, bool>,
    /// Variables standing for path parameters (name -> example value); the first value wins.
    path_vars: IndexMap<String, String>,
    /// Mapped auth per security scheme name (`None` = unsupported).
    schemes: HashMap<String, Option<Auth>>,
    /// Schemas on the current example-generation path (cycle guard).
    active: Vec<&'a Value>,
    /// Properties left to generate for the current example.
    budget: usize,
    /// Schemas left to visit for the current example.
    visits: usize,
    /// Output left for the rest of the import (see `MAX_OUTPUT`).
    output_left: usize,
    /// Generating response examples (read-only properties in, write-only ones out).
    for_response: bool,
}

impl<'a> Importer<'a> {
    fn new(doc: &'a Value, swagger2: bool) -> Self {
        Self {
            doc,
            swagger2,
            base_url: String::new(),
            warnings: Vec::new(),
            warned: HashSet::new(),
            vars: IndexMap::new(),
            path_vars: IndexMap::new(),
            schemes: HashMap::new(),
            active: Vec::new(),
            budget: 0,
            visits: 0,
            output_left: MAX_OUTPUT,
            for_response: false,
        }
    }

    fn run(mut self) -> ImportedCollection {
        let doc = self.doc;
        let name = doc
            .pointer("/info/title")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("OpenAPI import")
            .to_string();
        self.base_url = self.base_url();
        let auth = match doc.get("security").and_then(Value::as_array) {
            Some(reqs) => self.requirements_auth(reqs).unwrap_or(Auth::None),
            None => Auth::None,
        };

        // Folders follow the top-level `tags` order, then first appearance.
        let mut folders: IndexMap<String, (String, Vec<Request>)> = IndexMap::new();
        for tag in doc.get("tags").and_then(Value::as_array).into_iter().flatten() {
            // Trimmed like the operations' tags (`first_tag`), so they end up in the same folder.
            if let Some(name) = tag.get("name").and_then(Value::as_str).map(str::trim) {
                let docs = str_of(tag, "description").trim().to_string();
                folders.entry(name.to_string()).or_insert((docs, Vec::new()));
            }
        }
        let mut root = Vec::new();
        let mut skipped = 0;
        for (path, item) in doc.get("paths").and_then(Value::as_object).into_iter().flatten() {
            if path.starts_with("x-") {
                continue;
            }
            let Some(item) = self.resolve(item) else { continue };
            let Some(ops) = item.as_object() else { continue };
            for (method, op) in ops {
                let method = method.to_ascii_lowercase();
                if !METHODS.contains(&method.as_str()) || !op.is_object() {
                    continue;
                }
                if self.output_left == 0 {
                    skipped += 1;
                    continue;
                }
                let before = self.output_left;
                let request = self.request(path, &method, item, op, &auth);
                // Examples were charged while they were built; charge whatever else the request holds.
                let size =
                    serde_json::to_vec(&request).map_or(0, |json| json.len()) + first_tag(op).map_or(0, str::len);
                self.afford(size.saturating_sub(before - self.output_left));
                match first_tag(op) {
                    Some(tag) => folders.entry(tag.to_string()).or_default().1.push(request),
                    None => root.push(request),
                }
            }
        }

        let mut items = Vec::new();
        for (name, (docs, requests)) in folders {
            if requests.is_empty() {
                continue;
            }
            let children = requests
                .into_iter()
                .enumerate()
                .map(|(i, mut r)| {
                    r.seq = i as u32;
                    ImportedItem::Request(r)
                })
                .collect();
            let meta = FolderMeta { name, seq: items.len() as u32, docs, ..FolderMeta::default() };
            items.push(ImportedItem::Folder { meta, children });
        }
        for mut r in root {
            r.seq = items.len() as u32;
            items.push(ImportedItem::Request(r));
        }
        if items.is_empty() {
            self.warn("The document contains no operations".to_string());
        }
        if self.output_left == 0 {
            self.warnings.push(format!(
                "The document expands to more than {} MB of requests ($refs repeat the same content); \
                 examples were cut short and {skipped} operations were skipped",
                MAX_OUTPUT >> 20
            ));
        }

        let mut variables =
            vec![Variable { key: "baseUrl".to_string(), value: self.base_url.clone(), enabled: true, secret: false }];
        variables.extend(self.vars.iter().map(|(key, secret)| Variable {
            key: key.to_string(),
            value: String::new(),
            enabled: true,
            secret: *secret,
        }));
        // A path variable named like `baseUrl` or an auth placeholder shares that variable.
        let taken = variables.len();
        for (key, value) in self.path_vars {
            if !variables[..taken].iter().any(|v| v.key == key) {
                variables.push(Variable { key, value, enabled: true, secret: false });
            }
        }
        ImportedCollection {
            name,
            variables,
            auth,
            headers: Vec::new(),
            items,
            warnings: self.warnings,
            ..Default::default()
        }
    }

    /// Every operation's canned answer, in document order.
    fn mock_responses(mut self) -> (Vec<MockResponse>, Vec<String>) {
        let doc = self.doc;
        self.for_response = true;
        let mut out = Vec::new();
        let mut skipped = 0;
        for (path, item) in doc.get("paths").and_then(Value::as_object).into_iter().flatten() {
            if path.starts_with("x-") {
                continue;
            }
            let Some(item) = self.resolve(item) else { continue };
            let Some(ops) = item.as_object() else { continue };
            for (method, op) in ops {
                let method = method.to_ascii_uppercase();
                if !METHODS.contains(&method.to_ascii_lowercase().as_str()) || !op.is_object() {
                    continue;
                }
                if self.output_left == 0 {
                    skipped += 1;
                    continue;
                }
                let before = self.output_left;
                let label = format!("{method} {path}");
                let name = [str_of(op, "summary"), str_of(op, "operationId")]
                    .into_iter()
                    .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
                    .find(|s| !s.is_empty())
                    .unwrap_or_else(|| label.clone());
                let (status, content_type, body) = self.response_example(op, &label);
                let response = MockResponse { method, path: convert_path(path), name, status, content_type, body };
                // The example was charged while it was built; charge the rest.
                let size = response.name.len() + response.path.len() + response.body.len() + 64;
                self.afford(size.saturating_sub(before - self.output_left));
                out.push(response);
            }
        }
        if out.is_empty() {
            self.warn("The document contains no operations".to_string());
        }
        if self.output_left == 0 {
            self.warnings.push(format!(
                "The document expands to more than {} MB of examples ($refs repeat the same content); \
                 examples were cut short and {skipped} operations were skipped",
                MAX_OUTPUT >> 20
            ));
        }
        (out, self.warnings)
    }

    /// Status, content type and body of an operation's first 2xx response
    /// (then a `2XX` range, then `default`; 200 with no body when there is none).
    fn response_example(&mut self, op: &'a Value, label: &str) -> (u16, Option<String>, String) {
        let none = |status| (status, None, String::new());
        let Some(responses) = op.get("responses").and_then(Value::as_object) else { return none(200) };
        let mut codes: Vec<(u16, &str)> = responses
            .keys()
            .filter_map(|k| k.trim().parse::<u16>().ok().filter(|c| (200..300).contains(c)).map(|c| (c, k.as_str())))
            .collect();
        codes.sort();
        let picked = codes
            .first()
            .copied()
            .or_else(|| responses.keys().find(|k| k.trim().eq_ignore_ascii_case("2XX")).map(|k| (200, k.as_str())))
            .or_else(|| responses.contains_key("default").then_some((200, "default")));
        let Some((status, key)) = picked else { return none(200) };
        let Some(response) = responses.get(key).and_then(|r| self.resolve(r)) else { return none(status) };
        if matches!(status, 204 | 205) {
            return none(status);
        }
        let (content_type, body) =
            if self.swagger2 { self.response_v2(op, response) } else { self.response_v3(response, label) };
        (status, content_type, body)
    }

    fn response_v3(&mut self, response: &'a Value, label: &str) -> (Option<String>, String) {
        let Some(content) = response.get("content").and_then(Value::as_object) else { return (None, String::new()) };
        let picked = content
            .iter()
            .find(|(k, _)| media_base(k) == "application/json")
            .or_else(|| content.iter().find(|(k, _)| is_json(&media_base(k))))
            .or_else(|| content.iter().next());
        let Some((media_type, media)) = picked else { return (None, String::new()) };
        let base = media_base(media_type);
        let value = match self.media_example(media) {
            Some(v) => self.copy(v),
            None => media.get("schema").and_then(|s| self.sample(s, "")),
        };
        let json = is_json(&base) || base == "*/*";
        let body = match value {
            Some(Value::String(s)) if !json => s,
            Some(v) if json || !is_xml(&base) => json_text(v),
            _ => String::new(),
        };
        if !json && !is_xml(&base) && !base.starts_with("text/") && !body.is_empty() {
            self.warn(format!("{label}: response type \"{media_type}\" was mocked as text"));
        }
        let content_type = if base == "*/*" { "application/json".to_string() } else { media_type.clone() };
        (Some(content_type), body)
    }

    fn response_v2(&mut self, op: &'a Value, response: &'a Value) -> (Option<String>, String) {
        let doc = self.doc;
        let produces: Vec<String> = op
            .get("produces")
            .or_else(|| doc.get("produces"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect();
        // Swagger 2 examples are keyed by media type.
        if let Some(examples) = response.get("examples").and_then(Value::as_object) {
            let picked = examples.iter().find(|(k, _)| is_json(&media_base(k))).or_else(|| examples.iter().next());
            if let Some((media_type, value)) = picked {
                let body = match value {
                    Value::String(s) if !is_json(&media_base(media_type)) && self.afford(s.len()) => s.clone(),
                    v => self.copy(v).map(json_text).unwrap_or_default(),
                };
                return (Some(media_type.clone()), body);
            }
        }
        let Some(schema) = response.get("schema") else { return (None, String::new()) };
        let json_type = produces.iter().find(|p| is_json(&media_base(p))).cloned();
        if json_type.is_none() && produces.iter().any(|p| is_xml(&media_base(p))) {
            return (produces.first().cloned(), String::new());
        }
        let body = self.sample(schema, "").map(json_text).unwrap_or_default();
        (Some(json_type.unwrap_or_else(|| "application/json".to_string())), body)
    }

    fn warn(&mut self, message: String) {
        // Messages quote the document, so they count against the output too.
        if !self.warned.contains(&message) && self.afford(message.len()) {
            self.warned.insert(message.clone());
            self.warnings.push(message);
        }
    }

    fn var(&mut self, key: &'static str, secret: bool) {
        self.vars.entry(key).or_insert(secret);
    }

    /// Charges `bytes` against the import's output; once it is used up, every charge fails.
    fn afford(&mut self, bytes: usize) -> bool {
        let ok = bytes < self.output_left;
        self.output_left = if ok { self.output_left - bytes } else { 0 };
        ok
    }

    /// Charges a document value that is about to be copied into the collection.
    fn afford_value(&mut self, v: &Value) -> bool {
        let len = json_len(v, self.output_left);
        self.afford(len)
    }

    fn copy(&mut self, v: &Value) -> Option<Value> {
        self.afford_value(v).then(|| v.clone())
    }

    /// Follows `$ref` chains within the document.
    fn resolve(&mut self, v: &'a Value) -> Option<&'a Value> {
        let mut v = v;
        let mut last = "";
        for _ in 0..MAX_REF_HOPS {
            let Some(r) = v.get("$ref").and_then(Value::as_str) else { return Some(v) };
            last = r;
            v = self.lookup(r)?;
        }
        self.warn(format!("$ref \"{last}\" loops back on itself and was skipped"));
        None
    }

    fn lookup(&mut self, r: &str) -> Option<&'a Value> {
        let doc = self.doc;
        let Some(pointer) = r.strip_prefix('#') else {
            self.warn(format!("External $ref \"{r}\" is not supported and was skipped"));
            return None;
        };
        let found = doc.pointer(pointer).or_else(|| percent_decode(pointer).and_then(|p| doc.pointer(&p)));
        if found.is_none() {
            self.warn(format!("$ref \"{r}\" could not be resolved and was skipped"));
        }
        found
    }

    fn base_url(&mut self) -> String {
        let doc = self.doc;
        let url = if self.swagger2 {
            let base_path = str_of(doc, "basePath").trim();
            let slash = if base_path.is_empty() || base_path.starts_with('/') { "" } else { "/" };
            match str_of(doc, "host").trim() {
                "" => format!("{slash}{base_path}"),
                host => {
                    let scheme = doc
                        .get("schemes")
                        .and_then(Value::as_array)
                        .and_then(|s| s.first())
                        .and_then(Value::as_str)
                        .unwrap_or("https");
                    format!("{scheme}://{host}{slash}{base_path}")
                }
            }
        } else {
            doc.get("servers").and_then(Value::as_array).and_then(|s| s.first()).map(server_url).unwrap_or_default()
        };
        let url = normalize_server(&url);
        if url.is_empty() {
            self.warn(
                "The document has no server URL; set the baseUrl variable to the API address \
                 (e.g. https://api.example.com)"
                    .to_string(),
            );
        } else if !url.contains("://") {
            self.warn(format!(
                "The server URL \"{url}\" is relative; set the baseUrl variable to the full address \
                 (e.g. https://api.example.com{url})"
            ));
        }
        url
    }

    /// An absolute path- or operation-level server that differs from the document's.
    fn server_override(&self, item: &Value, op: &Value) -> Option<String> {
        if self.swagger2 {
            return None;
        }
        let server = [op, item].into_iter().find_map(|v| v.get("servers")?.as_array()?.first())?;
        let url = normalize_server(&server_url(server));
        (url.contains("://") && url != self.base_url).then_some(url)
    }

    fn request(&mut self, path: &str, method: &str, item: &'a Value, op: &'a Value, auth: &Auth) -> Request {
        let label = format!("{} {path}", method.to_ascii_uppercase());
        let name = [str_of(op, "summary"), str_of(op, "operationId")]
            .into_iter()
            .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
            .find(|s| !s.is_empty())
            .unwrap_or_else(|| label.clone());
        let mut req = Request::new(name, RequestKind::Http);
        req.method = method.to_ascii_uppercase();
        req.openapi = Some(OpenApiOperation { operation: label.clone(), removed: false });
        let description = str_of(op, "description").trim();
        req.docs = match (is_true(op, "deprecated"), description.is_empty()) {
            (true, true) => "Deprecated.".to_string(),
            (true, false) => format!("Deprecated.\n\n{description}"),
            (false, _) => description.to_string(),
        };

        let params = self.parameters(item, op);
        let (path_url, path_vars) = variable_path(path);
        let prefix = self.server_override(item, op).unwrap_or_else(|| "{{baseUrl}}".to_string());
        let mut url = prefix + &path_url;
        for (name, var) in path_vars {
            if !self.path_vars.contains_key(&var) {
                let value = self.path_value(&name, params.get(&(name.as_str(), "path")).copied());
                self.path_vars.insert(var, value);
            }
        }

        let mut query = Vec::new();
        let mut cookies = Vec::new();
        let mut cookie_required = false;
        for &p in params.values() {
            let name = str_of(p, "name");
            let required = is_true(p, "required");
            match str_of(p, "in") {
                "query" => {
                    let value = self.param_value(p);
                    if required {
                        query.push(format!("{}={}", encode(name), encode(&value)));
                    } else {
                        req.disabled_params.push(kv(name.to_string(), value, false, description_of(p)));
                    }
                }
                // OpenAPI says these header parameters are ignored (media types and auth cover them).
                "header"
                    if !["accept", "content-type", "authorization"].contains(&name.to_ascii_lowercase().as_str()) =>
                {
                    let value = self.param_value(p);
                    req.headers.push(kv(name.to_string(), value, required, description_of(p)));
                }
                "cookie" => {
                    cookies.push(format!("{name}={}", self.param_value(p)));
                    cookie_required |= required;
                }
                _ => {}
            }
        }
        if !query.is_empty() {
            url.push(if url.contains('?') { '&' } else { '?' });
            url.push_str(&query.join("&"));
        }
        if !cookies.is_empty() {
            req.headers.push(kv("Cookie".to_string(), cookies.join("; "), cookie_required, String::new()));
        }
        req.url = url;
        req.body = if self.swagger2 { self.body_v2(op, &params) } else { self.body_v3(op, &label) };
        req.auth = self.operation_auth(op, auth);
        req
    }

    /// Path-level parameters overridden by operation-level ones (same `name` + `in`).
    fn parameters(&mut self, item: &'a Value, op: &'a Value) -> Params<'a> {
        let mut out = Params::new();
        for list in [item.get("parameters"), op.get("parameters")] {
            for p in list.and_then(Value::as_array).into_iter().flatten() {
                let Some(p) = self.resolve(p) else { continue };
                let key = (str_of(p, "name"), str_of(p, "in"));
                if !key.0.is_empty() && !key.1.is_empty() {
                    out.insert(key, p);
                }
            }
        }
        out
    }

    /// A parameter's value as text: the one the document gives, else empty.
    fn param_value(&mut self, p: &'a Value) -> String {
        let value = self.given_value(p);
        value.filter(|v| self.afford_value(v)).map(stringify).unwrap_or_default()
    }

    /// A path parameter's value as text: the one the document gives, else a realistic one from
    /// its schema and name (`p` is `None` when the path uses a parameter it does not declare).
    fn path_value(&mut self, name: &str, p: Option<&'a Value>) -> String {
        if let Some(p) = p {
            if let Some(v) = self.given_value(p) {
                return if self.afford_value(v) { stringify(v) } else { String::new() };
            }
            if let Some(v) = self.param_schema(p).and_then(|s| self.sample(s, name)) {
                return stringify(&v);
            }
        }
        scalar("string", &Value::Null, name).as_ref().map(stringify).unwrap_or_default()
    }

    /// The value the document gives a parameter: its example(s), its media type's, or its schema's.
    fn given_value(&mut self, p: &'a Value) -> Option<&'a Value> {
        let media = param_media(p);
        let schema = self.param_schema(p);
        p.get("example")
            .or_else(|| p.get("x-example"))
            .filter(|v| !v.is_null())
            .or_else(|| self.first_example(p))
            .or_else(|| media.and_then(|m| self.media_example(m)))
            .or_else(|| schema.and_then(explicit_value))
    }

    /// OpenAPI 3 parameters carry a `schema` (or `content`); Swagger 2 ones describe the type inline.
    fn param_schema(&mut self, p: &'a Value) -> Option<&'a Value> {
        match p.get("schema").or_else(|| param_media(p).and_then(|m| m.get("schema"))) {
            Some(s) => self.resolve(s),
            None if self.swagger2 => Some(p),
            None => None,
        }
    }

    /// `example`, else the first `examples` entry of a media type (or parameter).
    fn media_example(&mut self, m: &'a Value) -> Option<&'a Value> {
        m.get("example").filter(|v| !v.is_null()).or_else(|| self.first_example(m))
    }

    /// A media type's example as text (XML, CSV, ...).
    fn media_text(&mut self, media: &'a Value) -> String {
        match self.media_example(media) {
            Some(Value::String(s)) if self.afford(s.len()) => s.clone(),
            _ => String::new(),
        }
    }

    fn first_example(&mut self, v: &'a Value) -> Option<&'a Value> {
        let examples = v.get("examples")?.as_object()?;
        examples.values().find_map(|e| self.resolve(e)?.get("value"))
    }

    fn body_v3(&mut self, op: &'a Value, label: &str) -> Body {
        let mut body = Body::default();
        let content = op
            .get("requestBody")
            .and_then(|b| self.resolve(b))
            .and_then(|b| b.get("content"))
            .and_then(Value::as_object);
        let Some(content) = content else { return body };
        let picked = content
            .iter()
            .find(|(k, _)| media_base(k) == "application/json")
            .or_else(|| content.iter().find(|(k, _)| is_json(&media_base(k))))
            .or_else(|| content.iter().next());
        let Some((media_type, media)) = picked else { return body };
        let base = media_base(media_type);
        let schema = media.get("schema");

        if is_json(&base) || base == "*/*" {
            body.body_type = BodyType::Json;
            let value = match self.media_example(media) {
                Some(v) => self.copy(v),
                None => schema.and_then(|s| self.sample(s, "")),
            };
            body.text = value.map(json_text).unwrap_or_default();
        } else if base == "application/x-www-form-urlencoded" {
            body.body_type = BodyType::FormUrlencoded;
            body.form =
                self.schema_fields(media).into_iter().map(|f| kv(f.name, f.value, true, f.description)).collect();
        } else if base.starts_with("multipart/") {
            body.body_type = BodyType::Multipart;
            body.multipart = self.schema_fields(media).into_iter().map(multipart_field).collect();
        } else if is_xml(&base) {
            body.body_type = BodyType::Xml;
            body.text = self.media_text(media);
        } else if base.starts_with("text/") {
            body.body_type = BodyType::Text;
            body.content_type = Some(media_type.clone());
            body.text = self.media_text(media);
        } else if base == "application/octet-stream" || self.is_file(schema) {
            body.body_type = BodyType::Binary;
            body.content_type = Some(media_type.clone());
        } else {
            body.body_type = BodyType::Text;
            body.content_type = Some(media_type.clone());
            body.text = self.media_text(media);
            self.warn(format!("{label}: request body type \"{media_type}\" was imported as plain text"));
        }
        body
    }

    fn body_v2(&mut self, op: &'a Value, params: &Params<'a>) -> Body {
        let doc = self.doc;
        let consumes: Vec<String> = op
            .get("consumes")
            .or_else(|| doc.get("consumes"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(media_base)
            .collect();
        let mut body = Body::default();
        if let Some(p) = params.values().copied().find(|p| str_of(p, "in") == "body") {
            let xml_only =
                !consumes.is_empty() && !consumes.iter().any(|c| is_json(c)) && consumes.iter().any(|c| is_xml(c));
            if xml_only {
                body.body_type = BodyType::Xml;
            } else {
                body.body_type = BodyType::Json;
                body.text = p.get("schema").and_then(|s| self.sample(s, "")).map(json_text).unwrap_or_default();
            }
            return body;
        }

        let form: Vec<&'a Value> = params.values().copied().filter(|p| str_of(p, "in") == "formData").collect();
        if form.is_empty() {
            return body;
        }
        let fields: Vec<Field> = form
            .into_iter()
            .map(|p| {
                let file = str_of(p, "type") == "file";
                Field {
                    name: str_of(p, "name").to_string(),
                    value: if file { String::new() } else { self.param_value(p) },
                    description: description_of(p),
                    file,
                    content_type: None,
                }
            })
            .collect();
        if consumes.iter().any(|c| c == "multipart/form-data") || fields.iter().any(|f| f.file) {
            body.body_type = BodyType::Multipart;
            body.multipart = fields.into_iter().map(multipart_field).collect();
        } else {
            body.body_type = BodyType::FormUrlencoded;
            body.form = fields.into_iter().map(|f| kv(f.name, f.value, true, f.description)).collect();
        }
        body
    }

    /// Form fields from the properties of a media type's object schema.
    fn schema_fields(&mut self, media: &'a Value) -> Vec<Field> {
        let mut props = IndexMap::new();
        if let Some(schema) = media.get("schema") {
            self.collect_props(schema, &mut props, &mut HashSet::new(), 0);
        }
        let example = self.media_example(media);
        let mut fields = Vec::new();
        for (name, prop) in props {
            let Some(prop) = self.resolve(prop) else { continue };
            if is_true(prop, "readOnly") {
                continue;
            }
            let file = self.is_file(Some(prop));
            let value = match example.and_then(|e| e.get(&name)).or_else(|| explicit_value(prop)) {
                Some(v) if !file && self.afford_value(v) => stringify(v),
                _ => String::new(),
            };
            // `prop` may be a `$ref` shared by every field.
            let description = str_of(prop, "description").trim();
            let description = if self.afford(description.len()) { description.to_string() } else { String::new() };
            let content_type = media
                .get("encoding")
                .and_then(|e| e.get(&name))
                .and_then(|e| e.get("contentType"))
                .and_then(Value::as_str)
                .map(str::to_string);
            fields.push(Field { description, name, value, file, content_type });
        }
        fields
    }

    fn collect_props(
        &mut self,
        schema: &'a Value,
        out: &mut IndexMap<String, &'a Value>,
        seen: &mut HashSet<*const Value>,
        depth: usize,
    ) {
        let Some(s) = self.resolve(schema) else { return };
        // A schema seen before adds nothing new; walking it again makes `allOf` fan-out exponential.
        if depth > MAX_DEPTH || !seen.insert(s) || !self.afford(VISIT_COST) {
            return;
        }
        let parts = s.get("allOf").and_then(Value::as_array).into_iter().flatten();
        let option = s.get("oneOf").or_else(|| s.get("anyOf")).and_then(Value::as_array).and_then(|o| o.first());
        for part in parts.chain(option) {
            self.collect_props(part, out, seen, depth + 1);
        }
        for (name, prop) in s.get("properties").and_then(Value::as_object).into_iter().flatten() {
            out.insert(name.clone(), prop);
        }
    }

    fn is_file(&mut self, schema: Option<&'a Value>) -> bool {
        let Some(mut s) = schema.and_then(|s| self.resolve(s)) else { return false };
        if schema_type(s) == Some("array") {
            match s.get("items").and_then(|i| self.resolve(i)) {
                Some(items) => s = items,
                None => return false,
            }
        }
        match schema_type(s) {
            Some("file") => true,
            Some("string") => {
                let media = s.get("contentMediaType").and_then(Value::as_str).map(media_base);
                matches!(str_of(s, "format"), "binary" | "base64")
                    || media.is_some_and(|m| !m.starts_with("text/") && !is_json(&m))
            }
            _ => false,
        }
    }

    /// An example value for a schema, or `None` if nothing sensible can be generated. `name` is
    /// the property or parameter the schema describes (empty for a body), which picks realistic
    /// values when the schema gives none.
    fn sample(&mut self, schema: &'a Value, name: &str) -> Option<Value> {
        self.active.clear();
        self.budget = MAX_PROPS;
        self.visits = MAX_VISITS;
        self.example(schema, name, 0)
    }

    /// Counts one schema visit for the current example; false once the limits are reached.
    fn visit(&mut self) -> bool {
        let ok = self.visits > 0 && self.afford(VISIT_COST);
        self.visits = self.visits.saturating_sub(1);
        ok
    }

    fn example(&mut self, schema: &'a Value, name: &str, depth: usize) -> Option<Value> {
        if !self.visit() {
            return None;
        }
        if let Some(v) = explicit_value(schema) {
            return self.copy(v);
        }
        let s = self.resolve(schema)?;
        if let Some(v) = explicit_value(s) {
            return self.copy(v);
        }
        if self.active.len() >= MAX_NESTING || self.active.iter().any(|a| std::ptr::eq(*a, s)) {
            return None;
        }
        self.active.push(s);
        let out = self.generate(s, name, depth);
        self.active.pop();
        out
    }

    fn generate(&mut self, s: &'a Value, name: &str, depth: usize) -> Option<Value> {
        let mut object: Option<Map<String, Value>> = None;
        let mut other = None;
        let mut absorb = |v: Value, object: &mut Option<Map<String, Value>>| match v {
            Value::Object(m) => object.get_or_insert_with(Map::new).extend(m),
            v => other = Some(v),
        };
        for part in s.get("allOf").and_then(Value::as_array).into_iter().flatten() {
            if let Some(v) = self.example(part, name, depth) {
                absorb(v, &mut object);
            }
        }
        let options = s.get("oneOf").or_else(|| s.get("anyOf")).and_then(Value::as_array);
        if let Some(v) =
            options.into_iter().flatten().find_map(|o| self.example(o, name, depth).filter(|v| !v.is_null()))
        {
            absorb(v, &mut object);
        }
        match schema_type(s) {
            Some("object") => {
                let props = self.properties(s, depth);
                object.get_or_insert_with(Map::new).extend(props);
            }
            Some("array") => {
                // Items are named after the array: `emails` holds `email`s.
                let item = match s.get("items") {
                    Some(items) if depth < MAX_DEPTH => self.example(items, &singular(name), depth + 1),
                    _ => None,
                };
                let count = item_count(s);
                let mut items = Vec::new();
                if let Some(item) = item.filter(|_| count > 0) {
                    while items.len() + 1 < count && self.afford_value(&item) {
                        items.push(item.clone());
                    }
                    items.push(item);
                }
                absorb(Value::Array(items), &mut object);
            }
            Some(kind) => {
                let value = scalar(kind, s, name);
                if let Some(v) = value.filter(|v| self.afford_value(v)) {
                    absorb(v, &mut object);
                }
            }
            None => {}
        }
        match (object, other) {
            (Some(m), Some(v)) if m.is_empty() => Some(v),
            (Some(m), _) => Some(Value::Object(m)),
            (None, v) => v,
        }
    }

    fn properties(&mut self, s: &'a Value, depth: usize) -> Map<String, Value> {
        let mut out = Map::new();
        if depth >= MAX_DEPTH {
            return out;
        }
        for (name, prop) in s.get("properties").and_then(Value::as_object).into_iter().flatten() {
            // Skipped properties cost a visit too.
            if self.budget == 0 || !self.visit() {
                break;
            }
            // Read-only properties only appear in responses, write-only ones only in requests.
            let hidden = if self.for_response { "writeOnly" } else { "readOnly" };
            if is_true(prop, hidden) || self.resolve(prop).is_some_and(|p| is_true(p, hidden)) {
                continue;
            }
            self.budget -= 1;
            if !self.afford(name.len()) {
                break;
            }
            if let Some(v) = self.example(prop, name, depth + 1) {
                out.insert(name.clone(), v);
            }
        }
        out
    }

    fn operation_auth(&mut self, op: &'a Value, collection: &Auth) -> Auth {
        let Some(reqs) = op.get("security").and_then(Value::as_array) else { return Auth::Inherit };
        if reqs.iter().all(|r| r.as_object().is_none_or(|m| m.is_empty())) {
            return Auth::None;
        }
        match self.requirements_auth(reqs) {
            Some(auth) if auth != *collection => auth,
            _ => Auth::Inherit,
        }
    }

    /// Auth for the first requirement whose scheme can be mapped.
    fn requirements_auth(&mut self, reqs: &'a [Value]) -> Option<Auth> {
        for req in reqs {
            let Some(names) = req.as_object() else { continue };
            if names.len() > 1 {
                let list = names.keys().map(String::as_str).collect::<Vec<_>>().join(" + ");
                self.warn(format!("Security requirement {list} combines several schemes; only one was imported"));
            }
            for name in names.keys() {
                if let Some(auth) = self.scheme_auth(name) {
                    return Some(auth);
                }
            }
        }
        None
    }

    fn scheme_auth(&mut self, name: &str) -> Option<Auth> {
        if let Some(auth) = self.schemes.get(name) {
            return auth.clone();
        }
        let doc = self.doc;
        let defs =
            if self.swagger2 { doc.get("securityDefinitions") } else { doc.pointer("/components/securitySchemes") };
        let auth = match defs.and_then(|d| d.get(name)).and_then(|s| self.resolve(s)) {
            Some(scheme) => self.map_scheme(name, scheme),
            None => {
                self.warn(format!("Security scheme \"{name}\" is not defined"));
                None
            }
        };
        self.schemes.insert(name.to_string(), auth.clone());
        auth
    }

    fn map_scheme(&mut self, name: &str, s: &'a Value) -> Option<Auth> {
        let unsupported =
            |what: &str| format!("Security scheme \"{name}\" ({what}) is not supported; set up auth manually");
        match str_of(s, "type") {
            "basic" => Some(self.basic()),
            "http" => match str_of(s, "scheme").to_ascii_lowercase().as_str() {
                "basic" => Some(self.basic()),
                "bearer" => {
                    self.var("bearerToken", true);
                    Some(Auth::Bearer { token: "{{bearerToken}}".to_string(), prefix: "Bearer".to_string() })
                }
                "digest" => {
                    self.var("username", false);
                    self.var("password", true);
                    Some(Auth::Digest { username: "{{username}}".to_string(), password: "{{password}}".to_string() })
                }
                other => {
                    self.warn(unsupported(&format!("HTTP {other}")));
                    None
                }
            },
            "apiKey" => {
                let location = match str_of(s, "in") {
                    "header" => ApiKeyLocation::Header,
                    "query" => ApiKeyLocation::Query,
                    other => {
                        self.warn(unsupported(&format!("API key in {other}")));
                        return None;
                    }
                };
                self.var("apiKey", true);
                Some(Auth::ApiKey { key: str_of(s, "name").to_string(), value: "{{apiKey}}".to_string(), location })
            }
            "oauth2" => self.oauth2(name, s),
            other => {
                self.warn(unsupported(other));
                None
            }
        }
    }

    fn basic(&mut self) -> Auth {
        self.var("username", false);
        self.var("password", true);
        Auth::Basic { username: "{{username}}".to_string(), password: "{{password}}".to_string() }
    }

    fn oauth2(&mut self, name: &str, s: &'a Value) -> Option<Auth> {
        // Swagger 2 keeps the single flow's fields on the scheme itself.
        let flow = if self.swagger2 {
            let grant = match str_of(s, "flow") {
                "application" => Some(GrantType::ClientCredentials),
                "accessCode" => Some(GrantType::AuthorizationCode),
                "password" => Some(GrantType::Password),
                "implicit" => Some(GrantType::Implicit),
                _ => None,
            };
            grant.map(|g| (g, s))
        } else {
            let flows = s.get("flows");
            [
                ("clientCredentials", GrantType::ClientCredentials),
                ("authorizationCode", GrantType::AuthorizationCode),
                ("password", GrantType::Password),
                ("implicit", GrantType::Implicit),
            ]
            .into_iter()
            .find_map(|(key, grant)| Some((grant, flows?.get(key)?)))
        };
        let Some((grant_type, flow)) = flow else {
            self.warn(format!("OAuth2 scheme \"{name}\" has no flow; set up auth manually"));
            return None;
        };
        let scope = flow
            .get("scopes")
            .and_then(Value::as_object)
            .map(|m| m.keys().map(String::as_str).collect::<Vec<_>>().join(" "))
            .unwrap_or_default();
        self.var("clientId", false);
        self.var("clientSecret", true);
        let mut config = OAuth2Config {
            grant_type,
            token_url: str_of(flow, "tokenUrl").to_string(),
            client_id: "{{clientId}}".to_string(),
            client_secret: "{{clientSecret}}".to_string(),
            scope,
            ..OAuth2Config::default()
        };
        match grant_type {
            GrantType::AuthorizationCode | GrantType::Implicit => {
                config.auth_url = str_of(flow, "authorizationUrl").to_string()
            }
            GrantType::Password => {
                self.var("username", false);
                self.var("password", true);
                config.username = "{{username}}".to_string();
                config.password = "{{password}}".to_string();
            }
            GrantType::ClientCredentials => {}
        }
        Some(Auth::OAuth2(config))
    }
}

fn str_of<'v>(v: &'v Value, key: &str) -> &'v str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

fn is_true(v: &Value, key: &str) -> bool {
    v.get(key) == Some(&Value::Bool(true))
}

fn description_of(v: &Value) -> String {
    str_of(v, "description").trim().to_string()
}

fn kv(key: String, value: String, enabled: bool, description: String) -> KeyValue {
    KeyValue { key, value, enabled, description }
}

fn multipart_field(f: Field) -> MultipartField {
    MultipartField { key: f.name, value: f.value, file: f.file, content_type: f.content_type, enabled: true }
}

/// The first media type of an OpenAPI 3 parameter's `content`.
fn param_media(p: &Value) -> Option<&Value> {
    p.get("content").and_then(Value::as_object).and_then(|c| c.values().next())
}

fn first_tag(op: &Value) -> Option<&str> {
    let tag = op.get("tags")?.as_array()?.first()?.as_str()?.trim();
    (!tag.is_empty()).then_some(tag)
}

/// A value given by the schema itself: `example`, `examples[0]`, `default`, `const` or the first `enum`.
fn explicit_value(s: &Value) -> Option<&Value> {
    let non_null = |key: &str| s.get(key).filter(|v| !v.is_null());
    non_null("example")
        .or_else(|| s.get("examples")?.as_array()?.first())
        .or_else(|| non_null("default"))
        .or_else(|| s.get("const"))
        .or_else(|| s.get("enum")?.as_array()?.iter().find(|v| !v.is_null()))
}

/// The schema's type; for 3.1 type arrays the first non-null one. Inferred when absent
/// (numeric formats such as `int64` mean a number).
fn schema_type(s: &Value) -> Option<&str> {
    let explicit = match s.get("type") {
        Some(Value::String(t)) => Some(t.as_str()),
        Some(Value::Array(types)) => {
            let mut names = types.iter().filter_map(Value::as_str);
            names.clone().find(|t| *t != "null").or_else(|| names.next())
        }
        _ => None,
    };
    explicit.or_else(|| {
        if s.get("properties").is_some() || s.get("additionalProperties").is_some() {
            Some("object")
        } else if s.get("items").is_some() {
            Some("array")
        } else {
            match str_of(s, "format") {
                "int32" | "int64" => Some("integer"),
                "float" | "double" => Some("number"),
                _ => None,
            }
        }
    })
}

/// A realistic value for a scalar schema that gives none itself. Strings follow the `pattern`
/// when it is a plain literal, then the `format`, then the name of the property or parameter
/// (see `name_hint`); numbers follow the name. Length, range and step constraints are respected.
fn scalar(kind: &str, s: &Value, name: &str) -> Option<Value> {
    Some(match kind {
        "string" => Value::String(text_value(s, name_hint(name))),
        "integer" => number_value(s, name_hint(name), true),
        "number" => number_value(s, name_hint(name), false),
        "boolean" => Value::Bool(true),
        "null" => Value::Null,
        _ => return None,
    })
}

fn text_value(s: &Value, hint: Option<Hint>) -> String {
    if let Some(literal) = pattern_literal(str_of(s, "pattern")) {
        return literal;
    }
    let named = hint.and_then(|h| h.text);
    let text = match str_of(s, "format") {
        "date-time" => "2024-01-01T00:00:00Z".to_string(),
        "date" => "2024-01-01".to_string(),
        "time" => "12:00:00".to_string(),
        "uuid" => EXAMPLE_UUID.to_string(),
        // The name may say which address (`avatar` is an image URL).
        "email" | "idn-email" => named.filter(|t| t.contains('@')).unwrap_or("user@example.com").to_string(),
        "uri" | "url" | "uri-reference" | "iri" => {
            named.filter(|t| t.starts_with("https://")).unwrap_or("https://example.com").to_string()
        }
        "hostname" | "idn-hostname" => "api.example.com".to_string(),
        "ipv4" => EXAMPLE_IP.to_string(),
        "ipv6" => "2001:db8::1".to_string(),
        "byte" => "U29tZSBkYXRh".to_string(),
        "password" => EXAMPLE_SECRET.to_string(),
        "int32" | "int64" => stringify(&number_value(s, hint, true)),
        "float" | "double" => stringify(&number_value(s, hint, false)),
        // Names with only a numeric example (`price`, `page`) still read well as text.
        _ => match hint {
            Some(Hint { text: Some(t), .. }) => t.to_string(),
            Some(Hint { num: Some(n), .. }) => n.to_string(),
            Some(Hint { int: Some(i), .. }) => i.to_string(),
            _ => "example".to_string(),
        },
    };
    fit_length(text, s)
}

/// Pads (with `x`) or cuts `text` to the schema's `minLength` and `maxLength`.
fn fit_length(mut text: String, s: &Value) -> String {
    let limit = |key| s.get(key).and_then(Value::as_u64);
    let count = text.chars().count() as u64;
    if let Some(min) = limit("minLength").map(|n| n.min(MAX_PADDING))
        && count < min
    {
        text.extend(std::iter::repeat_n('x', (min - count) as usize));
    }
    if let Some(max) = limit("maxLength")
        && let Some((cut, _)) = text.char_indices().nth(max.try_into().unwrap_or(usize::MAX))
    {
        text.truncate(cut);
    }
    text
}

/// The only text an anchored literal pattern such as `^v1$` or `^api\.example$` matches;
/// `None` for anything that needs a regex engine.
fn pattern_literal(pattern: &str) -> Option<String> {
    let body = pattern.strip_prefix('^')?.strip_suffix('$')?;
    let mut out = String::with_capacity(body.len());
    let mut chars = body.chars();
    while let Some(c) = chars.next() {
        match c {
            // `\.` is a literal dot; `\d`, `\w` and friends are classes.
            '\\' => match chars.next() {
                Some(escaped) if !escaped.is_alphanumeric() => out.push(escaped),
                _ => return None,
            },
            '.' | '[' | ']' | '(' | ')' | '{' | '}' | '*' | '+' | '?' | '|' | '^' | '$' => return None,
            c => out.push(c),
        }
    }
    Some(out)
}

fn number_value(s: &Value, hint: Option<Hint>, integer: bool) -> Value {
    let named = match hint {
        Some(h) if integer => h.int.or(h.num.map(|n| n.trunc() as i64)).map(|i| i as f64),
        Some(h) => h.num.or(h.int.map(|i| i as f64)),
        None => None,
    };
    let v = constrain(named.unwrap_or(if integer { 1.0 } else { 1.5 }), s, integer);
    // Whole numbers print without a fraction (`50`, not `50.0`).
    if integer || (v.fract() == 0.0 && v.abs() < 1e15) {
        Value::from(v as i64)
    } else {
        serde_json::Number::from_f64(v).map_or(Value::from(0), Value::Number)
    }
}

/// Moves `v` onto the schema's `multipleOf` and into its `minimum`/`maximum` range, including
/// the exclusive bounds of OpenAPI 3.0 (a flag) and 3.1 (a number). Integers stay whole.
fn constrain(v: f64, s: &Value, integer: bool) -> f64 {
    let num = |key: &str| s.get(key).and_then(Value::as_f64).filter(|n| n.is_finite());
    let step = match num("multipleOf").filter(|m| *m > 0.0) {
        Some(m) if !integer || m.fract() == 0.0 => Some(m),
        _ => integer.then_some(1.0),
    };
    // (bound, exclusive); when both forms are given the stricter one counts.
    let bound = |key: &str, exclusive: &str, lower: bool| {
        let plain = num(key).map(|b| (b, is_true(s, exclusive)));
        let strict = num(exclusive).map(|b| (b, true));
        match (plain, strict) {
            (Some(p), Some(e)) => Some(if e.0 == p.0 || (e.0 > p.0) == lower { e } else { p }),
            (p, e) => p.or(e),
        }
    };
    let low = bound("minimum", "exclusiveMinimum", true);
    let high = bound("maximum", "exclusiveMaximum", false);
    let snap = |x: f64, round: fn(f64) -> f64| match step {
        Some(st) => tidy(round(x / st) * st, st),
        None => x,
    };
    let mut v = snap(v, f64::round);
    if let Some((lo, exclusive)) = low
        && (v < lo || (exclusive && v <= lo))
    {
        v = match step {
            Some(st) => match snap(lo, f64::ceil) {
                x if exclusive && x <= lo => tidy(x + st, st),
                x => x,
            },
            None if exclusive => high.map_or(lo + 1.0, |(hi, _)| (lo + hi) / 2.0),
            None => lo,
        };
    }
    if let Some((hi, exclusive)) = high
        && (v > hi || (exclusive && v >= hi))
    {
        v = match step {
            Some(st) => match snap(hi, f64::floor) {
                x if exclusive && x >= hi => tidy(x - st, st),
                x => x,
            },
            None if exclusive => low.map_or(hi - 1.0, |(lo, _)| (lo + hi) / 2.0),
            None => hi,
        };
    }
    v
}

/// `v` rounded to as many decimals as `step` has, so steps of 0.1 give 0.3, not 0.30000000000000004.
fn tidy(v: f64, step: f64) -> f64 {
    let decimals = step.to_string().split_once('.').map_or(0, |(_, d)| d.len().min(12));
    let scale = 10f64.powi(decimals as i32);
    let scaled = (v * scale).round();
    if scaled.is_finite() { scaled / scale } else { v }
}

/// How many items to generate for an array: `minItems`, at least one, at most `maxItems`.
fn item_count(s: &Value) -> usize {
    let limit = |key| s.get(key).and_then(Value::as_u64);
    let count = limit("minItems").unwrap_or(1).clamp(1, MAX_ITEMS);
    limit("maxItems").map_or(count, |max| count.min(max)) as usize
}

/// Example values suggested by a property or parameter name, per type.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Hint {
    text: Option<&'static str>,
    int: Option<i64>,
    num: Option<f64>,
}

const fn text_hint(text: &'static str) -> Hint {
    Hint { text: Some(text), int: None, num: None }
}

const fn int_hint(int: i64) -> Hint {
    Hint { text: None, int: Some(int), num: None }
}

const fn num_hint(num: f64) -> Hint {
    Hint { text: None, int: None, num: Some(num) }
}

const ID_HINT: Hint = Hint { text: Some("abc123"), int: Some(1), num: None };
/// 2024-01-01T12:00:00Z; integers are Unix seconds.
const TIMESTAMP_HINT: Hint = Hint { text: Some(EXAMPLE_TIMESTAMP), int: Some(1_704_110_400), num: None };

/// Names (as `name_key` gives them) and the values they suggest.
const NAME_HINTS: &[(&[&str], Hint)] = &[
    (&["email", "emailaddress", "mail"], text_hint("jane.doe@example.com")),
    (&["name", "fullname", "displayname"], text_hint("Jane Doe")),
    (&["firstname", "givenname", "forename"], text_hint("Jane")),
    (&["lastname", "surname", "familyname"], text_hint("Doe")),
    (&["username", "login", "handle", "nickname"], text_hint("jane.doe")),
    (&["phone", "phonenumber", "mobile", "mobilenumber", "telephone", "tel"], text_hint("+1 555 0100")),
    (&["url", "uri", "website", "homepage", "link", "href"], text_hint("https://example.com")),
    (
        &["avatar", "avatarurl", "image", "imageurl", "photo", "photourl", "picture", "thumbnail", "thumbnailurl"],
        text_hint("https://example.com/image.png"),
    ),
    (&["city", "town"], text_hint("Berlin")),
    (&["country", "countryname"], text_hint("Germany")),
    (&["countrycode", "countryiso"], text_hint("DE")),
    (&["currency", "currencycode"], text_hint("EUR")),
    (&["zip", "zipcode", "postalcode", "postcode"], text_hint("10115")),
    (&["street", "streetaddress", "address", "addressline", "addressline1"], text_hint("Alexanderplatz 1")),
    (&["title"], text_hint("Example title")),
    (&["description", "summary", "bio", "notes", "note", "comment", "message"], text_hint("A short description.")),
    (&["status", "state"], text_hint("active")),
    (&["type", "kind", "category"], text_hint("default")),
    (&["token", "apikey", "secret", "password"], text_hint(EXAMPLE_SECRET)),
    (&["language", "lang", "languagecode", "locale"], text_hint("en")),
    (&["timezone", "tz"], text_hint("Europe/Berlin")),
    (&["color", "colour"], text_hint("#D97757")),
    (&["ip", "ipaddress"], text_hint(EXAMPLE_IP)),
    (&["slug"], text_hint("example-slug")),
    (&["timestamp"], TIMESTAMP_HINT),
    (
        &["price", "amount", "total", "subtotal", "cost", "balance"],
        Hint { text: None, int: Some(1999), num: Some(19.99) },
    ),
    (&["quantity", "qty", "count", "size"], int_hint(1)),
    (&["limit", "pagesize", "perpage"], int_hint(20)),
    (&["page", "pagenumber"], int_hint(1)),
    (&["offset"], int_hint(0)),
    (&["age"], int_hint(30)),
    (&["year"], int_hint(2024)),
    (&["rating", "score"], Hint { text: None, int: Some(5), num: Some(4.5) }),
    (&["percent", "percentage"], int_hint(50)),
    (&["latitude", "lat"], num_hint(52.52)),
    (&["longitude", "lng", "lon"], num_hint(13.405)),
];

/// Words from `NAME_HINTS` that only mean something as the whole name: `companyName` is not a
/// person and `billingState` is not a status.
const WHOLE_NAME_ONLY: [&str; 4] = ["name", "state", "login", "handle"];

/// The values a property or parameter name suggests: the whole name first (`firstName`), then
/// what it ends with (`createdAt`, `userId`, `unitPrice`).
fn name_hint(name: &str) -> Option<Hint> {
    let words = name_words(name);
    let key = words.concat();
    let lookup = |k: &str| NAME_HINTS.iter().find(|(names, _)| names.contains(&k)).map(|(_, hint)| *hint);
    if let Some(hint) = lookup(&key) {
        return Some(hint);
    }
    if key.ends_with("uuid") || key.ends_with("guid") {
        return Some(text_hint(EXAMPLE_UUID));
    }
    match words.last()?.as_str() {
        "id" => Some(ID_HINT),
        "at" | "date" => Some(TIMESTAMP_HINT),
        last if words.len() > 1 && !WHOLE_NAME_ONLY.contains(&last) => lookup(last),
        _ => None,
    }
}

/// The lowercase words of a name, split at `_`, `-` and other separators and at case changes:
/// `session_id`, `session-id`, `sessionId` and `SessionID` all give `["session", "id"]`.
fn name_words(name: &str) -> Vec<String> {
    let chars: Vec<char> = name.chars().collect();
    let mut words = Vec::new();
    let mut word = String::new();
    for (i, &c) in chars.iter().enumerate() {
        if !c.is_alphanumeric() {
            if !word.is_empty() {
                words.push(std::mem::take(&mut word));
            }
            continue;
        }
        if c.is_uppercase() && !word.is_empty() {
            let prev = chars[i - 1];
            // `sessionId` splits before `I`, `HTMLParser` before `P`.
            let next_lower = chars.get(i + 1).is_some_and(|n| n.is_lowercase());
            if !prev.is_uppercase() || next_lower {
                words.push(std::mem::take(&mut word));
            }
        }
        word.extend(c.to_lowercase());
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
}

/// `session_id`, `session-id` and `SessionID` all become `sessionId`.
fn camel_case(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for (i, word) in name_words(name).iter().enumerate() {
        let mut chars = word.chars();
        match chars.next() {
            Some(first) if i > 0 => out.extend(first.to_uppercase().chain(chars)),
            _ => out.push_str(word),
        }
    }
    out
}

/// Naive singular of an English noun: `categories` → `category`, `pets` → `pet`. Words ending in
/// `ss`, `us` or `is` (`address`, `status`, `analysis`) are left alone.
fn singular(word: &str) -> String {
    let lower = word.to_ascii_lowercase();
    if lower.len() > 3 && lower.ends_with("ies") {
        let y = if word.ends_with("IES") { "Y" } else { "y" };
        format!("{}{y}", &word[..word.len() - 3])
    } else if lower.ends_with('s') && !["ss", "us", "is"].iter().any(|e| lower.ends_with(e)) {
        word[..word.len() - 1].to_string()
    } else {
        word.to_string()
    }
}

/// Pretty JSON body text; a string holding a JSON document is expanded.
fn json_text(value: Value) -> String {
    let value = match value {
        Value::Null => return String::new(),
        Value::String(s) => match serde_json::from_str::<Value>(&s) {
            Ok(v @ (Value::Object(_) | Value::Array(_))) => v,
            _ => Value::String(s),
        },
        v => v,
    };
    serde_json::to_string_pretty(&value).unwrap_or_default()
}

/// Text form of an example value for a parameter or form field.
fn stringify(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Array(items) => items.iter().map(stringify).collect::<Vec<_>>().join(","),
        other => other.to_string(),
    }
}

/// Media type without parameters, lowercased.
fn media_base(media_type: &str) -> String {
    media_type.split(';').next().unwrap_or_default().trim().to_ascii_lowercase()
}

fn is_json(media: &str) -> bool {
    media == "application/json" || media == "text/json" || media.ends_with("+json")
}

fn is_xml(media: &str) -> bool {
    media == "application/xml" || media == "text/xml" || media.ends_with("+xml")
}

/// Rough length of `v` as JSON text; stops counting once it is past `limit`.
fn json_len(v: &Value, limit: usize) -> usize {
    let mut len = 0;
    let mut stack = vec![v];
    while let Some(v) = stack.pop() {
        match v {
            Value::String(s) => len += s.len() + 2,
            Value::Array(items) => {
                len += items.len() + 2;
                if len <= limit {
                    stack.extend(items);
                }
            }
            Value::Object(m) => {
                len += m.len() * 4 + 2;
                if len <= limit {
                    for (k, v) in m {
                        len += k.len();
                        stack.push(v);
                    }
                }
            }
            _ => len += 5,
        }
        if len > limit {
            break;
        }
    }
    len
}

fn server_url(server: &Value) -> String {
    let url = str_of(server, "url");
    let vars = server.get("variables");
    let mut out = String::with_capacity(url.len());
    let mut rest = url;
    while let Some(start) = rest.find('{') {
        let Some(len) = rest[start..].find('}') else { break };
        let name = &rest[start + 1..start + len];
        out.push_str(&rest[..start]);
        match vars.and_then(|v| v.get(name)).and_then(|v| v.get("default")) {
            Some(default) => out.push_str(&stringify(default)),
            None => out.push_str(&rest[start..=start + len]),
        }
        rest = &rest[start + len + 1..];
    }
    out.push_str(rest);
    out
}

fn normalize_server(url: &str) -> String {
    let url = url.trim().trim_end_matches('/');
    if url.starts_with("//") { format!("https:{url}") } else { url.to_string() }
}

/// Rewrites each `{name}` template of a path with `f(name, previous segment)`, where the
/// previous segment is the whole `/`-separated segment before the one holding the template.
/// Relative paths get a leading `/`; empty (`{}`) and unclosed templates stay as written.
fn map_templates(path: &str, mut f: impl FnMut(&str, Option<&str>) -> String) -> String {
    let mut out = String::with_capacity(path.len() + 1);
    if !path.starts_with('/') {
        out.push('/');
    }
    let mut pos = 0;
    while let Some(start) = path[pos..].find('{').map(|i| pos + i) {
        let Some(len) = path[start..].find('}') else { break };
        let name = &path[start + 1..start + len];
        out.push_str(&path[pos..start]);
        if name.is_empty() {
            out.push_str("{}");
        } else {
            let previous = path[..start].rfind('/').and_then(|slash| path[..slash].rsplit('/').next());
            out.push_str(&f(name, previous.filter(|s| !s.is_empty())));
        }
        pos = start + len + 1;
    }
    out.push_str(&path[pos..]);
    out
}

/// Converts `{name}` path templates to `:name` (the form mock routes use).
fn convert_path(path: &str) -> String {
    map_templates(path, |name, _| format!(":{name}"))
}

/// Converts `{name}` path templates to `{{variable}}` placeholders (see `path_variable`),
/// returning each parameter name with its variable, in order of first appearance. A template
/// with no letters or digits in its name stays as written.
fn variable_path(path: &str) -> (String, Vec<(String, String)>) {
    let mut vars: IndexMap<String, String> = IndexMap::new();
    let out = map_templates(path, |name, previous| {
        match vars.entry(name.to_string()).or_insert_with(|| path_variable(name, previous)) {
            var if var.is_empty() => format!("{{{name}}}"),
            var => format!("{{{{{var}}}}}"),
        }
    });
    (out, vars.into_iter().filter(|(_, var)| !var.is_empty()).collect())
}

/// Parameter names too generic to stand alone as a variable.
const GENERIC_PARAMS: [&str; 6] = ["id", "uuid", "key", "slug", "name", "code"];

/// The variable for a path parameter: its camelCase name (`session_id` → `sessionId`), with a
/// generic name prefixed by the singular of the literal segment before it (`/pets/{id}` →
/// `petId`, `/categories/{id}` → `categoryId`).
fn path_variable(name: &str, previous: Option<&str>) -> String {
    let var = camel_case(name);
    if !GENERIC_PARAMS.contains(&var.as_str()) {
        return var;
    }
    let prefix = previous.filter(|s| !s.contains(['{', '}'])).map(|s| camel_case(&singular(s))).unwrap_or_default();
    if prefix.is_empty() {
        return var;
    }
    // Generic names are short ASCII words.
    format!("{prefix}{}{}", var[..1].to_ascii_uppercase(), &var[1..])
}

/// Percent-encodes a query component, leaving `{{variable}}` placeholders intact.
fn encode(s: &str) -> String {
    fn percent(s: &str, out: &mut String) {
        for b in s.bytes() {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                out.push(b as char);
            } else {
                let _ = write!(out, "%{b:02X}");
            }
        }
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find("{{") {
        let Some(len) = rest[start..].find("}}") else { break };
        percent(&rest[..start], &mut out);
        out.push_str(&rest[start..start + len + 2]);
        rest = &rest[start + len + 2..];
    }
    percent(rest, &mut out);
    out
}

/// Decodes `%XX` escapes in a `$ref` fragment; `None` when there are none.
fn percent_decode(s: &str) -> Option<String> {
    if !s.contains('%') {
        return None;
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes
            .get(i + 1..i + 3)
            .and_then(|h| std::str::from_utf8(h).ok())
            .filter(|h| h.bytes().all(|b| b.is_ascii_hexdigit()))
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match (bytes[i], hex) {
            (b'%', Some(b)) => {
                out.push(b);
                i += 3;
            }
            (b, _) => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn var<'c>(c: &'c ImportedCollection, key: &str) -> Option<&'c Variable> {
        c.variables.iter().find(|v| v.key == key)
    }

    fn body_json(r: &Request) -> Value {
        serde_json::from_str(&r.body.text).expect("body is JSON")
    }

    const PETSTORE: &str = r##"{
      "openapi": "3.0.3",
      "info": {"title": "Petstore", "version": "1.0"},
      "servers": [{"url": "https://petstore.example.com/v1/"}],
      "tags": [{"name": "store", "description": "Orders"}, {"name": "pets", "description": "Everything about pets"}],
      "security": [{"api_key": []}],
      "paths": {
        "/pets": {
          "get": {
            "tags": ["pets"], "summary": "List  pets\n", "operationId": "listPets",
            "parameters": [
              {"name": "limit", "in": "query", "required": true, "schema": {"type": "integer", "example": 20}},
              {"name": "q", "in": "query", "description": "Search", "schema": {"type": "string"}, "example": "a b&c"},
              {"name": "X-Request-Id", "in": "header", "required": true, "description": "Trace id",
               "schema": {"type": "string", "format": "uuid", "example": "abc"}},
              {"name": "X-Debug", "in": "header", "schema": {"type": "boolean"}},
              {"name": "Accept", "in": "header", "schema": {"type": "string"}},
              {"name": "session", "in": "cookie", "schema": {"type": "string", "default": "s1"}},
              {"name": "theme", "in": "cookie", "schema": {"type": "string", "enum": ["dark"]}}
            ]
          },
          "post": {"tags": ["pets"], "operationId": "createPet", "requestBody": {"$ref": "#/components/requestBodies/PetBody"}}
        },
        "/pets/{petId}": {
          "parameters": [{"$ref": "#/components/parameters/PetId"}, {"name": "v", "in": "query", "required": true, "schema": {"default": 2}}],
          "get": {
            "tags": ["pets"], "description": "Returns a pet", "deprecated": true,
            "parameters": [{"name": "petId", "in": "path", "required": true, "description": "Overridden",
                            "schema": {"type": "string", "enum": ["dog-1", "cat-2"]}}]
          },
          "put": {"tags": ["pets"], "summary": "Update pet", "requestBody": {"content": {"application/json": {"example": {"name": "Rex"}}}}},
          "delete": {"summary": "Delete pet", "security": []}
        },
        "/store/orders": {
          "post": {"tags": ["store"], "summary": "Place order",
                   "requestBody": {"content": {"application/vnd.api+json": {"examples": {"first": {"$ref": "#/components/examples/Order"}}}}}}
        },
        "/health": {"get": {"tags": ["ops"]}},
        "x-internal": {"get": {}}
      },
      "components": {
        "parameters": {"PetId": {"name": "petId", "in": "path", "required": true, "schema": {"type": "integer", "example": 7}}},
        "examples": {"Order": {"value": {"id": 1, "qty": 2}}},
        "requestBodies": {"PetBody": {"content": {
          "application/xml": {"schema": {"$ref": "#/components/schemas/Pet"}},
          "application/json": {"schema": {"$ref": "#/components/schemas/Pet"}}
        }}},
        "schemas": {
          "Pet": {"type": "object", "required": ["name"], "properties": {
            "id": {"type": "integer", "format": "int64", "readOnly": true},
            "name": {"type": "string", "example": "doggie"},
            "born": {"type": "string", "format": "date"},
            "tags": {"type": "array", "items": {"$ref": "#/components/schemas/Tag"}},
            "status": {"type": "string", "enum": ["available", "sold"]},
            "weight": {"type": "number", "nullable": true},
            "owner": {"allOf": [{"$ref": "#/components/schemas/Person"}, {"type": "object", "properties": {"vip": {"type": "boolean"}}}]},
            "home": {"$ref": "#/components/schemas/Home~1Address"}
          }},
          "Tag": {"type": "object", "properties": {"id": {"type": "integer"}, "label": {"type": "string"}}},
          "Person": {"type": "object", "properties": {"email": {"type": "string", "format": "email"}, "id": {"type": "string", "format": "uuid"}}},
          "Home/Address": {"type": "object", "properties": {"site": {"type": "string", "format": "uri"}, "since": {"type": "string", "format": "date-time"}}}
        },
        "securitySchemes": {"api_key": {"type": "apiKey", "name": "X-API-Key", "in": "header"}}
      }
    }"##;

    #[test]
    fn petstore_openapi3() {
        let c = import_openapi(PETSTORE).unwrap();
        assert_eq!(c.name, "Petstore");
        assert!(c.warnings.is_empty(), "{:?}", c.warnings);
        assert_eq!(var(&c, "baseUrl").unwrap().value, "https://petstore.example.com/v1");
        assert!(var(&c, "apiKey").unwrap().secret);
        assert_eq!(
            c.auth,
            Auth::ApiKey { key: "X-API-Key".into(), value: "{{apiKey}}".into(), location: ApiKeyLocation::Header }
        );

        // Folders: declared tag order, then first appearance; untagged at the root.
        let layout: Vec<(String, u32, Vec<String>)> = c
            .items
            .iter()
            .map(|i| match i {
                ImportedItem::Folder { meta, children } => {
                    (meta.name.clone(), meta.seq, requests(children).iter().map(|r| r.name.clone()).collect())
                }
                ImportedItem::Request(r) => (r.name.clone(), r.seq, Vec::new()),
            })
            .collect();
        assert_eq!(
            layout,
            vec![
                ("store".into(), 0, vec!["Place order".into()]),
                (
                    "pets".into(),
                    1,
                    vec!["List pets".into(), "createPet".into(), "GET /pets/{petId}".into(), "Update pet".into()]
                ),
                ("ops".into(), 2, vec!["GET /health".into()]),
                ("Delete pet".into(), 3, vec![]),
            ]
        );
        let ImportedItem::Folder { meta, children } = &c.items[1] else { panic!() };
        assert_eq!(meta.docs, "Everything about pets");
        assert_eq!(requests(children).iter().map(|r| r.seq).collect::<Vec<_>>(), [0, 1, 2, 3]);

        let list = find(&c, "List pets");
        assert_eq!(list.method, "GET");
        assert_eq!(list.url, "{{baseUrl}}/pets?limit=20");
        assert_eq!(
            list.disabled_params,
            vec![KeyValue { key: "q".into(), value: "a b&c".into(), enabled: false, description: "Search".into() }]
        );
        assert_eq!(
            list.headers,
            vec![
                KeyValue {
                    key: "X-Request-Id".into(),
                    value: "abc".into(),
                    enabled: true,
                    description: "Trace id".into()
                },
                KeyValue { key: "X-Debug".into(), value: String::new(), enabled: false, description: String::new() },
                KeyValue {
                    key: "Cookie".into(),
                    value: "session=s1; theme=dark".into(),
                    enabled: false,
                    description: String::new()
                },
            ]
        );
        assert_eq!(list.auth, Auth::Inherit);
        assert_eq!(list.body, Body::default());

        let create = find(&c, "createPet");
        assert_eq!(create.body.body_type, BodyType::Json);
        assert!(create.body.text.starts_with("{\n  \"name\": \"doggie\""), "{}", create.body.text);
        assert_eq!(
            body_json(create),
            serde_json::json!({
                "name": "doggie",
                "born": "2024-01-01",
                "tags": [{"id": 1, "label": "example"}],
                "status": "available",
                "weight": 1.5,
                "owner": {"email": "jane.doe@example.com", "id": "3fa85f64-5717-4562-b3fc-2c963f66afa6", "vip": true},
                "home": {"site": "https://example.com", "since": "2024-01-01T00:00:00Z"}
            })
        );
        assert!(create.headers.is_empty());

        let get = find(&c, "GET /pets/{petId}");
        assert_eq!(get.url, "{{baseUrl}}/pets/{{petId}}?v=2");
        assert!(get.path_params.is_empty());
        assert_eq!(get.docs, "Deprecated.\n\nReturns a pet");
        // The first operation's value wins (the operation-level enum, not the shared example 7).
        assert_eq!(var(&c, "petId").map(|v| (v.value.as_str(), v.secret)), Some(("dog-1", false)));

        let update = find(&c, "Update pet");
        assert_eq!(update.url, "{{baseUrl}}/pets/{{petId}}?v=2");
        assert!(update.path_params.is_empty());
        assert_eq!(update.body.text, "{\n  \"name\": \"Rex\"\n}");

        assert_eq!(find(&c, "Delete pet").auth, Auth::None);
        assert_eq!(body_json(find(&c, "Place order")), serde_json::json!({"id": 1, "qty": 2}));
        assert_eq!(find(&c, "GET /health").url, "{{baseUrl}}/health");
    }

    const WIDGETS_31: &str = r#"
openapi: 3.1.0
info:
  title: Widgets
servers:
  - url: https://{region}.api.example.com/{version}/
    variables:
      region: {default: eu, enum: [eu, us]}
      version: {default: v2}
x-defaults: &defaults
  type: [integer, "null"]
paths:
  /widgets/{id}:
    patch:
      operationId: patchWidget
      parameters:
        - name: id
          in: path
          required: true
          schema: {type: [string, "null"], default: w1}
      requestBody:
        content:
          application/merge-patch+json:
            schema:
              type: object
              properties:
                name: {type: [string, "null"]}
                size:
                  <<: *defaults
                ratio: {type: number}
                when: {type: string, format: date-time}
                meta:
                  type: [object, "null"]
                  properties:
                    active: {type: boolean}
                    kind: {const: gadget}
                    tags: {type: array, examples: [[a, b]]}
                choice:
                  oneOf:
                    - {type: "null"}
                    - {type: string, format: uuid}
  /widgets:
    servers:
      - url: https://uploads.example.com
    post:
      summary: Upload
      requestBody:
        content:
          multipart/form-data:
            schema:
              type: object
              properties:
                file: {type: string, format: binary}
                photos: {type: array, items: {type: string, contentMediaType: image/png}}
                note: {type: string, example: hi}
            encoding:
              file: {contentType: image/png}
    put:
      summary: Form
      requestBody:
        content:
          application/x-www-form-urlencoded:
            schema:
              allOf:
                - type: object
                  properties:
                    a: {type: string, default: x, description: First}
                - properties:
                    b: {type: integer}
            example: {b: 5}
  /raw:
    post:
      summary: Raw
      requestBody:
        content:
          application/octet-stream: {}
    put:
      summary: Csv
      requestBody:
        content:
          text/csv:
            example: "a,b"
    patch:
      summary: Odd
      requestBody:
        content:
          application/x-custom: {}
webhooks:
  newWidget:
    post:
      summary: hook
"#;

    #[test]
    fn openapi31_yaml() {
        let c = import_openapi(WIDGETS_31).unwrap();
        assert_eq!(c.name, "Widgets");
        assert_eq!(var(&c, "baseUrl").unwrap().value, "https://eu.api.example.com/v2");
        assert_eq!(c.auth, Auth::None);
        assert_eq!(c.request_count(), 6, "webhooks are skipped");
        assert_eq!(c.warnings.len(), 1, "{:?}", c.warnings);
        assert!(c.warnings[0].contains("application/x-custom"));

        let patch = find(&c, "patchWidget");
        assert_eq!(patch.method, "PATCH");
        assert_eq!(patch.url, "{{baseUrl}}/widgets/{{widgetId}}");
        assert!(patch.path_params.is_empty());
        assert_eq!(var(&c, "widgetId").unwrap().value, "w1");
        assert_eq!(patch.body.body_type, BodyType::Json);
        assert_eq!(
            body_json(patch),
            serde_json::json!({
                "name": "Jane Doe",
                "size": 1,
                "ratio": 1.5,
                "when": "2024-01-01T00:00:00Z",
                "meta": {"active": true, "kind": "gadget", "tags": ["a", "b"]},
                "choice": "3fa85f64-5717-4562-b3fc-2c963f66afa6"
            })
        );

        let upload = find(&c, "Upload");
        assert_eq!(upload.url, "https://uploads.example.com/widgets", "path-level server override");
        assert_eq!(upload.body.body_type, BodyType::Multipart);
        assert_eq!(
            upload.body.multipart,
            vec![
                MultipartField {
                    key: "file".into(),
                    value: String::new(),
                    file: true,
                    content_type: Some("image/png".into()),
                    enabled: true
                },
                MultipartField {
                    key: "photos".into(),
                    value: String::new(),
                    file: true,
                    content_type: None,
                    enabled: true
                },
                MultipartField {
                    key: "note".into(),
                    value: "hi".into(),
                    file: false,
                    content_type: None,
                    enabled: true
                },
            ]
        );

        let form = find(&c, "Form");
        assert_eq!(form.body.body_type, BodyType::FormUrlencoded);
        assert_eq!(
            form.body.form,
            vec![KeyValue { description: "First".into(), ..KeyValue::new("a", "x") }, KeyValue::new("b", "5")]
        );

        let raw = find(&c, "Raw");
        assert_eq!(raw.body.body_type, BodyType::Binary);
        let csv = find(&c, "Csv");
        assert_eq!(
            (csv.body.body_type, csv.body.content_type.as_deref(), csv.body.text.as_str()),
            (BodyType::Text, Some("text/csv"), "a,b")
        );
        let odd = find(&c, "Odd");
        assert_eq!(
            (odd.body.body_type, odd.body.content_type.as_deref()),
            (BodyType::Text, Some("application/x-custom"))
        );
    }

    const SWAGGER2: &str = r##"{
      "swagger": "2.0",
      "info": {"title": "Legacy"},
      "host": "legacy.example.com",
      "basePath": "/api/",
      "schemes": ["http", "https"],
      "securityDefinitions": {
        "basicAuth": {"type": "basic"},
        "oauth": {"type": "oauth2", "flow": "accessCode", "authorizationUrl": "https://auth.example.com/authorize",
                  "tokenUrl": "https://auth.example.com/token", "scopes": {"read": "Read", "write": "Write"}},
        "robot": {"type": "oauth2", "flow": "application", "tokenUrl": "https://auth.example.com/token", "scopes": {}},
        "key": {"type": "apiKey", "name": "api_key", "in": "query"}
      },
      "security": [{"basicAuth": []}],
      "parameters": {"Limit": {"name": "limit", "in": "query", "type": "integer", "default": 10}},
      "paths": {
        "/users": {
          "get": {"tags": ["users"], "summary": "List users", "parameters": [{"$ref": "#/parameters/Limit"}],
                  "security": [{"key": []}]},
          "post": {"tags": ["users"], "summary": "Create user", "security": [{"oauth": ["write"]}],
                   "parameters": [{"name": "body", "in": "body", "schema": {"$ref": "#/definitions/User"}}]}
        },
        "/users/{id}/avatar": {
          "post": {"summary": "Upload avatar", "consumes": ["multipart/form-data"], "security": [{"robot": []}], "parameters": [
            {"name": "id", "in": "path", "required": true, "type": "string", "x-example": "u1"},
            {"name": "file", "in": "formData", "type": "file", "required": true},
            {"name": "caption", "in": "formData", "type": "string", "default": "me"}]}
        },
        "/login": {
          "post": {"summary": "Login", "parameters": [
            {"name": "user", "in": "formData", "type": "string", "required": true, "description": "Login name"},
            {"name": "remember", "in": "formData", "type": "boolean", "enum": [true, false]}]}
        }
      },
      "definitions": {"User": {"type": "object", "properties": {"name": {"type": "string"}, "age": {"type": "integer"},
                                                                 "roles": {"type": "array", "items": {"type": "string", "enum": ["admin"]}}}}}
    }"##;

    #[test]
    fn swagger2_json() {
        let c = import_openapi(SWAGGER2).unwrap();
        assert_eq!(c.name, "Legacy");
        assert!(c.warnings.is_empty(), "{:?}", c.warnings);
        assert_eq!(var(&c, "baseUrl").unwrap().value, "http://legacy.example.com/api");
        assert_eq!(c.auth, Auth::Basic { username: "{{username}}".into(), password: "{{password}}".into() });
        let keys: Vec<(&str, bool)> = c.variables.iter().map(|v| (v.key.as_str(), v.secret)).collect();
        assert_eq!(
            keys,
            [
                ("baseUrl", false),
                ("username", false),
                ("password", true),
                ("apiKey", true),
                ("clientId", false),
                ("clientSecret", true),
                ("userId", false)
            ]
        );
        assert_eq!(var(&c, "userId").unwrap().value, "u1");

        let list = find(&c, "List users");
        assert_eq!(list.url, "{{baseUrl}}/users");
        assert_eq!(list.disabled_params, vec![KeyValue { enabled: false, ..KeyValue::new("limit", "10") }]);
        assert_eq!(
            list.auth,
            Auth::ApiKey { key: "api_key".into(), value: "{{apiKey}}".into(), location: ApiKeyLocation::Query }
        );

        let create = find(&c, "Create user");
        assert_eq!(create.body.body_type, BodyType::Json);
        assert_eq!(body_json(create), serde_json::json!({"name": "Jane Doe", "age": 30, "roles": ["admin"]}));
        let Auth::OAuth2(cfg) = &create.auth else { panic!("{:?}", create.auth) };
        assert_eq!(cfg.grant_type, GrantType::AuthorizationCode);
        assert_eq!(cfg.auth_url, "https://auth.example.com/authorize");
        assert_eq!(cfg.token_url, "https://auth.example.com/token");
        assert_eq!(cfg.scope, "read write");
        assert_eq!((cfg.client_id.as_str(), cfg.client_secret.as_str()), ("{{clientId}}", "{{clientSecret}}"));

        let avatar = find(&c, "Upload avatar");
        assert_eq!(avatar.url, "{{baseUrl}}/users/{{userId}}/avatar");
        assert!(avatar.path_params.is_empty());
        assert_eq!(avatar.body.body_type, BodyType::Multipart);
        assert_eq!(
            avatar.body.multipart.iter().map(|f| (f.key.as_str(), f.value.as_str(), f.file)).collect::<Vec<_>>(),
            [("file", "", true), ("caption", "me", false)]
        );
        let Auth::OAuth2(cfg) = &avatar.auth else { panic!() };
        assert_eq!(cfg.grant_type, GrantType::ClientCredentials);

        let login = find(&c, "Login");
        assert_eq!(login.auth, Auth::Inherit);
        assert_eq!(login.body.body_type, BodyType::FormUrlencoded);
        assert_eq!(
            login.body.form,
            vec![
                KeyValue { description: "Login name".into(), ..KeyValue::new("user", "") },
                KeyValue::new("remember", "true")
            ]
        );
    }

    #[test]
    fn swagger2_yaml_without_host() {
        let c = import_openapi("swagger: 2.0\ninfo: {title: T}\nbasePath: /v1\npaths:\n  /a:\n    get: {}\n").unwrap();
        assert_eq!(var(&c, "baseUrl").unwrap().value, "/v1");
        assert!(c.warnings.iter().any(|w| w.contains("baseUrl")), "{:?}", c.warnings);
        assert_eq!(find(&c, "GET /a").url, "{{baseUrl}}/a");
    }

    #[test]
    fn recursive_schemas_terminate() {
        let doc = r##"{
          "openapi": "3.0.0",
          "info": {"title": "Tree"},
          "servers": [{"url": "https://x.test"}],
          "paths": {
            "/nodes": {"post": {"summary": "Node", "requestBody": {"content": {"application/json": {"schema": {"$ref": "#/components/schemas/Node"}}}}}},
            "/loop": {"post": {"summary": "Loop", "requestBody": {"content": {"application/json": {"schema": {"$ref": "#/components/schemas/A"}}}}}},
            "/self": {"post": {"summary": "Self", "requestBody": {"content": {"application/json": {"schema": {"$ref": "#/components/schemas/S"}}}}}},
            "/deep": {"post": {"summary": "Deep", "requestBody": {"content": {"application/json": {"schema": {"$ref": "#/components/schemas/D0"}}}}}}
          },
          "components": {"schemas": {
            "Node": {"type": "object", "properties": {
              "name": {"type": "string"},
              "children": {"type": "array", "items": {"$ref": "#/components/schemas/Node"}},
              "parent": {"$ref": "#/components/schemas/Node"},
              "pair": {"$ref": "#/components/schemas/Pair"}}},
            "Pair": {"type": "object", "properties": {"left": {"$ref": "#/components/schemas/Node"}, "n": {"type": "integer"}}},
            "A": {"$ref": "#/components/schemas/B"},
            "B": {"$ref": "#/components/schemas/A"},
            "S": {"allOf": [{"$ref": "#/components/schemas/S"}, {"type": "object", "properties": {"x": {"type": "integer"}}}]},
            "D0": {"type": "object", "properties": {"d": {"$ref": "#/components/schemas/D1"}}},
            "D1": {"type": "object", "properties": {"d": {"$ref": "#/components/schemas/D2"}}},
            "D2": {"type": "object", "properties": {"d": {"$ref": "#/components/schemas/D3"}}},
            "D3": {"type": "object", "properties": {"d": {"$ref": "#/components/schemas/D4"}}},
            "D4": {"type": "object", "properties": {"d": {"$ref": "#/components/schemas/D5"}}},
            "D5": {"type": "object", "properties": {"d": {"$ref": "#/components/schemas/D6"}}},
            "D6": {"type": "object", "properties": {"d": {"$ref": "#/components/schemas/D7"}}},
            "D7": {"type": "object", "properties": {"d": {"type": "string"}}}
          }}
        }"##;
        let c = import_openapi(doc).unwrap();
        assert_eq!(
            body_json(find(&c, "Node")),
            serde_json::json!({"name": "Jane Doe", "children": [], "pair": {"n": 1}})
        );
        assert_eq!(find(&c, "Loop").body.text, "");
        assert!(c.warnings.iter().any(|w| w.contains("loops")), "{:?}", c.warnings);
        assert_eq!(body_json(find(&c, "Self")), serde_json::json!({"x": 1}));
        // Nesting stops at MAX_DEPTH.
        assert_eq!(body_json(find(&c, "Deep")), serde_json::json!({"d": {"d": {"d": {"d": {"d": {"d": {}}}}}}}));
    }

    const SECURITY: &str = r#"
openapi: 3.0.0
info: {title: Sec}
servers: [{url: "https://sec.test"}]
security:
  - bearer: []
components:
  securitySchemes:
    basic: {type: http, scheme: Basic}
    bearer: {type: http, scheme: bearer, bearerFormat: JWT}
    digest: {type: http, scheme: digest}
    key_q: {type: apiKey, in: query, name: token}
    key_c: {type: apiKey, in: cookie, name: sid}
    oauth_cc:
      type: oauth2
      flows:
        authorizationCode: {authorizationUrl: "https://a.test/auth", tokenUrl: "https://a.test/ac-token", scopes: {}}
        clientCredentials: {tokenUrl: "https://a.test/token", scopes: {read: r, write: w}}
    oauth_ac:
      type: oauth2
      flows:
        implicit: {authorizationUrl: "https://a.test/imp", scopes: {}}
        authorizationCode: {authorizationUrl: "https://a.test/auth", tokenUrl: "https://a.test/token", scopes: {openid: o}}
    oauth_pw:
      type: oauth2
      flows:
        password: {tokenUrl: "https://a.test/token", scopes: {}}
    oauth_imp:
      type: oauth2
      flows:
        implicit: {authorizationUrl: "https://a.test/imp", scopes: {}}
    oidc: {type: openIdConnect, openIdConnectUrl: "https://a.test/.well-known/openid-configuration"}
paths:
  /a: {get: {summary: a, security: [{basic: []}]}}
  /b: {get: {summary: b, security: [{key_q: []}]}}
  /c: {get: {summary: c, security: [{key_c: []}]}}
  /d: {get: {summary: d, security: [{oauth_cc: [read]}]}}
  /e: {get: {summary: e, security: [{oauth_ac: []}]}}
  /f: {get: {summary: f, security: [{oauth_pw: []}]}}
  /g: {get: {summary: g, security: [{oauth_imp: []}]}}
  /h: {get: {summary: h, security: [{oidc: []}]}}
  /i: {get: {summary: i, security: [{bearer: []}]}}
  /j: {get: {summary: j, security: []}}
  /k: {get: {summary: k}}
  /l: {get: {summary: l, security: [{}]}}
  /m: {get: {summary: m, security: [{digest: []}, {basic: []}]}}
  /n: {get: {summary: n, security: [{missing: []}]}}
"#;

    #[test]
    fn security_mapping() {
        let c = import_openapi(SECURITY).unwrap();
        assert_eq!(c.auth, Auth::Bearer { token: "{{bearerToken}}".into(), prefix: "Bearer".into() });
        let auth = |name: &str| find(&c, name).auth.clone();
        let basic = Auth::Basic { username: "{{username}}".into(), password: "{{password}}".into() };
        assert_eq!(auth("a"), basic);
        assert_eq!(
            auth("b"),
            Auth::ApiKey { key: "token".into(), value: "{{apiKey}}".into(), location: ApiKeyLocation::Query }
        );
        assert_eq!(auth("c"), Auth::Inherit);

        let Auth::OAuth2(cc) = auth("d") else { panic!() };
        assert_eq!(
            (cc.grant_type, cc.token_url.as_str(), cc.scope.as_str()),
            (GrantType::ClientCredentials, "https://a.test/token", "read write")
        );
        let Auth::OAuth2(ac) = auth("e") else { panic!() };
        assert_eq!(
            (ac.grant_type, ac.auth_url.as_str(), ac.token_url.as_str(), ac.scope.as_str()),
            (GrantType::AuthorizationCode, "https://a.test/auth", "https://a.test/token", "openid")
        );
        assert_eq!((ac.client_id.as_str(), ac.client_secret.as_str()), ("{{clientId}}", "{{clientSecret}}"));
        let Auth::OAuth2(pw) = auth("f") else { panic!() };
        assert_eq!(
            (pw.grant_type, pw.username.as_str(), pw.password.as_str()),
            (GrantType::Password, "{{username}}", "{{password}}")
        );

        let Auth::OAuth2(imp) = auth("g") else { panic!() };
        assert_eq!((imp.grant_type, imp.auth_url.as_str()), (GrantType::Implicit, "https://a.test/imp"));
        assert_eq!(auth("h"), Auth::Inherit);
        assert_eq!(auth("i"), Auth::Inherit, "same as the collection auth");
        assert_eq!(auth("j"), Auth::None);
        assert_eq!(auth("k"), Auth::Inherit);
        assert_eq!(auth("l"), Auth::None);
        assert_eq!(
            auth("m"),
            Auth::Digest { username: "{{username}}".into(), password: "{{password}}".into() },
            "first mappable requirement"
        );
        assert_eq!(auth("n"), Auth::Inherit);

        for needle in ["key_c", "oidc", "\"missing\" is not defined"] {
            assert!(c.warnings.iter().any(|w| w.contains(needle)), "no warning for {needle}: {:?}", c.warnings);
        }
        assert_eq!(c.warnings.len(), 3, "{:?}", c.warnings);
        for (key, secret) in [
            ("bearerToken", true),
            ("username", false),
            ("password", true),
            ("apiKey", true),
            ("clientId", false),
            ("clientSecret", true),
        ] {
            let v = var(&c, key).unwrap_or_else(|| panic!("missing variable {key}"));
            assert_eq!((v.secret, v.enabled, v.value.as_str()), (secret, true, ""), "{key}");
        }
    }

    #[test]
    fn relative_or_missing_server_warns() {
        let c =
            import_openapi(r#"{"openapi": "3.0.1", "servers": [{"url": "/api/v1/"}], "paths": {"/x": {"get": {}}}}"#)
                .unwrap();
        assert_eq!(c.name, "OpenAPI import");
        assert_eq!(var(&c, "baseUrl").unwrap().value, "/api/v1");
        assert!(c.warnings.iter().any(|w| w.contains("relative") && w.contains("baseUrl")), "{:?}", c.warnings);

        let c = import_openapi("openapi: '3.0.0'\ninfo: {title: No servers}\npaths: {}\n").unwrap();
        assert_eq!(var(&c, "baseUrl").unwrap().value, "");
        assert!(c.warnings.iter().any(|w| w.contains("baseUrl")), "{:?}", c.warnings);
        assert!(c.warnings.iter().any(|w| w.contains("no operations")), "{:?}", c.warnings);
    }

    #[test]
    fn external_refs_are_skipped_with_a_warning() {
        let doc = r#"{"openapi": "3.0.0", "servers": [{"url": "https://x.test"}], "paths": {"/p": {"post": {
            "parameters": [{"$ref": "common.yaml#/components/parameters/Page"}],
            "requestBody": {"content": {"application/json": {"schema": {"$ref": "models.json#/Pet"}}}}}}}}"#;
        let c = import_openapi(doc).unwrap();
        let r = find(&c, "POST /p");
        assert_eq!((r.url.as_str(), r.body.body_type, r.body.text.as_str()), ("{{baseUrl}}/p", BodyType::Json, ""));
        assert_eq!(c.warnings.iter().filter(|w| w.contains("External $ref")).count(), 2, "{:?}", c.warnings);
    }

    #[test]
    fn invalid_input_errors() {
        for text in ["", "  \n", "\u{feff}"] {
            assert!(import_openapi(text).unwrap_err().0.contains("empty"));
        }
        assert!(import_openapi("{\"openapi\": ").unwrap_err().0.starts_with("Invalid JSON"));
        assert!(import_openapi("openapi: [3.0\n  bad: {").unwrap_err().0.starts_with("Invalid YAML"));
        for text in [
            r#"{"info": {"title": "x"}}"#,
            "just some text",
            "[1, 2]",
            r#"{"openapi": "4.0"}"#,
            r#"{"swagger": "1.2"}"#,
        ] {
            assert_eq!(import_openapi(text).unwrap_err().0, "Not an OpenAPI/Swagger document", "{text}");
        }
    }

    #[test]
    fn malformed_parts_do_not_panic() {
        let doc = r##"{
          "openapi": 3.1, "info": "nope", "servers": "x", "tags": {"a": 1}, "security": "all",
          "components": {"securitySchemes": [], "schemas": {"S": {"items": 5, "properties": [], "allOf": {}, "type": 7}}},
          "paths": {
            "/a/{}/{b": {"get": {"parameters": {"x": 1}, "tags": "t", "summary": 5, "security": {"k": []}}},
            "/b": {"post": {"parameters": [1, {"in": "query"}, {"name": "n", "in": "query", "required": true, "schema": {"$ref": 3}}],
                            "requestBody": {"content": {"application/json": {"schema": {"$ref": "#/components/schemas/S"}}}}},
                   "put": {"requestBody": {"content": []}}, "patch": "x", "delete": {"security": [1, "x", {}]},
                   "get": {"requestBody": {"content": {"application/json": {"schema": {"type": "array", "items": false}}}}}},
            "/c": [],
            "/d": {"$ref": "#/paths/~1d"}
          }
        }"##;
        let c = import_openapi(doc).unwrap();
        assert_eq!(c.request_count(), 5, "non-object operations and looping path refs are skipped");
        let a = find(&c, "GET /a/{}/{b");
        assert_eq!(a.url, "{{baseUrl}}/a/{}/{b");
        assert_eq!(find(&c, "POST /b").url, "{{baseUrl}}/b?n=");
        assert_eq!(find(&c, "GET /b").body.text, "[]");
        assert_eq!(find(&c, "DELETE /b").auth, Auth::None);
    }

    #[test]
    fn helpers() {
        assert_eq!(encode("a b&c/{{token}}é"), "a%20b%26c%2F{{token}}%C3%A9");
        assert_eq!(encode("{{unclosed"), "%7B%7Bunclosed");
        assert_eq!(convert_path("/a/{id}/b/{name}.{ext}/{id}"), "/a/:id/b/:name.:ext/:id");
        assert_eq!(convert_path("rel/{x"), "/rel/{x");
        let pairs =
            |list: &[(&str, &str)]| list.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect::<Vec<_>>();
        assert_eq!(
            variable_path("/files/{name}.{ext}/{id}/v/{id}"),
            (
                "/files/{{fileName}}.{{ext}}/{{id}}/v/{{id}}".into(),
                pairs(&[("name", "fileName"), ("ext", "ext"), ("id", "id")])
            ),
            "a repeated parameter keeps its first variable; `{{id}}` after a template stays `id`"
        );
        assert_eq!(variable_path("rel/{-}/{}/{x"), ("/rel/{-}/{}/{x".into(), vec![]));
        assert_eq!(variable_path("{id}"), ("/{{id}}".into(), pairs(&[("id", "id")])));

        for (name, words) in [
            ("session_id", &["session", "id"][..]),
            ("session-id", &["session", "id"]),
            ("SessionID", &["session", "id"]),
            ("HTMLParser", &["html", "parser"]),
            ("addressLine1", &["address", "line1"]),
            ("v2Items", &["v2", "items"]),
            ("", &[]),
        ] {
            assert_eq!(name_words(name), words, "{name}");
        }
        assert_eq!(camel_case("Created_AT"), "createdAt");
        for (plural, one) in [("pets", "pet"), ("categories", "category"), ("CATEGORIES", "CATEGORY")] {
            assert_eq!(singular(plural), one);
        }
        for word in ["address", "status", "analysis", "fish"] {
            assert_eq!(singular(word), word);
        }
        for (name, previous, var) in [
            ("session_id", Some("sessions"), "sessionId"),
            ("SessionID", None, "sessionId"),
            ("id", Some("pets"), "petId"),
            ("ID", Some("categories"), "categoryId"),
            ("uuid", Some("user-groups"), "userGroupUuid"),
            ("code", Some("countries"), "countryCode"),
            ("id", None, "id"),
            ("id", Some("{petId}"), "id"),
            ("petId", Some("pets"), "petId"),
        ] {
            assert_eq!(path_variable(name, previous), var, "{name} after {previous:?}");
        }

        assert_eq!(pattern_literal(r"^v1\.0$").as_deref(), Some("v1.0"));
        assert_eq!(pattern_literal("^$").as_deref(), Some(""));
        for pattern in ["v1", "^v1", r"^\d+$", "^a.b$", "^(a|b)$", r"^a\$"] {
            assert_eq!(pattern_literal(pattern), None, "{pattern}");
        }
        assert_eq!(percent_decode("/paths/~1pets~1%7Bid%7D").as_deref(), Some("/paths/~1pets~1{id}"));
        assert_eq!(percent_decode("/a%zz"), Some("/a%zz".into()));
        assert_eq!(normalize_server("//cdn.test/"), "https://cdn.test");
    }

    #[test]
    fn large_spec_imports_quickly() {
        let mut paths = Map::new();
        for i in 0..500 {
            let op = |verb: &str| {
                serde_json::json!({
                    "tags": [format!("tag{}", i % 20)],
                    "summary": format!("{verb} item {i}"),
                    "parameters": [{"$ref": "#/components/parameters/Id"}, {"name": "q", "in": "query", "schema": {"type": "string"}}],
                    "requestBody": {"content": {"application/json": {"schema": {"$ref": "#/components/schemas/Item"}}}}
                })
            };
            paths.insert(
                format!("/items{i}/{{id}}"),
                serde_json::json!({"get": op("get"), "put": op("put"), "post": op("post")}),
            );
        }
        let props: Map<String, Value> = (0..30)
            .map(|p| (format!("field{p}"), serde_json::json!({"$ref": if p % 3 == 0 { "#/components/schemas/Item" } else { "#/components/schemas/Leaf" }})))
            .collect();
        let doc = serde_json::json!({
            "openapi": "3.0.0",
            "info": {"title": "Big"},
            "servers": [{"url": "https://big.test"}],
            "paths": paths,
            "components": {
                "parameters": {"Id": {"name": "id", "in": "path", "required": true, "schema": {"type": "string"}}},
                "schemas": {
                    "Item": {"type": "object", "properties": props},
                    "Leaf": {"type": "object", "properties": {"a": {"type": "string"}, "b": {"type": "array", "items": {"type": "integer"}}}}
                }
            }
        });
        let text = serde_json::to_string(&doc).unwrap();
        let start = std::time::Instant::now();
        let c = import_openapi(&text).unwrap();
        let elapsed = start.elapsed();
        assert_eq!(c.request_count(), 1500);
        assert_eq!(c.items.len(), 20);
        assert!(elapsed < std::time::Duration::from_secs(5), "took {elapsed:?}");
    }

    /// A request body for every media type in `bodies`, each at its own path.
    fn doc_with_bodies(bodies: &[(&str, Value)], schemas: Map<String, Value>) -> String {
        let paths: Map<String, Value> = bodies
            .iter()
            .enumerate()
            .map(|(i, (media, schema))| {
                let op = serde_json::json!({"summary": format!("op{i}"), "requestBody": {"content": {*media: {"schema": schema}}}});
                (format!("/op{i}"), serde_json::json!({"post": op}))
            })
            .collect();
        serde_json::json!({"openapi": "3.0.0", "servers": [{"url": "https://x.test"}], "paths": paths,
            "components": {"schemas": schemas}})
        .to_string()
    }

    #[test]
    fn all_of_and_one_of_fan_out_is_bounded() {
        let mut schemas = Map::new();
        let schema_ref = |name: String| serde_json::json!({"$ref": format!("#/components/schemas/{name}")});
        // Each level lists the next one twice: 2^40 paths through `allOf`, `oneOf` and form fields.
        for i in 0..40 {
            schemas.insert(format!("All{i}"), serde_json::json!({"allOf": [schema_ref(format!("All{}", i + 1)), schema_ref(format!("All{}", i + 1))]}));
            schemas.insert(format!("One{i}"), serde_json::json!({"oneOf": [schema_ref(format!("One{}", i + 1)), schema_ref(format!("One{}", i + 1))]}));
        }
        schemas.insert("All40".into(), serde_json::json!({"type": "object", "properties": {"a": {"type": "string"}}}));
        schemas.insert("One40".into(), serde_json::json!({"not": {}}));
        let parts: Vec<Value> = (0..30).map(|_| schema_ref("Form".into())).collect();
        schemas.insert(
            "Form".into(),
            serde_json::json!({"allOf": parts, "properties": {"f": {"type": "string", "default": "x"}}}),
        );
        let doc = doc_with_bodies(
            &[
                ("application/json", schema_ref("All0".into())),
                ("application/json", schema_ref("One0".into())),
                ("application/x-www-form-urlencoded", schema_ref("Form".into())),
            ],
            schemas,
        );

        let start = std::time::Instant::now();
        let c = import_openapi(&doc).unwrap();
        assert!(start.elapsed() < std::time::Duration::from_secs(5), "took {:?}", start.elapsed());
        assert_eq!(body_json(find(&c, "op0")), serde_json::json!({"a": "example"}));
        assert_eq!(find(&c, "op1").body.text, "");
        assert_eq!(find(&c, "op2").body.form, [KeyValue::new("f", "x")]);
        assert!(c.warnings.is_empty(), "{:?}", c.warnings);
    }

    #[test]
    fn shared_refs_cannot_blow_up_the_output() {
        // Every operation and parameter points at the same 100 kB example.
        let params: Vec<Value> = (0..50)
            .map(|i| serde_json::json!({"name": format!("p{i}"), "in": "query", "schema": {"$ref": "#/components/schemas/Big"}}))
            .collect();
        let mut paths = Map::new();
        paths.insert("/params".into(), serde_json::json!({"get": {"parameters": params}}));
        for i in 0..50 {
            paths.insert(
                format!("/body{i}"),
                serde_json::json!({"post": {"requestBody": {"$ref": "#/components/requestBodies/B"}}}),
            );
        }
        let doc = serde_json::json!({"openapi": "3.0.0", "servers": [{"url": "https://x.test"}], "paths": paths,
            "components": {
                "requestBodies": {"B": {"content": {"application/json": {"schema": {"type": "object", "properties": {
                    "a": {"$ref": "#/components/schemas/Big"}, "b": {"$ref": "#/components/schemas/Big"}}}}}}},
                "schemas": {"Big": {"type": "string", "example": "x".repeat(100_000)}}}});

        let mut importer = Importer::new(&doc, false);
        importer.output_left = 1 << 20;
        let c = importer.run();
        let text: usize = requests(&c.items)
            .iter()
            .map(|r| r.body.text.len() + r.disabled_params.iter().map(|p| p.value.len()).sum::<usize>())
            .sum();
        assert!(text < 1 << 20, "{text} bytes");
        assert!(c.request_count() < 51);
        assert!(c.warnings.iter().any(|w| w.contains("operations were skipped")), "{:?}", c.warnings);

        // The full budget imports everything.
        let c = import_openapi(&doc.to_string()).unwrap();
        assert_eq!(c.request_count(), 51);
        assert!(c.warnings.is_empty(), "{:?}", c.warnings);
        assert_eq!(body_json(find(&c, "POST /body49"))["b"].as_str().map(str::len), Some(100_000));
    }

    #[test]
    fn many_parameters_and_path_names_import_quickly() {
        let n = 20_000;
        let path: String = (0..n).map(|i| format!("/{{p{i}}}")).collect();
        let params: Vec<Value> = (0..n)
            .map(|i| serde_json::json!({"name": format!("p{i}"), "in": "path", "required": true, "example": i}))
            .collect();
        let mut paths = Map::new();
        paths.insert(path, serde_json::json!({"get": {"summary": "many", "parameters": params}}));
        let doc = serde_json::json!({"openapi": "3.0.0", "servers": [{"url": "https://x.test"}], "paths": paths});

        let start = std::time::Instant::now();
        let c = import_openapi(&doc.to_string()).unwrap();
        assert!(start.elapsed() < std::time::Duration::from_secs(5), "took {:?}", start.elapsed());
        let r = find(&c, "many");
        assert!(r.path_params.is_empty());
        assert!(r.url.ends_with(&format!("/{{{{p{}}}}}", n - 1)), "{}", &r.url[r.url.len() - 20..]);
        assert_eq!(c.variables.len(), n + 1);
        assert_eq!(var(&c, &format!("p{}", n - 1)).unwrap().value, (n - 1).to_string());
    }

    #[test]
    fn yaml_alias_bombs_are_refused() {
        // 2,000 aliases of a 1,000-item list: 8 kB of YAML standing for two million values.
        let items = vec!["x"; 1000].join(",");
        let aliases = vec!["*a"; 2000].join(",");
        let yaml = format!("openapi: 3.0.0\nx-a: &a [{items}]\nx-b: [{aliases}]\npaths: {{}}\n");
        let err = import_openapi(&yaml).unwrap_err().0;
        assert!(err.starts_with("Invalid YAML") && err.contains("aliases"), "{err}");

        // A few aliases are fine.
        let yaml = format!("openapi: 3.0.0\nx-a: &a [{items}]\nx-b: [*a, *a]\npaths: {{}}\n");
        assert!(import_openapi(&yaml).is_ok());
    }

    #[test]
    fn declared_tags_are_trimmed_like_operation_tags() {
        let c = import_openapi(
            r#"{"openapi": "3.0.0", "servers": [{"url": "https://x.test"}],
                "tags": [{"name": "b"}, {"name": " a ", "description": "About a"}],
                "paths": {"/x": {"get": {"tags": ["a"]}}, "/y": {"get": {"tags": ["b "]}}}}"#,
        )
        .unwrap();
        let folders: Vec<(&str, &str)> = c
            .items
            .iter()
            .map(|i| match i {
                ImportedItem::Folder { meta, .. } => (meta.name.as_str(), meta.docs.as_str()),
                ImportedItem::Request(r) => (r.name.as_str(), ""),
            })
            .collect();
        assert_eq!(folders, [("b", ""), ("a", "About a")]);
    }

    #[test]
    fn yaml_big_integers_tags_and_duplicate_keys() {
        let yaml = r#"
openapi: 3.0.0
servers: [{url: "https://x.test"}]
paths:
  /n:
    get:
      summary: big
      parameters:
        - {name: n, in: query, required: true, schema: {type: integer, default: 123456789012345678901234567890}}
        - {name: m, in: query, required: true, schema: !Custom {type: integer, default: -123456789012345678901234567890}}
"#;
        let c = import_openapi(yaml).unwrap();
        assert_eq!(find(&c, "big").url, "{{baseUrl}}/n?n=1.2345678901234568e%2B29&m=-1.2345678901234568e%2B29");

        let err = import_openapi("openapi: 3.0.0\npaths: {}\npaths: {}\n").unwrap_err().0;
        assert!(err.starts_with("Invalid YAML: duplicate entry with key \"paths\""), "{err}");
    }

    /// A JSON request body generated from `properties` (YAML flow mappings, one per line).
    fn generated_body(properties: &str) -> Value {
        let doc = format!(
            "openapi: 3.0.3\nservers: [{{url: https://x.test}}]\npaths:\n  /x:\n    post:\n      summary: body\n      \
             requestBody:\n        content:\n          application/json:\n            schema:\n              \
             type: object\n              properties:\n{}\ncomponents:\n  schemas:\n    Name: {{type: string, example: Rex}}\n",
            properties.lines().map(|l| format!("                {}\n", l.trim())).collect::<String>()
        );
        let c = import_openapi(&doc).unwrap();
        assert!(c.warnings.is_empty(), "{:?}", c.warnings);
        body_json(find(&c, "body"))
    }

    #[test]
    fn realistic_values_from_names_and_formats() {
        let body = generated_body(
            r#"email: {type: string}
            contact_email: {type: string, format: email}
            price: {type: integer}
            unitPrice: {type: number}
            amount: {type: string}
            createdAt: {type: string}
            due_date: {type: string, format: date}
            updatedAt: {type: integer}
            userId: {type: integer}
            user_uuid: {type: string}
            orderId: {type: string}
            is_gift: {type: boolean}
            website: {type: string, format: uri}
            avatarUrl: {type: string, format: uri}
            countryCode: {type: string}
            country: {type: string}
            companyName: {type: string}
            weight: {type: number}
            count: {type: integer}
            customer: {type: object, properties: {firstName: {type: string}, last_name: {type: string}, phoneNumber: {type: string}, address: {type: object, properties: {street: {type: string}, city: {type: string}, postalCode: {type: string}}}}}
            emails: {type: array, minItems: 2, items: {type: string}}
            tags: {type: array, items: {type: string}}
            none: {type: array, maxItems: 0, items: {type: string}}
            lines: {type: array, minItems: 2, items: {type: object, properties: {sku: {type: string}, quantity: {type: integer}}}}
            ip: {type: string, format: ipv4}
            host: {type: string, format: hostname}
            start: {type: string, format: time}
            blob: {type: string, format: byte}
            big: {type: string, format: int64}
            inferred: {format: double}"#,
        );
        let line = serde_json::json!({"sku": "example", "quantity": 1});
        assert_eq!(
            body,
            serde_json::json!({
                "email": "jane.doe@example.com",
                "contact_email": "jane.doe@example.com",
                "price": 1999,
                "unitPrice": 19.99,
                "amount": "19.99",
                "createdAt": "2024-01-01T12:00:00Z",
                "due_date": "2024-01-01",
                "updatedAt": 1_704_110_400,
                "userId": 1,
                "user_uuid": EXAMPLE_UUID,
                "orderId": "abc123",
                "is_gift": true,
                "website": "https://example.com",
                "avatarUrl": "https://example.com/image.png",
                "countryCode": "DE",
                "country": "Germany",
                "companyName": "example",
                "weight": 1.5,
                "count": 1,
                "customer": {
                    "firstName": "Jane",
                    "last_name": "Doe",
                    "phoneNumber": "+1 555 0100",
                    "address": {"street": "Alexanderplatz 1", "city": "Berlin", "postalCode": "10115"}
                },
                "emails": ["jane.doe@example.com", "jane.doe@example.com"],
                "tags": ["example"],
                "none": [],
                "lines": [line.clone(), line],
                "ip": "203.0.113.10",
                "host": "api.example.com",
                "start": "12:00:00",
                "blob": "U29tZSBkYXRh",
                "big": "1",
                "inferred": 1.5
            })
        );
    }

    #[test]
    fn realistic_values_respect_constraints() {
        let body = generated_body(
            r#"age: {type: integer, minimum: 40}
            percent: {type: integer, maximum: 10}
            limit: {type: integer, minimum: 1, maximum: 100}
            quantity: {type: integer, minimum: 5, exclusiveMinimum: true}
            score: {type: number, exclusiveMaximum: 4}
            rating: {type: number, minimum: 0, exclusiveMaximum: 3}
            size: {type: integer, minimum: 1, multipleOf: 5}
            price: {type: number, multipleOf: 0.25}
            total: {type: number, minimum: 0.1, maximum: 0.35, multipleOf: 0.1}
            code: {type: string, minLength: 10}
            title: {type: string, maxLength: 7}
            slug: {type: string, minLength: 3, maxLength: 5}
            version: {type: string, pattern: "^v1\\.0$"}
            zip: {type: string, pattern: "^[0-9]{5}$"}"#,
        );
        assert_eq!(
            body,
            serde_json::json!({
                "age": 40,
                "percent": 10,
                "limit": 20,
                "quantity": 6,
                "score": 3,
                "rating": 1.5,
                "size": 5,
                "price": 20,
                "total": 0.3,
                "code": "examplexxx",
                "title": "Example",
                "slug": "examp",
                "version": "v1.0",
                "zip": "10115"
            })
        );
    }

    #[test]
    fn values_given_by_the_spec_win_over_names() {
        let body = generated_body(
            r##"email: {type: string, example: ops@shop.test}
            price: {type: number, default: 5}
            status: {type: string, enum: [pending, paid]}
            country: {const: FR}
            age: {type: integer, example: 3, minimum: 18}
            createdAt: {type: string, examples: ["2020-05-05T00:00:00Z"]}
            name: {$ref: "#/components/schemas/Name"}"##,
        );
        assert_eq!(
            body,
            serde_json::json!({
                "email": "ops@shop.test",
                "price": 5,
                "status": "pending",
                "country": "FR",
                "age": 3,
                "createdAt": "2020-05-05T00:00:00Z",
                "name": "Rex"
            })
        );
    }

    const PATH_VARS: &str = r#"
openapi: 3.0.3
info: {title: Paths}
security: [{basic: []}]
components:
  securitySchemes:
    basic: {type: http, scheme: basic}
paths:
  /sessions/{session_id}:
    get:
      summary: Session
      parameters:
        - {name: session_id, in: path, required: true, schema: {type: string}}
  /pets/{id}:
    parameters:
      - {name: id, in: path, required: true, description: The pet, schema: {type: integer, example: 7}}
    get: {summary: Pet}
    delete: {summary: Delete pet}
  /pets/{pet_id}/toys:
    get:
      summary: Toys
      parameters:
        - {name: pet_id, in: path, required: true, schema: {type: integer, example: 9}}
  /pets/{petId}/photos/{id}:
    get:
      summary: Photo
      parameters:
        - {name: petId, in: path, required: true, schema: {type: integer}}
        - {name: id, in: path, required: true, schema: {type: string, format: uuid}}
  /categories/{id}:
    get: {summary: Category}
  /users/{username}:
    get:
      summary: User
      parameters:
        - {name: username, in: path, required: true, schema: {type: string}}
  /orders/{OrderID}:
    get:
      summary: Order
      parameters:
        - {name: OrderID, in: path, required: true, schema: {type: integer, minimum: 1000}}
"#;

    #[test]
    fn path_parameters_become_environment_variables() {
        let c = import_openapi(PATH_VARS).unwrap();
        let url = |name: &str| {
            let r = find(&c, name);
            assert!(r.path_params.is_empty(), "{name}: {:?}", r.path_params);
            r.url.clone()
        };
        assert_eq!(url("Session"), "{{baseUrl}}/sessions/{{sessionId}}");
        assert_eq!(url("Pet"), "{{baseUrl}}/pets/{{petId}}");
        assert_eq!(url("Delete pet"), "{{baseUrl}}/pets/{{petId}}");
        assert_eq!(url("Toys"), "{{baseUrl}}/pets/{{petId}}/toys");
        assert_eq!(url("Photo"), "{{baseUrl}}/pets/{{petId}}/photos/{{photoId}}");
        assert_eq!(url("Category"), "{{baseUrl}}/categories/{{categoryId}}");
        assert_eq!(url("User"), "{{baseUrl}}/users/{{username}}");
        assert_eq!(url("Order"), "{{baseUrl}}/orders/{{orderId}}");

        // No servers: the variables still go into the imported environment, next to an empty baseUrl.
        assert!(c.warnings.iter().any(|w| w.contains("no server URL")), "{:?}", c.warnings);
        let vars: Vec<(&str, &str, bool)> =
            c.variables.iter().map(|v| (v.key.as_str(), v.value.as_str(), v.secret)).collect();
        assert_eq!(
            vars,
            [
                ("baseUrl", "", false),
                // `username` is also the basic auth placeholder: the path shares it and it stays empty.
                ("username", "", false),
                ("password", "", true),
                ("sessionId", "abc123", false),
                // Shared by `/pets/{id}`, `/pets/{pet_id}/toys` and `/pets/{petId}/photos/{id}`: the first value wins.
                ("petId", "7", false),
                ("photoId", EXAMPLE_UUID, false),
                // Not declared by the operation: a string named like an id.
                ("categoryId", "abc123", false),
                ("orderId", "1000", false),
            ]
        );
        assert!(c.variables.iter().all(|v| v.enabled));
    }

    #[test]
    fn swagger2_path_parameters_become_variables() {
        let c = import_openapi(
            r#"{"swagger": "2.0", "info": {"title": "Old"}, "host": "old.test", "paths": {
                "/stores/{id}/items/{item_id}": {"get": {"summary": "Item", "parameters": [
                    {"name": "id", "in": "path", "required": true, "type": "integer", "minimum": 3},
                    {"name": "item_id", "in": "path", "required": true, "type": "string", "minLength": 8}]}}}}"#,
        )
        .unwrap();
        let item = find(&c, "Item");
        assert_eq!(item.url, "{{baseUrl}}/stores/{{storeId}}/items/{{itemId}}");
        assert!(item.path_params.is_empty());
        let vars: Vec<(&str, &str)> = c.variables.iter().map(|v| (v.key.as_str(), v.value.as_str())).collect();
        assert_eq!(vars, [("baseUrl", "https://old.test"), ("storeId", "3"), ("itemId", "abc123xx")]);
    }

    #[test]
    fn mock_responses_get_realistic_values_and_colon_paths() {
        let (responses, _) = mock_responses(
            r#"{"openapi": "3.0.0", "paths": {"/pets/{id}": {"get": {"responses": {"200": {"content": {
                "application/json": {"schema": {"type": "object", "properties": {"email": {"type": "string"}, "price": {"type": "number"}}}}}}}}}}}"#,
        )
        .unwrap();
        assert_eq!(responses[0].path, "/pets/:id");
        assert_eq!(
            serde_json::from_str::<Value>(&responses[0].body).unwrap(),
            serde_json::json!({"email": "jane.doe@example.com", "price": 19.99})
        );
    }
}

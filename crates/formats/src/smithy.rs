//! Smithy 2.0 JSON AST importer, for the service models AWS publishes
//! (https://github.com/aws/api-models-aws). Each operation of the service becomes a request
//! shaped by the service's protocol (REST JSON or XML, AWS JSON 1.0/1.1, Query, EC2 Query);
//! AWS services are signed with SigV4 from the `awsRegion` and credential variables.
//!
//! Models are untrusted input: shapes are looked up by id, every walk is bounded (cycles,
//! nesting, visits and the total output), and parts that cannot be mapped are skipped with
//! a warning.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use base64::Engine as _;
use indexmap::IndexMap;
use serde_json::{Map, Value};

use crate::import::{ImportError, ImportedCollection, ImportedItem};
use crate::model::{Auth, AwsSigV4Config, Body, BodyType, FolderMeta, KeyValue, Request, RequestKind, Variable};

/// Deepest nesting of generated example values.
const MAX_DEPTH: usize = 10;
/// Deepest nesting walked when serializing a value (examples may nest deeper than generated values).
const MAX_NESTING: usize = 32;
/// Shapes and values visited for one request: shared members can fan out exponentially. AWS's
/// own requests need a few hundred at most.
const MAX_VISITS: usize = 2_000;
/// Rough limit, in bytes, on everything one import generates or scans. Shapes are shared, so
/// a small model can repeat one large shape (or its documentation or default) everywhere.
const MAX_OUTPUT: usize = 256 << 20;
/// What each visit costs against `MAX_OUTPUT`, so visits that produce nothing still count.
const VISIT_COST: usize = 16;
/// Deepest mixin chain followed.
const MAX_MIXINS: usize = 8;
/// Deepest resource nesting followed.
const MAX_RESOURCE_DEPTH: usize = 16;
/// Optional parameters, headers and form fields offered (switched off) per request.
const MAX_OPTIONAL: usize = 60;
/// Optional body members named in a request's docs.
const MAX_OPTIONAL_NAMED: usize = 30;
/// Longest Markdown kept for a request's, folder's or collection's docs.
const MAX_DOCS: usize = 4000;
/// Longest description kept for a parameter, header or form field.
const MAX_DESCRIPTION: usize = 200;
/// Most HTML read for one piece of documentation, and for one description.
const MAX_HTML: usize = 64 * 1024;
const MAX_DESCRIPTION_HTML: usize = 4 * 1024;
/// Most items generated for one list; a larger minimum length is not honoured.
const MAX_ITEMS: u64 = 5;
/// Longest string padded to reach a minimum length.
const MAX_PADDING: u64 = 1024;
/// Most endpoint test cases read.
const MAX_ENDPOINT_TESTS: usize = 10_000;
/// 2024-01-01T12:00:00Z, the example for timestamps.
const EXAMPLE_EPOCH: i64 = 1_704_110_400;
/// Seconds from 0001-01-01 to 9999-12-31 around the Unix epoch: timestamps outside are clamped.
const MIN_EPOCH: i64 = -62_135_596_800;
const MAX_EPOCH: i64 = 253_402_300_799;

const REGION_VAR: &str = "awsRegion";
const ACCESS_KEY_VAR: &str = "awsAccessKeyId";
const SECRET_KEY_VAR: &str = "awsSecretAccessKey";
const SESSION_TOKEN_VAR: &str = "awsSessionToken";
const DEFAULT_REGION: &str = "us-east-1";

const REQUIRED: &str = "smithy.api#required";
const DOCUMENTATION: &str = "smithy.api#documentation";
const XML_NAME: &str = "smithy.api#xmlName";
const XML_NAMESPACE: &str = "smithy.api#xmlNamespace";
const XML_FLATTENED: &str = "smithy.api#xmlFlattened";
const STREAMING: &str = "smithy.api#streaming";

/// Is `doc` a Smithy JSON AST model (a `smithy` version and a `shapes` object)?
pub fn is_smithy(doc: &Value) -> bool {
    doc.get("smithy").is_some_and(Value::is_string) && doc.get("shapes").is_some_and(Value::is_object)
}

/// Import the service of a Smithy 2.0 JSON AST model (an AWS service model, for example).
pub fn import_smithy(text: &str) -> Result<ImportedCollection, ImportError> {
    let text = text.trim_start_matches('\u{feff}');
    if text.trim().is_empty() {
        return Err(ImportError::new("The document is empty"));
    }
    let doc: Value = serde_json::from_str(text).map_err(|e| ImportError::new(format!("Invalid JSON: {e}")))?;
    if !is_smithy(&doc) {
        return Err(ImportError::new("Not a Smithy JSON model"));
    }
    Ok(Importer::new(&doc)?.run())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Protocol {
    RestJson,
    RestXml,
    AwsJson10,
    AwsJson11,
    AwsQuery,
    Ec2Query,
    RpcV2Cbor,
}

/// Protocol traits in the order AWS SDKs prefer them, except that Smithy RPC v2 CBOR (binary
/// bodies, which Zorvik cannot write) comes last.
const PROTOCOLS: [(&str, Protocol); 7] = [
    ("aws.protocols#awsJson1_0", Protocol::AwsJson10),
    ("aws.protocols#awsJson1_1", Protocol::AwsJson11),
    ("aws.protocols#restJson1", Protocol::RestJson),
    ("aws.protocols#restXml", Protocol::RestXml),
    ("aws.protocols#awsQuery", Protocol::AwsQuery),
    ("aws.protocols#ec2Query", Protocol::Ec2Query),
    ("smithy.protocols#rpcv2Cbor", Protocol::RpcV2Cbor),
];

/// AWS services with one endpoint for every region whose host names a region anyway (so the
/// model's endpoint tests cannot tell): endpoint prefix, host, and the region they are signed for.
/// Single endpoints without a region in the host (IAM, CloudFront, Route 53...) are found
/// from the endpoint tests.
const SINGLE_ENDPOINTS: [(&str, &str, &str); 8] = [
    ("organizations", "organizations.us-east-1.amazonaws.com", "us-east-1"),
    ("ce", "ce.us-east-1.amazonaws.com", "us-east-1"),
    ("route53domains", "route53domains.us-east-1.amazonaws.com", "us-east-1"),
    ("globalaccelerator", "globalaccelerator.us-west-2.amazonaws.com", "us-west-2"),
    ("networkmanager", "networkmanager.us-west-2.amazonaws.com", "us-west-2"),
    ("account", "account.us-east-1.amazonaws.com", "us-east-1"),
    ("cur", "cur.us-east-1.amazonaws.com", "us-east-1"),
    ("catalog.marketplace", "catalog.marketplace.us-east-1.amazonaws.com", "us-east-1"),
];

/// Where a value goes, which decides how timestamps, blobs and lists are written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Place {
    /// REST JSON body (honours `jsonName`).
    RestJson,
    /// AWS JSON body.
    Json,
    Xml,
    Query,
    Header,
    /// URL path label or query parameter.
    Url,
}

impl Place {
    fn timestamp_format(self) -> &'static str {
        match self {
            Place::RestJson | Place::Json => "epoch-seconds",
            Place::Header => "http-date",
            Place::Xml | Place::Query | Place::Url => "date-time",
        }
    }
}

static NULL: Value = Value::Null;

/// A shape: its id, its type, and its definition (`null` for prelude shapes).
#[derive(Clone, Copy)]
struct Shape<'a> {
    id: &'a str,
    kind: &'a str,
    def: &'a Value,
}

/// A member (of a structure, union, list or map) with its resolved target.
#[derive(Clone, Copy)]
struct Field<'a> {
    member: &'a Value,
    shape: Shape<'a>,
}

impl<'a> Field<'a> {
    /// A trait of the member, else of its target shape.
    fn trait_value(&self, name: &str) -> Option<&'a Value> {
        trait_of(self.member, name).or_else(|| trait_of(self.shape.def, name))
    }

    fn has(&self, name: &str) -> bool {
        self.trait_value(name).is_some()
    }
}

/// Operations of the service or of one of its resources (a folder).
#[derive(Default)]
struct Group<'a> {
    name: String,
    docs: &'a str,
    operations: Vec<&'a str>,
    groups: Vec<Group<'a>>,
}

struct Importer<'a> {
    shapes: &'a Map<String, Value>,
    service_id: &'a str,
    service: &'a Value,
    protocol: Option<Protocol>,
    /// The SigV4 signing name; empty when requests are not signed with SigV4.
    signing_name: String,
    /// The start of every URL: a host with `{{awsRegion}}`, a single endpoint, or `{{baseUrl}}`.
    endpoint: String,
    /// The region single-endpoint services are signed for.
    signing_region: Option<String>,
    xml_namespace: Option<&'a Value>,
    warnings: Vec<String>,
    warned: HashSet<String>,
    /// Placeholder variables referenced by auth (name -> secret), after the AWS ones.
    vars: IndexMap<&'static str, bool>,
    /// Variables standing for path labels (name -> example value); the first value wins.
    label_vars: IndexMap<String, String>,
    /// The `label_vars` key for each name in ASCII lowercase, to find them case-insensitively.
    label_keys: HashMap<String, String>,
    /// Structures on the current example-generation path (cycle guard).
    active: Vec<&'a str>,
    /// Visits left for the current request.
    visits: usize,
    /// Output left for the rest of the import (see `MAX_OUTPUT`).
    output_left: usize,
    /// Operations whose request leaves something out, summarized in one warning each.
    prefix_headers: IndexMap<String, Vec<String>>,
    query_maps: Vec<String>,
    event_streams: Vec<String>,
    checksums: Vec<String>,
}

impl<'a> Importer<'a> {
    fn new(doc: &'a Value) -> Result<Self, ImportError> {
        let shapes = doc.get("shapes").and_then(Value::as_object).ok_or_else(|| ImportError::new("No shapes"))?;
        let services: Vec<(&'a str, &'a Value)> =
            shapes.iter().filter(|(_, s)| str_of(s, "type") == "service").map(|(id, s)| (id.as_str(), s)).collect();
        // Models bundling helper services name the AWS one with `aws.api#service`; otherwise the
        // service with the most operations and resources is the main one.
        let size = |s: &Value| targets(s, "operations").count() + targets(s, "resources").count();
        let main = services.iter().rev().max_by_key(|(_, s)| size(s));
        let Some(&(service_id, service)) =
            services.iter().find(|(_, s)| trait_of(s, "aws.api#service").is_some()).or(main)
        else {
            return Err(ImportError::new("The Smithy model has no service shape"));
        };
        let mut importer = Self {
            shapes,
            service_id,
            service,
            protocol: None,
            signing_name: String::new(),
            endpoint: String::new(),
            signing_region: None,
            xml_namespace: trait_of(service, XML_NAMESPACE),
            warnings: Vec::new(),
            warned: HashSet::new(),
            vars: IndexMap::new(),
            label_vars: IndexMap::new(),
            label_keys: HashMap::new(),
            active: Vec::new(),
            visits: MAX_VISITS,
            output_left: MAX_OUTPUT,
            prefix_headers: IndexMap::new(),
            query_maps: Vec::new(),
            event_streams: Vec::new(),
            checksums: Vec::new(),
        };
        if services.len() > 1 {
            importer.warn(format!(
                "The model has {} services; only {} was imported",
                services.len(),
                local_name(service_id)
            ));
        }
        Ok(importer)
    }

    fn run(mut self) -> ImportedCollection {
        let service = self.service;
        let aws = trait_of(service, "aws.api#service");
        let name = [trait_of(service, "smithy.api#title"), aws.and_then(|a| a.get("sdkId"))]
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
            .find(|s| !s.is_empty())
            .unwrap_or_else(|| local_name(self.service_id).to_string());

        self.protocol = self.pick_protocol();
        let mut auth = self.service_auth();
        self.endpoint(aws);
        if let Auth::AwsSigV4(config) = &mut auth {
            config.region = self.signing_region.clone().unwrap_or_else(|| format!("{{{{{REGION_VAR}}}}}"));
        }

        // Folders first (resources), then the service's own operations.
        let mut root = self.groups();
        let items = self.items(&mut root);
        if items.is_empty() {
            self.warn("The model's service has no operations".to_string());
        }
        self.summarize();

        let docs = self.markdown(trait_str(service, DOCUMENTATION));
        let mut variables = Vec::new();
        if self.endpoint.starts_with("{{baseUrl}}") {
            variables.push(variable("baseUrl", "", false));
        }
        // Single-endpoint services don't use the region; it is there for the other AWS services.
        if self.endpoint.contains(REGION_VAR) || matches!(auth, Auth::AwsSigV4(_)) {
            variables.push(variable(REGION_VAR, DEFAULT_REGION, false));
        }
        if matches!(auth, Auth::AwsSigV4(_)) {
            variables.push(variable(ACCESS_KEY_VAR, "", true));
            variables.push(variable(SECRET_KEY_VAR, "", true));
            variables.push(variable(SESSION_TOKEN_VAR, "", true));
        }
        variables.extend(self.vars.iter().map(|(key, secret)| variable(key, "", *secret)));
        // A label named like one of the variables above shares it.
        let taken = variables.len();
        for (key, value) in self.label_vars {
            if !variables[..taken].iter().any(|v| v.key == key) {
                variables.push(Variable { key, value, enabled: true, secret: false });
            }
        }
        ImportedCollection { name, variables, auth, items, warnings: self.warnings, docs, ..Default::default() }
    }

    fn warn(&mut self, message: String) {
        if !self.warned.contains(&message) && self.afford(message.len()) {
            self.warned.insert(message.clone());
            self.warnings.push(message);
        }
    }

    /// One warning per kind of thing left out, naming the operations.
    fn summarize(&mut self) {
        let list = |ops: &[String]| {
            let mut names: Vec<&str> = ops.iter().map(String::as_str).take(5).collect();
            if ops.len() > names.len() {
                names.push("…");
            }
            names.join(", ")
        };
        let mut messages = Vec::new();
        for (prefix, ops) in &self.prefix_headers {
            messages.push(format!(
                "Headers starting with \"{prefix}\" were not added ({}); add them by hand when needed",
                list(ops)
            ));
        }
        if !self.query_maps.is_empty() {
            messages.push(format!(
                "Free-form query parameters were not added ({}); add them by hand when needed",
                list(&self.query_maps)
            ));
        }
        if !self.event_streams.is_empty() {
            messages.push(format!(
                "Event streams are not supported; these requests were created without their input stream: {}",
                list(&self.event_streams)
            ));
        }
        if !self.checksums.is_empty() {
            messages.push(format!(
                "These operations need a Content-MD5 or x-amz-checksum-* header, which is not added for you: {}",
                list(&self.checksums)
            ));
        }
        for message in messages {
            self.warn(message);
        }
        if self.output_left == 0 {
            self.warnings.push(format!(
                "The model expands to more than {} MB of requests (shapes repeat the same content); \
                 examples were cut short and some operations were skipped",
                MAX_OUTPUT >> 20
            ));
        }
    }

    /// Charges `bytes` against the import's output; once it is used up, every charge fails.
    fn afford(&mut self, bytes: usize) -> bool {
        let ok = bytes < self.output_left;
        self.output_left = if ok { self.output_left - bytes } else { 0 };
        ok
    }

    /// Counts one visit for the current request; false once the limits are reached.
    fn visit(&mut self) -> bool {
        let ok = self.visits > 0 && self.afford(VISIT_COST);
        self.visits = self.visits.saturating_sub(1);
        ok
    }

    /// A copy of a model value that goes into the collection, if it fits the output.
    fn copy(&mut self, v: &Value) -> Option<Value> {
        let len = json_len(v, self.output_left);
        self.afford(len).then(|| v.clone())
    }

    fn markdown(&mut self, html: &str) -> String {
        let html = cut(html, MAX_HTML);
        if !self.afford(html.len()) {
            return String::new();
        }
        html_to_markdown(html, MAX_DOCS)
    }

    fn description(&mut self, field: Option<Field<'a>>) -> String {
        let html = field.and_then(|f| f.trait_value(DOCUMENTATION)).and_then(Value::as_str).unwrap_or("");
        let html = cut(html, MAX_DESCRIPTION_HTML);
        if !self.afford(html.len()) {
            return String::new();
        }
        // Only the first sentence is kept, so the text after it is not converted.
        first_sentence(&html_to_text(html, MAX_DESCRIPTION * 2), MAX_DESCRIPTION)
    }

    // --- Shapes ---

    fn shape(&mut self, id: &'a str) -> Option<Shape<'a>> {
        if let Some(def) = self.shapes.get(id) {
            return match def.get("type").and_then(Value::as_str) {
                Some(kind) => Some(Shape { id, kind, def }),
                None => {
                    self.warn(format!("Shape \"{id}\" has no type and was skipped"));
                    None
                }
            };
        }
        match prelude(id) {
            Some(kind) => Some(Shape { id, kind, def: &NULL }),
            None => {
                self.warn(format!("Shape \"{id}\" is not defined; members that use it were left out"));
                None
            }
        }
    }

    fn field(&mut self, member: &'a Value) -> Option<Field<'a>> {
        let target = member.get("target").and_then(Value::as_str)?;
        Some(Field { member, shape: self.shape(target)? })
    }

    /// A structure's or union's members in declaration order, its mixins' first.
    fn members(&mut self, shape: Shape<'a>) -> IndexMap<&'a str, &'a Value> {
        let mut out = IndexMap::new();
        let mut seen = HashSet::new();
        self.collect_members(shape.def, &mut out, &mut seen, 0);
        out
    }

    fn collect_members(
        &mut self,
        def: &'a Value,
        out: &mut IndexMap<&'a str, &'a Value>,
        seen: &mut HashSet<&'a str>,
        depth: usize,
    ) {
        if depth > MAX_MIXINS {
            return;
        }
        for mixin in def.get("mixins").and_then(Value::as_array).into_iter().flatten() {
            if let Some(id) = mixin.get("target").and_then(Value::as_str)
                && seen.insert(id)
                && let Some(mixin) = self.shapes.get(id)
            {
                self.collect_members(mixin, out, seen, depth + 1);
            }
        }
        let members = def.get("members").and_then(Value::as_object);
        // Scanning members is work too: a huge shared shape must not be scanned without limit.
        if !self.afford(members.map_or(0, Map::len)) {
            return;
        }
        for (name, member) in members.into_iter().flatten() {
            if member.is_object() {
                out.insert(name.as_str(), member);
            }
        }
    }

    /// One member of a structure or union, looked up by name.
    fn member(&mut self, shape: Shape<'a>, name: &str) -> Option<&'a Value> {
        if let Some(m) = shape.def.get("members").and_then(|m| m.get(name)).filter(|m| m.is_object()) {
            return Some(m);
        }
        if shape.def.get("mixins").is_some() { self.members(shape).get(name).copied() } else { None }
    }

    fn pick_protocol(&mut self) -> Option<Protocol> {
        let picked = PROTOCOLS.iter().find(|(name, _)| trait_of(self.service, name).is_some()).map(|(_, p)| *p);
        match picked {
            Some(Protocol::RpcV2Cbor) => self.warn(
                "The service speaks Smithy RPC v2 CBOR, whose binary bodies Zorvik cannot write; \
                 the requests show their input as JSON for reference"
                    .to_string(),
            ),
            None => self.warn(
                "The model names no supported protocol; requests follow its HTTP bindings as REST JSON".to_string(),
            ),
            _ => {}
        }
        picked
    }

    /// Collection auth: SigV4 for AWS services, else the first scheme of the model that maps.
    fn service_auth(&mut self) -> Auth {
        let service = self.service;
        let declared: Vec<&str> = match trait_of(service, "smithy.api#auth").and_then(Value::as_array) {
            Some(order) => order.iter().filter_map(Value::as_str).collect(),
            None => traits(service).map(|t| t.keys().map(String::as_str).collect()).unwrap_or_default(),
        };
        for scheme in declared {
            let Some(config) = trait_of(service, scheme) else { continue };
            match scheme {
                "aws.auth#sigv4" => {
                    self.signing_name = str_of(config, "name").trim().to_string();
                    if self.signing_name.is_empty() {
                        continue;
                    }
                    return Auth::AwsSigV4(AwsSigV4Config {
                        access_key: format!("{{{{{ACCESS_KEY_VAR}}}}}"),
                        secret_key: format!("{{{{{SECRET_KEY_VAR}}}}}"),
                        session_token: format!("{{{{{SESSION_TOKEN_VAR}}}}}"),
                        // Set once the endpoint is known (single endpoints sign for their own region).
                        region: String::new(),
                        service: self.signing_name.clone(),
                        ..Default::default()
                    });
                }
                "smithy.api#httpBearerAuth" => {
                    self.vars.entry("bearerToken").or_insert(true);
                    return Auth::Bearer { token: "{{bearerToken}}".to_string(), prefix: "Bearer".to_string() };
                }
                "smithy.api#httpBasicAuth" => {
                    self.vars.entry("username").or_insert(false);
                    self.vars.entry("password").or_insert(true);
                    return Auth::Basic { username: "{{username}}".to_string(), password: "{{password}}".to_string() };
                }
                "smithy.api#httpApiKeyAuth" => {
                    // Names from the model: a header name must be one, and braces are dropped.
                    let name = str_of(config, "name").trim();
                    let (location, key) = match str_of(config, "in") {
                        "header" if is_token(name) => (crate::model::ApiKeyLocation::Header, name.to_string()),
                        "query" if !plain(name).is_empty() => (crate::model::ApiKeyLocation::Query, plain(name)),
                        _ => continue,
                    };
                    self.vars.entry("apiKey").or_insert(true);
                    let value = match plain(str_of(config, "scheme")).trim() {
                        "" => "{{apiKey}}".to_string(),
                        scheme => format!("{scheme} {{{{apiKey}}}}"),
                    };
                    return Auth::ApiKey { key, value, location };
                }
                _ => {}
            }
        }
        Auth::None
    }

    /// Sets where requests go, and fixes the SigV4 region of single-endpoint services.
    fn endpoint(&mut self, aws: Option<&'a Value>) {
        let Some(aws) = aws else {
            self.endpoint = "{{baseUrl}}".to_string();
            self.warn("The model doesn't say where the API runs; set the baseUrl variable to its address".to_string());
            return;
        };
        let prefix = [str_of(aws, "endpointPrefix"), self.signing_name.as_str(), str_of(aws, "arnNamespace")]
            .into_iter()
            .map(str::trim)
            .find(|p| is_host(p))
            .map(str::to_string)
            .unwrap_or_else(|| {
                let sdk = str_of(aws, "sdkId").to_ascii_lowercase().replace(' ', "");
                if is_host(&sdk) { sdk } else { "service".to_string() }
            });
        let tests = trait_of(self.service, "smithy.rules#endpointTests");
        let (endpoint, region) = aws_endpoint(&prefix, tests);
        self.endpoint = endpoint;
        self.signing_region = region;
    }

    // --- Operations and resources ---

    /// The service's operations, and its resources' as nested groups.
    fn groups(&mut self) -> Group<'a> {
        let service = self.service;
        let mut seen_ops = HashSet::new();
        let mut seen_resources = HashSet::new();
        let mut root = Group::default();
        for id in targets(service, "operations") {
            if seen_ops.insert(id) {
                root.operations.push(id);
            }
        }
        for id in targets(service, "resources") {
            if let Some(group) = self.resource(id, &mut seen_ops, &mut seen_resources, 0) {
                root.groups.push(group);
            }
        }
        root
    }

    fn resource(
        &mut self,
        id: &'a str,
        seen_ops: &mut HashSet<&'a str>,
        seen_resources: &mut HashSet<&'a str>,
        depth: usize,
    ) -> Option<Group<'a>> {
        if depth >= MAX_RESOURCE_DEPTH || !seen_resources.insert(id) || !self.afford(VISIT_COST) {
            return None;
        }
        let shapes = self.shapes;
        let def = shapes.get(id).filter(|r| str_of(r, "type") == "resource")?;
        let name = local_name(id);
        let name = match name.strip_suffix("Resource") {
            Some(short) if !short.is_empty() => short,
            _ => name,
        };
        let mut group = Group { name: name.to_string(), docs: trait_str(def, DOCUMENTATION), ..Group::default() };
        let lifecycle = ["create", "put", "read", "update", "delete", "list"]
            .into_iter()
            .filter_map(|key| def.get(key)?.get("target")?.as_str());
        let bound = targets(def, "operations").chain(targets(def, "collectionOperations"));
        for op in lifecycle.chain(bound) {
            if seen_ops.insert(op) {
                group.operations.push(op);
            }
        }
        for child in targets(def, "resources") {
            if let Some(child) = self.resource(child, seen_ops, seen_resources, depth + 1) {
                group.groups.push(child);
            }
        }
        Some(group)
    }

    /// A group's folders (by name) and requests (by operation name), empty folders left out.
    fn items(&mut self, group: &mut Group<'a>) -> Vec<ImportedItem> {
        let mut items = Vec::new();
        group.groups.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then(a.name.cmp(&b.name)));
        let mut names = HashSet::new();
        // The last number given to each name, so many folders sharing one stay linear.
        let mut numbered: HashMap<String, usize> = HashMap::new();
        for child in &mut group.groups {
            let children = self.items(child);
            if children.is_empty() {
                continue;
            }
            // Resources from different namespaces may share a name.
            let mut name = child.name.clone();
            if !names.insert(name.clone()) {
                let n = numbered.entry(child.name.clone()).or_insert(1);
                while !names.insert(name.clone()) {
                    *n += 1;
                    name = format!("{} {n}", child.name);
                }
            }
            let docs = self.markdown(child.docs);
            let meta = FolderMeta { name, seq: items.len() as u32, docs, ..FolderMeta::default() };
            items.push(ImportedItem::Folder { meta, children });
        }
        group.operations.sort_by_cached_key(|id| (local_name(id).to_lowercase(), *id));
        let shapes = self.shapes;
        for &id in &group.operations {
            let Some(op) = shapes.get(id).filter(|o| str_of(o, "type") == "operation") else {
                let problem = if shapes.contains_key(id) { "is not an operation" } else { "is not defined" };
                self.warn(format!("Operation \"{id}\" {problem} and was skipped"));
                continue;
            };
            if self.output_left == 0 {
                break;
            }
            let before = self.output_left;
            let mut request = self.request(id, op);
            request.seq = items.len() as u32;
            // Values were charged while they were built; charge whatever else the request holds.
            let size = serde_json::to_vec(&request).map_or(0, |json| json.len());
            self.afford(size.saturating_sub(before - self.output_left));
            items.push(ImportedItem::Request(request));
        }
        items
    }

    fn request(&mut self, id: &'a str, op: &'a Value) -> Request {
        let name = local_name(id);
        self.visits = MAX_VISITS;
        let mut req = Request::new(name, RequestKind::Http);
        let input = match op.get("input").and_then(|i| i.get("target")).and_then(Value::as_str) {
            Some(target) => self.shape(target).filter(|s| matches!(s.kind, "structure" | "union")),
            None => None,
        };
        let members = input.map(|s| self.members(s)).unwrap_or_default();
        let example = trait_of(op, "smithy.api#examples")
            .and_then(Value::as_array)
            .and_then(|e| e.iter().find_map(|e| e.get("input")?.as_object()));
        // An example's page token only works for the call it was taken from.
        let token = [op, self.service]
            .into_iter()
            .find_map(|v| trait_of(v, "smithy.api#paginated")?.get("inputToken")?.as_str());

        // Values for the members the request sends: the example's, else generated for required ones.
        let mut values: IndexMap<&'a str, Value> = IndexMap::new();
        let mut optional: Vec<&'a str> = Vec::new();
        let mut fields: IndexMap<&'a str, Field<'a>> = IndexMap::new();
        // The input is on the generation path too, so members leading back to it stop.
        self.active = input.map(|s| s.id).into_iter().collect();
        for (&member_name, &member) in &members {
            let Some(field) = self.field(member) else { continue };
            if field.shape.kind == "union" && field.has(STREAMING) {
                self.event_streams.push(name.to_string());
                continue;
            }
            fields.insert(member_name, field);
            let value = match example.and_then(|e| e.get(member_name)).filter(|_| token != Some(member_name)) {
                Some(given) => self.copy(given),
                None if has_trait(member, REQUIRED) => self.member_value(member_name, field, 0),
                None => {
                    optional.push(member_name);
                    None
                }
            };
            if let Some(value) = value {
                values.insert(member_name, value);
            }
        }

        let mut optional_body = Vec::new();
        match self.protocol.unwrap_or(Protocol::RestJson) {
            Protocol::RestJson | Protocol::RestXml => {
                let xml = self.protocol == Some(Protocol::RestXml);
                optional_body = self.rest(&mut req, op, input, &fields, &values, &optional, xml);
            }
            protocol @ (Protocol::AwsJson10 | Protocol::AwsJson11) => {
                let version = if protocol == Protocol::AwsJson10 { "1.0" } else { "1.1" };
                req.method = "POST".to_string();
                req.url = self.host(op, &fields, &values) + "/";
                let target = format!("{}.{name}", local_name(self.service_id));
                req.headers.push(KeyValue::new("X-Amz-Target", plain(&target)));
                req.headers.push(KeyValue::new("Content-Type", format!("application/x-amz-json-{version}")));
                req.body = self.json_body(&fields, &values, Place::Json);
                optional_body = optional.iter().map(|n| n.to_string()).collect();
            }
            protocol @ (Protocol::AwsQuery | Protocol::Ec2Query) => {
                req.method = "POST".to_string();
                req.url = self.host(op, &fields, &values) + "/";
                req.body = self.form_body(&plain(name), &fields, &values, protocol == Protocol::Ec2Query);
            }
            Protocol::RpcV2Cbor => {
                req.method = "POST".to_string();
                let (service, operation) = (plain(local_name(self.service_id)), plain(name));
                req.url = format!("{}/service/{service}/operation/{operation}", self.host(op, &fields, &values));
                req.headers.push(KeyValue::new("smithy-protocol", "rpc-v2-cbor"));
                req.headers.push(KeyValue::new("Accept", "application/cbor"));
                req.body = self.json_body(&fields, &values, Place::Json);
                optional_body = optional.iter().map(|n| n.to_string()).collect();
            }
        }

        if trait_of(op, "smithy.api#auth").and_then(Value::as_array).is_some_and(Vec::is_empty) {
            req.auth = Auth::None;
        }
        let checksum = trait_of(op, "aws.protocols#httpChecksum")
            .is_some_and(|c| c.get("requestChecksumRequired") == Some(&Value::Bool(true)));
        if checksum || trait_of(op, "smithy.api#httpChecksumRequired").is_some() {
            self.checksums.push(name.to_string());
        }
        req.docs = self.operation_docs(op, &optional_body);
        req
    }

    fn operation_docs(&mut self, op: &'a Value, optional_body: &[String]) -> String {
        let mut docs = self.markdown(trait_str(op, DOCUMENTATION));
        if trait_of(op, "smithy.api#deprecated").is_some() {
            docs = if docs.is_empty() { "Deprecated.".to_string() } else { format!("Deprecated.\n\n{docs}") };
        }
        if !optional_body.is_empty() {
            let mut names: Vec<String> =
                optional_body.iter().take(MAX_OPTIONAL_NAMED).map(|n| format!("`{}`", n.replace('`', "'"))).collect();
            if optional_body.len() > MAX_OPTIONAL_NAMED {
                names.push(format!("and {} more", optional_body.len() - MAX_OPTIONAL_NAMED));
            }
            let line = format!("Optional body members: {}.", names.join(", "));
            docs = if docs.is_empty() { line } else { format!("{docs}\n\n{line}") };
        }
        docs
    }

    /// The scheme and host of an operation's URL, with its `hostPrefix` (labels as variables).
    fn host(
        &mut self,
        op: &'a Value,
        fields: &IndexMap<&'a str, Field<'a>>,
        values: &IndexMap<&'a str, Value>,
    ) -> String {
        let endpoint = self.endpoint.clone();
        let Some(prefix) = trait_of(op, "smithy.api#endpoint").map(|e| str_of(e, "hostPrefix")) else {
            return endpoint;
        };
        let Some(rest) = endpoint.strip_prefix("https://") else {
            return endpoint;
        };
        let mut out = String::new();
        for (literal, label) in templates(prefix) {
            // Only host characters: the prefix comes from the model.
            if !literal.chars().all(|c| c.is_ascii_alphanumeric() || "-.".contains(c)) {
                return endpoint;
            }
            out.push_str(literal);
            if let Some(label) = label {
                let label = label.trim_end_matches('+');
                match fields.get(label) {
                    Some(&field) => {
                        let var = self.label_var(label, None, field, values.get(label));
                        let _ = write!(out, "{{{{{var}}}}}");
                    }
                    None => {
                        let _ = write!(out, "{{{label}}}");
                    }
                }
            }
        }
        format!("https://{out}{rest}")
    }

    /// The variable standing for a path or host label, registered with an example value.
    fn label_var(&mut self, name: &str, previous: Option<&str>, field: Field<'a>, given: Option<&Value>) -> String {
        let var = label_variable(name, previous);
        // `/hostedzone/{Id}` and `{HostedZoneId}` share `hostedZoneId`.
        let var = self.label_keys.entry(var.to_ascii_lowercase()).or_insert(var).clone();
        if !self.label_vars.contains_key(&var) {
            let value = match given {
                Some(v) => self.text(field, v, Place::Url),
                None => match self.member_value(name, field, 0) {
                    Some(v) => self.text(field, &v, Place::Url),
                    None => String::new(),
                },
            };
            self.label_vars.insert(var.clone(), value);
        }
        var
    }

    // --- REST protocols ---

    /// Fills a REST JSON or REST XML request; returns the optional body members left out.
    #[allow(clippy::too_many_arguments)]
    fn rest(
        &mut self,
        req: &mut Request,
        op: &'a Value,
        input: Option<Shape<'a>>,
        fields: &IndexMap<&'a str, Field<'a>>,
        values: &IndexMap<&'a str, Value>,
        optional: &[&'a str],
        xml: bool,
    ) -> Vec<String> {
        let name = req.name.clone();
        let http = trait_of(op, "smithy.api#http");
        let method = http.map(|h| str_of(h, "method").trim().to_ascii_uppercase()).unwrap_or_default();
        let uri = http.map(|h| str_of(h, "uri").trim()).unwrap_or_default();
        let valid_method = !method.is_empty() && method.bytes().all(|b| b.is_ascii_alphabetic());
        // A URI from the model must not reference variables (`{{awsSecretAccessKey}}`).
        let uri = if uri.contains("{{") || uri.contains("}}") { "" } else { uri };
        if http.is_none() || !valid_method || !uri.starts_with('/') {
            self.warn(format!("{name}: the operation has no valid HTTP binding; the request was created as POST /"));
        }
        req.method = if valid_method { method } else { "POST".to_string() };
        let uri = if uri.starts_with('/') { uri } else { "/" };
        let (path, literal_query) = uri.split_once('?').unwrap_or((uri, ""));

        let mut url = self.host(op, fields, values);
        let mut previous = None;
        for (literal, label) in templates(path) {
            url.push_str(literal);
            if let Some(segment) = literal.rsplit('/').nth(1).filter(|s| !s.is_empty()) {
                previous = Some(segment);
            } else if !literal.ends_with('/') && !literal.is_empty() {
                previous = None;
            }
            let Some(label) = label else { continue };
            let member = label.trim_end_matches('+');
            match fields.get(member).filter(|f| has_trait(f.member, "smithy.api#httpLabel")) {
                Some(&field) => {
                    let var = self.label_var(member, previous, field, values.get(member));
                    let _ = write!(url, "{{{{{var}}}}}");
                }
                None => {
                    let _ = write!(url, "{{{label}}}");
                }
            }
            previous = None;
        }
        let mut query: Vec<String> = literal_query.split('&').filter(|p| !p.is_empty()).map(str::to_string).collect();

        let mut payload = None;
        let mut body_fields = IndexMap::new();
        let mut optional_body = Vec::new();
        let mut offered = 0;
        for (&member, &field) in fields {
            let value = values.get(member);
            // Optional query parameters and headers are modifiers (`Range`, `If-Match`...): the
            // example's values fill them in, switched off.
            let wanted = value.is_some() && has_trait(field.member, REQUIRED);
            let m = field.member;
            if has_trait(m, "smithy.api#httpLabel") {
                continue;
            }
            if let Some(key) = trait_of(m, "smithy.api#httpQuery").and_then(Value::as_str) {
                if wanted || offered < MAX_OPTIONAL {
                    let value = match value {
                        Some(v) => Some(v.clone()),
                        None => self.member_value(member, field, 0),
                    };
                    let Some(value) = value else { continue };
                    let description = self.description(Some(field));
                    for text in self.texts(field, &value, Place::Url) {
                        if wanted {
                            query.push(format!("{}={}", encode(&plain(key)), encode(&text)));
                        } else {
                            req.disabled_params.push(row(&plain(key), text, false, description.clone()));
                            offered += 1;
                            break;
                        }
                    }
                    if wanted && !description.is_empty() {
                        req.param_descriptions.push(row(&plain(key), String::new(), true, description));
                    }
                }
            } else if has_trait(m, "smithy.api#httpQueryParams") {
                if self.query_maps.last() != Some(&name) {
                    self.query_maps.push(name.clone());
                }
            } else if let Some(header) = trait_of(m, "smithy.api#httpHeader").and_then(Value::as_str) {
                if !is_token(header) {
                    self.warn(format!("{name}: the header name \"{}\" is not valid and was skipped", plain(header)));
                } else if wanted || offered < MAX_OPTIONAL {
                    let value = match value {
                        Some(v) => Some(v.clone()),
                        None => self.member_value(member, field, 0),
                    };
                    let Some(value) = value else { continue };
                    let text = self.header_text(field, &value);
                    let description = self.description(Some(field));
                    req.headers.push(row(header, text, wanted, description));
                    offered += usize::from(!wanted);
                }
            } else if let Some(prefix) = trait_of(m, "smithy.api#httpPrefixHeaders").and_then(Value::as_str) {
                let ops = self.prefix_headers.entry(prefix.to_string()).or_default();
                if ops.last() != Some(&name) {
                    ops.push(name.clone());
                }
            } else if has_trait(m, "smithy.api#httpPayload") {
                payload = Some((member, field));
            } else {
                body_fields.insert(member, field);
                if optional.contains(&member) {
                    optional_body.push(self.body_name(member, field, xml));
                }
            }
        }
        if !query.is_empty() {
            url.push('?');
            url.push_str(&query.join("&"));
        }
        req.url = url;

        req.body = match (payload, input) {
            (Some((member, field)), _) => self.payload(field, values.get(member), xml),
            (None, Some(input)) if xml && !body_fields.is_empty() => {
                // The input structure is the root element.
                let root = trait_of(input.def, XML_NAME).and_then(Value::as_str).unwrap_or(local_name(input.id));
                let ns = trait_of(input.def, XML_NAMESPACE).or(self.xml_namespace);
                let members: Vec<(&str, Field<'a>, &Value)> =
                    body_fields.iter().filter_map(|(&name, &field)| Some((name, field, values.get(name)?))).collect();
                let mut out = String::new();
                self.xml_struct(&mut out, 0, root, ns, &members, 0);
                Body { body_type: BodyType::Xml, text: out, ..Body::default() }
            }
            _ if body_fields.is_empty() => Body::default(),
            _ => self.json_body(&body_fields, values, Place::RestJson),
        };
        optional_body
    }

    fn body_name(&self, member: &str, field: Field<'a>, xml: bool) -> String {
        let renamed =
            if xml { trait_of(field.member, XML_NAME) } else { trait_of(field.member, "smithy.api#jsonName") };
        renamed.and_then(Value::as_str).unwrap_or(member).to_string()
    }

    /// The body of a REST request bound to one member (`httpPayload`).
    fn payload(&mut self, field: Field<'a>, value: Option<&Value>, xml: bool) -> Body {
        let media_type = field.trait_value("smithy.api#mediaType").and_then(Value::as_str).map(str::to_string);
        match field.shape.kind {
            "blob" => {
                let given = value.and_then(Value::as_str).filter(|_| !field.has(STREAMING));
                match given {
                    Some(text) if is_json_text(text) => {
                        Body { body_type: BodyType::Json, text: text.to_string(), ..Body::default() }
                    }
                    Some(text) => Body {
                        body_type: BodyType::Text,
                        text: text.to_string(),
                        content_type: media_type.or(Some("text/plain".to_string())),
                        ..Body::default()
                    },
                    // Streams are files; other blobs are almost always JSON documents.
                    None if field.has(STREAMING) || media_type.as_deref().is_some_and(|m| !is_textual(m)) => {
                        Body { body_type: BodyType::Binary, content_type: media_type, ..Body::default() }
                    }
                    None if media_type.as_deref().is_some_and(|m| !is_json(m)) => {
                        Body { body_type: BodyType::Text, content_type: media_type, ..Body::default() }
                    }
                    None => Body { body_type: BodyType::Json, text: "{}".to_string(), ..Body::default() },
                }
            }
            _ if value.is_none() => Body::default(),
            "string" | "enum" => Body {
                body_type: BodyType::Text,
                text: self.text(field, value.unwrap_or(&NULL), Place::Json),
                content_type: media_type.or(Some("text/plain".to_string())),
                ..Body::default()
            },
            "structure" | "union" if xml => {
                let root = trait_of(field.member, XML_NAME)
                    .or_else(|| trait_of(field.shape.def, XML_NAME))
                    .and_then(Value::as_str)
                    .unwrap_or(local_name(field.shape.id));
                let ns = field.trait_value(XML_NAMESPACE).or(self.xml_namespace);
                let mut out = String::new();
                let value = value.unwrap_or(&NULL);
                self.xml_element(&mut out, 0, root, ns, field, value, 0);
                Body { body_type: BodyType::Xml, text: out, ..Body::default() }
            }
            _ => {
                let place = if xml { Place::Json } else { Place::RestJson };
                let json = self.json(field, value.unwrap_or(&NULL), place, 0).unwrap_or(Value::Null);
                Body { body_type: BodyType::Json, text: json_text(&json), ..Body::default() }
            }
        }
    }

    // --- Example values ---

    /// An example for a member: its default, else one generated from its target.
    fn member_value(&mut self, name: &str, field: Field<'a>, depth: usize) -> Option<Value> {
        if !self.visit() {
            return None;
        }
        if field.has("smithy.api#idempotencyToken") {
            return Some(Value::String("{{$guid}}".to_string()));
        }
        if let Some(default) = field.trait_value("smithy.api#default").filter(|d| !is_empty_value(d)) {
            return self.copy(default);
        }
        self.generate(name, field, depth)
    }

    fn generate(&mut self, name: &str, field: Field<'a>, depth: usize) -> Option<Value> {
        let shape = field.shape;
        match shape.kind {
            "structure" | "union" => {
                if depth >= MAX_DEPTH || self.active.contains(&shape.id) || self.active.len() >= MAX_NESTING {
                    return None;
                }
                self.active.push(shape.id);
                let members = self.members(shape);
                let mut out = Map::new();
                for (&member_name, &member) in &members {
                    let union = shape.kind == "union";
                    if !union && !has_trait(member, REQUIRED) {
                        continue;
                    }
                    let Some(f) = self.field(member) else { continue };
                    if f.has(STREAMING) && f.shape.kind == "union" {
                        continue;
                    }
                    if let Some(v) = self.member_value(member_name, f, depth + 1)
                        && self.afford(member_name.len())
                    {
                        out.insert(member_name.to_string(), v);
                        // A union holds one member.
                        if union {
                            break;
                        }
                    }
                    if self.visits == 0 {
                        break;
                    }
                }
                self.active.pop();
                Some(Value::Object(out))
            }
            "list" | "set" => {
                let item = shape.def.get("member").filter(|m| m.is_object()).and_then(|m| self.field(m));
                let item = match item {
                    Some(f) if depth < MAX_DEPTH => self.member_value(&singular(name), f, depth + 1),
                    _ => None,
                };
                let count = length_limit(field, "min").unwrap_or(1).clamp(1, MAX_ITEMS) as usize;
                let mut items = Vec::new();
                if let Some(item) = item {
                    while items.len() + 1 < count && self.afford(json_len(&item, self.output_left)) {
                        items.push(item.clone());
                    }
                    items.push(item);
                }
                Some(Value::Array(items))
            }
            "map" => {
                let key = shape.def.get("key").filter(|m| m.is_object()).and_then(|m| self.field(m));
                let value = shape.def.get("value").filter(|m| m.is_object()).and_then(|m| self.field(m));
                let mut out = Map::new();
                if let (Some(key), Some(value)) = (key, value)
                    && depth < MAX_DEPTH
                {
                    let k = self.member_value("key", key, depth + 1).map(|k| stringify(&k)).unwrap_or_default();
                    let k = if k.is_empty() { "key".to_string() } else { k };
                    if let Some(v) = self.member_value(&singular(name), value, depth + 1) {
                        out.insert(k, v);
                    }
                }
                Some(Value::Object(out))
            }
            "enum" | "intEnum" => {
                let first = shape.def.get("members").and_then(Value::as_object).and_then(|m| m.iter().next());
                first.map(|(member_name, m)| match trait_of(m, "smithy.api#enumValue") {
                    Some(v @ (Value::String(_) | Value::Number(_))) => v.clone(),
                    _ => Value::String(member_name.clone()),
                })
            }
            "string" => {
                // Smithy 1.0 enums.
                if let Some(v) =
                    field.trait_value("smithy.api#enum").and_then(Value::as_array).and_then(|e| e.first()?.get("value"))
                {
                    return self.copy(v);
                }
                let text = fit_length(self.text_example(name), field);
                self.afford(text.len()).then_some(Value::String(text))
            }
            "byte" | "short" | "integer" | "long" | "bigInteger" | "float" | "double" | "bigDecimal" => {
                Some(Value::from(number_example(name, field)))
            }
            "boolean" => Some(Value::Bool(false)),
            "timestamp" => Some(Value::from(EXAMPLE_EPOCH)),
            "blob" => Some(Value::String("example".to_string())),
            "document" => Some(Value::Object(Map::new())),
            _ => None,
        }
    }

    /// A realistic string for a member, from its name (`TableName` gives `example-table`).
    fn text_example(&self, name: &str) -> String {
        let words = name_words(name);
        let key = words.concat();
        let fixed = match key.as_str() {
            "accountid" | "awsaccountid" | "owner" | "ownerid" => "123456789012",
            "region" | "regionname" => DEFAULT_REGION,
            "bucket" | "bucketname" => "amzn-s3-demo-bucket",
            "queueurl" => "https://sqs.us-east-1.amazonaws.com/123456789012/example-queue",
            "modelid" => "amazon.nova-micro-v1:0",
            "contenttype" | "accept" => "application/json",
            "email" | "emailaddress" | "fromemailaddress" => "jane.doe@example.com",
            "path" | "pathprefix" => "/",
            "message" | "messagebody" | "subject" | "text" | "comment" | "description" => "Hello from Zorvik",
            "role" | "rolearn" | "executionrolearn" | "servicerolearn" => "arn:aws:iam::123456789012:role/example-role",
            _ => "",
        };
        if !fixed.is_empty() {
            return fixed.to_string();
        }
        let (last, rest) = match words.split_last() {
            Some((last, rest)) => (last.as_str(), rest),
            None => return "example".to_string(),
        };
        let what = if rest.is_empty() { "resource".to_string() } else { rest.join("-") };
        match last {
            "arn" => {
                let service = if self.signing_name.is_empty() { "service" } else { &self.signing_name };
                format!("arn:aws:{service}:{DEFAULT_REGION}:123456789012:example-{what}")
            }
            "name" if !rest.is_empty() => format!("example-{what}"),
            "id" if !rest.is_empty() => format!("example-{what}-id"),
            "url" | "uri" | "endpoint" => "https://example.com".to_string(),
            _ => "example".to_string(),
        }
    }

    // --- Serialization ---

    /// A member's value as a protocol writes it in a JSON body.
    fn json(&mut self, field: Field<'a>, node: &Value, place: Place, depth: usize) -> Option<Value> {
        if !self.visit() || depth > MAX_NESTING {
            return None;
        }
        let shape = field.shape;
        Some(match (shape.kind, node) {
            ("structure" | "union", Value::Object(given)) => {
                let mut out = Map::new();
                for (name, value) in given {
                    let Some(member) = self.member(shape, name) else { continue };
                    let Some(f) = self.field(member) else { continue };
                    let key = match place {
                        Place::RestJson => {
                            trait_of(member, "smithy.api#jsonName").and_then(Value::as_str).unwrap_or(name)
                        }
                        _ => name,
                    };
                    if let Some(v) = self.json(f, value, place, depth + 1) {
                        out.insert(plain(key), v);
                    }
                }
                Value::Object(out)
            }
            ("list" | "set", Value::Array(items)) => {
                let item = shape.def.get("member").and_then(|m| self.field(m));
                let mut out = Vec::new();
                for v in items {
                    match item {
                        Some(f) => out.extend(self.json(f, v, place, depth + 1)),
                        None => out.push(v.clone()),
                    }
                }
                Value::Array(out)
            }
            ("map", Value::Object(entries)) => {
                let value = shape.def.get("value").and_then(|m| self.field(m));
                let mut out = Map::new();
                for (k, v) in entries {
                    let v = match value {
                        Some(f) => self.json(f, v, place, depth + 1),
                        None => Some(v.clone()),
                    };
                    out.extend(v.map(|v| (k.clone(), v)));
                }
                Value::Object(out)
            }
            ("timestamp", _) => {
                let format = timestamp_format(field, place);
                timestamp_value(node, format)
            }
            ("blob", Value::String(s)) => Value::String(base64::engine::general_purpose::STANDARD.encode(s)),
            _ => node.clone(),
        })
    }

    /// A JSON object body from the members that have values (`{}` when none).
    fn json_body(
        &mut self,
        fields: &IndexMap<&'a str, Field<'a>>,
        values: &IndexMap<&'a str, Value>,
        place: Place,
    ) -> Body {
        let mut out = Map::new();
        for (&name, &field) in fields {
            let Some(value) = values.get(name) else { continue };
            let key = match place {
                Place::RestJson => {
                    trait_of(field.member, "smithy.api#jsonName").and_then(Value::as_str).unwrap_or(name)
                }
                _ => name,
            };
            if let Some(v) = self.json(field, value, place, 0) {
                out.insert(plain(key), v);
            }
        }
        Body { body_type: BodyType::Json, text: json_text(&Value::Object(out)), ..Body::default() }
    }

    /// Writes a structure's element: attributes, then each member (name, field, value).
    fn xml_struct(
        &mut self,
        out: &mut String,
        indent: usize,
        name: &str,
        ns: Option<&Value>,
        members: &[(&str, Field<'a>, &Value)],
        depth: usize,
    ) {
        let mut open = format!("<{}{}", xml_name(name), namespace_attr(ns));
        let mut children = String::new();
        for &(member, field, value) in members {
            let element = trait_of(field.member, XML_NAME).and_then(Value::as_str).unwrap_or(member);
            if has_trait(field.member, "smithy.api#xmlAttribute") {
                let text = self.text(field, value, Place::Xml);
                let _ = write!(open, " {}=\"{}\"", xml_name(element), xml_escape(&text));
                continue;
            }
            self.xml_member(&mut children, indent + 1, element, field, value, depth + 1);
        }
        close_element(out, indent, &open, name, &children);
    }

    /// Writes a member: flattened lists and maps repeat its element, others nest in it.
    fn xml_member(
        &mut self,
        out: &mut String,
        indent: usize,
        name: &str,
        field: Field<'a>,
        node: &Value,
        depth: usize,
    ) {
        if !self.visit() || depth > MAX_NESTING {
            return;
        }
        let ns = trait_of(field.member, XML_NAMESPACE);
        let flattened = has_trait(field.member, XML_FLATTENED);
        match (field.shape.kind, node) {
            ("list" | "set", Value::Array(items)) if flattened => {
                if let Some(item) = field.shape.def.get("member").and_then(|m| self.field(m)) {
                    for v in items {
                        self.xml_element(out, indent, name, ns, item, v, depth + 1);
                    }
                }
            }
            ("map", Value::Object(entries)) if flattened => {
                for (k, v) in entries {
                    let mut entry = String::new();
                    self.xml_entry(&mut entry, indent + 1, field.shape, k, v, depth + 1);
                    close_element(out, indent, &format!("<{}{}", xml_name(name), namespace_attr(ns)), name, &entry);
                }
            }
            _ => self.xml_element(out, indent, name, ns, field, node, depth),
        }
    }

    /// Writes one element holding a value of `field`'s shape.
    #[allow(clippy::too_many_arguments)]
    fn xml_element(
        &mut self,
        out: &mut String,
        indent: usize,
        name: &str,
        ns: Option<&Value>,
        field: Field<'a>,
        node: &Value,
        depth: usize,
    ) {
        if depth > MAX_NESTING {
            return;
        }
        let open = format!("<{}{}", xml_name(name), namespace_attr(ns));
        let shape = field.shape;
        match (shape.kind, node) {
            ("structure" | "union", Value::Object(given)) => {
                let mut members = Vec::new();
                for (member, value) in given {
                    let Some(m) = self.member(shape, member) else { continue };
                    let Some(f) = self.field(m) else { continue };
                    members.push((member.as_str(), f, value));
                }
                self.xml_struct(out, indent, name, ns, &members, depth);
            }
            ("list" | "set", Value::Array(items)) => {
                let mut children = String::new();
                if let Some(item) = shape.def.get("member").and_then(|m| self.field(m)) {
                    let item_name = trait_of(item.member, XML_NAME).and_then(Value::as_str).unwrap_or("member");
                    for v in items {
                        if !self.visit() {
                            break;
                        }
                        self.xml_element(&mut children, indent + 1, item_name, None, item, v, depth + 1);
                    }
                }
                close_element(out, indent, &open, name, &children);
            }
            ("map", Value::Object(entries)) => {
                let mut children = String::new();
                for (k, v) in entries {
                    let mut entry = String::new();
                    self.xml_entry(&mut entry, indent + 2, shape, k, v, depth + 1);
                    close_element(&mut children, indent + 1, "<entry", "entry", &entry);
                }
                close_element(out, indent, &open, name, &children);
            }
            ("structure" | "union" | "list" | "set" | "map", _) => close_element(out, indent, &open, name, ""),
            _ => {
                let text = self.text(field, node, Place::Xml);
                pad(out, indent);
                let _ = writeln!(out, "{open}>{}</{}>", xml_escape(&text), xml_name(name));
            }
        }
    }

    /// The key and value elements of one map entry.
    fn xml_entry(&mut self, out: &mut String, indent: usize, map: Shape<'a>, key: &str, value: &Value, depth: usize) {
        let k = map.def.get("key").filter(|m| m.is_object());
        let v = map.def.get("value").filter(|m| m.is_object());
        let key_name = k.and_then(|m| trait_of(m, XML_NAME)).and_then(Value::as_str).unwrap_or("key");
        let value_name = v.and_then(|m| trait_of(m, XML_NAME)).and_then(Value::as_str).unwrap_or("value");
        pad(out, indent);
        let _ = writeln!(out, "<{0}>{1}</{0}>", xml_name(key_name), xml_escape(key));
        if let Some(f) = v.and_then(|m| self.field(m)) {
            self.xml_element(out, indent, value_name, None, f, value, depth + 1);
        }
    }

    /// A Query or EC2 Query form: `Action`, `Version` and the required members, then the
    /// optional ones switched off (with the example's values when it gives them).
    fn form_body(
        &mut self,
        action: &str,
        fields: &IndexMap<&'a str, Field<'a>>,
        values: &IndexMap<&'a str, Value>,
        ec2: bool,
    ) -> Body {
        let version = plain(str_of(self.service, "version"));
        let mut form = vec![KeyValue::new("Action", action), KeyValue::new("Version", version)];
        let mut offered = 0;
        for required in [true, false] {
            for (&member, &field) in fields {
                if has_trait(field.member, REQUIRED) != required || (!required && offered >= MAX_OPTIONAL) {
                    continue;
                }
                let value = match values.get(member) {
                    Some(v) => Some(v.clone()),
                    None if !required => self.member_value(member, field, 0),
                    None => None,
                };
                let Some(value) = value else { continue };
                let mut pairs = Vec::new();
                self.query_pairs(&query_name(member, field.member, ec2), field, &value, ec2, &mut pairs, 0);
                let description = self.description(Some(field));
                let room = if required { usize::MAX } else { MAX_OPTIONAL - offered };
                for (k, v) in pairs.into_iter().take(room) {
                    form.push(row(&k, v, required, description.clone()));
                    offered += usize::from(!required);
                }
            }
        }
        Body { body_type: BodyType::FormUrlencoded, form, ..Body::default() }
    }

    /// The form fields for one value: `Key.Member`, `Key.member.1`, `Key.entry.1.key`...
    fn query_pairs(
        &mut self,
        key: &str,
        field: Field<'a>,
        node: &Value,
        ec2: bool,
        out: &mut Vec<(String, String)>,
        depth: usize,
    ) {
        if !self.visit() || depth > MAX_NESTING {
            return;
        }
        let shape = field.shape;
        match (shape.kind, node) {
            ("structure" | "union", Value::Object(given)) => {
                for (name, value) in given {
                    let Some(member) = self.member(shape, name) else { continue };
                    let Some(f) = self.field(member) else { continue };
                    let child = format!("{key}.{}", query_name(name, member, ec2));
                    self.query_pairs(&child, f, value, ec2, out, depth + 1);
                }
            }
            ("list" | "set", Value::Array(items)) => {
                let Some(item) = shape.def.get("member").and_then(|m| self.field(m)) else { return };
                let segment = if ec2 || has_trait(field.member, XML_FLATTENED) {
                    String::new()
                } else {
                    format!(".{}", trait_of(item.member, XML_NAME).and_then(Value::as_str).unwrap_or("member"))
                };
                for (i, v) in items.iter().enumerate() {
                    self.query_pairs(&format!("{key}{segment}.{}", i + 1), item, v, ec2, out, depth + 1);
                }
            }
            ("map", Value::Object(entries)) => {
                let k = shape.def.get("key").filter(|m| m.is_object());
                let key_name = k.and_then(|m| trait_of(m, XML_NAME)).and_then(Value::as_str).unwrap_or("key");
                let v = shape.def.get("value").filter(|m| m.is_object());
                let value_name = v.and_then(|m| trait_of(m, XML_NAME)).and_then(Value::as_str).unwrap_or("value");
                let entry = if has_trait(field.member, XML_FLATTENED) { "" } else { ".entry" };
                let value_field = v.and_then(|m| self.field(m));
                for (i, (k, v)) in entries.iter().enumerate() {
                    let prefix = format!("{key}{entry}.{}", i + 1);
                    out.push((plain(&format!("{prefix}.{key_name}")), k.clone()));
                    if let Some(f) = value_field {
                        self.query_pairs(&format!("{prefix}.{value_name}"), f, v, ec2, out, depth + 1);
                    }
                }
            }
            _ => {
                let text = self.text(field, node, Place::Query);
                if self.afford(key.len() + text.len()) {
                    out.push((plain(key), text));
                }
            }
        }
    }

    /// A scalar as text in a URL, header, form or XML element.
    fn text(&mut self, field: Field<'a>, node: &Value, place: Place) -> String {
        match (field.shape.kind, node) {
            ("timestamp", _) => stringify(&timestamp_value(node, timestamp_format(field, place))),
            ("blob", Value::String(s)) => base64::engine::general_purpose::STANDARD.encode(s),
            (_, Value::Object(_) | Value::Array(_)) => node.to_string(),
            _ => stringify(node),
        }
    }

    /// Values of a URL query parameter: one per list item.
    fn texts(&mut self, field: Field<'a>, node: &Value, place: Place) -> Vec<String> {
        match (field.shape.kind, node) {
            ("list" | "set", Value::Array(items)) => {
                let Some(item) = field.shape.def.get("member").and_then(|m| self.field(m)) else { return Vec::new() };
                items.iter().map(|v| self.text(item, v, place)).collect()
            }
            _ => vec![self.text(field, node, place)],
        }
    }

    /// A header value: lists are comma-separated, strings with a media type base64-encoded.
    fn header_text(&mut self, field: Field<'a>, node: &Value) -> String {
        if field.shape.kind == "string" && field.has("smithy.api#mediaType") {
            return base64::engine::general_purpose::STANDARD.encode(stringify(node));
        }
        self.texts(field, node, Place::Header).join(", ")
    }
}

// --- Model helpers ---

fn str_of<'v>(v: &'v Value, key: &str) -> &'v str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

fn traits(v: &Value) -> Option<&Map<String, Value>> {
    v.get("traits")?.as_object()
}

fn trait_of<'v>(v: &'v Value, name: &str) -> Option<&'v Value> {
    traits(v)?.get(name)
}

fn has_trait(v: &Value, name: &str) -> bool {
    trait_of(v, name).is_some()
}

fn trait_str<'v>(v: &'v Value, name: &str) -> &'v str {
    trait_of(v, name).and_then(Value::as_str).unwrap_or("")
}

/// The shape ids in a list of `{"target": id}` references.
fn targets<'v>(v: &'v Value, key: &str) -> impl Iterator<Item = &'v str> {
    v.get(key).and_then(Value::as_array).into_iter().flatten().filter_map(|r| r.get("target")?.as_str())
}

/// `com.amazonaws.dynamodb#ListTables` → `ListTables`.
fn local_name(id: &str) -> &str {
    id.rsplit_once('#').map_or(id, |(_, name)| name)
}

/// The type of a prelude shape (`smithy.api#String`...).
fn prelude(id: &str) -> Option<&'static str> {
    Some(match id.strip_prefix("smithy.api#")? {
        "String" => "string",
        "Blob" => "blob",
        "Boolean" | "PrimitiveBoolean" => "boolean",
        "Byte" | "PrimitiveByte" => "byte",
        "Short" | "PrimitiveShort" => "short",
        "Integer" | "PrimitiveInteger" => "integer",
        "Long" | "PrimitiveLong" => "long",
        "Float" | "PrimitiveFloat" => "float",
        "Double" | "PrimitiveDouble" => "double",
        "BigInteger" => "bigInteger",
        "BigDecimal" => "bigDecimal",
        "Timestamp" => "timestamp",
        "Document" => "document",
        "Unit" => "structure",
        _ => return None,
    })
}

/// A name from the model without braces, so it cannot reference a variable (`{{awsSecretAccessKey}}`).
fn plain(name: &str) -> String {
    name.replace(['{', '}'], "")
}

/// A valid header name (an HTTP token).
fn is_token(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+.^_`|~-".contains(&b))
}

fn variable(key: &str, value: &str, secret: bool) -> Variable {
    Variable { key: key.to_string(), value: value.to_string(), enabled: true, secret }
}

fn row(key: &str, value: String, enabled: bool, description: String) -> KeyValue {
    KeyValue { key: key.to_string(), value, enabled, description }
}

fn is_empty_value(v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::String(s) => s.is_empty(),
        Value::Array(a) => a.is_empty(),
        Value::Object(o) => o.is_empty(),
        _ => false,
    }
}

/// A form field's name: `xmlName` for Query; `ec2QueryName`, else `xmlName` or the member
/// name capitalized, for EC2 Query.
fn query_name(name: &str, member: &Value, ec2: bool) -> String {
    let xml = trait_of(member, XML_NAME).and_then(Value::as_str);
    if !ec2 {
        return xml.unwrap_or(name).to_string();
    }
    if let Some(ec2_name) = trait_of(member, "aws.protocols#ec2QueryName").and_then(Value::as_str) {
        return ec2_name.to_string();
    }
    let name = xml.unwrap_or(name);
    let mut chars = name.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

fn timestamp_format(field: Field<'_>, place: Place) -> &'static str {
    match field.trait_value("smithy.api#timestampFormat").and_then(Value::as_str) {
        Some("epoch-seconds") => "epoch-seconds",
        Some("http-date") => "http-date",
        Some("date-time") => "date-time",
        _ => place.timestamp_format(),
    }
}

/// `{min, max}` of a member's or its target's `length` trait.
fn length_limit(field: Field<'_>, key: &str) -> Option<u64> {
    field.trait_value("smithy.api#length")?.get(key)?.as_u64()
}

/// Pads (with `x`) or cuts `text` to the `length` trait's limits.
fn fit_length(mut text: String, field: Field<'_>) -> String {
    let count = text.chars().count() as u64;
    if let Some(min) = length_limit(field, "min").map(|n| n.min(MAX_PADDING))
        && count < min
    {
        text.extend(std::iter::repeat_n('x', (min - count) as usize));
    }
    if let Some(max) = length_limit(field, "max")
        && let Some((cut, _)) = text.char_indices().nth(max.try_into().unwrap_or(usize::MAX))
    {
        text.truncate(cut);
    }
    text
}

/// A number for a member, from its name (`MaxResults` gives 10), within its `range`.
fn number_example(name: &str, field: Field<'_>) -> i64 {
    let key = name_words(name).concat();
    let limits = ["maxresults", "maxitems", "maxrecords", "maxkeys", "limit", "pagesize", "maxentries"];
    let mut n = if limits.contains(&key.as_str()) { 10.0 } else { 1.0 };
    let range = field.trait_value("smithy.api#range");
    let bound = |key| range.and_then(|r| r.get(key)).and_then(Value::as_f64).filter(|b| b.is_finite());
    if let Some(min) = bound("min")
        && n < min
    {
        n = min.ceil();
    }
    if let Some(max) = bound("max")
        && n > max
    {
        n = max.floor();
    }
    n.clamp(i64::MIN as f64, i64::MAX as f64) as i64
}

// --- Endpoints ---

/// A DNS name (or label) without anything that could break out of a URL's host.
fn is_host(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 253
        && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.')
        && !s.starts_with(['.', '-'])
        && !s.contains("..")
}

/// A region of the standard AWS partition (or the `aws-global` pseudo region).
fn is_aws_region(region: &str) -> bool {
    let known = ["us-", "eu-", "ap-", "ca-", "sa-", "me-", "af-", "il-", "mx-"];
    region == "aws-global"
        || (known.iter().any(|p| region.starts_with(p))
            && !["us-gov-", "us-iso", "eu-isoe"].iter().any(|p| region.starts_with(p))
            && is_host(region)
            && !region.contains('.'))
}

/// An AWS service's endpoint: from the model's own endpoint tests when they show it (the
/// region swapped for `{{awsRegion}}`), else `https://<prefix>.{{awsRegion}}.amazonaws.com`.
/// Services with one endpoint for every region keep it, with the region they are signed for.
fn aws_endpoint(prefix: &str, tests: Option<&Value>) -> (String, Option<String>) {
    if let Some((_, host, region)) = SINGLE_ENDPOINTS.iter().find(|(p, ..)| *p == prefix) {
        return (format!("https://{host}"), Some(region.to_string()));
    }
    // (region, host, signing region) of each case that sets only the region.
    let mut cases: Vec<(&str, &str, Option<&str>)> = Vec::new();
    let all = tests.and_then(|t| t.get("testCases")).and_then(Value::as_array).into_iter().flatten();
    for case in all.take(MAX_ENDPOINT_TESTS) {
        let Some(params) = case.get("params").and_then(Value::as_object) else { continue };
        let plain = params.iter().all(|(k, v)| match k.as_str() {
            "Region" => v.as_str().is_some_and(is_aws_region),
            "UseFIPS" | "UseDualStack" | "UseGlobalEndpoint" => v == &Value::Bool(false),
            _ => false,
        });
        let endpoint = case.pointer("/expect/endpoint");
        let host = endpoint
            .and_then(|e| e.get("url"))
            .and_then(Value::as_str)
            .and_then(|u| u.strip_prefix("https://"))
            .map(|h| h.trim_end_matches('/'))
            .filter(|h| is_host(h));
        let (Some(region), Some(host), true) = (params.get("Region").and_then(Value::as_str), host, plain) else {
            continue;
        };
        let signing = endpoint
            .and_then(|e| e.pointer("/properties/authSchemes/0/signingRegion"))
            .and_then(Value::as_str)
            .filter(|r| is_aws_region(r) && *r != "aws-global");
        cases.push((region, host, signing));
    }
    let names_region = |host: &str, region: &str| host.split('.').any(|label| label == region);
    let single = |host: &str, signing: Option<&str>| {
        let region = signing
            .or_else(|| host.split('.').find(|l| is_aws_region(l) && *l != "aws-global"))
            .unwrap_or(DEFAULT_REGION);
        (format!("https://{host}"), Some(region.to_string()))
    };
    let regional: Vec<_> = cases.iter().filter(|(r, ..)| *r != "aws-global").collect();
    // The pseudo region `aws-global` answering like a real one means one endpoint for all.
    if let Some((_, host, signing)) = cases.iter().find(|(r, ..)| *r == "aws-global")
        && regional.iter().any(|(_, h, _)| h == host)
    {
        return single(host, *signing);
    }
    if let Some((_, host, signing)) = regional.first()
        && regional.iter().all(|(r, h, _)| h == host && !names_region(h, r))
    {
        return single(host, *signing);
    }
    let template =
        regional.iter().filter(|(r, h, _)| names_region(h, r)).min_by_key(|(r, ..)| *r != DEFAULT_REGION).map(
            |(r, h, _)| h.split('.').map(|l| if l == *r { "{{awsRegion}}" } else { l }).collect::<Vec<_>>().join("."),
        );
    match template {
        Some(host) => (format!("https://{host}"), None),
        None => (format!("https://{prefix}.{{{{{REGION_VAR}}}}}.amazonaws.com"), None),
    }
}

/// Splits a URI pattern into literal text, each followed by the label after it (`{Name}` or
/// `{Name+}`); unclosed braces stay literal.
fn templates(pattern: &str) -> Vec<(&str, Option<&str>)> {
    let mut out = Vec::new();
    let mut rest = pattern;
    while let Some(start) = rest.find('{') {
        let Some(len) = rest[start..].find('}') else { break };
        let label = &rest[start + 1..start + len];
        if label.is_empty() || label.contains('{') {
            break;
        }
        out.push((&rest[..start], Some(label)));
        rest = &rest[start + len + 1..];
    }
    out.push((rest, None));
    out
}

/// Labels too generic to stand alone as a variable.
const GENERIC_LABELS: [&str; 6] = ["id", "name", "key", "arn", "uuid", "identifier"];

/// The variable for a label: its camelCase name (`FunctionName` → `functionName`), with a
/// generic name prefixed by the singular of the literal segment before it
/// (`/hostedzone/{Id}` → `hostedzoneId`).
fn label_variable(name: &str, previous: Option<&str>) -> String {
    let var = camel_case(name);
    let var = if var.is_empty() { "label".to_string() } else { var };
    if !GENERIC_LABELS.contains(&var.as_str()) {
        return var;
    }
    let prefix = previous.map(|s| camel_case(&singular(s))).unwrap_or_default();
    if prefix.is_empty() {
        return var;
    }
    format!("{prefix}{}{}", var[..1].to_ascii_uppercase(), &var[1..])
}

// --- Names ---

/// The lowercase words of a name, split at `_`, `-` and other separators and at case changes:
/// `TableName`, `table_name` and `TABLEName` all give `["table", "name"]`.
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

/// Naive singular of an English noun: `Policies` → `Policy`, `Aliases` → `Alias`, `Tags` →
/// `Tag`; words ending in `ss`, `us` or `is` are left alone.
fn singular(word: &str) -> String {
    let lower = word.to_ascii_lowercase();
    if lower.len() > 3 && lower.ends_with("ies") && word.is_char_boundary(word.len() - 3) {
        format!("{}y", &word[..word.len() - 3])
    } else if ["sses", "uses", "iases"].iter().any(|e| lower.ends_with(e)) && word.is_char_boundary(word.len() - 2) {
        word[..word.len() - 2].to_string()
    } else if lower.ends_with('s') && !["ss", "us", "is"].iter().any(|e| lower.ends_with(e)) {
        word[..word.len() - 1].to_string()
    } else {
        word.to_string()
    }
}

// --- Values ---

/// Text form of a value for a URL, header or form field.
fn stringify(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn json_text(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default()
}

fn is_json(media: &str) -> bool {
    let base = media.split(';').next().unwrap_or_default().trim().to_ascii_lowercase();
    base == "application/json" || base.ends_with("+json")
}

/// Text media types (JSON, XML, `text/*`), which bodies show as text.
fn is_textual(media: &str) -> bool {
    let base = media.split(';').next().unwrap_or_default().trim().to_ascii_lowercase();
    is_json(&base) || base.starts_with("text/") || base.ends_with("/xml") || base.ends_with("+xml")
}

fn is_json_text(text: &str) -> bool {
    matches!(serde_json::from_str::<Value>(text), Ok(Value::Object(_) | Value::Array(_)))
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

// --- Timestamps ---

/// A timestamp (epoch seconds, or a date-time string as examples write them) in a format.
fn timestamp_value(node: &Value, format: &str) -> Value {
    let seconds = match node {
        Value::Number(n) => n.as_f64().filter(|f| f.is_finite()).map(|f| f.floor() as i64),
        Value::String(s) => parse_date_time(s),
        _ => None,
    };
    let Some(seconds) = seconds.map(|s| s.clamp(MIN_EPOCH, MAX_EPOCH)) else { return node.clone() };
    match format {
        "epoch-seconds" => Value::from(seconds),
        "http-date" => Value::String(http_date(seconds)),
        _ => Value::String(date_time(seconds)),
    }
}

fn date_time(seconds: i64) -> String {
    let (y, m, d) = civil_from_days(seconds.div_euclid(86_400));
    let s = seconds.rem_euclid(86_400);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", s / 3600, s / 60 % 60, s % 60)
}

fn http_date(seconds: i64) -> String {
    const DAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let days = seconds.div_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    let s = seconds.rem_euclid(86_400);
    let weekday = DAYS[days.rem_euclid(7) as usize];
    let month = MONTHS[(m - 1) as usize];
    format!("{weekday}, {d:02} {month} {y:04} {:02}:{:02}:{:02} GMT", s / 3600, s / 60 % 60, s % 60)
}

/// Seconds since the epoch of an RFC 3339 date-time (`2024-01-01T12:00:00Z`, with optional
/// fraction and offset).
fn parse_date_time(s: &str) -> Option<i64> {
    let s = s.trim();
    let b = s.as_bytes();
    let num = |range: std::ops::Range<usize>| -> Option<i64> {
        let part = s.get(range)?;
        part.bytes().all(|c| c.is_ascii_digit()).then(|| part.parse().ok())?
    };
    if b.len() < 20 || b[4] != b'-' || b[7] != b'-' || !matches!(b[10], b'T' | b't') || b[13] != b':' || b[16] != b':' {
        return None;
    }
    let (year, month, day) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (hour, minute, second) = (num(11..13)?, num(14..16)?, num(17..19)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let mut rest = &s[19..];
    if let Some(fraction) = rest.strip_prefix('.') {
        rest = fraction.trim_start_matches(|c: char| c.is_ascii_digit());
    }
    let offset = match rest {
        "Z" | "z" => 0,
        _ => {
            let sign = match rest.as_bytes().first()? {
                b'+' => 1,
                b'-' => -1,
                _ => return None,
            };
            let (h, m) = rest[1..].split_once(':')?;
            let (h, m): (i64, i64) = (h.parse().ok()?, m.parse().ok()?);
            // Offsets are `±hh:mm`; anything larger would overflow below.
            if !(0..=23).contains(&h) || !(0..=59).contains(&m) {
                return None;
            }
            sign * (h * 3600 + m * 60)
        }
    };
    let days = days_from_civil(year, month as u32, day as u32);
    Some(days * 86_400 + hour * 3600 + minute * 60 + second - offset)
}

/// Days since 1970-01-01 of a proleptic Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year.rem_euclid(400);
    let m = i64::from(month);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

// --- XML ---

fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
    out
}

/// An element or attribute name from the model, with characters a name cannot hold dropped.
fn xml_name(name: &str) -> String {
    let name: String = name.chars().filter(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | ':')).collect();
    if name.is_empty() { "element".to_string() } else { name }
}

/// ` xmlns="uri"` (or ` xmlns:prefix="uri"`) for an `xmlNamespace` trait.
fn namespace_attr(ns: Option<&Value>) -> String {
    let Some(ns) = ns else { return String::new() };
    let uri = str_of(ns, "uri");
    if uri.is_empty() {
        return String::new();
    }
    match str_of(ns, "prefix") {
        "" => format!(" xmlns=\"{}\"", xml_escape(uri)),
        prefix => format!(" xmlns:{}=\"{}\"", xml_name(prefix), xml_escape(uri)),
    }
}

fn pad(out: &mut String, indent: usize) {
    out.extend(std::iter::repeat_n("  ", indent));
}

/// Writes `open` (an unfinished start tag) with `children`, or as an empty element.
fn close_element(out: &mut String, indent: usize, open: &str, name: &str, children: &str) {
    pad(out, indent);
    if children.is_empty() {
        let _ = writeln!(out, "{open}/>");
    } else {
        let _ = write!(out, "{open}>\n{children}");
        pad(out, indent);
        let _ = writeln!(out, "</{}>", xml_name(name));
    }
}

// --- Documentation ---

/// `s` cut to at most `max` bytes, at a character boundary.
fn cut(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// Decodes the HTML entities AWS documentation uses (`&lt;`, `&#39;`...).
fn decode_entity(entity: &str) -> Option<char> {
    Some(match entity {
        "lt" => '<',
        "gt" => '>',
        "amp" => '&',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => ' ',
        _ => {
            let code = entity.strip_prefix('#')?;
            let n = match code.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => code.parse().ok()?,
            };
            char::from_u32(n).filter(|c| !c.is_control() || c.is_whitespace())?
        }
    })
}

/// Markdown for AWS's HTML documentation: paragraphs, lists, code, emphasis and http(s) links.
/// Other tags are dropped and the text escaped, so markup in it stays text. At most about
/// `max` bytes, cut at a paragraph or word.
fn html_to_markdown(html: &str, max: usize) -> String {
    convert_html(html, max, true)
}

/// Plain text of HTML documentation, on one line, of about `max` bytes at most.
fn html_to_text(html: &str, max: usize) -> String {
    convert_html(html, max, false)
}

fn convert_html(html: &str, max: usize, markdown: bool) -> String {
    let mut out = String::new();
    let mut space = false;
    // Inline markup just started: the space after it is dropped.
    let mut opened = false;
    let mut lists = 0usize;
    let mut code = 0usize;
    let mut skip = 0usize;
    let mut links: Vec<Option<String>> = Vec::new();
    let mut closable = true;
    let mut rest = html;
    let block = |out: &mut String| {
        // A note's label stays with its text.
        if out.ends_with(":** ") {
            return;
        }
        let trimmed = out.trim_end_matches([' ', '\n']).len();
        out.truncate(trimmed);
        if !out.is_empty() {
            out.push_str(if markdown { "\n\n" } else { " " });
        }
    };
    while !rest.is_empty() && out.len() < max * 2 {
        if let Some(comment) = rest.strip_prefix("<!--") {
            rest = comment.split_once("-->").map_or("", |(_, after)| after);
            continue;
        }
        // Once no `>` is left, no tag can close: looking for one after every `<` would take
        // quadratic time on text full of them.
        let tag = rest
            .strip_prefix('<')
            .filter(|t| closable && t.starts_with(|c: char| c.is_ascii_alphabetic() || c == '/'))
            .and_then(|t| {
                let split = t.split_once('>');
                closable = split.is_some();
                split
            });
        if let Some((tag, after)) = tag {
            rest = after;
            let closing = tag.starts_with('/');
            let name: String = tag
                .trim_start_matches('/')
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric())
                .collect::<String>()
                .to_ascii_lowercase();
            if name == "fullname" {
                skip = if closing { skip.saturating_sub(1) } else { skip + 1 };
                continue;
            }
            if skip > 0 {
                continue;
            }
            match name.as_str() {
                // Paragraphs inside list items stay in the item.
                "p" if lists > 0 => space = true,
                "p" | "div" | "dl" | "pre" | "codeblock" | "examples" | "example" | "h1" | "h2" | "h3" | "h4"
                | "h5" | "h6" | "dt" | "dd" => {
                    block(&mut out);
                    space = false;
                }
                "note" | "important" => {
                    block(&mut out);
                    space = false;
                    if markdown && !closing {
                        out.push_str(if name == "note" { "**Note:** " } else { "**Important:** " });
                    }
                }
                "br" => {
                    let trimmed = out.trim_end_matches(' ').len();
                    out.truncate(trimmed);
                    out.push(if markdown { '\n' } else { ' ' });
                    space = false;
                }
                "ul" | "ol" => {
                    lists = if closing { lists.saturating_sub(1) } else { lists + 1 };
                    // Nested lists stay inside their item.
                    if (closing && lists == 0) || (!closing && lists == 1) {
                        block(&mut out);
                    }
                    space = false;
                }
                "li" if !closing => {
                    let trimmed = out.trim_end_matches(' ').len();
                    out.truncate(trimmed);
                    if markdown {
                        if !out.is_empty() && !out.ends_with('\n') {
                            out.push('\n');
                        }
                        pad(&mut out, lists.saturating_sub(1));
                        out.push_str("- ");
                    } else if !out.is_empty() {
                        out.push(' ');
                    }
                    space = false;
                }
                "code" | "tt" => {
                    let before = code;
                    code = if closing { code.saturating_sub(1) } else { code + 1 };
                    // Only the outermost code tags make a code span.
                    if markdown && before == 0 && code == 1 {
                        opened = open_marker(&mut out, "`", &mut space);
                    } else if markdown && before == 1 && code == 0 {
                        out.push('`');
                    }
                }
                "b" | "strong" | "i" | "em" if markdown && code == 0 => {
                    let marker = if matches!(name.as_str(), "b" | "strong") { "**" } else { "_" };
                    if closing {
                        out.push_str(marker);
                    } else {
                        opened = open_marker(&mut out, marker, &mut space);
                    }
                }
                "a" if markdown && code == 0 => {
                    if closing {
                        if let Some(Some(href)) = links.pop() {
                            let _ = write!(out, "]({href})");
                        }
                    } else {
                        let href = attribute(tag, "href").map(|h| h.replace("&amp;", "&")).filter(|h| {
                            (h.starts_with("https://") || h.starts_with("http://"))
                                && !h.contains(|c: char| c.is_whitespace() || "()<>\"'`[]".contains(c))
                        });
                        if href.is_some() {
                            opened = open_marker(&mut out, "[", &mut space);
                        }
                        links.push(href);
                    }
                }
                _ => {}
            }
            continue;
        }
        // Text up to the next tag (or a lone `<`).
        let first = rest.chars().next().map_or(1, char::len_utf8);
        let next = rest[first..].find('<').map_or(rest.len(), |i| i + first);
        let (text, after) = rest.split_at(next);
        rest = after;
        if skip > 0 {
            continue;
        }
        let mut chars = text.char_indices().peekable();
        while let Some((i, c)) = chars.next() {
            let c = if c == '&' {
                let entity = text[i + 1..].split_once(';').map(|(e, _)| e).filter(|e| e.len() <= 10);
                match entity.and_then(decode_entity) {
                    Some(decoded) => {
                        let skip_to = i + 1 + entity.map_or(0, str::len) + 1;
                        while chars.peek().is_some_and(|(j, _)| *j < skip_to) {
                            chars.next();
                        }
                        decoded
                    }
                    None => '&',
                }
            } else {
                c
            };
            if c.is_whitespace() {
                space = true;
                continue;
            }
            if c.is_control() {
                continue;
            }
            if space && !opened && !out.is_empty() && !out.ends_with([' ', '\n']) && !out.ends_with("- ") {
                out.push(' ');
            }
            space = false;
            opened = false;
            if markdown && code > 0 {
                out.push(if c == '`' { '\'' } else { c });
            } else if markdown && "\\`*_[]<>#|".contains(c) {
                out.push('\\');
                out.push(c);
            } else {
                out.push(c);
            }
        }
    }
    let out = out.trim().to_string();
    shorten(out, max, markdown)
}

/// Starts inline markup: a pending space goes before it. Returns true (markup was opened).
fn open_marker(out: &mut String, marker: &str, space: &mut bool) -> bool {
    if *space && !out.is_empty() && !out.ends_with([' ', '\n']) {
        out.push(' ');
    }
    *space = false;
    out.push_str(marker);
    true
}

/// A tag's attribute value (`href="..."`).
fn attribute<'t>(tag: &'t str, name: &str) -> Option<&'t str> {
    let at = tag.find(&format!("{name}="))? + name.len() + 1;
    let value = &tag[at..];
    let quote = value.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    value[1..].split(quote).next()
}

/// Text cut to about `max` bytes at a paragraph (or word) end, marked with `…`.
fn shorten(text: String, max: usize, paragraphs: bool) -> String {
    if text.len() <= max {
        return text;
    }
    let head = cut(&text, max);
    let at = paragraphs
        .then(|| head.rfind("\n\n"))
        .flatten()
        .filter(|&i| i > max / 2)
        .or_else(|| head.rfind(' ').filter(|&i| i > max / 2))
        .unwrap_or(head.len());
    format!("{} …", head[..at].trim_end())
}

/// The first sentence of a text, at most `max` bytes.
fn first_sentence(text: &str, max: usize) -> String {
    let end = text.match_indices(". ").map(|(i, _)| i + 1).find(|&i| i >= 20).unwrap_or(text.len());
    shorten(text[..end].trim().to_string(), max, false)
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

    fn folders(items: &[ImportedItem]) -> usize {
        items
            .iter()
            .map(|i| match i {
                ImportedItem::Folder { children, .. } => 1 + folders(children),
                ImportedItem::Request(_) => 0,
            })
            .sum()
    }

    fn find<'c>(c: &'c ImportedCollection, name: &str) -> &'c Request {
        requests(&c.items).into_iter().find(|r| r.name == name).unwrap_or_else(|| panic!("no request {name}"))
    }

    fn var<'c>(c: &'c ImportedCollection, key: &str) -> &'c Variable {
        c.variables.iter().find(|v| v.key == key).unwrap_or_else(|| panic!("no variable {key}"))
    }

    fn body_json(r: &Request) -> Value {
        assert_eq!(r.body.body_type, BodyType::Json, "{}", r.name);
        serde_json::from_str(&r.body.text).expect("body is JSON")
    }

    fn header<'r>(r: &'r Request, key: &str) -> &'r KeyValue {
        r.headers.iter().find(|h| h.key == key).unwrap_or_else(|| panic!("{}: no header {key}", r.name))
    }

    /// (key, value, enabled) of each form field.
    fn form(r: &Request) -> Vec<(&str, &str, bool)> {
        assert_eq!(r.body.body_type, BodyType::FormUrlencoded, "{}", r.name);
        r.body.form.iter().map(|f| (f.key.as_str(), f.value.as_str(), f.enabled)).collect()
    }

    fn sigv4(service: &str, region: &str) -> Auth {
        Auth::AwsSigV4(AwsSigV4Config {
            access_key: "{{awsAccessKeyId}}".into(),
            secret_key: "{{awsSecretAccessKey}}".into(),
            session_token: "{{awsSessionToken}}".into(),
            region: region.into(),
            service: service.into(),
            ..Default::default()
        })
    }

    const REST_JSON: &str = r#"{
      "smithy": "2.0",
      "shapes": {
        "example.lambda#Lambda": {
          "type": "service", "version": "2015-03-31",
          "operations": [{"target": "example.lambda#GetAccountSettings"}],
          "resources": [{"target": "example.lambda#FunctionResource"}],
          "traits": {
            "aws.api#service": {"sdkId": "Lambda", "endpointPrefix": "lambda"},
            "aws.auth#sigv4": {"name": "lambda"},
            "aws.protocols#restJson1": {},
            "smithy.api#title": "AWS  Lambda",
            "smithy.api#documentation": "<p>Runs code.</p>"
          }
        },
        "example.lambda#FunctionResource": {
          "type": "resource",
          "identifiers": {"FunctionName": {"target": "smithy.api#String"}},
          "put": {"target": "example.lambda#CreateFunction"},
          "list": {"target": "example.lambda#ListFunctions"},
          "collectionOperations": [{"target": "example.lambda#Invoke"}, {"target": "example.lambda#Echo"}],
          "resources": [{"target": "example.lambda#AliasResource"}],
          "traits": {"smithy.api#documentation": "<p>A <b>function</b>.</p>"}
        },
        "example.lambda#AliasResource": {
          "type": "resource",
          "read": {"target": "example.lambda#GetAlias"},
          "resources": [{"target": "example.lambda#FunctionResource"}]
        },
        "example.lambda#GetAccountSettings": {
          "type": "operation", "input": {"target": "smithy.api#Unit"},
          "traits": {"smithy.api#http": {"method": "GET", "uri": "/2016-08-19/account-settings"}, "smithy.api#auth": []}
        },
        "example.lambda#ListFunctions": {
          "type": "operation", "input": {"target": "example.lambda#ListFunctionsRequest"},
          "traits": {
            "smithy.api#http": {"method": "GET", "uri": "/2015-03-31/functions"},
            "smithy.api#paginated": {"inputToken": "Marker"},
            "smithy.api#examples": [{"title": "List", "input": {"Marker": "abc", "MaxItems": 5}}]
          }
        },
        "example.lambda#ListFunctionsRequest": {"type": "structure", "members": {
          "Marker": {"target": "smithy.api#String", "traits": {"smithy.api#httpQuery": "Marker"}},
          "MaxItems": {"target": "example.lambda#MaxItems",
                       "traits": {"smithy.api#httpQuery": "MaxItems", "smithy.api#documentation": "<p>The <i>most</i> functions to list.</p> <p>More.</p>"}},
          "Region": {"target": "smithy.api#String", "traits": {"smithy.api#httpQuery": "MasterRegion", "smithy.api#required": {}}}
        }},
        "example.lambda#MaxItems": {"type": "integer", "traits": {"smithy.api#range": {"min": 1, "max": 5}}},
        "example.lambda#CreateFunction": {
          "type": "operation", "input": {"target": "example.lambda#CreateFunctionRequest"},
          "traits": {"smithy.api#http": {"method": "post", "uri": "/2015-03-31/functions", "code": 201}, "smithy.api#deprecated": {}}
        },
        "example.lambda#CreateFunctionRequest": {"type": "structure", "members": {
          "FunctionName": {"target": "smithy.api#String", "traits": {"smithy.api#required": {}}},
          "Role": {"target": "smithy.api#String", "traits": {"smithy.api#required": {}, "smithy.api#jsonName": "role"}},
          "Runtime": {"target": "example.lambda#Runtime", "traits": {"smithy.api#required": {}}},
          "Code": {"target": "example.lambda#Code", "traits": {"smithy.api#required": {}}},
          "Tags": {"target": "example.lambda#Tags", "traits": {"smithy.api#required": {}}},
          "Created": {"target": "smithy.api#Timestamp", "traits": {"smithy.api#required": {}}},
          "Memory": {"target": "smithy.api#Integer", "traits": {"smithy.api#required": {}, "smithy.api#default": 128}},
          "ClientToken": {"target": "smithy.api#String", "traits": {"smithy.api#required": {}, "smithy.api#idempotencyToken": {}}},
          "Timeout": {"target": "smithy.api#Integer"},
          "Description": {"target": "smithy.api#String", "traits": {"smithy.api#jsonName": "description"}}
        }},
        "example.lambda#Runtime": {"type": "enum", "members": {
          "NODEJS": {"target": "smithy.api#Unit", "traits": {"smithy.api#enumValue": "nodejs22.x"}},
          "PYTHON": {"target": "smithy.api#Unit", "traits": {"smithy.api#enumValue": "python3.13"}}
        }},
        "example.lambda#Code": {"type": "structure", "members": {
          "ZipFile": {"target": "smithy.api#Blob", "traits": {"smithy.api#required": {}}},
          "S3Bucket": {"target": "smithy.api#String"}
        }},
        "example.lambda#Tags": {"type": "map", "key": {"target": "smithy.api#String"}, "value": {"target": "smithy.api#String"}},
        "example.lambda#Invoke": {
          "type": "operation", "input": {"target": "example.lambda#InvokeRequest"},
          "traits": {"smithy.api#http": {"method": "POST", "uri": "/2015-03-31/functions/{FunctionName}/invocations"},
                     "smithy.api#documentation": "<p>Calls a function.</p>"}
        },
        "example.lambda#InvokeRequest": {"type": "structure", "members": {
          "FunctionName": {"target": "smithy.api#String", "traits": {"smithy.api#httpLabel": {}, "smithy.api#required": {}}},
          "InvocationType": {"target": "smithy.api#String", "traits": {"smithy.api#httpHeader": "X-Amz-Invocation-Type"}},
          "Since": {"target": "smithy.api#Timestamp", "traits": {"smithy.api#httpHeader": "If-Modified-Since", "smithy.api#required": {}}},
          "Ids": {"target": "example.lambda#Ids", "traits": {"smithy.api#httpQuery": "id", "smithy.api#required": {}}},
          "Payload": {"target": "smithy.api#Blob", "traits": {"smithy.api#httpPayload": {}}},
          "Meta": {"target": "example.lambda#Tags", "traits": {"smithy.api#httpPrefixHeaders": "x-amz-meta-"}},
          "Extra": {"target": "example.lambda#Tags", "traits": {"smithy.api#httpQueryParams": {}}}
        }},
        "example.lambda#Ids": {"type": "list", "member": {"target": "smithy.api#String"}, "traits": {"smithy.api#length": {"min": 2}}},
        "example.lambda#Echo": {
          "type": "operation", "input": {"target": "example.lambda#EchoRequest"},
          "traits": {"smithy.api#http": {"method": "PUT", "uri": "/echo/{Name+}"},
                     "smithy.api#examples": [{"title": "Echo", "input": {"Name": "a/b.txt", "Payload": "{\"ping\": true}"}}]}
        },
        "example.lambda#EchoRequest": {"type": "structure", "members": {
          "Name": {"target": "smithy.api#String", "traits": {"smithy.api#httpLabel": {}, "smithy.api#required": {}}},
          "Payload": {"target": "smithy.api#Blob", "traits": {"smithy.api#httpPayload": {}}}
        }},
        "example.lambda#GetAlias": {
          "type": "operation", "input": {"target": "example.lambda#GetAliasRequest"},
          "traits": {"smithy.api#http": {"method": "GET", "uri": "/2015-03-31/functions/{FunctionName}/aliases/{Name}"}}
        },
        "example.lambda#GetAliasRequest": {"type": "structure", "members": {
          "FunctionName": {"target": "smithy.api#String", "traits": {"smithy.api#httpLabel": {}, "smithy.api#required": {}}},
          "Name": {"target": "smithy.api#String", "traits": {"smithy.api#httpLabel": {}, "smithy.api#required": {}}}
        }}
      }
    }"#;

    #[test]
    fn rest_json_service() {
        let c = import_smithy(REST_JSON).unwrap();
        assert_eq!(c.name, "AWS Lambda");
        assert_eq!(c.auth, sigv4("lambda", "{{awsRegion}}"));
        let vars: Vec<(&str, &str, bool)> =
            c.variables.iter().map(|v| (v.key.as_str(), v.value.as_str(), v.secret)).collect();
        assert_eq!(
            vars,
            [
                ("awsRegion", "us-east-1", false),
                ("awsAccessKeyId", "", true),
                ("awsSecretAccessKey", "", true),
                ("awsSessionToken", "", true),
                ("functionName", "example-function", false),
                ("aliasName", "example", false),
                ("echoName", "a/b.txt", false),
            ]
        );

        // Resources become folders (the cycle back to Function is not followed); the
        // service's own operations follow them.
        let outline: Vec<String> = c
            .items
            .iter()
            .map(|i| match i {
                ImportedItem::Folder { meta, children } => {
                    let names: Vec<String> = children
                        .iter()
                        .map(|c| match c {
                            ImportedItem::Folder { meta, children } => format!("{}/{}", meta.name, children.len()),
                            ImportedItem::Request(r) => format!("{}#{}", r.name, r.seq),
                        })
                        .collect();
                    format!("{} [{}] {:?}", meta.name, names.join(", "), meta.docs)
                }
                ImportedItem::Request(r) => format!("{}#{}", r.name, r.seq),
            })
            .collect();
        assert_eq!(
            outline,
            [
                "Function [Alias/1, CreateFunction#1, Echo#2, Invoke#3, ListFunctions#4] \"A **function**.\"",
                "GetAccountSettings#1"
            ]
        );
        assert!(matches!(&c.items[0], ImportedItem::Folder { meta, .. } if meta.seq == 0));

        let settings = find(&c, "GetAccountSettings");
        assert_eq!(
            (settings.method.as_str(), settings.url.as_str(), &settings.auth),
            ("GET", "https://lambda.{{awsRegion}}.amazonaws.com/2016-08-19/account-settings", &Auth::None)
        );
        assert_eq!(settings.body, Body::default());

        let list = find(&c, "ListFunctions");
        assert_eq!(list.url, "https://lambda.{{awsRegion}}.amazonaws.com/2015-03-31/functions?MasterRegion=us-east-1");
        assert_eq!(list.auth, Auth::Inherit);
        // The example's page token is not reused; its other values fill in the switched-off parameters.
        assert_eq!(
            list.disabled_params,
            [
                KeyValue { enabled: false, ..KeyValue::new("Marker", "example") },
                KeyValue {
                    enabled: false,
                    description: "The most functions to list.".into(),
                    ..KeyValue::new("MaxItems", "5")
                },
            ]
        );

        let create = find(&c, "CreateFunction");
        assert_eq!(create.method, "POST");
        assert_eq!(create.url, "https://lambda.{{awsRegion}}.amazonaws.com/2015-03-31/functions");
        assert_eq!(
            body_json(create),
            serde_json::json!({
                "FunctionName": "example-function",
                "role": "arn:aws:iam::123456789012:role/example-role",
                "Runtime": "nodejs22.x",
                "Code": {"ZipFile": "ZXhhbXBsZQ=="},
                "Tags": {"example": "example"},
                "Created": 1_704_110_400,
                "Memory": 128,
                "ClientToken": "{{$guid}}"
            })
        );
        assert_eq!(create.docs, "Deprecated.\n\nOptional body members: `Timeout`, `description`.");

        let invoke = find(&c, "Invoke");
        assert_eq!(
            invoke.url,
            "https://lambda.{{awsRegion}}.amazonaws.com/2015-03-31/functions/{{functionName}}/invocations?id=example&id=example"
        );
        assert_eq!(invoke.docs, "Calls a function.");
        assert_eq!(
            invoke.headers,
            [
                KeyValue { enabled: false, ..KeyValue::new("X-Amz-Invocation-Type", "example") },
                KeyValue::new("If-Modified-Since", "Mon, 01 Jan 2024 12:00:00 GMT"),
            ]
        );
        // A blob payload is a JSON document unless it streams or says otherwise.
        assert_eq!(body_json(invoke), serde_json::json!({}));

        let echo = find(&c, "Echo");
        assert_eq!(
            (echo.method.as_str(), echo.url.as_str()),
            ("PUT", "https://lambda.{{awsRegion}}.amazonaws.com/echo/{{echoName}}")
        );
        assert_eq!(echo.body.text, "{\"ping\": true}");
        assert_eq!(
            find(&c, "GetAlias").url,
            "https://lambda.{{awsRegion}}.amazonaws.com/2015-03-31/functions/{{functionName}}/aliases/{{aliasName}}"
        );

        assert_eq!(
            c.warnings,
            [
                "Headers starting with \"x-amz-meta-\" were not added (Invoke); add them by hand when needed",
                "Free-form query parameters were not added (Invoke); add them by hand when needed",
            ]
        );
    }

    const AWS_JSON: &str = r#"{
      "smithy": "2.0",
      "shapes": {
        "example.ddb#DynamoDB_20120810": {
          "type": "service", "version": "2012-08-10",
          "operations": [{"target": "example.ddb#PutItem"}, {"target": "example.ddb#ListTables"}, {"target": "example.ddb#StartSync"}],
          "traits": {"aws.api#service": {"sdkId": "DynamoDB", "endpointPrefix": "dynamodb"}, "aws.auth#sigv4": {"name": "dynamodb"},
                     "aws.protocols#awsJson1_0": {}}
        },
        "example.ddb#ListTables": {"type": "operation", "input": {"target": "example.ddb#ListTablesInput"}},
        "example.ddb#ListTablesInput": {"type": "structure", "members": {"Limit": {"target": "smithy.api#Integer"}}},
        "example.ddb#StartSync": {"type": "operation", "traits": {"smithy.api#endpoint": {"hostPrefix": "sync-"}}},
        "example.ddb#PutItem": {
          "type": "operation", "input": {"target": "example.ddb#PutItemInput"},
          "traits": {"smithy.api#examples": [{"title": "Put", "input": {
            "TableName": "Music", "Item": {"Artist": {"S": "No One"}}, "Expected": "2016-10-03T23:00:00Z", "Unknown": 1
          }}]}
        },
        "example.ddb#PutItemInput": {"type": "structure", "members": {
          "TableName": {"target": "smithy.api#String", "traits": {"smithy.api#required": {}}},
          "Item": {"target": "example.ddb#Item", "traits": {"smithy.api#required": {}, "smithy.api#jsonName": "item"}},
          "Expected": {"target": "smithy.api#Timestamp"},
          "Binary": {"target": "smithy.api#Blob", "traits": {"smithy.api#required": {}}},
          "Value": {"target": "example.ddb#AttributeValue", "traits": {"smithy.api#required": {}}},
          "Mode": {"target": "example.ddb#Mode", "traits": {"smithy.api#required": {}}}
        }},
        "example.ddb#Item": {"type": "map", "key": {"target": "smithy.api#String"}, "value": {"target": "example.ddb#AttributeValue"}},
        "example.ddb#AttributeValue": {"type": "union", "members": {
          "L": {"target": "example.ddb#List"},
          "S": {"target": "smithy.api#String"}
        }},
        "example.ddb#List": {"type": "list", "member": {"target": "example.ddb#AttributeValue"}},
        "example.ddb#Mode": {"type": "intEnum", "members": {"FAST": {"target": "smithy.api#Unit", "traits": {"smithy.api#enumValue": 2}}}}
      }
    }"#;

    #[test]
    fn aws_json_service() {
        let c = import_smithy(AWS_JSON).unwrap();
        assert_eq!(c.name, "DynamoDB");
        assert_eq!(c.auth, sigv4("dynamodb", "{{awsRegion}}"));
        assert!(c.warnings.is_empty(), "{:?}", c.warnings);
        let names: Vec<&str> = requests(&c.items).iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["ListTables", "PutItem", "StartSync"]);

        let list = find(&c, "ListTables");
        assert_eq!(
            (list.method.as_str(), list.url.as_str()),
            ("POST", "https://dynamodb.{{awsRegion}}.amazonaws.com/")
        );
        assert_eq!(
            list.headers,
            [
                KeyValue::new("X-Amz-Target", "DynamoDB_20120810.ListTables"),
                KeyValue::new("Content-Type", "application/x-amz-json-1.0"),
            ]
        );
        assert_eq!(body_json(list), serde_json::json!({}));
        assert_eq!(list.docs, "Optional body members: `Limit`.");

        // Example values first (unknown members dropped, timestamps as epoch seconds, no
        // `jsonName`), then generated ones; a recursive union stops where it would repeat.
        let put = find(&c, "PutItem");
        assert_eq!(
            body_json(put),
            serde_json::json!({
                "TableName": "Music",
                "Item": {"Artist": {"S": "No One"}},
                "Expected": 1_475_535_600,
                "Binary": "ZXhhbXBsZQ==",
                "Value": {"L": []},
                "Mode": 2
            })
        );
        assert_eq!(find(&c, "StartSync").url, "https://sync-dynamodb.{{awsRegion}}.amazonaws.com/");

        let c = import_smithy(&AWS_JSON.replace("awsJson1_0", "awsJson1_1")).unwrap();
        assert_eq!(header(find(&c, "PutItem"), "Content-Type").value, "application/x-amz-json-1.1");
    }

    const REST_XML: &str = r#"{
      "smithy": "2.0",
      "shapes": {
        "example.s3#AmazonS3": {
          "type": "service", "version": "2006-03-01",
          "operations": [
            {"target": "example.s3#ListBuckets"}, {"target": "example.s3#PutObject"}, {"target": "example.s3#DeleteObjects"},
            {"target": "example.s3#PutBucketAcl"}, {"target": "example.s3#ChangeRecords"}
          ],
          "traits": {
            "aws.api#service": {"sdkId": "S3", "endpointPrefix": "s3"},
            "aws.auth#sigv4": {"name": "s3"},
            "aws.protocols#restXml": {"noErrorWrapping": true},
            "smithy.api#xmlNamespace": {"uri": "http://s3.amazonaws.com/doc/2006-03-01/"},
            "smithy.rules#endpointTests": {"testCases": [
              {"params": {"Region": "us-west-2", "UseFIPS": false, "UseDualStack": false},
               "expect": {"endpoint": {"url": "https://s3.us-west-2.amazonaws.com"}}},
              {"params": {"Region": "us-east-1", "UseFIPS": true, "UseDualStack": false},
               "expect": {"endpoint": {"url": "https://s3-fips.us-east-1.amazonaws.com"}}},
              {"params": {"Region": "us-east-1", "Bucket": "b"}, "expect": {"endpoint": {"url": "https://b.s3.us-east-1.amazonaws.com"}}}
            ]}
          }
        },
        "example.s3#ListBuckets": {"type": "operation", "input": {"target": "smithy.api#Unit"},
                                   "traits": {"smithy.api#http": {"method": "GET", "uri": "/?x-id=ListBuckets"}}},
        "example.s3#PutObject": {"type": "operation", "input": {"target": "example.s3#PutObjectRequest"},
                                 "traits": {"smithy.api#http": {"method": "PUT", "uri": "/{Bucket}/{Key+}?x-id=PutObject"}}},
        "example.s3#PutObjectRequest": {"type": "structure", "members": {
          "Bucket": {"target": "smithy.api#String", "traits": {"smithy.api#httpLabel": {}, "smithy.api#required": {}}},
          "Key": {"target": "smithy.api#String", "traits": {"smithy.api#httpLabel": {}, "smithy.api#required": {}}},
          "Body": {"target": "example.s3#StreamingBlob", "traits": {"smithy.api#httpPayload": {}}},
          "ACL": {"target": "smithy.api#String", "traits": {"smithy.api#httpHeader": "x-amz-acl"}}
        }},
        "example.s3#StreamingBlob": {"type": "blob", "traits": {"smithy.api#streaming": {}}},
        "example.s3#DeleteObjects": {
          "type": "operation", "input": {"target": "example.s3#DeleteObjectsRequest"},
          "traits": {
            "smithy.api#http": {"method": "POST", "uri": "/{Bucket}?delete"},
            "aws.protocols#httpChecksum": {"requestChecksumRequired": true},
            "smithy.api#examples": [{"title": "Delete", "input": {
              "Bucket": "examplebucket", "Delete": {"Objects": [{"Key": "a.jpg"}, {"Key": "b & <c>"}], "Quiet": false}
            }}]
          }
        },
        "example.s3#DeleteObjectsRequest": {"type": "structure", "members": {
          "Bucket": {"target": "smithy.api#String", "traits": {"smithy.api#httpLabel": {}, "smithy.api#required": {}}},
          "Delete": {"target": "example.s3#Delete", "traits": {"smithy.api#httpPayload": {}, "smithy.api#required": {}, "smithy.api#xmlName": "Delete"}}
        }},
        "example.s3#Delete": {"type": "structure", "members": {
          "Objects": {"target": "example.s3#ObjectList", "traits": {"smithy.api#required": {}, "smithy.api#xmlFlattened": {}, "smithy.api#xmlName": "Object"}},
          "Quiet": {"target": "smithy.api#Boolean"}
        }},
        "example.s3#ObjectList": {"type": "list", "member": {"target": "example.s3#ObjectIdentifier"}},
        "example.s3#ObjectIdentifier": {"type": "structure", "members": {"Key": {"target": "smithy.api#String", "traits": {"smithy.api#required": {}}}}},
        "example.s3#PutBucketAcl": {"type": "operation", "input": {"target": "example.s3#PutBucketAclRequest"},
                                    "traits": {"smithy.api#http": {"method": "PUT", "uri": "/{Bucket}?acl"}}},
        "example.s3#PutBucketAclRequest": {"type": "structure", "members": {
          "Bucket": {"target": "smithy.api#String", "traits": {"smithy.api#httpLabel": {}, "smithy.api#required": {}}},
          "AccessControlPolicy": {"target": "example.s3#AccessControlPolicy",
                                  "traits": {"smithy.api#httpPayload": {}, "smithy.api#required": {}, "smithy.api#xmlName": "AccessControlPolicy"}}
        }},
        "example.s3#AccessControlPolicy": {"type": "structure", "members": {
          "Grants": {"target": "example.s3#Grants", "traits": {"smithy.api#required": {}, "smithy.api#xmlName": "AccessControlList"}},
          "Tags": {"target": "example.s3#Tags", "traits": {"smithy.api#required": {}}},
          "Owner": {"target": "example.s3#ObjectIdentifier"}
        }},
        "example.s3#Grants": {"type": "list", "member": {"target": "example.s3#Grant", "traits": {"smithy.api#xmlName": "Grant"}}},
        "example.s3#Grant": {"type": "structure", "members": {
          "Grantee": {"target": "example.s3#Grantee", "traits": {"smithy.api#required": {},
                      "smithy.api#xmlNamespace": {"prefix": "xsi", "uri": "http://www.w3.org/2001/XMLSchema-instance"}}}
        }},
        "example.s3#Grantee": {"type": "structure", "members": {
          "ID": {"target": "smithy.api#String", "traits": {"smithy.api#required": {}}},
          "Type": {"target": "smithy.api#String", "traits": {"smithy.api#required": {}, "smithy.api#xmlAttribute": {}, "smithy.api#xmlName": "xsi:type"}}
        }},
        "example.s3#Tags": {"type": "map", "key": {"target": "smithy.api#String", "traits": {"smithy.api#xmlName": "K"}},
                            "value": {"target": "smithy.api#String", "traits": {"smithy.api#xmlName": "V"}}},
        "example.s3#ChangeRecords": {"type": "operation", "input": {"target": "example.s3#ChangeResourceRecordSetsRequest"},
                                     "traits": {"smithy.api#http": {"method": "POST", "uri": "/2013-04-01/hostedzone/{Id}/rrset"}}},
        "example.s3#ChangeResourceRecordSetsRequest": {"type": "structure", "members": {
          "Id": {"target": "smithy.api#String", "traits": {"smithy.api#httpLabel": {}, "smithy.api#required": {}}},
          "ChangeBatch": {"target": "example.s3#ChangeBatch", "traits": {"smithy.api#required": {}}}
        }},
        "example.s3#ChangeBatch": {"type": "structure", "members": {
          "Comment": {"target": "smithy.api#String", "traits": {"smithy.api#required": {}}},
          "Changes": {"target": "example.s3#ObjectList"}
        }}
      }
    }"#;

    #[test]
    fn rest_xml_service() {
        let c = import_smithy(REST_XML).unwrap();
        assert_eq!(c.auth, sigv4("s3", "{{awsRegion}}"));
        assert_eq!(var(&c, "bucket").value, "examplebucket");
        assert_eq!(var(&c, "key").value, "example");
        assert_eq!(
            c.warnings,
            [
                "These operations need a Content-MD5 or x-amz-checksum-* header, which is not added for you: DeleteObjects"
            ]
        );

        // The endpoint comes from the model's own tests (a region-only case).
        let list = find(&c, "ListBuckets");
        assert_eq!(
            (list.method.as_str(), list.url.as_str()),
            ("GET", "https://s3.{{awsRegion}}.amazonaws.com/?x-id=ListBuckets")
        );
        assert_eq!(list.body, Body::default());

        let put = find(&c, "PutObject");
        assert_eq!(put.url, "https://s3.{{awsRegion}}.amazonaws.com/{{bucket}}/{{key}}?x-id=PutObject");
        assert_eq!(put.body, Body { body_type: BodyType::Binary, ..Body::default() });
        assert_eq!(put.headers, [KeyValue { enabled: false, ..KeyValue::new("x-amz-acl", "example") }]);

        let delete = find(&c, "DeleteObjects");
        assert_eq!(delete.url, "https://s3.{{awsRegion}}.amazonaws.com/{{bucket}}?delete");
        assert_eq!(delete.body.body_type, BodyType::Xml);
        assert_eq!(
            delete.body.text,
            "<Delete xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">\n  <Object>\n    <Key>a.jpg</Key>\n  </Object>\n  \
             <Object>\n    <Key>b &amp; &lt;c&gt;</Key>\n  </Object>\n  <Quiet>false</Quiet>\n</Delete>\n"
        );

        // Attributes, member namespaces, wrapped lists and maps.
        assert_eq!(
            find(&c, "PutBucketAcl").body.text,
            "<AccessControlPolicy xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">\n  <AccessControlList>\n    <Grant>\n      \
             <Grantee xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:type=\"example\">\n        \
             <ID>example</ID>\n      </Grantee>\n    </Grant>\n  </AccessControlList>\n  <Tags>\n    <entry>\n      \
             <K>example</K>\n      <V>example</V>\n    </entry>\n  </Tags>\n</AccessControlPolicy>\n"
        );

        // Without a payload member the input structure is the root element.
        let change = find(&c, "ChangeRecords");
        assert_eq!(change.url, "https://s3.{{awsRegion}}.amazonaws.com/2013-04-01/hostedzone/{{hostedzoneId}}/rrset");
        assert_eq!(
            change.body.text,
            "<ChangeResourceRecordSetsRequest xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">\n  <ChangeBatch>\n    \
             <Comment>Hello from Zorvik</Comment>\n  </ChangeBatch>\n</ChangeResourceRecordSetsRequest>\n"
        );
    }

    const AWS_QUERY: &str = r#"{
      "smithy": "2.0",
      "shapes": {
        "example.iam#Iam": {
          "type": "service", "version": "2010-05-08",
          "operations": [{"target": "example.iam#ListUsers"}, {"target": "example.iam#Publish"}],
          "traits": {
            "aws.api#service": {"sdkId": "IAM", "endpointPrefix": "iam"},
            "aws.auth#sigv4": {"name": "iam"},
            "aws.protocols#awsQuery": {},
            "smithy.rules#endpointTests": {"testCases": [
              {"params": {"Region": "us-east-1", "UseFIPS": false, "UseDualStack": false},
               "expect": {"endpoint": {"url": "https://iam.amazonaws.com",
                                       "properties": {"authSchemes": [{"name": "sigv4", "signingRegion": "us-east-1"}]}}}}
            ]}
          }
        },
        "example.iam#ListUsers": {"type": "operation", "input": {"target": "example.iam#ListUsersRequest"}},
        "example.iam#ListUsersRequest": {"type": "structure", "members": {
          "PathPrefix": {"target": "smithy.api#String", "traits": {"smithy.api#documentation": "<p>The path prefix for filtering the results.</p>"}},
          "MaxItems": {"target": "smithy.api#Integer"}
        }},
        "example.iam#Publish": {"type": "operation", "input": {"target": "example.iam#PublishInput"},
          "traits": {"smithy.api#examples": [{"title": "Publish", "input": {"Message": "Hi", "TopicArn": "arn:aws:sns:us-east-1:1:t"}}]}},
        "example.iam#PublishInput": {"type": "structure", "members": {
          "Message": {"target": "smithy.api#String", "traits": {"smithy.api#required": {}}},
          "TopicArn": {"target": "smithy.api#String"},
          "Names": {"target": "example.iam#Names", "traits": {"smithy.api#required": {}}},
          "Flat": {"target": "example.iam#Names", "traits": {"smithy.api#required": {}, "smithy.api#xmlFlattened": {}, "smithy.api#xmlName": "Item"}},
          "Attributes": {"target": "example.iam#Attributes", "traits": {"smithy.api#required": {}}},
          "Config": {"target": "example.iam#Config", "traits": {"smithy.api#required": {}}},
          "When": {"target": "smithy.api#Timestamp", "traits": {"smithy.api#required": {}}}
        }},
        "example.iam#Names": {"type": "list", "member": {"target": "smithy.api#String", "traits": {"smithy.api#xmlName": "Name"}}},
        "example.iam#Attributes": {"type": "map",
          "key": {"target": "smithy.api#String", "traits": {"smithy.api#xmlName": "Name"}},
          "value": {"target": "smithy.api#String", "traits": {"smithy.api#xmlName": "Value"}}},
        "example.iam#Config": {"type": "structure", "members": {
          "Enabled": {"target": "smithy.api#Boolean", "traits": {"smithy.api#required": {}, "smithy.api#xmlName": "IsEnabled"}}
        }}
      }
    }"#;

    #[test]
    fn aws_query_service() {
        let c = import_smithy(AWS_QUERY).unwrap();
        // A single endpoint (no region in its host) is signed for the region its tests give.
        assert_eq!(c.auth, sigv4("iam", "us-east-1"));
        assert_eq!(var(&c, "awsRegion").value, "us-east-1");
        let list = find(&c, "ListUsers");
        assert_eq!((list.method.as_str(), list.url.as_str()), ("POST", "https://iam.amazonaws.com/"));
        assert!(list.headers.is_empty());
        assert_eq!(
            form(list),
            [
                ("Action", "ListUsers", true),
                ("Version", "2010-05-08", true),
                ("PathPrefix", "/", false),
                ("MaxItems", "10", false)
            ]
        );
        assert_eq!(list.body.form[2].description, "The path prefix for filtering the results.");

        assert_eq!(
            form(find(&c, "Publish")),
            [
                ("Action", "Publish", true),
                ("Version", "2010-05-08", true),
                ("Message", "Hi", true),
                ("Names.Name.1", "example", true),
                ("Item.1", "example", true),
                ("Attributes.entry.1.Name", "example", true),
                ("Attributes.entry.1.Value", "example", true),
                ("Config.IsEnabled", "false", true),
                ("When", "2024-01-01T12:00:00Z", true),
                ("TopicArn", "arn:aws:sns:us-east-1:1:t", false),
            ]
        );
        assert!(c.warnings.is_empty(), "{:?}", c.warnings);
    }

    #[test]
    fn ec2_query_service() {
        let model = r#"{
          "smithy": "2.0",
          "shapes": {
            "example.ec2#Ec2": {"type": "service", "version": "2016-11-15", "operations": [{"target": "example.ec2#DescribeInstances"}],
              "traits": {"aws.api#service": {"sdkId": "EC2", "endpointPrefix": "ec2"}, "aws.auth#sigv4": {"name": "ec2"}, "aws.protocols#ec2Query": {}}},
            "example.ec2#DescribeInstances": {"type": "operation", "input": {"target": "example.ec2#DescribeInstancesRequest"}},
            "example.ec2#DescribeInstancesRequest": {"type": "structure", "members": {
              "InstanceIds": {"target": "example.ec2#Ids", "traits": {"smithy.api#xmlName": "InstanceId", "smithy.api#required": {}}},
              "Filters": {"target": "example.ec2#Filters", "traits": {"smithy.api#xmlName": "Filter"}},
              "DryRun": {"target": "smithy.api#Boolean", "traits": {"aws.protocols#ec2QueryName": "DryRun", "smithy.api#xmlName": "dryRun"}},
              "maxResults": {"target": "smithy.api#Integer"}
            }},
            "example.ec2#Ids": {"type": "list", "member": {"target": "smithy.api#String", "traits": {"smithy.api#xmlName": "item"}}},
            "example.ec2#Filters": {"type": "list", "member": {"target": "example.ec2#Filter"}},
            "example.ec2#Filter": {"type": "structure", "members": {
              "Name": {"target": "smithy.api#String", "traits": {"smithy.api#required": {}}},
              "Values": {"target": "example.ec2#Ids", "traits": {"smithy.api#xmlName": "Value", "smithy.api#required": {}}}
            }}
          }
        }"#;
        let c = import_smithy(model).unwrap();
        let describe = find(&c, "DescribeInstances");
        assert_eq!(describe.url, "https://ec2.{{awsRegion}}.amazonaws.com/");
        // Lists are always flattened; names are capitalized `xmlName`s unless `ec2QueryName` says otherwise.
        assert_eq!(
            form(describe),
            [
                ("Action", "DescribeInstances", true),
                ("Version", "2016-11-15", true),
                ("InstanceId.1", "example-instance-id", true),
                ("Filter.1.Name", "example", false),
                ("Filter.1.Value.1", "example", false),
                ("DryRun", "false", false),
                ("MaxResults", "10", false),
            ]
        );
    }

    #[test]
    fn protocols_are_picked_like_the_aws_sdks() {
        let model = |protocols: &str| {
            format!(
                r#"{{"smithy": "2.0", "shapes": {{
                  "example.cw#Granite": {{"type": "service", "version": "2010-08-01", "operations": [{{"target": "example.cw#ListMetrics"}}],
                    "traits": {{"aws.api#service": {{"sdkId": "CloudWatch", "endpointPrefix": "monitoring"}},
                                "aws.auth#sigv4": {{"name": "monitoring"}}, {protocols}}}}},
                  "example.cw#ListMetrics": {{"type": "operation", "input": {{"target": "example.cw#ListMetricsInput"}}}},
                  "example.cw#ListMetricsInput": {{"type": "structure", "members": {{
                    "Namespace": {{"target": "smithy.api#String", "traits": {{"smithy.api#required": {{}}}}}}
                  }}}}
                }}}}"#
            )
        };
        // JSON wins over Query, and over CBOR (whose binary bodies Zorvik can't write).
        let c = import_smithy(&model(
            r#""smithy.protocols#rpcv2Cbor": {}, "aws.protocols#awsQuery": {}, "aws.protocols#awsJson1_0": {}"#,
        ))
        .unwrap();
        let list = find(&c, "ListMetrics");
        assert_eq!(header(list, "X-Amz-Target").value, "Granite.ListMetrics");
        assert_eq!(body_json(list), serde_json::json!({"Namespace": "example"}));
        assert!(c.warnings.is_empty(), "{:?}", c.warnings);

        let c = import_smithy(&model(r#""smithy.protocols#rpcv2Cbor": {}"#)).unwrap();
        let list = find(&c, "ListMetrics");
        assert_eq!(list.url, "https://monitoring.{{awsRegion}}.amazonaws.com/service/Granite/operation/ListMetrics");
        assert_eq!(header(list, "smithy-protocol").value, "rpc-v2-cbor");
        assert_eq!(body_json(list), serde_json::json!({"Namespace": "example"}));
        assert!(c.warnings[0].contains("CBOR"), "{:?}", c.warnings);

        let c = import_smithy(&model(r#""aws.protocols#somethingElse": {}"#)).unwrap();
        assert_eq!(find(&c, "ListMetrics").method, "POST");
        assert!(c.warnings.iter().any(|w| w.contains("no supported protocol")), "{:?}", c.warnings);
    }

    #[test]
    fn other_smithy_services() {
        // Not an AWS service: the address is a variable, auth comes from the model's own schemes.
        let model = r#"{
          "smithy": "2.0",
          "shapes": {
            "example.weather#Helper": {"type": "service"},
            "example.weather#Weather": {"type": "service", "version": "1",
              "operations": [{"target": "example.weather#GetCity"}],
              "traits": {"aws.protocols#restJson1": {}, "smithy.api#httpBearerAuth": {}, "smithy.api#httpApiKeyAuth": {"name": "X-Key", "in": "header"},
                         "smithy.api#auth": ["smithy.api#httpApiKeyAuth", "smithy.api#httpBearerAuth"]}},
            "example.weather#GetCity": {"type": "operation", "input": {"target": "example.weather#GetCityInput"},
              "traits": {"smithy.api#http": {"method": "GET", "uri": "/cities/{cityId}"}}},
            "example.weather#GetCityInput": {"type": "structure", "mixins": [{"target": "example.weather#CityMixin"}], "members": {}},
            "example.weather#CityMixin": {"type": "structure", "members": {
              "cityId": {"target": "smithy.api#String", "traits": {"smithy.api#httpLabel": {}, "smithy.api#required": {}}}
            }},
            "example.weather#Ping": {"type": "operation"}
          }
        }"#;
        let c = import_smithy(model).unwrap();
        assert_eq!(c.name, "Weather");
        assert_eq!(
            c.auth,
            Auth::ApiKey { key: "X-Key".into(), value: "{{apiKey}}".into(), location: Default::default() }
        );
        let keys: Vec<(&str, bool)> = c.variables.iter().map(|v| (v.key.as_str(), v.secret)).collect();
        assert_eq!(keys, [("baseUrl", false), ("apiKey", true), ("cityId", false)]);
        assert_eq!(find(&c, "GetCity").url, "{{baseUrl}}/cities/{{cityId}}");
        assert_eq!(c.request_count(), 1);
        assert!(c.warnings.iter().any(|w| w.contains("2 services; only Weather")), "{:?}", c.warnings);
        assert!(c.warnings.iter().any(|w| w.contains("baseUrl")), "{:?}", c.warnings);
    }

    #[test]
    fn endpoints() {
        let case = |region: &str, url: &str, signing: Option<&str>| {
            let mut endpoint = serde_json::json!({"url": url});
            if let Some(s) = signing {
                endpoint["properties"] = serde_json::json!({"authSchemes": [{"signingRegion": s}]});
            }
            serde_json::json!({"params": {"Region": region, "UseFIPS": false, "UseDualStack": false}, "expect": {"endpoint": endpoint}})
        };
        let tests = |cases: Vec<Value>| serde_json::json!({"testCases": cases});
        let regional = |host: &str| (format!("https://{host}"), None);
        let single = |host: &str, region: &str| (format!("https://{host}"), Some(region.to_string()));

        assert_eq!(aws_endpoint("kinesis", None), regional("kinesis.{{awsRegion}}.amazonaws.com"));
        assert_eq!(aws_endpoint("organizations", None), single("organizations.us-east-1.amazonaws.com", "us-east-1"));
        // Newer services live under api.aws.
        let t = tests(vec![case("us-east-1", "https://billing.us-east-1.api.aws", None)]);
        assert_eq!(aws_endpoint("billing", Some(&t)), regional("billing.{{awsRegion}}.api.aws"));
        // `aws-global` answering like a region means one endpoint.
        let t = tests(vec![
            case("aws-global", "https://shield.us-east-1.amazonaws.com", Some("us-east-1")),
            case("us-east-1", "https://shield.us-east-1.amazonaws.com", None),
        ]);
        assert_eq!(aws_endpoint("shield", Some(&t)), single("shield.us-east-1.amazonaws.com", "us-east-1"));
        // ...unlike STS, whose global endpoint is an option.
        let t = tests(vec![
            case("aws-global", "https://sts.amazonaws.com", Some("us-east-1")),
            case("eu-west-1", "https://sts.eu-west-1.amazonaws.com", None),
            case("us-east-1", "https://sts.us-east-1.amazonaws.com", None),
        ]);
        assert_eq!(aws_endpoint("sts", Some(&t)), regional("sts.{{awsRegion}}.amazonaws.com"));
        let t = tests(vec![case("us-east-1", "https://cloudfront.amazonaws.com/", Some("us-east-1"))]);
        assert_eq!(aws_endpoint("cloudfront", Some(&t)), single("cloudfront.amazonaws.com", "us-east-1"));
        // Other partitions, other parameters and odd URLs are ignored.
        let mut fips = case("us-east-1", "https://x-fips.us-east-1.amazonaws.com", None);
        fips["params"]["UseFIPS"] = Value::Bool(true);
        let t = tests(vec![
            fips,
            case("cn-north-1", "https://x.cn-north-1.amazonaws.com.cn", None),
            case("us-gov-west-1", "https://x.us-gov-west-1.amazonaws.com", None),
            case("us-east-1", "https://{{evil}}.example.com", None),
            case("us-east-1", "https://x.us-east-1.amazonaws.com/path", None),
            case("us-east-1", "http://x.us-east-1.amazonaws.com", None),
            serde_json::json!({"params": {"Region": "us-east-1"}, "expect": {"error": "no"}}),
            serde_json::json!("junk"),
        ]);
        assert_eq!(aws_endpoint("x", Some(&t)), regional("x.{{awsRegion}}.amazonaws.com"));
    }

    #[test]
    fn invalid_input_errors() {
        for (text, needle) in [
            ("", "empty"),
            ("\u{feff}  ", "empty"),
            ("not json", "Invalid JSON"),
            ("[1, 2]", "Not a Smithy"),
            (r#"{"smithy": 2, "shapes": {}}"#, "Not a Smithy"),
            (r#"{"smithy": "2.0", "shapes": []}"#, "Not a Smithy"),
            (r#"{"smithy": "2.0", "shapes": {"a#B": {"type": "structure"}}}"#, "no service"),
        ] {
            let err = import_smithy(text).unwrap_err();
            assert!(err.0.contains(needle), "{text:?}: {err}");
        }
        assert!(is_smithy(&serde_json::json!({"smithy": "1.0", "shapes": {}})));
        assert!(!is_smithy(&serde_json::json!({"openapi": "3.0.0", "paths": {}})));
        // Nesting deeper than serde_json allows is refused, not a stack overflow.
        let deep = format!(r#"{{"smithy": "2.0", "shapes": {}{}}}"#, "[".repeat(10_000), "]".repeat(10_000));
        assert!(import_smithy(&deep).is_err());
    }

    #[test]
    fn malformed_parts_do_not_panic() {
        let model = r#"{
          "smithy": "2.0",
          "shapes": {
            "a#S": {"type": "service", "version": 3,
              "operations": [{"target": "a#Bad"}, {"target": "a#Missing"}, {"target": 7}, "x", {"target": "a#NoType"}, {"target": "a#Weird"},
                             {"target": "a#Struct"}],
              "resources": [{"target": "a#R"}, {"target": "a#Missing"}, {"target": "a#Struct"}],
              "traits": {"aws.api#service": {"endpointPrefix": "Bad Host/"}, "aws.auth#sigv4": {"name": ""},
                         "aws.protocols#restJson1": {}, "smithy.api#title": 5, "smithy.api#auth": "nope"}},
            "a#R": {"type": "resource", "operations": "x", "resources": [{"target": "a#R"}], "read": {"target": 1}},
            "a#Bad": {"type": "operation", "input": {"target": "a#BadInput"},
                      "traits": {"smithy.api#http": {"method": "GE T", "uri": "no-slash/{x"}, "smithy.api#examples": "x",
                                 "smithy.api#endpoint": {"hostPrefix": "{Nope}.\"/"}}},
            "a#BadInput": {"type": "structure", "members": {
              "a": {"target": "a#Missing", "traits": {"smithy.api#required": {}}},
              "b": {"target": 1, "traits": {"smithy.api#required": {}}},
              "c": "not a member",
              "d": {"target": "a#List", "traits": {"smithy.api#required": {}, "smithy.api#httpQuery": 5}},
              "e": {"target": "a#Map", "traits": {"smithy.api#required": {}}},
              "f": {"target": "a#Enum", "traits": {"smithy.api#required": {}, "smithy.api#httpHeader": "X-E"}},
              "g": {"target": "a#Num", "traits": {"smithy.api#required": {}, "smithy.api#default": "x"}},
              "h": {"target": "smithy.api#String", "traits": {"smithy.api#required": {}, "smithy.api#length": {"min": 99999999, "max": -1}}},
              "i": {"target": "smithy.api#Timestamp", "traits": {"smithy.api#required": {}, "smithy.api#timestampFormat": "weird"}},
              "j": {"target": "smithy.api#Unknown", "traits": {"smithy.api#required": {}}},
              "k": {"target": "a#Struct", "traits": {"smithy.api#required": {}, "smithy.api#httpPayload": {}}}
            }, "mixins": [{"target": "a#BadInput"}, {"target": 3}]},
            "a#List": {"type": "list"},
            "a#Map": {"type": "map", "key": {"target": "smithy.api#String"}},
            "a#Enum": {"type": "enum", "members": []},
            "a#Num": {"type": "integer", "traits": {"smithy.api#range": {"min": "1", "max": 1e300}}},
            "a#NoType": {"traits": {}},
            "a#Weird": {"type": "operation", "input": "x", "traits": []},
            "a#Struct": {"type": "structure", "members": {"self": {"target": "a#Struct", "traits": {"smithy.api#required": {}}}}}
          }
        }"#;
        let c = import_smithy(model).unwrap();
        let names: Vec<&str> = requests(&c.items).iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["Bad", "Weird"]);
        let bad = find(&c, "Bad");
        assert_eq!(bad.method, "POST");
        assert!(bad.url.starts_with("https://service.{{awsRegion}}.amazonaws.com/"), "{}", bad.url);
        assert_eq!(c.auth, Auth::None);
        for needle in ["\"a#Missing\" is not defined", "no valid HTTP binding", "\"a#NoType\" is not an operation"] {
            assert!(c.warnings.iter().any(|w| w.contains(needle)), "no warning for {needle}: {:?}", c.warnings);
        }
    }

    #[test]
    fn model_names_cannot_reference_variables() {
        let model = r#"{
          "smithy": "2.0",
          "shapes": {
            "a#S": {"type": "service", "version": "1", "operations": [{"target": "a#Json"}, {"target": "a#Path"}, {"target": "a#Host"}],
                    "traits": {"aws.api#service": {"endpointPrefix": "x"}, "aws.auth#sigv4": {"name": "x"}, "aws.protocols#restJson1": {}}},
            "a#Json": {"type": "operation", "input": {"target": "a#In"}, "traits": {"smithy.api#http": {"method": "POST", "uri": "/j"}}},
            "a#Path": {"type": "operation", "traits": {"smithy.api#http": {"method": "GET", "uri": "/{{awsSecretAccessKey}}"}}},
            "a#Host": {"type": "operation", "traits": {"smithy.api#http": {"method": "GET", "uri": "/h"},
                                                       "smithy.api#endpoint": {"hostPrefix": "{{awsSecretAccessKey}}."}}},
            "a#In": {"type": "structure", "members": {
              "{{awsSecretAccessKey}}": {"target": "smithy.api#String", "traits": {"smithy.api#required": {}}},
              "b": {"target": "smithy.api#String", "traits": {"smithy.api#required": {}, "smithy.api#jsonName": "{{awsSessionToken}}"}},
              "c": {"target": "smithy.api#String", "traits": {"smithy.api#required": {}, "smithy.api#httpHeader": "X-{{awsAccessKeyId}}"}},
              "d": {"target": "smithy.api#String", "traits": {"smithy.api#required": {}, "smithy.api#httpQuery": "{{awsAccessKeyId}}"}}
            }}
          }
        }"#;
        let c = import_smithy(model).unwrap();
        for r in requests(&c.items) {
            let json = serde_json::to_string(r).unwrap();
            for secret in [ACCESS_KEY_VAR, SECRET_KEY_VAR, SESSION_TOKEN_VAR] {
                assert!(!json.contains(&format!("{{{{{secret}")), "{json}");
            }
        }
        assert_eq!(find(&c, "Path").url, "https://x.{{awsRegion}}.amazonaws.com/");
        assert_eq!(find(&c, "Host").url, "https://x.{{awsRegion}}.amazonaws.com/h");
        let json = find(&c, "Json");
        assert_eq!(json.url, "https://x.{{awsRegion}}.amazonaws.com/j?awsAccessKeyId=example");
        assert!(json.headers.is_empty());
        assert_eq!(body_json(json), serde_json::json!({"awsSecretAccessKey": "example", "awsSessionToken": "example"}));
        assert!(c.warnings.iter().any(|w| w.contains("header name")), "{:?}", c.warnings);
    }

    #[test]
    fn shape_and_auth_names_cannot_reference_variables() {
        let secrets = [ACCESS_KEY_VAR, SECRET_KEY_VAR, SESSION_TOKEN_VAR].map(|s| format!("{{{{{s}"));
        // What a request sends (its name is only shown).
        let leaks = |c: &ImportedCollection| {
            let sent = |r: &Request| serde_json::to_string(&Request { name: String::new(), ..r.clone() }).unwrap();
            requests(&c.items).into_iter().map(sent).find(|t| secrets.iter().any(|s| t.contains(s.as_str())))
        };
        // The service's and operations' names and version go into headers, forms and URLs.
        for protocol in ["aws.protocols#awsJson1_0", "aws.protocols#awsQuery", "smithy.protocols#rpcv2Cbor"] {
            let model = serde_json::json!({"smithy": "2.0", "shapes": {
                "a#{{awsSecretAccessKey}}": {"type": "service", "version": "{{awsSessionToken}}",
                    "operations": [{"target": "a#{{awsAccessKeyId}}"}],
                    "traits": {"aws.api#service": {"endpointPrefix": "x"}, "aws.auth#sigv4": {"name": "x"}, protocol: {}}},
                "a#{{awsAccessKeyId}}": {"type": "operation"}
            }});
            let c = import_smithy(&model.to_string()).unwrap();
            assert_eq!(c.request_count(), 1);
            assert_eq!(leaks(&c), None, "{protocol}");
        }
        // An API key's header name must be one; other names lose their braces.
        let model = |name: &str, place: &str| {
            serde_json::json!({"smithy": "2.0", "shapes": {
                "a#S": {"type": "service", "version": "1", "operations": [{"target": "a#Op"}],
                    "traits": {"aws.protocols#restJson1": {}, "smithy.api#httpBearerAuth": {},
                               "smithy.api#httpApiKeyAuth": {"name": name, "in": place, "scheme": "{{awsSecretAccessKey}}"},
                               "smithy.api#auth": ["smithy.api#httpApiKeyAuth", "smithy.api#httpBearerAuth"]}},
                "a#Op": {"type": "operation", "traits": {"smithy.api#http": {"method": "GET", "uri": "/"}}}
            }})
            .to_string()
        };
        let c = import_smithy(&model("X-{{awsSecretAccessKey}}", "header")).unwrap();
        assert!(matches!(c.auth, Auth::Bearer { .. }), "{:?}", c.auth);
        let c = import_smithy(&model("{{awsSecretAccessKey}}", "query")).unwrap();
        assert_eq!(
            c.auth,
            Auth::ApiKey {
                key: "awsSecretAccessKey".into(),
                value: "awsSecretAccessKey {{apiKey}}".into(),
                location: crate::model::ApiKeyLocation::Query
            }
        );
        assert_eq!(leaks(&c), None);
    }

    /// A Query model of `operations` operations sharing one input whose required members fan out
    /// `width` ways for `depth` levels, plus optional members sharing one long documentation.
    fn fan_out_model(operations: usize, width: usize, depth: usize) -> String {
        let mut shapes = Map::new();
        let ops: Vec<Value> = (0..operations).map(|i| serde_json::json!({"target": format!("a#Op{i}")})).collect();
        shapes.insert(
            "a#S".into(),
            serde_json::json!({"type": "service", "version": "1", "operations": ops,
                "traits": {"aws.api#service": {"endpointPrefix": "x"}, "aws.auth#sigv4": {"name": "x"}, "aws.protocols#awsQuery": {}}}),
        );
        let docs = "<p>Shared <b>words</b> without an end".repeat(40_000);
        shapes.insert(
            "a#Text".into(),
            serde_json::json!({"type": "string", "traits": {"smithy.api#documentation": docs}}),
        );
        for i in 0..operations {
            shapes.insert(
                format!("a#Op{i}"),
                serde_json::json!({"type": "operation", "input": {"target": "a#L0"},
                                   "traits": {"smithy.api#documentation": format!("<p>Op {i}.</p>{}", "<p>More words.</p>".repeat(500))}}),
            );
        }
        for level in 0..=depth {
            let mut members: Map<String, Value> = (0..width)
                .map(|m| {
                    let target = if level == depth { "a#Text".to_string() } else { format!("a#L{}", level + 1) };
                    (format!("m{m}"), serde_json::json!({"target": target, "traits": {"smithy.api#required": {}}}))
                })
                .collect();
            for m in 0..MAX_OPTIONAL + 10 {
                members.insert(format!("o{m}"), serde_json::json!({"target": "a#Text"}));
            }
            shapes.insert(format!("a#L{level}"), serde_json::json!({"type": "structure", "members": members}));
        }
        serde_json::json!({"smithy": "2.0", "shapes": shapes}).to_string()
    }

    #[test]
    fn shared_shapes_cannot_blow_up_the_output() {
        // 8^10 form fields if nothing stopped it.
        let text = fan_out_model(60, 8, 10);
        let start = std::time::Instant::now();
        let c = import_smithy(&text).unwrap();
        assert!(start.elapsed().as_secs() < 30, "took {:?}", start.elapsed());
        assert_eq!(c.request_count(), 60);
        let size: usize = requests(&c.items).iter().map(|r| serde_json::to_vec(r).unwrap().len()).sum();
        assert!(size < 200 << 20, "{size}");
        for r in requests(&c.items) {
            assert!(r.docs.len() <= MAX_DOCS + 8, "{}", r.docs.len());
            assert!(r.body.form.iter().filter(|f| !f.enabled).count() <= MAX_OPTIONAL);
            assert!(r.body.form.iter().all(|f| f.description.len() <= MAX_DESCRIPTION + 8));
        }
    }

    #[test]
    fn many_resources_and_labels_import_in_linear_time() {
        // Folders sharing a name are numbered, and labels share variables case-insensitively,
        // without comparing each with all the others.
        let n = 20_000;
        let mut shapes = Map::new();
        let mut resources = Vec::new();
        for i in 0..n {
            resources.push(serde_json::json!({"target": format!("n{i}#Thing")}));
            shapes.insert(
                format!("n{i}#Thing"),
                serde_json::json!({"type": "resource", "operations": [{"target": format!("n{i}#Get")}]}),
            );
            shapes.insert(
                format!("n{i}#Get"),
                serde_json::json!({"type": "operation", "input": {"target": format!("n{i}#In")},
                                   "traits": {"smithy.api#http": {"method": "GET", "uri": format!("/x/{{Label{i}}}")}}}),
            );
            shapes.insert(
                format!("n{i}#In"),
                serde_json::json!({"type": "structure", "members": {format!("Label{i}"): {"target": "smithy.api#String",
                                   "traits": {"smithy.api#required": {}, "smithy.api#httpLabel": {}}}}}),
            );
        }
        shapes.insert(
            "a#S".into(),
            serde_json::json!({"type": "service", "version": "1", "resources": resources,
                "traits": {"aws.api#service": {"endpointPrefix": "x"}, "aws.protocols#restJson1": {}}}),
        );
        let start = std::time::Instant::now();
        let c = import_smithy(&serde_json::json!({"smithy": "2.0", "shapes": shapes}).to_string()).unwrap();
        assert!(start.elapsed().as_secs() < 20, "took {:?}", start.elapsed());
        assert_eq!(c.request_count(), n);
        let names: Vec<&str> = c
            .items
            .iter()
            .filter_map(|i| match i {
                ImportedItem::Folder { meta, .. } => Some(meta.name.as_str()),
                ImportedItem::Request(_) => None,
            })
            .collect();
        assert_eq!(names[..3], ["Thing", "Thing 2", "Thing 3"]);
        assert_eq!(names.iter().collect::<HashSet<_>>().len(), n);
        assert_eq!(c.variables.iter().filter(|v| v.key.starts_with("label")).count(), n);
    }

    #[test]
    fn documentation_full_of_open_brackets_converts_in_linear_time() {
        // No `>` closes any of these `<`: each must not be searched for one again.
        let html = format!("<fullname>{}", "<a".repeat(500_000));
        let start = std::time::Instant::now();
        assert_eq!(html_to_markdown(&html, MAX_DOCS), "");
        assert!(start.elapsed().as_secs() < 2, "took {:?}", start.elapsed());
        assert_eq!(html_to_text("a < b and c <d", 100), "a < b and c <d");
        assert_eq!(html_to_text("<p>x <b", 100), "x <b");
    }

    #[test]
    fn recursive_shapes_terminate() {
        let model = r#"{
          "smithy": "2.0",
          "shapes": {
            "a#S": {"type": "service", "version": "1", "operations": [{"target": "a#Op"}],
                    "traits": {"aws.api#service": {"endpointPrefix": "x"}, "aws.auth#sigv4": {"name": "x"}, "aws.protocols#restXml": {}}},
            "a#Op": {"type": "operation", "input": {"target": "a#Node"}, "traits": {"smithy.api#http": {"method": "POST", "uri": "/"}}},
            "a#Node": {"type": "structure", "members": {
              "Next": {"target": "a#Other", "traits": {"smithy.api#required": {}}},
              "Children": {"target": "a#Nodes", "traits": {"smithy.api#required": {}}},
              "Choice": {"target": "a#Choice", "traits": {"smithy.api#required": {}}}
            }},
            "a#Other": {"type": "structure", "members": {"Back": {"target": "a#Node", "traits": {"smithy.api#required": {}}}}},
            "a#Nodes": {"type": "list", "member": {"target": "a#Node"}},
            "a#Choice": {"type": "union", "members": {"Again": {"target": "a#Choice"}, "Leaf": {"target": "smithy.api#String"}}}
          }
        }"#;
        let c = import_smithy(model).unwrap();
        let op = find(&c, "Op");
        assert_eq!(
            op.body.text,
            "<Node>\n  <Next/>\n  <Children/>\n  <Choice>\n    <Leaf>example</Leaf>\n  </Choice>\n</Node>\n"
        );

        // Example values nest as deep as the example does, up to a limit.
        let mut example = serde_json::json!({"Leaf": "end"});
        for _ in 0..100 {
            example = serde_json::json!({"Again": example});
        }
        let mut doc: Value = serde_json::from_str(model).unwrap();
        doc["shapes"]["a#Op"]["traits"]["smithy.api#examples"] = serde_json::json!([{"input": {"Choice": example}}]);
        let c = import_smithy(&doc.to_string()).unwrap();
        let text = &find(&c, "Op").body.text;
        let levels = text.matches("<Again").count();
        assert!((10..=MAX_NESTING).contains(&levels) && !text.contains("end"), "{text}");
    }

    #[test]
    fn documentation_becomes_markdown() {
        let html = "<fullname>Amazon Thing</fullname><p>Use <code>Get*</code> to <b> read </b> a <i>thing</i>, \
                    see <a href=\"https://docs.aws.amazon.com/x?a=1&amp;b=2\">the guide</a> or \
                    <a href=\"javascript:alert(1)\">this</a>.</p>\n<note><p>Costs &lt;money&gt; &amp; time&#39;s.</p></note>\
                    <ul><li><p>One</p></li><li>Two<ul><li>Nested</li></ul></li></ul><p>a_b [c] #1 &#x1F600; &bogus;</p><br/>";
        assert_eq!(
            html_to_markdown(html, MAX_DOCS),
            "Use `Get*` to **read** a _thing_, see [the guide](https://docs.aws.amazon.com/x?a=1&b=2) or this.\n\n\
             **Note:** Costs \\<money\\> & time's.\n\n- One\n- Two\n  - Nested\n\na\\_b \\[c\\] \\#1 😀 &bogus;"
        );
        assert_eq!(html_to_text("<p>Hello <b>world</b>.</p><p>Again.</p>", 100), "Hello world. Again.");
        assert_eq!(first_sentence("This sentence is long enough. And more.", 200), "This sentence is long enough.");
        assert_eq!(first_sentence("Short. e.g. more text here.", 200), "Short. e.g. more text here.");
        let long = "<p>word </p>".repeat(2000);
        let md = html_to_markdown(&long, 100);
        assert!(md.len() <= 104 && md.ends_with(" …"), "{md}");
        // Unclosed tags, lone brackets and multi-byte text.
        assert_eq!(html_to_markdown("é < 3 <b", 100), "é \\< 3 \\<b");
        assert_eq!(html_to_markdown("<!-- hidden --><p>x</p><!-- open", 100), "x");
    }

    #[test]
    fn timestamps() {
        assert_eq!(date_time(EXAMPLE_EPOCH), "2024-01-01T12:00:00Z");
        assert_eq!(http_date(EXAMPLE_EPOCH), "Mon, 01 Jan 2024 12:00:00 GMT");
        assert_eq!(date_time(0), "1970-01-01T00:00:00Z");
        assert_eq!(http_date(-1), "Wed, 31 Dec 1969 23:59:59 GMT");
        assert_eq!(parse_date_time("2016-10-03T23:00:00Z"), Some(1_475_535_600));
        assert_eq!(parse_date_time("2024-02-29T01:02:03.456+05:30"), Some(1_709_148_723));
        for bad in [
            "2024-13-01T00:00:00Z",
            "2024-01-01 00:00:00Z",
            "2024-01-01T00:00:00",
            "x",
            "2024-01-01T00:00:00+x",
            // Offsets too large to be one (they overflowed the arithmetic).
            "2024-01-01T00:00:00+9999999999999999:00",
            "2024-01-01T00:00:00-01:999999999999999999",
            "2024-01-01T00:00:00+24:00",
        ] {
            assert_eq!(parse_date_time(bad), None, "{bad}");
        }
        let at = |v: Value, f| timestamp_value(&v, f);
        assert_eq!(at(Value::from(1.9), "epoch-seconds"), Value::from(1));
        assert_eq!(at(Value::from("2016-10-03T23:00:00Z"), "epoch-seconds"), Value::from(1_475_535_600));
        assert_eq!(at(Value::from(1e300), "date-time"), Value::from("9999-12-31T23:59:59Z"));
        assert_eq!(at(Value::from(-1e300), "date-time"), Value::from("0001-01-01T00:00:00Z"));
        assert_eq!(at(Value::from("soon"), "epoch-seconds"), Value::from("soon"));
    }

    #[test]
    fn helpers() {
        assert_eq!(label_variable("FunctionName", Some("functions")), "functionName");
        assert_eq!(label_variable("Id", Some("hostedzone")), "hostedzoneId");
        assert_eq!(label_variable("Name", Some("aliases")), "aliasName");
        assert_eq!(label_variable("restapi_id", None), "restapiId");
        assert_eq!(label_variable("Key", None), "key");
        assert_eq!(label_variable("+", None), "label");
        for (plural, one) in [("Policies", "Policy"), ("aliases", "alias"), ("addresses", "address"), ("tags", "tag")] {
            assert_eq!(singular(plural), one);
        }
        for word in ["status", "analysis", "access"] {
            assert_eq!(singular(word), word);
        }
        assert_eq!(templates("/{Bucket}/{Key+}?x"), [("/", Some("Bucket")), ("/", Some("Key+")), ("?x", None)]);
        assert_eq!(templates("/a/{}/{b"), [("/a/{}/{b", None)]);
        assert_eq!(encode("a b&c/{{token}}é"), "a%20b%26c%2F{{token}}%C3%A9");
        let member = serde_json::json!({"traits": {"smithy.api#xmlName": "dryRun"}});
        assert_eq!(query_name("DryRun", &member, false), "dryRun");
        assert_eq!(query_name("DryRun", &member, true), "DryRun");
        assert!(is_host("bedrock-runtime.us-east-1.amazonaws.com"));
        for bad in ["", "a b", "x/y", "{{v}}", ".x", "a..b", "UPPER"] {
            assert!(!is_host(bad), "{bad}");
        }
        assert!(is_aws_region("eu-west-1") && is_aws_region("aws-global"));
        assert!(!is_aws_region("cn-north-1") && !is_aws_region("us-gov-west-1") && !is_aws_region("us-isob-east-1"));
    }

    /// Imports every model in the folder `ZORVIK_SMITHY_MODELS` (for example AWS's, from
    /// https://github.com/aws/api-models-aws) and prints a summary; the requests named in
    /// `ZORVIK_SMITHY_SHOW` (comma-separated) are printed whole.
    #[test]
    #[ignore = "needs downloaded models"]
    fn real_models() {
        let dir = std::env::var("ZORVIK_SMITHY_MODELS").expect("ZORVIK_SMITHY_MODELS");
        let show = std::env::var("ZORVIK_SMITHY_SHOW").unwrap_or_default();
        let show: Vec<&str> = show.split(',').filter(|s| !s.is_empty()).collect();
        let mut paths: Vec<_> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).collect();
        paths.sort();
        for path in paths.iter().filter(|p| p.extension().is_some_and(|e| e == "json")) {
            let text = std::fs::read_to_string(path).unwrap();
            let start = std::time::Instant::now();
            let c = import_smithy(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            let took = start.elapsed();
            let doc: Value = serde_json::from_str(&text).unwrap();
            let protocol = Importer::new(&doc).unwrap().pick_protocol();
            let signing = match &c.auth {
                Auth::AwsSigV4(a) => format!("{} in {}", a.service, a.region),
                other => format!("{other:?}"),
            };
            println!(
                "## {} | {} | {:?} | {signing} | {} requests | {} folders | {:?} | {} ms",
                path.file_name().unwrap().to_string_lossy(),
                c.name,
                protocol,
                c.request_count(),
                folders(&c.items),
                c.warnings,
                took.as_millis()
            );
            let vars: Vec<String> = c.variables.iter().map(|v| format!("{}={}", v.key, v.value)).collect();
            println!("## variables: {}", vars.join(", "));
            for r in requests(&c.items).into_iter().filter(|r| show.contains(&r.name.as_str())) {
                println!("{}", serde_json::to_string_pretty(r).unwrap());
            }
        }
    }
}

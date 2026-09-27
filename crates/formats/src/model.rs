//! Data model for requests, folders, workspaces and environments.
//! These types are stored as YAML files (see docs/architecture.md, "Workspace format") and
//! shared with the UI through generated TypeScript bindings.

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use zorvik_engine::HttpVersionPref;
pub use zorvik_engine::framing::{Framing, LineEnding, PayloadEncoding};

fn yes() -> bool {
    true
}
fn is_true(v: &bool) -> bool {
    *v
}
fn is_false(v: &bool) -> bool {
    !*v
}

/// One row in a key/value table (headers, query params, form fields, variables).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct KeyValue {
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub value: String,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    #[ts(optional, as = "Option<bool>")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub description: String,
}

impl KeyValue {
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self { key: key.into(), value: value.into(), enabled: true, description: String::new() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum RequestKind {
    #[default]
    Http,
    Websocket,
    Sse,
    /// Raw TCP (`tcp://host:port`, or `tls://host:port` for TLS).
    Tcp,
    /// UDP datagrams (`udp://host:port`).
    Udp,
    /// DNS query: `url` is the name, `method` the record type (A, AAAA, MX, …).
    Dns,
    /// MQTT client (`mqtt://host:1883`, `mqtts://host:8883`).
    Mqtt,
    /// gRPC call (`grpc://host:port`, `grpcs://` for TLS): `method` is
    /// `package.Service/Method`, `body.text` the JSON message.
    Grpc,
}

fn two() -> u8 {
    2
}
fn is_two(v: &u8) -> bool {
    *v == 2
}

/// Options of TCP and UDP requests (and of TCP/UDP servers).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SocketOptions {
    #[serde(default, skip_serializing_if = "is_default")]
    #[ts(optional, as = "Option<Framing>")]
    pub framing: Framing,
    /// Length prefix size for `lengthPrefixed` framing: 1, 2 or 4 bytes.
    #[serde(default = "two", skip_serializing_if = "is_two")]
    #[ts(optional, as = "Option<u8>")]
    pub length_bytes: u8,
    #[serde(default, skip_serializing_if = "is_default")]
    #[ts(optional, as = "Option<LineEnding>")]
    pub line_ending: LineEnding,
    /// UDP: allow sending to broadcast addresses.
    #[serde(default, skip_serializing_if = "is_false")]
    #[ts(optional, as = "Option<bool>")]
    pub broadcast: bool,
}

impl Default for SocketOptions {
    fn default() -> Self {
        Self { framing: Framing::Raw, length_bytes: 2, line_ending: LineEnding::None, broadcast: false }
    }
}

/// Options of DNS requests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DnsOptions {
    /// Resolver to ask. Empty: the system's. Otherwise `1.1.1.1`, `8.8.8.8:53`,
    /// `tcp://1.1.1.1`, `tls://1.1.1.1` (DNS over TLS) or `https://…/dns-query` (DNS over HTTPS).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub server: String,
    /// Ask the server to resolve recursively (RD flag).
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    #[ts(optional, as = "Option<bool>")]
    pub recursion: bool,
}

impl Default for DnsOptions {
    fn default() -> Self {
        Self { server: String::new(), recursion: true }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum MqttVersion {
    #[default]
    V311,
    V5,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MqttSubscription {
    pub topic: String,
    #[serde(default)]
    pub qos: u8,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    #[ts(optional, as = "Option<bool>")]
    pub enabled: bool,
}

fn thirty() -> u16 {
    30
}

/// Options of MQTT requests. The message to publish is `body.text`; the
/// username and password come from Basic auth.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MqttOptions {
    /// Empty: a random id per connection.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub client_id: String,
    #[serde(default, skip_serializing_if = "is_default")]
    #[ts(optional, as = "Option<MqttVersion>")]
    pub version: MqttVersion,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    #[ts(optional, as = "Option<bool>")]
    pub clean_session: bool,
    #[serde(default = "thirty")]
    pub keep_alive_secs: u16,
    /// Topics subscribed right after connecting.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, as = "Option<Vec<MqttSubscription>>")]
    pub subscriptions: Vec<MqttSubscription>,
    /// Topic the composer publishes to.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub topic: String,
    #[serde(default)]
    pub qos: u8,
    #[serde(default, skip_serializing_if = "is_false")]
    #[ts(optional, as = "Option<bool>")]
    pub retain: bool,
}

impl Default for MqttOptions {
    fn default() -> Self {
        Self {
            client_id: String::new(),
            version: MqttVersion::V311,
            clean_session: true,
            keep_alive_secs: 30,
            subscriptions: Vec::new(),
            topic: String::new(),
            qos: 0,
            retain: false,
        }
    }
}

pub(crate) fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum BodyType {
    #[default]
    None,
    Json,
    Text,
    Xml,
    FormUrlencoded,
    Multipart,
    Binary,
    /// `body.graphql`, sent as JSON `{"query", "variables", "operationName"}`.
    Graphql,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MultipartField {
    #[serde(default)]
    pub key: String,
    /// Text value, or a file path when `file` is true.
    #[serde(default)]
    pub value: String,
    #[serde(default, skip_serializing_if = "is_false")]
    #[ts(optional, as = "Option<bool>")]
    pub file: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub content_type: Option<String>,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    #[ts(optional, as = "Option<bool>")]
    pub enabled: bool,
}

/// GraphQL operation (`body.type: graphql`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GraphqlBody {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub query: String,
    /// Variables as JSON text (may contain `{{variables}}`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub variables: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub operation_name: Option<String>,
}

/// Request body. Data for every body type is kept, so switching the type in
/// the UI never loses what was typed; only `type` decides what is sent.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Body {
    #[serde(rename = "type", default)]
    pub body_type: BodyType,
    /// JSON / text / XML body (also the message draft for WebSocket requests).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub text: String,
    /// Content-Type for `text` bodies (default text/plain).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub content_type: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, as = "Option<Vec<KeyValue>>")]
    pub form: Vec<KeyValue>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, as = "Option<Vec<MultipartField>>")]
    pub multipart: Vec<MultipartField>,
    /// File path for binary bodies (absolute, or relative to the workspace root).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub file: String,
    #[serde(default, skip_serializing_if = "is_default")]
    #[ts(optional, as = "Option<GraphqlBody>")]
    pub graphql: GraphqlBody,
}

/// Where a gRPC request gets its service definitions.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GrpcOptions {
    /// `.proto` files (relative to the workspace root, or absolute). Empty: server reflection.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, as = "Option<Vec<String>>")]
    pub proto_files: Vec<String>,
    /// Folders searched for `import`s (the proto files' folders are always searched).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, as = "Option<Vec<String>>")]
    pub import_paths: Vec<String>,
}

/// JavaScript run before sending and after the response (Postman-compatible `pm` API).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Scripts {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub pre_request: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub post_response: String,
}

impl Scripts {
    pub fn is_empty(&self) -> bool {
        self.pre_request.trim().is_empty() && self.post_response.trim().is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ApiKeyLocation {
    #[default]
    Header,
    Query,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum GrantType {
    #[default]
    ClientCredentials,
    Password,
    AuthorizationCode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ClientAuthMethod {
    /// client_id/secret in an HTTP Basic Authorization header.
    #[default]
    BasicHeader,
    /// client_id/secret as form fields in the token request body.
    Body,
}

pub const DEFAULT_REDIRECT_URI: &str = "http://127.0.0.1:53682/callback";

fn default_redirect_uri() -> String {
    DEFAULT_REDIRECT_URI.to_string()
}
fn default_bearer() -> String {
    "Bearer".to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct OAuth2Config {
    #[serde(default)]
    pub grant_type: GrantType,
    #[serde(default)]
    pub token_url: String,
    /// Authorization endpoint (authorization code grant only).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub auth_url: String,
    /// Loopback redirect URI registered with the provider (authorization code grant).
    #[serde(default = "default_redirect_uri")]
    pub redirect_uri: String,
    #[serde(default)]
    pub client_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub client_secret: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub scope: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub audience: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub username: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub password: String,
    #[serde(default)]
    pub client_auth: ClientAuthMethod,
    /// Use PKCE (S256) for the authorization code grant.
    #[serde(default = "yes")]
    pub pkce: bool,
    /// Authorization header prefix; empty sends the bare token.
    #[serde(default = "default_bearer")]
    pub header_prefix: String,
}

impl Default for OAuth2Config {
    fn default() -> Self {
        Self {
            grant_type: GrantType::ClientCredentials,
            token_url: String::new(),
            auth_url: String::new(),
            redirect_uri: default_redirect_uri(),
            client_id: String::new(),
            client_secret: String::new(),
            scope: String::new(),
            audience: String::new(),
            username: String::new(),
            password: String::new(),
            client_auth: ClientAuthMethod::BasicHeader,
            pkce: true,
            header_prefix: default_bearer(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(export)]
pub enum Auth {
    /// Use the parent folder's (or workspace's) auth.
    #[default]
    Inherit,
    None,
    #[serde(rename_all = "camelCase")]
    Basic {
        #[serde(default)]
        username: String,
        #[serde(default)]
        password: String,
    },
    #[serde(rename_all = "camelCase")]
    Bearer {
        #[serde(default)]
        token: String,
        #[serde(default = "default_bearer")]
        prefix: String,
    },
    #[serde(rename_all = "camelCase")]
    ApiKey {
        #[serde(default)]
        key: String,
        #[serde(default)]
        value: String,
        #[serde(default)]
        location: ApiKeyLocation,
    },
    #[serde(rename = "oauth2")]
    OAuth2(OAuth2Config),
}

impl Auth {
    fn is_inherit(&self) -> bool {
        matches!(self, Auth::Inherit)
    }
}

/// Per-request overrides of the global request settings.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RequestSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub timeout_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub follow_redirects: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub max_redirects: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub verify_tls: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub http_version: Option<HttpVersionPref>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub decompress: Option<bool>,
}

impl RequestSettings {
    fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

fn default_method() -> String {
    "GET".to_string()
}

/// A saved request (a `.yaml` file under `requests/`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Request {
    pub name: String,
    #[serde(default, skip_serializing_if = "is_http")]
    #[ts(optional, as = "Option<RequestKind>")]
    pub kind: RequestKind,
    /// Sort position among siblings.
    #[serde(default)]
    pub seq: u32,
    #[serde(default = "default_method")]
    pub method: String,
    /// Raw URL as typed, including the enabled query string.
    #[serde(default)]
    pub url: String,
    /// Query params switched off in the UI (not part of `url`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, as = "Option<Vec<KeyValue>>")]
    pub disabled_params: Vec<KeyValue>,
    /// Values for `:name` path segments.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, as = "Option<Vec<KeyValue>>")]
    pub path_params: Vec<KeyValue>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, as = "Option<Vec<KeyValue>>")]
    pub headers: Vec<KeyValue>,
    #[serde(default, skip_serializing_if = "is_no_body")]
    #[ts(optional, as = "Option<Body>")]
    pub body: Body,
    #[serde(default, skip_serializing_if = "Auth::is_inherit")]
    #[ts(optional, as = "Option<Auth>")]
    pub auth: Auth,
    #[serde(default, skip_serializing_if = "RequestSettings::is_empty")]
    #[ts(optional, as = "Option<RequestSettings>")]
    pub settings: RequestSettings,
    /// TCP/UDP options.
    #[serde(default, skip_serializing_if = "is_default")]
    #[ts(optional, as = "Option<SocketOptions>")]
    pub socket: SocketOptions,
    #[serde(default, skip_serializing_if = "is_default")]
    #[ts(optional, as = "Option<DnsOptions>")]
    pub dns: DnsOptions,
    #[serde(default, skip_serializing_if = "is_default")]
    #[ts(optional, as = "Option<MqttOptions>")]
    pub mqtt: MqttOptions,
    #[serde(default, skip_serializing_if = "is_default")]
    #[ts(optional, as = "Option<GrpcOptions>")]
    pub grpc: GrpcOptions,
    #[serde(default, skip_serializing_if = "is_default")]
    #[ts(optional, as = "Option<Scripts>")]
    pub scripts: Scripts,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub docs: String,
}

fn is_http(k: &RequestKind) -> bool {
    *k == RequestKind::Http
}
fn is_no_body(b: &Body) -> bool {
    *b == Body::default()
}

impl Request {
    pub fn new(name: impl Into<String>, kind: RequestKind) -> Self {
        Self {
            name: name.into(),
            kind,
            seq: 0,
            method: default_method(),
            url: String::new(),
            disabled_params: Vec::new(),
            path_params: Vec::new(),
            headers: Vec::new(),
            body: Body::default(),
            auth: Auth::Inherit,
            settings: RequestSettings::default(),
            socket: SocketOptions::default(),
            dns: DnsOptions::default(),
            mqtt: MqttOptions::default(),
            grpc: GrpcOptions::default(),
            scripts: Scripts::default(),
            docs: String::new(),
        }
    }
}

/// Folder metadata (`_folder.yaml`). Auth and headers apply to everything inside.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FolderMeta {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub seq: u32,
    #[serde(default, skip_serializing_if = "Auth::is_inherit")]
    #[ts(optional, as = "Option<Auth>")]
    pub auth: Auth,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, as = "Option<Vec<KeyValue>>")]
    pub headers: Vec<KeyValue>,
    #[serde(default, skip_serializing_if = "is_default")]
    #[ts(optional, as = "Option<Scripts>")]
    pub scripts: Scripts,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub docs: String,
}

/// A variable in the workspace or an environment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Variable {
    pub key: String,
    /// Empty in the file for secrets; the real value lives in the local secret store.
    #[serde(default)]
    pub value: String,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    #[ts(optional, as = "Option<bool>")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    #[ts(optional, as = "Option<bool>")]
    pub secret: bool,
}

pub const FORMAT_VERSION: u32 = 1;

/// Workspace root metadata (`zorvik.yaml`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct WorkspaceMeta {
    pub version: u32,
    /// Stable id used to key local-only data (secrets, active environment).
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, as = "Option<Vec<Variable>>")]
    pub variables: Vec<Variable>,
    /// Default auth for requests set to "inherit".
    #[serde(default = "no_auth", skip_serializing_if = "is_no_auth")]
    #[ts(optional, as = "Option<Auth>")]
    pub auth: Auth,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, as = "Option<Vec<KeyValue>>")]
    pub headers: Vec<KeyValue>,
    #[serde(default, skip_serializing_if = "is_default")]
    #[ts(optional, as = "Option<Scripts>")]
    pub scripts: Scripts,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub docs: String,
}

fn no_auth() -> Auth {
    Auth::None
}
fn is_no_auth(a: &Auth) -> bool {
    matches!(a, Auth::None | Auth::Inherit)
}

/// An environment (`environments/<name>.yaml`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Environment {
    pub name: String,
    #[serde(default)]
    pub variables: Vec<Variable>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum NodeKind {
    Folder,
    Request,
}

/// Collection tree node for the sidebar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TreeNode {
    pub kind: NodeKind,
    /// Path relative to the workspace `requests/` directory, `/`-separated.
    pub path: String,
    pub name: String,
    pub seq: u32,
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_kind: Option<RequestKind>,
    /// An HTTP request with a GraphQL body (the sidebar shows `GQL` instead of the method).
    #[ts(optional, as = "Option<bool>")]
    #[serde(skip_serializing_if = "is_false")]
    pub graphql: bool,
    /// Set when the file could not be parsed.
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub children: Vec<TreeNode>,
}

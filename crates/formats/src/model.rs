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
    /// Socket.IO client: `url` is the server and namespace (`http://localhost:3000/chat`).
    #[serde(rename = "socketio")]
    SocketIo,
    /// MCP client: `url` is the server's endpoint (`https://example.com/mcp`), or the command
    /// that starts a stdio server (`npx -y @modelcontextprotocol/server-everything`).
    Mcp,
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

/// How a Socket.IO client reaches the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum SocketIoTransport {
    /// WebSocket, or HTTP long-polling when the server doesn't take WebSocket.
    #[default]
    Auto,
    Websocket,
    /// HTTP long-polling only.
    Polling,
}

fn socketio_path() -> String {
    "/socket.io/".to_string()
}
fn is_socketio_path(v: &String) -> bool {
    v == "/socket.io/"
}

/// Options of Socket.IO requests (Socket.IO 3 and 4, Engine.IO 4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SocketIoOptions {
    /// Where the server answers (socket.io's `path` option).
    #[serde(default = "socketio_path", skip_serializing_if = "is_socketio_path")]
    #[ts(optional, as = "Option<String>")]
    pub path: String,
    /// The connection's `auth` payload as JSON text (may contain `{{variables}}`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub auth: String,
    #[serde(default, skip_serializing_if = "is_default")]
    #[ts(optional, as = "Option<SocketIoTransport>")]
    pub transport: SocketIoTransport,
    /// The event the composer emits.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub event: String,
    /// The composer asks the server to acknowledge.
    #[serde(default, skip_serializing_if = "is_false")]
    #[ts(optional, as = "Option<bool>")]
    pub ack: bool,
}

impl Default for SocketIoOptions {
    fn default() -> Self {
        Self {
            path: socketio_path(),
            auth: String::new(),
            transport: SocketIoTransport::Auto,
            event: String::new(),
            ack: false,
        }
    }
}

/// How an MCP client reaches its server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum McpTransport {
    /// `http(s)://…`: Streamable HTTP, falling back to HTTP+SSE; anything else: a program (stdio).
    #[default]
    Auto,
    StreamableHttp,
    /// The HTTP+SSE transport of protocol version 2024-11-05.
    Sse,
    Stdio,
}

/// What an MCP request calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum McpCallKind {
    #[default]
    Tool,
    /// Read a resource (`name` is its URI or URI template).
    Resource,
    /// Get a prompt.
    Prompt,
}

/// Options of MCP requests: how to connect, and the call that Send (and collection runs) make.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct McpOptions {
    #[serde(default, skip_serializing_if = "is_default")]
    #[ts(optional, as = "Option<McpTransport>")]
    pub transport: McpTransport,
    /// stdio: environment variables for the program (values may use `{{variables}}`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, as = "Option<Vec<KeyValue>>")]
    pub env: Vec<KeyValue>,
    /// stdio: the program's working directory, relative to the workspace folder (empty: the folder).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub cwd: String,
    #[serde(default, skip_serializing_if = "is_default")]
    #[ts(optional, as = "Option<McpCallKind>")]
    pub call: McpCallKind,
    /// The tool or prompt to call, or the resource URI (a template's `{name}` parts come from
    /// the arguments).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub name: String,
    /// The arguments as JSON text (may contain `{{variables}}`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub arguments: String,
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
    /// How a subscription is sent (queries and mutations always go over HTTP).
    #[serde(default, skip_serializing_if = "is_default")]
    #[ts(optional, as = "Option<GraphqlTransport>")]
    pub transport: GraphqlTransport,
    /// Where a subscription connects. Empty: the request URL (as `ws://`/`wss://` for WebSocket).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub subscription_url: String,
    /// WebSocket: the `connection_init` payload as JSON text (may contain `{{variables}}`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub connection_params: String,
}

impl GraphqlBody {
    /// Whether the operation that runs is a subscription.
    pub fn is_subscription(&self) -> bool {
        crate::graphql::operation_type(&self.query, self.operation_name.as_deref())
            == Some(crate::graphql::OperationType::Subscription)
    }
}

/// How a GraphQL subscription reaches the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum GraphqlTransport {
    /// WebSocket with the `graphql-transport-ws` protocol (the graphql-ws library, Apollo Server 4+).
    #[default]
    Websocket,
    /// WebSocket with the older `subscriptions-transport-ws` protocol (subprotocol `graphql-ws`).
    WebsocketLegacy,
    /// Server-Sent Events (the graphql-sse protocol, GraphQL Yoga).
    Sse,
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
    /// The token comes back in the redirect's `#fragment` (older single-page apps).
    Implicit,
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

/// A signing method of OAuth 1.0a.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum OAuth1Method {
    #[default]
    #[serde(rename = "HMAC-SHA1")]
    HmacSha1,
    #[serde(rename = "HMAC-SHA256")]
    HmacSha256,
    #[serde(rename = "HMAC-SHA512")]
    HmacSha512,
    #[serde(rename = "RSA-SHA1")]
    RsaSha1,
    #[serde(rename = "RSA-SHA256")]
    RsaSha256,
    #[serde(rename = "RSA-SHA512")]
    RsaSha512,
    #[serde(rename = "PLAINTEXT")]
    Plaintext,
}

/// A JWT signing algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "UPPERCASE")]
#[ts(export)]
pub enum JwtAlgorithm {
    #[default]
    HS256,
    HS384,
    HS512,
    RS256,
    RS384,
    RS512,
    PS256,
    PS384,
    PS512,
    ES256,
    ES384,
}

impl JwtAlgorithm {
    /// Signs with a shared secret (HS*) rather than a private key.
    pub fn uses_secret(self) -> bool {
        matches!(self, Self::HS256 | Self::HS384 | Self::HS512)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum HawkAlgorithm {
    #[default]
    Sha256,
    Sha1,
}

/// AWS Signature Version 4.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AwsSigV4Config {
    #[serde(default)]
    pub access_key: String,
    #[serde(default)]
    pub secret_key: String,
    /// Temporary credentials (STS): sent as `X-Amz-Security-Token`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub session_token: String,
    /// e.g. `us-east-1`.
    #[serde(default)]
    pub region: String,
    /// e.g. `execute-api`, `s3`, `sts`.
    #[serde(default)]
    pub service: String,
    /// Sign in headers, or in the query string (a presigned URL).
    #[serde(default, skip_serializing_if = "is_default")]
    #[ts(optional, as = "Option<ApiKeyLocation>")]
    pub location: ApiKeyLocation,
}

/// OAuth 1.0a request signing (RFC 5849).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct OAuth1Config {
    #[serde(default)]
    pub consumer_key: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub consumer_secret: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub token: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub token_secret: String,
    #[serde(default, skip_serializing_if = "is_default")]
    #[ts(optional, as = "Option<OAuth1Method>")]
    pub signature_method: OAuth1Method,
    /// PEM private key, for the RSA methods.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub private_key: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub callback: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub verifier: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub realm: String,
    /// Send `oauth_version=1.0` (on unless switched off).
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    #[ts(optional, as = "Option<bool>")]
    pub include_version: bool,
    /// Add `oauth_body_hash` for bodies that aren't form data.
    #[serde(default, skip_serializing_if = "is_false")]
    #[ts(optional, as = "Option<bool>")]
    pub include_body_hash: bool,
    /// Parameters in the Authorization header, or in the query string.
    #[serde(default, skip_serializing_if = "is_default")]
    #[ts(optional, as = "Option<ApiKeyLocation>")]
    pub location: ApiKeyLocation,
}

fn default_jwt_payload() -> String {
    "{}".to_string()
}
fn default_token_param() -> String {
    "token".to_string()
}

/// A JWT that Zorvik signs for each request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct JwtConfig {
    #[serde(default)]
    pub algorithm: JwtAlgorithm,
    /// The shared secret (HS*) or a PEM private key (RS*, PS*, ES*).
    #[serde(default)]
    pub secret: String,
    /// The HS* secret is base64.
    #[serde(default, skip_serializing_if = "is_false")]
    #[ts(optional, as = "Option<bool>")]
    pub secret_base64: bool,
    /// The claims, as JSON (variables allowed).
    #[serde(default = "default_jwt_payload")]
    pub payload: String,
    /// Extra header fields, as JSON (`alg` and `typ` are set).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub header: String,
    /// Authorization header prefix; empty sends the bare token.
    #[serde(default = "default_bearer")]
    pub prefix: String,
    /// Send it in the Authorization header, or as a query parameter.
    #[serde(default, skip_serializing_if = "is_default")]
    #[ts(optional, as = "Option<ApiKeyLocation>")]
    pub location: ApiKeyLocation,
    /// The query parameter's name.
    #[serde(default = "default_token_param")]
    pub query_param: String,
}

impl Default for JwtConfig {
    fn default() -> Self {
        Self {
            algorithm: JwtAlgorithm::HS256,
            secret: String::new(),
            secret_base64: false,
            payload: default_jwt_payload(),
            header: String::new(),
            prefix: default_bearer(),
            location: ApiKeyLocation::Header,
            query_param: default_token_param(),
        }
    }
}

/// Hawk (https://github.com/mozilla/hawk).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HawkConfig {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub key: String,
    #[serde(default, skip_serializing_if = "is_default")]
    #[ts(optional, as = "Option<HawkAlgorithm>")]
    pub algorithm: HawkAlgorithm,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub ext: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub app: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub dlg: String,
    /// Sign the body too (the server must check it).
    #[serde(default, skip_serializing_if = "is_false")]
    #[ts(optional, as = "Option<bool>")]
    pub include_payload_hash: bool,
}

fn default_edgegrid_max_body() -> u64 {
    131_072
}

/// Akamai EdgeGrid (EG1-HMAC-SHA256).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct EdgeGridConfig {
    #[serde(default)]
    pub client_token: String,
    #[serde(default)]
    pub client_secret: String,
    #[serde(default)]
    pub access_token: String,
    /// Header names to sign, comma-separated (usually none).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub headers_to_sign: String,
    /// How much of a POST body is hashed, in bytes.
    #[serde(default = "default_edgegrid_max_body")]
    #[ts(type = "number")]
    pub max_body: u64,
}

impl Default for EdgeGridConfig {
    fn default() -> Self {
        Self {
            client_token: String::new(),
            client_secret: String::new(),
            access_token: String::new(),
            headers_to_sign: String::new(),
            max_body: default_edgegrid_max_body(),
        }
    }
}

fn default_asap_algorithm() -> JwtAlgorithm {
    JwtAlgorithm::RS256
}
fn default_asap_expiry() -> u32 {
    3600
}

/// Atlassian ASAP (service-to-service JWT).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AsapConfig {
    #[serde(default)]
    pub issuer: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub subject: String,
    /// One or more audiences, comma-separated.
    #[serde(default)]
    pub audience: String,
    /// The key id (`kid`) the receiver looks the public key up by.
    #[serde(default)]
    pub key_id: String,
    /// PEM private key.
    #[serde(default)]
    pub private_key: String,
    #[serde(default = "default_asap_algorithm")]
    pub algorithm: JwtAlgorithm,
    /// Lifetime of each token, in seconds (at most 3600).
    #[serde(default = "default_asap_expiry")]
    pub expires_in: u32,
    /// Extra claims, as JSON.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub claims: String,
}

impl Default for AsapConfig {
    fn default() -> Self {
        Self {
            issuer: String::new(),
            subject: String::new(),
            audience: String::new(),
            key_id: String::new(),
            private_key: String::new(),
            algorithm: default_asap_algorithm(),
            expires_in: default_asap_expiry(),
            claims: String::new(),
        }
    }
}

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
    /// HTTP Digest: answers the server's challenge (MD5, SHA-256, qop auth / auth-int).
    #[serde(rename_all = "camelCase")]
    Digest {
        #[serde(default)]
        username: String,
        #[serde(default)]
        password: String,
    },
    /// NTLMv2 (Windows and IIS servers), over one HTTP/1.1 connection.
    #[serde(rename_all = "camelCase")]
    Ntlm {
        /// `user`, `DOMAIN\user` or `user@domain`.
        #[serde(default)]
        username: String,
        #[serde(default)]
        password: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        #[ts(optional, as = "Option<String>")]
        domain: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        #[ts(optional, as = "Option<String>")]
        workstation: String,
    },
    AwsSigV4(AwsSigV4Config),
    #[serde(rename = "oauth1")]
    OAuth1(OAuth1Config),
    Jwt(JwtConfig),
    Hawk(HawkConfig),
    #[serde(rename = "akamaiEdgeGrid")]
    EdgeGrid(EdgeGridConfig),
    Asap(AsapConfig),
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
    /// Collection runs: send again until a condition holds (polling).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub repeat: Option<RepeatUntil>,
    /// Server-Sent Events in collection runs: when to stop reading.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub stream: Option<StreamUntil>,
}

/// "Send again until …": for polling an endpoint until work is done.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RepeatUntil {
    /// JavaScript checked after each send, after the post-response scripts, e.g.
    /// `pm.response.json().status === "done"`. Empty: until the request's tests pass
    /// (or, without tests, until the status is below 400).
    #[serde(default)]
    pub condition: String,
    /// Pause between sends.
    #[serde(default = "second")]
    #[ts(type = "number")]
    pub interval_ms: u64,
    /// Give up after this long; the request then fails.
    #[serde(default = "half_minute")]
    #[ts(type = "number")]
    pub timeout_ms: u64,
}

impl Default for RepeatUntil {
    fn default() -> Self {
        Self { condition: String::new(), interval_ms: second(), timeout_ms: half_minute() }
    }
}

fn second() -> u64 {
    1_000
}
fn half_minute() -> u64 {
    30_000
}

/// When reading a Server-Sent Events stream stops: a named event, a number of events,
/// the server closing it, or a time limit, whichever comes first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct StreamUntil {
    /// Stop after the first event with this name (`message` for events without a name).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub event: String,
    /// Stop after this many events (0: no limit but the time).
    #[serde(default = "hundred_events")]
    pub max_events: u32,
    /// Stop after this long.
    #[serde(default = "ten_seconds")]
    #[ts(type = "number")]
    pub timeout_ms: u64,
}

impl Default for StreamUntil {
    fn default() -> Self {
        Self { event: String::new(), max_events: hundred_events(), timeout_ms: ten_seconds() }
    }
}

fn hundred_events() -> u32 {
    100
}
fn ten_seconds() -> u64 {
    10_000
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
    /// Descriptions of enabled query params, by `key` (their values live in `url`,
    /// which has no room for them; disabled ones carry their own).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, as = "Option<Vec<KeyValue>>")]
    pub param_descriptions: Vec<KeyValue>,
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
    #[ts(optional, as = "Option<SocketIoOptions>")]
    pub socketio: SocketIoOptions,
    #[serde(default, skip_serializing_if = "is_default")]
    #[ts(optional, as = "Option<McpOptions>")]
    pub mcp: McpOptions,
    #[serde(default, skip_serializing_if = "is_default")]
    #[ts(optional, as = "Option<Scripts>")]
    pub scripts: Scripts,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub docs: String,
    /// The OpenAPI operation this request was imported from (see [`OpenApiSource`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub openapi: Option<OpenApiOperation>,
    /// Saved responses ("Save as example"): documentation, and what mocks answer.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, as = "Option<Vec<Example>>")]
    pub examples: Vec<Example>,
}

/// A saved response of a request.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Example {
    pub name: String,
    #[serde(default = "status_ok")]
    pub status: u16,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, as = "Option<Vec<KeyValue>>")]
    pub headers: Vec<KeyValue>,
    /// The response body as text.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub body: String,
    /// The URL the request was sent to, as typed (variables kept), when it differs from the
    /// request's own; its query parameters tell mocks which example to answer with.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub url: String,
}

/// The largest example body kept, in bytes.
pub const MAX_EXAMPLE_BODY: usize = 1024 * 1024;

fn status_ok() -> u16 {
    200
}

/// Where an imported request came from in its OpenAPI document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct OpenApiOperation {
    /// `METHOD /path/{template}` as the document has it, e.g. `GET /pets/{petId}`.
    pub operation: String,
    /// The operation is no longer in the document (found when updating from it).
    #[serde(default, skip_serializing_if = "is_false")]
    #[ts(optional, as = "Option<bool>")]
    pub removed: bool,
}

/// The OpenAPI document a folder was imported from, kept in the workspace so its requests'
/// responses can be checked against the documented schemas, and the folder updated later.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct OpenApiSource {
    /// The document, relative to the workspace folder (under `specs/`).
    pub spec: String,
    /// Where it was imported from: a URL or a file path (for "Update from OpenAPI").
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub source: String,
    /// Check responses against the documented schemas (on unless switched off).
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    #[ts(optional, as = "Option<bool>")]
    pub validate: bool,
}

fn is_http(k: &RequestKind) -> bool {
    *k == RequestKind::Http
}
fn is_no_body(b: &Body) -> bool {
    *b == Body::default()
}

impl Request {
    /// An HTTP request whose GraphQL operation is a subscription (sent over WebSocket or SSE).
    pub fn is_graphql_subscription(&self) -> bool {
        self.kind == RequestKind::Http
            && self.body.body_type == BodyType::Graphql
            && self.body.graphql.is_subscription()
    }

    pub fn new(name: impl Into<String>, kind: RequestKind) -> Self {
        Self {
            name: name.into(),
            kind,
            seq: 0,
            method: default_method(),
            url: String::new(),
            disabled_params: Vec::new(),
            path_params: Vec::new(),
            param_descriptions: Vec::new(),
            headers: Vec::new(),
            body: Body::default(),
            auth: Auth::Inherit,
            settings: RequestSettings::default(),
            socket: SocketOptions::default(),
            dns: DnsOptions::default(),
            mqtt: MqttOptions::default(),
            grpc: GrpcOptions::default(),
            socketio: SocketIoOptions::default(),
            mcp: McpOptions::default(),
            scripts: Scripts::default(),
            docs: String::new(),
            openapi: None,
            examples: Vec::new(),
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
    /// The OpenAPI document this folder was imported from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub openapi: Option<OpenApiSource>,
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
    /// An imported request whose operation is no longer in its OpenAPI document.
    #[ts(optional, as = "Option<bool>")]
    #[serde(skip_serializing_if = "is_false")]
    pub removed_from_spec: bool,
    /// A folder imported from an OpenAPI document the workspace keeps (it can be updated from it).
    #[ts(optional, as = "Option<bool>")]
    #[serde(skip_serializing_if = "is_false")]
    pub from_spec: bool,
    pub children: Vec<TreeNode>,
}

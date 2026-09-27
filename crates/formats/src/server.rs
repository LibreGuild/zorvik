//! Servers (`servers/<name>.yaml`): mock HTTP APIs, WebSocket and SSE servers,
//! TCP/UDP listeners, a DNS server and a TCP relay. Like requests they are
//! plain files, shared through Git.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::model::{Framing, KeyValue, LineEnding, PayloadEncoding, is_default};

fn yes() -> bool {
    true
}
fn is_true(v: &bool) -> bool {
    *v
}
fn is_false(v: &bool) -> bool {
    !*v
}
fn loopback() -> String {
    "127.0.0.1".to_string()
}
fn any_method() -> String {
    "*".to_string()
}
fn status_ok() -> u16 {
    200
}
fn hundred() -> u8 {
    100
}
fn is_hundred(v: &u8) -> bool {
    *v == 100
}
fn two() -> u8 {
    2
}
fn is_two(v: &u8) -> bool {
    *v == 2
}
fn sixty() -> u32 {
    60
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ServerKind {
    /// Mock HTTP API (routes with canned responses).
    #[default]
    Http,
    Websocket,
    Sse,
    Tcp,
    Udp,
    Dns,
    /// Relay to another TCP server, showing the traffic in both directions.
    TcpProxy,
}

impl ServerKind {
    /// Whether the server can listen with TLS.
    pub fn supports_tls(self) -> bool {
        matches!(self, ServerKind::Http | ServerKind::Websocket | ServerKind::Sse | ServerKind::Tcp)
    }
}

/// TLS for a listener. Without certificate paths a self-signed certificate for
/// `localhost` is generated (clients must trust it or skip verification).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ServerTls {
    #[serde(default)]
    pub enabled: bool,
    /// PEM certificate chain.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub cert_path: String,
    /// PEM private key.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub key_path: String,
}

/// What a WebSocket/TCP/UDP server does with incoming messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ReplyMode {
    /// Send every message back.
    #[default]
    Echo,
    /// Reply with the first matching rule (no reply when none matches).
    Rules,
    /// Only what you send from the UI.
    Manual,
    /// Read and drop everything.
    Discard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum MatchKind {
    /// Every message.
    Any,
    #[default]
    Contains,
    Exact,
    Regex,
}

/// "When a message matches `pattern`, reply with `reply`." Pattern and reply
/// are text or hex (see the server's `encoding`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ReplyRule {
    #[serde(default, rename = "match")]
    pub matcher: MatchKind,
    #[serde(default)]
    pub pattern: String,
    #[serde(default)]
    pub reply: String,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    #[ts(optional, as = "Option<u32>")]
    pub delay_ms: u64,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    #[ts(optional, as = "Option<bool>")]
    pub enabled: bool,
}

impl Default for ReplyRule {
    fn default() -> Self {
        Self { matcher: MatchKind::Contains, pattern: String::new(), reply: String::new(), delay_ms: 0, enabled: true }
    }
}

fn is_zero_u64(v: &u64) -> bool {
    *v == 0
}

// ---- HTTP mock --------------------------------------------------------------

/// Replaces a route's normal response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum MockFault {
    #[default]
    None,
    /// 500 Internal Server Error.
    Error,
    /// Close the connection without answering.
    Reset,
    /// Never answer (the client times out).
    Hang,
}

/// One mock route. `path` supports `:name` segments and a trailing `*`.
/// Status, headers and body may use `{{request.params.id}}`,
/// `{{request.query.x}}`, `{{request.headers.x}}`, `{{request.body}}`,
/// `{{request.method}}`, `{{request.path}}`, dynamic variables (`{{$uuid}}`)
/// and the active environment's variables.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MockRoute {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub name: String,
    /// HTTP method, or `*` for any.
    #[serde(default = "any_method")]
    pub method: String,
    #[serde(default)]
    pub path: String,
    #[serde(default = "status_ok")]
    pub status: u16,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, as = "Option<Vec<KeyValue>>")]
    pub headers: Vec<KeyValue>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub body: String,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    #[ts(optional, as = "Option<u32>")]
    pub delay_ms: u64,
    /// Only match requests with these query parameters (empty value = any value).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, as = "Option<Vec<KeyValue>>")]
    pub match_query: Vec<KeyValue>,
    /// Only match requests with these headers (empty value = any value).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, as = "Option<Vec<KeyValue>>")]
    pub match_headers: Vec<KeyValue>,
    /// Only match requests whose body contains this text.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub match_body: String,
    #[serde(default, skip_serializing_if = "is_default")]
    #[ts(optional, as = "Option<MockFault>")]
    pub fault: MockFault,
    /// How often the fault happens, in percent.
    #[serde(default = "hundred", skip_serializing_if = "is_hundred")]
    #[ts(optional, as = "Option<u8>")]
    pub fault_percent: u8,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    #[ts(optional, as = "Option<bool>")]
    pub enabled: bool,
}

impl Default for MockRoute {
    fn default() -> Self {
        Self {
            name: String::new(),
            method: any_method(),
            path: "/".into(),
            status: 200,
            headers: Vec::new(),
            body: String::new(),
            delay_ms: 0,
            match_query: Vec::new(),
            match_headers: Vec::new(),
            match_body: String::new(),
            fault: MockFault::None,
            fault_percent: 100,
            enabled: true,
        }
    }
}

/// What happens to requests no route matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum MockFallback {
    /// 404 with a short explanation.
    #[default]
    NotFound,
    /// Forward to `proxyUrl` (the real backend) and pass its answer back.
    Proxy,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HttpMockConfig {
    #[serde(default)]
    pub routes: Vec<MockRoute>,
    #[serde(default, skip_serializing_if = "is_default")]
    #[ts(optional, as = "Option<MockFallback>")]
    pub fallback: MockFallback,
    /// Base URL of the real backend for the `proxy` fallback.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub proxy_url: String,
    /// Allow browsers to call the mock from other origins (CORS headers, preflight).
    #[serde(default, skip_serializing_if = "is_false")]
    #[ts(optional, as = "Option<bool>")]
    pub cors: bool,
}

// ---- WebSocket / SSE ---------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct WsServerConfig {
    #[serde(default, skip_serializing_if = "is_default")]
    #[ts(optional, as = "Option<ReplyMode>")]
    pub mode: ReplyMode,
    /// Sent to each client right after it connects.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub greeting: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, as = "Option<Vec<ReplyRule>>")]
    pub rules: Vec<ReplyRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SseEventTemplate {
    /// Event name (empty: the default `message`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub event: String,
    #[serde(default)]
    pub data: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SseServerConfig {
    /// Sent to each client in order after it connects.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, as = "Option<Vec<SseEventTemplate>>")]
    pub events: Vec<SseEventTemplate>,
    /// Pause between events (0: all at once).
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    #[ts(optional, as = "Option<u32>")]
    pub interval_ms: u64,
    /// Start over after the last event.
    #[serde(default, skip_serializing_if = "is_false")]
    #[ts(optional, as = "Option<bool>")]
    pub repeat: bool,
}

// ---- TCP / UDP ---------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SocketServerConfig {
    #[serde(default, skip_serializing_if = "is_default")]
    #[ts(optional, as = "Option<ReplyMode>")]
    pub mode: ReplyMode,
    /// TCP: sent to each client right after it connects.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub greeting: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, as = "Option<Vec<ReplyRule>>")]
    pub rules: Vec<ReplyRule>,
    /// Encoding of greeting, rule patterns and replies.
    #[serde(default, skip_serializing_if = "is_default")]
    #[ts(optional, as = "Option<PayloadEncoding>")]
    pub encoding: PayloadEncoding,
    /// TCP: how incoming bytes are split into messages.
    #[serde(default, skip_serializing_if = "is_default")]
    #[ts(optional, as = "Option<Framing>")]
    pub framing: Framing,
    #[serde(default = "two", skip_serializing_if = "is_two")]
    #[ts(optional, as = "Option<u8>")]
    pub length_bytes: u8,
    /// Appended to text replies.
    #[serde(default, skip_serializing_if = "is_default")]
    #[ts(optional, as = "Option<LineEnding>")]
    pub line_ending: LineEnding,
}

impl Default for SocketServerConfig {
    fn default() -> Self {
        Self {
            mode: ReplyMode::Echo,
            greeting: String::new(),
            rules: Vec::new(),
            encoding: PayloadEncoding::Text,
            framing: Framing::Raw,
            length_bytes: 2,
            line_ending: LineEnding::None,
        }
    }
}

// ---- DNS ----------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DnsRecord {
    /// Name, e.g. `api.example.test` or `*.example.test`.
    #[serde(default)]
    pub name: String,
    /// A, AAAA, CNAME, TXT, MX, NS, PTR, SRV, CAA.
    #[serde(default, rename = "type")]
    pub record_type: String,
    /// Record data as in a zone file (`10 mail.example.test` for MX).
    #[serde(default)]
    pub value: String,
    #[serde(default = "sixty")]
    pub ttl: u32,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    #[ts(optional, as = "Option<bool>")]
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DnsServerConfig {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, as = "Option<Vec<DnsRecord>>")]
    pub records: Vec<DnsRecord>,
    /// Where names without a record go: empty = answer "no such name",
    /// `system` = this computer's resolver, or a server such as `1.1.1.1`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub upstream: String,
}

// ---- TCP relay ----------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TcpProxyConfig {
    /// `host:port` every client connection is relayed to.
    #[serde(default)]
    pub target: String,
    /// Connect to the target with TLS (clients still talk plain TCP to the relay).
    #[serde(default, skip_serializing_if = "is_false")]
    #[ts(optional, as = "Option<bool>")]
    pub upstream_tls: bool,
}

// ---- server -----------------------------------------------------------------

/// A saved server. Only the section for its `kind` is used; the others are
/// kept, so switching the kind in the UI never loses what was set up.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Server {
    pub name: String,
    #[serde(default)]
    pub kind: ServerKind,
    /// Sort position in the sidebar.
    #[serde(default)]
    pub seq: u32,
    /// Address to listen on: `127.0.0.1` (only this computer, the default) or
    /// `0.0.0.0` (other devices too).
    #[serde(default = "loopback")]
    pub host: String,
    /// Port (0: any free port).
    #[serde(default)]
    pub port: u16,
    #[serde(default, skip_serializing_if = "is_default")]
    #[ts(optional, as = "Option<ServerTls>")]
    pub tls: ServerTls,
    /// Start when the workspace is opened.
    #[serde(default, skip_serializing_if = "is_false")]
    #[ts(optional, as = "Option<bool>")]
    pub auto_start: bool,
    #[serde(default, skip_serializing_if = "is_default")]
    #[ts(optional, as = "Option<HttpMockConfig>")]
    pub http: HttpMockConfig,
    #[serde(default, skip_serializing_if = "is_default")]
    #[ts(optional, as = "Option<WsServerConfig>")]
    pub websocket: WsServerConfig,
    #[serde(default, skip_serializing_if = "is_default")]
    #[ts(optional, as = "Option<SseServerConfig>")]
    pub sse: SseServerConfig,
    /// TCP and UDP servers.
    #[serde(default, skip_serializing_if = "is_default")]
    #[ts(optional, as = "Option<SocketServerConfig>")]
    pub socket: SocketServerConfig,
    #[serde(default, skip_serializing_if = "is_default")]
    #[ts(optional, as = "Option<DnsServerConfig>")]
    pub dns: DnsServerConfig,
    #[serde(default, skip_serializing_if = "is_default")]
    #[ts(optional, as = "Option<TcpProxyConfig>")]
    pub proxy: TcpProxyConfig,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub docs: String,
}

impl Server {
    pub fn new(name: impl Into<String>, kind: ServerKind) -> Self {
        let port = match kind {
            ServerKind::Http | ServerKind::Sse => 3000,
            ServerKind::Websocket => 3001,
            ServerKind::Tcp => 9000,
            ServerKind::Udp => 9001,
            // Not 5353: that is mDNS, already taken on macOS and often on Windows.
            ServerKind::Dns => 1053,
            ServerKind::TcpProxy => 9100,
        };
        Self {
            name: name.into(),
            kind,
            seq: 0,
            host: loopback(),
            port,
            tls: ServerTls::default(),
            auto_start: false,
            http: HttpMockConfig::default(),
            websocket: WsServerConfig::default(),
            sse: SseServerConfig::default(),
            socket: SocketServerConfig::default(),
            dns: DnsServerConfig::default(),
            proxy: TcpProxyConfig::default(),
            docs: String::new(),
        }
    }
}

/// A saved server in the sidebar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ServerNode {
    /// File stem, used as the server id (`servers/<id>.yaml`).
    pub id: String,
    pub name: String,
    pub kind: ServerKind,
    pub host: String,
    pub port: u16,
    pub tls: bool,
    pub seq: u32,
    pub auto_start: bool,
    /// Set when the file could not be parsed.
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimal_yaml_and_round_trip() {
        let s: Server = serde_yaml_ng::from_str("name: Mock\nport: 8080\n").unwrap();
        assert_eq!(s.kind, ServerKind::Http);
        assert_eq!(s.host, "127.0.0.1");
        let route: MockRoute = serde_yaml_ng::from_str("path: /users/:id\n").unwrap();
        assert_eq!((route.method.as_str(), route.status, route.fault_percent, route.enabled), ("*", 200, 100, true));

        let mut s = Server::new("Echo", ServerKind::Tcp);
        s.socket.rules.push(ReplyRule { pattern: "ping".into(), reply: "pong".into(), ..Default::default() });
        let yaml = serde_yaml_ng::to_string(&s).unwrap();
        // Unused sections are left out of the file.
        assert!(!yaml.contains("http:") && !yaml.contains("dns:"), "{yaml}");
        assert!(yaml.contains("match: contains"), "{yaml}");
        assert_eq!(serde_yaml_ng::from_str::<Server>(&yaml).unwrap(), s);
    }
}

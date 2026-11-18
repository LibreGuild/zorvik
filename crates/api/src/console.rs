//! The console: every request the app sends with what was sent and what came back, and what
//! scripts write with `console.log`, in the order it happened (docs/architecture.md, "Console").
//!
//! It is kept in memory only (never in a file), bounded by lines and by bytes, and cleared
//! when another workspace opens. The UI gets each line as an event, small enough to list
//! thousands of them, and asks for a request's headers and bodies (`console.details`) when the
//! user opens it. Values are as sent, secrets included: that is what a failed request is
//! debugged with. What agents read (`read_console`) is redacted like everything they get.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use ts_rs::TS;
use zorvik_engine::{Header, HttpRequest, HttpResponse, RedirectHop, ResponseMeta, Timing, TlsInfo};
use zorvik_script::ConsoleLevel;
use zorvik_workspace::formats::{Request, RequestKind};

use crate::agents::redact::Redactor;
use crate::{ApiError, EventSink, StreamEvent, lock, now_ms};

/// Lines kept; beyond it the oldest go.
const MAX_LINES: usize = 5000;
/// Bytes kept (lines, headers and bodies together); beyond it the oldest lines go.
const MAX_BYTES: usize = 64 << 20;
/// Body text kept of each request and each response.
const MAX_BODY: usize = 64 * 1024;
/// Text kept of a log line or an error.
const MAX_MESSAGE: usize = 16 * 1024;

/// `origin` of a request a script sent, and of one that fetched an OAuth 2.0 token.
pub(crate) const SEND_REQUEST: &str = "pm.sendRequest";
pub(crate) const OAUTH_TOKEN: &str = "OAuth 2.0 token";
/// `origin` of the query that reads a GraphQL schema.
pub(crate) const GRAPHQL_SCHEMA: &str = "GraphQL schema";

/// Who had the request sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ConsoleSource {
    /// The user, from a request tab.
    User,
    /// A collection run.
    Runner,
    /// An AI agent.
    Agent,
}

/// One line of the console.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ConsoleLine {
    /// Counts up: the order things happened in.
    #[ts(type = "number")]
    pub id: u64,
    /// Milliseconds since the Unix epoch.
    #[ts(type = "number")]
    pub at: i64,
    pub source: ConsoleSource,
    /// The request it belongs to, by name.
    pub request: Option<String>,
    pub item: ConsoleItem,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(export)]
pub enum ConsoleItem {
    /// A line a script wrote (`console.log`), a script that failed, or a note of the app's.
    #[serde(rename_all = "camelCase")]
    Log { level: ConsoleLevel, message: String },
    /// A request, or a connection being opened. `console.details` has its headers and bodies.
    #[serde(rename_all = "camelCase")]
    Exchange {
        /// `HTTP`, `WebSocket`, `gRPC`, `MQTT 5`, …
        protocol: String,
        /// The HTTP method, or the gRPC method; empty when the protocol has none.
        method: String,
        url: String,
        status: Option<u16>,
        status_text: Option<String>,
        /// Why nothing came back, or what the server answered instead of success.
        error: Option<String>,
        duration_ms: Option<f64>,
        /// Bytes of the response body.
        #[ts(type = "number | null")]
        size: Option<u64>,
        /// A request the app made for another one: `pm.sendRequest`, `OAuth 2.0 token`,
        /// `GraphQL schema`.
        origin: Option<String>,
    },
}

/// A body as the console keeps it: the start of it as text.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ConsoleBody {
    /// `None` for binary data.
    pub text: Option<String>,
    /// Bytes of the whole body.
    #[ts(type = "number")]
    pub size: u64,
    /// `text` is only the start of the body.
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ConsoleResponse {
    pub status: u16,
    pub status_text: String,
    pub http_version: String,
    pub headers: Vec<Header>,
    pub body: Option<ConsoleBody>,
}

/// What an exchange sent and what came back.
#[derive(Debug, Clone, Default, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ConsoleDetails {
    pub request_headers: Vec<Header>,
    /// The headers are the request's own, as prepared: nothing came back, so the ones added
    /// while sending (`Host`, `User-Agent`, cookies) are not known.
    pub prepared: bool,
    pub request_body: Option<ConsoleBody>,
    pub response: Option<ConsoleResponse>,
    /// Where the response came from, when redirects led elsewhere.
    pub final_url: Option<String>,
    pub redirects: Vec<RedirectHop>,
    pub remote_addr: Option<String>,
    pub proxy: Option<String>,
    pub tls: Option<TlsInfo>,
    pub timing: Option<Timing>,
}

impl ConsoleDetails {
    fn weight(&self) -> usize {
        let headers = |h: &[Header]| h.iter().map(|h| h.name.len() + h.value.len() + 16).sum::<usize>();
        let body = |b: &Option<ConsoleBody>| b.as_ref().and_then(|b| b.text.as_ref()).map_or(0, String::len);
        headers(&self.request_headers)
            + body(&self.request_body)
            + self.response.as_ref().map_or(0, |r| headers(&r.headers) + body(&r.body))
            + self.redirects.iter().map(|r| r.url.len() + r.location.len() + 16).sum::<usize>()
            + 256
    }
}

/// What the UI is told.
#[allow(clippy::large_enum_variant)] // lines are the common case
#[derive(Debug, Clone, Serialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(export)]
pub enum ConsoleEvent {
    Line { line: ConsoleLine },
    Cleared,
}

/// The start of `bytes` as text, or `None` for binary data. Never cut inside a character.
fn body_of(bytes: &[u8], cut_before: bool) -> Option<ConsoleBody> {
    if bytes.is_empty() {
        return None;
    }
    let mut end = bytes.len().min(MAX_BODY);
    while end < bytes.len() && end > 0 && (bytes[end] & 0xC0) == 0x80 {
        end -= 1;
    }
    let head = &bytes[..end];
    // Text in another encoding still reads better than nothing; data with NUL bytes doesn't.
    let text = match std::str::from_utf8(head) {
        Ok(text) => Some(text.to_string()),
        Err(_) if !head.contains(&0) => Some(String::from_utf8_lossy(head).into_owned()),
        Err(_) => None,
    };
    Some(ConsoleBody { text, size: bytes.len() as u64, truncated: end < bytes.len() || cut_before })
}

fn clip(text: &str) -> String {
    if text.len() <= MAX_MESSAGE {
        return text.to_string();
    }
    let mut end = MAX_MESSAGE;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}… (cut)", &text[..end])
}

/// What the console shows for a request kind.
pub(crate) fn protocol_of(request: &Request) -> &'static str {
    match request.kind {
        RequestKind::Http if request.body.body_type == zorvik_workspace::formats::BodyType::Graphql => "GraphQL",
        RequestKind::Http => "HTTP",
        RequestKind::Grpc => "gRPC",
        RequestKind::Dns => "DNS",
        RequestKind::Websocket => "WebSocket",
        RequestKind::Sse => "SSE",
        RequestKind::Tcp => "TCP",
        RequestKind::Udp => "UDP",
        RequestKind::Mqtt => "MQTT",
        RequestKind::SocketIo => "Socket.IO",
        RequestKind::Mcp => "MCP",
    }
}

/// A request or a connection, ready for the console.
pub(crate) struct Exchange {
    protocol: String,
    method: String,
    url: String,
    status: Option<u16>,
    status_text: Option<String>,
    error: Option<String>,
    duration_ms: Option<f64>,
    size: Option<u64>,
    origin: Option<&'static str>,
    /// Credentials by nature (a token request): agents get the line without its details.
    sensitive: bool,
    details: ConsoleDetails,
}

impl Exchange {
    /// A request that got its response. `body` is the response body, when it was read.
    pub fn answered(
        protocol: &str,
        sent: &HttpRequest,
        meta: &ResponseMeta,
        timing: &Timing,
        body: Option<&[u8]>,
    ) -> Self {
        // Kinds that are not HTTP underneath (gRPC, MCP) describe no request of their own.
        let on_wire = !meta.request.headers.is_empty();
        let details = ConsoleDetails {
            request_headers: if on_wire { meta.request.headers.clone() } else { sent.headers.clone() },
            prepared: false,
            request_body: body_of(&sent.body, false),
            response: Some(ConsoleResponse {
                status: meta.status,
                status_text: meta.status_text.clone(),
                http_version: meta.http_version.clone(),
                headers: meta.headers.clone(),
                body: body.and_then(|b| body_of(b, false)),
            }),
            final_url: (!meta.redirects.is_empty()).then(|| meta.url.clone()),
            redirects: meta.redirects.clone(),
            remote_addr: meta.remote_addr.clone(),
            proxy: meta.request.proxy.clone(),
            tls: meta.tls.clone(),
            timing: Some(timing.clone()),
        };
        Self {
            protocol: protocol.to_string(),
            method: sent.method.clone(),
            url: sent.url.clone(),
            status: Some(meta.status),
            status_text: Some(meta.status_text.clone()),
            error: None,
            duration_ms: Some(timing.total_ms),
            size: body.map(|b| b.len() as u64),
            origin: None,
            sensitive: false,
            details,
        }
    }

    /// A connection that opened with an HTTP handshake (WebSocket, an event stream): the
    /// request as it went out and the answer's head.
    pub fn opened(protocol: &str, meta: &ResponseMeta, timing: &Timing) -> Self {
        let sent = HttpRequest {
            method: meta.request.method.clone(),
            url: meta.request.url.clone(),
            headers: Vec::new(),
            body: Default::default(),
        };
        Self::answered(protocol, &sent, meta, timing, None)
    }

    /// [`Self::answered`] for a response read to its end.
    pub fn response(protocol: &str, sent: &HttpRequest, response: &HttpResponse) -> Self {
        let mut x = Self::answered(protocol, sent, &response.meta, &response.timing, Some(&response.body));
        if response.body_truncated
            && let Some(body) = x.details.response.as_mut().and_then(|r| r.body.as_mut())
        {
            body.truncated = true;
        }
        x
    }

    /// A request that was prepared and got no response.
    pub fn failed(protocol: &str, sent: &HttpRequest, error: &str, elapsed: Duration) -> Self {
        Self {
            protocol: protocol.to_string(),
            method: sent.method.clone(),
            url: sent.url.clone(),
            status: None,
            status_text: None,
            error: Some(clip(error)),
            duration_ms: Some(elapsed.as_secs_f64() * 1000.0),
            size: None,
            origin: None,
            sensitive: false,
            details: ConsoleDetails {
                request_headers: sent.headers.clone(),
                prepared: true,
                request_body: body_of(&sent.body, false),
                ..Default::default()
            },
        }
    }

    /// A request that could not be prepared (an undefined variable, a file that is missing):
    /// as it is written, with `render` filling in what variables it can.
    pub fn unprepared(request: &Request, render: &dyn Fn(&str) -> String, error: &str, elapsed: Duration) -> Self {
        let text = request.body.text.trim();
        Self {
            protocol: protocol_of(request).to_string(),
            method: if matches!(request.kind, RequestKind::Http | RequestKind::Sse | RequestKind::Grpc) {
                request.method.clone()
            } else {
                String::new()
            },
            url: render(request.url.trim()),
            status: None,
            status_text: None,
            error: Some(clip(error)),
            duration_ms: Some(elapsed.as_secs_f64() * 1000.0),
            size: None,
            origin: None,
            sensitive: false,
            details: ConsoleDetails {
                request_headers: request
                    .headers
                    .iter()
                    .filter(|h| h.enabled && !h.key.trim().is_empty())
                    .map(|h| Header::new(render(&h.key), render(&h.value)))
                    .collect(),
                prepared: true,
                request_body: body_of(render(text).as_bytes(), false),
                ..Default::default()
            },
        }
    }

    /// A connection or a call of a protocol without HTTP's shape (TCP, MQTT, DNS, gRPC):
    /// `outcome` is what to show as its result, or the error.
    pub fn plain(
        protocol: &str,
        method: &str,
        url: &str,
        outcome: Result<Option<String>, &str>,
        elapsed: Duration,
    ) -> Self {
        let (status_text, error) = match outcome {
            Ok(text) => (text, None),
            Err(e) => (None, Some(clip(e))),
        };
        Self {
            protocol: protocol.to_string(),
            method: method.to_string(),
            url: url.to_string(),
            status: None,
            status_text,
            error,
            duration_ms: Some(elapsed.as_secs_f64() * 1000.0),
            size: None,
            origin: None,
            sensitive: false,
            details: ConsoleDetails::default(),
        }
    }

    pub fn origin(mut self, origin: &'static str) -> Self {
        self.origin = Some(origin);
        self
    }

    pub fn sensitive(mut self) -> Self {
        self.sensitive = true;
        self
    }

    pub fn error(mut self, error: Option<String>) -> Self {
        self.error = error.map(|e| clip(&e));
        self
    }

    pub fn network(mut self, remote_addr: Option<String>, tls: Option<TlsInfo>, timing: &Timing) -> Self {
        self.details.remote_addr = remote_addr;
        self.details.tls = tls;
        self.details.timing = Some(timing.clone());
        self.duration_ms = Some(timing.total_ms);
        self
    }
}

struct Kept {
    line: ConsoleLine,
    details: Option<Arc<ConsoleDetails>>,
    sensitive: bool,
    /// The secret values in use when it was logged, hidden from agents (they may have changed
    /// since: a script that rotates a key, another workspace). Shared with the scope that
    /// logged it, which may learn of more ([`ConsoleScope::hide`]).
    redact: Arc<Mutex<Redactor>>,
    weight: usize,
}

#[derive(Default)]
struct Store {
    next_id: u64,
    lines: VecDeque<Kept>,
    bytes: usize,
}

/// The app's console (one per `Api`).
pub(crate) struct Console {
    sink: Arc<dyn EventSink>,
    store: Mutex<Store>,
}

/// A line as agents read it: with its details, whether those are credentials, and the secret
/// values to hide in it.
pub(crate) struct Read {
    pub line: ConsoleLine,
    pub details: Option<Arc<ConsoleDetails>>,
    pub sensitive: bool,
    pub redact: Arc<Mutex<Redactor>>,
}

impl Console {
    pub fn new(sink: Arc<dyn EventSink>) -> Self {
        Self { sink, store: Mutex::new(Store::default()) }
    }

    fn push(
        &self,
        source: ConsoleSource,
        request: Option<&str>,
        item: ConsoleItem,
        details: Option<ConsoleDetails>,
        sensitive: bool,
        redact: Arc<Mutex<Redactor>>,
    ) {
        let text = match &item {
            ConsoleItem::Log { message, .. } => message.len(),
            ConsoleItem::Exchange { url, error, .. } => url.len() + error.as_ref().map_or(0, String::len),
        };
        let weight = 128 + text + details.as_ref().map_or(0, ConsoleDetails::weight);
        // The event leaves under the lock, so the UI gets lines in the order of their ids.
        let mut store = lock(&self.store);
        store.next_id += 1;
        let line = ConsoleLine { id: store.next_id, at: now_ms(), source, request: request.map(str::to_string), item };
        store.bytes += weight;
        store.lines.push_back(Kept { line: line.clone(), details: details.map(Arc::new), sensitive, redact, weight });
        while store.lines.len() > MAX_LINES || (store.bytes > MAX_BYTES && store.lines.len() > 1) {
            if let Some(old) = store.lines.pop_front() {
                store.bytes -= old.weight;
            }
        }
        self.sink.emit(StreamEvent::Console { event: ConsoleEvent::Line { line } });
    }

    /// `console.list`: every line kept, oldest first.
    pub fn list(&self) -> Vec<ConsoleLine> {
        lock(&self.store).lines.iter().map(|k| k.line.clone()).collect()
    }

    /// `console.details`: what the exchange `id` sent and got, while it is kept.
    pub fn details(&self, id: u64) -> Option<Arc<ConsoleDetails>> {
        let store = lock(&self.store);
        // Ids count up, so the line is found by its distance from the oldest one.
        let first = store.lines.front()?.line.id;
        store.lines.get(usize::try_from(id.checked_sub(first)?).ok()?)?.details.clone()
    }

    /// The newest `limit` lines that `keep` lets through, oldest first.
    pub fn read(&self, limit: usize, keep: impl Fn(&Read) -> bool) -> Vec<Read> {
        // Copied out first (lines are small, details shared): `keep` may take its time.
        let all: Vec<Read> = lock(&self.store)
            .lines
            .iter()
            .map(|k| Read {
                line: k.line.clone(),
                details: k.details.clone(),
                sensitive: k.sensitive,
                redact: k.redact.clone(),
            })
            .collect();
        let mut out: Vec<Read> = all.into_iter().rev().filter(|r| keep(r)).take(limit).collect();
        out.reverse();
        out
    }

    /// `console.clear`, and when another workspace opens. Ids go on counting.
    pub fn clear(&self) {
        let mut store = lock(&self.store);
        store.lines.clear();
        store.bytes = 0;
        self.sink.emit(StreamEvent::Console { event: ConsoleEvent::Cleared });
    }
}

/// The console, for the requests of one source (and one request, once named).
#[derive(Clone)]
pub struct ConsoleScope {
    console: Arc<Console>,
    source: ConsoleSource,
    request: Option<Arc<str>>,
    /// The workspace's secret values now, for what agents read of these lines later.
    redact: Arc<Mutex<Redactor>>,
}

impl ConsoleScope {
    pub(crate) fn new(console: Arc<Console>, source: ConsoleSource, redact: Option<Redactor>) -> Self {
        Self { console, source, request: None, redact: Arc::new(Mutex::new(redact.unwrap_or_default())) }
    }

    /// A secret value learned while running (a scenario's secret output, a secret answer):
    /// agents read `{{key}}` for it in every line of this scope, also the ones logged before.
    pub(crate) fn hide(&self, key: &str, value: &str) {
        lock(&self.redact).hide(key, value);
    }

    /// The same console, for the request named `name`.
    pub(crate) fn of(&self, name: &str) -> Self {
        let name = name.trim();
        Self { request: (!name.is_empty()).then(|| name.into()), ..self.clone() }
    }

    pub(crate) fn log(&self, level: ConsoleLevel, message: &str) {
        let item = ConsoleItem::Log { level, message: clip(message) };
        self.console.push(self.source, self.request.as_deref(), item, None, false, self.redact.clone());
    }

    pub(crate) fn exchange(&self, x: Exchange) {
        let item = ConsoleItem::Exchange {
            protocol: x.protocol,
            method: x.method,
            url: x.url,
            status: x.status,
            status_text: x.status_text,
            error: x.error,
            duration_ms: x.duration_ms,
            size: x.size,
            origin: x.origin.map(str::to_string),
        };
        self.console.push(
            self.source,
            self.request.as_deref(),
            item,
            Some(x.details),
            x.sensitive,
            self.redact.clone(),
        );
    }

    /// A request that ended with `error` before anything was sent, unless it was cancelled
    /// (the user stopped it) or skipped by a script, which are not failures.
    pub(crate) fn failure(
        &self,
        request: &Request,
        render: &dyn Fn(&str) -> String,
        error: &ApiError,
        elapsed: Duration,
    ) {
        if error.network_kind == Some(zorvik_engine::ErrorKind::Cancelled) || error.code == crate::scripting::SKIPPED {
            return;
        }
        self.exchange(Exchange::unprepared(request, render, &error.message, elapsed));
    }

    /// Engine options that report the requests made with them (an OAuth 2.0 token request)
    /// as `origin` lines.
    pub(crate) fn observing(
        &self,
        opts: &zorvik_engine::RequestOptions,
        origin: &'static str,
    ) -> zorvik_engine::RequestOptions {
        let scope = self.clone();
        let observer =
            move |sent: &HttpRequest, result: Result<&HttpResponse, &zorvik_engine::EngineError>, elapsed: Duration| {
                let x = match result {
                    Ok(response) => Exchange::response("HTTP", sent, response),
                    Err(e) => Exchange::failed("HTTP", sent, &e.message, elapsed),
                };
                scope.exchange(x.origin(origin).sensitive());
            };
        zorvik_engine::RequestOptions { observer: Some(zorvik_engine::Observer(Arc::new(observer))), ..opts.clone() }
    }
}

/// How a connection (or a call that isn't an HTTP request) went, for [`crate::Api::console_connect`].
pub(crate) enum Opened<'a> {
    /// An HTTP handshake was answered (WebSocket, an event stream).
    Handshake(&'a ResponseMeta, &'a Timing),
    /// A socket session (TCP, UDP, MQTT, Socket.IO, a GraphQL subscription).
    Socket(&'a zorvik_engine::SocketOpened),
    /// It worked, in a few words (`NOERROR · 2 answers`).
    Done(String),
    /// The other side answered with an error (a gRPC status).
    Refused(String),
}

impl crate::Api {
    /// A connection being opened, or a call of a protocol that isn't HTTP, as a console line:
    /// how it went or why it failed. `prepared` is the request as resolved, when it got that far.
    pub(crate) fn console_connect(
        &self,
        request: &Request,
        prepared: Option<&HttpRequest>,
        started: std::time::Instant,
        outcome: Result<Opened<'_>, &ApiError>,
    ) -> ConsoleScope {
        let scope = self.console_scope().of(&request.name);
        let protocol = protocol_of(request);
        let elapsed = started.elapsed();
        let render = |text: &str| match self.try_ws() {
            Some(ws) => self.var_context(&ws).render(text, &mut std::collections::BTreeSet::new()),
            None => text.to_string(),
        };
        let url = || prepared.map_or_else(|| render(request.url.trim()), |p| p.url.clone());
        let method = if request.kind == RequestKind::Grpc { request.method.as_str() } else { "" };
        let with_request = |x: Exchange| match prepared {
            Some(p) => Exchange { details: ConsoleDetails { request_headers: p.headers.clone(), ..x.details }, ..x },
            None => x,
        };
        match outcome {
            Ok(Opened::Handshake(meta, timing)) => scope.exchange(Exchange::opened(protocol, meta, timing)),
            Ok(Opened::Socket(opened)) => {
                let x = Exchange::plain(&opened.protocol, method, &url(), Ok(Some("Connected".into())), elapsed);
                scope.exchange(with_request(x).network(opened.remote_addr.clone(), opened.tls.clone(), &opened.timing));
            }
            Ok(Opened::Done(text)) => {
                scope.exchange(with_request(Exchange::plain(protocol, method, &url(), Ok(Some(text)), elapsed)))
            }
            Ok(Opened::Refused(text)) => {
                scope.exchange(with_request(Exchange::plain(protocol, method, &url(), Err(&text), elapsed)))
            }
            Err(e) if e.network_kind == Some(zorvik_engine::ErrorKind::Cancelled) => {}
            Err(e) => match prepared {
                Some(sent) => scope.exchange(Exchange {
                    method: method.into(),
                    ..Exchange::failed(protocol, sent, &e.message, elapsed)
                }),
                None => scope.failure(request, &render, e, elapsed),
            },
        }
        scope
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Collect(Mutex<Vec<StreamEvent>>);

    impl EventSink for Collect {
        fn emit(&self, event: StreamEvent) {
            self.0.lock().unwrap().push(event);
        }
    }

    fn scope() -> (ConsoleScope, Arc<Console>, Arc<Collect>) {
        let sink = Arc::new(Collect::default());
        let console = Arc::new(Console::new(sink.clone()));
        (ConsoleScope::new(console.clone(), ConsoleSource::User, None), console, sink)
    }

    fn sent(body: &'static [u8]) -> HttpRequest {
        HttpRequest {
            method: "POST".into(),
            url: "http://x.test/a".into(),
            headers: vec![Header::new("X-A", "1")],
            body: body.into(),
        }
    }

    #[test]
    fn lines_keep_their_order_and_details_are_found_by_id() {
        let (scope, console, sink) = scope();
        scope.log(ConsoleLevel::Log, "one");
        scope.of("Get user").exchange(Exchange::failed("HTTP", &sent(b"{}"), "refused", Duration::from_millis(5)));
        scope.log(ConsoleLevel::Warn, "three");
        let lines = console.list();
        assert_eq!(lines.iter().map(|l| l.id).collect::<Vec<_>>(), [1, 2, 3]);
        assert_eq!(lines[1].request.as_deref(), Some("Get user"));
        assert!(console.details(1).is_none(), "a log line has no details");
        let details = console.details(2).expect("the exchange's details");
        assert!(details.prepared);
        assert_eq!(details.request_body.as_ref().unwrap().text.as_deref(), Some("{}"));
        assert!(console.details(4).is_none() && console.details(0).is_none());
        assert_eq!(sink.0.lock().unwrap().len(), 3);

        console.clear();
        assert!(console.list().is_empty());
        scope.log(ConsoleLevel::Log, "after");
        assert_eq!(console.list()[0].id, 4, "ids go on after a clear");
        assert!(console.details(2).is_none());
    }

    #[test]
    fn a_secret_learned_later_is_hidden_in_the_lines_before_it_too() {
        let (scope, console, _) = scope();
        scope.of("Log in").log(ConsoleLevel::Log, "token tok-12345");
        scope.hide("token", "tok-12345");
        scope.log(ConsoleLevel::Log, "again tok-12345");
        let read = console.read(10, |_| true);
        let shown: Vec<String> = read
            .iter()
            .map(|r| match &r.line.item {
                ConsoleItem::Log { message, .. } => lock(&r.redact).text(message),
                ConsoleItem::Exchange { .. } => String::new(),
            })
            .collect();
        assert_eq!(shown, ["token {{token}}", "again {{token}}"]);
        // Another scope's lines don't hide it.
        let (other, console, _) = self::scope();
        other.log(ConsoleLevel::Log, "tok-12345");
        assert!(lock(&console.read(1, |_| true)[0].redact).is_empty());
    }

    #[test]
    fn the_oldest_lines_go_first() {
        let (scope, console, _) = scope();
        for i in 0..MAX_LINES + 10 {
            scope.log(ConsoleLevel::Log, &i.to_string());
        }
        let lines = console.list();
        assert_eq!(lines.len(), MAX_LINES);
        assert_eq!(lines[0].id, 11);
        // By bytes too: bodies of 64 KB each.
        let body: &'static [u8] = Box::leak(vec![b'a'; MAX_BODY].into_boxed_slice());
        for _ in 0..(MAX_BYTES / MAX_BODY) + 50 {
            scope.exchange(Exchange::failed("HTTP", &sent(body), "refused", Duration::ZERO));
        }
        let store = console.store.lock().unwrap();
        assert!(store.bytes <= MAX_BYTES, "{}", store.bytes);
        assert!(store.lines.len() < MAX_BYTES / MAX_BODY + 50);
        assert_eq!(store.bytes, store.lines.iter().map(|k| k.weight).sum::<usize>());
    }

    #[test]
    fn bodies_are_cut_and_binary_ones_have_no_text() {
        assert!(body_of(b"", false).is_none());
        let text = body_of("héllo".as_bytes(), false).unwrap();
        assert_eq!((text.text.as_deref(), text.size, text.truncated), (Some("héllo"), 6, false));
        // Cut before a character, not inside it.
        let mut long = vec![b'a'; MAX_BODY - 1];
        long.extend("é".as_bytes());
        let cut = body_of(&long, false).unwrap();
        assert_eq!(cut.text.as_ref().unwrap().len(), MAX_BODY - 1);
        assert!(cut.truncated);
        let binary = body_of(&[0, 159, 146, 150], false).unwrap();
        assert_eq!((binary.text, binary.size), (None, 4));
        // Latin-1 text is shown as well as it can be.
        assert!(body_of(&[b'c', b'a', b'f', 0xE9], false).unwrap().text.is_some());
        assert!(clip(&"é".repeat(MAX_MESSAGE)).ends_with("… (cut)"));
    }

    #[test]
    fn reading_takes_the_newest_lines_oldest_first() {
        let (scope, console, _) = scope();
        for i in 0..5 {
            scope.log(if i % 2 == 0 { ConsoleLevel::Log } else { ConsoleLevel::Error }, &i.to_string());
        }
        let errors = console.read(1, |r| matches!(r.line.item, ConsoleItem::Log { level: ConsoleLevel::Error, .. }));
        assert_eq!(errors.iter().map(|r| r.line.id).collect::<Vec<_>>(), [4]);
        assert_eq!(console.read(3, |_| true).iter().map(|r| r.line.id).collect::<Vec<_>>(), [3, 4, 5]);
    }
}

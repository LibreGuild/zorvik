//! Live connections for agents (WebSocket, Socket.IO, TCP, UDP, MQTT): open one (running a
//! WebSocket or Socket.IO request's on-connect steps), send, wait for a message that matches,
//! read the messages, close. A connection the agent opens shows in a tab when the user follows
//! agents, and closes when the agent disconnects.

use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};
use tokio::time::Instant;
use zorvik_engine::{SocketOutgoing, WsOutgoing};
use zorvik_workspace::formats::matching::MessageMatcher;
use zorvik_workspace::formats::{MatchKind, MessageFormat, RequestKind, SavedMessage, WaitStep};

use super::{Call, Def, Done, Fail, MAX_EVENT_CHARS, Outcome, cut, obj, string};
use crate::agents::redact::Redactor;
use crate::agents::{AgentEvent, AgentTarget, Opened};
use crate::live::{Connected, LoggedMessage, MessageLog, OVERRUN, StepResult, Waited, wait_target};
use crate::sockets::socket_outgoing;
use crate::websocket::outgoing;
use crate::{Api, lock};

const DEFAULT_WAIT_MS: u64 = 10_000;
const MAX_WAIT_MS: u64 = 120_000;
/// How long `open_connection` waits for the on-connect steps (default).
const STEPS_WAIT_SECS: u64 = 30;
const DEFAULT_MESSAGES: usize = 50;
/// Messages `open_connection` returns (the first ones).
const FIRST_MESSAGES: usize = 20;
const MAX_MESSAGES: usize = 500;
/// Closed connections an agent can still read, per agent (the newest).
const KEEP_CLOSED: usize = 10;

/// A connection an agent opened. Kept after it closes, so the agent can read how it ended,
/// until the agent disconnects.
pub(crate) struct AgentSocket {
    /// The agent session that owns it.
    session: String,
    /// Opened before the other connections with a higher number.
    seq: u64,
    kind: RequestKind,
    /// The request's saved messages (for `message`).
    messages: Vec<SavedMessage>,
    /// MQTT: where a message goes when the call doesn't say.
    topic: String,
    qos: u8,
    retain: bool,
    log: Arc<MessageLog>,
    /// Where waits start by default: the newest message the agent saw, or the one before its last send.
    cursor: u64,
}

const KINDS: &[RequestKind] =
    &[RequestKind::Websocket, RequestKind::SocketIo, RequestKind::Tcp, RequestKind::Udp, RequestKind::Mqtt];

fn connection_id() -> Value {
    string("The connectionId open_connection returned.")
}

pub(super) fn defs() -> Vec<Def> {
    vec![
        Def {
            name: "open_connection",
            title: "Open a connection",
            description: "Open a live connection from a saved request (path), a saved one with changes (path + request), or an unsaved one (request) of kind websocket, socketio, tcp, udp or mqtt. WebSocket and Socket.IO requests run their on-connect steps (send saved messages, wait for replies). Returns a connectionId for the other connection tools, the handshake, the steps' results, the first messages and the request's saved messages (savedMessages: what send_message's `message` sends by name). Scripts of the request run too: scripts.preRequest before connecting, websocket.onMessage / socketio.onMessage on each message that matches (their notes show in the user's log). It stays open until close_connection, the server closing it, or the agent disconnecting. The user sees it live in Zorvik when they follow agents.",
            schema: obj(
                json!({
                    "path": string("Saved request path as list_requests shows it."),
                    "request": super::request_schema(),
                    "folder": string("Unsaved request: the folder whose auth and headers it inherits."),
                    "waitSeconds": { "type": "integer", "description": "How long to wait for the on-connect steps (default 30, at most 600)." },
                }),
                &[],
            ),
            read_only: false,
            destructive: false,
            open_world: true,
        },
        Def {
            name: "send_message",
            title: "Send a message",
            description: "Send one message on an open connection: text ({{variables}} filled in), json (any JSON value, sent as text), hex (binary), message (a saved message of the request, by name), or ping (WebSocket). Socket.IO: give event (and ack to ask for an acknowledgement); json is the arguments (an array for several). MQTT: topic, qos, retain (default: the request's). Then call wait_for_message for the reply: it looks at messages from this send on.",
            schema: obj(
                json!({
                    "connectionId": connection_id(),
                    "text": string("A text message; {{variables}} are filled in. Socket.IO: one string argument."),
                    "json": { "description": "A JSON value sent as a text message ({{variables}} in it are filled in). Socket.IO: the arguments (an array for several)." },
                    "hex": string("A binary message as hex bytes, e.g. 48 65 6c 6c 6f. Socket.IO: one binary argument."),
                    "message": string("The name of a saved message of the request."),
                    "ping": { "type": "boolean", "description": "WebSocket: send a ping (the server answers with a pong)." },
                    "event": string("Socket.IO: the event to emit."),
                    "ack": { "type": "boolean", "description": "Socket.IO: ask the server to acknowledge (the acknowledgement shows as a received message)." },
                    "topic": string("MQTT: the topic to publish to."),
                    "qos": { "type": "integer", "description": "MQTT: 0, 1 or 2." },
                    "retain": { "type": "boolean", "description": "MQTT: the broker keeps it for new subscribers." },
                }),
                &["connectionId"],
            ),
            read_only: false,
            destructive: false,
            open_world: true,
        },
        Def {
            name: "wait_for_message",
            title: "Wait for a message",
            description: "Wait until a received message matches (or timeoutMs passes, or the connection closes). By default it looks at messages after the last one you sent or saw, so a reply that already arrived counts. Pass sinceId (a message id) to choose. Socket.IO: event narrows it to one event, and the pattern is matched against its arguments as a JSON array; MQTT: event is the topic.",
            schema: obj(
                json!({
                    "connectionId": connection_id(),
                    "event": string("Socket.IO: only this event. MQTT: only this topic."),
                    "match": { "type": "string", "enum": ["any", "contains", "exact", "regex", "json"], "description": "Default contains. json: pattern is a condition like $.type == \"welcome\" (Socket.IO: $[0].room == lobby), $.id != 0, or just $.id (the value is there)." },
                    "pattern": string("What to look for (empty with contains: any message)."),
                    "timeoutMs": { "type": "integer", "description": "Default 10000, at most 120000." },
                    "sinceId": { "type": "integer", "description": "Only messages after this id." },
                }),
                &["connectionId"],
            ),
            read_only: true,
            destructive: false,
            open_world: false,
        },
        Def {
            name: "read_messages",
            title: "Read messages",
            description: "The connection's messages (sent and received), oldest first, the newest `limit` after sinceId. Pass the lastId from the previous call as sinceId to get only new ones. The last 1000 messages are kept.",
            schema: obj(
                json!({
                    "connectionId": connection_id(),
                    "sinceId": { "type": "integer" },
                    "limit": { "type": "integer", "description": "Default 50, at most 500." },
                }),
                &["connectionId"],
            ),
            read_only: true,
            destructive: false,
            open_world: false,
        },
        Def {
            name: "close_connection",
            title: "Close a connection",
            description: "Close the connection. WebSocket: optionally with a close code (1000 normal, 3000–4999 for your application) and reason.",
            schema: obj(
                json!({
                    "connectionId": connection_id(),
                    "code": { "type": "integer", "description": "WebSocket: default 1000." },
                    "reason": string("WebSocket: sent with the close frame."),
                }),
                &["connectionId"],
            ),
            read_only: false,
            destructive: false,
            open_world: false,
        },
    ]
}

/// A message as the agent gets it (text cut to a few thousand characters, secrets hidden).
fn message_for_agent(m: &LoggedMessage, r: &Redactor) -> Value {
    let mut out = json!({ "id": m.id, "direction": m.direction, "size": m.size, "timestamp": m.timestamp });
    if let Some(event) = &m.event {
        out["event"] = Value::String(r.text(event));
    }
    if let Some(detail) = &m.detail {
        out["detail"] = Value::String(detail.clone());
    }
    match (&m.text, &m.base64) {
        (Some(text), _) => {
            // Cut before hiding secrets (a message can be megabytes), leaving room for a secret
            // that straddles the cut, then cut again.
            let (start, long) = cut(text, MAX_EVENT_CHARS + 1024);
            let (text, cut_again) = cut(&r.text(&start), MAX_EVENT_CHARS);
            let truncated = long || cut_again;
            out["text"] = Value::String(text);
            if truncated {
                out["textTruncated"] = Value::Bool(true);
            }
        }
        (None, Some(b64)) => {
            // The same for binary data: hide secrets in the start (whole base64 groups), then cut.
            use base64::Engine as _;
            let engine = &base64::engine::general_purpose::STANDARD;
            let start = &b64[..b64.len().min((MAX_EVENT_CHARS + 1024) / 4 * 4)];
            let shown = match engine.decode(start).map(|bytes| r.bytes(&bytes).into_owned()) {
                Ok(bytes) => engine.encode(bytes),
                Err(_) => start.to_string(),
            };
            let (b64_shown, cut_again) = cut(&shown, MAX_EVENT_CHARS);
            let truncated = start.len() < b64.len() || cut_again;
            out["base64"] = Value::String(b64_shown);
            if truncated {
                out["base64Truncated"] = Value::Bool(true);
            }
        }
        _ => {}
    }
    out
}

fn step_for_agent(s: &StepResult, r: &Redactor) -> Value {
    let mut out = json!({ "step": s.name, "passed": s.passed });
    if s.skipped {
        out["skipped"] = Value::Bool(true);
    }
    if let Some(e) = &s.error {
        out["error"] = Value::String(r.text(e));
    }
    if let Some(ms) = s.ms {
        out["ms"] = json!(ms.round());
    }
    if let Some(m) = &s.message {
        out["message"] = message_for_agent(m, r);
    }
    out
}

impl Api {
    /// What to hide from the agent about a connection: what its log hides (the secrets in
    /// scope when it opened and at each send, the handshake's credentials) and the secrets in
    /// scope now.
    fn connection_redactor(&self, log: &MessageLog) -> Redactor {
        let mut redactor = log.redactor().unwrap_or_default();
        if let Ok(ws) = self.agent_ws() {
            redactor.absorb(&self.redactor(&ws));
        }
        redactor
    }

    /// The agent's own connection.
    fn agent_connection(&self, c: &Call<'_>) -> Result<(String, RequestKind, Arc<MessageLog>), Fail> {
        let id = c.args.get("connectionId").and_then(Value::as_str).unwrap_or_default().to_string();
        let sockets = lock(&self.inner.agents.sockets);
        match sockets.get(&id).filter(|s| s.session == c.session.info.id) {
            Some(socket) => Ok((id, socket.kind, socket.log.clone())),
            None => Err(Fail::Invalid(format!("No connection '{id}' of yours; open_connection opens one"))),
        }
    }

    pub(super) async fn tool_open_connection(&self, c: &Call<'_>) -> Outcome {
        let ws = self.agent_ws()?;
        let (request, path) = self.request_to_send(&ws, c)?;
        if !KINDS.contains(&request.kind) {
            return Err(Fail::Invalid(
                "open_connection opens websocket, socketio, tcp, udp and mqtt requests; send_request sends the others"
                    .into(),
            ));
        }
        let saved_path = path.clone().filter(|p| ws.read_request(p).is_ok());
        let hosts = self.request_hosts(&ws, &request, path.as_deref());
        let guard = self.gate_traffic(c, &format!("Connect to {}", request.url), hosts).await?;
        let seq = self.next_generation();
        let conn_id = format!("agent-conn-{seq}");
        let websocket = request.kind == RequestKind::Websocket;
        // Secrets are hidden as messages come in: what the agent reads and what its waits
        // match never holds them, even after the user changes environments.
        let log = MessageLog::new(true).redacting(self.redactor(&ws));
        let opening = async {
            if websocket {
                let (opened, connected) = Box::pin(self.ws_connect(&conn_id, &request, path.as_deref(), log)).await?;
                Ok((Opened::Stream(Box::new(opened)), connected))
            } else {
                let (opened, connected) =
                    Box::pin(self.socket_connect(&conn_id, &request, path.as_deref(), log)).await?;
                Ok((Opened::Socket(Box::new(opened.opened)), connected))
            }
        };
        let opened = super::until_cancelled(
            c,
            super::super::with_guard(guard, async { opening.await.map_err(|e| self.blocked_hint(c, e)) }),
        )
        .await;
        let (opened, connected): (Opened, Connected) = match opened {
            Ok(o) => o,
            Err(fail) => {
                // Cancelled while connecting: don't leave the attempt behind.
                if websocket {
                    self.ws_close(&conn_id, None, None);
                } else {
                    self.socket_close(&conn_id);
                }
                return Err(fail);
            }
        };
        let messages = match request.kind {
            RequestKind::Websocket => request.websocket.messages.clone(),
            RequestKind::SocketIo => request.socketio.messages.clone(),
            _ => Vec::new(),
        };
        // What send_message can send by name.
        let saved: Vec<Value> = messages
            .iter()
            .map(|m| match m.event.is_empty() {
                true => json!({ "name": m.name, "format": m.format }),
                false => json!({ "name": m.name, "event": m.event, "format": m.format }),
            })
            .collect();
        {
            let mut sockets = lock(&self.inner.agents.sockets);
            let session = &c.session.info.id;
            let mut closed: Vec<(u64, String)> = sockets
                .iter()
                .filter(|(_, s)| &s.session == session && s.log.ended().is_some())
                .map(|(id, s)| (s.seq, id.clone()))
                .collect();
            closed.sort();
            for (_, old) in closed.iter().rev().skip(KEEP_CLOSED - 1) {
                sockets.remove(old);
            }
            let socket = AgentSocket {
                session: session.clone(),
                seq,
                kind: request.kind,
                messages,
                topic: request.mqtt.topic.clone(),
                qos: request.mqtt.qos,
                retain: request.mqtt.retain,
                log: connected.log.clone(),
                cursor: 0,
            };
            sockets.insert(conn_id.clone(), socket);
        }
        // Closed when the agent goes away, and forgotten then.
        let (api, id, closed, kind) = (self.clone(), conn_id.clone(), c.session.closed.clone(), request.kind);
        tokio::spawn(async move {
            closed.cancelled().await;
            if kind == RequestKind::Websocket {
                api.ws_close(&id, Some(1001), Some("The AI agent disconnected".into()));
            } else {
                api.socket_close(&id);
            }
            lock(&api.inner.agents.sockets).remove(&id);
        });
        self.emit_agent(AgentEvent::Stream {
            path: saved_path.clone(),
            request: Box::new(request.clone()),
            conn_id: conn_id.clone(),
            opened: opened.clone(),
        });
        let opened = serde_json::to_value(&opened).unwrap_or_default();

        let redactor = self.connection_redactor(&connected.log);
        let mut steps = Vec::new();
        if let Some(handle) = connected.steps {
            let limit = c.args.get("waitSeconds").and_then(Value::as_u64).unwrap_or(STEPS_WAIT_SECS).clamp(1, 600);
            let waited = tokio::select! {
                w = tokio::time::timeout(Duration::from_secs(limit), handle) => Some(w),
                _ = c.cancel.cancelled() => None,
            };
            match waited {
                Some(Ok(Ok(results))) => steps = results.iter().map(|s| step_for_agent(s, &redactor)).collect(),
                Some(Ok(Err(_))) => {}
                Some(Err(_)) | None => steps.push(json!({ "step": "On-connect steps", "passed": false, "error": "Still running; they go on in the background (read_messages shows what happens)" })),
            }
        }
        // The first messages (a greeting, the steps' replies); read_messages has the rest.
        let (mut messages, _, last_id) = connected.log.since(0, crate::live::MAX_LOG_MESSAGES);
        let more = messages.len().saturating_sub(FIRST_MESSAGES);
        messages.truncate(FIRST_MESSAGES);
        if let Some(socket) = lock(&self.inner.agents.sockets).get_mut(&conn_id) {
            socket.cursor = last_id;
        }
        let mut value = json!({
            "connectionId": conn_id,
            "kind": request.kind,
            "messages": messages.iter().map(|m| message_for_agent(m, &redactor)).collect::<Vec<_>>(),
            "lastId": last_id,
        });
        if more > 0 {
            value["moreMessages"] = json!(more);
        }
        if !saved.is_empty() {
            value["savedMessages"] = Value::Array(saved);
        }
        if websocket {
            let header = |name: &str| {
                opened["meta"]["headers"]
                    .as_array()
                    .and_then(|hs| hs.iter().find(|h| h["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(name))))
                    .and_then(|h| h["value"].as_str())
                    .map(str::to_string)
            };
            value["status"] = opened["meta"]["status"].clone();
            value["url"] = Value::String(redactor.text(opened["meta"]["url"].as_str().unwrap_or_default()));
            value["timeMs"] = json!(opened["timing"]["totalMs"].as_f64().unwrap_or_default().round());
            if let Some(p) = header("sec-websocket-protocol") {
                value["protocol"] = Value::String(redactor.text(&p));
            }
            if let Some(e) = header("sec-websocket-extensions") {
                value["extensions"] = Value::String(redactor.text(&e));
            }
            if let Some(unresolved) = opened["unresolved"].as_array().filter(|u| !u.is_empty()) {
                let names = unresolved.iter().map(|u| Value::String(redactor.text(u.as_str().unwrap_or_default())));
                value["undefinedVariables"] = Value::Array(names.collect());
            }
        } else {
            value["protocol"] = opened["protocol"].clone();
            value["remoteAddr"] = opened["remoteAddr"].clone();
            value["timeMs"] = json!(opened["timing"]["totalMs"].as_f64().unwrap_or_default().round());
        }
        if !steps.is_empty() {
            value["steps"] = Value::Array(steps);
        }
        if let Some(why) = connected.log.ended() {
            value["closed"] = Value::String(redactor.text(&why));
        }
        let done = Done::new(value, format!("Connected · {}", request.url));
        Ok(match saved_path {
            Some(path) => done.at(AgentTarget::Request { path }),
            None => done,
        })
    }

    pub(super) async fn tool_send_message(&self, c: &Call<'_>) -> Outcome {
        #[derive(Deserialize)]
        struct A {
            text: Option<String>,
            json: Option<Value>,
            hex: Option<String>,
            message: Option<String>,
            #[serde(default)]
            ping: bool,
            #[serde(default)]
            event: String,
            #[serde(default)]
            ack: bool,
            topic: Option<String>,
            qos: Option<u8>,
            retain: Option<bool>,
        }
        let (id, kind, log) = self.agent_connection(c)?;
        let a: A = c.args()?;
        let given = [a.text.is_some(), a.json.is_some(), a.hex.is_some(), a.message.is_some(), a.ping];
        if given.iter().filter(|g| **g).count() != 1 {
            return Err(Fail::Invalid("Give exactly one of text, json, hex, message or ping".into()));
        }
        // What this send fills in is hidden from here on, whatever the user changes later.
        if let Ok(ws) = self.agent_ws() {
            log.redact_also(&self.redactor(&ws));
        }
        let render = |t: &str| self.render_text(t);
        let (mut message, what) = if let Some(text) = a.text {
            (SavedMessage { text, ..Default::default() }, "text")
        } else if let Some(json) = a.json {
            (SavedMessage { format: MessageFormat::Json, text: json.to_string(), ..Default::default() }, "JSON")
        } else if let Some(hex) = a.hex {
            (SavedMessage { format: MessageFormat::Binary, text: hex, ..Default::default() }, "binary")
        } else if let Some(name) = a.message {
            let saved = lock(&self.inner.agents.sockets)
                .get(&id)
                .and_then(|s| s.messages.iter().find(|m| m.name == name).cloned());
            let saved =
                saved.ok_or_else(|| Fail::Invalid(format!("The request has no saved message named '{name}'")))?;
            (saved, "saved message")
        } else {
            if kind != RequestKind::Websocket {
                return Err(Fail::Invalid("ping is for WebSocket connections".into()));
            }
            let since = log.last_id();
            self.ws_send(&id, None, WsOutgoing::Ping).map_err(|e| Fail::Error(e.message))?;
            self.set_cursor(&id, since);
            return Ok(Done::new(json!({ "sent": "ping", "sinceId": since }), "Sent a ping"));
        };
        if !a.event.trim().is_empty() {
            message.event = a.event;
        }
        message.ack |= a.ack;
        let since = log.last_id();
        let sent = match kind {
            RequestKind::Websocket => {
                let out = outgoing(&message, &render).map_err(Fail::Invalid)?;
                self.ws_send(&id, None, out)
            }
            _ => {
                let mut out = if kind == RequestKind::Mqtt && message.event.trim().is_empty() {
                    let topic =
                        a.topic.clone().or_else(|| lock(&self.inner.agents.sockets).get(&id).map(|s| s.topic.clone()));
                    message.event = topic.unwrap_or_default();
                    socket_outgoing(kind, &message, &render)
                } else {
                    socket_outgoing(kind, &message, &render)
                }
                .map_err(Fail::Invalid)?;
                if let SocketOutgoing::Publish { qos, retain, .. } = &mut out {
                    let defaults =
                        lock(&self.inner.agents.sockets).get(&id).map(|s| (s.qos, s.retain)).unwrap_or_default();
                    *qos = a.qos.unwrap_or(defaults.0);
                    *retain = a.retain.unwrap_or(defaults.1);
                }
                self.socket_send(&id, None, out)
            }
        };
        sent.map_err(|e| Fail::Error(e.message))?;
        self.set_cursor(&id, since);
        Ok(Done::new(json!({ "sent": what, "sinceId": since }), format!("Sent {what}")))
    }

    fn set_cursor(&self, id: &str, cursor: u64) {
        if let Some(socket) = lock(&self.inner.agents.sockets).get_mut(id) {
            socket.cursor = cursor;
        }
    }

    pub(super) async fn tool_wait_for_message(&self, c: &Call<'_>) -> Outcome {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct A {
            #[serde(default, rename = "match")]
            matcher: Option<MatchKind>,
            #[serde(default)]
            pattern: String,
            #[serde(default)]
            event: String,
            timeout_ms: Option<u64>,
            since_id: Option<u64>,
        }
        let (id, _, log) = self.agent_connection(c)?;
        let a: A = c.args()?;
        let wait = WaitStep {
            matcher: a.matcher.unwrap_or(MatchKind::Contains),
            pattern: a.pattern,
            event: a.event.trim().to_string(),
            ..Default::default()
        };
        let matcher = MessageMatcher::new(wait.matcher, wait.pattern.as_bytes())
            .map_err(|e| Fail::Invalid(format!("pattern: {e}")))?;
        let since = a.since_id.unwrap_or_else(|| lock(&self.inner.agents.sockets).get(&id).map_or(0, |s| s.cursor));
        let limit = Duration::from_millis(a.timeout_ms.unwrap_or(DEFAULT_WAIT_MS).clamp(1, MAX_WAIT_MS));
        let started = Instant::now();
        let event = Some(wait.event.as_str()).filter(|e| !e.is_empty());
        // Matched against what the agent may see, so a match can't tell it a secret.
        let redactor = self.connection_redactor(&log);
        let wanted = |m: &LoggedMessage| {
            event.is_none_or(|e| m.event.as_deref().is_some_and(|v| redactor.text(v) == e))
                && matcher.matches(&redactor.bytes(&m.bytes()))
        };
        let waited =
            super::until_cancelled(c, async { Ok::<_, Fail>(log.wait_until(since, started + limit, wanted).await) })
                .await?;
        let ms = started.elapsed().as_millis() as u64;
        let what = wait_target(&wait);
        match waited {
            Waited::Found(m) => {
                self.set_cursor(&id, m.id);
                Ok(Done::new(
                    json!({ "message": message_for_agent(&m, &redactor), "waitedMs": ms }),
                    format!("Got {what}"),
                ))
            }
            Waited::TimedOut => Ok(Done::new(
                json!({ "timedOut": true, "waitedMs": ms, "hint": "Nothing matched; read_messages shows what came in." }),
                format!("Nothing matched {what}"),
            )),
            Waited::Ended(why) => {
                let why = redactor.text(&why);
                Ok(Done::new(json!({ "closed": why, "waitedMs": ms }), why))
            }
            Waited::Overrun => Err(Fail::Error(format!("{OVERRUN}; read_messages shows what is kept"))),
        }
    }

    pub(super) fn tool_read_messages(&self, c: &Call<'_>) -> Outcome {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct A {
            since_id: Option<u64>,
            limit: Option<usize>,
        }
        let (id, _, log) = self.agent_connection(c)?;
        let a: A = c.args()?;
        let limit = a.limit.unwrap_or(DEFAULT_MESSAGES).clamp(1, MAX_MESSAGES);
        let (messages, left_out, last_id) = log.since(a.since_id.unwrap_or(0), limit);
        if let Some(socket) = lock(&self.inner.agents.sockets).get_mut(&id) {
            socket.cursor = socket.cursor.max(last_id);
        }
        let redactor = self.connection_redactor(&log);
        let mut value = json!({
            "messages": messages.iter().map(|m| message_for_agent(m, &redactor)).collect::<Vec<_>>(),
            "lastId": last_id,
        });
        if left_out > 0 {
            value["leftOut"] = json!(left_out);
        }
        if let Some(why) = log.ended() {
            value["closed"] = Value::String(redactor.text(&why));
        }
        Ok(Done::new(value, super::plural(messages.len(), "message")))
    }

    pub(super) fn tool_close_connection(&self, c: &Call<'_>) -> Outcome {
        #[derive(Deserialize)]
        struct A {
            code: Option<u16>,
            reason: Option<String>,
        }
        let (id, kind, _) = self.agent_connection(c)?;
        let a: A = c.args()?;
        if kind != RequestKind::Websocket {
            self.socket_close(&id);
            return Ok(Done::new(json!({ "closing": true }), "Closed"));
        }
        crate::websocket::check_close(a.code, a.reason.as_deref()).map_err(Fail::Invalid)?;
        self.ws_close(&id, a.code.or(Some(1000)), a.reason);
        Ok(Done::new(json!({ "closing": true }), "Closed"))
    }
}

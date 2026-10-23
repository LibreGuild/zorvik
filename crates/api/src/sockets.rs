//! Socket client sessions (TCP, UDP, MQTT, Socket.IO, GraphQL subscriptions): `socket.connect`,
//! `socket.send`, `socket.close`, `socket.repeat`. Events go to the UI as
//! [`StreamEvent::Socket`]. Socket.IO requests run their on-connect steps (see [`crate::live`]).

use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_util::sync::CancellationToken;
use ts_rs::TS;
use zorvik_engine::mqtt::{MqttConfig, MqttProtocol};
use zorvik_engine::{
    EngineError, HttpResponse, SocketConfig, SocketConnected, SocketEvent, SocketOpened, SocketOutgoing, SocketSession,
};
use zorvik_workspace::formats::{MessageFormat, MqttVersion, Request, RequestKind, SavedMessage, SocketIoOptions};
use zorvik_workspace::resolve::Resolved;

use crate::agents::redact::Redactor;
use crate::live::{self, Connected, MIN_REPEAT, MessageLog, StepResult, Steps, every_text, run_steps};
use crate::message_scripts::{self, MessageScripts, Scripted};
use crate::{Api, ApiError, ApiResult, StreamEvent, lock, ok, params, remove_if_current};

/// A socket session by caller id. Registered before connecting (`session` is
/// `None` until then) so `socket.close` can abort a pending connect.
pub(crate) struct SocketEntry {
    generation: u64,
    cancel: CancellationToken,
    pub(crate) session: Option<SocketSession>,
    /// Outgoing messages done with, for file streams.
    pub(crate) settled: Arc<crate::file_stream::Settled>,
    /// The request's kind: how saved messages are sent.
    kind: RequestKind,
    /// Cancelled when the connection ends: stops its steps and repeating sends.
    alive: CancellationToken,
    /// The composer's repeating send, while one runs.
    repeat: Option<CancellationToken>,
}

/// A saved message as what to send on a session of `kind`, its `{{variables}}` filled in.
/// Socket.IO: an emit of its event (arguments: JSON, one text, or one binary argument); MQTT:
/// a publish to its `event` (the topic); TCP and UDP: text or bytes.
pub(crate) fn socket_outgoing(
    kind: RequestKind,
    message: &SavedMessage,
    render: &dyn Fn(&str) -> String,
) -> Result<SocketOutgoing, String> {
    let binary = || crate::websocket::hex_base64(message);
    let name = || if message.name.is_empty() { "The message".to_string() } else { format!("'{}'", message.name) };
    match kind {
        RequestKind::SocketIo => {
            let event = render(&message.event).trim().to_string();
            if event.is_empty() {
                return Err(format!("{} has no event to emit", name()));
            }
            let (args, base64) = match message.format {
                MessageFormat::Binary => (String::new(), Some(binary()?)),
                MessageFormat::Json => (render(&message.text), None),
                MessageFormat::Text => (Value::String(render(&message.text)).to_string(), None),
            };
            Ok(SocketOutgoing::Emit { event, args, base64, ack: message.ack })
        }
        RequestKind::Mqtt => {
            let topic = render(&message.event).trim().to_string();
            if topic.is_empty() {
                return Err(format!("{} has no topic to publish to", name()));
            }
            let (text, base64) = match message.format {
                MessageFormat::Binary => (None, Some(binary()?)),
                _ => (Some(render(&message.text)), None),
            };
            Ok(SocketOutgoing::Publish { topic, text, base64, qos: 0, retain: false })
        }
        _ => match message.format {
            MessageFormat::Binary => Ok(SocketOutgoing::Binary { base64: binary()? }),
            _ => Ok(SocketOutgoing::Text { text: render(&message.text) }),
        },
    }
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SocketConnectResult {
    pub opened: SocketOpened,
    /// Variables that were referenced but not defined.
    pub unresolved: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConnectParams {
    conn_id: String,
    request: Request,
    path: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConnParam {
    conn_id: String,
}

impl Api {
    /// `socket.*` methods.
    pub(crate) async fn call_socket(&self, method: &str, p: Value) -> ApiResult<Value> {
        match method {
            "socket.connect" => {
                let ConnectParams { conn_id, request, path } = params(p)?;
                // The log is for Socket.IO's on-connect steps; the UI keeps its own.
                let keep = request.kind == RequestKind::SocketIo && !request.socketio.on_connect.is_empty();
                ok(Box::pin(self.socket_connect(&conn_id, &request, path.as_deref(), MessageLog::new(keep))).await?.0)
            }
            "socket.send" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct P {
                    conn_id: String,
                    message: SocketOutgoing,
                }
                let P { conn_id, message } = params(p)?;
                self.socket_send(&conn_id, None, message)?;
                ok(())
            }
            "socket.close" => {
                let ConnParam { conn_id } = params(p)?;
                self.socket_close(&conn_id);
                ok(())
            }
            // Keep sending a message (its variables filled in each time) until stopped.
            "socket.repeat" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct P {
                    conn_id: String,
                    message: SavedMessage,
                    every_ms: u64,
                }
                let P { conn_id, message, every_ms } = params(p)?;
                let every = Duration::from_millis(every_ms);
                if every < MIN_REPEAT {
                    return Err(ApiError::invalid(format!("Send at most every {} ms", MIN_REPEAT.as_millis())));
                }
                let (kind, generation, stop) = {
                    let mut sessions = lock(&self.inner.socket_sessions);
                    let entry = sessions
                        .get_mut(&conn_id)
                        .filter(|e| e.session.is_some())
                        .ok_or_else(|| ApiError::new("notFound", "Not connected"))?;
                    socket_outgoing(entry.kind, &message, &|t| t.to_string()).map_err(ApiError::invalid)?;
                    let token = entry.alive.child_token();
                    if let Some(old) = entry.repeat.replace(token.clone()) {
                        old.cancel();
                    }
                    (entry.kind, entry.generation, token)
                };
                self.socket_repeat(conn_id, kind, generation, message, every, stop);
                ok(())
            }
            "socket.stopRepeat" => {
                let ConnParam { conn_id } = params(p)?;
                if let Some(token) = lock(&self.inner.socket_sessions).get_mut(&conn_id).and_then(|e| e.repeat.take()) {
                    token.cancel();
                }
                ok(())
            }
            other => Err(ApiError::new("notFound", format!("Unknown method '{other}'"))),
        }
    }

    /// Connect `conn_id` (replacing a connection with that id), forward its events to the UI,
    /// and start a Socket.IO request's on-connect steps.
    pub(crate) async fn socket_connect(
        &self,
        conn_id: &str,
        request: &Request,
        path: Option<&str>,
        log: MessageLog,
    ) -> ApiResult<(SocketConnectResult, Connected)> {
        let ws = self.ws()?;
        let settings = self.settings();
        let cancel = CancellationToken::new();
        let generation = self.next_generation();
        let settled = Arc::new(crate::file_stream::Settled::default());
        let log = Arc::new(log);
        let alive = CancellationToken::new();
        let entry = SocketEntry {
            generation,
            cancel: cancel.clone(),
            session: None,
            settled: settled.clone(),
            kind: request.kind,
            alive: alive.clone(),
            repeat: None,
        };
        if let Some(old) = lock(&self.inner.socket_sessions).insert(conn_id.to_string(), entry) {
            old.cancel.cancel();
            old.alive.cancel();
            crate::file_stream::stop_file_stream(&self.inner, conn_id);
        }
        let (note_api, note_id) = (self.clone(), conn_id.to_string());
        let note = move |text: String, error: bool| {
            let event = if error { SocketEvent::Error { message: text } } else { SocketEvent::info(text) };
            note_api.inner.sink.emit(StreamEvent::Socket { conn_id: note_id.clone(), event });
        };
        let mut request = request.clone();
        let connected = tokio::select! {
            r = async {
                // Pre-request scripts first: they may change the request and set variables.
                let (vars, scripts) = self.live_scripts(&ws, &mut request, path, &note).await?;
                let mut resolved = self.prepare_with(&ws, &request, path, &vars.var_context())?;
                let opts = crate::request_options(&settings, &request.settings)?;
                self.authorize(&ws, &mut resolved, &opts).await?;
                // An agent's connection: credentials of the handshake stay out of what it reads.
                log.redact_also(&Redactor::default().with_headers(&resolved.request.headers));
                let jar = settings.cookie_jar.then(|| self.jar(&ws));
                let unresolved = resolved.unresolved.clone();
                let conn = self.connect_socket(&request, resolved, &opts, jar.as_ref()).await?;
                if let Some(jar) = &jar {
                    self.save_jar(&ws, jar);
                }
                Ok::<_, ApiError>((conn, unresolved, scripts))
            } => r,
            _ = cancel.cancelled() => Err(EngineError::cancelled().into()),
        };
        let (conn, unresolved, scripts) = match connected {
            Ok(c) => c,
            Err(e) => {
                remove_if_current(&self.inner.socket_sessions, conn_id, |e| e.generation == generation);
                return Err(e);
            }
        };
        let SocketConnected { opened, session, mut events, .. } = conn;
        match lock(&self.inner.socket_sessions).get_mut(conn_id) {
            Some(entry) if entry.generation == generation => entry.session = Some(session),
            // Closed or replaced while connecting: dropping the session closes it.
            _ => return Err(EngineError::cancelled().into()),
        }
        let feed = scripts.map(|scripts| {
            let (api, id) = (self.clone(), conn_id.to_string());
            let send = Box::new(move |s: &zorvik_script::ScriptSend| {
                api.socket_send(&id, Some(generation), script_emit(s)).map_err(|e| e.message)
            });
            self.start_message_scripts(&ws, scripts, send, Box::new(note), alive.clone())
        });
        let inner = self.inner.clone();
        let id = conn_id.to_string();
        let (forward_log, forward_alive) = (log.clone(), alive.clone());
        tokio::spawn(async move {
            while let Some(event) = events.recv().await {
                if let Some(feed) = &feed {
                    feed.socket(&event);
                }
                let closed = matches!(event, SocketEvent::Closed { .. });
                settled.socket(&event);
                forward_log.record_socket(&event);
                inner.sink.emit(StreamEvent::Socket { conn_id: id.clone(), event });
                if closed {
                    break;
                }
            }
            settled.end();
            forward_log.end();
            forward_alive.cancel();
            remove_if_current(&inner.socket_sessions, &id, |e| e.generation == generation);
        });
        let has_steps = request.kind == RequestKind::SocketIo && !request.socketio.on_connect.is_empty();
        let steps = has_steps.then(|| {
            let api = self.clone();
            let (id, options, log) = (conn_id.to_string(), request.socketio.clone(), log.clone());
            tokio::spawn(async move { api.run_socketio_steps(id, generation, options, log, alive).await })
        });
        Ok((SocketConnectResult { opened, unresolved }, Connected { log, steps }))
    }

    async fn run_socketio_steps(
        &self,
        conn_id: String,
        generation: u64,
        options: SocketIoOptions,
        log: Arc<MessageLog>,
        alive: CancellationToken,
    ) -> Vec<StepResult> {
        let render = self.renderer();
        let send = |m: &SavedMessage| {
            let out = socket_outgoing(RequestKind::SocketIo, m, &*render)?;
            self.socket_send(&conn_id, Some(generation), out).map_err(|e| e.message)
        };
        let note = |text: String, error: bool| {
            let event = if error { SocketEvent::Error { message: text } } else { SocketEvent::info(text) };
            self.inner.sink.emit(StreamEvent::Socket { conn_id: conn_id.clone(), event });
        };
        let repeat = |message: &SavedMessage, every: Duration| {
            note(format!("On connect: sending {} every {}", message.name, every_text(every)), false);
            let stop = alive.child_token();
            self.socket_repeat(conn_id.clone(), RequestKind::SocketIo, generation, message.clone(), every, stop);
        };
        let steps = Steps {
            steps: &options.on_connect,
            messages: &options.messages,
            log: &log,
            send: &send,
            note: &note,
            repeat: &repeat,
            cancel: &alive,
        };
        run_steps(steps).await
    }

    /// Send on `conn_id` (only if it is still connection `generation`, when given).
    pub(crate) fn socket_send(&self, conn_id: &str, generation: Option<u64>, message: SocketOutgoing) -> ApiResult<()> {
        let sessions = lock(&self.inner.socket_sessions);
        let session = sessions
            .get(conn_id)
            .filter(|e| generation.is_none_or(|g| e.generation == g))
            .and_then(|e| e.session.as_ref())
            .ok_or_else(|| ApiError::new("notFound", "Not connected"))?;
        session.send(message)?;
        Ok(())
    }

    /// The kind of an open session.
    pub(crate) fn socket_kind(&self, conn_id: &str) -> Option<RequestKind> {
        lock(&self.inner.socket_sessions).get(conn_id).filter(|e| e.session.is_some()).map(|e| e.kind)
    }

    pub(crate) fn socket_close(&self, conn_id: &str) {
        crate::file_stream::stop_file_stream(&self.inner, conn_id);
        // Dropping the session closes the connection (its Closed event follows);
        // a connect still in progress is aborted.
        if let Some(entry) = lock(&self.inner.socket_sessions).remove(conn_id) {
            entry.cancel.cancel();
            entry.alive.cancel();
        }
    }

    /// Send `message` every `every` (the first at once) until `stop` or the connection ends.
    fn socket_repeat(
        &self,
        conn_id: String,
        kind: RequestKind,
        generation: u64,
        message: SavedMessage,
        every: Duration,
        stop: CancellationToken,
    ) {
        let (api, failed_api, failed_id) = (self.clone(), self.clone(), conn_id.clone());
        let render = self.renderer();
        let send = move |m: &SavedMessage| {
            let out = socket_outgoing(kind, m, &*render)?;
            api.socket_send(&conn_id, Some(generation), out).map_err(|e| e.message)
        };
        let failed = move |message: String| {
            failed_api
                .inner
                .sink
                .emit(StreamEvent::Socket { conn_id: failed_id, event: SocketEvent::Error { message } });
        };
        live::spawn_repeat(message, every, stop, send, failed);
    }

    /// Open the connection for the request's kind (variables already resolved).
    pub(crate) async fn connect_socket(
        &self,
        request: &Request,
        resolved: Resolved,
        opts: &zorvik_engine::RequestOptions,
        jar: Option<&Arc<zorvik_engine::CookieJar>>,
    ) -> ApiResult<SocketConnected> {
        if let Some(subscription) = resolved.subscription {
            return Ok(
                subscribe(&self.inner.client, resolved.request, subscription, opts, jar.map(|j| j.as_ref())).await?
            );
        }
        if request.kind == RequestKind::SocketIo {
            use zorvik_engine::socketio::{SocketIoConfig, SocketIoTransport as Transport};
            use zorvik_workspace::formats::SocketIoTransport;
            let config = SocketIoConfig {
                path: request.socketio.path.clone(),
                auth: resolved.socketio_auth,
                auto_ack: resolved.socketio_auto_ack,
                transport: match request.socketio.transport {
                    SocketIoTransport::Auto => Transport::Auto,
                    SocketIoTransport::Websocket => Transport::Websocket,
                    SocketIoTransport::Polling => Transport::Polling,
                },
            };
            return Ok(self.inner.client.socketio(resolved.request, config, opts, jar.cloned()).await?);
        }
        let resolved = &resolved;
        let socket = &request.socket;
        let config = SocketConfig {
            framing: socket.framing,
            length_bytes: socket.length_bytes,
            line_ending: socket.line_ending,
            broadcast: socket.broadcast,
        };
        let url = &resolved.request.url;
        match request.kind {
            RequestKind::Tcp => Ok(self.inner.client.tcp(url, opts, config).await?),
            RequestKind::Udp => Ok(self.inner.client.udp(url, opts, config).await?),
            RequestKind::Mqtt => self.connect_mqtt(request, resolved, opts).await,
            _ => Err(ApiError::invalid("This request is not a TCP, UDP, MQTT or Socket.IO connection")),
        }
    }
}

/// Collection runs: connect a Socket.IO client, run its on-connect steps, disconnect. The
/// response is the handshake's answer with a transcript of the events as its JSON body.
pub(crate) async fn run_once_socketio(
    client: &Arc<zorvik_engine::Client>,
    resolved: &Resolved,
    options: &SocketIoOptions,
    opts: &zorvik_engine::RequestOptions,
    jar: Option<&Arc<zorvik_engine::CookieJar>>,
    render: &(dyn Fn(&str) -> String + Send + Sync),
    scripts: Option<&Arc<MessageScripts>>,
) -> ApiResult<(HttpResponse, Vec<StepResult>, Vec<String>, Scripted)> {
    use zorvik_engine::socketio::{SocketIoConfig, SocketIoTransport as Transport};
    use zorvik_workspace::formats::SocketIoTransport;
    let started = std::time::Instant::now();
    let config = SocketIoConfig {
        path: options.path.clone(),
        auth: resolved.socketio_auth.clone(),
        transport: match options.transport {
            SocketIoTransport::Auto => Transport::Auto,
            SocketIoTransport::Websocket => Transport::Websocket,
            SocketIoTransport::Polling => Transport::Polling,
        },
        auto_ack: resolved.socketio_auto_ack.clone(),
    };
    let conn = client.socketio(resolved.request.clone(), config, opts, jar.cloned()).await?;
    let SocketConnected { opened, meta, session, mut events } = conn;
    let session = Arc::new(session);
    let log = Arc::new(MessageLog::default());
    let stop = CancellationToken::new();
    let (feed, worker) = match scripts {
        Some(scripts) => {
            let out = session.clone();
            let sinks = message_scripts::Sinks {
                send: Box::new(move |s| out.send(script_emit(s)).map_err(|e| e.message)),
                note: Box::new(|_, _| {}),
                keep: None,
            };
            let (feed, worker) = message_scripts::spawn(scripts.clone(), sinks, stop.clone());
            (Some(feed), Some(worker))
        }
        None => (None, None),
    };
    let pump = {
        let log = log.clone();
        let feed = feed.clone();
        tokio::spawn(async move {
            while let Some(event) = events.recv().await {
                log.record_socket(&event);
                if let Some(feed) = &feed {
                    feed.socket(&event);
                }
                if matches!(event, SocketEvent::Closed { .. }) {
                    break;
                }
            }
            log.end();
        })
    };
    let send =
        |m: &SavedMessage| session.send(socket_outgoing(RequestKind::SocketIo, m, render)?).map_err(|e| e.message);
    let (results, notes) = live::run_steps_once(&options.on_connect, &options.messages, &log, &send).await;
    if let Some(feed) = &feed {
        feed.settled(crate::websocket::SCRIPTS_FINISH).await;
    }
    // The scripts are done (they hold the session too); dropping the session says goodbye. Give
    // the transcript a moment to take the last events.
    let scripted = message_scripts::finish(feed, worker, &stop, crate::websocket::SCRIPTS_FINISH).await;
    drop(session);
    let _ = tokio::time::timeout(Duration::from_secs(2), pump).await;
    let body = live::transcript(&log);
    // The handshake's HTTP answer (Socket.IO sessions always start with one).
    let meta = meta.ok_or_else(|| ApiError::new("network", "The Socket.IO handshake gave no answer"))?;
    let mut timing = opened.timing;
    timing.total_ms = started.elapsed().as_secs_f64() * 1000.0;
    let response = HttpResponse {
        meta,
        timing,
        body_wire_size: body.len() as u64,
        body,
        body_truncated: false,
        decode_warning: None,
    };
    Ok((response, results, notes, scripted))
}

/// A message script's emit (an event and its arguments as a JSON array).
pub(crate) fn script_emit(s: &zorvik_script::ScriptSend) -> SocketOutgoing {
    SocketOutgoing::Emit {
        event: s.event.clone().unwrap_or_else(|| "message".into()),
        args: s.text.clone(),
        base64: None,
        ack: false,
    }
}

/// Start a GraphQL subscription: `request` is the resolved HTTP request (its body the operation).
pub(crate) async fn subscribe(
    client: &zorvik_engine::Client,
    mut request: zorvik_engine::HttpRequest,
    subscription: zorvik_workspace::resolve::Subscription,
    opts: &zorvik_engine::RequestOptions,
    jar: Option<&zorvik_engine::CookieJar>,
) -> Result<SocketConnected, EngineError> {
    use zorvik_engine::GraphqlWsProtocol;
    use zorvik_workspace::formats::GraphqlTransport;
    request.url = subscription.url;
    let protocol = match subscription.transport {
        GraphqlTransport::Websocket => GraphqlWsProtocol::TransportWs,
        GraphqlTransport::WebsocketLegacy => GraphqlWsProtocol::Legacy,
        GraphqlTransport::Sse => {
            request.method = "POST".into();
            return client.graphql_sse(request, opts, jar).await;
        }
    };
    let operation = serde_json::from_slice::<Value>(&request.body)
        .map_err(|e| EngineError::invalid(format!("The GraphQL operation is not valid JSON: {e}")))?;
    request.body = Default::default();
    client.graphql_ws(request, protocol, operation, subscription.connection_params, opts, jar).await
}

// ---- MQTT -----------------------------------------------------------------------

impl Api {
    /// MQTT: the user name and password are the effective Basic auth (request, folder
    /// or workspace, variables rendered), which `resolve` already turned into an
    /// Authorization header. Client id and subscription topics may use variables too.
    async fn connect_mqtt(
        &self,
        request: &Request,
        resolved: &Resolved,
        opts: &zorvik_engine::RequestOptions,
    ) -> ApiResult<SocketConnected> {
        let options = &request.mqtt;
        let vars = self.try_ws().map(|ws| self.var_context(&ws)).unwrap_or_default();
        let mut missing = std::collections::BTreeSet::new();
        let mut render = |s: &str| vars.render(s.trim(), &mut missing);
        let (username, password) = basic_credentials(&resolved.request.headers).unwrap_or_default();
        let config = MqttConfig {
            client_id: render(&options.client_id),
            protocol: match options.version {
                MqttVersion::V311 => MqttProtocol::V311,
                MqttVersion::V5 => MqttProtocol::V5,
            },
            clean_session: options.clean_session,
            keep_alive_secs: options.keep_alive_secs,
            username,
            password,
            subscriptions: options
                .subscriptions
                .iter()
                .filter(|s| s.enabled && !s.topic.trim().is_empty())
                .map(|s| (render(&s.topic), s.qos.min(2)))
                .collect(),
        };
        Ok(self.inner.client.mqtt(&resolved.request.url, opts, &config).await?)
    }
}

/// User name and password of a `Basic` Authorization header.
fn basic_credentials(headers: &[zorvik_engine::Header]) -> Option<(String, String)> {
    use base64::Engine as _;
    let value = headers.iter().rev().find(|h| h.name.eq_ignore_ascii_case("authorization"))?.value.trim();
    let (scheme, token) = value.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("basic") {
        return None;
    }
    let decoded = base64::engine::general_purpose::STANDARD.decode(token.trim()).ok()?;
    let (user, password) = std::str::from_utf8(&decoded).ok()?.split_once(':')?;
    Some((user.to_string(), password.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use zorvik_engine::Header;

    #[test]
    fn basic_credentials_from_the_header() {
        let h = |v: &str| vec![Header::new("X-Other", "1"), Header::new("authorization", v)];
        // "user:p:ss" — the password may contain colons.
        assert_eq!(basic_credentials(&h("Basic dXNlcjpwOnNz")), Some(("user".into(), "p:ss".into())));
        assert_eq!(basic_credentials(&h("Basic Og==")), Some((String::new(), String::new())));
        assert_eq!(basic_credentials(&h("Bearer abc")), None);
        assert_eq!(basic_credentials(&h("Basic !!!")), None);
        assert_eq!(basic_credentials(&[]), None);
    }
}

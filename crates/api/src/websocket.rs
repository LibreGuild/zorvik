//! WebSocket client sessions (`ws.*`): connect, send, close, a repeating send, and the
//! request's on-connect steps. What sessions share (the message log, steps, repeats) is in
//! [`crate::live`]. Collection runs use [`run_once`]: connect, run the steps (each wait is a
//! test), close.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use serde::Deserialize;
use serde_json::Value;
use tokio_util::sync::CancellationToken;
use zorvik_engine::framing::parse_hex;
use zorvik_engine::{EngineError, HttpResponse, WsEvent, WsOutgoing, WsSession};
use zorvik_workspace::formats::{MessageFormat, Request, SavedMessage, WebSocketOptions};

use crate::agents::redact::Redactor;
use crate::file_stream::Settled;
use crate::live::{self, Connected, MIN_REPEAT, MessageLog, StepResult, Steps, every_text, run_steps};
use crate::message_scripts::{self, MessageScripts, Scripted};
use crate::{Api, ApiError, ApiResult, StreamEvent, StreamOpened, lock, ok, params, remove_if_current};

/// How long a collection run waits for message scripts to finish before it closes the connection.
pub(crate) const SCRIPTS_FINISH: Duration = Duration::from_secs(5);

/// A WebSocket by caller id. Registered before connecting (`session` is `None`
/// until then) so `ws.close` can abort a pending connect; the generation keeps
/// an old connection from removing a newer one with the same id.
pub(crate) struct WsEntry {
    pub(crate) generation: u64,
    cancel: CancellationToken,
    pub(crate) session: Option<WsSession>,
    /// Outgoing messages done with, for file streams.
    pub(crate) settled: Arc<Settled>,
    /// Cancelled when the connection ends: stops its steps and repeating sends.
    pub(crate) alive: CancellationToken,
    /// The composer's repeating send, while one runs.
    repeat: Option<CancellationToken>,
}

/// Whether a client may close with `code` and `reason`: 1000 or 3000–4999, a reason of at
/// most 123 bytes (RFC 6455; servers refuse other close frames).
pub(crate) fn check_close(code: Option<u16>, reason: Option<&str>) -> Result<(), String> {
    if let Some(code) = code
        && !(code == 1000 || (3000..=4999).contains(&code))
    {
        return Err("Close with 1000, or a code from 3000 to 4999".into());
    }
    if reason.is_some_and(|r| r.len() > 123) {
        return Err("The close reason can be at most 123 bytes".into());
    }
    Ok(())
}

/// A saved message as what to send, with its `{{variables}}` filled in (binary: hex).
pub(crate) fn outgoing(message: &SavedMessage, render: &dyn Fn(&str) -> String) -> Result<WsOutgoing, String> {
    match message.format {
        MessageFormat::Binary => Ok(WsOutgoing::Binary { base64: hex_base64(message)? }),
        MessageFormat::Text | MessageFormat::Json => Ok(WsOutgoing::Text { text: render(&message.text) }),
    }
}

/// A binary message's hex text as base64.
pub(crate) fn hex_base64(message: &SavedMessage) -> Result<String, String> {
    let name =
        if message.name.is_empty() { "The message".to_string() } else { format!("The message '{}'", message.name) };
    let bytes = parse_hex(&message.text).map_err(|e| format!("{name} is not valid hex: {e}"))?;
    Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConnectParams {
    conn_id: String,
    request: Request,
    path: Option<String>,
}

impl Api {
    /// `ws.*` methods.
    pub(crate) async fn call_ws(&self, method: &str, p: Value) -> ApiResult<Value> {
        match method {
            "ws.connect" => {
                let ConnectParams { conn_id, request, path } = params(p)?;
                // The log is for the on-connect steps; the UI keeps its own.
                let keep = !request.websocket.on_connect.is_empty();
                ok(Box::pin(self.ws_connect(&conn_id, &request, path.as_deref(), MessageLog::new(keep))).await?.0)
            }
            "ws.send" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct P {
                    conn_id: String,
                    message: WsOutgoing,
                }
                let P { conn_id, message } = params(p)?;
                self.ws_send(&conn_id, None, message)?;
                ok(())
            }
            "ws.close" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct P {
                    conn_id: String,
                    code: Option<u16>,
                    reason: Option<String>,
                }
                let P { conn_id, code, reason } = params(p)?;
                check_close(code, reason.as_deref()).map_err(ApiError::invalid)?;
                self.ws_close(&conn_id, code, reason);
                ok(())
            }
            // Keep sending a message (its variables filled in each time) until stopped.
            "ws.repeat" => {
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
                outgoing(&message, &|t| t.to_string()).map_err(ApiError::invalid)?;
                let (generation, stop) = {
                    let mut sessions = lock(&self.inner.ws_sessions);
                    let entry = sessions
                        .get_mut(&conn_id)
                        .filter(|e| e.session.is_some())
                        .ok_or_else(|| ApiError::new("notFound", "WebSocket is not connected"))?;
                    let token = entry.alive.child_token();
                    if let Some(old) = entry.repeat.replace(token.clone()) {
                        old.cancel();
                    }
                    (entry.generation, token)
                };
                self.ws_repeat(conn_id, generation, message, every, stop);
                ok(())
            }
            "ws.stopRepeat" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct P {
                    conn_id: String,
                }
                let P { conn_id } = params(p)?;
                if let Some(token) = lock(&self.inner.ws_sessions).get_mut(&conn_id).and_then(|e| e.repeat.take()) {
                    token.cancel();
                }
                ok(())
            }
            other => Err(ApiError::new("notFound", format!("Unknown method '{other}'"))),
        }
    }

    /// Connect `conn_id` (replacing a connection with that id), forward its events to the UI,
    /// and start its on-connect steps.
    pub(crate) async fn ws_connect(
        &self,
        conn_id: &str,
        request: &Request,
        path: Option<&str>,
        log: MessageLog,
    ) -> ApiResult<(StreamOpened, Connected)> {
        let ws = self.ws()?;
        let settings = self.settings();
        let cancel = CancellationToken::new();
        let generation = self.next_generation();
        let settled = Arc::new(Settled::default());
        let log = Arc::new(log);
        let alive = CancellationToken::new();
        let entry = WsEntry {
            generation,
            cancel: cancel.clone(),
            session: None,
            settled: settled.clone(),
            alive: alive.clone(),
            repeat: None,
        };
        // Replacing an entry drops its session, which closes that connection.
        if let Some(old) = lock(&self.inner.ws_sessions).insert(conn_id.to_string(), entry) {
            old.cancel.cancel();
            old.alive.cancel();
            crate::file_stream::stop_file_stream(&self.inner, conn_id);
        }
        let (note_api, note_id) = (self.clone(), conn_id.to_string());
        let note = move |text: String, error: bool| {
            let event = if error { WsEvent::Error { message: text } } else { WsEvent::info(text) };
            note_api.inner.sink.emit(StreamEvent::Ws { conn_id: note_id.clone(), event });
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
                let conn = self.inner.client.websocket(resolved.request, &opts, jar.as_deref()).await?;
                if let Some(jar) = &jar {
                    self.save_jar(&ws, jar);
                }
                Ok::<_, ApiError>((conn, resolved.unresolved, scripts))
            } => r,
            _ = cancel.cancelled() => Err(EngineError::cancelled().into()),
        };
        let (conn, unresolved, scripts) = match connected {
            Ok(c) => c,
            Err(e) => {
                remove_if_current(&self.inner.ws_sessions, conn_id, |entry| entry.generation == generation);
                return Err(e);
            }
        };
        let zorvik_engine::WsConnected { meta, timing, session, mut events } = conn;
        match lock(&self.inner.ws_sessions).get_mut(conn_id) {
            Some(entry) if entry.generation == generation => entry.session = Some(session),
            // Closed or replaced while connecting: dropping the session closes the socket.
            _ => return Err(EngineError::cancelled().into()),
        }
        let feed = scripts.map(|scripts| {
            let (api, id) = (self.clone(), conn_id.to_string());
            let send = Box::new(move |s: &zorvik_script::ScriptSend| {
                api.ws_send(&id, Some(generation), WsOutgoing::Text { text: s.text.clone() }).map_err(|e| e.message)
            });
            self.start_message_scripts(&ws, scripts, send, Box::new(note), alive.clone())
        });
        let inner = self.inner.clone();
        let id = conn_id.to_string();
        let (forward_log, forward_alive) = (log.clone(), alive.clone());
        tokio::spawn(async move {
            while let Some(event) = events.recv().await {
                if let Some(feed) = &feed {
                    feed.ws(&event);
                }
                let closed = matches!(event, WsEvent::Closed { .. });
                settled.ws(&event);
                forward_log.record_ws(&event);
                inner.sink.emit(StreamEvent::Ws { conn_id: id.clone(), event });
                if closed {
                    break;
                }
            }
            settled.end();
            forward_log.end();
            forward_alive.cancel();
            remove_if_current(&inner.ws_sessions, &id, |entry| entry.generation == generation);
        });
        let steps = (!request.websocket.on_connect.is_empty()).then(|| {
            let api = self.clone();
            let (id, options, log) = (conn_id.to_string(), request.websocket.clone(), log.clone());
            tokio::spawn(async move { api.run_connect_steps(id, generation, options, log, alive).await })
        });
        Ok((StreamOpened { meta, timing, unresolved }, Connected { log, steps }))
    }

    async fn run_connect_steps(
        &self,
        conn_id: String,
        generation: u64,
        options: WebSocketOptions,
        log: Arc<MessageLog>,
        alive: CancellationToken,
    ) -> Vec<StepResult> {
        let render = self.renderer();
        let send = |m: &SavedMessage| {
            let out = outgoing(m, &*render)?;
            self.ws_send(&conn_id, Some(generation), out).map_err(|e| e.message)
        };
        let note = |text: String, error: bool| {
            let event = if error { WsEvent::Error { message: text } } else { WsEvent::info(text) };
            self.inner.sink.emit(StreamEvent::Ws { conn_id: conn_id.clone(), event });
        };
        let repeat = |message: &SavedMessage, every: Duration| {
            note(format!("On connect: sending {} every {}", message.name, every_text(every)), false);
            self.ws_repeat(conn_id.clone(), generation, message.clone(), every, alive.child_token());
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

    /// `text` with the workspace's variables filled in (as it is without a workspace).
    pub(crate) fn render_text(&self, text: &str) -> String {
        (self.renderer())(text)
    }

    /// Fills in the workspace's variables as they are now (dynamic ones, such as `{{$uuid}}`,
    /// change on every call). For repeating sends and steps: the environment is read once.
    pub(crate) fn renderer(&self) -> Arc<dyn Fn(&str) -> String + Send + Sync> {
        match self.ws() {
            Ok(ws) => {
                let ctx = self.var_context(&ws);
                Arc::new(move |text: &str| ctx.render(text, &mut BTreeSet::new()))
            }
            Err(_) => Arc::new(|text: &str| text.to_string()),
        }
    }

    /// Send on `conn_id` (only if it is still connection `generation`, when given).
    pub(crate) fn ws_send(&self, conn_id: &str, generation: Option<u64>, message: WsOutgoing) -> ApiResult<()> {
        let sessions = lock(&self.inner.ws_sessions);
        let session = sessions
            .get(conn_id)
            .filter(|e| generation.is_none_or(|g| e.generation == g))
            .and_then(|e| e.session.as_ref())
            .ok_or_else(|| ApiError::new("notFound", "WebSocket is not connected"))?;
        session.send(message)?;
        Ok(())
    }

    pub(crate) fn ws_close(&self, conn_id: &str, code: Option<u16>, reason: Option<String>) {
        crate::file_stream::stop_file_stream(&self.inner, conn_id);
        let mut sessions = lock(&self.inner.ws_sessions);
        if let Some(entry) = sessions.get(conn_id) {
            // Steps and repeating sends stop first: nothing may follow the close frame.
            entry.alive.cancel();
            match &entry.session {
                Some(session) => {
                    let _ = session.send(WsOutgoing::Close { code, reason });
                }
                // Still connecting: abort it.
                None => {
                    entry.cancel.cancel();
                    sessions.remove(conn_id);
                }
            }
        }
    }

    /// Send `message` every `every` (the first at once) until `stop` or the connection ends.
    fn ws_repeat(
        &self,
        conn_id: String,
        generation: u64,
        message: SavedMessage,
        every: Duration,
        stop: CancellationToken,
    ) {
        let (api, failed_api, failed_id) = (self.clone(), self.clone(), conn_id.clone());
        let render = self.renderer();
        let send = move |m: &SavedMessage| {
            let out = outgoing(m, &*render)?;
            api.ws_send(&conn_id, Some(generation), out).map_err(|e| e.message)
        };
        let failed = move |message: String| {
            failed_api.inner.sink.emit(StreamEvent::Ws { conn_id: failed_id, event: WsEvent::Error { message } });
        };
        live::spawn_repeat(message, every, stop, send, failed);
    }
}

// ---- collection runs ------------------------------------------------------------------------

/// Collection runs: connect, run the steps (repeats don't run: the connection closes when the
/// steps end), close. The response is the handshake with a transcript of the messages as its
/// JSON body; the steps' results are the tests.
pub(crate) async fn run_once(
    client: &zorvik_engine::Client,
    request: zorvik_engine::HttpRequest,
    options: &WebSocketOptions,
    opts: &zorvik_engine::RequestOptions,
    jar: Option<&zorvik_engine::CookieJar>,
    render: &(dyn Fn(&str) -> String + Send + Sync),
    scripts: Option<&Arc<MessageScripts>>,
) -> ApiResult<(HttpResponse, Vec<StepResult>, Vec<String>, Scripted)> {
    let started = std::time::Instant::now();
    let conn = client.websocket(request, opts, jar).await?;
    let zorvik_engine::WsConnected { meta, mut timing, session, mut events } = conn;
    let session = Arc::new(session);
    let log = Arc::new(MessageLog::default());
    let stop = CancellationToken::new();
    let (feed, worker) = match scripts {
        Some(scripts) => {
            let out = session.clone();
            let sinks = message_scripts::Sinks {
                send: Box::new(move |s| out.send(WsOutgoing::Text { text: s.text.clone() }).map_err(|e| e.message)),
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
                log.record_ws(&event);
                if let Some(feed) = &feed {
                    feed.ws(&event);
                }
                if matches!(event, WsEvent::Closed { .. }) {
                    break;
                }
            }
            log.end();
        })
    };
    let send = |m: &SavedMessage| session.send(outgoing(m, render)?).map_err(|e| e.message);
    let (results, notes) = live::run_steps_once(&options.on_connect, &options.messages, &log, &send).await;
    if let Some(feed) = &feed {
        feed.settled(SCRIPTS_FINISH).await;
    }
    // Close politely and give the server a moment to answer.
    let _ = session.send(WsOutgoing::Close { code: Some(1000), reason: None });
    let _ = tokio::time::timeout(Duration::from_secs(2), pump).await;
    let scripted = message_scripts::finish(feed, worker, &stop, SCRIPTS_FINISH).await;
    drop(session);
    let body = live::transcript(&log);
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

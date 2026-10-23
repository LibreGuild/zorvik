//! gRPC requests (`grpc.*`): `describe` lists the services from the request's
//! `.proto` files or from server reflection, `invoke` makes a unary call
//! (cancel with `http.cancel {requestId}`, recorded in history), and
//! `start` / `send` / `end` / `cancel` drive streaming calls whose events go
//! to the UI as [`StreamEvent::Grpc`].
//!
//! The request resolves like HTTP: variables in the URL and the message,
//! metadata = headers (with folder/workspace headers), auth as `authorization`
//! metadata (OAuth 2.0 tokens included). Descriptors are cached for the
//! session: per URL for reflection (until `refresh`), per files for `.proto`
//! files (until one of them changes on disk).

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_util::sync::CancellationToken;
use ts_rs::TS;
use zorvik_engine::grpc::{
    GrpcDescriptors, GrpcEvent, GrpcMethod, GrpcOpened, GrpcResponse, GrpcService, GrpcSession, GrpcStatus, GrpcStream,
    GrpcTarget, MethodDescriptor, message_from_json,
};
use zorvik_engine::{EngineError, ErrorKind, HttpResponse, RequestOptions};
use zorvik_workspace::Workspace;
use zorvik_workspace::formats::{ConnectStep, GrpcOptions, MessageFormat, Request, RequestKind, SavedMessage};
use zorvik_workspace::history::NewEntry;

use crate::live::{self, MessageLog, StepResult, Steps, run_steps};
use crate::{Api, ApiError, ApiResult, StreamEvent, join_err, lock, ok, params, remove_if_current};

/// Services of a gRPC request, for the method picker.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GrpcDescribeResult {
    pub services: Vec<GrpcService>,
    /// `reflection v1`, `reflection v1alpha` or `proto files`.
    pub source: String,
}

/// A unary call's answer plus the variables that were referenced but not defined.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GrpcInvokeResult {
    #[serde(flatten)]
    #[ts(flatten)]
    pub response: GrpcResponse,
    pub unresolved: Vec<String>,
    /// A streaming call's steps (waits are tests).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, as = "Option<Vec<zorvik_script::TestResult>>")]
    pub steps: Vec<zorvik_script::TestResult>,
    /// What the steps did, and why reading stopped.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, as = "Option<Vec<String>>")]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GrpcStartResult {
    pub opened: GrpcOpened,
    /// The method called (streaming flags tell the UI whether to offer Send / End).
    pub method: GrpcMethod,
    pub unresolved: Vec<String>,
}

#[derive(Default)]
pub(crate) struct GrpcState {
    /// Descriptors by source: `reflection` + URL, or `files` + paths.
    cache: Mutex<HashMap<String, Cached>>,
    sessions: Mutex<HashMap<String, SessionEntry>>,
}

struct Cached {
    descriptors: GrpcDescriptors,
    /// Every file compiled, with its modification time and size when it was read.
    stamps: Vec<(PathBuf, Option<Stamp>)>,
}

impl Cached {
    fn fresh(&self) -> bool {
        self.stamps.iter().all(|(path, s)| stamp(path) == *s)
    }
}

/// Modification time and size: the size catches edits within the file
/// system's time resolution (whole seconds on some shares).
type Stamp = (SystemTime, u64);

fn stamp(path: &Path) -> Option<Stamp> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

/// Descriptors kept at most; beyond that the cache starts over.
const MAX_CACHED: usize = 64;

fn remember(cache: &Mutex<HashMap<String, Cached>>, key: String, cached: Cached) {
    let mut cache = lock(cache);
    if cache.len() >= MAX_CACHED {
        cache.clear();
    }
    cache.insert(key, cached);
}

/// A streaming call by session id. Registered before connecting (`session` is
/// `None` until then) so `grpc.cancel` can abort a pending start.
struct SessionEntry {
    generation: u64,
    cancel: CancellationToken,
    session: Option<GrpcSession>,
    /// Cancelled when the call ends: stops its steps and repeating sends.
    alive: CancellationToken,
    /// Messages done with, for file streams.
    settled: Arc<crate::file_stream::Settled>,
}

/// The settle counter of an open streaming call (file streams send on it).
pub(crate) fn settled_of(inner: &crate::Inner, session_id: &str) -> Option<Arc<crate::file_stream::Settled>> {
    lock(&inner.grpc.sessions).get(session_id).filter(|e| e.session.is_some()).map(|e| e.settled.clone())
}

/// Send a JSON message on an open streaming call (a file stream's chunk).
pub(crate) fn send_json(inner: &crate::Inner, session_id: &str, json: &str) -> Result<(), String> {
    let sessions = lock(&inner.grpc.sessions);
    let session = sessions.get(session_id).and_then(|e| e.session.as_ref()).ok_or("The call is not open")?;
    session.send(json).map_err(|e| e.message)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DescribeParams {
    request: Request,
    path: Option<String>,
    /// Ask the server (or read the files) again instead of using the cache.
    #[serde(default)]
    refresh: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InvokeParams {
    /// Caller-chosen id used to cancel the call (`http.cancel`).
    request_id: String,
    request: Request,
    path: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StartParams {
    session_id: String,
    /// Id for `http.cancel` (default: the session id), e.g. the UI tab's.
    #[serde(default)]
    request_id: Option<String>,
    request: Request,
    path: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionParam {
    session_id: String,
}

impl Api {
    /// `grpc.*` methods.
    pub(crate) async fn call_grpc(&self, method: &str, p: Value) -> ApiResult<Value> {
        match method {
            "grpc.describe" => ok(self.grpc_describe(params(p)?).await?),
            "grpc.invoke" => ok(self.grpc_invoke(params(p)?).await?),
            "grpc.start" => ok(self.grpc_start(params(p)?).await?),
            "grpc.send" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct P {
                    session_id: String,
                    /// JSON message; `{{variables}}` are rendered.
                    message: String,
                }
                let P { session_id, message } = params(p)?;
                // Microphone chunks come many times a second and hold no variables.
                let message = match message.contains("{{") {
                    true => self
                        .try_ws()
                        .map(|ws| self.var_context(&ws))
                        .unwrap_or_default()
                        .render(&message, &mut BTreeSet::new()),
                    false => message,
                };
                let sessions = lock(&self.inner.grpc.sessions);
                let session = sessions
                    .get(&session_id)
                    .and_then(|e| e.session.as_ref())
                    .ok_or_else(|| ApiError::new("notFound", "The call is not open"))?;
                session.send(&message)?;
                ok(())
            }
            "grpc.end" => {
                let SessionParam { session_id } = params(p)?;
                let sessions = lock(&self.inner.grpc.sessions);
                let session = sessions
                    .get(&session_id)
                    .and_then(|e| e.session.as_ref())
                    .ok_or_else(|| ApiError::new("notFound", "The call is not open"))?;
                session.end();
                ok(())
            }
            "grpc.cancel" => {
                let SessionParam { session_id } = params(p)?;
                // A start in progress is aborted; an open call ends with CANCELLED (its `end` event follows).
                if let Some(entry) = lock(&self.inner.grpc.sessions).get(&session_id) {
                    entry.cancel.cancel();
                    if let Some(session) = &entry.session {
                        session.cancel();
                    }
                }
                ok(())
            }
            other => Err(ApiError::new("notFound", format!("Unknown method '{other}'"))),
        }
    }

    async fn grpc_describe(&self, p: DescribeParams) -> ApiResult<GrpcDescribeResult> {
        let ws = self.ws()?;
        let opts = crate::request_options(&self.settings(), &p.request.settings)?;
        let target = if uses_proto_files(&p.request) {
            None
        } else {
            Some(self.grpc_target(&ws, &p.request, p.path.as_deref(), &opts).await?.0)
        };
        let descriptors = self.grpc_descriptors(&ws, &p.request, target.as_ref(), &opts, p.refresh).await?;
        Ok(GrpcDescribeResult { services: descriptors.describe(), source: descriptors.source.clone() })
    }

    /// URL and metadata of the request: variables, inherited headers and auth
    /// resolved like an HTTP request (OAuth 2.0 tokens fetched when needed).
    async fn grpc_target(
        &self,
        ws: &Workspace,
        request: &Request,
        path: Option<&str>,
        opts: &RequestOptions,
    ) -> ApiResult<(GrpcTarget, Vec<String>)> {
        if request.kind != RequestKind::Grpc {
            return Err(ApiError::invalid("This request is not a gRPC request"));
        }
        let mut resolved = self.prepare(ws, request, path)?;
        self.authorize(ws, &mut resolved, opts).await?;
        let target = GrpcTarget { url: resolved.request.url, metadata: resolved.request.headers };
        Ok((target, resolved.unresolved))
    }

    /// Service definitions for the request: its `.proto` files, or server reflection.
    async fn grpc_descriptors(
        &self,
        ws: &Workspace,
        request: &Request,
        target: Option<&GrpcTarget>,
        opts: &RequestOptions,
        refresh: bool,
    ) -> ApiResult<GrpcDescriptors> {
        if uses_proto_files(request) {
            let outside = self.settings().files_outside_workspace;
            let (files, imports) = proto_paths(ws.root(), request, outside)?;
            // The setting is part of the key: imports were checked against it.
            let key = format!("files\n{outside}\n{files:?}\n{imports:?}");
            if !refresh && let Some(cached) = lock(&self.inner.grpc.cache).get(&key).filter(|c| c.fresh()) {
                return Ok(cached.descriptors.clone());
            }
            let descriptors = compile_protos(ws.root(), outside, files, imports).await?;
            let stamps = descriptors.files.iter().map(|f| (f.clone(), stamp(f))).collect();
            remember(&self.inner.grpc.cache, key, Cached { descriptors: descriptors.clone(), stamps });
            return Ok(descriptors);
        }
        let target = target.ok_or_else(|| ApiError::invalid("Enter the server URL to load its services"))?;
        let key = format!("reflection\n{}", target.url.trim());
        if !refresh && let Some(cached) = lock(&self.inner.grpc.cache).get(&key) {
            return Ok(cached.descriptors.clone());
        }
        let descriptors = self.inner.client.grpc_reflect(target, opts).await?;
        remember(&self.inner.grpc.cache, key, Cached { descriptors: descriptors.clone(), stamps: Vec::new() });
        Ok(descriptors)
    }

    /// The request's method. A method missing from cached descriptors reloads them once
    /// (the server or the files may have changed since).
    async fn grpc_method(
        &self,
        ws: &Workspace,
        request: &Request,
        target: &GrpcTarget,
        opts: &RequestOptions,
    ) -> ApiResult<MethodDescriptor> {
        let descriptors = self.grpc_descriptors(ws, request, Some(target), opts, false).await?;
        match descriptors.method(&request.method) {
            Ok(method) => Ok(method),
            Err(_) if !request.method.trim().is_empty() => {
                let fresh = self.grpc_descriptors(ws, request, Some(target), opts, true).await?;
                Ok(fresh.method(&request.method)?)
            }
            Err(e) => Err(e.into()),
        }
    }

    /// The JSON message with `{{variables}}` rendered.
    fn grpc_message(&self, ws: &Workspace, text: &str, unresolved: &mut Vec<String>) -> String {
        let mut missing = BTreeSet::new();
        let rendered = self.var_context(ws).render(text, &mut missing);
        for name in missing {
            if !unresolved.contains(&name) {
                unresolved.push(name);
            }
        }
        rendered
    }

    async fn grpc_invoke(&self, p: InvokeParams) -> ApiResult<GrpcInvokeResult> {
        let ws = self.ws()?;
        let settings = self.settings();
        let started = Instant::now();
        let cancel = CancellationToken::new();
        let generation = self.next_generation();
        if let Some((_, previous)) =
            lock(&self.inner.inflight).insert(p.request_id.clone(), (generation, cancel.clone()))
        {
            previous.cancel();
        }
        let outcome = tokio::select! {
            r = async {
                let opts = crate::request_options(&settings, &p.request.settings)?;
                let (target, mut unresolved) = self.grpc_target(&ws, &p.request, p.path.as_deref(), &opts).await?;
                let method = self.grpc_method(&ws, &p.request, &target, &opts).await?;
                // Variables the message uses that aren't defined.
                self.grpc_message(&ws, &p.request.body.text, &mut unresolved);
                let ctx = self.var_context(&ws);
                let render = move |t: &str| ctx.render(t, &mut BTreeSet::new());
                let call = Box::pin(run_call(&self.inner.client, &target, &method, &p.request, &opts, &render)).await?;
                Ok::<_, ApiError>((target.url, call, unresolved))
            } => r,
            _ = cancel.cancelled() => Err(EngineError::cancelled().into()),
        };
        remove_if_current(&self.inner.inflight, &p.request_id, |(g, _)| *g == generation);

        let workspace = ws.root().to_string_lossy().into_owned();
        match &outcome {
            Ok((url, CallRun { response, .. }, _)) => {
                let (status, error) = if response.status.is_ok() {
                    (Some(200), None)
                } else {
                    (None, Some(format!("{}: {}", response.status.name, response.status.message)))
                };
                let size = response.messages.iter().map(|m| m.size as i64).sum();
                let url = self.history_url(&ws, url);
                let entry =
                    HistoryEntry { url: &url, status, error: error.as_deref(), duration_ms: response.timing.total_ms };
                self.grpc_history(&workspace, &p, entry, Some(size), settings.history_limit);
            }
            Err(err) if err.network_kind != Some(ErrorKind::Cancelled) => {
                let duration_ms = started.elapsed().as_secs_f64() * 1000.0;
                let entry = HistoryEntry { url: &p.request.url, status: None, error: Some(&err.message), duration_ms };
                self.grpc_history(&workspace, &p, entry, None, settings.history_limit);
            }
            Err(_) => {}
        }
        outcome.map(|(_, call, unresolved)| GrpcInvokeResult {
            response: call.response,
            unresolved,
            steps: call.steps.iter().map(StepResult::test).collect(),
            notes: call.notes,
        })
    }

    /// History of a unary call: HTTP 200 when it succeeded (status OK); any
    /// other gRPC status is recorded as an error (`NOT_FOUND: …`).
    fn grpc_history(&self, workspace: &str, p: &InvokeParams, e: HistoryEntry<'_>, size: Option<i64>, limit: u32) {
        let Some(history) = &self.inner.history else { return };
        let entry = NewEntry {
            workspace,
            request_path: p.path.as_deref(),
            url: e.url,
            status: e.status,
            error: e.error,
            duration_ms: Some(e.duration_ms),
            size,
            request: &p.request,
        };
        if let Err(e) = history.add(entry, limit) {
            tracing::warn!("history write failed: {}", e.message);
        }
    }

    async fn grpc_start(&self, p: StartParams) -> ApiResult<GrpcStartResult> {
        let ws = self.ws()?;
        let settings = self.settings();
        let cancel = CancellationToken::new();
        let generation = self.next_generation();
        let sessions = &self.inner.grpc.sessions;
        // Replacing an entry drops its session, which cancels that call.
        let alive = CancellationToken::new();
        let settled = Arc::new(crate::file_stream::Settled::default());
        let entry = SessionEntry {
            generation,
            cancel: cancel.clone(),
            session: None,
            alive: alive.clone(),
            settled: settled.clone(),
        };
        if let Some(old) = lock(sessions).insert(p.session_id.clone(), entry) {
            old.cancel.cancel();
            old.alive.cancel();
        }
        // `http.cancel {requestId}` stops the call too (the UI's Cancel button).
        let request_id = p.request_id.clone().unwrap_or_else(|| p.session_id.clone());
        if let Some((_, old)) = lock(&self.inner.inflight).insert(request_id.clone(), (generation, cancel.clone())) {
            old.cancel();
        }
        let started = tokio::select! {
            r = async {
                let opts = crate::request_options(&settings, &p.request.settings)?;
                let (target, mut unresolved) = self.grpc_target(&ws, &p.request, p.path.as_deref(), &opts).await?;
                let method = self.grpc_method(&ws, &p.request, &target, &opts).await?;
                // Without a client stream the message goes out right away, then the client side ends.
                let first = (!method.is_client_streaming())
                    .then(|| self.grpc_message(&ws, &p.request.body.text, &mut unresolved));
                if let Some(text) = &first {
                    message_from_json(&method.input(), text)?;
                }
                let stream = self.inner.client.grpc_stream(&target, &method, &opts).await?;
                if let Some(text) = &first {
                    stream.session.send(text)?;
                    stream.session.end();
                }
                Ok::<_, ApiError>((stream, method, unresolved))
            } => r,
            _ = cancel.cancelled() => Err(EngineError::cancelled().into()),
        };
        let (stream, method, unresolved) = match started {
            Ok(s) => s,
            Err(e) => {
                remove_if_current(sessions, &p.session_id, |e| e.generation == generation);
                remove_if_current(&self.inner.inflight, &request_id, |(g, _)| *g == generation);
                return Err(e);
            }
        };
        let GrpcStream { opened, session, mut events } = stream;
        match lock(sessions).get_mut(&p.session_id) {
            Some(entry) if entry.generation == generation => entry.session = Some(session),
            // Cancelled or replaced while connecting: dropping the session cancels the call.
            _ => return Err(EngineError::cancelled().into()),
        }
        let inner = self.inner.clone();
        let session_id = p.session_id.clone();
        let log = Arc::new(MessageLog::new(!p.request.grpc.on_connect.is_empty()));
        let (forward_log, forward_alive) = (log.clone(), alive.clone());
        tokio::spawn(async move {
            let mut cancelled = false;
            loop {
                tokio::select! {
                    _ = cancel.cancelled(), if !cancelled => {
                        cancelled = true;
                        if let Some(entry) = lock(&inner.grpc.sessions).get(&session_id)
                            && entry.generation == generation
                            && let Some(session) = &entry.session
                        {
                            session.cancel();
                        }
                    }
                    event = events.recv() => {
                        let Some(event) = event else { break };
                        let end = matches!(event, GrpcEvent::End { .. });
                        forward_log.record_grpc(&event);
                        settled.grpc(&event);
                        inner.sink.emit(StreamEvent::Grpc { session_id: session_id.clone(), event });
                        if end {
                            break;
                        }
                    }
                }
            }
            forward_log.end();
            settled.end();
            crate::file_stream::stop_file_stream(&inner, &session_id);
            forward_alive.cancel();
            remove_if_current(&inner.grpc.sessions, &session_id, |e| e.generation == generation);
            remove_if_current(&inner.inflight, &request_id, |(g, _)| *g == generation);
        });
        if !p.request.grpc.on_connect.is_empty() {
            let api = self.clone();
            let (id, options, client_streaming) =
                (p.session_id.clone(), p.request.grpc.clone(), method.is_client_streaming());
            tokio::spawn(async move {
                api.run_grpc_steps(id, generation, options, client_streaming, log, alive).await;
            });
        }
        Ok(GrpcStartResult { opened, method: GrpcMethod::of(&method), unresolved })
    }
}

impl Api {
    /// Send a JSON message on the stream `session_id` (only if it is still call `generation`).
    fn grpc_session_send(&self, session_id: &str, generation: u64, json: &str) -> Result<(), String> {
        let sessions = lock(&self.inner.grpc.sessions);
        let session = sessions
            .get(session_id)
            .filter(|e| e.generation == generation)
            .and_then(|e| e.session.as_ref())
            .ok_or("The call is not open")?;
        session.send(json).map_err(|e| e.message)
    }

    /// A stream's steps, once it is open: send saved messages, wait for replies, keep sending.
    async fn run_grpc_steps(
        &self,
        session_id: String,
        generation: u64,
        options: GrpcOptions,
        client_streaming: bool,
        log: Arc<MessageLog>,
        alive: CancellationToken,
    ) {
        let render = self.renderer();
        let send = |m: &SavedMessage| {
            let text = grpc_text(m, &*render)?;
            self.grpc_session_send(&session_id, generation, &text)
        };
        let note = |text: String, error: bool| {
            let event = if error { GrpcEvent::Error { message: text } } else { GrpcEvent::info(text) };
            self.inner.sink.emit(StreamEvent::Grpc { session_id: session_id.clone(), event });
        };
        let repeat = |message: &SavedMessage, every: Duration| {
            note(format!("On connect: sending {} every {}", message.name, live::every_text(every)), false);
            let (api, id, render) = (self.clone(), session_id.clone(), self.renderer());
            let (failed_api, failed_id) = (self.clone(), session_id.clone());
            let send = move |m: &SavedMessage| {
                let text = grpc_text(m, &*render)?;
                api.grpc_session_send(&id, generation, &text)
            };
            let failed = move |message: String| {
                failed_api
                    .inner
                    .sink
                    .emit(StreamEvent::Grpc { session_id: failed_id, event: GrpcEvent::Error { message } });
            };
            live::spawn_repeat(message.clone(), every, alive.child_token(), send, failed);
        };
        let repeats = options.on_connect.iter().any(|s| matches!(s, ConnectStep::Repeat(_)));
        let steps = Steps {
            steps: &options.on_connect,
            messages: &options.messages,
            log: &log,
            send: &send,
            note: &note,
            repeat: &repeat,
            cancel: &alive,
        };
        let results = run_steps(steps).await;
        let passed = results.iter().all(|r| r.passed);
        // Ending the client side would stop a repeating send: only when nothing repeats.
        if options.end_after_steps && client_streaming && passed && !repeats && !alive.is_cancelled() {
            if let Some(session) = lock(&self.inner.grpc.sessions)
                .get(&session_id)
                .filter(|e| e.generation == generation)
                .and_then(|e| e.session.as_ref())
            {
                session.end();
            }
            note("On connect: ended the stream (no more messages from this side)".into(), false);
        }
    }
}

/// A saved message's JSON, its variables filled in (gRPC messages are JSON).
pub(crate) fn grpc_text(message: &SavedMessage, render: &dyn Fn(&str) -> String) -> Result<String, String> {
    if message.format == MessageFormat::Binary {
        let name = if message.name.is_empty() { "The message".to_string() } else { format!("'{}'", message.name) };
        return Err(format!("{name} is binary; gRPC messages are JSON"));
    }
    Ok(render(&message.text))
}

/// The messages a call made in one go keeps (their JSON): reading stops there.
const MAX_KEPT_BYTES: usize = 64 << 20;

/// A call made to its end (collection runs, AI agents, a stream sent in one go).
pub(crate) struct CallRun {
    pub response: GrpcResponse,
    /// A stream's steps.
    pub steps: Vec<StepResult>,
    /// What the steps did, and why reading stopped.
    pub notes: Vec<String>,
}

/// Make a call to its end. Unary: the message. Streams: the message (server streams), or the
/// steps then the message when there are none (client and bidirectional streams); the client
/// side ends after them. Reading stops as `settings.stream` says (messages, time), which ends
/// the call without counting as a failure.
pub(crate) async fn run_call(
    client: &zorvik_engine::Client,
    target: &GrpcTarget,
    method: &MethodDescriptor,
    request: &Request,
    opts: &RequestOptions,
    render: &(dyn Fn(&str) -> String + Send + Sync),
) -> ApiResult<CallRun> {
    if !method.is_client_streaming() && !method.is_server_streaming() {
        let response = client.grpc_unary(target, method, &render(&request.body.text), opts).await?;
        return Ok(CallRun { response, steps: Vec::new(), notes: Vec::new() });
    }
    let until = request.settings.stream.clone().unwrap_or_default();
    let limit = Duration::from_millis(until.timeout_ms.max(1)).min(crate::sse_read::MAX_WAIT);
    let deadline = tokio::time::Instant::now() + limit;
    let stream = tokio::time::timeout_at(deadline, client.grpc_stream(target, method, opts))
        .await
        .map_err(|_| ApiError::new("timeout", format!("No answer within {} ms", limit.as_millis())))??;
    let GrpcStream { opened, session, mut events } = stream;
    let log = Arc::new(MessageLog::default());
    let (tx, mut collected) = tokio::sync::mpsc::unbounded_channel();
    let pump = {
        let log = log.clone();
        tokio::spawn(async move {
            while let Some(event) = events.recv().await {
                log.record_grpc(&event);
                let end = matches!(event, GrpcEvent::End { .. });
                let _ = tx.send(event);
                if end {
                    break;
                }
            }
            log.end();
        })
    };
    let mut notes = Vec::new();
    let mut steps = Vec::new();
    let options = &request.grpc;
    if !method.is_client_streaming() {
        session.send(&render(&request.body.text))?;
        session.end();
    }
    if !options.on_connect.is_empty() {
        let send = |m: &SavedMessage| session.send(&grpc_text(m, render)?).map_err(|e| e.message);
        let (results, step_notes) = live::run_steps_once(&options.on_connect, &options.messages, &log, &send).await;
        steps = results;
        notes.extend(step_notes);
    } else if method.is_client_streaming() && !request.body.text.trim().is_empty() {
        session.send(&render(&request.body.text))?;
    }
    session.end();

    let mut response = GrpcResponse {
        status: GrpcStatus::new(2, "The call ended without a status", true),
        messages: Vec::new(),
        headers: Vec::new(),
        trailers: Vec::new(),
        timing: opened.timing.clone(),
        remote_addr: opened.remote_addr.clone(),
        tls: opened.tls.clone(),
        request_headers: opened.request_headers.clone(),
        warnings: Vec::new(),
    };
    let max = until.max_events as usize;
    let mut received = 0;
    let mut kept = 0;
    let mut stopped: Option<String> = None;
    loop {
        let event = tokio::select! {
            e = collected.recv() => e,
            _ = tokio::time::sleep_until(deadline), if stopped.is_none() => {
                stopped = Some(format!("Stopped reading after {} ms", limit.as_millis()));
                session.cancel();
                continue;
            }
        };
        let Some(event) = event else { break };
        match event {
            GrpcEvent::Headers { headers, .. } => response.headers = headers,
            // Reading stopped: what was still on the way isn't kept (the call is being cancelled).
            GrpcEvent::Message { .. } if stopped.is_some() => {}
            GrpcEvent::Message { message } => {
                let incoming = message.direction == zorvik_engine::Direction::Received;
                kept += message.json.len();
                response.messages.push(message);
                if kept > MAX_KEPT_BYTES && stopped.is_none() {
                    stopped = Some(format!("Stopped reading after {} MB of messages", MAX_KEPT_BYTES >> 20));
                    session.cancel();
                }
                if incoming {
                    received += 1;
                    if max > 0 && received >= max && stopped.is_none() {
                        stopped = Some(format!("Stopped reading after {received} messages"));
                        session.cancel();
                    }
                }
            }
            GrpcEvent::Error { message } => response.warnings.push(message),
            GrpcEvent::Info { .. } => {}
            GrpcEvent::End { status, trailers, timing } => {
                response.status = status;
                response.trailers = trailers;
                response.timing = timing;
                break;
            }
        }
    }
    drop(session);
    let _ = tokio::time::timeout(Duration::from_secs(2), pump).await;
    if let Some(why) = stopped {
        // Stopped by us, not by the server: a stream that answered did what it was asked to; one
        // that never answered in time didn't (a hung server isn't a pass).
        if response.status.local && response.status.code == 1 {
            response.status = if received > 0 {
                GrpcStatus::new(0, why.clone(), true)
            } else {
                GrpcStatus::new(4, format!("No answer within {} ms", limit.as_millis()), true)
            };
        }
        notes.push(why);
    }
    Ok(CallRun { response, steps, notes })
}

/// A call's answer as an HTTP-like response for scripts, tests and runs: status 200 when OK
/// (else the HTTP status gRPC maps the code to), headers then trailers, and the body: the
/// message (unary) or the received messages as a JSON array (streams).
pub(crate) fn as_response(url: &str, call: &GrpcResponse, streaming: bool) -> HttpResponse {
    use zorvik_engine::{Header, ResponseMeta, SentRequest};
    let received: Vec<&str> = call
        .messages
        .iter()
        .filter(|m| m.direction == zorvik_engine::Direction::Received)
        .map(|m| m.json.as_str())
        .collect();
    let body = if streaming {
        let list: Vec<Value> = received.iter().map(|j| serde_json::from_str(j).unwrap_or(Value::Null)).collect();
        serde_json::to_vec_pretty(&list).unwrap_or_default()
    } else {
        received.first().map(|j| j.as_bytes().to_vec()).unwrap_or_default()
    };
    let status = http_status(call.status.code);
    let status_text = if call.status.message.is_empty() {
        call.status.name.clone()
    } else {
        format!("{} · {}", call.status.name, call.status.message)
    };
    let mut headers = call.headers.clone();
    headers.extend(call.trailers.iter().cloned());
    if !headers.iter().any(|h| h.name.eq_ignore_ascii_case("grpc-status")) {
        headers.push(Header::new("grpc-status", call.status.code.to_string()));
    }
    let sent: u64 =
        call.messages.iter().filter(|m| m.direction == zorvik_engine::Direction::Sent).map(|m| m.size).sum();
    HttpResponse {
        meta: ResponseMeta {
            status,
            status_text,
            http_version: "HTTP/2 (gRPC)".into(),
            headers,
            url: url.to_string(),
            remote_addr: call.remote_addr.clone(),
            tls: call.tls.clone(),
            redirects: Vec::new(),
            request: SentRequest {
                method: "POST".into(),
                url: url.to_string(),
                http_version: "HTTP/2".into(),
                headers: call.request_headers.clone(),
                body_size: sent,
                proxy: None,
            },
            headers_size: 0,
            cookies: Vec::new(),
        },
        timing: call.timing.clone(),
        body_wire_size: body.len() as u64,
        body,
        body_truncated: false,
        decode_warning: (!call.warnings.is_empty()).then(|| call.warnings.join("; ")),
    }
}

/// The HTTP status gRPC's code corresponds to (as gRPC gateways map them).
fn http_status(code: u32) -> u16 {
    match code {
        0 => 200,
        1 => 499,
        3 | 9 | 11 => 400,
        4 => 504,
        5 => 404,
        6 | 10 => 409,
        7 => 403,
        8 => 429,
        12 => 501,
        14 => 503,
        16 => 401,
        _ => 500,
    }
}

/// The request's `.proto` files and import folders, checked to be inside the workspace (unless
/// files outside it are allowed).
fn proto_paths(root: &Path, request: &Request, outside: bool) -> ApiResult<(Vec<PathBuf>, Vec<PathBuf>)> {
    let grpc = &request.grpc;
    let files = grpc
        .proto_files
        .iter()
        .filter(|f| !f.trim().is_empty())
        .map(|f| workspace_path(root, f, outside, false))
        .collect::<ApiResult<Vec<_>>>()?;
    let imports = grpc
        .import_paths
        .iter()
        .filter(|f| !f.trim().is_empty())
        .map(|f| workspace_path(root, f, outside, true))
        .collect::<ApiResult<Vec<_>>>()?;
    Ok((files, imports))
}

async fn compile_protos(
    root: &Path,
    outside: bool,
    files: Vec<PathBuf>,
    imports: Vec<PathBuf>,
) -> ApiResult<GrpcDescriptors> {
    // Compiling is CPU-bound: keep it off the async workers.
    let descriptors = tokio::task::spawn_blocking(move || GrpcDescriptors::from_proto_files(&files, &imports))
        .await
        .map_err(join_err)??;
    if !outside {
        imports_inside(root, &descriptors.files)?;
    }
    Ok(descriptors)
}

/// Collection runs: the request's method from its `.proto` files or server reflection (not
/// cached), then the call to its end.
pub(crate) async fn run_once(
    client: &zorvik_engine::Client,
    root: &Path,
    outside: bool,
    request: &Request,
    target: &GrpcTarget,
    opts: &RequestOptions,
    render: &(dyn Fn(&str) -> String + Send + Sync),
) -> ApiResult<(HttpResponse, Vec<StepResult>, Vec<String>)> {
    let descriptors = if uses_proto_files(request) {
        let (files, imports) = proto_paths(root, request, outside)?;
        compile_protos(root, outside, files, imports).await?
    } else {
        client.grpc_reflect(target, opts).await?
    };
    let method = descriptors.method(&request.method)?;
    let streaming = method.is_client_streaming() || method.is_server_streaming();
    let call = Box::pin(run_call(client, target, &method, request, opts, render)).await?;
    let mut notes = call.notes;
    if !call.response.status.is_ok() || !call.response.status.message.is_empty() {
        notes.push(
            format!("gRPC status {} {}", call.response.status.name, call.response.status.message).trim().to_string(),
        );
    }
    notes.extend(call.response.warnings.iter().cloned());
    Ok((as_response(&target.url, &call.response, streaming), call.steps, notes))
}

struct HistoryEntry<'a> {
    url: &'a str,
    status: Option<u16>,
    error: Option<&'a str>,
    duration_ms: f64,
}

fn uses_proto_files(request: &Request) -> bool {
    request.grpc.proto_files.iter().any(|f| !f.trim().is_empty())
}

/// A `.proto` file or import folder: relative to the workspace root, or
/// absolute. Like body files, it must be inside the workspace unless files
/// outside it are allowed (Settings → Data & privacy).
fn workspace_path(root: &Path, raw: &str, outside: bool, folder: bool) -> ApiResult<PathBuf> {
    let what = if folder { "Import folder" } else { "Proto file" };
    let p = Path::new(raw.trim());
    let full = if p.is_absolute() { p.to_path_buf() } else { root.join(p) };
    let meta = std::fs::metadata(&full)
        .map_err(|e| ApiError::new("io", format!("{what} '{}' can't be read: {e}", full.display())))?;
    if meta.is_dir() != folder {
        let kind = if folder { "a folder" } else { "a file" };
        return Err(ApiError::invalid(format!("{what} '{}' is not {kind}", full.display())));
    }
    if !outside && !is_inside(root, &full) {
        return Err(ApiError::invalid(format!(
            "{what} '{}' is outside the workspace folder. Move it into the workspace, \
             or allow files outside the workspace in Settings → Data & privacy.",
            full.display()
        )));
    }
    Ok(full)
}

/// Canonical paths: `..` and links that lead out of the workspace count as outside.
fn is_inside(root: &Path, path: &Path) -> bool {
    match (std::fs::canonicalize(path), std::fs::canonicalize(root)) {
        (Ok(path), Ok(root)) => path.starts_with(root),
        _ => false,
    }
}

/// Every file the compiler read (imports too, e.g. through a link inside an
/// import folder) must be inside the workspace, like the files themselves.
fn imports_inside(root: &Path, read: &[PathBuf]) -> ApiResult<()> {
    match read.iter().find(|f| !is_inside(root, f)) {
        Some(file) => Err(ApiError::invalid(format!(
            "Imported file '{}' is outside the workspace folder. Move it into the workspace, \
             or allow files outside the workspace in Settings → Data & privacy.",
            file.display()
        ))),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proto_paths_stay_inside_the_workspace() {
        let root = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("protos")).unwrap();
        std::fs::write(root.path().join("protos/a.proto"), "").unwrap();
        std::fs::write(other.path().join("b.proto"), "").unwrap();
        assert!(workspace_path(root.path(), "protos/a.proto", false, false).is_ok());
        assert!(workspace_path(root.path(), "protos", false, true).is_ok());
        let outside = other.path().join("b.proto");
        let err = workspace_path(root.path(), &outside.to_string_lossy(), false, false).unwrap_err();
        assert!(err.message.contains("outside the workspace"), "{}", err.message);
        assert!(workspace_path(root.path(), &outside.to_string_lossy(), true, false).is_ok());
        assert!(workspace_path(root.path(), "../x.proto", false, false).is_err());
        assert!(workspace_path(root.path(), "protos", false, false).unwrap_err().message.contains("not a file"));
        assert!(
            workspace_path(root.path(), "missing.proto", false, false).unwrap_err().message.contains("can't be read")
        );
    }

    #[cfg(unix)]
    #[test]
    fn imports_through_links_out_of_the_workspace_are_refused() {
        let root = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        std::fs::write(other.path().join("b.proto"), "syntax = \"proto3\"; package ext; message B {}").unwrap();
        std::fs::create_dir_all(root.path().join("protos")).unwrap();
        std::os::unix::fs::symlink(other.path(), root.path().join("protos/ext")).unwrap();
        let api = root.path().join("protos/a.proto");
        std::fs::write(
            &api,
            "syntax = \"proto3\"; import \"ext/b.proto\"; service S { rpc M(ext.B) returns (ext.B); }",
        )
        .unwrap();
        let files = workspace_path(root.path(), "protos/a.proto", false, false).unwrap();
        let d = GrpcDescriptors::from_proto_files(&[files], &[]).unwrap();
        let err = imports_inside(root.path(), &d.files).unwrap_err();
        assert!(err.message.contains("b.proto") && err.message.contains("outside the workspace"), "{}", err.message);
        std::fs::write(root.path().join("protos/c.proto"), "syntax = \"proto3\"; message C {}").unwrap();
        let inside = GrpcDescriptors::from_proto_files(&[root.path().join("protos/c.proto")], &[]).unwrap();
        imports_inside(root.path(), &inside.files).unwrap();
    }

    #[test]
    fn cached_files_notice_same_second_edits() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.proto");
        std::fs::write(&file, "syntax = \"proto3\";").unwrap();
        let before = stamp(&file).unwrap();
        let mtime = std::fs::metadata(&file).unwrap().modified().unwrap();
        std::fs::write(&file, "syntax = \"proto3\"; message M {}").unwrap();
        // Same modification time (a coarse file system), other size: still a change.
        std::fs::File::options().write(true).open(&file).unwrap().set_modified(mtime).unwrap();
        assert_ne!(stamp(&file), Some(before));
    }
}

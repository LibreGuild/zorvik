//! Reading a Server-Sent Events stream, or a GraphQL subscription's results, to an end: a named
//! event, a number of events or a time limit. Agents (`send_request`) and the collection runner
//! use it; the app's live tabs stream events to the UI instead (`sse.connect`, `socket.connect`).

use std::time::{Duration, Instant};

use serde::Serialize;
use tokio_util::sync::CancellationToken;
use ts_rs::TS;
use zorvik_engine::{
    Direction, EngineError, ResponseMeta, SocketConnected, SocketEvent, SseEvent, SseParser, StreamingResponse, Timing,
};
use zorvik_workspace::Workspace;
use zorvik_workspace::formats::{Request, StreamUntil};

use crate::{Api, ApiError, ApiResult, request_options};

/// Events kept at most, and data characters kept per event.
pub const MAX_EVENTS: usize = 1000;
const MAX_EVENT_DATA: usize = 64 * 1024;
/// The longest a read may wait for events.
pub const MAX_WAIT: Duration = Duration::from_secs(300);
/// Body bytes kept when the answer is not an event stream.
const MAX_ERROR_BODY: usize = 4096;

/// Why reading stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum SseEnd {
    /// The awaited event arrived.
    Event,
    /// `max_events` arrived.
    Count,
    Timeout,
    /// The server ended the stream.
    Closed,
    /// The answer was not an event stream (see `body`).
    NotAStream,
}

/// What a read got.
pub struct SseRead {
    pub meta: ResponseMeta,
    pub timing: Timing,
    pub events: Vec<SseEvent>,
    pub end: SseEnd,
    /// Events after the first `MAX_EVENTS` (counted, not kept).
    pub dropped: u32,
    /// The start of the body when it was not an event stream.
    pub body: Option<String>,
    /// A read error after the stream started (the events before it are kept).
    pub error: Option<String>,
    pub unresolved: Vec<String>,
    pub duration: Duration,
}

impl Api {
    /// Open `request` as an event stream: resolved, authorized and sent like the app's SSE tab.
    pub(crate) async fn open_event_stream(
        &self,
        ws: &Workspace,
        request: &Request,
        path: Option<&str>,
    ) -> ApiResult<(StreamingResponse, Vec<String>)> {
        let settings = self.settings();
        let mut resolved = self.prepare(ws, request, path)?;
        let opts = request_options(&settings, &request.settings)?;
        self.authorize(ws, &mut resolved, &opts).await?;
        let has = |name: &str| resolved.request.headers.iter().any(|h| h.name.eq_ignore_ascii_case(name));
        if !has("accept") {
            resolved.request.headers.push(zorvik_engine::Header::new("Accept", "text/event-stream"));
        }
        if !resolved.request.headers.iter().any(|h| h.name.eq_ignore_ascii_case("cache-control")) {
            resolved.request.headers.push(zorvik_engine::Header::new("Cache-Control", "no-cache"));
        }
        let jar = settings.cookie_jar.then(|| self.jar(ws));
        let stream = self.inner.client.open_stream(resolved.request, &opts, jar.as_deref()).await?;
        if let Some(jar) = &jar {
            self.save_jar(ws, jar);
        }
        Ok((stream, resolved.unresolved))
    }

    /// Start `request`'s GraphQL subscription and read its results until `until` says to stop
    /// (or `cancel`). Each result is an event named `next`.
    pub(crate) async fn read_subscription(
        &self,
        ws: &Workspace,
        request: &Request,
        path: Option<&str>,
        until: &StreamUntil,
        cancel: &CancellationToken,
    ) -> ApiResult<SseRead> {
        let started = Instant::now();
        let deadline = tokio::time::Instant::now() + wait_of(until);
        let settings = self.settings();
        let opened = async {
            let mut resolved = self.prepare(ws, request, path)?;
            let opts = request_options(&settings, &request.settings)?;
            self.authorize(ws, &mut resolved, &opts).await?;
            let unresolved = resolved.unresolved.clone();
            let jar = settings.cookie_jar.then(|| self.jar(ws));
            let conn = self.connect_socket(request, resolved, &opts, jar.as_ref()).await?;
            if let Some(jar) = &jar {
                self.save_jar(ws, jar);
            }
            Ok::<_, ApiError>((conn, unresolved))
        };
        let (conn, unresolved) = tokio::select! {
            r = opened => r?,
            _ = cancel.cancelled() => return Err(EngineError::cancelled().into()),
            _ = tokio::time::sleep_until(deadline) => {
                return Err(ApiError::new("timeout", format!("No answer within {} ms", wait_of(until).as_millis())));
            }
        };
        let mut read = collect_results(conn, until, cancel, started, deadline).await?;
        read.unresolved = unresolved;
        Ok(read)
    }

    /// Send `request` and read its events until `until` says to stop (or `cancel`).
    pub(crate) async fn read_event_stream(
        &self,
        ws: &Workspace,
        request: &Request,
        path: Option<&str>,
        until: &StreamUntil,
        cancel: &CancellationToken,
    ) -> ApiResult<SseRead> {
        let started = Instant::now();
        let deadline = tokio::time::Instant::now() + wait_of(until);
        let opened = tokio::select! {
            r = self.open_event_stream(ws, request, path) => r?,
            _ = cancel.cancelled() => return Err(EngineError::cancelled().into()),
            _ = tokio::time::sleep_until(deadline) => {
                return Err(ApiError::new("timeout", format!("No answer within {} ms", wait_of(until).as_millis())));
            }
        };
        let (stream, unresolved) = opened;
        let mut read = collect_events(stream, until, cancel, started, deadline).await?;
        read.unresolved = unresolved;
        Ok(read)
    }
}

fn wait_of(until: &StreamUntil) -> Duration {
    Duration::from_millis(until.timeout_ms.max(1)).min(MAX_WAIT)
}

/// Read an opened stream until `until` says to stop (or `deadline`, or `cancel`). An answer
/// that is not an event stream comes back as `SseEnd::NotAStream` with the start of its body.
pub(crate) async fn collect_events(
    stream: StreamingResponse,
    until: &StreamUntil,
    cancel: &CancellationToken,
    started: Instant,
    deadline: tokio::time::Instant,
) -> ApiResult<SseRead> {
    let StreamingResponse { meta, timing, mut body } = stream;
    let is_stream = meta.headers.iter().any(|h| {
        h.name.eq_ignore_ascii_case("content-type") && h.value.to_ascii_lowercase().contains("text/event-stream")
    });
    let mut read = SseRead {
        meta,
        timing,
        events: Vec::new(),
        end: SseEnd::Timeout,
        dropped: 0,
        body: None,
        error: None,
        unresolved: Vec::new(),
        duration: Duration::ZERO,
    };
    if !is_stream || !(200..300).contains(&read.meta.status) {
        let mut bytes = Vec::new();
        tokio::select! {
            _ = async {
                while let Some(Ok(chunk)) = body.next_chunk().await {
                    bytes.extend_from_slice(&chunk);
                    if bytes.len() > MAX_ERROR_BODY {
                        break;
                    }
                }
            } => {}
            _ = cancel.cancelled() => {}
            _ = tokio::time::sleep_until(deadline) => {}
        }
        bytes.truncate(MAX_ERROR_BODY);
        read.body = Some(String::from_utf8_lossy(&bytes).into_owned());
        read.end = SseEnd::NotAStream;
        read.duration = started.elapsed();
        return Ok(read);
    }
    let mut parser = SseParser::new();
    let mut seen: u32 = 0;
    read.end = 'read: loop {
        tokio::select! {
            _ = cancel.cancelled() => return Err(EngineError::cancelled().into()),
            _ = tokio::time::sleep_until(deadline) => break 'read SseEnd::Timeout,
            chunk = body.next_chunk() => match chunk {
                Some(Ok(bytes)) => {
                    for mut event in parser.feed(&bytes) {
                        seen += 1;
                        let matched = !until.event.is_empty() && event.event == until.event;
                        if read.events.len() < MAX_EVENTS {
                            if let Some((at, _)) = event.data.char_indices().nth(MAX_EVENT_DATA) {
                                event.data.truncate(at);
                            }
                            read.events.push(event);
                        } else {
                            read.dropped += 1;
                        }
                        if matched {
                            break 'read SseEnd::Event;
                        }
                        if until.max_events > 0 && seen >= until.max_events {
                            break 'read SseEnd::Count;
                        }
                    }
                }
                Some(Err(e)) => {
                    read.error = Some(e.message);
                    break 'read SseEnd::Closed;
                }
                None => break 'read SseEnd::Closed,
            }
        }
    };
    read.duration = started.elapsed();
    Ok(read)
}

/// Read a started subscription's results (as `next` events) until `until` says to stop (or
/// `deadline`, or `cancel`). The session ends when this returns.
pub(crate) async fn collect_results(
    conn: SocketConnected,
    until: &StreamUntil,
    cancel: &CancellationToken,
    started: Instant,
    deadline: tokio::time::Instant,
) -> ApiResult<SseRead> {
    let SocketConnected { opened, meta, session, mut events } = conn;
    let Some(meta) = meta else { return Err(ApiError::invalid("This connection has no results to read")) };
    let mut read = SseRead {
        meta,
        timing: opened.timing,
        events: Vec::new(),
        end: SseEnd::Timeout,
        dropped: 0,
        body: None,
        error: None,
        unresolved: Vec::new(),
        duration: Duration::ZERO,
    };
    let mut seen: u32 = 0;
    read.end = 'read: loop {
        tokio::select! {
            _ = cancel.cancelled() => return Err(EngineError::cancelled().into()),
            _ = tokio::time::sleep_until(deadline) => break 'read SseEnd::Timeout,
            event = events.recv() => match event {
                Some(SocketEvent::Message { direction: Direction::Received, text, base64, .. }) => {
                    seen += 1;
                    let mut data = text.or(base64).unwrap_or_default();
                    if let Some((at, _)) = data.char_indices().nth(MAX_EVENT_DATA) {
                        data.truncate(at);
                    }
                    let event = SseEvent { event: "next".into(), data, id: None, retry: None };
                    let matched = !until.event.is_empty() && event.event == until.event;
                    if read.events.len() < MAX_EVENTS {
                        read.events.push(event);
                    } else {
                        read.dropped += 1;
                    }
                    if matched {
                        break 'read SseEnd::Event;
                    }
                    if until.max_events > 0 && seen >= until.max_events {
                        break 'read SseEnd::Count;
                    }
                }
                Some(SocketEvent::Error { message }) => read.error = Some(message),
                Some(SocketEvent::Closed { .. }) | None => break 'read SseEnd::Closed,
                Some(_) => {}
            }
        }
    };
    drop(session);
    read.duration = started.elapsed();
    Ok(read)
}

/// A subscription's results as a JSON array, for `pm.response.json()`.
pub(crate) fn results_json(events: &[SseEvent]) -> String {
    let results: Vec<serde_json::Value> = events
        .iter()
        .map(|e| serde_json::from_str(&e.data).unwrap_or_else(|_| serde_json::Value::String(e.data.clone())))
        .collect();
    serde_json::to_string_pretty(&results).unwrap_or_default()
}

/// The events as the wire had them (`event:`, `id:`, `data:` lines), for `pm.response.text()`.
pub(crate) fn events_text(events: &[SseEvent]) -> String {
    let mut out = String::new();
    for e in events {
        if e.event != "message" {
            out.push_str(&format!("event: {}\n", e.event));
        }
        if let Some(id) = &e.id {
            out.push_str(&format!("id: {id}\n"));
        }
        for line in e.data.split('\n') {
            out.push_str(&format!("data: {line}\n"));
        }
        out.push('\n');
    }
    out
}

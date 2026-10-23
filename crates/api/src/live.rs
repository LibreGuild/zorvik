//! What WebSocket and socket sessions (Socket.IO, TCP, UDP, MQTT) share: a log of each
//! connection's recent messages ([`MessageLog`]) that waits and AI agents read, the request's
//! on-connect steps ([`run_steps`]: send a saved message, wait for one that matches, keep
//! sending one), and repeating sends ([`spawn_repeat`]).

use std::collections::VecDeque;
use std::time::Duration;

use base64::Engine as _;
use serde::Serialize;
use tokio::sync::Notify;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use zorvik_engine::{Direction, SocketEvent, WsEvent, WsMessageKind};
use zorvik_workspace::formats::matching::{MessageMatcher, describe};
use zorvik_workspace::formats::{ConnectStep, MatchKind, SavedMessage, WaitStep};

use crate::agents::redact::Redactor;
use crate::lock;

/// Messages a connection's log keeps, and the payload bytes they may hold.
pub(crate) const MAX_LOG_MESSAGES: usize = 1000;
const MAX_LOG_BYTES: usize = 8 << 20;
/// The longest wait step, and the shortest pause between repeated sends.
pub(crate) const MAX_WAIT: Duration = Duration::from_secs(600);
pub(crate) const MIN_REPEAT: Duration = Duration::from_millis(10);
/// Why a wait stopped looking, for [`Waited::Overrun`].
pub(crate) const OVERRUN: &str = "Messages came in faster than they could be checked: some were dropped from the log (it keeps 1000 messages or 8 MB) before they were looked at";

// ---- the message log ----------------------------------------------------------------------

/// One message of a connection, numbered in order (sent and received alike).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LoggedMessage {
    pub id: u64,
    pub direction: Direction,
    /// Text messages (Socket.IO: the arguments as JSON).
    pub text: Option<String>,
    /// Binary messages, as base64.
    pub base64: Option<String>,
    pub size: u64,
    /// Unix epoch milliseconds.
    pub timestamp: f64,
    /// Socket.IO: the event's name; MQTT: the topic.
    pub event: Option<String>,
    /// Socket.IO: acknowledgement notes ("asks for an acknowledgement (#3)"); MQTT: QoS.
    pub detail: Option<String>,
}

impl LoggedMessage {
    pub fn bytes(&self) -> std::borrow::Cow<'_, [u8]> {
        match (&self.text, &self.base64) {
            (Some(text), _) => std::borrow::Cow::Borrowed(text.as_bytes()),
            (None, Some(b64)) => base64::engine::general_purpose::STANDARD.decode(b64).unwrap_or_default().into(),
            _ => std::borrow::Cow::Borrowed(&[]),
        }
    }

    fn weight(&self) -> usize {
        self.text.as_ref().map_or(0, String::len) + self.base64.as_ref().map_or(0, String::len)
    }
}

/// The recent messages of a connection, and how it ended.
pub(crate) struct MessageLog {
    state: std::sync::Mutex<LogState>,
    notify: Notify,
    /// Off when nothing will read the messages (no steps, no agent): only the end is kept.
    keep: bool,
    /// An agent's connection: secrets are hidden as messages come in, so what the agent reads
    /// and what its waits match never holds them.
    redact: Option<std::sync::Mutex<Redactor>>,
}

impl Default for MessageLog {
    fn default() -> Self {
        Self::new(true)
    }
}

#[derive(Default)]
struct LogState {
    messages: VecDeque<LoggedMessage>,
    next_id: u64,
    bytes: usize,
    ended: Option<String>,
}

/// What a wait found.
pub(crate) enum Waited {
    Found(LoggedMessage),
    TimedOut,
    /// The connection closed first (why).
    Ended(String),
    /// More messages came in than the log keeps before they could all be looked at.
    Overrun,
}

/// A message to note (`id` is given by the log).
pub(crate) struct NewMessage<'a> {
    pub direction: Direction,
    pub text: Option<&'a String>,
    pub base64: Option<&'a String>,
    pub size: u64,
    pub timestamp: f64,
    pub event: Option<&'a String>,
    pub detail: Option<&'a String>,
}

impl MessageLog {
    pub fn new(keep: bool) -> Self {
        Self { state: Default::default(), notify: Notify::new(), keep, redact: None }
    }

    /// Hide what `redactor` hides in every message and in how the connection ended.
    pub fn redacting(mut self, redactor: Redactor) -> Self {
        self.redact = Some(std::sync::Mutex::new(redactor));
        self
    }

    /// Hide more from here on (secrets of a later send, credentials of the handshake); only
    /// for a log made with `redacting`.
    pub fn redact_also(&self, more: &Redactor) {
        if let Some(r) = &self.redact {
            lock(r).absorb(more);
        }
    }

    /// What the log hides, for the rest of what an agent is told about the connection.
    pub fn redactor(&self) -> Option<Redactor> {
        self.redact.as_ref().map(|r| lock(r).clone())
    }

    /// A WebSocket's text and binary messages (pings and pongs are left out); a close ends the log.
    pub fn record_ws(&self, event: &WsEvent) {
        match event {
            WsEvent::Message {
                direction,
                kind: WsMessageKind::Text | WsMessageKind::Binary,
                text,
                base64,
                size,
                timestamp,
            } => self.push(NewMessage {
                direction: *direction,
                text: text.as_ref(),
                base64: base64.as_ref(),
                size: *size,
                timestamp: *timestamp,
                event: None,
                detail: None,
            }),
            WsEvent::Closed { code, reason, by_client, .. } => {
                let mut why = if *by_client { "Closed".to_string() } else { "The server closed the connection".into() };
                match (code, reason.is_empty()) {
                    (Some(code), false) => why.push_str(&format!(" (code {code} · {reason})")),
                    (Some(code), true) => why.push_str(&format!(" (code {code})")),
                    (None, false) => why.push_str(&format!(": {reason}")),
                    (None, true) => {}
                }
                self.end_with(why);
            }
            _ => {}
        }
    }

    /// A socket session's messages (Socket.IO events, TCP/UDP data, MQTT publishes).
    pub fn record_socket(&self, event: &SocketEvent) {
        match event {
            SocketEvent::Message { direction, text, base64, size, timestamp, topic, detail, .. } => {
                self.push(NewMessage {
                    direction: *direction,
                    text: text.as_ref(),
                    base64: base64.as_ref(),
                    size: *size,
                    timestamp: *timestamp,
                    event: topic.as_ref(),
                    detail: detail.as_ref(),
                })
            }
            SocketEvent::Closed { reason, by_client } => {
                let who = if *by_client { "Closed" } else { "The connection closed" };
                self.end_with(if reason.is_empty() { who.to_string() } else { format!("{who}: {reason}") });
            }
            _ => {}
        }
    }

    /// A gRPC stream's messages (their JSON); its end ends the log.
    pub fn record_grpc(&self, event: &zorvik_engine::GrpcEvent) {
        match event {
            zorvik_engine::GrpcEvent::Message { message } => self.push(NewMessage {
                direction: message.direction,
                text: Some(&message.json),
                base64: None,
                size: message.size,
                timestamp: message.timestamp,
                event: None,
                detail: None,
            }),
            zorvik_engine::GrpcEvent::End { status, .. } => {
                let why = if status.is_ok() {
                    "The call ended (OK)".to_string()
                } else if status.message.is_empty() {
                    format!("The call ended ({})", status.name)
                } else {
                    format!("The call ended ({}: {})", status.name, status.message)
                };
                self.end_with(why);
            }
            _ => {}
        }
    }

    fn push(&self, m: NewMessage<'_>) {
        if !self.keep {
            return;
        }
        let (mut text, mut base64, mut event) = (m.text.cloned(), m.base64.cloned(), m.event.cloned());
        if let Some(r) = &self.redact {
            let r = lock(r);
            if !r.is_empty() {
                let b64 = &base64::engine::general_purpose::STANDARD;
                text = text.map(|t| r.text(&t));
                event = event.map(|e| r.text(&e));
                base64 = base64.map(|b| match b64.decode(&b) {
                    Ok(bytes) => match r.bytes(&bytes) {
                        std::borrow::Cow::Owned(hidden) => b64.encode(hidden),
                        std::borrow::Cow::Borrowed(_) => b,
                    },
                    Err(_) => b,
                });
            }
        }
        let mut state = lock(&self.state);
        state.next_id += 1;
        let message = LoggedMessage {
            id: state.next_id,
            direction: m.direction,
            text,
            base64,
            size: m.size,
            timestamp: m.timestamp,
            event,
            detail: m.detail.cloned(),
        };
        state.bytes += message.weight();
        state.messages.push_back(message);
        while state.messages.len() > MAX_LOG_MESSAGES || (state.bytes > MAX_LOG_BYTES && state.messages.len() > 1) {
            if let Some(old) = state.messages.pop_front() {
                state.bytes -= old.weight();
            }
        }
        drop(state);
        self.notify.notify_waiters();
    }

    fn end_with(&self, why: String) {
        let why = match self.redactor() {
            Some(r) => r.text(&why),
            None => why,
        };
        lock(&self.state).ended.get_or_insert(why);
        self.notify.notify_waiters();
    }

    /// The events stopped without a close (the connection is gone).
    pub fn end(&self) {
        self.end_with("The connection closed".into());
    }

    /// The id of the newest message (0 before the first).
    pub fn last_id(&self) -> u64 {
        lock(&self.state).next_id
    }

    pub fn ended(&self) -> Option<String> {
        lock(&self.state).ended.clone()
    }

    /// Messages after `after`, the newest `limit` of them; how many newer ones were left out at
    /// the start (or are no longer kept); and the newest id, all at one moment.
    pub fn since(&self, after: u64, limit: usize) -> (Vec<LoggedMessage>, u64, u64) {
        let state = lock(&self.state);
        let newer: Vec<&LoggedMessage> = state.messages.iter().filter(|m| m.id > after).collect();
        let first_kept = state.messages.front().map_or(state.next_id + 1, |m| m.id);
        let gone = first_kept.saturating_sub(after + 1);
        let skip = newer.len().saturating_sub(limit);
        (newer[skip..].iter().map(|m| (*m).clone()).collect(), gone + skip as u64, state.next_id)
    }

    /// The first received message after `after` that matches (and is the event `event`, when
    /// given), waiting until `deadline`.
    pub async fn wait_for(
        &self,
        after: u64,
        event: Option<&str>,
        matcher: &MessageMatcher,
        deadline: Instant,
    ) -> Waited {
        self.wait_until(after, deadline, |m| {
            event.is_none_or(|e| m.event.as_deref() == Some(e)) && matcher.matches(&m.bytes())
        })
        .await
    }

    /// The first received message after `after` that `wanted` accepts.
    pub async fn wait_until(&self, after: u64, deadline: Instant, wanted: impl Fn(&LoggedMessage) -> bool) -> Waited {
        let mut checked = after;
        loop {
            let notified = self.notify.notified();
            tokio::pin!(notified);
            // Registered before looking, so a message in between still wakes it.
            notified.as_mut().enable();
            {
                let state = lock(&self.state);
                // Messages dropped before they were looked at: a match may be among them.
                let missed = state.messages.front().is_some_and(|m| m.id > checked + 1);
                let from = checked;
                for m in state.messages.iter().filter(|m| m.id > from) {
                    checked = m.id;
                    if m.direction == Direction::Received && wanted(m) {
                        return Waited::Found(m.clone());
                    }
                }
                if missed {
                    return Waited::Overrun;
                }
                if let Some(why) = &state.ended {
                    return Waited::Ended(why.clone());
                }
            }
            tokio::select! {
                _ = notified => {}
                _ = tokio::time::sleep_until(deadline) => return Waited::TimedOut,
            }
        }
    }
}

// ---- on-connect steps -----------------------------------------------------------------------

/// The outcome of one step: a wait, or a step that failed.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StepResult {
    pub name: String,
    pub passed: bool,
    /// Not run, because an earlier step failed.
    pub skipped: bool,
    pub error: Option<String>,
    /// Waits: how long it took, in milliseconds.
    pub ms: Option<f64>,
    /// Waits: the message that matched.
    pub message: Option<LoggedMessage>,
}

impl StepResult {
    fn failed(name: String, error: String, ms: Option<f64>) -> Self {
        Self { name, passed: false, skipped: false, error: Some(error), ms, message: None }
    }

    pub fn test(&self) -> zorvik_script::TestResult {
        zorvik_script::TestResult {
            name: self.name.clone(),
            passed: self.passed,
            skipped: self.skipped,
            error: self.error.clone(),
        }
    }
}

/// What the steps of a connection work with.
pub(crate) struct Steps<'a> {
    pub steps: &'a [ConnectStep],
    pub messages: &'a [SavedMessage],
    pub log: &'a MessageLog,
    /// Sends a saved message (variables filled in).
    pub send: &'a (dyn Fn(&SavedMessage) -> Result<(), String> + Send + Sync),
    /// A line for the message log (`true`: something went wrong).
    pub note: &'a (dyn Fn(String, bool) + Send + Sync),
    /// Starts a repeating send ("keep sending").
    pub repeat: &'a (dyn Fn(&SavedMessage, Duration) + Send + Sync),
    pub cancel: &'a CancellationToken,
}

/// What a wait looks for, in words: `a message containing "hi"`, or for Socket.IO
/// `the event "joined" where $[0].room == lobby`.
pub(crate) fn wait_target(wait: &WaitStep) -> String {
    let event = wait.event.trim();
    if event.is_empty() {
        return describe(wait.matcher, &wait.pattern);
    }
    let args = match wait.matcher {
        MatchKind::Any => return format!("the event \"{event}\""),
        MatchKind::Contains if wait.pattern.is_empty() => return format!("the event \"{event}\""),
        MatchKind::Contains => format!("arguments containing \"{}\"", wait.pattern),
        MatchKind::Exact => format!("arguments exactly {}", wait.pattern),
        MatchKind::Regex => format!("arguments matching /{}/", wait.pattern),
        MatchKind::Json => format!("arguments where {}", wait.pattern.trim()),
    };
    format!("the event \"{event}\" with {args}")
}

pub(crate) fn wait_name(wait: &WaitStep) -> String {
    format!("Wait for {}", wait_target(wait))
}

/// Run the steps in order; the first failure skips the rest. Results: every wait, and the
/// step that failed.
pub(crate) async fn run_steps(s: Steps<'_>) -> Vec<StepResult> {
    let mut results = Vec::new();
    let mut failed = false;
    // Replies count from the last message sent, so a reply that comes before the wait starts counts.
    let mut cursor = 0;
    let message = |name: &str| {
        s.messages.iter().find(|m| m.name == name).ok_or_else(|| format!("There's no saved message named '{name}'"))
    };
    for step in s.steps {
        if s.cancel.is_cancelled() {
            break;
        }
        match step {
            ConnectStep::Send { message: name } => {
                if failed {
                    continue;
                }
                let sent = message(name).and_then(|m| {
                    cursor = s.log.last_id();
                    (s.send)(m)
                });
                match sent {
                    Ok(()) => (s.note)(format!("On connect: sent {name}"), false),
                    Err(e) => {
                        (s.note)(format!("On connect: could not send {name}: {e}"), true);
                        results.push(StepResult::failed(format!("Send {name}"), e, None));
                        failed = true;
                    }
                }
            }
            ConnectStep::Wait(wait) => {
                let name = wait_name(wait);
                if failed {
                    results.push(StepResult {
                        name,
                        passed: false,
                        skipped: true,
                        error: None,
                        ms: None,
                        message: None,
                    });
                    continue;
                }
                let target = wait_target(wait);
                let matcher = match MessageMatcher::new(wait.matcher, wait.pattern.as_bytes()) {
                    Ok(m) => m,
                    Err(e) => {
                        (s.note)(format!("On connect: can't wait for {target}: {e}"), true);
                        results.push(StepResult::failed(name, e, None));
                        failed = true;
                        continue;
                    }
                };
                let event = Some(wait.event.trim()).filter(|e| !e.is_empty());
                let limit = Duration::from_millis(wait.timeout_ms.max(1)).min(MAX_WAIT);
                let started = Instant::now();
                let waited = tokio::select! {
                    w = s.log.wait_for(cursor, event, &matcher, started + limit) => w,
                    _ = s.cancel.cancelled() => break,
                };
                let ms = started.elapsed().as_secs_f64() * 1000.0;
                let error = match waited {
                    Waited::Found(m) => {
                        cursor = m.id;
                        (s.note)(format!("On connect: got {target} after {} ms", ms.round()), false);
                        results.push(StepResult {
                            name,
                            passed: true,
                            skipped: false,
                            error: None,
                            ms: Some(ms),
                            message: Some(m),
                        });
                        continue;
                    }
                    Waited::TimedOut => format!("Nothing matched within {} ms", limit.as_millis()),
                    Waited::Ended(why) => format!("{why} before a message matched"),
                    Waited::Overrun => OVERRUN.to_string(),
                };
                (s.note)(format!("On connect: waited for {target}: {error}"), true);
                results.push(StepResult::failed(name, error, Some(ms)));
                failed = true;
            }
            ConnectStep::Repeat(repeat) => {
                if failed {
                    continue;
                }
                match message(&repeat.message) {
                    Ok(m) => (s.repeat)(m, Duration::from_millis(repeat.every_ms).max(MIN_REPEAT)),
                    Err(e) => {
                        (s.note)(format!("On connect: can't keep sending {}: {e}", repeat.message), true);
                        results.push(StepResult::failed(format!("Keep sending {}", repeat.message), e, None));
                        failed = true;
                    }
                }
            }
        }
    }
    results
}

// ---- for collection runs --------------------------------------------------------------------

/// Collection runs: the steps without repeating sends (the connection closes when the steps
/// end), their notes for the console instead of the log.
pub(crate) async fn run_steps_once(
    steps: &[ConnectStep],
    messages: &[SavedMessage],
    log: &MessageLog,
    send: &(dyn Fn(&SavedMessage) -> Result<(), String> + Send + Sync),
) -> (Vec<StepResult>, Vec<String>) {
    let notes = std::sync::Mutex::new(Vec::new());
    let note = |text: String, _error: bool| lock(&notes).push(text);
    let repeat = |message: &SavedMessage, _every: Duration| {
        lock(&notes).push(format!(
            "Keep sending {}: not in collection runs (the connection closes after the steps)",
            message.name
        ));
    };
    let cancel = CancellationToken::new();
    let s = Steps { steps, messages, log, send, note: &note, repeat: &repeat, cancel: &cancel };
    let results = run_steps(s).await;
    (results, notes.into_inner().unwrap_or_default())
}

/// The messages of a log as a JSON transcript (a run's response body).
pub(crate) fn transcript(log: &MessageLog) -> Vec<u8> {
    let (messages, _, _) = log.since(0, MAX_LOG_MESSAGES);
    let list: Vec<serde_json::Value> = messages
        .iter()
        .map(|m| {
            let mut entry = serde_json::json!({ "direction": m.direction, "size": m.size, "timestamp": m.timestamp });
            if let Some(event) = &m.event {
                entry["event"] = serde_json::Value::String(event.clone());
            }
            match (&m.text, &m.base64) {
                (Some(text), _) => entry["text"] = serde_json::Value::String(text.clone()),
                (None, Some(b64)) => entry["base64"] = serde_json::Value::String(b64.clone()),
                _ => {}
            }
            entry
        })
        .collect();
    serde_json::to_vec_pretty(&list).unwrap_or_default()
}

/// A connection that is open: its log and what its on-connect steps are doing.
pub(crate) struct Connected {
    pub log: std::sync::Arc<MessageLog>,
    /// The on-connect steps, when there are any.
    pub steps: Option<tokio::task::JoinHandle<Vec<StepResult>>>,
}

// ---- repeating sends ------------------------------------------------------------------------

/// Send `message` every `every` (the first at once) until `stop`; `send` fills in its variables
/// each time. A failed send stops it and is told to `failed`.
pub(crate) fn spawn_repeat(
    message: SavedMessage,
    every: Duration,
    stop: CancellationToken,
    send: impl Fn(&SavedMessage) -> Result<(), String> + Send + 'static,
    failed: impl FnOnce(String) + Send + 'static,
) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(every);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                biased;
                _ = stop.cancelled() => return,
                _ = tick.tick() => {}
            }
            if let Err(e) = send(&message) {
                let name = if message.name.is_empty() { "the message".to_string() } else { message.name.clone() };
                failed(format!("Stopped sending {name} every {}: {e}", every_text(every)));
                return;
            }
        }
    });
}

/// "500 ms", "5 s", "1.5 s".
pub(crate) fn every_text(every: Duration) -> String {
    let ms = every.as_millis();
    if ms < 1000 || !ms.is_multiple_of(100) { format!("{ms} ms") } else { format!("{} s", ms as f64 / 1000.0) }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use zorvik_workspace::formats::{MessageFormat, WaitStep};

    use super::*;

    fn received(log: &MessageLog, text: &str, event: Option<&str>) {
        let (text, event) = (text.to_string(), event.map(str::to_string));
        log.push(NewMessage {
            direction: Direction::Received,
            text: Some(&text),
            base64: None,
            size: text.len() as u64,
            timestamp: 0.0,
            event: event.as_ref(),
            detail: None,
        });
    }

    fn saved(name: &str, text: &str) -> SavedMessage {
        SavedMessage { name: name.into(), format: MessageFormat::Json, text: text.into(), ..Default::default() }
    }

    #[tokio::test]
    async fn waits_count_from_the_last_send_and_the_first_failure_skips_the_rest() {
        let log = Arc::new(MessageLog::default());
        received(&log, "welcome", None);
        let messages = vec![saved("Hi", r#"{"id": 1}"#), saved("Raw", "01 02")];
        let steps = vec![
            ConnectStep::Wait(WaitStep { matcher: MatchKind::Exact, pattern: "welcome".into(), ..Default::default() }),
            ConnectStep::Send { message: "Hi".into() },
            ConnectStep::Wait(WaitStep { matcher: MatchKind::Json, pattern: "$.id == 1".into(), ..Default::default() }),
            ConnectStep::Send { message: "Raw".into() },
            ConnectStep::Wait(WaitStep { pattern: "never".into(), timeout_ms: 50, ..Default::default() }),
            ConnectStep::Send { message: "Hi".into() },
            ConnectStep::Wait(WaitStep::default()),
        ];
        let sent = std::sync::Mutex::new(Vec::new());
        let send = |m: &SavedMessage| {
            lock(&sent).push(m.name.clone());
            // The reply comes back before the wait starts.
            received(&log, &m.text, None);
            Ok(())
        };
        let note = |_: String, _: bool| {};
        let repeat = |_: &SavedMessage, _: Duration| {};
        let cancel = CancellationToken::new();
        let s = Steps {
            steps: &steps,
            messages: &messages,
            log: &log,
            send: &send,
            note: &note,
            repeat: &repeat,
            cancel: &cancel,
        };
        let results = run_steps(s).await;
        let summary: Vec<(bool, bool)> = results.iter().map(|r| (r.passed, r.skipped)).collect();
        assert_eq!(summary, [(true, false), (true, false), (false, false), (false, true)], "{results:?}");
        assert_eq!(results[1].message.as_ref().and_then(|m| m.text.as_deref()), Some(r#"{"id": 1}"#));
        assert_eq!(results[2].error.as_deref(), Some("Nothing matched within 50 ms"));
        assert_eq!(results[2].name, "Wait for a message containing \"never\"");
        assert_eq!(sent.into_inner().unwrap(), ["Hi", "Raw"]);
    }

    #[tokio::test]
    async fn socketio_waits_look_at_one_event() {
        let log = MessageLog::default();
        received(&log, r#"[{"room":"a"}]"#, Some("joined"));
        received(&log, r#"[{"room":"lobby"}]"#, Some("left"));
        received(&log, r#"[{"room":"lobby"}]"#, Some("joined"));
        let m = MessageMatcher::new(MatchKind::Json, b"$[0].room == lobby").unwrap();
        match log.wait_for(0, Some("joined"), &m, Instant::now() + Duration::from_millis(50)).await {
            Waited::Found(found) => assert_eq!(found.id, 3),
            _ => panic!("expected the third message"),
        }
        let wait = WaitStep {
            event: "joined".into(),
            matcher: MatchKind::Json,
            pattern: "$[0].room == lobby".into(),
            ..Default::default()
        };
        assert_eq!(wait_name(&wait), "Wait for the event \"joined\" with arguments where $[0].room == lobby");
        assert_eq!(
            wait_name(&WaitStep { event: "joined".into(), ..Default::default() }),
            "Wait for the event \"joined\""
        );
    }

    #[tokio::test]
    async fn waits_end_with_the_connection_and_the_log_is_bounded() {
        let log = Arc::new(MessageLog::default());
        let waiting = {
            let log = log.clone();
            tokio::spawn(async move {
                let m = MessageMatcher::new(MatchKind::Any, b"").unwrap();
                log.wait_for(0, None, &m, Instant::now() + Duration::from_secs(5)).await
            })
        };
        tokio::time::sleep(Duration::from_millis(20)).await;
        log.record_ws(&WsEvent::closed(Some(1008), "nope", false));
        match waiting.await.unwrap() {
            Waited::Ended(why) => assert_eq!(why, "The server closed the connection (code 1008 · nope)"),
            _ => panic!("expected the end"),
        }
        let log = MessageLog::default();
        for i in 0..MAX_LOG_MESSAGES + 5 {
            received(&log, &i.to_string(), None);
        }
        let (messages, gone, last) = log.since(0, 3);
        assert_eq!(last, 1005);
        assert_eq!(messages.iter().map(|m| m.id).collect::<Vec<_>>(), [1003, 1004, 1005]);
        assert_eq!(gone, 1002);
        assert_eq!(log.since(1004, 10).0.len(), 1);
        // A wait that fell behind what the log keeps says so, instead of missing a match.
        let m = MessageMatcher::new(MatchKind::Exact, b"0").unwrap();
        assert!(matches!(log.wait_for(0, None, &m, Instant::now()).await, Waited::Overrun));
    }

    #[tokio::test]
    async fn a_repeat_that_cannot_send_stops_and_says_so() {
        // The composer's repeat has no name; the UI knows its repeat stopped by this text
        // (app/src/store/composer.ts).
        let (tx, rx) = tokio::sync::oneshot::channel();
        let message = SavedMessage { text: "hi".into(), ..Default::default() };
        let send = |_: &SavedMessage| Err("broken pipe".to_string());
        spawn_repeat(message, Duration::from_millis(10), CancellationToken::new(), send, move |why| {
            let _ = tx.send(why);
        });
        let why = tokio::time::timeout(Duration::from_secs(5), rx).await.unwrap().unwrap();
        assert_eq!(why, "Stopped sending the message every 10 ms: broken pipe");
    }
}

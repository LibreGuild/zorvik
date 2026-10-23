//! Message scripts of WebSocket and Socket.IO requests (`onMessage`): JavaScript that runs on
//! each message that matches (its direction, Socket.IO event and pattern), with the message as
//! `pm.message` and `pm.connection` to send replies. One worker per connection runs them in
//! order, off the async workers; what they report goes to the connection's log as notes (live
//! connections) or to the run's tests and console (collection runs). Variables they set carry
//! from one message to the next, like the scripts of one run.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use zorvik_engine::{Direction, SocketEvent, WsEvent, WsMessageKind};
use zorvik_script::{ConsoleLevel, Event, Host, Info, Limits, ScriptMessage, ScriptSend, TestResult};
use zorvik_workspace::formats::matching::MessageMatcher;
use zorvik_workspace::formats::{MatchKind, MessageDirection, MessageScript, Request, RequestKind};

use crate::lock;
use crate::scripting::{ChainScript, ScriptVars, request_snapshot, run_chain, script_input};

/// Messages waiting for their scripts, and their bytes; more are skipped (and counted) until the
/// worker catches up.
const BACKLOG: usize = 1000;
const BACKLOG_BYTES: usize = 32 << 20;
/// The largest message scripts run on (each run copies it into a sandbox of 64 MB).
const MAX_MESSAGE: usize = 8 << 20;
/// How often a live connection's scripts keep the variables they set, at most.
const KEEP_EVERY: Duration = Duration::from_secs(1);
/// Sends of scripts remembered, so their own messages don't run "sent" scripts again.
const OWN_SENDS: usize = 200;

struct Entry {
    script: MessageScript,
    /// The pattern's matcher, or why it can't match.
    matcher: Result<MessageMatcher, String>,
    label: String,
}

/// The message scripts of one connection, and the variables they share.
pub(crate) struct MessageScripts {
    entries: Vec<Entry>,
    owner: String,
    info: Info,
    request: zorvik_script::ScriptRequest,
    vars: std::sync::Mutex<ScriptVars>,
    limits: Limits,
    host: Option<Arc<dyn Host>>,
}

/// What a connection's message scripts reported.
#[derive(Default)]
pub(crate) struct Scripted {
    /// Their tests (collection runs).
    pub tests: Vec<TestResult>,
    /// Their console lines and errors (collection runs).
    pub notes: Vec<String>,
}

/// "On a message containing \"ping\"", "On the event \"joined\"", or the script's name.
fn label_of(script: &MessageScript) -> String {
    if !script.name.trim().is_empty() {
        return script.name.trim().to_string();
    }
    let (noun, also) = match script.direction {
        MessageDirection::Received => ("message", ""),
        MessageDirection::Sent => ("sent message", ""),
        MessageDirection::Both => ("message", " (sent or received)"),
    };
    let p = &script.pattern;
    let any = script.matcher == MatchKind::Any || (script.matcher == MatchKind::Contains && p.is_empty());
    let event = script.event.trim();
    if !event.is_empty() {
        let which = if script.direction == MessageDirection::Sent { "sent event" } else { "event" };
        let args = match script.matcher {
            _ if any => String::new(),
            MatchKind::Contains => format!(" with arguments containing \"{p}\""),
            MatchKind::Exact => format!(" with arguments exactly {p}"),
            MatchKind::Regex => format!(" with arguments matching /{p}/"),
            MatchKind::Json => format!(" with arguments where {}", p.trim()),
            MatchKind::Any => String::new(),
        };
        return format!("On the {which} \"{event}\"{args}{also}");
    }
    let target = match script.matcher {
        _ if any => format!("any {noun}"),
        MatchKind::Contains => format!("a {noun} containing \"{p}\""),
        MatchKind::Exact => format!("the {noun} \"{p}\""),
        MatchKind::Regex => format!("a {noun} matching /{p}/"),
        MatchKind::Json => format!("a JSON {noun} where {}", p.trim()),
        MatchKind::Any => format!("any {noun}"),
    };
    format!("On {target}{also}")
}

impl MessageScripts {
    /// The request's enabled message scripts (none for other kinds, or without any).
    pub fn new(
        request: &Request,
        path: Option<&str>,
        info: &Info,
        vars: &ScriptVars,
        limits: Limits,
        host: Option<Arc<dyn Host>>,
    ) -> Option<Arc<Self>> {
        let scripts = match request.kind {
            RequestKind::Websocket => &request.websocket.on_message,
            RequestKind::SocketIo => &request.socketio.on_message,
            _ => return None,
        };
        let entries: Vec<Entry> = scripts
            .iter()
            .filter(|s| s.enabled && !s.script.trim().is_empty())
            .map(|s| Entry {
                matcher: MessageMatcher::new(s.matcher, s.pattern.as_bytes()),
                label: label_of(s),
                script: s.clone(),
            })
            .collect();
        if entries.is_empty() {
            return None;
        }
        let mut info = info.clone();
        if info.request_id.is_empty() {
            info.request_id = path.unwrap_or_default().to_string();
        }
        Some(Arc::new(Self {
            entries,
            owner: request.name.clone(),
            info,
            request: request_snapshot(request),
            vars: std::sync::Mutex::new(ScriptVars { changes: Vec::new(), ..vars.clone() }),
            limits,
            host,
        }))
    }

    /// The scripts that run on `m`.
    fn matching(&self, m: &ScriptMessage) -> Vec<&Entry> {
        let sent = m.direction == "sent";
        let bytes = match (&m.text, &m.base64) {
            (Some(text), _) => text.as_bytes().to_vec(),
            (None, Some(b64)) => base64::engine::general_purpose::STANDARD.decode(b64).unwrap_or_default(),
            _ => Vec::new(),
        };
        self.entries
            .iter()
            .filter(|e| match e.script.direction {
                MessageDirection::Received => !sent,
                MessageDirection::Sent => sent,
                MessageDirection::Both => true,
            })
            .filter(|e| {
                let event = e.script.event.trim();
                event.is_empty() || m.event.as_deref() == Some(event)
            })
            .filter(|e| e.matcher.as_ref().is_ok_and(|matcher| matcher.matches(&bytes)))
            .collect()
    }

    /// Patterns that can't match, said once when the connection opens.
    fn problems(&self) -> Vec<String> {
        self.entries
            .iter()
            .filter_map(|e| e.matcher.as_ref().err().map(|why| format!("{}: never runs, {why}", e.label)))
            .collect()
    }

    /// Variables the scripts changed that are kept (environment, workspace, globals), taken out.
    pub fn take_changes(&self) -> ScriptVars {
        let mut vars = lock(&self.vars);
        let changes = std::mem::take(&mut vars.changes);
        ScriptVars { changes, ..vars.clone() }
    }

    /// Collection runs: what the scripts set goes on to the next requests of the run.
    pub fn merge_into(&self, vars: &mut ScriptVars) {
        let mine = self.take_changes();
        vars.values = mine.values;
        vars.changes.extend(mine.changes);
    }

    /// Run the scripts that match `m`, one after the other; what they sent goes out through `send`.
    async fn run(
        &self,
        m: &ScriptMessage,
        send: &(dyn Fn(&ScriptSend) -> Result<(), String> + Sync),
        out: &(dyn Fn(Report) + Sync),
    ) {
        for entry in self.matching(m) {
            let (input, has_environment) = {
                let vars = lock(&self.vars);
                let mut input = script_input(Event::Message, &self.info, &vars, self.request.clone(), None);
                input.message = Some(m.clone());
                (input, vars.environment.is_some())
            };
            let script = ChainScript {
                label: entry.label.clone(),
                owner: self.owner.clone(),
                code: entry.script.script.clone(),
            };
            let ran =
                run_chain(Event::Message, vec![script], input, has_environment, self.limits, self.host.clone()).await;
            let outcome = match ran {
                Ok(outcome) => outcome,
                Err(e) => {
                    out(Report::Line(format!("{}: {}", entry.label, e.message), true));
                    continue;
                }
            };
            {
                let mut vars = lock(&self.vars);
                vars.values = outcome.values;
                vars.changes.extend(outcome.changes);
            }
            for line in outcome.report.console {
                let error = matches!(line.level, ConsoleLevel::Error | ConsoleLevel::Warn);
                out(Report::Line(format!("{}: {}", entry.label, line.message), error));
            }
            for test in outcome.report.tests {
                out(Report::Test(entry.label.clone(), test));
            }
            for e in outcome.report.errors {
                let at = e.line.map(|l| format!(" at line {l}")).unwrap_or_default();
                out(Report::Line(format!("{} failed{at}: {}", entry.label, e.message), true));
            }
            for s in outcome.sends {
                if let Err(e) = send(&s) {
                    out(Report::Line(format!("{}: could not send: {e}", entry.label), true));
                    break;
                }
            }
            out(Report::Ran);
        }
    }
}

/// What a script run reports, as it happens.
enum Report {
    /// A console line or an error (`true`).
    Line(String, bool),
    Test(String, TestResult),
    /// One script finished (variables may have changed).
    Ran,
}

/// Feeds a connection's messages to its scripts.
#[derive(Clone)]
pub(crate) struct Feed {
    tx: mpsc::Sender<ScriptMessage>,
    /// The direction and event (empty: any) each script runs on.
    wants: Arc<Vec<(MessageDirection, String)>>,
    skipped: Arc<std::sync::atomic::AtomicU64>,
    busy: Arc<Busy>,
}

/// Messages handed to the worker and not done yet.
#[derive(Default)]
struct Busy {
    pending: std::sync::atomic::AtomicUsize,
    /// Bytes of the messages waiting.
    bytes: std::sync::atomic::AtomicUsize,
    /// Messages too large for scripts.
    too_large: std::sync::atomic::AtomicU64,
    done: tokio::sync::Notify,
}

impl Feed {
    fn offer(&self, m: ScriptMessage) {
        use std::sync::atomic::Ordering;
        let weight = weight(&m);
        if weight > MAX_MESSAGE {
            self.busy.too_large.fetch_add(1, Ordering::Relaxed);
            return;
        }
        if self.busy.bytes.load(Ordering::SeqCst) + weight > BACKLOG_BYTES {
            self.skipped.fetch_add(1, Ordering::Relaxed);
            return;
        }
        self.busy.pending.fetch_add(1, Ordering::SeqCst);
        self.busy.bytes.fetch_add(weight, Ordering::SeqCst);
        if self.tx.try_send(m).is_err() {
            self.busy.pending.fetch_sub(1, Ordering::SeqCst);
            self.busy.bytes.fetch_sub(weight, Ordering::SeqCst);
            self.skipped.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Whether a script may run on a message of this direction and Socket.IO event (checked
    /// before the message is copied).
    fn wanted(&self, sent: bool, event: Option<&str>) -> bool {
        self.wants.iter().any(|(direction, wanted)| {
            let way = match direction {
                MessageDirection::Received => !sent,
                MessageDirection::Sent => sent,
                MessageDirection::Both => true,
            };
            way && (wanted.is_empty() || event == Some(wanted.as_str()))
        })
    }

    /// Wait until the scripts have run on every message so far (up to `limit`), so what they
    /// send goes out before a run closes the connection.
    pub async fn settled(&self, limit: Duration) {
        let deadline = tokio::time::Instant::now() + limit;
        loop {
            let done = self.busy.done.notified();
            tokio::pin!(done);
            done.as_mut().enable();
            if self.busy.pending.load(std::sync::atomic::Ordering::SeqCst) == 0 {
                return;
            }
            if tokio::time::timeout_at(deadline, done).await.is_err() {
                return;
            }
        }
    }

    /// A WebSocket's text and binary messages.
    pub fn ws(&self, event: &WsEvent) {
        if let WsEvent::Message {
            direction,
            kind: WsMessageKind::Text | WsMessageKind::Binary,
            text,
            base64,
            size,
            timestamp,
        } = event
            && self.wanted(*direction == Direction::Sent, None)
        {
            self.offer(message(*direction, text.clone(), base64.clone(), None, *size, *timestamp));
        }
    }

    /// A Socket.IO session's events (acknowledgements have no event name and don't count).
    pub fn socket(&self, event: &SocketEvent) {
        if let SocketEvent::Message { direction, text, base64, size, timestamp, topic: Some(topic), .. } = event
            && self.wanted(*direction == Direction::Sent, Some(topic))
        {
            self.offer(message(*direction, text.clone(), base64.clone(), Some(topic.clone()), *size, *timestamp));
        }
    }
}

fn weight(m: &ScriptMessage) -> usize {
    m.text.as_ref().map_or(0, String::len) + m.base64.as_ref().map_or(0, String::len)
}

fn message(
    direction: Direction,
    text: Option<String>,
    base64: Option<String>,
    event: Option<String>,
    size: u64,
    timestamp: f64,
) -> ScriptMessage {
    let direction = if direction == Direction::Sent { "sent" } else { "received" };
    ScriptMessage { direction: direction.into(), text, base64, event, size, timestamp }
}

/// A message as compared with what scripts sent: JSON as written by serde (Socket.IO arguments
/// come back re-encoded), else the text.
fn normalized(text: &str) -> String {
    serde_json::from_str::<serde_json::Value>(text).map(|v| v.to_string()).unwrap_or_else(|_| text.to_string())
}

/// Sends a message a script sent, on its connection.
pub(crate) type SendFn = Box<dyn Fn(&ScriptSend) -> Result<(), String> + Send + Sync>;

/// What the worker does with the scripts' results.
pub(crate) struct Sinks {
    /// Sends a message a script sent.
    pub send: SendFn,
    /// A line for the connection's log (`true`: something went wrong).
    pub note: Box<dyn Fn(String, bool) + Send + Sync>,
    /// Keeps what the scripts set on the environment, the workspace and globals (live
    /// connections). Collection runs have none: the run takes the changes (`merge_into`).
    pub keep: Option<Box<dyn Fn(ScriptVars) + Send + Sync>>,
}

/// Start the connection's script worker. It stops with `stop`, or when every [`Feed`] is gone
/// and the messages it holds are done; the handle then gives what the scripts reported.
pub(crate) fn spawn(scripts: Arc<MessageScripts>, sinks: Sinks, stop: CancellationToken) -> (Feed, Worker) {
    let (tx, mut rx) = mpsc::channel::<ScriptMessage>(BACKLOG);
    let skipped = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let busy = Arc::new(Busy::default());
    let wants = scripts.entries.iter().map(|e| (e.script.direction, e.script.event.trim().to_string())).collect();
    let feed = Feed { tx, wants: Arc::new(wants), skipped: skipped.clone(), busy: busy.clone() };
    for problem in scripts.problems() {
        (sinks.note)(problem, true);
    }
    let reported = Arc::new(std::sync::Mutex::new(Scripted::default()));
    let task_reported = reported.clone();
    let handle = tokio::spawn(async move {
        let reported = &*task_reported;
        let own = std::sync::Mutex::new(VecDeque::<String>::new());
        let send = |s: &ScriptSend| {
            let text = normalized(&s.text);
            {
                let mut own = lock(&own);
                own.push_back(text.clone());
                if own.len() > OWN_SENDS {
                    own.pop_front();
                }
            }
            let sent = (sinks.send)(s);
            // Not sent: no echo of it will come.
            if sent.is_err() {
                let mut own = lock(&own);
                if let Some(i) = own.iter().rposition(|t| *t == text) {
                    own.remove(i);
                }
            }
            sent
        };
        let out = |r: Report| match r {
            Report::Line(text, error) => {
                lock(reported).notes.push(text.clone());
                (sinks.note)(text, error);
            }
            Report::Test(label, mut test) => {
                let line = match (&test.error, test.passed, test.skipped) {
                    (_, _, true) => format!("{label}: skipped {}", test.name),
                    (_, true, _) => format!("{label}: ✓ {}", test.name),
                    (Some(e), _, _) => format!("{label}: ✗ {}: {e}", test.name),
                    (None, _, _) => format!("{label}: ✗ {}", test.name),
                };
                (sinks.note)(line, !test.passed && !test.skipped);
                test.name = format!("{label}: {}", test.name);
                lock(reported).tests.push(test);
            }
            Report::Ran => {}
        };
        let (mut told, mut told_large) = (0, 0);
        let mut last_keep = std::time::Instant::now();
        loop {
            let m = tokio::select! {
                biased;
                _ = stop.cancelled() => break,
                m = rx.recv() => match m { Some(m) => m, None => break },
            };
            // Counted done whichever way this round ends.
            let _done = Done(&busy, weight(&m));
            // The scripts' own sends don't run "sent" scripts again (no loops).
            if m.direction == "sent"
                && let Some(text) = &m.text
            {
                let mut own = lock(&own);
                let text = normalized(text);
                if let Some(i) = own.iter().position(|t| *t == text) {
                    own.remove(i);
                    continue;
                }
            }
            scripts.run(&m, &send, &out).await;
            // Kept at most about once a second while messages keep coming (a ticker's every
            // message may set a value), and as soon as the worker is idle.
            if let Some(keep) = &sinks.keep
                && (rx.is_empty() || last_keep.elapsed() >= KEEP_EVERY)
            {
                let changed = scripts.take_changes();
                if !changed.changes.is_empty() {
                    keep(changed);
                    last_keep = std::time::Instant::now();
                }
            }
            let now = skipped.load(std::sync::atomic::Ordering::Relaxed);
            if now > told {
                (sinks.note)(format!("Message scripts fell behind: {} messages were not looked at", now - told), true);
                told = now;
            }
            let large = busy.too_large.load(std::sync::atomic::Ordering::Relaxed);
            if large > told_large {
                let mb = MAX_MESSAGE >> 20;
                (sinks.note)(format!("Message scripts don't run on messages over {mb} MB ({} so far)", large), true);
                told_large = large;
            }
        }
        if let Some(keep) = &sinks.keep {
            let changed = scripts.take_changes();
            if !changed.changes.is_empty() {
                keep(changed);
            }
        }
    });
    (feed, Worker { handle, reported })
}

struct Done<'a>(&'a Busy, usize);

impl Drop for Done<'_> {
    fn drop(&mut self) {
        self.0.pending.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        self.0.bytes.fetch_sub(self.1, std::sync::atomic::Ordering::SeqCst);
        self.0.done.notify_waiters();
    }
}

/// A connection's script worker, and what its scripts reported so far.
pub(crate) struct Worker {
    handle: tokio::task::JoinHandle<()>,
    reported: Arc<std::sync::Mutex<Scripted>>,
}

/// Collection runs: let the script running now finish (up to `limit`), and take what the
/// scripts reported. Messages that came after the steps don't run scripts.
pub(crate) async fn finish(
    feed: Option<Feed>,
    worker: Option<Worker>,
    stop: &CancellationToken,
    limit: Duration,
) -> Scripted {
    drop(feed);
    stop.cancel();
    let Some(Worker { handle, reported }) = worker else { return Scripted::default() };
    let abort = handle.abort_handle();
    let finished = tokio::time::timeout(limit, handle).await.is_ok();
    if !finished {
        abort.abort();
    }
    let mut scripted = std::mem::take(&mut *lock(&reported));
    if !finished {
        scripted.notes.push("Message scripts were still running when the request ended".into());
    }
    scripted
}

impl crate::Api {
    /// Live WebSocket and Socket.IO connections: the pre-request scripts before connecting (they
    /// may change `request` and set variables), and the request's message scripts. Returns the
    /// variables to connect with. What the scripts report goes to `note` (the connection's log).
    pub(crate) async fn live_scripts(
        &self,
        ws: &zorvik_workspace::Workspace,
        request: &mut Request,
        path: Option<&str>,
        note: &(dyn Fn(String, bool) + Sync),
    ) -> crate::ApiResult<(ScriptVars, Option<Arc<MessageScripts>>)> {
        use crate::scripting::{Pre, SendContext, pre_request, script_host};
        // Other live sessions (TCP, UDP, MQTT, subscriptions) run no scripts.
        if !matches!(request.kind, RequestKind::Websocket | RequestKind::SocketIo) {
            return Ok((self.script_vars(ws), None));
        }
        let settings = self.settings();
        let meta = self.revealed_meta(ws);
        let jar = settings.cookie_jar.then(|| self.jar(ws));
        let cx = SendContext {
            ws,
            meta: &meta,
            client: &self.inner.client,
            settings: &settings,
            tokens: &self.inner.tokens,
            jar: jar.as_ref(),
            guard: crate::agents::scope_guard(),
            specs: None,
            // Scripts never start programs.
            programs: crate::mcp::Programs::Approved(None),
            mcp_session: None,
        };
        let folders = path.map(|p| ws.ancestors(p)).unwrap_or_default();
        let info = Info {
            request_name: request.name.clone(),
            request_id: path.unwrap_or_default().to_string(),
            iteration: 0,
            iteration_count: 1,
            location: folders.iter().map(|f| f.name.clone()).chain([request.name.clone()]).collect(),
            current: String::new(),
        };
        let limits = Limits { timeout: settings.script_timeout(), ..Default::default() };
        let mut vars = self.script_vars(ws);
        let url = vars.var_context().render(request.url.trim(), &mut std::collections::BTreeSet::new());
        let host = script_host(&cx, &url)?;
        let pre = Pre { cx: &cx, folders: &folders, info: &info, host: host.clone(), url: &url, limits };
        let ran = pre_request(&pre, request, &mut vars).await;
        if let Some(report) = &ran.report {
            for line in &report.console {
                note(format!("Pre-request script: {}", line.message), line.level == ConsoleLevel::Error);
            }
            for t in &report.tests {
                let mark = if t.skipped {
                    "skipped"
                } else if t.passed {
                    "✓"
                } else {
                    "✗"
                };
                let error = t.error.as_deref().map(|e| format!(": {e}")).unwrap_or_default();
                note(format!("Pre-request script: {mark} {}{error}", t.name), !t.passed && !t.skipped);
            }
        }
        self.keep_script_values(ws, &mut vars);
        ran.result?;
        let scripts = MessageScripts::new(request, path, &info, &vars, limits, host);
        Ok((vars, scripts))
    }

    /// Keep what scripts set on the environment, the workspace and globals, and tell the UI.
    pub(crate) fn keep_script_values(&self, ws: &zorvik_workspace::Workspace, vars: &mut ScriptVars) {
        if vars.changes.is_empty() {
            return;
        }
        if let Some(warning) = self.keep_local_values(ws, vars) {
            tracing::warn!("{warning}");
        }
        vars.changes.clear();
        self.inner.sink.emit(crate::StreamEvent::VariablesChanged {});
    }

    /// Start a live connection's message scripts: their sends go out through `send`, their
    /// notes to `note`; they stop with `stop`.
    pub(crate) fn start_message_scripts(
        &self,
        ws: &zorvik_workspace::Workspace,
        scripts: Arc<MessageScripts>,
        send: SendFn,
        note: Box<dyn Fn(String, bool) + Send + Sync>,
        stop: CancellationToken,
    ) -> Feed {
        let (api, ws) = (self.clone(), ws.clone());
        let keep = Box::new(move |mut vars: ScriptVars| api.keep_script_values(&ws, &mut vars));
        spawn(scripts, Sinks { send, note, keep: Some(keep) }, stop).0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn script(direction: MessageDirection, event: &str, matcher: MatchKind, pattern: &str) -> MessageScript {
        MessageScript {
            direction,
            event: event.into(),
            matcher,
            pattern: pattern.into(),
            script: "1".into(),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn the_feed_takes_only_what_scripts_may_run_on_and_stays_bounded() {
        let mut request = Request::new("Chat", Default::default());
        request.kind = RequestKind::SocketIo;
        request.socketio.on_message = vec![script(MessageDirection::Received, "joined", MatchKind::Any, "")];
        let scripts =
            MessageScripts::new(&request, None, &Info::default(), &ScriptVars::default(), Limits::default(), None)
                .unwrap();
        let sinks = Sinks { send: Box::new(|_| Ok(())), note: Box::new(|_, _| {}), keep: None };
        let stop = CancellationToken::new();
        // A worker that never starts reading: what the feed queues stays queued.
        stop.cancel();
        let (feed, _worker) = spawn(scripts, sinks, stop);
        let event = |direction, topic: &str, text: String| SocketEvent::Message {
            direction,
            text: Some(text),
            base64: None,
            size: 0,
            timestamp: 0.0,
            topic: Some(topic.into()),
            detail: None,
            peer: None,
        };
        let pending = || feed.busy.pending.load(std::sync::atomic::Ordering::SeqCst);
        feed.socket(&event(Direction::Sent, "joined", "[1]".into()));
        feed.socket(&event(Direction::Received, "left", "[1]".into()));
        assert_eq!(pending(), 0, "no script runs on these: they aren't copied");
        feed.socket(&event(Direction::Received, "joined", "[1]".into()));
        assert_eq!(pending(), 1);
        feed.socket(&event(Direction::Received, "joined", "x".repeat(MAX_MESSAGE + 1)));
        assert_eq!(feed.busy.too_large.load(std::sync::atomic::Ordering::Relaxed), 1);
        // Waiting messages are bounded by their bytes too.
        for _ in 0..10 {
            feed.socket(&event(Direction::Received, "joined", "x".repeat(MAX_MESSAGE)));
        }
        assert!(feed.busy.bytes.load(std::sync::atomic::Ordering::SeqCst) <= BACKLOG_BYTES);
        assert!(feed.skipped.load(std::sync::atomic::Ordering::Relaxed) > 0);
    }

    #[test]
    fn labels_say_what_runs_the_script() {
        assert_eq!(label_of(&script(MessageDirection::Received, "", MatchKind::Any, "")), "On any message");
        assert_eq!(
            label_of(&script(MessageDirection::Sent, "", MatchKind::Contains, "ping")),
            "On a sent message containing \"ping\""
        );
        assert_eq!(
            label_of(&script(MessageDirection::Received, "joined", MatchKind::Any, "")),
            "On the event \"joined\""
        );
        assert_eq!(
            label_of(&script(MessageDirection::Sent, "chat", MatchKind::Json, "$[0].to == bob")),
            "On the sent event \"chat\" with arguments where $[0].to == bob"
        );
        let named = MessageScript { name: "Track prices".into(), ..Default::default() };
        assert_eq!(label_of(&named), "Track prices");
    }

    #[test]
    fn scripts_run_on_the_messages_they_match() {
        let mut request = Request::new("Chat", Default::default());
        request.kind = RequestKind::SocketIo;
        request.socketio.on_message = vec![
            script(MessageDirection::Received, "joined", MatchKind::Json, "$[0].room == lobby"),
            script(MessageDirection::Sent, "", MatchKind::Any, ""),
            MessageScript { enabled: false, ..script(MessageDirection::Both, "", MatchKind::Any, "") },
            script(MessageDirection::Received, "", MatchKind::Regex, "("),
        ];
        let scripts =
            MessageScripts::new(&request, None, &Info::default(), &ScriptVars::default(), Limits::default(), None)
                .unwrap();
        assert_eq!(scripts.entries.len(), 3, "the disabled one is left out");
        let m = |direction: &str, event: &str, text: &str| ScriptMessage {
            direction: direction.into(),
            text: Some(text.into()),
            event: Some(event.into()),
            ..Default::default()
        };
        let labels = |m: &ScriptMessage| scripts.matching(m).iter().map(|e| e.label.clone()).collect::<Vec<_>>();
        assert_eq!(
            labels(&m("received", "joined", r#"[{"room":"lobby"}]"#)),
            ["On the event \"joined\" with arguments where $[0].room == lobby"]
        );
        assert!(labels(&m("received", "joined", r#"[{"room":"a"}]"#)).is_empty());
        assert!(labels(&m("received", "left", r#"[{"room":"lobby"}]"#)).is_empty());
        assert_eq!(labels(&m("sent", "chat", "[1]")), ["On any sent message"]);
        assert_eq!(scripts.problems().len(), 1, "a bad regex is said once");
    }
}

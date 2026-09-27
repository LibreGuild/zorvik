//! AI agents (MCP, docs/architecture.md, "AI agents"): the agents connected, the confirmations
//! they wait for in the app, the activity log, and the tools they call
//! (`tools.rs`). The MCP protocol and transport live in `zorvik-mcp`.

mod prompts;
mod redact;
mod tools;

use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use zorvik_engine::HostGuard;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
use ts_rs::TS;
use zorvik_workspace::formats::Request;
use zorvik_workspace::settings::Settings;

pub use prompts::{get_prompt, prompt_definitions};
pub use tools::{ProgressFn, ToolOutput, tool_definitions};

use crate::runner::RunStarted;
use crate::{Api, ApiError, ApiResult, StreamEvent, lock, ok, params};

/// How long an agent's action waits for the user to answer. Under a minute: some
/// agents give up on a tool call after 60 s, and an action approved after that
/// would run although the agent already counted it as failed.
const CONFIRM_TIMEOUT: Duration = Duration::from_secs(55);
/// After "don't allow AI agents", the question is not asked again for this long.
const ENABLE_DECLINED_QUIET: Duration = Duration::from_secs(60);

tokio::task_local! {
    /// Set while an AI agent's tool call runs (see [`scope_guard`]).
    static AGENT_CALL: CallScope;
}

#[derive(Clone)]
struct CallScope {
    /// Hosts the call's requests may reach; `None` = any.
    guard: Option<HostGuard>,
}

/// During an agent's call: the hosts its requests may reach (`None` = any). Every
/// request option the API builds takes it (see `crate::request_options`).
pub(crate) fn scope_guard() -> Option<HostGuard> {
    AGENT_CALL.try_with(|s| s.guard.clone()).ok().flatten()
}

/// An AI agent's call is running (in this task).
pub(crate) fn in_agent_call() -> bool {
    AGENT_CALL.try_with(|_| ()).is_ok()
}

/// Run `work` as (part of) an agent's call, its requests limited by `guard`.
pub(crate) async fn with_guard<F: std::future::Future>(guard: Option<HostGuard>, work: F) -> F::Output {
    AGENT_CALL.scope(CallScope { guard }, work).await
}

/// Allows no host: an agent's call reaches the network only after `gate_traffic`.
pub(crate) fn no_hosts() -> HostGuard {
    HostGuard(Arc::new(|_| false))
}
/// Activity entries kept (the log in the Agents panel).
const MAX_ACTIVITY: usize = 200;

/// An agent connected over MCP.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentSession {
    pub id: String,
    /// Display name, e.g. "Claude Code".
    pub client: String,
    /// Unix epoch milliseconds.
    pub connected_at: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ActivityStatus {
    Running,
    Done,
    Failed,
    /// The user said no, or didn't answer.
    Denied,
}

/// Something in the app an agent action is about (clicking the entry opens it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(export)]
pub enum AgentTarget {
    /// A saved request (path relative to `requests/`).
    #[serde(rename_all = "camelCase")]
    Request {
        path: String,
    },
    /// A folder ("" = the whole collection).
    #[serde(rename_all = "camelCase")]
    Folder {
        path: String,
    },
    #[serde(rename_all = "camelCase")]
    LoadTest {
        id: String,
    },
    #[serde(rename_all = "camelCase")]
    Server {
        id: String,
    },
    /// The runner of a folder ("" = the whole collection).
    #[serde(rename_all = "camelCase")]
    Runner {
        folder: String,
        name: String,
    },
    Environments,
}

/// One tool call in the activity log (updated in place while it runs).
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentActivity {
    pub id: String,
    pub session_id: String,
    pub client: String,
    pub tool: String,
    /// What the agent asked for, in words ("Send GET {{baseUrl}}/users").
    pub title: String,
    pub status: ActivityStatus,
    /// The outcome in a few words, or why it failed.
    pub detail: Option<String>,
    /// Unix epoch milliseconds.
    pub started_at: f64,
    pub duration_ms: Option<f64>,
    pub target: Option<AgentTarget>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ConfirmKind {
    /// Turn on agent access.
    Enable,
    Change,
    Traffic,
    Delete,
    LoadTest,
    Server,
    Workspace,
    File,
}

/// An agent's action waiting for the user's answer.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentConfirm {
    pub id: String,
    pub session_id: String,
    pub client: String,
    pub kind: ConfirmKind,
    pub title: String,
    pub message: String,
    /// What exactly (requests, hosts, the load plan…), one line each.
    pub items: Vec<String>,
    pub confirm_label: String,
    /// "Allow for this session" is offered.
    pub session_option: bool,
    pub danger: bool,
    /// Unix epoch milliseconds; unanswered by then = refused.
    pub expires_at: f64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(export)]
pub enum AgentEvent {
    /// The agents connected now.
    #[serde(rename_all = "camelCase")]
    Sessions { sessions: Vec<AgentSession> },
    /// A new or updated activity entry.
    #[serde(rename_all = "camelCase")]
    Activity { entry: AgentActivity },
    #[serde(rename_all = "camelCase")]
    Confirm { request: AgentConfirm },
    /// Answered, timed out or cancelled: close the dialog.
    #[serde(rename_all = "camelCase")]
    ConfirmClosed { id: String },
    /// Open what the agent worked on (when following; always when `explicit`).
    #[serde(rename_all = "camelCase")]
    Show { target: AgentTarget, explicit: bool },
    /// A request the agent sent and its result (`kind` as in the UI's one-shot results:
    /// `http` = a `SendResult`, `grpc`, `dns`).
    #[serde(rename_all = "camelCase")]
    Response {
        path: Option<String>,
        request: Box<Request>,
        kind: String,
        #[ts(type = "unknown")]
        result: Value,
    },
    /// A collection run the agent started.
    #[serde(rename_all = "camelCase")]
    Run { folder: String, run: RunStarted },
    /// The open workspace changed (another one opened, or the active environment).
    WorkspaceChanged,
}

/// Everything the Agents panel shows (after a window reload).
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentState {
    pub sessions: Vec<AgentSession>,
    /// Oldest first.
    pub activity: Vec<AgentActivity>,
    pub pending: Vec<AgentConfirm>,
}

/// What the user allowed for the rest of a session.
#[derive(Default)]
struct Grants {
    changes: bool,
    /// Hosts approved for the session.
    hosts: HashSet<String>,
    /// Hosts a request was stopped from reaching (a script, redirect or token URL);
    /// the next request asks about them.
    blocked: HashSet<String>,
}

pub(crate) struct SessionState {
    info: AgentSession,
    /// Cancelled when the user disconnects the agent.
    closed: CancellationToken,
    grants: Mutex<Grants>,
    /// One question at a time per agent.
    asking: tokio::sync::Mutex<()>,
    /// When the user last said no to "Allow AI agents?".
    enable_declined: Mutex<Option<Instant>>,
}

/// An agent's connection, held by the MCP server for as long as it lasts.
#[derive(Clone)]
pub struct AgentSessionHandle(Arc<SessionState>);

impl AgentSessionHandle {
    pub fn id(&self) -> &str {
        &self.0.info.id
    }

    /// Cancelled when the user disconnects the agent in the app.
    pub fn closed(&self) -> CancellationToken {
        self.0.closed.clone()
    }
}

struct Pending {
    request: AgentConfirm,
    answer: oneshot::Sender<Answer>,
}

#[derive(Debug, Clone, Copy)]
struct Answer {
    allow: bool,
    for_session: bool,
}

#[derive(Default)]
pub(crate) struct AgentHub {
    sessions: Mutex<Vec<Arc<SessionState>>>,
    activity: Mutex<VecDeque<AgentActivity>>,
    pending: Mutex<Vec<Pending>>,
    /// No app window: nothing can be approved.
    headless: AtomicBool,
    /// One "Allow AI agents?" question at a time.
    enabling: tokio::sync::Mutex<()>,
}

/// What an action needs approved.
pub(crate) struct Ask {
    pub kind: ConfirmKind,
    pub title: String,
    pub message: String,
    pub items: Vec<String>,
    pub confirm_label: String,
    pub session_option: bool,
    pub danger: bool,
}

/// Names agents send in `clientInfo`, as people know them.
fn display_name(client: &str) -> String {
    let known = [
        ("claude-code", "Claude Code"),
        ("claude", "Claude"),
        ("codex", "Codex"),
        ("gemini", "Gemini CLI"),
        ("cursor", "Cursor"),
        ("antigravity", "Antigravity"),
        ("visual studio code", "VS Code"),
        ("vscode", "VS Code"),
        ("windsurf", "Windsurf"),
        ("zed", "Zed"),
    ];
    let lower = client.to_ascii_lowercase();
    // The name is the agent's own claim: it may not pass itself off as the app.
    if lower.contains("zorvik") {
        return "An AI agent".into();
    }
    known.iter().find(|(k, _)| lower.starts_with(k)).map(|(_, name)| name.to_string()).unwrap_or_else(|| {
        match client.trim() {
            "" => "An AI agent".into(),
            // Shown in the title bar and dialogs: keep it short and printable.
            name => name.chars().filter(|c| !c.is_control()).take(40).collect(),
        }
    })
}

impl Api {
    /// Register an agent connection (`client`: its MCP `clientInfo` name or title).
    pub fn agent_connect(&self, client: &str) -> AgentSessionHandle {
        let state = Arc::new(SessionState {
            info: AgentSession {
                id: uuid::Uuid::new_v4().to_string(),
                client: display_name(client),
                connected_at: crate::now_ms() as f64,
            },
            closed: CancellationToken::new(),
            grants: Mutex::default(),
            asking: Default::default(),
            enable_declined: Mutex::default(),
        });
        lock(&self.inner.agents.sessions).push(state.clone());
        self.emit_sessions();
        AgentSessionHandle(state)
    }

    /// The agent's connection ended.
    pub fn agent_disconnect(&self, session: &AgentSessionHandle) {
        session.0.closed.cancel();
        let removed = {
            let mut sessions = lock(&self.inner.agents.sessions);
            let before = sessions.len();
            sessions.retain(|s| !Arc::ptr_eq(s, &session.0));
            before != sessions.len()
        };
        if removed {
            self.emit_sessions();
        }
    }

    /// Tools run without the app window (`zorvik mcp` with headless allowed):
    /// actions that need the user's approval are refused.
    pub fn set_agent_headless(&self, headless: bool) {
        self.inner.agents.headless.store(headless, Ordering::SeqCst);
    }

    fn emit_sessions(&self) {
        let sessions = lock(&self.inner.agents.sessions).iter().map(|s| s.info.clone()).collect();
        self.emit_agent(AgentEvent::Sessions { sessions });
    }

    pub(crate) fn emit_agent(&self, event: AgentEvent) {
        self.inner.sink.emit(StreamEvent::Agent { event });
    }

    /// Change settings from the backend (the UI is told).
    pub(crate) fn update_settings(&self, change: impl FnOnce(&mut Settings)) -> ApiResult<()> {
        let settings = {
            let mut settings = self.inner.settings.write().unwrap_or_else(|e| e.into_inner());
            change(&mut settings);
            settings.save(&self.inner.data_dir.join("settings.json"))?;
            settings.clone()
        };
        self.inner.sink.emit(StreamEvent::SettingsChanged { settings });
        Ok(())
    }

    /// `agent.*` methods (the UI).
    pub(crate) async fn call_agent(&self, method: &str, p: Value) -> ApiResult<Value> {
        match method {
            "agent.state" => ok(AgentState {
                sessions: lock(&self.inner.agents.sessions).iter().map(|s| s.info.clone()).collect(),
                activity: lock(&self.inner.agents.activity).iter().cloned().collect(),
                pending: lock(&self.inner.agents.pending).iter().map(|p| p.request.clone()).collect(),
            }),
            "agent.answer" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct P {
                    id: String,
                    allow: bool,
                    #[serde(default)]
                    for_session: bool,
                }
                let P { id, allow, for_session } = params(p)?;
                let pending = {
                    let mut pending = lock(&self.inner.agents.pending);
                    pending.iter().position(|p| p.request.id == id).map(|i| pending.remove(i))
                };
                let Some(pending) = pending else {
                    return Err(ApiError::new("notFound", "That question was already answered or has expired"));
                };
                let for_session = for_session && pending.request.session_option;
                let _ = pending.answer.send(Answer { allow, for_session });
                self.emit_agent(AgentEvent::ConfirmClosed { id });
                ok(())
            }
            "agent.disconnect" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct P {
                    /// Every agent when missing.
                    session_id: Option<String>,
                }
                let P { session_id } = params(p)?;
                let sessions: Vec<_> = lock(&self.inner.agents.sessions)
                    .iter()
                    .filter(|s| session_id.as_ref().is_none_or(|id| *id == s.info.id))
                    .cloned()
                    .collect();
                for s in sessions {
                    self.agent_disconnect(&AgentSessionHandle(s));
                }
                ok(())
            }
            other => Err(ApiError::new("notFound", format!("Unknown method '{other}'"))),
        }
    }

    /// Ask the user in the app. `Ok(true)`: allowed for the rest of the session.
    pub(crate) async fn agent_ask(
        &self,
        session: &SessionState,
        ask: Ask,
        cancel: &CancellationToken,
    ) -> Result<bool, String> {
        if self.inner.agents.headless.load(Ordering::SeqCst) {
            return Err(format!(
                "{} needs the user's approval in the Zorvik app, which is not open. Ask the user to open Zorvik and try again.",
                ask.title.trim_end_matches('?')
            ));
        }
        let _turn = tokio::select! {
            turn = session.asking.lock() => turn,
            _ = cancel.cancelled() => return Err("Cancelled.".into()),
            _ = session.closed.cancelled() => return Err("The agent was disconnected.".into()),
        };
        let (tx, rx) = oneshot::channel();
        let request = AgentConfirm {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session.info.id.clone(),
            client: session.info.client.clone(),
            kind: ask.kind,
            title: ask.title,
            message: ask.message,
            items: ask.items,
            confirm_label: ask.confirm_label,
            session_option: ask.session_option,
            danger: ask.danger,
            expires_at: crate::now_ms() as f64 + CONFIRM_TIMEOUT.as_millis() as f64,
        };
        let id = request.id.clone();
        lock(&self.inner.agents.pending).push(Pending { request: request.clone(), answer: tx });
        self.emit_agent(AgentEvent::Confirm { request });
        let outcome = tokio::select! {
            answer = rx => answer.ok(),
            _ = tokio::time::sleep(CONFIRM_TIMEOUT) => None,
            _ = session.closed.cancelled() => None,
            _ = cancel.cancelled() => None,
        };
        // Still listed unless answered: drop it and close the dialog.
        let unanswered = {
            let mut pending = lock(&self.inner.agents.pending);
            let before = pending.len();
            pending.retain(|p| p.request.id != id);
            before != pending.len()
        };
        if unanswered {
            self.emit_agent(AgentEvent::ConfirmClosed { id });
        }
        match outcome {
            Some(Answer { allow: true, for_session }) => Ok(for_session),
            Some(_) => Err("The user declined this in Zorvik.".into()),
            None if cancel.is_cancelled() => Err("Cancelled.".into()),
            None if session.closed.is_cancelled() => Err("The user disconnected this agent in Zorvik.".into()),
            None => {
                Err("No answer from the user in Zorvik in time; nothing was done. Ask the user before trying again."
                    .into())
            }
        }
    }

    /// Agent access is on, or the user turns it on now.
    pub(crate) async fn agent_enabled(&self, session: &SessionState, cancel: &CancellationToken) -> Result<(), String> {
        if self.settings().agents.enabled {
            return Ok(());
        }
        let _one = tokio::select! {
            one = self.inner.agents.enabling.lock() => one,
            _ = cancel.cancelled() => return Err("Cancelled.".into()),
        };
        if self.settings().agents.enabled {
            return Ok(());
        }
        let not_allowed = "The user did not allow AI agents in Zorvik (Settings → AI agents turns it on).";
        if lock(&session.enable_declined).is_some_and(|t| t.elapsed() < ENABLE_DECLINED_QUIET) {
            return Err(not_allowed.into());
        }
        let ask = Ask {
            kind: ConfirmKind::Enable,
            title: "Allow AI agents?".into(),
            message: format!(
                "{} wants to control Zorvik: read and edit your collection, send requests and run tests. \
                 Deletes, load tests and servers always ask first; Settings → AI agents sets what else does.",
                session.info.client
            ),
            items: Vec::new(),
            confirm_label: "Allow AI agents".into(),
            session_option: false,
            danger: false,
        };
        self.agent_ask(session, ask, cancel).await.map_err(|e| {
            if e.starts_with("The user declined") {
                *lock(&session.enable_declined) = Some(Instant::now());
                not_allowed.into()
            } else {
                e
            }
        })?;
        self.update_settings(|s| s.agents.enabled = true).map_err(|e| e.message)
    }

    fn activity_start(&self, session: &SessionState, tool: &str, title: String) -> String {
        let entry = AgentActivity {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session.info.id.clone(),
            client: session.info.client.clone(),
            tool: tool.into(),
            title,
            status: ActivityStatus::Running,
            detail: None,
            started_at: crate::now_ms() as f64,
            duration_ms: None,
            target: None,
        };
        let id = entry.id.clone();
        {
            let mut log = lock(&self.inner.agents.activity);
            log.push_back(entry.clone());
            while log.len() > MAX_ACTIVITY {
                log.pop_front();
            }
        }
        self.emit_agent(AgentEvent::Activity { entry });
        id
    }

    fn activity_finish(&self, id: &str, status: ActivityStatus, detail: Option<String>, target: Option<AgentTarget>) {
        let entry = {
            let mut log = lock(&self.inner.agents.activity);
            let Some(entry) = log.iter_mut().find(|e| e.id == id) else { return };
            entry.status = status;
            entry.detail = detail;
            entry.target = target;
            entry.duration_ms = Some(crate::now_ms() as f64 - entry.started_at);
            entry.clone()
        };
        self.emit_agent(AgentEvent::Activity { entry });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_names() {
        assert_eq!(display_name("claude-code"), "Claude Code");
        assert_eq!(display_name("codex-mcp-client"), "Codex");
        assert_eq!(display_name("gemini-cli-mcp-client"), "Gemini CLI");
        assert_eq!(display_name(""), "An AI agent");
        assert_eq!(display_name("my\u{7}agent"), "myagent");
        assert_eq!(display_name("Zorvik"), "An AI agent", "may not pose as the app");
    }
}

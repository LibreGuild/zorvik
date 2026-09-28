//! Request/response payloads of the RPC API (TypeScript bindings are generated).

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use zorvik_engine::{ErrorKind, ResponseMeta, Timing};
use zorvik_workspace::EnvironmentEntry;
use zorvik_workspace::formats::{Request, TreeNode, WorkspaceMeta};

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ApiError {
    /// Machine-readable code (`notFound`, `network`, `invalidInput`, ...).
    pub code: String,
    pub message: String,
    /// Network failure category when `code` is `network`.
    pub network_kind: Option<ErrorKind>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AppInfo {
    pub version: String,
    /// CI build number and commit, when built by CI.
    pub build: Option<String>,
    pub data_dir: String,
    pub platform: String,
    pub dynamic_variables: Vec<String>,
    /// Every dynamic variable with its description and an example (autocomplete, hints).
    pub dynamic_catalog: Vec<zorvik_workspace::dynamic::DynamicVarInfo>,
    /// The `zorvik` command-line tool installed with the app (AI agents run `zorvik mcp`).
    pub cli_path: Option<String>,
    /// Typing `zorvik` in a terminal runs it.
    pub cli_on_path: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RecentWorkspace {
    pub path: String,
    pub name: String,
    #[ts(type = "number")]
    pub opened_at: i64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct WorkspaceInfo {
    pub path: String,
    /// Variables include secret values from the local secret store.
    pub meta: WorkspaceMeta,
    pub tree: Vec<TreeNode>,
    pub environments: Vec<EnvironmentEntry>,
    pub active_environment: Option<String>,
}

/// Response body prepared for display.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ResponseBody {
    /// `text`, `image` or `binary`.
    pub kind: String,
    pub content_type: Option<String>,
    /// Decoded size in bytes.
    #[ts(type = "number")]
    pub size: u64,
    /// Text content (UTF-8, lossy) for text bodies, capped for display.
    pub text: Option<String>,
    /// Pretty-printed JSON when the body is JSON.
    pub pretty: Option<String>,
    /// Base64 data for image previews.
    pub base64: Option<String>,
    /// Only part of the body is shown; save it to a file to see everything.
    pub display_truncated: bool,
    /// The response exceeded the size limit and was cut while downloading.
    pub download_truncated: bool,
    pub decode_warning: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SendResult {
    /// Handle for `response.save`.
    pub response_id: String,
    pub meta: ResponseMeta,
    pub timing: Timing,
    pub body: ResponseBody,
    #[ts(type = "number")]
    pub body_wire_size: u64,
    /// Variables that were referenced but not defined.
    pub unresolved: Vec<String>,
    /// Tests, console output and errors of the request's scripts, when any ran.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub scripts: Option<crate::scripting::ScriptReport>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct StreamOpened {
    pub meta: ResponseMeta,
    pub timing: Timing,
    pub unresolved: Vec<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct VariableInfo {
    pub key: String,
    /// Masked for secrets.
    pub value: String,
    /// `environment`, `workspace` or `globals`.
    pub source: String,
    pub secret: bool,
    /// The value was set by a script and is kept on this computer only.
    pub local: bool,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TokenStatus {
    pub has_token: bool,
    /// First/last characters only.
    pub token_preview: Option<String>,
    #[ts(type = "number | null")]
    pub expires_at: Option<i64>,
    pub scope: Option<String>,
    pub has_refresh_token: bool,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CurlImportResult {
    pub request: Request,
    pub warnings: Vec<String>,
}

/// Events pushed to the UI.
#[allow(clippy::large_enum_variant)] // server traffic is the common case
#[derive(Debug, Clone, Serialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(export)]
pub enum StreamEvent {
    #[serde(rename_all = "camelCase")]
    Ws { conn_id: String, event: zorvik_engine::WsEvent },
    #[serde(rename_all = "camelCase")]
    Sse { conn_id: String, event: SseStreamEvent },
    /// Files in the open workspace changed on disk.
    #[serde(rename_all = "camelCase")]
    WorkspaceChanged { paths: Vec<String> },
    /// Ask the UI to open a URL in the system browser.
    #[serde(rename_all = "camelCase")]
    OpenUrl { url: String },
    /// A TCP/UDP/MQTT client session.
    #[serde(rename_all = "camelCase")]
    Socket { conn_id: String, event: zorvik_engine::SocketEvent },
    /// A running server's traffic, counters or stop.
    #[serde(rename_all = "camelCase")]
    Server { run_id: String, event: zorvik_servers::ServerEvent },
    /// Progress of a network tool run.
    #[serde(rename_all = "camelCase")]
    Tool { run_id: String, event: crate::tools::ToolEvent },
    /// The window is closing while servers run: the UI asks, then calls `quit`.
    #[serde(rename_all = "camelCase")]
    QuitRequested { running: u32, load_test: bool },
    /// A load test's snapshots (4× per second) and final summary.
    #[serde(rename_all = "camelCase")]
    Load { run_id: String, event: zorvik_load::LoadEvent },
    /// A streaming gRPC call.
    #[serde(rename_all = "camelCase")]
    Grpc { session_id: String, event: zorvik_engine::GrpcEvent },
    /// A collection run: started, each result, finished.
    #[serde(rename_all = "camelCase")]
    Runner {
        run_id: String,
        event: crate::runner::RunEvent,
        /// An AI agent started the run (the UI shows it only when following agents).
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        #[ts(optional, as = "Option<bool>")]
        agent: bool,
    },
    /// AI agents: connections, activity, questions for the user, what to show.
    #[serde(rename_all = "camelCase")]
    Agent { event: crate::agents::AgentEvent },
    /// Settings changed in the backend (e.g. the user allowed AI agents).
    #[serde(rename_all = "camelCase")]
    SettingsChanged { settings: zorvik_workspace::settings::Settings },
    /// Training Bootcamp: the lab, progress and what was just earned.
    Academy { update: Box<crate::academy::AcademyUpdate> },
    /// Several events at once, in order (see `BatchingSink`).
    #[serde(rename_all = "camelCase")]
    Batch { events: Vec<StreamEvent> },
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(export)]
pub enum SseStreamEvent {
    #[serde(rename_all = "camelCase")]
    Event { event: zorvik_engine::SseEvent, timestamp: f64 },
    #[serde(rename_all = "camelCase")]
    Error { message: String },
    #[serde(rename_all = "camelCase")]
    Closed { reason: String },
}

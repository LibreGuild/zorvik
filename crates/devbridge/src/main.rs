//! Dev/test only (never shipped): serves the same RPC API as the desktop app
//! over HTTP so the UI can run in a normal browser (Vite dev server,
//! Playwright E2E tests).
//!
//! `POST /bridge/rpc {method, params}` -> `{result}` | `{error}`
//! `GET  /bridge/events` (WebSocket) -> stream of `StreamEvent` JSON
//!
//! Binds to 127.0.0.1 only and rejects cross-site requests (custom header +
//! Host/Origin checks), so a web page cannot drive it.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use clap::Parser;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::broadcast;
use zorvik_api::{Api, BatchingSink, EventSink, StreamEvent};

#[derive(Parser)]
#[command(about = "Zorvik dev bridge (development and E2E tests only)")]
struct Args {
    #[arg(long, default_value_t = 18799)]
    port: u16,
    /// App data dir (settings, history, secrets). Defaults to a fresh temp dir.
    #[arg(long)]
    data_dir: Option<PathBuf>,
    /// Workspace to open on start (created if missing).
    #[arg(long)]
    workspace: Option<PathBuf>,
}

struct BroadcastSink(broadcast::Sender<StreamEvent>);

impl EventSink for BroadcastSink {
    fn emit(&self, event: StreamEvent) {
        let _ = self.0.send(event);
    }
}

#[derive(Clone)]
struct AppState {
    api: Api,
    events: broadcast::Sender<StreamEvent>,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_env_filter("info").init();
    let args = Args::parse();
    let data_dir = args.data_dir.unwrap_or_else(|| {
        let dir = std::env::temp_dir().join(format!("zorvik-devbridge-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        dir
    });
    let (tx, _) = broadcast::channel(4096);
    let api = Api::new(data_dir.clone(), Arc::new(BatchingSink::new(Arc::new(BroadcastSink(tx.clone())))));
    if let Some(ws) = &args.workspace {
        let path = ws.to_string_lossy().to_string();
        let method = if ws.join("zorvik.yaml").exists() { "workspace.open" } else { "workspace.create" };
        let _ = std::fs::create_dir_all(ws);
        if let Err(e) = api.call(method, json!({ "path": path, "name": "E2E Workspace" })).await {
            eprintln!("could not open workspace: {}", e.message);
        }
    }
    // AI agents (`zorvik mcp --data-dir <this data dir>`), like the app.
    let _agents = zorvik_mcp::listener::start(api.clone(), &data_dir).await.expect("agent listener");
    let app = Router::new()
        .route("/bridge/rpc", post(rpc))
        .route("/bridge/events", get(events))
        .route("/bridge/health", get(|| async { "ok" }))
        .with_state(AppState { api, events: tx });
    let addr = SocketAddr::from(([127, 0, 0, 1], args.port));
    let listener = tokio::net::TcpListener::bind(addr).await.expect("bind dev bridge");
    println!("Zorvik dev bridge on http://{addr} (data: {})", data_dir.display());
    axum::serve(listener, app).await.expect("serve");
}

fn local_request(headers: &HeaderMap) -> bool {
    let host = headers.get("host").and_then(|h| h.to_str().ok()).unwrap_or_default();
    let host_ok = ["localhost", "127.0.0.1", "[::1]"].iter().any(|h| host == *h || host.starts_with(&format!("{h}:")));
    let origin_ok = headers
        .get("origin")
        .and_then(|o| o.to_str().ok())
        .is_none_or(|o| o.starts_with("http://localhost:") || o.starts_with("http://127.0.0.1:"));
    host_ok && origin_ok
}

#[derive(Deserialize)]
struct RpcBody {
    method: String,
    #[serde(default)]
    params: Value,
}

async fn rpc(State(state): State<AppState>, headers: HeaderMap, Json(body): Json<RpcBody>) -> Response {
    if !local_request(&headers) || headers.get("x-zorvik-bridge").is_none() {
        return StatusCode::FORBIDDEN.into_response();
    }
    match state.api.call(&body.method, body.params).await {
        Ok(result) => Json(json!({ "result": result })).into_response(),
        Err(error) => Json(json!({ "error": error })).into_response(),
    }
}

async fn events(State(state): State<AppState>, headers: HeaderMap, upgrade: WebSocketUpgrade) -> Response {
    if !local_request(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let rx = state.events.subscribe();
    upgrade.on_upgrade(move |socket| forward(socket, rx))
}

async fn forward(mut socket: WebSocket, mut rx: broadcast::Receiver<StreamEvent>) {
    loop {
        match rx.recv().await {
            Ok(event) => {
                let Ok(text) = serde_json::to_string(&event) else { continue };
                if socket.send(Message::Text(text.into())).await.is_err() {
                    return;
                }
            }
            Err(broadcast::error::RecvError::Lagged(_)) => continue,
            Err(broadcast::error::RecvError::Closed) => return,
        }
    }
}

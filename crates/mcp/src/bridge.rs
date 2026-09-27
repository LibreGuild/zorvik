//! `zorvik mcp`: the MCP server an agent starts (stdio). It answers the
//! handshake, the tool list and prompts itself, so starting an agent never
//! starts Zorvik. The first tool call connects to the running app (or starts
//! it), or runs the tools here when the app is closed and headless use is allowed.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tokio::io::{AsyncBufRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::net::tcp::OwnedWriteHalf;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use zorvik_api::{Api, EventSink, StreamEvent};
use zorvik_workspace::settings::Settings;

use crate::discovery::{AgentFile, proof, same_secret};
use crate::exec::{Executor, MAX_MESSAGE, Out, read_line};
use crate::protocol::{self, DISCONNECTED, METHOD_NOT_FOUND, PARSE_ERROR, id_key};

/// How long to wait for the app to start.
const LAUNCH_TIMEOUT: Duration = Duration::from_secs(30);
/// When the agent goes away: time left to deliver the last answers.
const FLUSH_TIMEOUT: Duration = Duration::from_secs(2);

pub struct BridgeOptions {
    /// The app's data dir (settings, `agent.json`).
    pub data_dir: PathBuf,
    /// Start this instead of the installed app (tests).
    pub app: Option<PathBuf>,
}

/// Serve MCP on stdin/stdout until the agent closes stdin.
pub async fn run(options: BridgeOptions) -> std::io::Result<()> {
    serve(BufReader::new(tokio::io::stdin()), tokio::io::stdout(), options).await
}

/// Serve MCP over any pair of streams (stdio, or a test's pipes).
pub async fn serve<R, W>(mut input: R, mut output: W, options: BridgeOptions) -> std::io::Result<()>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    let writer = tokio::spawn(async move {
        while let Some(line) = rx.recv().await {
            if output.write_all(line.as_bytes()).await.is_err() || output.write_all(b"\n").await.is_err() {
                break;
            }
            let _ = output.flush().await;
        }
    });
    let bridge = Arc::new(Bridge {
        options,
        out: tx,
        client: Mutex::new(String::new()),
        backend: Default::default(),
        revoked: AtomicBool::new(false),
    });
    while let Some(line) = read_line(&mut input, MAX_MESSAGE).await? {
        let line = match line {
            Ok(line) if line.trim().is_empty() => continue,
            Ok(line) => line,
            Err(reason) => {
                bridge.send(protocol::error(&Value::Null, PARSE_ERROR, reason));
                continue;
            }
        };
        match serde_json::from_str::<Value>(&line) {
            Ok(Value::Array(batch)) => batch.into_iter().for_each(|m| bridge.handle(m)),
            Ok(msg) => bridge.handle(msg),
            Err(_) => bridge.send(protocol::error(&Value::Null, PARSE_ERROR, "Not JSON")),
        }
    }
    // The agent is gone: deliver what is ready, but don't wait on calls still running.
    bridge.close();
    drop(bridge);
    let _ = tokio::time::timeout(FLUSH_TIMEOUT, writer).await;
    Ok(())
}

/// Where tool calls go.
enum Backend {
    App(AppConnection),
    Local(Executor),
}

struct AppConnection {
    to_app: mpsc::UnboundedSender<String>,
    /// Tool calls forwarded and not answered yet; `None` once the connection ended
    /// (they were answered with an error then).
    pending: Arc<Mutex<Option<HashSet<String>>>>,
    /// Cancelled when the app side closes.
    closed: CancellationToken,
}

impl AppConnection {
    /// Forward a tool call; `false` when the connection has ended.
    fn forward(&self, id: &Value, msg: &Value) -> bool {
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        let Some(set) = pending.as_mut() else { return false };
        set.insert(id_key(id));
        if self.to_app.send(msg.to_string()).is_err() {
            set.remove(&id_key(id));
            return false;
        }
        true
    }
}

struct Bridge {
    options: BridgeOptions,
    out: Out,
    /// `clientInfo` of `initialize`, shown in the app.
    client: Mutex<String>,
    backend: tokio::sync::Mutex<Option<Arc<Backend>>>,
    /// The user disconnected this agent in the app: no more calls.
    revoked: AtomicBool,
}

impl Bridge {
    fn send(&self, line: String) {
        let _ = self.out.send(line);
    }

    fn handle(self: &Arc<Self>, msg: Value) {
        let method = msg["method"].as_str().unwrap_or_default().to_string();
        let id = msg.get("id").cloned().filter(|id| !id.is_null());
        if msg.get("result").is_some() || msg.get("error").is_some() {
            return; // an answer to a request of ours: we send none
        }
        match (method.as_str(), id) {
            ("initialize", Some(id)) => {
                let info = &msg["params"]["clientInfo"];
                let name = info["title"].as_str().or(info["name"].as_str()).unwrap_or_default();
                *self.client.lock().unwrap_or_else(|e| e.into_inner()) = name.to_string();
                let version = msg["params"]["protocolVersion"].as_str();
                self.send(protocol::result(&id, protocol::initialize_result(version)));
            }
            ("tools/call", Some(id)) => {
                let bridge = self.clone();
                tokio::spawn(async move { bridge.forward_call(id, msg).await });
            }
            ("notifications/cancelled", None) => {
                let bridge = self.clone();
                tokio::spawn(async move {
                    let Some(backend) = bridge.backend.lock().await.clone() else { return };
                    match &*backend {
                        Backend::App(app) => {
                            // A cancelled call gets no answer: nothing to fail later.
                            if let Some(set) = app.pending.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
                                set.remove(&id_key(&msg["params"]["requestId"]));
                            }
                            let _ = app.to_app.send(msg.to_string());
                        }
                        Backend::Local(exec) => exec.handle(msg),
                    }
                });
            }
            (method, Some(id)) => match protocol::answer_locally(method, &msg["params"]) {
                Some(Ok(result)) => self.send(protocol::result(&id, result)),
                Some(Err((code, message))) => self.send(protocol::error(&id, code, &message)),
                None => self.send(protocol::error(&id, METHOD_NOT_FOUND, &format!("Unknown method '{method}'"))),
            },
            _ => {} // notifications/initialized and others
        }
    }

    async fn forward_call(self: Arc<Self>, id: Value, msg: Value) {
        // A connection that just ended is noticed on the first try; the second reconnects.
        for _ in 0..2 {
            let backend = match self.backend().await {
                Ok(b) => b,
                Err(message) => return self.send(protocol::tool_error(&id, &message)),
            };
            match &*backend {
                Backend::App(app) => {
                    if app.forward(&id, &msg) {
                        return;
                    }
                    app.closed.cancel();
                }
                Backend::Local(exec) => return exec.handle(msg),
            }
        }
        self.send(protocol::tool_error(&id, "Zorvik closed the connection; try again."));
    }

    /// The app connection (made now if needed), or local tools when headless.
    async fn backend(self: &Arc<Self>) -> Result<Arc<Backend>, String> {
        if self.revoked.load(Ordering::SeqCst) {
            return Err(REVOKED.into());
        }
        let mut slot = self.backend.lock().await;
        match slot.as_deref() {
            Some(Backend::App(app)) if !app.closed.is_cancelled() => return Ok(slot.clone().expect("set")),
            Some(Backend::Local(exec)) => {
                // Headless until the app opens (then it takes over), and only while allowed.
                if let Some(app) = self.try_app().await {
                    exec.end();
                    let backend = Arc::new(Backend::App(app));
                    *slot = Some(backend.clone());
                    return Ok(backend);
                }
                if headless_allowed(&self.options.data_dir) {
                    return Ok(slot.clone().expect("set"));
                }
                exec.end();
            }
            _ => {}
        }
        *slot = None;
        let backend = Arc::new(self.connect().await?);
        *slot = Some(backend.clone());
        Ok(backend)
    }

    async fn connect(self: &Arc<Self>) -> Result<Backend, String> {
        if let Some(app) = self.try_app().await {
            return Ok(Backend::App(app));
        }
        if headless_allowed(&self.options.data_dir) {
            return Ok(Backend::Local(self.local_executor()));
        }
        launch_app(self.options.app.as_deref()).map_err(|e| {
            format!("Zorvik is not running and could not be started ({e}). Ask the user to open Zorvik.")
        })?;
        let deadline = Instant::now() + LAUNCH_TIMEOUT;
        while Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(300)).await;
            if let Some(app) = self.try_app().await {
                return Ok(Backend::App(app));
            }
        }
        Err("Zorvik did not start within 30 seconds. Ask the user to open it.".into())
    }

    /// Connect to the running app (its `agent.json`), if there is one. Both sides prove
    /// they know the token without sending it, so a stale port reused by another
    /// program learns nothing and can't pose as the app.
    async fn try_app(self: &Arc<Self>) -> Option<AppConnection> {
        let file = AgentFile::read(&self.options.data_dir)?;
        let stream = tokio::time::timeout(Duration::from_secs(3), TcpStream::connect(("127.0.0.1", file.port)))
            .await
            .ok()?
            .ok()?;
        let _ = stream.set_nodelay(true);
        let (read, mut write) = stream.into_split();
        let mut reader = BufReader::new(read);
        let client = self.client.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let nonce = uuid::Uuid::new_v4().simple().to_string();
        let hello = json!({ "client": client, "nonce": nonce, "proof": proof(&file.token, "bridge", &nonce) });
        write.write_all(format!("{hello}\n").as_bytes()).await.ok()?;
        let answer =
            tokio::time::timeout(Duration::from_secs(5), read_line(&mut reader, 64 * 1024)).await.ok()?.ok()??.ok()?;
        let answer: Value = serde_json::from_str(&answer).ok()?;
        let expected = proof(&file.token, "app", &nonce);
        if answer["ok"] != true || !same_secret(answer["proof"].as_str().unwrap_or_default(), &expected) {
            return None;
        }
        let (to_app, from_bridge) = mpsc::unbounded_channel::<String>();
        let pending: Arc<Mutex<Option<HashSet<String>>>> = Arc::new(Mutex::new(Some(HashSet::new())));
        let closed = CancellationToken::new();
        tokio::spawn(pump_to_app(write, from_bridge, closed.clone()));
        let (bridge, waiting, ended) = (self.clone(), pending.clone(), closed.clone());
        tokio::spawn(async move {
            loop {
                let line = tokio::select! {
                    line = read_line(&mut reader, MAX_MESSAGE) => line,
                    _ = ended.cancelled() => break,
                };
                let Ok(Some(line)) = line else { break };
                let Ok(line) = line else { continue };
                if let Ok(msg) = serde_json::from_str::<Value>(&line) {
                    if msg["method"] == DISCONNECTED {
                        bridge.revoked.store(true, Ordering::SeqCst);
                        continue;
                    }
                    if let Some(id) = msg.get("id")
                        && let Some(set) = waiting.lock().unwrap_or_else(|e| e.into_inner()).as_mut()
                    {
                        set.remove(&id_key(id));
                    }
                }
                bridge.send(line);
            }
            // The app went away: calls still waiting fail (the next call reconnects).
            ended.cancel();
            let ids = waiting.lock().unwrap_or_else(|e| e.into_inner()).take().unwrap_or_default();
            let message =
                if bridge.revoked.load(Ordering::SeqCst) { REVOKED } else { "Zorvik closed before the call finished." };
            for key in ids {
                if let Ok(id) = serde_json::from_str::<Value>(&key) {
                    bridge.send(protocol::tool_error(&id, message));
                }
            }
        });
        Some(AppConnection { to_app, pending, closed })
    }

    /// Tools run here, on the app's data, with nothing to approve them.
    fn local_executor(&self) -> Executor {
        let api = Api::new(self.options.data_dir.clone(), Arc::new(NoEvents));
        api.set_agent_headless(true);
        api.restore_last_workspace();
        let client = self.client.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let session = api.agent_connect(&client);
        Executor::new(api, session, self.out.clone())
    }

    fn close(&self) {
        if let Ok(slot) = self.backend.try_lock()
            && let Some(backend) = slot.as_ref()
        {
            match &**backend {
                Backend::Local(exec) => exec.end(),
                Backend::App(app) => app.closed.cancel(),
            }
        }
    }
}

const REVOKED: &str = "The user disconnected this agent in Zorvik. Stop using Zorvik unless the user asks you to reconnect \
     (restart this MCP server).";

/// Headless use is allowed right now (the settings may change while the agent runs).
fn headless_allowed(data_dir: &Path) -> bool {
    let settings = Settings::load(&data_dir.join("settings.json"));
    settings.agents.enabled && settings.agents.headless
}

async fn pump_to_app(mut write: OwnedWriteHalf, mut rx: mpsc::UnboundedReceiver<String>, closed: CancellationToken) {
    loop {
        let line = tokio::select! {
            line = rx.recv() => line,
            _ = closed.cancelled() => None,
        };
        let Some(line) = line else { break };
        if write.write_all(line.as_bytes()).await.is_err() || write.write_all(b"\n").await.is_err() {
            closed.cancel();
            break;
        }
    }
}

struct NoEvents;

impl EventSink for NoEvents {
    fn emit(&self, _event: StreamEvent) {}

    fn open_url(&self, _url: &str) -> Result<(), String> {
        Err("No browser in headless mode".into())
    }
}

/// Start the Zorvik app next to this executable (or the one given).
fn launch_app(app: Option<&Path>) -> std::io::Result<()> {
    let mut cmd = match app {
        Some(app) => std::process::Command::new(app),
        None => installed_app()?,
    };
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    // Its own process group (Unix) / no console (Windows): it keeps running when the agent stops us.
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    #[cfg(windows)]
    {
        std::os::windows::process::CommandExt::creation_flags(&mut cmd, 0x0000_0008 | 0x0000_0200);
        keep_stdio_from_children();
    }
    cmd.spawn().map(drop)
}

/// Windows children inherit every inheritable handle: the app would hold the agent's
/// pipes open (the agent never sees our stdout close) unless ours are marked private.
#[cfg(windows)]
fn keep_stdio_from_children() {
    use windows_sys::Win32::Foundation::{HANDLE_FLAG_INHERIT, SetHandleInformation};
    use windows_sys::Win32::System::Console::{GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE};
    for which in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        // SAFETY: plain Win32 calls on this process's own standard handles; failures are harmless.
        unsafe {
            let handle = GetStdHandle(which);
            if !handle.is_null() {
                SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0);
            }
        }
    }
}

fn installed_app() -> std::io::Result<std::process::Command> {
    let exe = std::env::current_exe()?;
    #[cfg(target_os = "macos")]
    {
        // Inside `Zorvik.app/Contents/MacOS/`: open the bundle; elsewhere by its identifier.
        let mut cmd = std::process::Command::new("open");
        match exe.ancestors().find(|p| p.extension().is_some_and(|e| e == "app")) {
            Some(bundle) => cmd.arg(bundle),
            None => cmd.args(["-b", crate::discovery::APP_IDENTIFIER]),
        };
        Ok(cmd)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let dir = exe.parent().ok_or_else(|| std::io::Error::other("no folder"))?;
        // The app is `zorvik-desktop` (`zorvik` is this CLI, and Windows names ignore case).
        let names: &[&str] = if cfg!(windows) { &["zorvik-desktop.exe"] } else { &["zorvik-desktop"] };
        names
            .iter()
            .map(|n| dir.join(n))
            .find(|p| p.is_file())
            .map(std::process::Command::new)
            .ok_or_else(|| std::io::Error::other(format!("the Zorvik app is not in {}", dir.display())))
    }
}

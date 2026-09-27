//! The app's side: listens on 127.0.0.1 for `zorvik mcp` bridges and runs
//! their tool calls. Both sides prove they know the token in `agent.json`
//! without sending it:
//!
//! ```text
//! bridge → app   {"client": "claude-code", "nonce": "…", "proof": sha256(bridge, nonce, token)}
//! app → bridge   {"ok": true, "proof": sha256(app, nonce, token)}   (or {"error": "…"} and close)
//! then JSON-RPC lines both ways (tools/call, notifications/cancelled → results, progress)
//! ```

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Semaphore, mpsc};
use tokio_util::sync::CancellationToken;
use zorvik_api::Api;

use crate::discovery::{AgentFile, proof, same_secret};
use crate::exec::{Executor, MAX_MESSAGE, read_line};
use crate::protocol::{self, DISCONNECTED, PARSE_ERROR};

/// Time a new connection has to prove it knows the token.
const HELLO_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_HELLO: usize = 64 * 1024;
/// Connections still proving themselves at once; more are closed right away.
const MAX_UNPROVEN: usize = 16;
/// When a session ends: time left to deliver the last answers.
const FLUSH_TIMEOUT: Duration = Duration::from_secs(2);

/// Listening for agents; stops (and removes `agent.json`) when dropped or shut down.
pub struct AgentListener {
    file: AgentFile,
    data_dir: PathBuf,
    stop: CancellationToken,
}

impl AgentListener {
    pub fn port(&self) -> u16 {
        self.file.port
    }

    pub fn shutdown(&self) {
        self.stop.cancel();
        self.file.remove_if_ours(&self.data_dir);
    }
}

impl Drop for AgentListener {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Start listening and write `agent.json` to `data_dir`. Must run inside a Tokio runtime.
pub async fn start(api: Api, data_dir: &Path) -> std::io::Result<AgentListener> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
    let port = listener.local_addr()?.port();
    let token = format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple());
    let file = AgentFile { port, token: token.clone(), pid: std::process::id() };
    file.write(data_dir)?;
    let stop = CancellationToken::new();
    let accepting = stop.clone();
    let unproven = Arc::new(Semaphore::new(MAX_UNPROVEN));
    tokio::spawn(async move {
        loop {
            let (stream, _) = tokio::select! {
                r = listener.accept() => match r {
                    Ok(conn) => conn,
                    Err(e) => {
                        tracing::warn!("agent connection failed: {e}");
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        continue;
                    }
                },
                _ = accepting.cancelled() => break,
            };
            let Ok(permit) = unproven.clone().try_acquire_owned() else { continue };
            let (api, token, stop) = (api.clone(), token.clone(), accepting.clone());
            tokio::spawn(async move {
                if let Err(e) = serve(api, stream, &token, stop, permit).await {
                    tracing::debug!("agent connection ended: {e}");
                }
            });
        }
    });
    tracing::info!("listening for AI agents on 127.0.0.1:{port}");
    Ok(AgentListener { file, data_dir: data_dir.to_path_buf(), stop })
}

#[derive(Deserialize)]
struct Hello {
    #[serde(default)]
    client: String,
    nonce: String,
    proof: String,
}

async fn serve(
    api: Api,
    stream: TcpStream,
    token: &str,
    stop: CancellationToken,
    unproven: tokio::sync::OwnedSemaphorePermit,
) -> std::io::Result<()> {
    let _ = stream.set_nodelay(true);
    let (read, mut write) = stream.into_split();
    let mut reader = BufReader::new(read);
    let hello = tokio::time::timeout(HELLO_TIMEOUT, read_line(&mut reader, MAX_HELLO))
        .await
        .map_err(|_| std::io::Error::other("no hello"))??
        .ok_or_else(|| std::io::Error::other("closed before hello"))?;
    let hello: Option<Hello> = hello.ok().and_then(|h| serde_json::from_str(&h).ok());
    let proven = hello
        .filter(|h| (16..=128).contains(&h.nonce.len()) && same_secret(&h.proof, &proof(token, "bridge", &h.nonce)));
    let Some(hello) = proven else {
        let _ = write.write_all(format!("{}\n", json!({ "error": "Not allowed" })).as_bytes()).await;
        return Ok(());
    };
    let welcome = json!({ "ok": true, "proof": proof(token, "app", &hello.nonce) });
    write.write_all(format!("{welcome}\n").as_bytes()).await?;
    drop(unproven);

    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    let writer = tokio::spawn(async move {
        while let Some(line) = rx.recv().await {
            if write.write_all(line.as_bytes()).await.is_err() || write.write_all(b"\n").await.is_err() {
                break;
            }
            let _ = write.flush().await;
        }
    });
    let exec = Executor::new(api.clone(), api.agent_connect(&hello.client), tx.clone());
    let closed = exec.session().closed();
    loop {
        let line = tokio::select! {
            line = read_line(&mut reader, MAX_MESSAGE) => line?,
            _ = closed.cancelled() => {
                // The user disconnected this agent: the bridge takes no more calls.
                let reason = "The user disconnected this agent in Zorvik.";
                let _ = tx.send(protocol::notification(DISCONNECTED, json!({ "reason": reason })));
                break;
            }
            _ = stop.cancelled() => break,
        };
        let line = match line {
            None => break,
            Some(Ok(line)) if line.trim().is_empty() => continue,
            Some(Ok(line)) => line,
            Some(Err(reason)) => {
                let _ = tx.send(protocol::error(&Value::Null, PARSE_ERROR, reason));
                continue;
            }
        };
        match serde_json::from_str::<Value>(&line) {
            Ok(Value::Array(batch)) => batch.into_iter().for_each(|m| exec.handle(m)),
            Ok(msg) => exec.handle(msg),
            Err(_) => {
                let _ = tx.send(protocol::error(&Value::Null, PARSE_ERROR, "Not JSON"));
            }
        }
    }
    // Calls still running are cancelled; the last answers get a moment to go out.
    exec.end();
    drop((exec, tx, reader));
    let _ = tokio::time::timeout(FLUSH_TIMEOUT, writer).await;
    Ok(())
}

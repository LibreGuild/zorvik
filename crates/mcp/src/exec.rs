//! Runs an agent session's tool calls against an [`Api`]: in the app for a
//! connected bridge, or inside `zorvik mcp` when headless.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tokio::io::{AsyncBufRead, AsyncBufReadExt};
use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;
use zorvik_api::agents::ProgressFn;
use zorvik_api::{AgentSessionHandle, Api};

use crate::protocol::{self, METHOD_NOT_FOUND, id_key};

/// Longest message accepted (an import can carry a big OpenAPI document).
pub(crate) const MAX_MESSAGE: usize = 64 * 1024 * 1024;
/// Progress notifications of one call, at most this often.
const PROGRESS_EVERY: Duration = Duration::from_secs(1);
/// Tool calls of one session running at once; more wait their turn.
const MAX_CALLS: usize = 8;

/// Outgoing lines (JSON-RPC messages) of a session.
pub(crate) type Out = UnboundedSender<String>;

/// One line (without the newline), `None` at the end. A line longer than `max` or not
/// UTF-8 is skipped and comes back as `Err(reason)`: one bad message doesn't end the session.
pub(crate) async fn read_line<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    max: usize,
) -> std::io::Result<Option<Result<String, &'static str>>> {
    let mut buf = Vec::new();
    let n = tokio::io::AsyncReadExt::take(&mut *reader, max as u64 + 1).read_until(b'\n', &mut buf).await?;
    if n == 0 {
        return Ok(None);
    }
    if buf.last() != Some(&b'\n') && buf.len() > max {
        // Skip the rest of the line.
        loop {
            let chunk = reader.fill_buf().await?;
            if chunk.is_empty() {
                break;
            }
            match chunk.iter().position(|b| *b == b'\n') {
                Some(i) => {
                    reader.consume(i + 1);
                    break;
                }
                None => {
                    let len = chunk.len();
                    reader.consume(len);
                }
            }
        }
        return Ok(Some(Err("Message too long")));
    }
    while matches!(buf.last(), Some(b'\n' | b'\r')) {
        buf.pop();
    }
    Ok(Some(String::from_utf8(buf).map_err(|_| "Message is not UTF-8")))
}

pub(crate) struct Executor {
    api: Api,
    session: AgentSessionHandle,
    out: Out,
    /// Calls in progress by id (for `notifications/cancelled`).
    calls: Arc<Mutex<HashMap<String, CancellationToken>>>,
    /// Cancelled when the session ends.
    ended: CancellationToken,
    turns: Arc<tokio::sync::Semaphore>,
}

impl Executor {
    pub fn new(api: Api, session: AgentSessionHandle, out: Out) -> Self {
        let turns = Arc::new(tokio::sync::Semaphore::new(MAX_CALLS));
        Self { api, session, out, calls: Default::default(), ended: CancellationToken::new(), turns }
    }

    pub fn session(&self) -> &AgentSessionHandle {
        &self.session
    }

    /// Handle one message from the agent: `tools/call`, `notifications/cancelled`, `ping`.
    pub fn handle(&self, msg: Value) {
        let method = msg["method"].as_str().unwrap_or_default();
        let id = msg.get("id").cloned().filter(|id| !id.is_null());
        match (method, id) {
            ("tools/call", Some(id)) => self.call(id, &msg["params"]),
            ("notifications/cancelled", None) => {
                let key = id_key(&msg["params"]["requestId"]);
                if let Some(token) = self.calls.lock().unwrap_or_else(|e| e.into_inner()).remove(&key) {
                    token.cancel();
                }
            }
            (_, Some(id)) => match protocol::answer_locally(method, &msg["params"]) {
                Some(Ok(result)) => self.send(protocol::result(&id, result)),
                Some(Err((code, message))) => self.send(protocol::error(&id, code, &message)),
                None => self.send(protocol::error(&id, METHOD_NOT_FOUND, &format!("Unknown method '{method}'"))),
            },
            _ => {}
        }
    }

    fn send(&self, line: String) {
        let _ = self.out.send(line);
    }

    fn call(&self, id: Value, params: &Value) {
        let name = params["name"].as_str().unwrap_or_default().to_string();
        let args = params.get("arguments").cloned().unwrap_or(json!({}));
        let token = self.ended.child_token();
        let key = id_key(&id);
        self.calls.lock().unwrap_or_else(|e| e.into_inner()).insert(key.clone(), token.clone());
        let progress = self.progress_fn(params["_meta"]["progressToken"].clone());
        let (api, session, out, calls) = (self.api.clone(), self.session.clone(), self.out.clone(), self.calls.clone());
        let turns = self.turns.clone();
        tokio::spawn(async move {
            let _turn = tokio::select! {
                turn = turns.acquire_owned() => turn,
                _ = token.cancelled() => return,
            };
            let output = api.agent_call(&session, &name, args, progress, token.clone()).await;
            let cancelled_by_client = calls.lock().unwrap_or_else(|e| e.into_inner()).remove(&key).is_none();
            // A call the agent cancelled gets no answer (MCP); one cut short by the session end neither.
            if !cancelled_by_client && !token.is_cancelled() {
                let _ = out.send(protocol::result(&id, output.to_mcp()));
            }
        });
    }

    /// Progress notifications for a call that asked for them (at most one a second).
    fn progress_fn(&self, token: Value) -> ProgressFn {
        if token.is_null() {
            return Arc::new(|_, _, _| {});
        }
        let out = self.out.clone();
        let last = Mutex::new(None::<Instant>);
        Arc::new(move |progress: f64, total: Option<f64>, message: &str| {
            let mut last = last.lock().unwrap_or_else(|e| e.into_inner());
            if last.is_some_and(|t| t.elapsed() < PROGRESS_EVERY) {
                return;
            }
            *last = Some(Instant::now());
            let mut params = json!({ "progressToken": token, "progress": progress, "message": message });
            if let Some(total) = total {
                params["total"] = json!(total);
            }
            let _ = out.send(protocol::notification("notifications/progress", params));
        })
    }

    /// The session is over: stop every call.
    pub fn end(&self) {
        self.ended.cancel();
        self.api.agent_disconnect(&self.session);
    }
}

impl Drop for Executor {
    fn drop(&mut self) {
        self.end();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn bounded_lines() {
        let data = b"one\r\ntwo\nthree-is-long\nfour\n\xff\n".to_vec();
        let mut r = tokio::io::BufReader::with_capacity(4, &data[..]);
        let mut next = async || read_line(&mut r, 8).await.unwrap();
        assert_eq!(next().await, Some(Ok("one".into())));
        assert_eq!(next().await, Some(Ok("two".into())));
        assert_eq!(next().await, Some(Err("Message too long")));
        assert_eq!(next().await, Some(Ok("four".into())), "the stream goes on after a bad line");
        assert_eq!(next().await, Some(Err("Message is not UTF-8")));
        assert_eq!(next().await, None);
        let mut r = tokio::io::BufReader::new(&b"last"[..]);
        assert_eq!(read_line(&mut r, 8).await.unwrap(), Some(Ok("last".into())));
    }
}

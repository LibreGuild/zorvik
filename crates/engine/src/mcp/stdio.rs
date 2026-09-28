//! The stdio transport: Zorvik starts the server program and exchanges newline-delimited
//! JSON-RPC messages over its stdin and stdout; what it writes to stderr is its log. Ending
//! the session closes stdin, waits briefly for the program to exit, then stops it (and the
//! programs it started, such as the `node` that `npx` runs).

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, oneshot};

use super::{Inbound, Link, MAX_MESSAGE, Outbound};
use crate::error::{EngineError, ErrorKind, Result};

/// How long a program gets to exit by itself once its stdin is closed.
const EXIT_GRACE: Duration = Duration::from_secs(2);
/// Longest stderr line kept for the log.
const MAX_LOG_LINE: usize = 8 * 1024;

/// Split a command line into the program and its arguments, like a shell would for simple
/// cases: spaces separate words, `'…'` and `"…"` group them, and a backslash escapes a space,
/// a quote or a backslash (other backslashes stay, so Windows paths work unquoted).
pub fn split_command(line: &str) -> std::result::Result<Vec<String>, String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut chars = line.trim().chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut word));
                    in_word = false;
                }
            }
            '\'' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(c) => word.push(c),
                        None => return Err("A quote (') is not closed".into()),
                    }
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') if matches!(chars.peek(), Some('"' | '\\')) => {
                            word.push(chars.next().unwrap_or('\\'))
                        }
                        Some(c) => word.push(c),
                        None => return Err("A quote (\") is not closed".into()),
                    }
                }
            }
            '\\' if chars.peek().is_some_and(|n| n.is_whitespace() || matches!(n, '"' | '\'' | '\\')) => {
                in_word = true;
                word.push(chars.next().unwrap_or('\\'));
            }
            c => {
                in_word = true;
                word.push(c);
            }
        }
    }
    if in_word {
        words.push(word);
    }
    Ok(words)
}

/// The program as a path to start: on Windows, a name without an extension is looked up on
/// the PATH with its extensions (`npx` is `npx.cmd`), so batch files start too.
fn program_path(program: &str, env: &[(String, String)]) -> PathBuf {
    if !cfg!(windows) || Path::new(program).extension().is_some() || program.contains(['\\', '/']) {
        return PathBuf::from(program);
    }
    let path = env
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("PATH"))
        .map(|(_, v)| v.clone())
        .or_else(|| std::env::var("PATH").ok())
        .unwrap_or_default();
    let extensions = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
    for dir in std::env::split_paths(&path) {
        for ext in extensions.split(';').filter(|e| !e.is_empty()) {
            let candidate = dir.join(format!("{program}{}", ext.to_ascii_lowercase()));
            if candidate.is_file() {
                return candidate;
            }
        }
    }
    PathBuf::from(program)
}

pub(super) fn spawn(command_line: &str, env: &[(String, String)], cwd: Option<&Path>) -> Result<Link> {
    let words = split_command(command_line).map_err(EngineError::invalid)?;
    let Some((program, args)) = words.split_first() else {
        return Err(EngineError::invalid("Enter the command that starts the server"));
    };
    let mut command = Command::new(program_path(program, env));
    command
        .args(args)
        .envs(env.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some(dir) = cwd {
        if !dir.is_dir() {
            return Err(EngineError::invalid(format!(
                "The folder to start the program in doesn't exist: {} (Connection tab)",
                dir.display()
            )));
        }
        command.current_dir(dir);
    }
    // Its own process group, so stopping it stops what it started.
    #[cfg(unix)]
    command.process_group(0);
    #[cfg(windows)]
    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let mut child = command.spawn().map_err(|e| {
        let message = match e.kind() {
            std::io::ErrorKind::NotFound => format!(
                "Couldn't start {program}: it isn't installed, or isn't on the PATH Zorvik uses. Use its full path, \
                 or set PATH in the Connection tab."
            ),
            std::io::ErrorKind::PermissionDenied => format!("Couldn't start {program}: permission denied"),
            _ => format!("Couldn't start {program}: {e}"),
        };
        EngineError::new(ErrorKind::Connect, message)
    })?;
    let pid = child.id();
    let (Some(stdin), Some(stdout), Some(stderr)) = (child.stdin.take(), child.stdout.take(), child.stderr.take())
    else {
        return Err(EngineError::new(ErrorKind::Io, "The program's input and output are not available"));
    };

    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Outbound>();
    let (in_tx, in_rx) = mpsc::unbounded_channel();
    let (stop_tx, stop_rx) = oneshot::channel::<()>();
    // Writes messages until the session ends, then closes stdin and asks the waiter to stop it.
    tokio::spawn(async move {
        let mut stdin = stdin;
        while let Some(Outbound::Message(message)) = out_rx.recv().await {
            let mut line = message.to_string();
            line.push('\n');
            if stdin.write_all(line.as_bytes()).await.is_err() || stdin.flush().await.is_err() {
                break;
            }
        }
        drop(stdin);
        let _ = stop_tx.send(());
    });
    let readers = [tokio::spawn(read_messages(stdout, in_tx.clone())), tokio::spawn(read_log(stderr, in_tx.clone()))];
    tokio::spawn(wait(child, stop_rx, readers, in_tx));
    Ok(Link { tx: out_tx, rx: in_rx, label: "stdio", pid, http: None })
}

/// One JSON-RPC message (or batch) per line.
async fn read_messages(stdout: impl AsyncRead + Unpin, events: mpsc::UnboundedSender<Inbound>) {
    let mut reader = BufReader::new(stdout);
    let mut line = Vec::new();
    loop {
        line.clear();
        match read_line(&mut reader, &mut line).await {
            Ok(0) => return,
            Ok(_) => {}
            Err(e) => {
                // Nothing more can be read from it: the session is over.
                let _ = events.send(Inbound::Closed(e));
                return;
            }
        }
        let text = String::from_utf8_lossy(&line);
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        match serde_json::from_str::<Value>(text) {
            Ok(Value::Array(batch)) => {
                for message in batch {
                    let _ = events.send(Inbound::Message(message));
                }
            }
            Ok(message @ Value::Object(_)) => {
                let _ = events.send(Inbound::Message(message));
            }
            _ => {
                let start: String = text.chars().take(300).collect();
                let _ = events.send(Inbound::Error(format!(
                    "The program wrote something that isn't a JSON-RPC message to stdout (servers must log to stderr): {start}"
                )));
            }
        }
    }
}

/// A line of at most [`MAX_MESSAGE`] bytes (a longer one is an error rather than unbounded memory).
async fn read_line(
    reader: &mut BufReader<impl AsyncRead + Unpin>,
    line: &mut Vec<u8>,
) -> std::result::Result<usize, String> {
    read_bounded_line(reader, line, MAX_MESSAGE, false).await
}

/// Read up to and including the next `\n` into `line`, at most `max` bytes. Over `max`: an
/// error, or with `cut` the rest of the line is skipped (read, not kept).
async fn read_bounded_line(
    reader: &mut BufReader<impl AsyncRead + Unpin>,
    line: &mut Vec<u8>,
    max: usize,
    cut: bool,
) -> std::result::Result<usize, String> {
    let mut total = 0;
    loop {
        let available = reader.fill_buf().await.map_err(|e| format!("Reading from the program failed: {e}"))?;
        if available.is_empty() {
            return Ok(line.len());
        }
        let (take, done) = match available.iter().position(|b| *b == b'\n') {
            Some(i) => (i + 1, true),
            None => (available.len(), false),
        };
        let room = max.saturating_sub(line.len());
        line.extend_from_slice(&available[..take.min(if cut { room } else { take })]);
        reader.consume(take);
        total += take;
        if !cut && line.len() > max {
            return Err(format!("The program sent a message larger than {} MB", max >> 20));
        }
        if done {
            return Ok(total);
        }
    }
}

/// The program's log, line by line (any bytes, long lines cut). Read to the end even when
/// nobody listens: a program whose stderr is closed can die of a broken pipe.
async fn read_log(stderr: impl AsyncRead + Unpin, events: mpsc::UnboundedSender<Inbound>) {
    let mut reader = BufReader::new(stderr);
    let mut line = Vec::new();
    loop {
        line.clear();
        match read_bounded_line(&mut reader, &mut line, MAX_LOG_LINE, true).await {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        let text = String::from_utf8_lossy(&line);
        let text = text.trim_end_matches(['\n', '\r']);
        let _ = events.send(Inbound::Stderr(text.to_string()));
    }
}

/// Reports the program's exit; when the session ends first, gives it [`EXIT_GRACE`] and then
/// stops it and its process group.
async fn wait(
    mut child: Child,
    stop: oneshot::Receiver<()>,
    readers: [tokio::task::JoinHandle<()>; 2],
    events: mpsc::UnboundedSender<Inbound>,
) {
    let status = tokio::select! {
        status = child.wait() => status,
        _ = stop => match tokio::time::timeout(EXIT_GRACE, child.wait()).await {
            Ok(status) => status,
            Err(_) => {
                stop_tree(&mut child).await;
                child.wait().await
            }
        },
    };
    let reason = match status {
        Ok(s) if s.success() => "the program exited".to_string(),
        Ok(s) => match s.code() {
            Some(code) => format!("the program exited with code {code}"),
            None => "the program was stopped".to_string(),
        },
        Err(e) => format!("waiting for the program failed: {e}"),
    };
    // What it wrote last (often why it stopped) comes before the news that it stopped.
    let _ = tokio::time::timeout(Duration::from_millis(500), futures_util::future::join_all(readers)).await;
    let _ = events.send(Inbound::Closed(reason));
}

async fn stop_tree(child: &mut Child) {
    #[cfg(unix)]
    if let Some(pid) = child.id() {
        // The group the program leads (see `process_group(0)`).
        // SAFETY: `killpg` only sends a signal; the id is the child's own process group.
        unsafe {
            libc::killpg(pid as libc::pid_t, libc::SIGTERM);
        }
        if tokio::time::timeout(Duration::from_millis(500), child.wait()).await.is_ok() {
            return;
        }
        unsafe {
            libc::killpg(pid as libc::pid_t, libc::SIGKILL);
        }
    }
    #[cfg(windows)]
    if let Some(pid) = child.id() {
        let _ = tokio::process::Command::new("taskkill")
            .args(["/T", "/F", "/PID", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW: no console flashes up
            .status()
            .await;
    }
    let _ = child.kill().await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_lines_split_like_a_shell() {
        let s = |l: &str| split_command(l).unwrap();
        assert_eq!(
            s("npx -y @modelcontextprotocol/server-everything"),
            ["npx", "-y", "@modelcontextprotocol/server-everything"]
        );
        assert_eq!(s(r#"node "my server.js" --name 'a b' x\ y"#), ["node", "my server.js", "--name", "a b", "x y"]);
        assert_eq!(s(r#"C:\tools\server.exe --dir C:\data"#), [r"C:\tools\server.exe", "--dir", r"C:\data"]);
        assert_eq!(s(r#"say "a \"quoted\" word" ''"#), ["say", r#"a "quoted" word"#, ""]);
        assert_eq!(s("   "), Vec::<String>::new());
        assert!(split_command("node 'open").is_err());
        assert!(split_command("node \"open").is_err());
    }
}

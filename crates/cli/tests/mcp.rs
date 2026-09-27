//! `zorvik mcp`: the real binary speaks MCP on stdio and ends with it.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use serde_json::{Value, json};

#[test]
fn answers_on_stdio_and_exits_when_the_agent_does() {
    let data = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_zorvik"))
        .args(["mcp", "--data-dir"])
        .arg(data.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    for msg in [
        json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "test" } } }),
        json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
        json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
    ] {
        writeln!(stdin, "{msg}").unwrap();
    }
    drop(stdin);
    let lines: Vec<Value> = BufReader::new(child.stdout.take().unwrap())
        .lines()
        .map(|l| serde_json::from_str(&l.unwrap()).expect("only JSON-RPC on stdout"))
        .collect();
    assert!(child.wait().unwrap().success());
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[0]["result"]["serverInfo"]["name"], "zorvik");
    assert!(lines[1]["result"]["tools"].as_array().unwrap().len() > 20);
}

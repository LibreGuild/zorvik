//! `zorvik serve`: run the real binary with a workspace's mock API, talk to
//! it, read its traffic lines, stop it.

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use zorvik_engine::{Client, HttpRequest, RequestOptions};
use zorvik_workspace::Workspace;
use zorvik_workspace::formats::{Environment, McpToolMock, MockRoute, Server, ServerKind, Variable};

fn workspace() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let ws = Workspace::create(dir.path(), "Serve").unwrap();
    let mut server = Server::new("Users API", ServerKind::Http);
    server.http.routes = vec![MockRoute {
        method: "GET".into(),
        path: "/hello/:who".into(),
        body: "hello {{request.params.who}} from {{place}}".into(),
        ..Default::default()
    }];
    ws.create_server(&server).unwrap();
    ws.create_environment(&Environment {
        name: "Local".into(),
        variables: vec![Variable { key: "place".into(), value: "local".into(), enabled: true, secret: false }],
    })
    .unwrap();
    dir
}

fn zorvik(args: &[&str]) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_zorvik"));
    c.args(args).env("NO_COLOR", "1");
    c
}

/// Lines of the child's stdout as they come.
fn lines(child: &mut Child) -> mpsc::Receiver<String> {
    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    rx
}

fn wait_line(rx: &mpsc::Receiver<String>, what: impl Fn(&str) -> bool) -> String {
    let mut seen = Vec::new();
    while let Ok(line) = rx.recv_timeout(Duration::from_secs(10)) {
        if what(&line) {
            return line;
        }
        seen.push(line);
    }
    panic!("line not printed; got {seen:#?}");
}

async fn get(url: &str) -> String {
    let request = HttpRequest { method: "GET".into(), url: url.into(), headers: Vec::new(), body: Default::default() };
    let r = Client::new().send(request, &RequestOptions::default(), None).await.expect("request succeeds");
    String::from_utf8_lossy(&r.body).into_owned()
}

#[tokio::test(flavor = "multi_thread")]
async fn serves_a_mock_and_prints_its_traffic() {
    let dir = workspace();
    let path = dir.path().to_string_lossy().into_owned();
    let mut child = zorvik(&["serve", &path, "users api", "-e", "local", "--port", "0"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let rx = lines(&mut child);
    let started = wait_line(&rx, |l| l.contains("is running at"));
    assert!(started.starts_with("Mock API \"Users API\" is running at http://127.0.0.1:"), "{started}");
    let url = started.split("running at ").nth(1).unwrap().split(' ').next().unwrap().to_string();
    wait_line(&rx, |l| l.contains("GET") && l.contains("/hello/:who"));

    assert_eq!(get(&format!("{url}/hello/ann")).await, "hello ann from local");
    let line = wait_line(&rx, |l| l.contains("GET /hello/ann → 200"));
    assert!(line.trim_start().contains("s  #"), "time and connection first: {line}");

    #[cfg(unix)]
    {
        // Ctrl+C stops the server and exits cleanly.
        let status = Command::new("kill").args(["-INT", &child.id().to_string()]).status().unwrap();
        assert!(status.success());
        wait_line(&rx, |l| l == "Stopped.");
        let code = tokio::task::spawn_blocking(move || child.wait().unwrap()).await.unwrap();
        assert_eq!(code.code(), Some(0));
    }
    #[cfg(not(unix))]
    {
        child.kill().unwrap();
        let _ = child.wait();
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn json_lines_and_errors() {
    let dir = workspace();
    let path = dir.path().to_string_lossy().into_owned();
    let mut child = zorvik(&["serve", &path, "Users API", "--port", "0", "--json", "--var", "place=cli"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let rx = lines(&mut child);
    let started: serde_json::Value = serde_json::from_str(&wait_line(&rx, |l| l.contains("started"))).unwrap();
    assert_eq!((started["kind"].as_str(), started["name"].as_str()), (Some("http"), Some("Users API")));
    let url = started["url"].as_str().unwrap().to_string();
    assert_eq!(get(&format!("{url}/hello/bob")).await, "hello bob from cli");
    let entry: serde_json::Value = serde_json::from_str(&wait_line(&rx, |l| l.contains("\"traffic\""))).unwrap();
    assert_eq!(entry["entry"]["http"]["path"], "/hello/bob");
    child.kill().unwrap();
    let _ = child.wait();

    // Unknown server, environment, and a port that is taken: exit code 2 with a reason.
    let run = |args: Vec<String>| {
        let out = zorvik(&args.iter().map(String::as_str).collect::<Vec<_>>()).output().unwrap();
        (out.status.code(), String::from_utf8_lossy(&out.stderr).into_owned())
    };
    let (code, err) = run(vec!["serve".into(), path.clone(), "nope".into()]);
    assert_eq!(code, Some(2));
    assert!(err.contains("server 'nope' not found (servers: Users API)"), "{err}");
    let (code, err) = run(vec!["serve".into(), path.clone(), "Users API".into(), "-e".into(), "prod".into()]);
    assert_eq!(code, Some(2));
    assert!(err.contains("environment 'prod' not found"), "{err}");
    let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = taken.local_addr().unwrap().port().to_string();
    let (code, err) = run(vec!["serve".into(), path, "Users API".into(), "--port".into(), port]);
    assert_eq!(code, Some(2));
    assert!(err.contains("already in use"), "{err}");
}

/// Clients control what the traffic lines show: escape sequences in their data
/// must reach the terminal escaped, not as terminal commands.
#[tokio::test(flavor = "multi_thread")]
async fn control_characters_from_clients_are_escaped() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let dir = tempfile::tempdir().unwrap();
    let ws = Workspace::create(dir.path(), "Serve").unwrap();
    ws.create_server(&Server::new("Echo", ServerKind::Tcp)).unwrap();
    let path = dir.path().to_string_lossy().into_owned();
    let mut child =
        zorvik(&["serve", &path, "Echo", "--port", "0"]).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
    let rx = lines(&mut child);
    let started = wait_line(&rx, |l| l.contains("is running at"));
    let addr = started.split("tcp://").nth(1).unwrap().split(' ').next().unwrap().to_string();

    let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
    client.write_all(b"\x1b]0;owned\x07\x1b[2Jhi\r\n").await.unwrap();
    let mut echo = [0u8; 64];
    let _ = tokio::time::timeout(Duration::from_secs(5), client.read(&mut echo)).await;
    let line = wait_line(&rx, |l| l.contains("in ") && l.contains("hi"));
    assert!(!line.contains('\x1b') && !line.contains('\x07'), "{line:?}");
    assert!(line.contains(r"\u{1b}]0;owned\u{7}\u{1b}[2Jhi\r\n"), "{line:?}");
    child.kill().unwrap();
    let _ = child.wait();
}

#[test]
fn serves_an_mcp_server_on_stdio() {
    use std::io::Write as _;
    let dir = workspace();
    let ws = Workspace::open(dir.path()).unwrap();
    let mut server = Server::new("Tools", ServerKind::Mcp);
    server.mcp.tools = vec![McpToolMock { name: "where".into(), result: "in {{place}}".into(), ..Default::default() }];
    ws.create_server(&server).unwrap();
    let path = dir.path().to_str().unwrap();

    let mut child = zorvik(&["serve", path, "Tools", "--stdio", "--env", "Local"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let rx = lines(&mut child);
    let messages = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"where","arguments":{}}}"#,
    ];
    for m in messages {
        writeln!(stdin, "{m}").unwrap();
    }
    let init: serde_json::Value = serde_json::from_str(&wait_line(&rx, |l| l.contains(r#""id":1"#))).unwrap();
    assert_eq!(init["result"]["serverInfo"]["name"], "Tools");
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
    let call: serde_json::Value = serde_json::from_str(&wait_line(&rx, |l| l.contains(r#""id":2"#))).unwrap();
    assert_eq!(call["result"]["content"][0]["text"], "in local");

    // Closing stdin ends it; the traffic log went to stderr, stdout had only messages.
    drop(stdin);
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    let log = String::from_utf8_lossy(&out.stderr);
    assert!(log.contains("tools/call"), "{log}");

    // Other kinds don't speak stdio.
    let out = zorvik(&["serve", path, "Users API", "--stdio"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("MCP servers only"));
}

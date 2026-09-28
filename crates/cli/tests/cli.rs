//! Run the real `zorvik` binary against a workspace and local servers.

use std::process::Command;

use zorvik_testkit::TestServer;
use zorvik_workspace::Workspace;
use zorvik_workspace::formats::{Auth, Environment, McpToolMock, Request, RequestKind, Server, ServerKind, Variable};

fn request(name: &str, url: &str) -> Request {
    let mut r = Request::new(name, RequestKind::Http);
    r.url = url.into();
    r
}

fn run(args: &[&str]) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_zorvik")).args(args).env("NO_COLOR", "1").output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr),
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn runs_workspace_requests_and_sets_exit_code() {
    let server = TestServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let ws = Workspace::create(dir.path(), "CLI").unwrap();
    let folder = ws.create_folder("", "Smoke").unwrap();
    ws.create_request(&folder, request("Ok", "{{base}}/status/200")).unwrap();
    ws.create_request(&folder, request("Token", "{{base}}/bearer")).unwrap();
    let mut ws_req = request("Socket", "ws://127.0.0.1:1/ws");
    ws_req.kind = RequestKind::Websocket;
    ws.create_request("", ws_req).unwrap();
    ws.create_environment(&Environment {
        name: "Local".into(),
        variables: vec![Variable { key: "base".into(), value: server.url(""), enabled: true, secret: false }],
    })
    .unwrap();
    let path = dir.path().to_string_lossy().into_owned();

    let p = path.clone();
    let (code, out) =
        tokio::task::spawn_blocking(move || run(&["run", &p, "--env", "local", "--folder", "Smoke"])).await.unwrap();
    // /bearer without a token → 401 → failure.
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("1 passed, 1 failed"), "{out}");

    let p = path.clone();
    let (code, out) =
        tokio::task::spawn_blocking(move || run(&["run", &p, "--env", "Local", "--allow-http-errors", "--json"]))
            .await
            .unwrap();
    assert_eq!(code, 0, "{out}");
    let report: serde_json::Value = serde_json::from_str(&out).unwrap();
    let results = report["results"].as_array().unwrap();
    assert_eq!(results.len(), 3);
    assert!(results.iter().any(|r| r["skipped"] == true));
    // The fields of the earlier report are still there.
    for field in ["path", "name", "method", "url", "status", "durationMs", "size", "error", "unresolved", "passed"] {
        assert!(results[0].get(field).is_some(), "{field} missing: {}", results[0]);
    }
    assert_eq!(report["summary"]["requests"], 2);
    assert_eq!(report["summary"]["skipped"], 1);
    assert_eq!(report["summary"]["passed"], true);

    // Text mode lists skipped requests too.
    let p = path.clone();
    let (code, out) =
        tokio::task::spawn_blocking(move || run(&["run", &p, "--env", "Local", "--allow-http-errors"])).await.unwrap();
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("WS      Socket  (skipped: WebSocket connections are live sessions"), "{out}");

    let (code, out) = tokio::task::spawn_blocking(move || run(&["run", &path, "--env", "nope"])).await.unwrap();
    assert_eq!(code, 2, "{out}");
    assert!(out.contains("environment 'nope' not found"));
}

#[tokio::test(flavor = "multi_thread")]
async fn blank_secret_variables_are_reported_as_undefined() {
    let server = TestServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let ws = Workspace::create(dir.path(), "CLI").unwrap();
    let mut req = request("Token", "{{base}}/bearer");
    req.auth = Auth::Bearer { token: "{{token}}".into(), prefix: "Bearer".into() };
    ws.create_request("", req).unwrap();
    ws.create_environment(&Environment {
        name: "Local".into(),
        variables: vec![
            Variable { key: "base".into(), value: server.url(""), enabled: true, secret: false },
            // As the app saves it: the secret value lives outside the workspace.
            Variable { key: "token".into(), value: String::new(), enabled: true, secret: true },
        ],
    })
    .unwrap();
    let path = dir.path().to_string_lossy().into_owned();

    let p = path.clone();
    let (_, out) = tokio::task::spawn_blocking(move || run(&["run", &p, "--env", "Local"])).await.unwrap();
    assert!(out.contains("undefined variables: token"), "{out}");

    let (code, out) = tokio::task::spawn_blocking(move || run(&["run", &path, "--env", "Local", "--var", "token=abc"]))
        .await
        .unwrap();
    assert_eq!(code, 0, "{out}");
    assert!(!out.contains("undefined"), "{out}");
}

#[tokio::test(flavor = "multi_thread")]
async fn scripts_tests_data_files_and_junit() {
    let server = TestServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let ws = Workspace::create(dir.path(), "Shop").unwrap();
    let mut login = request("Login", "{{base}}/echo?user={{user}}");
    login.scripts.pre_request = "pm.variables.set('started', pm.info.iteration);".into();
    login.scripts.post_response = r#"
        pm.test('status is 200', () => pm.response.to.have.status(200));
        pm.test('user from data', () => pm.expect(pm.response.json().args.user).to.equal(pm.iterationData.get('user')));
        pm.environment.set('token', 'tok-' + pm.iterationData.get('user'));
    "#
    .into();
    ws.create_request("", login).unwrap();
    let mut profile = request("Profile", "{{base}}/echo?t={{token}}");
    profile.scripts.post_response = r#"
        pm.test('token from Login', () => pm.expect(pm.response.json().args.t).to.equal('tok-' + pm.iterationData.get('user')));
        pm.test('fails for bob', () => pm.expect(pm.iterationData.get('user')).to.not.equal('bob'));
    "#
    .into();
    ws.create_request("", profile).unwrap();
    ws.create_environment(&Environment {
        name: "Local".into(),
        variables: vec![Variable { key: "base".into(), value: server.url(""), enabled: true, secret: false }],
    })
    .unwrap();
    let data = dir.path().join("users.csv");
    std::fs::write(&data, "user\nada\nbob\n").unwrap();
    let junit = dir.path().join("junit.xml");
    let (path, data, junit_arg) = (
        dir.path().to_string_lossy().into_owned(),
        data.to_string_lossy().into_owned(),
        junit.to_string_lossy().into_owned(),
    );

    let (p, d, j) = (path.clone(), data.clone(), junit_arg.clone());
    let (code, out) =
        tokio::task::spawn_blocking(move || run(&["run", &p, "-e", "Local", "--data", &d, "--junit", &j]))
            .await
            .unwrap();
    assert_eq!(code, 1, "a failed test fails the run: {out}");
    assert!(out.contains("Iteration 1 of 2"), "{out}");
    assert!(out.contains("Iteration 2 of 2"), "{out}");
    assert!(out.contains("    ✓ status is 200"), "{out}");
    assert!(out.contains("    ✓ token from Login"), "{out}");
    assert!(out.contains("    ✗ fails for bob — expected 'bob' to not equal 'bob'"), "{out}");
    assert!(out.contains("Requests  3 passed, 1 failed, 0 skipped (4 total)"), "{out}");
    assert!(out.contains("Tests     7 passed, 1 failed"), "{out}");
    let xml = std::fs::read_to_string(&junit).unwrap();
    assert!(xml.contains(r#"<testsuite name="Profile" tests="4" failures="1""#), "{xml}");
    assert!(xml.contains(r#"<testcase name="fails for bob (iteration 2)""#), "{xml}");

    // --iterations wins over the row count; --bail stops at the first failure.
    let (p, d) = (path.clone(), data.clone());
    let (code, out) =
        tokio::task::spawn_blocking(move || run(&["run", &p, "-e", "Local", "-d", &d, "-n", "1", "--json"]))
            .await
            .unwrap();
    assert_eq!(code, 0, "{out}");
    let report: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(report["summary"]["iterations"], 1);
    assert_eq!(report["summary"]["testsPassed"], 4);
    assert_eq!(report["results"][0]["tests"][0]["name"], "status is 200");
    let (p, d) = (path.clone(), data.clone());
    let (code, out) = tokio::task::spawn_blocking(move || {
        run(&["run", &p, "-e", "Local", "-d", &d, "--bail", "--delay", "10", "--json"])
    })
    .await
    .unwrap();
    assert_eq!(code, 1, "{out}");
    let report: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(report["summary"]["bailed"], true);
    assert_eq!(report["results"].as_array().unwrap().len(), 4, "bob's Profile was the last one");

    // The workspace files never get script values (the CLI keeps nothing).
    let env_file = std::fs::read_dir(dir.path().join("environments")).unwrap().next().unwrap().unwrap().path();
    assert!(!std::fs::read_to_string(env_file).unwrap().contains("tok-"));

    // Usage errors: exit code 2.
    let p = path.clone();
    let (code, out) =
        tokio::task::spawn_blocking(move || run(&["run", &p, "--data", "/definitely/missing.csv"])).await.unwrap();
    assert_eq!(code, 2, "{out}");
    assert!(out.contains("error: data file /definitely/missing.csv"), "{out}");
    let bad = dir.path().join("bad.json");
    std::fs::write(&bad, "{}").unwrap();
    let (p, b) = (path.clone(), bad.to_string_lossy().into_owned());
    let (code, out) = tokio::task::spawn_blocking(move || run(&["run", &p, "--data", &b])).await.unwrap();
    assert_eq!(code, 2, "{out}");
    assert!(out.contains("JSON data must be an array of objects"), "{out}");
    let (code, out) = tokio::task::spawn_blocking(move || run(&["run", &path, "-n", "0"])).await.unwrap();
    assert_eq!(code, 2, "{out}");
}

#[tokio::test(flavor = "multi_thread")]
async fn script_errors_and_set_next_request() {
    let server = TestServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let ws = Workspace::create(dir.path(), "Flow").unwrap();
    let mut first = request("First", &server.url("/status/200"));
    first.scripts.post_response = "pm.execution.setNextRequest('Last');".into();
    ws.create_request("", first).unwrap();
    ws.create_request("", request("Middle", &server.url("/status/200"))).unwrap();
    let mut last = request("Last", &server.url("/status/200"));
    last.scripts.post_response = "undefinedFunction();".into();
    ws.create_request("", last).unwrap();
    let path = dir.path().to_string_lossy().into_owned();
    let (code, out) = tokio::task::spawn_blocking(move || run(&["run", &path])).await.unwrap();
    assert_eq!(code, 1, "{out}");
    assert!(!out.contains("Middle"), "setNextRequest jumped over it: {out}");
    assert!(
        out.contains("    ! Post-response script of request 'Last' failed at line 1: ReferenceError: undefinedFunction is not defined"),
        "{out}"
    );
    assert!(out.contains("Requests  1 passed, 1 failed"), "{out}");
}

#[tokio::test(flavor = "multi_thread")]
async fn secrets_set_by_scripts_stay_hidden_in_reported_urls() {
    let server = TestServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let ws = Workspace::create(dir.path(), "CLI").unwrap();
    let mut login = request("Login", &server.url("/status/200"));
    login.scripts.post_response = "pm.environment.set('token', 'tok-from-login');".into();
    ws.create_request("", login).unwrap();
    ws.create_request("", request("Profile", &format!("{}?t={{{{token}}}}", server.url("/echo")))).unwrap();
    ws.create_environment(&Environment {
        name: "Local".into(),
        // Declared secret, blank in the file (as the app saves it): the login script sets it.
        variables: vec![Variable { key: "token".into(), value: String::new(), enabled: true, secret: true }],
    })
    .unwrap();
    let path = dir.path().to_string_lossy().into_owned();
    let (code, out) = tokio::task::spawn_blocking(move || run(&["run", &path, "-e", "Local"])).await.unwrap();
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("/echo?t={{token}}"), "{out}");
    assert!(!out.contains("tok-from-login"), "{out}");
}

#[tokio::test(flavor = "multi_thread")]
async fn mcp_requests_start_programs_only_when_allowed() {
    let dir = tempfile::tempdir().unwrap();
    let ws = Workspace::create(dir.path(), "MCP").unwrap();
    // The program is this very CLI serving the workspace's MCP server over stdio.
    let mut server = Server::new("Tools", ServerKind::Mcp);
    server.mcp.tools = vec![McpToolMock {
        name: "add".into(),
        input_schema: r#"{"type": "object", "required": ["a", "b"]}"#.into(),
        result: "{{args.a}}+{{args.b}}".into(),
        ..Default::default()
    }];
    ws.create_server(&server).unwrap();
    let mut r = Request::new("Add", RequestKind::Mcp);
    r.url = format!("'{}' serve '{}' Tools --stdio", env!("CARGO_BIN_EXE_zorvik"), dir.path().display());
    r.mcp.name = "add".into();
    r.mcp.arguments = r#"{"a": 1, "b": 2}"#.into();
    r.scripts.post_response =
        "pm.test('adds', () => pm.expect(pm.response.json().content[0].text).to.eql('1+2'));".into();
    ws.create_request("", r).unwrap();
    let path = dir.path().to_str().unwrap();

    let (code, out) = run(&["run", path]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("--allow-programs"), "{out}");

    let (code, out) = run(&["run", path, "--allow-programs"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("adds") && out.contains("MCP"), "{out}");
}

//! `zorvik load`: run the real binary against a workspace load test and a
//! local server; the exit code gates CI on thresholds.

use std::process::Command;

use zorvik_testkit::TestServer;
use zorvik_workspace::Workspace;
use zorvik_workspace::formats::{
    CaptureFrom, Environment, LoadCapture, LoadStage, LoadTest, Request, RequestKind, Threshold, ThresholdMetric,
    ThresholdOp, Variable,
};

fn request(name: &str, url: &str) -> Request {
    let mut r = Request::new(name, RequestKind::Http);
    r.url = url.into();
    r
}

fn load_test(name: &str, target: &str) -> LoadTest {
    let mut test = LoadTest::new(name, vec![target.into()]);
    test.stages = vec![LoadStage { duration_secs: 0, target: 2 }, LoadStage { duration_secs: 1, target: 2 }];
    test.think_time_ms = 20;
    test.thresholds = vec![Threshold {
        metric: ThresholdMetric::ErrorRate,
        op: ThresholdOp::Lt,
        value: 1.0,
        target: None,
        enabled: true,
    }];
    test
}

fn zorvik(args: &[&str]) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_zorvik")).args(args).env("NO_COLOR", "1").output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr),
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn load_command_reports_and_gates_on_thresholds() {
    let server = TestServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let ws = Workspace::create(dir.path(), "Load").unwrap();
    let ok = ws.create_request("", request("Ok", "{{base}}/status/200")).unwrap();
    let broken = ws.create_request("", request("Broken", "{{base}}/status/503")).unwrap();
    ws.create_load_test(&load_test("Smoke", &ok)).unwrap();
    ws.create_load_test(&load_test("Outage", &broken)).unwrap();
    ws.create_environment(&Environment {
        name: "Local".into(),
        variables: vec![Variable { key: "base".into(), value: server.url(""), enabled: true, secret: false }],
    })
    .unwrap();
    let path = dir.path().to_string_lossy().into_owned();
    let json = dir.path().join("out.json").to_string_lossy().into_owned();
    let html = dir.path().join("out.html").to_string_lossy().into_owned();

    let (p, j, h) = (path.clone(), json.clone(), html.clone());
    let (code, out) = tokio::task::spawn_blocking(move || {
        zorvik(&["load", &p, "smoke", "-e", "local", "--json", &j, "--html", &h, "--quiet"])
    })
    .await
    .unwrap();
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("Load test \"Smoke\": up to 2 virtual users, 1 s, 1 request"), "{out}");
    assert!(out.contains("✓ errorRate < 1 %") && out.contains("PASSED"), "{out}");
    let summary: serde_json::Value = serde_json::from_slice(&std::fs::read(&json).unwrap()).unwrap();
    assert_eq!(summary["passed"], true);
    assert!(summary["totals"]["requests"].as_u64().unwrap() > 10);
    assert!(std::fs::read_to_string(&html).unwrap().contains("<svg"));

    // Every answer is a 503: the threshold fails and so does the command.
    let p = path.clone();
    let (code, out) =
        tokio::task::spawn_blocking(move || zorvik(&["load", &p, "Outage", "-e", "Local"])).await.unwrap();
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("✗ errorRate < 1 %") && out.contains("FAILED") && out.contains("503 ×"), "{out}");

    let (code, out) = tokio::task::spawn_blocking(move || zorvik(&["load", &path, "nope"])).await.unwrap();
    assert_eq!(code, 2, "{out}");
    assert!(out.contains("load test 'nope' not found (load tests: "), "{out}");
}

#[tokio::test(flavor = "multi_thread")]
async fn load_command_uses_data_rows_captures_and_reports_timing() {
    let server = TestServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let ws = Workspace::create(dir.path(), "Load").unwrap();
    std::fs::write(dir.path().join("codes.csv"), "code\n200\n201\n").unwrap();
    let status = ws.create_request("", request("Status", &format!("{}/status/{{{{code}}}}", server.url("")))).unwrap();
    let create = ws.create_request("", request("Create", &server.url("/echo?next=204"))).unwrap();
    let mut rows = load_test("Rows", &status);
    rows.data_file = Some("codes.csv".into());
    ws.create_load_test(&rows).unwrap();
    let mut chain = load_test("Chain", &create);
    chain.targets.extend(load_test("x", &status).targets);
    chain.targets[0].captures = vec![
        LoadCapture { variable: "code".into(), from: CaptureFrom::Json, path: "$.args.next".into() },
        LoadCapture { variable: "etag".into(), from: CaptureFrom::Header, path: "ETag".into() },
    ];
    ws.create_load_test(&chain).unwrap();
    let path = dir.path().to_string_lossy().into_owned();
    let json = dir.path().join("rows.json").to_string_lossy().into_owned();
    let codes = |file: &str| -> Vec<u64> {
        let summary: serde_json::Value = serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
        let mut codes: Vec<u64> =
            summary["totals"]["statusCodes"].as_array().unwrap().iter().map(|c| c[0].as_u64().unwrap()).collect();
        codes.sort();
        codes
    };

    // Two users, two rows.
    let (p, j) = (path.clone(), json.clone());
    let (code, out) =
        tokio::task::spawn_blocking(move || zorvik(&["load", &p, "Rows", "--quiet", "--json", &j])).await.unwrap();
    assert_eq!(code, 0, "{out}");
    assert_eq!(codes(&json), [200, 201], "{out}");
    assert!(out.contains("First byte p50 ") && out.contains("(request sent to first byte: server + network)"), "{out}");
    assert!(out.contains("Connect    p50 ") && !out.contains("Server     p50"), "{out}");

    // --var wins over the data file.
    let (p, j) = (path.clone(), json.clone());
    let (code, out) = tokio::task::spawn_blocking(move || {
        zorvik(&["load", &p, "Rows", "--quiet", "--json", &j, "--var", "code=203"])
    })
    .await
    .unwrap();
    assert_eq!(code, 0, "{out}");
    assert_eq!(codes(&json), [203], "{out}");

    // The create's answer names the code the next request sends; the ETag capture always misses.
    let (p, j) = (path.clone(), json.clone());
    let (code, out) =
        tokio::task::spawn_blocking(move || zorvik(&["load", &p, "Chain", "--quiet", "--json", &j])).await.unwrap();
    assert_eq!(code, 0, "{out}");
    assert_eq!(codes(&json), [200, 204], "{out}");
    assert!(out.contains("missed (found nothing; the variable kept its value)"), "{out}");
    assert!(out.contains("1st byte p95") && out.contains("Missed"), "{out}");
    let summary: serde_json::Value = serde_json::from_slice(&std::fs::read(&json).unwrap()).unwrap();
    assert_eq!(summary["targets"][0]["metrics"]["captureMisses"], summary["targets"][0]["metrics"]["requests"]);
    assert!(summary["totals"]["timing"]["ttfb"]["count"].as_u64().unwrap() > 0);

    // A data file outside the workspace needs --allow-outside-files.
    let outside = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(outside.path(), "code\n200\n").unwrap();
    rows.name = "Outside".into();
    rows.data_file = Some(outside.path().to_string_lossy().into_owned());
    ws.create_load_test(&rows).unwrap();
    let p = path.clone();
    let (code, out) = tokio::task::spawn_blocking(move || zorvik(&["load", &p, "Outside", "--quiet"])).await.unwrap();
    assert_eq!(code, 2, "{out}");
    assert!(out.contains("outside the workspace folder"), "{out}");
    let (code, out) =
        tokio::task::spawn_blocking(move || zorvik(&["load", &path, "Outside", "--quiet", "--allow-outside-files"]))
            .await
            .unwrap();
    assert_eq!(code, 0, "{out}");
}

/// Ctrl+C stops the run early and still reports it; the exit code follows the thresholds.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn ctrl_c_stops_the_run_and_reports() {
    use std::io::{BufRead, BufReader, Read};
    use std::process::Stdio;

    let server = TestServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let ws = Workspace::create(dir.path(), "Load").unwrap();
    let ok = ws.create_request("", request("Ok", &server.url("/status/200"))).unwrap();
    let mut test = load_test("Long", &ok);
    test.stages = vec![LoadStage { duration_secs: 0, target: 2 }, LoadStage { duration_secs: 60, target: 2 }];
    ws.create_load_test(&test).unwrap();
    let path = dir.path().to_string_lossy().into_owned();

    let (code, out) = tokio::task::spawn_blocking(move || {
        let mut child = Command::new(env!("CARGO_BIN_EXE_zorvik"))
            .args(["load", &path, "Long"])
            .env("NO_COLOR", "1")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdout = BufReader::new(child.stdout.take().unwrap());
        let mut out = String::new();
        // The first progress line: the run (and its Ctrl+C listener) is going.
        while !out.contains("req/s") {
            assert!(stdout.read_line(&mut out).unwrap() > 0, "{out}");
        }
        let killed = Command::new("kill").args(["-INT", &child.id().to_string()]).status().unwrap();
        assert!(killed.success());
        stdout.read_to_string(&mut out).unwrap();
        child.stderr.take().unwrap().read_to_string(&mut out).unwrap();
        (child.wait().unwrap().code(), out)
    })
    .await
    .unwrap();
    assert_eq!(code, Some(0), "{out}");
    assert!(out.contains("(stopped early)") && out.contains("PASSED") && out.contains("Stopping"), "{out}");
}

//! The load generator against local servers: both models, stages, dropped
//! iterations, latency from the scheduled start, stop, thresholds, events,
//! data rows per user, captures and the timing phases.

use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bytes::Bytes;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use zorvik_engine::{HttpRequest, HttpVersionPref, RequestOptions};
use zorvik_formats::{CaptureFrom, LoadCapture, LoadModel, LoadStage, Threshold, ThresholdMetric, ThresholdOp};
use zorvik_load::{
    DataRow, EventFn, LoadEvent, LoadRun, MetricsSummary, PhaseSummary, Plan, PlanTarget, RequestSource, RunPhase,
    Summary, TargetSummary, TimePoint, TimingSummary, UserVars, html_report, start, validate,
};
use zorvik_testkit::TestServer;

fn get(url: String) -> HttpRequest {
    HttpRequest { method: "GET".into(), url, headers: vec![], body: Bytes::new() }
}

fn target(name: &str, url: String, weight: u32) -> PlanTarget {
    PlanTarget {
        name: name.into(),
        request: format!("{name}.yaml"),
        source: RequestSource::Fixed(get(url)),
        weight,
        captures: Vec::new(),
    }
}

/// A target rendered per iteration from the user's variables.
fn rendered(name: &str, render: impl Fn(&UserVars) -> String + Send + Sync + 'static) -> PlanTarget {
    PlanTarget {
        name: name.into(),
        request: format!("{name}.yaml"),
        source: RequestSource::Dynamic(Arc::new(move |vars| Ok(get(render(vars))))),
        weight: 1,
        captures: Vec::new(),
    }
}

fn capture(variable: &str, from: CaptureFrom, path: &str) -> LoadCapture {
    LoadCapture { variable: variable.into(), from, path: path.into() }
}

fn row(pairs: &[(&str, &str)]) -> DataRow {
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect::<Vec<_>>().into()
}

fn plan(model: LoadModel, stages: &[(u32, u32)], targets: Vec<PlanTarget>) -> Plan {
    Plan {
        targets,
        model,
        stages: stages.iter().map(|&(duration_secs, target)| LoadStage { duration_secs, target }).collect(),
        think_time: Duration::ZERO,
        max_in_flight: 1000,
        keep_alive: true,
        options: RequestOptions::default(),
        thresholds: Vec::new(),
        rows: Vec::new(),
    }
}

fn threshold(metric: ThresholdMetric, op: ThresholdOp, value: f64, target: Option<&str>) -> Threshold {
    Threshold { metric, op, value, target: target.map(str::to_string), enabled: true }
}

/// Run to the end (calling `stop` after `stop_after`), checking the event
/// order: snapshots, then exactly one `Finished`, then nothing.
async fn run(plan: Plan, stop_after: Option<Duration>) -> Summary {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let on_event: EventFn = Arc::new(move |event| {
        let _ = tx.send(event);
    });
    let run: Arc<LoadRun> = Arc::new(start(plan, on_event).expect("plan starts"));
    if let Some(after) = stop_after {
        let run = run.clone();
        tokio::spawn(async move {
            tokio::time::sleep(after).await;
            run.stop();
        });
    }
    let mut snapshots = 0;
    let summary = loop {
        match tokio::time::timeout(Duration::from_secs(30), rx.recv()).await.expect("run finishes") {
            Some(LoadEvent::Snapshot { snapshot }) => {
                assert_ne!(snapshot.phase, RunPhase::Finished);
                snapshots += 1;
            }
            Some(LoadEvent::Finished { summary }) => break summary,
            None => panic!("events ended without Finished"),
        }
    };
    assert!(snapshots >= 1, "no snapshots");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(rx.try_recv().is_err(), "an event came after Finished");
    assert!(summary.error.is_none(), "{:?}", summary.error);
    let per_target: u64 = summary.targets.iter().map(|t| t.metrics.requests).sum();
    assert_eq!(per_target, summary.totals.requests);
    summary
}

#[tokio::test(flavor = "multi_thread")]
async fn closed_model_follows_think_time_and_reuses_connections() {
    let server = TestServer::start().await;
    let mut p = plan(LoadModel::VirtualUsers, &[(0, 5), (2, 5)], vec![target("Ok", server.url("/status/200"), 1)]);
    p.think_time = Duration::from_millis(100);
    let summary = run(p, None).await;
    // 5 users × 2 s / (100 ms think + ~1 ms answer) ≈ 100 (fewer where timers are coarse, as on Windows).
    let n = summary.totals.requests;
    assert!((70..=110).contains(&n), "{n} requests");
    assert_eq!(summary.totals.errors, 0);
    assert_eq!(summary.totals.connections, 5);
    assert_eq!(summary.totals.status_codes, vec![(200, n)]);
    assert!(summary.passed && !summary.stopped_early);
    assert!(summary.duration_ms >= 2000 && summary.duration_ms < 3000, "{}", summary.duration_ms);
    assert_eq!(summary.points.iter().map(|p| p.active).collect::<Vec<_>>()[..2], [5, 5]);
}

#[tokio::test(flavor = "multi_thread")]
async fn open_model_holds_the_rate() {
    let server = TestServer::start().await;
    let summary = run(
        plan(LoadModel::ArrivalRate, &[(0, 200), (3, 200)], vec![target("Ok", server.url("/status/200"), 1)]),
        None,
    )
    .await;
    let n = summary.totals.requests;
    println!("open model: {n} requests, {:.1} req/s, p99 {} ms", summary.totals.rps, summary.totals.latency.p99);
    assert!((510..=690).contains(&n), "{n} requests");
    assert!((170.0..=230.0).contains(&summary.totals.rps), "{} req/s", summary.totals.rps);
    assert_eq!(summary.totals.dropped, 0);
    assert!((170.0..=230.0).contains(&summary.points[1].rps), "{:?}", summary.points);
    assert_eq!(summary.points[1].target, 200.0);
}

#[tokio::test(flavor = "multi_thread")]
async fn open_model_drops_iterations_over_the_in_flight_cap() {
    let server = TestServer::start().await;
    let mut p = plan(LoadModel::ArrivalRate, &[(0, 100), (2, 100)], vec![target("Slow", server.url("/delay/200"), 1)]);
    p.max_in_flight = 5;
    let summary = run(p, None).await;
    let t = &summary.totals;
    // 200 scheduled: ~5 per 200 ms can run, the rest are dropped (not queued).
    assert_eq!(t.requests + t.dropped, 200);
    assert!(t.dropped >= 120, "{} dropped", t.dropped);
    assert!(t.latency.p50 >= 200.0 && t.latency.p50 < 400.0, "{}", t.latency.p50);
    assert_eq!(summary.targets[0].metrics.dropped, t.dropped);
}

/// HTTP/1.1 server that answers one request at a time (20 ms each) across
/// all connections: 50 requests per second at most.
async fn serial_server() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let turn = Arc::new(tokio::sync::Mutex::new(()));
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let turn = turn.clone();
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                loop {
                    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                        match socket.read(&mut chunk).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => buf.extend_from_slice(&chunk[..n]),
                        }
                    }
                    let end = buf.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
                    buf.drain(..end);
                    let _turn = turn.lock().await;
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    if socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok").await.is_err() {
                        return;
                    }
                }
            });
        }
    });
    addr
}

#[tokio::test(flavor = "multi_thread")]
async fn open_model_latency_counts_from_the_scheduled_start() {
    let addr = serial_server().await;
    let url = format!("http://{addr}/");
    // One user never waits in the server's queue: ~20 ms.
    let closed =
        run(plan(LoadModel::VirtualUsers, &[(0, 1), (1, 1)], vec![target("Serial", url.clone(), 1)]), None).await;
    assert!(closed.totals.latency.p99 < 100.0, "{}", closed.totals.latency.p99);

    // 100/s offered, 50/s served: the queue grows and so does the latency.
    let open = run(plan(LoadModel::ArrivalRate, &[(0, 100), (2, 100)], vec![target("Serial", url, 1)]), None).await;
    let l = &open.totals.latency;
    println!("overload: p50 {} ms, p99 {} ms, max {} ms, {} requests", l.p50, l.p99, l.max, open.totals.requests);
    assert_eq!(open.totals.requests, 200);
    assert!(l.p99 > 1000.0, "p99 {}", l.p99);
    assert!(open.points[1].p50 > open.points[0].p50 + 200.0, "{:?}", open.points);
}

#[tokio::test(flavor = "multi_thread")]
async fn stages_ramp_the_load() {
    let server = TestServer::start().await;
    // 0 → 200 requests/s over 2 s: 50 in the first second, 150 in the second.
    let summary =
        run(plan(LoadModel::ArrivalRate, &[(2, 200)], vec![target("Ok", server.url("/status/200"), 1)]), None).await;
    assert_eq!(summary.totals.requests, 200);
    let p: Vec<&TimePoint> = summary.points.iter().collect();
    assert!((35.0..=65.0).contains(&p[0].rps) && (130.0..=170.0).contains(&p[1].rps), "{p:?}");
    // The chart target of a rate is its average over the second, like the count.
    assert_eq!((p[0].target, p[1].target), (50.0, 150.0));

    // Users ramp too: 0 → 6 over 2 s.
    let mut users = plan(LoadModel::VirtualUsers, &[(2, 6)], vec![target("Ok", server.url("/status/200"), 1)]);
    users.think_time = Duration::from_millis(50);
    let summary = run(users, None).await;
    let active: Vec<u32> = summary.points.iter().map(|p| p.active).collect();
    // Sampled just after each second ends: ~3 users after 1 s, 6 at the end.
    assert!((2..=5).contains(&active[0]) && active[1] >= 5, "{active:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn stop_finishes_promptly_and_cuts_off_stuck_requests() {
    let server = TestServer::start().await;
    let stopped = Instant::now() + Duration::from_millis(700);
    let summary = run(
        plan(LoadModel::VirtualUsers, &[(0, 10), (30, 10)], vec![target("Ok", server.url("/delay/50"), 1)]),
        Some(Duration::from_millis(700)),
    )
    .await;
    assert!(summary.stopped_early && summary.passed);
    assert!(stopped.elapsed() < Duration::from_secs(2), "{:?}", stopped.elapsed());
    assert!(summary.duration_ms < 2000 && summary.totals.requests > 50, "{summary:?}");
    assert_eq!(summary.totals.errors, 0);

    // No request timeout: requests still running after the 5 s grace are cut off and count as errors.
    let mut p = plan(LoadModel::VirtualUsers, &[(0, 3), (30, 3)], vec![target("Stuck", server.url("/delay/60000"), 1)]);
    p.options.timeout = None;
    let summary = run(p, Some(Duration::from_millis(300))).await;
    assert_eq!(summary.totals.requests, 3);
    assert_eq!(summary.totals.error_kinds, vec![("cancelled".to_string(), 3)]);
    assert!(summary.totals.latency.min >= 5000.0, "{:?}", summary.totals.latency);
    assert!(summary.duration_ms < 7000, "{}", summary.duration_ms);
}

#[tokio::test(flavor = "multi_thread")]
async fn thresholds_decide_pass_or_fail() {
    let server = TestServer::start().await;
    let targets = vec![target("Ok", server.url("/status/200"), 3), target("Broken", server.url("/status/500"), 1)];
    let mut p = plan(LoadModel::VirtualUsers, &[(0, 2), (1, 2)], targets);
    p.think_time = Duration::from_millis(20);
    p.thresholds = vec![
        threshold(ThresholdMetric::P95, ThresholdOp::Lt, 5000.0, None),
        threshold(ThresholdMetric::ErrorRate, ThresholdOp::Lte, 30.0, None),
        threshold(ThresholdMetric::ErrorRate, ThresholdOp::Lt, 1.0, Some("Broken.yaml")),
        threshold(ThresholdMetric::Rps, ThresholdOp::Gt, 1e9, None),
        Threshold { enabled: false, ..threshold(ThresholdMetric::Max, ThresholdOp::Lt, 0.001, None) },
    ];
    let summary = run(p.clone(), None).await;
    let results: Vec<(&str, bool)> = summary.thresholds.iter().map(|t| (t.label.as_str(), t.passed)).collect();
    assert_eq!(
        results,
        [
            ("p95 < 5000 ms", true),
            ("errorRate <= 30 %", true),
            ("errorRate < 1 % · Broken", false),
            ("rps > 1000000000", false)
        ]
    );
    assert_eq!(summary.thresholds[2].actual, Some(100.0));
    assert!(!summary.passed);
    // Weights 3:1.
    let ok = summary.targets[0].metrics.requests as f64;
    let broken = summary.targets[1].metrics.requests as f64;
    assert!((ok / broken - 3.0).abs() < 0.3, "{ok} vs {broken}");

    p.thresholds.truncate(2);
    assert!(run(p, None).await.passed);
}

#[tokio::test(flavor = "multi_thread")]
async fn errors_are_counted_by_kind() {
    let server = TestServer::start().await;
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap();
    let targets =
        vec![target("Refused", format!("http://{closed}/"), 1), target("Slow", server.url("/delay/10000"), 1)];
    let mut p = plan(LoadModel::VirtualUsers, &[(0, 2), (1, 2)], targets);
    p.think_time = Duration::from_millis(50);
    // Longer than Windows' ~2 s of retrying a refused connection.
    p.options.timeout = Some(Duration::from_secs(3));
    let summary = run(p, None).await;
    let kinds: Vec<&str> = summary.totals.error_kinds.iter().map(|(k, _)| k.as_str()).collect();
    assert!(kinds.contains(&"connect") && kinds.contains(&"timeout"), "{kinds:?}");
    assert_eq!(summary.totals.error_rate, 100.0);
    // Timeouts have a latency (the time waited); refused connections don't.
    assert!(summary.targets[1].metrics.latency.p50 >= 3000.0);
    assert_eq!(summary.targets[0].metrics.latency, Default::default());
}

#[tokio::test(flavor = "multi_thread")]
async fn dynamic_requests_are_rendered_per_iteration_and_keep_alive_can_be_off() {
    let server = TestServer::start().await;
    let renders = Arc::new(AtomicUsize::new(0));
    let (count, url) = (renders.clone(), server.url("/status/200"));
    let dynamic = PlanTarget {
        name: "Dynamic".into(),
        request: "dynamic.yaml".into(),
        source: RequestSource::Dynamic(Arc::new(move |_| {
            count.fetch_add(1, Ordering::SeqCst);
            Ok(get(url.clone()))
        })),
        weight: 1,
        captures: Vec::new(),
    };
    let mut p = plan(LoadModel::VirtualUsers, &[(0, 2), (1, 2)], vec![dynamic]);
    p.keep_alive = false;
    p.think_time = Duration::from_millis(50);
    let summary = run(p, None).await;
    let n = summary.totals.requests;
    assert!(n > 10);
    // Two renders up front (check the request, list its host), then one per iteration.
    assert_eq!(renders.load(Ordering::SeqCst) as u64, n + 2);
    assert_eq!(summary.totals.connections, n);
    // Every request opened its connection: each has a connect time.
    assert_eq!(summary.totals.timing.connect.count, n);
}

#[tokio::test(flavor = "multi_thread")]
async fn each_user_takes_its_own_data_row() {
    let server = TestServer::start().await;
    let base = server.url("/echo");
    // What each user's second and later requests carry: the row value the
    // server echoed back to its first one (captured from the JSON answer).
    let seen = Arc::new(Mutex::new(BTreeSet::new()));
    let log = seen.clone();
    let mut echo = rendered("Echo", move |vars| {
        // The checks before the run render with a probe value in every variable.
        if let (Some(user), Some(value)) = (vars.get("user"), vars.get("echoed"))
            && !user.starts_with("zvprobe")
        {
            log.lock().unwrap().insert((user.to_string(), value.to_string()));
        }
        format!("{base}?u={}", vars.get("user").unwrap_or("none"))
    });
    echo.captures = vec![capture("echoed", CaptureFrom::Json, "$.args.u")];
    let mut p = plan(LoadModel::VirtualUsers, &[(0, 3), (1, 3)], vec![echo]);
    p.think_time = Duration::from_millis(50);
    p.rows = vec![row(&[("user", "ada")]), row(&[("user", "grace")]), row(&[("user", "linus")])];
    let summary = run(p, None).await;
    assert_eq!(summary.totals.errors, 0);
    assert_eq!(summary.totals.capture_misses, 0);
    let seen = seen.lock().unwrap().clone();
    let pairs: Vec<(&str, &str)> = seen.iter().map(|(u, e)| (u.as_str(), e.as_str())).collect();
    assert_eq!(pairs, [("ada", "ada"), ("grace", "grace"), ("linus", "linus")]);
    let t = &summary.totals.timing;
    assert_eq!(t.ttfb.count, summary.totals.requests);
    assert!(t.ttfb.p50 > 0.0 && t.ttfb.p95 <= summary.totals.latency.max, "{t:?}");
    assert_eq!(t.transfer.count, summary.totals.requests);
    // Keep-alive: one connection per user, each with a connect time.
    assert_eq!((summary.totals.connections, t.connect.count), (3, 3));
    assert_eq!(t.server, PhaseSummary::default());
}

#[tokio::test(flavor = "multi_thread")]
async fn captures_feed_the_users_later_requests() {
    let server = TestServer::start().await;
    let base = server.url("");
    // Create: the server echoes the id; Get: uses the captured id.
    let create_base = base.clone();
    let mut create =
        rendered("Create", move |vars| format!("{create_base}/echo?id=order-{}", vars.get("user").unwrap_or_default()));
    create.captures = vec![
        capture("orderId", CaptureFrom::Json, "$.args.id"),
        capture("kind", CaptureFrom::Header, "Content-Type"),
        capture("never", CaptureFrom::Json, "$.args.missing"),
    ];
    let got = Arc::new(Mutex::new(BTreeSet::new()));
    let log = got.clone();
    let mut read = rendered("Get", move |vars| {
        let id = vars.get("orderId");
        if let (Some(user), Some(id), Some(kind)) = (vars.get("user"), id, vars.get("kind"))
            && !user.starts_with("zvprobe")
        {
            log.lock().unwrap().insert((user.to_string(), id.to_string(), kind.to_string()));
        }
        format!("{base}/echo?got={}", id.unwrap_or("none"))
    });
    // Misses when a get went out without an order id.
    read.captures = vec![capture("gotBack", CaptureFrom::Regex, r#""got":\s*"(order-\d+)""#)];
    let mut p = plan(LoadModel::VirtualUsers, &[(0, 2), (1, 2)], vec![create, read]);
    p.think_time = Duration::from_millis(20);
    p.rows = vec![row(&[("user", "1")]), row(&[("user", "2")])];
    let summary = run(p, None).await;
    assert_eq!(summary.totals.errors, 0);
    // Each user sends create, then get: the get always has its own user's id.
    let got = got.lock().unwrap().clone();
    let got: Vec<(&str, &str, &str)> =
        got.iter().map(|(u, id, kind)| (u.as_str(), id.as_str(), kind.as_str())).collect();
    assert_eq!(got, [("1", "order-1", "application/json"), ("2", "order-2", "application/json")]);
    // "never" misses on every create; the get's regex always finds its value.
    let create = &summary.targets[0].metrics;
    assert_eq!(create.capture_misses, create.requests);
    assert_eq!(summary.targets[1].metrics.capture_misses, 0);
    assert_eq!(summary.totals.capture_misses, create.requests);
}

/// HTTP/1.1 server whose answers carry `Server-Timing: <value>`.
async fn server_timing_server(value: &'static str) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                loop {
                    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                        match socket.read(&mut chunk).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => buf.extend_from_slice(&chunk[..n]),
                        }
                    }
                    let end = buf.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
                    buf.drain(..end);
                    let answer = format!("HTTP/1.1 200 OK\r\nServer-Timing: {value}\r\nContent-Length: 2\r\n\r\nok");
                    if socket.write_all(answer.as_bytes()).await.is_err() {
                        return;
                    }
                }
            });
        }
    });
    addr
}

#[tokio::test(flavor = "multi_thread")]
async fn server_reported_time_comes_from_server_timing() {
    let addr = server_timing_server("db;dur=2.5, app;dur=5, cache;desc=\"a, b\"").await;
    let mut p = plan(LoadModel::VirtualUsers, &[(0, 2), (1, 2)], vec![target("Timed", format!("http://{addr}/"), 1)]);
    p.think_time = Duration::from_millis(20);
    let summary = run(p, None).await;
    let server = &summary.totals.timing.server;
    assert_eq!(server.count, summary.totals.requests);
    assert_eq!((server.p50, server.max), (7.5, 7.5));
    assert_eq!(summary.targets[0].metrics.timing.server.count, server.count);
}

#[tokio::test(flavor = "multi_thread")]
async fn arrival_rate_takes_the_next_row_per_request() {
    let server = TestServer::start().await;
    let base = server.url("/status");
    let target = rendered("Status", move |vars| format!("{base}/{}", vars.get("code").unwrap_or("400")));
    let mut p = plan(LoadModel::ArrivalRate, &[(0, 60), (1, 60)], vec![target]);
    p.rows = vec![row(&[("code", "200")]), row(&[("code", "201")]), row(&[("code", "202")])];
    let summary = run(p, None).await;
    // 60 requests over 3 rows: 20 each.
    let codes = summary.totals.status_codes.clone();
    assert_eq!(codes.iter().map(|(_, n)| n).sum::<u64>(), 60);
    assert!(codes.iter().all(|(code, n)| [200, 201, 202].contains(code) && *n == 20), "{codes:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_rendered_request_may_not_leave_the_hosts_it_started_with() {
    let server = TestServer::start().await;
    // The host comes from a captured value: unknown before the run, so refused.
    let (base, other) = (server.url(""), server.url("").replace("127.0.0.1", "localhost"));
    let mut hop = rendered("Hop", move |vars| match vars.get("next") {
        Some(next) => format!("{next}/status/200"),
        None => format!("{base}/echo?next={other}"),
    });
    hop.captures = vec![capture("next", CaptureFrom::Json, "$.args.next")];
    let mut p = plan(LoadModel::VirtualUsers, &[(0, 1), (1, 1)], vec![hop]);
    p.think_time = Duration::from_millis(20);
    let summary = run(p, None).await;
    assert_eq!(summary.totals.status_codes, vec![(200, 1)]);
    assert_eq!(summary.totals.error_kinds[0].0, "notAllowed");
    assert_eq!(zorvik_load::hosts(&plan(LoadModel::VirtualUsers, &[(1, 1)], vec![])), Vec::<String>::new());
}

#[test]
fn hosts_follow_data_rows() {
    let mut p = plan(
        LoadModel::VirtualUsers,
        &[(1, 1)],
        vec![
            rendered("Row host", |vars| format!("https://{}/x", vars.get("host").unwrap_or("{{host}}"))),
            rendered("Row path", |vars| format!("http://127.0.0.1:1/{}", vars.get("host").unwrap_or_default())),
            target("Fixed", "http://LOCALHOST:3000/".into(), 1),
        ],
    );
    p.rows = vec![row(&[("host", "a.example")]), row(&[("host", "B.example")]), row(&[("host", "a.example")])];
    assert_eq!(zorvik_load::hosts(&p), ["a.example", "b.example", "127.0.0.1", "localhost"]);
    assert_eq!(p.user_variables(), ["host"]);
}

#[test]
fn only_requests_that_use_user_variables_are_rendered_per_iteration() {
    let render = |url: &'static str| -> zorvik_load::Render {
        Arc::new(move |vars| Ok(get(url.replace("{{id}}", vars.get("id").unwrap_or("{{id}}")))))
    };
    let names = vec!["id".to_string()];
    let fixed = RequestSource::from_render(render("http://h/users"), &names, false).unwrap();
    assert!(matches!(fixed, RequestSource::Fixed(r) if r.url == "http://h/users"));
    let per_user = RequestSource::from_render(render("http://h/users/{{id}}"), &names, false).unwrap();
    assert!(matches!(per_user, RequestSource::Dynamic(_)));
    let dynamic = RequestSource::from_render(render("http://h/users"), &[], true).unwrap();
    assert!(matches!(dynamic, RequestSource::Dynamic(_)));
}

#[test]
fn plans_are_checked_before_starting() {
    let on_event: EventFn = Arc::new(|_| {});
    let mut p = plan(LoadModel::ArrivalRate, &[(1, 10)], vec![target("Bad", "ftp://x/".into(), 1)]);
    let err = start(p.clone(), on_event.clone()).err().unwrap();
    assert!(err.starts_with("Bad: "), "{err}");
    p.options.http_version = HttpVersionPref::Http3;
    assert_eq!(validate(&p).unwrap_err(), "HTTP/3 isn't supported for load tests yet");
    p.options.http_version = HttpVersionPref::Auto;
    p.stages[0].target = 60_000;
    assert!(validate(&p).unwrap_err().contains("limit"));
    p.stages[0].target = 10;
    p.max_in_flight = 1_000_000;
    assert!(validate(&p).unwrap_err().contains("in flight"));
    p.max_in_flight = 10;
    p.targets[0].captures = vec![capture("id", CaptureFrom::Regex, "(")];
    assert!(validate(&p).unwrap_err().starts_with("Bad: capture 'id': invalid regular expression"));
}

#[test]
fn html_report_is_self_contained_and_escapes_text() {
    let phase = |ms: f64| PhaseSummary { count: 1200, avg: ms, p50: ms, p95: ms * 2.0, p99: ms * 3.0, max: ms * 4.0 };
    let metrics = MetricsSummary {
        requests: 1200,
        errors: 12,
        error_rate: 1.0,
        rps: 200.0,
        status_codes: vec![(200, 1188), (500, 12)],
        error_kinds: vec![("timeout".into(), 2)],
        connections: 12,
        timing: TimingSummary {
            connect: PhaseSummary { count: 12, ..phase(3.0) },
            ttfb: phase(8.0),
            transfer: phase(1.0),
            server: PhaseSummary::default(),
        },
        capture_misses: 7,
        ..Default::default()
    };
    let summary = Summary {
        started_at: 1_790_000_000_000.0,
        duration_ms: 6000,
        totals: metrics.clone(),
        targets: vec![TargetSummary {
            name: "<img src=x onerror=alert(1)>".into(),
            request: "a&b.yaml".into(),
            metrics,
        }],
        points: (0..6)
            .map(|s| TimePoint {
                second: s,
                rps: 200.0,
                errors: 2,
                p50: 10.0 + f64::from(s),
                p95: 20.0,
                p99: 30.0,
                active: 10,
                target: 200.0,
            })
            .collect(),
        thresholds: Vec::new(),
        passed: true,
        stopped_early: false,
        error: None,
        peak_cpu_percent: Some(42.0),
    };
    let html = html_report("<script>alert(1)</script>", &summary);
    assert!(!html.contains("<script"), "{html}");
    assert!(!html.contains("<img"));
    assert!(html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
    assert!(html.contains("a&amp;b.yaml"));
    assert!(html.contains("<svg") && html.contains("prefers-color-scheme"));
    assert!(html.contains("Time to first byte") && html.contains("p95 first byte") && html.contains("16 ms"), "{html}");
    assert!(html.contains("Capture misses") && html.contains("opened by 1 % of requests"), "{html}");
    // Without Server-Timing, no server-reported row, and a note that says why.
    assert!(!html.contains("<td>Server-reported") && html.contains("No response had a Server-Timing header"));
    assert!(!html.contains("http://") && !html.contains("https://"), "no external resources");
}

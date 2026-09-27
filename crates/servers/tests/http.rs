//! Mock HTTP server: routes, templates, conditions, fallback (404 and proxy),
//! delays and faults, CORS, limits, HTTP/2 and TLS, keep-alive, stop.

mod common;

use std::time::{Duration, Instant};

use common::{Events, start, start_with};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use zorvik_engine::{
    Client, EngineError, Header, HttpRequest, HttpResponse, HttpVersionPref, RequestOptions, TlsOptions,
};
use zorvik_formats::{KeyValue, MockFallback, MockFault, MockRoute, Server, ServerKind, ServerTls, Variable};
use zorvik_servers::{OutgoingMessage, RunningServer, TrafficKind};
use zorvik_workspace::vars::VarContext;

fn route(method: &str, path: &str, status: u16, body: &str) -> MockRoute {
    MockRoute { method: method.into(), path: path.into(), status, body: body.into(), ..Default::default() }
}

fn mock(routes: Vec<MockRoute>) -> Server {
    let mut server = Server::new("Mock", ServerKind::Http);
    server.port = 0; // like `start` does, so live updates apply
    server.http.routes = routes;
    server
}

fn opts() -> RequestOptions {
    RequestOptions { follow_redirects: false, timeout: Some(Duration::from_secs(10)), ..Default::default() }
}

async fn send_with(
    method: &str,
    url: &str,
    headers: &[(&str, &str)],
    body: &str,
    opts: &RequestOptions,
) -> Result<HttpResponse, EngineError> {
    let request = HttpRequest {
        method: method.into(),
        url: url.into(),
        headers: headers.iter().map(|(k, v)| Header::new(*k, *v)).collect(),
        body: body.to_string().into(),
    };
    Client::new().send(request, opts, None).await
}

async fn send(method: &str, url: &str, headers: &[(&str, &str)], body: &str) -> HttpResponse {
    send_with(method, url, headers, body, &opts()).await.expect("request succeeds")
}

fn header<'r>(r: &'r HttpResponse, name: &str) -> Option<&'r str> {
    r.meta.headers.iter().find(|h| h.name.eq_ignore_ascii_case(name)).map(|h| h.value.as_str())
}

fn text(r: &HttpResponse) -> String {
    String::from_utf8_lossy(&r.body).into_owned()
}

async fn wait_http(events: &Events, path: &str) -> zorvik_servers::HttpExchange {
    let entry =
        events.wait(path, |e| e.kind == TrafficKind::Http && e.http.as_ref().is_some_and(|h| h.path == path)).await;
    entry.http.unwrap()
}

#[tokio::test]
async fn routes_templates_conditions_and_not_found() {
    let mut user = route(
        "GET",
        "/users/:id",
        200,
        r#"{"id": "{{request.params.id}}", "q": "{{request.query.q}}", "ua": "{{request.headers.user-agent}}", "env": "{{name}}", "uuid": "{{$uuid}}"}"#,
    );
    user.name = "Get user".into();
    user.headers = vec![KeyValue::new("X-Id", "{{request.params.id}}")];
    let mut echo = route("POST", "/echo", 201, "{{request.method}} {{request.body}}");
    echo.headers = vec![KeyValue::new("Content-Type", "text/plain")];
    let files = route("*", "/files/*", 200, "rest={{request.params.*}}");
    let disabled = MockRoute { enabled: false, ..route("GET", "/admin", 200, "secret") };
    let mut only = route("GET", "/only", 200, "matched");
    only.match_query = vec![KeyValue::new("role", "{{role}}")];
    only.match_headers = vec![KeyValue::new("x-key", "")];
    let mut vars = VarContext::new();
    vars.push_layer(&[
        Variable { key: "name".into(), value: "world".into(), enabled: true, secret: false },
        Variable { key: "role".into(), value: "admin".into(), enabled: true, secret: false },
    ]);
    let (running, events) = start_with(mock(vec![user, echo, files, disabled, only]), vars).await;
    assert!(running.url.starts_with("http://127.0.0.1:"), "{}", running.url);
    let url = |path: &str| format!("{}{path}", running.url);

    let r = send("GET", &url("/users/42?q=a%20b"), &[("User-Agent", "tester")], "").await;
    assert_eq!(r.meta.status, 200);
    assert_eq!(header(&r, "x-id"), Some("42"));
    assert_eq!(header(&r, "content-type"), Some("application/json"), "guessed from the body");
    assert_eq!(header(&r, "content-length"), Some(r.body.len().to_string().as_str()));
    let json: serde_json::Value = serde_json::from_slice(&r.body).unwrap();
    assert_eq!((json["id"].as_str(), json["q"].as_str()), (Some("42"), Some("a b")));
    assert_eq!((json["ua"].as_str(), json["env"].as_str()), (Some("tester"), Some("world")));
    assert_eq!(json["uuid"].as_str().unwrap().len(), 36);
    // Trailing slash tolerant.
    assert_eq!(send("GET", &url("/users/42/"), &[], "").await.meta.status, 200);

    // The request body is inserted as it is (never expanded).
    let r = send("POST", &url("/echo"), &[], "hello {{name}}").await;
    assert_eq!((r.meta.status, text(&r)), (201, "POST hello {{name}}".to_string()));
    assert_eq!(send("DELETE", &url("/files/a/b.txt"), &[], "").await.body, b"rest=a/b.txt");

    let r = send("GET", &url("/admin"), &[], "").await;
    assert_eq!(r.meta.status, 404);
    let json: serde_json::Value = serde_json::from_slice(&r.body).unwrap();
    assert_eq!(json["error"], "No route matches GET /admin");
    let routes = json["routes"].as_array().unwrap();
    assert!(routes.contains(&"GET /users/:id".into()) && routes.contains(&"ANY /files/*".into()), "{routes:?}");
    assert!(!routes.contains(&"GET /admin".into()), "disabled routes are not listed");

    assert_eq!(send("GET", &url("/only?role=admin"), &[], "").await.meta.status, 404, "needs the header");
    assert_eq!(send("GET", &url("/only?role=user"), &[("X-Key", "1")], "").await.meta.status, 404);
    assert_eq!(send("GET", &url("/only?role=admin"), &[("X-Key", "1")], "").await.meta.status, 200);

    // HEAD answers with the headers of GET, including the length, and no body.
    let r = send("HEAD", &url("/users/7"), &[], "").await;
    assert_eq!(r.meta.status, 200);
    assert!(r.body.is_empty());
    let get = send("GET", &url("/users/7"), &[("User-Agent", "Zorvik")], "").await;
    let head_len: usize = header(&r, "content-length").unwrap().parse().unwrap();
    assert!(head_len > 60 && head_len.abs_diff(get.body.len()) < 16, "{head_len} vs {}", get.body.len());

    // The log has the exchange, the route and the bodies.
    let x = wait_http(&events, "/users/42?q=a%20b").await;
    assert_eq!((x.method.as_str(), x.status, x.route.as_deref()), ("GET", 200, Some("Get user")));
    assert!(x.response_body.contains("\"42\"") && x.http_version == "HTTP/1.1");
    let x = wait_http(&events, "/echo").await;
    assert_eq!((x.request_body.as_str(), x.route.as_deref()), ("hello {{name}}", Some("POST /echo")));
    let x = wait_http(&events, "/admin").await;
    assert_eq!((x.status, x.route), (404, None));
    let first = events.wait("request", |e| e.kind == TrafficKind::Http).await;
    assert!(first.summary.starts_with("GET /users/42?q=a%20b → 200 · "), "{}", first.summary);

    // A mock API has no connections to send to.
    let err = running.send(None, OutgoingMessage::Text { text: "x".into() }).await.unwrap_err();
    assert!(err.message.contains("only answers requests"), "{}", err.message);
}

#[tokio::test]
async fn live_edits_delays_and_faults() {
    let mut server = mock(vec![MockRoute { delay_ms: 300, ..route("GET", "/slow", 200, "done") }]);
    let (running, events) = start(server.clone()).await;
    let url = format!("{}/slow", running.url);

    let started = Instant::now();
    assert_eq!(send("GET", &url, &[], "").await.body, b"done");
    assert!(started.elapsed() >= Duration::from_millis(300));

    let with_fault = |server: &mut Server, fault: MockFault, percent: u8| {
        server.http.routes[0].delay_ms = 0;
        server.http.routes[0].fault = fault;
        server.http.routes[0].fault_percent = percent;
    };
    with_fault(&mut server, MockFault::Error, 100);
    assert!(running.update(server.clone(), VarContext::new()));
    let r = send("GET", &url, &[], "").await;
    assert_eq!(r.meta.status, 500);
    assert!(text(&r).contains("Injected fault"), "{}", text(&r));

    // Never happens at 0 %.
    with_fault(&mut server, MockFault::Error, 0);
    running.update(server.clone(), VarContext::new());
    assert_eq!(send("GET", &url, &[], "").await.meta.status, 200);

    with_fault(&mut server, MockFault::Reset, 100);
    running.update(server.clone(), VarContext::new());
    let err = send_with("GET", &url, &[], "", &opts()).await.unwrap_err();
    assert!(err.message.contains("closed"), "{}", err.message);
    events.wait("reset", |e| e.http.as_ref().is_some_and(|h| h.note.as_deref() == Some("reset"))).await;

    with_fault(&mut server, MockFault::Hang, 100);
    running.update(server.clone(), VarContext::new());
    let short = RequestOptions { timeout: Some(Duration::from_millis(300)), ..opts() };
    let err = send_with("GET", &url, &[], "", &short).await.unwrap_err();
    assert!(err.message.to_lowercase().contains("timed out"), "{}", err.message);
    let hang = events.wait("hang", |e| e.http.as_ref().is_some_and(|h| h.note.as_deref() == Some("hang"))).await;
    assert!(hang.summary.contains("→ hang"), "{}", hang.summary);

    // Stopping the server ends a request that hangs.
    let pending = tokio::spawn({
        let url = url.clone();
        async move { send_with("GET", &url, &[], "", &opts()).await }
    });
    tokio::time::sleep(Duration::from_millis(200)).await;
    let started = Instant::now();
    running.stop();
    let result = tokio::time::timeout(Duration::from_secs(5), pending).await.expect("ends after stop").unwrap();
    assert!(result.is_err());
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[tokio::test]
async fn delayed_request_is_logged_when_the_client_leaves() {
    let (running, events) = start(mock(vec![MockRoute { delay_ms: 5_000, ..route("GET", "/wait", 200, "") }])).await;
    let short = RequestOptions { timeout: Some(Duration::from_millis(200)), ..opts() };
    assert!(send_with("GET", &format!("{}/wait", running.url), &[], "", &short).await.is_err());
    let left =
        events.wait("client left", |e| e.http.as_ref().is_some_and(|h| h.note.as_deref() == Some("client left"))).await;
    assert!(left.http.unwrap().duration_ms < 4_000.0);
}

#[tokio::test]
async fn cors_preflight_and_headers() {
    let mut server = mock(vec![MockRoute {
        headers: vec![KeyValue::new("X-Custom", "1")],
        ..route("PUT", "/items/:id", 200, "{}")
    }]);
    server.http.cors = true;
    let (running, events) = start(server).await;
    let url = format!("{}/items/1", running.url);

    let r = send(
        "OPTIONS",
        &url,
        &[
            ("Origin", "http://app.test"),
            ("Access-Control-Request-Method", "PUT"),
            ("Access-Control-Request-Headers", "content-type, x-token"),
        ],
        "",
    )
    .await;
    assert_eq!(r.meta.status, 204);
    assert_eq!(header(&r, "access-control-allow-origin"), Some("http://app.test"));
    assert_eq!(header(&r, "access-control-allow-credentials"), Some("true"));
    assert!(header(&r, "access-control-allow-methods").unwrap().contains("PUT"));
    assert_eq!(header(&r, "access-control-allow-headers"), Some("content-type, x-token"));
    events.wait("preflight", |e| e.http.as_ref().is_some_and(|h| h.note.as_deref() == Some("CORS preflight"))).await;

    let r = send("PUT", &url, &[("Origin", "http://app.test")], "").await;
    assert_eq!(header(&r, "access-control-allow-origin"), Some("http://app.test"));
    assert!(header(&r, "access-control-expose-headers").unwrap().contains("x-custom"));
    let r = send("GET", &format!("{}/nope", running.url), &[], "").await;
    assert_eq!((r.meta.status, header(&r, "access-control-allow-origin")), (404, Some("*")));
}

async fn stopped(running: &RunningServer, events: &Events) {
    running.stop();
    for _ in 0..100 {
        if events.stopped().is_some() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test]
async fn proxy_fallback_forwards_unmatched_requests() {
    let mut redirect = route("GET", "/moved", 302, "");
    redirect.headers = vec![KeyValue::new("Location", "/elsewhere")];
    let backend_routes = vec![
        MockRoute {
            headers: vec![KeyValue::new("X-Backend", "yes"), KeyValue::new("Content-Type", "text/plain")],
            ..route("POST", "/real", 202, "backend got {{request.body}} from {{request.headers.x-client}}")
        },
        redirect,
        route("GET", "/host", 200, "{{request.headers.host}}"),
    ];
    let (backend, _backend_events) = start(mock(backend_routes)).await;

    let mut front = mock(vec![route("GET", "/local", 200, "front")]);
    front.http.fallback = MockFallback::Proxy;
    front.http.proxy_url = "{{backend}}/".into();
    let mut vars = VarContext::new();
    vars.push_layer(&[Variable { key: "backend".into(), value: backend.url.clone(), enabled: true, secret: false }]);
    let (running, events) = start_with(front.clone(), vars.clone()).await;
    let url = |path: &str| format!("{}{path}", running.url);

    assert_eq!(send("GET", &url("/local"), &[], "").await.body, b"front");
    let r = send("POST", &url("/real?x=1"), &[("X-Client", "c1"), ("Connection", "keep-alive")], "data").await;
    assert_eq!((r.meta.status, text(&r)), (202, "backend got data from c1".to_string()));
    assert_eq!(header(&r, "x-backend"), Some("yes"));
    // Redirects are passed on, not followed; Host is the backend's.
    let r = send("GET", &url("/moved"), &[], "").await;
    assert_eq!((r.meta.status, header(&r, "location")), (302, Some("/elsewhere")));
    let host = backend.url.trim_start_matches("http://").to_string();
    assert_eq!(text(&send("GET", &url("/host"), &[], "").await), host);
    // HEAD keeps the backend's length.
    let r = send("HEAD", &url("/host"), &[], "").await;
    assert_eq!((r.body.len(), header(&r, "content-length")), (0, Some(host.len().to_string().as_str())));
    let x = wait_http(&events, "/real?x=1").await;
    assert_eq!((x.note.as_deref(), x.status, x.route), (Some("proxy"), 202, None));

    // A backend that is down, and one that points back at the mock.
    let backend_url = backend.url.clone();
    drop(backend);
    tokio::time::sleep(Duration::from_millis(50)).await;
    let r = send("GET", &url("/gone"), &[], "").await;
    assert_eq!(r.meta.status, 502);
    assert!(text(&r).contains("Could not reach the backend"), "{}", text(&r));
    front.http.proxy_url = running.url.clone();
    assert!(running.update(front, vars));
    let r = send("GET", &url("/loop"), &[], "").await;
    assert_eq!(r.meta.status, 502);
    assert!(text(&r).contains("came back to the mock"), "{}", text(&r));
    assert!(!backend_url.is_empty());
    stopped(&running, &events).await;
}

/// Raw HTTP/1.1 exchange on one connection: returns everything read until `until` has been seen `times` times.
async fn raw(stream: &mut TcpStream, request: &[u8], until: &str, times: usize) -> String {
    stream.write_all(request).await.unwrap();
    let mut got = Vec::new();
    let mut buf = [0u8; 8192];
    while String::from_utf8_lossy(&got).matches(until).count() < times {
        match tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buf)).await.expect("answer in time") {
            Ok(0) | Err(_) => break,
            Ok(n) => got.extend_from_slice(&buf[..n]),
        }
    }
    String::from_utf8_lossy(&got).into_owned()
}

#[tokio::test]
async fn keep_alive_limits_and_malformed_requests() {
    let (running, events) = start(mock(vec![route("GET", "/ok", 200, "ok")])).await;

    // Two requests on one connection (pipelined) are both answered, on the same connection id.
    let mut c = TcpStream::connect(running.addr).await.unwrap();
    let out =
        raw(&mut c, b"GET /ok HTTP/1.1\r\nHost: x\r\n\r\nGET /ok HTTP/1.1\r\nHost: x\r\n\r\n", "HTTP/1.1 200", 2).await;
    assert_eq!(out.matches("HTTP/1.1 200").count(), 2, "{out}");
    let conns: Vec<Option<u64>> =
        events.traffic().iter().filter(|e| e.kind == TrafficKind::Http).map(|e| e.conn).collect();
    assert!(conns.len() == 2 && conns[0] == conns[1] && conns[0].is_some(), "{conns:?}");

    // A body larger than 10 MB is refused from its declared length.
    let mut c = TcpStream::connect(running.addr).await.unwrap();
    let out = raw(&mut c, b"POST /ok HTTP/1.1\r\nHost: x\r\nContent-Length: 11000000\r\n\r\n", "\r\n\r\n", 1).await;
    assert!(out.starts_with("HTTP/1.1 413"), "{out}");

    // Garbage and huge headers get an error (or a closed connection), and the server keeps going.
    let mut c = TcpStream::connect(running.addr).await.unwrap();
    let out = raw(&mut c, b"NOT HTTP AT ALL\r\n\r\n", "\r\n\r\n", 1).await;
    assert!(out.is_empty() || out.starts_with("HTTP/1.1 400"), "{out}");
    let mut c = TcpStream::connect(running.addr).await.unwrap();
    let huge = format!("GET /ok HTTP/1.1\r\nHost: x\r\nX-Big: {}\r\n\r\n", "a".repeat(1 << 20));
    c.write_all(huge.as_bytes()).await.ok();
    let mut buf = [0u8; 256];
    let n = tokio::time::timeout(Duration::from_secs(5), c.read(&mut buf)).await.expect("answer in time").unwrap_or(0);
    let head = String::from_utf8_lossy(&buf[..n]);
    assert!(n == 0 || head.starts_with("HTTP/1.1 431") || head.starts_with("HTTP/1.1 400"), "{head}");
    assert_eq!(send("GET", &format!("{}/ok", running.url), &[], "").await.body, b"ok");
}

#[tokio::test]
async fn http2_with_and_without_tls() {
    let (plain, _events) = start(mock(vec![route("GET", "/v", 200, "{{request.method}}")])).await;
    let h2 = RequestOptions { http_version: HttpVersionPref::Http2, ..opts() };
    let r = send_with("GET", &format!("{}/v", plain.url), &[], "", &h2).await.unwrap();
    assert_eq!((r.meta.http_version.as_str(), r.body.as_slice()), ("HTTP/2", b"GET".as_slice()));

    let mut server = mock(vec![route("GET", "/v", 200, "secure")]);
    server.tls = ServerTls { enabled: true, ..Default::default() };
    let (running, events) = start(server).await;
    assert!(running.url.starts_with("https://"));
    let insecure = RequestOptions { tls: TlsOptions { verify: false, ..Default::default() }, ..opts() };
    let url = format!("https://localhost:{}/v", running.addr.port());
    let r = send_with("GET", &url, &[], "", &insecure).await.unwrap();
    assert_eq!((r.meta.http_version.as_str(), r.body.as_slice()), ("HTTP/2", b"secure".as_slice()));
    let http1 = RequestOptions { http_version: HttpVersionPref::Http1, ..insecure };
    assert_eq!(send_with("GET", &url, &[], "", &http1).await.unwrap().meta.http_version, "HTTP/1.1");
    let x = wait_http(&events, "/v").await;
    assert_eq!(x.http_version, "HTTP/2");

    // Plain HTTP to a TLS listener: the handshake fails and is reported.
    let mut c = TcpStream::connect(running.addr).await.unwrap();
    c.write_all(b"GET /v HTTP/1.1\r\nHost: x\r\n\r\n").await.unwrap();
    events.wait("TLS error", |e| e.kind == TrafficKind::Error && e.summary.contains("TLS")).await;
}

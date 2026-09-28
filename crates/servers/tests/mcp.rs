//! MCP server with Zorvik's MCP client over Streamable HTTP and HTTP+SSE: initialize, tools
//! (templates, missing arguments, errors, structured results), resources and templates,
//! prompts, change notifications, sessions, and the raw HTTP rules.

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{Events, start};
use serde_json::{Value, json};
use zorvik_engine::mcp::{McpConnected, McpEvent, McpTarget, McpTransport, list_all};
use zorvik_engine::{Client, Direction, Header, HttpRequest, RequestOptions};
use zorvik_formats::{
    McpPromptArgument, McpPromptMessage, McpPromptMock, McpResourceMock, McpToolMock, Server, ServerKind,
};
use zorvik_servers::TrafficKind;

const WAIT: Duration = Duration::from_secs(5);

fn weather_server() -> Server {
    let mut server = Server::new("Weather", ServerKind::Mcp);
    server.mcp.instructions = "Ask for the weather.".into();
    server.mcp.tools = vec![
        McpToolMock {
            name: "get_weather".into(),
            description: "Today's weather in a city".into(),
            input_schema: r#"{"type": "object", "properties": {"city": {"type": "string"}}, "required": ["city"]}"#
                .into(),
            result: "Sunny in {{args.city}}, id {{$uuid}}".into(),
            ..Default::default()
        },
        McpToolMock {
            name: "forecast".into(),
            output_schema: r#"{"type": "object"}"#.into(),
            result: r#"{"city": "{{args.city}}", "days": {{args.days}}}"#.into(),
            ..Default::default()
        },
        McpToolMock {
            name: "broken".into(),
            result: "The weather station is down".into(),
            is_error: true,
            ..Default::default()
        },
        McpToolMock { name: "hidden".into(), enabled: false, ..Default::default() },
    ];
    server.mcp.resources = vec![
        McpResourceMock {
            uri: "weather://stations".into(),
            name: "Stations".into(),
            text: "Lisbon, Porto".into(),
            ..Default::default()
        },
        McpResourceMock {
            uri: "weather://{city}/today".into(),
            name: "Today".into(),
            mime_type: "application/json".into(),
            text: r#"{"city": "{{params.city}}"}"#.into(),
            ..Default::default()
        },
    ];
    server.mcp.prompts = vec![McpPromptMock {
        name: "plan_trip".into(),
        description: "Plan a trip".into(),
        arguments: vec![McpPromptArgument { name: "city".into(), required: true, ..Default::default() }],
        messages: vec![McpPromptMessage { role: String::new(), text: "Plan a day in {{args.city}}".into() }],
        ..Default::default()
    }];
    server
}

async fn connect(url: &str, transport: McpTransport) -> McpConnected {
    let target = McpTarget { address: url.into(), transport, headers: Vec::new(), env: Vec::new(), cwd: None };
    Arc::new(Client::new()).mcp(target, &RequestOptions::default(), None).await.expect("connects")
}

async fn exercise(conn: &McpConnected) {
    let client = &conn.client;
    assert_eq!(conn.info.name, "Weather");
    assert_eq!(conn.info.protocol_version, "2025-11-25");
    assert_eq!(conn.info.instructions.as_deref(), Some("Ask for the weather."));
    assert_eq!(conn.info.capabilities["tools"]["listChanged"], true);

    let tools = list_all(client, "tools/list", "tools", WAIT, 10).await.unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["get_weather", "forecast", "broken"]);
    assert_eq!(tools[0]["inputSchema"]["required"], json!(["city"]));

    let result = client
        .request("tools/call", json!({ "name": "get_weather", "arguments": { "city": "Lisbon" } }), WAIT)
        .await
        .unwrap();
    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(text.starts_with("Sunny in Lisbon, id ") && text.len() == "Sunny in Lisbon, id ".len() + 36, "{text}");
    let missing = client.request("tools/call", json!({ "name": "get_weather", "arguments": {} }), WAIT).await.unwrap();
    assert_eq!(
        (missing["isError"].clone(), missing["content"][0]["text"].clone()),
        (json!(true), json!("Missing required argument: city"))
    );
    let structured = client
        .request("tools/call", json!({ "name": "forecast", "arguments": { "city": "Porto", "days": 3 } }), WAIT)
        .await
        .unwrap();
    assert_eq!(structured["structuredContent"], json!({ "city": "Porto", "days": 3 }));
    let broken = client.request("tools/call", json!({ "name": "broken" }), WAIT).await.unwrap();
    assert_eq!(broken["isError"], true);
    let unknown = client.request("tools/call", json!({ "name": "hidden" }), WAIT).await.unwrap_err();
    assert_eq!((unknown.code, unknown.message.as_str()), (-32602, "Unknown tool: hidden"));

    let resources = list_all(client, "resources/list", "resources", WAIT, 10).await.unwrap();
    assert_eq!(resources, [json!({ "uri": "weather://stations", "name": "Stations", "mimeType": "text/plain" })]);
    let templates = list_all(client, "resources/templates/list", "resourceTemplates", WAIT, 10).await.unwrap();
    assert_eq!(templates[0]["uriTemplate"], "weather://{city}/today");
    let read = client.request("resources/read", json!({ "uri": "weather://Faro/today" }), WAIT).await.unwrap();
    assert_eq!(
        read["contents"][0],
        json!({ "uri": "weather://Faro/today", "mimeType": "application/json", "text": r#"{"city": "Faro"}"# })
    );
    let nothing = client.request("resources/read", json!({ "uri": "weather://nowhere" }), WAIT).await.unwrap_err();
    assert_eq!(nothing.code, -32002);

    let prompts = list_all(client, "prompts/list", "prompts", WAIT, 10).await.unwrap();
    assert_eq!(prompts[0]["arguments"], json!([{ "name": "city", "required": true }]));
    let prompt = client
        .request("prompts/get", json!({ "name": "plan_trip", "arguments": { "city": "Braga" } }), WAIT)
        .await
        .unwrap();
    assert_eq!(
        prompt["messages"],
        json!([{ "role": "user", "content": { "type": "text", "text": "Plan a day in Braga" } }])
    );
    let method = client.request("sampling/nope", Value::Null, WAIT).await.unwrap_err();
    assert_eq!(method.code, -32601);
}

async fn next_event(conn: &mut McpConnected, what: impl Fn(&McpEvent) -> bool) -> McpEvent {
    loop {
        let event = tokio::time::timeout(WAIT, conn.events.recv()).await.expect("an event in time").expect("open");
        if what(&event) {
            return event;
        }
    }
}

#[tokio::test]
async fn streamable_http_and_sessions() {
    let (running, events) = start(weather_server()).await;
    assert!(running.url.ends_with("/mcp"), "{}", running.url);
    let mut conn = connect(&running.url, McpTransport::Auto).await;
    assert_eq!(conn.info.transport, "Streamable HTTP");
    assert!(conn.info.session_id.as_deref().is_some_and(|id| id.len() == 24));
    exercise(&conn).await;

    // A changed tool list reaches the client on its GET stream.
    let mut changed = weather_server();
    changed.port = 0;
    changed.mcp.tools.pop();
    assert!(running.update(changed, Default::default()));
    let notice = next_event(&mut conn, |e| matches!(e, McpEvent::Message { method: Some(m), direction: Direction::Received, .. } if m == "notifications/tools/list_changed")).await;
    assert!(matches!(notice, McpEvent::Message { .. }));

    // The log: a connection per session, each request and answer.
    let traffic = events.traffic();
    assert!(traffic.iter().any(|e| e.kind == TrafficKind::Open));
    assert!(traffic.iter().any(|e| e.summary == "tools/call get_weather"));
    assert!(traffic.iter().any(|e| e.summary == "tools/call broken failed (isError)"));

    // Closing deletes the session.
    drop(conn);
    events
        .wait("the session's end", |e| {
            e.kind == TrafficKind::Close && e.summary.contains("the client ended the session")
        })
        .await;
}

#[tokio::test]
async fn http_sse_transport_and_the_fallback_to_it() {
    let (running, events) = start(weather_server()).await;
    let base = running.url.trim_end_matches("/mcp").to_string();
    let conn = connect(&format!("{base}/sse"), McpTransport::Sse).await;
    assert_eq!(conn.info.transport, "HTTP+SSE");
    exercise(&conn).await;
    drop(conn);
    events.wait("the client leaving", |e| e.kind == TrafficKind::Close).await;

    // Auto: /sse refuses a POST (404 here), so the client falls back to HTTP+SSE by itself.
    let mut conn = connect(&format!("{base}/sse"), McpTransport::Auto).await;
    assert_eq!(conn.info.transport, "HTTP+SSE");
    let note = next_event(&mut conn, |e| matches!(e, McpEvent::Info { .. })).await;
    match note {
        McpEvent::Info { text, .. } => assert!(text.contains("older HTTP+SSE transport"), "{text}"),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn http_rules() {
    let (running, _events) = start(weather_server()).await;
    let client = Client::new();
    let opts = RequestOptions::default();
    let post = |body: &str, session: Option<&str>| {
        let mut headers = vec![
            Header::new("Content-Type", "application/json"),
            Header::new("Accept", "application/json, text/event-stream"),
        ];
        if let Some(s) = session {
            headers.push(Header::new("Mcp-Session-Id", s));
        }
        HttpRequest { method: "POST".into(), url: running.url.clone(), headers, body: body.as_bytes().to_vec().into() }
    };
    // A notification is accepted without an answer.
    let r = client
        .send(post(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#, None), &opts, None)
        .await
        .unwrap();
    assert_eq!(r.meta.status, 202);
    // An unknown session must initialize again.
    let r = client.send(post(r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#, Some("nope")), &opts, None).await.unwrap();
    assert_eq!(r.meta.status, 404);
    // Not JSON: a parse error.
    let r = client.send(post("{oops", None), &opts, None).await.unwrap();
    assert_eq!(r.meta.status, 400);
    assert_eq!(serde_json::from_slice::<Value>(&r.body).unwrap()["error"]["code"], -32700);
    // A batch gets a batch.
    let r = client
        .send(
            post(r#"[{"jsonrpc":"2.0","id":1,"method":"ping"},{"jsonrpc":"2.0","id":2,"method":"ping"}]"#, None),
            &opts,
            None,
        )
        .await
        .unwrap();
    assert_eq!(serde_json::from_slice::<Value>(&r.body).unwrap().as_array().map(Vec::len), Some(2));
    // Elsewhere: where to connect.
    let get = HttpRequest {
        method: "GET".into(),
        url: running.url.replace("/mcp", "/other"),
        headers: Vec::new(),
        body: Default::default(),
    };
    let r = client.send(get, &opts, None).await.unwrap();
    assert_eq!(r.meta.status, 404);
    assert!(String::from_utf8_lossy(&r.body).contains("clients connect to /mcp"));
    // Without CORS, a browser's preflight gets no permission.
    let preflight = HttpRequest {
        method: "OPTIONS".into(),
        url: running.url.clone(),
        headers: vec![
            Header::new("Origin", "https://evil.example"),
            Header::new("Access-Control-Request-Method", "POST"),
        ],
        body: Default::default(),
    };
    let r = client.send(preflight, &opts, None).await.unwrap();
    assert!(!r.meta.headers.iter().any(|h| h.name.eq_ignore_ascii_case("access-control-allow-origin")));
}

#[tokio::test]
async fn stdio_serving() {
    let (client_end, server_end) = tokio::io::duplex(1 << 16);
    let (server_read, server_write) = tokio::io::split(server_end);
    let events = Events::default();
    let sink = events.clone();
    let reporter = zorvik_servers::Reporter::new(move |e| sink.0.lock().unwrap().push(e));
    let task = tokio::spawn(zorvik_servers::mcp::serve_stdio(
        weather_server(),
        Default::default(),
        reporter,
        server_read,
        server_write,
    ));
    let (client_read, mut client_write) = tokio::io::split(client_end);
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
    client_write
        .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2025-06-18\"}}\n")
        .await
        .unwrap();
    client_write.write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n").await.unwrap();
    client_write.write_all(b"{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{\"name\":\"get_weather\",\"arguments\":{\"city\":\"Evora\"}}}\n").await.unwrap();
    let mut lines = tokio::io::BufReader::new(client_read).lines();
    let init: Value = serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
    let call: Value = serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
    assert!(call["result"]["content"][0]["text"].as_str().unwrap().starts_with("Sunny in Evora"));
    // The pipe closes once both of its halves are gone.
    drop(client_write);
    drop(lines);
    tokio::time::timeout(WAIT, task).await.unwrap().unwrap().unwrap();
    assert!(events.traffic().iter().any(|e| e.kind == TrafficKind::Close && e.summary.contains("stdin closed")));
}

#[tokio::test]
async fn a_call_given_up_on_is_cancelled_on_the_server() {
    let mut server = weather_server();
    server.mcp.tools.push(McpToolMock {
        name: "slow".into(),
        result: "late".into(),
        delay_ms: 5_000,
        ..Default::default()
    });
    let (running, events) = start(server).await;
    let conn = connect(&running.url, McpTransport::StreamableHttp).await;
    let answer = conn.client.request("tools/call", json!({ "name": "slow" }), Duration::from_millis(200)).await;
    assert!(answer.unwrap_err().message.contains("didn't answer"));
    events.wait("the cancellation", |e| e.summary == "notifications/cancelled").await;

    // A dropped call (the caller cancelled) is cancelled too.
    let call = conn.client.request("tools/call", json!({ "name": "slow" }), WAIT);
    assert!(tokio::time::timeout(Duration::from_millis(200), call).await.is_err());
    events
        .wait("the second cancellation", |e| {
            e.summary == "notifications/cancelled" && e.text.as_deref().is_some_and(|t| t.contains("Cancelled"))
        })
        .await;
}

#[tokio::test]
async fn an_sse_endpoint_on_another_site_is_refused() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    // A server whose event stream says to post messages to another host.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buf = [0u8; 4096];
        let _ = socket.read(&mut buf).await;
        let body = "event: endpoint\ndata: http://elsewhere.test/messages\n\n";
        let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\n\r\n";
        socket.write_all(head.as_bytes()).await.unwrap();
        socket.write_all(body.as_bytes()).await.unwrap();
        tokio::time::sleep(Duration::from_secs(2)).await;
    });
    let target = McpTarget {
        address: format!("http://127.0.0.1:{port}/sse"),
        transport: McpTransport::Sse,
        headers: vec![Header::new("Authorization", "Bearer secret")],
        env: Vec::new(),
        cwd: None,
    };
    let error = Arc::new(Client::new()).mcp(target, &RequestOptions::default(), None).await.err().expect("refused");
    assert!(error.message.contains("another site"), "{}", error.message);
}

#[cfg(unix)]
#[tokio::test]
async fn a_program_that_fails_to_start_says_why() {
    let target = McpTarget {
        address: "sh -c 'echo \"Missing API_KEY\" >&2; exit 3'".into(),
        transport: McpTransport::Stdio,
        headers: Vec::new(),
        env: Vec::new(),
        cwd: None,
    };
    let error = Arc::new(Client::new()).mcp(target, &RequestOptions::default(), None).await.err().expect("fails");
    assert!(error.message.contains("exited with code 3"), "{}", error.message);
    assert!(error.message.contains("Missing API_KEY"), "{}", error.message);

    let missing = McpTarget {
        address: "sh -c true".into(),
        transport: McpTransport::Stdio,
        headers: Vec::new(),
        env: Vec::new(),
        cwd: Some("/no/such/folder/zorvik".into()),
    };
    let error = Arc::new(Client::new()).mcp(missing, &RequestOptions::default(), None).await.err().expect("fails");
    assert!(error.message.contains("doesn't exist"), "{}", error.message);
}

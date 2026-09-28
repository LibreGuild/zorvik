//! Socket.IO server with Zorvik's Socket.IO client over both transports, and the raw
//! Engine.IO exchange socket.io-client makes by default (long-polling, then the upgrade to
//! WebSocket): namespaces, echo, rules, acknowledgements, broadcasts, emits from the UI,
//! binary arguments and disconnects.

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{Events, start};
use serde_json::{Value, json};
use zorvik_engine::socketio::{SocketIoConfig, SocketIoTransport};
use zorvik_engine::{
    Client, Direction, HttpRequest, RequestOptions, SocketConnected, SocketEvent, SocketOutgoing, WsEvent, WsOutgoing,
};
use zorvik_formats::{MatchKind, ReplyMode, Server, ServerKind, SocketIoRule};
use zorvik_servers::{OutgoingMessage, TrafficDirection, TrafficKind};

async fn connect(url: &str, transport: SocketIoTransport, auth: Option<Value>) -> SocketConnected {
    let request = HttpRequest { method: "GET".into(), url: url.into(), headers: Vec::new(), body: Default::default() };
    let config = SocketIoConfig { path: "/socket.io/".into(), auth, transport };
    Arc::new(Client::new()).socketio(request, config, &RequestOptions::default(), None).await.expect("connects")
}

/// The next received message as (event name, arguments JSON, detail), skipping notes.
async fn next(conn: &mut SocketConnected) -> (Option<String>, Value, Option<String>) {
    loop {
        let event = tokio::time::timeout(Duration::from_secs(5), conn.events.recv())
            .await
            .expect("an event in time")
            .expect("still open");
        match event {
            SocketEvent::Message { direction: Direction::Received, text, topic, detail, .. } => {
                return (topic, serde_json::from_str(&text.unwrap()).unwrap(), detail);
            }
            SocketEvent::Closed { reason, .. } => panic!("closed: {reason}"),
            SocketEvent::Error { message } => panic!("error: {message}"),
            _ => {}
        }
    }
}

fn emit(conn: &SocketConnected, event: &str, args: &str, ack: bool) {
    let message = SocketOutgoing::Emit { event: event.into(), args: args.into(), base64: None, ack };
    conn.session.send(message).unwrap();
}

async fn joined(events: &Events, count: usize) {
    for _ in 0..250 {
        if events.traffic().iter().filter(|e| e.summary.starts_with("joined")).count() >= count {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("{count} clients did not join: {:#?}", events.traffic());
}

#[tokio::test]
async fn echo_over_both_transports() {
    let mut server = Server::new("IO", ServerKind::SocketIo);
    server.socketio.greeting_event = "welcome".into();
    server.socketio.greeting_args = r#"{"id": "{{$uuid}}"}"#.into();
    let (running, events) = start(server).await;
    assert!(running.url.starts_with("http://127.0.0.1:"), "{}", running.url);
    for transport in [SocketIoTransport::Websocket, SocketIoTransport::Polling] {
        let mut io = connect(&format!("{}/chat", running.url), transport, Some(json!({ "token": "t" }))).await;
        assert!(io.opened.protocol.starts_with("Socket.IO over"), "{}", io.opened.protocol);
        let (event, args, _) = next(&mut io).await;
        assert_eq!(event.as_deref(), Some("welcome"));
        assert_eq!(args[0]["id"].as_str().unwrap().len(), 36);

        emit(&io, "chat", r#"["hi", {"n": 1}]"#, true);
        // The acknowledgement comes first, then the echoed event.
        let (event, args, detail) = next(&mut io).await;
        assert_eq!((event, args, detail.as_deref()), (None, json!(["hi", { "n": 1 }]), Some("acknowledgement of #1")));
        let (event, args, _) = next(&mut io).await;
        assert_eq!((event.as_deref(), args), (Some("chat"), json!(["hi", { "n": 1 }])));

        // A binary argument comes back as binary.
        io.session
            .send(SocketOutgoing::Emit {
                event: "file".into(),
                args: String::new(),
                base64: Some("AAEC".into()),
                ack: false,
            })
            .unwrap();
        let (event, args, _) = next(&mut io).await;
        assert_eq!((event.as_deref(), args), (Some("file"), json!([{ "base64": "AAEC", "bytes": 3 }])));
    }
    let traffic = events.traffic();
    let join = traffic.iter().find(|e| e.summary == "joined /chat").expect("joined");
    assert_eq!(join.text.as_deref(), Some(r#"{"token":"t"}"#));
    assert!(
        traffic
            .iter()
            .any(|e| e.summary == "/chat chat (asks for acknowledgement #1)"
                && e.direction == Some(TrafficDirection::In))
    );
}

#[tokio::test]
async fn rules_acknowledge_reply_and_broadcast() {
    let mut server = Server::new("IO", ServerKind::SocketIo);
    server.socketio.mode = ReplyMode::Rules;
    server.socketio.rules = vec![
        SocketIoRule {
            event: "join".into(),
            ack: r#"{"ok": true, "room": {{event.arg0}}}"#.into(),
            ..Default::default()
        },
        SocketIoRule {
            event: "say".into(),
            matcher: MatchKind::Contains,
            pattern: "hello".into(),
            reply_event: "said".into(),
            reply_args: r#"[{{event.arg0}}, "{{$randomInt(1, 1)}}"]"#.into(),
            broadcast: true,
            delay_ms: 30,
            ..Default::default()
        },
        SocketIoRule {
            event: "*".into(),
            reply_event: "unknown".into(),
            reply_args: "{{event.name}}".into(),
            ..Default::default()
        },
    ];
    let (running, events) = start(server).await;
    let mut a = connect(&running.url, SocketIoTransport::Auto, None).await;
    let mut b = connect(&running.url, SocketIoTransport::Polling, None).await;
    joined(&events, 2).await;

    emit(&a, "join", r#""lobby""#, true);
    let (_, args, detail) = next(&mut a).await;
    assert_eq!((args, detail.as_deref()), (json!([{ "ok": true, "room": "lobby" }]), Some("acknowledgement of #1")));

    emit(&b, "say", r#""hello all""#, false);
    for io in [&mut a, &mut b] {
        let (event, args, _) = next(io).await;
        assert_eq!((event.as_deref(), args), (Some("said"), json!(["hello all", "1"])));
    }
    emit(&a, "dance", "", false);
    let (event, args, _) = next(&mut a).await;
    assert_eq!((event.as_deref(), args), (Some("unknown"), json!(["dance"])));
}

#[tokio::test]
async fn emits_from_the_ui_and_disconnects() {
    let (running, events) = start(Server::new("IO", ServerKind::SocketIo)).await;
    let mut main = connect(&running.url, SocketIoTransport::Websocket, None).await;
    let mut admin = connect(&format!("{}/admin", running.url), SocketIoTransport::Polling, None).await;
    joined(&events, 2).await;

    let emit_to = |namespace: &str| OutgoingMessage::Emit {
        event: "news".into(),
        args: r#"[1, "{{$uuid}}"]"#.into(),
        namespace: namespace.into(),
    };
    assert_eq!(running.send(None, emit_to("/")).await, Ok(1));
    let (event, args, _) = next(&mut main).await;
    assert_eq!((event.as_deref(), &args[0]), (Some("news"), &json!(1)));
    assert_eq!(args[1].as_str().unwrap().len(), 36);
    assert_eq!(running.send(None, emit_to("admin")).await, Ok(1));
    assert_eq!(next(&mut admin).await.0.as_deref(), Some("news"));
    let e = running.send(None, emit_to("/nobody")).await.unwrap_err();
    assert_eq!(e.message, "No client has joined /nobody");
    // Plain text is the "message" event, like socket.send().
    assert_eq!(running.send(None, OutgoingMessage::Text { text: "hey".into() }).await, Ok(1));
    assert_eq!(next(&mut main).await, (Some("message".into()), json!(["hey"]), None));
    let bad = OutgoingMessage::Emit { event: "x".into(), args: "{oops".into(), namespace: String::new() };
    assert!(running.send(None, bad).await.unwrap_err().message.contains("not valid JSON"));

    // Disconnect the polling client from the server side.
    let conn = events.traffic().iter().find(|e| e.summary == "joined /admin").unwrap().conn.unwrap();
    running.disconnect(conn);
    let closed = loop {
        match tokio::time::timeout(Duration::from_secs(5), admin.events.recv()).await.unwrap() {
            Some(SocketEvent::Closed { reason, .. }) => break reason,
            Some(_) => {}
            None => panic!("no close"),
        }
    };
    assert_eq!(closed, "The server disconnected this client");
    events
        .wait("the close", |e| {
            e.kind == TrafficKind::Close && e.conn == Some(conn) && e.summary.contains("closed by you")
        })
        .await;

    // The client leaving is logged too.
    drop(main);
    events.wait("the main client leaving", |e| e.kind == TrafficKind::Close && e.conn != Some(conn)).await;
}

/// What socket.io-client does by default: open with long-polling, then upgrade to WebSocket.
#[tokio::test]
async fn polling_then_upgrade_like_socket_io_client() {
    let (running, events) = start(Server::new("IO", ServerKind::SocketIo)).await;
    let client = Client::new();
    let opts = RequestOptions::default();
    let base = format!("{}/socket.io/?EIO=4", running.url);
    let get = |url: String| HttpRequest { method: "GET".into(), url, headers: Vec::new(), body: Default::default() };
    let open = client.send(get(format!("{base}&transport=polling")), &opts, None).await.unwrap();
    let open = String::from_utf8(open.body).unwrap();
    assert!(open.starts_with('0'), "{open}");
    let handshake: Value = serde_json::from_str(&open[1..]).unwrap();
    assert_eq!(handshake["upgrades"], json!(["websocket"]));
    let sid = handshake["sid"].as_str().unwrap().to_string();
    let session = format!("{base}&transport=polling&sid={sid}");

    let post = |body: &str| HttpRequest {
        method: "POST".into(),
        url: session.clone(),
        headers: vec![zorvik_engine::Header::new("Content-Type", "text/plain;charset=UTF-8")],
        body: body.as_bytes().to_vec().into(),
    };
    let ok = client.send(post("40"), &opts, None).await.unwrap();
    assert_eq!(ok.body, b"ok");
    let answer = client.send(get(session.clone()), &opts, None).await.unwrap();
    assert!(String::from_utf8(answer.body).unwrap().starts_with("40{\"sid\":"));

    // The upgrade: probe, a poll ended with a noop, then "5".
    let ws_url = format!("{}&transport=websocket&sid={sid}", base.replace("http://", "ws://"));
    let mut ws = client.websocket(get(ws_url), &opts, None).await.unwrap();
    let waiting = tokio::spawn({
        let session = session.clone();
        async move { Client::new().send(get(session), &RequestOptions::default(), None).await.unwrap() }
    });
    ws.session.send(WsOutgoing::Text { text: "2probe".into() }).unwrap();
    assert_eq!(next_ws(&mut ws).await, "3probe");
    assert_eq!(waiting.await.unwrap().body, b"6");
    ws.session.send(WsOutgoing::Text { text: "5".into() }).unwrap();
    ws.session.send(WsOutgoing::Text { text: r#"42["ping",1]"#.into() }).unwrap();
    assert_eq!(next_ws(&mut ws).await, r#"42["ping",1]"#);
    // Polling is over for this session: POSTs still reach it, the echo comes over WebSocket.
    client.send(post(r#"42["late"]"#), &opts, None).await.unwrap();
    assert_eq!(next_ws(&mut ws).await, r#"42["late"]"#);

    // Errors as socket.io words them.
    let unknown = client.send(get(format!("{base}&transport=polling&sid=nope")), &opts, None).await.unwrap();
    assert_eq!(
        (unknown.meta.status, String::from_utf8(unknown.body).unwrap()),
        (400, r#"{"code":1,"message":"Session ID unknown"}"#.to_string())
    );
    let old =
        client.send(get(format!("{}/socket.io/?EIO=3&transport=polling", running.url)), &opts, None).await.unwrap();
    assert!(String::from_utf8(old.body).unwrap().contains("Unsupported protocol version"));
    let elsewhere = client.send(get(format!("{}/other", running.url)), &opts, None).await.unwrap();
    assert_eq!(elsewhere.meta.status, 404);
    assert_eq!(events.traffic().iter().filter(|e| e.kind == TrafficKind::Open).count(), 1);
}

async fn next_ws(ws: &mut zorvik_engine::WsConnected) -> String {
    loop {
        match tokio::time::timeout(Duration::from_secs(5), ws.events.recv()).await.unwrap().unwrap() {
            WsEvent::Message { direction: Direction::Received, text: Some(t), .. } if t != "2" => return t,
            WsEvent::Closed { reason, .. } => panic!("closed: {reason}"),
            _ => {}
        }
    }
}

#[tokio::test]
async fn clients_refused_or_on_old_servers_get_clear_errors() {
    // Zorvik's client against a server that only takes polling falls back by itself.
    let (running, _events) = start(Server::new("IO", ServerKind::SocketIo)).await;
    let request = HttpRequest {
        method: "GET".into(),
        url: format!("{}/nope", running.url),
        headers: Vec::new(),
        body: Default::default(),
    };
    let config = SocketIoConfig { path: "/elsewhere/".into(), auth: None, transport: SocketIoTransport::Auto };
    let Err(e) = Arc::new(Client::new()).socketio(request, config, &RequestOptions::default(), None).await else {
        panic!("connected to the wrong path");
    };
    assert!(e.message.contains("404") || e.message.contains("refused"), "{}", e.message);
}

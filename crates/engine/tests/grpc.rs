//! gRPC against the testkit's echo service: reflection (v1 and the v1alpha
//! fallback), `.proto` files, unary and streaming calls, errors, metadata,
//! compression, deadlines, TLS and a proxy tunnel.

use std::sync::atomic::Ordering;
use std::time::Duration;

use serde_json::Value;
use tokio::sync::mpsc::UnboundedReceiver;
use zorvik_engine::grpc::{GrpcDescriptors, GrpcEvent, GrpcMessage, GrpcStatus, GrpcTarget};
use zorvik_engine::{Client, Direction, ErrorKind, Header, ProxyMode, ProxySettings, RequestOptions, TlsOptions};
use zorvik_testkit::grpc::{ECHO_PROTO, Reflection, grpc_proto_dir};
use zorvik_testkit::{GrpcTestServer, TestCerts, TestProxy, TestServer};

fn target(url: &str) -> GrpcTarget {
    GrpcTarget { url: url.into(), metadata: Vec::new() }
}

async fn reflect(server: &GrpcTestServer) -> GrpcDescriptors {
    Client::new().grpc_reflect(&target(&server.url()), &RequestOptions::default()).await.unwrap()
}

fn json(message: &GrpcMessage) -> Value {
    serde_json::from_str(&message.json).unwrap()
}

async fn next(events: &mut UnboundedReceiver<GrpcEvent>) -> GrpcEvent {
    tokio::time::timeout(Duration::from_secs(5), events.recv()).await.expect("event in time").expect("channel open")
}

/// Events until (and including) `End`: received messages and the final status.
async fn until_end(events: &mut UnboundedReceiver<GrpcEvent>) -> (Vec<Value>, GrpcStatus) {
    let mut received = Vec::new();
    loop {
        match next(events).await {
            GrpcEvent::Message { message } if message.direction == Direction::Received => received.push(json(&message)),
            GrpcEvent::End { status, .. } => return (received, status),
            _ => {}
        }
    }
}

#[tokio::test]
async fn reflection_describes_the_services() {
    let server = GrpcTestServer::start().await;
    let descriptors = reflect(&server).await;
    assert_eq!(descriptors.source, "reflection v1");
    let services = descriptors.describe();
    // The reflection service itself is left out.
    assert_eq!(services.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["zorvik.test.v1.Echo"]);
    let methods: Vec<_> =
        services[0].methods.iter().map(|m| (m.name.as_str(), m.client_streaming, m.server_streaming)).collect();
    assert_eq!(
        methods,
        [
            ("Unary", false, false),
            ("ServerStream", false, true),
            ("ClientStream", true, false),
            ("Bidi", true, true),
            ("Fail", false, false),
            ("Metadata", false, false),
        ]
    );
    let unary = &services[0].methods[0];
    assert_eq!(unary.path, "zorvik.test.v1.Echo/Unary");
    assert_eq!(unary.input_type, "zorvik.test.v1.EchoRequest");
    // Types from the imported file (fetched by name) are in the example.
    let example: Value = serde_json::from_str(&unary.example).unwrap();
    assert_eq!(example["mood"], "MOOD_UNSPECIFIED");
    assert_eq!(example["tags"][0]["parent"]["name"], "");
    assert_eq!(example["at"], "1970-01-01T00:00:00Z");
}

#[tokio::test]
async fn reflection_falls_back_to_v1alpha_and_explains_when_missing() {
    let alpha = GrpcTestServer::bind("127.0.0.1:0".parse().unwrap(), None, Reflection::V1AlphaOnly).await;
    assert_eq!(reflect(&alpha).await.source, "reflection v1alpha");

    let none = GrpcTestServer::bind("127.0.0.1:0".parse().unwrap(), None, Reflection::Off).await;
    let err = Client::new().grpc_reflect(&target(&none.url()), &RequestOptions::default()).await.unwrap_err();
    assert_eq!(err.kind, ErrorKind::Protocol);
    assert!(err.message.contains("does not offer reflection") && err.message.contains(".proto"), "{}", err.message);
}

#[tokio::test]
async fn unary_call_with_proto_files() {
    let server = GrpcTestServer::bind("127.0.0.1:0".parse().unwrap(), None, Reflection::Off).await;
    let dir = grpc_proto_dir();
    let descriptors = GrpcDescriptors::from_proto_files(&[dir.join(ECHO_PROTO)], std::slice::from_ref(&dir)).unwrap();
    assert_eq!(descriptors.source, "proto files");
    assert!(descriptors.files.len() >= 2, "the imported file counts too: {:?}", descriptors.files);
    let method = descriptors.method("zorvik.test.v1.Echo/Unary").unwrap();
    let message =
        r#"{"message": "hi", "mood": "HAPPY", "labels": {"a": "b"}, "number": 7, "at": "2026-09-27T10:00:00Z"}"#;
    let response =
        Client::new().grpc_unary(&target(&server.url()), &method, message, &RequestOptions::default()).await.unwrap();
    assert!(response.status.is_ok(), "{:?}", response.status);
    assert_eq!(response.status.name, "OK");
    assert_eq!(response.messages.len(), 1);
    let reply = json(&response.messages[0]);
    assert_eq!(reply["message"], "hi");
    assert_eq!(reply["request"]["mood"], "HAPPY");
    assert_eq!(reply["request"]["labels"]["a"], "b");
    assert_eq!(reply["request"]["number"], 7);
    assert_eq!(reply["request"]["at"], "2026-09-27T10:00:00Z");
    // Default values are shown too.
    assert_eq!(reply["request"]["count"], 0);
    assert!(response.headers.iter().any(|h| h.name == "content-type" && h.value == "application/grpc"));
    assert!(response.trailers.iter().any(|h| h.name == "grpc-status" && h.value == "0"));
    for name in ["content-type", "te", "grpc-timeout", "user-agent"] {
        assert!(response.request_headers.iter().any(|h| h.name == name), "{name} sent");
    }
    assert!(response.timing.total_ms > 0.0 && response.timing.ttfb_ms > 0.0);
    assert!(response.remote_addr.is_some() && response.tls.is_none());
}

#[tokio::test]
async fn errors_come_back_as_statuses_with_details() {
    let server = GrpcTestServer::start().await;
    let descriptors = reflect(&server).await;
    let fail = descriptors.method("zorvik.test.v1.Echo/Fail").unwrap();
    let response = Client::new()
        .grpc_unary(
            &target(&server.url()),
            &fail,
            r#"{"code": 7, "message": "nope: ✓ 100%"}"#,
            &RequestOptions::default(),
        )
        .await
        .unwrap();
    assert_eq!((response.status.code, response.status.name.as_str()), (7, "PERMISSION_DENIED"));
    assert_eq!(response.status.message, "nope: ✓ 100%");
    assert!(!response.status.local);
    // Trailers-only: no headers, the status arrives as trailers.
    assert!(response.headers.is_empty());
    assert!(response.trailers.iter().any(|h| h.name == "grpc-status" && h.value == "7"));
    let details: Value = serde_json::from_str(response.status.details.as_deref().unwrap()).unwrap();
    assert_eq!(details[0]["@type"], "type.googleapis.com/zorvik.test.v1.FailRequest");
    assert_eq!(details[0]["code"], 7, "a detail of a known type is decoded: {details}");
    assert!(response.messages.is_empty());

    // A method the server does not have: UNIMPLEMENTED.
    let dir = tempfile::tempdir().unwrap();
    let proto = dir.path().join("extra.proto");
    std::fs::write(
        &proto,
        "syntax = \"proto3\"; package zorvik.test.v1; message M {} service Echo { rpc Missing(M) returns (M); }",
    )
    .unwrap();
    let extra = GrpcDescriptors::from_proto_files(&[proto], &[]).unwrap();
    let missing = extra.method("zorvik.test.v1.Echo/Missing").unwrap();
    let response =
        Client::new().grpc_unary(&target(&server.url()), &missing, "{}", &RequestOptions::default()).await.unwrap();
    assert_eq!(response.status.name, "UNIMPLEMENTED");

    // Not a gRPC server: the HTTP status is mapped (404 → UNIMPLEMENTED).
    let http = TestServer::start().await;
    let unary = descriptors.method("zorvik.test.v1.Echo/Unary").unwrap();
    let url = format!("grpc://{}", http.addr);
    let response = Client::new().grpc_unary(&target(&url), &unary, "{}", &RequestOptions::default()).await.unwrap();
    assert_eq!(response.status.name, "UNIMPLEMENTED");
    assert!(response.status.local && response.status.message.contains("HTTP 404"), "{:?}", response.status);
}

#[tokio::test]
async fn invalid_messages_and_urls_fail_before_connecting() {
    let server = GrpcTestServer::start().await;
    let unary = reflect(&server).await.method("zorvik.test.v1.Echo/Unary").unwrap();
    // Port 9 (discard) on loopback is closed: an attempt to connect would fail differently.
    let client = Client::new();
    let closed = target("grpc://127.0.0.1:9");
    let opts = RequestOptions::default();
    let err = client.grpc_unary(&closed, &unary, r#"{"mesage": "typo"}"#, &opts).await.unwrap_err();
    assert_eq!(err.kind, ErrorKind::InvalidRequest);
    assert!(err.message.contains("mesage"), "{}", err.message);
    let err = client.grpc_unary(&target("ftp://h:1"), &unary, "{}", &RequestOptions::default()).await.unwrap_err();
    assert!(err.message.contains("grpc://"), "{}", err.message);
    let refused = client.grpc_unary(&target("grpc://127.0.0.1:9"), &unary, "{}", &RequestOptions::default()).await;
    assert_eq!(refused.unwrap_err().kind, ErrorKind::Connect);
}

#[tokio::test]
async fn metadata_compression_and_deadlines() {
    let server = GrpcTestServer::start().await;
    let descriptors = reflect(&server).await;
    let client = Client::new();
    let opts = RequestOptions::default();

    let metadata = descriptors.method("zorvik.test.v1.Echo/Metadata").unwrap();
    let mut t = target(&server.url());
    t.metadata = vec![
        Header::new("X-Api-Key", "k1"),
        Header::new("trace-bin", "hello"),
        Header::new("authorization", "Bearer t"),
    ];
    let response = client.grpc_unary(&t, &metadata, "{}", &opts).await.unwrap();
    assert!(response.status.is_ok(), "{:?}", response.status);
    let seen = json(&response.messages[0]);
    assert_eq!(seen["metadata"]["x-api-key"], "k1");
    assert_eq!(seen["metadata"]["trace-bin"], "aGVsbG8");
    assert_eq!(seen["metadata"]["authorization"], "Bearer t");
    assert!(seen["metadata"]["grpc-timeout"].as_str().unwrap().ends_with('m'));
    assert!(response.headers.iter().any(|h| h.name == "x-echo-header" && h.value == "header-value"));
    assert!(response.trailers.iter().any(|h| h.name == "x-echo-trailer" && h.value == "trailer-value"));

    // gzip-compressed answers are decoded.
    let unary = descriptors.method("zorvik.test.v1.Echo/Unary").unwrap();
    let mut t = target(&server.url());
    t.metadata = vec![Header::new("x-compress", "gzip")];
    let response = client.grpc_unary(&t, &unary, r#"{"message": "zipped"}"#, &opts).await.unwrap();
    assert!(response.status.is_ok(), "{:?}", response.status);
    assert_eq!(json(&response.messages[0])["message"], "zipped");

    // The request timeout is the deadline.
    let short = RequestOptions { timeout: Some(Duration::from_millis(300)), ..Default::default() };
    let response = client.grpc_unary(&target(&server.url()), &unary, r#"{"delayMs": "3000"}"#, &short).await.unwrap();
    assert_eq!(response.status.name, "DEADLINE_EXCEEDED");
    assert!(response.status.local);
    assert!(response.timing.total_ms < 2500.0);
}

#[tokio::test]
async fn server_client_and_bidi_streams() {
    let server = GrpcTestServer::start().await;
    let descriptors = reflect(&server).await;
    let client = Client::new();
    let opts = RequestOptions::default();
    let t = target(&server.url());

    // Server streaming: one message, half-close, several answers.
    let method = descriptors.method("zorvik.test.v1.Echo/ServerStream").unwrap();
    let mut stream = client.grpc_stream(&t, &method, &opts).await.unwrap();
    assert!(stream.opened.remote_addr.is_some());
    stream.session.send(r#"{"message": "tick", "count": 4}"#).unwrap();
    stream.session.end();
    let (received, status) = until_end(&mut stream.events).await;
    assert!(status.is_ok(), "{status:?}");
    let texts: Vec<_> = received.iter().map(|m| m["message"].as_str().unwrap().to_string()).collect();
    assert_eq!(texts, ["tick #1", "tick #2", "tick #3", "tick #4"]);
    assert!(stream.session.is_ended());
    assert!(stream.session.send("{}").is_err(), "no sending after the end");

    // Client streaming: several messages, one answer after the half-close.
    let method = descriptors.method("zorvik.test.v1.Echo/ClientStream").unwrap();
    let mut stream = client.grpc_stream(&t, &method, &opts).await.unwrap();
    for word in ["a", "b", "c"] {
        stream.session.send(&format!(r#"{{"message": "{word}"}}"#)).unwrap();
    }
    stream.session.end();
    assert!(stream.session.send("{}").unwrap_err().message.contains("ended"));
    let (received, status) = until_end(&mut stream.events).await;
    assert!(status.is_ok(), "{status:?}");
    assert_eq!(received.len(), 1);
    assert_eq!(received[0]["message"], "a,b,c");
    assert_eq!(received[0]["messages"], serde_json::json!(["a", "b", "c"]));

    // Bidi: each message answered right away; sent messages are events too.
    let method = descriptors.method("zorvik.test.v1.Echo/Bidi").unwrap();
    let mut stream = client.grpc_stream(&t, &method, &opts).await.unwrap();
    stream.session.send(r#"{"message": "ping"}"#).unwrap();
    let mut saw_headers = false;
    let mut sent = 0;
    let first = loop {
        match next(&mut stream.events).await {
            GrpcEvent::Headers { .. } => saw_headers = true,
            GrpcEvent::Message { message } if message.direction == Direction::Sent => sent += 1,
            GrpcEvent::Message { message } => break json(&message),
            other => panic!("unexpected {other:?}"),
        }
    };
    assert!(saw_headers && sent == 1);
    assert_eq!((first["message"].as_str(), first["index"].as_i64()), (Some("ping"), Some(0)));
    assert!(stream.session.send(r#"{"nope": 1}"#).unwrap_err().message.contains("nope"));
    stream.session.send(r#"{"message": "pong"}"#).unwrap();
    stream.session.end();
    let (received, status) = until_end(&mut stream.events).await;
    assert!(status.is_ok(), "{status:?}");
    assert_eq!(received.len(), 1);
    assert_eq!(received[0]["index"], 1);
}

#[tokio::test]
async fn cancelling_a_stream_ends_it() {
    let server = GrpcTestServer::start().await;
    let method = reflect(&server).await.method("zorvik.test.v1.Echo/Bidi").unwrap();
    let mut stream =
        Client::new().grpc_stream(&target(&server.url()), &method, &RequestOptions::default()).await.unwrap();
    stream.session.send(r#"{"message": "x"}"#).unwrap();
    stream.session.cancel();
    let (_, status) = until_end(&mut stream.events).await;
    assert_eq!(status.name, "CANCELLED");
    assert!(status.local);
    // Dropping the session of an open call cancels it as well.
    let mut stream =
        Client::new().grpc_stream(&target(&server.url()), &method, &RequestOptions::default()).await.unwrap();
    drop(stream.session);
    let (_, status) = until_end(&mut stream.events).await;
    assert_eq!(status.name, "CANCELLED");
}

#[tokio::test]
async fn tls_and_proxy_tunnel() {
    let certs = TestCerts::generate();
    let dir = tempfile::tempdir().unwrap();
    let server = GrpcTestServer::bind("127.0.0.1:0".parse().unwrap(), Some(&certs), Reflection::Both).await;
    assert!(server.url().starts_with("grpcs://"));
    let opts = RequestOptions {
        tls: TlsOptions { ca_cert_path: Some(certs.write_ca(dir.path())), ..Default::default() },
        ..Default::default()
    };
    let client = Client::new();
    let url = format!("grpcs://localhost:{}", server.addr.port());
    let descriptors = client.grpc_reflect(&target(&url), &opts).await.unwrap();
    let unary = descriptors.method("zorvik.test.v1.Echo/Unary").unwrap();
    let response = client.grpc_unary(&target(&url), &unary, r#"{"message": "secure"}"#, &opts).await.unwrap();
    assert!(response.status.is_ok(), "{:?}", response.status);
    let tls = response.tls.expect("tls info");
    assert_eq!(tls.alpn.as_deref(), Some("h2"));
    assert!(response.timing.tls_ms > 0.0);

    // Plaintext gRPC through an HTTP proxy uses a CONNECT tunnel.
    let plain = GrpcTestServer::start().await;
    let proxy = TestProxy::start_with_upstream(None, Some(plain.addr)).await;
    let proxied = RequestOptions {
        proxy: ProxySettings::from_mode(&ProxyMode::Manual { url: proxy.url(), bypass: String::new() }).unwrap(),
        ..Default::default()
    };
    let fake_host = target("grpc://echo.example.test:50051");
    let response = client.grpc_unary(&fake_host, &unary, r#"{"message": "via proxy"}"#, &proxied).await.unwrap();
    assert!(response.status.is_ok(), "{:?}", response.status);
    assert_eq!(json(&response.messages[0])["message"], "via proxy");
    assert_eq!(proxy.hits.load(Ordering::SeqCst), 1);
}

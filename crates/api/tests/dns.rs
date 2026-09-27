//! `dns.query` through the RPC API against a local UDP responder.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tokio::net::UdpSocket;
use zorvik_api::{Api, EventSink, StreamEvent};

struct NoEvents;

impl EventSink for NoEvents {
    fn emit(&self, _: StreamEvent) {}
}

/// Answers `*.example.test` A queries with 192.0.2.7, NXDOMAIN for other
/// names, and never answers `slow.test`. Hand-built wire format (RFC 1035).
async fn responder() -> SocketAddr {
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let addr = socket.local_addr().unwrap();
    tokio::spawn(async move {
        let mut buf = [0u8; 1500];
        loop {
            let Ok((n, peer)) = socket.recv_from(&mut buf).await else { return };
            let query = &buf[..n];
            // End of the question: the name's labels, then type and class.
            let mut i = 12;
            let mut labels = Vec::new();
            while i < n && query[i] != 0 {
                let len = usize::from(query[i]);
                labels.push(String::from_utf8_lossy(&query[i + 1..i + 1 + len]).to_lowercase());
                i += 1 + len;
            }
            let question_end = i + 5;
            let name = labels.join(".");
            if name == "slow.test" {
                continue;
            }
            let found = name.ends_with("example.test");
            let mut reply = query[..question_end].to_vec();
            reply[2] = 0x80 | (query[2] & 0x01); // QR, RD copied
            reply[3] = if found { 0x80 } else { 0x83 }; // RA, NOERROR / NXDOMAIN
            reply[6..12].copy_from_slice(&[0, u8::from(found), 0, 0, 0, 0]);
            if found {
                reply.extend_from_slice(&[0xc0, 0x0c, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4, 192, 0, 2, 7]);
            }
            let _ = socket.send_to(&reply, peer).await;
        }
    });
    addr
}

fn dns_request(name: &str, server: &str) -> Value {
    json!({ "name": "q", "kind": "dns", "seq": 0, "method": "A", "url": name, "dns": { "server": server } })
}

async fn api_with_workspace() -> (Api, tempfile::TempDir, tempfile::TempDir) {
    let data = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    let api = Api::new(data.path().to_path_buf(), Arc::new(NoEvents));
    api.call("workspace.create", json!({ "path": ws.path(), "name": "Test" })).await.unwrap();
    (api, data, ws)
}

#[tokio::test]
async fn query_with_environment_variables() {
    let addr = responder().await;
    let (api, _data, _ws) = api_with_workspace().await;
    let env = api
        .call(
            "env.create",
            json!({ "environment": { "name": "Dev", "variables": [
                { "key": "zone", "value": "example.test" },
                { "key": "dns", "value": addr.to_string() }
            ] } }),
        )
        .await
        .unwrap();
    api.call("env.setActive", json!({ "id": env })).await.unwrap();

    let r = api
        .call("dns.query", json!({ "request": dns_request("api.{{zone}}", "{{dns}}"), "path": null }))
        .await
        .unwrap();
    assert_eq!(r["question"]["name"], "api.example.test.");
    assert_eq!(r["answers"][0]["data"], "192.0.2.7");
    assert_eq!(r["answers"][0]["ttl"], 60);
    assert_eq!((r["rcode"].as_str(), r["protocol"].as_str()), (Some("NOERROR"), Some("UDP")));
    assert_eq!(r["server"], addr.to_string());
    assert_eq!(r["flags"]["ra"], true);
    assert_eq!(r["unresolved"], json!([]));

    // NXDOMAIN is an answer, not an error.
    let r = api.call("dns.query", json!({ "request": dns_request("nope.test", "{{dns}}") })).await.unwrap();
    assert_eq!((r["rcode"].as_str(), r["rcodeValue"].as_u64()), (Some("NXDOMAIN"), Some(3)));

    let err =
        api.call("dns.query", json!({ "request": dns_request("{{missing}}.test", "{{dns}}") })).await.unwrap_err();
    assert_eq!(err.code, "undefinedVariable", "{}", err.message);
}

#[tokio::test]
async fn works_without_a_workspace_and_can_be_cancelled() {
    let addr = responder().await;
    let data = tempfile::tempdir().unwrap();
    let api = Api::new(data.path().to_path_buf(), Arc::new(NoEvents));
    let r =
        api.call("dns.query", json!({ "request": dns_request("www.example.test", &addr.to_string()) })).await.unwrap();
    assert_eq!(r["answers"][0]["data"], "192.0.2.7");

    let started = Instant::now();
    let mut request = dns_request("slow.test", &addr.to_string());
    request["settings"] = json!({ "timeoutMs": 10000 });
    let query = {
        let api = api.clone();
        tokio::spawn(async move { api.call("dns.query", json!({ "requestId": "q1", "request": request })).await })
    };
    tokio::time::sleep(Duration::from_millis(200)).await;
    api.call("http.cancel", json!({ "requestId": "q1" })).await.unwrap();
    let err = query.await.unwrap().unwrap_err();
    assert_eq!(err.network_kind, Some(zorvik_engine::ErrorKind::Cancelled), "{}", err.message);
    assert!(started.elapsed() < Duration::from_secs(5));

    // The per-request timeout applies.
    let mut request = dns_request("slow.test", &addr.to_string());
    request["settings"] = json!({ "timeoutMs": 300 });
    let err = api.call("dns.query", json!({ "request": request })).await.unwrap_err();
    assert_eq!(err.network_kind, Some(zorvik_engine::ErrorKind::Timeout), "{}", err.message);
}

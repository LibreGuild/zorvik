//! Digest and NTLM against the testkit servers, over a plain HTTP/1.1 connection
//! driven by the client functions in `zorvik_workspace::auth`.

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use zorvik_testkit::TestServer;
use zorvik_workspace::auth::digest::{self, DigestAlgorithm, DigestChallenge, DigestCredentials, Qop};
use zorvik_workspace::auth::{ntlm, parse_challenges};

/// One kept-alive HTTP/1.1 connection.
struct Connection {
    stream: BufReader<TcpStream>,
    host: String,
}

struct Reply {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Reply {
    fn header_values(&self, name: &str) -> Vec<&str> {
        self.headers.iter().filter(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str()).collect()
    }

    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap()
    }
}

impl Connection {
    async fn open(server: &TestServer) -> Self {
        let stream = TcpStream::connect(server.addr).await.unwrap();
        Self { stream: BufReader::new(stream), host: server.addr.to_string() }
    }

    async fn send(&mut self, method: &str, target: &str, authorization: Option<&str>, body: &[u8]) -> Reply {
        let mut head =
            format!("{method} {target} HTTP/1.1\r\nHost: {}\r\nContent-Length: {}\r\n", self.host, body.len());
        if let Some(value) = authorization {
            head.push_str(&format!("Authorization: {value}\r\n"));
        }
        head.push_str("\r\n");
        let stream = self.stream.get_mut();
        stream.write_all(head.as_bytes()).await.unwrap();
        stream.write_all(body).await.unwrap();

        let mut line = String::new();
        self.stream.read_line(&mut line).await.unwrap();
        let status = line.split_whitespace().nth(1).unwrap().parse().unwrap();
        let mut headers = Vec::new();
        loop {
            line.clear();
            self.stream.read_line(&mut line).await.unwrap();
            let Some((k, v)) = line.trim_end().split_once(':') else { break };
            headers.push((k.trim().to_string(), v.trim().to_string()));
        }
        let mut reply = Reply { status, headers, body: Vec::new() };
        if let Some(len) = reply.header_values("content-length").first().map(|v| v.parse::<usize>().unwrap()) {
            reply.body = vec![0; len];
            self.stream.read_exact(&mut reply.body).await.unwrap();
        } else if reply.header_values("transfer-encoding").iter().any(|v| v.eq_ignore_ascii_case("chunked")) {
            loop {
                line.clear();
                self.stream.read_line(&mut line).await.unwrap();
                let size = usize::from_str_radix(line.trim(), 16).unwrap();
                let mut chunk = vec![0; size + 2];
                self.stream.read_exact(&mut chunk).await.unwrap();
                if size == 0 {
                    break;
                }
                reply.body.extend_from_slice(&chunk[..size]);
            }
        }
        reply
    }
}

#[tokio::test]
async fn digest_round_trips() {
    let server = TestServer::start().await;
    for (qop, algorithm, method, body) in [
        ("auth", "", "GET", &b""[..]),
        ("auth", "/SHA-256", "GET", b""),
        ("auth", "/MD5-sess", "PUT", b"some body"),
        ("auth-int", "/SHA-256-sess", "POST", b"{\"signed\": true}"),
        ("auth-int", "", "POST", b""),
    ] {
        let target = format!("/digest-auth/{qop}/alice/wonder%20land{algorithm}?x=1");
        let mut conn = Connection::open(&server).await;
        let first = conn.send(method, &target, None, body).await;
        assert_eq!(first.status, 401);
        let challenge = DigestChallenge::from_headers(&first.header_values("www-authenticate")).unwrap();
        let expected = DigestAlgorithm::parse(algorithm.trim_start_matches('/')).unwrap_or(DigestAlgorithm::Md5);
        assert_eq!(challenge.algorithm, expected);
        assert_eq!(challenge.qop, [if qop == "auth" { Qop::Auth } else { Qop::AuthInt }]);

        let creds = DigestCredentials { username: "alice", password: "wonder land" };
        let value = digest::authorization(&challenge, &creds, method, &target, body, "0a4f113b", 1).unwrap();
        let ok = conn.send(method, &target, Some(&value), body).await;
        assert_eq!(ok.status, 200, "{qop} {algorithm}: {value}");
        assert_eq!(ok.json()["user"], "alice");

        // Wrong password, or a body changed after signing with auth-int.
        let wrong = DigestCredentials { username: "alice", password: "wonderland" };
        let value = digest::authorization(&challenge, &wrong, method, &target, body, "0a4f113b", 2).unwrap();
        assert_eq!(conn.send(method, &target, Some(&value), body).await.status, 401);
        if qop == "auth-int" {
            let value = digest::authorization(&challenge, &creds, method, &target, body, "c", 3).unwrap();
            assert_eq!(conn.send(method, &target, Some(&value), b"tampered").await.status, 401);
        }
    }
}

#[tokio::test]
async fn digest_unicode_credentials_and_foreign_nonces() {
    let server = TestServer::start().await;
    let target = "/digest-auth/auth/j%C3%BCrgen/p%C3%A4ss%E2%9C%93/SHA-256";
    let mut conn = Connection::open(&server).await;
    let first = conn.send("GET", target, None, b"").await;
    let challenge = DigestChallenge::from_headers(&first.header_values("www-authenticate")).unwrap();
    let creds = DigestCredentials { username: "jürgen", password: "päss✓" };
    let value = digest::authorization(&challenge, &creds, "GET", target, b"", "abc", 1).unwrap();
    assert!(value.starts_with("Digest username*=UTF-8''j%C3%BCrgen, "), "{value}");
    let ok = conn.send("GET", target, Some(&value), b"").await;
    assert_eq!(ok.status, 200, "{value}");
    assert_eq!(ok.json()["user"], "jürgen");

    // A nonce the server never issued is refused.
    let forged = DigestChallenge { nonce: "made-up".into(), ..challenge };
    let value = digest::authorization(&forged, &creds, "GET", target, b"", "abc", 1).unwrap();
    assert_eq!(conn.send("GET", target, Some(&value), b"").await.status, 401);
}

/// Sends NEGOTIATE on `conn` and returns the server's challenge.
async fn ntlm_challenge(conn: &mut Connection, target: &str) -> ntlm::NtlmChallenge {
    let negotiate = format!("NTLM {}", ntlm::negotiate_message());
    let reply = conn.send("GET", target, Some(&negotiate), b"").await;
    assert_eq!(reply.status, 401);
    let challenges = parse_challenges(&reply.header_values("www-authenticate"));
    let token = challenges.iter().find(|c| c.is("NTLM")).and_then(|c| c.token.clone()).expect("an NTLM challenge");
    ntlm::parse_challenge(&token).unwrap()
}

#[tokio::test]
async fn ntlm_round_trip_on_one_connection() {
    let server = TestServer::start().await;
    let target = "/ntlm/CORP/alice/s3cret";
    let mut conn = Connection::open(&server).await;
    let plain = conn.send("GET", target, None, b"").await;
    assert_eq!((plain.status, plain.header_values("www-authenticate")), (401, vec!["NTLM"]));

    let challenge = ntlm_challenge(&mut conn, target).await;
    assert_eq!(challenge.target_name, "CORP");
    assert!(challenge.timestamp.is_some(), "the test server sends MsvAvTimestamp");
    let type3 = ntlm::authenticate_message(&challenge, "alice", "s3cret", "CORP", "WORKSTATION", [7; 8], None).unwrap();
    let ok = conn.send("GET", target, Some(&format!("NTLM {type3}")), b"").await;
    assert_eq!(ok.status, 200);
    assert_eq!(ok.json()["user"], "alice");

    // The same connection again: a new handshake works, a wrong password doesn't.
    let challenge = ntlm_challenge(&mut conn, target).await;
    let type3 = ntlm::authenticate_message(&challenge, "alice", "wrong", "CORP", "", [7; 8], None).unwrap();
    assert_eq!(conn.send("GET", target, Some(&format!("NTLM {type3}")), b"").await.status, 401);
}

#[tokio::test]
async fn ntlm_needs_the_same_connection() {
    let server = TestServer::start().await;
    let target = "/ntlm/CORP/alice/s3cret";
    let mut first = Connection::open(&server).await;
    let challenge = ntlm_challenge(&mut first, target).await;
    let type3 = ntlm::authenticate_message(&challenge, "alice", "s3cret", "CORP", "", [1; 8], None).unwrap();
    let mut second = Connection::open(&server).await;
    assert_eq!(second.send("GET", target, Some(&format!("NTLM {type3}")), b"").await.status, 401);
    // The challenge is still waiting on the first connection.
    assert_eq!(first.send("GET", target, Some(&format!("NTLM {type3}")), b"").await.status, 200);
}

#[tokio::test]
async fn ntlm_unicode_and_domain_in_the_user_name() {
    let server = TestServer::start().await;
    let target = "/ntlm/Dom%C3%A4ne/J%C3%BCrgen/p%C3%A4ssw%C3%B6rd%20%E2%9C%93";
    let mut conn = Connection::open(&server).await;
    let challenge = ntlm_challenge(&mut conn, target).await;
    let type3 = ntlm::authenticate_message(&challenge, "Domäne\\jürgen", "pässwörd ✓", "", "", [9; 8], None).unwrap();
    let ok = conn.send("POST", target, Some(&format!("NTLM {type3}")), b"payload").await;
    assert_eq!(ok.status, 200);
    assert_eq!(ok.json()["domain"], "Domäne");
}

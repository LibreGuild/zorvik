//! Auth types sent through the API the way the UI sends them: Digest and NTLM answer the
//! test server's challenge; the signing types reach the server signed, fresh every time.

use base64::Engine as _;
use serde_json::{Value, json};
use zorvik_api::{Api, EventSink, StreamEvent};
use zorvik_testkit::TestServer;

struct Quiet;

impl EventSink for Quiet {
    fn emit(&self, _: StreamEvent) {}
}

struct Harness {
    api: Api,
    _data: tempfile::TempDir,
    _ws: tempfile::TempDir,
}

impl Harness {
    async fn new() -> Self {
        let data = tempfile::tempdir().unwrap();
        let ws = tempfile::tempdir().unwrap();
        let api = Api::new(data.path().to_path_buf(), std::sync::Arc::new(Quiet));
        api.call("workspace.create", json!({ "path": ws.path(), "name": "Auth" })).await.unwrap();
        Self { api, _data: data, _ws: ws }
    }

    async fn send(&self, url: &str, auth: Value) -> Value {
        let request = json!({ "name": "r", "seq": 0, "method": "GET", "url": url, "auth": auth });
        match self.api.call("http.send", json!({ "requestId": "t", "request": request, "path": null })).await {
            Ok(v) => v,
            Err(e) => panic!("send failed: {} ({})", e.message, e.code),
        }
    }

    async fn send_err(&self, url: &str, auth: Value) -> zorvik_api::ApiError {
        let request = json!({ "name": "r", "seq": 0, "method": "GET", "url": url, "auth": auth });
        self.api.call("http.send", json!({ "requestId": "t", "request": request, "path": null })).await.unwrap_err()
    }
}

/// The request headers the echo endpoint saw, by lower-case name.
fn echoed(result: &Value) -> std::collections::HashMap<String, String> {
    let body: Value = serde_json::from_str(result["body"]["text"].as_str().unwrap()).unwrap();
    body["headers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| (h[0].as_str().unwrap().to_ascii_lowercase(), h[1].as_str().unwrap().to_string()))
        .collect()
}

const RSA_KEY: &str = include_str!("../../workspace/src/auth/testdata/rsa2048-pkcs8.pem");

#[tokio::test]
async fn digest_answers_the_challenge() {
    let server = TestServer::start().await;
    let h = Harness::new().await;
    for path in
        ["/digest-auth/auth/ada/s3cret", "/digest-auth/auth/ada/s3cret/SHA-256", "/digest-auth/auth-int/ada/s3cret"]
    {
        let r = h.send(&server.url(path), json!({ "type": "digest", "username": "ada", "password": "s3cret" })).await;
        assert_eq!(r["meta"]["status"], 200, "{path}: {}", r["body"]["text"]);
        let sent = r["meta"]["request"]["headers"].as_array().unwrap();
        assert!(
            sent.iter().any(|h| h["name"] == "Authorization" && h["value"].as_str().unwrap().starts_with("Digest "))
        );
    }
    let wrong = h
        .send(
            &server.url("/digest-auth/auth/ada/s3cret"),
            json!({ "type": "digest", "username": "ada", "password": "nope" }),
        )
        .await;
    assert_eq!(wrong["meta"]["status"], 401, "a wrong password gets the server's answer");
}

#[tokio::test]
async fn ntlm_handshake_on_one_connection() {
    let server = TestServer::start().await;
    let h = Harness::new().await;
    let url = server.url("/ntlm/CORP/ada/s3cret");
    let r = h.send(&url, json!({ "type": "ntlm", "username": "ada", "password": "s3cret", "domain": "CORP" })).await;
    assert_eq!(r["meta"]["status"], 200, "{}", r["body"]["text"]);
    assert_eq!(r["meta"]["httpVersion"], "HTTP/1.1");
    let r = h.send(&url, json!({ "type": "ntlm", "username": "CORP\\ada", "password": "s3cret" })).await;
    assert_eq!(r["meta"]["status"], 200, "DOMAIN\\user");
    let r = h.send(&url, json!({ "type": "ntlm", "username": "ada", "password": "bad", "domain": "CORP" })).await;
    assert_eq!(r["meta"]["status"], 401);
}

#[tokio::test]
async fn signing_auth_reaches_the_server_signed() {
    let server = TestServer::start().await;
    let h = Harness::new().await;
    let echo = server.url("/echo?a=1");

    let aws = json!({ "type": "awsSigV4", "accessKey": "AKIDEXAMPLE", "secretKey": "secret", "region": "eu-west-1", "service": "execute-api" });
    let headers = echoed(&h.send(&echo, aws.clone()).await);
    assert!(headers["authorization"].starts_with("AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/"), "{headers:?}");
    assert!(headers["authorization"].contains("/eu-west-1/execute-api/aws4_request"));
    assert!(headers.contains_key("x-amz-date"));
    let mut presigned = aws;
    presigned["location"] = json!("query");
    let r = h.send(&echo, presigned).await;
    let query = serde_json::from_str::<Value>(r["body"]["text"].as_str().unwrap()).unwrap()["query"].to_string();
    assert!(query.contains("X-Amz-Signature=") && query.contains("a=1"), "{query}");

    let oauth1 =
        json!({ "type": "oauth1", "consumerKey": "ck", "consumerSecret": "cs", "token": "t", "tokenSecret": "ts" });
    let first = echoed(&h.send(&echo, oauth1.clone()).await)["authorization"].clone();
    let second = echoed(&h.send(&echo, oauth1).await)["authorization"].clone();
    assert!(first.starts_with("OAuth ") && first.contains("oauth_signature=\""), "{first}");
    assert_ne!(first, second, "a fresh nonce for every send");

    let jwt =
        json!({ "type": "jwt", "algorithm": "HS256", "secret": "k", "payload": "{\"sub\": \"{{user}}\", \"n\": 1}" });
    let value = echoed(&h.send(&echo, jwt).await)["authorization"].clone();
    let token = value.strip_prefix("Bearer ").expect("Bearer prefix");
    let claims = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(token.split('.').nth(1).unwrap()).unwrap();
    let claims: Value = serde_json::from_slice(&claims).unwrap();
    assert_eq!(claims["n"], 1);
    let jwt_query = json!({ "type": "jwt", "algorithm": "RS256", "secret": RSA_KEY, "location": "query", "queryParam": "access_token" });
    let r = h.send(&echo, jwt_query).await;
    let args = &serde_json::from_str::<Value>(r["body"]["text"].as_str().unwrap()).unwrap()["args"];
    assert_eq!(args["access_token"].as_str().unwrap().split('.').count(), 3);

    let hawk = json!({ "type": "hawk", "id": "dh37fgj492je", "key": "werxhqb98rpaxn39848xrunpaw3489ruxnpa98w4rxn", "includePayloadHash": true });
    let value = echoed(&h.send(&echo, hawk).await)["authorization"].clone();
    assert!(
        value.starts_with("Hawk id=\"dh37fgj492je\"") && value.contains("mac=\"") && value.contains("hash=\""),
        "{value}"
    );

    let edgegrid =
        json!({ "type": "akamaiEdgeGrid", "clientToken": "ct", "clientSecret": "c2VjcmV0", "accessToken": "at" });
    let value = echoed(&h.send(&echo, edgegrid).await)["authorization"].clone();
    assert!(value.starts_with("EG1-HMAC-SHA256 client_token=ct;access_token=at;timestamp="), "{value}");
    assert!(value.contains(";signature="));

    let asap = json!({ "type": "asap", "issuer": "svc", "audience": "api", "keyId": "svc/1", "privateKey": RSA_KEY, "expiresIn": 60 });
    let value = echoed(&h.send(&echo, asap).await)["authorization"].clone();
    let token = value.strip_prefix("Bearer ").unwrap();
    let header = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(token.split('.').next().unwrap()).unwrap();
    assert_eq!(serde_json::from_slice::<Value>(&header).unwrap()["kid"], "svc/1");
}

#[tokio::test]
async fn signing_errors_say_what_to_fix() {
    let server = TestServer::start().await;
    let h = Harness::new().await;
    let e = h
        .send_err(
            &server.url("/echo"),
            json!({ "type": "jwt", "algorithm": "RS256", "secret": "not a key", "payload": "{}" }),
        )
        .await;
    assert_eq!(e.code, "auth");
    assert!(e.message.contains("PEM") || e.message.contains("key"), "{}", e.message);
    let e = h
        .send_err(&server.url("/echo"), json!({ "type": "jwt", "algorithm": "HS256", "secret": "k", "payload": "[1]" }))
        .await;
    assert!(e.message.contains("JSON object"), "{}", e.message);
}

#[tokio::test]
async fn load_tests_refuse_challenge_auth() {
    let server = TestServer::start().await;
    let h = Harness::new().await;
    let request = json!({ "name": "Digest", "seq": 0, "method": "GET", "url": server.url("/digest-auth/auth/a/b"),
        "auth": { "type": "digest", "username": "a", "password": "b" } });
    let path = h.api.call("request.create", json!({ "parent": "", "request": request })).await.unwrap();
    let test = json!({ "name": "L", "targets": [{ "request": path }], "stages": [{ "durationSecs": 1, "target": 1 }] });
    let e = h.api.call("load.start", json!({ "id": "x", "test": test })).await.unwrap_err();
    assert!(e.message.contains("can't be load tested"), "{}", e.message);
}

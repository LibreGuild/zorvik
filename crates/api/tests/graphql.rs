//! GraphQL requests through the RPC API against the test server's `/graphql`
//! endpoint: schema introspection (`graphql.schema`), sending, cURL export.

use std::sync::Arc;

use serde_json::{Value, json};
use zorvik_api::{Api, EventSink, StreamEvent};
use zorvik_testkit::TestServer;
use zorvik_testkit::graphql::GRAPHQL_TOKEN;

struct Quiet;

impl EventSink for Quiet {
    fn emit(&self, _: StreamEvent) {}
}

struct Harness {
    api: Api,
    _data: tempfile::TempDir,
    _ws_dir: tempfile::TempDir,
}

impl Harness {
    async fn new() -> Self {
        let data = tempfile::tempdir().unwrap();
        let ws_dir = tempfile::tempdir().unwrap();
        let api = Api::new(data.path().to_path_buf(), Arc::new(Quiet));
        let h = Self { api, _data: data, _ws_dir: ws_dir };
        h.ok("workspace.create", json!({ "path": h._ws_dir.path(), "name": "Test" })).await;
        h
    }

    async fn ok(&self, method: &str, params: Value) -> Value {
        match self.api.call(method, params).await {
            Ok(v) => v,
            Err(e) => panic!("{method} failed: {} ({})", e.message, e.code),
        }
    }

    async fn err(&self, method: &str, params: Value) -> zorvik_api::ApiError {
        self.api.call(method, params).await.expect_err("expected an error")
    }

    async fn schema(&self, request: &Value, refresh: bool) -> Value {
        self.ok("graphql.schema", json!({ "request": request, "refresh": refresh })).await
    }
}

fn graphql_request(url: &str, query: &str) -> Value {
    json!({ "name": "g", "seq": 0, "method": "POST", "url": url,
            "body": { "type": "graphql", "graphql": { "query": query } } })
}

fn type_names(schema: &Value) -> Vec<&str> {
    schema["data"]["__schema"]["types"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect()
}

#[tokio::test]
async fn schema_is_introspected_with_the_request_and_cached() {
    let server = TestServer::start().await;
    let h = Harness::new().await;
    let env = json!({ "environment": { "name": "Dev", "variables": [{ "key": "base", "value": server.url("") }] } });
    let env_id = h.ok("env.create", env).await;
    h.ok("env.setActive", json!({ "id": env_id })).await;

    // The request's own method and body don't matter: introspection is always a POST.
    let mut req = graphql_request("{{base}}/graphql", "{ hello }");
    req["method"] = json!("GET");
    let schema = h.schema(&req, false).await;
    assert_eq!(schema["url"], server.url("/graphql"));
    assert_eq!((&schema["cached"], &schema["legacy"]), (&json!(false), &json!(false)));
    let data = &schema["data"]["__schema"];
    assert_eq!(data["queryType"]["name"], "Query");
    assert_eq!(data["mutationType"]["name"], "Mutation");
    assert_eq!(type_names(&schema), ["Query", "Mutation", "User", "Post", "Role", "ID", "String", "Int", "Boolean"]);
    // Descriptions, deprecated fields, arguments with defaults and nested type refs come along.
    let query = &data["types"][0];
    assert_eq!(query["description"], "The root of all queries.");
    assert!(query.get("specifiedByURL").is_some());
    let greeting = &query["fields"][3];
    assert_eq!((&greeting["name"], &greeting["isDeprecated"]), (&json!("greeting"), &json!(true)));
    assert_eq!(query["fields"][0]["args"][0]["defaultValue"], "\"world\"");
    let users = &query["fields"][2]["type"];
    assert_eq!(users["kind"], "NON_NULL");
    assert_eq!(users["ofType"]["ofType"]["ofType"]["name"], "User");
    assert_eq!(data["directives"][0]["isRepeatable"], false);

    // Cached until refreshed.
    let again = h.schema(&req, false).await;
    assert_eq!((&again["cached"], &again["fetchedAt"]), (&json!(true), &schema["fetchedAt"]));
    assert_eq!(again["data"], schema["data"]);
    assert_eq!(h.schema(&req, true).await["cached"], false);
    // Other headers are another cache entry.
    req["headers"] = json!([{ "key": "X-Tenant", "value": "b" }]);
    assert_eq!(h.schema(&req, false).await["cached"], false);
    // So are other TLS settings: a schema read without certificate checks isn't reused with them.
    req["settings"] = json!({ "verifyTls": false });
    assert_eq!(h.schema(&req, false).await["cached"], false);
    assert_eq!(h.schema(&req, false).await["cached"], true);
    // Introspection leaves no history.
    assert_eq!(h.ok("history.list", json!({})).await, json!([]));

    // The request itself runs its own operation with variables and operation name.
    let mut req = graphql_request("{{base}}/graphql", "query A { hello } query B($id: ID!) { user(id: $id) { name } }");
    req["body"]["graphql"]["variables"] = json!("{\"id\": \"1\"}");
    req["body"]["graphql"]["operationName"] = json!("B");
    let sent = h.ok("http.send", json!({ "requestId": "g1", "request": req })).await;
    assert_eq!(sent["meta"]["status"], 200);
    let body: Value = serde_json::from_str(sent["body"]["text"].as_str().unwrap()).unwrap();
    assert_eq!(body, json!({ "data": { "user": { "name": "Ada Lovelace" } } }));

    // cURL export sends the same JSON.
    let curl = h.ok("export.curl", json!({ "request": req, "flavor": "bash", "resolveVariables": true })).await;
    let curl = curl.as_str().unwrap();
    assert!(curl.contains("-H 'Content-Type: application/json'"), "{curl}");
    assert!(curl.contains(r#""variables":{"id": "1"},"operationName":"B"}"#), "{curl}");
    // Without resolving, `{{variables}}` stay as typed, also where they aren't JSON strings.
    req["body"]["graphql"]["variables"] = json!("{\"id\": {{userId}}}");
    let curl = h.ok("export.curl", json!({ "request": req, "flavor": "bash", "resolveVariables": false })).await;
    assert!(curl.as_str().unwrap().contains(r#""variables":{"id": {{userId}}},"operationName":"B"}"#), "{curl}");
}

#[tokio::test]
async fn older_servers_get_the_older_query() {
    let server = TestServer::start().await;
    let h = Harness::new().await;
    let schema = h.schema(&graphql_request(&server.url("/graphql?legacy=1"), ""), false).await;
    assert_eq!(schema["legacy"], true);
    assert_eq!(type_names(&schema).len(), 9);
    assert!(schema["data"]["__schema"]["types"][0].get("specifiedByURL").is_none());
}

#[tokio::test]
async fn credentials_come_from_the_request_auth() {
    let server = TestServer::start().await;
    let h = Harness::new().await;
    let mut req = graphql_request(&server.url("/graphql-auth"), "");
    let err = h.err("graphql.schema", json!({ "request": req })).await;
    assert_eq!(err.code, "graphql");
    assert!(err.message.contains("401") && err.message.contains("Not authenticated"), "{}", err.message);

    req["auth"] = json!({ "type": "bearer", "token": GRAPHQL_TOKEN, "prefix": "Bearer" });
    assert_eq!(h.schema(&req, false).await["cached"], false);
    // A cached schema is only reused with the same credentials.
    req["auth"]["token"] = json!("wrong");
    assert!(h.err("graphql.schema", json!({ "request": req })).await.message.contains("Not authenticated"));

    // OAuth 2.0: the token is fetched through the usual path.
    req["auth"] = json!({ "type": "oauth2", "grantType": "clientCredentials", "tokenUrl": server.url("/oauth/token"),
                          "clientId": "test-client", "clientSecret": "test-secret" });
    let schema = h.schema(&req, false).await;
    assert_eq!(schema["data"]["__schema"]["queryType"]["name"], "Query");
    let status = h.ok("oauth2.status", json!({ "auth": req["auth"] })).await;
    assert_eq!(status["hasToken"], true, "{status}");
}

#[tokio::test]
async fn clear_errors_for_other_endpoints() {
    let server = TestServer::start().await;
    let h = Harness::new().await;
    let message = |url: String| {
        let h = &h;
        async move { h.err("graphql.schema", json!({ "request": graphql_request(&url, "") })).await.message }
    };
    assert!(message(server.url("/echo")).await.contains("no schema"));
    assert!(message(server.url("/status/200")).await.contains("not JSON"));
    assert!(message(server.url("/status/503")).await.contains("HTTP 503"));
    assert!(message(server.url("/json")).await.contains("HTTP 405"), "GET-only endpoint");
    let err = h.err("graphql.schema", json!({ "request": graphql_request("{{nowhere}}/graphql", "") })).await;
    assert_eq!(err.code, "undefinedVariable");
}

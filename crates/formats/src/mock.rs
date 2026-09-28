//! Mock routes built from what a workspace already has: saved requests (a
//! folder or the whole collection) or an OpenAPI/Swagger document.

use crate::import::ImportError;
use crate::model::{Example, KeyValue, Request, RequestKind};
use crate::openapi::mock_responses;
use crate::server::MockRoute;

/// One route per HTTP request (in the given order). A request with saved examples
/// answers with them: an example saved with query parameters only matches requests
/// that carry them, and the first example without any is the fallback. A request
/// without examples answers 200 with an empty JSON object. Requests of other kinds
/// and repeated method + path pairs are left out. `requests` are `(workspace path,
/// request)` pairs.
pub fn routes_from_requests(requests: &[(String, Request)]) -> Vec<MockRoute> {
    let mut routes: Vec<MockRoute> = Vec::new();
    for (file, request) in requests {
        if request.kind != RequestKind::Http {
            continue;
        }
        let method = match request.method.trim().to_ascii_uppercase() {
            m if m.is_empty() => "GET".to_string(),
            m => m,
        };
        let path = route_path(&request.url);
        if routes.iter().any(|r| r.method == method && r.path == path) {
            continue;
        }
        let name = match request.name.trim() {
            "" => file.rsplit('/').next().unwrap_or(file).trim_end_matches(".yaml").to_string(),
            name => name.to_string(),
        };
        if !request.examples.is_empty() {
            routes.extend(example_routes(&name, &method, &path, &request.examples));
            continue;
        }
        let bodyless = matches!(method.as_str(), "HEAD" | "OPTIONS");
        routes.push(MockRoute {
            name,
            headers: if bodyless { Vec::new() } else { vec![KeyValue::new("Content-Type", "application/json")] },
            body: if bodyless { String::new() } else { "{}".to_string() },
            method,
            path,
            ..MockRoute::default()
        });
    }
    routes
}

/// Routes answering with `examples`: the ones saved with query parameters first (they
/// only match requests carrying those), then one fallback (the first example without
/// a query, else the first example).
fn example_routes(name: &str, method: &str, path: &str, examples: &[Example]) -> Vec<MockRoute> {
    let route = |e: &Example, match_query: Vec<KeyValue>| MockRoute {
        name: match e.name.trim() {
            "" => name.to_string(),
            n => format!("{name} · {n}"),
        },
        method: method.to_string(),
        path: path.to_string(),
        status: e.status,
        headers: e.headers.iter().filter(|h| h.enabled && !skip_header(&h.key)).cloned().collect(),
        body: e.body.clone(),
        match_query,
        ..MockRoute::default()
    };
    let mut routes: Vec<MockRoute> = Vec::new();
    let mut fallback = None;
    for e in examples {
        let query = query_pairs(&e.url);
        if query.is_empty() {
            fallback.get_or_insert(e);
        } else if !routes.iter().any(|r| r.match_query == query) {
            routes.push(route(e, query));
        }
    }
    routes.push(route(fallback.unwrap_or(&examples[0]), Vec::new()));
    routes
}

/// Headers a mock sets itself (or that would be wrong for its body).
fn skip_header(name: &str) -> bool {
    ["content-length", "transfer-encoding", "content-encoding", "connection", "date", "keep-alive"]
        .iter()
        .any(|h| name.eq_ignore_ascii_case(h))
}

/// `?a=1&b=` → `[a=1, b=]` (outside `{{…}}`; a `{{var}}` value matches any value).
fn query_pairs(url: &str) -> Vec<KeyValue> {
    let url = url.split('#').next().unwrap_or_default();
    let base = cut_query(url);
    let Some(query) = url.get(base.len() + 1..) else { return Vec::new() };
    query
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            let v = if v.contains("{{") { "" } else { v };
            KeyValue::new(k, v)
        })
        .collect()
}

/// One route per operation of an OpenAPI 3.x / Swagger 2.0 document (JSON or
/// YAML), answering with its first 2xx response: the document's example, or
/// one generated from the schema. Paths are as in the document (without the
/// server's base path). Returns the routes and warnings.
pub fn routes_from_openapi(text: &str) -> Result<(Vec<MockRoute>, Vec<String>), ImportError> {
    let (responses, warnings) = mock_responses(text)?;
    let routes = responses
        .into_iter()
        .map(|r| MockRoute {
            name: r.name,
            method: r.method,
            path: r.path,
            status: r.status,
            headers: r.content_type.map(|ct| vec![KeyValue::new("Content-Type", ct)]).unwrap_or_default(),
            body: r.body,
            ..MockRoute::default()
        })
        .collect();
    Ok((routes, warnings))
}

/// The route path for a request URL: scheme, host and a leading `{{baseUrl}}`-style
/// variable are dropped, and so are the query and fragment; `{{id}}` / `{id}`
/// segments become `:id`.
///
/// `https://api.test/users/{{id}}?x=1` → `/users/:id`, `{{baseUrl}}/pets/:petId` → `/pets/:petId`.
pub fn route_path(url: &str) -> String {
    let url = url.trim();
    let url = url.split('#').next().unwrap_or_default();
    let url = cut_query(url);
    let path = match url.find("://") {
        // Only a scheme before the first `/` (not `{{a}}/x://y`).
        Some(i) if !url[..i].contains('/') => url[i + 3..].find('/').map_or("", |j| &url[i + 3 + j..]),
        _ if url.starts_with('/') => url,
        // `{{baseUrl}}/users`, `localhost:3000/x`: the path starts at the first `/` outside `{{…}}`.
        _ => first_slash_outside_braces(url).map_or("", |i| &url[i..]),
    };
    let segments: Vec<String> = path.split('/').filter(|s| !s.is_empty()).map(segment).collect();
    format!("/{}", segments.join("/"))
}

/// `?` starts the query unless it is inside `{{…}}`.
fn cut_query(url: &str) -> &str {
    let mut depth = 0usize;
    let bytes = url.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i..].starts_with(b"{{") {
            depth += 1;
            i += 2;
        } else if bytes[i..].starts_with(b"}}") && depth > 0 {
            depth -= 1;
            i += 2;
        } else if bytes[i] == b'?' && depth == 0 {
            return &url[..i];
        } else {
            i += 1;
        }
    }
    url
}

fn first_slash_outside_braces(url: &str) -> Option<usize> {
    let mut depth = 0usize;
    let bytes = url.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i..].starts_with(b"{{") {
            depth += 1;
            i += 2;
        } else if bytes[i..].starts_with(b"}}") && depth > 0 {
            depth -= 1;
            i += 2;
        } else if bytes[i] == b'/' && depth == 0 {
            return Some(i);
        } else {
            i += 1;
        }
    }
    None
}

/// A path segment with variables turned into a `:param`.
fn segment(s: &str) -> String {
    let variable = s
        .find("{{")
        .and_then(|start| Some((start, s[start + 2..].find("}}")? + start + 2)))
        .map(|(start, end)| s[start + 2..end].trim())
        .or_else(|| s.strip_prefix('{').and_then(|v| v.strip_suffix('}')).map(str::trim));
    match variable {
        Some(name) => {
            let name: String = name
                .trim_start_matches('$')
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
                .collect();
            if name.is_empty() { ":param".to_string() } else { format!(":{name}") }
        }
        None => s.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::MockFault;

    fn request(name: &str, method: &str, url: &str) -> (String, Request) {
        let mut r = Request::new(name, RequestKind::Http);
        r.method = method.into();
        r.url = url.into();
        (format!("Folder/{name}.yaml"), r)
    }

    #[test]
    fn examples_become_routes() {
        let (file, mut r) = request("User", "GET", "{{baseUrl}}/users/{{id}}");
        let example = |name: &str, status: u16, url: &str, body: &str| Example {
            name: name.into(),
            status,
            headers: vec![KeyValue::new("Content-Type", "application/json"), KeyValue::new("Content-Length", "9")],
            body: body.into(),
            url: url.into(),
        };
        r.examples = vec![
            example("Found", 200, "", r#"{"id":1}"#),
            example("Missing", 404, "{{baseUrl}}/users/9?include=all", r#"{"error":"no"}"#),
            example("Other", 200, "", "{}"),
        ];
        let routes = routes_from_requests(&[(file, r)]);
        assert_eq!(routes.len(), 2, "{routes:?}");
        assert_eq!((routes[0].name.as_str(), routes[0].status), ("User · Missing", 404));
        assert_eq!(routes[0].match_query, vec![KeyValue::new("include", "all")]);
        assert_eq!(
            (routes[1].name.as_str(), routes[1].status, routes[1].body.as_str()),
            ("User · Found", 200, r#"{"id":1}"#)
        );
        assert!(routes[1].match_query.is_empty());
        assert_eq!(routes[1].headers, vec![KeyValue::new("Content-Type", "application/json")], "no Content-Length");
        assert_eq!(routes[1].path, "/users/:id");
    }

    #[test]
    fn paths_from_urls() {
        let cases = [
            ("https://api.test/users/{{id}}?x=1", "/users/:id"),
            ("{{baseUrl}}/pets/:petId", "/pets/:petId"),
            ("{{ baseUrl }}/a/{{$uuid}}/b#frag", "/a/:uuid/b"),
            ("localhost:3000/api/v1/", "/api/v1"),
            ("http://localhost:3000", "/"),
            ("{{host}}:{{port}}/x", "/x"),
            ("/relative//path", "/relative/path"),
            ("{{baseUrl}}", "/"),
            ("{{base}}/search?q={{q}}", "/search"),
            ("{{base}}/files/{name}/user-{{id}}", "/files/:name/:id"),
            ("", "/"),
        ];
        for (url, path) in cases {
            assert_eq!(route_path(url), path, "{url}");
        }
    }

    #[test]
    fn routes_from_saved_requests() {
        let mut ws = request("Socket", "GET", "ws://x/y");
        ws.1.kind = RequestKind::Websocket;
        let routes = routes_from_requests(&[
            request("List users", "get", "{{baseUrl}}/users"),
            request("Get user", "GET", "{{baseUrl}}/users/{{id}}"),
            request("Same again", "GET", "https://other.test/users/{{userId}}"),
            request("", "HEAD", "{{baseUrl}}/health"),
            ws,
        ]);
        let summary: Vec<(&str, &str, &str)> =
            routes.iter().map(|r| (r.method.as_str(), r.path.as_str(), r.name.as_str())).collect();
        assert_eq!(
            summary,
            [
                ("GET", "/users", "List users"),
                ("GET", "/users/:id", "Get user"),
                ("GET", "/users/:userId", "Same again"),
                ("HEAD", "/health", ""),
            ]
        );
        assert_eq!((routes[0].status, routes[0].body.as_str()), (200, "{}"));
        assert_eq!(routes[0].headers[0].value, "application/json");
        assert!(routes[3].body.is_empty() && routes[3].headers.is_empty());
        assert_eq!(routes[3].name, "", "an unnamed request keeps an empty name");
        assert_eq!(routes[0].fault, MockFault::None);
    }

    const PETS: &str = r##"{
      "openapi": "3.0.3",
      "info": {"title": "Pets", "version": "1"},
      "servers": [{"url": "https://pets.test/v1"}],
      "paths": {
        "/pets": {
          "get": {
            "summary": "List pets",
            "responses": {
              "default": {"description": "error"},
              "200": {"description": "ok", "content": {"application/json": {
                "schema": {"type": "array", "items": {"$ref": "#/components/schemas/Pet"}}}}}
            }
          },
          "post": {
            "operationId": "createPet",
            "responses": {"201": {"description": "created", "content": {"application/json": {
              "example": {"id": 7, "name": "Rex"}}}}}
          }
        },
        "/pets/{petId}": {
          "delete": {"responses": {"204": {"description": "gone"}}},
          "get": {"responses": {"2XX": {"$ref": "#/components/responses/One"}}}
        },
        "/text": {"get": {"responses": {"200": {"description": "t", "content": {"text/plain": {"example": "pong"}}}}}}
      },
      "components": {
        "responses": {"One": {"description": "one", "content": {"application/json": {
          "schema": {"$ref": "#/components/schemas/Pet"}}}}},
        "schemas": {"Pet": {"type": "object", "properties": {
          "id": {"type": "integer", "readOnly": true},
          "name": {"type": "string", "example": "Tom"},
          "password": {"type": "string", "writeOnly": true}
        }}}
      }
    }"##;

    #[test]
    fn routes_from_openapi3() {
        let (routes, warnings) = routes_from_openapi(PETS).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        let summary: Vec<(&str, &str, u16, &str)> =
            routes.iter().map(|r| (r.method.as_str(), r.path.as_str(), r.status, r.name.as_str())).collect();
        assert_eq!(
            summary,
            [
                ("GET", "/pets", 200, "List pets"),
                ("POST", "/pets", 201, "createPet"),
                ("DELETE", "/pets/:petId", 204, "DELETE /pets/{petId}"),
                ("GET", "/pets/:petId", 200, "GET /pets/{petId}"),
                ("GET", "/text", 200, "GET /text"),
            ]
        );
        // Responses include read-only properties and leave write-only ones out.
        let list: serde_json::Value = serde_json::from_str(&routes[0].body).unwrap();
        assert_eq!(list, serde_json::json!([{"id": 1, "name": "Tom"}]));
        assert_eq!(serde_json::from_str::<serde_json::Value>(&routes[1].body).unwrap()["name"], "Rex");
        assert!(routes[2].body.is_empty() && routes[2].headers.is_empty());
        assert!(routes[3].body.contains("\"Tom\""), "{}", routes[3].body);
        assert_eq!((routes[4].body.as_str(), routes[4].headers[0].value.as_str()), ("pong", "text/plain"));
    }

    #[test]
    fn routes_from_swagger2_yaml() {
        let doc = r#"
swagger: "2.0"
info: {title: Old, version: "1"}
produces: [application/json]
paths:
  /users/{id}:
    get:
      summary: Get user
      responses:
        "200":
          description: ok
          schema: {type: object, properties: {id: {type: string, format: uuid}}}
  /ping:
    get:
      responses:
        "200":
          description: ok
          examples: {text/plain: pong}
"#;
        let (routes, warnings) = routes_from_openapi(doc).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(routes[0].path, "/users/:id");
        assert!(routes[0].body.contains("3fa85f64"), "{}", routes[0].body);
        assert_eq!(routes[0].headers[0].value, "application/json");
        assert_eq!((routes[1].body.as_str(), routes[1].headers[0].value.as_str()), ("pong", "text/plain"));
    }

    #[test]
    fn invalid_and_empty_documents() {
        assert!(routes_from_openapi("hello: world").is_err());
        assert!(routes_from_openapi("").is_err());
        let (routes, warnings) = routes_from_openapi(r#"{"openapi":"3.1.0","paths":{}}"#).unwrap();
        assert!(routes.is_empty());
        assert_eq!(warnings, ["The document contains no operations"]);
        // Operations without responses answer 200 with no body.
        let (routes, _) =
            routes_from_openapi(r#"{"openapi":"3.0.0","paths":{"/x":{"put":{},"get":{"responses":{"404":{}}}}}}"#)
                .unwrap();
        assert_eq!(
            routes.iter().map(|r| (r.method.as_str(), r.status)).collect::<Vec<_>>(),
            [("PUT", 200), ("GET", 200)]
        );
    }

    #[test]
    fn recursive_and_huge_schemas_stay_bounded() {
        let doc = r##"{"openapi":"3.0.0","paths":{"/n":{"get":{"responses":{"200":{"content":{"application/json":{
            "schema":{"$ref":"#/components/schemas/Node"}}}}}}}},
            "components":{"schemas":{"Node":{"type":"object","properties":{
              "child":{"$ref":"#/components/schemas/Node"},"items":{"type":"array","items":{"$ref":"#/components/schemas/Node"}}}}}}}"##;
        let (routes, _) = routes_from_openapi(doc).unwrap();
        assert_eq!(routes.len(), 1);
        assert!(routes[0].body.len() < 10_000, "{}", routes[0].body.len());
    }
}

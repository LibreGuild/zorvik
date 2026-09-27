//! GraphQL schema introspection (`graphql.schema`). The request is resolved like
//! `http.send` (variables, folder/workspace headers, auth incl. OAuth2, settings,
//! proxy, TLS, cookie jar) and sent as `POST` with the standard introspection
//! query instead of its own body; no history entry. Results stay in memory per
//! resolved URL and credentials until `refresh`. A server that rejects the query
//! (older ones know no `specifiedByURL` / `isRepeatable`) is asked once more
//! with the older form.

use std::collections::VecDeque;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use ts_rs::TS;
use zorvik_engine::{HttpResponse, TlsOptions};
use zorvik_workspace::formats::{Body, BodyType, GraphqlBody, Request};
use zorvik_workspace::oauth2;
use zorvik_workspace::resolve::Resolved;

use crate::{Api, ApiError, ApiResult, lock, now_ms, ok, params};

/// Largest introspection response accepted.
const MAX_SCHEMA_BYTES: usize = 20 * 1024 * 1024;
/// Schemas kept in memory, by count and by total response size.
const MAX_CACHED: usize = 16;
const MAX_CACHED_BYTES: usize = 64 * 1024 * 1024;

/// An introspected GraphQL schema.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GraphqlSchema {
    /// The response's `data` (`{"__schema": …}`), as graphql-js `buildClientSchema` takes it.
    #[ts(type = "Record<string, unknown>")]
    pub data: Value,
    /// The URL that was introspected (variables resolved).
    pub url: String,
    /// When it was fetched (ms since the Unix epoch).
    #[ts(type = "number")]
    pub fetched_at: i64,
    /// Served from the in-memory cache.
    pub cached: bool,
    /// The server rejected the current introspection query; the older form
    /// (no `specifiedByURL`, `isRepeatable`) was used.
    pub legacy: bool,
}

/// Introspection results by request fingerprint, oldest first.
#[derive(Default)]
pub(crate) struct SchemaCache {
    items: Mutex<VecDeque<(u64, usize, GraphqlSchema)>>,
}

impl SchemaCache {
    fn get(&self, key: u64) -> Option<GraphqlSchema> {
        lock(&self.items).iter().find(|(k, ..)| *k == key).map(|(.., schema)| schema.clone())
    }

    fn put(&self, key: u64, size: usize, schema: GraphqlSchema) {
        let mut items = lock(&self.items);
        items.retain(|(k, ..)| *k != key);
        items.push_back((key, size, schema));
        while items.len() > MAX_CACHED
            || (items.len() > 1 && items.iter().map(|(_, size, _)| size).sum::<usize>() > MAX_CACHED_BYTES)
        {
            items.pop_front();
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SchemaParams {
    pub request: Request,
    /// Saved location of the request (for folder auth/header inheritance).
    pub path: Option<String>,
    /// Ask the server again instead of using the cached schema.
    #[serde(default)]
    pub refresh: bool,
}

impl Api {
    /// `graphql.*` methods.
    pub(crate) async fn call_graphql(&self, method: &str, p: Value) -> ApiResult<Value> {
        match method {
            "graphql.schema" => ok(self.graphql_schema(params(p)?).await?),
            other => Err(ApiError::new("notFound", format!("Unknown method '{other}'"))),
        }
    }

    pub(crate) async fn graphql_schema(&self, p: SchemaParams) -> ApiResult<GraphqlSchema> {
        let ws = self.ws()?;
        let settings = self.settings();
        let mut request = p.request;
        request.method = "POST".into();
        request.body = Body {
            body_type: BodyType::Graphql,
            graphql: GraphqlBody { query: introspection_query(false), ..Default::default() },
            ..Default::default()
        };
        let mut resolved = self.prepare(&ws, &request, p.path.as_deref())?;
        let mut opts = crate::request_options(&settings, &request.settings)?;
        opts.max_body_bytes = MAX_SCHEMA_BYTES;
        let key = fingerprint(&resolved, &opts.tls, &ws.local_key());
        if !p.refresh
            && let Some(cached) = self.inner.graphql.get(key)
        {
            return Ok(GraphqlSchema { cached: true, ..cached });
        }

        let jar = settings.cookie_jar.then(|| self.jar(&ws));
        self.authorize(&ws, &mut resolved, &opts).await?;
        let mut legacy = false;
        let (data, size) = loop {
            let response = self.inner.client.send(resolved.request.clone(), &opts, jar.as_deref()).await?;
            if let Some(jar) = &jar {
                self.save_jar(&ws, jar);
            }
            match read_schema(&response) {
                Ok(data) => break (data, response.body.len()),
                // Once more with the older query, unless the server wants credentials.
                Err(Failure::Graphql(_)) if !legacy && !matches!(response.meta.status, 401 | 403 | 407) => {
                    legacy = true;
                    resolved.request.body =
                        json!({ "query": introspection_query(true) }).to_string().into_bytes().into();
                }
                Err(Failure::Graphql(message) | Failure::Other(message)) => {
                    return Err(ApiError::new("graphql", message));
                }
            }
        };
        let schema = GraphqlSchema { data, url: resolved.request.url, fetched_at: now_ms(), cached: false, legacy };
        self.inner.graphql.put(key, size, schema.clone());
        Ok(schema)
    }
}

/// Same workspace, URL, headers, OAuth2 client and TLS settings (certificate
/// checks, client certificate) → same schema.
fn fingerprint(resolved: &Resolved, tls: &TlsOptions, workspace: &str) -> u64 {
    let mut h = DefaultHasher::new();
    workspace.hash(&mut h);
    tls.hash(&mut h);
    resolved.request.url.hash(&mut h);
    let mut headers: Vec<(String, &str)> =
        resolved.request.headers.iter().map(|x| (x.name.to_ascii_lowercase(), x.value.as_str())).collect();
    headers.sort();
    headers.hash(&mut h);
    resolved.oauth2.as_ref().map(|config| oauth2::cache_key(workspace, config)).hash(&mut h);
    h.finish()
}

enum Failure {
    /// The server answered with GraphQL errors and no schema.
    Graphql(String),
    Other(String),
}

/// `data` of an introspection response, or why there is none.
fn read_schema(response: &HttpResponse) -> Result<Value, Failure> {
    let status = response.meta.status;
    let success = (200..300).contains(&status);
    let http = format!("HTTP {status} {}", response.meta.status_text).trim_end().to_string();
    if response.body_truncated {
        return Err(Failure::Other(format!("The schema is larger than {} MB", MAX_SCHEMA_BYTES / (1024 * 1024))));
    }
    let mut body: Value = match serde_json::from_slice(&response.body) {
        Ok(body) => body,
        Err(_) if !success => return Err(Failure::Other(format!("The server answered {http}"))),
        Err(_) => return Err(Failure::Other("The response is not JSON. Is this a GraphQL endpoint?".into())),
    };
    if body.pointer("/data/__schema").is_some_and(Value::is_object) {
        return Ok(body["data"].take());
    }
    let messages: Vec<String> = body
        .get("errors")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|e| e.get("message").and_then(Value::as_str).map_or_else(|| e.to_string(), str::to_string))
        .collect();
    if !messages.is_empty() {
        let at = if success { String::new() } else { format!(" ({http})") };
        return Err(Failure::Graphql(format!("The schema request failed{at}: {}", messages.join("; "))));
    }
    if !success {
        return Err(Failure::Other(format!("The server answered {http}")));
    }
    Err(Failure::Other("The response has no schema (`data.__schema`). Is this a GraphQL endpoint?".into()))
}

/// The standard introspection query (as graphql-js and GraphiQL send it, with
/// descriptions and deprecated fields). `legacy` leaves out what servers from
/// before the October 2021 spec don't know.
fn introspection_query(legacy: bool) -> String {
    let (specified_by, repeatable) = if legacy { ("", "") } else { ("\n  specifiedByURL", "\n      isRepeatable") };
    format!(
        r#"query IntrospectionQuery {{
  __schema {{
    queryType {{ name }}
    mutationType {{ name }}
    subscriptionType {{ name }}
    types {{
      ...FullType
    }}
    directives {{
      name
      description{repeatable}
      locations
      args {{
        ...InputValue
      }}
    }}
  }}
}}

fragment FullType on __Type {{
  kind
  name
  description{specified_by}
  fields(includeDeprecated: true) {{
    name
    description
    args {{
      ...InputValue
    }}
    type {{
      ...TypeRef
    }}
    isDeprecated
    deprecationReason
  }}
  inputFields {{
    ...InputValue
  }}
  interfaces {{
    ...TypeRef
  }}
  enumValues(includeDeprecated: true) {{
    name
    description
    isDeprecated
    deprecationReason
  }}
  possibleTypes {{
    ...TypeRef
  }}
}}

fragment InputValue on __InputValue {{
  name
  description
  type {{
    ...TypeRef
  }}
  defaultValue
}}

fragment TypeRef on __Type {{
  kind
  name
  ofType {{
    kind
    name
    ofType {{
      kind
      name
      ofType {{
        kind
        name
        ofType {{
          kind
          name
          ofType {{
            kind
            name
            ofType {{
              kind
              name
              ofType {{
                kind
                name
              }}
            }}
          }}
        }}
      }}
    }}
  }}
}}
"#
    )
}

/// Built-in scalars (not printed).
const BUILTIN_SCALARS: &[&str] = &["String", "Int", "Float", "Boolean", "ID"];

/// An introspected schema as SDL (types, fields with arguments, inputs, enums), for
/// AI agents to write queries against. Descriptions become `#` comments.
pub(crate) fn schema_sdl(data: &Value) -> String {
    fn type_ref(t: &Value) -> String {
        match t["kind"].as_str() {
            Some("NON_NULL") => format!("{}!", type_ref(&t["ofType"])),
            Some("LIST") => format!("[{}]", type_ref(&t["ofType"])),
            _ => t["name"].as_str().unwrap_or("?").to_string(),
        }
    }
    fn comment(out: &mut String, item: &Value, indent: &str) {
        if let Some(d) = item["description"].as_str().map(str::trim).filter(|d| !d.is_empty()) {
            for line in d.lines().take(3) {
                out.push_str(&format!("{indent}# {}\n", line.trim()));
            }
        }
    }
    fn deprecated(item: &Value) -> String {
        if item["isDeprecated"].as_bool() == Some(true) {
            match item["deprecationReason"].as_str() {
                Some(r) if !r.is_empty() => format!(" @deprecated(reason: {})", Value::from(r)),
                _ => " @deprecated".into(),
            }
        } else {
            String::new()
        }
    }
    fn input_value(v: &Value) -> String {
        let default = v["defaultValue"].as_str().map(|d| format!(" = {d}")).unwrap_or_default();
        format!("{}: {}{default}", v["name"].as_str().unwrap_or("?"), type_ref(&v["type"]))
    }
    let schema = &data["__schema"];
    let mut out = String::new();
    let roots: Vec<(&str, &str)> =
        [("query", "queryType"), ("mutation", "mutationType"), ("subscription", "subscriptionType")]
            .into_iter()
            .filter_map(|(op, key)| schema[key]["name"].as_str().map(|n| (op, n)))
            .collect();
    if !roots.is_empty() {
        out.push_str("schema {\n");
        for (op, name) in roots {
            out.push_str(&format!("  {op}: {name}\n"));
        }
        out.push_str("}\n\n");
    }
    for t in schema["types"].as_array().into_iter().flatten() {
        let name = t["name"].as_str().unwrap_or_default();
        let kind = t["kind"].as_str().unwrap_or_default();
        if name.starts_with("__") || (kind == "SCALAR" && BUILTIN_SCALARS.contains(&name)) {
            continue;
        }
        comment(&mut out, t, "");
        match kind {
            "SCALAR" => out.push_str(&format!("scalar {name}\n\n")),
            "OBJECT" | "INTERFACE" => {
                let keyword = if kind == "OBJECT" { "type" } else { "interface" };
                let interfaces: Vec<&str> =
                    t["interfaces"].as_array().into_iter().flatten().filter_map(|i| i["name"].as_str()).collect();
                let implements = if interfaces.is_empty() {
                    String::new()
                } else {
                    format!(" implements {}", interfaces.join(" & "))
                };
                out.push_str(&format!("{keyword} {name}{implements} {{\n"));
                for f in t["fields"].as_array().into_iter().flatten() {
                    comment(&mut out, f, "  ");
                    let args: Vec<String> = f["args"].as_array().into_iter().flatten().map(input_value).collect();
                    let args = if args.is_empty() { String::new() } else { format!("({})", args.join(", ")) };
                    out.push_str(&format!(
                        "  {}{args}: {}{}\n",
                        f["name"].as_str().unwrap_or("?"),
                        type_ref(&f["type"]),
                        deprecated(f)
                    ));
                }
                out.push_str("}\n\n");
            }
            "INPUT_OBJECT" => {
                out.push_str(&format!("input {name} {{\n"));
                for f in t["inputFields"].as_array().into_iter().flatten() {
                    comment(&mut out, f, "  ");
                    out.push_str(&format!("  {}\n", input_value(f)));
                }
                out.push_str("}\n\n");
            }
            "ENUM" => {
                out.push_str(&format!("enum {name} {{\n"));
                for v in t["enumValues"].as_array().into_iter().flatten() {
                    out.push_str(&format!("  {}{}\n", v["name"].as_str().unwrap_or("?"), deprecated(v)));
                }
                out.push_str("}\n\n");
            }
            "UNION" => {
                let members: Vec<&str> =
                    t["possibleTypes"].as_array().into_iter().flatten().filter_map(|p| p["name"].as_str()).collect();
                out.push_str(&format!("union {name} = {}\n\n", members.join(" | ")));
            }
            _ => {}
        }
    }
    out.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_query_leaves_out_newer_fields() {
        let (modern, legacy) = (introspection_query(false), introspection_query(true));
        assert!(modern.contains("\n  description\n  specifiedByURL\n  fields(includeDeprecated: true)"));
        assert!(modern.contains("\n      description\n      isRepeatable\n      locations"));
        assert!(!legacy.contains("specifiedByURL") && !legacy.contains("isRepeatable"));
        assert_eq!(modern.matches("ofType").count(), 7);
    }
}

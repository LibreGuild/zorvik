//! The tools agents call (MCP `tools/list` and `tools/call`). Every call is
//! logged in the activity, checked against the user's settings (asking in the
//! app when needed) and runs through the same code as the UI.

use std::collections::{BTreeSet, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use zorvik_engine::HostGuard;
use zorvik_workspace::Workspace;
use zorvik_workspace::formats::{
    Auth, Environment, FolderMeta, KeyValue, LoadTest, NodeKind, Request, RequestKind, Scripts, TreeNode, Variable,
};
use zorvik_workspace::settings::{AgentChanges, AgentTraffic};

mod files;
mod servers;
mod state;

use super::redact::Redactor;
use super::{ActivityStatus, AgentEvent, AgentSessionHandle, AgentTarget, Ask, ConfirmKind, SessionState};
use crate::load::is_local;
use crate::runner::RunReport;
use crate::{Api, ApiError, MASK, SendParams, SendResult, lock};

/// Progress of a long call: (progress, total, message).
pub type ProgressFn = Arc<dyn Fn(f64, Option<f64>, &str) + Send + Sync>;

/// What a tool call returns to the agent.
#[derive(Debug, Clone)]
pub struct ToolOutput {
    pub text: String,
    pub structured: Option<Value>,
    pub is_error: bool,
}

impl ToolOutput {
    fn error(message: impl Into<String>) -> Self {
        Self { text: message.into(), structured: None, is_error: true }
    }

    /// The MCP `CallToolResult`.
    pub fn to_mcp(&self) -> Value {
        let mut out = json!({ "content": [{ "type": "text", "text": self.text }], "isError": self.is_error });
        if let Some(s) = &self.structured {
            out["structuredContent"] = s.clone();
        }
        out
    }
}

/// Longest wait of a call for a run to finish (then the agent polls).
const MAX_WAIT_SECS: u64 = 600;
/// Default: some agents give up on a tool call after 60 s.
const DEFAULT_WAIT_SECS: u64 = 45;
/// Response body characters returned by default, and at most.
const DEFAULT_BODY_CHARS: usize = 20_000;
const MAX_BODY_CHARS: usize = 80_000;
/// The longest an agent's SSE read waits, and event data characters returned per event.
const MAX_SSE_WAIT_MS: u64 = 120_000;
const MAX_EVENT_CHARS: usize = 8_000;
/// GraphQL schema text returned at most (big APIs have hundreds of KB).
const MAX_SCHEMA_CHARS: usize = 60_000;
/// Requests saved in one call.
const MAX_BATCH: usize = 200;
/// Requests read in one call.
const MAX_READ: usize = 50;
/// Request files read to list URLs.
const MAX_LISTED: usize = 5_000;
/// Failed results returned for a collection run.
const MAX_FAILURES: usize = 50;

// ---- definitions -----------------------------------------------------------------------------

struct Def {
    name: &'static str,
    title: &'static str,
    description: &'static str,
    schema: Value,
    read_only: bool,
    destructive: bool,
    /// Talks to other systems (sends requests).
    open_world: bool,
}

fn obj(properties: Value, required: &[&str]) -> Value {
    json!({ "type": "object", "properties": properties, "required": required })
}

fn string(description: &str) -> Value {
    json!({ "type": "string", "description": description })
}

fn request_schema() -> Value {
    let kv = json!({
        "type": "array",
        "items": obj(json!({ "key": { "type": "string" }, "value": { "type": "string" }, "enabled": { "type": "boolean" } }), &["key", "value"]),
    });
    obj(
        json!({
            "name": string("Request name, e.g. \"Create order\" (also its file name)."),
            "kind": { "type": "string", "enum": ["http", "grpc", "dns", "websocket", "sse", "tcp", "udp", "mqtt"], "description": "Default http. GraphQL is http with a graphql body." },
            "method": string("HTTP method (default GET). gRPC: package.Service/Method. DNS: the record type (A, AAAA, MX…)."),
            "url": string("Full URL with the query string, using {{variables}}: {{baseUrl}}/orders/{{orderId}}?expand=items. :name path segments take their value from pathParams ({{baseUrl}}/users/:id). gRPC: grpc://host:port (grpcs:// for TLS). DNS: the name."),
            "query": {
                "type": "array",
                "description": "Query parameters, instead of writing them into url: enabled ones become the URL's query string (replacing the one in url), disabled ones are kept switched off, and descriptions are kept for both.",
                "items": obj(json!({ "key": { "type": "string" }, "value": { "type": "string" }, "enabled": { "type": "boolean", "description": "Default true." }, "description": { "type": "string" } }), &["key"]),
            },
            "pathParams": {
                "type": "array",
                "description": "Values of :name segments in the URL path, e.g. [{key: \"id\", value: \"{{userId}}\"}] for {{baseUrl}}/users/:id.",
                "items": obj(json!({ "key": { "type": "string" }, "value": { "type": "string" }, "description": { "type": "string" } }), &["key"]),
            },
            "headers": { "description": "Headers as [{key, value}] (an object of name → value works too).", "type": "array", "items": obj(json!({ "key": { "type": "string" }, "value": { "type": "string" }, "enabled": { "type": "boolean" } }), &["key", "value"]) },
            "body": obj(json!({
                "type": { "type": "string", "enum": ["none", "json", "text", "xml", "formUrlencoded", "multipart", "binary", "graphql"] },
                "text": string("Body of json/text/xml requests (a gRPC request's JSON message)."),
                "contentType": string("Content-Type of text bodies."),
                "form": kv,
                "multipart": { "type": "array", "items": obj(json!({ "key": { "type": "string" }, "value": { "type": "string", "description": "Text, or a file path when file is true" }, "file": { "type": "boolean" } }), &["key", "value"]) },
                "file": string("Binary body: a file inside the workspace folder."),
                "graphql": obj(json!({ "query": { "type": "string" }, "variables": string("Variables as JSON text."), "operationName": { "type": "string" } }), &[]),
            }), &["type"]),
            "auth": obj(json!({
                "type": { "type": "string", "enum": ["inherit", "none", "basic", "bearer", "apiKey", "oauth2"], "description": "Default inherit: from the folders, then the collection." },
                "username": { "type": "string" }, "password": { "type": "string" },
                "token": string("bearer: the token, e.g. {{accessToken}}."),
                "key": string("apiKey: header or query parameter name."), "value": { "type": "string" },
                "location": { "type": "string", "enum": ["header", "query"] },
                "tokenUrl": string("oauth2"), "clientId": { "type": "string" }, "clientSecret": { "type": "string" }, "scope": { "type": "string" },
                "grantType": { "type": "string", "enum": ["clientCredentials", "password", "authorizationCode"] },
            }), &["type"]),
            "scripts": obj(json!({
                "preRequest": string("JavaScript run before sending (Postman pm API)."),
                "postResponse": string("JavaScript run after the response; tests: pm.test('ok', () => pm.response.to.have.status(200))."),
            }), &[]),
            "settings": obj(json!({ "timeoutMs": { "type": "integer" }, "followRedirects": { "type": "boolean" }, "verifyTls": { "type": "boolean" } }), &[]),
            "grpc": obj(json!({ "protoFiles": { "type": "array", "items": { "type": "string" }, "description": ".proto files (relative to the workspace folder). Empty: server reflection." } }), &[]),
            "docs": string("Markdown notes: what it does, where the handler is in the code."),
        }),
        &[],
    )
}

/// A request to save: the request's fields plus where it goes.
fn saved_request_schema() -> Value {
    let mut schema = request_schema();
    let props = schema["properties"].as_object_mut().expect("properties");
    props.insert("folder".into(), string("Folder names separated by /, e.g. \"Orders/Admin\" (\"\" = top level)."));
    props.insert(
        "path".into(),
        string("An existing request to update (from list_requests), instead of folder and name."),
    );
    schema
}

fn variables_schema() -> Value {
    json!({
        "type": "array",
        "items": obj(json!({
            "key": { "type": "string" },
            "value": { "type": "string" },
            "secret": { "type": "boolean", "description": "Kept on this computer only, never in the workspace files; shown to agents as ••••••." },
            "enabled": { "type": "boolean" },
        }), &["key", "value"]),
    })
}

/// A load test as agents give it.
fn load_test_schema() -> Value {
    obj(
        json!({
            "targets": {
                "type": "array",
                "description": "Saved requests to send.",
                "items": obj(json!({
                    "request": string("Request path as list_requests shows it, e.g. \"Users/List users.yaml\"."),
                    "weight": { "type": "integer", "description": "How often, relative to the others (default 1): weight 3 is sent three times as often." },
                    "enabled": { "type": "boolean" },
                    "captures": {
                        "type": "array",
                        "description": "Values saved from this request's responses as variables for the same virtual user's later requests, e.g. create an order, then GET /orders/{{orderId}}. With captures, each user sends the requests in order (by weight), so the create comes before the get. A capture that finds nothing keeps the variable's old value and counts as a capture miss (in the results, not an error). arrivalRate: each request is its own iteration, so captured values are not used by other requests (misses are still counted).",
                        "items": obj(json!({
                            "variable": string("Variable name, used as {{name}} in later requests (no spaces or braces)."),
                            "from": { "type": "string", "enum": ["json", "header", "regex"], "description": "json (default): a JSON path into the body. header: a response header by name (any case). regex: a regular expression over the body; its first group is the value (the whole match without a group)." },
                            "path": string("json: a path like $.id, $.items[0].id, $.items[-1].id (last item) or $['odd key'] (no wildcards or filters); strings are taken as they are, numbers and booleans as text, objects and arrays as JSON, null counts as a miss. header: the header name, e.g. ETag. regex: e.g. token=(\\w+). Only the first 1 MB of a body is searched."),
                        }), &["variable", "path"]),
                    },
                }), &["request"]),
            },
            "dataFile": string("CSV file with a header row, or a JSON array of objects: path relative to the workspace folder (it must be inside the workspace). virtualUsers: user N (in start order) takes row N % rows for its whole life. arrivalRate: each request takes the next row. Columns are variables ({{column}}) that override environment and workspace variables. Requests using them are rendered for every request."),
            "model": { "type": "string", "enum": ["virtualUsers", "arrivalRate"], "description": "virtualUsers (default): each user sends, waits for the answer, thinks, repeats; stage targets are users. arrivalRate: requests start at a fixed rate however slow the server is; stage targets are requests per second." },
            "stages": {
                "type": "array",
                "description": "Ramped linearly from 0, one after the other, e.g. [{durationSecs: 10, target: 20}, {durationSecs: 60, target: 20}, {durationSecs: 10, target: 0}]. durationSecs 0 jumps straight to the target.",
                "items": obj(json!({ "durationSecs": { "type": "integer" }, "target": { "type": "integer", "description": "Users (virtualUsers) or requests per second (arrivalRate)." } }), &["durationSecs", "target"]),
            },
            "thinkTimeMs": { "type": "integer", "description": "virtualUsers: pause after each answer, per user." },
            "maxInFlight": { "type": "integer", "description": "arrivalRate: most requests in flight at once (default 1000); more are counted as dropped." },
            "keepAlive": { "type": "boolean", "description": "Reuse connections (default true). false: a new connection per request." },
            "timeoutMs": { "type": "integer", "description": "Per-request timeout (default: the app setting)." },
            "httpVersion": { "type": "string", "enum": ["auto", "http1", "http2"] },
            "thresholds": {
                "type": "array",
                "description": "Checks that pass or fail the run, e.g. {metric: \"p95\", op: \"<\", value: 300} and {metric: \"errorRate\", op: \"<\", value: 1}.",
                "items": obj(json!({
                    "metric": { "type": "string", "enum": ["p50", "p90", "p95", "p99", "p999", "avg", "max", "errorRate", "rps"], "description": "p50…p999, avg, max: latency in milliseconds (p999 = 99.9th percentile). errorRate: failed requests (network errors and HTTP status >= 400) in percent, 0-100. rps: completed requests per second." },
                    "op": { "type": "string", "enum": ["<", "<=", ">", ">="] },
                    "value": { "type": "number", "description": "In the metric's unit: milliseconds, percent (1 = 1 %) or requests per second." },
                    "target": string("Only this request path (default: the whole test)."),
                    "enabled": { "type": "boolean" },
                }), &["metric", "op", "value"]),
            },
            "docs": string("Markdown notes."),
        }),
        &["targets", "stages"],
    )
}

fn wait_schema() -> Value {
    json!({ "type": "integer", "description": "Seconds to wait for the end (default 45, at most 600); still running then: call again with the runId." })
}

fn defs() -> Vec<Def> {
    let path = |what: &str| string(&format!("{what} path as list_requests shows it, e.g. \"Users/Get user.yaml\"."));
    vec![
        Def {
            name: "get_workspace",
            title: "Get the open workspace",
            description: "The workspace (collection) open in Zorvik: name, folder, environments, counts. Call this first.",
            schema: obj(json!({}), &[]),
            read_only: true,
            destructive: false,
            open_world: false,
        },
        Def {
            name: "list_requests",
            title: "List requests",
            description: "Folders and requests of the collection (or of one folder) with their method and URL.",
            schema: obj(json!({ "folder": string("Folder path (default: the whole collection).") }), &[]),
            read_only: true,
            destructive: false,
            open_world: false,
        },
        Def {
            name: "read_request",
            title: "Read a request",
            description: "Everything saved in one or more requests: URL, query and path parameters, headers, body, auth, scripts, docs.",
            schema: obj(
                json!({
                    "path": path("Request"),
                    "paths": { "type": "array", "items": { "type": "string" }, "description": "Several requests at once (up to 50), instead of path." },
                }),
                &[],
            ),
            read_only: true,
            destructive: false,
            open_world: false,
        },
        Def {
            name: "save_requests",
            title: "Save requests",
            description: "Create or update requests (up to 200 per call). Each goes into `folder` (folder names separated by /, created when missing) under its `name`; a request with that name already there is updated: the fields given change, the others stay (replace: true replaces it whole). Give `path` instead to update that exact request.",
            schema: obj(
                json!({
                    "requests": { "type": "array", "items": saved_request_schema() },
                    "replace": { "type": "boolean", "description": "Replace existing requests with exactly what is given (default: update only the fields given)." },
                }),
                &["requests"],
            ),
            read_only: false,
            destructive: false,
            open_world: false,
        },
        Def {
            name: "save_folder_settings",
            title: "Save folder settings",
            description: "Auth, headers, scripts and docs that requests in a folder inherit (merged into what is there). folder \"\" = the whole collection, which also has variables (merged by key).",
            schema: obj(
                json!({
                    "folder": string("Folder names separated by / (created when missing); \"\" = the collection."),
                    "auth": request_schema()["properties"]["auth"].clone(),
                    "headers": request_schema()["properties"]["headers"].clone(),
                    "scripts": request_schema()["properties"]["scripts"].clone(),
                    "docs": { "type": "string" },
                    "variables": variables_schema(),
                }),
                &["folder"],
            ),
            read_only: false,
            destructive: false,
            open_world: false,
        },
        Def {
            name: "move_item",
            title: "Move or rename",
            description: "Move a request or folder to another folder and/or rename it.",
            schema: obj(
                json!({ "path": path("Request or folder"), "toFolder": string("Destination folder path (\"\" = top level)."), "newName": { "type": "string" } }),
                &["path"],
            ),
            read_only: false,
            destructive: false,
            open_world: false,
        },
        Def {
            name: "delete_items",
            title: "Delete",
            description: "Delete requests, folders, environments, load tests or servers (to the trash). The user always confirms in Zorvik.",
            schema: obj(
                json!({
                    "paths": { "type": "array", "items": { "type": "string" }, "description": "Requests and folders (paths from list_requests)." },
                    "environments": { "type": "array", "items": { "type": "string" } },
                    "loadTests": { "type": "array", "items": { "type": "string" } },
                    "servers": { "type": "array", "items": { "type": "string" } },
                }),
                &[],
            ),
            read_only: false,
            destructive: true,
            open_world: false,
        },
        Def {
            name: "list_environments",
            title: "List environments",
            description: "Environments and their variables, the active one, and the collection's variables. Values that scripts saved (pm.environment.set, pm.collectionVariables.set) are included and marked setByScript: they are kept on this computer and win over the file's value. Secret values show as ••••••.",
            schema: obj(json!({}), &[]),
            read_only: true,
            destructive: false,
            open_world: false,
        },
        Def {
            name: "save_environment",
            title: "Save an environment",
            description: "Create an environment or update its variables (merged by key; `replace` to set exactly these). Put credentials in secret variables.",
            schema: obj(
                json!({ "name": { "type": "string" }, "variables": variables_schema(), "replace": { "type": "boolean" }, "activate": { "type": "boolean", "description": "Make it the active environment." } }),
                &["name"],
            ),
            read_only: false,
            destructive: false,
            open_world: false,
        },
        Def {
            name: "set_active_environment",
            title: "Switch environment",
            description: "Make an environment active (its variables apply to requests); an empty name for none.",
            schema: obj(json!({ "name": string("Environment name; \"\" for none.") }), &["name"]),
            read_only: false,
            destructive: false,
            open_world: false,
        },
        Def {
            name: "import",
            title: "Import",
            description: "Import requests from a Postman collection (or environment), an OpenAPI 3 / Swagger 2 document or a curl command: give its text, a URL, or an absolute file path (a file asks the user first). OpenAPI path parameters become {{variables}} (e.g. {session_id} → {{sessionId}}) in the new environment, with example values. An OpenAPI document without a full server URL needs baseUrl (the import says so). The document is kept in the workspace (specs/): every response to an imported request is then checked against the documented schema (a test named \"Matches the API spec\"), and update_from_openapi brings in a new version later.",
            schema: obj(
                json!({
                    "text": { "type": "string" }, "url": { "type": "string" }, "file": { "type": "string" },
                    "folder": string("Folder path to import into (default: top level)."),
                    "baseUrl": string("OpenAPI: where the API runs, e.g. http://localhost:8080 (needed when the document has no server URL or a relative one)."),
                }),
                &[],
            ),
            read_only: false,
            destructive: false,
            open_world: true,
        },
        Def {
            name: "update_from_openapi",
            title: "Update from OpenAPI",
            description: "Update a folder imported from an OpenAPI document from a new version of the document: new operations are added, changed ones updated field by field where the user left the field as the old document had it (their edits to docs, URLs, headers… stay), and operations the document no longer has are kept and marked removed. Scripts, settings and names are never touched. Call with preview: true first to see what would change, then without it to apply. Give the new document's text, url or file.",
            schema: obj(
                json!({
                    "folder": string("The imported folder (path as list_requests shows it)."),
                    "text": string("The new document (YAML or JSON)."),
                    "url": string("URL of the new document."),
                    "file": string("Absolute path of the new document (the user confirms reading it)."),
                    "preview": { "type": "boolean", "description": "Only say what would change (default false)." },
                }),
                &["folder"],
            ),
            read_only: false,
            destructive: false,
            open_world: true,
        },
        Def {
            name: "send_request",
            title: "Send a request",
            description: "Send a saved request (`path`), a saved one with changes (`path` + `request`: only the fields given change, nothing is saved), or an unsaved one (`request`), with its scripts and tests, and return the response. HTTP, GraphQL, gRPC (unary), DNS, and Server-Sent Events: an SSE request is read until the event named in stream.untilEvent, stream.maxEvents events, or stream.timeoutMs, and returns the events. The user sees it in Zorvik.",
            schema: obj(
                json!({
                    "path": path("Saved request"),
                    "request": request_schema(),
                    "folder": string("Unsaved request: the folder whose auth and headers it inherits."),
                    "maxBodyChars": { "type": "integer", "description": "Response body characters to return (default 20000, at most 80000)." },
                    "stream": obj(json!({
                        "untilEvent": string("SSE: stop after the first event with this name (\"message\" for events without a name)."),
                        "maxEvents": { "type": "integer", "description": "SSE: stop after this many events (default 100; 0 = only the time limit)." },
                        "timeoutMs": { "type": "integer", "description": "SSE: stop after this long (default 10000, at most 120000)." },
                    }), &[]),
                }),
                &[],
            ),
            read_only: false,
            destructive: false,
            open_world: true,
        },
        Def {
            name: "run_collection",
            title: "Run a collection",
            description: "Run the requests of a folder (or the collection) with their scripts and tests, like Zorvik's runner, and return the summary and the failures. The runner tab shows it live.",
            schema: obj(
                json!({
                    "folder": string("Folder path (default: the whole collection)."),
                    "requests": { "type": "array", "items": { "type": "string" }, "description": "Only these request paths, in this order." },
                    "iterations": { "type": "integer" },
                    "delayMs": { "type": "integer" },
                    "dataFile": string("CSV or JSON data file (one iteration per row), inside the workspace folder."),
                    "stopOnFailure": { "type": "boolean" },
                    "waitSeconds": wait_schema(),
                }),
                &[],
            ),
            read_only: false,
            destructive: false,
            open_world: true,
        },
        Def {
            name: "get_run_status",
            title: "Collection run status",
            description: "A collection run's summary and failures when it has finished; waits for it up to waitSeconds.",
            schema: obj(json!({ "runId": { "type": "string" }, "waitSeconds": wait_schema() }), &["runId"]),
            read_only: true,
            destructive: false,
            open_world: false,
        },
        Def {
            name: "stop_collection_run",
            title: "Stop a collection run",
            description: "Stop the collection run in progress.",
            schema: obj(json!({ "runId": { "type": "string" } }), &["runId"]),
            read_only: false,
            destructive: false,
            open_world: false,
        },
        Def {
            name: "graphql_schema",
            title: "GraphQL schema",
            description: "The schema of a GraphQL endpoint (introspection) as SDL, to write queries against.",
            schema: obj(
                json!({ "path": path("GraphQL request"), "request": request_schema(), "maxChars": { "type": "integer", "description": "SDL characters to return (default and most 60000)." } }),
                &[],
            ),
            read_only: true,
            destructive: false,
            open_world: true,
        },
        Def {
            name: "grpc_describe",
            title: "gRPC services",
            description: "Services and methods of a gRPC request (from its .proto files or server reflection).",
            schema: obj(json!({ "path": path("gRPC request"), "request": request_schema() }), &[]),
            read_only: true,
            destructive: false,
            open_world: true,
        },
        Def {
            name: "list_load_tests",
            title: "List load tests",
            description: "Saved load tests, and the one running.",
            schema: obj(json!({}), &[]),
            read_only: true,
            destructive: false,
            open_world: false,
        },
        Def {
            name: "read_load_test",
            title: "Read a load test",
            description: "A load test's targets, model, stages and thresholds.",
            schema: obj(json!({ "name": { "type": "string" } }), &["name"]),
            read_only: true,
            destructive: false,
            open_world: false,
        },
        Def {
            name: "save_load_test",
            title: "Save a load test",
            description: "Create or replace a load test. Latency thresholds are in milliseconds, errorRate in percent (1 = 1 %, not 0.01), rps in requests per second. Unknown or misspelled fields are refused.",
            schema: obj(json!({ "name": { "type": "string" }, "test": load_test_schema() }), &["name", "test"]),
            read_only: false,
            destructive: false,
            open_world: false,
        },
        Def {
            name: "run_load_test",
            title: "Run a load test",
            description: "Start a saved load test (the user always confirms in Zorvik) and return its results when it ends within waitSeconds; otherwise it returns the runId to poll with get_load_test_status. waitSeconds: 0 returns the runId as soon as it starts.",
            schema: obj(json!({ "name": { "type": "string" }, "waitSeconds": wait_schema() }), &["name"]),
            read_only: false,
            destructive: false,
            open_world: true,
        },
        Def {
            name: "get_load_test_status",
            title: "Load test status",
            description: "Live numbers of the running load test, or the results of a finished run.",
            schema: obj(
                json!({ "name": { "type": "string" }, "runId": { "type": "string" }, "waitSeconds": wait_schema() }),
                &["name", "runId"],
            ),
            read_only: true,
            destructive: false,
            open_world: false,
        },
        Def {
            name: "stop_load_test",
            title: "Stop the load test",
            description: "Stop the running load test (its results are kept).",
            schema: obj(json!({}), &[]),
            read_only: false,
            destructive: false,
            open_world: false,
        },
        Def {
            name: "open_workspace",
            title: "Open a workspace",
            description: "Open another workspace folder in Zorvik, or create one there with create: true (the user confirms).",
            schema: obj(
                json!({ "path": string("Absolute folder path, e.g. <repo>/.zorvik."), "create": { "type": "boolean" }, "name": string("Name of a new workspace.") }),
                &["path"],
            ),
            read_only: false,
            destructive: false,
            open_world: false,
        },
        Def {
            name: "open_in_app",
            title: "Show in Zorvik",
            description: "Open something in the Zorvik window for the user: a request, a folder's runner, a load test, a server or the environments.",
            schema: obj(
                json!({ "request": { "type": "string" }, "runner": string("Folder path (\"\" = collection)."), "loadTest": { "type": "string" }, "server": { "type": "string" }, "environments": { "type": "boolean" } }),
                &[],
            ),
            read_only: true,
            destructive: false,
            open_world: false,
        },
    ]
}

/// Every tool: the ones here, then each area's.
fn all_defs() -> Vec<Def> {
    let mut all = defs();
    all.extend(servers::defs());
    all.extend(state::defs());
    all.extend(files::defs());
    all
}

/// `tools/list` entries.
pub fn tool_definitions() -> Vec<Value> {
    all_defs()
        .into_iter()
        .map(|d| {
            json!({
                "name": d.name,
                "title": d.title,
                "description": d.description,
                "inputSchema": d.schema,
                "annotations": {
                    "title": d.title,
                    "readOnlyHint": d.read_only,
                    "destructiveHint": d.destructive,
                    "openWorldHint": d.open_world,
                },
            })
        })
        .collect()
}

// ---- calling -----------------------------------------------------------------------------------

enum Fail {
    /// Bad arguments: the agent can fix them.
    Invalid(String),
    /// The user said no (or wasn't asked in time).
    Denied(String),
    Error(String),
}

impl From<ApiError> for Fail {
    fn from(e: ApiError) -> Self {
        match e.code.as_str() {
            "invalidInput" | "notFound" | "invalidName" => Fail::Invalid(e.message),
            "noWorkspace" => Fail::Invalid(NO_WORKSPACE.into()),
            _ => Fail::Error(e.message),
        }
    }
}

impl From<zorvik_workspace::Error> for Fail {
    fn from(e: zorvik_workspace::Error) -> Self {
        ApiError::from(e).into()
    }
}

const NO_WORKSPACE: &str = "No workspace is open in Zorvik. Ask the user which folder to use (a .zorvik folder in the repository is a good default) and call open_workspace.";

struct Done {
    value: Value,
    detail: String,
    target: Option<AgentTarget>,
}

impl Done {
    fn new(value: Value, detail: impl Into<String>) -> Self {
        Self { value, detail: detail.into(), target: None }
    }

    fn at(mut self, target: AgentTarget) -> Self {
        self.target = Some(target);
        self
    }
}

type Outcome = Result<Done, Fail>;
type BoxedOutcome<'a> = std::pin::Pin<Box<dyn std::future::Future<Output = Outcome> + Send + 'a>>;

struct Call<'a> {
    session: &'a Arc<SessionState>,
    args: Value,
    progress: ProgressFn,
    cancel: CancellationToken,
}

impl Call<'_> {
    /// A question that got no "yes": declined, unanswered, or the agent cancelled the call.
    fn refused(&self, message: String) -> Fail {
        if self.cancel.is_cancelled() { Fail::Error(message) } else { Fail::Denied(message) }
    }

    fn args<T: DeserializeOwned>(&self) -> Result<T, Fail> {
        let args = if self.args.is_null() { json!({}) } else { self.args.clone() };
        serde_json::from_value(args).map_err(|e| Fail::Invalid(format!("Invalid arguments: {e}")))
    }
}

/// What the activity log says for a call, before it runs.
fn describe(name: &str, args: &Value) -> String {
    let s = |key: &str| args.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
    let count = |key: &str| args.get(key).and_then(Value::as_array).map_or(0, Vec::len);
    let title = all_defs().into_iter().find(|d| d.name == name).map_or(name, |d| d.title);
    let what = match name {
        "read_request" | "write_file" => s("path"),
        "export_request" => {
            [s("path"), s("format")].into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>().join(" as ")
        }
        "save_requests" => plural(count("requests"), "request"),
        "save_folder_settings" => {
            if s("folder").is_empty() {
                "collection".into()
            } else {
                s("folder")
            }
        }
        "move_item" => s("path"),
        "save_environment" | "read_load_test" | "save_load_test" | "run_load_test" | "start_server" | "stop_server"
        | "read_server" | "save_server" | "create_mock" | "get_server_traffic" => s("name"),
        "set_active_environment" => {
            args.get("name").and_then(Value::as_str).map_or_else(|| "none".into(), str::to_string)
        }
        "send_request" => {
            let req = args.get("request");
            let field = |k: &str| req.and_then(|r| r.get(k)).and_then(Value::as_str).unwrap_or_default();
            match (s("path"), field("url")) {
                (p, _) if !p.is_empty() => p,
                (_, url) => format!("{} {url}", if field("method").is_empty() { "GET" } else { field("method") }),
            }
        }
        "run_collection" => {
            if s("folder").is_empty() {
                "collection".into()
            } else {
                s("folder")
            }
        }
        "delete_items" => {
            plural(count("paths") + count("environments") + count("loadTests") + count("servers"), "item")
        }
        "update_from_openapi" => s("folder"),
        "open_workspace" | "import" => {
            [s("path"), s("url"), s("file")].into_iter().find(|v| !v.is_empty()).unwrap_or_default()
        }
        _ => String::new(),
    };
    if what.is_empty() { title.to_string() } else { format!("{title}: {what}") }
}

fn plural(n: usize, what: &str) -> String {
    format!("{n} {what}{}", if n == 1 { "" } else { "s" })
}

impl Api {
    /// Run one tool call of an agent (MCP `tools/call`).
    pub async fn agent_call(
        &self,
        session: &AgentSessionHandle,
        name: &str,
        args: Value,
        progress: ProgressFn,
        cancel: CancellationToken,
    ) -> ToolOutput {
        if !all_defs().iter().any(|d| d.name == name) {
            return ToolOutput::error(format!("Unknown tool '{name}'"));
        }
        let activity = self.activity_start(&session.0, name, describe(name, &args));
        let call = Call { session: &session.0, args, progress, cancel };
        // No network until a tool's `gate_traffic` says which hosts (fail closed).
        let outcome = super::with_guard(Some(super::no_hosts()), async {
            self.agent_enabled(call.session, &call.cancel).await.map_err(|e| call.refused(e))?;
            self.run_tool(name, &call).await
        })
        .await;
        // Last line of defence: secret values never go back to the agent, errors included.
        let redactor = self.try_ws().map(|ws| self.redactor(&ws)).unwrap_or_default();
        match outcome {
            Ok(done) => {
                let value = redact_value(&done.value, &redactor);
                self.activity_finish(&activity, ActivityStatus::Done, Some(redactor.text(&done.detail)), done.target);
                let text = serde_json::to_string_pretty(&value).unwrap_or_default();
                let structured = value.is_object().then_some(value);
                ToolOutput { text, structured, is_error: false }
            }
            Err(fail) => {
                let (status, message) = match fail {
                    Fail::Denied(m) => (ActivityStatus::Denied, m),
                    Fail::Invalid(m) | Fail::Error(m) => (ActivityStatus::Failed, m),
                };
                let message = redactor.text(&message);
                self.activity_finish(&activity, status, Some(message.clone()), None);
                ToolOutput::error(message)
            }
        }
    }

    /// The tool's work, on the heap. Each future is built in its own short call: an
    /// unoptimized build would otherwise reserve stack for every tool's future at once
    /// (and overflow the 2 MB of a Tokio worker).
    fn run_tool<'a>(&'a self, name: &str, c: &'a Call<'a>) -> BoxedOutcome<'a> {
        macro_rules! run {
            ($work:expr) => {
                in_own_frame(move || Box::pin($work))
            };
        }
        match name {
            "get_workspace" => run!(async move { self.tool_get_workspace() }),
            "list_requests" => run!(self.tool_list_requests(c)),
            "read_request" => run!(async move { self.tool_read_request(c) }),
            "save_requests" => run!(self.tool_save_requests(c)),
            "save_folder_settings" => run!(self.tool_save_folder_settings(c)),
            "move_item" => run!(self.tool_move_item(c)),
            "delete_items" => run!(self.tool_delete_items(c)),
            "list_environments" => run!(async move { self.tool_list_environments() }),
            "save_environment" => run!(self.tool_save_environment(c)),
            "set_active_environment" => run!(self.tool_set_active_environment(c)),
            "import" => run!(self.tool_import(c)),
            "update_from_openapi" => run!(self.tool_update_from_openapi(c)),
            "send_request" => run!(self.tool_send_request(c)),
            "run_collection" => run!(self.tool_run_collection(c)),
            "get_run_status" => run!(self.tool_run_status(c)),
            "stop_collection_run" => run!(self.tool_stop_run(c)),
            "graphql_schema" => run!(self.tool_graphql_schema(c)),
            "grpc_describe" => run!(self.tool_grpc_describe(c)),
            "list_load_tests" => run!(async move { self.tool_list_load_tests() }),
            "read_load_test" => run!(async move { self.tool_read_load_test(c) }),
            "save_load_test" => run!(self.tool_save_load_test(c)),
            "run_load_test" => run!(self.tool_run_load_test(c)),
            "get_load_test_status" => run!(self.tool_load_status(c)),
            "stop_load_test" => run!(self.tool_stop_load_test()),
            "list_servers" => run!(async move { self.tool_list_servers() }),
            "read_server" => run!(async move { self.tool_read_server(c) }),
            "save_server" => run!(self.tool_save_server(c)),
            "create_mock" => run!(self.tool_create_mock(c)),
            "start_server" => run!(self.tool_start_server(c)),
            "stop_server" => run!(self.tool_stop_server(c)),
            "get_server_traffic" => run!(async move { self.tool_server_traffic(c) }),
            "get_variables" => run!(async move { self.tool_get_variables() }),
            "read_history" => run!(async move { self.tool_read_history(c) }),
            "write_file" => run!(self.tool_write_file(c)),
            "export_request" => run!(async move { self.tool_export_request(c) }),
            "open_workspace" => run!(self.tool_open_workspace(c)),
            "open_in_app" => run!(async move { self.tool_open_in_app(c) }),
            _ => {
                let unknown = Fail::Invalid(format!("Unknown tool '{name}'"));
                run!(async move { Err(unknown) })
            }
        }
    }

    fn agent_ws(&self) -> Result<Workspace, Fail> {
        self.try_ws().ok_or_else(|| Fail::Invalid(NO_WORKSPACE.into()))
    }

    /// Show something in the app (the UI follows only when the user wants it to).
    fn show(&self, target: AgentTarget) {
        self.emit_agent(AgentEvent::Show { target, explicit: false });
    }

    // ---- approval ------------------------------------------------------------------------------

    async fn approve(&self, c: &Call<'_>, ask: Ask) -> Result<bool, Fail> {
        // Progress while waiting: clients that reset their timeout on progress keep waiting.
        let started = Instant::now();
        let waiting = async {
            loop {
                tokio::time::sleep(Duration::from_secs(5)).await;
                (c.progress)(started.elapsed().as_secs_f64(), None, "Waiting for the user's answer in Zorvik");
            }
        };
        let answer = tokio::select! {
            answer = self.agent_ask(c.session, ask, &c.cancel) => answer,
            _ = waiting => unreachable!("waits forever"),
        };
        answer.map_err(|e| c.refused(e))
    }

    /// Edits: allowed, or asked (once per session when the user says so).
    async fn gate_change(&self, c: &Call<'_>, title: String, items: Vec<String>) -> Result<(), Fail> {
        if self.settings().agents.changes == AgentChanges::Allow || lock(&c.session.grants).changes {
            return Ok(());
        }
        let ask = Ask {
            kind: ConfirmKind::Change,
            title,
            message: format!("{} wants to change your collection:", c.session.info.client),
            items: cap_items(items),
            confirm_label: "Allow".into(),
            session_option: true,
            danger: false,
        };
        if self.approve(c, ask).await? {
            lock(&c.session.grants).changes = true;
        }
        Ok(())
    }

    /// Requests to `hosts`: allowed, or asked per the settings. Returns the guard that
    /// keeps the requests (scripts, redirects, token requests) to approved hosts.
    async fn gate_traffic(&self, c: &Call<'_>, what: &str, hosts: Vec<String>) -> Result<Option<HostGuard>, Fail> {
        let policy = self.settings().agents.traffic;
        if policy == AgentTraffic::Allow {
            return Ok(None);
        }
        // "Ask every time" asks about this computer and private networks too; either way,
        // hosts allowed for the session aren't asked about again (and only those hosts).
        let ask_all = policy == AgentTraffic::Ask;
        let mut hosts: BTreeSet<String> = hosts.into_iter().map(|h| h.to_ascii_lowercase()).collect();
        let known = {
            let mut grants = lock(&c.session.grants);
            hosts.extend(grants.blocked.drain());
            grants.hosts.clone()
        };
        let need: Vec<String> =
            hosts.iter().filter(|h| (ask_all || !is_local(h)) && !known.contains(*h)).cloned().collect();
        let mut allowed: HashSet<String> = HashSet::new();
        if !need.is_empty() {
            let outside = need.iter().any(|h| !is_local(h));
            let ask = Ask {
                kind: ConfirmKind::Traffic,
                title: format!("{what}?"),
                message: format!("{} wants to send requests to:", c.session.info.client),
                items: cap_items(need.clone()),
                confirm_label: "Send".into(),
                session_option: true,
                danger: outside,
            };
            let for_session = self.approve(c, ask).await?;
            if for_session {
                lock(&c.session.grants).hosts.extend(need.iter().cloned());
            }
            allowed.extend(need);
        }
        let session = c.session.clone();
        Ok(Some(HostGuard(Arc::new(move |host: &str| {
            let mut grants = lock(&session.grants);
            let ok = (!ask_all && is_local(host)) || allowed.contains(host) || grants.hosts.contains(host);
            if !ok {
                grants.blocked.insert(host.to_string());
            }
            ok
        }))))
    }

    /// Hosts a request goes to: its URL and an OAuth 2.0 token URL.
    fn request_hosts(&self, ws: &Workspace, request: &Request, path: Option<&str>) -> Vec<String> {
        let mut urls = Vec::new();
        match self.resolve_only(ws, request, path) {
            Ok(resolved) => {
                urls.push(resolved.request.url.clone());
                if let Some(config) = &resolved.oauth2 {
                    urls.push(config.token_url.clone());
                }
            }
            Err(_) => urls.push(self.var_context(ws).render(&request.url, &mut BTreeSet::new())),
        }
        let mut hosts: Vec<String> = urls.iter().filter_map(|u| host_of(u)).collect();
        hosts.dedup();
        hosts
    }

    /// The resolver a DNS request asks (none for the system's resolvers).
    fn dns_hosts(&self, ws: &Workspace, request: &Request) -> Vec<String> {
        let server = self.var_context(ws).render(request.dns.server.trim(), &mut BTreeSet::new());
        match zorvik_engine::DnsResolver::parse(&server) {
            Ok(zorvik_engine::DnsResolver::Udp { host, .. })
            | Ok(zorvik_engine::DnsResolver::Tcp { host, .. })
            | Ok(zorvik_engine::DnsResolver::Tls { host, .. }) => vec![host.to_ascii_lowercase()],
            Ok(zorvik_engine::DnsResolver::Https { url }) => host_of(&url).into_iter().collect(),
            Ok(zorvik_engine::DnsResolver::System) => Vec::new(),
            // Unparseable: the query fails before sending anything.
            Err(_) => Vec::new(),
        }
    }

    // ---- workspace -----------------------------------------------------------------------------

    fn tool_get_workspace(&self) -> Outcome {
        let Some(ws) = self.try_ws() else {
            let recent: Vec<Value> =
                lock(&self.inner.local).recent.iter().map(|r| json!({ "name": r.name, "path": r.path })).collect();
            return Ok(Done::new(
                json!({ "open": false, "recent": recent, "hint": NO_WORKSPACE }),
                "No workspace open",
            ));
        };
        let tree = ws.tree()?;
        let (mut requests, mut folders) = (0, 0);
        walk(&tree, &mut |n| match n.kind {
            NodeKind::Request => requests += 1,
            NodeKind::Folder => folders += 1,
        });
        let environments = self.environments(&ws)?;
        let active = self.active_env_id(&ws);
        let value = json!({
            "open": true,
            "name": ws.meta().name,
            "path": ws.root().to_string_lossy(),
            "requests": requests,
            "folders": folders,
            "environments": environments.iter().map(|e| &e.environment.name).collect::<Vec<_>>(),
            "activeEnvironment": environments.iter().find(|e| Some(&e.id) == active.as_ref()).map(|e| &e.environment.name),
            "collectionVariables": ws.meta().variables.iter().map(|v| &v.key).collect::<Vec<_>>(),
            "loadTests": ws.list_load_tests().map(|l| l.len()).unwrap_or(0),
            "servers": ws.list_servers().map(|l| l.len()).unwrap_or(0),
            "files": "Requests are YAML files under requests/ in the workspace folder (environments/, loadtests/, servers/ beside it).",
        });
        Ok(Done::new(value, format!("{}: {}", ws.meta().name, plural(requests, "request"))))
    }

    async fn tool_list_requests(&self, c: &Call<'_>) -> Outcome {
        #[derive(Deserialize)]
        struct A {
            #[serde(default)]
            folder: String,
        }
        let A { folder } = c.args()?;
        let ws = self.agent_ws()?;
        let tree = ws.tree()?;
        let folder = folder.trim_matches('/').to_string();
        let nodes = if folder.is_empty() {
            tree
        } else {
            find_node(&tree, &folder)
                .filter(|n| n.kind == NodeKind::Folder)
                .map(|n| n.children.clone())
                .ok_or_else(|| Fail::Invalid(format!("Folder '{folder}' not found (paths come from list_requests)")))?
        };
        let items = tokio::task::spawn_blocking(move || {
            let mut items = Vec::new();
            let mut read = 0;
            walk(&nodes, &mut |n| {
                let mut item = json!({ "path": n.path, "type": if n.kind == NodeKind::Folder { "folder" } else { "request" }, "name": n.name });
                if n.kind == NodeKind::Request {
                    if let Some(m) = &n.method {
                        item["method"] = json!(if n.graphql { "GraphQL" } else { m.as_str() });
                    }
                    if let Some(kind) = n.request_kind.filter(|k| *k != RequestKind::Http) {
                        item["kind"] = json!(kind);
                    }
                    if read < MAX_LISTED
                        && let Ok(r) = ws.read_request(&n.path)
                    {
                        read += 1;
                        item["url"] = json!(r.url);
                    }
                    if let Some(e) = &n.error {
                        item["error"] = json!(e);
                    }
                }
                items.push(item);
            });
            items
        })
        .await
        .map_err(|e| Fail::Error(e.to_string()))?;
        let n = items.iter().filter(|i| i["type"] == "request").count();
        Ok(Done::new(json!({ "items": items }), plural(n, "request")))
    }

    fn tool_read_request(&self, c: &Call<'_>) -> Outcome {
        #[derive(Deserialize)]
        struct A {
            path: Option<String>,
            paths: Option<Vec<String>>,
        }
        let A { path, paths } = c.args()?;
        let ws = self.agent_ws()?;
        match (path, paths) {
            (Some(path), None) => {
                let request = ws.read_request(&path)?;
                Ok(Done::new(json!({ "path": path, "request": request }), path.clone())
                    .at(AgentTarget::Request { path }))
            }
            (None, Some(paths)) => {
                if paths.is_empty() || paths.len() > MAX_READ {
                    return Err(Fail::Invalid(format!("paths: give 1 to {MAX_READ} request paths")));
                }
                let requests: Vec<Value> = paths
                    .iter()
                    .map(|p| match ws.read_request(p) {
                        Ok(request) => json!({ "path": p, "request": request }),
                        Err(e) => json!({ "path": p, "error": e.to_string() }),
                    })
                    .collect();
                Ok(Done::new(json!({ "requests": requests }), plural(paths.len(), "request")))
            }
            _ => Err(Fail::Invalid("Give path, or paths for several requests".into())),
        }
    }

    async fn tool_save_requests(&self, c: &Call<'_>) -> Outcome {
        #[derive(Deserialize)]
        struct A {
            requests: Vec<Value>,
            /// Replace existing requests with exactly what is given (default: update the fields given).
            #[serde(default)]
            replace: bool,
        }
        let A { requests, replace } = c.args()?;
        if requests.is_empty() {
            return Err(Fail::Invalid("requests is empty".into()));
        }
        if requests.len() > MAX_BATCH {
            return Err(Fail::Invalid(format!("At most {MAX_BATCH} requests per call; send them in batches")));
        }
        let ws = self.agent_ws()?;
        let mut items = Vec::new();
        for (i, raw) in requests.into_iter().enumerate() {
            let Value::Object(mut map) = raw else {
                return Err(Fail::Invalid(format!("requests[{i}] is not an object")));
            };
            let folder = map.remove("folder").and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
            let path = map.remove("path").and_then(|v| v.as_str().map(str::to_string)).filter(|p| !p.trim().is_empty());
            let patch = Value::Object(map);
            let request = parse_request(patch.clone()).map_err(|e| Fail::Invalid(format!("requests[{i}]: {e}")))?;
            if request.name.trim().is_empty() && path.is_none() {
                return Err(Fail::Invalid(format!("requests[{i}] needs a name")));
            }
            items.push((folder, path, request, patch));
        }
        // An existing request gets the fields given (all of them with `replace`).
        let update = |existing: Request, request: Request, patch: Value, i: usize| -> Result<Request, Fail> {
            if replace {
                let name = if request.name.trim().is_empty() { existing.name } else { request.name.clone() };
                return Ok(Request { name, ..request });
            }
            merge_request(&existing, patch).map_err(|e| Fail::Invalid(format!("requests[{i}]: {e}")))
        };
        let lines = items
            .iter()
            .map(|(folder, path, r, _)| {
                let at = path.clone().unwrap_or_else(|| join(folder, &r.name));
                format!("{} {at}", method_label(r))
            })
            .collect();
        self.gate_change(c, format!("Save {}?", plural(items.len(), "request")), lines).await?;

        let mut saved = Vec::new();
        for (i, (folder, path, request, patch)) in items.into_iter().enumerate() {
            let (path, created) = match path {
                Some(path) => {
                    let existing = ws.read_request(&path)?;
                    ws.save_request(&path, &update(existing, request, patch, i)?)?;
                    (path, false)
                }
                None => {
                    let parent = self.ensure_folder(&ws, &folder)?;
                    let tree = ws.tree()?;
                    let siblings =
                        if parent.is_empty() { Some(&tree) } else { find_node(&tree, &parent).map(|n| &n.children) };
                    let same = siblings.and_then(|s| {
                        s.iter().find(|n| {
                            n.kind == NodeKind::Request && n.name.trim().eq_ignore_ascii_case(request.name.trim())
                        })
                    });
                    match same {
                        Some(node) => {
                            let existing = ws.read_request(&node.path)?;
                            ws.save_request(&node.path, &update(existing, request, patch, i)?)?;
                            (node.path.clone(), false)
                        }
                        None => (ws.create_request(&parent, request)?, true),
                    }
                }
            };
            saved.push(json!({ "path": path, "created": created }));
        }
        let (created, updated) = saved.iter().partition::<Vec<_>, _>(|s| s["created"] == true);
        let detail = match (created.len(), updated.len()) {
            (c, 0) => format!("Added {}", plural(c, "request")),
            (0, u) => format!("Updated {}", plural(u, "request")),
            (c, u) => format!("Added {c}, updated {u}"),
        };
        let first = saved[0]["path"].as_str().unwrap_or_default().to_string();
        let target = if saved.len() == 1 {
            AgentTarget::Request { path: first }
        } else {
            AgentTarget::Folder { path: parent_of(&first).to_string() }
        };
        self.show(target.clone());
        Ok(Done::new(json!({ "saved": saved }), detail).at(target))
    }

    /// The folder at `names` (folder names or paths separated by /), created when missing.
    fn ensure_folder(&self, ws: &Workspace, names: &str) -> Result<String, Fail> {
        let mut path = String::new();
        for name in names.split('/').map(str::trim).filter(|n| !n.is_empty()) {
            let tree = ws.tree()?;
            let children = if path.is_empty() { Some(&tree) } else { find_node(&tree, &path).map(|n| &n.children) };
            let existing = children.and_then(|c| {
                c.iter().find(|n| {
                    n.kind == NodeKind::Folder
                        && (n.name.trim().eq_ignore_ascii_case(name) || n.path.rsplit('/').next() == Some(name))
                })
            });
            path = match existing {
                Some(n) => n.path.clone(),
                None => ws.create_folder(&path, name)?,
            };
        }
        Ok(path)
    }

    async fn tool_save_folder_settings(&self, c: &Call<'_>) -> Outcome {
        #[derive(Deserialize)]
        struct A {
            folder: String,
            auth: Option<Value>,
            headers: Option<Value>,
            scripts: Option<Scripts>,
            docs: Option<String>,
            variables: Option<Vec<Variable>>,
        }
        let A { folder, auth, headers, scripts, docs, variables } = c.args()?;
        let auth = auth.map(parse_auth).transpose().map_err(Fail::Invalid)?;
        let headers = headers.map(parse_headers).transpose().map_err(Fail::Invalid)?;
        let ws = self.agent_ws()?;
        let root = folder.trim_matches('/').is_empty();
        if variables.is_some() && !root {
            return Err(Fail::Invalid(
                "Only the collection (folder \"\") has variables; use save_environment for per-environment values"
                    .into(),
            ));
        }
        let name = if root { "the collection".to_string() } else { format!("folder {folder}") };
        let mut changed: Vec<String> = Vec::new();
        for (set, what) in [
            (auth.is_some(), "auth"),
            (headers.is_some(), "headers"),
            (scripts.is_some(), "scripts"),
            (docs.is_some(), "docs"),
            (variables.is_some(), "variables"),
        ] {
            if set {
                changed.push(what.into());
            }
        }
        self.gate_change(c, format!("Change the settings of {name}?"), vec![changed.join(", ")]).await?;
        if root {
            let mut meta = self.revealed_meta(&ws);
            if let Some(a) = auth {
                meta.auth = a;
            }
            if let Some(h) = headers {
                meta.headers = h;
            }
            if let Some(s) = scripts {
                meta.scripts = s;
            }
            if let Some(d) = docs {
                meta.docs = d;
            }
            if let Some(v) = variables {
                meta.variables = merge_variables(&meta.variables, v, false);
            }
            self.call("workspace.saveMeta", json!({ "meta": meta })).await?;
            self.show(AgentTarget::Folder { path: String::new() });
            return Ok(Done::new(json!({ "saved": "collection" }), format!("Updated {}", changed.join(", ")))
                .at(AgentTarget::Folder { path: String::new() }));
        }
        let path = self.ensure_folder(&ws, &folder)?;
        let mut meta: FolderMeta = ws.read_folder(&path)?;
        if let Some(a) = auth {
            meta.auth = a;
        }
        if let Some(h) = headers {
            meta.headers = h;
        }
        if let Some(s) = scripts {
            meta.scripts = s;
        }
        if let Some(d) = docs {
            meta.docs = d;
        }
        ws.save_folder(&path, &meta)?;
        self.show(AgentTarget::Folder { path: path.clone() });
        Ok(Done::new(json!({ "saved": path }), format!("Updated {}", changed.join(", ")))
            .at(AgentTarget::Folder { path }))
    }

    async fn tool_move_item(&self, c: &Call<'_>) -> Outcome {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct A {
            path: String,
            to_folder: Option<String>,
            new_name: Option<String>,
        }
        let A { path, to_folder, new_name } = c.args()?;
        if to_folder.is_none() && new_name.is_none() {
            return Err(Fail::Invalid("Give toFolder and/or newName".into()));
        }
        let ws = self.agent_ws()?;
        let mut line = path.clone();
        if let Some(f) = &to_folder {
            line.push_str(&format!(" → {}", if f.is_empty() { "top level" } else { f }));
        }
        if let Some(n) = &new_name {
            line.push_str(&format!(" (as “{n}”)"));
        }
        self.gate_change(c, "Move or rename?".into(), vec![line]).await?;
        let mut current = path;
        if let Some(name) = new_name {
            current = serde_json::from_value(self.call("item.rename", json!({ "path": current, "name": name })).await?)
                .map_err(|e| Fail::Error(e.to_string()))?;
        }
        if let Some(folder) = to_folder {
            let parent = self.ensure_folder(&ws, &folder)?;
            current = serde_json::from_value(
                self.call("item.move", json!({ "path": current, "parent": parent, "index": null })).await?,
            )
            .map_err(|e| Fail::Error(e.to_string()))?;
        }
        let target = if current.ends_with(".yaml") {
            AgentTarget::Request { path: current.clone() }
        } else {
            AgentTarget::Folder { path: current.clone() }
        };
        Ok(Done::new(json!({ "path": current }), current).at(target))
    }

    async fn tool_delete_items(&self, c: &Call<'_>) -> Outcome {
        #[derive(Deserialize, Default)]
        #[serde(rename_all = "camelCase", default)]
        struct A {
            paths: Vec<String>,
            environments: Vec<String>,
            load_tests: Vec<String>,
            servers: Vec<String>,
        }
        let a: A = c.args()?;
        let ws = self.agent_ws()?;
        let mut plan: Vec<(String, &str, String)> = Vec::new(); // (label, method, id)
        let tree = ws.tree()?;
        for p in &a.paths {
            let node = find_node(&tree, p.trim_matches('/'))
                .ok_or_else(|| Fail::Invalid(format!("'{p}' not found (paths come from list_requests)")))?;
            let label = match node.kind {
                NodeKind::Folder => {
                    let mut n = 0;
                    walk(&node.children, &mut |x| n += usize::from(x.kind == NodeKind::Request));
                    format!("Folder {} ({} inside)", node.path, plural(n, "request"))
                }
                NodeKind::Request => format!("Request {}", node.path),
            };
            plan.push((label, "item.delete", node.path.clone()));
        }
        let envs = self.environments(&ws)?;
        for name in &a.environments {
            let e = envs
                .iter()
                .find(|e| e.environment.name.eq_ignore_ascii_case(name) || e.id == *name)
                .ok_or_else(|| Fail::Invalid(format!("Environment '{name}' not found")))?;
            plan.push((format!("Environment {}", e.environment.name), "env.delete", e.id.clone()));
        }
        let tests = ws.list_load_tests()?;
        for name in &a.load_tests {
            let t = tests
                .iter()
                .find(|t| t.name.eq_ignore_ascii_case(name) || t.id == *name)
                .ok_or_else(|| Fail::Invalid(format!("Load test '{name}' not found")))?;
            plan.push((format!("Load test {} (and its run history)", t.name), "load.delete", t.id.clone()));
        }
        let servers = ws.list_servers()?;
        for name in &a.servers {
            let s = servers
                .iter()
                .find(|s| s.name.eq_ignore_ascii_case(name) || s.id == *name)
                .ok_or_else(|| Fail::Invalid(format!("Server '{name}' not found")))?;
            plan.push((format!("Server {}", s.name), "server.delete", s.id.clone()));
        }
        if plan.is_empty() {
            return Err(Fail::Invalid("Nothing to delete".into()));
        }
        let ask = Ask {
            kind: ConfirmKind::Delete,
            title: format!("Delete {}?", plural(plan.len(), "item")),
            message: format!("{} wants to delete (to the trash):", c.session.info.client),
            items: cap_items(plan.iter().map(|(l, ..)| l.clone()).collect()),
            confirm_label: "Delete".into(),
            session_option: false,
            danger: true,
        };
        self.approve(c, ask).await?;
        let mut deleted = Vec::new();
        let mut failed = Vec::new();
        for (label, method, id) in plan {
            let params = if method == "item.delete" { json!({ "path": id }) } else { json!({ "id": id }) };
            match self.call(method, params).await {
                Ok(_) => deleted.push(label),
                Err(e) => failed.push(json!({ "item": label, "error": e.message })),
            }
        }
        let detail = format!("Deleted {}", plural(deleted.len(), "item"));
        Ok(Done::new(json!({ "deleted": deleted, "failed": failed }), detail))
    }

    // ---- environments ----------------------------------------------------------------------------

    fn tool_list_environments(&self) -> Outcome {
        let ws = self.agent_ws()?;
        let envs = self.environments(&ws)?;
        let active = self.active_env_id(&ws);
        let local = self.local_values();
        // File values, then what scripts saved in that scope (kept on this computer, winning).
        let listed = |vars: &[Variable], scope: &str, env: Option<&str>| -> Vec<Value> {
            let mut out: Vec<Value> = vars
                .iter()
                .map(|v| {
                    let mut o = json!({ "key": v.key, "value": if v.secret { MASK } else { v.value.as_str() } });
                    if v.secret {
                        o["secret"] = json!(true);
                    }
                    if !v.enabled {
                        o["enabled"] = json!(false);
                    }
                    o
                })
                .collect();
            for l in local.iter().filter(|l| l.scope == scope && l.environment_id.as_deref() == env) {
                let secret = vars.iter().any(|v| v.key == l.key && v.secret);
                let value = if secret { MASK } else { l.value.as_str() };
                match out.iter_mut().find(|o| o["key"] == l.key.as_str()) {
                    Some(o) => {
                        o["value"] = json!(value);
                        o["setByScript"] = json!(true);
                    }
                    None => out.push(json!({ "key": l.key, "value": value, "setByScript": true })),
                }
            }
            out
        };
        let value = json!({
            "active": envs.iter().find(|e| Some(&e.id) == active.as_ref()).map(|e| &e.environment.name),
            "environments": envs.iter().map(|e| json!({
                "name": e.environment.name,
                "active": Some(&e.id) == active.as_ref(),
                "variables": listed(&e.environment.variables, "environment", Some(&e.id)),
            })).collect::<Vec<_>>(),
            "collectionVariables": listed(&self.revealed_meta(&ws).variables, "workspace", None),
        });
        Ok(Done::new(value, plural(envs.len(), "environment")).at(AgentTarget::Environments))
    }

    async fn tool_save_environment(&self, c: &Call<'_>) -> Outcome {
        #[derive(Deserialize)]
        struct A {
            name: String,
            #[serde(default)]
            variables: Vec<Variable>,
            #[serde(default)]
            replace: bool,
            #[serde(default)]
            activate: bool,
        }
        let A { name, variables, replace, activate } = c.args()?;
        let ws = self.agent_ws()?;
        let envs = self.environments(&ws)?;
        let existing = envs.iter().find(|e| e.environment.name.trim().eq_ignore_ascii_case(name.trim()));
        let lines = variables
            .iter()
            .map(|v| format!("{} = {}", v.key, if v.secret { MASK } else { v.value.as_str() }))
            .collect();
        let verb = if existing.is_some() { "Update" } else { "Create" };
        self.gate_change(c, format!("{verb} environment “{name}”?"), lines).await?;
        let id = match existing {
            Some(e) => {
                let environment = Environment {
                    name: e.environment.name.clone(),
                    variables: merge_variables(&e.environment.variables, variables, replace),
                };
                let id: String = serde_json::from_value(
                    self.call("env.save", json!({ "id": e.id, "environment": environment })).await?,
                )
                .map_err(|e| Fail::Error(e.to_string()))?;
                id
            }
            None => {
                let environment =
                    Environment { name: name.trim().to_string(), variables: merge_variables(&[], variables, true) };
                serde_json::from_value(self.call("env.create", json!({ "environment": environment })).await?)
                    .map_err(|e| Fail::Error(e.to_string()))?
            }
        };
        if activate {
            self.call("env.setActive", json!({ "id": id })).await?;
            self.emit_agent(AgentEvent::WorkspaceChanged);
        }
        self.show(AgentTarget::Environments);
        // Whether it is the active environment now (it may have been before this call).
        let active = self.active_env_id(&ws).as_deref() == Some(id.as_str());
        Ok(Done::new(json!({ "saved": name, "active": active }), format!("{verb}d {name}"))
            .at(AgentTarget::Environments))
    }

    async fn tool_set_active_environment(&self, c: &Call<'_>) -> Outcome {
        #[derive(Deserialize)]
        struct A {
            name: Option<String>,
        }
        let A { name } = c.args()?;
        let name = name.filter(|n| !n.trim().is_empty());
        let ws = self.agent_ws()?;
        let id = match &name {
            None => None,
            Some(n) => Some(
                self.environments(&ws)?
                    .into_iter()
                    .find(|e| e.environment.name.eq_ignore_ascii_case(n.trim()) || e.id == *n)
                    .map(|e| e.id)
                    .ok_or_else(|| Fail::Invalid(format!("Environment '{n}' not found")))?,
            ),
        };
        let label = name.clone().unwrap_or_else(|| "no environment".into());
        self.gate_change(c, format!("Switch to {label}?"), Vec::new()).await?;
        self.call("env.setActive", json!({ "id": id })).await?;
        self.emit_agent(AgentEvent::WorkspaceChanged);
        Ok(Done::new(json!({ "active": name }), label))
    }

    async fn tool_import(&self, c: &Call<'_>) -> Outcome {
        #[derive(Deserialize)]
        struct A {
            text: Option<String>,
            url: Option<String>,
            file: Option<String>,
            #[serde(default)]
            folder: String,
            #[serde(rename = "baseUrl")]
            base_url: Option<String>,
        }
        let A { text, url, file, folder, base_url } = c.args()?;
        let ws = self.agent_ws()?;
        let at;
        let summary = match (text, url, file) {
            (Some(text), None, None) => {
                self.gate_change(
                    c,
                    "Import into your collection?".into(),
                    vec![format!("{} of text", human_size(text.len()))],
                )
                .await?;
                let parent = self.ensure_folder(&ws, &folder)?;
                at = parent.clone();
                self.import_text(text, parent, base_url.clone()).await?
            }
            (None, Some(url), None) => {
                let hosts = host_of(&url).into_iter().collect();
                let guard = self.gate_traffic(c, "Download a file to import", hosts).await?;
                self.gate_change(c, "Import into your collection?".into(), vec![url.clone()]).await?;
                let parent = self.ensure_folder(&ws, &folder)?;
                at = parent.clone();
                let download = super::with_guard(
                    guard,
                    self.import_url(crate::ImportUrlParams { url, parent, base_url: base_url.clone() }),
                );
                until_cancelled(c, download).await?
            }
            (None, None, Some(file)) => {
                if !std::path::Path::new(&file).is_absolute() {
                    return Err(Fail::Invalid("file must be an absolute path".into()));
                }
                let ask = Ask {
                    kind: ConfirmKind::File,
                    title: "Import a file?".into(),
                    message: format!("{} wants Zorvik to read this file and import it:", c.session.info.client),
                    items: vec![file.clone()],
                    confirm_label: "Import".into(),
                    session_option: false,
                    danger: false,
                };
                self.approve(c, ask).await?;
                let parent = self.ensure_folder(&ws, &folder)?;
                at = parent.clone();
                let params =
                    crate::ImportFileParams { path: Some(file), text: None, parent, base_url: base_url.clone() };
                until_cancelled(c, self.import_file(params)).await?
            }
            _ => return Err(Fail::Invalid("Give exactly one of text, url or file".into())),
        };
        let target = AgentTarget::Folder { path: at };
        self.show(target.clone());
        let value = serde_json::to_value(&summary).unwrap_or_default();
        Ok(Done::new(value, format!("Imported {}", plural(summary.requests as usize, "request"))).at(target))
    }

    async fn tool_update_from_openapi(&self, c: &Call<'_>) -> Outcome {
        #[derive(Deserialize)]
        struct A {
            folder: String,
            text: Option<String>,
            url: Option<String>,
            file: Option<String>,
            #[serde(default)]
            preview: bool,
        }
        let A { folder, text, url, file, preview } = c.args()?;
        let folder = folder.trim_matches('/').to_string();
        let mut guard = None;
        match (&text, &url, &file) {
            (Some(_), None, None) => {}
            (None, Some(url), None) => {
                guard =
                    self.gate_traffic(c, "Download the OpenAPI document", host_of(url).into_iter().collect()).await?;
            }
            (None, None, Some(file)) => {
                if !std::path::Path::new(file).is_absolute() {
                    return Err(Fail::Invalid("file must be an absolute path".into()));
                }
                let ask = Ask {
                    kind: ConfirmKind::File,
                    title: "Read a file?".into(),
                    message: format!(
                        "{} wants Zorvik to read this OpenAPI document to update “{folder}”:",
                        c.session.info.client
                    ),
                    items: vec![file.clone()],
                    confirm_label: "Read".into(),
                    session_option: false,
                    danger: false,
                };
                self.approve(c, ask).await?;
            }
            _ => return Err(Fail::Invalid("Give the new document's text, url or file (one of them)".into())),
        }
        let params = crate::spec_update::SpecUpdateParams { folder: folder.clone(), text, path: file, url };
        let planned = super::with_guard(guard.clone(), self.spec_update(params.clone(), false))
            .await
            .map_err(|e| self.blocked_hint(c, e))?;
        let summary = |u: &crate::spec_update::SpecUpdate| {
            format!(
                "{} added, {} changed, {} removed, {} unchanged",
                u.added.len(),
                u.changed.len() + u.restored.len(),
                u.removed.len(),
                u.unchanged
            )
        };
        if preview {
            let detail = format!("Preview: {}", summary(&planned));
            return Ok(Done::new(serde_json::to_value(&planned).unwrap_or_default(), detail));
        }
        let mut items = vec![summary(&planned)];
        items.extend(planned.added.iter().map(|a| format!("+ {}", a.operation)));
        items.extend(planned.changed.iter().map(|a| format!("~ {} ({})", a.operation, a.fields.join(", "))));
        items.extend(planned.removed.iter().map(|a| format!("− {} (kept, marked removed)", a.operation)));
        self.gate_change(c, format!("Update “{folder}” from its API spec?"), items).await?;
        let applied =
            super::with_guard(guard, self.spec_update(params, true)).await.map_err(|e| self.blocked_hint(c, e))?;
        let target = AgentTarget::Folder { path: folder };
        self.show(target.clone());
        let detail = summary(&applied);
        Ok(Done::new(serde_json::to_value(&applied).unwrap_or_default(), detail).at(target))
    }

    // ---- sending -------------------------------------------------------------------------------

    /// The request to send: saved, saved with changes, or unsaved.
    fn request_to_send(&self, ws: &Workspace, c: &Call<'_>) -> Result<(Request, Option<String>), Fail> {
        let path = c.args.get("path").and_then(Value::as_str).map(str::trim).filter(|p| !p.is_empty());
        let given = c.args.get("request").filter(|r| !r.is_null()).cloned();
        match (path, given) {
            (Some(path), None) => Ok((ws.read_request(path)?, Some(path.to_string()))),
            (Some(path), Some(r)) => {
                let saved = ws.read_request(path)?;
                let request = merge_request(&saved, r).map_err(|e| Fail::Invalid(format!("request: {e}")))?;
                Ok((request, Some(path.to_string())))
            }
            (None, Some(r)) => {
                let mut request = parse_request(r).map_err(|e| Fail::Invalid(format!("request: {e}")))?;
                if request.name.trim().is_empty() {
                    request.name = "Untitled".into();
                }
                // Inherit a folder's auth and headers through a path inside it.
                let folder =
                    c.args.get("folder").and_then(Value::as_str).map(|f| f.trim_matches('/')).unwrap_or_default();
                let path = (!folder.is_empty()).then(|| format!("{folder}/{}", request.name));
                Ok((request, path))
            }
            (None, None) => Err(Fail::Invalid("Give path (a saved request) and/or request".into())),
        }
    }

    async fn tool_send_request(&self, c: &Call<'_>) -> Outcome {
        let ws = self.agent_ws()?;
        let (request, path) = self.request_to_send(&ws, c)?;
        let saved_path = path.clone().filter(|p| ws.read_request(p).is_ok());
        let max_chars = c
            .args
            .get("maxBodyChars")
            .and_then(Value::as_u64)
            .map_or(DEFAULT_BODY_CHARS, |n| (n as usize).min(MAX_BODY_CHARS));
        if !matches!(request.kind, RequestKind::Http | RequestKind::Grpc | RequestKind::Dns | RequestKind::Sse) {
            return Err(Fail::Invalid(format!(
                "{} requests are live sessions, which agents can't use yet; ask the user to open it in Zorvik",
                method_label(&request)
            )));
        }
        let hosts = match request.kind {
            RequestKind::Dns => self.dns_hosts(&ws, &request),
            _ => self.request_hosts(&ws, &request, path.as_deref()),
        };
        let label = format!("{} {}", method_label(&request), request.url);
        let guard = self.gate_traffic(c, &format!("Send {label}"), hosts).await?;
        super::with_guard(guard, self.send_now(c, &ws, request, path, saved_path, max_chars)).await
    }

    /// Send what `send_request` approved (in the call's scope, limited to the approved hosts).
    async fn send_now(
        &self,
        c: &Call<'_>,
        ws: &Workspace,
        request: Request,
        path: Option<String>,
        saved_path: Option<String>,
        max_chars: usize,
    ) -> Outcome {
        let request_id = format!("agent-{}", uuid::Uuid::new_v4());
        let target = saved_path.clone().map(|path| AgentTarget::Request { path });
        match request.kind {
            RequestKind::Http => {
                let params = SendParams {
                    request_id: request_id.clone(),
                    request: request.clone(),
                    path: path.clone(),
                    standalone: false,
                };
                let result = self.cancellable(&c.cancel, &request_id, self.http_send(params)).await;
                let result = result.map_err(|e| self.blocked_hint(c, e))?;
                self.emit_agent(AgentEvent::Response {
                    path: saved_path,
                    request: Box::new(request.clone()),
                    kind: "http".into(),
                    result: serde_json::to_value(&result).unwrap_or_default(),
                });
                let redactor = self.redactor(ws).with_headers(&result.meta.request.headers);
                let detail =
                    format!("{} {} · {:.0} ms", result.meta.status, result.meta.status_text, result.timing.total_ms);
                let value = response_for_agent(&result, &redactor, max_chars);
                Ok(Done { value, detail, target })
            }
            RequestKind::Grpc => {
                let params = json!({ "requestId": request_id, "request": request, "path": path });
                let result = self
                    .cancellable(&c.cancel, &request_id, self.call("grpc.invoke", params))
                    .await
                    .map_err(|e| self.blocked_hint(c, e))?;
                self.emit_agent(AgentEvent::Response {
                    path: saved_path,
                    request: Box::new(request.clone()),
                    kind: "grpc".into(),
                    result: result.clone(),
                });
                let redactor = self.redactor(ws);
                let status = result["status"]["name"].as_str().unwrap_or_default().to_string();
                let value = json!({
                    "status": result["status"],
                    "messages": result["messages"].as_array().map(|m| m.iter().map(|x| redact_value(&x["json"], &redactor)).collect::<Vec<_>>()),
                    "headers": result["headers"], "trailers": result["trailers"],
                    "timeMs": result["timing"]["totalMs"],
                    "warnings": result["warnings"],
                    "unresolvedVariables": result["unresolved"],
                });
                Ok(Done { value: redact_value(&value, &redactor), detail: status, target })
            }
            RequestKind::Dns => {
                let params = json!({ "requestId": request_id, "request": request, "path": path });
                let result = self
                    .cancellable(&c.cancel, &request_id, self.call("dns.query", params))
                    .await
                    .map_err(|e| self.blocked_hint(c, e))?;
                self.emit_agent(AgentEvent::Response {
                    path: saved_path,
                    request: Box::new(request.clone()),
                    kind: "dns".into(),
                    result: result.clone(),
                });
                let detail = format!(
                    "{} · {} answers",
                    result["rcode"].as_str().unwrap_or_default(),
                    result["answers"].as_array().map_or(0, Vec::len)
                );
                Ok(Done { value: result, detail, target })
            }
            RequestKind::Sse => self.read_sse_for_agent(c, ws, &request, path.as_deref(), target).await,
            _ => Err(Fail::Invalid("Agents can send HTTP, GraphQL, gRPC, DNS and SSE requests".into())),
        }
    }

    /// An SSE request read to the end the agent asked for (`stream`), with its events.
    async fn read_sse_for_agent(
        &self,
        c: &Call<'_>,
        ws: &Workspace,
        request: &Request,
        path: Option<&str>,
        target: Option<AgentTarget>,
    ) -> Outcome {
        #[derive(Deserialize, Default)]
        #[serde(rename_all = "camelCase", default)]
        struct Stream {
            until_event: String,
            max_events: Option<u32>,
            timeout_ms: Option<u64>,
        }
        let stream: Stream = match c.args.get("stream") {
            Some(v) if !v.is_null() => {
                serde_json::from_value(v.clone()).map_err(|e| Fail::Invalid(format!("stream: {e}")))?
            }
            _ => Stream::default(),
        };
        let until = zorvik_workspace::formats::StreamUntil {
            event: stream.until_event.trim().to_string(),
            max_events: stream.max_events.unwrap_or(100),
            timeout_ms: stream.timeout_ms.unwrap_or(10_000).clamp(100, MAX_SSE_WAIT_MS),
        };
        let started = Instant::now();
        let reading = self.read_event_stream(ws, request, path, &until, &c.cancel);
        let ticking = async {
            loop {
                tokio::time::sleep(Duration::from_secs(5)).await;
                (c.progress)(started.elapsed().as_secs_f64(), Some(until.timeout_ms as f64 / 1000.0), "Reading events");
            }
        };
        let read = tokio::select! {
            r = reading => r.map_err(|e| self.blocked_hint(c, e))?,
            _ = ticking => unreachable!("ticks forever"),
        };
        let redactor = self.redactor(ws);
        let events: Vec<Value> = read
            .events
            .iter()
            .map(|e| {
                let (data, cut_now) = cut(&redactor.text(&e.data), MAX_EVENT_CHARS);
                let mut o = json!({ "event": e.event, "data": data });
                if let Some(id) = &e.id {
                    o["id"] = json!(id);
                }
                if cut_now {
                    o["dataTruncated"] = json!(true);
                }
                o
            })
            .collect();
        let mut value = json!({
            "status": read.meta.status,
            "statusText": read.meta.status_text,
            "headers": redactor.headers(&read.meta.headers),
            "ended": read.end,
            "events": events,
            "timeMs": read.duration.as_millis() as u64,
        });
        if read.dropped > 0 {
            value["eventsNotKept"] = json!(read.dropped);
        }
        if let Some(body) = &read.body {
            value["body"] = json!(redactor.text(body));
        }
        if let Some(error) = &read.error {
            value["error"] = json!(redactor.text(error));
        }
        if !read.unresolved.is_empty() {
            value["unresolvedVariables"] = json!(read.unresolved);
        }
        let detail = format!("{} · {}", read.meta.status, plural(read.events.len(), "event"));
        Ok(Done { value, detail, target })
    }

    /// Run `work` (keyed by `request_id` for `http.cancel`), cancelling it with the call.
    async fn cancellable<T>(
        &self,
        cancel: &CancellationToken,
        request_id: &str,
        work: impl std::future::Future<Output = Result<T, ApiError>>,
    ) -> Result<T, ApiError> {
        let mut work = std::pin::pin!(work);
        tokio::select! {
            r = &mut work => r,
            _ = cancel.cancelled() => {
                let _ = self.call("http.cancel", json!({ "requestId": request_id })).await;
                work.await
            }
        }
    }

    /// A request stopped by the host guard: say how to get it approved.
    fn blocked_hint(&self, c: &Call<'_>, e: ApiError) -> Fail {
        if e.network_kind == Some(zorvik_engine::ErrorKind::NotAllowed) {
            let hosts: Vec<String> = lock(&c.session.grants).blocked.iter().cloned().collect();
            return Fail::Denied(format!(
                "Zorvik stopped a request to {} (a script, redirect or token URL leads there, and the user hasn't \
                 approved it). Send it again to ask the user.",
                hosts.join(", ")
            ));
        }
        e.into()
    }

    fn redactor(&self, ws: &Workspace) -> Redactor {
        let mut vars = self.active_env_vars(ws);
        vars.extend(self.workspace_vars(ws));
        Redactor::new(&vars)
    }

    async fn tool_run_collection(&self, c: &Call<'_>) -> Outcome {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct A {
            #[serde(default)]
            folder: String,
            requests: Option<Vec<String>>,
            iterations: Option<u32>,
            #[serde(default)]
            delay_ms: u64,
            data_file: Option<String>,
            #[serde(default)]
            stop_on_failure: bool,
            wait_seconds: Option<u64>,
        }
        let a: A = c.args()?;
        let ws = self.agent_ws()?;
        let folder = a.folder.trim_matches('/').to_string();
        let (name, items) = crate::runner::collect(&ws, &folder, a.requests.as_deref()).map_err(Fail::Invalid)?;
        let mut hosts = Vec::new();
        for item in &items {
            if let Ok(r) = &item.request {
                for h in self.request_hosts(&ws, r, Some(&item.path)) {
                    if !hosts.contains(&h) {
                        hosts.push(h);
                    }
                }
            }
        }
        let guard = self.gate_traffic(c, &format!("Run “{name}” ({})", plural(items.len(), "request")), hosts).await?;
        // The run takes the call's scope (approved hosts, no files outside the workspace) along.
        let started = super::with_guard(
            guard,
            self.runner_start(crate::runner::StartParams {
                folder: folder.clone(),
                requests: a.requests,
                iterations: a.iterations,
                delay_ms: a.delay_ms,
                data_file: a.data_file,
                stop_on_failure: a.stop_on_failure,
                allow_http_errors: false,
            }),
        )
        .await?;
        self.emit_agent(AgentEvent::Run { folder: folder.clone(), run: started.clone() });
        let target = AgentTarget::Runner { folder, name: started.name.clone() };
        Ok(self.wait_run(c, &started.run_id, a.wait_seconds).await?.at(target))
    }

    async fn wait_run(&self, c: &Call<'_>, run_id: &str, wait: Option<u64>) -> Outcome {
        let deadline = Instant::now() + Duration::from_secs(wait.unwrap_or(DEFAULT_WAIT_SECS).min(MAX_WAIT_SECS));
        let started = Instant::now();
        loop {
            if let Some(report) = self.finished_run(run_id) {
                return Ok(run_outcome(run_id, &report));
            }
            if !self.run_active(run_id) && self.finished_run(run_id).is_none() {
                return Err(Fail::Invalid(format!(
                    "No collection run '{run_id}' (finished runs are kept for a while only)"
                )));
            }
            if Instant::now() >= deadline || c.cancel.is_cancelled() {
                return Ok(Done::new(
                    json!({ "status": "running", "runId": run_id, "hint": "Call get_run_status with this runId for the results." }),
                    "Still running",
                ));
            }
            (c.progress)(started.elapsed().as_secs_f64(), None, "Collection run in progress");
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_millis(250)) => {}
                _ = c.cancel.cancelled() => {}
            }
        }
    }

    async fn tool_run_status(&self, c: &Call<'_>) -> Outcome {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct A {
            run_id: String,
            wait_seconds: Option<u64>,
        }
        let A { run_id, wait_seconds } = c.args()?;
        self.wait_run(c, &run_id, wait_seconds).await
    }

    async fn tool_stop_run(&self, c: &Call<'_>) -> Outcome {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct A {
            run_id: String,
        }
        let A { run_id } = c.args()?;
        self.call("runner.stop", json!({ "runId": run_id })).await?;
        Ok(Done::new(json!({ "stopped": run_id }), "Stopped"))
    }

    async fn tool_graphql_schema(&self, c: &Call<'_>) -> Outcome {
        let ws = self.agent_ws()?;
        let (request, path) = self.request_to_send(&ws, c)?;
        let hosts = self.request_hosts(&ws, &request, path.as_deref());
        let max_chars = c.args.get("maxChars").and_then(Value::as_u64).map_or(MAX_SCHEMA_CHARS, |n| n as usize);
        let guard = self.gate_traffic(c, &format!("Fetch the GraphQL schema of {}", request.url), hosts).await?;
        let fetch = self.graphql_schema(crate::graphql::SchemaParams { request, path: path.clone(), refresh: false });
        let schema =
            until_cancelled(c, async { super::with_guard(guard, fetch).await.map_err(|e| self.blocked_hint(c, e)) })
                .await?;
        let sdl = crate::graphql::schema_sdl(&schema.data);
        let detail = format!("{} lines of SDL", sdl.lines().count());
        let (sdl, truncated) = cut(&sdl, max_chars.min(MAX_SCHEMA_CHARS));
        Ok(Done::new(json!({ "sdl": sdl, "truncated": truncated }), detail))
    }

    async fn tool_grpc_describe(&self, c: &Call<'_>) -> Outcome {
        let ws = self.agent_ws()?;
        let (request, path) = self.request_to_send(&ws, c)?;
        let hosts = if request.grpc.proto_files.is_empty() {
            self.request_hosts(&ws, &request, path.as_deref())
        } else {
            Vec::new()
        };
        let guard = self.gate_traffic(c, &format!("Ask {} for its gRPC services", request.url), hosts).await?;
        let describe = self.call("grpc.describe", json!({ "request": request, "path": path, "refresh": false }));
        let result =
            until_cancelled(c, async { super::with_guard(guard, describe).await.map_err(|e| self.blocked_hint(c, e)) })
                .await?;
        let n = result["services"].as_array().map_or(0, Vec::len);
        Ok(Done::new(result, plural(n, "service")))
    }

    // ---- load tests ------------------------------------------------------------------------------

    fn find_load_test(&self, ws: &Workspace, name: &str) -> Result<(String, LoadTest), Fail> {
        let node = ws
            .list_load_tests()?
            .into_iter()
            .find(|t| t.name.eq_ignore_ascii_case(name.trim()) || t.id == name)
            .ok_or_else(|| Fail::Invalid(format!("Load test '{name}' not found (list_load_tests)")))?;
        Ok((node.id.clone(), ws.read_load_test(&node.id)?))
    }

    fn tool_list_load_tests(&self) -> Outcome {
        let ws = self.agent_ws()?;
        let tests = ws.list_load_tests()?;
        let active = self.active_load_run();
        let n = tests.len();
        let value = json!({
            "loadTests": tests,
            "running": active.map(|a| json!({ "name": a.name, "runId": a.run_id, "plannedMs": a.planned_ms })),
        });
        Ok(Done::new(value, plural(n, "load test")))
    }

    fn tool_read_load_test(&self, c: &Call<'_>) -> Outcome {
        #[derive(Deserialize)]
        struct A {
            name: String,
        }
        let A { name } = c.args()?;
        let (id, test) = self.find_load_test(&self.agent_ws()?, &name)?;
        Ok(Done::new(json!({ "id": id, "test": test }), name).at(AgentTarget::LoadTest { id }))
    }

    async fn tool_save_load_test(&self, c: &Call<'_>) -> Outcome {
        #[derive(Deserialize)]
        struct A {
            name: String,
            test: Value,
        }
        let A { name, mut test } = c.args()?;
        if let Some(t) = test.as_object_mut() {
            t.insert("name".into(), json!(name.trim()));
        }
        let test: LoadTest = strict(test, "test", &load_test_schema()).map_err(Fail::Invalid)?;
        if let Some(t) = test.thresholds.iter().find(|t| {
            t.metric == zorvik_workspace::formats::ThresholdMetric::ErrorRate && !(0.0..=100.0).contains(&t.value)
        }) {
            return Err(Fail::Invalid(format!("errorRate thresholds are in percent, 0-100 (got {})", t.value)));
        }
        let ws = self.agent_ws()?;
        let existing = self.find_load_test(&ws, &name).ok();
        let summary =
            vec![format!("{} targets", test.targets.len()), format!("{:?}, {} s", test.model, test.duration_secs())];
        self.gate_change(
            c,
            format!("{} load test “{name}”?", if existing.is_some() { "Update" } else { "Create" }),
            summary,
        )
        .await?;
        let id: String = match existing {
            Some((id, _)) => serde_json::from_value(self.call("load.save", json!({ "id": id, "test": test })).await?),
            None => serde_json::from_value(self.call("load.create", json!({ "test": test })).await?),
        }
        .map_err(|e| Fail::Error(e.to_string()))?;
        let target = AgentTarget::LoadTest { id: id.clone() };
        self.show(target.clone());
        Ok(Done::new(json!({ "id": id, "name": name }), format!("Saved {name}")).at(target))
    }

    async fn tool_run_load_test(&self, c: &Call<'_>) -> Outcome {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct A {
            name: String,
            wait_seconds: Option<u64>,
        }
        let A { name, wait_seconds } = c.args()?;
        let ws = self.agent_ws()?;
        let (id, test) = self.find_load_test(&ws, &name)?;
        let peak = test.stages.iter().map(|s| s.target).max().unwrap_or(0);
        let unit = match test.model {
            zorvik_workspace::formats::LoadModel::VirtualUsers => "virtual users",
            zorvik_workspace::formats::LoadModel::ArrivalRate => "requests/s",
        };
        let mut hosts = Vec::new();
        for t in test.targets.iter().filter(|t| t.enabled) {
            if let Ok(r) = ws.read_request(&t.request) {
                for h in self.request_hosts(&ws, &r, Some(&t.request)) {
                    if !hosts.contains(&h) {
                        hosts.push(h);
                    }
                }
            }
        }
        let outside = hosts.iter().any(|h| !is_local(h));
        let mut items = vec![
            format!("Up to {peak} {unit} for {} s", test.duration_secs()),
            format!(
                "Targets: {}",
                test.targets.iter().filter(|t| t.enabled).map(|t| t.request.as_str()).collect::<Vec<_>>().join(", ")
            ),
            format!("Hosts: {}", hosts.join(", ")),
        ];
        if outside {
            items.push("Only load test systems you own or are allowed to test.".into());
        }
        let ask = Ask {
            kind: ConfirmKind::LoadTest,
            title: format!("Run load test “{}”?", test.name),
            message: format!("{} wants to start a load test:", c.session.info.client),
            items,
            confirm_label: "Run load test".into(),
            session_option: false,
            danger: outside,
        };
        self.approve(c, ask).await?;
        // Exactly the hosts shown (a token request too); the plan is checked against them again.
        let only: HashSet<String> = hosts.iter().cloned().collect();
        let guard = HostGuard(Arc::new(move |h: &str| only.contains(h)));
        let started = super::with_guard(Some(guard), self.load_start(id.clone(), test, true, Some(&hosts))).await?;
        let target = AgentTarget::LoadTest { id: id.clone() };
        self.show(target.clone());
        Ok(self.wait_load(c, &id, &started.run_id, wait_seconds, true).await?.at(target))
    }

    /// `seen`: this call saw the run in progress (its result is being saved).
    async fn wait_load(&self, c: &Call<'_>, test_id: &str, run_id: &str, wait: Option<u64>, mut seen: bool) -> Outcome {
        let deadline = Instant::now() + Duration::from_secs(wait.unwrap_or(DEFAULT_WAIT_SECS).min(MAX_WAIT_SECS));
        loop {
            let active = self.active_load_run().filter(|a| a.run_id == run_id);
            seen |= active.is_some();
            let Some(active) = active else {
                // Finished: the result is in the test's history (saved as the run ends).
                let tries = if seen { 40 } else { 1 };
                let mut summary = None;
                for _ in 0..tries {
                    if let Ok(s) = self.read_summary(&self.agent_ws()?, test_id, run_id) {
                        summary = Some(s);
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(250)).await;
                }
                let summary = summary.ok_or_else(|| match seen {
                    true => Fail::Error("The load test ended but its result was not saved".into()),
                    false => Fail::Invalid(format!("No run '{run_id}' of this load test (list_load_tests)")),
                })?;
                let mut value = serde_json::to_value(&summary).unwrap_or_default();
                if let Some(v) = value.as_object_mut() {
                    v.remove("points");
                    v.insert("status".into(), json!("finished"));
                    v.insert("runId".into(), json!(run_id));
                }
                let detail =
                    format!("{} · {:.0} req/s", if summary.passed { "Passed" } else { "Failed" }, summary.totals.rps);
                return Ok(Done::new(value, detail));
            };
            if Instant::now() >= deadline || c.cancel.is_cancelled() {
                let mut snapshot = serde_json::to_value(&active.snapshot).unwrap_or_default();
                if let Some(s) = snapshot.as_object_mut() {
                    s.remove("points");
                }
                return Ok(Done::new(
                    json!({ "status": "running", "runId": run_id, "snapshot": snapshot, "hint": "Call get_load_test_status for more." }),
                    "Still running",
                ));
            }
            let elapsed = active.snapshot.as_ref().map_or(0, |s| s.elapsed_ms) as f64 / 1000.0;
            (c.progress)(elapsed, Some(active.planned_ms as f64 / 1000.0), "Load test running");
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_millis(500)) => {}
                _ = c.cancel.cancelled() => {}
            }
        }
    }

    async fn tool_load_status(&self, c: &Call<'_>) -> Outcome {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct A {
            name: String,
            run_id: String,
            wait_seconds: Option<u64>,
        }
        let A { name, run_id, wait_seconds } = c.args()?;
        let (id, _) = self.find_load_test(&self.agent_ws()?, &name)?;
        self.wait_load(c, &id, &run_id, wait_seconds, false).await
    }

    async fn tool_stop_load_test(&self) -> Outcome {
        let run_id = self.active_load_run().map(|a| a.run_id);
        let Some(run_id) = run_id else { return Err(Fail::Invalid("No load test is running".into())) };
        self.call("load.stop", json!({ "runId": run_id })).await?;
        Ok(Done::new(json!({ "stopped": run_id }), "Stopping"))
    }

    // ---- app -----------------------------------------------------------------------------------

    async fn tool_open_workspace(&self, c: &Call<'_>) -> Outcome {
        #[derive(Deserialize)]
        struct A {
            path: String,
            #[serde(default)]
            create: bool,
            name: Option<String>,
        }
        let A { path, create, name } = c.args()?;
        let dir = std::path::Path::new(path.trim());
        if !dir.is_absolute() {
            return Err(Fail::Invalid("path must be an absolute folder path".into()));
        }
        let is_workspace = dir.join("zorvik.yaml").is_file();
        if !is_workspace && !create {
            return Err(Fail::Invalid(format!(
                "{path} is not a Zorvik workspace; call again with create: true to make one there"
            )));
        }
        let name = name
            .filter(|n| !n.trim().is_empty())
            .or_else(|| dir.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "Workspace".into());
        let ask = Ask {
            kind: ConfirmKind::Workspace,
            title: if is_workspace { "Open another workspace?".into() } else { "Create a workspace?".into() },
            message: format!(
                "{} wants to {} this folder in Zorvik:",
                c.session.info.client,
                if is_workspace { "open" } else { "create a workspace in" }
            ),
            items: vec![path.clone()],
            confirm_label: if is_workspace { "Open".into() } else { "Create".into() },
            session_option: false,
            danger: false,
        };
        self.approve(c, ask).await?;
        let info = if is_workspace {
            self.call("workspace.open", json!({ "path": path })).await?
        } else {
            self.call("workspace.create", json!({ "path": path, "name": name })).await?
        };
        self.emit_agent(AgentEvent::WorkspaceChanged);
        let name = info["meta"]["name"].as_str().unwrap_or_default().to_string();
        Ok(Done::new(json!({ "opened": path, "name": name }), format!("Opened {name}")))
    }

    fn tool_open_in_app(&self, c: &Call<'_>) -> Outcome {
        let s = |k: &str| c.args.get(k).and_then(Value::as_str).map(str::to_string);
        let ws = self.agent_ws()?;
        let target = if let Some(path) = s("request") {
            ws.read_request(&path)?;
            AgentTarget::Request { path }
        } else if let Some(folder) = s("runner") {
            let folder = folder.trim_matches('/').to_string();
            let name = if folder.is_empty() { ws.meta().name.clone() } else { ws.read_folder(&folder)?.name };
            AgentTarget::Runner { folder, name }
        } else if let Some(name) = s("loadTest") {
            AgentTarget::LoadTest { id: self.find_load_test(&ws, &name)?.0 }
        } else if let Some(name) = s("server") {
            let id = ws
                .list_servers()?
                .into_iter()
                .find(|x| x.name.eq_ignore_ascii_case(&name) || x.id == name)
                .map(|x| x.id)
                .ok_or_else(|| Fail::Invalid(format!("Server '{name}' not found")))?;
            AgentTarget::Server { id }
        } else if c.args.get("environments").and_then(Value::as_bool) == Some(true) {
            AgentTarget::Environments
        } else {
            return Err(Fail::Invalid("Say what to open: request, runner, loadTest, server or environments".into()));
        };
        self.emit_agent(AgentEvent::Show { target: target.clone(), explicit: true });
        Ok(Done::new(json!({ "shown": true }), "Shown").at(target))
    }
}

// ---- helpers ---------------------------------------------------------------------------------

/// Build a tool's future in a call of its own (see `run_tool`).
fn in_own_frame<'a>(build: impl FnOnce() -> BoxedOutcome<'a>) -> BoxedOutcome<'a> {
    build()
}

/// `work`, unless the agent cancels the call first.
async fn until_cancelled<T, E: Into<Fail>>(
    c: &Call<'_>,
    work: impl std::future::Future<Output = Result<T, E>>,
) -> Result<T, Fail> {
    tokio::select! {
        r = work => r.map_err(Into::into),
        _ = c.cancel.cancelled() => Err(Fail::Error("Cancelled.".into())),
    }
}

/// The first `max` characters of `text`, and whether that cut anything.
fn cut(text: &str, max: usize) -> (String, bool) {
    match text.char_indices().nth(max) {
        Some((at, _)) => (text[..at].to_string(), true),
        None => (text.to_string(), false),
    }
}

fn walk(nodes: &[TreeNode], f: &mut impl FnMut(&TreeNode)) {
    for n in nodes {
        f(n);
        walk(&n.children, f);
    }
}

fn find_node<'a>(nodes: &'a [TreeNode], path: &str) -> Option<&'a TreeNode> {
    for n in nodes {
        if n.path == path {
            return Some(n);
        }
        if path.starts_with(&format!("{}/", n.path))
            && let Some(found) = find_node(&n.children, path)
        {
            return Some(found);
        }
    }
    None
}

fn join(folder: &str, name: &str) -> String {
    let folder = folder.trim_matches('/');
    if folder.is_empty() { name.to_string() } else { format!("{folder}/{name}") }
}

fn parent_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(p, _)| p)
}

fn host_of(url: &str) -> Option<String> {
    // gRPC and other schemes as they are; web URLs may leave out `http://`.
    let url = url::Url::parse(url.trim())
        .ok()
        .filter(|u| u.host_str().is_some())
        .or_else(|| zorvik_engine::http::normalize_url(url).ok())?;
    let host = url.host_str()?.trim_start_matches('[').trim_end_matches(']').to_ascii_lowercase();
    (!host.is_empty()).then_some(host)
}

fn method_label(r: &Request) -> String {
    match r.kind {
        RequestKind::Http if r.body.body_type == zorvik_workspace::formats::BodyType::Graphql => "GraphQL".into(),
        RequestKind::Http => r.method.to_uppercase(),
        other => serde_json::to_value(other).ok().and_then(|v| v.as_str().map(str::to_uppercase)).unwrap_or_default(),
    }
}

fn human_size(bytes: usize) -> String {
    if bytes < 1024 { format!("{bytes} bytes") } else { format!("{} KB", bytes / 1024) }
}

/// Long lists in a dialog: the first ones and a count of the rest.
fn cap_items(mut items: Vec<String>) -> Vec<String> {
    const MAX: usize = 30;
    if items.len() > MAX {
        let more = items.len() - MAX;
        items.truncate(MAX);
        items.push(format!("… and {more} more"));
    }
    items
}

/// `value` as a `T`, refusing fields `T` doesn't have (serde would drop them silently, so a
/// misspelled field would look saved). `what` names the argument in errors; the names in
/// `schema` (the tool's input schema) are offered as "did you mean".
fn strict<T: DeserializeOwned>(value: Value, what: &str, schema: &Value) -> Result<T, String> {
    let mut unknown: Vec<String> = Vec::new();
    let parsed: T =
        serde_ignored::deserialize(value, |path| unknown.push(path.to_string())).map_err(|e| format!("{what}: {e}"))?;
    if unknown.is_empty() {
        return Ok(parsed);
    }
    let mut known = BTreeSet::new();
    schema_fields(schema, &mut known);
    let lines: Vec<String> = unknown
        .iter()
        .map(|path| {
            let field = path.rsplit('.').next().unwrap_or(path);
            let near = known
                .iter()
                .map(|k| (strsim::jaro_winkler(&field.to_ascii_lowercase(), &k.to_ascii_lowercase()), k))
                .filter(|(score, _)| *score >= 0.85)
                .max_by(|a, b| a.0.total_cmp(&b.0));
            match near {
                Some((_, k)) => format!("`{path}` (did you mean `{k}`?)"),
                None => format!("`{path}`"),
            }
        })
        .collect();
    Err(format!(
        "{what}: unknown field{} {}. Nothing was saved; the tool's input schema lists every field.",
        if lines.len() == 1 { "" } else { "s" },
        lines.join(", ")
    ))
}

/// Every property name in a JSON schema, at any depth.
fn schema_fields(schema: &Value, out: &mut BTreeSet<String>) {
    match schema {
        Value::Object(map) => {
            if let Some(Value::Object(props)) = map.get("properties") {
                for (name, sub) in props {
                    out.insert(name.clone());
                    schema_fields(sub, out);
                }
            }
            for (key, sub) in map {
                if key != "properties" {
                    schema_fields(sub, out);
                }
            }
        }
        Value::Array(items) => items.iter().for_each(|v| schema_fields(v, out)),
        _ => {}
    }
}

/// JSON Merge Patch (RFC 7396): objects merge field by field, anything else (arrays
/// included) replaces, and `null` removes the field (it goes back to its default).
fn merge_patch(target: &mut Value, patch: Value) {
    match patch {
        Value::Object(patch) => {
            if !target.is_object() {
                *target = json!({});
            }
            let map = target.as_object_mut().expect("object");
            for (key, value) in patch {
                if value.is_null() {
                    map.remove(&key);
                } else {
                    merge_patch(map.entry(key).or_insert(Value::Null), value);
                }
            }
        }
        other => *target = other,
    }
}

/// A request from an agent, forgiving common shapes (headers as an object, a body
/// given as text or JSON, auth fields in camelCase like the UI's) but not unknown fields.
fn parse_request(value: Value) -> Result<Request, String> {
    let (mut value, query) = normalize_request(value)?;
    if let Some(map) = value.as_object_mut() {
        map.entry("name").or_insert(json!(""));
    }
    let mut request: Request = strict(value, "request", &request_schema())?;
    if let Some(rows) = query {
        apply_query(&mut request, rows);
    }
    Ok(request)
}

/// `patch` (the fields an agent gave) laid over a saved request: what it left out stays.
fn merge_request(saved: &Request, patch: Value) -> Result<Request, String> {
    let (patch, query) = normalize_request(patch)?;
    let Value::Object(patch) = patch else { return Err("expected an object".into()) };
    // Check the fields given (with the saved name, so a patch without one parses).
    let mut check = patch.clone();
    check.entry("name").or_insert(json!(saved.name));
    strict::<Request>(Value::Object(check), "request", &request_schema())?;
    let mut merged = serde_json::to_value(saved).map_err(|e| e.to_string())?;
    if let Some(map) = merged.as_object_mut() {
        map.extend(patch);
    }
    let mut request: Request = serde_json::from_value(merged).map_err(|e| e.to_string())?;
    if let Some(rows) = query {
        apply_query(&mut request, rows);
    }
    Ok(request)
}

/// Query rows from an agent into the request: enabled ones become the URL's query string,
/// disabled ones its switched-off params, and descriptions go where each kind keeps them.
fn apply_query(request: &mut Request, rows: Vec<KeyValue>) {
    let (base, hash) = {
        let (rest, hash) = request.url.split_once('#').map_or((request.url.as_str(), ""), |(r, h)| (r, h));
        (rest.split_once('?').map_or(rest, |(b, _)| b).to_string(), hash.to_string())
    };
    let on: Vec<&KeyValue> = rows.iter().filter(|r| r.enabled && !r.key.is_empty()).collect();
    let query: Vec<String> =
        on.iter().map(|r| format!("{}={}", encode_query(&r.key), encode_query(&r.value))).collect();
    let mut url = base;
    if !query.is_empty() {
        url.push('?');
        url.push_str(&query.join("&"));
    }
    if !hash.is_empty() {
        url.push('#');
        url.push_str(&hash);
    }
    request.url = url;
    request.disabled_params = rows.iter().filter(|r| !r.enabled && !r.key.is_empty()).cloned().collect();
    request.param_descriptions = on
        .iter()
        .filter(|r| !r.description.is_empty())
        .map(|r| KeyValue {
            key: r.key.clone(),
            value: String::new(),
            enabled: true,
            description: r.description.clone(),
        })
        .collect();
}

/// Percent-encodes a query component, leaving `{{variable}}` placeholders as they are.
fn encode_query(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while !rest.is_empty() {
        if let Some(start) = rest.find("{{")
            && let Some(len) = rest[start..].find("}}")
        {
            out.push_str(&encode_plain(&rest[..start]));
            out.push_str(&rest[start..start + len + 2]);
            rest = &rest[start + len + 2..];
        } else {
            out.push_str(&encode_plain(rest));
            break;
        }
    }
    out
}

fn encode_plain(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// The request shapes agents send, as the model has them, and the `query` rows (applied
/// once the URL is known).
fn normalize_request(mut value: Value) -> Result<(Value, Option<Vec<KeyValue>>), String> {
    let Some(map) = value.as_object_mut() else { return Err("expected an object".into()) };
    let query = match map.remove("query") {
        None | Some(Value::Null) => None,
        Some(rows) => Some(serde_json::from_value::<Vec<KeyValue>>(rows).map_err(|e| format!("query: {e}"))?),
    };
    if let Some(h) = map.get("headers").cloned() {
        map.insert("headers".into(), serde_json::to_value(parse_headers(h)?).unwrap_or_default());
    }
    if let Some(body) = map.get("body").cloned() {
        let body = match body {
            Value::String(text) => {
                let kind = if serde_json::from_str::<Value>(&text).is_ok() { "json" } else { "text" };
                json!({ "type": kind, "text": text })
            }
            Value::Object(ref o) if !o.contains_key("type") => {
                json!({ "type": "json", "text": serde_json::to_string_pretty(&body).unwrap_or_default() })
            }
            Value::Object(mut o) => {
                // `text` given as JSON instead of JSON text.
                if let Some(t) = o.get("text").filter(|t| !t.is_string() && !t.is_null()).cloned() {
                    o.insert("text".into(), json!(serde_json::to_string_pretty(&t).unwrap_or_default()));
                }
                if let Some(g) = o.get_mut("graphql").and_then(Value::as_object_mut)
                    && let Some(v) = g.get("variables").filter(|v| !v.is_string() && !v.is_null()).cloned()
                {
                    g.insert("variables".into(), json!(serde_json::to_string_pretty(&v).unwrap_or_default()));
                }
                Value::Object(o)
            }
            other => other,
        };
        map.insert("body".into(), body);
    }
    if let Some(auth) = map.get("auth").cloned() {
        map.insert("auth".into(), serde_json::to_value(parse_auth(auth)?).unwrap_or_default());
    }
    Ok((value, query))
}

fn parse_headers(value: Value) -> Result<Vec<KeyValue>, String> {
    match value {
        Value::Object(map) => Ok(map
            .into_iter()
            .map(|(k, v)| KeyValue::new(k, v.as_str().map_or_else(|| v.to_string(), str::to_string)))
            .collect()),
        other => serde_json::from_value(other).map_err(|e| format!("headers: {e}")),
    }
}

fn parse_auth(value: Value) -> Result<Auth, String> {
    serde_json::from_value(value).map_err(|e| format!("auth: {e}"))
}

/// Variables from an agent merged into `existing` by key. A secret sent back as
/// `••••••` (as agents see it) keeps its value.
fn merge_variables(existing: &[Variable], incoming: Vec<Variable>, replace: bool) -> Vec<Variable> {
    let mut out: Vec<Variable> = if replace { Vec::new() } else { existing.to_vec() };
    for mut v in incoming {
        let old = existing.iter().find(|e| e.key == v.key);
        if v.value == MASK
            && let Some(old) = old
        {
            v.value = old.value.clone();
            v.secret = v.secret || old.secret;
        }
        match out.iter_mut().find(|e| e.key == v.key) {
            Some(slot) => *slot = v,
            None => out.push(v),
        }
    }
    out
}

fn redact_value(value: &Value, r: &Redactor) -> Value {
    match value {
        Value::String(s) => Value::String(r.text(s)),
        Value::Array(a) => Value::Array(a.iter().map(|v| redact_value(v, r)).collect()),
        Value::Object(o) => Value::Object(o.iter().map(|(k, v)| (k.clone(), redact_value(v, r))).collect()),
        other => other.clone(),
    }
}

/// A response as agents get it: status, headers, the body (cut), tests, with credentials hidden.
fn response_for_agent(result: &SendResult, r: &Redactor, max_chars: usize) -> Value {
    let body = &result.body;
    let text = body.pretty.as_ref().or(body.text.as_ref()).map(|t| r.text(t));
    let (text, cut) = match text {
        Some(t) if t.chars().count() > max_chars => (Some(t.chars().take(max_chars).collect::<String>()), true),
        other => (other, false),
    };
    let mut out = json!({
        "status": result.meta.status,
        "statusText": result.meta.status_text,
        "timeMs": (result.timing.total_ms * 10.0).round() / 10.0,
        "url": r.text(&result.meta.url),
        "httpVersion": result.meta.http_version,
        "headers": r.headers(&result.meta.headers),
        "body": text,
        "bodySize": body.size,
        "contentType": body.content_type,
    });
    if body.kind != "text" {
        out["bodyKind"] = json!(body.kind);
    }
    if cut || body.display_truncated || body.download_truncated {
        out["bodyTruncated"] = json!(true);
    }
    if !result.meta.redirects.is_empty() {
        out["redirects"] = json!(
            result.meta.redirects.iter().map(|h| format!("{} {}", h.status, r.text(&h.location))).collect::<Vec<_>>()
        );
    }
    out["request"] = json!({
        "method": result.meta.request.method,
        "url": r.text(&result.meta.request.url),
        "headers": r.headers(&result.meta.request.headers),
    });
    if !result.unresolved.is_empty() {
        out["unresolvedVariables"] = json!(result.unresolved);
    }
    if let Some(scripts) = &result.scripts {
        out["tests"] = redact_value(&json!(scripts.tests), r);
        if !scripts.console.is_empty() {
            out["console"] = json!(scripts.console.iter().map(|c| r.text(&c.message)).collect::<Vec<_>>());
        }
        if !scripts.errors.is_empty() {
            out["scriptErrors"] = redact_value(&json!(scripts.errors), r);
        }
    }
    out
}

fn run_outcome(run_id: &str, report: &RunReport) -> Done {
    let s = &report.summary;
    let failures: Vec<Value> = report
        .results
        .iter()
        .filter(|r| !r.passed)
        .take(MAX_FAILURES)
        .map(|r| {
            json!({
                "path": r.path,
                "iteration": r.iteration + 1,
                "status": r.status,
                "error": r.error,
                "failedTests": r.tests.iter().filter(|t| !t.passed && !t.skipped).map(|t| json!({ "name": t.name, "error": t.error })).collect::<Vec<_>>(),
                "scriptErrors": r.script_errors,
                "unresolvedVariables": r.unresolved,
            })
        })
        .collect();
    let detail = format!(
        "{} · {} failed · tests {}/{}",
        plural(s.requests as usize, "request"),
        s.failed,
        s.tests_passed,
        s.tests_passed + s.tests_failed
    );
    Done::new(json!({ "status": "finished", "runId": run_id, "summary": s, "failures": failures }), detail)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn definitions_are_valid() {
        let defs = tool_definitions();
        let mut names = HashSet::new();
        for d in &defs {
            assert!(names.insert(d["name"].as_str().unwrap().to_string()), "duplicate {}", d["name"]);
            assert_eq!(d["inputSchema"]["type"], "object", "{}", d["name"]);
            assert!(d["description"].as_str().unwrap().len() > 20);
        }
        assert!(names.contains("send_request") && names.contains("delete_items"));
    }

    #[test]
    fn forgiving_request_shapes() {
        let r = parse_request(json!({
            "name": "Create", "method": "POST", "url": "{{baseUrl}}/orders",
            "headers": { "X-Trace": "1" },
            "body": { "type": "json", "text": { "qty": 2 } },
            "auth": { "type": "bearer", "token": "{{token}}" },
        }))
        .unwrap();
        assert_eq!(r.headers[0].key, "X-Trace");
        assert!(r.body.text.contains("\"qty\": 2"));
        assert!(matches!(r.auth, Auth::Bearer { .. }));
        let r = parse_request(json!({ "url": "x", "body": { "a": 1 } })).unwrap();
        assert_eq!(r.body.body_type, zorvik_workspace::formats::BodyType::Json);
        let r = parse_request(json!({ "url": "x", "body": "plain" })).unwrap();
        assert_eq!(r.body.body_type, zorvik_workspace::formats::BodyType::Text);
        assert!(parse_request(json!("nope")).is_err());
    }

    #[test]
    fn partial_updates_keep_what_was_left_out() {
        let saved = parse_request(json!({
            "name": "Create", "method": "POST", "url": "{{baseUrl}}/orders", "docs": "handler: orders.ts",
            "scripts": { "postResponse": "pm.test('ok', () => {})" },
        }))
        .unwrap();
        let merged = merge_request(&saved, json!({ "headers": { "X-Trace": "1" } })).unwrap();
        assert_eq!(
            (merged.method.as_str(), merged.url.as_str(), merged.docs.as_str()),
            ("POST", "{{baseUrl}}/orders", "handler: orders.ts")
        );
        assert!(!merged.scripts.post_response.is_empty());
        assert_eq!(merged.headers[0].key, "X-Trace");
        let renamed = merge_request(&saved, json!({ "url": "{{baseUrl}}/v2/orders" })).unwrap();
        assert_eq!((renamed.name.as_str(), renamed.url.as_str()), ("Create", "{{baseUrl}}/v2/orders"));
    }

    #[test]
    fn masked_secrets_keep_their_value() {
        let old = vec![Variable { key: "token".into(), value: "real".into(), enabled: true, secret: true }];
        let merged = merge_variables(
            &old,
            vec![
                Variable { key: "token".into(), value: MASK.into(), enabled: true, secret: false },
                Variable { key: "base".into(), value: "http://x".into(), enabled: true, secret: false },
            ],
            false,
        );
        assert_eq!(merged[0].value, "real");
        assert!(merged[0].secret);
        assert_eq!(merged[1].key, "base");
    }

    #[test]
    fn hosts_and_labels() {
        assert_eq!(host_of("https://API.example.com:8443/x").as_deref(), Some("api.example.com"));
        assert_eq!(host_of("localhost:3000/a").as_deref(), Some("localhost"));
        assert_eq!(host_of("grpc://[::1]:50051").as_deref(), Some("::1"));
        assert_eq!(cap_items((0..40).map(|i| i.to_string()).collect()).last().unwrap(), "… and 10 more");
    }
}

//! Tools for mock APIs and servers: list, read, create or change, build a mock from a
//! folder or an OpenAPI document, start and stop, and read what a running server received.

use serde::Deserialize;
use serde_json::{Value, json};
use zorvik_servers::TrafficEntry;
use zorvik_workspace::Workspace;
use zorvik_workspace::formats::{Server, ServerKind};

use super::{Call, Def, Done, Fail, Outcome, cut, host_of, human_size, merge_patch, obj, plural, strict, string};
use crate::Api;
use crate::agents::{AgentTarget, Ask, ConfirmKind};

/// Traffic entries returned by default, and at most.
const DEFAULT_TRAFFIC: usize = 50;
const MAX_TRAFFIC: usize = 500;
/// Body and payload characters kept per traffic entry.
const TRAFFIC_BODY_CHARS: usize = 4_000;

/// Placeholders mock bodies, headers, greetings and SSE events can use.
const PLACEHOLDERS: &str = "Templates in status, headers and body: {{request.params.id}} (a :id path segment; \
    {{request.params.*}} for a trailing *), {{request.query.name}}, {{request.headers.name}}, {{request.body}}, \
    {{request.method}}, {{request.path}}, {{request.url}}; dynamic values {{$uuid}} (= {{$randomUUID}}, {{$guid}}), \
    {{$isoTimestamp}}, {{$timestamp}} (seconds), {{$timestampMs}}, {{$randomInt}} (0-1000), {{$randomBoolean}}, \
    {{$randomAlphaNumeric}}, {{$randomEmail}}; and the active environment's and the collection's variables (secret \
    ones stay literal). Text a client sends is inserted as-is, never expanded.";

fn key_values(description: &str) -> Value {
    json!({
        "type": "array",
        "description": description,
        "items": obj(json!({ "key": { "type": "string" }, "value": { "type": "string" }, "enabled": { "type": "boolean" } }), &["key"]),
    })
}

fn reply_rules() -> Value {
    json!({
        "type": "array",
        "description": "Replies for mode rules: the first enabled rule whose pattern matches answers.",
        "items": obj(json!({
            "match": { "type": "string", "enum": ["any", "contains", "exact", "regex"], "description": "Default contains." },
            "pattern": { "type": "string" },
            "reply": string("Reply text (templates allowed, {{message}} is the incoming message)."),
            "delayMs": { "type": "integer" },
            "enabled": { "type": "boolean" },
        }), &[]),
    })
}

fn mode() -> Value {
    json!({ "type": "string", "enum": ["echo", "rules", "manual", "discard"], "description": "echo (default): send every message back; rules: reply with the first matching rule; manual: only what the user sends; discard: read and drop." })
}

/// A server as agents give it: every field, all optional (a change sends only what changes).
pub(super) fn server_schema() -> Value {
    obj(
        json!({
            "name": string("Display name (also its file name). Renames the server when it differs from the one being saved."),
            "kind": { "type": "string", "enum": ["http", "websocket", "sse", "tcp", "udp", "dns", "tcpProxy"], "description": "http = mock API (default). Only the section of the kind is used." },
            "host": string("127.0.0.1 (default, this computer only) or 0.0.0.0 (other devices too)."),
            "port": { "type": "integer", "description": "Port to listen on; 0 = any free port when it starts." },
            "tls": obj(json!({
                "enabled": { "type": "boolean" },
                "certPath": string("PEM certificate chain inside the workspace (empty: a self-signed one for localhost)."),
                "keyPath": string("PEM private key."),
            }), &[]),
            "autoStart": { "type": "boolean", "description": "Start with the workspace (only after the user started it once)." },
            "docs": string("Markdown notes."),
            "http": obj(json!({
                "routes": {
                    "type": "array",
                    "description": format!("Routes, tried in order: the first enabled route that matches answers. {PLACEHOLDERS}"),
                    "items": obj(json!({
                        "name": string("Shown in the traffic log instead of the path."),
                        "method": string("GET, POST, … or * for any method (default *). HEAD falls back to GET routes."),
                        "path": string("Starts with /. :name matches one segment (/users/:id), a trailing * matches the rest (/files/*)."),
                        "status": { "type": "integer", "description": "HTTP status (default 200)." },
                        "headers": key_values("Response headers, e.g. [{key: \"Content-Type\", value: \"application/json\"}]."),
                        "body": string("Response body text (templates allowed). JSON goes in as text."),
                        "delayMs": { "type": "integer", "description": "Wait before answering." },
                        "matchQuery": key_values("Only match requests with these query parameters (empty value = any value)."),
                        "matchHeaders": key_values("Only match requests with these headers (empty value = any value)."),
                        "matchBody": string("Only match requests whose body contains this text."),
                        "fault": { "type": "string", "enum": ["none", "error", "reset", "hang"], "description": "Instead of the answer: error = 500, reset = close the connection, hang = never answer." },
                        "faultPercent": { "type": "integer", "description": "How often the fault happens, 0-100 (default 100)." },
                        "enabled": { "type": "boolean", "description": "Default true." },
                    }), &[]),
                },
                "fallback": { "type": "string", "enum": ["notFound", "proxy"], "description": "Requests no route matches: notFound (default) answers 404 with the list of routes; proxy forwards them to proxyUrl." },
                "proxyUrl": string("Base URL of the real backend for fallback proxy, e.g. http://localhost:8080."),
                "cors": { "type": "boolean", "description": "Answer CORS preflights and add Access-Control-Allow-* headers, so a web app on another origin can call the mock." },
            }), &[]),
            "websocket": obj(json!({ "mode": mode(), "greeting": string("Sent to each client right after it connects."), "rules": reply_rules() }), &[]),
            "sse": obj(json!({
                "events": {
                    "type": "array",
                    "description": "Sent in order to each client after it connects (templates allowed).",
                    "items": obj(json!({ "event": string("Event name (empty: message)."), "data": { "type": "string" }, "id": { "type": "string" } }), &[]),
                },
                "intervalMs": { "type": "integer", "description": "Pause between events (0: all at once)." },
                "repeat": { "type": "boolean", "description": "Start over after the last event." },
            }), &[]),
            "socket": obj(json!({
                "mode": mode(),
                "greeting": string("TCP: sent to each client right after it connects."),
                "rules": reply_rules(),
                "encoding": { "type": "string", "enum": ["text", "hex"], "description": "Of greeting, patterns and replies." },
                "framing": { "type": "string", "enum": ["raw", "line", "lengthPrefixed"], "description": "TCP: how incoming bytes split into messages." },
                "lengthBytes": { "type": "integer", "enum": [1, 2, 4], "description": "lengthPrefixed: size of the big-endian length (default 2)." },
                "lineEnding": { "type": "string", "enum": ["none", "lf", "crLf"], "description": "Appended to text replies." },
            }), &[]),
            "dns": obj(json!({
                "records": {
                    "type": "array",
                    "items": obj(json!({
                        "name": string("api.example.test or *.example.test"),
                        "type": string("A, AAAA, CNAME, TXT, MX, NS, PTR, SRV or CAA."),
                        "value": string("Record data as in a zone file (MX: \"10 mail.example.test\")."),
                        "ttl": { "type": "integer", "description": "Default 60." },
                        "enabled": { "type": "boolean" },
                    }), &["name", "type", "value"]),
                },
                "upstream": string("Names without a record: empty = no such name, system = this computer's resolver, or a server such as 1.1.1.1."),
            }), &[]),
            "proxy": obj(json!({
                "target": string("host:port every client connection is relayed to."),
                "upstreamTls": { "type": "boolean", "description": "Connect to the target with TLS." },
            }), &[]),
        }),
        &[],
    )
}

pub(super) fn defs() -> Vec<Def> {
    let name = || string("Server name (as list_servers shows it) or id.");
    vec![
        Def {
            name: "list_servers",
            title: "List servers",
            description: "Saved mock APIs and servers (HTTP, WebSocket, SSE, TCP, UDP, DNS, relay): kind, address, route count, and which are running.",
            schema: obj(json!({}), &[]),
            read_only: true,
            destructive: false,
            open_world: false,
        },
        Def {
            name: "read_server",
            title: "Read a server",
            description: "A saved server's full definition (routes, bodies, rules, records…), in the shape save_server takes.",
            schema: obj(json!({ "name": name() }), &["name"]),
            read_only: true,
            destructive: false,
            open_world: false,
        },
        Def {
            name: "save_server",
            title: "Save a server",
            description: "Create a mock API or server, or change one. An existing server (by name) gets only the fields given, merged like a JSON Merge Patch: objects merge, arrays (routes, rules, records) replace the whole list, null resets a field. replace: true saves exactly what is given. A running server takes the change at once (a new address, port, TLS or kind needs a restart). Unknown or misspelled fields are refused.",
            schema: obj(
                json!({
                    "name": string("The server to create or change (name or id)."),
                    "server": server_schema(),
                    "replace": { "type": "boolean", "description": "Replace the whole server with `server` (default: merge)." },
                }),
                &["name"],
            ),
            read_only: false,
            destructive: false,
            open_world: false,
        },
        Def {
            name: "create_mock",
            title: "Create a mock API",
            description: "Build a mock API from the collection (every HTTP request of a folder becomes a route answering with its saved example response or a 200) or from an OpenAPI 3 / Swagger 2 document (each operation answers with its first 2xx example). Give folder, or one of openapiText / openapiUrl / openapiFile. Returns the new server; start it with start_server.",
            schema: obj(
                json!({
                    "name": string("Name of the new mock (default \"Mock API\")."),
                    "folder": string("Folder path to mock (\"\" = the whole collection)."),
                    "openapiText": string("The OpenAPI document itself (YAML or JSON)."),
                    "openapiUrl": string("URL of the OpenAPI document."),
                    "openapiFile": string("Absolute path of the OpenAPI file (the user confirms reading it)."),
                    "port": { "type": "integer", "description": "Port to save (default: the next free one from 4000)." },
                }),
                &[],
            ),
            read_only: false,
            destructive: false,
            open_world: true,
        },
        Def {
            name: "start_server",
            title: "Start a server",
            description: "Start a saved server or mock API (the user confirms in Zorvik). port overrides the saved port for this run only; port 0 picks any free port. When the port is taken, the error says which program holds it.",
            schema: obj(
                json!({ "name": name(), "port": { "type": "integer", "description": "Port for this run (0 = any free port)." } }),
                &["name"],
            ),
            read_only: false,
            destructive: false,
            open_world: false,
        },
        Def {
            name: "stop_server",
            title: "Stop a server",
            description: "Stop a running server.",
            schema: obj(json!({ "name": name() }), &["name"]),
            read_only: false,
            destructive: false,
            open_world: false,
        },
        Def {
            name: "get_server_traffic",
            title: "Server traffic",
            description: "What a running server received and answered, newest last. HTTP mocks: method, path, request headers and body, the route that matched (null = no route, the fallback answered), status, response headers and body, time. Other kinds: connections and messages. Pass the lastId from the previous call as sinceId to get only new entries. Kept while the server runs (the last 5000 entries).",
            schema: obj(
                json!({
                    "name": name(),
                    "limit": { "type": "integer", "description": "Entries to return (default 50, at most 500): the newest ones." },
                    "sinceId": { "type": "integer", "description": "Only entries after this id." },
                }),
                &["name"],
            ),
            read_only: true,
            destructive: false,
            open_world: false,
        },
    ]
}

/// A saved server's id, by name or id.
fn find_server(ws: &Workspace, name: &str) -> Result<Option<String>, Fail> {
    let name = name.trim();
    Ok(ws.list_servers()?.into_iter().find(|s| s.name.trim().eq_ignore_ascii_case(name) || s.id == name).map(|s| s.id))
}

fn not_found(name: &str) -> Fail {
    Fail::Invalid(format!("Server '{name}' not found (list_servers)"))
}

/// Problems an agent can fix, found before anything is saved.
fn check_server(server: &Server) -> Result<(), String> {
    if server.name.trim().is_empty() {
        return Err("server.name is empty".into());
    }
    for (i, route) in server.http.routes.iter().enumerate() {
        let at = format!("http.routes[{i}]");
        if !route.path.starts_with('/') {
            return Err(format!("{at}.path must start with / (got \"{}\")", route.path));
        }
        let method = route.method.trim();
        if method != "*" && (method.is_empty() || !method.bytes().all(|b| b.is_ascii_alphabetic())) {
            return Err(format!("{at}.method must be an HTTP method such as GET, or * (got \"{method}\")"));
        }
        if !(100..=599).contains(&route.status) {
            return Err(format!("{at}.status must be 100-599 (got {})", route.status));
        }
        if route.fault_percent > 100 {
            return Err(format!("{at}.faultPercent must be 0-100"));
        }
    }
    if server.kind == ServerKind::TcpProxy && server.proxy.target.trim().is_empty() {
        return Err("proxy.target is empty (host:port to relay to)".into());
    }
    Ok(())
}

/// One traffic entry as agents get it, with long payloads cut.
fn traffic_for_agent(entry: &TrafficEntry, redact: &impl Fn(&str) -> String) -> Value {
    let text = |s: &str| {
        let (text, cut) = cut(&redact(s), TRAFFIC_BODY_CHARS);
        if cut { format!("{text}… (cut)") } else { text }
    };
    let headers = |list: &[zorvik_engine::Header]| -> Value {
        list.iter().map(|h| json!({ "name": h.name, "value": redact(&h.value) })).collect()
    };
    let time = time::OffsetDateTime::from_unix_timestamp_nanos((entry.timestamp * 1_000_000.0) as i128)
        .ok()
        .and_then(|t| t.format(&time::format_description::well_known::Rfc3339).ok());
    let mut out = json!({ "id": entry.id, "time": time, "kind": entry.kind, "summary": redact(&entry.summary) });
    if let Some(peer) = &entry.peer {
        out["peer"] = json!(peer);
    }
    if let Some(conn) = entry.conn {
        out["connection"] = json!(conn);
    }
    if let Some(direction) = entry.direction {
        out["direction"] = json!(direction);
    }
    match &entry.http {
        Some(http) => {
            out["http"] = json!({
                "method": http.method,
                "path": redact(&http.path),
                "route": http.route,
                "note": http.note,
                "status": http.status,
                "durationMs": (http.duration_ms * 10.0).round() / 10.0,
                "requestHeaders": headers(&http.request_headers),
                "requestBody": text(&http.request_body),
                "responseHeaders": headers(&http.response_headers),
                "responseBody": text(&http.response_body),
            });
        }
        None => {
            if let Some(t) = &entry.text {
                out["text"] = json!(text(t));
            } else if entry.base64.is_some() {
                out["binary"] = json!(format!("{} of binary data", human_size(entry.size as usize)));
            }
        }
    }
    if entry.truncated {
        out["truncated"] = json!(true);
    }
    out
}

impl Api {
    pub(super) fn tool_list_servers(&self) -> Outcome {
        let ws = self.agent_ws()?;
        let root = ws.root().to_string_lossy().into_owned();
        let running = self.inner.servers.running();
        let mut list = Vec::new();
        for node in ws.list_servers()? {
            let run = running.iter().find(|r| r.server_id == node.id && r.workspace_path == root);
            let mut item = json!({
                "name": node.name,
                "kind": node.kind,
                "port": node.port,
                "running": run.map(|r| json!({ "url": r.url, "runId": r.run_id })),
            });
            if let Ok(server) = ws.read_server(&node.id) {
                item["host"] = json!(server.host);
                if server.kind == ServerKind::Http {
                    item["routes"] = json!(server.http.routes.len());
                }
            }
            if let Some(e) = &node.error {
                item["error"] = json!(e);
            }
            list.push(item);
        }
        let n = list.len();
        Ok(Done::new(json!({ "servers": list }), plural(n, "server")))
    }

    pub(super) fn tool_read_server(&self, c: &Call<'_>) -> Outcome {
        #[derive(Deserialize)]
        struct A {
            name: String,
        }
        let A { name } = c.args()?;
        let ws = self.agent_ws()?;
        let id = find_server(&ws, &name)?.ok_or_else(|| not_found(&name))?;
        let server = ws.read_server(&id)?;
        let root = ws.root().to_string_lossy().into_owned();
        let run = self.inner.servers.running().into_iter().find(|r| r.server_id == id && r.workspace_path == root);
        let value = json!({
            "id": id,
            "server": server,
            "running": run.map(|r| json!({ "url": r.url, "runId": r.run_id })),
        });
        Ok(Done::new(value, server.name.clone()).at(AgentTarget::Server { id }))
    }

    pub(super) async fn tool_save_server(&self, c: &Call<'_>) -> Outcome {
        #[derive(Deserialize)]
        struct A {
            name: String,
            #[serde(default)]
            server: Value,
            #[serde(default)]
            replace: bool,
        }
        let A { name, server: patch, replace } = c.args()?;
        if !patch.is_null() && !patch.is_object() {
            return Err(Fail::Invalid("server must be an object".into()));
        }
        let ws = self.agent_ws()?;
        let existing = find_server(&ws, &name)?;
        let schema = server_schema();
        // The server as it will be saved: the patch over the saved one, or a new one of its kind.
        let mut merged = match (&existing, replace) {
            (Some(id), false) => serde_json::to_value(ws.read_server(id)?).map_err(|e| Fail::Error(e.to_string()))?,
            _ => {
                let kind: ServerKind = match patch.get("kind") {
                    Some(k) => serde_json::from_value(k.clone()).map_err(|_| {
                        Fail::Invalid(format!(
                            "server.kind: unknown kind {k} (http, websocket, sse, tcp, udp, dns, tcpProxy)"
                        ))
                    })?,
                    None => ServerKind::Http,
                };
                let used: Vec<u16> = ws.list_servers()?.iter().map(|s| s.port).collect();
                let mut fresh = Server::new(name.trim(), kind);
                while used.contains(&fresh.port) {
                    fresh.port = fresh.port.saturating_add(1);
                }
                serde_json::to_value(fresh).map_err(|e| Fail::Error(e.to_string()))?
            }
        };
        if !patch.is_null() {
            // Check the fields given before merging: a misspelled one must not disappear.
            strict::<Server>(with_name(patch.clone(), &name), "server", &schema).map_err(Fail::Invalid)?;
            merge_patch(&mut merged, patch);
        }
        if merged.get("name").and_then(Value::as_str).is_none_or(|n| n.trim().is_empty()) {
            merged["name"] = json!(name.trim());
        }
        let server: Server = strict(merged, "server", &schema).map_err(Fail::Invalid)?;
        check_server(&server).map_err(Fail::Invalid)?;

        let verb = if existing.is_some() { "Update" } else { "Create" };
        let mut items = vec![format!("{:?} on {}:{}", server.kind, server.host, server.port)];
        if server.kind == ServerKind::Http {
            items.push(plural(server.http.routes.len(), "route"));
        }
        self.gate_change(c, format!("{verb} server “{}”?", server.name), items).await?;

        let id: String = match &existing {
            Some(id) => serde_json::from_value(self.call("server.save", json!({ "id": id, "server": server })).await?),
            None => serde_json::from_value(self.call("server.create", json!({ "server": server })).await?),
        }
        .map_err(|e| Fail::Error(e.to_string()))?;

        // A running copy takes the change now (or says it needs a restart).
        let root = ws.root().to_string_lossy().into_owned();
        let run = self.inner.servers.running().into_iter().find(|r| r.server_id == id && r.workspace_path == root);
        let mut value = json!({ "id": id, "name": server.name, "created": existing.is_none() });
        if let Some(run) = run {
            let updated = self.call("server.update", json!({ "runId": run.run_id, "server": server })).await?;
            let applied = updated["applied"].as_bool().unwrap_or(false);
            value["running"] = json!({ "url": run.url, "applied": applied });
            if !applied {
                value["hint"] =
                    json!("The address, port, TLS or kind changed: stop_server and start_server to apply them.");
            }
        }
        let target = AgentTarget::Server { id };
        self.show(target.clone());
        Ok(Done::new(value, format!("{verb}d {}", server.name)).at(target))
    }

    pub(super) async fn tool_create_mock(&self, c: &Call<'_>) -> Outcome {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct A {
            #[serde(default)]
            name: String,
            folder: Option<String>,
            openapi_text: Option<String>,
            openapi_url: Option<String>,
            openapi_file: Option<String>,
            port: Option<u16>,
        }
        let a: A = c.args()?;
        let ws = self.agent_ws()?;
        let name = if a.name.trim().is_empty() { "Mock API".to_string() } else { a.name.trim().to_string() };
        let created = match (a.folder, a.openapi_text, a.openapi_url, a.openapi_file) {
            (Some(folder), None, None, None) => {
                let folder = folder.trim_matches('/').to_string();
                let from = if folder.is_empty() { "the collection".to_string() } else { format!("folder {folder}") };
                self.gate_change(c, format!("Create mock “{name}”?"), vec![format!("Routes from {from}")]).await?;
                self.call("mock.fromFolder", json!({ "folder": folder, "name": name })).await?
            }
            (None, Some(text), None, None) => {
                self.gate_change(
                    c,
                    format!("Create mock “{name}”?"),
                    vec![format!("From {} of OpenAPI", human_size(text.len()))],
                )
                .await?;
                self.call("mock.fromOpenApi", json!({ "text": text, "name": name })).await?
            }
            (None, None, Some(url), None) => {
                let guard =
                    self.gate_traffic(c, "Download the OpenAPI document", host_of(&url).into_iter().collect()).await?;
                self.gate_change(c, format!("Create mock “{name}”?"), vec![format!("From {url}")]).await?;
                let fetch = self.call("mock.fromOpenApi", json!({ "url": url, "name": name }));
                crate::agents::with_guard(guard, fetch).await.map_err(|e| self.blocked_hint(c, e))?
            }
            (None, None, None, Some(file)) => {
                if !std::path::Path::new(&file).is_absolute() {
                    return Err(Fail::Invalid("openapiFile must be an absolute path".into()));
                }
                let ask = Ask {
                    kind: ConfirmKind::File,
                    title: "Read a file for a mock?".into(),
                    message: format!(
                        "{} wants Zorvik to read this OpenAPI file and build a mock API:",
                        c.session.info.client
                    ),
                    items: vec![file.clone()],
                    confirm_label: "Create mock".into(),
                    session_option: false,
                    danger: false,
                };
                self.approve(c, ask).await?;
                self.call("mock.fromOpenApi", json!({ "path": file, "name": name })).await?
            }
            _ => return Err(Fail::Invalid("Give folder, or one of openapiText, openapiUrl or openapiFile".into())),
        };
        let id = created["id"].as_str().unwrap_or_default().to_string();
        let mut server = ws.read_server(&id)?;
        if let Some(port) = a.port.filter(|p| *p != server.port) {
            server.port = port;
            self.call("server.save", json!({ "id": id, "server": server })).await?;
        }
        let target = AgentTarget::Server { id: id.clone() };
        self.show(target.clone());
        let routes = created["routes"].as_u64().unwrap_or(0) as usize;
        let value = json!({
            "id": id,
            "name": server.name,
            "port": server.port,
            "routes": server.http.routes.iter().map(|r| format!("{} {}", r.method, r.path)).collect::<Vec<_>>(),
            "warnings": created["warnings"],
        });
        Ok(Done::new(value, format!("Created {} with {}", server.name, plural(routes, "route"))).at(target))
    }

    pub(super) async fn tool_start_server(&self, c: &Call<'_>) -> Outcome {
        #[derive(Deserialize)]
        struct A {
            name: String,
            port: Option<u16>,
        }
        let A { name, port } = c.args()?;
        let ws = self.agent_ws()?;
        let id = find_server(&ws, &name)?.ok_or_else(|| not_found(&name))?;
        let mut server = ws.read_server(&id)?;
        if let Some(port) = port {
            server.port = port;
        }
        let open = matches!(server.host.trim(), "0.0.0.0" | "::" | "[::]");
        let port_label = if server.port == 0 { "any free port".to_string() } else { format!("port {}", server.port) };
        let mut items = vec![format!("{:?} on {}, {port_label}", server.kind, server.host)];
        if open {
            items.push("Other devices on the network can reach it.".into());
        }
        let ask = Ask {
            kind: ConfirmKind::Server,
            title: format!("Start “{}”?", server.name),
            message: format!("{} wants to start a server:", c.session.info.client),
            items,
            confirm_label: "Start".into(),
            session_option: false,
            danger: open,
        };
        self.approve(c, ask).await?;
        let info = self.call("server.start", json!({ "id": id, "server": server })).await.map_err(|e| {
            let hint = if port == Some(0) { "" } else { " Call start_server with port: 0 to take any free port." };
            match e.code.as_str() {
                "server" if e.message.contains("already in use") => Fail::Invalid(format!("{}{hint}", e.message)),
                _ => e.into(),
            }
        })?;
        let target = AgentTarget::Server { id };
        self.show(target.clone());
        let url = info["url"].as_str().unwrap_or_default().to_string();
        let value = json!({ "url": url, "port": info["port"], "runId": info["runId"] });
        Ok(Done::new(value, format!("Running at {url}")).at(target))
    }

    pub(super) async fn tool_stop_server(&self, c: &Call<'_>) -> Outcome {
        #[derive(Deserialize)]
        struct A {
            name: String,
        }
        let A { name } = c.args()?;
        let run = self
            .find_running(&self.agent_ws()?, &name)
            .ok_or_else(|| Fail::Invalid(format!("'{name}' is not running")))?;
        self.call("server.stop", json!({ "runId": run.run_id })).await?;
        Ok(Done::new(json!({ "stopped": run.name }), "Stopped"))
    }

    pub(super) fn tool_server_traffic(&self, c: &Call<'_>) -> Outcome {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct A {
            name: String,
            limit: Option<usize>,
            since_id: Option<u64>,
        }
        let A { name, limit, since_id } = c.args()?;
        let ws = self.agent_ws()?;
        let run = self.find_running(&ws, &name).ok_or_else(|| {
            Fail::Invalid(format!(
                "'{name}' is not running (traffic is kept only while a server runs; start_server first)"
            ))
        })?;
        let entries = self.server_log(&run.run_id).ok_or_else(|| Fail::Invalid(format!("'{name}' is not running")))?;
        let limit = limit.unwrap_or(DEFAULT_TRAFFIC).clamp(1, MAX_TRAFFIC);
        let newer: Vec<&TrafficEntry> = entries.iter().filter(|e| since_id.is_none_or(|s| e.id > s)).collect();
        let skipped = newer.len().saturating_sub(limit);
        let redactor = self.redactor(&ws);
        let redact = |s: &str| redactor.text(s);
        let list: Vec<Value> = newer[skipped..].iter().map(|e| traffic_for_agent(e, &redact)).collect();
        let last_id = entries.last().map(|e| e.id);
        let value = json!({
            "server": run.name,
            "url": run.url,
            "entries": list,
            "lastId": last_id,
            "olderLeftOut": skipped,
        });
        Ok(Done::new(value, plural(list.len(), "entry")).at(AgentTarget::Server { id: run.server_id }))
    }

    /// The running copy of a server of the open workspace, by name or id.
    fn find_running(&self, ws: &Workspace, name: &str) -> Option<crate::servers::RunningServerInfo> {
        let root = ws.root().to_string_lossy().into_owned();
        let name = name.trim();
        self.inner
            .servers
            .running()
            .into_iter()
            .find(|r| r.workspace_path == root && (r.name.trim().eq_ignore_ascii_case(name) || r.server_id == name))
    }
}

/// The patch with a name, so a patch that leaves the name out still parses as a server.
fn with_name(mut patch: Value, name: &str) -> Value {
    if let Some(map) = patch.as_object_mut() {
        map.entry("name").or_insert_with(|| json!(name.trim()));
    }
    patch
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn misspelled_fields_are_refused_with_a_suggestion() {
        let schema = server_schema();
        let err = strict::<Server>(
            json!({ "name": "m", "http": { "routes": [{ "path": "/a", "stauts": 201 }] } }),
            "server",
            &schema,
        )
        .unwrap_err();
        assert!(err.contains("stauts") && err.contains("did you mean `status`"), "{err}");
        let err = strict::<Server>(json!({ "name": "m", "prot": 1 }), "server", &schema).unwrap_err();
        assert!(err.contains("`prot`") && err.contains("`port`"), "{err}");
        assert!(
            strict::<Server>(json!({ "name": "m", "http": { "routes": [{ "path": "/a" }] } }), "server", &schema)
                .is_ok()
        );
    }

    #[test]
    fn route_checks() {
        let mut s = Server::new("m", ServerKind::Http);
        s.http.routes.push(serde_json::from_value(json!({ "path": "users" })).unwrap());
        assert!(check_server(&s).unwrap_err().contains("must start with /"));
        s.http.routes[0].path = "/users/:id".into();
        s.http.routes[0].status = 42;
        assert!(check_server(&s).unwrap_err().contains("100-599"));
        s.http.routes[0].status = 201;
        s.http.routes[0].method = "GE T".into();
        assert!(check_server(&s).is_err());
        s.http.routes[0].method = "*".into();
        assert!(check_server(&s).is_ok());
    }

    #[test]
    fn documented_fields_match_the_model() {
        // Every field the schema documents must be one the model accepts.
        let mut fields = std::collections::BTreeSet::new();
        super::super::schema_fields(&server_schema(), &mut fields);
        let full = json!({
            "name": "x", "kind": "http", "host": "127.0.0.1", "port": 1, "autoStart": true, "docs": "",
            "tls": { "enabled": true, "certPath": "", "keyPath": "" },
            "http": { "routes": [{ "name": "", "method": "*", "path": "/", "status": 200, "headers": [{ "key": "a", "value": "b", "enabled": true }], "body": "", "delayMs": 0, "matchQuery": [], "matchHeaders": [], "matchBody": "", "fault": "none", "faultPercent": 100, "enabled": true }], "fallback": "proxy", "proxyUrl": "", "cors": true },
            "websocket": { "mode": "rules", "greeting": "", "rules": [{ "match": "regex", "pattern": "", "reply": "", "delayMs": 0, "enabled": true }] },
            "sse": { "events": [{ "event": "", "data": "", "id": "" }], "intervalMs": 0, "repeat": true },
            "socket": { "mode": "echo", "greeting": "", "rules": [], "encoding": "hex", "framing": "lengthPrefixed", "lengthBytes": 4, "lineEnding": "crLf" },
            "dns": { "records": [{ "name": "a", "type": "A", "value": "1.2.3.4", "ttl": 60, "enabled": true }], "upstream": "" },
            "proxy": { "target": "a:1", "upstreamTls": true },
        });
        strict::<Server>(full.clone(), "server", &server_schema()).unwrap();
        let mut given = std::collections::BTreeSet::new();
        collect_keys(&full, &mut given);
        for f in &fields {
            assert!(given.contains(f), "schema documents `{f}`, which the test server above doesn't exercise");
        }
    }

    fn collect_keys(v: &Value, out: &mut std::collections::BTreeSet<String>) {
        match v {
            Value::Object(m) => {
                for (k, x) in m {
                    out.insert(k.clone());
                    collect_keys(x, out);
                }
            }
            Value::Array(a) => a.iter().for_each(|x| collect_keys(x, out)),
            _ => {}
        }
    }

    #[test]
    fn merge_patch_semantics() {
        let mut v = json!({ "port": 1, "http": { "routes": [{ "path": "/a" }], "cors": true } });
        super::super::merge_patch(&mut v, json!({ "http": { "routes": [{ "path": "/b" }], "cors": null } }));
        assert_eq!(v, json!({ "port": 1, "http": { "routes": [{ "path": "/b" }] } }));
    }

    #[test]
    fn items_are_capped() {
        assert_eq!(super::super::cap_items((0..31).map(|i| i.to_string()).collect()).len(), 31);
    }
}

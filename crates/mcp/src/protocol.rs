//! MCP messages: JSON-RPC 2.0, one message per line.

use serde_json::{Value, json};

/// Protocol versions spoken, newest first.
pub const PROTOCOL_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];
/// Answered to a client asking for a version not in the list: the newest (the spec's "SHOULD").
const FALLBACK_VERSION: &str = PROTOCOL_VERSIONS[0];

pub const PARSE_ERROR: i64 = -32700;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;

/// App → bridge: the user disconnected this agent; the bridge takes no more calls.
pub const DISCONNECTED: &str = "notifications/zorvik/disconnected";

/// What agents are told about Zorvik when they connect.
const INSTRUCTIONS: &str = "Zorvik is the user's desktop API client (HTTP, GraphQL, gRPC, WebSocket, load tests, mock \
servers). Its workspace is a folder of YAML files the user keeps in Git. Start with get_workspace. The user watches your \
actions live in the Zorvik window and approves risky ones there (deletes, load tests, servers, and depending on their \
settings edits and requests to outside hosts); a declined action should not be retried in another way — ask the user. \
Put hosts and credentials in {{variables}} (environments, secret variables) rather than in requests. Secret values show \
as ••••••.";

pub fn negotiate(requested: Option<&str>) -> &'static str {
    requested.and_then(|r| PROTOCOL_VERSIONS.iter().find(|v| **v == r).copied()).unwrap_or(FALLBACK_VERSION)
}

pub fn initialize_result(requested: Option<&str>) -> Value {
    json!({
        "protocolVersion": negotiate(requested),
        "capabilities": { "tools": { "listChanged": false }, "prompts": { "listChanged": false } },
        "serverInfo": { "name": "zorvik", "title": "Zorvik", "version": env!("CARGO_PKG_VERSION") },
        "instructions": INSTRUCTIONS,
    })
}

pub fn result(id: &Value, result: Value) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string()
}

pub fn error(id: &Value, code: i64, message: &str) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } }).to_string()
}

pub fn notification(method: &str, params: Value) -> String {
    json!({ "jsonrpc": "2.0", "method": method, "params": params }).to_string()
}

/// A tool call's result that says it failed (the agent reads the message).
pub fn tool_error(id: &Value, message: &str) -> String {
    result(id, json!({ "content": [{ "type": "text", "text": message }], "isError": true }))
}

/// Requests answered without the app. `None`: not one of them.
pub fn answer_locally(method: &str, params: &Value) -> Option<Result<Value, (i64, String)>> {
    Some(match method {
        "ping" | "logging/setLevel" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": zorvik_api::agents::tool_definitions() })),
        "prompts/list" => Ok(json!({ "prompts": zorvik_api::agents::prompt_definitions() })),
        "prompts/get" => {
            let name = params["name"].as_str().unwrap_or_default();
            zorvik_api::agents::get_prompt(name, &params["arguments"])
                .ok_or_else(|| (INVALID_PARAMS, format!("Unknown prompt '{name}'")))
        }
        // Not offered, but some clients ask anyway.
        "resources/list" => Ok(json!({ "resources": [] })),
        "resources/templates/list" => Ok(json!({ "resourceTemplates": [] })),
        _ => return None,
    })
}

/// A message's id as a map key (ids are numbers or strings).
pub fn id_key(id: &Value) -> String {
    id.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_and_local_answers() {
        assert_eq!(negotiate(Some("2025-03-26")), "2025-03-26");
        assert_eq!(negotiate(Some("1999-01-01")), PROTOCOL_VERSIONS[0]);
        assert_eq!(negotiate(None), FALLBACK_VERSION);
        let init = initialize_result(Some("2025-06-18"));
        assert_eq!(init["serverInfo"]["name"], "zorvik");
        assert!(init["capabilities"]["tools"].is_object());
        let tools = answer_locally("tools/list", &json!({})).unwrap().unwrap();
        assert!(tools["tools"].as_array().unwrap().len() > 20);
        assert!(answer_locally("prompts/get", &json!({ "name": "nope" })).unwrap().is_err());
        assert!(answer_locally("tools/call", &json!({})).is_none());
        assert_eq!(id_key(&json!(7)), "7");
        assert_eq!(id_key(&json!("7")), "\"7\"");
    }
}

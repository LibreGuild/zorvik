//! MCP prompts: ready-made instructions agents offer as commands
//! (in Claude Code: `/mcp__zorvik__map_apis`).

use serde_json::{Value, json};

struct PromptDef {
    name: &'static str,
    title: &'static str,
    description: &'static str,
    /// (name, description, required)
    arguments: &'static [(&'static str, &'static str, bool)],
}

const PROMPTS: &[PromptDef] = &[
    PromptDef {
        name: "map_apis",
        title: "Map this code's APIs to Zorvik",
        description: "Find the HTTP/GraphQL/gRPC endpoints in the current codebase and save them as a Zorvik collection.",
        arguments: &[
            ("scope", "Part of the code to map, e.g. a folder or a service (default: the whole repository).", false),
            ("folder", "Collection folder to put the requests in (default: one folder per area of the API).", false),
        ],
    },
    PromptDef {
        name: "test_apis",
        title: "Test APIs with Zorvik",
        description: "Add tests to the collection's requests, run them in Zorvik and report what fails.",
        arguments: &[("folder", "Collection folder to test (default: the whole collection).", false)],
    },
];

/// `prompts/list` entries.
pub fn prompt_definitions() -> Vec<Value> {
    PROMPTS
        .iter()
        .map(|p| {
            json!({
                "name": p.name,
                "title": p.title,
                "description": p.description,
                "arguments": p.arguments.iter().map(|(name, description, required)| json!({
                    "name": name, "description": description, "required": required
                })).collect::<Vec<_>>(),
            })
        })
        .collect()
}

/// `prompts/get`: the prompt's messages, or `None` for an unknown name.
pub fn get_prompt(name: &str, arguments: &Value) -> Option<Value> {
    let def = PROMPTS.iter().find(|p| p.name == name)?;
    let arg = |key: &str| arguments.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty());
    let text = match name {
        "map_apis" => {
            let scope = arg("scope").map(|s| format!(" Only map {s}.")).unwrap_or_default();
            let folder = arg("folder")
                .map(|f| format!("Put everything under the collection folder \"{f}\"."))
                .unwrap_or_else(|| "Make one folder per area of the API (controller, router or service).".into());
            format!(
                "Map the APIs of this codebase into the Zorvik collection.{scope}\n\n\
                 1. Call get_workspace. If no workspace is open, ask me where it should live (a `.zorvik` folder in this \
                 repository is a good default) and call open_workspace.\n\
                 2. Call list_requests to see what is there already; update those requests instead of adding duplicates.\n\
                 3. Find every endpoint in the code: route definitions, controllers, GraphQL schemas and resolvers, .proto \
                 services. For each, work out the method, path, path and query parameters, headers, the request body \
                 (with a realistic example built from the types or validation rules) and the auth it needs.\n\
                 4. {folder} Use {{{{baseUrl}}}} for the server address (and save_environment for a \"Local\" environment \
                 with baseUrl, plus others you can find in config files). Use {{{{variables}}}} for ids and tokens; never \
                 put real secrets in requests — mark secret variables with \"secret\": true. Put shared auth on the \
                 folder or the collection with save_folder_settings (auth: inherit on the requests).\n\
                 5. Save them with save_requests (in batches of up to 50). Put query parameters in `query` (optional ones \
                 with enabled: false, each with a description) and values of :name path segments in `pathParams`. Name \
                 requests the way people would say them (\"Create order\"), and write in docs where the handler is (file \
                 and line) and what it returns.\n\
                 6. If an OpenAPI/Swagger or Postman file exists and is current, import it instead (import with its \
                 text), then fill in what is missing.\n\
                 7. Finally, tell me what you added, what you could not map, and offer to send a request to check it \
                 (or, when the server isn't running, to build a mock of it with create_mock)."
            )
        }
        "test_apis" => {
            let folder =
                arg("folder").map(|f| format!("the folder \"{f}\"")).unwrap_or_else(|| "the whole collection".into());
            format!(
                "Test {folder} in Zorvik.\n\n\
                 1. Call list_requests and read_request to see the requests and what they should return (check the code \
                 when unsure).\n\
                 2. Add Postman-style tests to each request's post-response script with save_requests, for example: \
                 pm.test('status is 200', () => pm.response.to.have.status(200)); \
                 pm.test('has an id', () => pm.expect(pm.response.json().id).to.be.a('string')). Check the status, the \
                 shape of the body and important values; save ids for later requests with pm.environment.set. For work \
                 that finishes later, give the polling request settings.repeat ({{condition, intervalMs, timeoutMs}}) \
                 instead of a fixed delay. Requests imported from an OpenAPI document are already checked against it \
                 (a \"Matches the API spec\" test).\n\
                 3. Run them with run_collection. For failures, read the results, find out whether the API or the test is \
                 wrong (look at the code), and fix the test or tell me about the bug.\n\
                 4. Report what passed, what failed and why."
            )
        }
        _ => return None,
    };
    Some(json!({
        "description": def.description,
        "messages": [{ "role": "user", "content": { "type": "text", "text": text } }],
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompts_render() {
        assert_eq!(prompt_definitions().len(), PROMPTS.len());
        let p = get_prompt("map_apis", &json!({ "folder": "Orders" })).unwrap();
        let text = p["messages"][0]["content"]["text"].as_str().unwrap();
        assert!(text.contains("\"Orders\"") && text.contains("{{baseUrl}}"), "{text}");
        assert!(get_prompt("nope", &json!({})).is_none());
    }
}

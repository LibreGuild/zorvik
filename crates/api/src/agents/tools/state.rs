//! Tools that read what the app knows now: variable values (with the ones scripts saved)
//! and the history of sent requests with their last responses.

use serde::Deserialize;
use serde_json::{Value, json};

use super::{Call, Def, Done, Fail, Outcome, cut, obj, plural, string};
use crate::Api;

/// History entries returned by default, and at most.
const DEFAULT_HISTORY: u32 = 20;
const MAX_HISTORY: u32 = 100;
/// Response body characters per history entry by default, and at most.
const DEFAULT_HISTORY_BODY: usize = 4_000;
const MAX_HISTORY_BODY: usize = 20_000;

pub(super) fn defs() -> Vec<Def> {
    vec![
        Def {
            name: "get_variables",
            title: "Current variables",
            description: "The variables requests use right now, as Zorvik resolves them: the active environment, then the collection, then globals, each with the values scripts saved (pm.environment.set, pm.collectionVariables.set, pm.globals.set) over the file's values. Call it after a collection run to pick up ids and tokens the run saved. pm.variables values last only for one send or run and are not here. Secret values show as ••••••.",
            schema: obj(json!({}), &[]),
            read_only: true,
            destructive: false,
            open_world: false,
        },
        Def {
            name: "read_history",
            title: "Read history",
            description: "Requests sent from Zorvik (by the user or by agents), newest first: method, URL, status, time, size, and for the last 50 sends the response headers and body. Use it to pick up what the user was just doing.",
            schema: obj(
                json!({
                    "limit": { "type": "integer", "description": "Entries to return (default 20, at most 100)." },
                    "path": string("Only sends of this saved request (path as list_requests shows it)."),
                    "search": string("Only entries whose URL or method contains this text."),
                    "maxBodyChars": { "type": "integer", "description": "Response body characters per entry (default 4000, at most 20000; 0 for none)." },
                }),
                &[],
            ),
            read_only: true,
            destructive: false,
            open_world: false,
        },
    ]
}

impl Api {
    pub(super) fn tool_get_variables(&self) -> Outcome {
        let ws = self.agent_ws()?;
        let active = self.active_env_id(&ws);
        let active_name =
            self.environments(&ws)?.into_iter().find(|e| Some(&e.id) == active.as_ref()).map(|e| e.environment.name);
        let variables: Vec<Value> = self
            .variable_infos(&ws)
            .into_iter()
            .map(|v| {
                let mut o = json!({ "key": v.key, "value": v.value, "source": v.source });
                if v.secret {
                    o["secret"] = json!(true);
                }
                if v.local {
                    o["setByScript"] = json!(true);
                }
                o
            })
            .collect();
        let n = variables.len();
        let value = json!({ "activeEnvironment": active_name, "variables": variables });
        Ok(Done::new(value, plural(n, "variable")))
    }

    pub(super) fn tool_read_history(&self, c: &Call<'_>) -> Outcome {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct A {
            limit: Option<u32>,
            path: Option<String>,
            #[serde(default)]
            search: String,
            max_body_chars: Option<usize>,
        }
        let A { limit, path, search, max_body_chars } = c.args()?;
        let ws = self.agent_ws()?;
        let Some(history) = &self.inner.history else {
            return Ok(Done::new(json!({ "entries": [] }), "No history"));
        };
        let limit = limit.unwrap_or(DEFAULT_HISTORY).clamp(1, MAX_HISTORY);
        let body_chars = max_body_chars.unwrap_or(DEFAULT_HISTORY_BODY).min(MAX_HISTORY_BODY);
        let path = path.map(|p| p.trim().to_string()).filter(|p| !p.is_empty());
        // A path filter looks further back: other requests' entries are skipped.
        let fetch = if path.is_some() { 1000 } else { limit };
        let entries =
            history.list(&ws.root().to_string_lossy(), &search, fetch, 0).map_err(|e| Fail::Error(e.to_string()))?;
        let redactor = self.redactor(&ws);
        let list: Vec<Value> = entries
            .into_iter()
            .filter(|e| path.as_ref().is_none_or(|p| e.request_path.as_deref() == Some(p.as_str())))
            .take(limit as usize)
            .map(|e| {
                let time = time::OffsetDateTime::from_unix_timestamp_nanos(i128::from(e.created_at) * 1_000_000)
                    .ok()
                    .and_then(|t| t.format(&time::format_description::well_known::Rfc3339).ok());
                let mut out = json!({
                    "id": e.id,
                    "time": time,
                    "path": e.request_path,
                    "method": e.method,
                    "url": redactor.text(&e.url),
                    "status": e.status,
                    "error": e.error.as_deref().map(|m| redactor.text(m)),
                    "timeMs": e.duration_ms.map(|d| (d * 10.0).round() / 10.0),
                    "size": e.size,
                });
                if let Some(response) = self.recent_response(e.id) {
                    let mut r = json!({
                        "status": response.status,
                        "statusText": response.status_text,
                        "headers": redactor.headers(&response.headers),
                    });
                    if body_chars > 0 {
                        match &response.body {
                            Some(text) => {
                                let (body, cut_now) = cut(&redactor.text(text), body_chars);
                                r["body"] = json!(body);
                                if cut_now || response.body_cut {
                                    r["bodyTruncated"] = json!(true);
                                }
                            }
                            None => r["body"] = json!("(binary)"),
                        }
                    }
                    out["response"] = r;
                }
                out
            })
            .collect();
        let n = list.len();
        Ok(Done::new(json!({ "entries": list }), plural(n, "entry")))
    }
}

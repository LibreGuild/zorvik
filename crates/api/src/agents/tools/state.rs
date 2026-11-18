//! Tools that read what the app knows now: variable values (with the ones scripts saved),
//! the history of sent requests with their last responses, and the console.

use serde::Deserialize;
use serde_json::{Value, json};

use super::{Call, Def, Done, Fail, Outcome, cut, obj, plural, string};
use crate::Api;
use crate::agents::redact::Redactor;
use crate::console::{ConsoleBody, ConsoleItem};

/// History entries returned by default, and at most.
const DEFAULT_HISTORY: u32 = 20;
const MAX_HISTORY: u32 = 100;
/// Response body characters per history entry by default, and at most.
const DEFAULT_HISTORY_BODY: usize = 4_000;
const MAX_HISTORY_BODY: usize = 20_000;
/// Console lines returned by default, and at most.
const DEFAULT_CONSOLE: usize = 50;
const MAX_CONSOLE: usize = 200;

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
        Def {
            name: "read_console",
            title: "Read the console",
            description: "Zorvik's console, oldest first: every request the app sent since it started or the console was cleared (by the user, by collection runs, by agents, by scripts with pm.sendRequest, and OAuth 2.0 token requests) with the headers and body that were sent and that came back, connections being opened and closed, and what scripts wrote with console.log. Use it to see why a request failed. Kept in memory only (the newest 5000 lines); bodies are the first 64 KB. Credentials are hidden, and token requests come without their headers and bodies.",
            schema: obj(
                json!({
                    "limit": { "type": "integer", "description": "Lines to return, the newest ones (default 50, at most 200)." },
                    "search": string("Only lines whose URL, request name, error or message contains this text (any case)."),
                    "errorsOnly": { "type": "boolean", "description": "Only requests that failed or answered with a status of 400 or more, and error lines." },
                    "kind": { "type": "string", "enum": ["all", "requests", "logs"], "description": "Only requests, or only log lines (default all)." },
                    "details": { "type": "boolean", "description": "Include each request's headers and bodies (default true)." },
                    "maxBodyChars": { "type": "integer", "description": "Body characters per request and per response (default 4000, at most 20000; 0 for none)." },
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

    pub(super) fn tool_read_console(&self, c: &Call<'_>) -> Outcome {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct A {
            limit: Option<usize>,
            #[serde(default)]
            search: String,
            #[serde(default)]
            errors_only: bool,
            kind: Option<String>,
            details: Option<bool>,
            max_body_chars: Option<usize>,
        }
        let A { limit, search, errors_only, kind, details, max_body_chars } = c.args()?;
        let ws = self.agent_ws()?;
        let limit = limit.unwrap_or(DEFAULT_CONSOLE).clamp(1, MAX_CONSOLE);
        let body_chars = max_body_chars.unwrap_or(DEFAULT_HISTORY_BODY).min(MAX_HISTORY_BODY);
        let with_details = details.unwrap_or(true);
        let (requests, logs) = match kind.as_deref().unwrap_or("all") {
            "all" => (true, true),
            "requests" => (true, false),
            "logs" => (false, true),
            other => return Err(Fail::Error(format!("kind is all, requests or logs, not '{other}'"))),
        };
        let needle = search.trim().to_lowercase();
        let has = |text: &str| text.to_lowercase().contains(&needle);
        let secrets = self.redactor(&ws);
        // What hides a line's credentials: the secrets now, the ones in use when it was logged
        // (they may have changed since), and what its own credential headers carried.
        let redactor_of = |read: &crate::console::Read| {
            let mut redactor = secrets.clone();
            redactor.absorb(&crate::lock(&read.redact));
            if let Some(d) = &read.details {
                redactor.absorb(&Redactor::default().with_headers(&d.request_headers));
                if let Some(r) = &d.response {
                    redactor.absorb(&Redactor::default().with_headers(&r.headers));
                }
            }
            redactor
        };
        let lines = self.inner.console.read(limit, |read| {
            let kind = match &read.line.item {
                ConsoleItem::Log { level, .. } => {
                    logs && (!errors_only || *level == zorvik_script::ConsoleLevel::Error)
                }
                ConsoleItem::Exchange { status, error, .. } => {
                    requests && (!errors_only || error.is_some() || status.is_some_and(|s| s >= 400))
                }
            };
            if !kind || needle.is_empty() {
                return kind;
            }
            // The search sees what the agent sees: looking for a secret's value finds nothing.
            let redactor = redactor_of(read);
            let shown = |text: &str| has(&redactor.text(text));
            read.line.request.as_deref().is_some_and(shown)
                || match &read.line.item {
                    ConsoleItem::Log { message, .. } => shown(message),
                    ConsoleItem::Exchange { url, error, .. } => shown(url) || error.as_deref().is_some_and(shown),
                }
        });
        let body = |b: &Option<ConsoleBody>, r: &Redactor| -> Option<Value> {
            let b = b.as_ref()?;
            let Some(text) = &b.text else { return Some(json!({ "binary": true, "size": b.size })) };
            if body_chars == 0 {
                return Some(json!({ "size": b.size }));
            }
            // A body the console cut may end inside a secret, which nothing would recognize:
            // its end goes.
            let mut end = if b.truncated { text.len().saturating_sub(r.longest()) } else { text.len() };
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            let (text, cut_now) = cut(&r.text(&text[..end]), body_chars);
            let mut o = json!({ "text": text, "size": b.size });
            if cut_now || b.truncated {
                o["truncated"] = json!(true);
            }
            Some(o)
        };
        let list: Vec<Value> = lines
            .into_iter()
            .map(|read| {
                let redactor = redactor_of(&read);
                let line = read.line;
                let time = time::OffsetDateTime::from_unix_timestamp_nanos(i128::from(line.at) * 1_000_000)
                    .ok()
                    .and_then(|t| t.format(&time::format_description::well_known::Rfc3339).ok());
                let mut out = json!({ "id": line.id, "time": time, "source": line.source, "request": line.request });
                match line.item {
                    ConsoleItem::Log { level, message } => {
                        out["type"] = json!("log");
                        out["level"] = json!(level);
                        out["message"] = json!(redactor.text(&message));
                    }
                    ConsoleItem::Exchange {
                        protocol,
                        method,
                        url,
                        status,
                        status_text,
                        error,
                        duration_ms,
                        size,
                        origin,
                    } => {
                        out["type"] = json!("request");
                        out["protocol"] = json!(protocol);
                        out["method"] = json!(method);
                        out["url"] = json!(redactor.text(&url));
                        out["status"] = json!(status);
                        out["statusText"] = json!(status_text);
                        out["error"] = json!(error.as_deref().map(|e| redactor.text(e)));
                        out["timeMs"] = json!(duration_ms.map(|d| (d * 10.0).round() / 10.0));
                        out["size"] = json!(size);
                        if let Some(origin) = origin {
                            out["origin"] = json!(origin);
                        }
                        match (&read.details, read.sensitive) {
                            (_, true) => out["detailsHidden"] = json!("This request carries credentials."),
                            (Some(d), false) if with_details => {
                                out["requestHeaders"] = json!(redactor.headers(&d.request_headers));
                                if let Some(b) = body(&d.request_body, &redactor) {
                                    out["requestBody"] = b;
                                }
                                if let Some(r) = &d.response {
                                    out["responseHeaders"] = json!(redactor.headers(&r.headers));
                                    if let Some(b) = body(&r.body, &redactor) {
                                        out["responseBody"] = b;
                                    }
                                }
                                if !d.redirects.is_empty() {
                                    let hops: Vec<String> = d
                                        .redirects
                                        .iter()
                                        .map(|h| {
                                            format!(
                                                "{} {} → {}",
                                                h.status,
                                                redactor.text(&h.url),
                                                redactor.text(&h.location)
                                            )
                                        })
                                        .collect();
                                    out["redirects"] = json!(hops);
                                }
                                if let Some(addr) = &d.remote_addr {
                                    out["remoteAddress"] = json!(addr);
                                }
                            }
                            _ => {}
                        }
                    }
                }
                out
            })
            .collect();
        let n = list.len();
        Ok(Done::new(json!({ "lines": list }), plural(n, "line")))
    }
}

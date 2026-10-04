//! What a script can ask of the app while it runs: send a request (`pm.sendRequest`),
//! read and change cookies (`pm.cookies.jar()`), and fill in dynamic variables
//! (`pm.variables.replaceIn('{{$randomEmail}}')`). The app implements [`Host`]; the
//! prelude reaches it through functions on `setup`'s `host` argument. Calls block the
//! script's thread, and none of them takes longer than what is left of the script's time.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rquickjs::{Ctx, Function, Object};
use serde::{Deserialize, Serialize};

use crate::ScriptHeader;

/// Most `pm.sendRequest` calls in one script run.
pub const MAX_SENDS: u32 = 100;

/// A request a script sends (`pm.sendRequest`). Variables are not filled in: like Postman,
/// scripts call `pm.variables.replaceIn` themselves.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
pub struct HostRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<ScriptHeader>,
    pub body: String,
}

/// A cookie as scripts see it (`pm.cookies`, `pm.response.cookies`, the jar).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptCookie {
    pub name: String,
    pub value: String,
    #[serde(default)]
    pub domain: String,
    #[serde(default)]
    pub path: String,
    /// RFC 3339, `None` for session cookies.
    #[serde(default)]
    pub expires: Option<String>,
    #[serde(default)]
    pub secure: bool,
    #[serde(default)]
    pub http_only: bool,
}

/// The answer to a [`HostRequest`].
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostResponse {
    pub code: u16,
    pub status: String,
    pub headers: Vec<ScriptHeader>,
    /// The body as text (binary bodies are decoded lossily).
    pub body: String,
    /// Milliseconds.
    pub response_time: f64,
    pub response_size: u64,
    /// Cookies the response set.
    pub cookies: Vec<ScriptCookie>,
}

/// The app's side of a script run.
pub trait Host: Send + Sync {
    /// Send `request`, answering within `timeout`. Errors are sentences for the script.
    fn send(&self, request: HostRequest, timeout: Duration) -> Result<HostResponse, String>;
    /// The cookie jar's cookies for `url`.
    fn cookies(&self, url: &str) -> Result<Vec<ScriptCookie>, String>;
    /// Store `name=value` for `url`.
    fn set_cookie(&self, url: &str, name: &str, value: &str) -> Result<(), String>;
    /// Remove the cookie `name` for `url`, or all of them (`None`).
    fn remove_cookies(&self, url: &str, name: Option<&str>) -> Result<(), String>;
    /// A dynamic variable's value (`$randomEmail`, `$randomInt(1, 5)`); `None` when unknown.
    fn dynamic(&self, expression: &str) -> Option<String>;
}

/// Adds the host functions to the prelude's `host` object: `send(json)`, `cookies(url)`,
/// `cookie(op, url, name, value)`, `dynamic(expression)`, `now()` and `sleep(ms)`. JSON goes in
/// and out, so the prelude sees plain objects. `deadline` is when the script must stop.
pub(crate) fn install<'js>(
    ctx: &Ctx<'js>,
    object: &Object<'js>,
    app: Option<Arc<dyn Host>>,
    deadline: Rc<Cell<Option<Instant>>>,
) -> rquickjs::Result<()> {
    let left = {
        let deadline = deadline.clone();
        move || deadline.get().map_or(Duration::ZERO, |d| d.saturating_duration_since(Instant::now()))
    };
    let sends = Rc::new(Cell::new(0u32));

    let host = app.clone();
    let time_left = left.clone();
    object.set(
        "send",
        Function::new(ctx.clone(), move |json: String| -> String {
            let reply = |r: Result<HostResponse, String>| match r {
                Ok(response) => serde_json::json!({ "ok": true, "response": response }).to_string(),
                Err(error) => serde_json::json!({ "ok": false, "error": error }).to_string(),
            };
            let Some(host) = host.as_ref() else {
                return reply(Err("pm.sendRequest isn't available here".into()));
            };
            if sends.get() >= MAX_SENDS {
                return reply(Err(format!("pm.sendRequest: at most {MAX_SENDS} requests per script")));
            }
            sends.set(sends.get() + 1);
            let request = match serde_json::from_str::<HostRequest>(&json) {
                Ok(r) => r,
                Err(e) => return reply(Err(format!("pm.sendRequest: {e}"))),
            };
            let timeout = time_left();
            if timeout.is_zero() {
                return reply(Err("The script ran out of time before the request".into()));
            }
            reply(host.send(request, timeout))
        })?,
    )?;

    let host = app.clone();
    object.set(
        "cookies",
        Function::new(ctx.clone(), move |url: String| -> String {
            let result = match host.as_ref() {
                Some(h) => h.cookies(&url),
                None => Ok(Vec::new()),
            };
            match result {
                Ok(list) => serde_json::json!({ "ok": true, "cookies": list }).to_string(),
                Err(error) => serde_json::json!({ "ok": false, "error": error }).to_string(),
            }
        })?,
    )?;

    let host = app.clone();
    object.set(
        "cookie",
        Function::new(
            ctx.clone(),
            move |op: String, url: String, name: Option<String>, value: Option<String>| -> String {
                let Some(host) = host.as_ref() else {
                    return serde_json::json!({ "ok": false, "error": "The cookie jar isn't available here" })
                        .to_string();
                };
                let result = match op.as_str() {
                    "set" => {
                        host.set_cookie(&url, name.as_deref().unwrap_or_default(), value.as_deref().unwrap_or_default())
                    }
                    "unset" => host.remove_cookies(&url, Some(name.as_deref().unwrap_or_default())),
                    "clear" => host.remove_cookies(&url, None),
                    other => Err(format!("unknown cookie operation {other}")),
                };
                match result {
                    Ok(()) => serde_json::json!({ "ok": true }).to_string(),
                    Err(error) => serde_json::json!({ "ok": false, "error": error }).to_string(),
                }
            },
        )?,
    )?;

    let host = app;
    object.set(
        "dynamic",
        Function::new(ctx.clone(), move |expression: String| -> Option<String> {
            host.as_ref().and_then(|h| h.dynamic(&expression))
        })?,
    )?;

    // Timers keep time with `now` and wait with `sleep`, both on the monotonic clock: `Date.now()`
    // follows the wall clock, which on Windows moves in steps of about 15 ms.
    let epoch = Instant::now();
    object.set("now", Function::new(ctx.clone(), move || epoch.elapsed().as_secs_f64() * 1000.0)?)?;

    object.set(
        "sleep",
        Function::new(ctx.clone(), move |ms: f64| {
            let wanted = Duration::try_from_secs_f64(ms.min(3_600_000.0) / 1000.0).unwrap_or_default();
            std::thread::sleep(wanted.min(left()));
        })?,
    )?;
    Ok(())
}

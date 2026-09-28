//! OAuth 2.0 token acquisition: client credentials, password, and authorization
//! code (+PKCE) and implicit via a loopback redirect.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use base64::Engine as _;
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use sha2::Digest as _;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use ts_rs::TS;
use zorvik_engine::{Client, Header, HttpRequest, RequestOptions};
use zorvik_formats::{ClientAuthMethod, GrantType, OAuth2Config};

use crate::error::{Error, ErrorCode, Result};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TokenSet {
    pub access_token: String,
    pub token_type: String,
    /// Unix epoch milliseconds; `None` when the server gave no expiry.
    #[ts(type = "number | null")]
    pub expires_at: Option<i64>,
    pub refresh_token: Option<String>,
    pub scope: Option<String>,
    #[ts(type = "number")]
    pub obtained_at: i64,
}

impl TokenSet {
    /// Valid for at least another 30 seconds.
    pub fn is_fresh(&self) -> bool {
        self.expires_at.is_none_or(|t| t.saturating_sub(30_000) > now_ms())
    }
}

fn now_ms() -> i64 {
    (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000) as i64
}

fn auth_err(message: impl Into<String>) -> Error {
    Error::new(ErrorCode::Auth, message)
}

/// Cache key: tokens are shared by the requests of one workspace (its local key)
/// with the same OAuth2 identity. Credentials are part of it, so a config (e.g.
/// from another environment) that lacks the right secret or password never gets
/// the token, and another workspace never gets it at all.
pub fn cache_key(workspace: &str, config: &OAuth2Config) -> String {
    let parts = [
        workspace.to_string(),
        format!("{:?}", config.grant_type),
        config.token_url.clone(),
        config.auth_url.clone(),
        config.client_id.clone(),
        config.client_secret.clone(),
        config.scope.clone(),
        config.audience.clone(),
        config.username.clone(),
        config.password.clone(),
    ];
    let digest = sha2::Sha256::digest(parts.join("\u{1}").as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&digest[..16])
}

/// Token cache, optionally persisted to a JSON file in the app data dir.
pub struct TokenCache {
    path: Option<PathBuf>,
    tokens: Mutex<HashMap<String, TokenSet>>,
}

impl TokenCache {
    pub fn in_memory() -> Self {
        Self { path: None, tokens: Mutex::new(HashMap::new()) }
    }

    pub fn persistent(path: PathBuf) -> Self {
        let tokens = std::fs::read(&path).ok().and_then(|d| serde_json::from_slice(&d).ok()).unwrap_or_default();
        Self { path: Some(path), tokens: Mutex::new(tokens) }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, TokenSet>> {
        self.tokens.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn get(&self, key: &str) -> Option<TokenSet> {
        self.lock().get(key).cloned()
    }

    pub fn put(&self, key: &str, token: TokenSet) {
        let mut tokens = self.lock();
        tokens.insert(key.to_string(), token);
        self.persist(&tokens);
    }

    pub fn remove(&self, key: &str) {
        let mut tokens = self.lock();
        tokens.remove(key);
        self.persist(&tokens);
    }

    fn persist(&self, tokens: &HashMap<String, TokenSet>) {
        if let Some(path) = &self.path {
            let _ = crate::store::save_json_private(path, tokens);
        }
    }
}

/// Get a usable token without user interaction: cached, refreshed, or (for
/// client credentials / password grants) freshly requested.
pub async fn ensure_token(
    client: &Client,
    opts: &RequestOptions,
    config: &OAuth2Config,
    cache: &TokenCache,
    workspace: &str,
) -> Result<TokenSet> {
    let key = cache_key(workspace, config);
    if let Some(token) = cache.get(&key) {
        if token.is_fresh() {
            return Ok(token);
        }
        if let Some(refresh) = &token.refresh_token
            && let Ok(new_token) = refresh_token(client, opts, config, refresh).await
        {
            cache.put(&key, new_token.clone());
            return Ok(new_token);
        }
    }
    let token = match config.grant_type {
        GrantType::ClientCredentials | GrantType::Password => request_token(client, opts, config, &[]).await?,
        GrantType::AuthorizationCode | GrantType::Implicit => {
            return Err(auth_err("No valid OAuth 2.0 token. Open the Auth tab and click \"Get token\" to sign in."));
        }
    };
    cache.put(&key, token.clone());
    Ok(token)
}

async fn refresh_token(
    client: &Client,
    opts: &RequestOptions,
    config: &OAuth2Config,
    refresh: &str,
) -> Result<TokenSet> {
    let mut token =
        post_token(client, opts, config, &[("grant_type", "refresh_token"), ("refresh_token", refresh)]).await?;
    if token.refresh_token.is_none() {
        token.refresh_token = Some(refresh.to_string());
    }
    Ok(token)
}

async fn request_token(
    client: &Client,
    opts: &RequestOptions,
    config: &OAuth2Config,
    extra: &[(&str, &str)],
) -> Result<TokenSet> {
    let mut form: Vec<(&str, &str)> = Vec::new();
    match config.grant_type {
        GrantType::ClientCredentials => form.push(("grant_type", "client_credentials")),
        GrantType::Password => {
            form.push(("grant_type", "password"));
            form.push(("username", &config.username));
            form.push(("password", &config.password));
        }
        GrantType::AuthorizationCode => form.push(("grant_type", "authorization_code")),
        // The implicit grant gets its token from the redirect, never from the token URL.
        GrantType::Implicit => return Err(auth_err("The implicit grant gets its token by signing in")),
    }
    form.extend_from_slice(extra);
    post_token(client, opts, config, &form).await
}

async fn post_token(
    client: &Client,
    opts: &RequestOptions,
    config: &OAuth2Config,
    fields: &[(&str, &str)],
) -> Result<TokenSet> {
    if config.token_url.trim().is_empty() {
        return Err(auth_err("OAuth 2.0 token URL is empty"));
    }
    // Built in a block: the serializer is not Send and must not live across an await.
    let (body, headers) = {
        let mut form = url::form_urlencoded::Serializer::new(String::new());
        for (k, v) in fields {
            form.append_pair(k, v);
        }
        if !config.scope.trim().is_empty() {
            form.append_pair("scope", config.scope.trim());
        }
        if !config.audience.trim().is_empty() {
            form.append_pair("audience", config.audience.trim());
        }
        let mut headers = vec![
            Header::new("Content-Type", "application/x-www-form-urlencoded"),
            Header::new("Accept", "application/json"),
        ];
        match config.client_auth {
            ClientAuthMethod::BasicHeader => {
                let enc = |s: &str| url::form_urlencoded::byte_serialize(s.as_bytes()).collect::<String>();
                let creds = base64::engine::general_purpose::STANDARD.encode(format!(
                    "{}:{}",
                    enc(&config.client_id),
                    enc(&config.client_secret)
                ));
                headers.push(Header::new("Authorization", format!("Basic {creds}")));
            }
            ClientAuthMethod::Body => {
                form.append_pair("client_id", &config.client_id);
                if !config.client_secret.is_empty() {
                    form.append_pair("client_secret", &config.client_secret);
                }
            }
        }
        (form.finish(), headers)
    };
    let req = HttpRequest { method: "POST".into(), url: config.token_url.clone(), headers, body: Bytes::from(body) };
    let resp = client.send(req, opts, None).await?;
    let text = String::from_utf8_lossy(&resp.body).into_owned();
    parse_token_response(resp.meta.status, &text)
}

fn parse_token_response(status: u16, text: &str) -> Result<TokenSet> {
    let json: serde_json::Value = serde_json::from_str(text)
        .or_else(|_| {
            // Some providers (old GitHub) answer form-encoded.
            let map: serde_json::Map<String, serde_json::Value> = url::form_urlencoded::parse(text.as_bytes())
                .map(|(k, v)| (k.into_owned(), serde_json::Value::String(v.into_owned())))
                .collect();
            if map.contains_key("access_token") || map.contains_key("error") {
                Ok(serde_json::Value::Object(map))
            } else {
                Err(())
            }
        })
        .map_err(|_| auth_err(format!("Token endpoint returned {status} with a non-JSON body: {}", snippet(text))))?;
    if let Some(error) = json.get("error").and_then(|e| e.as_str()) {
        let desc = json.get("error_description").and_then(|d| d.as_str()).unwrap_or_default();
        return Err(auth_err(format!("Token request failed ({status}): {error} {desc}").trim().to_string()));
    }
    if !(200..300).contains(&status) {
        return Err(auth_err(format!("Token request failed with status {status}: {}", snippet(text))));
    }
    let access_token = json
        .get("access_token")
        .and_then(|t| t.as_str())
        .filter(|t| !t.is_empty())
        .ok_or_else(|| auth_err("Token response has no access_token"))?
        .to_string();
    let expires_in = json.get("expires_in").and_then(|v| v.as_i64().or_else(|| v.as_str()?.parse().ok()));
    let now = now_ms();
    Ok(TokenSet {
        access_token,
        token_type: json.get("token_type").and_then(|t| t.as_str()).unwrap_or("Bearer").to_string(),
        expires_at: expires_in.map(|s| now.saturating_add(s.saturating_mul(1000))),
        refresh_token: json.get("refresh_token").and_then(|t| t.as_str()).map(str::to_string),
        scope: json.get("scope").and_then(|t| t.as_str()).map(str::to_string),
        obtained_at: now,
    })
}

fn snippet(text: &str) -> String {
    let s: String = text.chars().take(300).collect();
    if s.len() < text.len() { format!("{s}…") } else { s }
}

/// PKCE verifier and S256 challenge.
pub fn pkce_pair() -> (String, String) {
    let bytes: [u8; 32] = rand::random();
    let verifier = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sha2::Sha256::digest(verifier.as_bytes()));
    (verifier, challenge)
}

/// Build the authorization URL for the browser.
pub fn authorization_url(config: &OAuth2Config, state: &str, challenge: Option<&str>) -> Result<String> {
    let mut url = url::Url::parse(config.auth_url.trim())
        .map_err(|e| auth_err(format!("Invalid authorization URL '{}': {e}", config.auth_url)))?;
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("response_type", if config.grant_type == GrantType::Implicit { "token" } else { "code" });
        q.append_pair("client_id", &config.client_id);
        q.append_pair("redirect_uri", &config.redirect_uri);
        q.append_pair("state", state);
        if !config.scope.trim().is_empty() {
            q.append_pair("scope", config.scope.trim());
        }
        if !config.audience.trim().is_empty() {
            q.append_pair("audience", config.audience.trim());
        }
        if let Some(c) = challenge {
            q.append_pair("code_challenge", c);
            q.append_pair("code_challenge_method", "S256");
        }
    }
    Ok(url.to_string())
}

/// Run the authorization code flow: listen on the loopback redirect URI, let
/// `open_browser` show the consent page, exchange the code for a token.
pub async fn authorization_code_flow(
    client: &Client,
    opts: &RequestOptions,
    config: &OAuth2Config,
    cache: &TokenCache,
    workspace: &str,
    open_browser: impl FnOnce(&str) -> std::result::Result<(), String>,
    timeout: Duration,
) -> Result<TokenSet> {
    let implicit = config.grant_type == GrantType::Implicit;
    if !implicit && config.grant_type != GrantType::AuthorizationCode {
        let token = request_token(client, opts, config, &[]).await?;
        cache.put(&cache_key(workspace, config), token.clone());
        return Ok(token);
    }
    let redirect = url::Url::parse(&config.redirect_uri)
        .map_err(|e| auth_err(format!("Invalid redirect URI '{}': {e}", config.redirect_uri)))?;
    let host = redirect.host_str().unwrap_or_default();
    if redirect.scheme() != "http" || !matches!(host, "127.0.0.1" | "localhost" | "[::1]") {
        return Err(auth_err("Redirect URI must be a loopback address such as http://127.0.0.1:53682/callback"));
    }
    let port = redirect.port().ok_or_else(|| auth_err("Redirect URI must include a port"))?;
    let bind_host = if host == "[::1]" { "::1" } else { "127.0.0.1" };
    let listener = tokio::net::TcpListener::bind((bind_host, port))
        .await
        .map_err(|e| auth_err(format!("Could not listen on {bind_host}:{port} for the OAuth redirect: {e}")))?;

    let state = uuid::Uuid::new_v4().simple().to_string();
    let (verifier, challenge) = pkce_pair();
    let url = authorization_url(config, &state, (config.pkce && !implicit).then_some(challenge.as_str()))?;
    open_browser(&url).map_err(|e| auth_err(format!("Could not open the browser: {e}")))?;

    let expected_path = redirect.path().to_string();
    let want = if implicit { "access_token" } else { "code" };
    let params = tokio::time::timeout(timeout, wait_for_redirect(&listener, &expected_path, &state, want))
        .await
        .map_err(|_| auth_err("Timed out waiting for the sign-in to finish in the browser"))??;
    if implicit {
        let token = implicit_token(&params)?;
        cache.put(&cache_key(workspace, config), token.clone());
        return Ok(token);
    }
    let code = params.get("code").cloned().unwrap_or_default();

    let mut extra = vec![("code", code.as_str()), ("redirect_uri", config.redirect_uri.as_str())];
    if config.pkce {
        extra.push(("code_verifier", verifier.as_str()));
    }
    let token = request_token(client, opts, config, &extra).await?;
    cache.put(&cache_key(workspace, config), token.clone());
    Ok(token)
}

/// How long one connection to the redirect listener may take to send its request.
const REDIRECT_READ_TIMEOUT: Duration = Duration::from_secs(3);

/// The token the implicit grant's redirect carried (in its `#fragment`).
fn implicit_token(params: &HashMap<String, String>) -> Result<TokenSet> {
    let access_token = params
        .get("access_token")
        .filter(|t| !t.is_empty())
        .cloned()
        .ok_or_else(|| auth_err("The redirect did not include an access token"))?;
    let now = now_ms();
    let expires_in = params.get("expires_in").and_then(|v| v.parse::<i64>().ok());
    Ok(TokenSet {
        access_token,
        token_type: params.get("token_type").cloned().unwrap_or_else(|| "Bearer".into()),
        expires_at: expires_in.map(|s| now.saturating_add(s.saturating_mul(1000))),
        refresh_token: None,
        scope: params.get("scope").cloned(),
        obtained_at: now,
    })
}

/// Waits for the provider's redirect and returns its parameters once it belongs to this
/// sign-in and carries `want` (`code`, or `access_token` for the implicit grant). The
/// implicit grant puts them in the `#fragment`, which browsers never send: a redirect
/// without parameters gets a page that sends the fragment back as a query.
async fn wait_for_redirect(
    listener: &tokio::net::TcpListener,
    path: &str,
    state: &str,
    want: &str,
) -> Result<HashMap<String, String>> {
    loop {
        let (mut sock, _) = listener.accept().await.map_err(|e| auth_err(format!("Redirect listener failed: {e}")))?;
        let mut buf = vec![0u8; 8192];
        // Browsers may open a speculative connection and never use it; don't
        // let an idle socket keep the real redirect waiting in the backlog.
        let Ok(read) = tokio::time::timeout(REDIRECT_READ_TIMEOUT, sock.read(&mut buf)).await else { continue };
        let n = read.unwrap_or(0);
        let head = String::from_utf8_lossy(&buf[..n]);
        let target = head.lines().next().and_then(|l| l.split_whitespace().nth(1)).unwrap_or("/").to_string();
        let url = url::Url::parse(&format!("http://localhost{target}")).ok();
        let Some(url) = url.filter(|u| u.path() == path) else {
            let _ = sock.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
            continue;
        };
        let params: HashMap<String, String> = url.query_pairs().into_owned().collect();
        if want == "access_token" && params.is_empty() {
            let _ = sock.write_all(&fragment_page()).await;
            continue;
        }
        // Not from this sign-in (any web page can make the browser call this port): answer
        // it, but keep waiting, so it can neither complete nor cancel the sign-in.
        if params.get("state").map(String::as_str) != Some(state) {
            let _ = sock.write_all(&redirect_page(false, "This link does not belong to the current sign-in.")).await;
            continue;
        }
        let (ok, message, result) = if let Some(error) = params.get("error") {
            let desc = params.get("error_description").cloned().unwrap_or_default();
            (
                false,
                format!("Sign-in failed: {error} {desc}"),
                Err(auth_err(format!("Authorization failed: {error} {desc}"))),
            )
        } else if params.get(want).is_some_and(|v| !v.is_empty()) {
            (true, "Signed in. You can close this tab and return to Zorvik.".to_string(), Ok(params))
        } else if want == "code" {
            (
                false,
                "Sign-in failed: no code".to_string(),
                Err(auth_err("Redirect did not include an authorization code")),
            )
        } else {
            (
                false,
                "Sign-in failed: no token".to_string(),
                Err(auth_err("The redirect did not include an access token")),
            )
        };
        let _ = sock.write_all(&redirect_page(ok, &message)).await;
        let _ = sock.shutdown().await;
        return result;
    }
}

/// Sends the implicit grant's `#fragment` back to the listener as a query string.
fn fragment_page() -> Vec<u8> {
    let html = "<!doctype html><meta charset=utf-8><title>Zorvik</title><body style=\"font-family:system-ui;padding:3em;text-align:center\"><p>Finishing sign-in…</p><script>var h=location.hash.slice(1);if(h){location.replace(location.pathname+'?'+h)}else{document.body.textContent='Sign-in failed: the redirect has no token.'}</script></body>";
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{html}",
        html.len()
    )
    .into_bytes()
}

/// The page the browser shows after the redirect.
fn redirect_page(ok: bool, message: &str) -> Vec<u8> {
    let html = format!(
        "<!doctype html><meta charset=utf-8><title>Zorvik</title><body style=\"font-family:system-ui;padding:3em;text-align:center\"><h2>{}</h2><p>{}</p></body>",
        if ok { "✓ Done" } else { "Something went wrong" },
        html_escape(message)
    );
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{html}",
        html.len()
    )
    .into_bytes()
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_token_responses() {
        let t = parse_token_response(200, r#"{"access_token":"a","expires_in":"60","refresh_token":"r"}"#).unwrap();
        assert_eq!(t.access_token, "a");
        assert!(t.expires_at.unwrap() > now_ms());
        assert_eq!(t.token_type, "Bearer");
        let t = parse_token_response(200, "access_token=x&token_type=bearer").unwrap();
        assert_eq!(t.access_token, "x");
        let e =
            parse_token_response(400, r#"{"error":"invalid_client","error_description":"bad secret"}"#).unwrap_err();
        assert!(e.message.contains("invalid_client") && e.message.contains("bad secret"));
        assert!(parse_token_response(500, "<html>").is_err());
        assert!(parse_token_response(200, "{}").is_err());
    }

    #[test]
    fn huge_or_negative_expiry_does_not_overflow() {
        let t = parse_token_response(200, r#"{"access_token":"a","expires_in":9223372036854775807}"#).unwrap();
        assert!(t.is_fresh());
        let t = parse_token_response(200, r#"{"access_token":"a","expires_in":-9223372036854775807}"#).unwrap();
        assert!(!t.is_fresh());
    }

    #[test]
    fn freshness_and_keys() {
        let mut t = TokenSet {
            access_token: "a".into(),
            token_type: "Bearer".into(),
            expires_at: Some(now_ms() + 10_000),
            refresh_token: None,
            scope: None,
            obtained_at: 0,
        };
        assert!(!t.is_fresh());
        t.expires_at = None;
        assert!(t.is_fresh());
        let a = OAuth2Config { client_id: "a".into(), ..Default::default() };
        let b = OAuth2Config { client_id: "b".into(), ..Default::default() };
        assert_ne!(cache_key("w", &a), cache_key("w", &b));
        assert_eq!(cache_key("w", &a), cache_key("w", &a.clone()));
        // Same client and user, different credentials: tokens must not be shared.
        let secret = OAuth2Config { client_secret: "s1".into(), ..a.clone() };
        assert_ne!(cache_key("w", &secret), cache_key("w", &OAuth2Config { client_secret: "s2".into(), ..a.clone() }));
        let pw = OAuth2Config { username: "u".into(), password: "p1".into(), ..a.clone() };
        assert_ne!(cache_key("w", &pw), cache_key("w", &OAuth2Config { password: "p2".into(), ..pw.clone() }));
        // Another workspace with an identical config never shares the token.
        assert_ne!(cache_key("w", &a), cache_key("other", &a));
    }

    #[test]
    fn pkce_and_auth_url() {
        let (v, c) = pkce_pair();
        assert!(v.len() >= 43 && c.len() == 43);
        let cfg = OAuth2Config {
            auth_url: "https://id.test/authorize?x=1".into(),
            client_id: "cid".into(),
            scope: "read write".into(),
            ..Default::default()
        };
        let url = authorization_url(&cfg, "st", Some(&c)).unwrap();
        assert!(url.starts_with("https://id.test/authorize?x=1&response_type=code&client_id=cid"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("scope=read+write"));
    }
}

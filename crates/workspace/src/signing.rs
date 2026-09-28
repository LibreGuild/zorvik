//! Auth that is computed for each send: signatures over the final request (AWS SigV4,
//! OAuth 1.0a, Hawk, Akamai EdgeGrid, JWT, ASAP) and answers to the server's challenge
//! (Digest, NTLM). The algorithms live in [`crate::auth`]; this module renders the saved
//! settings (variables substituted) and applies them.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use zorvik_engine::{ChallengeAuth, ChallengeAuthRef, Header, HttpRequest};
use zorvik_formats::{
    ApiKeyLocation, AsapConfig, Auth, AwsSigV4Config, EdgeGridConfig, HawkAlgorithm, HawkConfig, JwtAlgorithm,
    JwtConfig, OAuth1Config, OAuth1Method,
};

use crate::auth::{self, AuthError};
use crate::error::{Error, ErrorCode, Result};

/// Credentials that answer a 401 challenge while the request is sent.
#[derive(Clone, PartialEq, Eq)]
pub enum Challenge {
    Digest { username: String, password: String },
    Ntlm { username: String, password: String, domain: String, workstation: String },
}

impl std::fmt::Debug for Challenge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Digest { .. } => "Challenge::Digest",
            Self::Ntlm { .. } => "Challenge::Ntlm",
        })
    }
}

/// A signing auth with its variables substituted.
#[derive(Clone)]
pub enum Signer {
    AwsSigV4(AwsSigV4Config),
    OAuth1(OAuth1Config),
    Jwt(JwtConfig),
    Hawk(HawkConfig),
    EdgeGrid(EdgeGridConfig),
    Asap(AsapConfig),
}

/// Load tests can't use it: Digest and NTLM answer a challenge on the connection that got it.
pub const LOAD_TEST_CHALLENGE: &str =
    "Digest and NTLM auth can't be load tested: use another auth for the load test (for example a Bearer token)";

/// Signs each send differently (timestamps, nonces): load tests render it for every request.
pub fn signs_each_send(auth: &Auth) -> bool {
    matches!(
        auth,
        Auth::AwsSigV4(_) | Auth::OAuth1(_) | Auth::Jwt(_) | Auth::Hawk(_) | Auth::EdgeGrid(_) | Auth::Asap(_)
    )
}

pub enum Prepared {
    Challenge(Challenge),
    Signer(Signer),
}

/// The per-send part of `auth`, with variables substituted; `None` for the other kinds.
pub fn prepare(auth: &Auth, render: &mut impl FnMut(&str) -> String) -> Option<Prepared> {
    let mut r = |s: &String| render(s);
    Some(match auth {
        Auth::Digest { username, password } => {
            Prepared::Challenge(Challenge::Digest { username: r(username), password: r(password) })
        }
        Auth::Ntlm { username, password, domain, workstation } => Prepared::Challenge(Challenge::Ntlm {
            username: r(username),
            password: r(password),
            domain: r(domain),
            workstation: r(workstation),
        }),
        Auth::AwsSigV4(c) => Prepared::Signer(Signer::AwsSigV4(AwsSigV4Config {
            access_key: r(&c.access_key).trim().to_string(),
            secret_key: r(&c.secret_key).trim().to_string(),
            session_token: r(&c.session_token).trim().to_string(),
            region: r(&c.region).trim().to_string(),
            service: r(&c.service).trim().to_string(),
            location: c.location,
        })),
        Auth::OAuth1(c) => Prepared::Signer(Signer::OAuth1(OAuth1Config {
            consumer_key: r(&c.consumer_key),
            consumer_secret: r(&c.consumer_secret),
            token: r(&c.token),
            token_secret: r(&c.token_secret),
            private_key: r(&c.private_key),
            callback: r(&c.callback),
            verifier: r(&c.verifier),
            realm: r(&c.realm),
            ..c.clone()
        })),
        Auth::Jwt(c) => Prepared::Signer(Signer::Jwt(JwtConfig {
            secret: r(&c.secret),
            payload: r(&c.payload),
            header: r(&c.header),
            prefix: r(&c.prefix),
            query_param: r(&c.query_param),
            ..c.clone()
        })),
        Auth::Hawk(c) => Prepared::Signer(Signer::Hawk(HawkConfig {
            id: r(&c.id),
            key: r(&c.key),
            ext: r(&c.ext),
            app: r(&c.app),
            dlg: r(&c.dlg),
            ..c.clone()
        })),
        Auth::EdgeGrid(c) => Prepared::Signer(Signer::EdgeGrid(EdgeGridConfig {
            client_token: r(&c.client_token).trim().to_string(),
            client_secret: r(&c.client_secret).trim().to_string(),
            access_token: r(&c.access_token).trim().to_string(),
            headers_to_sign: r(&c.headers_to_sign),
            ..c.clone()
        })),
        Auth::Asap(c) => Prepared::Signer(Signer::Asap(AsapConfig {
            issuer: r(&c.issuer),
            subject: r(&c.subject),
            audience: r(&c.audience),
            key_id: r(&c.key_id),
            private_key: r(&c.private_key),
            claims: r(&c.claims),
            ..c.clone()
        })),
        Auth::Inherit
        | Auth::None
        | Auth::Basic { .. }
        | Auth::Bearer { .. }
        | Auth::ApiKey { .. }
        | Auth::OAuth2(_) => {
            return None;
        }
    })
}

impl Challenge {
    /// The engine's side: it answers the server's 401 on the same connection.
    pub fn engine_auth(&self) -> ChallengeAuthRef {
        ChallengeAuthRef(match self.clone() {
            Self::Digest { username, password } => Arc::new(DigestAuth { username, password, nc: AtomicU32::new(0) }),
            Self::Ntlm { username, password, domain, workstation } => {
                Arc::new(NtlmAuth { username, password, domain, workstation })
            }
        })
    }
}

struct DigestAuth {
    username: String,
    password: String,
    /// Requests sent with the server's nonce (`nc`).
    nc: AtomicU32,
}

impl ChallengeAuth for DigestAuth {
    fn initial(&self) -> Option<String> {
        None
    }

    fn answer(
        &self,
        challenges: &[String],
        method: &str,
        target: &str,
        body: &[u8],
    ) -> std::result::Result<Option<String>, String> {
        let values: Vec<&str> = challenges.iter().map(String::as_str).collect();
        let parsed = auth::parse_challenges(&values);
        if !parsed.iter().any(|c| c.is("Digest")) {
            return Ok(None);
        }
        let challenge = auth::digest::DigestChallenge::select(&parsed).map_err(|e| e.to_string())?;
        let creds = auth::digest::DigestCredentials { username: &self.username, password: &self.password };
        let nc = self.nc.fetch_add(1, Ordering::Relaxed) + 1;
        auth::digest::authorization(&challenge, &creds, method, target, body, &hex_nonce(8), nc)
            .map(Some)
            .map_err(|e| e.to_string())
    }
}

struct NtlmAuth {
    username: String,
    password: String,
    domain: String,
    workstation: String,
}

impl ChallengeAuth for NtlmAuth {
    fn initial(&self) -> Option<String> {
        Some(format!("NTLM {}", auth::ntlm::negotiate_message()))
    }

    fn answer(
        &self,
        challenges: &[String],
        _method: &str,
        _target: &str,
        _body: &[u8],
    ) -> std::result::Result<Option<String>, String> {
        let values: Vec<&str> = challenges.iter().map(String::as_str).collect();
        let Some(token) = auth::parse_challenges(&values).into_iter().find(|c| c.is("NTLM")).and_then(|c| c.token)
        else {
            return Ok(None);
        };
        let challenge = auth::ntlm::parse_challenge(&token).map_err(|e| e.to_string())?;
        let message = auth::ntlm::authenticate_message(
            &challenge,
            &self.username,
            &self.password,
            &self.domain,
            &self.workstation,
            rand::random(),
            None,
        )
        .map_err(|e| e.to_string())?;
        Ok(Some(format!("NTLM {message}")))
    }

    fn connection_bound(&self) -> bool {
        true
    }
}

/// `2n` random hex characters (nonces).
fn hex_nonce(n: usize) -> String {
    (0..n).map(|_| format!("{:02x}", rand::random::<u8>())).collect()
}

fn now() -> time::OffsetDateTime {
    time::OffsetDateTime::now_utc()
}

fn header<'a>(request: &'a HttpRequest, name: &str) -> Option<&'a str> {
    request.headers.iter().find(|h| h.name.eq_ignore_ascii_case(name)).map(|h| h.value.as_str())
}

/// Sets `name` (replacing a header of that name).
fn set_header(request: &mut HttpRequest, name: &str, value: String) {
    request.headers.retain(|h| !h.name.eq_ignore_ascii_case(name));
    request.headers.push(Header::new(name, value));
}

fn pairs(request: &HttpRequest) -> Vec<(String, String)> {
    request.headers.iter().map(|h| (h.name.clone(), h.value.clone())).collect()
}

fn json_object(text: &str, what: &str) -> Result<serde_json::Map<String, serde_json::Value>> {
    if text.trim().is_empty() {
        return Ok(serde_json::Map::new());
    }
    match serde_json::from_str::<serde_json::Value>(text) {
        Ok(serde_json::Value::Object(map)) => Ok(map),
        Ok(_) => Err(Error::new(ErrorCode::Auth, format!("{what} must be a JSON object ({{ … }})"))),
        Err(e) => Err(Error::new(ErrorCode::Auth, format!("{what} isn't valid JSON: {e}"))),
    }
}

fn jwt_algorithm(algorithm: JwtAlgorithm) -> std::result::Result<auth::jwt::JwtAlgorithm, AuthError> {
    let name = serde_json::to_value(algorithm).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
    auth::jwt::JwtAlgorithm::parse(&name).ok_or_else(|| AuthError::Input(format!("Unknown JWT algorithm {name}")))
}

/// Signs `request` in place (headers or query string).
pub fn sign(request: &mut HttpRequest, signer: &Signer) -> Result<()> {
    match signer {
        Signer::AwsSigV4(c) => {
            let placement = match c.location {
                ApiKeyLocation::Header => auth::sigv4::Placement::Headers,
                ApiKeyLocation::Query => auth::sigv4::Placement::Query { expires_in: 3600 },
            };
            let config = auth::sigv4::SigV4 {
                access_key: &c.access_key,
                secret_key: &c.secret_key,
                session_token: Some(c.session_token.as_str()).filter(|t| !t.is_empty()),
                region: &c.region,
                service: &c.service,
                now: now(),
                placement,
            };
            let signed = auth::sigv4::sign(&config, &request.method, &request.url, &pairs(request), &request.body)?;
            request.url = signed.url;
            for (name, value) in signed.headers {
                set_header(request, &name, value);
            }
        }
        Signer::OAuth1(c) => {
            let method = match c.signature_method {
                OAuth1Method::HmacSha1 => auth::oauth1::SignatureMethod::HmacSha1,
                OAuth1Method::HmacSha256 => auth::oauth1::SignatureMethod::HmacSha256,
                OAuth1Method::HmacSha512 => auth::oauth1::SignatureMethod::HmacSha512,
                OAuth1Method::RsaSha1 => auth::oauth1::SignatureMethod::RsaSha1,
                OAuth1Method::RsaSha256 => auth::oauth1::SignatureMethod::RsaSha256,
                OAuth1Method::RsaSha512 => auth::oauth1::SignatureMethod::RsaSha512,
                OAuth1Method::Plaintext => auth::oauth1::SignatureMethod::Plaintext,
            };
            let nonce = hex_nonce(16);
            let config = auth::oauth1::OAuth1 {
                consumer_key: &c.consumer_key,
                consumer_secret: &c.consumer_secret,
                token: &c.token,
                token_secret: &c.token_secret,
                private_key: &c.private_key,
                method,
                callback: &c.callback,
                verifier: &c.verifier,
                realm: &c.realm,
                include_version: c.include_version,
                include_body_hash: c.include_body_hash,
                timestamp: now().unix_timestamp().max(0) as u64,
                nonce: &nonce,
                placement: match c.location {
                    ApiKeyLocation::Header => auth::oauth1::Placement::Header,
                    ApiKeyLocation::Query => auth::oauth1::Placement::Query,
                },
            };
            let content_type = header(request, "content-type").map(str::to_string);
            let signed =
                auth::oauth1::sign(&config, &request.method, &request.url, content_type.as_deref(), &request.body)?;
            request.url = signed.url;
            if let Some(value) = signed.authorization {
                set_header(request, "Authorization", value);
            }
        }
        Signer::Jwt(c) => {
            let claims = serde_json::Value::Object(json_object(&c.payload, "The JWT payload")?);
            let extra = json_object(&c.header, "The JWT header")?;
            let token = auth::jwt::sign(jwt_algorithm(c.algorithm)?, &c.secret, c.secret_base64, &extra, &claims)?;
            match c.location {
                ApiKeyLocation::Header => {
                    let prefix = c.prefix.trim();
                    set_header(
                        request,
                        "Authorization",
                        if prefix.is_empty() { token } else { format!("{prefix} {token}") },
                    );
                }
                ApiKeyLocation::Query => request.url = with_query(&request.url, c.query_param.trim(), &token),
            }
        }
        Signer::Hawk(c) => {
            let algorithm = match c.algorithm {
                HawkAlgorithm::Sha256 => auth::hawk::HawkAlgorithm::Sha256,
                HawkAlgorithm::Sha1 => auth::hawk::HawkAlgorithm::Sha1,
            };
            let nonce = hex_nonce(6);
            let config = auth::hawk::Hawk {
                id: &c.id,
                key: &c.key,
                algorithm,
                timestamp: now().unix_timestamp().max(0) as u64,
                nonce: &nonce,
                ext: &c.ext,
                app: &c.app,
                dlg: &c.dlg,
            };
            let content_type = header(request, "content-type").unwrap_or_default().to_string();
            let payload = c.include_payload_hash.then_some((content_type.as_str(), request.body.as_ref()));
            let value = auth::hawk::authorization(&config, &request.method, &request.url, payload)?;
            set_header(request, "Authorization", value);
        }
        Signer::EdgeGrid(c) => {
            let names: Vec<String> =
                c.headers_to_sign.split(',').map(|n| n.trim().to_string()).filter(|n| !n.is_empty()).collect();
            let timestamp = auth::edgegrid::timestamp(now());
            let nonce = uuid::Uuid::new_v4().to_string();
            let config = auth::edgegrid::EdgeGrid {
                client_token: &c.client_token,
                client_secret: &c.client_secret,
                access_token: &c.access_token,
                headers_to_sign: &names,
                max_body: usize::try_from(c.max_body).unwrap_or(usize::MAX),
                timestamp: &timestamp,
                nonce: &nonce,
            };
            let value =
                auth::edgegrid::authorization(&config, &request.method, &request.url, &pairs(request), &request.body)?;
            set_header(request, "Authorization", value);
        }
        Signer::Asap(c) => {
            let extra = json_object(&c.claims, "The extra claims")?;
            let jti = uuid::Uuid::new_v4().to_string();
            let config = auth::asap::Asap {
                algorithm: jwt_algorithm(c.algorithm)?,
                key_id: &c.key_id,
                private_key: &c.private_key,
                issuer: &c.issuer,
                subject: &c.subject,
                audience: &c.audience,
                issued_at: now().unix_timestamp(),
                expires_in: u64::from(c.expires_in),
                jti: &jti,
                extra_claims: Some(&extra).filter(|m| !m.is_empty()),
            };
            let token = auth::asap::token(&config)?;
            set_header(request, "Authorization", format!("Bearer {token}"));
        }
    }
    Ok(())
}

/// `url` with `name=value` added to its query string (URL-encoded).
fn with_query(url: &str, name: &str, value: &str) -> String {
    let (base, fragment) = url.split_once('#').map_or((url, None), |(b, f)| (b, Some(f)));
    let encode = |s: &str| url::form_urlencoded::byte_serialize(s.as_bytes()).collect::<String>();
    let sep = if base.contains('?') { if base.ends_with('?') || base.ends_with('&') { "" } else { "&" } } else { "?" };
    let mut out = format!("{base}{sep}{}={}", encode(name), encode(value));
    if let Some(f) = fragment {
        out.push('#');
        out.push_str(f);
    }
    out
}

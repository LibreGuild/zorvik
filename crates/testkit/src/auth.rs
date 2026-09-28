//! Digest (httpbin-compatible) and NTLMv2 endpoints. The checks are written from the
//! RFCs and MS-NLMP, independently of the client in `zorvik-workspace`.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use axum::Extension;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, Method, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use base64::Engine as _;
use hmac::{KeyInit as _, Mac as _};
use md5::Digest as _;
use serde_json::json;

use crate::AppState;

pub(crate) const DIGEST_REALM: &str = "zorvik@test";

/// Identifies the TCP connection a request came on: NTLM authenticates connections.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConnectionId(pub u64);

/// Nonces and NTLM challenges from the counter and the clock (unique, not secret).
fn fresh_bytes(state: &AppState) -> [u8; 32] {
    let n = state.counter.fetch_add(1, Ordering::SeqCst);
    let now = time::OffsetDateTime::now_utc().unix_timestamp_nanos();
    sha2::Sha256::digest(format!("{n}:{now}").as_bytes()).into()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn authorization<'a>(headers: &'a HeaderMap, scheme: &str) -> Option<&'a str> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (given, rest) = value.split_once(' ')?;
    given.eq_ignore_ascii_case(scheme).then(|| rest.trim())
}

// ---------------------------------------------------------------- Digest

/// `/digest-auth/{qop}/{user}/{passwd}[/{algorithm}]`: 401 with a fresh challenge
/// until the `Authorization` header answers one of this server's nonces.
pub(crate) async fn digest(
    State(state): State<Arc<AppState>>,
    Path(p): Path<HashMap<String, String>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let (qop, user, passwd) = (&p["qop"], &p["user"], &p["passwd"]);
    let algorithm = p.get("algorithm").map(String::as_str).unwrap_or("MD5");
    if !["MD5", "MD5-sess", "SHA-256", "SHA-256-sess"].iter().any(|a| a.eq_ignore_ascii_case(algorithm)) {
        return (StatusCode::BAD_REQUEST, "algorithm must be MD5, MD5-sess, SHA-256 or SHA-256-sess").into_response();
    }
    if let Some(value) = authorization(&headers, "Digest") {
        let params = digest_params(value);
        let issued = params.get("nonce").is_some_and(|n| state.digest_nonces.lock().unwrap().contains(n));
        let request_uri = uri.path_and_query().map(|p| p.as_str()).unwrap_or("/");
        if issued && digest_matches(&params, method.as_str(), request_uri, &body, qop, user, passwd, algorithm) {
            return axum::Json(json!({ "authenticated": true, "user": user })).into_response();
        }
    }
    let nonce = hex(&fresh_bytes(&state)[..16]);
    {
        let mut nonces = state.digest_nonces.lock().unwrap();
        if nonces.len() > 10_000 {
            nonces.clear();
        }
        nonces.insert(nonce.clone());
    }
    let opaque = hex(&fresh_bytes(&state)[..16]);
    let challenge = format!(
        "Digest realm=\"{DIGEST_REALM}\", nonce=\"{nonce}\", qop=\"{qop}\", opaque=\"{opaque}\", algorithm={algorithm}, stale=FALSE"
    );
    (StatusCode::UNAUTHORIZED, [(header::WWW_AUTHENTICATE, challenge)]).into_response()
}

/// `name=value` pairs of a Digest `Authorization` header (values unquoted).
fn digest_params(value: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut rest = value;
    while !rest.is_empty() {
        let Some((name, after)) = rest.split_once('=') else { break };
        let name = name.trim().trim_start_matches(',').trim().to_ascii_lowercase();
        let after = after.trim_start();
        let (val, remaining) = if let Some(quoted) = after.strip_prefix('"') {
            let mut val = String::new();
            let mut chars = quoted.char_indices();
            let mut end = quoted.len();
            while let Some((i, c)) = chars.next() {
                match c {
                    '\\' => val.extend(chars.next().map(|(_, c)| c)),
                    '"' => {
                        end = i + 1;
                        break;
                    }
                    c => val.push(c),
                }
            }
            (val, &quoted[end..])
        } else {
            let end = after.find(',').unwrap_or(after.len());
            (after[..end].trim().to_string(), &after[end..])
        };
        out.insert(name, val);
        rest = remaining.trim_start().trim_start_matches(',');
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn digest_matches(
    params: &HashMap<String, String>,
    method: &str,
    request_uri: &str,
    body: &[u8],
    offered_qop: &str,
    user: &str,
    passwd: &str,
    algorithm: &str,
) -> bool {
    let get = |k: &str| params.get(k).map(String::as_str).unwrap_or_default();
    // username*=UTF-8''percent-encoded (RFC 7616 section 3.4)
    let username = match params.get("username*") {
        Some(ext) => match ext.split_once("''") {
            Some((charset, encoded)) if charset.eq_ignore_ascii_case("UTF-8") => {
                crate::decode(&encoded.replace('+', "%2B"))
            }
            _ => return false,
        },
        None => get("username").to_string(),
    };
    let sha256 = algorithm.to_ascii_uppercase().starts_with("SHA-256");
    let sess = algorithm.to_ascii_lowercase().ends_with("-sess");
    let h = |data: &[u8]| if sha256 { hex(&sha2::Sha256::digest(data)) } else { hex(&md5::Md5::digest(data)) };
    let (nonce, cnonce, nc, qop) = (get("nonce"), get("cnonce"), get("nc"), get("qop"));
    let qop_ok = offered_qop.split(',').any(|q| q.trim() == qop) && !qop.is_empty();
    if username != user
        || get("realm") != DIGEST_REALM
        || get("uri") != request_uri
        || !params.get("algorithm").map_or("MD5", String::as_str).eq_ignore_ascii_case(algorithm)
        || !qop_ok
        || cnonce.is_empty()
        || nc.len() != 8
    {
        return false;
    }
    let mut ha1 = h(format!("{user}:{DIGEST_REALM}:{passwd}").as_bytes());
    if sess {
        ha1 = h(format!("{ha1}:{nonce}:{cnonce}").as_bytes());
    }
    let ha2 = if qop == "auth-int" {
        h(format!("{method}:{request_uri}:{}", h(body)).as_bytes())
    } else {
        h(format!("{method}:{request_uri}").as_bytes())
    };
    get("response") == h(format!("{ha1}:{nonce}:{nc}:{cnonce}:{qop}:{ha2}").as_bytes())
}

// ---------------------------------------------------------------- NTLM

const NTLM_SIGNATURE: &[u8] = b"NTLMSSP\0";

/// `/ntlm/{domain}/{user}/{passwd}`: NEGOTIATE gets a CHALLENGE (remembered for this
/// connection), then an AUTHENTICATE on the same connection with a valid NTLMv2
/// response gets 200. Anything else gets 401.
pub(crate) async fn ntlm(
    State(state): State<Arc<AppState>>,
    Path((domain, user, passwd)): Path<(String, String, String)>,
    connection: Option<Extension<ConnectionId>>,
    headers: HeaderMap,
) -> Response {
    let unauthorized = || (StatusCode::UNAUTHORIZED, [(header::WWW_AUTHENTICATE, "NTLM")]).into_response();
    let connection = connection.map(|Extension(ConnectionId(id))| id);
    let token = authorization(&headers, "NTLM").or_else(|| authorization(&headers, "Negotiate"));
    let Some(message) = token.and_then(|t| base64::engine::general_purpose::STANDARD.decode(t).ok()) else {
        return unauthorized();
    };
    if message.len() < 12 || &message[..8] != NTLM_SIGNATURE {
        return unauthorized();
    }
    match u32::from_le_bytes(message[8..12].try_into().unwrap()) {
        1 => {
            let Some(connection) = connection else { return unauthorized() };
            let mut server_challenge = [0u8; 8];
            server_challenge.copy_from_slice(&fresh_bytes(&state)[..8]);
            {
                let mut challenges = state.ntlm_challenges.lock().unwrap();
                if challenges.len() > 10_000 {
                    challenges.clear();
                }
                challenges.insert(connection, server_challenge);
            }
            let type2 = base64::engine::general_purpose::STANDARD.encode(challenge_message(&domain, server_challenge));
            (StatusCode::UNAUTHORIZED, [(header::WWW_AUTHENTICATE, format!("NTLM {type2}"))]).into_response()
        }
        3 => {
            // The challenge is used once, and only on the connection that got it.
            let challenge = connection.and_then(|c| state.ntlm_challenges.lock().unwrap().remove(&c));
            match challenge {
                Some(challenge) if ntlm_verifies(&message, challenge, &domain, &user, &passwd) => {
                    axum::Json(json!({ "authenticated": true, "user": user, "domain": domain })).into_response()
                }
                _ => unauthorized(),
            }
        }
        _ => unauthorized(),
    }
}

fn utf16(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(|u| u.to_le_bytes()).collect()
}

/// CHALLENGE_MESSAGE (MS-NLMP 2.2.1.2) with NetBIOS names and a timestamp.
fn challenge_message(domain: &str, server_challenge: [u8; 8]) -> Vec<u8> {
    // UNICODE | REQUEST_TARGET | NTLM | ALWAYS_SIGN | TARGET_TYPE_DOMAIN | EXTENDED_SESSIONSECURITY | TARGET_INFO | 128 | 56
    let flags: u32 = 0xa089_8205;
    let target_name = utf16(&domain.to_uppercase());
    let filetime = (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 100 + 116_444_736_000_000_000) as u64;
    let mut info = Vec::new();
    for (id, value) in
        [(2u16, utf16(&domain.to_uppercase())), (1, utf16("ZORVIK-TEST")), (7, filetime.to_le_bytes().to_vec())]
    {
        info.extend_from_slice(&id.to_le_bytes());
        info.extend_from_slice(&(value.len() as u16).to_le_bytes());
        info.extend_from_slice(&value);
    }
    info.extend_from_slice(&[0; 4]); // MsvAvEOL
    let mut m = Vec::new();
    m.extend_from_slice(NTLM_SIGNATURE);
    m.extend_from_slice(&2u32.to_le_bytes());
    let name_offset = 48u32;
    m.extend_from_slice(&(target_name.len() as u16).to_le_bytes());
    m.extend_from_slice(&(target_name.len() as u16).to_le_bytes());
    m.extend_from_slice(&name_offset.to_le_bytes());
    m.extend_from_slice(&flags.to_le_bytes());
    m.extend_from_slice(&server_challenge);
    m.extend_from_slice(&[0; 8]);
    m.extend_from_slice(&(info.len() as u16).to_le_bytes());
    m.extend_from_slice(&(info.len() as u16).to_le_bytes());
    m.extend_from_slice(&(name_offset + target_name.len() as u32).to_le_bytes());
    m.extend_from_slice(&target_name);
    m.extend_from_slice(&info);
    m
}

/// Checks an AUTHENTICATE_MESSAGE (MS-NLMP 2.2.1.3) like a domain controller: the
/// NTProofStr is recomputed from the password and the names in the message.
fn ntlm_verifies(m: &[u8], server_challenge: [u8; 8], domain: &str, user: &str, passwd: &str) -> bool {
    let field = |at: usize| -> Option<&[u8]> {
        let len = u16::from_le_bytes(m.get(at..at + 2)?.try_into().ok()?) as usize;
        let offset = u32::from_le_bytes(m.get(at + 4..at + 8)?.try_into().ok()?) as usize;
        m.get(offset..offset + len)
    };
    let text = |bytes: &[u8]| {
        String::from_utf16_lossy(&bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect::<Vec<_>>())
    };
    let (Some(nt), Some(msg_domain), Some(msg_user)) = (field(20), field(28), field(36)) else { return false };
    let unicode = m.get(60..64).is_some_and(|f| f[0] & 1 == 1);
    if !unicode || nt.len() < 48 || nt[16..18] != [1, 1] {
        return false;
    }
    let (msg_domain, msg_user) = (text(msg_domain), text(msg_user));
    if msg_user.to_uppercase() != user.to_uppercase() || msg_domain.to_uppercase() != domain.to_uppercase() {
        return false;
    }
    let hmac_md5 = |key: &[u8], parts: &[&[u8]]| {
        let mut mac = hmac::Hmac::<md5::Md5>::new_from_slice(key).unwrap();
        parts.iter().for_each(|p| mac.update(p));
        mac.finalize().into_bytes().to_vec()
    };
    let nt_hash = md4::Md4::digest(utf16(passwd));
    let key = hmac_md5(&nt_hash, &[&utf16(&format!("{}{msg_domain}", msg_user.to_uppercase()))]);
    hmac_md5(&key, &[&server_challenge, &nt[16..]]) == nt[..16]
}

#[test]
fn digest_header_parsing() {
    let p = digest_params(r#"username="a \"b\"", realm="r, s", nc=00000001, qop=auth,uri="/x?y=1""#);
    assert_eq!(p["username"], "a \"b\"");
    assert_eq!(p["realm"], "r, s");
    assert_eq!(p["nc"], "00000001");
    assert_eq!(p["qop"], "auth");
    assert_eq!(p["uri"], "/x?y=1");
}

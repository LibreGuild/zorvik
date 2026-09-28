//! HTTP authentication schemes beyond Basic, Bearer and OAuth 2.0: Digest, NTLM,
//! AWS Signature Version 4, OAuth 1.0a, Hawk, Akamai EdgeGrid, JWT and Atlassian ASAP.
//!
//! Every scheme is a pure function of its inputs: the time, nonces and client
//! challenges are parameters, so results are reproducible and testable. The only
//! randomness is inside signatures that need it (RSA-PSS salts, ECDSA nonces).
//! Structs that hold secrets implement `Debug` by hand and never print them, and
//! errors never quote a secret.

pub mod asap;
pub mod digest;
pub mod edgegrid;
pub mod hawk;
pub mod jwt;
mod keys;
pub mod ntlm;
pub mod oauth1;
pub mod sigv4;

use base64::Engine as _;

use crate::error::{Error, ErrorCode};

pub use digest::{Challenge, parse_challenges};

/// Why credentials or a signature couldn't be produced. The messages are for users:
/// they say what is wrong and what to change.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AuthError {
    /// A private key or secret can't be used.
    #[error("{0}")]
    Key(String),
    /// The server's challenge (`WWW-Authenticate`) can't be answered.
    #[error("{0}")]
    Challenge(String),
    /// A setting is missing or has a value the scheme can't use.
    #[error("{0}")]
    Input(String),
}

impl AuthError {
    pub(crate) fn key(message: impl Into<String>) -> Self {
        Self::Key(message.into())
    }

    pub(crate) fn challenge(message: impl Into<String>) -> Self {
        Self::Challenge(message.into())
    }

    pub(crate) fn input(message: impl Into<String>) -> Self {
        Self::Input(message.into())
    }
}

impl From<AuthError> for Error {
    fn from(e: AuthError) -> Self {
        Error::new(ErrorCode::Auth, e.to_string())
    }
}

/// Lowercase hex.
pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub(crate) fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// Percent-encodes everything but the RFC 3986 unreserved characters (and `keep`),
/// with uppercase hex: the encoding AWS, OAuth 1.0a and RFC 5987 values use.
pub(crate) fn percent_encode(input: impl AsRef<[u8]>, keep: &[u8]) -> String {
    let mut out = String::new();
    for &b in input.as_ref() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') || keep.contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Decodes `%XX` escapes (not `+`); malformed escapes are kept as they are.
pub(crate) fn percent_decode(input: &str) -> Vec<u8> {
    let bytes = input.as_bytes();
    let digit = |i: usize| bytes.get(i).and_then(|&c| (c as char).to_digit(16));
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let (Some(high), Some(low)) = (digit(i + 1), digit(i + 2))
        {
            out.push((high * 16 + low) as u8);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

/// An absolute `http(s)` URL; the message never repeats the URL (it may carry secrets).
pub(crate) fn parse_url(url: &str) -> Result<url::Url, AuthError> {
    let parsed = url::Url::parse(url.trim())
        .map_err(|_| AuthError::input("The request URL isn't a valid absolute URL (http://… or https://…)."))?;
    if parsed.host_str().is_none() {
        return Err(AuthError::input("The request URL has no host name."));
    }
    Ok(parsed)
}

/// `host` or `host:port` as a client sends it in the `Host` header (default ports left out).
pub(crate) fn host_header(url: &url::Url) -> String {
    let host = url.host_str().unwrap_or_default();
    match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_string(),
    }
}

/// Escapes `"` and `\` for a quoted-string header parameter.
pub(crate) fn quote(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

pub(crate) fn hmac(algorithm: ring::hmac::Algorithm, key: &[u8], data: &[u8]) -> Vec<u8> {
    ring::hmac::sign(&ring::hmac::Key::new(algorithm, key), data).as_ref().to_vec()
}

pub(crate) fn sha256(data: &[u8]) -> Vec<u8> {
    ring::digest::digest(&ring::digest::SHA256, data).as_ref().to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_encoding_round_trip() {
        assert_eq!(percent_encode("a b/ç~", b""), "a%20b%2F%C3%A7~");
        assert_eq!(percent_encode("a/b", b"/"), "a/b");
        assert_eq!(percent_decode("a%20b%2f%zz%"), b"a b/%zz%");
        assert_eq!(percent_decode("%+1%é"), "%+1%é".as_bytes());
        assert_eq!(percent_decode("%E1%88%B4"), "ሴ".as_bytes());
    }

    #[test]
    fn errors_become_auth_errors() {
        let e: Error = AuthError::input("Needs a key.").into();
        assert_eq!(e.code, ErrorCode::Auth);
        assert_eq!(e.message, "Needs a key.");
    }
}

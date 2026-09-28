//! Atlassian S2S Authentication (ASAP): a short-lived JWT signed with the service's
//! private key, sent as `Authorization: Bearer <token>`.
//! Spec: https://s2sauth.bitbucket.io/spec/

use serde_json::{Map, Value};

use super::AuthError;
use super::jwt::{self, JwtAlgorithm};

/// ASAP tokens live at most an hour.
pub const MAX_EXPIRY_SECS: u64 = 3600;

pub struct Asap<'a> {
    /// Any RS, PS or ES algorithm; RS256 is the usual one.
    pub algorithm: JwtAlgorithm,
    /// The key id (`kid`), which names the public key on the key server.
    pub key_id: &'a str,
    /// PEM private key.
    pub private_key: &'a str,
    pub issuer: &'a str,
    /// Empty when not used.
    pub subject: &'a str,
    /// One audience, or several separated by commas.
    pub audience: &'a str,
    /// Seconds since 1970.
    pub issued_at: i64,
    /// Seconds until `exp`, 1 to 3600.
    pub expires_in: u64,
    /// A unique token id (`jti`), such as a UUID.
    pub jti: &'a str,
    /// More claims; the ASAP claims above win over these.
    pub extra_claims: Option<&'a Map<String, Value>>,
}

impl std::fmt::Debug for Asap<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Asap")
            .field("algorithm", &self.algorithm)
            .field("key_id", &self.key_id)
            .field("private_key", &"<hidden>")
            .field("issuer", &self.issuer)
            .field("audience", &self.audience)
            .field("issued_at", &self.issued_at)
            .field("expires_in", &self.expires_in)
            .finish()
    }
}

/// The signed token.
pub fn token(asap: &Asap) -> Result<String, AuthError> {
    if asap.algorithm.uses_secret() {
        return Err(AuthError::input("ASAP tokens are signed with a private key: use an RS, PS or ES algorithm."));
    }
    for (value, what) in [(asap.key_id, "a key id (kid)"), (asap.issuer, "an issuer"), (asap.jti, "a token id (jti)")] {
        if value.trim().is_empty() {
            return Err(AuthError::input(format!("ASAP needs {what}.")));
        }
    }
    let audience: Vec<&str> = asap.audience.split(',').map(str::trim).filter(|a| !a.is_empty()).collect();
    if audience.is_empty() {
        return Err(AuthError::input("ASAP needs an audience."));
    }
    if !(1..=MAX_EXPIRY_SECS).contains(&asap.expires_in) {
        return Err(AuthError::input("An ASAP token must expire after 1 to 3600 seconds."));
    }

    let mut claims = Map::new();
    claims.insert("iss".into(), Value::from(asap.issuer.trim()));
    if !asap.subject.trim().is_empty() {
        claims.insert("sub".into(), Value::from(asap.subject.trim()));
    }
    claims.insert(
        "aud".into(),
        match audience.as_slice() {
            [one] => Value::from(*one),
            many => Value::from(many.to_vec()),
        },
    );
    claims.insert("iat".into(), Value::from(asap.issued_at));
    claims.insert("exp".into(), Value::from(asap.issued_at.saturating_add(asap.expires_in as i64)));
    claims.insert("jti".into(), Value::from(asap.jti.trim()));
    for (name, value) in asap.extra_claims.into_iter().flatten() {
        if !claims.contains_key(name) {
            claims.insert(name.clone(), value.clone());
        }
    }
    let mut header = Map::new();
    header.insert("kid".into(), Value::from(asap.key_id.trim()));
    jwt::sign(asap.algorithm, asap.private_key, false, &header, &Value::Object(claims))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::keys::tests::{EC_P256_SEC1, RSA_PKCS8, rsa_public_der};
    use base64::Engine as _;
    use serde_json::json;

    fn asap<'a>() -> Asap<'a> {
        Asap {
            algorithm: JwtAlgorithm::Rs256,
            key_id: "zorvik/key-1",
            private_key: RSA_PKCS8,
            issuer: "zorvik",
            subject: "",
            audience: "jira",
            issued_at: 1_700_000_000,
            expires_in: 60,
            jti: "0b6f4a14-7a3e-4b5a-9d49-2d43d1d3c0de",
            extra_claims: None,
        }
    }

    fn decode(token: &str) -> (Value, Value) {
        let part = |i: usize| {
            let bytes =
                base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(token.split('.').nth(i).unwrap()).unwrap();
            serde_json::from_slice::<Value>(&bytes).unwrap()
        };
        (part(0), part(1))
    }

    #[test]
    fn claims_and_header() {
        let extra: Map<String, Value> = serde_json::from_value(json!({"scope": "read", "iss": "ignored"})).unwrap();
        let token = token(&Asap { subject: "user-1", extra_claims: Some(&extra), ..asap() }).unwrap();
        let (header, claims) = decode(&token);
        assert_eq!(header, json!({"alg": "RS256", "typ": "JWT", "kid": "zorvik/key-1"}));
        assert_eq!(
            claims,
            json!({"iss": "zorvik", "sub": "user-1", "aud": "jira", "iat": 1_700_000_000, "exp": 1_700_000_060,
                "jti": "0b6f4a14-7a3e-4b5a-9d49-2d43d1d3c0de", "scope": "read"})
        );
        let (input, sig) = token.rsplit_once('.').unwrap();
        let sig = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(sig).unwrap();
        ring::signature::UnparsedPublicKey::new(
            &ring::signature::RSA_PKCS1_2048_8192_SHA256,
            rsa_public_der(RSA_PKCS8),
        )
        .verify(input.as_bytes(), &sig)
        .unwrap();
    }

    #[test]
    fn several_audiences_and_ec_keys() {
        let token =
            token(&Asap { algorithm: JwtAlgorithm::Es256, private_key: EC_P256_SEC1, audience: "a, b,,", ..asap() })
                .unwrap();
        let (header, claims) = decode(&token);
        assert_eq!(header["alg"], "ES256");
        assert_eq!(claims["aud"], json!(["a", "b"]));
        assert!(claims.get("sub").is_none());
    }

    #[test]
    fn bad_settings() {
        let err = |a: Asap| token(&a).unwrap_err().to_string();
        assert!(err(Asap { algorithm: JwtAlgorithm::Hs256, ..asap() }).contains("private key"));
        assert_eq!(err(Asap { key_id: "", ..asap() }), "ASAP needs a key id (kid).");
        assert_eq!(err(Asap { audience: " , ", ..asap() }), "ASAP needs an audience.");
        assert!(err(Asap { expires_in: 3601, ..asap() }).contains("3600"));
        assert!(err(Asap { expires_in: 0, ..asap() }).contains("3600"));
        assert_eq!(err(Asap { jti: " ", ..asap() }), "ASAP needs a token id (jti).");
        assert!(!format!("{:?}", asap()).contains("BEGIN"));
    }
}

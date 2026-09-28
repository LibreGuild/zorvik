//! JSON Web Tokens (RFC 7519) signed as JWS (RFC 7515, algorithms of RFC 7518):
//! HMAC, RSA PKCS#1 v1.5, RSA-PSS and ECDSA P-256/P-384.

use base64::Engine as _;
use serde_json::{Map, Value};

use super::keys::{self, Curve, RsaScheme};
use super::{AuthError, hmac};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JwtAlgorithm {
    Hs256,
    Hs384,
    Hs512,
    Rs256,
    Rs384,
    Rs512,
    Ps256,
    Ps384,
    Ps512,
    Es256,
    Es384,
}

impl JwtAlgorithm {
    pub const ALL: [Self; 11] = [
        Self::Hs256,
        Self::Hs384,
        Self::Hs512,
        Self::Rs256,
        Self::Rs384,
        Self::Rs512,
        Self::Ps256,
        Self::Ps384,
        Self::Ps512,
        Self::Es256,
        Self::Es384,
    ];

    /// The JWS `alg` value.
    pub fn name(self) -> &'static str {
        match self {
            Self::Hs256 => "HS256",
            Self::Hs384 => "HS384",
            Self::Hs512 => "HS512",
            Self::Rs256 => "RS256",
            Self::Rs384 => "RS384",
            Self::Rs512 => "RS512",
            Self::Ps256 => "PS256",
            Self::Ps384 => "PS384",
            Self::Ps512 => "PS512",
            Self::Es256 => "ES256",
            Self::Es384 => "ES384",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.name().eq_ignore_ascii_case(name.trim()))
    }

    /// HMAC algorithms take a shared secret; the others a PEM private key.
    pub fn uses_secret(self) -> bool {
        matches!(self, Self::Hs256 | Self::Hs384 | Self::Hs512)
    }
}

/// A signed token. `key` is the secret for HS algorithms (base64-decoded first when
/// `secret_is_base64`) or a PEM private key: RSA (PKCS#1 or PKCS#8) for RS/PS, EC
/// (SEC1 or PKCS#8) for ES. The header is `{"alg", "typ": "JWT"}` plus
/// `header_extra` (`kid` and so on; `"typ": null` leaves `typ` out, `alg` is ignored).
pub fn sign(
    alg: JwtAlgorithm,
    key: &str,
    secret_is_base64: bool,
    header_extra: &Map<String, Value>,
    payload: &Value,
) -> Result<String, AuthError> {
    if !payload.is_object() {
        return Err(AuthError::input("The JWT payload must be a JSON object ({ … })."));
    }
    let mut header = Map::new();
    header.insert("alg".into(), Value::from(alg.name()));
    header.insert("typ".into(), Value::from("JWT"));
    for (name, value) in header_extra {
        match (name.as_str(), value) {
            ("alg", _) => {}
            ("typ", Value::Null) => {
                header.shift_remove("typ");
            }
            _ => {
                header.insert(name.clone(), value.clone());
            }
        }
    }
    let input = format!("{}.{}", b64url(&to_json(&Value::Object(header))), b64url(&to_json(payload)));
    let signature = sign_input(alg, key, secret_is_base64, input.as_bytes())?;
    Ok(format!("{input}.{}", b64url(&signature)))
}

/// The JWS signature of a signing input (`header.payload`).
fn sign_input(alg: JwtAlgorithm, key: &str, secret_is_base64: bool, input: &[u8]) -> Result<Vec<u8>, AuthError> {
    let hs = |algorithm| {
        let secret = if secret_is_base64 { decode_secret(key)? } else { key.as_bytes().to_vec() };
        if secret.is_empty() {
            return Err(AuthError::key("The JWT secret is empty."));
        }
        Ok(hmac(algorithm, &secret, input))
    };
    if !alg.uses_secret() && key.trim().is_empty() {
        return Err(AuthError::key(format!("{} needs a private key (PEM).", alg.name())));
    }
    match alg {
        JwtAlgorithm::Hs256 => hs(ring::hmac::HMAC_SHA256),
        JwtAlgorithm::Hs384 => hs(ring::hmac::HMAC_SHA384),
        JwtAlgorithm::Hs512 => hs(ring::hmac::HMAC_SHA512),
        JwtAlgorithm::Rs256 => keys::rsa_sign(key, RsaScheme::Pkcs1Sha256, input),
        JwtAlgorithm::Rs384 => keys::rsa_sign(key, RsaScheme::Pkcs1Sha384, input),
        JwtAlgorithm::Rs512 => keys::rsa_sign(key, RsaScheme::Pkcs1Sha512, input),
        JwtAlgorithm::Ps256 => keys::rsa_sign(key, RsaScheme::PssSha256, input),
        JwtAlgorithm::Ps384 => keys::rsa_sign(key, RsaScheme::PssSha384, input),
        JwtAlgorithm::Ps512 => keys::rsa_sign(key, RsaScheme::PssSha512, input),
        JwtAlgorithm::Es256 => keys::ec_sign(key, Curve::P256, input),
        JwtAlgorithm::Es384 => keys::ec_sign(key, Curve::P384, input),
    }
}

/// Standard or URL-safe base64, with or without padding.
fn decode_secret(text: &str) -> Result<Vec<u8>, AuthError> {
    use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};
    let text = text.trim();
    [STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD]
        .iter()
        .find_map(|engine| engine.decode(text).ok())
        .ok_or_else(|| AuthError::key("The JWT secret isn't valid base64. Fix it, or turn off the base64 option."))
}

fn to_json(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).expect("JSON values always serialize")
}

fn b64url(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::keys::tests::{
        EC_P256_SEC1, EC_P384_PKCS8, RSA_PKCS1, RSA_PKCS8, ec_public_point, rsa_public_der,
    };
    use ring::signature::{self, UnparsedPublicKey, VerificationAlgorithm};
    use serde_json::json;

    fn parts(token: &str) -> (Value, Value, Vec<u8>, String) {
        let pieces: Vec<&str> = token.split('.').collect();
        assert_eq!(pieces.len(), 3);
        let decode = |s: &str| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(s).unwrap();
        (
            serde_json::from_slice(&decode(pieces[0])).unwrap(),
            serde_json::from_slice(&decode(pieces[1])).unwrap(),
            decode(pieces[2]),
            format!("{}.{}", pieces[0], pieces[1]),
        )
    }

    #[test]
    fn jwt_io_example() {
        // The default HS256 token on https://jwt.io
        let payload = json!({"sub": "1234567890", "name": "John Doe", "iat": 1516239022});
        let token = sign(JwtAlgorithm::Hs256, "your-256-bit-secret", false, &Map::new(), &payload).unwrap();
        assert_eq!(
            token,
            "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiaWF0IjoxNTE2MjM5MDIyfQ.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c"
        );
    }

    #[test]
    fn rfc7515_hs256_with_a_base64_key() {
        // RFC 7515 appendix A.1: https://www.rfc-editor.org/rfc/rfc7515#appendix-A.1
        let key = "AyM1SysPpbyDfgZld3umj1qzKObwVMkoqQ-EstJQLr_T-1qS0gZH75aKtMN3Yj0iPS4hcgUuTwjAzZr1Z9CAow";
        let input = "eyJ0eXAiOiJKV1QiLA0KICJhbGciOiJIUzI1NiJ9.eyJpc3MiOiJqb2UiLA0KICJleHAiOjEzMDA4MTkzODAsDQogImh0dHA6Ly9leGFtcGxlLmNvbS9pc19yb290Ijp0cnVlfQ";
        let sig = sign_input(JwtAlgorithm::Hs256, key, true, input.as_bytes()).unwrap();
        assert_eq!(b64url(&sig), "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk");
    }

    #[test]
    fn rsa_and_ec_tokens_verify() {
        let payload = json!({"sub": "zörvik ✓", "n": 1});
        #[rustfmt::skip]
        let cases: [(JwtAlgorithm, &str, Vec<u8>, &'static dyn VerificationAlgorithm); 8] = [
            (JwtAlgorithm::Rs256, RSA_PKCS1, rsa_public_der(RSA_PKCS1), &signature::RSA_PKCS1_2048_8192_SHA256),
            (JwtAlgorithm::Rs384, RSA_PKCS8, rsa_public_der(RSA_PKCS8), &signature::RSA_PKCS1_2048_8192_SHA384),
            (JwtAlgorithm::Rs512, RSA_PKCS1, rsa_public_der(RSA_PKCS1), &signature::RSA_PKCS1_2048_8192_SHA512),
            (JwtAlgorithm::Ps256, RSA_PKCS8, rsa_public_der(RSA_PKCS8), &signature::RSA_PSS_2048_8192_SHA256),
            (JwtAlgorithm::Ps384, RSA_PKCS1, rsa_public_der(RSA_PKCS1), &signature::RSA_PSS_2048_8192_SHA384),
            (JwtAlgorithm::Ps512, RSA_PKCS8, rsa_public_der(RSA_PKCS8), &signature::RSA_PSS_2048_8192_SHA512),
            (JwtAlgorithm::Es256, EC_P256_SEC1, ec_public_point(EC_P256_SEC1, Curve::P256), &signature::ECDSA_P256_SHA256_FIXED),
            (JwtAlgorithm::Es384, EC_P384_PKCS8, ec_public_point(EC_P384_PKCS8, Curve::P384), &signature::ECDSA_P384_SHA384_FIXED),
        ];
        for (alg, key, public, verify) in cases {
            let token = sign(alg, key, false, &Map::new(), &payload).unwrap();
            let (header, claims, sig, input) = parts(&token);
            assert_eq!(header, json!({"alg": alg.name(), "typ": "JWT"}));
            assert_eq!(claims, payload);
            UnparsedPublicKey::new(verify, public).verify(input.as_bytes(), &sig).unwrap_or_else(|_| panic!("{alg:?}"));
        }
    }

    #[test]
    fn header_extras() {
        let extra: Map<String, Value> =
            serde_json::from_value(json!({"kid": "key-1", "alg": "none", "typ": "at+jwt", "x5t": "abc"})).unwrap();
        let token = sign(JwtAlgorithm::Hs512, "secret", false, &extra, &json!({})).unwrap();
        let (header, ..) = parts(&token);
        assert_eq!(
            serde_json::to_string(&header).unwrap(),
            r#"{"alg":"HS512","typ":"at+jwt","kid":"key-1","x5t":"abc"}"#
        );
        let no_typ: Map<String, Value> = serde_json::from_value(json!({"typ": null, "kid": "k"})).unwrap();
        let (header, ..) = parts(&sign(JwtAlgorithm::Hs384, "secret", false, &no_typ, &json!({})).unwrap());
        assert_eq!(serde_json::to_string(&header).unwrap(), r#"{"alg":"HS384","kid":"k"}"#);
    }

    #[test]
    fn errors() {
        let none = Map::new();
        let err =
            |alg, key: &str, b64: bool, payload: Value| sign(alg, key, b64, &none, &payload).unwrap_err().to_string();
        assert_eq!(err(JwtAlgorithm::Hs256, "s", false, json!([1])), "The JWT payload must be a JSON object ({ … }).");
        assert_eq!(err(JwtAlgorithm::Hs256, "", false, json!({})), "The JWT secret is empty.");
        assert!(err(JwtAlgorithm::Hs256, "not base64!", true, json!({})).contains("isn't valid base64"));
        assert_eq!(err(JwtAlgorithm::Rs256, " ", false, json!({})), "RS256 needs a private key (PEM).");
        assert!(err(JwtAlgorithm::Es256, RSA_PKCS1, false, json!({})).contains("needs an EC key"));
        assert!(err(JwtAlgorithm::Ps256, EC_P256_SEC1, false, json!({})).contains("needs an RSA key"));
        assert_eq!(JwtAlgorithm::parse("rs256"), Some(JwtAlgorithm::Rs256));
        assert_eq!(JwtAlgorithm::parse("none"), None);
    }
}

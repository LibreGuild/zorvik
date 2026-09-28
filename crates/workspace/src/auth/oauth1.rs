//! OAuth 1.0a request signing (RFC 5849), with the body hash extension
//! (`oauth_body_hash`) for bodies that aren't forms.

use super::keys::{self, RsaScheme};
use super::{AuthError, b64, hmac, parse_url, percent_encode, quote};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignatureMethod {
    HmacSha1,
    HmacSha256,
    HmacSha512,
    RsaSha1,
    RsaSha256,
    RsaSha512,
    Plaintext,
}

impl SignatureMethod {
    pub fn name(self) -> &'static str {
        match self {
            Self::HmacSha1 => "HMAC-SHA1",
            Self::HmacSha256 => "HMAC-SHA256",
            Self::HmacSha512 => "HMAC-SHA512",
            Self::RsaSha1 => "RSA-SHA1",
            Self::RsaSha256 => "RSA-SHA256",
            Self::RsaSha512 => "RSA-SHA512",
            Self::Plaintext => "PLAINTEXT",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        [
            Self::HmacSha1,
            Self::HmacSha256,
            Self::HmacSha512,
            Self::RsaSha1,
            Self::RsaSha256,
            Self::RsaSha512,
            Self::Plaintext,
        ]
        .into_iter()
        .find(|m| m.name().eq_ignore_ascii_case(name.trim()))
    }

    /// The digest used for `oauth_body_hash`.
    fn body_digest(self) -> &'static ring::digest::Algorithm {
        match self {
            Self::HmacSha256 | Self::RsaSha256 => &ring::digest::SHA256,
            Self::HmacSha512 | Self::RsaSha512 => &ring::digest::SHA512,
            Self::HmacSha1 | Self::RsaSha1 | Self::Plaintext => &ring::digest::SHA1_FOR_LEGACY_USE_ONLY,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// `Authorization: OAuth …`
    Header,
    /// `oauth_*` parameters in the URL's query string.
    Query,
}

pub struct OAuth1<'a> {
    pub consumer_key: &'a str,
    /// Used by the HMAC and PLAINTEXT methods.
    pub consumer_secret: &'a str,
    /// Empty when there is no token yet (the temporary credentials request).
    pub token: &'a str,
    pub token_secret: &'a str,
    /// PEM private key (PKCS#1 or PKCS#8) for the RSA methods.
    pub private_key: &'a str,
    pub method: SignatureMethod,
    pub callback: &'a str,
    pub verifier: &'a str,
    /// Sent in the header only, never signed.
    pub realm: &'a str,
    /// Adds `oauth_version="1.0"`.
    pub include_version: bool,
    /// Adds `oauth_body_hash` when the body isn't `application/x-www-form-urlencoded`.
    pub include_body_hash: bool,
    /// Seconds since 1970.
    pub timestamp: u64,
    pub nonce: &'a str,
    pub placement: Placement,
}

impl std::fmt::Debug for OAuth1<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OAuth1")
            .field("consumer_key", &self.consumer_key)
            .field("consumer_secret", &"<hidden>")
            .field("token", &"<hidden>")
            .field("token_secret", &"<hidden>")
            .field("private_key", &"<hidden>")
            .field("method", &self.method)
            .field("realm", &self.realm)
            .field("timestamp", &self.timestamp)
            .field("nonce", &self.nonce)
            .field("placement", &self.placement)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct Signed {
    /// The URL to send: with the `oauth_*` parameters for query placement.
    pub url: String,
    /// The `Authorization` header value for header placement.
    pub authorization: Option<String>,
}

/// A PLAINTEXT signature is the secrets themselves: nothing is shown.
impl std::fmt::Debug for Signed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Signed")
            .field("url", &"<hidden>")
            .field("authorization", &self.authorization.as_ref().map(|_| "<hidden>"))
            .finish()
    }
}

/// Signs a request. Query parameters and, for `application/x-www-form-urlencoded`
/// bodies, the body's parameters are part of the signature.
pub fn sign(
    oauth: &OAuth1,
    method: &str,
    url: &str,
    content_type: Option<&str>,
    body: &[u8],
) -> Result<Signed, AuthError> {
    if oauth.consumer_key.is_empty() {
        return Err(AuthError::input("OAuth 1.0 needs a consumer key."));
    }
    if oauth.nonce.is_empty() {
        return Err(AuthError::input("OAuth 1.0 needs a nonce."));
    }
    let parsed = parse_url(url)?;
    let form = content_type.is_some_and(|t| {
        t.split(';').next().unwrap_or_default().trim().eq_ignore_ascii_case("application/x-www-form-urlencoded")
    });

    let mut protocol: Vec<(&str, String)> = vec![("oauth_consumer_key", oauth.consumer_key.to_string())];
    if !oauth.token.is_empty() {
        protocol.push(("oauth_token", oauth.token.to_string()));
    }
    protocol.push(("oauth_signature_method", oauth.method.name().to_string()));
    protocol.push(("oauth_timestamp", oauth.timestamp.to_string()));
    protocol.push(("oauth_nonce", oauth.nonce.to_string()));
    if oauth.include_version {
        protocol.push(("oauth_version", "1.0".to_string()));
    }
    if !oauth.callback.is_empty() {
        protocol.push(("oauth_callback", oauth.callback.to_string()));
    }
    if !oauth.verifier.is_empty() {
        protocol.push(("oauth_verifier", oauth.verifier.to_string()));
    }
    if oauth.include_body_hash && !form {
        protocol.push(("oauth_body_hash", b64(ring::digest::digest(oauth.method.body_digest(), body).as_ref())));
    }

    let base = base_string(method, &parsed, &protocol, form.then_some(body));
    let key = format!("{}&{}", percent_encode(oauth.consumer_secret, b""), percent_encode(oauth.token_secret, b""));
    let signature = match oauth.method {
        SignatureMethod::HmacSha1 => {
            b64(&hmac(ring::hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, key.as_bytes(), base.as_bytes()))
        }
        SignatureMethod::HmacSha256 => b64(&hmac(ring::hmac::HMAC_SHA256, key.as_bytes(), base.as_bytes())),
        SignatureMethod::HmacSha512 => b64(&hmac(ring::hmac::HMAC_SHA512, key.as_bytes(), base.as_bytes())),
        SignatureMethod::RsaSha1 => b64(&rsa(oauth, RsaScheme::Pkcs1Sha1, &base)?),
        SignatureMethod::RsaSha256 => b64(&rsa(oauth, RsaScheme::Pkcs1Sha256, &base)?),
        SignatureMethod::RsaSha512 => b64(&rsa(oauth, RsaScheme::Pkcs1Sha512, &base)?),
        SignatureMethod::Plaintext => key,
    };
    protocol.push(("oauth_signature", signature));
    protocol.sort();

    match oauth.placement {
        Placement::Header => {
            let mut parts = Vec::new();
            if !oauth.realm.is_empty() {
                parts.push(format!("realm=\"{}\"", quote(oauth.realm)));
            }
            parts.extend(protocol.iter().map(|(k, v)| format!("{k}=\"{}\"", percent_encode(v, b""))));
            Ok(Signed { url: url.trim().to_string(), authorization: Some(format!("OAuth {}", parts.join(", "))) })
        }
        Placement::Query => {
            let mut with_params = parsed.clone();
            let mut query: Vec<String> =
                parsed.query().unwrap_or_default().split('&').filter(|p| !p.is_empty()).map(str::to_string).collect();
            query.extend(protocol.iter().map(|(k, v)| format!("{k}={}", percent_encode(v, b""))));
            with_params.set_query(Some(&query.join("&")));
            Ok(Signed { url: with_params.to_string(), authorization: None })
        }
    }
}

fn rsa(oauth: &OAuth1, scheme: RsaScheme, base: &str) -> Result<Vec<u8>, AuthError> {
    if oauth.private_key.trim().is_empty() {
        return Err(AuthError::input(format!("{} needs the private key (PEM).", oauth.method.name())));
    }
    keys::rsa_sign(oauth.private_key, scheme, base.as_bytes())
}

/// The signature base string (RFC 5849 section 3.4.1): the method, the URL without
/// its query, and the query, protocol and form body parameters, encoded and sorted.
fn base_string(method: &str, url: &url::Url, protocol: &[(&str, String)], form_body: Option<&[u8]>) -> String {
    let encode_pair = |(k, v): (std::borrow::Cow<str>, std::borrow::Cow<str>)| {
        (percent_encode(k.as_bytes(), b""), percent_encode(v.as_bytes(), b""))
    };
    let mut params: Vec<(String, String)> =
        url::form_urlencoded::parse(url.query().unwrap_or_default().as_bytes()).map(encode_pair).collect();
    params.extend(protocol.iter().map(|(k, v)| (k.to_string(), percent_encode(v, b""))));
    if let Some(body) = form_body {
        params.extend(url::form_urlencoded::parse(body).map(encode_pair));
    }
    params.sort();
    let normalized = params.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("&");
    // Section 3.4.1.2: lowercase scheme and host, the port only when not the default.
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    let port = url.port().map(|p| format!(":{p}")).unwrap_or_default();
    let base_uri = format!("{}://{host}{port}{}", url.scheme(), url.path());
    format!(
        "{}&{}&{}",
        percent_encode(method.trim().to_ascii_uppercase(), b""),
        percent_encode(base_uri, b""),
        percent_encode(normalized, b"")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::keys::tests::{RSA_PKCS1, rsa_public_der};

    fn oauth<'a>(method: SignatureMethod) -> OAuth1<'a> {
        OAuth1 {
            consumer_key: "dpf43f3p2l4k3l03",
            consumer_secret: "kd94hf93k423kf44",
            token: "",
            token_secret: "",
            private_key: "",
            method,
            callback: "",
            verifier: "",
            realm: "",
            include_version: false,
            include_body_hash: false,
            timestamp: 137131200,
            nonce: "wIjqoS",
            placement: Placement::Header,
        }
    }

    fn header(signed: &Signed) -> &str {
        signed.authorization.as_deref().unwrap()
    }

    #[test]
    fn twitter_example() {
        // "Creating a signature" in the Twitter/X docs, which ends with the signature
        // hCtSmYh+iHYCEqBWrE7C7hYmtUk= (the tnnArxj06… value on "Authorizing a request"
        // was made with secrets that page doesn't give):
        // https://developer.x.com/en/docs/authentication/oauth-1-0a/creating-a-signature
        let o = OAuth1 {
            consumer_key: "xvz1evFS4wEEPTGEFPHBog",
            consumer_secret: "kAcSOqF21Fu85e7zjz7ZN2U4ZRhfV3WpwPAoE3Z7kBw",
            token: "370773112-GmHxMAgYyLbNEtIKZeRNFsMKPR9EyMZeS9weJAEb",
            token_secret: "LswwdoUaIvS8ltyTt5jkRh4J50vUPVVHtR2YPi5kE",
            timestamp: 1318622958,
            nonce: "kYjzVBB8Y0ZFabxSWbWovY3uYSQ2pTgmZeNu2VS4cg",
            include_version: true,
            ..oauth(SignatureMethod::HmacSha1)
        };
        let signed = sign(
            &o,
            "POST",
            "https://api.twitter.com/1.1/statuses/update.json?include_entities=true",
            Some("application/x-www-form-urlencoded"),
            b"status=Hello%20Ladies%20%2B%20Gentlemen%2C%20a%20signed%20OAuth%20request%21",
        )
        .unwrap();
        assert_eq!(
            header(&signed),
            "OAuth oauth_consumer_key=\"xvz1evFS4wEEPTGEFPHBog\", oauth_nonce=\"kYjzVBB8Y0ZFabxSWbWovY3uYSQ2pTgmZeNu2VS4cg\", oauth_signature=\"hCtSmYh%2BiHYCEqBWrE7C7hYmtUk%3D\", oauth_signature_method=\"HMAC-SHA1\", oauth_timestamp=\"1318622958\", oauth_token=\"370773112-GmHxMAgYyLbNEtIKZeRNFsMKPR9EyMZeS9weJAEb\", oauth_version=\"1.0\""
        );
    }

    #[test]
    fn rfc5849_section_1_2() {
        // https://www.rfc-editor.org/rfc/rfc5849#section-1.2
        let initiate = OAuth1 {
            realm: "Photos",
            callback: "http://printer.example.com/ready",
            ..oauth(SignatureMethod::HmacSha1)
        };
        let signed = sign(&initiate, "POST", "https://photos.example.net/initiate", None, b"").unwrap();
        assert_eq!(
            header(&signed),
            "OAuth realm=\"Photos\", oauth_callback=\"http%3A%2F%2Fprinter.example.com%2Fready\", oauth_consumer_key=\"dpf43f3p2l4k3l03\", oauth_nonce=\"wIjqoS\", oauth_signature=\"74KNZJeDHnMBp0EMJ9ZHt%2FXKycU%3D\", oauth_signature_method=\"HMAC-SHA1\", oauth_timestamp=\"137131200\""
        );
        let token = OAuth1 {
            realm: "Photos",
            token: "hh5s93j4hdidpola",
            token_secret: "hdhd0244k9j7ao03",
            verifier: "hfdp7dh39dks9884",
            timestamp: 137131201,
            nonce: "walatlh",
            ..oauth(SignatureMethod::HmacSha1)
        };
        let signed = sign(&token, "POST", "https://photos.example.net/token", None, b"").unwrap();
        assert!(
            header(&signed).contains("oauth_signature=\"gKgrFCywp7rO0OXSjdot%2FIHF7IU%3D\""),
            "{}",
            header(&signed)
        );
        let resource = OAuth1 {
            realm: "Photos",
            token: "nnch734d00sl2jdk",
            token_secret: "pfkkdhi9sl3r4s00",
            timestamp: 137131202,
            nonce: "chapoH",
            ..oauth(SignatureMethod::HmacSha1)
        };
        let signed =
            sign(&resource, "GET", "http://photos.example.net/photos?file=vacation.jpg&size=original", None, b"")
                .unwrap();
        assert!(
            header(&signed).contains("oauth_signature=\"MdpQcU8iPSUjWoN%2FUDMsK2sui9I%3D\""),
            "{}",
            header(&signed)
        );
    }

    #[test]
    fn rfc5849_plaintext() {
        // https://www.rfc-editor.org/rfc/rfc5849#section-2.3
        let o = OAuth1 {
            consumer_key: "jd83jd92dhsh93js",
            consumer_secret: "ja893SD9",
            token: "hdk48Djdsa",
            token_secret: "xyz4992k83j47x0b",
            verifier: "473f82d3",
            realm: "Example",
            ..oauth(SignatureMethod::Plaintext)
        };
        let signed = sign(&o, "POST", "https://server.example.com/request_token", None, b"").unwrap();
        assert!(header(&signed).contains("oauth_signature=\"ja893SD9%26xyz4992k83j47x0b\""), "{}", header(&signed));
    }

    #[test]
    fn rfc5849_base_string() {
        // https://www.rfc-editor.org/rfc/rfc5849#section-3.4.1.1 (a repeated name, empty
        // values, `+` and double encoding).
        let url = url::Url::parse("http://example.com/request?b5=%3D%253D&a3=a&c%40=&a2=r%20b").unwrap();
        let protocol = [
            ("oauth_consumer_key", "9djdj82h48djs9d2".to_string()),
            ("oauth_token", "kkk9d7dh3k39sjv7".to_string()),
            ("oauth_signature_method", "HMAC-SHA1".to_string()),
            ("oauth_timestamp", "137131201".to_string()),
            ("oauth_nonce", "7d8f3e4a".to_string()),
        ];
        assert_eq!(
            base_string("POST", &url, &protocol, Some(b"c2&a3=2+q")),
            "POST&http%3A%2F%2Fexample.com%2Frequest&a2%3Dr%2520b%26a3%3D2%2520q%26a3%3Da%26b5%3D%253D%25253D%26c%2540%3D%26c2%3D%26oauth_consumer_key%3D9djdj82h48djs9d2%26oauth_nonce%3D7d8f3e4a%26oauth_signature_method%3DHMAC-SHA1%26oauth_timestamp%3D137131201%26oauth_token%3Dkkk9d7dh3k39sjv7"
        );
        // Section 3.4.1.2: scheme and host lowercased, default port dropped, others kept.
        let url = url::Url::parse("HTTP://EXAMPLE.COM:80/r%20v/X?id=123").unwrap();
        assert!(base_string("get", &url, &[], None).starts_with("GET&http%3A%2F%2Fexample.com%2Fr%2520v%2FX&"));
        let url = url::Url::parse("https://www.example.net:8080/?q=1").unwrap();
        assert!(base_string("GET", &url, &[], None).starts_with("GET&https%3A%2F%2Fwww.example.net%3A8080%2F&"));
    }

    #[test]
    fn parameter_order_does_not_matter() {
        let o = OAuth1 {
            consumer_key: "9djdj82h48djs9d2",
            token: "kkk9d7dh3k39sjv7",
            timestamp: 137131201,
            nonce: "7d8f3e4a",
            ..oauth(SignatureMethod::HmacSha1)
        };
        let form = Some("application/x-www-form-urlencoded");
        let a =
            sign(&o, "POST", "http://example.com/request?b5=%3D%253D&a3=a&c%40=&a2=r%20b", form, b"c2&a3=2+q").unwrap();
        let b = sign(&o, "POST", "http://EXAMPLE.com:80/request?a3=2%20q&c2=", form, b"a2=r+b&c%40&a3=a&b5=%3D%253D")
            .unwrap();
        assert_eq!(a.authorization, b.authorization);
        let c =
            sign(&o, "POST", "http://example.com/request?b5=%3D%253D&a3=a&c%40=&a2=r%20b", form, b"c2&a3=3+q").unwrap();
        assert_ne!(a.authorization, c.authorization);
    }

    #[test]
    fn rsa_and_other_hmacs() {
        let base_for = |method| {
            let o = OAuth1 { private_key: RSA_PKCS1, ..oauth(method) };
            sign(&o, "GET", "https://api.example.com/r?q=1", None, b"").unwrap()
        };
        for (method, alg) in [
            (SignatureMethod::RsaSha1, &ring::signature::RSA_PKCS1_2048_8192_SHA1_FOR_LEGACY_USE_ONLY),
            (SignatureMethod::RsaSha256, &ring::signature::RSA_PKCS1_2048_8192_SHA256),
            (SignatureMethod::RsaSha512, &ring::signature::RSA_PKCS1_2048_8192_SHA512),
        ] {
            let signed = base_for(method);
            let value = header(&signed);
            let sig = value.split("oauth_signature=\"").nth(1).unwrap().split('"').next().unwrap();
            let sig = crate::auth::percent_decode(sig);
            let sig = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, sig).unwrap();
            let base = format!(
                "GET&https%3A%2F%2Fapi.example.com%2Fr&oauth_consumer_key%3Ddpf43f3p2l4k3l03%26oauth_nonce%3DwIjqoS%26oauth_signature_method%3D{}%26oauth_timestamp%3D137131200%26q%3D1",
                method.name()
            );
            ring::signature::UnparsedPublicKey::new(alg, rsa_public_der(RSA_PKCS1))
                .verify(base.as_bytes(), &sig)
                .unwrap();
        }
        // HMAC-SHA256 and -SHA512 differ from SHA1 and from each other.
        let sigs: Vec<String> = [SignatureMethod::HmacSha1, SignatureMethod::HmacSha256, SignatureMethod::HmacSha512]
            .into_iter()
            .map(|m| header(&sign(&oauth(m), "GET", "https://a.example/", None, b"").unwrap()).to_string())
            .collect();
        assert!(sigs[0] != sigs[1] && sigs[1] != sigs[2]);
        let missing = sign(&oauth(SignatureMethod::RsaSha256), "GET", "https://a.example/", None, b"").unwrap_err();
        assert_eq!(missing.to_string(), "RSA-SHA256 needs the private key (PEM).");
    }

    #[test]
    fn body_hash_query_placement_and_unicode() {
        let o = OAuth1 { include_body_hash: true, placement: Placement::Query, ..oauth(SignatureMethod::HmacSha1) };
        let signed = sign(&o, "POST", "https://a.example/p?x=1", Some("application/json"), b"Hello World!").unwrap();
        assert!(signed.authorization.is_none());
        // oauth-bodyhash draft, section 3.2: SHA-1 of "Hello World!".
        assert!(signed.url.contains("oauth_body_hash=Lve95gjOVATpfV8EL5X4nxwjKHE%3D"), "{}", signed.url);
        assert!(signed.url.starts_with("https://a.example/p?x=1&oauth_body_hash="));
        let form = sign(&o, "POST", "https://a.example/p", Some("application/x-www-form-urlencoded"), b"a=1").unwrap();
        assert!(!form.url.contains("oauth_body_hash"), "form bodies are signed as parameters instead");
        // Unicode in secrets, parameters and the token secret.
        let u = OAuth1 {
            consumer_secret: "sécret ✓",
            token: "t",
            token_secret: "tøken",
            ..oauth(SignatureMethod::HmacSha256)
        };
        let a = sign(&u, "GET", "https://a.example/ü?q=ü", None, b"").unwrap();
        let b = sign(&u, "GET", "https://a.example/%C3%BC?q=%C3%BC", None, b"").unwrap();
        assert_eq!(a.authorization, b.authorization);
        assert!(!format!("{u:?}").contains("sécret"));
        assert!(
            sign(&OAuth1 { nonce: "", ..oauth(SignatureMethod::HmacSha1) }, "GET", "https://a.example/", None, b"")
                .is_err()
        );
    }
}

//! Hawk request authentication (header version 1), as in the reference
//! implementation (github.com/mozilla/hawk).

use super::{AuthError, b64, hmac, parse_url, quote};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HawkAlgorithm {
    Sha256,
    Sha1,
}

impl HawkAlgorithm {
    fn digest(self) -> &'static ring::digest::Algorithm {
        match self {
            Self::Sha256 => &ring::digest::SHA256,
            Self::Sha1 => &ring::digest::SHA1_FOR_LEGACY_USE_ONLY,
        }
    }

    fn hmac(self) -> ring::hmac::Algorithm {
        match self {
            Self::Sha256 => ring::hmac::HMAC_SHA256,
            Self::Sha1 => ring::hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY,
        }
    }
}

pub struct Hawk<'a> {
    pub id: &'a str,
    pub key: &'a str,
    pub algorithm: HawkAlgorithm,
    /// Seconds since 1970.
    pub timestamp: u64,
    pub nonce: &'a str,
    /// Application data (`ext`); empty when not used.
    pub ext: &'a str,
    /// Oz application id and delegating application (`app`, `dlg`); empty when not used.
    pub app: &'a str,
    pub dlg: &'a str,
}

impl std::fmt::Debug for Hawk<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Hawk")
            .field("id", &self.id)
            .field("key", &"<hidden>")
            .field("algorithm", &self.algorithm)
            .field("timestamp", &self.timestamp)
            .field("nonce", &self.nonce)
            .finish()
    }
}

/// The payload hash: over the content type (lowercase, without parameters) and the body.
pub fn payload_hash(algorithm: HawkAlgorithm, content_type: &str, body: &[u8]) -> String {
    let content_type = content_type.split(';').next().unwrap_or_default().trim().to_ascii_lowercase();
    let mut data = format!("hawk.1.payload\n{content_type}\n").into_bytes();
    data.extend_from_slice(body);
    data.push(b'\n');
    b64(ring::digest::digest(algorithm.digest(), &data).as_ref())
}

/// The `Authorization` header value. `payload` is `(content type, body)` when the
/// payload should be covered by the MAC.
pub fn authorization(
    hawk: &Hawk,
    method: &str,
    url: &str,
    payload: Option<(&str, &[u8])>,
) -> Result<String, AuthError> {
    if hawk.id.is_empty() || hawk.key.is_empty() {
        return Err(AuthError::input("Hawk needs a key id and a key."));
    }
    if hawk.nonce.is_empty() {
        return Err(AuthError::input("Hawk needs a nonce."));
    }
    let parsed = parse_url(url)?;
    let resource = match parsed.query() {
        Some(q) => format!("{}?{q}", parsed.path()),
        None => parsed.path().to_string(),
    };
    let host = parsed.host_str().unwrap_or_default().to_ascii_lowercase();
    let port = parsed.port_or_known_default().unwrap_or(80);
    let hash = payload.map(|(content_type, body)| payload_hash(hawk.algorithm, content_type, body));
    let mut normalized = format!(
        "hawk.1.header\n{}\n{}\n{}\n{resource}\n{host}\n{port}\n{}\n{}\n",
        hawk.timestamp,
        hawk.nonce,
        method.trim().to_ascii_uppercase(),
        hash.as_deref().unwrap_or_default(),
        hawk.ext.replace('\\', "\\\\").replace('\n', "\\n"),
    );
    if !hawk.app.is_empty() {
        normalized.push_str(&format!("{}\n{}\n", hawk.app, hawk.dlg));
    }
    let mac = b64(&hmac(hawk.algorithm.hmac(), hawk.key.as_bytes(), normalized.as_bytes()));

    let mut header =
        format!("Hawk id=\"{}\", ts=\"{}\", nonce=\"{}\"", quote(hawk.id), hawk.timestamp, quote(hawk.nonce));
    if let Some(hash) = hash {
        header.push_str(&format!(", hash=\"{hash}\""));
    }
    if !hawk.ext.is_empty() {
        header.push_str(&format!(", ext=\"{}\"", quote(hawk.ext)));
    }
    header.push_str(&format!(", mac=\"{mac}\""));
    if !hawk.app.is_empty() {
        header.push_str(&format!(", app=\"{}\"", quote(hawk.app)));
        if !hawk.dlg.is_empty() {
            header.push_str(&format!(", dlg=\"{}\"", quote(hawk.dlg)));
        }
    }
    Ok(header)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hawk<'a>() -> Hawk<'a> {
        Hawk {
            id: "dh37fgj492je",
            key: "werxhqb98rpaxn39848xrunpaw3489ruxnpa98w4rxn",
            algorithm: HawkAlgorithm::Sha256,
            timestamp: 1353832234,
            nonce: "j4h3g2",
            ext: "some-app-ext-data",
            app: "",
            dlg: "",
        }
    }

    #[test]
    fn protocol_examples() {
        // https://github.com/mozilla/hawk/blob/main/API.md#protocol-example
        let url = "http://example.com:8000/resource/1?b=1&a=2";
        assert_eq!(
            authorization(&hawk(), "GET", url, None).unwrap(),
            "Hawk id=\"dh37fgj492je\", ts=\"1353832234\", nonce=\"j4h3g2\", ext=\"some-app-ext-data\", mac=\"6R4rV5iE+NPoym+WwjeHzjAGXUtLNIxmo1vpMofpLAE=\""
        );
        // https://github.com/mozilla/hawk/blob/main/API.md#payload-validation
        assert_eq!(
            payload_hash(HawkAlgorithm::Sha256, "text/plain", b"Thank you for flying Hawk"),
            "Yi9LfIIFRtBEPt74PVmbTF/xVAwPn7ub15ePICfgnuY="
        );
        assert_eq!(
            authorization(&hawk(), "POST", url, Some(("text/plain; charset=utf-8", b"Thank you for flying Hawk")))
                .unwrap(),
            "Hawk id=\"dh37fgj492je\", ts=\"1353832234\", nonce=\"j4h3g2\", hash=\"Yi9LfIIFRtBEPt74PVmbTF/xVAwPn7ub15ePICfgnuY=\", ext=\"some-app-ext-data\", mac=\"aSe1DERmZuRl3pI36/9BdZmnErTw3sNzOOAUlfeKjVw=\""
        );
    }

    #[test]
    fn app_dlg_sha1_and_default_ports() {
        let with_app = Hawk { app: "my-app", dlg: "their-app", ext: "", ..hawk() };
        let header = authorization(&with_app, "GET", "https://example.com/r", None).unwrap();
        assert!(header.ends_with(", app=\"my-app\", dlg=\"their-app\""), "{header}");
        assert!(!header.contains("ext="));
        // The MAC covers the app: another one gives another MAC.
        let other = authorization(&Hawk { app: "other", ..with_app }, "GET", "https://example.com/r", None).unwrap();
        assert_ne!(header.split("mac=").nth(1), other.split("mac=").nth(1));
        // An explicit default port is the same request.
        let a = authorization(&hawk(), "GET", "https://example.com/r", None).unwrap();
        let b = authorization(&hawk(), "get", "https://EXAMPLE.com:443/r", None).unwrap();
        assert_eq!(a, b);
        let sha1 =
            authorization(&Hawk { algorithm: HawkAlgorithm::Sha1, ..hawk() }, "GET", "https://example.com/r", None)
                .unwrap();
        let mac = sha1.split("mac=\"").nth(1).unwrap().trim_end_matches('"');
        assert_eq!(mac.len(), 28, "a base64 SHA-1 MAC");
    }

    #[test]
    fn ext_is_escaped_and_secrets_stay_out() {
        let odd = Hawk { ext: "say \"hi\"\\", ..hawk() };
        let header = authorization(&odd, "GET", "https://example.com/", None).unwrap();
        assert!(header.contains(r#"ext="say \"hi\"\\""#), "{header}");
        assert!(authorization(&Hawk { key: "", ..hawk() }, "GET", "https://example.com/", None).is_err());
        assert!(!format!("{:?}", hawk()).contains("werxhqb98"));
    }
}

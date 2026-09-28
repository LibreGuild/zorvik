//! Akamai EdgeGrid (`EG1-HMAC-SHA256`), as in Akamai's reference clients
//! (github.com/akamai/AkamaiOPEN-edgegrid-*).

use time::OffsetDateTime;
use time::macros::format_description;

use super::{AuthError, b64, hmac, host_header, parse_url, sha256};

/// The body bytes hashed by default.
pub const DEFAULT_MAX_BODY: usize = 131_072;

pub struct EdgeGrid<'a> {
    pub client_token: &'a str,
    pub client_secret: &'a str,
    pub access_token: &'a str,
    /// Header names whose values are signed, in this order (when the request has them).
    pub headers_to_sign: &'a [String],
    /// Only the first `max_body` bytes of a POST body are hashed.
    pub max_body: usize,
    /// `yyyyMMddTHH:mm:ss+0000` (see [`timestamp`]).
    pub timestamp: &'a str,
    /// A fresh unique value, usually a UUID.
    pub nonce: &'a str,
}

impl std::fmt::Debug for EdgeGrid<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EdgeGrid")
            .field("client_token", &self.client_token)
            .field("client_secret", &"<hidden>")
            .field("access_token", &"<hidden>")
            .field("headers_to_sign", &self.headers_to_sign)
            .field("max_body", &self.max_body)
            .field("timestamp", &self.timestamp)
            .field("nonce", &self.nonce)
            .finish()
    }
}

/// An EdgeGrid timestamp: `20140321T19:34:21+0000`.
pub fn timestamp(now: OffsetDateTime) -> String {
    now.to_offset(time::UtcOffset::UTC)
        .format(format_description!("[year][month][day]T[hour]:[minute]:[second]+0000"))
        .unwrap_or_default()
}

/// The `Authorization` header value. `headers` are the request's headers as sent.
pub fn authorization(
    eg: &EdgeGrid,
    method: &str,
    url: &str,
    headers: &[(String, String)],
    body: &[u8],
) -> Result<String, AuthError> {
    for (value, what) in [
        (eg.client_token, "a client token"),
        (eg.client_secret, "a client secret"),
        (eg.access_token, "an access token"),
        (eg.timestamp, "a timestamp"),
        (eg.nonce, "a nonce"),
    ] {
        if value.trim().is_empty() {
            return Err(AuthError::input(format!("Akamai EdgeGrid needs {what}.")));
        }
    }
    let parsed = parse_url(url)?;
    let method = method.trim().to_ascii_uppercase();
    let auth = format!(
        "EG1-HMAC-SHA256 client_token={};access_token={};timestamp={};nonce={};",
        eg.client_token.trim(),
        eg.access_token.trim(),
        eg.timestamp,
        eg.nonce
    );
    let relative = match parsed.query() {
        Some(q) => format!("{}?{q}", parsed.path()),
        None => parsed.path().to_string(),
    };
    let canonical_headers = eg
        .headers_to_sign
        .iter()
        .filter_map(|name| {
            let value = headers.iter().find(|(k, _)| k.trim().eq_ignore_ascii_case(name.trim()))?.1.trim();
            Some(format!(
                "{}:{}",
                name.trim().to_ascii_lowercase(),
                value.split_whitespace().collect::<Vec<_>>().join(" ")
            ))
        })
        .collect::<Vec<_>>()
        .join("\t");
    let content_hash = if method == "POST" && !body.is_empty() {
        b64(&sha256(&body[..body.len().min(eg.max_body)]))
    } else {
        String::new()
    };
    let data =
        [method.as_str(), parsed.scheme(), &host_header(&parsed), &relative, &canonical_headers, &content_hash, &auth]
            .join("\t");
    let signing_key = b64(&hmac(ring::hmac::HMAC_SHA256, eg.client_secret.trim().as_bytes(), eg.timestamp.as_bytes()));
    let signature = b64(&hmac(ring::hmac::HMAC_SHA256, signing_key.as_bytes(), data.as_bytes()));
    Ok(format!("{auth}signature={signature}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Akamai's test cases (testdata.json for the credentials, testcases.json for the requests):
    /// https://github.com/akamai/AkamaiOPEN-edgegrid-python/tree/master/akamai/edgegrid/test
    const BASE: &str = "https://akaa-baseurl-xxxxxxxxxxx-xxxxxxxxxxxxx.luna.akamaiapis.net";
    const PREFIX: &str = "EG1-HMAC-SHA256 client_token=akab-client-token-xxx-xxxxxxxxxxxxxxxx;access_token=akab-access-token-xxx-xxxxxxxxxxxxxxxx;timestamp=20140321T19:34:21+0000;nonce=nonce-xx-xxxx-xxxx-xxxx-xxxxxxxxxxxx;signature=";

    fn sign(method: &str, path: &str, headers: &[(&str, &str)], body: &str) -> String {
        let to_sign: Vec<String> = ["X-Test1", "X-Test2", "X-Test3"].map(String::from).to_vec();
        let eg = EdgeGrid {
            client_token: "akab-client-token-xxx-xxxxxxxxxxxxxxxx",
            client_secret: "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=",
            access_token: "akab-access-token-xxx-xxxxxxxxxxxxxxxx",
            headers_to_sign: &to_sign,
            max_body: 2048,
            timestamp: "20140321T19:34:21+0000",
            nonce: "nonce-xx-xxxx-xxxx-xxxx-xxxxxxxxxxxx",
        };
        let headers: Vec<(String, String)> = headers.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        let value = authorization(&eg, method, &format!("{BASE}{path}"), &headers, body.as_bytes()).unwrap();
        value.strip_prefix(PREFIX).expect("the header starts with the fixed fields").to_string()
    }

    #[test]
    fn akamai_test_data() {
        assert_eq!(sign("GET", "/", &[], ""), "tL+y4hxyHxgWVD30X3pWnGKHcPzmrIF+LThiAOhMxYU=");
        assert_eq!(sign("GET", "/testapi/v1/t1?p1=1&p2=2", &[], ""), "hKDH1UlnQySSHjvIcZpDMbQHihTQ0XyVAKZaApabdeA=");
        assert_eq!(
            sign("GET", "/testapi/v1/configs/111;222;333?from=12345&limit=200000", &[], ""),
            "pmQF7Is2+O4r/mMojPR4yeF58BrempNNoBX5/DT0Fxs="
        );
        let data = "datadatadatadatadatadatadatadata";
        assert_eq!(sign("POST", "/testapi/v1/t3", &[], data), "hXm4iCxtpN22m4cbZb4lVLW5rhX8Ca82vCFqXzSTPe4=");
        // "POST too large" (2049 bytes) and "POST length equals max_body": only 2048 bytes are hashed.
        let exact = "d".repeat(2048);
        assert_eq!(sign("POST", "/testapi/v1/t3", &[], &exact), "6Q6PiTipLae6n4GsSIDTCJ54bEbHUBp+4MUXrbQCBoY=");
        assert_eq!(
            sign("POST", "/testapi/v1/t3", &[], &"d".repeat(2049)),
            "6Q6PiTipLae6n4GsSIDTCJ54bEbHUBp+4MUXrbQCBoY="
        );
        assert_eq!(sign("POST", "/testapi/v1/t6", &[], ""), "1gEDxeQGD5GovIkJJGcBaKnZ+VaPtrc4qBUHixjsPCQ=");
        // Only POST bodies are hashed.
        let p = "PPPPPPPPPPPPPPPPPPPPPPPPPPPPPPP";
        assert_eq!(sign("PUT", "/testapi/v1/t6", &[], p), "GNBWEYSEWOLtu+7dD52da2C39aX/Jchpon3K/AmBqBU=");
        assert_eq!(sign("PATCH", "/testapi/v1/t6", &[], p), "JIl05ImY1AOnMtmw+9LKgaFA8mnzsEKabbnHmI8LsQ4=");
    }

    #[test]
    fn akamai_header_signing() {
        let get = |path: &str, headers: &[(&str, &str)]| sign("GET", path, headers, "");
        let t4 = "/testapi/v1/t4";
        assert_eq!(get(t4, &[("X-Test1", "test-simple-header")]), "8F9AybcRw+PLxnvT+H0JRkjROrrUgsxJTnRXMzqvcwY=");
        assert_eq!(
            get(t4, &[("X-Test1", "\"     test-header-with-spaces     \"")]),
            "ucq2AbjCNtobHfCTuS38fdkl5UDdWHZhQX46fYR8CqI="
        );
        assert_eq!(
            get(t4, &[("X-Test1", "     first-thing      second-thing")]),
            "WtnneL539UadAAOJwnsXvPqT4Kt6z7HMgBEwAFpt3+c="
        );
        let out_of_order = [("X-Test2", "t2"), ("X-Test1", "t1"), ("X-Test3", "t3")];
        assert_eq!(get(t4, &out_of_order), "Wus73Nx8jOYM+kkBFF2q8D1EATRIMr0WLWwpLBgkBqY=");
        let extra = [("X-Test2", "t2"), ("X-Test1", "t1"), ("X-Test3", "t3"), ("X-Extra", "this won't be included")];
        assert_eq!(get("/testapi/v1/t5", &extra), "Knd/jc0A5Ghhizjayr0AUUvl2MZjBpS3FDSzvtq4Ixc=");
    }

    #[test]
    fn timestamps_and_missing_settings() {
        use time::macros::datetime;
        assert_eq!(timestamp(datetime!(2014-03-21 19:34:21 UTC)), "20140321T19:34:21+0000");
        assert_eq!(timestamp(datetime!(2014-03-21 21:34:21 +2)), "20140321T19:34:21+0000");
        let eg = EdgeGrid {
            client_token: "c",
            client_secret: "",
            access_token: "a",
            headers_to_sign: &[],
            max_body: DEFAULT_MAX_BODY,
            timestamp: "t",
            nonce: "n",
        };
        let err = authorization(&eg, "GET", "https://x.example/", &[], b"").unwrap_err();
        assert_eq!(err.to_string(), "Akamai EdgeGrid needs a client secret.");
        assert!(!format!("{:?}", EdgeGrid { client_secret: "s3cr3t", ..eg }).contains("s3cr3t"));
    }
}

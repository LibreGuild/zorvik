//! AWS Signature Version 4: signs a request in headers (`Authorization`) or in the
//! query string (a presigned URL).
//!
//! Signed headers: `host` plus every header given, except hop-by-hop headers and
//! the ones clients and proxies rewrite (`user-agent`, `expect`, `x-amzn-trace-id`…),
//! as the AWS SDKs do. Pass the headers exactly as they will be sent; headers added
//! later (such as `content-length` by the HTTP client) are simply not signed.
//!
//! Paths: for S3 each segment is encoded once; for other services the path is
//! normalized (`.`/`..` and empty segments) and encoded again as sent, so `%20` in
//! the URL signs as `%2520` (AWS's "encode twice").

use std::collections::BTreeMap;

use time::OffsetDateTime;
use time::macros::format_description;

use super::{AuthError, hex, hmac, host_header, parse_url, percent_decode, percent_encode, sha256};

const ALGORITHM: &str = "AWS4-HMAC-SHA256";
const UNSIGNED_PAYLOAD: &str = "UNSIGNED-PAYLOAD";
/// A presigned URL lives at most 7 days.
const MAX_EXPIRES: u32 = 604_800;
/// Never signed: hop-by-hop headers and ones clients or proxies rewrite.
const SKIPPED_HEADERS: &[&str] = &[
    "authorization",
    "connection",
    "expect",
    "keep-alive",
    "proxy-authorization",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
    "user-agent",
    "x-amzn-trace-id",
];
/// Query parameters the signer sets itself; given ones are replaced (as are the
/// `x-amz-date` header and, with a session token, `x-amz-security-token`).
const OWN_QUERY: &[&str] = &[
    "X-Amz-Algorithm",
    "X-Amz-Credential",
    "X-Amz-Date",
    "X-Amz-Expires",
    "X-Amz-SignedHeaders",
    "X-Amz-Signature",
    "X-Amz-Security-Token",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// `Authorization`, `X-Amz-Date` (and `X-Amz-Security-Token`, `X-Amz-Content-Sha256` for S3) headers.
    Headers,
    /// A presigned URL valid for `expires_in` seconds (1 to 604800; 3600 is usual).
    Query { expires_in: u32 },
}

pub struct SigV4<'a> {
    pub access_key: &'a str,
    pub secret_key: &'a str,
    /// Temporary credentials (STS); `None` or empty when not used.
    pub session_token: Option<&'a str>,
    pub region: &'a str,
    pub service: &'a str,
    pub now: OffsetDateTime,
    pub placement: Placement,
}

impl std::fmt::Debug for SigV4<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SigV4")
            .field("access_key", &self.access_key)
            .field("secret_key", &"<hidden>")
            .field("session_token", &self.session_token.map(|_| "<hidden>"))
            .field("region", &self.region)
            .field("service", &self.service)
            .field("now", &self.now)
            .field("placement", &self.placement)
            .finish()
    }
}

/// What to change on the request.
#[derive(Clone, PartialEq, Eq)]
pub struct Signed {
    /// The URL to send: as given for header signing, with the signature for a presigned URL.
    pub url: String,
    /// Headers to set, replacing any with the same name.
    pub headers: Vec<(String, String)>,
}

/// The values carry the session token and signatures: only header names are shown.
impl std::fmt::Debug for Signed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names: Vec<&str> = self.headers.iter().map(|(k, _)| k.as_str()).collect();
        f.debug_struct("Signed").field("url", &"<hidden>").field("headers", &names).finish()
    }
}

/// Signs a request. `headers` are the request's headers as they will be sent;
/// an `x-amz-content-sha256` among them is used as the payload hash (for example
/// `UNSIGNED-PAYLOAD`).
pub fn sign(
    config: &SigV4,
    method: &str,
    url: &str,
    headers: &[(String, String)],
    body: &[u8],
) -> Result<Signed, AuthError> {
    let parsed = parse_url(url)?;
    let target = Target { host: host_header(&parsed), path: parsed.path(), query: parsed.query().unwrap_or_default() };
    let out = sign_target(config, method, &target, headers, body)?;
    let url = if out.query.is_empty() {
        url.trim().to_string()
    } else {
        let mut signed_url = parsed.clone();
        let mut pairs: Vec<String> = target
            .query
            .split('&')
            .filter(|p| !p.is_empty() && !is_own_query(p.split('=').next().unwrap_or_default()))
            .map(str::to_string)
            .collect();
        pairs.extend(out.query.iter().map(|(k, v)| format!("{k}={v}")));
        signed_url.set_query(Some(&pairs.join("&")));
        signed_url.set_fragment(None);
        signed_url.to_string()
    };
    Ok(Signed { url, headers: out.headers })
}

/// The request as it goes on the wire: `host[:port]`, path and query as sent.
struct Target<'a> {
    host: String,
    path: &'a str,
    query: &'a str,
}

struct Output {
    headers: Vec<(String, String)>,
    /// Encoded query parameters to append (presigned URLs).
    query: Vec<(String, String)>,
}

fn sign_target(
    config: &SigV4,
    method: &str,
    target: &Target,
    headers: &[(String, String)],
    body: &[u8],
) -> Result<Output, AuthError> {
    for (value, what) in [
        (config.access_key, "an access key"),
        (config.secret_key, "a secret key"),
        (config.region, "a region (such as us-east-1)"),
        (config.service, "a service name (such as s3 or execute-api)"),
    ] {
        if value.trim().is_empty() {
            return Err(AuthError::input(format!("AWS Signature needs {what}.")));
        }
    }
    if let Placement::Query { expires_in } = config.placement
        && !(1..=MAX_EXPIRES).contains(&expires_in)
    {
        return Err(AuthError::input("A presigned URL must expire after 1 second to 7 days (604800 seconds)."));
    }
    let (access_key, region, service) = (config.access_key.trim(), config.region.trim(), config.service.trim());
    let amz_date = config
        .now
        .to_offset(time::UtcOffset::UTC)
        .format(format_description!("[year][month][day]T[hour][minute][second]Z"))
        .map_err(|_| AuthError::input("The signing time can't be written as an AWS date."))?;
    let scope = format!("{}/{region}/{service}/aws4_request", &amz_date[..8]);
    let s3 = service.eq_ignore_ascii_case("s3");
    let token = config.session_token.map(str::trim).filter(|t| !t.is_empty());

    let mut signed: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut given_payload_hash = None;
    for (name, value) in headers {
        let name = name.trim().to_ascii_lowercase();
        let own = name == "x-amz-date" || (name == "x-amz-security-token" && token.is_some());
        if name.is_empty() || own || SKIPPED_HEADERS.contains(&name.as_str()) {
            continue;
        }
        if name == "x-amz-content-sha256" {
            given_payload_hash = Some(value.trim().to_string());
        }
        signed.entry(name).or_default().push(header_value(value));
    }
    signed.entry("host".into()).or_insert_with(|| vec![target.host.clone()]);
    let payload_hash = match (given_payload_hash, config.placement) {
        (Some(hash), _) => hash,
        (None, Placement::Query { .. }) if s3 => UNSIGNED_PAYLOAD.to_string(),
        (None, _) => hex(&sha256(body)),
    };

    let mut add_headers = Vec::new();
    let mut add_query = Vec::new();
    match config.placement {
        Placement::Headers => {
            signed.insert("x-amz-date".into(), vec![amz_date.clone()]);
            add_headers.push(("X-Amz-Date".to_string(), amz_date.clone()));
            if let Some(token) = token {
                signed.insert("x-amz-security-token".into(), vec![token.to_string()]);
                add_headers.push(("X-Amz-Security-Token".to_string(), token.to_string()));
            }
            if s3 && !signed.contains_key("x-amz-content-sha256") {
                signed.insert("x-amz-content-sha256".into(), vec![payload_hash.clone()]);
                add_headers.push(("X-Amz-Content-Sha256".to_string(), payload_hash.clone()));
            }
        }
        Placement::Query { expires_in } => {
            let signed_names = signed.keys().cloned().collect::<Vec<_>>().join(";");
            add_query.push(("X-Amz-Algorithm", ALGORITHM.to_string()));
            add_query.push(("X-Amz-Credential", format!("{access_key}/{scope}")));
            add_query.push(("X-Amz-Date", amz_date.clone()));
            add_query.push(("X-Amz-Expires", expires_in.to_string()));
            add_query.push(("X-Amz-SignedHeaders", signed_names));
            if let Some(token) = token {
                add_query.push(("X-Amz-Security-Token", token.to_string()));
            }
        }
    }
    let signed_names = signed.keys().cloned().collect::<Vec<_>>().join(";");
    let canonical_headers: String = signed.iter().map(|(k, v)| format!("{k}:{}\n", v.join(","))).collect();

    let presigned = matches!(config.placement, Placement::Query { .. });
    let mut query: Vec<(String, String)> = target
        .query
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| p.split_once('=').unwrap_or((p, "")))
        .filter(|(k, _)| !(presigned && is_own_query(k)))
        .map(|(k, v)| (percent_encode(percent_decode(k), b""), percent_encode(percent_decode(v), b"")))
        .collect();
    let add_query: Vec<(String, String)> =
        add_query.into_iter().map(|(k, v)| (k.to_string(), percent_encode(v, b""))).collect();
    query.extend(add_query.iter().cloned());
    query.sort();
    let canonical_query = query.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("&");

    let canonical_request = format!(
        "{}\n{}\n{canonical_query}\n{canonical_headers}\n{signed_names}\n{payload_hash}",
        method.trim(),
        canonical_uri(target.path, s3)
    );
    let string_to_sign = format!("{ALGORITHM}\n{amz_date}\n{scope}\n{}", hex(&sha256(canonical_request.as_bytes())));
    let key = [&amz_date.as_bytes()[..8], region.as_bytes(), service.as_bytes(), b"aws4_request"]
        .iter()
        .fold(format!("AWS4{}", config.secret_key).into_bytes(), |key, part| hmac(ring::hmac::HMAC_SHA256, &key, part));
    let signature = hex(&hmac(ring::hmac::HMAC_SHA256, &key, string_to_sign.as_bytes()));

    let mut query_out = add_query;
    if presigned {
        query_out.push(("X-Amz-Signature".to_string(), signature));
    } else {
        add_headers.push((
            "Authorization".to_string(),
            format!("{ALGORITHM} Credential={access_key}/{scope}, SignedHeaders={signed_names}, Signature={signature}"),
        ));
    }
    Ok(Output { headers: add_headers, query: query_out })
}

fn is_own_query(raw_key: &str) -> bool {
    let key = String::from_utf8_lossy(&percent_decode(raw_key)).into_owned();
    OWN_QUERY.contains(&key.as_str())
}

/// Trimmed, with runs of spaces collapsed to one.
fn header_value(value: &str) -> String {
    value.split([' ', '\t']).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ")
}

fn canonical_uri(path: &str, s3: bool) -> String {
    let path = if path.is_empty() { "/" } else { path };
    if s3 {
        // S3 compares against the decoded key encoded once, and doesn't normalize.
        return path
            .split('/')
            .map(|segment| percent_encode(percent_decode(segment), b""))
            .collect::<Vec<_>>()
            .join("/");
    }
    let segments: Vec<&str> = path.split('/').collect();
    let mut kept: Vec<&str> = Vec::new();
    for segment in &segments {
        match *segment {
            "" | "." => {}
            ".." => {
                kept.pop();
            }
            s => kept.push(s),
        }
    }
    let mut normalized = format!("/{}", kept.join("/"));
    if !kept.is_empty() && (path.ends_with('/') || matches!(segments.last(), Some(&"." | &".."))) {
        normalized.push('/');
    }
    percent_encode(normalized, b"/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    const SUITE_TOKEN: &str = "6e86291e8372ff2a2260956d9b8aae1d763fbf315fa00fa31553b73ebf194267";

    fn config(placement: Placement, token: Option<&'static str>) -> SigV4<'static> {
        SigV4 {
            access_key: "AKIDEXAMPLE",
            secret_key: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
            session_token: token,
            region: "us-east-1",
            service: "service",
            now: datetime!(2015-08-30 12:36:00 UTC),
            placement,
        }
    }

    fn headers(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    fn header_signature(out: &Output) -> String {
        let auth = &out.headers.iter().find(|(k, _)| k == "Authorization").unwrap().1;
        auth.rsplit("Signature=").next().unwrap().to_string()
    }

    fn query_signature(out: &Output) -> String {
        out.query.iter().find(|(k, _)| k == "X-Amz-Signature").unwrap().1.clone()
    }

    /// Cases of the AWS SigV4 test suite (context: AKIDEXAMPLE, us-east-1, service
    /// "service", 2015-08-30T12:36:00Z, presigned for 3600 s), from
    /// https://github.com/awslabs/aws-c-auth/tree/main/tests/aws-signing-test-suite/v4
    /// (request.txt, header-signature.txt, query-signature.txt).
    #[test]
    fn aws_test_suite() {
        /// Name, method, path, query, headers, session token, header signature, query signature.
        type Case = (
            &'static str,
            &'static str,
            &'static str,
            &'static str,
            &'static [(&'static str, &'static str)],
            Option<&'static str>,
            &'static str,
            &'static str,
        );
        #[rustfmt::skip]
        let cases: &[Case] = &[
            ("get-vanilla", "GET", "/", "", &[], None,
                "5fa00fa31553b73ebf1942676e86291e8372ff2a2260956d9b8aae1d763fbf31",
                "e93c787ed7f371d5c6b165c1b38ede9550f4dce4144713e844b25b7192d3865d"),
            ("get-vanilla-query-order-key-case", "GET", "/", "Param2=value2&Param1=value1", &[], None,
                "b97d918cfa904a5beff61c982a1b6f458b799221646efd99d3219ec94cdf2500",
                "86012e2c9ad4d77369f5d81c11f75158aae4f895a085212cc6d3f923d300bed5"),
            ("get-vanilla-query-order-encoded", "GET", "/", "Param-3=Value3&Param=Value2&%E1%88%B4=Value1", &[], None,
                "371d3713e185cc334048618a97f809c9ffe339c62934c032af5a0e595648fcac",
                "c5f1848ceec943ac2ca68ee720460c23aaae30a2300586597ada94c4a65e4787"),
            ("get-vanilla-query-unreserved", "GET", "/",
                "-._~0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz=-._~0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz",
                &[], None,
                "9c3e54bfcdf0b19771a7f523ee5669cdf59bc7cc0884027167c21bb143a40197",
                "8e76a88a7433637b12778d5592799b29ad21ecd6cf6325051c21d86f0acda2bf"),
            ("get-vanilla-utf8-query", "GET", "/", "ሴ=bar", &[], None,
                "2cdec8eed098649ff3a119c94853b13c643bcf08f8b0a1d91e12c9027818dd04",
                "0bdd809b1519ac4f0c1dc3540e2cc46bd0c7f778eda408b2ebf3b913d21ff600"),
            ("get-vanilla-empty-query-key", "GET", "/", "Param1=value1", &[], None,
                "a67d582fa61cc504c4bae71f336f98b97f1ea3c7a6bfe1b6e45aec72011b9aeb",
                "49096700cbbaa5753443850f40df10f904fc2fdb544dc9512203cc77c471a9de"),
            ("get-space-normalized", "GET", "/example space/", "", &[], None,
                "652487583200325589f1fba4c7e578f72c47cb61beeca81406b39ddec1366741",
                "7a1f416954786484c9824d93c1f26ef64acb9b1b6c9154d08c9f07d0e394abf6"),
            ("get-utf8", "GET", "/ሴ", "", &[], None,
                "8318018e0b0f223aa2bbf98705b62bb787dc9c0e678f255a891fd03141be5d85",
                "10eae3f14a260bd3911cc6d008d3c576d143b05b62f09782a7a4b37f52178e44"),
            ("get-unreserved", "GET", "/-._~0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz", "", &[], None,
                "07ef7494c76fa4850883e2b006601f940f8a34d404d0cfa977f52a65bbf5f24f",
                "95968482db1b9e0fadef6efc1bd24689f77c77d9ef56919c96a28cc92e0d6005"),
            ("get-slashes-normalized", "GET", "//example//", "", &[], None,
                "9a624bd73a37c9a373b5312afbebe7a714a789de108f0bdfe846570885f57e84",
                "c1834e8fb0307243711f0f907f6ab7311ed300d87f13792d7ee4da89ab93e082"),
            ("get-relative-relative-normalized", "GET", "/example1/example2/../..", "", &[], None,
                "5fa00fa31553b73ebf1942676e86291e8372ff2a2260956d9b8aae1d763fbf31",
                "e93c787ed7f371d5c6b165c1b38ede9550f4dce4144713e844b25b7192d3865d"),
            ("get-header-key-duplicate", "GET", "/", "",
                &[("My-Header1", "value2"), ("My-Header1", "value2"), ("My-Header1", "value1")], None,
                "c9d5ea9f3f72853aea855b47ea873832890dbdd183b4468f858259531a5138ea",
                "3349ee0b81b4b589da0ff28a395c3591e04de515651dd74f298fa992d1507a97"),
            ("get-header-value-trim", "GET", "/", "",
                &[("My-Header1", " value1"), ("My-Header2", " \"a   b   c\"")], None,
                "acc3ed3afb60bb290fc8d2dd0098b9911fcaa05412b367055dee359757a9c736",
                "e7bb0fd515e125e1aec2ecc4c0c17484fb06f6846b927c35e46005dd3df3acd4"),
            ("get-vanilla-with-session-token", "GET", "/", "", &[], Some(SUITE_TOKEN),
                "07ec1639c89043aa0e3e2de82b96708f198cceab042d4a97044c66dd9f74e7f8",
                "7ff2b50b376cb4d151970630573d6291dc128cc5c2a12ffb237f73cc53f67b6c"),
        ];
        for (name, method, path, query, extra, token, header_sig, query_sig) in cases {
            let mut given = headers(&[("Host", "example.amazonaws.com")]);
            given.extend(headers(extra));
            let target = Target { host: "example.amazonaws.com".into(), path, query };
            let out = sign_target(&config(Placement::Headers, *token), method, &target, &given, b"").unwrap();
            assert_eq!(header_signature(&out), *header_sig, "{name} (headers)");
            let out = sign_target(&config(Placement::Query { expires_in: 3600 }, *token), method, &target, &given, b"")
                .unwrap();
            assert_eq!(query_signature(&out), *query_sig, "{name} (query)");
        }
    }

    #[test]
    fn aws_test_suite_signed_body() {
        // post-x-www-form-urlencoded and its -parameters variant ("sign_body": true, so
        // the header form carries x-amz-content-sha256), same source as above.
        let body = b"Param1=value1";
        for (content_type, header_sig, query_sig) in [
            (
                "application/x-www-form-urlencoded",
                "d3875051da38690788ef43de4db0d8f280229d82040bfac253562e56c3f20e0b",
                "89a40deed0f26f9461242825a082d2222717248abc7ab41f552ad84a94ad46e9",
            ),
            (
                "application/x-www-form-urlencoded; charset=utf-8",
                "328d1b9eaadca9f5818ef05e8392801e091653bafec24fcab71e7344e7f51422",
                "0dbeb9b026c7b6675f266b8427efec9b4fa8b1f6ef1477d717aea231106eab4d",
            ),
        ] {
            let given =
                headers(&[("Content-Type", content_type), ("Host", "example.amazonaws.com"), ("Content-Length", "13")]);
            let target = Target { host: "example.amazonaws.com".into(), path: "/", query: "" };
            let query =
                sign_target(&config(Placement::Query { expires_in: 3600 }, None), "POST", &target, &given, body)
                    .unwrap();
            assert_eq!(query_signature(&query), query_sig);
            let mut given = given;
            given.push(("x-amz-content-sha256".into(), hex(&sha256(body))));
            let out = sign_target(&config(Placement::Headers, None), "POST", &target, &given, body).unwrap();
            assert_eq!(header_signature(&out), header_sig);
        }
    }

    #[test]
    fn aws_docs_iam_example() {
        // https://docs.aws.amazon.com/IAM/latest/UserGuide/create-signed-request.html (signature example)
        let cfg = SigV4 { service: "iam", ..config(Placement::Headers, None) };
        let given = headers(&[("Content-Type", "application/x-www-form-urlencoded; charset=utf-8")]);
        let signed =
            sign(&cfg, "GET", "https://iam.amazonaws.com/?Action=ListUsers&Version=2010-05-08", &given, b"").unwrap();
        assert_eq!(signed.url, "https://iam.amazonaws.com/?Action=ListUsers&Version=2010-05-08");
        assert_eq!(
            signed.headers,
            headers(&[
                ("X-Amz-Date", "20150830T123600Z"),
                (
                    "Authorization",
                    "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/iam/aws4_request, SignedHeaders=content-type;host;x-amz-date, Signature=5d672d79c15b13162d9279b0855cfba6789a8edb4c82c400e06b5924a6f2b5d7"
                ),
            ])
        );
    }

    #[test]
    fn presigned_url_matches_the_suite() {
        let cfg = config(Placement::Query { expires_in: 3600 }, Some(SUITE_TOKEN));
        let signed = sign(&cfg, "GET", "https://example.amazonaws.com/#frag", &[], b"").unwrap();
        assert!(signed.headers.is_empty());
        assert_eq!(
            signed.url,
            format!(
                "https://example.amazonaws.com/?X-Amz-Algorithm=AWS4-HMAC-SHA256&X-Amz-Credential=AKIDEXAMPLE%2F20150830%2Fus-east-1%2Fservice%2Faws4_request&X-Amz-Date=20150830T123600Z&X-Amz-Expires=3600&X-Amz-SignedHeaders=host&X-Amz-Security-Token={SUITE_TOKEN}&X-Amz-Signature=7ff2b50b376cb4d151970630573d6291dc128cc5c2a12ffb237f73cc53f67b6c"
            )
        );
        // Signing a presigned URL again replaces the old signature parameters.
        let again = sign(&cfg, "GET", &signed.url, &[], b"").unwrap();
        assert_eq!(again.url, signed.url);
    }

    #[test]
    fn s3_signs_the_payload_and_encodes_once() {
        let cfg = SigV4 { service: "s3", ..config(Placement::Headers, None) };
        let signed = sign(&cfg, "PUT", "https://bucket.s3.amazonaws.com/a%20b/c+d!", &[], b"hello").unwrap();
        let hash = hex(&sha256(b"hello"));
        assert!(signed.headers.contains(&("X-Amz-Content-Sha256".into(), hash.clone())));
        assert!(signed.headers[2].1.contains("SignedHeaders=host;x-amz-content-sha256;x-amz-date"));
        assert_eq!(canonical_uri("/a%20b/c+d!", true), "/a%20b/c%2Bd%21");
        assert_eq!(canonical_uri("//example//", true), "//example//", "S3 paths are not normalized");
        assert_eq!(canonical_uri("/a%20b/c+d!", false), "/a%2520b/c%2Bd%21", "other services encode twice");
        // A given payload hash wins; presigned S3 URLs don't sign the payload.
        let given = headers(&[("x-amz-content-sha256", "UNSIGNED-PAYLOAD")]);
        let unsigned = sign(&cfg, "PUT", "https://bucket.s3.amazonaws.com/k", &given, b"hello").unwrap();
        assert!(!unsigned.headers.iter().any(|(k, _)| k == "X-Amz-Content-Sha256"));
        let target = Target { host: "bucket.s3.amazonaws.com".into(), path: "/k", query: "" };
        let presigned = SigV4 { service: "s3", ..config(Placement::Query { expires_in: 60 }, None) };
        let a = sign_target(&presigned, "GET", &target, &[], b"one").unwrap();
        let b = sign_target(&presigned, "GET", &target, &[], b"two").unwrap();
        assert_eq!(query_signature(&a), query_signature(&b));
    }

    #[test]
    fn skipped_headers_and_bad_settings() {
        let cfg = config(Placement::Headers, Some(""));
        let given = headers(&[("User-Agent", "zorvik"), ("X-Amz-Date", "19990101T000000Z"), ("Accept", "a")]);
        let signed = sign(&cfg, "GET", "http://localhost:8080/p", &given, b"").unwrap();
        let auth = &signed.headers.last().unwrap().1;
        assert!(auth.contains("SignedHeaders=accept;host;x-amz-date,"), "{auth}");
        assert!(!signed.headers.iter().any(|(k, _)| k == "X-Amz-Security-Token"), "an empty token isn't sent");
        let err = |cfg: SigV4| sign(&cfg, "GET", "https://x.amazonaws.com/", &[], b"").unwrap_err().to_string();
        assert!(err(SigV4 { region: " ", ..config(Placement::Headers, None) }).contains("region"));
        assert!(err(SigV4 { secret_key: "", ..config(Placement::Headers, None) }).contains("secret key"));
        assert!(err(config(Placement::Query { expires_in: 0 }, None)).contains("expire"));
        assert!(err(config(Placement::Query { expires_in: 604_801 }, None)).contains("expire"));
        assert!(sign(&config(Placement::Headers, None), "GET", "not a url", &[], b"").is_err());
        let debug = format!("{:?}", config(Placement::Headers, Some("tok")));
        assert!(!debug.contains("wJalrXUtnFEMI") && !debug.contains("tok\""), "{debug}");
    }
}

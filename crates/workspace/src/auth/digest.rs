//! HTTP Digest authentication (RFC 7616; RFC 2617 and RFC 2069 servers too) and the
//! `WWW-Authenticate` parser shared with NTLM and Negotiate.

use md5::Digest as _;

use super::{AuthError, hex, percent_encode, quote};

/// One challenge of a `WWW-Authenticate` (or `Proxy-Authenticate`) header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Challenge {
    /// The scheme as the server wrote it (`Digest`, `NTLM`, `Negotiate`…); compare with [`Challenge::is`].
    pub scheme: String,
    /// A token68 value, as in `NTLM TlRMTVNTUAACAAAA…`.
    pub token: Option<String>,
    /// Parameters with lowercase names and unquoted values.
    pub params: Vec<(String, String)>,
}

impl Challenge {
    pub fn is(&self, scheme: &str) -> bool {
        self.scheme.eq_ignore_ascii_case(scheme)
    }

    pub fn param(&self, name: &str) -> Option<&str> {
        self.params.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

/// Every challenge in a response's `WWW-Authenticate` headers. One header may hold
/// several challenges (`Digest realm="a, b", nonce="…", Basic realm="x"`); quoted
/// strings may contain commas and escaped quotes.
pub fn parse_challenges(values: &[&str]) -> Vec<Challenge> {
    let mut out = Vec::new();
    for value in values {
        let mut p = Parser { s: value.as_bytes(), i: 0 };
        loop {
            p.skip(b" \t,");
            if p.at_end() {
                break;
            }
            let Some(scheme) = p.token() else {
                p.i += 1; // not a token character: skip it
                continue;
            };
            let mut challenge = Challenge { scheme, token: None, params: Vec::new() };
            // Only whitespace separates the scheme from its token68 or parameters.
            if p.skip(b" \t") && !p.at_end() && p.peek() != Some(b',') {
                challenge.token = p.token68();
                if challenge.token.is_none() {
                    p.params(&mut challenge.params);
                }
            }
            out.push(challenge);
        }
    }
    out
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn at_end(&self) -> bool {
        self.i >= self.s.len()
    }

    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }

    /// Skips bytes in `set`; true when it skipped any.
    fn skip(&mut self, set: &[u8]) -> bool {
        let start = self.i;
        while self.peek().is_some_and(|b| set.contains(&b)) {
            self.i += 1;
        }
        self.i > start
    }

    fn take_while(&mut self, f: impl Fn(u8) -> bool) -> String {
        let start = self.i;
        while self.peek().is_some_and(&f) {
            self.i += 1;
        }
        String::from_utf8_lossy(&self.s[start..self.i]).into_owned()
    }

    /// RFC 7230 token.
    fn token(&mut self) -> Option<String> {
        let t = self.take_while(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b));
        (!t.is_empty()).then_some(t)
    }

    /// RFC 7235 token68, only when nothing but the end or a comma follows it.
    fn token68(&mut self) -> Option<String> {
        let start = self.i;
        let mut j = start;
        while j < self.s.len() && (self.s[j].is_ascii_alphanumeric() || b"-._~+/".contains(&self.s[j])) {
            j += 1;
        }
        if j == start {
            return None;
        }
        while j < self.s.len() && self.s[j] == b'=' {
            j += 1;
        }
        let mut k = j;
        while k < self.s.len() && matches!(self.s[k], b' ' | b'\t') {
            k += 1;
        }
        if k < self.s.len() && self.s[k] != b',' {
            return None;
        }
        self.i = j;
        Some(String::from_utf8_lossy(&self.s[start..j]).into_owned())
    }

    fn quoted(&mut self) -> String {
        self.i += 1; // opening quote
        let mut out = Vec::new();
        while let Some(b) = self.peek() {
            self.i += 1;
            match b {
                b'"' => break,
                b'\\' if !self.at_end() => {
                    out.push(self.s[self.i]);
                    self.i += 1;
                }
                _ => out.push(b),
            }
        }
        String::from_utf8_lossy(&out).into_owned()
    }

    /// `name=value` pairs up to the next challenge (a token not followed by `=`).
    fn params(&mut self, out: &mut Vec<(String, String)>) {
        loop {
            self.skip(b" \t,");
            let start = self.i;
            let Some(name) = self.token() else { break };
            self.skip(b" \t");
            if self.peek() != Some(b'=') {
                self.i = start; // the next challenge's scheme
                break;
            }
            self.i += 1;
            self.skip(b" \t");
            let value = if self.peek() == Some(b'"') {
                self.quoted()
            } else {
                self.take_while(|b| !matches!(b, b',' | b' ' | b'\t'))
            };
            out.push((name.to_ascii_lowercase(), value));
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DigestAlgorithm {
    Md5,
    Md5Sess,
    Sha256,
    Sha256Sess,
    Sha512_256,
    Sha512_256Sess,
}

impl DigestAlgorithm {
    pub fn name(self) -> &'static str {
        match self {
            Self::Md5 => "MD5",
            Self::Md5Sess => "MD5-sess",
            Self::Sha256 => "SHA-256",
            Self::Sha256Sess => "SHA-256-sess",
            Self::Sha512_256 => "SHA-512-256",
            Self::Sha512_256Sess => "SHA-512-256-sess",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        [Self::Md5, Self::Md5Sess, Self::Sha256, Self::Sha256Sess, Self::Sha512_256, Self::Sha512_256Sess]
            .into_iter()
            .find(|a| a.name().eq_ignore_ascii_case(name.trim()))
    }

    fn session(self) -> bool {
        matches!(self, Self::Md5Sess | Self::Sha256Sess | Self::Sha512_256Sess)
    }

    /// Preference when a server offers several (RFC 7616 section 3.7).
    fn strength(self) -> u8 {
        match self {
            Self::Md5 | Self::Md5Sess => 0,
            Self::Sha256 | Self::Sha256Sess => 1,
            Self::Sha512_256 | Self::Sha512_256Sess => 2,
        }
    }

    /// Lowercase hex digest.
    fn hash(self, data: &[u8]) -> String {
        match self {
            Self::Md5 | Self::Md5Sess => hex(&md5::Md5::digest(data)),
            Self::Sha256 | Self::Sha256Sess => hex(ring::digest::digest(&ring::digest::SHA256, data).as_ref()),
            Self::Sha512_256 | Self::Sha512_256Sess => {
                hex(ring::digest::digest(&ring::digest::SHA512_256, data).as_ref())
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Qop {
    Auth,
    AuthInt,
}

impl Qop {
    pub fn name(self) -> &'static str {
        match self {
            Self::Auth => "auth",
            Self::AuthInt => "auth-int",
        }
    }
}

/// A Digest challenge Zorvik can answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DigestChallenge {
    pub realm: String,
    pub nonce: String,
    pub opaque: Option<String>,
    pub algorithm: DigestAlgorithm,
    /// The server named the algorithm. RFC 2069 servers don't, and then it isn't echoed.
    pub algorithm_named: bool,
    /// The qop values offered that Zorvik supports; empty when the server sent no qop (RFC 2069).
    pub qop: Vec<Qop>,
    /// The previous nonce expired but the credentials were right: answer again with this one.
    pub stale: bool,
    /// The server wants the username hashed (RFC 7616 section 3.4.4).
    pub userhash: bool,
}

impl DigestChallenge {
    /// Reads one `Digest` challenge.
    pub fn from_challenge(challenge: &Challenge) -> Result<Self, AuthError> {
        if !challenge.is("Digest") {
            return Err(AuthError::challenge(format!(
                "The server asked for {} authentication, not Digest.",
                challenge.scheme
            )));
        }
        let nonce = challenge
            .param("nonce")
            .filter(|n| !n.is_empty())
            .ok_or_else(|| AuthError::challenge("The server's Digest challenge has no nonce."))?;
        let (algorithm, algorithm_named) = match challenge.param("algorithm") {
            None => (DigestAlgorithm::Md5, false),
            Some(name) => (
                DigestAlgorithm::parse(name).ok_or_else(|| {
                    AuthError::challenge(format!(
                        "The server asked for the Digest algorithm {name}, which Zorvik doesn't support (it supports MD5, SHA-256 and SHA-512-256, with or without -sess)."
                    ))
                })?,
                true,
            ),
        };
        let qop = match challenge.param("qop") {
            None => Vec::new(),
            Some(list) => {
                let offered: Vec<Qop> = list
                    .split(',')
                    .filter_map(|q| match q.trim().to_ascii_lowercase().as_str() {
                        "auth" => Some(Qop::Auth),
                        "auth-int" => Some(Qop::AuthInt),
                        _ => None,
                    })
                    .collect();
                if offered.is_empty() {
                    return Err(AuthError::challenge(format!(
                        "The server asked for Digest with qop \"{list}\"; Zorvik supports auth and auth-int."
                    )));
                }
                offered
            }
        };
        let flag = |name: &str| challenge.param(name).is_some_and(|v| v.eq_ignore_ascii_case("true"));
        Ok(Self {
            realm: challenge.param("realm").unwrap_or_default().to_string(),
            nonce: nonce.to_string(),
            opaque: challenge.param("opaque").map(str::to_string),
            algorithm,
            algorithm_named,
            qop,
            stale: flag("stale"),
            userhash: flag("userhash"),
        })
    }

    /// The strongest Digest challenge Zorvik can answer among a response's challenges
    /// (servers may offer SHA-256 and MD5 side by side).
    pub fn select(challenges: &[Challenge]) -> Result<Self, AuthError> {
        let digests: Vec<&Challenge> = challenges.iter().filter(|c| c.is("Digest")).collect();
        if digests.is_empty() {
            let offered: Vec<&str> = challenges.iter().map(|c| c.scheme.as_str()).collect();
            return Err(AuthError::challenge(if offered.is_empty() {
                "The server didn't ask for Digest authentication (no WWW-Authenticate header).".to_string()
            } else {
                format!("The server didn't ask for Digest authentication (it offered {}).", offered.join(", "))
            }));
        }
        let parsed: Vec<Result<Self, AuthError>> = digests.into_iter().map(Self::from_challenge).collect();
        if let Some(best) = parsed.iter().flatten().max_by_key(|c| c.algorithm.strength()) {
            return Ok(best.clone());
        }
        Err(parsed.into_iter().find_map(Result::err).expect("at least one Digest challenge"))
    }

    /// Parses `WWW-Authenticate` header values and selects the Digest challenge.
    pub fn from_headers(values: &[&str]) -> Result<Self, AuthError> {
        Self::select(&parse_challenges(values))
    }
}

pub struct DigestCredentials<'a> {
    pub username: &'a str,
    pub password: &'a str,
}

impl std::fmt::Debug for DigestCredentials<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DigestCredentials").field("username", &self.username).field("password", &"<hidden>").finish()
    }
}

/// The `Authorization` header value answering `challenge`.
///
/// `uri` is the request target as sent (`/path?query`), `body` is used for
/// `qop=auth-int`, `cnonce` is a fresh client nonce and `nc` counts the requests
/// sent with this server nonce (starting at 1). qop `auth` is preferred when offered.
/// A non-ASCII username is sent as `username*` (RFC 7616 section 3.4) unless the
/// server asked for `userhash`.
pub fn authorization(
    challenge: &DigestChallenge,
    creds: &DigestCredentials,
    method: &str,
    uri: &str,
    body: &[u8],
    cnonce: &str,
    nc: u32,
) -> Result<String, AuthError> {
    let qop = if challenge.qop.contains(&Qop::Auth) { Some(Qop::Auth) } else { challenge.qop.first().copied() };
    if (qop.is_some() || challenge.algorithm.session()) && cnonce.is_empty() {
        return Err(AuthError::input("Digest authentication needs a client nonce (cnonce)."));
    }
    let alg = challenge.algorithm;
    let (user, realm, nonce) = (creds.username, challenge.realm.as_str(), challenge.nonce.as_str());
    let mut ha1 = alg.hash(format!("{user}:{realm}:{}", creds.password).as_bytes());
    if alg.session() {
        ha1 = alg.hash(format!("{ha1}:{nonce}:{cnonce}").as_bytes());
    }
    let ha2 = match qop {
        Some(Qop::AuthInt) => alg.hash(format!("{method}:{uri}:{}", alg.hash(body)).as_bytes()),
        _ => alg.hash(format!("{method}:{uri}").as_bytes()),
    };
    let nc = format!("{nc:08x}");
    let response = match qop {
        Some(q) => alg.hash(format!("{ha1}:{nonce}:{nc}:{cnonce}:{}:{ha2}", q.name()).as_bytes()),
        None => alg.hash(format!("{ha1}:{nonce}:{ha2}").as_bytes()),
    };

    let mut parts = Vec::new();
    if challenge.userhash {
        parts.push(format!("username=\"{}\"", alg.hash(format!("{user}:{realm}").as_bytes())));
    } else if user.is_ascii() {
        parts.push(format!("username=\"{}\"", quote(user)));
    } else {
        parts.push(format!("username*=UTF-8''{}", percent_encode(user, b"")));
    }
    parts.push(format!("realm=\"{}\"", quote(realm)));
    parts.push(format!("nonce=\"{}\"", quote(nonce)));
    parts.push(format!("uri=\"{}\"", quote(uri)));
    if challenge.algorithm_named {
        parts.push(format!("algorithm={}", alg.name()));
    }
    if let Some(q) = qop {
        parts.push(format!("qop={}", q.name()));
        parts.push(format!("nc={nc}"));
        parts.push(format!("cnonce=\"{}\"", quote(cnonce)));
    }
    parts.push(format!("response=\"{response}\""));
    if let Some(opaque) = &challenge.opaque {
        parts.push(format!("opaque=\"{}\"", quote(opaque)));
    }
    if challenge.userhash {
        parts.push("userhash=true".to_string());
    }
    Ok(format!("Digest {}", parts.join(", ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn param(header: &str, name: &str) -> String {
        parse_challenges(&[header])[0].param(name).unwrap().to_string()
    }

    #[test]
    fn rfc7616_sha256_and_md5() {
        // RFC 7616 section 3.9.1: https://www.rfc-editor.org/rfc/rfc7616#section-3.9.1
        let headers = [
            r#"Digest realm="http-auth@example.org", qop="auth, auth-int", algorithm=SHA-256, nonce="7ypf/xlj9XXwfDPEoM4URrv/xwf94BcCAzFZH4GiTo0v", opaque="FQhe/qaU925kfnzjCev0ciny7QMkPqMAFRtzCUYo5tdS""#,
            r#"Digest realm="http-auth@example.org", qop="auth, auth-int", algorithm=MD5, nonce="7ypf/xlj9XXwfDPEoM4URrv/xwf94BcCAzFZH4GiTo0v", opaque="FQhe/qaU925kfnzjCev0ciny7QMkPqMAFRtzCUYo5tdS""#,
        ];
        let creds = DigestCredentials { username: "Mufasa", password: "Circle of Life" };
        let cnonce = "f2/wE4q74E6zIJEtWaHKaf5wv/H5QzzpXusqGemxURZJ";
        let sha256 = DigestChallenge::from_headers(&headers).unwrap();
        assert_eq!(sha256.algorithm, DigestAlgorithm::Sha256, "the stronger algorithm wins");
        let header = authorization(&sha256, &creds, "GET", "/dir/index.html", b"", cnonce, 1).unwrap();
        assert_eq!(
            header,
            "Digest username=\"Mufasa\", realm=\"http-auth@example.org\", nonce=\"7ypf/xlj9XXwfDPEoM4URrv/xwf94BcCAzFZH4GiTo0v\", uri=\"/dir/index.html\", algorithm=SHA-256, qop=auth, nc=00000001, cnonce=\"f2/wE4q74E6zIJEtWaHKaf5wv/H5QzzpXusqGemxURZJ\", response=\"753927fa0e85d155564e2e272a28d1802ca10daf4496794697cf8db5856cb6c1\", opaque=\"FQhe/qaU925kfnzjCev0ciny7QMkPqMAFRtzCUYo5tdS\""
        );
        let md5 = DigestChallenge::from_challenge(&parse_challenges(&headers[1..])[0]).unwrap();
        let header = authorization(&md5, &creds, "GET", "/dir/index.html", b"", cnonce, 1).unwrap();
        assert_eq!(param(&header, "response"), "8ca523f5e9506fed4657c9700eebdbec");
        assert_eq!(param(&header, "algorithm"), "MD5");
    }

    #[test]
    fn rfc7616_sha512_256_userhash() {
        // RFC 7616 section 3.9.2 with the values of erratum 4897
        // (https://www.rfc-editor.org/errata/eid4897): the RFC's own values use a
        // non-standard SHA-512/256.
        let header = r#"Digest realm="api@example.org", qop="auth", algorithm=SHA-512-256, nonce="5TsQWLVdgBdmrQ0XsxbDODV+57QdFR34I9HAbC/RVvkK", opaque="HRPCssKJSGjCrkzDg8OhwpzCiGPChXYjwrI2QmXDnsOS", charset=UTF-8, userhash=true"#;
        let challenge = DigestChallenge::from_headers(&[header]).unwrap();
        assert!(challenge.userhash);
        let creds = DigestCredentials { username: "J\u{e4}s\u{f8}n Doe", password: "Secret, or not?" };
        let cnonce = "NTg6RKcb9boFIAS3KrFK9BGeh+iDa/sm6jUMp2wds69v";
        let value = authorization(&challenge, &creds, "GET", "/doe.json", b"", cnonce, 1).unwrap();
        assert_eq!(param(&value, "username"), "793263caabb707a56211940d90411ea4a575adeccb7e360aeb624ed06ece9b0b");
        assert_eq!(param(&value, "response"), "3798d4131c277846293534c3edc11bd8a5e4cdcbff78b05db9d95eeb1cec68a5");
        assert!(value.ends_with(", userhash=true"));

        // Without userhash, the non-ASCII name goes in username* and the response is the same.
        let plain = DigestChallenge { userhash: false, ..challenge };
        let value = authorization(&plain, &creds, "GET", "/doe.json", b"", cnonce, 1).unwrap();
        assert!(value.starts_with("Digest username*=UTF-8''J%C3%A4s%C3%B8n%20Doe, "), "{value}");
        assert_eq!(param(&value, "response"), "3798d4131c277846293534c3edc11bd8a5e4cdcbff78b05db9d95eeb1cec68a5");
    }

    #[test]
    fn rfc2617_example() {
        // RFC 2617 section 3.5: https://www.rfc-editor.org/rfc/rfc2617#section-3.5
        let header = r#"Digest realm="testrealm@host.com", qop="auth,auth-int", nonce="dcd98b7102dd2f0e8b11d0f600bfb0c093", opaque="5ccc069c403ebaf9f0171e9517f40e41""#;
        let challenge = DigestChallenge::from_headers(&[header]).unwrap();
        let creds = DigestCredentials { username: "Mufasa", password: "Circle Of Life" };
        let value = authorization(&challenge, &creds, "GET", "/dir/index.html", b"", "0a4f113b", 1).unwrap();
        assert_eq!(
            value,
            "Digest username=\"Mufasa\", realm=\"testrealm@host.com\", nonce=\"dcd98b7102dd2f0e8b11d0f600bfb0c093\", uri=\"/dir/index.html\", qop=auth, nc=00000001, cnonce=\"0a4f113b\", response=\"6629fae49393a05397450978507c4ef1\", opaque=\"5ccc069c403ebaf9f0171e9517f40e41\""
        );
    }

    #[test]
    fn rfc2069_without_qop() {
        // RFC 2069 section 2.4 (the response printed there is wrong; this is the corrected
        // value, also computed with Python's hashlib): https://www.rfc-editor.org/rfc/rfc2069#section-2.4
        let header = r#"Digest realm="testrealm@host.com", nonce="dcd98b7102dd2f0e8b11d0f600bfb0c093", opaque="5ccc069c403ebaf9f0171e9517f40e41""#;
        let challenge = DigestChallenge::from_headers(&[header]).unwrap();
        assert!(challenge.qop.is_empty());
        let creds = DigestCredentials { username: "Mufasa", password: "CircleOfLife" };
        let value = authorization(&challenge, &creds, "GET", "/dir/index.html", b"", "", 1).unwrap();
        assert_eq!(param(&value, "response"), "1949323746fe6a43ef61f9606e7febea");
        assert!(!value.contains("qop=") && !value.contains("nc=") && !value.contains("cnonce="), "{value}");
    }

    #[test]
    fn auth_int_hashes_the_body_and_sess_mixes_the_nonces() {
        let header = r#"Digest realm="r", nonce="n", qop="auth-int", algorithm=MD5-sess"#;
        let challenge = DigestChallenge::from_headers(&[header]).unwrap();
        assert_eq!(challenge.qop, [Qop::AuthInt]);
        let creds = DigestCredentials { username: "u", password: "p" };
        let value = authorization(&challenge, &creds, "POST", "/x?y=1", b"body", "c", 2).unwrap();
        // python3: md5(ha1 + ":n:00000002:c:auth-int:" + md5("POST:/x?y=1:" + md5("body")))
        // with ha1 = md5(md5("u:r:p") + ":n:c")
        assert_eq!(param(&value, "response"), "1921b94bb437538a61f400e181571c2f");
        assert!(value.contains("qop=auth-int, nc=00000002, cnonce=\"c\""));
        assert!(value.contains("algorithm=MD5-sess"));
    }

    #[test]
    fn quotes_in_values_are_escaped() {
        let challenge = DigestChallenge::from_headers(&[r#"Digest realm="a \"b\"", nonce="n""#]).unwrap();
        assert_eq!(challenge.realm, "a \"b\"");
        let creds = DigestCredentials { username: "we\"ird", password: "" };
        let value = authorization(&challenge, &creds, "GET", "/", b"", "c", 1).unwrap();
        assert!(value.starts_with(r#"Digest username="we\"ird", realm="a \"b\"""#), "{value}");
    }

    #[test]
    fn several_challenges_in_one_header() {
        let found = parse_challenges(&[
            r#"Newauth realm="apps", type=1, title="Login to \"apps\"", Basic realm="simple, really""#,
            "NTLM, Negotiate",
            "NTLM TlRMTVNTUAACAAAADAAMADgAAAA=",
            r#"Digest realm="x",nonce="a,b" , qop=auth,Bearer"#,
        ]);
        let schemes: Vec<&str> = found.iter().map(|c| c.scheme.as_str()).collect();
        assert_eq!(schemes, ["Newauth", "Basic", "NTLM", "Negotiate", "NTLM", "Digest", "Bearer"]);
        assert_eq!(found[0].param("title"), Some("Login to \"apps\""));
        assert_eq!(found[0].param("type"), Some("1"));
        assert_eq!(found[1].param("realm"), Some("simple, really"));
        assert_eq!(found[4].token.as_deref(), Some("TlRMTVNTUAACAAAADAAMADgAAAA="));
        assert_eq!(found[5].param("nonce"), Some("a,b"));
        assert_eq!(found[5].param("QOP"), Some("auth"));
        assert!(parse_challenges(&["", " , "]).is_empty());
    }

    #[test]
    fn challenges_zorvik_cannot_answer() {
        let err = DigestChallenge::from_headers(&[r#"Basic realm="x""#, "NTLM"]).unwrap_err();
        assert_eq!(err.to_string(), "The server didn't ask for Digest authentication (it offered Basic, NTLM).");
        let err = DigestChallenge::from_headers(&[r#"Digest realm="x", nonce="n", algorithm=SHA-1"#]).unwrap_err();
        assert!(err.to_string().contains("algorithm SHA-1"), "{err}");
        let err = DigestChallenge::from_headers(&[r#"Digest realm="x""#]).unwrap_err();
        assert!(err.to_string().contains("no nonce"));
        let err = DigestChallenge::from_headers(&[r#"Digest realm="x", nonce="n", qop="token""#]).unwrap_err();
        assert!(err.to_string().contains("qop"));
        // An unsupported algorithm next to a supported one is skipped.
        let ok = DigestChallenge::from_headers(&[
            r#"Digest realm="x", nonce="n", algorithm=SHA-1"#,
            r#"Digest realm="x", nonce="n", algorithm=MD5"#,
        ])
        .unwrap();
        assert_eq!(ok.algorithm, DigestAlgorithm::Md5);
        let creds = DigestCredentials { username: "u", password: "hunter2" };
        let qop = DigestChallenge::from_headers(&[r#"Digest realm="x", nonce="n", qop=auth"#]).unwrap();
        assert!(authorization(&qop, &creds, "GET", "/", b"", "", 1).is_err(), "qop needs a cnonce");
        assert!(!format!("{creds:?}").contains("hunter2"));
    }
}

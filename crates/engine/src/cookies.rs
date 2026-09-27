//! Cookie jar (RFC 6265 via `cookie_store`) and Set-Cookie parsing for display.

use std::sync::Mutex;

use cookie_store::{CookieDomain, CookieExpiration, CookieStore, RawCookie};
use serde::Serialize;
use ts_rs::TS;
use url::Url;

/// A cookie as shown in the UI (response Cookies tab and the cookie manager).
#[derive(Debug, Clone, Serialize, TS, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CookieInfo {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    /// RFC 3339 expiry, `None` for session cookies.
    pub expires: Option<String>,
    pub secure: bool,
    pub http_only: bool,
    pub same_site: Option<String>,
}

/// Thread-safe cookie jar. Cookies set by responses are sent on later
/// matching requests, including redirect hops.
#[derive(Default)]
pub struct CookieJar {
    store: Mutex<CookieStore>,
}

impl CookieJar {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, CookieStore> {
        self.store.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// `Cookie` header value for a request to `url`, if any cookies match.
    pub fn header_for(&self, url: &Url) -> Option<String> {
        let store = self.lock();
        let pairs: Vec<String> = store.get_request_values(url).map(|(n, v)| format!("{n}={v}")).collect();
        (!pairs.is_empty()).then(|| pairs.join("; "))
    }

    /// Store cookies from `Set-Cookie` header values received from `url`.
    pub fn store(&self, url: &Url, set_cookie_values: &[String]) {
        let cookies = set_cookie_values
            .iter()
            .filter_map(|v| RawCookie::parse(v.clone()).ok())
            .filter(|c| domain_allowed(c, url))
            .map(RawCookie::into_owned)
            .collect::<Vec<_>>();
        if !cookies.is_empty() {
            self.lock().store_response_cookies(cookies.into_iter(), url);
        }
    }

    pub fn list(&self) -> Vec<CookieInfo> {
        let store = self.lock();
        let mut out: Vec<CookieInfo> = store
            .iter_unexpired()
            .map(|c| {
                let domain = match &c.domain {
                    CookieDomain::HostOnly(d) | CookieDomain::Suffix(d) => d.clone(),
                    _ => String::new(),
                };
                let expires = match &c.expires {
                    CookieExpiration::AtUtc(t) => t.format(&time::format_description::well_known::Rfc3339).ok(),
                    CookieExpiration::SessionEnd => None,
                };
                CookieInfo {
                    name: c.name().to_string(),
                    value: c.value().to_string(),
                    domain,
                    path: String::from(c.path.clone()),
                    expires,
                    secure: c.secure().unwrap_or(false),
                    http_only: c.http_only().unwrap_or(false),
                    same_site: c.same_site().map(|s| s.to_string()),
                }
            })
            .collect();
        out.sort_by(|a, b| (&a.domain, &a.name).cmp(&(&b.domain, &b.name)));
        out
    }

    pub fn remove(&self, domain: &str, path: &str, name: &str) -> bool {
        self.lock().remove(domain, path, name).is_some()
    }

    pub fn clear(&self) {
        self.lock().clear();
    }

    /// Serialize persistent, unexpired cookies as JSON.
    pub fn to_json(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let store = self.lock();
        #[allow(deprecated)]
        let _ = cookie_store::serde::json::save(&store, &mut out);
        out
    }

    pub fn from_json(data: &[u8]) -> Self {
        #[allow(deprecated)]
        let store = cookie_store::serde::json::load(std::io::BufReader::new(data)).unwrap_or_default();
        Self { store: Mutex::new(store) }
    }
}

/// Reject `Domain=com`-style cookies, which would be sent to every host under a
/// top-level domain. There is no public suffix list, so this is curl's fallback
/// rule: the domain needs a dot unless it is the request host or `localhost`.
fn domain_allowed(cookie: &RawCookie<'_>, url: &Url) -> bool {
    let Some(domain) = cookie.domain() else { return true };
    let domain = domain.trim().trim_start_matches('.');
    domain.is_empty()
        || domain.contains('.')
        || domain.eq_ignore_ascii_case("localhost")
        || url.host_str().is_some_and(|h| h.eq_ignore_ascii_case(domain))
}

/// Parse `Set-Cookie` header values for display (independent of the jar).
pub fn parse_set_cookies(values: &[String], request_url: &Url) -> Vec<CookieInfo> {
    values
        .iter()
        .filter_map(|v| RawCookie::parse(v.as_str()).ok())
        .map(|c| {
            // A huge Max-Age from the server must not overflow (`+` panics).
            let latest = time::PrimitiveDateTime::MAX.assume_utc();
            let expires = c
                .max_age()
                .map(|age| time::OffsetDateTime::now_utc().checked_add(age).unwrap_or(latest))
                .or_else(|| c.expires_datetime())
                .and_then(|t| t.format(&time::format_description::well_known::Rfc3339).ok());
            CookieInfo {
                name: c.name().to_string(),
                value: c.value().to_string(),
                domain: c
                    .domain()
                    .map(|d| d.trim_start_matches('.').to_string())
                    .unwrap_or_else(|| request_url.host_str().unwrap_or_default().to_string()),
                path: c.path().unwrap_or("/").to_string(),
                expires,
                secure: c.secure().unwrap_or(false),
                http_only: c.http_only().unwrap_or(false),
                same_site: c.same_site().map(|s| s.to_string()),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jar_round_trip() {
        let jar = CookieJar::new();
        let url = Url::parse("https://api.example.com/login").unwrap();
        jar.store(&url, &["sid=abc; Path=/; HttpOnly".into(), "pref=dark; Max-Age=3600".into()]);
        let header = jar.header_for(&Url::parse("https://api.example.com/x").unwrap()).unwrap();
        assert!(header.contains("sid=abc"));
        assert!(header.contains("pref=dark"));
        assert!(jar.header_for(&Url::parse("https://other.com/").unwrap()).is_none());
        assert_eq!(jar.list().len(), 2);

        let restored = CookieJar::from_json(&jar.to_json());
        // Session cookie `sid` is not persisted; `pref` has Max-Age so it is.
        let names: Vec<_> = restored.list().into_iter().map(|c| c.name).collect();
        assert_eq!(names, vec!["pref"]);

        assert!(jar.remove("api.example.com", "/", "sid"));
        jar.clear();
        assert!(jar.list().is_empty());
    }

    #[test]
    fn parses_set_cookie_for_display() {
        let url = Url::parse("http://h.test/").unwrap();
        let c = parse_set_cookies(&["a=1; Secure; SameSite=Lax; Domain=.h.test".into(), "garbage".into()], &url);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].domain, "h.test");
        assert!(c[0].secure);
        assert_eq!(c[0].same_site.as_deref(), Some("Lax"));
    }

    #[test]
    fn huge_max_age_does_not_panic() {
        let url = Url::parse("http://h.test/").unwrap();
        let c = parse_set_cookies(&["a=1; Max-Age=99999999999999999999".into()], &url);
        assert!(c[0].expires.as_deref().unwrap().starts_with("9999-"));
    }

    #[test]
    fn rejects_top_level_domain_cookies() {
        let jar = CookieJar::new();
        jar.store(
            &Url::parse("https://evil.com/").unwrap(),
            &["tld=1; Domain=com".into(), "dot=1; Domain=.com".into(), "own=1; Domain=evil.com".into()],
        );
        assert!(jar.header_for(&Url::parse("https://bank.com/").unwrap()).is_none());
        assert_eq!(jar.header_for(&Url::parse("https://api.evil.com/").unwrap()).as_deref(), Some("own=1"));
        // A single-label host may still name itself.
        jar.store(&Url::parse("http://intranet/").unwrap(), &["x=1; Domain=intranet".into()]);
        assert_eq!(jar.header_for(&Url::parse("http://intranet/").unwrap()).as_deref(), Some("x=1"));
    }
}

//! HTTP proxy selection: manual settings, environment variables and the
//! operating system proxy (Windows registry / macOS `scutil`).
//!
//! Only plain `http://` proxies with optional Basic auth are supported.
//! PAC scripts, SOCKS and NTLM/Kerberos proxy auth are not (see docs/architecture.md, "Networking").

use std::net::IpAddr;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::{EngineError, ErrorKind, Result};

/// Proxy selection mode as chosen in Settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, Default)]
#[serde(tag = "mode", rename_all = "camelCase")]
#[ts(export)]
pub enum ProxyMode {
    /// Connect directly.
    None,
    /// Environment variables, then the OS proxy settings.
    #[default]
    System,
    Manual {
        /// e.g. `http://user:pass@proxy.corp:8080`
        url: String,
        /// Hosts that bypass the proxy (comma or semicolon separated patterns).
        #[serde(default)]
        bypass: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyEndpoint {
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
}

impl ProxyEndpoint {
    pub fn parse(raw: &str) -> Result<Self> {
        let raw = raw.trim();
        // Errors show the URL without its user name and password.
        let shown = match raw.rsplit_once('@') {
            Some((before, after)) => match before.split_once("://") {
                Some((scheme, _)) => format!("{scheme}://…@{after}"),
                None => format!("…@{after}"),
            },
            None => raw.to_string(),
        };
        let with_scheme = if raw.contains("://") { raw.to_string() } else { format!("http://{raw}") };
        let url = url::Url::parse(&with_scheme).map_err(|e| proxy_err(format!("Invalid proxy URL '{shown}': {e}")))?;
        match url.scheme() {
            "http" => {}
            other => {
                return Err(proxy_err(format!("Proxy scheme '{other}' is not supported; use an http:// proxy")));
            }
        }
        let host = url
            .host_str()
            .ok_or_else(|| proxy_err(format!("Proxy URL '{shown}' has no host")))?
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_string();
        Ok(Self {
            host,
            port: url.port().unwrap_or(80),
            username: (!url.username().is_empty()).then(|| percent_decode(url.username())),
            password: url.password().map(percent_decode),
        })
    }

    /// Value for the `Proxy-Authorization` header, if credentials are set.
    pub fn authorization(&self) -> Option<String> {
        let user = self.username.as_deref()?;
        Some(basic_authorization(user, self.password.as_deref().unwrap_or("")))
    }
}

/// `Basic` credentials from a URL's userinfo (`http://user:pass@host`), like curl.
pub(crate) fn userinfo_authorization(url: &url::Url) -> Option<String> {
    if url.username().is_empty() {
        return None;
    }
    Some(basic_authorization(&percent_decode(url.username()), &url.password().map(percent_decode).unwrap_or_default()))
}

fn basic_authorization(user: &str, pass: &str) -> String {
    format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(format!("{user}:{pass}")))
}

fn percent_decode(s: &str) -> String {
    percent_encoding::percent_decode_str(s).decode_utf8_lossy().into_owned()
}

fn proxy_err(message: impl Into<String>) -> EngineError {
    EngineError::new(ErrorKind::Proxy, message)
}

/// Resolved proxy configuration for a request.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProxySettings {
    pub http: Option<ProxyEndpoint>,
    pub https: Option<ProxyEndpoint>,
    pub bypass: Vec<String>,
}

impl ProxySettings {
    pub fn from_mode(mode: &ProxyMode) -> Result<Self> {
        match mode {
            ProxyMode::None => Ok(Self::default()),
            ProxyMode::Manual { url, bypass } => {
                if url.trim().is_empty() {
                    return Ok(Self::default());
                }
                let endpoint = ProxyEndpoint::parse(url)?;
                Ok(Self { http: Some(endpoint.clone()), https: Some(endpoint), bypass: split_list(bypass) })
            }
            ProxyMode::System => Ok(system_proxy()),
        }
    }

    /// The proxy to use for `host` over `https` or plain http, if any.
    pub fn for_target(&self, host: &str, https: bool) -> Option<&ProxyEndpoint> {
        let endpoint = if https { self.https.as_ref() } else { self.http.as_ref() }?;
        if is_loopback(host) || self.bypass.iter().any(|p| bypass_matches(p, host)) {
            return None;
        }
        Some(endpoint)
    }
}

fn split_list(s: &str) -> Vec<String> {
    s.split([',', ';', ' ', '\n']).map(str::trim).filter(|p| !p.is_empty()).map(str::to_string).collect()
}

fn is_loopback(host: &str) -> bool {
    let host = host.trim_start_matches('[').trim_end_matches(']');
    host.eq_ignore_ascii_case("localhost")
        || host.to_ascii_lowercase().ends_with(".localhost")
        || host.parse::<IpAddr>().map(|ip| ip.is_loopback()).unwrap_or(false)
}

/// Match one bypass pattern. Supports curl-style `NO_PROXY` entries
/// (`example.com` also matches subdomains, `.example.com`, `*`), Windows
/// `ProxyOverride` entries (`*.corp.local`, `10.*`, `<local>`), and CIDR ranges.
pub fn bypass_matches(pattern: &str, host: &str) -> bool {
    let host = host.trim_start_matches('[').trim_end_matches(']').to_ascii_lowercase();
    let mut pattern = pattern.trim().to_ascii_lowercase();
    if pattern.is_empty() {
        return false;
    }
    if pattern == "*" {
        return true;
    }
    if pattern == "<local>" {
        return !host.contains('.') && !host.contains(':');
    }
    for scheme in ["http://", "https://"] {
        if let Some(rest) = pattern.strip_prefix(scheme) {
            pattern = rest.to_string();
        }
    }
    if let Some((net, bits)) = pattern.split_once('/') {
        return cidr_contains(net, bits, &host);
    }
    // Strip a port suffix (`host:8080`), but keep bare IPv6 addresses intact.
    if pattern.matches(':').count() == 1 {
        pattern = pattern.split(':').next().unwrap_or_default().to_string();
    }
    let pattern = pattern.trim_start_matches('[').trim_end_matches(']');
    if pattern.contains('*') {
        return glob_match(pattern, &host);
    }
    if let Some(suffix) = pattern.strip_prefix('.') {
        return host == suffix || host.ends_with(&format!(".{suffix}"));
    }
    host == pattern || host.ends_with(&format!(".{pattern}"))
}

fn cidr_contains(net: &str, bits: &str, host: &str) -> bool {
    // macOS writes abbreviated IPv4 networks such as `169.254/16`.
    let dots = net.matches('.').count();
    let net = if !net.contains(':') && dots < 3 { format!("{net}{}", ".0".repeat(3 - dots)) } else { net.to_string() };
    let (Ok(net), Ok(bits), Ok(ip)) = (net.parse::<IpAddr>(), bits.parse::<u32>(), host.parse::<IpAddr>()) else {
        return false;
    };
    match (net, ip) {
        (IpAddr::V4(n), IpAddr::V4(i)) if bits <= 32 => {
            let mask = if bits == 0 { 0 } else { u32::MAX << (32 - bits) };
            u32::from(n) & mask == u32::from(i) & mask
        }
        (IpAddr::V6(n), IpAddr::V6(i)) if bits <= 128 => {
            let mask = if bits == 0 { 0 } else { u128::MAX << (128 - bits) };
            u128::from(n) & mask == u128::from(i) & mask
        }
        _ => false,
    }
}

fn glob_match(pattern: &str, text: &str) -> bool {
    let p = pattern.as_bytes();
    let t = text.as_bytes();
    let (mut pi, mut ti) = (0, 0);
    let (mut star, mut mark) = (None, 0);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == t[ti] || p[pi] == b'?') {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == b'*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == b'*' {
        pi += 1;
    }
    pi == p.len()
}

/// Environment variables first (they are explicit), then OS settings. The OS
/// lookup (registry / `scutil`) runs for every request, so it is cached briefly.
pub fn system_proxy() -> ProxySettings {
    const TTL: Duration = Duration::from_secs(10);
    static OS_CACHE: Mutex<Option<(Instant, ProxySettings)>> = Mutex::new(None);

    let from_env = env_proxy();
    if from_env.http.is_some() || from_env.https.is_some() {
        return from_env;
    }
    let mut cache = OS_CACHE.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some((at, settings)) = cache.as_ref()
        && at.elapsed() < TTL
    {
        return settings.clone();
    }
    let settings = os_proxy().unwrap_or_default();
    *cache = Some((Instant::now(), settings.clone()));
    settings
}

fn env_var(names: &[&str]) -> Option<String> {
    names.iter().find_map(|n| std::env::var(n).ok()).filter(|v| !v.trim().is_empty())
}

fn env_proxy() -> ProxySettings {
    let all = env_var(&["ALL_PROXY", "all_proxy"]);
    let parse = |v: Option<String>| v.and_then(|v| ProxyEndpoint::parse(&v).ok());
    ProxySettings {
        http: parse(env_var(&["HTTP_PROXY", "http_proxy"]).or(all.clone())),
        https: parse(env_var(&["HTTPS_PROXY", "https_proxy"]).or(all)),
        bypass: env_var(&["NO_PROXY", "no_proxy"]).map(|v| split_list(&v)).unwrap_or_default(),
    }
}

#[cfg(windows)]
fn os_proxy() -> Option<ProxySettings> {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;
    let key = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Internet Settings")
        .ok()?;
    let enabled: u32 = key.get_value("ProxyEnable").unwrap_or(0);
    if enabled == 0 {
        return None;
    }
    let server: String = key.get_value("ProxyServer").ok()?;
    let bypass: String = key.get_value("ProxyOverride").unwrap_or_default();
    Some(parse_windows_proxy(&server, &bypass))
}

/// Parse the Windows `ProxyServer` value: either `host:port` or
/// `http=host:port;https=host:port;ftp=...`.
pub fn parse_windows_proxy(server: &str, bypass: &str) -> ProxySettings {
    let mut settings = ProxySettings { bypass: split_list(bypass), ..Default::default() };
    if server.contains('=') {
        for part in server.split(';') {
            if let Some((scheme, addr)) = part.split_once('=') {
                let endpoint = ProxyEndpoint::parse(addr).ok();
                match scheme.trim().to_ascii_lowercase().as_str() {
                    "http" => settings.http = endpoint,
                    "https" => settings.https = endpoint,
                    _ => {}
                }
            }
        }
    } else if let Ok(endpoint) = ProxyEndpoint::parse(server) {
        settings.http = Some(endpoint.clone());
        settings.https = Some(endpoint);
    }
    settings
}

#[cfg(target_os = "macos")]
fn os_proxy() -> Option<ProxySettings> {
    let output = std::process::Command::new("/usr/sbin/scutil").arg("--proxy").output().ok()?;
    Some(parse_scutil(&String::from_utf8_lossy(&output.stdout)))
}

/// Parse `scutil --proxy` output. Only the top-level (effective) dictionary is
/// read; per-interface copies under `__SCOPED__` must not override it.
pub fn parse_scutil(output: &str) -> ProxySettings {
    let mut values = std::collections::HashMap::new();
    let mut exceptions = Vec::new();
    let mut in_exceptions = false;
    let mut depth = 0usize;
    for line in output.lines() {
        let line = line.trim();
        if line == "}" {
            depth = depth.saturating_sub(1);
            in_exceptions = false;
            continue;
        }
        if let Some((k, v)) = line.split_once(" : ") {
            if depth == 1 && k.trim() == "ExceptionsList" {
                in_exceptions = true;
            } else if depth == 1 {
                values.insert(k.trim().to_string(), v.trim().to_string());
            } else if depth == 2 && in_exceptions {
                exceptions.push(v.trim().to_string());
            }
        }
        if line.ends_with('{') {
            depth += 1;
        }
    }
    let endpoint = |prefix: &str| -> Option<ProxyEndpoint> {
        if values.get(&format!("{prefix}Enable")).map(String::as_str) != Some("1") {
            return None;
        }
        let host = values.get(&format!("{prefix}Proxy"))?;
        let port = values.get(&format!("{prefix}Port")).and_then(|p| p.parse().ok()).unwrap_or(80);
        Some(ProxyEndpoint { host: host.clone(), port, username: None, password: None })
    };
    let mut bypass = exceptions;
    if values.get("ExcludeSimpleHostnames").map(String::as_str) == Some("1") {
        bypass.push("<local>".into());
    }
    ProxySettings { http: endpoint("HTTP"), https: endpoint("HTTPS"), bypass }
}

#[cfg(not(any(windows, target_os = "macos")))]
fn os_proxy() -> Option<ProxySettings> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_hide_the_password() {
        let e = ProxyEndpoint::parse("http://ada:s3cret@proxy.local:99999").unwrap_err();
        assert!(!e.message.contains("s3cret") && e.message.contains("proxy.local"), "{}", e.message);
        let e = ProxyEndpoint::parse("ada:s3cret@:8080").unwrap_err();
        assert!(!e.message.contains("s3cret"), "{}", e.message);
    }

    #[test]
    fn parses_endpoint_with_credentials() {
        let e = ProxyEndpoint::parse("http://us%40er:p%3Ass@proxy.corp:3128").unwrap();
        assert_eq!(e.host, "proxy.corp");
        assert_eq!(e.port, 3128);
        assert_eq!(e.username.as_deref(), Some("us@er"));
        assert_eq!(e.password.as_deref(), Some("p:ss"));
        assert!(e.authorization().unwrap().starts_with("Basic "));
        let bare = ProxyEndpoint::parse("10.0.0.1:8080").unwrap();
        assert_eq!((bare.host.as_str(), bare.port), ("10.0.0.1", 8080));
        assert!(ProxyEndpoint::parse("socks5://x:1").is_err());
    }

    #[test]
    fn bypass_patterns() {
        assert!(bypass_matches("example.com", "api.example.com"));
        assert!(bypass_matches("example.com", "example.com"));
        assert!(!bypass_matches("example.com", "badexample.com"));
        assert!(bypass_matches(".corp", "x.corp"));
        assert!(bypass_matches("*.corp.local", "a.corp.local"));
        assert!(bypass_matches("10.*", "10.1.2.3"));
        assert!(bypass_matches("<local>", "intranet"));
        assert!(!bypass_matches("<local>", "intranet.corp"));
        assert!(bypass_matches("192.168.0.0/16", "192.168.4.5"));
        assert!(!bypass_matches("192.168.0.0/16", "192.169.4.5"));
        // macOS default exception.
        assert!(bypass_matches("169.254/16", "169.254.10.20"));
        assert!(!bypass_matches("169.254/16", "169.255.0.1"));
        assert!(bypass_matches("host:8080", "host"));
        assert!(bypass_matches("*", "anything"));
    }

    #[test]
    fn loopback_never_proxied() {
        let s = ProxySettings::from_mode(&ProxyMode::Manual { url: "proxy:1".into(), bypass: String::new() }).unwrap();
        assert!(s.for_target("localhost", false).is_none());
        assert!(s.for_target("127.0.0.1", true).is_none());
        assert!(s.for_target("[::1]", true).is_none());
        assert!(s.for_target("example.com", true).is_some());
    }

    #[test]
    fn windows_proxy_formats() {
        let s = parse_windows_proxy("http=a:1;https=b:2;ftp=c:3", "<local>;*.corp");
        assert_eq!(s.http.unwrap().host, "a");
        assert_eq!(s.https.unwrap().port, 2);
        assert_eq!(s.bypass, vec!["<local>", "*.corp"]);
        let s = parse_windows_proxy("proxy:8080", "");
        assert_eq!(s.https.unwrap().host, "proxy");
    }

    #[test]
    fn scutil_output() {
        let out = "<dictionary> {\n  ExceptionsList : <array> {\n    0 : *.local\n    1 : 169.254/16\n  }\n  HTTPEnable : 1\n  HTTPPort : 3128\n  HTTPProxy : proxy.corp\n  HTTPSEnable : 0\n}\n";
        let s = parse_scutil(out);
        assert_eq!(s.http.unwrap().port, 3128);
        assert!(s.https.is_none());
        assert_eq!(s.bypass, vec!["*.local", "169.254/16"]);
    }

    #[test]
    fn scutil_scoped_dictionaries_do_not_override_top_level() {
        let out = "<dictionary> {\n  ExceptionsList : <array> {\n    0 : *.local\n  }\n  HTTPEnable : 1\n  HTTPPort : 3128\n  HTTPProxy : proxy.corp\n  __SCOPED__ : <dictionary> {\n    en7 : <dictionary> {\n      ExceptionsList : <array> {\n        0 : other.corp\n      }\n      HTTPEnable : 0\n      HTTPProxy : stale.corp\n    }\n  }\n}\n";
        let s = parse_scutil(out);
        assert_eq!(s.http.unwrap().host, "proxy.corp");
        assert_eq!(s.bypass, vec!["*.local"]);
    }
}

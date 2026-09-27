//! App-wide settings (stored in the app data dir as settings.json).

use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use zorvik_engine::{HttpVersionPref, ProxyMode, ProxySettings, RequestOptions, TlsOptions};
use zorvik_formats::RequestSettings;

use crate::error::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct RequestDefaults {
    /// Whole-request timeout; 0 = no limit.
    #[ts(type = "number")]
    pub timeout_ms: u64,
    #[ts(type = "number")]
    pub connect_timeout_ms: u64,
    pub follow_redirects: bool,
    pub max_redirects: u32,
    pub verify_tls: bool,
    pub http_version: HttpVersionPref,
    pub decompress: bool,
    /// Responses larger than this are truncated.
    pub max_response_mb: u32,
    /// Add User-Agent, Accept and Accept-Encoding automatically.
    pub send_default_headers: bool,
}

impl Default for RequestDefaults {
    fn default() -> Self {
        Self {
            timeout_ms: 60_000,
            connect_timeout_ms: 15_000,
            follow_redirects: true,
            max_redirects: 10,
            verify_tls: true,
            http_version: HttpVersionPref::Auto,
            decompress: true,
            max_response_mb: 100,
            send_default_headers: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct TlsSettings {
    /// Extra trusted CA certificate (PEM). Empty = OS trust store only.
    pub ca_cert_path: String,
    pub client_cert_path: String,
    pub client_key_path: String,
}

/// Whether edits by AI agents need the user's approval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum AgentChanges {
    #[default]
    Allow,
    Ask,
}

/// Whether requests sent by AI agents need the user's approval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum AgentTraffic {
    Allow,
    /// Ask once per host outside this computer and private networks.
    #[default]
    AskOutside,
    Ask,
}

/// AI agents controlling the app over MCP (docs/architecture.md, "AI agents").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct AgentSettings {
    /// Off: the first tool call asks the user to turn it on.
    pub enabled: bool,
    pub changes: AgentChanges,
    pub traffic: AgentTraffic,
    /// Open what an agent works on (tabs, responses, runs).
    pub follow: bool,
    /// Run agent tools without the app when it is closed (nothing can be approved then).
    pub headless: bool,
}

impl Default for AgentSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            changes: AgentChanges::Allow,
            traffic: AgentTraffic::AskOutside,
            follow: true,
            headless: false,
        }
    }
}

/// Zoom and fonts of the app window (Settings → General).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct Appearance {
    /// Page zoom in percent (⌘+ / ⌘− / ⌘0).
    pub zoom: u32,
    /// Font of the interface; empty = the system font.
    pub ui_font: String,
    /// Font of editors, responses and other code; empty = the system's monospace font.
    pub code_font: String,
    /// Text size in editors and response bodies, in CSS pixels (before zoom).
    pub code_font_size: u32,
    /// Show programming ligatures (`=>` as one glyph) when the code font has them.
    pub ligatures: bool,
}

impl Default for Appearance {
    fn default() -> Self {
        Self { zoom: 100, ui_font: String::new(), code_font: String::new(), code_font_size: 13, ligatures: false }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct Settings {
    pub theme: Theme,
    pub appearance: Appearance,
    pub request: RequestDefaults,
    pub proxy: ProxyMode,
    pub tls: TlsSettings,
    /// History entries kept per workspace.
    pub history_limit: u32,
    /// Keep cookies from responses and send them on later requests.
    pub cookie_jar: bool,
    /// Let requests send body files from outside the workspace folder.
    pub files_outside_workspace: bool,
    /// Time limit of one pre-request or post-response script.
    #[ts(type = "number")]
    pub script_timeout_ms: u64,
    pub agents: AgentSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Theme::System,
            appearance: Appearance::default(),
            request: RequestDefaults::default(),
            proxy: ProxyMode::System,
            tls: TlsSettings::default(),
            history_limit: 500,
            cookie_jar: true,
            files_outside_workspace: false,
            script_timeout_ms: 5_000,
            agents: AgentSettings::default(),
        }
    }
}

fn opt_path(s: &str) -> Option<std::path::PathBuf> {
    let s = s.trim();
    (!s.is_empty()).then(|| s.into())
}

impl Settings {
    /// Load settings; a missing or unreadable file yields defaults.
    pub fn load(path: &Path) -> Self {
        std::fs::read(path).ok().and_then(|d| serde_json::from_slice(&d).ok()).unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        crate::store::save_json(path, self)
    }

    /// Time limit of one script (100 ms to 60 s).
    pub fn script_timeout(&self) -> Duration {
        Duration::from_millis(self.script_timeout_ms.clamp(100, 60_000))
    }

    /// Engine options for a request, applying its per-request overrides.
    pub fn request_options(&self, overrides: &RequestSettings) -> Result<RequestOptions> {
        let d = &self.request;
        let timeout_ms = overrides.timeout_ms.unwrap_or(d.timeout_ms);
        Ok(RequestOptions {
            timeout: (timeout_ms > 0).then(|| Duration::from_millis(timeout_ms)),
            connect_timeout: Duration::from_millis(d.connect_timeout_ms.max(100)),
            follow_redirects: overrides.follow_redirects.unwrap_or(d.follow_redirects),
            max_redirects: overrides.max_redirects.unwrap_or(d.max_redirects),
            http_version: overrides.http_version.unwrap_or(d.http_version),
            tls: TlsOptions {
                verify: overrides.verify_tls.unwrap_or(d.verify_tls),
                ca_cert_path: opt_path(&self.tls.ca_cert_path),
                client_cert_path: opt_path(&self.tls.client_cert_path),
                client_key_path: opt_path(&self.tls.client_key_path),
            },
            proxy: ProxySettings::from_mode(&self.proxy)?,
            decompress: overrides.decompress.unwrap_or(d.decompress),
            max_body_bytes: (d.max_response_mb.clamp(1, 2048) as usize) * 1024 * 1024,
            default_headers: d.send_default_headers,
            host_guard: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overrides_and_partial_files() {
        let s: Settings = serde_json::from_str(r#"{"request":{"timeoutMs":0},"historyLimit":7}"#).unwrap();
        assert_eq!(s.history_limit, 7);
        assert_eq!(s.appearance, Appearance::default(), "older files have no appearance");
        assert_eq!(s.script_timeout(), Duration::from_secs(5));
        assert!(s.request.follow_redirects);
        let o = s.request_options(&RequestSettings::default()).unwrap();
        assert!(o.timeout.is_none());
        let o = s
            .request_options(&RequestSettings { timeout_ms: Some(5), verify_tls: Some(false), ..Default::default() })
            .unwrap();
        assert_eq!(o.timeout, Some(Duration::from_millis(5)));
        assert!(!o.tls.verify);
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        assert_eq!(Settings::load(&p), Settings::default());
        s.save(&p).unwrap();
        assert_eq!(Settings::load(&p), s);
        std::fs::write(&p, "garbage").unwrap();
        assert_eq!(Settings::load(&p), Settings::default());
    }
}

use serde::Serialize;
use ts_rs::TS;

/// Broad failure category. The UI uses it to pick an icon and a hint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ErrorKind {
    InvalidRequest,
    Dns,
    Connect,
    Tls,
    Proxy,
    Timeout,
    Protocol,
    Io,
    Cancelled,
    TooManyRedirects,
    /// The host needs the user's approval first (requests made by AI agents).
    NotAllowed,
}

#[derive(Debug, Clone, thiserror::Error, Serialize, TS)]
#[error("{message}")]
#[ts(export)]
pub struct EngineError {
    pub kind: ErrorKind,
    pub message: String,
}

impl EngineError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self { kind, message: message.into() }
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::InvalidRequest, message)
    }

    pub fn timeout(what: &str, after: std::time::Duration) -> Self {
        Self::new(ErrorKind::Timeout, format!("{what} timed out after {}", human_duration(after)))
    }

    pub fn cancelled() -> Self {
        Self::new(ErrorKind::Cancelled, "Request cancelled")
    }

    /// Classify a hyper error that happened after the connection was established.
    pub(crate) fn from_hyper(err: hyper::Error) -> Self {
        let mut message = err.to_string();
        let mut source = std::error::Error::source(&err);
        while let Some(s) = source {
            message.push_str(": ");
            message.push_str(&s.to_string());
            source = s.source();
        }
        let kind = if err.is_timeout() {
            ErrorKind::Timeout
        } else if err.is_parse() || err.is_user() {
            ErrorKind::Protocol
        } else {
            ErrorKind::Io
        };
        Self::new(kind, message)
    }
}

pub type Result<T> = std::result::Result<T, EngineError>;

pub(crate) fn human_duration(d: std::time::Duration) -> String {
    let ms = d.as_millis();
    if ms >= 1000 && ms.is_multiple_of(1000) { format!("{}s", ms / 1000) } else { format!("{ms}ms") }
}

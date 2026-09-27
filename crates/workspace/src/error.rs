use serde::Serialize;
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ErrorCode {
    NotFound,
    AlreadyExists,
    InvalidInput,
    NotAWorkspace,
    Parse,
    Io,
    Network,
    Auth,
    /// A `{{variable}}` needed to send the request is not defined.
    UndefinedVariable,
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("{message}")]
pub struct Error {
    pub code: ErrorCode,
    pub message: String,
    /// Set when the failure came from the network engine.
    pub engine: Option<zorvik_engine::EngineError>,
}

impl Error {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self { code, message: message.into(), engine: None }
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidInput, message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::NotFound, message)
    }

    pub fn io(context: impl std::fmt::Display, err: std::io::Error) -> Self {
        let code = match err.kind() {
            std::io::ErrorKind::NotFound => ErrorCode::NotFound,
            std::io::ErrorKind::AlreadyExists => ErrorCode::AlreadyExists,
            _ => ErrorCode::Io,
        };
        Self::new(code, format!("{context}: {err}"))
    }
}

impl From<zorvik_engine::EngineError> for Error {
    fn from(e: zorvik_engine::EngineError) -> Self {
        Self { code: ErrorCode::Network, message: e.message.clone(), engine: Some(e) }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

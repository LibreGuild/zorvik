//! Workspace layer: collections on disk, environments, variables, auth,
//! request resolution, OAuth 2.0, history, settings, secrets and script-set values.

pub mod auth;
pub mod dynamic;
pub mod error;
pub mod fsutil;
pub mod history;
pub mod localvalues;
pub mod oauth2;
pub mod resolve;
pub mod secrets;
pub mod settings;
pub mod signing;
pub mod store;
pub mod vars;

pub use error::{Error, ErrorCode, Result};
pub use store::{EnvironmentEntry, Workspace};
pub use zorvik_formats as formats;

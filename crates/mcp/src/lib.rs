//! MCP server for AI agents (docs/architecture.md, "AI agents").
//!
//! - [`bridge`]: `zorvik mcp`, the process an agent starts. Speaks MCP on
//!   stdio, answers the handshake and lists itself, and forwards tool calls to
//!   the running app (starting it when needed), or runs them itself when
//!   headless use is allowed.
//! - [`listener`]: the app's side: a local socket the bridge connects to with
//!   the token from `agent.json`.

pub mod bridge;
mod discovery;
mod exec;
pub mod listener;
mod protocol;

pub use discovery::{AgentFile, default_data_dir};
pub use protocol::PROTOCOL_VERSIONS;

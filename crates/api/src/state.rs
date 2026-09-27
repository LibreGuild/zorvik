//! Local, per-machine state (app data dir): recent workspaces and the active
//! environment per workspace. Never stored inside a workspace.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::RecentWorkspace;

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LocalState {
    pub recent: Vec<RecentWorkspace>,
    /// Workspace id -> environment id.
    pub active_env: HashMap<String, String>,
    pub last_workspace: Option<String>,
}

impl LocalState {
    pub fn load(path: &Path) -> Self {
        std::fs::read(path).ok().and_then(|d| serde_json::from_slice(&d).ok()).unwrap_or_default()
    }

    pub fn save(&self, path: &Path) {
        if let Err(e) = zorvik_workspace::store::save_json(path, self) {
            tracing::warn!("could not save local state: {}", e.message);
        }
    }
}

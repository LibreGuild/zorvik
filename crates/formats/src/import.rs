//! Common output of all importers: a collection tree ready to be written into a workspace.

use serde::Serialize;
use ts_rs::TS;

use crate::model::{Auth, Environment, FolderMeta, KeyValue, Request, Scripts, Variable};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct ImportError(pub String);

impl ImportError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

/// A folder or request produced by an importer. Order in `children`/`items` is the display order.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportedItem {
    Folder { meta: FolderMeta, children: Vec<ImportedItem> },
    Request(Request),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ImportedCollection {
    pub name: String,
    /// Collection-level variables that become an environment named after the collection
    /// (e.g. OpenAPI servers, so the user can switch between them).
    pub variables: Vec<Variable>,
    /// Postman collection variables: added to the workspace variables, which is what
    /// `pm.collectionVariables` reads and writes (keys the workspace has are kept).
    pub workspace_variables: Vec<Variable>,
    /// Collection-level auth (applies to requests with `Auth::Inherit`).
    pub auth: Auth,
    /// Collection-level headers.
    pub headers: Vec<KeyValue>,
    /// Collection-level scripts (they run before/after every request in it).
    pub scripts: Scripts,
    pub items: Vec<ImportedItem>,
    /// Things that could not be imported faithfully (shown to the user).
    pub warnings: Vec<String>,
}

impl ImportedCollection {
    /// Number of requests in the tree.
    pub fn request_count(&self) -> usize {
        fn count(items: &[ImportedItem]) -> usize {
            items
                .iter()
                .map(|i| match i {
                    ImportedItem::Folder { children, .. } => count(children),
                    ImportedItem::Request(_) => 1,
                })
                .sum()
        }
        count(&self.items)
    }
}

/// Summary returned to the UI after an import.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ImportSummary {
    pub name: String,
    pub requests: u32,
    pub folders: u32,
    pub environments: u32,
    /// Collection variables added to the workspace variables.
    pub workspace_variables: u32,
    pub warnings: Vec<String>,
    /// Path of the created top-level folder (relative to requests/), if any.
    pub folder_path: Option<String>,
}

/// Result of importing a Postman environment file.
pub type ImportedEnvironment = Environment;

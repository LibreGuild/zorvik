//! OpenAPI documents kept in the workspace (`specs/`): checking responses of requests imported
//! from them against the documented schemas. Parsed documents are cached until the file changes.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use serde_json::Value;
use zorvik_engine::HttpResponse;
use zorvik_script::TestResult;
use zorvik_workspace::Workspace;
use zorvik_workspace::formats::spec_check::check_response;
use zorvik_workspace::formats::{FolderMeta, Request, RequestKind};

use crate::lock;

/// The folder specs are kept in, inside the workspace folder.
pub const SPECS_DIR: &str = "specs";
/// Largest spec file read.
const MAX_SPEC_BYTES: u64 = 50 * 1024 * 1024;
/// Parsed documents kept.
const MAX_CACHED: usize = 16;

/// A parsed document and the file's modification time and size when it was read.
type Cached = (SystemTime, u64, Arc<Value>);

/// Parsed OpenAPI documents by file, while the file is unchanged.
#[derive(Default)]
pub struct SpecCache {
    docs: Mutex<HashMap<PathBuf, Cached>>,
}

impl SpecCache {
    /// The document at `spec` (relative to the workspace folder), when it is a readable
    /// OpenAPI document inside the workspace.
    pub fn get(&self, ws: &Workspace, spec: &str) -> Option<Arc<Value>> {
        let path = spec_path(ws.root(), spec)?;
        let meta = std::fs::metadata(&path).ok()?;
        if !meta.is_file() || meta.len() > MAX_SPEC_BYTES {
            return None;
        }
        let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        if let Some((m, len, doc)) = lock(&self.docs).get(&path)
            && *m == modified
            && *len == meta.len()
        {
            return Some(doc.clone());
        }
        let text = std::fs::read_to_string(&path).ok()?;
        let doc = Arc::new(zorvik_workspace::formats::openapi::parse_document(&text).ok()?);
        let mut docs = lock(&self.docs);
        if docs.len() >= MAX_CACHED {
            docs.clear();
        }
        docs.insert(path, (modified, meta.len(), doc.clone()));
        Some(doc)
    }
}

/// `spec` as a path inside the workspace folder: relative, no `..`, no links on the way.
pub(crate) fn spec_path(root: &Path, spec: &str) -> Option<PathBuf> {
    let spec = spec.trim().replace('\\', "/");
    let relative = Path::new(&spec);
    if spec.is_empty() || relative.is_absolute() {
        return None;
    }
    let mut at = root.to_path_buf();
    for part in relative.components() {
        match part {
            std::path::Component::Normal(name) => at.push(name),
            std::path::Component::CurDir => {}
            _ => return None,
        }
        if zorvik_workspace::fsutil::is_symlink(&at) {
            return None;
        }
    }
    Some(at)
}

/// The response checked against the OpenAPI operation `request` was imported from, as a test
/// result; `None` when there is nothing to check (not imported, switched off, not found).
pub(crate) fn spec_test(
    specs: &SpecCache,
    ws: &Workspace,
    folders: &[FolderMeta],
    request: &Request,
    response: &HttpResponse,
) -> Option<TestResult> {
    if request.kind != RequestKind::Http {
        return None;
    }
    let operation = request.openapi.as_ref().filter(|o| !o.removed)?;
    // The innermost folder that knows the document.
    let source = folders.iter().rev().find_map(|f| f.openapi.as_ref())?;
    if !source.validate {
        return None;
    }
    let doc = specs.get(ws, &source.spec)?;
    let content_type =
        response.meta.headers.iter().find(|h| h.name.eq_ignore_ascii_case("content-type")).map(|h| h.value.as_str());
    let check = check_response(&doc, &operation.operation, response.meta.status, content_type, &response.body)?;
    let passed = check.passed();
    let error = (!passed).then(|| check.problems.join("\n"));
    Some(TestResult { name: format!("Matches the API spec ({})", check.label), passed, skipped: false, error })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_paths_stay_inside() {
        let root = Path::new("/ws");
        assert_eq!(spec_path(root, "specs/a.json"), Some(PathBuf::from("/ws/specs/a.json")));
        for bad in ["", "/etc/x", "../x", "specs/../../x"] {
            assert_eq!(spec_path(root, bad), None, "{bad}");
        }
    }
}

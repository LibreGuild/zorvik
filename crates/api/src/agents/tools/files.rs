//! Tools that put things into files: a small file into the workspace (a sample upload, a
//! data file for a run) and a request as a cURL command or code.

use std::path::{Component, Path, PathBuf};

use base64::Engine as _;
use serde::Deserialize;
use serde_json::json;
use zorvik_workspace::Workspace;

use super::{Call, Def, Done, Fail, Outcome, human_size, obj, string};
use crate::Api;

/// Largest file an agent may add.
const MAX_FILE_BYTES: usize = 10 * 1024 * 1024;
/// Folders and files the workspace's own tools own (a request file is saved with save_requests).
const OWNED: [&str; 6] = ["zorvik.yaml", "requests", "environments", "servers", "loadtests", ".git"];

pub(super) fn defs() -> Vec<Def> {
    vec![
        Def {
            name: "export_request",
            title: "Export as code",
            description: "A request as a ready-to-run cURL command (bash, Windows cmd or PowerShell) or code: Kotlin (OkHttp, for Android), Swift (URLSession), JavaScript (fetch) or Python (requests). Give path (a saved request) or request (unsaved), like send_request. Variables are filled in unless resolveVariables is false; secret values come back as •••••• (the user can copy the full version in Zorvik: Copy as cURL or code).",
            schema: obj(
                json!({
                    "path": string("Saved request path as list_requests shows it."),
                    "request": super::request_schema(),
                    "folder": string("Unsaved request: the folder whose auth and headers it inherits."),
                    "format": { "type": "string", "enum": ["curl", "curlCmd", "curlPowerShell", "kotlin", "swift", "javascript", "python"], "description": "Default curl (bash/zsh)." },
                    "resolveVariables": { "type": "boolean", "description": "Fill in {{variables}} (default true); false keeps them as written." },
                }),
                &[],
            ),
            read_only: true,
            destructive: false,
            open_world: false,
        },
        Def {
            name: "write_file",
            title: "Add a file",
            description: "Put a small file (up to 10 MB) into the workspace folder: a sample image for an upload request, a CSV or JSON data file for run_collection, a .proto file. Give text, or base64 for binary content. Requests, environments, servers and load tests are saved with their own tools, so their folders (and zorvik.yaml, .git) can't be written here. An existing file is replaced only with overwrite: true.",
            schema: obj(
                json!({
                    "path": string("Path inside the workspace folder, with / between folders, e.g. \"fixtures/avatar.png\" or \"data/users.csv\"."),
                    "text": string("The file's content as text (UTF-8)."),
                    "base64": string("The file's content as base64, for binary files."),
                    "overwrite": { "type": "boolean", "description": "Replace the file when it exists (default false)." },
                }),
                &["path"],
            ),
            read_only: false,
            destructive: false,
            open_world: false,
        },
    ]
}

/// Where `path` goes inside the workspace, or why it can't go there.
fn target_path(root: &Path, path: &str) -> Result<PathBuf, String> {
    let path = path.trim().replace('\\', "/");
    let relative = Path::new(&path);
    if path.is_empty() || relative.is_absolute() || path.starts_with('/') {
        return Err("path must be relative to the workspace folder, e.g. fixtures/avatar.png".into());
    }
    let mut parts = Vec::new();
    for c in relative.components() {
        match c {
            Component::Normal(name) => {
                let name = name.to_string_lossy();
                if zorvik_workspace::fsutil::is_reserved_name(&name) || name.ends_with('.') || name.ends_with(' ') {
                    return Err(format!("\"{name}\" can't be a file or folder name on every system"));
                }
                parts.push(name.into_owned());
            }
            Component::CurDir => {}
            _ => return Err("path can't leave the workspace folder (no .. or drive letters)".into()),
        }
    }
    let Some(first) = parts.first() else { return Err("path names no file".into()) };
    if OWNED.iter().any(|o| first.eq_ignore_ascii_case(o)) {
        return Err(format!(
            "{first} belongs to Zorvik: save requests, environments, servers and load tests with their own tools"
        ));
    }
    // Links could lead out of the workspace: none on the way, and not the file itself.
    let mut at = root.to_path_buf();
    for part in &parts {
        at.push(part);
        if zorvik_workspace::fsutil::is_symlink(&at) {
            return Err(format!("{} is a link; Zorvik doesn't write through links", at.display()));
        }
    }
    Ok(at)
}

impl Api {
    pub(super) fn tool_export_request(&self, c: &Call<'_>) -> Outcome {
        use zorvik_workspace::formats::curl::{CurlFlavor, to_curl};
        use zorvik_workspace::formats::snippet::{SnippetLanguage, to_snippet};
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct A {
            format: Option<String>,
            resolve_variables: Option<bool>,
        }
        let A { format, resolve_variables } = c.args()?;
        let ws = self.agent_ws()?;
        let (request, path) = self.request_to_send(&ws, c)?;
        let http = self
            .request_for_export(&request, path.as_deref(), resolve_variables.unwrap_or(true))
            .map_err(Fail::from)?;
        let format = format.unwrap_or_else(|| "curl".into());
        let text = match format.as_str() {
            "curl" => to_curl(&http, CurlFlavor::Bash),
            "curlCmd" => to_curl(&http, CurlFlavor::Cmd),
            "curlPowerShell" => to_curl(&http, CurlFlavor::PowerShell),
            "kotlin" => to_snippet(&http, SnippetLanguage::Kotlin),
            "swift" => to_snippet(&http, SnippetLanguage::Swift),
            "javascript" => to_snippet(&http, SnippetLanguage::JavaScript),
            "python" => to_snippet(&http, SnippetLanguage::Python),
            other => {
                return Err(Fail::Invalid(format!(
                    "format: unknown \"{other}\" (curl, curlCmd, curlPowerShell, kotlin, swift, javascript, python)"
                )));
            }
        };
        Ok(Done::new(json!({ "format": format, "code": text }), format!("{} as {format}", request.name)))
    }

    pub(super) async fn tool_write_file(&self, c: &Call<'_>) -> Outcome {
        #[derive(Deserialize)]
        struct A {
            path: String,
            text: Option<String>,
            base64: Option<String>,
            #[serde(default)]
            overwrite: bool,
        }
        let A { path, text, base64, overwrite } = c.args()?;
        let data = match (text, base64) {
            (Some(text), None) => text.into_bytes(),
            (None, Some(b64)) => base64::engine::general_purpose::STANDARD
                .decode(b64.trim())
                .map_err(|e| Fail::Invalid(format!("base64: {e}")))?,
            _ => return Err(Fail::Invalid("Give text or base64 (one of them)".into())),
        };
        if data.len() > MAX_FILE_BYTES {
            return Err(Fail::Invalid(format!("The file is {}; at most 10 MB", human_size(data.len()))));
        }
        let ws: Workspace = self.agent_ws()?;
        let full = target_path(ws.root(), &path).map_err(Fail::Invalid)?;
        let exists = full.exists();
        if exists && !full.is_file() {
            return Err(Fail::Invalid(format!("{path} is a folder")));
        }
        if exists && !overwrite {
            return Err(Fail::Invalid(format!("{path} already exists; call again with overwrite: true to replace it")));
        }
        let verb = if exists { "Replace" } else { "Add" };
        self.gate_change(
            c,
            format!("{verb} a file in the workspace?"),
            vec![format!("{path} ({})", human_size(data.len()))],
        )
        .await?;
        let size = data.len();
        let write_to = full.clone();
        tokio::task::spawn_blocking(move || -> Result<(), String> {
            if let Some(parent) = write_to.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("Could not create {}: {e}", parent.display()))?;
            }
            zorvik_workspace::fsutil::atomic_write(&write_to, &data).map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| Fail::Error(e.to_string()))?
        .map_err(Fail::Error)?;
        let relative = full.strip_prefix(ws.root()).unwrap_or(&full).to_string_lossy().replace('\\', "/");
        let done = if exists { "Replaced" } else { "Added" };
        Ok(Done::new(
            json!({ "path": relative, "size": size, "replaced": exists }),
            format!("{done} {relative} ({})", human_size(size)),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_stay_inside_and_off_zorviks_folders() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        assert_eq!(target_path(root, "fixtures/a.png").unwrap(), root.join("fixtures").join("a.png"));
        assert_eq!(target_path(root, "./data\\users.csv").unwrap(), root.join("data").join("users.csv"));
        for bad in
            ["", "/etc/passwd", "../x", "a/../../x", "requests/x.yaml", "Zorvik.yaml", ".git/config", "con.txt", "a./b"]
        {
            assert!(target_path(root, bad).is_err(), "{bad}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn links_are_not_followed() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("out")).unwrap();
        assert!(target_path(dir.path(), "out/x.txt").unwrap_err().contains("link"));
    }
}

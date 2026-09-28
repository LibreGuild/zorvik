//! Updating a folder imported from an OpenAPI document from a new version of the document.
//! New operations are added; changed ones are updated field by field where the user left
//! the field as the old document had it (their edits win); operations the document no longer
//! has are kept and marked. The kept copy of the old document is what tells the two apart.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use zorvik_workspace::formats::import::{ImportedCollection, ImportedItem};
use zorvik_workspace::formats::openapi::import_openapi;
use zorvik_workspace::formats::{Environment, NodeKind, OpenApiOperation, Request, TreeNode, Variable};

use crate::{Api, ApiError, ApiResult, join_err};

/// Request fields an update may change. Scripts, settings and the name are always the user's.
const FIELDS: [&str; 9] =
    ["method", "url", "headers", "body", "pathParams", "disabledParams", "paramDescriptions", "auth", "docs"];
/// Fields taken from the new document when there is no kept old one to compare with.
const SPEC_OWNED: [&str; 7] = ["method", "url", "headers", "body", "pathParams", "disabledParams", "paramDescriptions"];

/// Where the new version comes from.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpecUpdateParams {
    /// The folder imported from the document (it keeps a link to it).
    pub folder: String,
    pub text: Option<String>,
    pub path: Option<String>,
    pub url: Option<String>,
}

/// One operation in an update.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SpecChange {
    /// `METHOD /path` as the document has it.
    pub operation: String,
    pub name: String,
    /// The saved request (none yet for added ones).
    pub path: Option<String>,
    /// Fields the update changes.
    pub fields: Vec<String>,
    /// Fields both the document and the user changed: the user's version stays.
    pub kept: Vec<String>,
}

/// What an update does (or would do: the preview).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SpecUpdate {
    pub added: Vec<SpecChange>,
    pub changed: Vec<SpecChange>,
    /// No longer in the document: kept and marked.
    pub removed: Vec<SpecChange>,
    /// Back in the document after being marked removed.
    pub restored: Vec<SpecChange>,
    pub unchanged: u32,
    /// Variables added to the folder's environment (path parameters of new operations).
    pub variables: Vec<String>,
    pub warnings: Vec<String>,
    /// Whether it was written (false for a preview).
    pub applied: bool,
}

/// A planned write.
enum Write {
    Save { path: String, request: Request },
    Create { folders: Vec<String>, request: Request },
}

/// Operations of an imported collection: request and the folder names it sits in.
fn operations(collection: &ImportedCollection) -> Vec<(String, Vec<String>, Request)> {
    fn walk(items: &[ImportedItem], folders: &mut Vec<String>, out: &mut Vec<(String, Vec<String>, Request)>) {
        for item in items {
            match item {
                ImportedItem::Folder { meta, children } => {
                    folders.push(meta.name.clone());
                    walk(children, folders, out);
                    folders.pop();
                }
                ImportedItem::Request(r) => {
                    if let Some(op) = &r.openapi {
                        out.push((op.operation.clone(), folders.clone(), r.clone()));
                    }
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(&collection.items, &mut Vec::new(), &mut out);
    out
}

/// `saved` updated from `new`, using `old` (the request the old document made) to tell the
/// user's edits from the document's changes. Returns the request, the fields changed and
/// the fields kept because the user changed them too.
fn merge(saved: &Request, old: Option<&Request>, new: &Request) -> (Request, Vec<String>, Vec<String>) {
    let saved_v = serde_json::to_value(saved).unwrap_or_default();
    let old_v = old.map(|o| serde_json::to_value(o).unwrap_or_default());
    let new_v = serde_json::to_value(new).unwrap_or_default();
    let mut merged = saved_v.clone();
    let (mut fields, mut kept) = (Vec::new(), Vec::new());
    for key in FIELDS {
        let (s, n) = (saved_v.get(key), new_v.get(key));
        if s == n {
            continue;
        }
        let take = match &old_v {
            Some(o) => {
                let b = o.get(key);
                if n == b {
                    false
                } else if s == b {
                    true
                } else {
                    kept.push(key.to_string());
                    false
                }
            }
            None => SPEC_OWNED.contains(&key),
        };
        if take {
            match n {
                Some(v) => merged[key] = v.clone(),
                None => {
                    merged.as_object_mut().map(|m| m.remove(key));
                }
            }
            fields.push(key.to_string());
        }
    }
    let request = serde_json::from_value(merged).unwrap_or_else(|_| saved.clone());
    (request, fields, kept)
}

fn requests_under(nodes: &[TreeNode], out: &mut Vec<String>) {
    for n in nodes {
        match n.kind {
            NodeKind::Request => out.push(n.path.clone()),
            NodeKind::Folder => requests_under(&n.children, out),
        }
    }
}

fn find<'a>(nodes: &'a [TreeNode], path: &str) -> Option<&'a TreeNode> {
    nodes.iter().find_map(|n| if n.path == path { Some(n) } else { find(&n.children, path) })
}

impl Api {
    /// The new version's text, from `text`, a file or a URL.
    async fn update_text(&self, p: &SpecUpdateParams) -> ApiResult<String> {
        match (&p.text, &p.path, &p.url) {
            (Some(t), None, None) => Ok(t.clone()),
            (None, Some(path), None) => {
                let path = path.clone();
                tokio::task::spawn_blocking(move || crate::mock::read_document(&path)).await.map_err(join_err)?
            }
            (None, None, Some(url)) => self.download_document(url).await,
            _ => Err(ApiError::invalid("Give the new document's text, file or URL (one of them)")),
        }
    }

    /// Plan (and with `apply`, do) an update of `p.folder` from a new version of its document.
    pub(crate) async fn spec_update(&self, p: SpecUpdateParams, apply: bool) -> ApiResult<SpecUpdate> {
        let ws = self.ws()?;
        let folder = p.folder.trim_matches('/').to_string();
        let meta = ws.read_folder(&folder)?;
        let Some(source) = meta.openapi.clone() else {
            return Err(ApiError::invalid(format!(
                "\"{}\" wasn't imported from an OpenAPI document; import the document instead",
                meta.name
            )));
        };
        let text = self.update_text(&p).await?;
        let new_text = text.clone();
        let spec_file = crate::specs::spec_path(ws.root(), &source.spec);
        let (new, old) = tokio::task::spawn_blocking(move || {
            let new = import_openapi(&new_text)?;
            let old = spec_file.and_then(|f| std::fs::read_to_string(f).ok()).and_then(|t| import_openapi(&t).ok());
            Ok::<_, ApiError>((new, old))
        })
        .await
        .map_err(join_err)??;

        let mut update = SpecUpdate::default();
        if old.is_none() {
            update.warnings.push(
                "The old version of the document isn't in the workspace, so edits can't be told apart from the \
                 old document: URLs, parameters, headers and bodies take the new version; docs and auth stay."
                    .into(),
            );
        }
        let old_ops: HashMap<String, Request> =
            old.as_ref().map(|o| operations(o).into_iter().map(|(op, _, r)| (op, r)).collect()).unwrap_or_default();

        // The requests the folder has, by operation.
        let tree = ws.tree()?;
        let mut paths = Vec::new();
        if let Some(node) = find(&tree, &folder) {
            requests_under(&node.children, &mut paths);
        }
        let mut existing: HashMap<String, (String, Request)> = HashMap::new();
        for path in paths {
            if let Ok(r) = ws.read_request(&path)
                && let Some(op) = r.openapi.clone()
            {
                existing.entry(op.operation).or_insert((path, r));
            }
        }

        let mut writes = Vec::new();
        let new_ops = operations(&new);
        for (op, folders, request) in &new_ops {
            match existing.remove(op) {
                Some((path, saved)) => {
                    let (mut merged, fields, kept) = merge(&saved, old_ops.get(op), request);
                    let was_removed = saved.openapi.as_ref().is_some_and(|o| o.removed);
                    merged.openapi = Some(OpenApiOperation { operation: op.clone(), removed: false });
                    let change = SpecChange {
                        operation: op.clone(),
                        name: saved.name.clone(),
                        path: Some(path.clone()),
                        fields,
                        kept,
                    };
                    if was_removed {
                        update.restored.push(change);
                    } else if !change.fields.is_empty() || !change.kept.is_empty() {
                        update.changed.push(change);
                    } else {
                        update.unchanged += 1;
                    }
                    if merged != saved {
                        writes.push(Write::Save { path, request: merged });
                    }
                }
                None => {
                    update.added.push(SpecChange {
                        operation: op.clone(),
                        name: request.name.clone(),
                        path: None,
                        fields: Vec::new(),
                        kept: Vec::new(),
                    });
                    writes.push(Write::Create { folders: folders.clone(), request: request.clone() });
                }
            }
        }
        // What is left is gone from the document: kept, and marked.
        let mut gone: Vec<(String, Request)> = existing.into_values().collect();
        gone.sort_by(|a, b| a.0.cmp(&b.0));
        for (path, mut request) in gone {
            let Some(op) = request.openapi.as_mut() else { continue };
            if op.removed {
                continue;
            }
            op.removed = true;
            update.removed.push(SpecChange {
                operation: op.operation.clone(),
                name: request.name.clone(),
                path: Some(path.clone()),
                fields: Vec::new(),
                kept: Vec::new(),
            });
            writes.push(Write::Save { path, request });
        }

        // New variables (path parameters of new operations) go into the folder's environment.
        // Read from the files as they are: secret values stay out of them.
        let envs = ws.list_environments()?;
        let env = envs.iter().find(|e| e.environment.name == meta.name);
        let have: Vec<&str> =
            env.map(|e| e.environment.variables.iter().map(|v| v.key.as_str()).collect()).unwrap_or_default();
        let add: Vec<Variable> = new.variables.iter().filter(|v| !have.contains(&v.key.as_str())).cloned().collect();
        update.variables = add.iter().map(|v| v.key.clone()).collect();

        if !apply {
            return Ok(update);
        }
        for write in writes {
            match write {
                Write::Save { path, request } => ws.save_request(&path, &request)?,
                Write::Create { folders, request } => {
                    let mut parent = folder.clone();
                    for name in folders {
                        let tree = ws.tree()?;
                        let children = find(&tree, &parent).map(|n| n.children.clone()).unwrap_or_default();
                        parent = match children
                            .iter()
                            .find(|n| n.kind == NodeKind::Folder && n.name.eq_ignore_ascii_case(name.trim()))
                        {
                            Some(n) => n.path.clone(),
                            None => ws.create_folder(&parent, &name)?,
                        };
                    }
                    ws.create_request(&parent, request)?;
                }
            }
        }
        if !add.is_empty() {
            match env {
                Some(e) => {
                    let mut environment = e.environment.clone();
                    environment.variables.extend(add);
                    ws.save_environment(&e.id, &environment)?;
                }
                None => {
                    ws.create_environment(&Environment { name: meta.name.clone(), variables: add })?;
                }
            }
        }
        // The new version becomes the one kept (and the one compared with next time).
        if let Some(file) = crate::specs::spec_path(ws.root(), &source.spec) {
            if let Some(dir) = file.parent() {
                std::fs::create_dir_all(dir).map_err(|e| ApiError::new("io", e.to_string()))?;
            }
            zorvik_workspace::fsutil::atomic_write(&file, text.as_bytes())?;
        }
        let new_source = p.url.clone().or_else(|| {
            let root = std::fs::canonicalize(ws.root()).ok()?;
            let file = std::fs::canonicalize(p.path.as_deref()?).ok()?;
            Some(file.strip_prefix(&root).ok()?.to_string_lossy().replace('\\', "/"))
        });
        if let Some(src) = new_source.filter(|s| *s != source.source) {
            let mut meta = ws.read_folder(&folder)?;
            if let Some(o) = meta.openapi.as_mut() {
                o.source = src;
            }
            ws.save_folder(&folder, &meta)?;
        }
        update.applied = true;
        Ok(update)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zorvik_workspace::formats::RequestKind;

    fn req(url: &str, docs: &str) -> Request {
        Request { url: url.into(), docs: docs.into(), ..Request::new("Get pet", RequestKind::Http) }
    }

    #[test]
    fn edits_win_and_untouched_fields_follow_the_document() {
        let old = req("{{baseUrl}}/pets/{{petId}}", "Old docs");
        // The user changed the docs; the document changed the URL and the docs.
        let saved = req("{{baseUrl}}/pets/{{petId}}", "My notes");
        let new = req("{{baseUrl}}/v2/pets/{{petId}}", "New docs");
        let (merged, fields, kept) = merge(&saved, Some(&old), &new);
        assert_eq!(merged.url, "{{baseUrl}}/v2/pets/{{petId}}");
        assert_eq!(merged.docs, "My notes");
        assert_eq!((fields, kept), (vec!["url".to_string()], vec!["docs".to_string()]));
        // The user's URL edit stays when the document didn't change the URL.
        let saved = req("http://localhost:3000/pets/1", "Old docs");
        let (merged, fields, _) = merge(&saved, Some(&old), &req("{{baseUrl}}/pets/{{petId}}", "New docs"));
        assert_eq!((merged.url.as_str(), merged.docs.as_str()), ("http://localhost:3000/pets/1", "New docs"));
        assert_eq!(fields, vec!["docs".to_string()]);
    }

    #[test]
    fn without_the_old_document_the_spec_fields_update() {
        let saved = req("{{baseUrl}}/pets/1", "My notes");
        let (merged, fields, _) = merge(&saved, None, &req("{{baseUrl}}/v2/pets/{{petId}}", "New docs"));
        assert_eq!((merged.url.as_str(), merged.docs.as_str()), ("{{baseUrl}}/v2/pets/{{petId}}", "My notes"));
        assert_eq!(fields, vec!["url".to_string()]);
    }
}

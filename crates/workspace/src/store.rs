//! Workspace on disk:
//!
//! ```text
//! <root>/zorvik.yaml          workspace meta (name, variables, default auth/headers)
//! <root>/environments/*.yaml    environments
//! <root>/requests/**            folders (with optional _folder.yaml) and request files (*.yaml)
//! <root>/servers/*.yaml         servers (mock HTTP, WebSocket, TCP, UDP, DNS, …)
//! <root>/loadtests/*.yaml       load tests
//! ```
//!
//! Item paths in the API are relative to `requests/` and use `/` separators.

use std::path::{Component, Path, PathBuf};

use serde::Serialize;
use ts_rs::TS;
use zorvik_formats::{
    BodyType, Environment, FORMAT_VERSION, FolderMeta, ImportSummary, ImportedCollection, ImportedItem, LoadTest,
    LoadTestNode, NodeKind, Request, RequestKind, Server, ServerNode, TreeNode, WorkspaceMeta,
};

use crate::error::{Error, ErrorCode, Result};
use crate::fsutil::{
    MAX_YAML_FILE, atomic_write, atomic_write_private, copy_dir, is_reserved_name, is_symlink, read_yaml,
    sanitize_file_stem, unique_name, write_yaml,
};

pub const META_FILE: &str = "zorvik.yaml";
pub const REQUESTS_DIR: &str = "requests";
pub const ENV_DIR: &str = "environments";
pub const SERVERS_DIR: &str = "servers";
pub const LOAD_TESTS_DIR: &str = "loadtests";
pub const FOLDER_FILE: &str = "_folder.yaml";
const EXT: &str = ".yaml";
/// Folder levels shown in the tree. Deeper content (e.g. from a hostile repo)
/// is left out: the recursive walk would overflow the stack and crash the app.
const MAX_TREE_DEPTH: usize = 32;

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct EnvironmentEntry {
    /// File stem, used as the environment id.
    pub id: String,
    pub environment: Environment,
}

#[derive(Debug, Clone)]
pub struct Workspace {
    root: PathBuf,
    meta: WorkspaceMeta,
    /// Hash of the canonical root path (see [`Self::local_key`]).
    root_hash: String,
}

impl Workspace {
    /// Create a new workspace in `root` (which may exist, but must not already be a workspace).
    pub fn create(root: &Path, name: &str) -> Result<Self> {
        if root.join(META_FILE).exists() {
            return Err(Error::new(
                ErrorCode::AlreadyExists,
                format!("{} is already a Zorvik workspace", root.display()),
            ));
        }
        let name = name.trim();
        let meta = WorkspaceMeta {
            version: FORMAT_VERSION,
            id: uuid::Uuid::new_v4().to_string(),
            name: if name.is_empty() { "My Workspace".into() } else { name.into() },
            variables: Vec::new(),
            auth: zorvik_formats::Auth::None,
            headers: Vec::new(),
            scripts: Default::default(),
            docs: String::new(),
        };
        for dir in [REQUESTS_DIR, ENV_DIR] {
            std::fs::create_dir_all(root.join(dir))
                .map_err(|e| Error::io(format!("Could not create {}", root.display()), e))?;
        }
        write_yaml(&root.join(META_FILE), &meta)?;
        Ok(Self { root: root.to_path_buf(), meta, root_hash: root_hash(root) })
    }

    pub fn open(root: &Path) -> Result<Self> {
        let meta_path = root.join(META_FILE);
        if !meta_path.exists() {
            return Err(Error::new(
                ErrorCode::NotAWorkspace,
                format!("{} is not a Zorvik workspace (no {META_FILE})", root.display()),
            ));
        }
        let meta = load_meta(&meta_path)?;
        for dir in [REQUESTS_DIR, ENV_DIR] {
            let _ = std::fs::create_dir_all(root.join(dir));
        }
        Ok(Self { root: root.to_path_buf(), meta, root_hash: root_hash(root) })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn meta(&self) -> &WorkspaceMeta {
        &self.meta
    }

    /// Key for what this machine keeps about the workspace outside its folder (secret
    /// values, cookies, OAuth tokens). The id comes from a shared file, so it is bound to
    /// the folder: a workspace that copies another's id must not get that one's secrets.
    pub fn local_key(&self) -> String {
        format!("{}-{}", self.meta.id, self.root_hash)
    }

    pub fn save_meta(&mut self, mut meta: WorkspaceMeta) -> Result<()> {
        meta.version = FORMAT_VERSION;
        meta.id = self.meta.id.clone();
        if meta.name.trim().is_empty() {
            return Err(Error::invalid("Workspace name cannot be empty"));
        }
        write_yaml(&self.root.join(META_FILE), &meta)?;
        self.meta = meta;
        Ok(())
    }

    /// Re-read `zorvik.yaml` (after external edits), with the same checks as [`Self::open`].
    pub fn reload_meta(&mut self) -> Result<()> {
        self.meta = load_meta(&self.root.join(META_FILE))?;
        Ok(())
    }

    fn requests_dir(&self) -> PathBuf {
        self.root.join(REQUESTS_DIR)
    }

    /// Absolute path for an item path, rejecting anything that escapes `requests/`,
    /// including through a symbolic link (workspaces come from Git).
    pub fn resolve_path(&self, rel: &str) -> Result<PathBuf> {
        let rel = rel.trim_matches('/');
        let linked = || Error::invalid(format!("'{rel}' goes through a symbolic link, which is not followed"));
        let mut out = self.requests_dir();
        if is_symlink(&out) {
            return Err(linked());
        }
        if rel.is_empty() {
            return Ok(out);
        }
        for part in rel.split('/') {
            if !valid_path_part(part) {
                return Err(Error::invalid(format!("Invalid item path '{rel}'")));
            }
            out.push(part);
            if is_symlink(&out) {
                return Err(linked());
            }
        }
        Ok(out)
    }

    fn rel_path(&self, abs: &Path) -> String {
        abs.strip_prefix(self.requests_dir())
            .map(|p| p.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/"))
            .unwrap_or_default()
    }

    fn folder_dir(&self, rel: &str) -> Result<PathBuf> {
        let dir = self.resolve_path(rel)?;
        if !dir.is_dir() {
            return Err(Error::not_found(format!("Folder '{rel}' not found")));
        }
        Ok(dir)
    }

    fn request_file(&self, rel: &str) -> Result<PathBuf> {
        let file = self.resolve_path(rel)?;
        if !is_request_file(&file) {
            return Err(Error::invalid(format!("'{rel}' is not a request")));
        }
        if !file.is_file() {
            return Err(Error::not_found(format!("Request '{rel}' not found (it may have been moved or deleted)")));
        }
        Ok(file)
    }

    // ---- tree -------------------------------------------------------------

    pub fn tree(&self) -> Result<Vec<TreeNode>> {
        std::fs::create_dir_all(self.requests_dir()).map_err(|e| Error::io("Could not create requests folder", e))?;
        let dir = self.resolve_path("")?;
        Ok(self.scan(&dir, MAX_TREE_DEPTH))
    }

    /// Items in `dir` in sidebar order, with folder children filled in `levels` deep.
    /// Symbolic links are skipped: they could point outside the workspace or loop.
    fn scan(&self, dir: &Path, levels: usize) -> Vec<TreeNode> {
        let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
        let mut nodes: Vec<TreeNode> = entries
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                let file_name = entry.file_name().to_string_lossy().into_owned();
                if file_name.starts_with('.') || file_name.eq_ignore_ascii_case(FOLDER_FILE) {
                    return None;
                }
                let file_type = entry.file_type().ok()?;
                if file_type.is_dir() {
                    let meta = read_folder_meta(&path);
                    let (name, seq, from_spec, error) = match meta {
                        Ok(m) => (
                            if m.name.trim().is_empty() { file_name.clone() } else { m.name },
                            m.seq,
                            m.openapi.is_some(),
                            None,
                        ),
                        Err(e) => (file_name.clone(), 0, false, Some(e.message)),
                    };
                    Some(TreeNode {
                        kind: NodeKind::Folder,
                        path: self.rel_path(&path),
                        name,
                        seq,
                        method: None,
                        request_kind: None,
                        graphql: false,
                        error,
                        removed_from_spec: false,
                        from_spec,
                        children: if levels > 0 { self.scan(&path, levels - 1) } else { Vec::new() },
                    })
                } else if file_type.is_file() && is_request_file(&path) {
                    let stem = file_name[..file_name.len() - EXT.len()].to_string();
                    let (name, seq, method, kind, graphql, removed, error) = match read_yaml::<Request>(&path) {
                        Ok(r) => (
                            if r.name.trim().is_empty() { stem } else { r.name },
                            r.seq,
                            Some(r.method),
                            Some(r.kind),
                            r.kind == RequestKind::Http && r.body.body_type == BodyType::Graphql,
                            r.openapi.is_some_and(|o| o.removed),
                            None,
                        ),
                        Err(e) => (stem, u32::MAX, None, None, false, false, Some(e.message)),
                    };
                    Some(TreeNode {
                        kind: NodeKind::Request,
                        path: self.rel_path(&path),
                        name,
                        seq,
                        method,
                        request_kind: kind,
                        graphql,
                        error,
                        removed_from_spec: removed,
                        from_spec: false,
                        children: Vec::new(),
                    })
                } else {
                    None
                }
            })
            .collect();
        nodes.sort_by(|a, b| {
            a.seq
                .cmp(&b.seq)
                .then((a.kind != NodeKind::Folder).cmp(&(b.kind != NodeKind::Folder)))
                .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        nodes
    }

    fn next_seq(&self, dir: &Path) -> u32 {
        self.scan(dir, 0).iter().filter(|n| n.seq != u32::MAX).map(|n| n.seq).max().unwrap_or(0) + 1
    }

    // ---- requests ---------------------------------------------------------

    pub fn read_request(&self, rel: &str) -> Result<Request> {
        read_yaml(&self.request_file(rel)?)
    }

    /// Overwrite an existing request file. The sidebar position (`seq`) on
    /// disk wins, so saving a tab never undoes a reorder done meanwhile.
    pub fn save_request(&self, rel: &str, request: &Request) -> Result<()> {
        let file = self.request_file(rel)?;
        validate_name(&request.name)?;
        let mut request = request.clone();
        if let Ok(existing) = read_yaml::<Request>(&file) {
            request.seq = existing.seq;
        }
        write_yaml(&file, &request)
    }

    /// Create a request in folder `parent` ("" = root). Returns its path.
    pub fn create_request(&self, parent: &str, mut request: Request) -> Result<String> {
        let dir = self.folder_dir(parent)?;
        validate_name(&request.name)?;
        request.seq = self.next_seq(&dir);
        Ok(self.rel_path(&self.write_new_request(&dir, request)?))
    }

    /// Write `request` (with its `seq` set) as a new file in `dir`.
    fn write_new_request(&self, dir: &Path, mut request: Request) -> Result<PathBuf> {
        validate_name(&request.name)?;
        request.name = request.name.trim().to_string();
        let file = dir.join(unique_name(dir, &sanitize_file_stem(&request.name), EXT, None));
        write_yaml(&file, &request)?;
        Ok(file)
    }

    // ---- folders ----------------------------------------------------------

    pub fn create_folder(&self, parent: &str, name: &str) -> Result<String> {
        validate_name(name)?;
        let dir = self.folder_dir(parent)?;
        let seq = self.next_seq(&dir);
        Ok(self.rel_path(&self.write_new_folder(&dir, FolderMeta { name: name.into(), seq, ..Default::default() })?))
    }

    /// Create a new folder in `dir` with `meta` (its `seq` set).
    fn write_new_folder(&self, dir: &Path, mut meta: FolderMeta) -> Result<PathBuf> {
        validate_name(&meta.name)?;
        meta.name = meta.name.trim().into();
        let folder = dir.join(unique_name(dir, &sanitize_file_stem(&meta.name), "", None));
        std::fs::create_dir_all(&folder).map_err(|e| Error::io("Could not create folder", e))?;
        write_yaml(&folder.join(FOLDER_FILE), &meta)?;
        Ok(folder)
    }

    pub fn read_folder(&self, rel: &str) -> Result<FolderMeta> {
        let dir = self.folder_dir(rel)?;
        let mut meta = read_folder_meta(&dir)?;
        if meta.name.trim().is_empty() {
            meta.name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        }
        Ok(meta)
    }

    pub fn save_folder(&self, rel: &str, meta: &FolderMeta) -> Result<()> {
        if rel.trim_matches('/').is_empty() {
            return Err(Error::invalid("The root folder has no settings; edit the workspace instead"));
        }
        validate_name(&meta.name)?;
        let dir = self.folder_dir(rel)?;
        let mut meta = meta.clone();
        // Like requests: the sidebar position on disk wins over a stale copy.
        if let Ok(existing) = read_yaml::<FolderMeta>(&dir.join(FOLDER_FILE)) {
            meta.seq = existing.seq;
        }
        write_yaml(&dir.join(FOLDER_FILE), &meta)
    }

    /// Folder metadata from the top-level folder down to the request's parent,
    /// used for auth and header inheritance.
    pub fn ancestors(&self, rel: &str) -> Vec<FolderMeta> {
        // An invalid path inherits nothing rather than reading outside `requests/`.
        if self.resolve_path(rel).is_err() {
            return Vec::new();
        }
        let parts: Vec<&str> = rel.trim_matches('/').split('/').filter(|p| !p.is_empty()).collect();
        let mut out = Vec::new();
        let mut dir = self.requests_dir();
        for part in parts.iter().take(parts.len().saturating_sub(1)) {
            dir.push(part);
            if let Ok(meta) = read_folder_meta(&dir) {
                out.push(meta);
            }
        }
        out
    }

    // ---- generic item operations -----------------------------------------

    fn item_path(&self, rel: &str) -> Result<PathBuf> {
        if rel.trim_matches('/').is_empty() {
            return Err(Error::invalid("This operation needs a request or folder"));
        }
        let path = self.resolve_path(rel)?;
        if !path.exists() {
            return Err(Error::not_found(format!("'{rel}' not found")));
        }
        if !path.is_dir() && !is_request_file(&path) {
            return Err(Error::invalid(format!("'{rel}' is not a request or folder")));
        }
        Ok(path)
    }

    /// Rename a request or folder; returns the new path.
    pub fn rename(&self, rel: &str, new_name: &str) -> Result<String> {
        validate_name(new_name)?;
        let new_name = new_name.trim();
        let path = self.item_path(rel)?;
        let parent = path.parent().expect("item has parent").to_path_buf();
        if path.is_dir() {
            let mut meta = read_folder_meta(&path)?;
            meta.name = new_name.into();
            write_yaml(&path.join(FOLDER_FILE), &meta)?;
            let target = parent.join(unique_name(&parent, &sanitize_file_stem(new_name), "", Some(&path)));
            rename_path(&path, &target)?;
            Ok(self.rel_path(&target))
        } else {
            let mut request: Request = read_yaml(&path)?;
            request.name = new_name.into();
            write_yaml(&path, &request)?;
            let target = parent.join(unique_name(&parent, &sanitize_file_stem(new_name), EXT, Some(&path)));
            rename_path(&path, &target)?;
            Ok(self.rel_path(&target))
        }
    }

    /// Move to the OS trash (recoverable).
    pub fn delete(&self, rel: &str) -> Result<()> {
        let path = self.item_path(rel)?;
        trash::delete(&path).map_err(|e| Error::new(ErrorCode::Io, format!("Could not move '{rel}' to the trash: {e}")))
    }

    /// Duplicate next to the original; returns the new path.
    pub fn duplicate(&self, rel: &str) -> Result<String> {
        let path = self.item_path(rel)?;
        let parent_rel = parent_of(rel);
        if path.is_dir() {
            let meta = read_folder_meta(&path)?;
            let name = format!("{} copy", if meta.name.is_empty() { "Folder" } else { &meta.name });
            let parent = path.parent().expect("parent").to_path_buf();
            let target = parent.join(unique_name(&parent, &sanitize_file_stem(&name), "", None));
            copy_dir(&path, &target)?;
            let seq = self.next_seq(&parent);
            write_yaml(&target.join(FOLDER_FILE), &FolderMeta { name, seq, ..meta })?;
            Ok(self.rel_path(&target))
        } else {
            let mut request: Request = read_yaml(&path)?;
            request.name = format!("{} copy", request.name);
            self.create_request(&parent_rel, request)
        }
    }

    /// Move an item into folder `new_parent` at position `index` among its
    /// siblings (end when `None`). Returns the new path.
    pub fn move_item(&self, rel: &str, new_parent: &str, index: Option<usize>) -> Result<String> {
        let path = self.item_path(rel)?;
        let target_dir = self.folder_dir(new_parent)?;
        if path.is_dir() && (target_dir == path || target_dir.starts_with(&path)) {
            return Err(Error::invalid("A folder cannot be moved into itself"));
        }
        let target = if path.parent() == Some(target_dir.as_path()) {
            path.clone()
        } else {
            let file_name = path.file_name().expect("file name").to_string_lossy().into_owned();
            let (stem, ext) =
                if path.is_dir() { (file_name.as_str(), "") } else { (&file_name[..file_name.len() - EXT.len()], EXT) };
            let target = target_dir.join(unique_name(&target_dir, stem, ext, None));
            rename_path(&path, &target)?;
            target
        };

        // Re-sequence siblings so the moved item lands at `index`.
        let target_rel = self.rel_path(&target);
        let mut siblings: Vec<TreeNode> =
            self.scan(&target_dir, 0).into_iter().filter(|n| n.path != target_rel).collect();
        let moved = TreeNode {
            kind: if target.is_dir() { NodeKind::Folder } else { NodeKind::Request },
            path: target_rel.clone(),
            name: String::new(),
            seq: 0,
            method: None,
            request_kind: None,
            graphql: false,
            error: None,
            removed_from_spec: false,
            from_spec: false,
            children: Vec::new(),
        };
        let at = index.unwrap_or(siblings.len()).min(siblings.len());
        siblings.insert(at, moved);
        for (i, node) in siblings.iter().enumerate() {
            let seq = i as u32 + 1;
            if node.seq == seq && node.path != target_rel {
                continue;
            }
            self.set_seq(&node.path, node.kind, seq)?;
        }
        Ok(target_rel)
    }

    fn set_seq(&self, rel: &str, kind: NodeKind, seq: u32) -> Result<()> {
        let path = self.resolve_path(rel)?;
        match kind {
            NodeKind::Folder => {
                // Leave a broken _folder.yaml (e.g. a Git conflict) alone rather than reset it.
                let Ok(mut meta) = read_folder_meta(&path) else { return Ok(()) };
                if meta.name.is_empty() {
                    meta.name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                }
                meta.seq = seq;
                write_yaml(&path.join(FOLDER_FILE), &meta)
            }
            NodeKind::Request => match read_yaml::<Request>(&path) {
                Ok(mut request) => {
                    request.seq = seq;
                    write_yaml(&path, &request)
                }
                // Leave unparseable files alone.
                Err(_) => Ok(()),
            },
        }
    }

    // ---- environments -----------------------------------------------------

    fn env_dir(&self) -> PathBuf {
        self.root.join(ENV_DIR)
    }

    fn env_file(&self, id: &str) -> Result<PathBuf> {
        if id.starts_with('.') || !valid_path_part(id) {
            return Err(Error::invalid(format!("Invalid environment id '{id}'")));
        }
        let file = self.env_dir().join(format!("{id}{EXT}"));
        if is_symlink(&self.env_dir()) || is_symlink(&file) {
            return Err(Error::invalid(format!("Environment '{id}' is a symbolic link, which is not followed")));
        }
        Ok(file)
    }

    pub fn list_environments(&self) -> Result<Vec<EnvironmentEntry>> {
        let dir = self.env_dir();
        if is_symlink(&dir) {
            return Ok(Vec::new());
        }
        let Ok(entries) = std::fs::read_dir(&dir) else { return Ok(Vec::new()) };
        let mut out: Vec<EnvironmentEntry> = entries
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                let id = name.strip_suffix(EXT)?.to_string();
                if id.starts_with('.') || !e.file_type().is_ok_and(|t| t.is_file()) {
                    return None;
                }
                let mut environment: Environment = read_yaml(&e.path()).ok()?;
                if environment.name.trim().is_empty() {
                    environment.name = id.clone();
                }
                Some(EnvironmentEntry { id, environment })
            })
            .collect();
        out.sort_by_key(|e| e.environment.name.to_lowercase());
        Ok(out)
    }

    pub fn read_environment(&self, id: &str) -> Result<Environment> {
        let file = self.env_file(id)?;
        if !file.exists() {
            return Err(Error::not_found(format!("Environment '{id}' not found")));
        }
        read_yaml(&file)
    }

    /// Save an environment. Renaming it also renames the file; returns the (possibly new) id.
    pub fn save_environment(&self, id: &str, env: &Environment) -> Result<String> {
        validate_name(&env.name)?;
        let file = self.env_file(id)?;
        if !file.exists() {
            return Err(Error::not_found(format!("Environment '{id}' not found")));
        }
        write_yaml(&file, env)?;
        let stem = sanitize_file_stem(&env.name);
        if stem.to_lowercase() == id.to_lowercase() {
            return Ok(id.to_string());
        }
        let dir = self.env_dir();
        let new_name = unique_name(&dir, &stem, EXT, Some(&file));
        rename_path(&file, &dir.join(&new_name))?;
        Ok(new_name[..new_name.len() - EXT.len()].to_string())
    }

    pub fn create_environment(&self, env: &Environment) -> Result<String> {
        validate_name(&env.name)?;
        let dir = self.env_dir();
        if is_symlink(&dir) {
            return Err(Error::invalid(format!("{} is a symbolic link, which is not followed", dir.display())));
        }
        std::fs::create_dir_all(&dir).map_err(|e| Error::io("Could not create environments folder", e))?;
        let file_name = unique_name(&dir, &sanitize_file_stem(&env.name), EXT, None);
        write_yaml(&dir.join(&file_name), env)?;
        Ok(file_name[..file_name.len() - EXT.len()].to_string())
    }

    pub fn delete_environment(&self, id: &str) -> Result<()> {
        let file = self.env_file(id)?;
        if !file.exists() {
            return Err(Error::not_found(format!("Environment '{id}' not found")));
        }
        trash::delete(&file)
            .map_err(|e| Error::new(ErrorCode::Io, format!("Could not move environment to the trash: {e}")))
    }

    // ---- servers and load tests (flat folders of named YAML files) -------------

    fn flat_dir<T: FlatItem>(&self) -> PathBuf {
        self.root.join(T::DIR)
    }

    fn flat_file<T: FlatItem>(&self, id: &str) -> Result<PathBuf> {
        if id.starts_with('.') || !valid_path_part(id) {
            return Err(Error::invalid(format!("Invalid {} id '{id}'", T::WHAT)));
        }
        let dir = self.flat_dir::<T>();
        let file = dir.join(format!("{id}{EXT}"));
        if is_symlink(&dir) || is_symlink(&file) {
            return Err(Error::invalid(format!(
                "{} '{id}' is a symbolic link, which is not followed",
                capitalized(T::WHAT)
            )));
        }
        Ok(file)
    }

    /// Every file of the folder with what it parsed to, in sidebar order
    /// (broken files last).
    fn flat_list<T: FlatItem>(&self) -> Vec<(String, std::result::Result<T, Error>)> {
        let dir = self.flat_dir::<T>();
        if is_symlink(&dir) {
            return Vec::new();
        }
        let Ok(entries) = std::fs::read_dir(&dir) else { return Vec::new() };
        let mut out: Vec<(String, std::result::Result<T, Error>)> = entries
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                let id = name.strip_suffix(EXT)?.to_string();
                if id.starts_with('.') || !e.file_type().is_ok_and(|t| t.is_file()) {
                    return None;
                }
                Some((id, read_yaml::<T>(&e.path())))
            })
            .collect();
        let key = |(id, item): &(String, std::result::Result<T, Error>)| match item {
            Ok(t) => (t.seq(), display_name(t.name(), id).to_lowercase()),
            Err(_) => (u32::MAX, id.to_lowercase()),
        };
        out.sort_by_key(key);
        out
    }

    fn flat_read<T: FlatItem>(&self, id: &str) -> Result<T> {
        let file = self.flat_file::<T>(id)?;
        if !file.exists() {
            return Err(Error::not_found(format!("{} '{id}' not found", capitalized(T::WHAT))));
        }
        read_yaml(&file)
    }

    /// Overwrite an existing file; its name stays. Returns the file.
    fn flat_overwrite<T: FlatItem>(&self, id: &str, item: &T) -> Result<PathBuf> {
        validate_name(item.name())?;
        let file = self.flat_file::<T>(id)?;
        if !file.exists() {
            return Err(Error::not_found(format!("{} '{id}' not found", capitalized(T::WHAT))));
        }
        write_flat(&file, item)?;
        Ok(file)
    }

    /// Save; renaming the item also renames the file. Returns the (possibly new) id.
    fn flat_save<T: FlatItem>(&self, id: &str, item: &T) -> Result<String> {
        let file = self.flat_overwrite(id, item)?;
        let stem = sanitize_file_stem(item.name());
        if stem.to_lowercase() == id.to_lowercase() {
            return Ok(id.to_string());
        }
        let dir = self.flat_dir::<T>();
        let new_name = unique_name(&dir, &stem, EXT, Some(&file));
        rename_path(&file, &dir.join(&new_name))?;
        Ok(new_name[..new_name.len() - EXT.len()].to_string())
    }

    /// Create a file (placed last in the sidebar); returns its id.
    fn flat_create<T: FlatItem>(&self, item: &T) -> Result<String> {
        validate_name(item.name())?;
        let dir = self.flat_dir::<T>();
        if is_symlink(&dir) {
            return Err(Error::invalid(format!("{} is a symbolic link, which is not followed", dir.display())));
        }
        std::fs::create_dir_all(&dir).map_err(|e| Error::io(format!("Could not create the {} folder", T::DIR), e))?;
        let seq =
            self.flat_list::<T>().iter().filter_map(|(_, t)| t.as_ref().ok().map(|t| t.seq() + 1)).max().unwrap_or(0);
        let file_name = unique_name(&dir, &sanitize_file_stem(item.name()), EXT, None);
        let mut item = item.clone();
        item.set_seq(seq);
        write_flat(&dir.join(&file_name), &item)?;
        Ok(file_name[..file_name.len() - EXT.len()].to_string())
    }

    fn flat_delete<T: FlatItem>(&self, id: &str) -> Result<()> {
        let file = self.flat_file::<T>(id)?;
        if !file.exists() {
            return Err(Error::not_found(format!("{} '{id}' not found", capitalized(T::WHAT))));
        }
        trash::delete(&file)
            .map_err(|e| Error::new(ErrorCode::Io, format!("Could not move the {} to the trash: {e}", T::WHAT)))
    }

    /// Put the items in the order of `ids` (sidebar drag and drop).
    fn flat_reorder<T: FlatItem>(&self, ids: &[String]) -> Result<()> {
        for (seq, id) in ids.iter().enumerate() {
            let file = self.flat_file::<T>(id)?;
            // Unreadable files are left alone (like broken requests).
            let Ok(mut item) = read_yaml::<T>(&file) else { continue };
            if item.seq() != seq as u32 {
                item.set_seq(seq as u32);
                write_flat(&file, &item)?;
            }
        }
        Ok(())
    }

    /// Saved servers in sidebar order. Files that don't parse are listed with an error.
    pub fn list_servers(&self) -> Result<Vec<ServerNode>> {
        Ok(self
            .flat_list::<Server>()
            .into_iter()
            .map(|(id, server)| match server {
                Ok(s) => ServerNode {
                    name: display_name(&s.name, &id).to_string(),
                    id,
                    kind: s.kind,
                    host: s.host,
                    port: s.port,
                    tls: s.tls.enabled,
                    seq: s.seq,
                    auto_start: s.auto_start,
                    error: None,
                },
                Err(err) => ServerNode {
                    name: id.clone(),
                    id,
                    kind: Default::default(),
                    host: String::new(),
                    port: 0,
                    tls: false,
                    seq: u32::MAX,
                    auto_start: false,
                    error: Some(err.message),
                },
            })
            .collect())
    }

    pub fn read_server(&self, id: &str) -> Result<Server> {
        self.flat_read(id)
    }

    /// Save a server. Renaming it also renames the file; returns the (possibly new) id.
    pub fn save_server(&self, id: &str, server: &Server) -> Result<String> {
        self.flat_save(id, server)
    }

    /// Create a server file (placed last in the sidebar); returns its id.
    pub fn create_server(&self, server: &Server) -> Result<String> {
        self.flat_create(server)
    }

    pub fn duplicate_server(&self, id: &str) -> Result<String> {
        let server = self.read_server(id)?;
        self.create_server(&Server { name: format!("{} copy", server.name), auto_start: false, ..server })
    }

    pub fn delete_server(&self, id: &str) -> Result<()> {
        self.flat_delete::<Server>(id)
    }

    /// Put the servers in the order of `ids` (sidebar drag and drop).
    pub fn reorder_servers(&self, ids: &[String]) -> Result<()> {
        self.flat_reorder::<Server>(ids)
    }

    /// Saved load tests in sidebar order. Files that don't parse are listed with an error.
    pub fn list_load_tests(&self) -> Result<Vec<LoadTestNode>> {
        Ok(self
            .flat_list::<LoadTest>()
            .into_iter()
            .map(|(id, test)| match test {
                Ok(t) => LoadTestNode {
                    name: display_name(&t.name, &id).to_string(),
                    id,
                    model: t.model,
                    targets: t.targets.iter().filter(|x| x.enabled).count() as u32,
                    duration_secs: t.duration_secs(),
                    seq: t.seq,
                    error: None,
                },
                Err(err) => LoadTestNode {
                    name: id.clone(),
                    id,
                    model: Default::default(),
                    targets: 0,
                    duration_secs: 0,
                    seq: u32::MAX,
                    error: Some(err.message),
                },
            })
            .collect())
    }

    pub fn read_load_test(&self, id: &str) -> Result<LoadTest> {
        self.flat_read(id)
    }

    pub fn save_load_test(&self, id: &str, test: &LoadTest) -> Result<String> {
        self.flat_save(id, test)
    }

    /// Overwrite a load test without renaming its file, even when the file name
    /// doesn't follow the test's name (e.g. to follow a renamed request).
    pub fn update_load_test(&self, id: &str, test: &LoadTest) -> Result<()> {
        self.flat_overwrite(id, test).map(drop)
    }

    pub fn create_load_test(&self, test: &LoadTest) -> Result<String> {
        self.flat_create(test)
    }

    pub fn duplicate_load_test(&self, id: &str) -> Result<String> {
        let test = self.read_load_test(id)?;
        self.create_load_test(&LoadTest { name: format!("{} copy", test.name), ..test })
    }

    pub fn delete_load_test(&self, id: &str) -> Result<()> {
        self.flat_delete::<LoadTest>(id)
    }

    pub fn reorder_load_tests(&self, ids: &[String]) -> Result<()> {
        self.flat_reorder::<LoadTest>(ids)
    }

    // ---- import -----------------------------------------------------------

    /// Write an imported collection into a new folder under `parent`.
    /// Collection auth/headers/scripts go on that folder; variables become an environment.
    pub fn write_imported(&self, parent: &str, collection: &ImportedCollection) -> Result<ImportSummary> {
        let name = if collection.name.trim().is_empty() { "Imported" } else { collection.name.trim() };
        let parent_dir = self.folder_dir(parent)?;
        let auth = match &collection.auth {
            zorvik_formats::Auth::None => zorvik_formats::Auth::Inherit,
            other => other.clone(),
        };
        let meta = FolderMeta {
            name: name.into(),
            seq: self.next_seq(&parent_dir),
            auth,
            headers: collection.headers.clone(),
            scripts: collection.scripts.clone(),
            ..Default::default()
        };
        let folder = self.write_new_folder(&parent_dir, meta)?;
        let folder_rel = self.rel_path(&folder);

        let mut folders = 1u32;
        let mut requests = 0u32;
        self.write_items(&folder, &collection.items, &mut folders, &mut requests)?;

        let mut environments = 0;
        if !collection.variables.is_empty() {
            self.create_environment(&Environment { name: name.to_string(), variables: collection.variables.clone() })?;
            environments = 1;
        }
        Ok(ImportSummary {
            name: name.to_string(),
            requests,
            folders,
            environments,
            workspace_variables: 0,
            warnings: collection.warnings.clone(),
            folder_path: Some(folder_rel),
        })
    }

    /// Write imported items into the new, empty folder `dir`. Positions follow
    /// the import order, so no re-scan of the folder is needed per item.
    fn write_items(&self, dir: &Path, items: &[ImportedItem], folders: &mut u32, requests: &mut u32) -> Result<()> {
        for (i, item) in items.iter().enumerate() {
            let seq = i as u32 + 1;
            match item {
                ImportedItem::Folder { meta, children } => {
                    let name = if meta.name.trim().is_empty() { "Untitled folder" } else { meta.name.trim() };
                    let folder = self.write_new_folder(dir, FolderMeta { name: name.into(), seq, ..meta.clone() })?;
                    *folders += 1;
                    self.write_items(&folder, children, folders, requests)?;
                }
                ImportedItem::Request(request) => {
                    let mut request = request.clone();
                    if request.name.trim().is_empty() {
                        request.name = "Untitled request".into();
                    }
                    request.seq = seq;
                    self.write_new_request(dir, request)?;
                    *requests += 1;
                }
            }
        }
        Ok(())
    }
}

fn read_folder_meta(dir: &Path) -> Result<FolderMeta> {
    let file = dir.join(FOLDER_FILE);
    if is_symlink(&file) {
        return Err(Error::invalid(format!("{} is a symbolic link, which is not followed", file.display())));
    }
    if !file.exists() {
        return Ok(FolderMeta::default());
    }
    read_yaml(&file)
}

/// Read and check `zorvik.yaml`, giving it an id if it has none.
fn load_meta(path: &Path) -> Result<WorkspaceMeta> {
    if is_symlink(path) {
        return Err(Error::invalid(format!("{} is a symbolic link, which is not followed", path.display())));
    }
    let mut meta: WorkspaceMeta = read_yaml(path)?;
    if meta.version > FORMAT_VERSION {
        return Err(Error::invalid(format!(
            "This workspace was created by a newer Zorvik (format v{}); please update the app",
            meta.version
        )));
    }
    if meta.id.trim().is_empty() {
        meta.id = uuid::Uuid::new_v4().to_string();
        write_yaml(path, &meta)?;
    } else if !valid_id(&meta.id) {
        return Err(Error::invalid(format!(
            "{META_FILE} has an invalid id (use up to 64 letters, digits, '-' or '_')"
        )));
    }
    Ok(meta)
}

/// The workspace id names local files (cookie jar) and keys secrets, but it
/// comes from a shared file: allow only plain names.
fn valid_id(id: &str) -> bool {
    id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') && !is_reserved_name(id)
}

/// One `/`-separated part of an item path: a plain name that cannot climb out
/// of its folder or, on Windows, alias another file or a device.
fn valid_path_part(part: &str) -> bool {
    let plain = matches!(Path::new(part).components().collect::<Vec<_>>().as_slice(), [Component::Normal(_)]);
    let windows_alias = cfg!(windows) && (part.contains(':') || part.ends_with(['.', ' ']) || is_reserved_name(part));
    plain && !part.contains('\\') && !part.trim_matches(['.', ' ']).is_empty() && !windows_alias
}

/// A request file name: `*.yaml` (any case) other than the folder settings file.
fn is_request_file(path: &Path) -> bool {
    let name = path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
    name.len() > EXT.len() && name.to_ascii_lowercase().ends_with(EXT) && !name.eq_ignore_ascii_case(FOLDER_FILE)
}

fn rename_path(from: &Path, to: &Path) -> Result<()> {
    if from == to {
        return Ok(());
    }
    // Case-only renames need a hop on case-insensitive file systems.
    if from.to_string_lossy().to_lowercase() == to.to_string_lossy().to_lowercase() {
        let hop = from.with_file_name(format!(".rename-{}", uuid::Uuid::new_v4()));
        std::fs::rename(from, &hop).map_err(|e| Error::io("Could not rename", e))?;
        return std::fs::rename(&hop, to).map_err(|e| {
            // Don't leave the item hidden under the temporary name.
            let _ = std::fs::rename(&hop, from);
            Error::io("Could not rename", e)
        });
    }
    std::fs::rename(from, to).map_err(|e| Error::io(format!("Could not move to {}", to.display()), e))
}

fn validate_name(name: &str) -> Result<()> {
    if name.trim().is_empty() {
        return Err(Error::invalid("Name cannot be empty"));
    }
    if name.chars().count() > 200 {
        return Err(Error::invalid("Name is too long (max 200 characters)"));
    }
    Ok(())
}

pub fn parent_of(rel: &str) -> String {
    let rel = rel.trim_matches('/');
    rel.rsplit_once('/').map(|(p, _)| p.to_string()).unwrap_or_default()
}

/// New empty request of a given kind with sensible defaults.
pub fn new_request(name: &str, kind: RequestKind) -> Request {
    let mut r = Request::new(name, kind);
    if kind == RequestKind::Websocket {
        r.url = "ws://".into();
    }
    r
}

/// Files of a flat folder: servers (`servers/`) and load tests (`loadtests/`).
trait FlatItem: Serialize + serde::de::DeserializeOwned + Clone {
    const DIR: &'static str;
    /// Lower-case noun for messages.
    const WHAT: &'static str;
    fn name(&self) -> &str;
    fn seq(&self) -> u32;
    fn set_seq(&mut self, seq: u32);
}

impl FlatItem for Server {
    const DIR: &'static str = SERVERS_DIR;
    const WHAT: &'static str = "server";
    fn name(&self) -> &str {
        &self.name
    }
    fn seq(&self) -> u32 {
        self.seq
    }
    fn set_seq(&mut self, seq: u32) {
        self.seq = seq;
    }
}

impl FlatItem for LoadTest {
    const DIR: &'static str = LOAD_TESTS_DIR;
    const WHAT: &'static str = "load test";
    fn name(&self) -> &str {
        &self.name
    }
    fn seq(&self) -> u32 {
        self.seq
    }
    fn set_seq(&mut self, seq: u32) {
        self.seq = seq;
    }
}

fn capitalized(s: &str) -> String {
    let mut chars = s.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

/// The name shown for an item (its file stem when the name is empty).
fn display_name<'a>(name: &'a str, id: &'a str) -> &'a str {
    if name.trim().is_empty() { id } else { name }
}

/// Write a flat item, refusing what could not be read back (see `MAX_YAML_FILE`).
fn write_flat<T: FlatItem>(file: &Path, item: &T) -> Result<()> {
    let text = serde_yaml_ng::to_string(item).map_err(|e| Error::invalid(format!("Could not serialize: {e}")))?;
    if text.len() as u64 > MAX_YAML_FILE {
        return Err(Error::invalid(format!(
            "The {} is larger than {} MB: remove routes or shorten their bodies",
            T::WHAT,
            MAX_YAML_FILE >> 20
        )));
    }
    atomic_write(file, text.as_bytes())
}

/// Identifies a server's configuration, so the app can tell whether this
/// computer already ran (or saved) exactly this one. The sidebar position and
/// the "start with workspace" switch don't count.
pub fn server_fingerprint(server: &Server) -> String {
    use sha2::Digest as _;
    let normalized = Server { seq: 0, auto_start: false, ..server.clone() };
    let json = serde_json::to_vec(&normalized).unwrap_or_default();
    sha2::Sha256::digest(&json).iter().map(|b| format!("{b:02x}")).collect()
}

/// Save helper for the local data dir (not part of a workspace).
pub fn save_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let data = serde_json::to_vec_pretty(value).map_err(|e| Error::invalid(e.to_string()))?;
    atomic_write(path, &data)
}

/// [`save_json`] for secrets and tokens: the file is owner-only.
pub(crate) fn save_json_private<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let data = serde_json::to_vec_pretty(value).map_err(|e| Error::invalid(e.to_string()))?;
    atomic_write_private(path, &data)
}

fn root_hash(root: &Path) -> String {
    use sha2::Digest as _;
    let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let digest = sha2::Sha256::digest(root.to_string_lossy().as_bytes());
    digest[..6].iter().map(|b| format!("{b:02x}")).collect()
}

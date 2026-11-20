//! Scenarios on disk: `scenarios/**/<name>.yaml`, in folders like the collection's (a folder's
//! `_folder.yaml` has its name and place). A scenario's id is its path under `scenarios/`
//! without `.yaml` (`Checkout`, `Shop/Checkout`); a folder's id is its path (`Shop`). Ids
//! use `/` on every system.

use std::path::{Path, PathBuf};

use zorvik_formats::{FolderMeta, Scenario, ScenarioNode};

use crate::error::{Error, ErrorCode, Result};
use crate::fsutil::{
    MAX_YAML_FILE, atomic_write, claim_unique_name, is_symlink, read_yaml, sanitize_file_stem, unique_name, write_yaml,
};
use crate::store::{
    EXT, FOLDER_FILE, MAX_TREE_DEPTH, SCENARIOS_DIR, Workspace, parent_of, read_folder_meta, rename_path,
    valid_path_part, validate_name,
};

/// What is moved in the scenarios' tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScenarioItem {
    Scenario,
    Folder,
}

impl Workspace {
    fn scenarios_dir(&self) -> PathBuf {
        self.root().join(SCENARIOS_DIR)
    }

    /// `rel` under `scenarios/`, refusing anything that climbs out, is hidden or goes through a
    /// symbolic link (workspaces come from Git).
    fn scenario_path(&self, rel: &str) -> Result<PathBuf> {
        let rel = rel.trim_matches('/');
        let mut out = self.scenarios_dir();
        let linked = || Error::invalid(format!("Scenario '{rel}' goes through a symbolic link, which is not followed"));
        if is_symlink(&out) {
            return Err(linked());
        }
        if rel.is_empty() {
            return Ok(out);
        }
        for part in rel.split('/') {
            if part.starts_with('.') || !valid_path_part(part) {
                return Err(Error::invalid(format!("Invalid scenario id '{rel}'")));
            }
            out.push(part);
            if is_symlink(&out) {
                return Err(linked());
            }
        }
        Ok(out)
    }

    fn scenario_file(&self, id: &str) -> Result<PathBuf> {
        let id = id.trim_matches('/');
        if id.is_empty() {
            return Err(Error::invalid("Give the scenario's id"));
        }
        let path = self.scenario_path(id)?;
        let file = path.with_file_name(format!("{}{EXT}", path.file_name().unwrap_or_default().to_string_lossy()));
        if is_symlink(&file) {
            return Err(Error::invalid(format!("Scenario '{id}' is a symbolic link, which is not followed")));
        }
        Ok(file)
    }

    /// A scenario's file that is there.
    fn existing_scenario(&self, id: &str) -> Result<PathBuf> {
        let file = self.scenario_file(id)?;
        if !file.is_file() {
            return Err(Error::not_found(format!("Scenario '{id}' not found")));
        }
        Ok(file)
    }

    /// A folder of scenarios that is there ("" is `scenarios/` itself, made when missing).
    fn scenario_folder(&self, rel: &str) -> Result<PathBuf> {
        let dir = self.scenario_path(rel)?;
        if rel.trim_matches('/').is_empty() {
            std::fs::create_dir_all(&dir).map_err(|e| Error::io("Could not create the scenarios folder", e))?;
        } else if !dir.is_dir() {
            return Err(Error::not_found(format!("Scenario folder '{rel}' not found")));
        }
        Ok(dir)
    }

    /// The id of the scenario file or `folder` at `path` under `scenarios/`.
    fn scenario_id(&self, path: &Path, folder: bool) -> String {
        let rel = path.strip_prefix(self.scenarios_dir()).unwrap_or(path);
        let id = rel.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/");
        match folder {
            true => id,
            false => id.strip_suffix(EXT).map(str::to_string).unwrap_or(id),
        }
    }

    // ---- listing ------------------------------------------------------------------------------

    /// Scenarios and their folders in sidebar order. Files that don't parse are listed with an
    /// error.
    pub fn scenario_tree(&self) -> Result<Vec<ScenarioNode>> {
        let dir = self.scenario_path("")?;
        Ok(self.scan_scenarios(&dir, MAX_TREE_DEPTH))
    }

    /// Every scenario (not the folders), in sidebar order.
    pub fn list_scenarios(&self) -> Result<Vec<ScenarioNode>> {
        fn flatten(nodes: Vec<ScenarioNode>, out: &mut Vec<ScenarioNode>) {
            for node in nodes {
                match node.folder {
                    true => flatten(node.children, out),
                    false => out.push(node),
                }
            }
        }
        let mut out = Vec::new();
        flatten(self.scenario_tree()?, &mut out);
        Ok(out)
    }

    /// The folders, every level, as their ids.
    pub fn scenario_folders(&self) -> Result<Vec<String>> {
        fn walk(nodes: &[ScenarioNode], out: &mut Vec<String>) {
            for node in nodes.iter().filter(|n| n.folder) {
                out.push(node.id.clone());
                walk(&node.children, out);
            }
        }
        let mut out = Vec::new();
        walk(&self.scenario_tree()?, &mut out);
        Ok(out)
    }

    /// What is in `dir`, in sidebar order, with folders filled in `levels` deep. Symbolic links
    /// and hidden files are left out.
    fn scan_scenarios(&self, dir: &Path, levels: usize) -> Vec<ScenarioNode> {
        let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
        let mut nodes: Vec<ScenarioNode> = entries
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                let kind = entry.file_type().ok()?;
                if name.starts_with('.') || name.eq_ignore_ascii_case(FOLDER_FILE) {
                    return None;
                }
                let path = entry.path();
                let id = self.scenario_id(&path, kind.is_dir());
                if kind.is_dir() {
                    let (shown, seq, error) = match read_folder_meta(&path) {
                        Ok(meta) => (if meta.name.trim().is_empty() { name } else { meta.name }, meta.seq, None),
                        Err(e) => (name, 0, Some(e.message)),
                    };
                    let children = if levels > 0 { self.scan_scenarios(&path, levels - 1) } else { Vec::new() };
                    return Some(ScenarioNode { id, name: shown, folder: true, steps: 0, seq, error, children });
                }
                let stem = name.strip_suffix(EXT)?;
                if !kind.is_file() {
                    return None;
                }
                Some(match read_yaml::<Scenario>(&path).and_then(checked) {
                    Ok(s) => ScenarioNode {
                        name: if s.name.trim().is_empty() { stem.to_string() } else { s.name.clone() },
                        id,
                        folder: false,
                        steps: s.step_count(),
                        seq: s.seq,
                        error: None,
                        children: Vec::new(),
                    },
                    Err(e) => ScenarioNode {
                        name: stem.to_string(),
                        id,
                        folder: false,
                        steps: 0,
                        seq: u32::MAX,
                        error: Some(e.message),
                        children: Vec::new(),
                    },
                })
            })
            .collect();
        nodes.sort_by(|a, b| {
            a.seq.cmp(&b.seq).then(b.folder.cmp(&a.folder)).then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        nodes
    }

    /// The place after everything in `dir`.
    fn next_scenario_seq(&self, dir: &Path) -> u32 {
        self.scan_scenarios(dir, 0).iter().filter(|n| n.seq != u32::MAX).map(|n| n.seq + 1).max().unwrap_or(0)
    }

    // ---- scenarios ----------------------------------------------------------------------------

    pub fn read_scenario(&self, id: &str) -> Result<Scenario> {
        read_yaml(&self.existing_scenario(id)?).and_then(checked)
    }

    /// Save a scenario. Renaming it also renames the file (in its folder); returns the
    /// (possibly new) id. Its place on disk wins, so saving never undoes a move made meanwhile.
    pub fn save_scenario(&self, id: &str, scenario: &Scenario) -> Result<String> {
        checked(scenario.clone())?;
        let file = self.existing_scenario(id)?;
        write_in_place(&file, scenario)?;
        let stem = sanitize_file_stem(&scenario.name);
        let current = file.file_name().unwrap_or_default().to_string_lossy().into_owned();
        if format!("{stem}{EXT}").to_lowercase() == current.to_lowercase() {
            return Ok(id.trim_matches('/').to_string());
        }
        let dir = file.parent().expect("a scenario's folder").to_path_buf();
        let target = dir.join(unique_name(&dir, &stem, EXT, Some(&file)));
        rename_path(&file, &target)?;
        Ok(self.scenario_id(&target, false))
    }

    /// Overwrite a scenario without renaming its file (e.g. to follow a renamed request).
    pub fn update_scenario(&self, id: &str, scenario: &Scenario) -> Result<()> {
        write_in_place(&self.existing_scenario(id)?, scenario)
    }

    /// Create a scenario in `folder` ("" = the top level), placed last; returns its id.
    pub fn create_scenario(&self, folder: &str, scenario: &Scenario) -> Result<String> {
        checked(scenario.clone())?;
        let dir = self.scenario_folder(folder)?;
        let mut scenario = scenario.clone();
        scenario.seq = self.next_scenario_seq(&dir);
        let file = dir.join(claim_unique_name(&dir, &sanitize_file_stem(&scenario.name), EXT)?);
        write_scenario(&file, &scenario).inspect_err(|_| drop(std::fs::remove_file(&file)))?;
        Ok(self.scenario_id(&file, false))
    }

    /// A copy next to the original, named "… copy"; returns its id.
    pub fn duplicate_scenario(&self, id: &str) -> Result<String> {
        let scenario = self.read_scenario(id)?;
        let copy = Scenario { name: format!("{} copy", scenario.name), ..scenario };
        self.create_scenario(&parent_of(id), &copy)
    }

    pub fn delete_scenario(&self, id: &str) -> Result<()> {
        let file = self.existing_scenario(id)?;
        trash::delete(&file)
            .map_err(|e| Error::new(ErrorCode::Io, format!("Could not move the scenario to the trash: {e}")))
    }

    // ---- folders ------------------------------------------------------------------------------

    /// Create a folder in `parent` ("" = the top level), placed last; returns its id.
    pub fn create_scenario_folder(&self, parent: &str, name: &str) -> Result<String> {
        validate_name(name)?;
        let dir = self.scenario_folder(parent)?;
        let meta = FolderMeta { name: name.trim().into(), seq: self.next_scenario_seq(&dir), ..Default::default() };
        let folder = dir.join(claim_unique_name(&dir, &sanitize_file_stem(name), "")?);
        write_yaml(&folder.join(FOLDER_FILE), &meta).inspect_err(|_| drop(std::fs::remove_dir_all(&folder)))?;
        Ok(self.scenario_id(&folder, true))
    }

    /// The folder `path` (`Shop/Checkout flows`: each part a folder's id part or name), with the
    /// folders on the way made when missing (named as written); returns its id.
    pub fn ensure_scenario_folder(&self, path: &str) -> Result<String> {
        let mut at = String::new();
        for part in path.split('/').map(str::trim).filter(|p| !p.is_empty()) {
            let dir = self.scenario_folder(&at)?;
            let found = self.scan_scenarios(&dir, 0).into_iter().find(|n| {
                n.folder
                    && (n.id.rsplit('/').next().is_some_and(|last| last.eq_ignore_ascii_case(part))
                        || n.name.eq_ignore_ascii_case(part))
            });
            at = match found {
                Some(folder) => folder.id,
                None => self.create_scenario_folder(&at, part)?,
            };
        }
        Ok(at)
    }

    /// Rename a folder; returns its new id.
    pub fn rename_scenario_folder(&self, id: &str, name: &str) -> Result<String> {
        validate_name(name)?;
        let dir = self.existing_scenario_folder(id)?;
        let mut meta = read_folder_meta(&dir)?;
        meta.name = name.trim().into();
        write_yaml(&dir.join(FOLDER_FILE), &meta)?;
        let parent = dir.parent().expect("a folder's parent").to_path_buf();
        let target = parent.join(unique_name(&parent, &sanitize_file_stem(name), "", Some(&dir)));
        rename_path(&dir, &target)?;
        Ok(self.scenario_id(&target, true))
    }

    /// Move a folder and everything in it to the OS trash.
    pub fn delete_scenario_folder(&self, id: &str) -> Result<()> {
        let dir = self.existing_scenario_folder(id)?;
        trash::delete(&dir)
            .map_err(|e| Error::new(ErrorCode::Io, format!("Could not move the folder to the trash: {e}")))
    }

    fn existing_scenario_folder(&self, id: &str) -> Result<PathBuf> {
        if id.trim_matches('/').is_empty() {
            return Err(Error::invalid("This needs a folder of scenarios"));
        }
        self.scenario_folder(id)
    }

    /// Move a scenario or a folder into the folder `parent` ("" = the top level) at place `index`
    /// among what is there (last when `None`). Returns its new id.
    pub fn move_scenario(&self, id: &str, item: ScenarioItem, parent: &str, index: Option<usize>) -> Result<String> {
        let path = match item {
            ScenarioItem::Scenario => self.existing_scenario(id)?,
            ScenarioItem::Folder => self.existing_scenario_folder(id)?,
        };
        let target_dir = self.scenario_folder(parent)?;
        if item == ScenarioItem::Folder && target_dir.starts_with(&path) {
            return Err(Error::invalid("A folder cannot be moved into itself"));
        }
        let target = if path.parent() == Some(target_dir.as_path()) {
            path.clone()
        } else {
            let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
            let (stem, ext) = match item {
                ScenarioItem::Scenario => (&name[..name.len() - EXT.len()], EXT),
                ScenarioItem::Folder => (name.as_str(), ""),
            };
            let target = target_dir.join(unique_name(&target_dir, stem, ext, None));
            rename_path(&path, &target)?;
            target
        };
        // Number what is there again, with the moved one at `index`.
        let moved = self.scenario_id(&target, item == ScenarioItem::Folder);
        let mut siblings: Vec<ScenarioNode> = self
            .scan_scenarios(&target_dir, 0)
            .into_iter()
            .filter(|n| !(n.id == moved && n.folder == (item == ScenarioItem::Folder)))
            .collect();
        let at = index.unwrap_or(siblings.len()).min(siblings.len());
        siblings.insert(
            at,
            ScenarioNode {
                id: moved.clone(),
                name: String::new(),
                folder: item == ScenarioItem::Folder,
                steps: 0,
                seq: u32::MAX,
                error: None,
                children: Vec::new(),
            },
        );
        for (seq, node) in siblings.iter().enumerate() {
            let seq = seq as u32;
            if node.seq != seq {
                self.set_scenario_seq(node, seq)?;
            }
        }
        Ok(moved)
    }

    fn set_scenario_seq(&self, node: &ScenarioNode, seq: u32) -> Result<()> {
        if node.folder {
            let dir = self.scenario_path(&node.id)?;
            // A broken `_folder.yaml` (a Git conflict) is left alone rather than reset.
            let Ok(mut meta) = read_folder_meta(&dir) else { return Ok(()) };
            if meta.name.trim().is_empty() {
                meta.name = dir.file_name().unwrap_or_default().to_string_lossy().into_owned();
            }
            meta.seq = seq;
            return write_yaml(&dir.join(FOLDER_FILE), &meta);
        }
        let file = self.scenario_file(&node.id)?;
        match read_yaml::<Scenario>(&file) {
            Ok(mut scenario) => {
                scenario.seq = seq;
                write_scenario(&file, &scenario)
            }
            // Files that don't parse are left alone.
            Err(_) => Ok(()),
        }
    }
}

/// A scenario that can be used (see [`Scenario::problem`]).
fn checked(scenario: Scenario) -> Result<Scenario> {
    match scenario.problem() {
        Some(problem) => Err(Error::invalid(problem)),
        None => Ok(scenario),
    }
}

/// Write over a scenario's file, keeping the place it has on disk.
fn write_in_place(file: &Path, scenario: &Scenario) -> Result<()> {
    let mut scenario = scenario.clone();
    if let Ok(existing) = read_yaml::<Scenario>(file) {
        scenario.seq = existing.seq;
    }
    write_scenario(file, &scenario)
}

/// Write a scenario, refusing what could not be read back (see `MAX_YAML_FILE`).
fn write_scenario(file: &Path, scenario: &Scenario) -> Result<()> {
    validate_name(&scenario.name)?;
    let text = serde_yaml_ng::to_string(scenario).map_err(|e| Error::invalid(format!("Could not serialize: {e}")))?;
    if text.len() as u64 > MAX_YAML_FILE {
        return Err(Error::invalid(format!("The scenario is larger than {} MB", MAX_YAML_FILE >> 20)));
    }
    atomic_write(file, text.as_bytes())
}

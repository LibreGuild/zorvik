//! Current values set by scripts (`pm.environment.set`, `pm.collectionVariables.set`,
//! `pm.globals.set`). They live in the local app data dir, never in workspace files
//! (shared via Git, they would leak tokens), and win over the file values.
//! Scopes are named like the secret store's (`secrets::env_scope`, …) plus [`GLOBALS`].

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

use zorvik_formats::Variable;

type Scopes = BTreeMap<String, BTreeMap<String, String>>;

/// Scope of global variables (app-wide, not per workspace).
pub const GLOBALS: &str = "globals";

/// Most text (keys and values) kept for all workspaces together. Scripts come
/// from workspace files; without a bound a loop of `set` calls could grow the
/// file (rewritten on every send) without end.
pub const MAX_BYTES: usize = 32 * 1024 * 1024;

pub struct LocalValues {
    path: Option<PathBuf>,
    scopes: Mutex<Scopes>,
}

impl LocalValues {
    pub fn in_memory() -> Self {
        Self { path: None, scopes: Mutex::new(Scopes::new()) }
    }

    /// An unreadable file starts empty (these are only current values, the file ones stay).
    pub fn open(path: PathBuf) -> Self {
        let scopes = std::fs::read(&path)
            .ok()
            .and_then(|d| serde_json::from_slice(&d).map_err(|e| tracing::warn!("local values unreadable: {e}")).ok())
            .unwrap_or_default();
        Self { path: Some(path), scopes: Mutex::new(scopes) }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Scopes> {
        self.scopes.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn persist(&self, scopes: &Scopes) -> crate::Result<()> {
        if let Some(path) = &self.path {
            crate::store::save_json_private(path, scopes)?;
        }
        Ok(())
    }

    pub fn get(&self, scope: &str) -> BTreeMap<String, String> {
        self.lock().get(scope).cloned().unwrap_or_default()
    }

    /// Scopes starting with `prefix`, with their values.
    pub fn list(&self, prefix: &str) -> Vec<(String, BTreeMap<String, String>)> {
        self.lock().iter().filter(|(k, _)| k.starts_with(prefix)).map(|(k, v)| (k.clone(), v.clone())).collect()
    }

    /// Set (`Some`) or remove (`None`) values, in order. Refused (nothing
    /// changes) when the values would grow past [`MAX_BYTES`].
    pub fn update(&self, scope: &str, changes: &[(String, Option<String>)]) -> crate::Result<()> {
        if changes.is_empty() {
            return Ok(());
        }
        let mut scopes = self.lock();
        let before = size(&scopes);
        let previous = scopes.get(scope).cloned();
        let values = scopes.entry(scope.to_string()).or_default();
        for (key, value) in changes {
            match value {
                Some(v) => values.insert(key.clone(), v.clone()),
                None => values.remove(key),
            };
        }
        if values.is_empty() {
            scopes.remove(scope);
        }
        let after = size(&scopes);
        if after > MAX_BYTES && after > before {
            match previous {
                Some(values) => scopes.insert(scope.to_string(), values),
                None => scopes.remove(scope),
            };
            return Err(crate::Error::invalid(format!(
                "Values set by scripts would take more than {} MB; clear some under Set by scripts",
                MAX_BYTES / (1024 * 1024)
            )));
        }
        self.persist(&scopes)
    }

    /// Remove one value, or (`key` = `None`) the whole scope.
    pub fn clear(&self, scope: &str, key: Option<&str>) -> crate::Result<()> {
        let mut scopes = self.lock();
        let changed = match key {
            Some(key) => {
                let removed = scopes.get_mut(scope).is_some_and(|v| v.remove(key).is_some());
                if scopes.get(scope).is_some_and(BTreeMap::is_empty) {
                    scopes.remove(scope);
                }
                removed
            }
            None => scopes.remove(scope).is_some(),
        };
        if changed {
            self.persist(&scopes)?;
        }
        Ok(())
    }

    pub fn rename_scope(&self, from: &str, to: &str) -> crate::Result<()> {
        let mut scopes = self.lock();
        if let Some(values) = scopes.remove(from) {
            scopes.insert(to.to_string(), values);
            self.persist(&scopes)?;
        }
        Ok(())
    }
}

/// Bytes of all keys and values.
fn size(scopes: &Scopes) -> usize {
    scopes.values().flat_map(|values| values.iter()).map(|(k, v)| k.len() + v.len()).sum()
}

/// `file` variables with local values applied: a local value replaces the file value
/// (and enables the variable); local values without a file variable are added.
pub fn overlay(file: &[Variable], local: &BTreeMap<String, String>) -> Vec<Variable> {
    let mut out: Vec<Variable> = file
        .iter()
        .map(|v| match local.get(v.key.trim()) {
            Some(value) => Variable { value: value.clone(), enabled: true, ..v.clone() },
            None => v.clone(),
        })
        .collect();
    for (key, value) in local {
        if !file.iter().any(|v| v.key.trim() == key) {
            out.push(Variable { key: key.clone(), value: value.clone(), enabled: true, secret: false });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(k: &str, val: &str) -> Variable {
        Variable { key: k.into(), value: val.into(), enabled: true, secret: false }
    }

    #[test]
    fn update_clear_persist() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("local-values.json");
        let store = LocalValues::open(path.clone());
        store
            .update(
                "w/env/dev",
                &[("token".into(), Some("t1".into())), ("a".into(), Some("1".into())), ("a".into(), None)],
            )
            .unwrap();
        store.update(GLOBALS, &[("g".into(), Some("x".into()))]).unwrap();
        let reopened = LocalValues::open(path.clone());
        assert_eq!(reopened.get("w/env/dev"), BTreeMap::from([("token".to_string(), "t1".to_string())]));
        assert_eq!(reopened.list("w/").len(), 1);
        reopened.rename_scope("w/env/dev", "w/env/prod").unwrap();
        assert!(reopened.get("w/env/dev").is_empty());
        reopened.clear("w/env/prod", Some("token")).unwrap();
        assert!(reopened.list("w/").is_empty(), "an emptied scope is removed");
        reopened.clear(GLOBALS, None).unwrap();
        assert!(LocalValues::open(path.clone()).get(GLOBALS).is_empty());
        std::fs::write(&path, "garbage").unwrap();
        assert!(LocalValues::open(path).get(GLOBALS).is_empty());
    }

    #[test]
    fn size_is_bounded() {
        let store = LocalValues::in_memory();
        let big = "x".repeat(MAX_BYTES / 2);
        store.update("w/workspace", &[("a".into(), Some(big.clone()))]).unwrap();
        let err = store.update("w/env/dev", &[("b".into(), Some(big.clone())), ("c".into(), Some("1".into()))]);
        assert!(err.unwrap_err().message.contains("more than 32 MB"));
        assert!(store.get("w/env/dev").is_empty(), "a refused update changes nothing");
        assert!(store.update("w/workspace", &[("a".into(), Some(big))]).is_ok(), "replacing a value doesn't grow");
        store.update("w/workspace", &[("a".into(), None)]).unwrap();
        store.update("w/env/dev", &[("b".into(), Some("x".repeat(MAX_BYTES / 2)))]).unwrap();
    }

    #[test]
    fn overlay_replaces_and_adds() {
        let file = [
            v("host", "file.test"),
            Variable { enabled: false, ..v("off", "x") },
            Variable { secret: true, ..v("key", "") },
        ];
        let local = BTreeMap::from([
            ("off".to_string(), "on".to_string()),
            ("key".to_string(), "s3cret".to_string()),
            ("new".to_string(), "n".to_string()),
        ]);
        let out = overlay(&file, &local);
        assert_eq!(out[0], v("host", "file.test"));
        assert_eq!(out[1], v("off", "on"));
        assert_eq!(out[2], Variable { secret: true, ..v("key", "s3cret") });
        assert_eq!(out[3], v("new", "n"));
    }
}

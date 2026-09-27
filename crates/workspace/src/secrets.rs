//! Values of variables marked secret. They live in the local app data dir,
//! never in workspace files, so they are not committed to Git.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

use zorvik_formats::Variable;

type Scopes = BTreeMap<String, BTreeMap<String, String>>;

pub struct SecretStore {
    path: Option<PathBuf>,
    scopes: Mutex<Scopes>,
}

/// Scope name for a workspace's own variables.
pub fn workspace_scope(workspace_id: &str) -> String {
    format!("{workspace_id}/workspace")
}

/// Scope name for an environment.
pub fn env_scope(workspace_id: &str, env_id: &str) -> String {
    format!("{workspace_id}/env/{env_id}")
}

impl SecretStore {
    pub fn in_memory() -> Self {
        Self { path: None, scopes: Mutex::new(Scopes::new()) }
    }

    pub fn open(path: PathBuf) -> Self {
        let scopes = match std::fs::read(&path) {
            Ok(data) => serde_json::from_slice(&data).unwrap_or_else(|e| {
                // Keep the unreadable file: the next save would otherwise wipe every secret.
                let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
                let aside = path.with_extension(format!("corrupt-{}.json", stamp.as_secs()));
                tracing::error!("secrets file unreadable ({e}); moved to {}", aside.display());
                let _ = std::fs::rename(&path, &aside);
                Scopes::new()
            }),
            Err(_) => Scopes::new(),
        };
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

    /// Fill in secret values (the files keep them empty).
    pub fn reveal(&self, scope: &str, vars: &mut [Variable]) {
        let scopes = self.lock();
        let Some(values) = scopes.get(scope) else { return };
        for v in vars.iter_mut().filter(|v| v.secret) {
            if let Some(value) = values.get(&v.key) {
                v.value = value.clone();
            }
        }
    }

    /// Store secret values from `vars` and return the variables with those values blanked.
    pub fn conceal(&self, scope: &str, vars: &[Variable]) -> crate::Result<Vec<Variable>> {
        let mut values = BTreeMap::new();
        let blanked = vars
            .iter()
            .map(|v| {
                if v.secret {
                    values.insert(v.key.clone(), v.value.clone());
                    Variable { value: String::new(), ..v.clone() }
                } else {
                    v.clone()
                }
            })
            .collect();
        let mut scopes = self.lock();
        if values.is_empty() {
            scopes.remove(scope);
        } else {
            scopes.insert(scope.to_string(), values);
        }
        self.persist(&scopes)?;
        Ok(blanked)
    }

    pub fn rename_scope(&self, from: &str, to: &str) -> crate::Result<()> {
        let mut scopes = self.lock();
        if let Some(values) = scopes.remove(from) {
            scopes.insert(to.to_string(), values);
            self.persist(&scopes)?;
        }
        Ok(())
    }

    /// Move every scope under `from/` to `to/`, unless `to/` already has secrets
    /// (used once when a workspace's data moves from its id to its local key).
    pub fn move_prefix(&self, from: &str, to: &str) -> crate::Result<()> {
        let (from, to) = (format!("{from}/"), format!("{to}/"));
        let mut scopes = self.lock();
        if scopes.keys().any(|k| k.starts_with(&to)) {
            return Ok(());
        }
        let moved: Vec<String> = scopes.keys().filter(|k| k.starts_with(&from)).cloned().collect();
        if moved.is_empty() {
            return Ok(());
        }
        for key in moved {
            let values = scopes.remove(&key).unwrap_or_default();
            scopes.insert(format!("{to}{}", &key[from.len()..]), values);
        }
        self.persist(&scopes)
    }

    pub fn remove_scope(&self, scope: &str) -> crate::Result<()> {
        let mut scopes = self.lock();
        if scopes.remove(scope).is_some() {
            self.persist(&scopes)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(k: &str, val: &str, secret: bool) -> Variable {
        Variable { key: k.into(), value: val.into(), enabled: true, secret }
    }

    #[test]
    fn conceal_reveal_persist() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.json");
        let store = SecretStore::open(path.clone());
        let blanked = store.conceal("w/env/dev", &[v("token", "s3", true), v("host", "h", false)]).unwrap();
        assert_eq!(blanked[0].value, "");
        assert_eq!(blanked[1].value, "h");
        let reopened = SecretStore::open(path);
        let mut vars = blanked.clone();
        reopened.reveal("w/env/dev", &mut vars);
        assert_eq!(vars[0].value, "s3");
        reopened.rename_scope("w/env/dev", "w/env/prod").unwrap();
        let mut vars = blanked.clone();
        reopened.reveal("w/env/prod", &mut vars);
        assert_eq!(vars[0].value, "s3");
        reopened.move_prefix("w", "w-k").unwrap();
        let mut vars = blanked.clone();
        reopened.reveal("w-k/env/prod", &mut vars);
        assert_eq!(vars[0].value, "s3");
        reopened.move_prefix("w-k", "w").unwrap();
        reopened.conceal("w-k/workspace", &[v("x", "1", true)]).unwrap();
        reopened.move_prefix("w", "w-k").unwrap(); // target already has secrets: nothing moves
        let mut vars = blanked.clone();
        reopened.reveal("w/env/prod", &mut vars);
        assert_eq!(vars[0].value, "s3");
        reopened.remove_scope("w/env/prod").unwrap();
        let mut vars = blanked;
        reopened.reveal("w/env/prod", &mut vars);
        assert_eq!(vars[0].value, "");
    }

    #[test]
    fn unreadable_file_is_kept_aside() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.json");
        std::fs::write(&path, "{ not json").unwrap();
        let store = SecretStore::open(path.clone());
        store.conceal("w/workspace", &[v("t", "x", true)]).unwrap();
        let kept: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .filter(|n| n.starts_with("secrets.corrupt-"))
            .collect();
        assert_eq!(kept.len(), 1, "{kept:?}");
        assert_eq!(std::fs::read_to_string(dir.path().join(&kept[0])).unwrap(), "{ not json");
    }
}

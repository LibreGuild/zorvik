//! How `zorvik mcp` finds the running app: `agent.json` in the app's data dir
//! (port and token), readable by the current user only.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The app's bundle identifier: its data dir is `<OS data dir>/<identifier>`, like Tauri's.
pub const APP_IDENTIFIER: &str = "org.libreguild.zorvik";
const AGENT_FILE: &str = "agent.json";

/// The app's data dir (`ZORVIK_DATA_DIR` overrides it, e.g. for tests).
pub fn default_data_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("ZORVIK_DATA_DIR").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    Some(dirs::data_dir()?.join(APP_IDENTIFIER))
}

/// Where the running app listens for agents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentFile {
    pub port: u16,
    pub token: String,
    pub pid: u32,
}

impl AgentFile {
    pub fn path(data_dir: &Path) -> PathBuf {
        data_dir.join(AGENT_FILE)
    }

    pub fn read(data_dir: &Path) -> Option<Self> {
        let data = std::fs::read(Self::path(data_dir)).ok()?;
        serde_json::from_slice(&data).ok()
    }

    /// Written to a temporary file first (created user-only), then moved in place.
    pub fn write(&self, data_dir: &Path) -> io::Result<()> {
        std::fs::create_dir_all(data_dir)?;
        let path = Self::path(data_dir);
        let tmp = data_dir.join(format!(".{AGENT_FILE}.{}", std::process::id()));
        let _ = std::fs::remove_file(&tmp);
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let mut file = options.open(&tmp)?;
        io::Write::write_all(&mut file, &serde_json::to_vec(self).map_err(io::Error::other)?)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&tmp, &path)
    }

    /// Remove the file if it is still ours (another app instance may have replaced it).
    pub fn remove_if_ours(&self, data_dir: &Path) {
        if Self::read(data_dir).as_ref() == Some(self) {
            let _ = std::fs::remove_file(Self::path(data_dir));
        }
    }
}

/// Proof of knowing the token for one connection: `role` ("bridge" or "app") and a
/// fresh nonce, so neither side ever sends the token itself.
pub(crate) fn proof(token: &str, role: &str, nonce: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(format!("zorvik-agent\n{role}\n{nonce}\n{token}").as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// Compare secrets in constant time.
pub(crate) fn same_secret(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_file_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let file = AgentFile { port: 4242, token: "t".repeat(64), pid: 7 };
        file.write(dir.path()).unwrap();
        assert_eq!(AgentFile::read(dir.path()), Some(file.clone()));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(AgentFile::path(dir.path())).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        AgentFile { pid: 8, ..file.clone() }.remove_if_ours(dir.path());
        assert!(AgentFile::read(dir.path()).is_some(), "someone else's file stays");
        file.remove_if_ours(dir.path());
        assert!(AgentFile::read(dir.path()).is_none());
        assert!(same_secret("abc", "abc") && !same_secret("abc", "abd") && !same_secret("abc", "ab"));
        assert_eq!(proof("t", "app", "n"), proof("t", "app", "n"));
        assert_ne!(proof("t", "app", "n"), proof("t", "bridge", "n"));
        assert_ne!(proof("t", "app", "n"), proof("t", "app", "m"));
    }
}

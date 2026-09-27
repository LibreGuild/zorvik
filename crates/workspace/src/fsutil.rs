//! File helpers: atomic writes, YAML I/O and cross-platform safe file names.

use std::collections::HashSet;
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::error::{Error, ErrorCode, Result};

/// Largest YAML file read from a workspace. Workspaces come from Git, so a
/// huge file (or a link to a device such as /dev/zero) must not exhaust memory.
pub const MAX_YAML_FILE: u64 = 50 * 1024 * 1024;

/// Write via a temp file + rename so a crash never leaves a half-written file.
pub fn atomic_write(path: &Path, data: &[u8]) -> Result<()> {
    write_via_temp(path, data, false)
}

/// Like [`atomic_write`], but owner-only (0600 on Unix) from the moment the
/// file exists. For secrets and tokens.
pub fn atomic_write_private(path: &Path, data: &[u8]) -> Result<()> {
    write_via_temp(path, data, true)
}

fn write_via_temp(path: &Path, data: &[u8], private: bool) -> Result<()> {
    let dir = path.parent().ok_or_else(|| Error::invalid(format!("Invalid path {}", path.display())))?;
    std::fs::create_dir_all(dir).map_err(|e| Error::io(format!("Could not create {}", dir.display()), e))?;
    // Unique per write, so concurrent saves of one file never share a temp
    // file; `create_new` also refuses to write through a planted symlink.
    let name: String = path.file_name().and_then(|n| n.to_str()).unwrap_or("file").chars().take(64).collect();
    let tmp = dir.join(format!(".{name}.{}.tmp", uuid::Uuid::new_v4().simple()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    if private {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    #[cfg(not(unix))]
    let _ = private;
    let mut file = options.open(&tmp).map_err(|e| Error::io(format!("Could not write {}", tmp.display()), e))?;
    if let Err(e) = file.write_all(data) {
        drop(file);
        let _ = std::fs::remove_file(&tmp);
        return Err(Error::io(format!("Could not write {}", tmp.display()), e));
    }
    drop(file);
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        Error::io(format!("Could not save {}", path.display()), e)
    })
}

/// Make an existing file owner-only (0600) on Unix.
pub(crate) fn restrict_to_owner(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    #[cfg(not(unix))]
    let _ = path;
}

/// True when `path` itself is a symbolic link (or a Windows junction).
/// Workspaces come from Git, so links are never followed.
pub fn is_symlink(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink())
}

pub fn read_yaml<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let text = read_text(path)?;
    parse_yaml(&text).map_err(|e| Error::new(ErrorCode::Parse, format!("{}: {}", path.display(), e.message)))
}

/// Read a regular file of at most [`MAX_YAML_FILE`] bytes as UTF-8.
fn read_text(path: &Path) -> Result<String> {
    let read_err = |e| Error::io(format!("Could not read {}", path.display()), e);
    let meta = std::fs::metadata(path).map_err(read_err)?;
    if !meta.is_file() {
        return Err(Error::invalid(format!("{} is not a regular file", path.display())));
    }
    let too_big = || Error::invalid(format!("{} is larger than {} MB", path.display(), MAX_YAML_FILE >> 20));
    if meta.len() > MAX_YAML_FILE {
        return Err(too_big());
    }
    let mut text = String::new();
    std::fs::File::open(path).and_then(|f| f.take(MAX_YAML_FILE + 1).read_to_string(&mut text)).map_err(read_err)?;
    if text.len() as u64 > MAX_YAML_FILE {
        return Err(too_big());
    }
    Ok(text)
}

pub fn parse_yaml<T: DeserializeOwned>(text: &str) -> Result<T> {
    // An empty file is treated like an empty mapping.
    let text = if text.trim().is_empty() { "{}" } else { text };
    serde_yaml_ng::from_str(text).map_err(|e| Error::new(ErrorCode::Parse, format!("Invalid YAML: {e}")))
}

pub fn write_yaml<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let text = serde_yaml_ng::to_string(value).map_err(|e| Error::invalid(format!("Could not serialize: {e}")))?;
    atomic_write(path, text.as_bytes())
}

const RESERVED: &[&str] = &[
    "con", "prn", "aux", "nul", "conin$", "conout$", "com0", "com1", "com2", "com3", "com4", "com5", "com6", "com7",
    "com8", "com9", "com¹", "com²", "com³", "lpt0", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8",
    "lpt9", "lpt¹", "lpt²", "lpt³",
];

/// Length of the part of a file name Windows checks for device names: `NUL`,
/// `nul.txt` and `NUL .tar.gz` all mean the NUL device.
fn device_base_len(name: &str) -> usize {
    name.split('.').next().unwrap_or_default().trim_end_matches(' ').len()
}

/// True for names Windows reserves for devices (`CON`, `nul.yaml`, `COM1`, …).
pub fn is_reserved_name(name: &str) -> bool {
    let base = &name[..device_base_len(name)];
    RESERVED.iter().any(|r| r.eq_ignore_ascii_case(base))
}

/// Turn a display name into a file-name stem that is valid on Windows, macOS and Linux.
pub fn sanitize_file_stem(name: &str) -> String {
    let mut out: String =
        name.chars()
            .map(|c| {
                if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
                    '-'
                } else {
                    c
                }
            })
            .collect();
    out = out.trim().trim_end_matches(['.', ' ']).trim_start_matches('.').to_string();
    if out.chars().count() > 80 {
        out = out.chars().take(80).collect::<String>().trim_end_matches(['.', ' ']).to_string();
    }
    // `_Folder.yaml` is the folder settings file on case-insensitive file systems.
    if out.is_empty() || out.eq_ignore_ascii_case("_folder") {
        out = "untitled".into();
    }
    if is_reserved_name(&out) {
        out.insert(device_base_len(&out), '_');
    }
    out
}

/// A name `stem[ N]ext` in `dir` that doesn't collide (case-insensitively) with
/// existing entries, ignoring `exclude` (the item being renamed).
pub fn unique_name(dir: &Path, stem: &str, ext: &str, exclude: Option<&Path>) -> String {
    let existing: HashSet<String> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| exclude.is_none_or(|x| e.path() != x))
                .map(|e| e.file_name().to_string_lossy().to_lowercase())
                .collect()
        })
        .unwrap_or_default();
    let mut n = 1;
    loop {
        let candidate = if n == 1 { format!("{stem}{ext}") } else { format!("{stem} {n}{ext}") };
        if !existing.contains(&candidate.to_lowercase()) {
            return candidate;
        }
        n += 1;
    }
}

/// Recursively copy a directory. Symbolic links are skipped: following one
/// could copy files from outside the workspace into it, or loop forever.
pub fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to).map_err(|e| Error::io(format!("Could not create {}", to.display()), e))?;
    let entries = std::fs::read_dir(from).map_err(|e| Error::io(format!("Could not read {}", from.display()), e))?;
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else { continue };
        let target: PathBuf = to.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else if file_type.is_file() {
            std::fs::copy(entry.path(), &target)
                .map_err(|e| Error::io(format!("Could not copy {}", entry.path().display()), e))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_names_for_all_platforms() {
        assert_eq!(sanitize_file_stem("Get user: by id?"), "Get user- by id-");
        assert_eq!(sanitize_file_stem("  ..hidden.  "), "hidden");
        assert_eq!(sanitize_file_stem("CON"), "CON_");
        assert_eq!(sanitize_file_stem("a/b\\c"), "a-b-c");
        assert_eq!(sanitize_file_stem(""), "untitled");
        assert_eq!(sanitize_file_stem("_folder"), "untitled");
        assert_eq!(sanitize_file_stem(&"x".repeat(200)).len(), 80);
        assert_eq!(sanitize_file_stem("Ünïcödé ✓"), "Ünïcödé ✓");
    }

    #[test]
    fn reserved_names_with_extensions_and_folder_file_case() {
        // Windows maps `nul.backup.yaml` to the NUL device too.
        assert_eq!(sanitize_file_stem("nul.backup"), "nul_.backup");
        assert_eq!(sanitize_file_stem("Com1 .x"), "Com1_ .x");
        assert_eq!(sanitize_file_stem("LPT0"), "LPT0_");
        assert_eq!(sanitize_file_stem("com¹"), "com¹_");
        assert_eq!(sanitize_file_stem("console"), "console");
        assert_eq!(sanitize_file_stem("_FOLDER"), "untitled");
        assert!(is_reserved_name("aux.yaml") && !is_reserved_name("auxiliary.yaml"));
    }

    #[test]
    fn unique_names_are_case_insensitive() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Get.yaml"), "").unwrap();
        assert_eq!(unique_name(dir.path(), "get", ".yaml", None), "get 2.yaml");
        assert_eq!(unique_name(dir.path(), "get", ".yaml", Some(&dir.path().join("Get.yaml"))), "get.yaml");
        assert_eq!(unique_name(dir.path(), "post", ".yaml", None), "post.yaml");
    }

    #[test]
    fn atomic_write_replaces_existing() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.yaml");
        atomic_write(&p, b"one").unwrap();
        atomic_write(&p, b"two").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"two");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn concurrent_atomic_writes_never_corrupt() {
        // Writers used to share one temp file per process and could rename
        // each other's half-written data into place.
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("cookies.json");
        std::thread::scope(|s| {
            for t in 0..8u8 {
                let p = &p;
                s.spawn(move || {
                    let data = vec![b'a' + t; 256 * 1024];
                    for _ in 0..20 {
                        let result = atomic_write(p, &data);
                        // Windows may refuse to replace a file that another rename is replacing.
                        if cfg!(unix) {
                            result.unwrap();
                        }
                    }
                });
            }
        });
        let data = std::fs::read(&p).unwrap();
        assert_eq!(data.len(), 256 * 1024);
        assert!(data.iter().all(|b| *b == data[0]));
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn private_files_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("secrets.json");
        atomic_write_private(&p, b"{}").unwrap();
        assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn yaml_reads_refuse_devices_and_huge_files() {
        let dir = tempfile::tempdir().unwrap();
        let link = dir.path().join("zero.yaml");
        std::os::unix::fs::symlink("/dev/zero", &link).unwrap();
        assert!(read_yaml::<serde_json::Value>(&link).is_err());
        let big = dir.path().join("big.yaml");
        std::fs::File::create(&big).unwrap().set_len(MAX_YAML_FILE + 1).unwrap();
        assert!(read_yaml::<serde_json::Value>(&big).unwrap_err().message.contains("larger than"));
    }

    #[cfg(unix)]
    #[test]
    fn copy_dir_skips_symlinks() {
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("id_rsa"), "private").unwrap();
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir(&src).unwrap();
        std::fs::write(src.join("a.yaml"), "name: a").unwrap();
        std::os::unix::fs::symlink(outside.path().join("id_rsa"), src.join("key.yaml")).unwrap();
        std::os::unix::fs::symlink(&src, src.join("loop")).unwrap();
        copy_dir(&src, &dir.path().join("dst")).unwrap();
        let copied: Vec<_> =
            std::fs::read_dir(dir.path().join("dst")).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(copied, ["a.yaml"]);
    }
}

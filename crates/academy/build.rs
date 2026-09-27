//! Embeds the course: every file under `course/` becomes `(relative path, contents)` in
//! `COURSE_FILES`, sorted, so adding a lesson needs no code change.

use std::path::{Path, PathBuf};

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir).expect("read course dir").flatten().map(|e| e.path()).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            collect(&path, out);
        } else if matches!(path.extension().and_then(|e| e.to_str()), Some("md" | "yaml")) {
            out.push(path);
        }
    }
}

fn main() {
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("course");
    println!("cargo:rerun-if-changed={}", root.display());
    let mut files = Vec::new();
    collect(&root, &mut files);
    let mut code = String::from("pub(crate) const COURSE_FILES: &[(&str, &str)] = &[\n");
    for path in &files {
        println!("cargo:rerun-if-changed={}", path.display());
        let rel = path.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
        code.push_str(&format!("    ({rel:?}, include_str!({:?})),\n", path.display().to_string()));
    }
    code.push_str("];\n");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("course_files.rs");
    std::fs::write(out, code).expect("write course_files.rs");
}

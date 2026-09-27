// Set the version everywhere it is written down, before tagging a release:
//   npm run set-version -- 0.2.0
// Cargo.toml (all crates), app/package.json, app/package-lock.json and tauri.conf.json.
// CI refuses a release tag that doesn't match these.
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

const version = process.argv[2];
if (!/^\d+\.\d+\.\d+$/.test(version ?? "")) {
  console.error("Usage: npm run set-version -- <major.minor.patch>");
  process.exit(2);
}

const app = path.resolve(import.meta.dirname, "..");
const root = path.resolve(app, "..");

/** Replace the first `count` `"version": "…"` fields of a JSON file, keeping its formatting. */
function setJsonVersion(file, count = 1) {
  let left = count;
  const text = fs.readFileSync(file, "utf8").replace(/("version":\s*)"[^"]*"/g, (all, key) => (left-- > 0 ? `${key}"${version}"` : all));
  if (left > 0) {
    console.error(`Could not find the version in ${file}`);
    process.exit(1);
  }
  fs.writeFileSync(file, text);
}

// The workspace version is the first `version = "…"` line after `[workspace.package]`.
const cargoToml = path.join(root, "Cargo.toml");
const toml = fs.readFileSync(cargoToml, "utf8");
const updated = toml.replace(/(\[workspace\.package\][^[]*?\nversion = )"[^"]*"/, `$1"${version}"`);
if (updated === toml && !toml.includes(`version = "${version}"`)) {
  console.error("Could not find the version in [workspace.package] of Cargo.toml");
  process.exit(1);
}
fs.writeFileSync(cargoToml, updated);

setJsonVersion(path.join(app, "package.json"));
// The lockfile's own version and its root package's, the first two in the file.
setJsonVersion(path.join(app, "package-lock.json"), 2);
setJsonVersion(path.join(app, "src-tauri", "tauri.conf.json"));

// Cargo.lock records every crate's version: update only this workspace's own entries.
execFileSync("cargo", ["update", "--workspace"], { cwd: root, stdio: "ignore" });
console.log(`Version set to ${version}. Commit the changes, then tag v${version}.`);

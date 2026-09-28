// Writes latest.json, the file the app's updater reads (app/src-tauri/src/updates.rs), from
// the signatures the sign-updates job made (ci.yml). Usage:
//   node update-manifest.mjs <dir with .sig files> <download base URL> <version> <pub date> <notes> > latest.json
// Only platforms whose signature is present are listed. Exits with 2 when there is none
// (the updater key isn't set up): the release then goes out without automatic updates.
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";

const [dir, base, version, pubDate, notes] = process.argv.slice(2);
if (!dir || !base || !version || !pubDate) {
  console.error("usage: update-manifest.mjs <dir> <base url> <version> <pub date> [notes]");
  process.exit(1);
}

// Updater target → the file it installs (the macOS archive is universal: both chips use it).
const PLATFORMS = {
  "windows-x86_64": "Zorvik-Windows-Setup-x64.exe",
  "darwin-aarch64": "Zorvik-macOS-universal.app.tar.gz",
  "darwin-x86_64": "Zorvik-macOS-universal.app.tar.gz",
  "linux-x86_64": "Zorvik-Linux-x86_64.AppImage",
};

const platforms = {};
for (const [target, file] of Object.entries(PLATFORMS)) {
  const sig = join(dir, `${file}.sig`);
  if (!existsSync(sig)) continue;
  platforms[target] = { signature: readFileSync(sig, "utf8").trim(), url: `${base.replace(/\/?$/, "/")}${encodeURIComponent(file)}` };
}
if (!Object.keys(platforms).length) {
  console.error("no update signatures found: the updater key is not set up");
  process.exit(2);
}
process.stdout.write(`${JSON.stringify({ version, notes: notes ?? "", pub_date: pubDate, platforms }, null, 2)}\n`);

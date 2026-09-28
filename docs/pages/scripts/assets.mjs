// Copies the pictures the website shows from the repository (README screenshots, Academy art),
// so they aren't stored twice. Runs before `astro dev` and `astro build`.
import { cpSync, mkdirSync, readdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const site = join(dirname(fileURLToPath(import.meta.url)), "..");
const repo = join(site, "..", "..");

function copy(from, to, keep = () => true) {
  mkdirSync(to, { recursive: true });
  for (const name of readdirSync(from).filter(keep)) cpSync(join(from, name), join(to, name));
}

copy(join(repo, ".github", "assets"), join(site, "public", "shots"), (n) => n.endsWith(".webp"));
copy(join(repo, "app", "src", "assets", "academy"), join(site, "public", "art"), (n) => n.endsWith(".webp"));
copy(join(repo, "app", "src", "assets", "docs"), join(site, "public", "art"), (n) => n.endsWith(".webp"));

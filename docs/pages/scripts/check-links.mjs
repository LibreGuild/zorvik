// Checks the built site (dist/): every link and image inside the site points at a file that
// exists, and every #anchor at an id on that page. Run after `npm run build`.
import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const dist = join(dirname(fileURLToPath(import.meta.url)), "..", "dist");
const BASE = "/zorvik/";
const pages = [];
const walk = (dir) => {
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) walk(path);
    else if (name.endsWith(".html")) pages.push(path);
  }
};
walk(dist);

const ids = new Map();
const idsOf = (file) => {
  if (!ids.has(file)) ids.set(file, new Set([...readFileSync(file, "utf8").matchAll(/\sid="([^"]+)"/g)].map((m) => m[1])));
  return ids.get(file);
};
const target = (url, from) => {
  const path = url.startsWith("/") ? url : new URL(url, `http://x${from}`).pathname;
  if (!path.startsWith(BASE)) return null;
  const rel = decodeURIComponent(path.slice(BASE.length));
  const file = join(dist, rel);
  if (rel === "" || rel.endsWith("/")) return join(file, "index.html");
  if (existsSync(file) && statSync(file).isFile()) return file;
  return existsSync(join(file, "index.html")) ? join(file, "index.html") : file;
};

let broken = 0;
for (const page of pages) {
  const html = readFileSync(page, "utf8");
  const here = "/" + page.slice(dist.length + 1).replace(/index\.html$/, "").replace(/\\/g, "/");
  const from = BASE + here.slice(1);
  for (const m of html.matchAll(/\s(?:href|src)="([^"]+)"/g)) {
    const url = m[1];
    if (/^(https?:|mailto:|data:|javascript:)/.test(url) || url.startsWith("//")) continue;
    const [path, hash] = url.split("#");
    const file = path ? target(path, from) : page;
    if (file === null) continue;
    if (!existsSync(file)) {
      console.log(`${here}: missing ${url}`);
      broken++;
    } else if (hash && file.endsWith(".html") && !idsOf(file).has(decodeURIComponent(hash))) {
      console.log(`${here}: no #${hash} in ${url}`);
      broken++;
    }
  }
}
console.log(`${pages.length} pages checked, ${broken} broken link(s)`);
process.exit(broken ? 1 : 0);

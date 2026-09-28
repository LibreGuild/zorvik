// Where this build of the website is published. The main site (/zorvik/) shows the latest
// release; the preview of the next version (/zorvik/next/) is built from main with
// SITE_BASE=/zorvik/next SITE_CHANNEL=next. The Pages workflow builds both and puts them together.
// Read by astro.config.mjs, the pages and scripts/check-links.mjs (SITE_BASE only).
import { existsSync } from "node:fs";
import { join } from "node:path";

/** The main site's base path. */
export const RELEASE_BASE = "/zorvik";

/** This build's base path, without a trailing slash. */
export const BASE = (process.env.SITE_BASE || RELEASE_BASE).replace(/\/+$/, "");

/** The preview of the next version: a banner on every page, and kept out of search engines. */
export const NEXT = process.env.SITE_CHANNEL === "next";

// The main site's build (dist/), so the preview's banner links to the same page there only when
// it exists. The workflow sets it; without it (a local build), every page is taken to exist.
const releaseDist = process.env.SITE_RELEASE_DIST;
if (releaseDist && !existsSync(join(releaseDist, "index.html"))) {
  throw new Error(`SITE_RELEASE_DIST: no built site in ${releaseDist}`);
}

/**
 * Where a page of this build is on the main site: the same page when the main site has it,
 * otherwise the docs home. `kind` says which ("home", "page" or "docs").
 */
export function onMainSite(pathname) {
  const path = pathname.startsWith(`${BASE}/`) ? pathname.slice(BASE.length + 1) : "";
  if (path === "") return { href: `${RELEASE_BASE}/`, kind: "home" };
  const known = !releaseDist || existsSync(join(releaseDist, decodeURI(path), "index.html"));
  // The "page not found" page (404.html) has no address of its own.
  if (known && !/^404\/?$/.test(path)) return { href: `${RELEASE_BASE}/${path}`, kind: "page" };
  return { href: `${RELEASE_BASE}/docs/`, kind: "docs" };
}

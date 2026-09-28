# Website

The Zorvik website, published to GitHub Pages by [`.github/workflows/pages.yml`](../../.github/workflows/pages.yml) in two parts: the main site at <https://libreguild.github.io/zorvik/>, built from the latest release, and a preview of the next version at <https://libreguild.github.io/zorvik/next/>, built from `main`. So a change here shows in the preview when it merges and on the main site with the next release (see [Website](../ci-release.md#website)).

- **Home page:** [`src/pages/index.astro`](src/pages/index.astro) and [`src/styles/site.css`](src/styles/site.css). Static HTML and CSS; a few small scripts pick the download for the visitor's system, switch the screenshots, and refresh the version (the download count is hidden for now; see the comment in `index.astro`).
- **Docs:** Markdown under [`src/content/docs/docs/`](src/content/docs/docs), served at `/docs` with [Starlight](https://starlight.astro.build) (sidebar, search, dark and light themes). The sidebar groups follow the folders; `sidebar.order` in each page's front matter sets the order.
- **Release data:** [`src/lib/releases.ts`](src/lib/releases.ts) reads the latest release from GitHub when the site is built. Download buttons use `releases/latest/download/<file>`, so they always point at the newest version.
- **Pictures:** the README screenshots (`.github/assets`) and the Academy art (`app/src/assets`) are copied in at build time by [`scripts/assets.mjs`](scripts/assets.mjs), not stored twice.
- **Page views:** GoatCounter (no cookies, nothing personal), at <https://zorvik.goatcounter.com>. Download buttons are counted as clicks too.
- **Main site or preview:** [`src/lib/site.mjs`](src/lib/site.mjs) reads `SITE_BASE` (the base path, `/zorvik` by default) and `SITE_CHANNEL`. With `SITE_CHANNEL=next`, every page gets the preview banner ([`src/components/NextBanner.astro`](src/components/NextBanner.astro)) and `noindex`, there is no sitemap, and pictures written as `/zorvik/...` in the docs move to the preview's base.

## Work on it
```bash
cd docs/pages
npm install
npm run dev        # http://localhost:4321/zorvik/
npm run build      # into dist/
npm run check      # every link and #anchor in dist/
```
A `GITHUB_TOKEN` in the environment avoids GitHub's anonymous rate limit when the build reads the releases.

The preview, as published at `/zorvik/next/`:
```bash
SITE_BASE=/zorvik/next SITE_CHANNEL=next npm run build
SITE_BASE=/zorvik/next npm run check
```
Locally the banner links every page to the same page on the main site. The workflow also sets `SITE_RELEASE_DIST` to the main site's `dist/`, so pages that are new link to the main site's docs home instead.

## Writing docs
- One page per topic, with `title`, `description` and `sidebar.order` in the front matter.
- Link to other pages with relative links ending in `/` (for example `../pm-reference/`).
- Check every statement against the code: the docs describe what Zorvik does, not what it might do.

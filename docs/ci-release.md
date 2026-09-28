# CI and releases

Three workflows:
- [`ci.yml`](../.github/workflows/ci.yml): the checks on every pull request and push, and the builds of releases and nightlies.
- [`nightly.yml`](../.github/workflows/nightly.yml): once a day, starts a nightly build when `main` changed.
- [`pages.yml`](../.github/workflows/pages.yml): the website on GitHub Pages.

## Checks (every pull request and every push)
| Job | What it does |
|---|---|
| Rust (Linux) | `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`, and that the generated files (TypeScript bindings, the dynamic variables reference) are committed |
| Rust (Windows) | `cargo test` on real Windows networking, paths and trash |
| UI | TypeScript typecheck, Vitest unit tests, production build |
| E2E | Playwright drives the real UI against the real Rust API and local test servers |

A pull request can merge only when all four pass. Pull requests from first-time contributors wait for a maintainer to approve the workflow run.

## Release channels
| Channel | Made by | Where | Version |
|---|---|---|---|
| **Release** | pushing a tag `vX.Y.Z` | [Releases → Latest](https://github.com/LibreGuild/zorvik/releases/latest) | `X.Y.Z` |
| **Nightly** | once a day (03:17 UTC), when `main` has new commits since the last nightly | the `nightly` pre-release, replaced each time | the version in the code, plus "nightly N · commit" in Settings |

A push to `main` only runs the checks. The nightly workflow compares `main` with the `nightly` tag and, when they differ, starts CI on `main` by hand (`workflow_dispatch`), which builds and publishes. A maintainer can do the same at any time: **Actions → CI → Run workflow** on `main`.

After the checks pass, `release-draft` creates a draft release for the run and checks that the tag matches the version in `Cargo.toml`, `app/package.json` and `tauri.conf.json`. Then three jobs build in parallel with `npm run package` (the app with the `zorvik` command line inside), pack the command line on its own, and upload to the draft. Then `sign-updates` signs the files the app's updater installs and adds `latest.json` (see [Updates](#updates)). Finally `publish` makes the draft public: as the tagged release with notes, or as the new nightly. If any build fails, nothing is published and the previous release stays.

## Cutting a release
1. Make sure `main` is green and the nightly build works.
2. Set the version and commit it in a pull request:
   ```bash
   cd app && npm run set-version -- 0.2.0
   ```
   This updates `Cargo.toml`, `Cargo.lock`, `app/package.json`, `app/package-lock.json` and `tauri.conf.json`. In the same pull request, move the `Unreleased` notes in [`CHANGELOG.md`](../CHANGELOG.md) under the new version.
3. After it merges, tag the merge commit. The tag message opens the release notes:
   ```bash
   git switch main && git pull
   git tag -a v0.2.0 -m "Highlights of this release, in a few lines."
   git push origin v0.2.0
   ```
4. CI builds and publishes "Zorvik 0.2.0", then rebuilds the [website](#website), whose main site now shows this version's docs. Below the tag message, the notes list the downloads and the merged pull requests, grouped by label ([`.github/release.yml`](../.github/release.yml)).

Only maintainers can push `v*` tags. Versions follow [SemVer](https://semver.org): while Zorvik is `0.x`, a minor bump may change behaviour; patch releases only fix.

## Downloads
Every app download has the app **and** the `zorvik` command line.

| System | File | Notes |
|---|---|---|
| Windows 10/11 | `Zorvik-Windows-Setup-x64.exe` | Installs for the current user (no admin rights) and puts `zorvik` on PATH; the uninstaller removes it again |
| Windows 10/11 | `Zorvik-Windows-Portable-x64.zip` | No install: `zorvik-desktop.exe` (the app) and `zorvik.exe` (the command line) |
| macOS 11+ | `Zorvik-macOS-universal.dmg` | Apple silicon and Intel. Settings → AI agents → **Add zorvik to PATH** links the command line into `/usr/local/bin` |
| Ubuntu 22.04+, Debian 12+ | `Zorvik-Linux-amd64.deb` | `zorvik` goes to `/usr/bin` |
| Fedora, RHEL, openSUSE | `Zorvik-Linux-x86_64.rpm` | `zorvik` goes to `/usr/bin` |
| Any Linux (x86-64) | `Zorvik-Linux-x86_64.AppImage` | Runs anywhere; use the deb or rpm to get `zorvik` on PATH |

The command line on its own, for CI machines and servers (each archive has `zorvik` and the licenses):

| System | File |
|---|---|
| Windows (x64) | `zorvik-cli-windows-x64.zip` |
| macOS (Apple silicon and Intel) | `zorvik-cli-macos-universal.tar.gz` |
| Linux (x86-64, glibc 2.35+) | `zorvik-cli-linux-x86_64.tar.gz` |

Linux packages are built on Ubuntu 22.04, the oldest supported system, so they run on newer ones.

Two more files are for the updater, not for people: `latest.json` and `Zorvik-macOS-universal.app.tar.gz` (the macOS app the updater installs).

## Updates
The app updates itself from GitHub Releases (`app/src-tauri/src/updates.rs`, the Tauri updater). It reads one file:
- stable: `releases/latest/download/latest.json`, which GitHub serves from the newest release;
- nightly: `releases/download/nightly/latest.json`.

`latest.json` lists, per platform, the file to install and its signature: the Windows installer, `Zorvik-macOS-universal.app.tar.gz` (packed by the macOS build, with `Zorvik.app` at its root) and the AppImage. For a nightly, `pub_date` is the build time, which the app also has built in (`ZORVIK_BUILT_AT`), so a nightly with the same version number is still recognised as newer.

The app accepts an update only when its signature matches the public key built into `tauri.conf.json` (`plugins.updater.pubkey`), and only for the version the signature names. When an update can't be downloaded, verified or installed, the app doesn't show an error: it says the new version is out and links to its release page.

### Signing
The build jobs never see the private key. After they finish, the `sign-updates` job downloads the three update files from the draft, signs each with `tauri signer sign --app-version <version>` (the Tauri CLI, pinned to the app's version), writes `latest.json` with `.github/scripts/update-manifest.mjs`, and uploads it. Only this job gets the key: it runs in the GitHub environment **`release`**, whose secrets `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` are only given to runs from `main` and from `v*` tags (Settings → Environments → release: deployment branches and tags, no admin bypass). A workflow on any other branch, and every pull request, gets nothing. Without the secrets, releases still publish, without automatic updates (the job warns).

### The updater key
The key pair is set up: the public key is in `tauri.conf.json`, the private key and its password are in the `release` environment, and the maintainers keep a backup. **Never lose it or replace it casually:** installed copies only accept updates signed with this key. With a new key, installed copies can't verify the next update, so they show "download it from GitHub" once instead of updating themselves. If it ever has to be replaced (lost or leaked):
```bash
cd app && npx tauri signer generate -w ~/.tauri/zorvik-updater.key   # choose a password
gh secret set TAURI_SIGNING_PRIVATE_KEY --repo LibreGuild/zorvik --env release < ~/.tauri/zorvik-updater.key
gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD --repo LibreGuild/zorvik --env release   # paste the password
```
Then put the contents of `~/.tauri/zorvik-updater.key.pub` in `plugins.updater.pubkey` and release a new version with it.

### Testing an update locally
With a throwaway key (`npx tauri signer generate`):
1. Build two versions with the throwaway public key: `npm run package -- --version 0.9.1 --bundles app --config '{"plugins":{"updater":{"pubkey":"<pub>","dangerousInsecureTransportProtocol":true}}}'`, and the same for `0.9.0`. Keep a copy of each `Zorvik.app`.
2. Pack and sign the newer one as CI does: `COPYFILE_DISABLE=1 tar --no-mac-metadata -czf updates/Zorvik-macOS-universal.app.tar.gz -C <folder> Zorvik.app`, then `npx tauri signer sign --app-version 0.9.1 updates/Zorvik-macOS-universal.app.tar.gz` (with `TAURI_SIGNING_PRIVATE_KEY` and its password set).
3. `node .github/scripts/update-manifest.mjs updates http://127.0.0.1:8000 0.9.1 <date> > updates/latest.json` and serve `updates/` with `python3 -m http.server 8000 --bind 127.0.0.1`.
4. Start the older app with `ZORVIK_UPDATE_URL=http://127.0.0.1:8000/latest.json`. After about 20 seconds it downloads the update; quit it and it installs.

Updates only install from release builds: debug builds check but never install.

## Signing
The builds are not code-signed yet. Windows SmartScreen asks once (**More info → Run anyway**). On macOS the first open is blocked; allow it in System Settings → Privacy & Security → **Open Anyway**, or run `xattr -dr com.apple.quarantine /Applications/Zorvik.app`. The macOS app is ad-hoc signed so it runs on Apple silicon.

## Website
The website lives in [`docs/pages`](pages/README.md): an Astro site with the home page and the developer docs (Starlight) under `/docs`. `pages.yml` publishes it to GitHub Pages in two parts, built in one run and published together:

| Part | Address | Built from | Shows |
|---|---|---|---|
| **Main site** | <https://libreguild.github.io/zorvik/> | the latest release's tag | the docs of the version people download |
| **Preview** | <https://libreguild.github.io/zorvik/next/> | `main` | the docs of the next version, with a banner on every page that links to the same page on the main site (or its docs home when the page is new). Search engines are asked not to list it, and it has no sitemap |

The latest release is GitHub's "latest" release (`releases/latest`): the newest published release that isn't a pre-release, the same one the download buttons and the app's update check use. A `v*` tag whose release is still a draft doesn't count. Without a release that has the website, the main site is built from `main`. Both parts' download buttons point at the latest release; the preview's home page adds that nightly builds are on the releases page.

The workflow runs:
- on pushes to `main` that change the site, its pictures or the course (the preview changes; the main site is rebuilt as it was);
- after every release (the release job starts it), so the main site shows the new release's docs, version and sizes;
- once a day, so the release details stay fresh;
- by hand: **Actions → Website → Run workflow** on `main`.

Pull requests that touch the site build their own version both ways (as the main site and as the preview) and check the links, without publishing. Page views are counted by GoatCounter on both parts, without cookies.

**Docs changes go live with the next release.** Docs describe the code on the same branch, so a docs change belongs in the pull request of the change it describes. To correct the docs of the current release, fix them on `main` too: the fix shows in the preview as soon as it merges, and on the main site with the next release.

The build reads `SITE_BASE` (the base path, `/zorvik` by default) and `SITE_CHANNEL=next` (the banner and `noindex`); the workflow also sets `SITE_RELEASE_DIST` to the built main site, so the banner only links to pages that exist there. The main site is built with the release's own sources, config and link check; releases whose config doesn't read `SITE_BASE` are built for `/zorvik` anyway. After both builds, the workflow checks the links once more across the combined site (the main site with the preview in `next/`).

## Repository automation
- **Dependabot** opens grouped weekly updates for Cargo, npm (the app and the website) and GitHub Actions ([`.github/dependabot.yml`](../.github/dependabot.yml)), plus immediate security updates.
- **CodeQL** scans the TypeScript and workflow code on every push and pull request.
- **Secret scanning** with push protection blocks commits that contain credentials.
- `main` is protected: changes arrive through pull requests with passing checks; no force pushes or deletion.

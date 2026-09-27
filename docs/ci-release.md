# CI and releases

Everything runs in one workflow: [`.github/workflows/ci.yml`](../.github/workflows/ci.yml).

## Checks (every pull request and every push)
| Job | What it does |
|---|---|
| Rust (Linux) | `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`, and that the generated TypeScript bindings are committed |
| Rust (Windows) | `cargo test` on real Windows networking, paths and trash |
| UI | TypeScript typecheck, Vitest unit tests, production build |
| E2E | Playwright drives the real UI against the real Rust API and local test servers |

A pull request can merge only when all four pass. Pull requests from first-time contributors wait for a maintainer to approve the workflow run.

## Release channels
| Channel | Made by | Where | Version |
|---|---|---|---|
| **Release** | pushing a tag `vX.Y.Z` | [Releases → Latest](https://github.com/LibreGuild/zorvik/releases/latest) | `X.Y.Z` |
| **Nightly** | every push to `main` | the `nightly` pre-release, replaced each time | the version in the code, plus "nightly N · commit" in Settings |

After the checks pass, `release-draft` creates a draft release for the run and checks that the tag matches the version in `Cargo.toml`, `app/package.json` and `tauri.conf.json`. Then three jobs build in parallel with `npm run package` (the app with the `zorvik` command line inside) and upload to the draft. Finally `publish` makes the draft public: as the tagged release with notes, or as the new nightly. If any build fails, nothing is published and the previous release stays.

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
4. CI builds and publishes "Zorvik 0.2.0". Below the tag message, the notes list the downloads and the merged pull requests, grouped by label ([`.github/release.yml`](../.github/release.yml)).

Only maintainers can push `v*` tags. Versions follow [SemVer](https://semver.org): while Zorvik is `0.x`, a minor bump may change behaviour; patch releases only fix.

## Downloads
Every download has the app **and** the `zorvik` command line.

| System | File | Notes |
|---|---|---|
| Windows 10/11 | `Zorvik-Windows-Setup-x64.exe` | Installs for the current user (no admin rights) and puts `zorvik` on PATH; the uninstaller removes it again |
| Windows 10/11 | `Zorvik-Windows-Portable-x64.zip` | No install: `zorvik-desktop.exe` (the app) and `zorvik.exe` (the command line) |
| macOS 11+ | `Zorvik-macOS-universal.dmg` | Apple silicon and Intel. Settings → AI agents → **Add zorvik to PATH** links the command line into `/usr/local/bin` |
| Ubuntu 22.04+, Debian 12+ | `Zorvik-Linux-amd64.deb` | `zorvik` goes to `/usr/bin` |
| Fedora, RHEL, openSUSE | `Zorvik-Linux-x86_64.rpm` | `zorvik` goes to `/usr/bin` |
| Any Linux (x86-64) | `Zorvik-Linux-x86_64.AppImage` | Runs anywhere; use the deb or rpm to get `zorvik` on PATH |

Linux packages are built on Ubuntu 22.04, the oldest supported system, so they run on newer ones.

## Signing
The builds are not code-signed yet. Windows SmartScreen asks once (**More info → Run anyway**). On macOS the first open is blocked; allow it in System Settings → Privacy & Security → **Open Anyway**, or run `xattr -dr com.apple.quarantine /Applications/Zorvik.app`. The macOS app is ad-hoc signed so it runs on Apple silicon.

## Repository automation
- **Dependabot** opens grouped weekly updates for Cargo, npm and GitHub Actions ([`.github/dependabot.yml`](../.github/dependabot.yml)), plus immediate security updates.
- **CodeQL** scans the TypeScript and workflow code on every push and pull request.
- **Secret scanning** with push protection blocks commits that contain credentials.
- `main` is protected: changes arrive through pull requests with passing checks; no force pushes or deletion.

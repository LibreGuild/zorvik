# Contributing to Zorvik

Thanks for helping! Bug reports, ideas, documentation and code are all welcome. Please follow the [Code of Conduct](CODE_OF_CONDUCT.md) in every interaction.

## Ways to help
- **Report a bug:** [open an issue](https://github.com/LibreGuild/zorvik/issues/new/choose) with the steps to reproduce it, what you expected and your system.
- **Suggest a feature:** start with a [discussion](https://github.com/LibreGuild/zorvik/discussions/categories/ideas) or a feature request, so the idea can be shaped before anyone writes code.
- **Ask a question:** use [Discussions → Q&A](https://github.com/LibreGuild/zorvik/discussions/categories/q-a).
- **Report a security problem:** never in a public issue. See [SECURITY.md](SECURITY.md).
- **Send a pull request:** fixes and small improvements can go straight to a PR. For anything big (a new protocol, a new panel, a change to the file format), open an issue first so we agree on the approach.

Issues labelled [`good first issue`](https://github.com/LibreGuild/zorvik/labels/good%20first%20issue) are a good place to start.

## Set up
You need:
- **Rust** stable, 1.88 or newer ([rustup](https://rustup.rs))
- **Node.js** 22 or newer
- **macOS:** Xcode command line tools (`xcode-select --install`)
- **Windows:** Visual Studio Build Tools with "Desktop development with C++", and WebView2 (already on Windows 10 and 11)
- **Linux (Debian/Ubuntu):** `sudo apt install libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev libssl-dev patchelf file`

Then:
```bash
git clone https://github.com/LibreGuild/zorvik.git
cd zorvik/app
npm ci
npm run tauri dev        # the desktop app, with hot reload of the UI
```

### The UI in a browser
The dev bridge serves the same Rust API over HTTP, so the UI runs in a normal browser with its dev tools:
```bash
cargo run -p zorvik-devbridge -- --workspace /tmp/demo-ws   # the API on 127.0.0.1:18799
cargo run -p zorvik-testkit                                 # test servers on :18787 (HTTP) and :18788 (HTTPS)
cd app && npm run dev                                       # http://localhost:14200
```
The dev bridge only listens on 127.0.0.1, refuses cross-site calls and is never shipped.

### Build installers
```bash
cd app
npm run package                                     # the app with the zorvik CLI inside, for this system
npm run package -- --bundles app                    # macOS: just Zorvik.app
cargo build --release -p zorvik-cli                 # the command line alone
```

## Before you open a pull request
Run the same checks as CI:
```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test                     # also regenerates app/src/bindings (commit them)
cd app
npm run typecheck
npm test
npm run e2e                    # Playwright against the real backend (see docs/testing.md)
```

## Guidelines
- **Read [docs/architecture.md](docs/architecture.md) first.** It explains where things belong: networking in `crates/engine`, files in `crates/workspace`, the RPC surface in `crates/api`, the UI in `app/src`.
- **Match the code around you:** naming, comments, error messages and structure. Keep changes focused; unrelated refactors belong in their own PR.
- **Add tests** for behaviour you add or fix: Rust tests next to the code or in the crate's `tests/`, Vitest for UI logic, Playwright for user flows.
- **Write for users.** Messages, labels and docs use plain words and say what to do next.
- **Keep the docs true.** When a feature changes, update its topic in the in-app docs (`app/src/components/docs/content.ts`) and the README; when the design changes, update `docs/`.
- **Generated files:** `app/src/bindings/*.ts` come from Rust (`cargo test`); don't edit them by hand.
- **Security matters here.** Workspace files come from Git and are untrusted; requests may carry secrets. Keep secrets out of files, logs and error messages.

## Pull requests
- Give the PR a clear title; it becomes the line in the release notes ("Add SOCKS5 proxy support", "Fix timing of redirected requests").
- Fill in the template: what changed, why, and how you tested it. Add screenshots for UI changes.
- CI must pass. A maintainer reviews every PR; PRs are squash-merged into `main`.
- Draft PRs are welcome for early feedback.

## Releases
Maintainers cut releases by tagging `main` (see [docs/ci-release.md](docs/ci-release.md)). Every merge to `main` also updates the nightly build.

## License
Zorvik is licensed under either of [Apache License 2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT), at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in Zorvik by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.

# Changelog

Notable changes in each release. Every release on [GitHub Releases](https://github.com/LibreGuild/zorvik/releases) also lists the merged pull requests.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- **Automatic updates** from GitHub Releases. Zorvik checks a little after it starts and every six hours, downloads a new version in the background, and installs it when you quit, or when you choose **Restart now** (it first tells you what a restart would stop). Settings → Updates: automatic, notify only, or off; stable or nightly channel. The check only reads this project's release list on GitHub and sends nothing about you. The Windows installer, the macOS app and the AppImage update themselves; the portable zip, `.deb` and `.rpm` tell you when a new version is out. If an update can't install itself, Zorvik links to its release page instead of showing an error.
- **More auth types**, each with its own form and imported from Postman: OAuth 1.0 (HMAC-SHA1/256/512, RSA-SHA1/256/512, PLAINTEXT), JWT that Zorvik signs from your claims (HS, RS, PS and ES algorithms), AWS Signature v4 (headers or a presigned URL), Hawk, Akamai EdgeGrid and Atlassian ASAP, all signed fresh for every send; Digest (MD5 and SHA-256, qop auth and auth-int) and NTLMv2, which answer the server's challenge; and the OAuth 2.0 implicit grant. Load tests sign every request separately.
- **Filter responses** with JSONPath, jq or XPath (the funnel icon above the body). JSONPath and jq run on the whole body; AI agents can pass a `filter` to `send_request`.
- **Save responses as examples:** kept in the request (its Examples tab), imported from Postman's saved responses, and answered by mocks built from the request (examples saved with query parameters answer only those).
- **170 dynamic variables**: every one of Postman's (`{{$randomFirstName}}`, `{{$randomCity}}`, …) plus modern IDs (`$uuidv7`, `$ulid`, `$nanoid`), valid test card numbers, IBANs, ISBNs and EANs, dates relative to now, and arguments: `{{$randomInt(1, 100)}}`, `{{$timestamp(+1h)}}`, `{{$randomFrom(a, b, c)}}`. They work in requests, scripts and mock servers; typing `{{$` shows each with an example.
- **Scripts:** `pm.sendRequest` (with a callback or `await`, which now works at the top level), `pm.cookies`, `pm.cookies.jar()` and `pm.response.cookies`, timers (`setTimeout`, `setInterval`), `pm.visualizer` (a Visualize tab next to the response), `pm.execution.skipRequest()`, and `pm.response.to.have.jsonSchema`.
- **Training Bootcamp:** lessons an update adds show as **New**, and everything you finished stays finished.
- **Script libraries, as in Postman:** `require('lodash')`, `crypto-js` (and the `CryptoJS` global), `moment`, `ajv` (JSON Schema draft-07, 2019-09 and 2020-12), `uuid`, `tv4`, `chai`, `csv-parse/lib/sync`, `xml2js` (and `xml2Json`), `cheerio`, `handlebars`, and Node's `buffer`, `events`, `path`, `querystring`, `url` and `util`. They are built into Zorvik and work offline; `pm.require('npm:name@version')` gives the built-in copy. Scripts also get `crypto.getRandomValues` and `crypto.randomUUID`.
- **GraphQL subscriptions:** a `subscription` operation streams its results live, over WebSocket (`graphql-transport-ws`, or `subscriptions-transport-ws` for older servers) or Server-Sent Events (graphql-sse), with its own URL and connection params when needed. Collection runs, `zorvik run` and AI agents read subscriptions like event streams, and their tests get the results.
- **Socket.IO client:** connect to Socket.IO 3 and 4 servers over WebSocket or long-polling (it falls back by itself), join a namespace with an auth payload, and emit events with JSON or binary arguments, asking for acknowledgements.
- **Socket.IO servers** that socket.io-client apps connect to (long-polling, WebSocket and the upgrade between them): echo, rules with acknowledgements, replies and broadcasts, a greeting event, and emits from the traffic panel.
- **Copy as code in 16 languages:** JavaScript (fetch, axios), Python (requests, HTTPX), Go, Java, Kotlin, Swift, C#, PHP, Ruby, Rust, Dart, C (libcurl), PowerShell, HTTPie and Wget, besides cURL. The dialog lists them with search and highlighting, and remembers your choice. Comments at the top say what the code can't do that Zorvik does, such as answering a Digest challenge or signing each request.
- **The command line on its own** for CI machines and servers: `zorvik-cli-windows-x64.zip`, `zorvik-cli-macos-universal.tar.gz` and `zorvik-cli-linux-x86_64.tar.gz` in every release.
- `PROJECT.md` holds the project's rules for contributors and AI coding agents; `CLAUDE.md`, `AGENTS.md` and `GEMINI.md` point to it.

### Changed
- The website shows the docs of the latest release; the docs for the next version (what nightly builds have) are at `/zorvik/next/`.
- `{{$isoTimestamp}}` has millisecond precision, like Postman, and `{{$randomEmail}}` gives name-based addresses at `example.com`, `example.net` and `example.org`.
- Errors thrown in promise callbacks and timers now fail the script instead of being lost.
- The website and README lead with what coding agents can do through MCP, and that every feature is free (no paid plans, no account, no cloud sync). The website no longer shows a download count.

### Fixed
- Load test HTML reports: pointing at a chart (or tapping it, or focusing it and using the arrow keys) shows a guide line and a tooltip with the time and every value, like in the app. The report is still a single file with no external resources.

## [0.1.2] - 2026-09-28

0.1.1 was never published on its own: its changes ship here.

### Added
- **Website and developer docs** at <https://libreguild.github.io/zorvik/>: every feature explained, with the complete scripting (`pm`) and command-line references and the workspace file format. Download buttons pick your system and always point at the newest release.
- **Training Bootcamp:** a course inside Zorvik, from networking basics to load testing. 16 units of short lessons with diagrams, hands-on labs in the real workbench (practice servers start on your computer; a Lab Guide checks each step, with hints and "Do it for me"), quick checks, XP, levels, streaks, badges and a graduation certificate. Open it from **Training Bootcamp** at the top of the workspace menu or on the welcome screen; the Training Bootcamp workspace is always there and can be reset.
- AI agents can create and change mock servers (`read_server`, `save_server`), build a mock from a folder or an OpenAPI document (`create_mock`), start a server on any free port, and read what it received (`get_server_traffic`).
- AI agents can read Server-Sent Events streams with `send_request`, read several requests at once, set query parameters (also switched-off ones, with descriptions) and path parameters, read current variables (`get_variables`) and history (`read_history`), export a request as code (`export_request`), and add a small file to the workspace (`write_file`).
- Copy a request as Kotlin (OkHttp), Swift (URLSession), JavaScript (fetch) or Python (requests) code, next to cURL.
- Collection runs: "Repeat until" sends a request again until a condition holds (polling), and Server-Sent Events requests run, with `pm.response.events` in scripts.
- OpenAPI: imported requests are checked against the spec on every send and run (a "Matches the API spec" test); "Update from API spec" brings in a new version with a preview, keeping your edits and marking operations that were removed.
- OpenAPI import: realistic example values, path parameters become variables (`{session_id}` → `{{sessionId}}`), and a document without a full server URL asks for the base URL.
- A port that's already in use names the program holding it.
- Load tests: a data file gives each virtual user its own row; captures (JSON path, header or regex) pass values from a response to the same user's next requests; timing shows time to first byte, transfer, connect and `Server-Timing`; Compare shows a run next to an earlier one.

### Changed
- Nightly builds come once a day, when `main` has changed, instead of after every merge.
- Tools that save requests, servers and load tests refuse unknown or misspelled fields, with a suggestion, instead of ignoring them. Their input schemas document every field, unit and placeholder.
- Load-test threshold units are documented (`errorRate` is a percent, 0-100); `waitSeconds: 0` returns the run id at once.
- Collection runs say why a request was skipped.

### Fixed
- The app's Runner tab now sends event-stream (SSE) requests too, like `zorvik run` and agents do.
- Load tests: an arrival-rate run could drop its last request on Windows (a timer woke too late and the end of the run won).
- Long menus now fit the window and scroll.
- `save_environment` reported `active: false` for an environment that was active.

## [0.1.0] - 2026-09-27
The first public release.

### Added
- HTTP/1.1, HTTP/2 and HTTP/3 requests with a timing waterfall, TLS details, redirects, cookies and the headers really sent.
- GraphQL with schema autocomplete and a schema explorer; gRPC with server reflection or `.proto` files and every streaming mode.
- WebSocket, Server-Sent Events, TCP, UDP, MQTT and DNS clients.
- Environments, variables and secrets that stay on your computer; Basic, Bearer, API key and OAuth 2.0 auth; proxies and client certificates.
- Postman-compatible scripts and tests, and a collection runner with CSV/JSON data and JSON/JUnit reports.
- Load testing with virtual users or arrival rate, stages, thresholds, a live dashboard and HTML/JSON reports.
- Mock APIs (from scratch, a folder, OpenAPI or a response) and WebSocket, SSE, TCP, UDP and DNS servers, plus a TCP relay.
- Network tools: TLS inspector, DNS lookup, port check, ping, network interfaces, HTTP/3 check and encoders.
- Import from Postman, OpenAPI/Swagger and cURL; export to cURL.
- The `zorvik` command line (`run`, `load`, `serve`, `mcp`) inside every install.
- AI agents control Zorvik over MCP, with confirmations for risky actions.
- In-app docs, light and dark themes, zoom and font settings.
- Installers for Windows, macOS (universal) and Linux (deb, rpm, AppImage).

[Unreleased]: https://github.com/LibreGuild/zorvik/compare/v0.1.2...HEAD
[0.1.2]: https://github.com/LibreGuild/zorvik/compare/v0.1.0...v0.1.2
[0.1.0]: https://github.com/LibreGuild/zorvik/releases/tag/v0.1.0

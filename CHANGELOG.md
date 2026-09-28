# Changelog

Notable changes in each release. Every release on [GitHub Releases](https://github.com/LibreGuild/zorvik/releases) also lists the merged pull requests.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

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

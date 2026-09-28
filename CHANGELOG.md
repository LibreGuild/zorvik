# Changelog

Notable changes in each release. Every release on [GitHub Releases](https://github.com/LibreGuild/zorvik/releases) also lists the merged pull requests.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.2.0] - 2026-09-29

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
- **MCP testing: call, test and mock the tools AI agents use.** A new request kind, **MCP call**, connects to MCP servers over Streamable HTTP, the older HTTP+SSE transport (picked by itself for servers that need it), or stdio: programs Zorvik starts, such as `npx -y @modelcontextprotocol/server-everything`. Connect to see the server's tools with their input schemas, resources, resource templates, prompts and instructions; pick one and its arguments start from the schema; the answer shows as an AI app gets it (text, images, audio, structured content, failed calls and JSON-RPC errors), and the Messages tab shows every JSON-RPC message with a program's own log. MCP calls are requests like any other: tests on the answer, history, collection runs and `zorvik run`. Headers, auth (OAuth 2.0 included) and secret variables in a program's environment work too.
- **MCP servers:** build one from settings (tools with input and output schemas and templated answers, failed calls and delays, resources and templates, prompts, instructions) for AI apps and MCP clients, over Streamable HTTP, HTTP+SSE, or stdio with `zorvik serve <workspace> <server> --stdio`. Every call shows in its traffic panel, and edits reach connected clients at once (`list_changed`).
- **Programs from workspaces run only once you allow them:** the first time an MCP request starts a program, Zorvik shows the exact command, folder and environment and asks; it asks again when any of them changes. `zorvik run` starts programs only with `--allow-programs`.
- AI agents can send MCP calls, list what an MCP server offers (`mcp_catalog`, the 38th tool) and build MCP servers (`save_server` with kind `mcp`); they're asked before every program they start.
- **Training Bootcamp: MCP: tools for AI agents**, a new unit of five lessons: why MCP exists, calling an MCP server, testing one, building one, and using MCP safely, with three labs.
- **Socket.IO client:** connect to Socket.IO 3 and 4 servers over WebSocket or long-polling (it falls back by itself), join a namespace with an auth payload, and emit events with JSON or binary arguments, asking for acknowledgements.
- **Socket.IO servers** that socket.io-client apps connect to (long-polling, WebSocket and the upgrade between them): echo, rules with acknowledgements, replies and broadcasts, a greeting event, and emits from the traffic panel.
- **Copy as code in 16 languages:** JavaScript (fetch, axios), Python (requests, HTTPX), Go, Java, Kotlin, Swift, C#, PHP, Ruby, Rust, Dart, C (libcurl), PowerShell, HTTPie and Wget, besides cURL. The dialog lists them with search and highlighting, and remembers your choice. Comments at the top say what the code can't do that Zorvik does, such as answering a Digest challenge or signing each request.
- **The command line on its own** for CI machines and servers: `zorvik-cli-windows-x64.zip`, `zorvik-cli-macos-universal.tar.gz` and `zorvik-cli-linux-x86_64.tar.gz` in every release.
- `PROJECT.md` holds the project's rules for contributors and AI coding agents; `CLAUDE.md`, `AGENTS.md` and `GEMINI.md` point to it.

### Changed
- Saving a running server applies the change to it at once (a new address, port, TLS setting or kind still needs a restart).
- The website shows the docs of the latest release; the docs for the next version (what nightly builds have) are at `/zorvik/next/`.
- `{{$isoTimestamp}}` has millisecond precision, like Postman, and `{{$randomEmail}}` gives name-based addresses at `example.com`, `example.net` and `example.org`.
- Errors thrown in promise callbacks and timers now fail the script instead of being lost.
- The website and README lead with what coding agents can do through MCP, and that every feature is free (no paid plans, no account, no cloud sync). The website no longer shows a download count.

### Fixed
- **Checking for updates** no longer fails with "Could not reach GitHub" when one of GitHub's download servers doesn't answer from your network: Zorvik tries the next one after a few seconds. A slow update download is no longer cut off after 30 seconds, and the log says why a check failed.
- Load test HTML reports: pointing at a chart (or tapping it, or focusing it and using the arrow keys) shows a guide line and a tooltip with the time and every value, like in the app. The report is still a single file with no external resources.
- Load tests: requests dropped at the in-flight limit (request-rate model) now count as failed in the error rate, so a run that couldn't start half its requests no longer passes an `errorRate` threshold and `zorvik load` exits with 1.
- Load tests: throughput (requests per second) is measured over the time requests were being started, so a request still in flight at the end no longer lowers it (and fails an `rps` threshold) while the run waits for it.
- Load tests: a captured value can no longer move requests to another port, scheme or virtual host (`Host` header) of a host the run was started for, and a captured value over 64 KB counts as a capture miss instead of being kept. Responses kept for captures no longer set aside memory for the size a server claims.
- **AI agents** can no longer reach an unapproved host through a WebSocket (a GraphQL subscription), TCP, UDP or MQTT connection, or through the name of a DNS query (resolvers pass it on), and exported code and server traffic hide credentials (a Basic header, cookies, tokens) like secret values. Starting a relay or a forwarding mock shows where it forwards to.
- Digest and NTLM credentials are no longer sent to another site a redirect leads to; Digest works with servers that close the connection after their challenge; the body of a challenge is read only up to 1 MB; HTTP/3 says that Digest and NTLM need HTTP/1.1 or HTTP/2 instead of ignoring them.
- A proxy password no longer appears in the error for a proxy URL that can't be used, and `settings.json` and cookie jars are readable by you only.
- WebSocket and MQTT sessions end when the other side stops reading for a minute, instead of hanging; Socket.IO limits the attachments of one event (64, and 64 MB) and a server's ping settings.
- Servers keep at most 1,024 connections open each, and the app and `zorvik` may open more files than macOS allows a Finder-started app by default.
- Socket.IO servers no longer answer browsers' CORS preflights when CORS is off.
- The file watcher doesn't follow symbolic links in a workspace (one pointing at `/` made it walk the whole disk).
- Two requests (environments, servers, load tests) created with the same name at the same moment, say by you and an AI agent, no longer overwrite each other.
- Scripts: a timer set for after the time limit no longer runs early at the limit; a rejected promise whose reason itself rejects no longer loses the run's results; `pm.sendRequest` responses reach scripts cut at 16 MB.
- A crafted OAuth 1.0 RSA key can no longer crash the app or keep it busy.
- Response filters: jq refuses responses nested more than 128 levels deep and stops expressions that recurse or loop without end, instead of crashing the app. OpenAPI contract checks stop in time on schemas that refer to themselves over and over, and resolve `$ref`s with escaped characters (`#/paths/~1pets`).
- cURL: binary bodies are exported byte for byte (through base64 in bash, cmd and PowerShell); `--digest` and `--ntlm` import as Digest and NTLM auth; `--data-urlencode 'name=content'` keeps an `@` in the name.
- An update that fails to install on Windows leaves the connection for AI agents running.
- The app: servers an AI agent starts show as running (with their traffic); ⌘/Ctrl+K no longer replaces a dialog with unsaved changes; the Settings dialog asks before discarding changes; Stop all keeps a server listed when it couldn't be stopped; Save as example no longer saves edits typed while it works; closing a new GraphQL tab doesn't ask about unsaved changes; the URL bar's Cancel also ends a gRPC stream; tabs can be reached and switched with the keyboard, and request settings have labels screen readers read.

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

[Unreleased]: https://github.com/LibreGuild/zorvik/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/LibreGuild/zorvik/compare/v0.1.2...v0.2.0
[0.1.2]: https://github.com/LibreGuild/zorvik/compare/v0.1.0...v0.1.2
[0.1.0]: https://github.com/LibreGuild/zorvik/releases/tag/v0.1.0

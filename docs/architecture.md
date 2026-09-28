# Architecture

Zorvik is a Rust workspace with a React UI in a Tauri 2 shell. Almost everything lives in Rust: networking, files, scripts, load tests, servers and the AI agent tools. The UI renders the state and calls one RPC entry point.

```
app/                 React UI (Vite, TypeScript, Tailwind, CodeMirror)   ─┐
app/src-tauri/       Tauri shell: one `rpc` command, events, menus        │  rpc(method, params)
crates/devbridge/    The same API over HTTP, for a browser (dev, E2E)    ─┘
      │
crates/api/          RPC dispatcher and app state: in-flight requests, stream sessions, cookie
      │              jars, history, secrets, file watcher, runner, load runs, servers, agents
crates/workspace/    Workspace on disk, variables, auth and OAuth 2.0, request resolution,
      │              history (SQLite), settings, secret store
crates/formats/      Data model (YAML files) and import/export: cURL, Postman, OpenAPI, code in 16 languages
crates/script/       Sandboxed JavaScript (QuickJS) for pre-request and post-response scripts
crates/load/         Load generator: own runtime, pooled client, HdrHistogram metrics, reports
crates/servers/      Servers the user runs: mock HTTP, MCP, WebSocket, Socket.IO, SSE, TCP, UDP, DNS, TCP relay
crates/engine/       Networking only: HTTP/1.1, HTTP/2, HTTP/3, TLS, proxy, WebSocket, SSE,
                     TCP/UDP/DNS/MQTT/gRPC/Socket.IO/MCP clients, GraphQL subscriptions,
                     cookies, decoding, timing, network tools
crates/cli/          `zorvik`: run, load, serve and mcp (the command line inside every install)
crates/mcp/          Zorvik's own MCP server for AI agents: the stdio bridge (`zorvik mcp`) and the app's listener
crates/testkit/      Local test servers used by the tests (HTTP, TLS, proxy, OAuth, GraphQL, gRPC),
                     and the practice servers of Training Bootcamp labs
crates/academy/      Training Bootcamp: the course (lessons, labs, quizzes as files), patterns, progress rules
```

## Design rules
- **The engine knows nothing about files or the UI.** Callers hand it fully resolved requests.
- **One entry point.** `Api::call(method, params)` in `crates/api` serves the desktop app (Tauri IPC) and the dev bridge (HTTP), so browser E2E tests run the production code path. The methods are the arms of the `match` in `Api::dispatch`. The CLI reuses the same modules (runner, load tests, servers) directly.
- **Types are generated.** Rust structs with `#[derive(TS)]` are written to `app/src/bindings/*.ts` by `cargo test`. Never edit them by hand; CI fails when they are out of date.
- **Events flow Rust → UI** through an `EventSink` (the `zv:event` event in Tauri): stream messages, server traffic, run progress, file changes, agent activity. Sinks batch them every 25 ms so floods don't freeze the webview.
- **Big data stays in Rust.** The UI gets a display copy of a response (text up to 10 MB, images up to 8 MB); "Save to file" writes the full body from Rust.
- **Secrets never go into workspace files.** Secret variable values, cookies and OAuth tokens live in the app data folder.
- **Workspace files are untrusted input.** They come from Git: symlinks are not followed, sizes and depths are capped, IDs are validated, body files must be inside the workspace unless the user allows others.

## Request flow (HTTP)
1. The UI calls `http.send {requestId, request, path}`.
2. Pre-request scripts run: workspace, then folders (outer to inner), then the request. They may change the request and set variables.
3. The request is resolved: variables, inherited headers and auth from folders and the workspace, body encoding. Signing auth (AWS SigV4, OAuth 1.0a, JWT, Hawk, Akamai EdgeGrid, ASAP) signs the final method, URL, headers and body here, fresh for every send (`crates/workspace/src/signing.rs`, algorithms in `crates/workspace/src/auth/`).
4. OAuth 2.0: a cached token is used, refreshed or fetched.
5. `engine::Client::send`: DNS, TCP (Happy Eyeballs), proxy tunnel, TLS, HTTP/1.1 or HTTP/2 (or QUIC for HTTP/3), redirects, decoding. Digest and NTLM are a `ChallengeAuth` in the request options: when the server answers 401 with a challenge, the engine sends the request again with the answer on the same connection (NTLM forces HTTP/1.1).
6. Post-response scripts run the tests and fill the console. A request imported from an OpenAPI document the workspace keeps also gets a "Matches the API spec" test (see below).
7. A history row is written. The response goes back with timing, TLS details, cookies, the headers really sent and the script results.

## Live sessions
WebSocket and SSE requests have their own session APIs (`ws.*`, `sse.*`). TCP, UDP, MQTT, Socket.IO and GraphQL subscriptions share one (`socket.*` in `crates/api/src/sockets.rs`): `socket.connect` resolves the request, opens the engine's session for its kind, and forwards its `SocketEvent`s to the UI; `socket.send` passes `SocketOutgoing` messages (text, binary, MQTT publishes, Socket.IO emits).

- **GraphQL subscriptions** are HTTP requests with a GraphQL body whose operation is a `subscription` (`crates/formats/src/graphql.rs` reads the operation type without a full parser; the UI does the same with graphql-js's lexer). `resolve` adds the subscription's URL and connection params. `crates/engine/src/graphql.rs` speaks `graphql-transport-ws` and `subscriptions-transport-ws` over the engine's WebSocket, and graphql-sse over a streaming POST. Collection runs, the CLI and agents read a subscription to an end like an SSE request (`crates/api/src/sse_read.rs`): results become `next` events, and the response body is the results as a JSON array.
- **Socket.IO** (`crates/engine/src/socketio/`): `packet.rs` encodes Engine.IO 4 and Socket.IO packets (namespaces, acknowledgement ids, binary attachments as placeholders) for both the client and the server. The client runs the same session over WebSocket or long-polling (a GET loop and POSTs through the engine's HTTP client), falling back to polling when the server refuses the upgrade.

## MCP client
MCP requests (kind `mcp`) talk to MCP servers. `crates/engine/src/mcp/` is the client: `mod.rs` runs a session over a `Link` (outbound and inbound channels) that each transport provides, routes answers to waiting requests by id, answers the server's own requests (`ping`, `roots/list`; sampling and elicitation get `-32601`), and reports every message as an `McpEvent`. `http.rs` is Streamable HTTP (a POST per message, the answer as JSON or an event stream, the session id and protocol version headers, a `GET` stream for the server's messages once initialized, `DELETE` on close) and the older HTTP+SSE transport, which `Auto` falls back to when the first POST gets 400, 404 or 405; both go through the engine's HTTP client, so proxies, TLS settings, cookies and the agent host guard apply. `stdio.rs` starts a program (split like a simple shell line, never through a shell), reads one JSON message per line with a size limit, keeps its standard error as the log, and on close ends its input, waits, then stops its whole process group (`taskkill /T` on Windows).

`crates/api/src/mcp.rs` has the `mcp.*` session RPC (`connect`, `catalog`, `close`, `program`, `trust`) keyed by the tab id. A call is sent like any request (`http.send` → `scripting::send`): on the tab's session when it is open to the same address and transport, otherwise in one go (connect, call, disconnect); the answer becomes an HTTP-like response (the JSON-RPC result or error as the body, status 200 or 500) so scripts, tests, history, runs and agents need nothing new. Workspaces are untrusted, so a program starts only when its fingerprint (SHA-256 of the command, folder and sorted environment) is in `trusted-programs.json` for that workspace; `zorvik run` needs `--allow-programs`, and agents are asked every time (`Programs::Approved`). In the app, a program also gets the login shell's `PATH` (GUI apps start with a short one).

## Workspace format
A workspace is a plain folder of YAML files, meant to be committed to Git.

```
my-api/
  zorvik.yaml              # name, id, workspace variables, default auth and headers, scripts
  environments/
    Dev.yaml
  requests/
    Get status.yaml
    Users/
      _folder.yaml         # optional: order, auth, headers, scripts, docs of the folder
      List users.yaml
  servers/
    Payments mock.yaml     # a server: kind, address, port, TLS and its settings
  loadtests/
    Checkout smoke.yaml    # a load test: requests, model, stages, thresholds
  specs/
    Payments API.yaml      # OpenAPI documents kept by imports (see "OpenAPI")
```

A request file:
```yaml
name: Create user
seq: 2                                  # order in the sidebar
method: POST
url: "{{baseUrl}}/users?notify=true"
headers:
  - key: X-Request-ID
    value: "{{$uuid}}"
body:
  type: json                            # none | json | text | xml | formUrlencoded | multipart | binary | graphql
  text: |
    { "name": "{{name}}" }
auth:
  type: bearer                          # inherit (default) | none | basic | bearer | apiKey | oauth2
  token: "{{token}}"
scripts:
  postResponse: |
    pm.test("Status is 201", () => pm.response.to.have.status(201));
```
Other request kinds add `kind:` (`websocket`, `socketio`, `sse`, `grpc`, `tcp`, `udp`, `dns`, `mqtt`). Default values are left out so files stay small and diffs stay clean.

An environment file marks secrets; their real values stay on each computer:
```yaml
name: Dev
variables:
  - { key: baseUrl, value: "http://localhost:3000" }
  - { key: token, value: "", secret: true }
```

Rules:
- File names come from item names, made safe for Windows and macOS. The display name is inside the file.
- Broken YAML shows up in the sidebar with a warning instead of failing the workspace.
- `zorvik.yaml` carries `version: 1`; a newer format is refused with a clear message.
- Secret values, cookies and tokens are keyed by the workspace `id` **and** its folder, so a copied `id` doesn't unlock another workspace's secrets.

Per-request settings (`settings:`) include `repeat: {condition, intervalMs, timeoutMs}` (collection runs send again until the JavaScript condition holds) and, for SSE requests, `stream: {event, maxEvents, timeoutMs}` (when a run or an agent stops reading). Enabled query parameters live in `url`; switched-off ones in `disabledParams`, and descriptions of enabled ones in `paramDescriptions`.

## OpenAPI
- **Import** (`crates/formats/src/openapi.rs`) turns each operation into a request: realistic example bodies from the schema (examples, enums, formats, then property names), path parameters as variables (`{session_id}` → `{{sessionId}}`, generic names get the resource: `/pets/{id}` → `{{petId}}`) with their example values in the new environment. A document without a full server URL is refused with `needsBaseUrl` until the caller gives one.
- **The document is kept** in `specs/`; the imported folder links it (`openapi: {spec, source, validate}` in `_folder.yaml`) and each request names its operation (`openapi: {operation: "GET /pets/{petId}"}`).
- **Response checks** (`crates/formats/src/spec_check.rs`, `crates/api/src/specs.rs`): after each send, the status must be documented (exact, `2XX` or `default`) and a JSON body must match the documented schema (types, required fields, enums, `$ref`, `nullable`, `allOf`/`anyOf`/`oneOf`, lengths, ranges; formats and patterns are not checked). The result is a test, so it counts in runs, the CLI and JUnit reports. Parsed documents are cached until the file changes.
- **Update from a new version** (`crates/api/src/spec_update.rs`, `import.updatePreview` / `import.update`): new operations are added; for changed ones each field (method, URL, headers, body, parameters, auth, docs) takes the new version only where the saved request still has the old document's value, so the user's edits win; scripts, settings and names are never touched. Operations no longer in the document are kept and marked `removed` (crossed out in the sidebar). New path variables are added to the folder's environment.

## Variables
Precedence, highest first: CLI `--var`, `pm.variables` (this send or run), the runner's data row, the active environment, workspace variables, globals, then dynamic values. Values that scripts set on an environment, the workspace or globals are kept on this computer (`local-values.json`), never in the files, and win over the file value in their scope. Variables can reference variables (depth 10; cycles are reported).

Dynamic variables (`{{$uuid}}`, `{{$randomInt(1, 5)}}`) are one catalog in `crates/workspace/src/dynamic/`: every Postman name plus more, each a row with its group, description, example, argument form and generator. Requests, scripts (through the host) and mock server templates use it; the UI gets it in `app.info` for autocomplete, and a test writes the website's reference table from it. An unknown name or bad arguments leave the text as written and report it as undefined.

## Networking
- **A fresh connection per request**, so DNS, connect and TLS are always measured. Timing phases: DNS, TCP connect (with the proxy tunnel), TLS, time to first byte, download.
- **DNS** uses the OS resolver (hosts file and VPN split DNS work); IPv6 and IPv4 race with Happy Eyeballs.
- **TLS** trusts the OS certificate store (Windows store, macOS keychain), so corporate inspection CAs work. An extra CA and client certificates (mTLS) are set globally in Settings; "don't verify" is available globally or per request.
- **Proxy**: system (environment variables, then the OS settings), none or manual, with Basic auth and a bypass list; localhost is always direct. PAC files, SOCKS and NTLM/Kerberos proxy auth are not supported.
- **Redirects** follow browser rules; `Authorization` and `Cookie` are dropped when the origin changes.
- **Limits**: response bodies are capped (100 MB by default) and decompression is bounded; WebSocket and gRPC messages up to 64 MB; stream lines and framed socket messages up to 16 MB.
- **HTTP/3** is opt-in per request or globally: `https://` only and never through a proxy.
- **gRPC**: `grpc://` (plaintext HTTP/2) and `grpcs://` (TLS), with server reflection or `.proto` files and every call type.
- **Other clients**: WebSocket, SSE, TCP (raw, line or length-prefixed framing, optional TLS), UDP, MQTT 3.1.1 and 5, DNS (UDP, TCP, DoT, DoH).

## Scripts
`crates/script` runs JavaScript in QuickJS. Each script gets a fresh runtime with no file or process access, a 64 MB memory limit and a time limit (Settings, 5 s by default). Input and results cross as JSON. The app's side is a `Host` (`crates/script/src/host.rs`, implemented in `crates/api/src/scripting.rs`) the prelude calls synchronously: `pm.sendRequest` sends through the engine with the app's settings and the AI agent's host guard (at most 100 per script, within the time left), `pm.cookies.jar()` reads and changes the jar for the request's own site only, and `{{$…}}` in `replaceIn` comes from the same dynamic-variable catalog as requests. After the script, the runner drains promise jobs, then timers in time order (sleeping until they're due), within the time limit. Scripts that use `await` run as an async function; exceptions in promise jobs, rejected promises nobody handled and a rejected top-level promise fail the script (a rejection tracker records them). `pm.visualizer.set` renders Handlebars in the sandbox; the UI shows the HTML in a script-less sandboxed frame.

The API is a Postman-compatible subset (`crates/script/src/prelude.js`), so imported collections run unchanged: `pm.variables`, `pm.environment`, `pm.collectionVariables`, `pm.globals`, `pm.iterationData`, `pm.request`, `pm.response`, `pm.test`, `pm.expect` (a chai subset), `pm.execution.setNextRequest` and `skipRequest`, `pm.sendRequest`, `pm.cookies`, `pm.visualizer`, timers, `console`, plus the legacy `tests[...]` and `postman.*` forms. `pm.vault` and `pm.execution.runRequest` are not supported and say so.

**Libraries**: `require` (and `pm.require('npm:name@version')`, version ignored) gives the libraries of Postman's sandbox (lodash, crypto-js, moment, ajv, uuid, tv4, chai, csv-parse, xml2js, cheerio) and browser versions of Node's `buffer`, `events`, `path`, `querystring`, `url` and `util`; nothing else. `crates/script/vendor` bundles them with esbuild into one CommonJS file each (`dist/`, committed with `THIRD_PARTY_LICENSES.md`; its README says how to update them), so building Zorvik needs no npm. `src/libs.rs` includes the files, compiles each to bytecode once per process on first use (as a sloppy-mode script, not a module), and the prelude's `require` runs it in a Node-style module wrapper and keeps it for the rest of the run: a few milliseconds per library and run. Libraries get `crypto.getRandomValues` (from Rust's `rand`) and a private `process`/`Buffer`, never the host's. QuickJS is compiled with optimizations in debug builds too (`[profile.dev.package.rquickjs-sys]`): unoptimized, its stack frames are ten times larger and libraries such as ajv overflow the script stack.

## Responses
- **Bodies stay in Rust**: the last 30 response bodies (512 MB together) are kept by response id for **Save to file**, filters and examples.
- **Filters**: `response.filter` runs JSONPath (RFC 9535, `serde_json_path`) or jq (`jaq`) over the whole stored body (`crates/formats/src/filter.rs`); agents get the same through `send_request`'s `filter`. XPath runs in the web view (`app/src/components/response/filterModel.ts`) with the browser's XPath engine, on XML or HTML.
- **Examples**: saved responses are part of the request file (`examples`, text bodies up to 1 MB). `response.text` hands the UI the whole body to keep; mocks built from requests answer with them (`crates/formats/src/mock.rs`); Postman responses import as examples.

## Collection runner
`crates/api/src/runner.rs` is shared by the app's Runner tab and `zorvik run`. It reads the requests of a folder once, then sends them in order for each iteration through the same scripted send as a single request. CSV or JSON data files give one iteration per row. A request fails when it can't be sent, a script throws or a test fails; without tests, an HTTP status of 400 or more fails it. Reports are JSON or JUnit XML, and memory stays bounded on long runs.
- **Repeat until**: a request with `settings.repeat` is sent again, after `intervalMs`, until its condition holds (run as a hidden test after the post-response scripts; empty = until the request's tests pass), and fails after `timeoutMs`. The result shows the number of sends.
- **Event streams**: SSE requests are read until `settings.stream` says to stop (`crates/api/src/sse_read.rs`, shared with agents); scripts get `pm.response.events` and the events as text.
- WebSocket, TCP, UDP, MQTT, gRPC and DNS requests are skipped, with the reason in the result.

## Load testing
`crates/load` runs a plan on its own Tokio runtime so the app stays responsive.
- **Models**: virtual users (closed: send, wait, think, repeat) or arrival rate (open: requests start on schedule however slow the server is). In the open model latency is measured from the scheduled start, so a slow server can't hide its queueing.
- **Stages** ramp users or requests per second linearly; thresholds (`p50` … `p99.9`, `avg`, `max`, `errorRate`, `rps`) show pass or fail live and decide the result at the end (a failing threshold does not stop the run).
- **Client**: pooled HTTP/1.1 keep-alive and HTTP/2 multiplexing on the engine's connect path (DNS, proxy tunnel, OS-trusted TLS). Requests are resolved once; ones that use dynamic variables, data file columns or captured values, or signing auth (a fresh timestamp and nonce each time), are rendered per request (a render with every user variable set to a probe value tells which). Digest and NTLM can't be load tested: they answer a challenge on the connection that got it.
- **Per-user data**: `dataFile` (CSV or JSON, the runner's parser and file rules): virtual user N takes row N % rows for its life; in the arrival-rate model each request takes the next row. Precedence: `--var`, captured values, the row, then the environment.
- **Captures** (`json` path like `$.items[0].id`, `header`, or `regex` first group) save a value from a target's response for the same user's later requests; with captures each user sends the targets in their weighted order. A capture that finds nothing (or a value over 64 KB) keeps the old value and counts as a miss. Arrival rate: every request is its own iteration, so captured values are only checked, not reused.
- **Metrics**: an HdrHistogram per target and in total, 1-second buckets for the charts, snapshots to the UI four times a second. Timing phases too: connect (new connections only), time to first byte (server plus one round trip), transfer, and server-reported time from `Server-Timing` (its `total`, else the sum of `dur`).
- **Safety**: a run against a host outside this computer and private networks asks first (hosts that come from the data file included). A rendered request must stay on the destinations known at the start (scheme, host, port and `Host` header), so a captured value can't move the load elsewhere. Dropped requests count as errors; `rps` leaves out the wait for requests in flight after the end. Caps: 5,000 users, 50,000 requests/s, 100,000 in flight.
- Run history (the last 30 runs per test) is kept in the app data folder, not the workspace. `zorvik load` exits with 1 when a threshold fails.

## Servers and mocks
`crates/servers` has one listener per saved server, reporting traffic through a `Reporter`; `crates/api/src/servers.rs` starts and stops them, applies live edits and keeps bounded logs. Every kind follows one contract: serve until `ctx.cancel`, read the latest settings from `ctx.live`, answer `ctx.control` (send, disconnect).

Kinds: **mock API** (routes with `:params`, templated bodies, delays, faults, matching on query, headers and body, CORS, forward to a real backend), **MCP**, **WebSocket**, **Socket.IO**, **SSE**, **TCP**, **UDP**, **DNS** and a **TCP relay** that logs both directions. The Socket.IO server (`crates/servers/src/socketio.rs`) is built on the mock's hyper stack with upgrades on: one task per Engine.IO session owns its namespaces and heartbeat, long-polling GETs and POSTs and the WebSocket only carry packets in and out, and the upgrade from polling to WebSocket (probe, noop, upgrade) moves a session's output to the socket. Templates can use environment and workspace variables but never secret ones, and text sent by clients is never expanded. The MCP server (`crates/servers/src/mcp.rs`) answers JSON-RPC from its settings (tools with schemas and templated results, resources and templates, prompts) over Streamable HTTP (sessions by `Mcp-Session-Id`, a `GET` stream per session for `list_changed` notifications when the settings change), HTTP+SSE (`/sse` and `/messages`), and stdio for `zorvik serve --stdio`, where the traffic log goes to standard error. A server starts with its workspace only if this computer already started or saved that exact configuration, and saving a running server applies the change to it.

## AI agents
Coding agents (Claude Code, Codex, Gemini CLI, Cursor and others) control Zorvik through MCP.

```
Agent ──MCP over stdio──► zorvik mcp ──127.0.0.1 TCP + token──► Zorvik app ──► Api ──events──► UI
```
- **`zorvik mcp`** is the MCP server the agent starts. It answers `initialize` and `tools/list` itself; on the first tool call it connects to the running app, or starts it and waits.
- **The app** listens on `127.0.0.1` (random port) and writes `agent.json` (port and token) to its data folder, readable by the current user only. Both sides prove they know the token with a SHA-256 over a fresh nonce; the token itself is never sent.
- **Headless** (off by default): when the app is closed, the tools run inside `zorvik mcp`. Nothing can be approved there, so actions that would ask are refused.
- **Permissions**, checked in Rust on every call: reading is allowed; edits (requests, folders, environments, servers, load tests, files added with `write_file`) are allowed or asked (setting); requests to outside hosts are asked once per host and agent session; deletes, load tests, starting servers, reading files outside the workspace and opening workspaces always ask, and starting a program (an MCP request over stdio) asks every time. Settings, cookies and secret values are not available to agents; history, variables, server traffic and everything returned have credentials redacted.
- **Input**: tools that save files (`save_requests`, `save_server`, `save_load_test`) refuse fields the model doesn't have, with the nearest known name ("did you mean `status`?"), so a typo is never saved silently. Their JSON schemas document every field, unit and placeholder.
- **Tools** (`crates/api/src/agents/tools/`): workspace, requests (read several at once, save with query and path parameters), environments and variables (with values scripts saved), import and `update_from_openapi`, `send_request` (HTTP, GraphQL, gRPC, DNS, MCP, SSE read to an end), collection runs, GraphQL schemas, gRPC services, MCP catalogs (`mcp_catalog`), load tests, servers (`read_server`, `save_server`, `create_mock`, `start_server` with any port, `get_server_traffic`), `read_history`, `export_request` (cURL and code in 16 languages), `write_file` (up to 10 MB, not into Zorvik's own folders).
- **Where requests go**: each tool call runs in an agent scope. A host guard in the engine keeps every request (redirects, OAuth token requests, gRPC channels, DNS resolvers, collection runs) on the approved hosts. During an agent's call no files outside the workspace are read.
- **Visible**: the title bar shows the connected agent; the Agents panel lists every tool call; the app opens what the agent works on (setting).

Code: `crates/api/src/agents/` (sessions, confirmations, tool definitions and implementations), `crates/mcp` (JSON-RPC protocol, bridge, listener, `agent.json`).

## Training Bootcamp
The **Academy** is a course inside the app, opened from the pinned Training Bootcamp workspace (its title bar then has a Workbench | Academy switch); see [academy.md](academy.md) for the course format and how to write a lesson.
- **Course**: `crates/academy/course/` (a folder per unit, a Markdown file with YAML front matter per lesson), embedded at build time and validated by tests.
- **Labs**: `crates/api/src/academy/` saves the lab's servers into the Bootcamp workspace (`Lab · …`), starts them on free ports with the normal server manager, fills and activates the **Lab** environment, and starts practice servers from `crates/testkit` when a lab needs them. While a lab runs, `Api::call` notes each call in a short journal; the steps are checked in order after each call and once a second, against lab servers' traffic, the journal, workspace files, finished runs and typed answers. Rewards reach the UI as `academy` events.
- **UI**: `app/src/components/academy/` (Academy view, lesson reader with diagrams, Lab Guide docked beside the workbench, celebrations, certificate), `app/src/store/academy.ts`.
- **Every lab is tested**: `crates/api/tests/academy.rs` runs each step's solution and checks the step passes only after it.
- **Updates keep progress**: progress is kept by lesson and unit id. `knownLessons` in the progress file lists the lessons a learner has seen; lessons an update adds are "new" until opened. Progress saved before `knownLessons` existed knows the lessons without an `added` release in their front matter.

## Updates
The desktop app updates itself from GitHub Releases; nothing else is contacted and nothing about the user is sent.
- **`app/src-tauri/src/updates.rs`** wraps the Tauri updater: a check 20 seconds after start and every 6 hours, through the proxy from Settings. The state (`idle`, `checking`, `upToDate`, `available`, `downloading`, `ready`, `installing`, `failed`) goes to the UI as the `zv:update` event.
- **Settings → Updates** (`updates.mode`, `updates.channel` in `settings.json`): automatic downloads in the background and installs on quit; notify only shows a notice; off never checks. Stable and nightly channels read different `latest.json` files.
- **Only signed updates install**: the signature must match the public key in `tauri.conf.json` and name the version. Nightlies share a version number, so the build time decides (`ZORVIK_BUILT_AT`).
- **Which copies install**: the Windows installer (NSIS), the macOS app in a writable folder, the AppImage. The portable zip, `.deb`, `.rpm`, a copy running from the disk image and debug builds only tell the user a new version is out.
- **Never an error for a failed update**: when a download, its signature check or the install fails, the status becomes `available` with `byHand`, and the notice links to the release page; that version isn't downloaded again until the next start. The reason goes to the log.
- **Restarting**: `update_install` stops servers and the agent listener, installs and restarts; the UI first lists what a restart would interrupt. Otherwise a downloaded update installs on exit (`RunEvent::Exit`).
- **UI**: `app/src/store/updates.ts`, the notice in `app/src/components/UpdateNotice.tsx`, the Settings section in `app/src/components/modals/UpdateSettings.tsx`.

See [ci-release.md](ci-release.md#updates) for how `latest.json` is made and the signing key.

## Data locations
- **Workspace folder** (shared through Git): requests, folders, environments, servers, load tests, kept OpenAPI documents.
- **App data folder** (per computer, `org.libreguild.zorvik` in the OS data directory): `settings.json`, `history.sqlite3`, `secrets.json`, `cookies/`, `oauth-tokens.json`, `local-values.json`, `state.json`, `trusted-servers.json`, `load-runs/`, `agent.json`, the Training Bootcamp workspace (`bootcamp/`) and progress (`academy-progress.json`). Logs go to the OS log folder for the app.

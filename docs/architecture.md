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
crates/formats/      Data model (YAML files) and import/export: cURL, Postman, OpenAPI
crates/script/       Sandboxed JavaScript (QuickJS) for pre-request and post-response scripts
crates/load/         Load generator: own runtime, pooled client, HdrHistogram metrics, reports
crates/servers/      Servers the user runs: mock HTTP, WebSocket, SSE, TCP, UDP, DNS, TCP relay
crates/engine/       Networking only: HTTP/1.1, HTTP/2, HTTP/3, TLS, proxy, WebSocket, SSE,
                     TCP/UDP/DNS/MQTT/gRPC clients, cookies, decoding, timing, network tools
crates/cli/          `zorvik`: run, load, serve and mcp (the command line inside every install)
crates/mcp/          MCP for AI agents: the stdio bridge (`zorvik mcp`) and the app's listener
crates/testkit/      Local test servers used by the tests (HTTP, TLS, proxy, OAuth, GraphQL, gRPC)
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
3. The request is resolved: variables, inherited headers and auth from folders and the workspace, body encoding.
4. OAuth 2.0: a cached token is used, refreshed or fetched.
5. `engine::Client::send`: DNS, TCP (Happy Eyeballs), proxy tunnel, TLS, HTTP/1.1 or HTTP/2 (or QUIC for HTTP/3), redirects, decoding.
6. Post-response scripts run the tests and fill the console.
7. A history row is written. The response goes back with timing, TLS details, cookies, the headers really sent and the script results.

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
Other request kinds add `kind:` (`websocket`, `sse`, `grpc`, `tcp`, `udp`, `dns`, `mqtt`). Default values are left out so files stay small and diffs stay clean.

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

## Variables
Precedence, highest first: CLI `--var`, `pm.variables` (this send or run), the runner's data row, the active environment, workspace variables, globals, then dynamic values (`{{$uuid}}`, `{{$timestamp}}`, `{{$randomInt}}`, `{{$randomEmail}}` and others). Values that scripts set on an environment, the workspace or globals are kept on this computer (`local-values.json`), never in the files, and win over the file value in their scope. Variables can reference variables (depth 10; cycles are reported).

## Networking
- **A fresh connection per request**, so DNS, connect and TLS are always measured. Timing phases: DNS, TCP connect (with the proxy tunnel), TLS, time to first byte, download.
- **DNS** uses the OS resolver (hosts file and VPN split DNS work); IPv6 and IPv4 race with Happy Eyeballs.
- **TLS** trusts the OS certificate store (Windows store, macOS keychain), so corporate inspection CAs work. Extra CA, client certificates (mTLS) and "don't verify" are available per request or globally.
- **Proxy**: system (environment variables, then the OS settings), none or manual, with Basic auth and a bypass list; localhost is always direct. PAC files, SOCKS and NTLM/Kerberos are not supported.
- **Redirects** follow browser rules; `Authorization` and `Cookie` are dropped when the origin changes.
- **Limits**: response bodies are capped (100 MB by default) and decompression is bounded; WebSocket and gRPC messages up to 64 MB; stream lines and framed socket messages up to 16 MB.
- **HTTP/3** is opt-in per request or globally: `https://` only and never through a proxy.
- **gRPC**: `grpc://` (plaintext HTTP/2) and `grpcs://` (TLS), with server reflection or `.proto` files and every call type.
- **Other clients**: WebSocket, SSE, TCP (raw, line or length-prefixed framing, optional TLS), UDP, MQTT 3.1.1 and 5, DNS (UDP, TCP, DoT, DoH).

## Scripts
`crates/script` runs JavaScript in QuickJS. Each script gets a fresh runtime with no file, network, process, timer or module access, a 64 MB memory limit and a time limit (Settings, 5 s by default). Input and results cross as JSON, so a script can only compute.

The API is a Postman-compatible subset (`crates/script/src/prelude.js`), so imported collections run unchanged: `pm.variables`, `pm.environment`, `pm.collectionVariables`, `pm.globals`, `pm.iterationData`, `pm.request`, `pm.response`, `pm.test`, `pm.expect` (a chai subset), `pm.execution.setNextRequest`, `console`, plus the legacy `tests[...]` and `postman.*` forms. `pm.sendRequest`, `require` and timers are not supported and say so.

## Collection runner
`crates/api/src/runner.rs` is shared by the app's Runner tab and `zorvik run`. It reads the requests of a folder once, then sends them in order for each iteration through the same scripted send as a single request. CSV or JSON data files give one iteration per row. Non-HTTP requests are reported as skipped. A request fails when it can't be sent, a script throws or a test fails; without tests, an HTTP status of 400 or more fails it. Reports are JSON or JUnit XML, and memory stays bounded on long runs.

## Load testing
`crates/load` runs a plan on its own Tokio runtime so the app stays responsive.
- **Models**: virtual users (closed: send, wait, think, repeat) or arrival rate (open: requests start on schedule however slow the server is). In the open model latency is measured from the scheduled start, so a slow server can't hide its queueing.
- **Stages** ramp users or requests per second linearly; thresholds (`p50` … `p99.9`, `avg`, `max`, `errorRate`, `rps`) gate the run live and at the end.
- **Client**: pooled HTTP/1.1 keep-alive and HTTP/2 multiplexing on the engine's connect path (DNS, proxy tunnel, OS-trusted TLS). Requests are resolved once; ones with dynamic variables are rendered per iteration.
- **Metrics**: an HdrHistogram per target and in total, 1-second buckets for the charts, snapshots to the UI four times a second.
- **Safety**: a run against a host outside this computer and private networks asks first. Caps: 5,000 users, 50,000 requests/s, 100,000 in flight.
- Run history (the last 30 runs per test) is kept in the app data folder, not the workspace. `zorvik load` exits with 1 when a threshold fails.

## Servers and mocks
`crates/servers` has one listener per saved server, reporting traffic through a `Reporter`; `crates/api/src/servers.rs` starts and stops them, applies live edits and keeps bounded logs. Every kind follows one contract: serve until `ctx.cancel`, read the latest settings from `ctx.live`, answer `ctx.control` (send, disconnect).

Kinds: **mock API** (routes with `:params`, templated bodies, delays, faults, matching on query, headers and body, CORS, forward to a real backend), **WebSocket**, **SSE**, **TCP**, **UDP**, **DNS** and a **TCP relay** that logs both directions. Templates can use environment and workspace variables but never secret ones, and text sent by clients is never expanded. A server starts with its workspace only if this computer already started or saved that exact configuration.

## AI agents
Coding agents (Claude Code, Codex, Gemini CLI, Cursor and others) control Zorvik through MCP.

```
Agent ──MCP over stdio──► zorvik mcp ──127.0.0.1 TCP + token──► Zorvik app ──► Api ──events──► UI
```
- **`zorvik mcp`** is the MCP server the agent starts. It answers `initialize` and `tools/list` itself; on the first tool call it connects to the running app, or starts it and waits.
- **The app** listens on `127.0.0.1` (random port) and writes `agent.json` (port and token) to its data folder, readable by the current user only. Both sides prove they know the token with a SHA-256 over a fresh nonce; the token itself is never sent.
- **Headless** (off by default): when the app is closed, the tools run inside `zorvik mcp`. Nothing can be approved there, so actions that would ask are refused.
- **Permissions**, checked in Rust on every call: reading is allowed; edits are allowed or asked (setting); requests to outside hosts are asked once per host and agent session; deletes, load tests, starting servers and opening workspaces always ask. Settings, cookies, history and secret values are not available to agents, and credentials are redacted from what agents get back.
- **Where requests go**: each tool call runs in an agent scope. A host guard in the engine keeps every request (redirects, OAuth token requests, gRPC channels, DNS resolvers, collection runs) on the approved hosts. During an agent's call no files outside the workspace are read.
- **Visible**: the title bar shows the connected agent; the Agents panel lists every tool call; the app opens what the agent works on (setting).

Code: `crates/api/src/agents/` (sessions, confirmations, tool definitions and implementations), `crates/mcp` (JSON-RPC protocol, bridge, listener, `agent.json`).

## Data locations
- **Workspace folder** (shared through Git): requests, folders, environments, servers, load tests.
- **App data folder** (per computer, `org.libreguild.zorvik` in the OS data directory): `settings.json`, `history.sqlite3`, `secrets.json`, `cookies/`, `oauth-tokens.json`, `local-values.json`, `state.json`, `trusted-servers.json`, `load-runs/`, `agent.json`. Logs go to the OS log folder for the app.

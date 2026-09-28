# Testing

Tests talk to real servers on `127.0.0.1` wherever they can: real TLS, real proxies, real HTTP/2, real DNS packets. Mocks are the exception.

## Running them
```bash
cargo test                          # every Rust test (also regenerates app/src/bindings)
cargo test -p zorvik-engine         # one crate
cd app && npm test                  # UI unit tests (Vitest)
cd app && npm run e2e               # end-to-end (Playwright, Chromium)
```
E2E needs the CLI built once, because the AI agent tests start the real `zorvik mcp`: `cargo build -p zorvik-cli`. To run E2E in WebKit (the engine of the macOS app), use `E2E_WEBKIT=1 npx playwright test --project=webkit` on its own.

## Where the tests are
| Area | Tests |
|---|---|
| Networking | `crates/engine/tests/`: HTTP/1.1, HTTP/2, TLS, proxies, redirects and decoding (`http_client.rs`), WebSocket and SSE (`streaming.rs`), TCP (`socket.rs`), UDP, DNS, MQTT, HTTP/3, gRPC, the load-test connection pool (`pool.rs`), network tools |
| Workspace and auth | `crates/workspace/tests/`: files on disk (`store.rs`), OAuth 2.0 flows (`oauth.rs`) |
| Import and export | inline tests in `crates/formats/src/{curl,postman,openapi,mock,snippet}.rs` (`snippet` has an ignored test that syntax-checks the generated code with node, python3 and swiftc: `cargo test -p zorvik-formats snippet -- --ignored`) |
| API specs | `crates/formats/src/spec_check.rs` (the schema checks), `crates/api/src/spec_update.rs` (merging edits), `crates/api/tests/api.rs` (import keeps the spec, responses checked, preview and update, base URL prompt) |
| Scripts | `crates/script/src/tests.rs` (the `pm` API, sandbox limits), `crates/api/tests/scripts.rs` (scripts around real sends) |
| App API | `crates/api/tests/`: RPC methods, GraphQL, gRPC, DNS, MQTT, mocks, the collection runner (repeat until, event streams, skip reasons), load tests |
| Servers | `crates/servers/tests/`: every server kind on port 0, driven by real clients |
| Load generator | `crates/load/tests/runner.rs`: both models against real servers (counts, ramps, overload latency, stop, errors, data rows per user, capture chains, timing phases, Server-Timing); JSON paths, captures and Server-Timing parsing in `crates/load/src/capture.rs` |
| Command line | `crates/cli/tests/`: `zorvik run`, `load`, `serve` and `mcp` as real processes |
| AI agents | `crates/mcp/tests/agents.rs` (bridge, listener, token proof, confirmations; mock servers built, started and inspected; variables, history, streams, files and exports) and `app/e2e/agents.spec.ts` |
| Training Bootcamp | `crates/academy` (the course loads and validates, patterns, XP and badge rules), `crates/api/tests/academy.rs` (every lab finished step by step with its solutions; progress, rewards, reset), `app/src/components/academy/academy.test.ts`, `app/e2e/academy.spec.ts`. One lab: `ACADEMY_LESSON=<id> cargo test -p zorvik-api --test academy` |
| UI logic | `app/src/**/*.test.ts(x)` (Vitest, jsdom) |
| User flows | `app/e2e/*.spec.ts`: requests, GraphQL, gRPC, scripts, runner, load tests, servers, docs, appearance, AI agents, the Academy |

## End-to-end setup
`app/playwright.config.ts` starts three things, on ports chosen to avoid clashes with your own servers:
- the test servers (`zorvik-testkit`) on 18787,
- the dev bridge (the real Rust API over HTTP) on 18799, with a fresh temporary workspace and data folder,
- Vite on 15420.

The browser drives the production UI against the production API, so E2E covers the same code the desktop app runs. Locally it reuses servers that are already running; CI starts fresh ones.

## Test servers (`crates/testkit`)
- **`TestServer`**: HTTP/1.1 and h2c, or TLS with h2 through ALPN and a generated CA. Endpoints include `/echo`, `/anything/*`, `/status/{code}`, `/redirect/{n}`, `/delay/{ms}`, `/gzip`, `/brotli`, `/bytes/{n}`, `/cookies`, `/basic-auth/{user}/{pass}`, `/bearer`, `/json`, `/sse`, `/ws` (echo), `/graphql` and `/graphql-auth`.
- **`TestProxy`**: CONNECT and forward proxy with optional Basic auth.
- **`H3TestServer`**: HTTP/3 over QUIC.
- **OAuth 2.0 server**: client credentials, password, authorization code with PKCE, refresh.
- **GraphQL**: a small hand-written executor with introspection; `?legacy=1` answers like an older server.
- **`GrpcTestServer`**: `zorvik.test.v1.Echo` (unary, server, client and bidi streaming, failures, metadata, gzip) with server reflection v1 and v1alpha. The `.proto` files are in `crates/testkit/proto`.

Run them by hand with `cargo run -p zorvik-testkit`: HTTP on 18787, HTTPS on 18788, gRPC on 18790, gRPC over TLS on 18791.

## Notes
- Tests that move files to the OS trash only run with `ZORVIK_TEST_TRASH=1` (CI sets it).
- CI runs the Rust tests on Linux and Windows and E2E on Linux; macOS is covered by the release build and by maintainers.
- In E2E, pick editor completions by clicking the option: CodeMirror ignores Enter for 75 ms after the list opens.

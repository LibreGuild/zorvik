---
title: zorvik serve
description: Start a saved mock API or server (MCP, WebSocket, Socket.IO, SSE, TCP, UDP, DNS, TCP relay) from a terminal or CI and print its traffic as text or JSON lines, or serve an MCP server over stdio for AI apps.
sidebar:
  order: 4
---

`zorvik serve` starts a server saved in the workspace (under `servers/`) and prints its traffic until you press <kbd>Ctrl</kbd>+<kbd>C</kbd>. Use it to run a mock API next to your tests in CI, or to share a mock with someone who doesn't have the app open.

```text
zorvik serve [OPTIONS] <WORKSPACE> <SERVER>
```

```bash
zorvik serve ./my-api "Payments mock" --port 3100
```

Servers are built in the app. See [Mock servers](../../servers/mock-api/) for routes, templates, delays, faults, CORS and forwarding.

## Arguments

| Argument | Description |
|---|---|
| `<WORKSPACE>` | The workspace folder (the one that contains `zorvik.yaml`) |
| `<SERVER>` | The server's name, or its id (its file name under `servers/` without `.yaml`), ignoring case |

## Options

| Option | Value | Default | Description |
|---|---|---|---|
| `-e`, `--env` | `<ENV>` | None | Environment for `{{variables}}` in answers, by name or id, ignoring case |
| `--var` | `<KEY=VALUE>` | | Set a variable with the highest precedence. Repeatable. |
| `--port` | `<PORT>` | The saved port | Listen on this port instead. `0` picks any free port. |
| `--host` | `<HOST>` | The saved address | Listen on this address instead. `127.0.0.1` is this computer only; `0.0.0.0` lets other devices connect. |
| `-k`, `--insecure` | | Off | Don't verify TLS certificates when forwarding (a mock's fallback to a real backend, and the TCP relay) |
| `--json` | | Off | Print JSON lines (the start, then one per traffic entry) instead of text |
| `--stdio` | | Off | MCP servers only: talk MCP over standard input and output instead of listening on a port, so an AI app can start the server as a program. The traffic log goes to standard error. |
| `-h`, `--help` | | | Print help |

### Variables

Answers can use `{{variables}}` (for example in a mock route's body). They come from, highest first: `--var`, the environment given with `--env`, then workspace variables. Secret values are not in workspace files, so they're not available unless you pass them with `--var`.

### Port and address

Without `--port` and `--host`, the server listens where it's saved to (a saved port of `0` means any free port). The URL it prints tells you where it really listens, which is how you find the port after `--port 0`.

If the port is taken, the command stops with exit code 2 and, when the system tells, says which program holds it, for example `Port 3100 is already in use by node (PID 4242). Stop it or pick another port.`

## Server kinds

| Kind | Printed as | What it does |
|---|---|---|
| Mock API | `Mock API` | Answers HTTP requests from its routes; can forward others to a real backend |
| MCP | `MCP server` | Offers tools, resources and prompts to MCP clients and AI apps (the URL includes the endpoint path, such as `/mcp`) |
| WebSocket | `WebSocket server` | Accepts WebSocket connections and answers messages by its rules |
| Socket.IO | `Socket.IO server` | Accepts socket.io-client connections (long-polling and WebSocket) and answers events |
| Server-Sent Events | `Event stream server` | Sends its events to each client that connects |
| TCP | `TCP server` | Accepts TCP connections and answers by its rules |
| UDP | `UDP server` | Receives datagrams and answers by its rules |
| DNS | `DNS server` | Answers DNS queries from its records |
| TCP relay | `TCP relay` | Passes connections on to a target and logs both directions |

## Output

```text
Mock API "Payments mock" is running at http://127.0.0.1:3100 (Ctrl+C to stop)
  POST    /payments  → 201
  GET     /payments/:id  → 200
  ANY     /health  → 200
    2.314s  #1 connected from 127.0.0.1:52182
    2.316s  #1 POST /payments → 201
    9.870s  #2 GET /payments/pay_123 → 200
^C
Stopped.
```

- **The first line** names the kind, the server and the URL it listens on.
- **For a mock API**, the enabled routes follow: method (`ANY` for `*`), path and status.
- **Then one line per traffic entry**: the time since the start in seconds, the connection number (`#3`), and what happened. HTTP exchanges are colored by status (green, yellow for 4xx, red for 5xx and errors). Messages show their direction (`in`, `out`, `→ target` and `← target` for the relay), an optional `[label]`, and the payload: text up to 160 characters, or `(N bytes of binary data)`.

Control characters that clients send are printed escaped (`\u{1b}`), so traffic can't change your terminal.

### JSON lines

With `--json`, every line is one JSON object:

```json
{"type":"started","name":"Payments mock","kind":"http","url":"http://127.0.0.1:3100"}
{"type":"traffic","entry":{"id":2,"timestamp":1790571915806.0,"kind":"http","conn":1,"peer":"127.0.0.1:52182","direction":null,"summary":"POST /payments → 201","text":null,"base64":null,"size":0,"truncated":false,"http":{"method":"POST","path":"/payments","httpVersion":"HTTP/1.1","requestHeaders":[{"name":"Content-Type","value":"application/json"}],"requestBody":"{\"amount\":10}","status":201,"responseHeaders":[{"name":"Content-Type","value":"application/json"}],"responseBody":"{\"id\":\"pay_123\"}","durationMs":0.4,"route":"Create payment","note":null}}}
{"type":"stopped"}
```

| Line `type` | When | Fields |
|---|---|---|
| `started` | The server is listening | `name`, `kind` (`http`, `mcp`, `websocket`, `socketio`, `sse`, `tcp`, `udp`, `dns` or `tcpProxy`), `url` |
| `traffic` | Each traffic entry | `entry` (below) |
| `stopped` | After Ctrl+C | — |

The `entry` object:

| Field | Description |
|---|---|
| `id` | Entry number |
| `timestamp` | Unix epoch milliseconds |
| `kind` | `open` (a client connected), `close`, `data` (a message or bytes), `http` (a request and the mock's answer), `dns` (a query and the answer), `info`, `error` |
| `conn` | Connection number, when the entry belongs to one |
| `peer` | The client's address |
| `direction` | `in` (from the client), `out` (to the client), `toTarget` and `fromTarget` (relay), or `null` |
| `summary` | The one-line description the text output shows |
| `text` | The payload as text when it's valid UTF-8 (or details, such as a DNS answer) |
| `base64` | The payload as Base64 when it isn't text |
| `size` | Payload size in bytes |
| `truncated` | `true` when only the first 64 KB of the payload is kept |
| `http` | For `http` entries: `method`, `path` (with the query), `httpVersion`, `requestHeaders`, `requestBody`, `status`, `responseHeaders`, `responseBody`, `durationMs`, `route` (the route that answered, `null` for the fallback) and `note` (an injected fault, `error`, `reset` or `hang`, or `proxy` when forwarded) |

Very busy servers log at most 1,000 entries and 8 MB of payload per second; traffic beyond that is still served, just not printed.

## --stdio

With `--stdio`, an [MCP server](../../mcp/servers/) talks to one client over its standard input and output, the way AI apps start local MCP servers: one JSON-RPC message per line in each direction. Nothing but MCP messages goes to standard output; the traffic log goes to standard error (as JSON lines with `--json`), where AI apps show it as the server's log. The server stops when its input closes.

```bash
zorvik serve ./my-workspace "Weather mock" --stdio
```

```json title="An AI app's MCP configuration"
{ "mcpServers": { "weather-mock": { "command": "zorvik", "args": ["serve", "/path/to/my-workspace", "Weather mock", "--stdio"] } } }
```

Other kinds of servers don't speak stdio: `error: --stdio works with MCP servers only; "Users API" is a Mock API`.

## Stopping

Press <kbd>Ctrl</kbd>+<kbd>C</kbd>. The server stops accepting connections, gives open ones up to 3 seconds to close, prints `Stopped.` (or `{"type":"stopped"}`), and exits with code 0.

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Stopped with Ctrl+C |
| `2` | Couldn't start (server or environment not found, the server file can't be read, the port is in use, an invalid setting), or the server stopped with an error: `error: the server stopped: …` |

Examples of start errors:

```text
error: server 'nope' not found (servers: Payments mock, Users API)
error: environment 'prod' not found
error: Port 3100 is already in use by another server or app. Stop it or pick another port.
```

## Examples

```bash
# The saved port and address
zorvik serve . "Payments mock"

# Any free port, in the background; the first JSON line has the URL
zorvik serve . "Payments mock" --port 0 --json > mock.jsonl &
sleep 1 && head -n 1 mock.jsonl

# Reachable from other devices (phones, containers) on port 8080
zorvik serve . "Payments mock" --host 0.0.0.0 --port 8080

# Answers that use Staging's variables, plus one override
zorvik serve . "Users API" -e Staging --var region=eu

# A relay to a server with a self-signed certificate
zorvik serve . "DB relay" -k
```

## CI examples

Start the mock in the background, wait until it listens, run the tests, then stop it. Install the command line as shown in the [zorvik run CI examples](../run/#ci-examples).

### GitHub Actions

```yaml title=".github/workflows/contract.yml"
jobs:
  with-mock:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Install Zorvik
        run: |
          curl -fsSL -o zorvik.deb https://github.com/LibreGuild/zorvik/releases/latest/download/Zorvik-Linux-amd64.deb
          sudo apt-get update
          sudo apt-get install -y ./zorvik.deb
      - name: Start the mock
        run: |
          zorvik serve ./api "Payments mock" --port 3100 --json > mock.jsonl &
          until grep -q '"type":"started"' mock.jsonl; do sleep 0.2; done
      - name: Run the app's tests against the mock
        run: npm test
        env:
          PAYMENTS_URL: http://127.0.0.1:3100
      - name: Show what the mock received
        if: always()
        run: cat mock.jsonl
```

The background server keeps running until the job ends. `mock.jsonl` shows every request the mock received, which helps when a test fails.

### GitLab CI

```yaml title=".gitlab-ci.yml"
test-with-mock:
  image: ubuntu:24.04
  variables:
    DEBIAN_FRONTEND: noninteractive
  before_script:
    - apt-get update
    - apt-get install -y curl ca-certificates
    - curl -fsSL -o /tmp/zorvik.deb https://github.com/LibreGuild/zorvik/releases/latest/download/Zorvik-Linux-amd64.deb
    - apt-get install -y /tmp/zorvik.deb
  script:
    - zorvik serve ./api "Payments mock" --port 3100 --json > mock.jsonl &
    - until grep -q '"type":"started"' mock.jsonl; do sleep 0.2; done
    - zorvik run ./api --folder Checkout --var base=http://127.0.0.1:3100 --junit zorvik-junit.xml
  artifacts:
    when: always
    paths:
      - mock.jsonl
    reports:
      junit: zorvik-junit.xml
```

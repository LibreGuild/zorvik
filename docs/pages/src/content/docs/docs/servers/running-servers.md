---
title: Running servers
description: Start and stop servers, choose address and port, start servers with the workspace, use TLS, and read the traffic log.
sidebar:
  order: 9
---

This page covers what all servers have in common: starting and stopping, the address and port they listen on, starting with the workspace, TLS, variables and the traffic log.

## Kinds of servers

| Kind | In the file (`kind`) | New servers listen on | TLS | Address shown |
|---|---|---|---|---|
| [Mock API (HTTP)](../mock-api/) | `http` | 3000 | Yes | `http://…` / `https://…` |
| [WebSocket server](../websocket-and-sse-servers/#websocket-server) | `websocket` | 3001 | Yes | `ws://…` / `wss://…` |
| [Socket.IO server](../socketio-server/) | `socketio` | 3003 | Yes | `http://…` / `https://…` |
| [Event stream (SSE) server](../websocket-and-sse-servers/#event-stream-sse-server) | `sse` | 3002 | Yes | `http://…` / `https://…` |
| [TCP server](../tcp-udp-dns-servers/#tcp-server) | `tcp` | 9000 | Yes | `tcp://…` / `tls://…` |
| [UDP server](../tcp-udp-dns-servers/#udp-server) | `udp` | 9001 | No | `udp://…` |
| [DNS server](../tcp-udp-dns-servers/#dns-server) | `dns` | 1053 | No | `dns://…` |
| [TCP relay](../relay/) | `tcpProxy` | 9100 | No | `tcp://…` |

A new server takes the listed port, or the next one that no other saved server uses. All servers start on `127.0.0.1`.

## Servers in the sidebar

The **Servers** sidebar lists the workspace's servers with their kind, name and port; the port turns green and a green dot appears while a server runs.

- **+** (**New server**) creates a server of any kind, or a mock from an OpenAPI document (**Mock from OpenAPI…**).
- Hover a server to get a start/stop button and its **⋯** menu (also on right-click): **Start** / **Stop**, **Copy address** (while running), **Rename…** (<kbd>F2</kbd>), **Duplicate** and **Delete**.
- Drag servers to reorder them. The filter box matches names and ports.
- **Duplicate** copies the server as `<name> copy`, with **Start with workspace** off. **Delete** moves the file to the trash and stops the server if it runs.

A server file that can't be read is marked with a warning; hover it for the reason.

## Start, stop and restart

Open a server to get its tab. At the top:

- **Start** (<kbd>Mod</kbd>+<kbd>Enter</kbd>) starts the server with the settings in the tab, **including unsaved changes**.
- While it runs, the status reads **Running at** and the address, with a copy button, and the buttons are **Restart** and **Stop**. <kbd>Mod</kbd>+<kbd>Enter</kbd> restarts.
- **Save** (<kbd>Mod</kbd>+<kbd>S</kbd>) writes the file. Running doesn't need saving, but other people only get what you save.

**Changes apply while the server runs.** A moment after you stop typing, the running server takes the new routes, rules, records, events and so on, without a restart and without dropping clients. Only the listen address, the port, TLS and the kind need a restart; then a banner says **Restart the server to use the new address, port or TLS setting.**

**Servers keep running** when you close their tab or open another workspace. The title bar shows **N running** while any server runs, in any workspace: its menu lists them with their addresses, opens one (in the current workspace) or stops it, and has **Stop all** (which asks first when several run). Quitting Zorvik while servers run asks **Quit Zorvik?** with **Stop and quit**.

When a server stops by itself, for example because of an error, a message says **Users API stopped** (with the server's name) and gives the reason.

## Address and port

**Listen on** chooses which network interfaces the server listens on:

| Choice | Address | Who can connect |
|---|---|---|
| **This computer only (127.0.0.1)** | `127.0.0.1` | Programs on this computer. The default. |
| **Other devices too (0.0.0.0)** | `0.0.0.0` | Also other devices on your network (phones, other computers, containers). A banner reminds you; on Windows, the firewall may ask to allow it. |
| **Another address…** | What you type, e.g. `192.168.1.20` or `::1` | Only that interface. |

The address shown for a server listening on `0.0.0.0` uses `127.0.0.1`. To reach it from another device, use this computer's address on the network; the [Network interfaces](../../tools/network-tools/#network-interfaces) tool lists it.

**Port**: 1 to 65535, or `0` for any free port (the address shows the one chosen).

If the server can't listen, starting fails with one of these messages:

| Message | What to do |
|---|---|
| **Port 3000 is already in use by node (PID 4312). Stop it or pick another port.** | Another program holds the port; Zorvik names it when the system says which one. |
| **Port 3000 is already in use by another server in Zorvik. Stop it or pick another port.** | Another of your servers uses it. |
| **Not allowed to listen on port 80 (ports below 1024 may need administrator rights).** | Pick a port from 1024 up. |
| **This computer has no address 192.168.1.99. Use 127.0.0.1 or 0.0.0.0.** | The custom address is not one of this computer's. |

## Start with workspace

Turn on **Start with workspace** to start the server whenever its workspace is opened.

Because server files come from Git, a cloned or pulled workspace must not open listeners on your computer by itself. So only configurations **this computer has started or saved before** start with the workspace:

- Starting or saving a server marks its exact configuration as trusted. The last 8 configurations of each server are remembered, in `trusted-servers.json` in the app's data folder (not in the workspace).
- Every setting counts except the server's position in the list and the **Start with workspace** switch itself.
- A server that is new to this computer, or whose file changed outside Zorvik (for example by `git pull`), is not started. Instead a message says **A server could not start** with **Users API: not started automatically because it is new or was changed outside Zorvik (e.g. by a Git pull). Start it once to let it start with the workspace.**
- A server that is already running is left alone.

Adding a route with **Mock this response** keeps a trusted mock trusted.

## Variables

Server answers can use `{{variables}}` from the **active environment** and the **workspace**; secret variables are never inserted, and global variables are not used. A running server reads them when it starts and whenever its settings change; after switching environments, restart the server. The details are in [Templates](../templates/#variables).

## TLS

Mock APIs, WebSocket, Socket.IO, event stream and TCP servers can listen with TLS: turn on **TLS** next to the port. Two fields appear:

| Field | Description |
|---|---|
| **Certificate (PEM)** | Your certificate chain, a PEM file. |
| **Key (PEM)** | Its private key, a PEM file. |

- **Both empty**: Zorvik generates a **self-signed certificate** for `localhost`, `127.0.0.1` and `::1` (named "Zorvik local server"). It is created anew each time the app (or `zorvik serve`) starts, so clients must skip certificate verification, or trust it again after every restart of the app.
- **Your own certificate**: set both files (one alone is an error). Relative paths start at the workspace folder. Each file must be a regular file of at most 1 MB, and the key must match the certificate.

TLS 1.2 and 1.3 are offered. Client certificates are not requested. A client must finish the TLS handshake within 10 seconds.

| Kind | ALPN protocols offered |
|---|---|
| Mock API, event stream | `h2`, `http/1.1` |
| WebSocket, Socket.IO | `http/1.1` |
| TCP | none |

UDP, DNS and relay servers can't use TLS (**TLS is not available for this kind of server**); the relay can speak TLS to its target instead.

To call a server that uses the self-signed certificate from Zorvik, set **Verify TLS certificates** to **Off** in the request's **Settings** tab. With curl, use `-k`. For a certificate your clients trust, see [TLS & certificates](../../requests/tls-and-certificates/).

## Traffic log

The right side of a server's tab is its **Traffic** panel.

**Counters** at the top: open and total connections (for servers with connections), requests, queries or messages received, bytes in (↓) and out (↑), and errors. When the server is stopped, the panel says **Stopped · showing the last run** and keeps the last run's traffic until the next start.

**Entries**, newest at the bottom (the log follows new entries while you are at the bottom):

| Entry | Shows |
|---|---|
| Connection opened / closed | `#3 connected from 127.0.0.1:53422`, and why it closed. |
| Message | Direction, time, connection, the start of the payload and its size. Relays show **→ target** and **← target**; event streams the event name. |
| HTTP exchange | Method, path, status, and the time the mock took. |
| DNS query | Question and answer summary. |
| Note / error | Server notes, rule problems, TLS handshake failures, bad clients. |

Click a message, HTTP or DNS entry to expand it: payloads in full (JSON pretty-printed, binary as hex), HTTP request and response headers and bodies with the route that answered (or **no route matched**) and any note (**error**, **reset**, **hang**, **proxy**, **CORS preflight**, **client left**, **server stopped**), or the DNS answer in `dig` format. Every block has a copy button.

**Filter traffic** matches entry summaries, payloads and client addresses (case-insensitive); type `#3` for connection 3. **Clear traffic** (bin icon) empties the log.

Limits, so a flood can't swamp the app:

- Each entry keeps the first 64 KB of its payload (the size shows the full length).
- The log keeps the newest 5,000 entries.
- At most 1,000 entries and 8 MB of payload per second reach the log. Beyond that, a note says how many entries were left out; the counters still include everything.

## Sending from the traffic panel

WebSocket, Socket.IO, event stream, TCP, UDP and relay servers have a composer under the log to send to **All clients** or one connection. Hover a log row for its **Send to** and **Disconnect** buttons. See [WebSocket & SSE servers](../websocket-and-sse-servers/#sending-from-the-traffic-panel), [TCP, UDP & DNS servers](../tcp-udp-dns-servers/) and [TCP relay](../relay/#sending-to-a-client) for what each kind sends. Mock APIs and DNS servers only answer, so they have no composer.

## Command line

`zorvik serve` runs a saved server without the app, for CI pipelines and containers, and prints its traffic until <kbd>Ctrl</kbd>+<kbd>C</kbd>:

```sh
zorvik serve ./my-workspace "Users API" --env Staging --port 8080
```

It takes the environment (`--env`), variables (`--var key=value`), another port or address (`--port`, `--host`), `-k` to skip certificate checks when forwarding, and `--json` for JSON lines. See [zorvik serve](../../cli/serve/).

## AI agents

Agents connected to Zorvik can list, read, create and save servers, build mocks, start and stop them and read their traffic (`list_servers`, `read_server`, `save_server`, `create_mock`, `start_server`, `stop_server`, `get_server_traffic`). See [Agent tools](../../agents/tools/).

## Saved format

Each server is one YAML file under `servers/` in the workspace. Only the section for its `kind` is used; the others are kept, so switching the kind in the app never loses what you set up.

```yaml title="servers/Users API.yaml"
name: Users API
kind: http
seq: 0
host: 0.0.0.0
port: 3443
tls:
  enabled: true
  certPath: certs/dev.pem
  keyPath: certs/dev-key.pem
autoStart: true
http:
  routes:
    - method: GET
      path: /health
      body: ok
```

| Field | Default | Description |
|---|---|---|
| `name` | | The server's name. |
| `kind` | `http` | `http`, `websocket`, `sse`, `tcp`, `udp`, `dns` or `tcpProxy`. |
| `seq` | `0` | Position in the sidebar. |
| `host` | `127.0.0.1` | Address to listen on. |
| `port` | `0` | Port; `0` means any free port. |
| `tls.enabled` | `false` | Listen with TLS. |
| `tls.certPath` | empty | PEM certificate chain (relative to the workspace, or absolute). |
| `tls.keyPath` | empty | PEM private key. |
| `autoStart` | `false` | **Start with workspace**. |
| `http` | | [Mock API settings](../mock-api/#saved-format). |
| `websocket`, `sse` | | [WebSocket and event stream settings](../websocket-and-sse-servers/#saved-format). |
| `socketio` | | [Socket.IO settings](../socketio-server/#saved-format). |
| `socket` | | [TCP and UDP settings](../tcp-udp-dns-servers/#saved-format). |
| `dns` | | [DNS settings](../tcp-udp-dns-servers/#saved-format). |
| `proxy` | | [Relay settings](../relay/#saved-format). |
| `docs` | empty | Notes about the server. |

See also [Workspace format](../../reference/workspace-format/).

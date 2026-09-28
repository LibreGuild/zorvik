---
title: Troubleshooting
description: Fixes for common problems with Zorvik, from blocked installs and TLS errors to proxies, busy ports, load tests and AI agents that can't connect.
sidebar:
  order: 1
---

Most error messages in Zorvik say what to do. This page collects the common ones, grouped by topic. If yours isn't here, look at the [log file](../../reference/data-locations/#logs) and ask in [Discussions](https://github.com/LibreGuild/zorvik/discussions) or open an [issue](https://github.com/LibreGuild/zorvik/issues/new/choose).

## Installing and opening

### Windows: "Windows protected your PC"

The builds are not code-signed yet, so SmartScreen may stop the installer or the portable app the first time. Choose **More info**, then **Run anyway**. It asks once.

### macOS: "Zorvik can't be opened" or "Apple could not verify…"

The first open of an unsigned app is blocked by Gatekeeper. Open **System Settings → Privacy & Security**, scroll to the message about Zorvik and choose **Open Anyway**. Or, in a terminal:

```bash
xattr -dr com.apple.quarantine /Applications/Zorvik.app
```

Move Zorvik to **Applications** before you open it. Run from the disk image (or a quarantined copy), the app can't give a stable path to its `zorvik` command line, so AI agent setup and **Add zorvik to PATH…** don't work.

### Linux: the AppImage doesn't start

Make it executable first: `chmod +x Zorvik-Linux-x86_64.AppImage`, then run it. The AppImage has no `zorvik` command line; install the `.deb` or `.rpm` to use `zorvik` in a terminal or with AI agents.

### `zorvik: command not found`

| System | Fix |
|---|---|
| Windows (installer) | Open a **new** terminal: the installer added `zorvik` to your PATH. |
| Windows (portable) | Use the full path of `zorvik.exe` in the unzipped folder, or add that folder to your PATH. |
| macOS | Settings → AI agents → **Add zorvik to PATH…** (links `/usr/local/bin/zorvik`), then open a new terminal. |
| Linux | Install the `.deb` or `.rpm` (they put `zorvik` in `/usr/bin`). |

## Workspaces

| Message | Meaning and fix |
|---|---|
| "*folder* is not a Zorvik workspace (no zorvik.yaml)" | Pick the folder that contains `zorvik.yaml`, or create a workspace there. |
| "This workspace was created by a newer Zorvik (format v*N*); please update the app" | Update Zorvik (see [Install](../../getting-started/install/)). |
| "zorvik.yaml has an invalid id" | The `id` must be up to 64 letters, digits, `-` or `_`. Fix it, or set it to `id: ""` to get a new one (secrets saved on this computer for the old id are then no longer found). |
| A request or folder shows a warning in the sidebar | Its YAML doesn't parse, often after a Git merge conflict. Hover for the error, fix the file, and Zorvik picks it up. |
| "… goes through a symbolic link, which is not followed" | Zorvik doesn't follow symbolic links inside a workspace. Use a real folder or file. |

Deleted something by mistake? Requests, folders, environments, servers and load tests go to the system **trash** (Recycle Bin), where you can restore them. (A load test's run history is deleted with it.)

## Requests

### TLS errors

"TLS handshake with *host* failed: …" ends with a hint:

| Hint | Cause | Fix |
|---|---|---|
| "The certificate is not trusted." | The server's certificate comes from an authority your OS doesn't trust: a corporate TLS-inspection CA, an internal CA, or a self-signed certificate. | Install the CA in your OS trust store, or add its PEM file in **Settings → Certificates → Extra CA certificate**. |
| "The certificate does not match the host name." | You connect by a name (or IP address) the certificate isn't for. | Use the name in the certificate. |
| "The certificate has expired." | Exactly that. | Renew it on the server. |

Zorvik trusts the operating system's certificate store (Windows certificate store, macOS keychain), so corporate CAs installed by IT already work. As a last resort, for servers you trust: turn off **Verify TLS certificates** in Settings → Requests, or only for one request in its settings (`verifyTls: false`), or pass `-k` to `zorvik`.

Other certificate messages: "No certificates found in … (expected PEM)" and "Invalid PEM in …" mean the file is not a PEM certificate (convert DER with `openssl x509 -inform der -in cert.cer -out cert.pem`). "Client certificate and client key must both be set for mutual TLS" means only one of the two is filled in. See [TLS and certificates](../../requests/tls-and-certificates/).

### Proxies

| Symptom | Fix |
|---|---|
| Requests hang or fail inside a corporate network | Settings → Proxy → **Use system proxy** reads `HTTPS_PROXY` / `HTTP_PROXY` / `NO_PROXY`, then the OS settings. If the OS uses a PAC script, find the proxy it picks and set it under **Manual**. |
| "Proxy scheme 'socks5' is not supported; use an http:// proxy" | Only HTTP proxies (`http://host:port`) are supported: no SOCKS. |
| The proxy answers 407 | Put `user:password@` in the manual proxy URL (Basic auth). NTLM and Kerberos proxy authentication are not supported. |
| A local service goes through the proxy | `localhost` is always direct; add other internal hosts to **Bypass** (for example `*.corp.local, 10.0.0.0/8`). |
| HTTP/3 fails behind a proxy | HTTP/3 (QUIC over UDP) never uses a proxy. Use HTTP/2 or HTTP/1.1 there. |

`zorvik` on the command line doesn't read the app's settings: it uses the system proxy (environment variables, then the OS settings). Set `HTTPS_PROXY` in CI. See [Proxies](../../requests/proxies/).

### Timeouts

"Request timed out after 60s", "Connection timed out after 15s", "TLS handshake timed out after 15s" and "DNS lookup for '*host*' timed out after …" name the limit that was hit. Raise **Request timeout** or **Connect timeout** in Settings → Requests, or the request's own timeout. `0` means no request time limit.

### Variables

- "{{baseUrl}} in the URL is not defined. Select an environment that defines it, or add it under Environments. Names are case-sensitive." Pick the right environment in the environment menu, or define the variable.
- A `{{name}}` sent literally: the variable is undefined (the request editor highlights it). Names are case-sensitive.
- Secret values are empty on another computer: secret values stay on the computer where they were typed. Enter them again in **Environments** (<kbd>Ctrl</kbd>/<kbd>⌘</kbd> <kbd>E</kbd>), or pass them to `zorvik` with `--var name=value`. See [Secret variables](../../variables/secrets/).

### Files

"… is outside the workspace folder. Move it into the workspace, or allow files outside the workspace in Settings → Data & privacy." A body file, data file or `.proto` file lies outside the workspace (links that lead out count too). Move it into the workspace (best, so it travels with it), or turn on **Files outside the workspace**; on the command line use `--allow-outside-files`. During an AI agent's call, files outside the workspace are never used.

### Other

| Symptom | Fix |
|---|---|
| "The script took longer than 5 s and was stopped" | Raise **Script time limit** in Settings → Requests (up to 60 s), or make the script do less. |
| "Only the first part is shown" | The response is too big to show in full (text over 10 MB). **Save to file…** saves the whole body. |
| A response body is cut even when saved | It is larger than **Max response size** (100 MB by default) in Settings → Requests. Raise it (up to 2048 MB). |
| "The data file is not UTF-8 text (line *n*)" | Save the CSV as UTF-8 (in Excel: "CSV UTF-8"). |

## Servers and ports

| Message | Fix |
|---|---|
| "Port 3000 is already in use by node (PID 4242). Stop it or pick another port." | Another program listens on that port; the message names it when the OS says which. Stop it, or change the server's port (0 = any free port). `zorvik serve` takes `--port`. |
| "Port 3000 is already in use by another server in Zorvik." | Another Zorvik server (maybe of another workspace: running servers keep going when you switch) uses it. Stop it in the Servers sidebar. |
| "Not allowed to listen on port 80 (ports below 1024 may need administrator rights)." | Use a port of 1024 or above. |
| "This computer has no address … Use 127.0.0.1 or 0.0.0.0." | The server's host is not an address of this computer. |
| "*server*: not started automatically because it is new or was changed outside Zorvik (e.g. by a Git pull). Start it once to let it start with the workspace." | Servers from Git don't open ports by themselves. Start it once by hand; after that, **Start with workspace** works for that exact configuration. |
| Other devices can't reach the server | It listens on `127.0.0.1` (this computer only) by default. Set the host to `0.0.0.0`, and allow the port in the firewall. |

## Load tests

| Message | Fix |
|---|---|
| "A load test is already running. Stop it first." | One load test runs at a time in the whole app. Stop the other one (the title bar pill). |
| "*name* is not an HTTP request: only HTTP requests can be load tested" | Remove the WebSocket, gRPC, TCP … request from the test. |
| "HTTP/3 isn't supported for load tests yet" | The app's HTTP version is HTTP/3. Set the test's **HTTP version** to Auto, HTTP/1.1 or HTTP/2. |
| "Request '*path*' can't be loaded: …" | The target request was deleted or its file is broken. |
| "capture '*x*': invalid regular expression …" or "… is not a JSON path …" | Fix the capture (see [Captures](../../load-testing/data-and-captures/#captures)). |
| "*host* is not a host this run was started for" (as `notAllowed` errors) | A captured value changed a request's host. Load tests only send to hosts known when they start. |
| The generator CPU warning (over 85 %) | Your computer is the bottleneck. Lower the load, or run `zorvik load` on a bigger machine. |
| Many `connect` errors with keep-alive off, especially on Windows | Every request opened a new connection and the local ports ran out. Turn **Reuse connections** back on. |
| Many **dropped** requests (request rate) | The server can't keep up with the rate, or **Max in flight** is too low. |
| A threshold shows "no data" | No request completed for it (for example, every request failed to connect), or it names a request the test doesn't send. |

See [Load testing](../../load-testing/overview/).

## AI agents

| Symptom or message | Fix |
|---|---|
| The agent doesn't list Zorvik's tools | Check the command you registered (Settings → AI agents → **Connect an agent** shows it with the full path). Run `zorvik mcp` in a terminal: it should wait silently for input (press Ctrl+C to quit). "command not found" means the path is wrong. |
| "Zorvik is not running and could not be started (…). Ask the user to open Zorvik." | `zorvik mcp` could not find the app next to itself (for example "the Zorvik app is not in …"). Open Zorvik yourself, then try again. |
| "Zorvik did not start within 30 seconds. Ask the user to open it." | Open Zorvik, then try again. |
| "The user did not allow AI agents in Zorvik (Settings → AI agents turns it on)." | Turn on **Allow AI agents**, or answer **Allow AI agents** when asked. After a "no", the same agent isn't asked again for a minute. |
| "The user disconnected this agent in Zorvik. …" | You disconnected it. Restart the agent's MCP server (usually: restart the agent) to reconnect. |
| "… needs the user's approval in the Zorvik app, which is not open." | **Work without the app** is on and Zorvik is closed. Open Zorvik, or change the setting. |
| "No answer from the user in Zorvik in time; nothing was done." | Approval questions expire after 55 seconds. Ask the agent to try again and answer in the Zorvik window. |
| "No workspace is open in Zorvik. …" | Open a workspace, or let the agent call `open_workspace` (you approve it). |
| "Zorvik stopped a request to *host* …" | A redirect, script or token URL led to a host you hadn't approved. Ask the agent to send again; Zorvik then asks about that host. |
| "WEBSOCKET requests are live sessions, which agents can't use yet …" | Agents can send HTTP, GraphQL, gRPC (unary), DNS and SSE requests only. |
| Nothing happens and no question appears | Questions queue one at a time per agent; look for the Zorvik window (it may be behind others) and the title bar pill. |

The app writes "AI agents can't connect: …" to its [log](../../reference/data-locations/#logs) when it could not start listening for `zorvik mcp`. `zorvik mcp` itself prints its problems to standard error; most agents show that output in their MCP server logs.

Two environment variables change where `zorvik mcp` looks, mainly for testing: `ZORVIK_DATA_DIR` (the app data folder that holds `agent.json` and `settings.json`) and `ZORVIK_APP` (the app executable to start instead of the one installed next to `zorvik`).

See [Connect an AI agent](../../agents/setup/) and [Permissions and safety](../../agents/permissions-and-safety/).

## Updates

- **Settings → Updates says "Could not reach GitHub"**: Zorvik is offline, or a proxy or firewall blocks `github.com`. Set the proxy in **Settings → Proxy** and click **Check for updates** again, or download the new version by hand from [Releases](https://github.com/LibreGuild/zorvik/releases/latest).
- **"This copy doesn't update itself"**: the portable zip, the `.deb` and `.rpm` packages and a macOS copy opened from the disk image can't replace themselves. Install the new version the same way as the first one (on macOS, move Zorvik to `/Applications` first).
- **The notice says a new version is out, with Open downloads, although automatic updates are on**: this time the update couldn't install itself (an interrupted download, a network filter, or a folder Zorvik can't write to). Download it from the release page it opens; the reason is in the [log file](../../reference/data-locations/#logs).
- **An update was downloaded but nothing changed**: it installs when you quit Zorvik, or with **Restart now** in the notice or in **Settings → Updates**.

See [Updates](../../getting-started/updates/) for how it works and what is sent.

## Still stuck?

- The [log file](../../reference/data-locations/#logs), with `RUST_LOG=debug` for more detail.
- [FAQ](../faq/).
- Security problems: report them privately, see [SECURITY.md](https://github.com/LibreGuild/zorvik/blob/main/SECURITY.md).

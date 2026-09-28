---
title: Settings
description: Every setting in Zorvik's Settings window, its key in settings.json, its default and what it does.
sidebar:
  order: 2
---

Open **Settings** with the gear in the title bar or <kbd>Ctrl</kbd>/<kbd>⌘</kbd> <kbd>,</kbd>. Settings belong to your computer, not to a workspace: they are saved in `settings.json` in the app's data folder (see [Data locations](../data-locations/)) and apply to every workspace.

Changes apply when you press **Save**, except **Zoom** and **Response position**, which apply (and are kept) at once. Font changes preview while the window is open and go back if you close it without saving. The footer shows the Zorvik version (and, for nightly builds, the build).

A missing or unreadable `settings.json` means defaults. The file can be partial: missing keys take their defaults.

## General

| Setting | Key | Default | What it does |
|---|---|---|---|
| Theme | `theme` | `system` | `system` (match the OS), `dark` or `light`. The theme button in the title bar cycles through them too. |
| Response position | (window) | Beside the request | Whether responses show **beside** or **below** the request. Kept by the app window, not in `settings.json`. |
| Zoom | `appearance.zoom` | `100` | Page zoom in percent: 50, 67, 75, 80, 90, 100, 110, 125, 150, 175 or 200. <kbd>Ctrl</kbd>/<kbd>⌘</kbd> <kbd>+</kbd> and <kbd>−</kbd> step through them; <kbd>0</kbd> resets. |
| Interface font | `appearance.uiFont` | `""` (system font) | Font of menus, lists, forms and tabs. A font that isn't installed says so and falls back to the default. |
| Code font | `appearance.codeFont` | `""` (system monospace) | Font of editors, response bodies, headers and other code. |
| Code text size | `appearance.codeFontSize` | `13` | In editors and response bodies, in pixels before zoom: 10, 11, 12, 13, 14, 15, 16, 18, 20, 22 or 24. |
| Ligatures | `appearance.ligatures` | `false` | Draw `=>`, `!=`, `>=` as single symbols with code fonts that have them (Fira Code, JetBrains Mono, Cascadia Code). |

## Requests

Defaults for every request. A request can override most of them in its own settings (see [Request settings](../workspace-format/#request-settings)).

| Setting | Key | Default | What it does |
|---|---|---|---|
| Request timeout | `request.timeoutMs` | `60000` | Time limit of the whole request, in milliseconds. `0` = no limit. |
| Connect timeout | `request.connectTimeoutMs` | `15000` | Time limit of DNS, TCP and TLS, in milliseconds. At least 100. |
| Follow redirects | `request.followRedirects` | `true` | Follow 3xx redirects. `Authorization` and `Cookie` are dropped when a redirect changes origin. |
| Max redirects | `request.maxRedirects` | `10` | Most redirects followed (0 to 100 in the window). |
| Verify TLS certificates | `request.verifyTls` | `true` | Check server certificates against the OS trust store and the extra CA. Turn off only for testing against servers you trust. |
| HTTP version | `request.httpVersion` | `auto` | `auto` (HTTP/2 when the server offers it, else HTTP/1.1), `http1` (HTTP/1.1 only), `http2` (HTTP/2 only) or `http3` (QUIC: `https://` only, never through a proxy). |
| Decompress responses | `request.decompress` | `true` | Decode gzip, deflate, br and zstd bodies. |
| Default headers | `request.sendDefaultHeaders` | `true` | Send `User-Agent`, `Accept` and `Accept-Encoding` when the request doesn't set them. |
| Max response size | `request.maxResponseMb` | `100` | Larger response bodies are cut, in MB (1 to 2048). |
| Script time limit | `scriptTimeoutMs` | `5000` | Time limit of each pre-request or post-response script, in milliseconds (100 to 60,000). |

Load tests use these settings too, except that a load test's own **Timeout** and **HTTP version** win, and HTTP/3 is not supported there (see [Load testing](../../load-testing/overview/#how-requests-are-sent)).

## Proxy

| Setting | Key | Default | What it does |
|---|---|---|---|
| Proxy | `proxy.mode` | `system` | `system`: the `HTTPS_PROXY` / `HTTP_PROXY` / `NO_PROXY` environment variables, then the OS settings (Windows Internet Options, macOS Network). `none`: connect directly. `manual`: the proxy below. |
| Proxy URL | `proxy.url` | | Manual only: `http://host:port`, with optional `user:password@` for Basic auth. Only `http://` proxies are supported. |
| Bypass | `proxy.bypass` | | Manual only: hosts that go direct, comma separated, e.g. `*.corp.local, 10.0.0.0/8, <local>`. |

`localhost` always goes direct. PAC scripts, SOCKS proxies and NTLM or Kerberos proxy authentication are not supported. See [Proxies](../../requests/proxies/).

```json title="settings.json (proxy only)"
{ "proxy": { "mode": "manual", "url": "http://proxy.corp.local:8080", "bypass": "*.corp.local" } }
```

## Certificates

| Setting | Key | Default | What it does |
|---|---|---|---|
| Extra CA certificate | `tls.caCertPath` | `""` | A PEM file trusted in addition to the OS trust store, for internal or self-signed certificate authorities. Corporate CAs installed in the OS are already trusted. |
| Client certificate | `tls.clientCertPath` | `""` | PEM certificate for mutual TLS. |
| Client key | `tls.clientKeyPath` | `""` | PEM private key of the client certificate. Set both or neither. |

See [TLS and certificates](../../requests/tls-and-certificates/).

## Data & privacy

| Setting | Key | Default | What it does |
|---|---|---|---|
| Cookie jar | `cookieJar` | `true` | Keep cookies from responses and send them on later requests, per workspace. |
| Files outside the workspace | `filesOutsideWorkspace` | `false` | Let requests upload body files, and runs and load tests read data files, from anywhere on this computer. Off: only files inside the workspace folder, so a shared workspace can't send your private files. Always off during an AI agent's call. |
| History size | `historyLimit` | `500` | History entries kept per workspace (10 to 100,000 in the window). |
| App data folder | (read only) | | Where settings, history, cookies, OAuth tokens and secret values are kept on this computer. |

## Updates

See [Updates](../../getting-started/updates/).

| Setting | Key | Default | What it does |
|---|---|---|---|
| This version | (read only) | | The version, where the update stands, and **Check for updates**. |
| Updates | `updates.mode` | `automatic` | `automatic` (check, download in the background, install on restart or quit), `notify` (check and tell you, download when you click) or `off` (never check by itself). |
| Channel | `updates.channel` | `stable` | `stable` (versioned releases) or `nightly` (a daily build of the newest code). |

The update check reads one file from the project's releases on GitHub and sends nothing about you.

## AI agents

See [Connect an AI agent](../../agents/setup/) and [Permissions and safety](../../agents/permissions-and-safety/).

| Setting | Key | Default | What it does |
|---|---|---|---|
| Allow AI agents | `agents.enabled` | `false` | Let agents connected through `zorvik mcp` use Zorvik. When off, an agent's first action asks you to turn it on. |
| Edits by agents | `agents.changes` | `allow` | `allow` or `ask`: whether creating and changing requests, folders, environments, load tests, servers and files asks first. Deleting always asks. |
| Requests sent by agents | `agents.traffic` | `askOutside` | `askOutside` (ask once per host outside this computer and private networks), `ask` (ask about every host) or `allow`. Covers requests, collection runs and schema downloads. Load tests and servers always ask. |
| Follow agents | `agents.follow` | `true` | Open what an agent works on: its requests, their responses, runs and load tests. |
| Work without the app | `agents.headless` | `false` | When Zorvik is closed, agents use it in the background instead of opening it; actions that would ask are refused. |
| Command-line tool | (read only) | | Where `zorvik` is and whether it is on PATH. On macOS, **Add zorvik to PATH…** links it into `/usr/local/bin` (asks for your password). |
| Connect an agent | (read only) | | The setup commands for Claude Code, Codex, Gemini CLI and other MCP clients, with this computer's path to `zorvik`. |

When an agent is allowed through the "Allow AI agents?" question, `agents.enabled` is turned on for you.

## A complete `settings.json`

With every default:

```json title="settings.json"
{
  "theme": "system",
  "appearance": { "zoom": 100, "uiFont": "", "codeFont": "", "codeFontSize": 13, "ligatures": false },
  "request": {
    "timeoutMs": 60000,
    "connectTimeoutMs": 15000,
    "followRedirects": true,
    "maxRedirects": 10,
    "verifyTls": true,
    "httpVersion": "auto",
    "decompress": true,
    "maxResponseMb": 100,
    "sendDefaultHeaders": true
  },
  "proxy": { "mode": "system" },
  "tls": { "caCertPath": "", "clientCertPath": "", "clientKeyPath": "" },
  "historyLimit": 500,
  "cookieJar": true,
  "filesOutsideWorkspace": false,
  "scriptTimeoutMs": 5000,
  "agents": { "enabled": false, "changes": "allow", "traffic": "askOutside", "follow": true, "headless": false },
  "updates": { "mode": "automatic", "channel": "stable" }
}
```

Edit the file only while Zorvik is closed: the app writes the whole file when you save in the Settings window.

## Workspace settings

Settings that belong to a workspace are in its files and travel with it through Git:

- **Workspace settings** (workspace menu): name, default auth, default headers and scripts, saved in [`zorvik.yaml`](../workspace-format/#zorvikyaml).
- **Folder settings** (right-click a folder): auth, headers, scripts and docs, saved in [`_folder.yaml`](../workspace-format/#folders-_folderyaml).
- **Environments** (<kbd>Ctrl</kbd>/<kbd>⌘</kbd> <kbd>E</kbd>): environments and workspace variables.

## The command line

`zorvik run`, `zorvik load` and `zorvik serve` don't read `settings.json`. They use the defaults above (system proxy, OS trust store, 60 s timeout, no extra CA or client certificate) plus their own flags, such as `-k` / `--insecure` to skip TLS verification and `--allow-outside-files`. See [`zorvik load`](../../cli/load/).

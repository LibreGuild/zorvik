---
title: zorvik mcp
description: The MCP server AI agents start to control the Zorvik app, how to register it with Claude Code, Codex, Gemini CLI and other clients, and how it connects to the app.
sidebar:
  order: 5
---

`zorvik mcp` is the [Model Context Protocol](https://modelcontextprotocol.io/) server for AI coding agents such as Claude Code, Codex, Gemini CLI and Cursor. You don't run it yourself: you register it with your agent once, and the agent starts it when it needs Zorvik. Through it, the agent controls the **Zorvik app**, and you watch and approve what it does in the app's window.

```text
zorvik mcp
```

See [AI agents](../../agents/setup/) for what agents can do, the permissions you can set, and the tools they get.

## Options

| Option | Value | Description |
|---|---|---|
| `--data-dir` | `<DATA_DIR>` | The app's data folder, instead of where the installed app keeps it. Hidden from `--help`: it's meant for testing. Wins over `ZORVIK_DATA_DIR`. |
| `-h`, `--help` | | Print help |

## Register it with your agent

Run the command for your agent once. **Settings → AI agents → Connect an agent** in the app shows these commands with the right path for your computer.

```bash
claude mcp add --scope user zorvik -- zorvik mcp       # Claude Code
codex mcp add zorvik -- zorvik mcp                     # Codex
gemini mcp add --scope user zorvik zorvik mcp          # Gemini CLI
```

For Cursor, Windsurf, VS Code and other MCP clients, add this to their MCP settings:

```json
{ "mcpServers": { "zorvik": { "command": "zorvik", "args": ["mcp"] } } }
```

These use the name `zorvik`, so the command must be on your `PATH` (see [Install](../overview/#install)). If it isn't, use the full path to the command line instead, for example `/Applications/Zorvik.app/Contents/MacOS/zorvik` on macOS.

## How it works

```text
Agent ──MCP over stdio──► zorvik mcp ──127.0.0.1 + token──► Zorvik app
```

1. **The agent starts `zorvik mcp`** and talks to it on standard input and output: JSON-RPC 2.0 messages, one per line. Nothing else is written to standard output; problems go to standard error.
2. **The handshake is answered right away**, without the app: `initialize`, `ping`, the tool list (`tools/list`) and the prompts (`prompts/list`, `prompts/get`). Starting an agent never starts Zorvik.
3. **On the first tool call**, `zorvik mcp` connects to the running app. The app listens on `127.0.0.1` on a random port and writes the port and a token to `agent.json` in its data folder, readable by your user only. Both sides prove they know the token without ever sending it.
4. **If the app isn't running**, `zorvik mcp` starts it and waits up to 30 seconds for it, unless headless use is on (below).
5. **Tool calls go to the app**, which runs them with your permissions and shows them live. Answers come back the same way.
6. **When the agent closes the connection** (ends standard input), `zorvik mcp` exits.

The first time an agent calls a tool while **Settings → AI agents → Allow AI agents** is off, the app asks you to turn it on.

### Working without the app

With **Settings → AI agents → Work without the app** on (and **Allow AI agents** on), `zorvik mcp` doesn't open the app when it's closed: it runs the tools itself, in the background, on the last workspace you had open. Nothing can be approved in that mode, so every action that would ask you is refused. When you open the app, it takes over from the next tool call.

### Where it finds the app

| What | Where |
|---|---|
| The app's data folder | macOS: `~/Library/Application Support/org.libreguild.zorvik`. Windows: `%APPDATA%\org.libreguild.zorvik`. Linux: `~/.local/share/org.libreguild.zorvik` (or under `$XDG_DATA_HOME`). |
| The app it starts | macOS: the `Zorvik.app` bundle `zorvik` is part of (or the installed app, found by its identifier). Windows and Linux: `zorvik-desktop` in the same folder as `zorvik`. |

Two environment variables change these, mainly for testing:

| Variable | Effect |
|---|---|
| `ZORVIK_DATA_DIR` | Use this folder as the app's data folder |
| `ZORVIK_APP` | Start this program instead of the installed app |

## Protocol details

| | |
|---|---|
| Transport | Standard input and output, one JSON-RPC 2.0 message per line (batches accepted) |
| Protocol versions | `2025-11-25`, `2025-06-18`, `2025-03-26`, `2024-11-05`. A client asking for another version gets `2025-11-25`. |
| Server name | `zorvik` (title `Zorvik`), with the command line's version |
| Capabilities | Tools and prompts. `resources/list` and `resources/templates/list` answer with empty lists. |
| Prompts | `map_apis` and `test_apis` |
| Largest message | 64 MB (a longer line is skipped with a parse error) |
| Cancelling | `notifications/cancelled` cancels a tool call in progress |

On connecting, the agent receives short instructions: start with `get_workspace`, the user watches and approves actions in the app, and hosts and credentials belong in `{{variables}}`.

## Messages agents may report

| Message | What to do |
|---|---|
| `Zorvik is not running and could not be started (…). Ask the user to open Zorvik.` | Open the app yourself. On Windows and Linux, the app must be next to `zorvik` (`zorvik-desktop`). |
| `Zorvik did not start within 30 seconds. Ask the user to open it.` | Open the app, then let the agent try again. |
| `Zorvik closed the connection; try again.` / `Zorvik closed before the call finished.` | The app was closed or restarted during a call. |
| `The user disconnected this agent in Zorvik. Stop using Zorvik unless the user asks you to reconnect (restart this MCP server).` | You disconnected the agent in the app. Restart the agent's MCP server to reconnect. |

## Exit codes

| Code | Meaning |
|---|---|
| `0` | The agent closed the connection |
| `1` | No data folder could be determined for this system, or reading or writing standard input/output failed (the reason is on standard error, starting with `zorvik mcp:`) |

## Check that it works

You can talk to it by hand. It answers the handshake and the tool list without the app:

```bash
printf '%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test"}}}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' \
  | zorvik mcp
```

The first answer has `"serverInfo":{"name":"zorvik",…}`, the second the list of tools. `zorvik mcp` exits when its input ends.

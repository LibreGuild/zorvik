---
title: Permissions and safety
description: What AI agents may do in Zorvik on their own, what always asks you, how host approvals and redaction work, and what headless mode changes.
sidebar:
  order: 3
---

An agent works with your real workspace, your network and your secrets, so Zorvik checks every tool call in its Rust core, whatever the agent or the MCP client claims. Reading is allowed; edits and requests follow your settings; anything that is hard to undo or reaches beyond your computer asks you in the Zorvik window.

## Turning agents on

Agents are off until you allow them (**Settings → AI agents → Allow AI agents**). While it is off, the first tool call of an agent shows:

> **Allow AI agents?** Claude Code wants to control Zorvik: read and edit your collection, send requests and run tests. Deletes, load tests and servers always ask first; Settings → AI agents sets what else does.

**Allow AI agents** turns the setting on for every agent. If you decline (or don't answer), the agent is told "The user did not allow AI agents in Zorvik (Settings → AI agents turns it on)." and that same agent is not asked again for a minute.

## What is allowed, what asks

| Action | Tools | Default | Setting |
|---|---|---|---|
| **Reading** the workspace, variables, history, server traffic, run and load test status | `get_workspace`, `list_*`, `read_*`, `get_variables`, `read_history`, `get_server_traffic`, `get_run_status`, `get_load_test_status`, `export_request`, `open_in_app` | Allowed | none |
| **Edits**: requests, folders, environments (and switching the active one), imports, load tests, servers, mocks, files added with `write_file` | `save_requests`, `save_folder_settings`, `move_item`, `save_environment`, `set_active_environment`, `import`, `update_from_openapi`, `save_load_test`, `save_server`, `create_mock`, `write_file` | Allowed | **Edits by agents**: *Allow* or *Ask me* |
| **Requests** to other systems: sends, collection runs, GraphQL and gRPC schema lookups, MCP catalogs, downloads for imports and mocks | `send_request`, `run_collection`, `graphql_schema`, `grpc_describe`, `mcp_catalog`, and `import` / `update_from_openapi` / `create_mock` with a URL | Asks for outside hosts | **Requests sent by agents**: *Ask for outside hosts*, *Ask every time* or *Allow* |
| **Deleting** (to the trash) | `delete_items` | Always asks | none |
| **Running a load test** | `run_load_test` | Always asks | none |
| **Starting a server** (any port) | `start_server` | Always asks | none |
| **Starting a program** (an MCP server over stdio) | `send_request` and `mcp_catalog` with an MCP request whose address is a command | Always asks, every time | none |
| **Opening or creating a workspace** | `open_workspace` | Always asks | none |
| **Reading a file outside the workspace** | `import`, `update_from_openapi` and `create_mock` with an absolute `file` path | Always asks | none |
| **Stopping** a run, load test or server | `stop_collection_run`, `stop_load_test`, `stop_server` | Allowed | none |

With **Edits by agents** set to *Ask me*, each change shows what will change ("Save 3 requests?", "Update load test “Smoke”?", "Add a file in the workspace?") and offers **Allow for this session**: after that, the agent's edits no longer ask until it disconnects.

## Host approvals

The **Requests sent by agents** setting decides which hosts need your approval:

| Setting | Asks about |
|---|---|
| **Ask for outside hosts** (default) | Hosts outside this computer and your private networks. `localhost`, private and link-local addresses, `*.local`, `*.home.arpa` and `*.internal` go without asking (the same list as [load tests](../../load-testing/overview/#safety-hosts-outside-your-computer)). |
| **Ask every time** | Every host, local ones included. |
| **Allow** | Nothing. |

Before a call sends anything, Zorvik works out the hosts it will reach (the request's URL and, for OAuth 2.0, the token URL; for DNS queries, the resolver and the name looked up, since resolvers pass it on) and asks once for all of them, for example "Send GET {{baseUrl}}/users?" with "Claude Code wants to send requests to:" and the host list. **Send** allows them for this call; **Allow for this session** allows those hosts for the rest of the agent's session, so the same hosts don't ask again.

**Requests can't wander.** Every agent call starts with no network access at all, and then gets only the hosts your setting and your answers allow: the approved hosts, plus any local host with *Ask for outside hosts*, or every host with *Allow*. A host guard in the engine enforces it on everything the call sends: redirects, OAuth token requests, gRPC channels, DNS resolvers and collection runs (a run keeps its call's limits even after the call returns). A request that would reach another host (a redirect to a new domain, a script-built URL) is stopped, and the agent is told:

> Zorvik stopped a request to other.example.com (a script, redirect or token URL leads there, and the user hasn't approved it). Send it again to ask the user.

The stopped host is remembered, and the next call asks about it.

## Programs

An MCP request can name a program instead of a URL (`npx -y @modelcontextprotocol/server-filesystem ./docs`): sending it starts that program on your computer, with your permissions. So `send_request` and `mcp_catalog` ask **every time** before they start one, whatever the settings, even for programs you allowed in the app: "Claude Code wants to start this program on your computer (an MCP server). Allow it only if you know what it does:", listing the command, the folder it starts in and its environment (secret values as `{{name}}`), with a red **Start program** button. A program talks over stdio, so nothing goes to the network from Zorvik itself.

Only the program you were asked about starts: if a pre-request script or a variable changes the command, folder or environment afterwards, the send is refused. Collection runs started by an agent (`run_collection`) start no programs at all; those requests fail with a message saying so.

## Load tests

`run_load_test` always asks, even for local hosts. The dialog shows the peak ("Up to 50 virtual users for 60 s"), the target requests and every host, plus "Only load test systems you own or are allowed to test." when a host is outside; the confirm button is then red.

After you approve, the run may reach **exactly** the hosts shown. If the files changed while you decided and the resolved test now sends somewhere else, nothing starts: "The load test now sends to *host*, which the user did not approve; nothing was started".

## Files

- During an agent's call, requests, collection runs, load tests and gRPC `.proto` files may only use files **inside the workspace folder**, even when **Files outside the workspace** is on in Settings.
- A file outside the workspace is read only when the agent names it with an absolute path in `import`, `update_from_openapi` or `create_mock`, and you approve that file ("Import a file?", "Read").
- `write_file` writes only inside the workspace folder, up to 10 MB, never through a symbolic link and never into Zorvik's own files and folders: `zorvik.yaml`, `requests/`, `environments/`, `servers/`, `loadtests/` and `.git`. Replacing an existing file needs `overwrite: true`. Requests, environments, load tests and servers are saved with their own tools, which check every field.

## The approval dialog

Every question appears in the Zorvik window:

- The **title** says what the agent wants ("Delete 2 items?", "Run load test “Checkout smoke”?", "Start “Payments mock”?").
- The **list** says exactly what: the requests to save, the hosts, the load plan, the items to delete ("Load test Smoke (and its run history)"), the server's address.
- **Deny** has the keyboard focus, so an Enter or Space meant for something else says no. The other buttons become active after a short moment for the same reason.
- **Allow for this session** appears for edits and hosts only.
- The confirm button (**Allow**, **Send**, **Delete**, **Run load test**, **Start**, **Open**, **Create**, **Import**, **Read**) is red for risky cases: deletes, outside hosts, a load test against an outside host, a server listening on `0.0.0.0` ("Other devices on your network will be able to reach it").
- **Escape** or closing the dialog denies.
- A countdown shows how long the question stays open: after **55 seconds** without an answer it is refused (some agents give up on a call after 60 seconds, and an action approved after that would run although the agent had given up).
- Each agent asks one question at a time. Questions from several agents queue: "2 more waiting".

While an agent waits for you, it receives a progress message every 5 seconds ("Waiting for the user's answer in Zorvik"), so clients that extend their timeout on progress keep waiting.

What the agent is told:

| Outcome | Message to the agent |
|---|---|
| Denied | "The user declined this in Zorvik." |
| No answer in time | "No answer from the user in Zorvik in time; nothing was done. Ask the user before trying again." |
| Disconnected meanwhile | "The user disconnected this agent in Zorvik." |
| Headless | "*Action* needs the user's approval in the Zorvik app, which is not open. Ask the user to open Zorvik and try again." |

When it connects, every agent is also told that a declined action should not be retried another way, and to ask you instead. Declined actions show as **Not allowed** in the AI agents panel.

## Redaction

What goes back to the agent never contains your credentials:

- **Secret variables**: the value of every enabled secret variable (of the active environment and the workspace) is replaced by `{{name}}` wherever it appears: response bodies, headers, URLs, test results, console output and error messages. Values shorter than 4 characters are left alone (masking "a" everywhere would ruin the text).
- **Credential headers**: `Authorization`, `Proxy-Authorization`, `Cookie`, `Set-Cookie`, `X-Api-Key`, `Api-Key`, `Apikey`, `X-Auth-Token`, `X-Access-Token`, `X-Csrf-Token`, `X-Xsrf-Token` and `X-Amz-Security-Token` are shown as `••••••`. Their values (and the parts of them 8 characters or longer, such as the token of `Bearer <token>` or a cookie's value) are also hidden anywhere else in the result, URL-encoded forms included.
- **Variables**: `list_environments` and `get_variables` show secret values as `••••••`. `export_request` returns secret values as `••••••`; you can copy the full command in Zorvik (**Copy as cURL or code…**).
- **History** returned by `read_history` is redacted the same way.

The redaction runs last, on the finished result and on errors, as a safety net over each tool's own care.

Agents never see Zorvik's settings, the cookie jar, OAuth tokens or the secret store: no tool reads them.

## Headless mode

**Settings → AI agents → Work without the app** (off by default) lets agents use Zorvik while the app is closed. Instead of opening the app, `zorvik mcp` runs the tools itself, on the same data folder and the last workspace you opened.

- It applies only when **Allow AI agents** is also on, and it is checked on each call, so turning it off takes effect at once.
- Nothing can be approved without the window, so every action that would ask is **refused** with a message telling the agent to ask you to open Zorvik. With the default settings that means deletes, load tests, starting servers and programs, opening workspaces, reading outside files, and requests to outside hosts; edits too when **Edits by agents** is *Ask me*.
- When you open the app later, the next call moves to the app.
- The app's windows show nothing of what happened in headless mode (there was no window); the effects are in your files and history.

## Disconnecting and the connection itself

- **Disconnect** (AI agents panel or the title bar pill) cancels the agent's calls in progress, closes its open questions, ends its session approvals and refuses every later call until the agent restarts its MCP server.
- The app listens for `zorvik mcp` on `127.0.0.1` only, on a random port, and writes the port and a random token to `agent.json` in its data folder, readable only by your user (on macOS and Linux). A connection must prove it knows the token within 10 seconds, without sending it; at most 16 connections may be proving themselves at once. The app proves itself back, so a stale port reused by another program learns nothing.
- `agent.json` is removed when the app quits.

## For teams

The agent settings are per computer (in `settings.json` in the app data folder), not in the workspace. A workspace you clone can't turn agents on or loosen what asks. See [Settings](../../reference/settings/#ai-agents).

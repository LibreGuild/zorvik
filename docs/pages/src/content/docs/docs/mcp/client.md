---
title: MCP client
description: Connect to MCP servers over Streamable HTTP, HTTP+SSE or stdio, browse their tools, resources and prompts, make calls with your own arguments, test the answers and run them in CI.
sidebar:
  order: 2
---

An **MCP call** is a request that talks to an MCP server: it calls one of its **tools**, reads a **resource**, or gets a **prompt**. Like any request it is saved in your collection, sent with **Send**, tested with scripts, kept in history, and run by the collection runner and `zorvik run`.

## Create an MCP call

Click **+** at the end of the tab bar (or **New** in the Collection sidebar) and choose **MCP call (AI tools)**. The request has these tabs: **Call**, **Connection**, **Headers**, **Auth**, **Settings**, **Scripts** and **Docs**.

## The address

The URL bar holds where the server is:

| Address | Transport |
|---|---|
| `http://localhost:3000/mcp`, `https://mcp.example.com/mcp` | **Streamable HTTP**. When the server answers the first message with 400, 404 or 405 (servers of the 2024-11-05 protocol), Zorvik switches to the older **HTTP+SSE** transport by itself and says so. |
| `https://example.com/sse` | HTTP+SSE servers, the same way |
| `npx -y @modelcontextprotocol/server-everything`, `uvx mcp-server-git`, `./my-server --verbose` | **stdio**: Zorvik starts the program and exchanges JSON-RPC messages on its standard input and output |

The **Connection** tab can force a transport (**Streamable HTTP**, **HTTP+SSE**, or **Program (stdio)**). `{{variables}}` work in the address.

### Programs

A command is split like a shell does for simple cases: spaces separate words, `'…'` and `"…"` group them, and a backslash escapes a space or a quote. It is not run through a shell, so pipes and `&&` don't apply. In the **Connection** tab:

| Setting | Default | What it does |
|---|---|---|
| Folder | the workspace folder | Where the program starts, relative to the workspace folder. |
| Environment | none | Variables added to the program's environment. Values can use `{{variables}}`; put tokens in [secret variables](../../variables/secrets/). |

On macOS and Linux, Zorvik adds your login shell's `PATH` when you don't set one, so `npx`, `uvx` and `docker` are found even when Zorvik was started from the Dock. The program's standard error is its log: its lines show in the **Messages** tab. When you disconnect, Zorvik closes the program's input, waits two seconds, then stops it and anything it started.

:::caution[Zorvik asks before it starts a program]
A program runs with your permissions. The first time a request starts one, Zorvik shows the exact **command**, **folder** and **environment** and asks: **Allow and start**. Your answer is kept for this workspace on this computer (not in the workspace files), and Zorvik asks again when any of the three changes. Secret values show as `{{name}}` in the question. See [Safety](#safety).
:::

### Remote servers

For HTTP servers, the **Headers** tab and the **Auth** tab apply to every request of the session: a bearer token, an API key, or **OAuth 2.0** (most hosted MCP servers use OAuth 2.0 with the authorization code grant and PKCE: set its authorization and token URLs, client id and scopes in the Auth tab). Cookies from the cookie jar go along too, and the **Settings** tab's TLS verification, proxy and timeout apply.

## Connect and browse

Press **Connect** at the top of the result pane to open a **session**: Zorvik sends `initialize` (offering protocol version 2025-11-25 and older ones), then `notifications/initialized`, and lists what the server offers. The bar shows the server's name and version, the transport, the protocol version, and a program's process id.

The **Server** tab shows:
- the server's **instructions** for the model, if it sent any,
- its **tools** with their descriptions, **resources**, **resource templates** and **prompts** (with a filter box),
- its **capabilities**, session id and how long connecting took.

Click an item to call it: the **Call** tab takes its name, shows its description and arguments (from the input schema, with types and required ones marked), and starts the arguments from the schema when they're empty. **Refresh** lists everything again; when the server says a list changed (`notifications/…/list_changed`), Zorvik refreshes by itself.

**Disconnect** ends the session (an HTTP session is deleted with `DELETE`; a program is stopped). Closing the tab does the same.

## The call

| Setting | What it is |
|---|---|
| **Tool** | `tools/call` with the tool's name and the arguments |
| **Resource** | `resources/read` with the URI. For a template such as `users://{id}`, the `{parts}` are filled in from the arguments (`{"id": "42"}` reads `users://42`). |
| **Prompt** | `prompts/get` with the prompt's name; its arguments are sent as text |

**Arguments** are a JSON object and can use `{{variables}}`. **Start from the schema** replaces them with the tool's required arguments (all of them when none is required), each with its default, first allowed value, or an empty value of its type.

Press **Send** (<kbd>Mod</kbd>+<kbd>Enter</kbd>). While the tab's session is open to the same address and transport, the call goes over it and shows in its **Messages**; otherwise Send connects, calls and disconnects. A call waits for the request's timeout (**Settings**), or 60 seconds; when it gives up, Zorvik tells the server (`notifications/cancelled`).

## The answer

The **Result** tab shows the answer as an AI app would get it:

- **text** blocks (JSON in them pretty-printed), **images** and **audio**, **resource links** and **embedded resources**,
- **structured content**, for tools with an output schema,
- a resource's **contents** (text, or the size of binary data), a prompt's **messages** with their roles,
- a red note when a tool reports that it failed (`isError`): the call worked, the tool couldn't do it,
- a **JSON-RPC error** (the code, message and data) when the server refused the request itself, such as an unknown tool or bad parameters.

**JSON** shows the raw answer. The status is **OK** (200) for a result and **500** for a JSON-RPC error, so tests and runs can tell them apart; the time includes connecting when the call made its own connection.

The **Messages** tab lists every JSON-RPC message of the session with its time, direction, method (answers name the request they answer) and size; click one for its JSON. A program's standard error, notices and errors show there too. **Clear** empties it.

Zorvik answers the server's own requests: `ping`, and `roots/list` with no roots. It declines sampling and elicitation (`-32601`), since there's no model in Zorvik to ask.

## Tests and scripts

Scripts run around the call like for HTTP requests. In a post-response script, `pm.response.json()` is the answer: the **result** (`content`, `structuredContent`, `isError`, `contents`, `messages`) or, for a JSON-RPC error, `{"error": {"code", "message", "data"}}`.

```js
const answer = pm.response.json();
pm.test("the tool worked", () => pm.expect(answer.isError).to.not.eql(true));
pm.test("in NOK", () => pm.expect(answer.structuredContent.currency).to.eql("NOK"));
pm.test("a JSON-RPC error for unknown tools", () => pm.response.to.have.status(500));
```

A pre-request script can set variables the arguments use (`pm.variables.set("city", "Oslo")`).

## In runs, the CLI and for agents

- The **collection runner** and `zorvik run` make each MCP call in one go (connect, call, disconnect) and run its tests. Runs from the app start only programs you allowed; `zorvik run` starts programs only with [`--allow-programs`](../../cli/run/#allow-programs).
- **AI agents** can send MCP calls (`send_request`) and list what a server offers (`mcp_catalog`); you're asked before every program they start, and runs they start don't start programs. See [Agent tools](../../agents/tools/).
- Load tests send HTTP requests only.

## Safety

- Workspaces come from Git and are untrusted: a program named in one runs only after you allowed that exact command, folder and environment on your computer. The list is kept in `trusted-programs.json` in the [app data folder](../../reference/data-locations/).
- Tool descriptions and results are text from the server. Review a server's tools before you connect it to an AI app; text in them can try to steer a model (prompt injection).
- Secret values in headers, auth and the environment are hidden in history, logs and everything AI agents see.

## Saved format

```yaml title="requests/Forecast.yaml"
name: Forecast
kind: mcp
url: http://localhost:3004/mcp
mcp:
  call: tool
  name: get_weather
  arguments: '{"city": "{{city}}"}'
scripts:
  postResponse: |
    pm.test("sunny", () => pm.expect(pm.response.json().content[0].text).to.include("sunny"));
```

```yaml title="requests/Local files.yaml"
name: Local files
kind: mcp
url: npx -y @modelcontextprotocol/server-filesystem ./docs
mcp:
  call: resource
  name: file:///{path}
  arguments: '{"path": "docs/README.md"}'
  cwd: .
  env:
    - key: LOG_LEVEL
      value: debug
```

| Field | Default | Description |
|---|---|---|
| `kind` | | `mcp` |
| `url` | | The server's URL, or the command that starts it |
| `mcp.transport` | `auto` | `auto`, `streamableHttp`, `sse` or `stdio` |
| `mcp.call` | `tool` | `tool`, `resource` or `prompt` |
| `mcp.name` | | The tool or prompt name, or the resource URI (template) |
| `mcp.arguments` | `{}` | The arguments as JSON text |
| `mcp.cwd` | the workspace folder | Programs: the folder they start in, relative to the workspace |
| `mcp.env` | none | Programs: environment variables (`key`, `value`, `enabled`) |

:::tip[A server to talk to]
Zorvik runs MCP servers too: tools, resources and prompts you set up, over every transport. See [MCP servers](../servers/).
:::

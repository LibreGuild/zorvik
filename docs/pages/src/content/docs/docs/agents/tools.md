---
title: Tools
description: Every MCP tool Zorvik offers AI agents, what it does, its main inputs, and whether it asks you first.
sidebar:
  order: 2
---

`zorvik mcp` offers 43 tools. Agents read their full JSON schemas (every field, unit and placeholder) from `tools/list`; this page is the human overview. The **Asks** column says when you are asked, with the default settings (see [Permissions and safety](../permissions-and-safety/)):

- **no**: never asks.
- **edit**: an edit. Allowed by default; asks when **Edits by agents** is *Ask me*.
- **traffic**: sends network requests. Asks for hosts outside this computer and private networks by default (**Requests sent by agents**).
- **always**: always asks.

Every tool is annotated for the client: `readOnlyHint` (reads only), `destructiveHint` (only `delete_items`) and `openWorldHint` (talks to other systems).

## Conventions

- **Paths** of requests and folders are relative to the workspace's `requests/` folder, with `/` between folders, exactly as `list_requests` shows them: `Users/Get user.yaml`, `Users/Admin`. `""` means the whole collection.
- **Names** of environments, load tests and servers match without regard to case; their file id works too.
- **Long calls** (`run_collection`, `run_load_test`, the status tools) wait up to `waitSeconds` (default 45, at most 600) and then return a `runId` to poll. Some agents give up on a call after 60 seconds, hence the default.
- **Strict input**: `save_requests`, `send_request` (with `request`), `save_server` and `save_load_test` refuse fields they don't know, name the nearest known one ("did you mean `status`?") and save nothing, so a typo is never saved silently.
- **Redaction**: secret variable values come back as `{{name}}` and credential headers as `••••••`, in results and errors alike. See [Redaction](../permissions-and-safety/#redaction).
- **No workspace open**: tools that need one fail with a message telling the agent to ask you which folder to use and call `open_workspace`.

## Workspace

| Tool | What it does | Main inputs | Asks |
|---|---|---|---|
| `get_workspace` | The open workspace: name, folder, request and folder counts, environments and the active one, collection variable names, load test and server counts. With none open: the recent workspaces. The agent is told to call this first. | none | no |
| `open_workspace` | Open another workspace folder, or create a workspace there. | `path` (absolute folder), `create` (make one when the folder isn't a workspace), `name` (of a new workspace) | always |
| `open_in_app` | Show something in the Zorvik window: a request, a folder's runner, a load test, a server or the environments. It opens even when **Follow agents** is off. | one of `request`, `runner` (folder path), `loadTest`, `server`, `environments: true` | no |

## Requests and folders

| Tool | What it does | Main inputs | Asks |
|---|---|---|---|
| `list_requests` | Folders and requests of the collection or one folder, with method and URL. | `folder` | no |
| `read_request` | Everything saved in one or more requests: URL, query and path parameters, headers, body, auth, scripts, settings, docs, examples. | `path`, or `paths` (up to 50) | no |
| `save_requests` | Create or update up to 200 requests. Each goes into `folder` (created when missing) under its `name`; a request with that name there is updated. Only the fields given change, unless `replace: true`. Give `path` to update that exact request. | `requests[]`: `name`, `folder` or `path`, `kind`, `method`, `url`, `query`, `pathParams`, `headers`, `body`, `auth`, `scripts`, `settings`, `grpc`, `mcp`, `websocket`, `socketio`, `docs`, `examples`; `replace` | edit |
| `save_folder_settings` | Auth, headers, scripts and docs that requests in a folder inherit, merged into what is there. `folder: ""` is the collection, which also has variables (merged by key). | `folder`, `auth`, `headers`, `scripts`, `docs`, `variables` | edit |
| `move_item` | Move a request or folder to another folder and/or rename it. Load tests that send it follow. | `path`, `toFolder`, `newName` | edit |
| `delete_items` | Move requests, folders, environments, load tests or servers to the trash. Deleting a load test also deletes its run history. | `paths`, `environments`, `loadTests`, `servers` | always |
| `export_request` | A request as a ready-to-run command or code: cURL for bash (`curl`), Windows cmd (`curlCmd`) or PowerShell (`curlPowerShell`), or code: `javascript` (fetch), `javascriptAxios`, `python` (requests), `pythonHttpx`, `go`, `java`, `kotlin` (OkHttp), `swift`, `csharp`, `php`, `ruby`, `rust`, `dart`, `c` (libcurl), `powerShell`, `httpie` or `wget`. Comments at the top say what the code can't do that Zorvik does (answer a Digest challenge, sign each request). Variables are filled in unless `resolveVariables: false`; secret values come back as `••••••` (you can copy the full version in Zorvik). | `path` or `request` (+ `folder`), `format`, `resolveVariables` | no |

Request fields for `save_requests` and `send_request`:

| Field | Notes |
|---|---|
| `kind` | `http` (default), `grpc`, `dns`, `websocket`, `socketio`, `sse`, `tcp`, `udp`, `mqtt`, `mcp`. GraphQL is `http` with a `graphql` body. For `mcp`, `url` is the MCP server's URL or the command that starts it. |
| `method` | HTTP method (default GET). gRPC: `package.Service/Method`. DNS: the record type. |
| `url` | Full URL with `{{variables}}`; `:name` path segments take `pathParams`. gRPC: `grpc://` or `grpcs://`. DNS: the name. |
| `query` | `[{key, value, enabled, description}]`: enabled ones replace the URL's query string, disabled ones are kept switched off. |
| `pathParams` | `[{key, value, description}]` for `:name` segments. |
| `headers` | `[{key, value, enabled}]` (an object of name → value works too). |
| `body` | `type` (`none`, `json`, `text`, `xml`, `formUrlencoded`, `multipart`, `binary`, `graphql`) and `text`, `contentType`, `form`, `multipart`, `file` or `graphql {query, variables, operationName}`. |
| `auth` | `type` (`inherit`, `none`, `basic`, `bearer`, `apiKey`, `oauth2`, `oauth1`, `jwt`, `digest`, `ntlm`, `awsSigV4`, `hawk`, `akamaiEdgeGrid`, `asap`) and its fields, as in the [workspace files](../../requests/auth/). Secrets belong in secret variables (`{{awsSecretKey}}`). |
| `scripts` | `preRequest`, `postResponse` (Postman `pm` API). |
| `settings` | `timeoutMs`, `followRedirects`, `verifyTls`. |
| `grpc` | `protoFiles` (inside the workspace; empty: server reflection). |
| `websocket` | `subprotocols` (a list, the preferred first), `messages` (`[{name, format, text}]`: saved messages; `format` is `text`, `json` or `binary` for hex bytes), `onConnect` (steps: `{type: "send", message}`, `{type: "wait", match, pattern, timeoutMs}` with `match` `any`, `contains`, `exact`, `regex` or `json`, `{type: "repeat", message, everyMs}`), `replyKey` (default `$.id`). See [WebSocket](../../protocols/websocket/#saved-format). |
| `socketio` | `path`, `transport` (`auto`, `websocket`, `polling`), `auth` (JSON text), `event` and `ack` (what the composer emits), `messages` (`[{name, event, ack, format, text}]`: saved emits; `format` `text` is one string argument, `json` the arguments, `binary` hex bytes), `onConnect` (as for `websocket`; a wait also takes `event`, and its pattern is matched against the event's arguments as a JSON array, e.g. `$[0].room == lobby`), `autoAck` (arguments that answer the server's acknowledgement requests, as JSON text). See [Socket.IO](../../protocols/socketio/#saved-format). |
| `mcp` | `call` (`tool`, `resource` or `prompt`), `name` (the tool or prompt, or the resource URI), `arguments` (a JSON object, or JSON text), `transport` (`auto`, `streamableHttp`, `sse`, `stdio`), and for programs `cwd` and `env` (`[{key, value}]` or an object). See [MCP client](../../mcp/client/#saved-format). |
| `docs` | Markdown notes. |
| `examples` | `[{name, status, headers, body}]`: saved responses (bodies up to 1 MB). Mocks built from the request answer with them. Replaces the request's examples. |

## Environments and variables

| Tool | What it does | Main inputs | Asks |
|---|---|---|---|
| `list_environments` | Environments and their variables, the active one, and the collection variables. Values scripts saved are included and marked `setByScript`. Secret values show as `••••••`. | none | no |
| `save_environment` | Create an environment or update its variables, merged by key (`replace` sets exactly these). Mark credentials `secret: true`: their values stay on this computer. | `name`, `variables[] {key, value, secret, enabled}`, `replace`, `activate` | edit |
| `set_active_environment` | Make an environment active, or none with `""`. | `name` | edit |
| `get_variables` | The variables requests use right now, as Zorvik resolves them (active environment, then collection, then globals, with values scripts saved), each with its source. Useful after a run that saved ids or tokens. | none | no |

## Import and OpenAPI

| Tool | What it does | Main inputs | Asks |
|---|---|---|---|
| `import` | Import a Postman collection or environment, an OpenAPI 3 or Swagger 2 document, or a cURL command, from its text, a URL or an absolute file path. OpenAPI documents are kept in `specs/`, their path parameters become `{{variables}}` with example values in a new environment, and responses are checked against the documented schemas. | exactly one of `text`, `url`, `file`; `folder`; `baseUrl` (OpenAPI without a full server URL) | edit; a `url` also traffic; a `file` always |
| `update_from_openapi` | Update a folder imported from OpenAPI from a new version of the document: new operations added, changed ones updated field by field where you left the old value, removed ones kept and marked removed. Scripts, settings and names are never touched. Call with `preview: true` first. | `folder`; one of `text`, `url`, `file`; `preview` | edit; a `url` also traffic; a `file` always |
| `search_catalog` | The [API catalog](../../requests/api-catalog/): well-known APIs with their id, what they do, how they sign in, the format and license of their description, and whether they can be imported (and why not). | `query`; `category` | none |
| `import_from_catalog` | Import an API from the catalog: its description is downloaded from the publisher, its requests go into a new folder, and its environment (made active) holds `baseUrl` and empty secret variables for the credentials, for you to fill in. An API with thousands of operations and no preset groups needs `groups`; the error lists them. | `id`; `folder`; `groups` | edit; a download also traffic (to the publisher's host) |

## Sending

| Tool | What it does | Main inputs | Asks |
|---|---|---|---|
| `send_request` | Send a saved request (`path`), a saved one with changes (`path` + `request`, nothing is saved), or an unsaved one (`request`, optionally inheriting a `folder`'s auth and headers), with its scripts and tests. Returns status, time, URL, HTTP version, headers, body (up to `maxBodyChars`), redirects, the request as sent, unresolved variables, test results, console and script errors. The response also shows in Zorvik. | `path`, `request`, `folder`, `maxBodyChars` (default 20,000, at most 80,000), `stream`, `filter` | traffic |
| `graphql_schema` | The schema of a GraphQL endpoint by introspection, as SDL. | `path` or `request`, `maxChars` (default and at most 60,000) | traffic |
| `grpc_describe` | Services and methods of a gRPC server, from the request's `.proto` files or server reflection. | `path` or `request` | traffic |
| `mcp_catalog` | What an MCP request's server offers: its name, version, instructions, tools (with input schemas), resources, resource templates and prompts, to call them with `send_request`. | `path` or `request` (kind `mcp`), `maxChars` (default and at most 60,000) | traffic; a program always asks |

`send_request` supports HTTP, GraphQL, gRPC, DNS, MCP and Server-Sent Events. A gRPC stream runs to its end as in [collection runs](../../protocols/grpc/#in-collection-runs) (reading stops at `stream.maxEvents` received messages or `stream.timeoutMs`): the answer has the received `messages`, the `sent` ones, the `steps` (waits are tests) and `notes` on why reading stopped. An MCP request makes its one call (a tool, a resource read or a prompt) in one go, and the response's body is the answer as JSON (status 500 for a JSON-RPC error), so its scripts and tests work as for HTTP. When the request starts a program (a stdio server), the user is asked every time, with the command, folder and environment. An SSE request is read until the first event named `stream.untilEvent` (`message` for unnamed events), `stream.maxEvents` events (default 100; 0 for only the time limit), or `stream.timeoutMs` (default 10,000, at most 120,000), and returns the events. A GraphQL subscription is read the same way, each result an event named `next`. WebSocket, Socket.IO, TCP, UDP and MQTT requests are live connections: the tool points the agent to the [connection tools](#live-connections).

To keep a large JSON response short, give `send_request` a `filter`: `{language: "jsonPath", expression: "$.items[*].id"}` or `{language: "jq", expression: ".items | map(.id)"}`. It runs on the whole body; the matches replace `body`, with `filtered: {matches, truncated}`, or `filterError` when the expression is wrong.

## Live connections

| Tool | What it does | Main inputs | Asks |
|---|---|---|---|
| `open_connection` | Open a live connection from a saved request, a saved one with changes, or an unsaved one, of kind `websocket`, `socketio`, `tcp`, `udp` or `mqtt`. WebSocket and Socket.IO requests run their scripts (before connecting, and [message scripts](../../protocols/websocket/#message-scripts) on each message) and On connect steps ([WebSocket](../../protocols/websocket/#on-connect), [Socket.IO](../../protocols/socketio/#on-connect)). Returns a `connectionId`, the `kind`, each step's result (`steps`), the first 20 messages, the `lastId` and the request's saved messages or emits (`savedMessages`, what `send_message`'s `message` sends); for WebSocket also the status (`101`), the URL, the time and the agreed subprotocol and extensions, for the others the protocol, the server's address and the time. The connection stays open until `close_connection`, the server closing it, or the agent disconnecting. | `path` or `request` (+ `folder`), `waitSeconds` (how long to wait for the steps; default 30, at most 600) | traffic |
| `send_message` | Send one message on the connection: exactly one of `text` (`{{variables}}` filled in), `json` (any JSON value, sent as text), `hex` (a binary message), `message` (a saved message or emit of the request, by name) or `ping: true` (WebSocket). Socket.IO: `event` is the event to emit and `ack: true` asks for an acknowledgement; `json` is the arguments (an array for several), `text` one string argument, `hex` one binary argument. MQTT: `topic`, `qos` and `retain`, by default the request's. Returns `sinceId`, where a wait for the reply starts. | `connectionId` and one of `text`, `json`, `hex`, `message`, `ping`; `event`, `ack` (Socket.IO); `topic`, `qos`, `retain` (MQTT) | no |
| `wait_for_message` | Wait until a received message matches, the time is up (`timedOut: true`) or the connection closes (`closed` says why). By default it looks at messages after the last one the agent sent or saw, so a reply that came before the call still counts. `event` narrows it to one Socket.IO event, whose arguments the pattern is matched against as a JSON array (`$[0].room == lobby`), or to one MQTT topic. Returns the message and `waitedMs`. | `connectionId`, `event`, `match` (`any`, `contains` (default), `exact`, `regex` or `json`), `pattern` (a [JSON condition](../../protocols/websocket/#json-conditions) for `json`), `timeoutMs` (default 10,000, at most 120,000), `sinceId` | no |
| `read_messages` | The connection's messages in both directions, oldest first: `id`, `direction`, `text` or `base64`, `size`, `timestamp`, and where they apply `event` (the Socket.IO event or MQTT topic) and `detail` (acknowledgement notes such as `asks for an acknowledgement (#3)`, an MQTT message's QoS). Pass the `lastId` of the previous call as `sinceId` to get only new ones; `leftOut` counts messages that didn't fit. | `connectionId`, `sinceId`, `limit` (default 50, at most 500) | no |
| `close_connection` | Close the connection. A WebSocket closes with code `1000` or one from `3000` to `4999`, and a reason. | `connectionId`; `code`, `reason` (WebSocket) | no |

- Each connection keeps its last 1,000 messages (up to 8 MB); texts reach the agent cut to 8,000 characters, with secret values hidden like everything else. Secrets are hidden as messages come in, with every secret variable in scope when the connection opened or a message was sent, so switching environments later doesn't reveal them, and waits match only what the agent may see.
- A connection belongs to the agent that opened it. When the agent disconnects, its connections close (a WebSocket with code `1001`). A closed connection stays readable until then (the newest 10 per agent), so the agent can see how it ended.
- While you follow agents, each connection shows live in a Zorvik tab, marked **Connected by an AI agent**.
- `open_connection` asks like `send_request` (see [Host approvals](../permissions-and-safety/#host-approvals)); messages on a connection you allowed don't ask again.

## Collection runs

| Tool | What it does | Main inputs | Asks |
|---|---|---|---|
| `run_collection` | Run the requests of a folder (or the collection) with their scripts and tests, like the Runner tab, which shows it live. Returns the summary and the failures (up to 50), or a `runId` if it runs longer than `waitSeconds`. | `folder`, `requests` (only these paths, in this order), `iterations`, `delayMs`, `dataFile` (inside the workspace), `stopOnFailure`, `waitSeconds` | traffic |
| `get_run_status` | A run's summary and failures once finished; waits up to `waitSeconds`. | `runId`, `waitSeconds` | no |
| `stop_collection_run` | Stop the run in progress. | `runId` | no |

## Load tests

| Tool | What it does | Main inputs | Asks |
|---|---|---|---|
| `list_load_tests` | Saved load tests (id, name, model, number of targets, duration) and the one running (name, `runId`, planned time). | none | no |
| `read_load_test` | A load test's targets, model, stages, thresholds and options. | `name` | no |
| `save_load_test` | Create or replace a load test. Latency thresholds in milliseconds, `errorRate` in percent (`1` = 1 %; values outside 0 to 100 are refused), `rps` in requests per second. A threshold's `abortAfterSecs` stops the run once it has failed that long. `thinkTimeMaxMs` below `thinkTimeMs`, or a `warmUpSecs` as long as the stages, is refused. | `name`, `test {targets, dataFile, model, stages, thinkTimeMs, thinkTimeMaxMs, warmUpSecs, maxInFlight, keepAlive, timeoutMs, httpVersion, thresholds, docs}` | edit |
| `run_load_test` | Start a saved load test and return its results when it ends within `waitSeconds` (without the per-second chart points); otherwise the `runId` and a live snapshot. `waitSeconds: 0` returns as soon as it starts. | `name`, `waitSeconds` | always |
| `get_load_test_status` | Live numbers of the running test, or the results of a finished run. | `name`, `runId`, `waitSeconds` | no |
| `stop_load_test` | Stop the running load test. Its result is kept in the history. | none | no |

The `test` object uses the [load test file format](../../reference/workspace-format/#load-tests-loadtestsyaml): `targets[] {request, weight, enabled, captures[] {variable, from, path}}`, `stages[] {durationSecs, target}`, `thresholds[] {metric, op, value, target, enabled}` and the options. See [Load testing](../../load-testing/overview/).

## Mock APIs and servers

| Tool | What it does | Main inputs | Asks |
|---|---|---|---|
| `list_servers` | Saved mock APIs and servers (HTTP, MCP, WebSocket, Socket.IO, SSE, TCP, UDP, DNS, relay): kind, address, route count, and which are running. | none | no |
| `read_server` | A server's full definition, in the shape `save_server` takes. | `name` | no |
| `save_server` | Create a server or change one. Changes merge like a JSON Merge Patch: objects merge, arrays (routes, rules, records) replace the whole list, `null` resets a field; `replace: true` saves exactly what is given. A running server takes the change at once (a new address, port, TLS or kind needs a restart). | `name`, `server` (the [server format](../../reference/workspace-format/#servers-serversyaml)), `replace` | edit |
| `create_mock` | Build a mock API from a folder (each HTTP request becomes a route answering with its saved example response or a 200) or from an OpenAPI 3 / Swagger 2 document (each operation answers with its first 2xx example). Returns the new server; start it with `start_server`. | `name` (default "Mock API"), one of `folder`, `openapiText`, `openapiUrl`, `openapiFile`; `port` (default: the next free port from 4000) | edit; `openapiUrl` also traffic; `openapiFile` always |
| `start_server` | Start a saved server. `port` overrides the saved port for this run only; `0` picks any free port. When the port is taken, the error names the program holding it. | `name`, `port` | always |
| `stop_server` | Stop a running server. | `name` | no |
| `get_server_traffic` | What a running server received and answered, newest last. HTTP mocks: method, path, request headers and body, the matched route (`null`: no route, the fallback answered), status, response headers and body (bodies up to 4,000 characters), time. Other kinds: connections and messages. The server keeps its last 5,000 entries while it runs. | `name`, `limit` (default 50, at most 500), `sinceId` (the `lastId` of the previous call) | no |

WebSocket servers in `save_server` take everything the app has: `websocket.rules` with `match: json` (a [JSON condition](../../servers/websocket-and-sse-servers/#json-where)), `{{message.id}}` in replies and `close` (a close code), and `websocket.schedule` for [scheduled messages](../../servers/websocket-and-sse-servers/#scheduled-messages). Socket.IO rules take `match: json` too, and `socketio.schedule` [scheduled events](../../servers/socketio-server/#scheduled-events). Mock API routes take `matchJson`, `sequence` and `sequenceRepeat` ([answer sequences](../../servers/mock-api/#answer-sequences)), relays their faults (`delayMs`, `bytesPerSecond`, `dropAfterMs`, `dropAfterBytes`), and event streams `retryMs` and `closeAfterLast`. In `get_server_traffic`, a route's answer from its sequence has a note such as `answer 2 of 3`.

## History and files

| Tool | What it does | Main inputs | Asks |
|---|---|---|---|
| `read_history` | Requests sent from Zorvik (by you or by agents), newest first: method, URL, status, time, size, and for the last 50 sends the response headers and body. | `limit` (default 20, at most 100), `path`, `search`, `maxBodyChars` (default 4,000, at most 20,000; 0 for none) | no |
| `write_file` | Put a file of up to 10 MB into the workspace folder: a sample upload, a CSV or JSON data file, a `.proto` file. An existing file is replaced only with `overwrite: true`. It can't write outside the workspace, through a symbolic link, or into `zorvik.yaml`, `requests/`, `environments/`, `servers/`, `loadtests/` or `.git`. | `path` (relative, `/` between folders), `text` or `base64`, `overwrite` | edit |

## What agents can't do

- Read or change **settings**, **cookies** or **secret values**. There is no tool for them.
- **Delete permanently**: deletes go to the trash, after you confirm.
- Read files **outside the workspace** during a call, except a file you approve for `import`, `update_from_openapi` or `create_mock`.

See [Permissions and safety](../permissions-and-safety/) for the rules behind the **Asks** column.

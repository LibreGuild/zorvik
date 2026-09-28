---
title: Tools
description: Every MCP tool Zorvik offers AI agents, what it does, its main inputs, and whether it asks you first.
sidebar:
  order: 2
---

`zorvik mcp` offers 37 tools. Agents read their full JSON schemas (every field, unit and placeholder) from `tools/list`; this page is the human overview. The **Asks** column says when you are asked, with the default settings (see [Permissions and safety](../permissions-and-safety/)):

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
| `read_request` | Everything saved in one or more requests: URL, query and path parameters, headers, body, auth, scripts, settings, docs. | `path`, or `paths` (up to 50) | no |
| `save_requests` | Create or update up to 200 requests. Each goes into `folder` (created when missing) under its `name`; a request with that name there is updated. Only the fields given change, unless `replace: true`. Give `path` to update that exact request. | `requests[]`: `name`, `folder` or `path`, `kind`, `method`, `url`, `query`, `pathParams`, `headers`, `body`, `auth`, `scripts`, `settings`, `grpc`, `docs`; `replace` | edit |
| `save_folder_settings` | Auth, headers, scripts and docs that requests in a folder inherit, merged into what is there. `folder: ""` is the collection, which also has variables (merged by key). | `folder`, `auth`, `headers`, `scripts`, `docs`, `variables` | edit |
| `move_item` | Move a request or folder to another folder and/or rename it. Load tests that send it follow. | `path`, `toFolder`, `newName` | edit |
| `delete_items` | Move requests, folders, environments, load tests or servers to the trash. Deleting a load test also deletes its run history. | `paths`, `environments`, `loadTests`, `servers` | always |
| `export_request` | A request as a ready-to-run command or code: cURL for bash (`curl`), Windows cmd (`curlCmd`) or PowerShell (`curlPowerShell`), Kotlin with OkHttp, Swift with URLSession, JavaScript with fetch or Python with requests. Variables are filled in unless `resolveVariables: false`; secret values come back as `••••••` (you can copy the full version in Zorvik). | `path` or `request` (+ `folder`), `format`, `resolveVariables` | no |

Request fields for `save_requests` and `send_request`:

| Field | Notes |
|---|---|
| `kind` | `http` (default), `grpc`, `dns`, `websocket`, `sse`, `tcp`, `udp`, `mqtt`. GraphQL is `http` with a `graphql` body. |
| `method` | HTTP method (default GET). gRPC: `package.Service/Method`. DNS: the record type. |
| `url` | Full URL with `{{variables}}`; `:name` path segments take `pathParams`. gRPC: `grpc://` or `grpcs://`. DNS: the name. |
| `query` | `[{key, value, enabled, description}]`: enabled ones replace the URL's query string, disabled ones are kept switched off. |
| `pathParams` | `[{key, value, description}]` for `:name` segments. |
| `headers` | `[{key, value, enabled}]` (an object of name → value works too). |
| `body` | `type` (`none`, `json`, `text`, `xml`, `formUrlencoded`, `multipart`, `binary`, `graphql`) and `text`, `contentType`, `form`, `multipart`, `file` or `graphql {query, variables, operationName}`. |
| `auth` | `type` (`inherit`, `none`, `basic`, `bearer`, `apiKey`, `oauth2`) and its fields. |
| `scripts` | `preRequest`, `postResponse` (Postman `pm` API). |
| `settings` | `timeoutMs`, `followRedirects`, `verifyTls`. |
| `grpc` | `protoFiles` (inside the workspace; empty: server reflection). |
| `docs` | Markdown notes. |

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

## Sending

| Tool | What it does | Main inputs | Asks |
|---|---|---|---|
| `send_request` | Send a saved request (`path`), a saved one with changes (`path` + `request`, nothing is saved), or an unsaved one (`request`, optionally inheriting a `folder`'s auth and headers), with its scripts and tests. Returns status, time, URL, HTTP version, headers, body (up to `maxBodyChars`), redirects, the request as sent, unresolved variables, test results, console and script errors. The response also shows in Zorvik. | `path`, `request`, `folder`, `maxBodyChars` (default 20,000, at most 80,000), `stream` | traffic |
| `graphql_schema` | The schema of a GraphQL endpoint by introspection, as SDL. | `path` or `request`, `maxChars` (default and at most 60,000) | traffic |
| `grpc_describe` | Services and methods of a gRPC server, from the request's `.proto` files or server reflection. | `path` or `request` | traffic |

`send_request` supports HTTP, GraphQL, gRPC (unary calls), DNS and Server-Sent Events. An SSE request is read until the first event named `stream.untilEvent` (`message` for unnamed events), `stream.maxEvents` events (default 100; 0 for only the time limit), or `stream.timeoutMs` (default 10,000, at most 120,000), and returns the events. WebSocket, TCP, UDP and MQTT are live sessions that agents can't use yet: the tool says so.

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
| `save_load_test` | Create or replace a load test. Latency thresholds in milliseconds, `errorRate` in percent (`1` = 1 %; values outside 0 to 100 are refused), `rps` in requests per second. | `name`, `test {targets, dataFile, model, stages, thinkTimeMs, maxInFlight, keepAlive, timeoutMs, httpVersion, thresholds, docs}` | edit |
| `run_load_test` | Start a saved load test and return its results when it ends within `waitSeconds` (without the per-second chart points); otherwise the `runId` and a live snapshot. `waitSeconds: 0` returns as soon as it starts. | `name`, `waitSeconds` | always |
| `get_load_test_status` | Live numbers of the running test, or the results of a finished run. | `name`, `runId`, `waitSeconds` | no |
| `stop_load_test` | Stop the running load test. Its result is kept in the history. | none | no |

The `test` object uses the [load test file format](../../reference/workspace-format/#load-tests-loadtestsyaml): `targets[] {request, weight, enabled, captures[] {variable, from, path}}`, `stages[] {durationSecs, target}`, `thresholds[] {metric, op, value, target, enabled}` and the options. See [Load testing](../../load-testing/overview/).

## Mock APIs and servers

| Tool | What it does | Main inputs | Asks |
|---|---|---|---|
| `list_servers` | Saved mock APIs and servers (HTTP, WebSocket, SSE, TCP, UDP, DNS, relay): kind, address, route count, and which are running. | none | no |
| `read_server` | A server's full definition, in the shape `save_server` takes. | `name` | no |
| `save_server` | Create a server or change one. Changes merge like a JSON Merge Patch: objects merge, arrays (routes, rules, records) replace the whole list, `null` resets a field; `replace: true` saves exactly what is given. A running server takes the change at once (a new address, port, TLS or kind needs a restart). | `name`, `server` (the [server format](../../reference/workspace-format/#servers-serversyaml)), `replace` | edit |
| `create_mock` | Build a mock API from a folder (each HTTP request becomes a route answering with its saved example response or a 200) or from an OpenAPI 3 / Swagger 2 document (each operation answers with its first 2xx example). Returns the new server; start it with `start_server`. | `name` (default "Mock API"), one of `folder`, `openapiText`, `openapiUrl`, `openapiFile`; `port` (default: the next free port from 4000) | edit; `openapiUrl` also traffic; `openapiFile` always |
| `start_server` | Start a saved server. `port` overrides the saved port for this run only; `0` picks any free port. When the port is taken, the error names the program holding it. | `name`, `port` | always |
| `stop_server` | Stop a running server. | `name` | no |
| `get_server_traffic` | What a running server received and answered, newest last. HTTP mocks: method, path, request headers and body, the matched route (`null`: no route, the fallback answered), status, response headers and body (bodies up to 4,000 characters), time. Other kinds: connections and messages. The server keeps its last 5,000 entries while it runs. | `name`, `limit` (default 50, at most 500), `sinceId` (the `lastId` of the previous call) | no |

## History and files

| Tool | What it does | Main inputs | Asks |
|---|---|---|---|
| `read_history` | Requests sent from Zorvik (by you or by agents), newest first: method, URL, status, time, size, and for the last 50 sends the response headers and body. | `limit` (default 20, at most 100), `path`, `search`, `maxBodyChars` (default 4,000, at most 20,000; 0 for none) | no |
| `write_file` | Put a file of up to 10 MB into the workspace folder: a sample upload, a CSV or JSON data file, a `.proto` file. An existing file is replaced only with `overwrite: true`. It can't write outside the workspace, through a symbolic link, or into `zorvik.yaml`, `requests/`, `environments/`, `servers/`, `loadtests/` or `.git`. | `path` (relative, `/` between folders), `text` or `base64`, `overwrite` | edit |

## What agents can't do

- Read or change **settings**, **cookies** or **secret values**. There is no tool for them.
- Use **live sessions**: WebSocket, TCP, UDP and MQTT requests (they can save them, not send them).
- **Delete permanently**: deletes go to the trash, after you confirm.
- Read files **outside the workspace** during a call, except a file you approve for `import`, `update_from_openapi` or `create_mock`.

See [Permissions and safety](../permissions-and-safety/) for the rules behind the **Asks** column.

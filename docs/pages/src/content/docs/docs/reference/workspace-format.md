---
title: Workspace format
description: Every file in a Zorvik workspace (zorvik.yaml, requests, folders, environments, servers, load tests and specs) with examples, fields and defaults.
sidebar:
  order: 1
---

A workspace is a plain folder of YAML files, meant to be committed to Git and reviewed in pull requests. This page describes every file Zorvik reads and writes there. For how to create and open workspaces, see [Workspaces](../../getting-started/workspaces/).

```text
my-api/
  zorvik.yaml                 # workspace: name, id, variables, default auth, headers, scripts
  environments/
    Local.yaml                # an environment and its variables
    Staging.yaml
  requests/
    Get status.yaml           # a request
    Users/                    # a folder
      _folder.yaml            # optional: the folder's order, auth, headers, scripts, docs
      List users.yaml
      Admin/
        Delete user.yaml
  servers/
    Payments mock.yaml        # a mock API or another server
  loadtests/
    Checkout smoke.yaml       # a load test
  specs/
    Payments API.yaml         # an OpenAPI document kept by an import
  data/users.csv              # any other files you add (data files, bodies, .proto files…)
```

## General rules

- **YAML, camelCase keys.** Every file is YAML 1.2 with camelCase field names. Zorvik writes files in a stable order and **leaves out fields at their default value**, so files stay small and diffs stay clean. You can write them by hand; missing fields take their defaults.
- **File names come from names.** The display name is inside the file (`name:`). The file or folder name is derived from it: `< > : " / \ | ?` `*` and control characters become `-`, leading dots and trailing dots and spaces are dropped, names are cut to 80 characters, Windows device names (`CON`, `NUL`, `COM1` …) get a `_`, and `_folder` becomes `untitled`. When the name is taken (ignoring case), Zorvik adds ` 2`, ` 3` … Renaming an item in Zorvik renames its file.
- **Names** can't be empty and are at most 200 characters.
- **Order.** `seq` is the position among siblings in the sidebar. Items with the same `seq` sort folders first, then by name.
- **Hidden and linked files.** Files and folders whose name starts with `.` are ignored. Symbolic links are not followed anywhere in the workspace (they could point outside it).
- **Limits.** YAML files over 50 MB are not read. The request tree is read at most 32 folder levels deep.
- **Broken files.** A file that doesn't parse shows in the sidebar with a warning (and the parse error) instead of breaking the workspace.
- **What is not in the workspace.** Secret values, values set by scripts, cookies, OAuth tokens, history, load test results and the active environment live in the app's data folder on each computer. See [Data locations](../data-locations/).

`{{variables}}` can be used in almost every text field of requests, folders and the workspace (URLs, headers, bodies, auth fields). See [Variables](../../variables/secrets/).

## `zorvik.yaml`

The workspace file. A folder is a workspace when it has one.

```yaml title="zorvik.yaml"
version: 1
id: 5b7f0c1e-2d44-4b8a-9d3a-1f0e6c2a9b71
name: Shop API
variables:
  - { key: apiVersion, value: v2 }
  - { key: apiKey, value: "", secret: true }
auth:
  type: bearer
  token: "{{accessToken}}"
headers:
  - { key: X-Client, value: zorvik }
scripts:
  postResponse: |
    pm.test("No server error", () => pm.expect(pm.response.code).to.be.below(500));
docs: |
  # Shop API
  Local setup: `make run`, then use the Local environment.
```

| Field | Type | Default | Meaning |
|---|---|---|---|
| `version` | number | | Format version, `1`. A workspace with a newer version is refused: "This workspace was created by a newer Zorvik (format vN); please update the app". |
| `id` | string | generated | Stable id: up to 64 letters, digits, `-` or `_`. Written on first open when missing. Together with the folder's path, it keys this computer's secrets, cookies and tokens for the workspace, so a copied `id` doesn't unlock another workspace's secrets. |
| `name` | string | | Display name. |
| `variables` | [variables](#variables) | `[]` | Workspace (collection) variables. |
| `auth` | [auth](#auth) | `none` | Default auth for requests and folders set to `inherit`. |
| `headers` | [key/values](#key-values) | `[]` | Headers added to every request (a folder's or the request's header of the same name replaces it). |
| `scripts` | [scripts](#scripts) | | Run before and after every request of the workspace, before the folders' and the request's own. |
| `docs` | string | | Markdown notes. |

Edit it in the app with **Workspace settings** (name, default auth, default headers, scripts) and **Environments** (workspace variables).

## Requests (`requests/**/*.yaml`)

Each request is one `.yaml` file (any case of the extension) under `requests/`. Folders are folders. A request's **path** is its file path relative to `requests/`, with `/`: `Users/List users.yaml`. Load tests, agents and the command line refer to requests by this path.

```yaml title="requests/Users/Create user.yaml"
name: Create user
seq: 2
method: POST
url: "{{baseUrl}}/users?notify=true"
paramDescriptions:
  - { key: notify, description: Send a welcome email }
disabledParams:
  - { key: dryRun, value: "true", description: Validate only }
headers:
  - key: X-Request-ID
    value: "{{$uuid}}"
body:
  type: json
  text: |
    { "name": "{{name}}", "email": "{{email}}" }
auth:
  type: bearer
  token: "{{token}}"
settings:
  timeoutMs: 10000
scripts:
  postResponse: |
    pm.test("Status is 201", () => pm.response.to.have.status(201));
    pm.environment.set("userId", pm.response.json().id);
docs: Handler in `src/users/create.ts`.
```

| Field | Type | Default | Meaning |
|---|---|---|---|
| `name` | string | | Display name (also the file name). |
| `kind` | string | `http` | `http`, `websocket`, `socketio`, `sse`, `grpc`, `tcp`, `udp`, `dns` or `mqtt`. |
| `seq` | number | `0` | Position among siblings. |
| `method` | string | `GET` | HTTP method. gRPC: `package.Service/Method`. DNS: the record type (`A`, `AAAA`, `MX` …). |
| `url` | string | `""` | The URL as typed, **including the enabled query parameters**. `:name` path segments take their values from `pathParams`. |
| `disabledParams` | [key/values](#key-values) | `[]` | Query parameters switched off (not in `url`), with their descriptions. |
| `pathParams` | [key/values](#key-values) | `[]` | Values of `:name` path segments. |
| `paramDescriptions` | [key/values](#key-values) | `[]` | Descriptions of the enabled query parameters, by `key` (the `url` has no room for them). |
| `headers` | [key/values](#key-values) | `[]` | Request headers. |
| `body` | [body](#body) | none | The request body. |
| `auth` | [auth](#auth) | `inherit` | Auth. `inherit` takes the nearest folder's, then the workspace's. |
| `settings` | [settings](#request-settings) | | Per-request overrides of the app's request settings. |
| `scripts` | [scripts](#scripts) | | Pre-request and post-response JavaScript. |
| `socket` | [socket](#tcp-and-udp-socket) | | TCP and UDP options. |
| `dns` | [dns](#dns-dns) | | DNS options. |
| `mqtt` | [mqtt](#mqtt-mqtt) | | MQTT options. |
| `grpc` | [grpc](#grpc-grpc) | | gRPC options. |
| `socketio` | [socketio](#socketio-socketio) | | Socket.IO options. |
| `docs` | string | | Markdown notes. |
| `openapi` | object | | Set by an OpenAPI import: `operation` (`"GET /pets/{petId}"`) and `removed: true` when the operation is no longer in the document. |

### Key-values

Headers, query parameters, form fields and path parameters are lists of:

| Field | Default | Meaning |
|---|---|---|
| `key` | `""` | Name. |
| `value` | `""` | Value (`{{variables}}` allowed). |
| `enabled` | `true` | `false` keeps the row but doesn't send it. |
| `description` | `""` | Notes. |

### Body

Data for every body type is kept, so switching the type in the app never loses what was typed; only `type` decides what is sent.

| Field | Meaning |
|---|---|
| `type` | `none` (default), `json`, `text`, `xml`, `formUrlencoded`, `multipart`, `binary` or `graphql`. |
| `text` | The JSON, text or XML body. For WebSocket requests, the message draft; for Socket.IO, the arguments to emit; for gRPC, the JSON message; for MQTT, the message to publish. |
| `contentType` | Content-Type of `text` bodies (default `text/plain`). |
| `form` | [Key/values](#key-values) of a `formUrlencoded` body. |
| `multipart` | Parts: `key`, `value` (text, or a file path when `file: true`), `file`, `contentType`, `enabled`. |
| `file` | The file of a `binary` body: absolute, or relative to the workspace folder. |
| `graphql` | `query`, `variables` (JSON text, may contain `{{variables}}`), `operationName`. Sent as JSON `{"query", "variables", "operationName"}`. For subscriptions also `transport` (`websocket`, `websocketLegacy` or `sse`), `subscriptionUrl` and `connectionParams` (JSON text); see [GraphQL subscriptions](../../protocols/graphql/#subscriptions). |

Body files (binary bodies and multipart files) must be inside the workspace folder unless **Files outside the workspace** is on in Settings.

### Auth

`auth` is an object with a `type`:

| `type` | Fields |
|---|---|
| `inherit` | none. Use the nearest folder's auth, then the workspace's. The default for requests and folders. |
| `none` | none. No auth, even if a folder has some. The default for `zorvik.yaml`. |
| `basic` | `username`, `password`. |
| `bearer` | `token`; `prefix` (default `Bearer`). |
| `apiKey` | `key` (header or parameter name), `value`, `location`: `header` (default) or `query`. |
| `oauth2` | See below. |

OAuth 2.0 fields:

| Field | Default | Meaning |
|---|---|---|
| `grantType` | `clientCredentials` | `clientCredentials`, `password` or `authorizationCode`. |
| `tokenUrl` | | Token endpoint. |
| `authUrl` | | Authorization endpoint (authorization code). |
| `redirectUri` | `http://127.0.0.1:53682/callback` | Loopback redirect registered with the provider (authorization code). |
| `clientId`, `clientSecret` | | Client credentials. |
| `scope`, `audience` | | Optional. |
| `username`, `password` | | Password grant. |
| `clientAuth` | `basicHeader` | `basicHeader` (client id and secret in a Basic header) or `body` (form fields). |
| `pkce` | `true` | PKCE (S256) for the authorization code grant. |
| `headerPrefix` | `Bearer` | `Authorization` header prefix; empty sends the bare token. |

Tokens are cached in the app's data folder, never in the workspace.

### Request settings

Each field overrides the app setting for this request only (see [Settings](../settings/#requests)).

| Field | Meaning |
|---|---|
| `timeoutMs` | Whole-request timeout; `0` = no limit. |
| `followRedirects` | Follow redirects. |
| `maxRedirects` | Most redirects followed. |
| `verifyTls` | Verify the server's certificate. |
| `httpVersion` | `auto`, `http1`, `http2` or `http3`. |
| `decompress` | Decode gzip, deflate, br and zstd responses. |
| `repeat` | Collection runs: send again until a condition holds. `condition` (JavaScript, e.g. `pm.response.json().status === "done"`; empty = until the request's tests pass, or without tests until the status is below 400), `intervalMs` (default 1000), `timeoutMs` (default 30000; then the request fails). |
| `stream` | SSE requests in collection runs and for agents: when to stop reading. `event` (stop after the first event with this name; `message` for unnamed events), `maxEvents` (default 100; 0 = only the time limit), `timeoutMs` (default 10000). |

Load tests don't use a request's own settings: they use the load test's options and the app settings.

### Scripts

| Field | Meaning |
|---|---|
| `preRequest` | JavaScript run before sending. |
| `postResponse` | JavaScript run after the response: tests with `pm.test`. |

Workspace scripts run first, then the folders' (outer to inner), then the request's. See [Scripts](../../scripting/overview/).

### Kind-specific options

#### TCP and UDP (`socket`)

`url` is `tcp://host:port` (`tls://host:port` for TLS) or `udp://host:port`.

| Field | Default | Meaning |
|---|---|---|
| `framing` | `raw` | How incoming bytes become messages: `raw` (as they arrive; one message per UDP datagram), `line` (one per line) or `lengthPrefixed`. |
| `lengthBytes` | `2` | Size of the big-endian length prefix: 1, 2 or 4. |
| `lineEnding` | `none` | Added to each text message sent: `none`, `lf` or `crLf`. |
| `broadcast` | `false` | UDP: allow sending to broadcast addresses. |

#### DNS (`dns`)

`url` is the name to look up and `method` the record type.

| Field | Default | Meaning |
|---|---|---|
| `server` | `""` | The resolver. Empty: the system's. Otherwise `1.1.1.1`, `8.8.8.8:53`, `tcp://1.1.1.1`, `tls://1.1.1.1` (DNS over TLS) or `https://…/dns-query` (DNS over HTTPS). |
| `recursion` | `true` | Ask for recursive resolution. |

#### MQTT (`mqtt`)

`url` is `mqtt://host:1883` or `mqtts://host:8883`. The username and password come from `basic` auth; the message to publish is `body.text`.

| Field | Default | Meaning |
|---|---|---|
| `clientId` | `""` | Empty: a random id per connection. |
| `version` | `v311` | `v311` or `v5`. |
| `cleanSession` | `true` | |
| `keepAliveSecs` | `30` | |
| `subscriptions` | `[]` | Topics subscribed after connecting: `topic`, `qos`, `enabled`. |
| `topic` | `""` | Topic the composer publishes to. |
| `qos` | `0` | QoS of published messages. |
| `retain` | `false` | Retain published messages. |

#### gRPC (`grpc`)

`url` is `grpc://host:port` (plaintext HTTP/2) or `grpcs://host:port` (TLS), `method` is `package.Service/Method`, and `body.text` is the JSON message.

| Field | Meaning |
|---|---|
| `protoFiles` | `.proto` files, relative to the workspace folder or absolute. Empty: server reflection. |
| `importPaths` | Folders searched for `import`s (the proto files' folders are always searched). |

#### Socket.IO (`socketio`)

`url` is the server and the namespace (`http://localhost:3000/chat`); `body.text` holds the arguments the composer emits.

| Field | Default | Meaning |
|---|---|---|
| `path` | `/socket.io/` | The server's Socket.IO path. |
| `transport` | `auto` | `auto` (WebSocket, else long-polling), `websocket` or `polling`. |
| `auth` | `""` | The connection's auth payload as JSON text (may contain `{{variables}}`). |
| `event` | `""` | The event the composer emits. |
| `ack` | `false` | The composer asks for an acknowledgement. |

#### WebSocket and SSE

WebSocket requests use `ws://` or `wss://` URLs and keep the message draft in `body.text`. SSE requests use `http://` or `https://` URLs; `settings.stream` says when runs stop reading.

## Folders (`_folder.yaml`)

A folder under `requests/` is a folder in the collection. An optional `_folder.yaml` inside it holds its settings; requests inside inherit its auth, headers and scripts.

```yaml title="requests/Users/_folder.yaml"
name: Users
seq: 1
auth:
  type: apiKey
  key: X-Api-Key
  value: "{{apiKey}}"
headers:
  - { key: Accept, value: application/json }
docs: Everything under /users.
```

| Field | Default | Meaning |
|---|---|---|
| `name` | the folder name | Display name. |
| `seq` | `0` | Position among siblings. |
| `auth` | `inherit` | Auth for requests inside set to `inherit`. |
| `headers` | `[]` | Headers for requests inside (inner folders' and the request's headers of the same name win). |
| `scripts` | | Run for every request inside, after the workspace's and outer folders' scripts. |
| `docs` | | Markdown notes. |
| `openapi` | | Set by an OpenAPI import: `spec` (the kept document, e.g. `specs/Payments API.yaml`), `source` (the URL or file it came from, for **Update from API spec**), `validate` (default `true`: check responses against the document). |

The collection root (`requests/` itself) has no `_folder.yaml`: its settings are in `zorvik.yaml`.

## Environments (`environments/*.yaml`)

One file per environment. The file name (without `.yaml`) is the environment's id; renaming the environment renames the file.

```yaml title="environments/Staging.yaml"
name: Staging
variables:
  - { key: baseUrl, value: "https://staging.example.com" }
  - { key: token, value: "", secret: true }
  - { key: debug, value: "true", enabled: false }
```

| Field | Meaning |
|---|---|
| `name` | Display name. |
| `variables` | The environment's [variables](#variables). |

### Variables

| Field | Default | Meaning |
|---|---|---|
| `key` | | Name, used as `{{key}}`. Case-sensitive. |
| `value` | `""` | Value. Always empty in the file for secret variables. |
| `enabled` | `true` | `false` keeps it but doesn't define it. |
| `secret` | `false` | The real value is kept on this computer only (the app's secret store), never in the file. |

Which environment is active is saved per computer, not in the workspace. Values set by scripts (`pm.environment.set`, `pm.collectionVariables.set`) are also kept on the computer and win over the file's value. See [Secret variables](../../variables/secrets/).

## Servers (`servers/*.yaml`)

One file per mock API or server. The file name (without `.yaml`) is the server's id. Only the section of the server's `kind` is used; the others are kept, so switching the kind in the app never loses what was set up. See [Mock APIs](../../servers/mock-api/) and [Running servers](../../servers/running-servers/).

```yaml title="servers/Payments mock.yaml"
name: Payments mock
seq: 0
host: 127.0.0.1
port: 4010
http:
  routes:
    - name: Create payment
      method: POST
      path: /payments
      status: 201
      headers:
        - { key: Content-Type, value: application/json }
      body: '{"id": "{{$uuid}}", "status": "created"}'
      delayMs: 120
    - method: GET
      path: /payments/:id
      body: '{"id": "{{request.params.id}}", "status": "paid"}'
    - method: POST
      path: /refunds
      fault: error
      faultPercent: 20
  fallback: proxy
  proxyUrl: http://localhost:8080
  cors: true
```

| Field | Default | Meaning |
|---|---|---|
| `name` | | Display name. |
| `kind` | `http` | `http` (mock API), `websocket`, `socketio`, `sse`, `tcp`, `udp`, `dns` or `tcpProxy` (a TCP relay that shows both directions). |
| `seq` | `0` | Position in the sidebar. |
| `host` | `127.0.0.1` | Address to listen on: `127.0.0.1` (this computer only) or `0.0.0.0` (other devices too). |
| `port` | `0` | Port; `0` = any free port. New servers made in the app get 3000 (HTTP), 3001 (WebSocket), 3002 (SSE), 3003 (Socket.IO), 9000 (TCP), 9001 (UDP), 1053 (DNS) or 9100 (relay). |
| `tls` | off | `enabled`, `certPath`, `keyPath` (PEM). Without paths, a self-signed certificate for `localhost` is generated. For `http`, `websocket`, `socketio`, `sse` and `tcp`. |
| `autoStart` | `false` | Start with the workspace. Only configurations this computer has started or saved before start by themselves; one that is new or changed outside Zorvik (e.g. by a Git pull) must be started once by hand. |
| `http` | | Mock API: routes and fallback (below). |
| `websocket` | | WebSocket server: `mode`, `greeting`, `rules`. |
| `socketio` | | Socket.IO server (below). |
| `sse` | | SSE server: `events`, `intervalMs`, `repeat`. |
| `socket` | | TCP and UDP servers: `mode`, `greeting`, `rules`, `encoding`, `framing`, `lengthBytes`, `lineEnding`. |
| `dns` | | DNS server: `records`, `upstream`. |
| `proxy` | | TCP relay: `target`, `upstreamTls`. |
| `docs` | | Markdown notes. |

### Mock API (`http`)

| Field | Default | Meaning |
|---|---|---|
| `routes` | `[]` | Tried in order: the first enabled route that matches answers. |
| `fallback` | `notFound` | Requests no route matches: `notFound` (404 with a short explanation) or `proxy` (forward to `proxyUrl`). |
| `proxyUrl` | | Base URL of the real backend for `fallback: proxy`. |
| `cors` | `false` | Answer CORS preflights and add `Access-Control-Allow-*` headers. |

Route fields:

| Field | Default | Meaning |
|---|---|---|
| `name` | | Shown in the traffic log. |
| `method` | `*` | HTTP method, or `*` for any. |
| `path` | | Starts with `/`. `:name` matches one segment, a trailing `*` the rest. |
| `status` | `200` | Status code. |
| `headers` | `[]` | Response headers. |
| `body` | `""` | Response body. |
| `delayMs` | `0` | Wait before answering. |
| `matchQuery` | `[]` | Only match requests with these query parameters (empty value = any value). |
| `matchHeaders` | `[]` | Only match requests with these headers (empty value = any value). |
| `matchBody` | `""` | Only match requests whose body contains this text. |
| `fault` | `none` | Instead of the answer: `error` (500), `reset` (close the connection) or `hang` (never answer). |
| `faultPercent` | `100` | How often the fault happens, in percent. |
| `enabled` | `true` | |

Status, headers and body are templates: `{{request.params.id}}`, `{{request.query.name}}`, `{{request.headers.name}}`, `{{request.body}}`, `{{request.method}}`, `{{request.path}}`, dynamic values such as `{{$uuid}}`, and the active environment's and workspace variables. Secret variables are never filled in, and text a client sends is never expanded.

### WebSocket, TCP and UDP servers

| Field | Default | Meaning |
|---|---|---|
| `mode` | `echo` | `echo` (send every message back), `rules` (reply with the first matching rule), `manual` (only what you send from the app) or `discard`. |
| `greeting` | `""` | Sent to each client right after it connects (WebSocket, TCP). |
| `rules` | `[]` | Replies for `mode: rules`: `match` (`any`, `contains` (default), `exact`, `regex`), `pattern`, `reply`, `delayMs`, `enabled`. |
| `encoding` | `text` | TCP and UDP: `text` or `hex` (e.g. `48 65 6c 6c 6f`) for greeting, patterns and replies. |
| `framing` | `raw` | TCP: `raw`, `line` or `lengthPrefixed`. |
| `lengthBytes` | `2` | TCP, `lengthPrefixed`: 1, 2 or 4. |
| `lineEnding` | `none` | Appended to text replies: `none`, `lf`, `crLf`. |

### Socket.IO server (`socketio`)

| Field | Default | Meaning |
|---|---|---|
| `mode` | `echo` | `echo` (emit every event back, acknowledge with its arguments), `rules`, `manual` or `discard`. |
| `greetingEvent`, `greetingArgs` | `""` | An event (and its JSON arguments) emitted to each client that joins a namespace. |
| `rules` | `[]` | For `mode: rules`: `event` (`*` for any), `match` (`any` (default), `contains`, `exact`, `regex`) and `pattern` on the arguments, `ack` (acknowledgement arguments), `replyEvent` and `replyArgs`, `broadcast`, `delayMs`, `enabled`. |
| `path` | `/socket.io/` | Where the server answers. |
| `cors` | `false` | Allow browsers on other origins. |

See [Socket.IO servers](../../servers/socketio-server/).

### SSE server (`sse`)

| Field | Default | Meaning |
|---|---|---|
| `events` | `[]` | Sent in order to each client after it connects: `event` (empty: `message`), `data`, `id`. |
| `intervalMs` | `0` | Pause between events (0: all at once). |
| `repeat` | `false` | Start over after the last event. |

### DNS server (`dns`)

| Field | Default | Meaning |
|---|---|---|
| `records` | `[]` | `name` (e.g. `api.example.test` or `*.example.test`), `type` (`A`, `AAAA`, `CNAME`, `TXT`, `MX`, `NS`, `PTR`, `SRV`, `CAA`), `value` (as in a zone file, e.g. `10 mail.example.test`), `ttl` (default 60), `enabled`. |
| `upstream` | `""` | Names without a record: empty = "no such name", `system` = this computer's resolver, or a server such as `1.1.1.1`. |

### TCP relay (`proxy`)

| Field | Default | Meaning |
|---|---|---|
| `target` | `""` | `host:port` every client connection is relayed to. |
| `upstreamTls` | `false` | Connect to the target with TLS (clients still talk plain TCP to the relay). |

## Load tests (`loadtests/*.yaml`)

One file per load test. The file name (without `.yaml`) is its id, used by `zorvik load` and in the run history. See [Load testing](../../load-testing/overview/).

```yaml title="loadtests/Checkout smoke.yaml"
name: Checkout smoke
seq: 0
dataFile: data/users.csv
targets:
  - request: Auth/Login.yaml
    captures:
      - { variable: token, from: json, path: $.accessToken }
  - request: Cart/Add to cart.yaml
    weight: 3
  - request: Checkout/Pay.yaml
model: virtualUsers
stages:
  - { durationSecs: 10, target: 20 }
  - { durationSecs: 60, target: 20 }
  - { durationSecs: 10, target: 0 }
thinkTimeMs: 500
thresholds:
  - { metric: p95, op: "<", value: 300 }
  - { metric: errorRate, op: "<", value: 1 }
  - { metric: p99, op: "<", value: 800, target: Checkout/Pay.yaml }
```

| Field | Type | Default | Meaning |
|---|---|---|---|
| `name` | string | | Display name. |
| `seq` | number | `0` | Position in the sidebar. |
| `targets` | list | `[]` | Requests to send (below). |
| `model` | string | `virtualUsers` | `virtualUsers` (closed model) or `arrivalRate` (open model, requests per second). |
| `stages` | list | `[]` | `durationSecs` and `target` (both required): ramp linearly to `target` users or requests per second over `durationSecs`. |
| `thinkTimeMs` | number | `0` | Virtual users: pause after each answer. |
| `maxInFlight` | number | `1000` | Arrival rate: most requests in flight; more are dropped. |
| `keepAlive` | bool | `true` | Reuse connections. |
| `timeoutMs` | number | app setting | Per-request timeout; `0` = no limit. |
| `httpVersion` | string | app setting | `auto`, `http1` or `http2` (`http3` is refused when the test starts). |
| `thresholds` | list | `[]` | Pass/fail rules (below). |
| `dataFile` | string | | CSV or JSON data file, relative to the workspace folder (or absolute). |
| `docs` | string | | Markdown notes. |

`targets[]`:

| Field | Default | Meaning |
|---|---|---|
| `request` | | Request path relative to `requests/`. |
| `weight` | `1` | How often, relative to the others. `0` = not sent. |
| `enabled` | `true` | `false` = not sent. |
| `captures` | `[]` | `variable`, `from` (`json` (default), `header`, `regex`), `path`. See [Captures](../../load-testing/data-and-captures/#captures). |

`thresholds[]`:

| Field | Default | Meaning |
|---|---|---|
| `metric` | | `p50`, `p90`, `p95`, `p99`, `p999`, `avg`, `max` (ms), `errorRate` (%), `rps` (req/s). |
| `op` | | `<`, `<=`, `>`, `>=` (quote them in YAML). |
| `value` | | The limit. |
| `target` | | A request path: only that request. |
| `enabled` | `true` | |

See [Thresholds](../../load-testing/thresholds/). When a request is renamed or moved in Zorvik, load tests that send it are updated in place (their file name doesn't change).

## Specs (`specs/`)

An OpenAPI import keeps the document in `specs/`, named after the import (`.json` when the document is JSON, else `.yaml`). The imported folder's `_folder.yaml` points at it (`openapi.spec`), and each imported request names its operation (`openapi.operation`). Zorvik uses it to check every response of those requests against the documented status codes and schemas (a test named "Matches the API spec"), and to update the folder from a new version of the document. Documents up to 50 MB are read.

## Other files

A workspace can hold any other files: data files for runs and load tests, files sent as bodies, `.proto` files. Zorvik reads them only where a request, run or load test refers to them. Paths in those fields are relative to the workspace folder (or absolute). By default, files outside the workspace folder can't be used (Settings → Data & privacy → **Files outside the workspace**), so a workspace you clone can't make Zorvik send your private files.

Zorvik creates `requests/` and `environments/` when it opens a workspace, and `servers/`, `loadtests/` and `specs/` when something is first saved there. It adds nothing else: no lock files, caches or `.gitignore`.

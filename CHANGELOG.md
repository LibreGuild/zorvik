# Changelog

Notable changes in each release. Every release on [GitHub Releases](https://github.com/LibreGuild/zorvik/releases) also lists the merged pull requests.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- **Scripts for WebSocket and Socket.IO requests:** a **Before connecting** script (set variables, change the URL or headers, or stop the connection), and **On message** scripts that run on the messages that match their direction (received, sent or both), Socket.IO event and pattern. They see the message as `pm.message` (`text()`, `json()`, `args`, `event`), test it with `pm.test`, keep values with `pm.environment.set`, and reply with `pm.connection.send(data)` or `pm.connection.emit(event, …args)`. Their notes and test results show in the message log; in collection runs their tests count.
- **gRPC streams with saved messages and On connect steps:** send saved messages once a stream is open, wait for an answer (`$.status == READY`), keep sending one, and end the stream after the steps.
- **Stream a file or the microphone into a gRPC stream:** client and bidirectional streams send audio (or any bytes) in real time, each chunk as a message with its bytes in a field you name (`audioContent` for Google Speech-to-Text), and end the stream when the file is sent or the recording stops, for live transcripts from speech services.
- **gRPC calls in collection runs, `zorvik run` and for AI agents:** unary calls and streams run to their end (reading stops after the request's **Stop after** messages or **Time limit**); each wait is a test, scripts get the answer as `pm.response.json()`, and a status other than `OK` fails the request. The **Scripts** tab is there for gRPC requests too.
- **Collection runner details:** open a result and choose **Request** or **Response** to see the headers and body it sent and the status, headers and body it got (secrets shown as `{{name}}`), next to its tests and errors.
- **Mermaid diagrams in docs:** a ```` ```mermaid ```` block in the docs of a request, folder, workspace, server or load test is drawn as a diagram, in the app's light or dark theme.
- AI agents see the saved messages and emits of a connection (`savedMessages` in `open_connection`) and can set message scripts and gRPC steps with `save_requests`.

### Changed
- The message log shows JSON messages pretty-printed with JSON held in strings (a Socket.IO argument sent as `JSON.stringify(…)`) expanded; **Raw** shows them as they came.
- **Save as message…** and **Save as emit…** on a saved request write the message to the request's file at once, where teammates and AI agents see it.

### Fixed
- A WebSocket or Socket.IO connection error longer than the log's width can be opened to read all of it.
- Hovering a `{{variable}}` in a body, a message, a script or any other editor shows its value and where it comes from, as in the URL.
- The **MCP: test AI tools** section of the docs has its icon.

## [0.3.0] - 2026-10-20

### Added
- **API catalog:** import well-known APIs in one step from the **API catalog** tab of the Import dialog (or **Browse API catalog** on the start screen): AI and LLM providers (OpenAI, Anthropic, Gemini, Mistral, Groq, OpenRouter, Ollama…), cloud and developer tools (AWS, Google Cloud, Cloudflare, GitHub, GitLab, Kubernetes, Docker…), messaging, productivity, payments, commerce, identity and more. Search or pick a category, see how each API signs in and its description's license, and choose **Import**: the requests arrive in a folder with sign-in set up, an environment with empty secret variables for your key is made active, and the request to try first opens. Very large APIs let you tick the groups you want. Descriptions are downloaded from their publishers only when you import; when a file has moved or a download fails, the message says what happened and how to download it yourself and import the file. AI agents can use it with `search_catalog` and `import_from_catalog`.
- **Google Discovery documents import** like OpenAPI (Gmail, Drive, Calendar, Sheets, YouTube, Gemini, Vertex AI, Cloud Storage, BigQuery and every other Google API), with Google's OAuth 2.0 or an API key, response checks, mocks and **Update from API spec**.
- **AWS service models import** (S3, DynamoDB, Lambda, SQS, IAM, EC2, Bedrock and the rest): one request per operation in the service's own protocol, signed with AWS Signature, with `awsRegion` and your access keys in the new environment.
- **More Postman collections import:** Collection v1 files (from before 2017), Postman API exports, collections without an `info` block, globals files and unnamed environments. The collection's description becomes the folder's docs.
- **More of Postman's script sandbox:** SugarJS methods (`responseBody.has(…)`, `(56).ordinalize()`), the older `request`, `responseCookies` and `postman.getResponseCookie`, `Buffer`, `require('assert')` and `require('timers')`, Postman's lodash 3 names on `_` (`pluck`, `where`, `_.max(list, 'n')`), `pm.request.body.urlencoded` and `.formdata`, `pm.request.url.variables`, `pm.request.description`, `pm.request.to.have.header`, `pm.response.stream`, `details()`, `size()` and `contentInfo()`, `pm.execution.location`, and more chai: `frozen`, `sealed`, `respondTo`, `change`/`increase`/`decrease`, nested and own `include`, `pm.expect.fail`. chai-postman assertions (`jsonSchema`, `statusCode`, `responseTime`…) work on any value.
- Variable names built from variables (`{{host_{{env}}}}`) resolve as in Postman.
- An OpenAPI document whose security combines several API keys (a key and an application key, a key and a token) imports all of them: the others as headers of the folder or query parameters, each with its own secret variable.
- The imported folder's docs hold the API's description (OpenAPI `info.description`, the AWS service's documentation).
- **Stream a file** over WebSocket, TCP, UDP, MQTT and Socket.IO: in **Binary** mode, choose **File**, then send it in pieces at your pace (N bytes every M ms), from any byte, for any length, once or in a loop, with live progress. WAV files are recognized and set up to play in real time (20 ms of samples per message, header skipped).
- **Stream the microphone** the same way: in **Binary** mode, choose **Microphone** and pick the device, the format (PCM 16-bit, PCM float, G.711 μ-law or A-law, Opus or AAC), the sample rate or bitrate, mono or stereo, how much audio each message carries, an optional WAV header first, and the system's echo cancellation, noise suppression and auto gain. A level meter shows what is recorded.
- **Saved messages and On connect steps** for WebSocket requests: keep the messages you send often with the request (text, JSON or hex) and send them from the composer's bookmark menu; steps run after every connect to send a saved message, wait for a message (contains, is exactly, matches regex, a JSON condition such as `$.type == welcome`, or any message, within a time limit) and keep sending a heartbeat.
- **WebSocket requests run in collection runs and `zorvik run`** instead of being skipped: Zorvik connects, runs the On connect steps and closes. Each wait is a test (in the JSON and JUnit reports too), and the response body is the conversation as JSON.
- **More ways to send on a WebSocket:** send the composer's message every second, every 5 or 30 seconds or at your own pace (variables filled in fresh each time; TCP and UDP connections can too), send a ping, close with a code and reason (**Close with a code…**), and ask for subprotocols in **Settings**. The connected line shows the subprotocol the server picked, and a close shows what its code means ("1008 · Policy violation").
- **A better message log** for WebSocket, Socket.IO, SSE, TCP, UDP, MQTT and GraphQL subscriptions: filter by text or by a JSON condition (`$.type == trade`), show only sent or received messages, show the time since the previous or the first message, export the log as JSON Lines, jump back to new messages, save a binary message to a file, and put a sent WebSocket message back in the composer. WebSocket replies show how long they took, paired with the message they answer by `$.id` (or a key you choose).
- **Socket.IO client: saved emits and On connect steps** (emit, wait for an event whose arguments match, keep emitting), **answering the server's acknowledgements** automatically, emitting every few seconds, **event chips** above the log to show one event or hide noisy ones (MQTT topics get them too), and acknowledgement times. Socket.IO requests now run in collection runs and `zorvik run`, each wait a test.
- **WebSocket mock servers:** reply rules can match a JSON condition (also in TCP, UDP and Socket.IO servers), use values of the message in the reply (`{{message.id}}`), and close the connection with a code after replying; **Scheduled messages** go to every client on a timer, such as a price ticker with `{{$randomPrice(5, 50)}}`, and Socket.IO servers emit **scheduled events** to a namespace the same way.
- **Mock API answer sequences:** a route can answer its first calls differently, in order (for example `503`, `503` with `Retry-After`, then its own `200`), to test how a client retries; each answer can also drop the connection, never answer or answer 500. **Start over after the route's answer** makes a backend that keeps failing now and then. The traffic log shows which answer each call got (**answer 2 of 3**).
- **Mock APIs read JSON bodies:** a route can match a JSON condition on the request body (**Match only when → JSON body**, e.g. `$.user.role == "admin"`), and answers can use its values: `{{request.json.user.id}}`, `{{request.json.items[0]}}`. Like every request value, they are inserted as sent and never expanded.
- **Record a real backend as a mock:** with **Forward to a backend** on, **Save as route** on a forwarded exchange in the traffic log (or **Save proxied answers as routes** for all of them) adds routes that answer like the backend did: method, path, query conditions, status, headers and body. Credentials stay out of the server file: `Set-Cookie` and credential headers are left out, and query values such as `api_key` or `access_token` and tokens in `Location` addresses are left empty, each with a note.
- **TCP relay faults** for resilience tests: a delay per chunk, a bandwidth limit (bytes per second), and dropping both connections after a time or a number of bytes.
- **Event stream servers** can send a `retry:` time and end the stream after the last event, so clients reconnect and continue after their `Last-Event-ID`; a client that already has every event gets `204`, which tells an `EventSource` to stop.
- **AI agents can use live connections:** `open_connection` (running the On connect steps of WebSocket and Socket.IO requests), `send_message`, `wait_for_message`, `read_messages` and `close_connection` work with WebSocket, Socket.IO, TCP, UDP and MQTT connections. A connection an agent opens shows live in a tab while you follow agents, and closes when the agent disconnects. `save_requests` takes the new WebSocket and Socket.IO fields, and `save_server` the new server settings (`matchJson`, `sequence`, `sequenceRepeat`, relay `delayMs`/`bytesPerSecond`/`dropAfterMs`/`dropAfterBytes`, SSE `retryMs`/`closeAfterLast`, `schedule`), checking JSON conditions and sequence statuses before saving.
- **Stop a load test early when a threshold keeps failing:** **Stop early** on a threshold stops the run once it has failed for that many seconds in a row (`abortAfterSecs`), so a system that is down stops getting load and CI gets its answer sooner. The run fails, and the results, notification, reports and `zorvik load` (exit code 1) say which threshold stopped it.
- **Warm-up for load tests:** requests that finish in the first seconds (`warmUpSecs`) are charted, shaded, but left out of the numbers and thresholds, so cold caches and filling connection pools don't skew the percentiles.
- **Random think time:** **Random up to (ms)** (`thinkTimeMaxMs`) makes each virtual user's pause random between the two values, so users don't send in lock-step.
- **CSV exports of load test runs:** one row per second (the charts' data) or one row per request with its status codes and network errors, from **Export** in the results and with `zorvik load --csv-seconds` / `--csv-requests`.
- **Status codes and errors per request** in load test results: pick a request above the two panels to see which one returns the 503s or times out; the status codes panel shows the share of each class (2xx, 5xx…), and the HTML report lists each request's status codes and network errors.
- **Four new tools** in the Tools rail. **Certificate decoder**: paste PEM (a certificate, a whole chain, a CSR, keys) or open a `.pem`/`.crt`/`.der` file to see names, issuer, validity, key, usages and fingerprints, copy one certificate out of a chain, check the chain's order, and see which certificate a private key belongs to (the key itself is never shown). **JSON compare**: what was added, removed or changed between two JSON documents, path by path, ignoring key order and formatting, with arrays aligned like a text diff (or compared in any order); take either side from a recent response with **From history**. **Filter playground**: try JSONPath, jq or XPath on JSON or XML you paste or pick from history, as you type, with examples to start from. **Regex tester**: matches and groups highlighted as you type, with the same Rust regex syntax reply rules, wait steps and load test captures use, and what a capture would take.
- **Docs show as formatted text:** the Docs of requests, folders, the workspace, servers and load tests open in **Preview** (headings, lists and task lists, tables, code, quotes and `> [!NOTE]` notes, links) when they have text, and in **Edit** when empty; switch with **Preview / Edit**, a double-click or Enter. HTML in docs is shown as written, links open in your browser (http, https and mailto only), and pictures on the web aren't downloaded.
- **Docs for the workspace, servers and load tests:** a **Docs** tab in Workspace settings, and a **Docs** section at the bottom of a server's and a load test's settings (agents could already write them).
- **Edit an example's headers:** add, change, switch off or remove them in the example's new **Headers** view; mocks answer with the ones switched on. **Beautify** indents a JSON example body (numbers kept as written).
- **Training Bootcamp:** two new lessons, **WebSocket workflows** (saved messages, On connect steps, reply times, heartbeats, close codes, WebSocket tests in runs) and **Mock a WebSocket server** (JSON rules, `{{message.id}}`, scheduled messages, closing by rule).

### Changed
- An OpenAPI operation that asks for particular OAuth 2.0 scopes now imports with those scopes, instead of every scope its scheme lists.
- `pm.expect` rejects words chai doesn't know, as Postman does: a typo such as `.to.be.ture` used to pass and now fails with *Invalid Chai property: ture. Did you mean "true"?*
- Values scripts set keep their type (numbers, arrays, objects) from one script to the next within a send or a run, as in Postman; `{{name}}` still uses the text.
- Postman import sends what Postman sends: an old `Authorization` header saved next to an auth setting is switched off (with a note), and a body on GET, HEAD, COPY, PURGE or UNLOCK (unless `disableBodyPruning` is on) or a disabled body is kept with the body type set to None. Request settings Zorvik doesn't have are listed in a note.
- Names an import takes from long descriptions are cut to 120 characters instead of failing the import.
- **Save as example** keeps a JSON body indented with two spaces; numbers (even ones too large for JavaScript), text and key order stay exactly as sent.
- A tab's message log keeps the newest 5,000 messages and at most about 64 MB of them (the newest one always), and stays fast when busy: rows don't redraw on every incoming batch, and a filter only checks new messages.

### Fixed
- A response with a line of megabytes (an image a JSON API returns as base64, such as OpenAI's image generation) no longer freezes the app for up to a minute when its body is shown, or again when you come back to the Body tab: a line longer than 10,000 characters shows its start and end, with **Show more** for the rest. Copying, saving and filtering still use the whole body. Editors (request bodies, examples) draw long lines the same way.
- AI agents no longer see secret values in a collection run's failures or in the names of undefined variables a send reports.
- Requests, folders and environments with long names in scripts whose letters take several bytes (such as Chinese or emoji) are saved, instead of failing on file systems' name length limit.
- Postman import: OAuth 2.0 auth whose access token is a `{{variable}}` (one a script fetches) becomes a Bearer token with that variable, as Postman sends it; environment variables marked disabled stay off.
- Files saved as UTF-16 (with a byte order mark, as some Windows tools save them) import.
- An OpenAPI document in YAML whose keys are sorted, so `openapi:` comes far down the file, is recognized.
- Mock routes for paths with a custom method after a parameter (`/models/:model:generateContent`, as in Google APIs) match only their own method, instead of the first such route answering them all.
- Importing a collection whose environment has the same name as an older one keeps the new environment's secret values in the new environment.
- An example's **Status** can be typed as usual: typing `404` over `200` no longer jumps to 100 or 599 on the first digit.
- Number fields (intervals, delays, timeouts, close codes, load test settings) can be cleared and retyped: a value out of range becomes the nearest allowed one when you leave the field or press Enter, and the arrow keys step it.
- Load test results name every network error in plain words (a broken connection, a stop cutting a request off, a request sent to another host), instead of showing some as raw codes.

## [0.2.1] - 2026-10-04

### Fixed
- Variable suggestions (after typing `{{`) no longer run off the window: long examples, such as lorem ipsum text, are cut short.
- Training Bootcamp: where a step asks you to type into a field that doesn't take variables (a port, Tools → Ping, Port check and TLS inspector, environment values, import and mock-from-OpenAPI URLs), the Lab Guide now shows the lab's actual value instead of `{{variable}}`.
- Script timers (`setTimeout`, `setInterval`) on Windows no longer run a few milliseconds early or get skipped.

## [0.2.0] - 2026-09-29

### Added
- **Automatic updates** from GitHub Releases. Zorvik checks a little after it starts and every six hours, downloads a new version in the background, and installs it when you quit, or when you choose **Restart now** (it first tells you what a restart would stop). Settings → Updates: automatic, notify only, or off; stable or nightly channel. The check only reads this project's release list on GitHub and sends nothing about you. The Windows installer, the macOS app and the AppImage update themselves; the portable zip, `.deb` and `.rpm` tell you when a new version is out. If an update can't install itself, Zorvik links to its release page instead of showing an error.
- **More auth types**, each with its own form and imported from Postman: OAuth 1.0 (HMAC-SHA1/256/512, RSA-SHA1/256/512, PLAINTEXT), JWT that Zorvik signs from your claims (HS, RS, PS and ES algorithms), AWS Signature v4 (headers or a presigned URL), Hawk, Akamai EdgeGrid and Atlassian ASAP, all signed fresh for every send; Digest (MD5 and SHA-256, qop auth and auth-int) and NTLMv2, which answer the server's challenge; and the OAuth 2.0 implicit grant. Load tests sign every request separately.
- **Filter responses** with JSONPath, jq or XPath (the funnel icon above the body). JSONPath and jq run on the whole body; AI agents can pass a `filter` to `send_request`.
- **Save responses as examples:** kept in the request (its Examples tab), imported from Postman's saved responses, and answered by mocks built from the request (examples saved with query parameters answer only those).
- **170 dynamic variables**: every one of Postman's (`{{$randomFirstName}}`, `{{$randomCity}}`, …) plus modern IDs (`$uuidv7`, `$ulid`, `$nanoid`), valid test card numbers, IBANs, ISBNs and EANs, dates relative to now, and arguments: `{{$randomInt(1, 100)}}`, `{{$timestamp(+1h)}}`, `{{$randomFrom(a, b, c)}}`. They work in requests, scripts and mock servers; typing `{{$` shows each with an example.
- **Scripts:** `pm.sendRequest` (with a callback or `await`, which now works at the top level), `pm.cookies`, `pm.cookies.jar()` and `pm.response.cookies`, timers (`setTimeout`, `setInterval`), `pm.visualizer` (a Visualize tab next to the response), `pm.execution.skipRequest()`, and `pm.response.to.have.jsonSchema`.
- **Training Bootcamp:** lessons an update adds show as **New**, and everything you finished stays finished.
- **Script libraries, as in Postman:** `require('lodash')`, `crypto-js` (and the `CryptoJS` global), `moment`, `ajv` (JSON Schema draft-07, 2019-09 and 2020-12), `uuid`, `tv4`, `chai`, `csv-parse/lib/sync`, `xml2js` (and `xml2Json`), `cheerio`, `handlebars`, and Node's `buffer`, `events`, `path`, `querystring`, `url` and `util`. They are built into Zorvik and work offline; `pm.require('npm:name@version')` gives the built-in copy. Scripts also get `crypto.getRandomValues` and `crypto.randomUUID`.
- **GraphQL subscriptions:** a `subscription` operation streams its results live, over WebSocket (`graphql-transport-ws`, or `subscriptions-transport-ws` for older servers) or Server-Sent Events (graphql-sse), with its own URL and connection params when needed. Collection runs, `zorvik run` and AI agents read subscriptions like event streams, and their tests get the results.
- **MCP testing: call, test and mock the tools AI agents use.** A new request kind, **MCP call**, connects to MCP servers over Streamable HTTP, the older HTTP+SSE transport (picked by itself for servers that need it), or stdio: programs Zorvik starts, such as `npx -y @modelcontextprotocol/server-everything`. Connect to see the server's tools with their input schemas, resources, resource templates, prompts and instructions; pick one and its arguments start from the schema; the answer shows as an AI app gets it (text, images, audio, structured content, failed calls and JSON-RPC errors), and the Messages tab shows every JSON-RPC message with a program's own log. MCP calls are requests like any other: tests on the answer, history, collection runs and `zorvik run`. Headers, auth (OAuth 2.0 included) and secret variables in a program's environment work too.
- **MCP servers:** build one from settings (tools with input and output schemas and templated answers, failed calls and delays, resources and templates, prompts, instructions) for AI apps and MCP clients, over Streamable HTTP, HTTP+SSE, or stdio with `zorvik serve <workspace> <server> --stdio`. Every call shows in its traffic panel, and edits reach connected clients at once (`list_changed`).
- **Programs from workspaces run only once you allow them:** the first time an MCP request starts a program, Zorvik shows the exact command, folder and environment and asks; it asks again when any of them changes. `zorvik run` starts programs only with `--allow-programs`.
- AI agents can send MCP calls, list what an MCP server offers (`mcp_catalog`, the 38th tool) and build MCP servers (`save_server` with kind `mcp`); they're asked before every program they start.
- **Training Bootcamp: MCP: tools for AI agents**, a new unit of five lessons: why MCP exists, calling an MCP server, testing one, building one, and using MCP safely, with three labs.
- **Socket.IO client:** connect to Socket.IO 3 and 4 servers over WebSocket or long-polling (it falls back by itself), join a namespace with an auth payload, and emit events with JSON or binary arguments, asking for acknowledgements.
- **Socket.IO servers** that socket.io-client apps connect to (long-polling, WebSocket and the upgrade between them): echo, rules with acknowledgements, replies and broadcasts, a greeting event, and emits from the traffic panel.
- **Copy as code in 16 languages:** JavaScript (fetch, axios), Python (requests, HTTPX), Go, Java, Kotlin, Swift, C#, PHP, Ruby, Rust, Dart, C (libcurl), PowerShell, HTTPie and Wget, besides cURL. The dialog lists them with search and highlighting, and remembers your choice. Comments at the top say what the code can't do that Zorvik does, such as answering a Digest challenge or signing each request.
- **The command line on its own** for CI machines and servers: `zorvik-cli-windows-x64.zip`, `zorvik-cli-macos-universal.tar.gz` and `zorvik-cli-linux-x86_64.tar.gz` in every release.
- `PROJECT.md` holds the project's rules for contributors and AI coding agents; `CLAUDE.md`, `AGENTS.md` and `GEMINI.md` point to it.

### Changed
- Saving a running server applies the change to it at once (a new address, port, TLS setting or kind still needs a restart).
- The website shows the docs of the latest release; the docs for the next version (what nightly builds have) are at `/zorvik/next/`.
- `{{$isoTimestamp}}` has millisecond precision, like Postman, and `{{$randomEmail}}` gives name-based addresses at `example.com`, `example.net` and `example.org`.
- Errors thrown in promise callbacks and timers now fail the script instead of being lost.
- The website and README lead with what coding agents can do through MCP, and that every feature is free (no paid plans, no account, no cloud sync). The website no longer shows a download count.

### Fixed
- **Checking for updates** no longer fails with "Could not reach GitHub" when one of GitHub's download servers doesn't answer from your network: Zorvik tries the next one after a few seconds. A slow update download is no longer cut off after 30 seconds, and the log says why a check failed.
- Load test HTML reports: pointing at a chart (or tapping it, or focusing it and using the arrow keys) shows a guide line and a tooltip with the time and every value, like in the app. The report is still a single file with no external resources.
- Load tests: requests dropped at the in-flight limit (request-rate model) now count as failed in the error rate, so a run that couldn't start half its requests no longer passes an `errorRate` threshold and `zorvik load` exits with 1.
- Load tests: throughput (requests per second) is measured over the time requests were being started, so a request still in flight at the end no longer lowers it (and fails an `rps` threshold) while the run waits for it.
- Load tests: a captured value can no longer move requests to another port, scheme or virtual host (`Host` header) of a host the run was started for, and a captured value over 64 KB counts as a capture miss instead of being kept. Responses kept for captures no longer set aside memory for the size a server claims.
- **AI agents** can no longer reach an unapproved host through a WebSocket (a GraphQL subscription), TCP, UDP or MQTT connection, or through the name of a DNS query (resolvers pass it on), and exported code and server traffic hide credentials (a Basic header, cookies, tokens) like secret values. Starting a relay or a forwarding mock shows where it forwards to.
- Digest and NTLM credentials are no longer sent to another site a redirect leads to; Digest works with servers that close the connection after their challenge; the body of a challenge is read only up to 1 MB; HTTP/3 says that Digest and NTLM need HTTP/1.1 or HTTP/2 instead of ignoring them.
- A proxy password no longer appears in the error for a proxy URL that can't be used, and `settings.json` and cookie jars are readable by you only.
- WebSocket and MQTT sessions end when the other side stops reading for a minute, instead of hanging; Socket.IO limits the attachments of one event (64, and 64 MB) and a server's ping settings.
- Servers keep at most 1,024 connections open each, and the app and `zorvik` may open more files than macOS allows a Finder-started app by default.
- Socket.IO servers no longer answer browsers' CORS preflights when CORS is off.
- The file watcher doesn't follow symbolic links in a workspace (one pointing at `/` made it walk the whole disk).
- Two requests (environments, servers, load tests) created with the same name at the same moment, say by you and an AI agent, no longer overwrite each other.
- Scripts: a timer set for after the time limit no longer runs early at the limit; a rejected promise whose reason itself rejects no longer loses the run's results; `pm.sendRequest` responses reach scripts cut at 16 MB.
- A crafted OAuth 1.0 RSA key can no longer crash the app or keep it busy.
- Response filters: jq refuses responses nested more than 128 levels deep and stops expressions that recurse or loop without end, instead of crashing the app. OpenAPI contract checks stop in time on schemas that refer to themselves over and over, and resolve `$ref`s with escaped characters (`#/paths/~1pets`).
- cURL: binary bodies are exported byte for byte (through base64 in bash, cmd and PowerShell); `--digest` and `--ntlm` import as Digest and NTLM auth; `--data-urlencode 'name=content'` keeps an `@` in the name.
- An update that fails to install on Windows leaves the connection for AI agents running.
- The app: servers an AI agent starts show as running (with their traffic); ⌘/Ctrl+K no longer replaces a dialog with unsaved changes; the Settings dialog asks before discarding changes; Stop all keeps a server listed when it couldn't be stopped; Save as example no longer saves edits typed while it works; closing a new GraphQL tab doesn't ask about unsaved changes; the URL bar's Cancel also ends a gRPC stream; tabs can be reached and switched with the keyboard, and request settings have labels screen readers read.

## [0.1.2] - 2026-09-28

0.1.1 was never published on its own: its changes ship here.

### Added
- **Website and developer docs** at <https://libreguild.github.io/zorvik/>: every feature explained, with the complete scripting (`pm`) and command-line references and the workspace file format. Download buttons pick your system and always point at the newest release.
- **Training Bootcamp:** a course inside Zorvik, from networking basics to load testing. 16 units of short lessons with diagrams, hands-on labs in the real workbench (practice servers start on your computer; a Lab Guide checks each step, with hints and "Do it for me"), quick checks, XP, levels, streaks, badges and a graduation certificate. Open it from **Training Bootcamp** at the top of the workspace menu or on the welcome screen; the Training Bootcamp workspace is always there and can be reset.
- AI agents can create and change mock servers (`read_server`, `save_server`), build a mock from a folder or an OpenAPI document (`create_mock`), start a server on any free port, and read what it received (`get_server_traffic`).
- AI agents can read Server-Sent Events streams with `send_request`, read several requests at once, set query parameters (also switched-off ones, with descriptions) and path parameters, read current variables (`get_variables`) and history (`read_history`), export a request as code (`export_request`), and add a small file to the workspace (`write_file`).
- Copy a request as Kotlin (OkHttp), Swift (URLSession), JavaScript (fetch) or Python (requests) code, next to cURL.
- Collection runs: "Repeat until" sends a request again until a condition holds (polling), and Server-Sent Events requests run, with `pm.response.events` in scripts.
- OpenAPI: imported requests are checked against the spec on every send and run (a "Matches the API spec" test); "Update from API spec" brings in a new version with a preview, keeping your edits and marking operations that were removed.
- OpenAPI import: realistic example values, path parameters become variables (`{session_id}` → `{{sessionId}}`), and a document without a full server URL asks for the base URL.
- A port that's already in use names the program holding it.
- Load tests: a data file gives each virtual user its own row; captures (JSON path, header or regex) pass values from a response to the same user's next requests; timing shows time to first byte, transfer, connect and `Server-Timing`; Compare shows a run next to an earlier one.

### Changed
- Nightly builds come once a day, when `main` has changed, instead of after every merge.
- Tools that save requests, servers and load tests refuse unknown or misspelled fields, with a suggestion, instead of ignoring them. Their input schemas document every field, unit and placeholder.
- Load-test threshold units are documented (`errorRate` is a percent, 0-100); `waitSeconds: 0` returns the run id at once.
- Collection runs say why a request was skipped.

### Fixed
- The app's Runner tab now sends event-stream (SSE) requests too, like `zorvik run` and agents do.
- Load tests: an arrival-rate run could drop its last request on Windows (a timer woke too late and the end of the run won).
- Long menus now fit the window and scroll.
- `save_environment` reported `active: false` for an environment that was active.

## [0.1.0] - 2026-09-27
The first public release.

### Added
- HTTP/1.1, HTTP/2 and HTTP/3 requests with a timing waterfall, TLS details, redirects, cookies and the headers really sent.
- GraphQL with schema autocomplete and a schema explorer; gRPC with server reflection or `.proto` files and every streaming mode.
- WebSocket, Server-Sent Events, TCP, UDP, MQTT and DNS clients.
- Environments, variables and secrets that stay on your computer; Basic, Bearer, API key and OAuth 2.0 auth; proxies and client certificates.
- Postman-compatible scripts and tests, and a collection runner with CSV/JSON data and JSON/JUnit reports.
- Load testing with virtual users or arrival rate, stages, thresholds, a live dashboard and HTML/JSON reports.
- Mock APIs (from scratch, a folder, OpenAPI or a response) and WebSocket, SSE, TCP, UDP and DNS servers, plus a TCP relay.
- Network tools: TLS inspector, DNS lookup, port check, ping, network interfaces, HTTP/3 check and encoders.
- Import from Postman, OpenAPI/Swagger and cURL; export to cURL.
- The `zorvik` command line (`run`, `load`, `serve`, `mcp`) inside every install.
- AI agents control Zorvik over MCP, with confirmations for risky actions.
- In-app docs, light and dark themes, zoom and font settings.
- Installers for Windows, macOS (universal) and Linux (deb, rpm, AppImage).

[Unreleased]: https://github.com/LibreGuild/zorvik/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/LibreGuild/zorvik/compare/v0.2.1...v0.3.0
[0.2.1]: https://github.com/LibreGuild/zorvik/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/LibreGuild/zorvik/compare/v0.1.2...v0.2.0
[0.1.2]: https://github.com/LibreGuild/zorvik/compare/v0.1.0...v0.1.2
[0.1.0]: https://github.com/LibreGuild/zorvik/releases/tag/v0.1.0

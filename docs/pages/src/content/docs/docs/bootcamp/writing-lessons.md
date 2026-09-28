---
title: Writing lessons
description: How the Training Bootcamp course is built, and how to write a lesson, a lab, its checks and solutions, and test it.
sidebar:
  order: 2
---

The [Training Bootcamp](../training-bootcamp/) course is plain files in the Zorvik repository: a folder per unit, a Markdown file per lesson. This page is for contributors who want to add or improve a lesson. It follows [`docs/academy.md`](https://github.com/LibreGuild/zorvik/blob/main/docs/academy.md) in the repository.

## How it fits together

```text
crates/academy/course/          The course: one folder per unit (unit.yaml + NN-lesson.md files)
crates/academy/                 Loads the course (embedded at build time), pattern matching, progress rules
crates/api/src/academy/         academy.* API: the Bootcamp workspace, labs (servers, checks), progress file
app/src/components/academy/     Academy view (course map, lessons, rewards), Lab Guide, diagrams
crates/api/tests/academy.rs     Runs every lab step's solution and checks the step
```

- The course is **embedded at build time**: every `.md` and `.yaml` file under `crates/academy/course/` is compiled into the app. New files are picked up by the build; no code changes.
- Folders and files are read in **name order**, so number them: `03-http/`, `01-requests-and-responses.md`.
- The course is **validated** when it loads, and the tests fail on any mistake, naming the file.
- **A lab** saves its servers into the Bootcamp workspace as `Lab · <name>`, starts them on free ports, fills the **Lab** environment and makes it active. While it runs, every app call is noted in a short journal; after each call and once a second, the steps are checked in order from the first one not done, each looking at everything since the lab started.

## Files

```text
crates/academy/course/03-http/unit.yaml
crates/academy/course/03-http/01-requests-and-responses.md
crates/academy/course/03-http/02-methods.md
```

### `unit.yaml`

```yaml
id: http                      # unique, kebab-case
title: HTTP basics
summary: One sentence.
color: "#3A9A5B"              # the unit's accent colour
image: http                   # app/src/assets/academy/unit-http.webp
badge:
  id: status-sage             # unique across all badges
  name: Status Sage
  description: Reads requests, responses and status codes like a pro.
capstone: false               # true only for the final project
```

| Field | Required | Notes |
|---|---|---|
| `id` | yes | Unique among units. |
| `title`, `summary` | yes | Shown on the course map. |
| `color` | yes | The unit's accent. |
| `image` | yes | The art file `app/src/assets/academy/unit-<image>.webp`. |
| `badge` | yes | `id`, `name`, `description`. The id must not clash with another unit's badge or the extra badges. |
| `capstone` | no | `true` for the final project: finishing it graduates the learner. It can't be tested out of. At most one unit. |

A unit must have at least one lesson. Unknown fields are refused.

### A lesson

YAML front matter between `---` lines, then Markdown:

```markdown
---
id: status-codes              # unique across the course
title: Status codes
summary: One sentence, the idea of the lesson.
minutes: 5                    # reading time
added: 0.2.0                  # only for a lesson added to a released course: the release that adds it
lab: { … }                    # optional, see below
quiz:                         # usually 3 questions
  - question: What does 404 mean?
    options: [The server crashed, Nothing was found at that address, You must log in]
    answer: 1                 # index of the right option, from 0
    explain: Why, in one or two sentences.
---
The reading…
```

| Field | Required | Notes |
|---|---|---|
| `id` | yes | Unique across the whole course. |
| `title`, `summary` | yes | |
| `minutes` | yes | Reading time. |
| `lab` | no | See [Labs](#labs). |
| `quiz` | no | Questions with `question`, `options` (two or more), `answer` (a valid index) and `explain`. |

A lesson without a lab and without a quiz gets a **Mark as done** button. A lesson completes when its lab is done and its quiz is passed (60 %, rounded up).

When you add a lesson to a course people already use, set `added` to the release that ships it: learners who started before see it marked **New** until they open it, and everything they finished stays finished. Keep lesson ids stable, because progress is kept by id.

## Style

- Plain, friendly words. Short paragraphs. Explain every term the first time it appears.
- 350 to 700 words. At least one diagram. One `> [!note] Think of it like…` analogy where it helps.
- End with **You'll use this when…** tied to real work.
- Refer to the app as it is: menu names, buttons and shortcuts that exist.

## Markdown the Academy shows

- `##` and `###` headings, paragraphs, `**bold**`, `*italic*`, `` `code` ``, `[links](https://…)` (they open in the browser).
- Lists (`-` and `1.`), GitHub tables, fenced code blocks (`json`, `http`, `bash`, `js`, `yaml`, `text`).
- Callouts: `> [!note] Title`, `> [!tip] Title`, `> [!warning] Title`, followed by `>` lines.
- `` `{{name}}` `` shows a variable (its value while a lab runs).

### Diagrams

Four kinds, in fenced blocks:

````markdown
```sequence
participants: You, Resolver, Server
You -> Resolver: Where is shop.example?
Resolver --> You: 93.184.216.34
Note over Resolver: checks its cache
```
````

| Diagram | Syntax |
|---|---|
| `sequence` | `participants:` (optional; otherwise in order of appearance), `A -> B: text` for a message, `A --> B: text` for a dashed answer, `Note over A: text` or `Note over A, B: text`. |
| `flow` | Boxes joined by arrows, one row per line: `Browser -> DNS -> Server`; a label on an arrow: `Client -[TLS]-> Server`. |
| `layers` | Top to bottom, one layer per line: `Application \| HTTP, DNS, WebSocket`. |
| `anatomy` | One part per line with its explanation: `GET /users?page=2 HTTP/1.1 \| request line: method, path, version`. |

## Labs

```yaml
lab:
  title: Break things on purpose
  goal: Make the server answer 404 and 500, and read what each means.
  minutes: 8
  servers:                    # started for the lab; {{api}} holds the address
    api:                      # id: a variable name (letters, digits, _)
      name: Shop API          # saved as "Lab · Shop API"
      kind: http              # http, mcp, websocket, socketio, sse, tcp, udp, dns, tcpProxy
      http:
        routes:
          - { method: GET, path: /products/:id, status: 200, body: '{"id": "{{request.params.id}}"}' }
  playground: true            # or { http: true, tls: true, grpc: true }
  vars: { token: "{{secret.token}}" }                 # more variables in the Lab environment
  files: { "data/users.csv": "id,name\n1,Ada\n" }     # written into the workspace
  steps:
    - text: Send `GET {{api}}/products/7`.            # Markdown
      hints: [a nudge, where to click, exactly what to do]
      check: { request: { server: api, method: GET, path: /products/7 } }
      solution:
        - send: { method: GET, url: "{{api}}/products/7" }
```

| Field | Required | Notes |
|---|---|---|
| `title`, `goal`, `minutes` | yes | Shown on the lab card and in the Lab Guide. |
| `servers` | no | Map of id → server in the [saved-server format](../../reference/workspace-format/#servers-serversyaml). Ids must be variable names; each server needs a `name`. Ports are picked by the lab. |
| `playground` | no | `true` (HTTP playground) or `{http, tls, grpc}`. |
| `vars` | no | More variables for the Lab environment. |
| `files` | no | Path → content, written into the workspace with placeholders filled in. |
| `steps` | yes | At least one. Each has `text` (Markdown), `hints` (at most three), `check` and `solution` (at least one action). |

- **Servers**: each server `x` gives `{{x}}` (its URL), `{{x_port}}` and `{{x_host}}` (`127.0.0.1:port`).
- **Placeholders** in servers, vars, files, checks and solutions: server ids, `playground`, `playgroundTls`, `grpc`, the lab's vars, and `{{secret.<name>}}`, a random value such as `falcon-4821` made when the lab starts (so answers can't be copied). Mock templates like `{{request.params.id}}` are left for the server.
- **The playground** (from `crates/testkit`): `/echo`, `/anything/*`, `/status/{code}`, `/redirect/{n}`, `/delay/{ms}`, `/gzip`, `/cookies`, `/cookies/set?name=value`, `/basic-auth/{user}/{pass}`, `/bearer`, `/json`, `/sse`, `/ws`, `/graphql`, and OAuth 2.0 at `/oauth/token`. `{{playgroundTls}}` is the same over HTTPS with a certificate from a private authority, written to `lab-ca.pem` in the workspace. `{{grpc}}` is the `zorvik.test.v1.Echo` service with server reflection.
- **Binary servers**: reply rules with `hex` encoding match raw bytes; start their regular expressions with `(?-u)` so `\x82` means the byte, not a Unicode character. `message` checks see text payloads only; check binary exchanges with a `call` check on `socket.send`.

## Checks

A step passes when its check matches something that happened since the lab started. Patterns are plain YAML values:

| Pattern | Matches |
|---|---|
| An object | When every listed key matches (keys without regard to case; other keys are ignored). |
| An array | When every listed item matches some item. |
| A string | Without regard to case, with `*` wildcards. |
| `re:<regex>` | A regular expression. |
| `>=10`, `<200`, `!=0` | A number comparison. |
| `2xx`, `4xx` | A status class. |
| `!<pattern>` | Not the pattern. `"!*"` means missing. |

| Check | Matches | What it looks at |
|---|---|---|
| `request: {server, …}` | An HTTP request a lab server received | `method`, `path`, `query` (object), `headers` (lower-case names), `body`, `json` (parsed body), `status` (the mock's answer), `route`, `httpVersion`; `count: 2` = at least two |
| `message: {server, …}` | A message, connection or DNS query a lab server saw | `kind` (`data`, `open`, `close`, `dns`, `info`, `error`), `direction` (`in`, `out`), `text`, `summary`, `size` |
| `send: {…}` | An HTTP request sent from Zorvik and its answer | `kind`, `method`, `url` (as sent), `finalUrl`, `status`, `httpVersion`, `headers` (sent), `responseHeaders`, `body`, `json`, `tests` (`[{name, passed}]`), `testsPassed`, `testsFailed`, `visualized` (a script called `pm.visualizer.set`), `redirects`, `tls`, `auth` (type), `request` (as written, variables unresolved), `error`, `errorCode` (e.g. `skipped`), `errorKind` |
| `call: {method, params, ok, result, error}` | Any app action (methods and parameters in `app/src/lib/rpc.ts`) | e.g. `{method: dns.query, params: {request: {url: "shop.lab.test"}}, ok: true}` |
| `saved: {request \| folder \| environment \| server \| loadTest \| workspace: …}` | Something saved in the workspace (the file's fields, plus `path`, `id`, `running` for servers) | e.g. `{request: {name: Login, auth: {type: bearer}}}` |
| `run: {…}` | A collection run that finished during the lab | `passed`, `requests`, `failed`, `testsPassed`, `testsFailed`, `iterations`, `name` |
| `load: {…}` | A load test run that finished during the lab | `passed`, `requests`, `rps`, `p95`, `errorRate`, `stoppedEarly`, `name` |
| `probe: {kind, name?, method?, path?, headers?, body?, expect}` | The lab calls a server the learner runs (`http`, `tcp`, `udp`, `mcp`; not a lab server) | `expect` on `{status, headers, body, json}`, `{text}`, or for `mcp` (connects over Streamable HTTP and asks `method`, default `tools/list`, with `body` as the JSON parameters) `{result}` or `{error}` |
| `answer: "…"` or `answer: [a, b]` | What the learner types in the Lab Guide | e.g. `answer: "{{secret.word}}"` |
| `all: [...]`, `any: [...]` | Every one / one of the checks | |

Rules:

- `request` and `message` checks must name a server the lab starts.
- `send` and `call` only see actions from the app's UI, not requests a collection run sends. Check those with `request` or `run`.
- A step's check must **not** pass before its solution runs (the course tests fail otherwise), so each step needs something new. Every check sees everything since the lab started: make each step ask for something new, or use `count` on server traffic.
- Big results that are never useful are not kept for checks: `graphql.schema`, `grpc.describe`, `workspace.*`, `load.run`, `response.save`.
- A failed send reports the URL as written (`{{api}}/x`), a successful one the URL as sent. Match both with `*/x`.

## Solutions

What **Do it for me** runs, and what the course tests run:

| Action | Does |
|---|---|
| `send: {method, url, headers, body, auth, scripts, settings, kind, …}` | Sends a request (the saved-request format; `name` and `seq` may be left out). It goes out without a saved path; to send a saved request (folder auth, OpenAPI checks), `call` `http.send` with its `path`. |
| `save: {name, method, url, …}` | Saves a request at the top of the collection. |
| `call: {method, params}` | Any app method, e.g. `{method: env.create, params: {environment: {name: Staging, variables: []}}}`. |
| `answer: "{{secret.word}}"` | Types an answer. |
| `wait: 500` | Waits, in milliseconds. |

Placeholders in solutions are filled in before they run, except in requests, folders and servers they save (`save`, and `call` to `request.*`, `folder.*`, `server.create` / `server.save`, `mock.addRoute`, `workspace.saveMeta`): those keep `{{variables}}` as a learner's would, and only `{{secret.*}}` is filled in. Placeholders are always text (`"{{x_port}}"` is a string).

## Good to know

- Starting a lab stops the servers the learner ran in earlier labs (they stay saved) and clears values scripts set in the Lab environment, so every lab starts clean. Cookies are shared: a lab that sets one should delete it.
- Lab `files` are written with placeholders filled in.
- **Do it for me** earns no XP for its step, and a lab finished with it doesn't give the No Hints Needed or Speed Runner badges.

## Test a lesson

```bash
cargo test -p zorvik-academy                                        # the course loads; progress rules
ACADEMY_LESSON=status-codes cargo test -p zorvik-api --test academy  # one lab, step by step
cargo test -p zorvik-api --test academy                              # every lab
```

The lab test runs each step's solution in order and checks that the step did **not** pass before it and **does** pass after it. Run the app too (`npm run tauri dev` in `app/`) and do the lab by hand: read it as a learner would, and try the hints.

Load-time checks that fail the build's tests:

- a unit or lesson id used twice, a badge id used twice (also against the eight extra badges);
- a unit without lessons, a lab without steps, a step without a solution or with more than three hints;
- a question with fewer than two options or an `answer` out of range;
- a server id that isn't a variable name, a server without a name or that doesn't parse, a check on a server the lab doesn't start;
- unknown fields anywhere in `unit.yaml` or the front matter, and a lesson without front matter.

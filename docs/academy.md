# Training Bootcamp (Academy)

A course inside Zorvik: from "what is a network?" to testing, mocking and load testing APIs. Each lesson is a short reading, a hands-on **lab** in the real workbench, and a quick quiz. Learners earn XP, levels, ranks, badges, a streak and, after the capstone, a certificate.

## How it fits together
```
crates/academy/course/   The course: one folder per unit (unit.yaml + NN-lesson.md files)
crates/academy/          Loads the course (embedded at build time), pattern matching, progress rules
crates/api/src/academy/  academy.* RPC: the Bootcamp workspace, labs (servers, checks), progress file
app/src/components/academy/  Academy view (course map, lessons, rewards), Lab Guide, diagrams
```
- **The Bootcamp workspace** is a normal workspace in the app data folder (`bootcamp/`). It is always listed first (pinned in the workspace menu and on the welcome screen, which open the Academy) and can't be removed; **Reset** empties it (progress stays). Only this workspace shows the *Workbench | Academy* switch in the title bar.
- **Progress** lives in `academy-progress.json` in the app data folder, not in the workspace. It is kept by lesson and unit id, so updates that add lessons keep everything a learner finished: units stay done, badges and the certificate stay. Lessons an update adds show **New** (on the lesson, its unit and the **Continue** card, graduates included) until the learner opens them; `knownLessons` in the progress file records the lessons the learner has seen.
- **A lab** saves its servers into the workspace as `Lab · <name>` and starts them on free ports, fills the **Lab** environment (`{{api}}`, …) and makes it active. Starting another lab removes the previous lab's servers. The practice servers (the same ones the tests use, `crates/testkit`) start on first use and stay up.
- **Checks.** While a lab runs, every app call is noted in a short journal. After each call, and once a second, the steps are checked in order from the first one not done; each check looks at everything since the lab started. Rewards reach the UI only as `academy` events.
- **Nobody gets stuck.** Up to three hints per step (the last one says exactly what to do), and **Do it for me** runs the step's solution (the step then earns no XP).
- **Every lab is tested.** `crates/api/tests/academy.rs` runs each step's solution and checks that the step did *not* pass before it and *does* pass after it.

## XP, levels, badges
| Earned for | XP |
|---|---|
| A lab step (not "Do it for me") | 10 |
| A lesson complete (its lab done and its quiz ≥ 60 %, whichever it has; "Mark as done" for reading-only lessons) | 50 |
| A quiz answer right on the first try | 5 each, +20 when all are right (3+ questions) |
| A unit complete, or its test-out quiz passed (≥ 80 %) | 100 and the unit's badge |
| Graduating (the capstone unit) | 250 and the certificate |

Level *L* starts at `20·(L−1)·L` XP. Ranks: Newbie Node (1), Packet Pusher (3), Header Hacker (6), Protocol Pro (10), Network Ninja (14), Wire Wizard (18). Extra badges (`crates/academy/src/progress.rs`): No Hints Needed, Perfect Score, Speed Runner, Night Owl, Early Bird, On Fire (3-day streak), Unstoppable (7 days), Halfway There.

## Writing a lesson

### Files
```
crates/academy/course/03-http/unit.yaml
crates/academy/course/03-http/01-requests-and-responses.md
crates/academy/course/03-http/02-methods.md
```
Folders and files are read in name order. New files are picked up by the build; no code changes.

`unit.yaml`:
```yaml
id: http                      # unique, kebab-case
title: HTTP basics
summary: One sentence.
color: "#3A9A5B"              # the unit's accent
image: http                   # app/src/assets/academy/unit-http.webp
badge: { id: status-sage, name: Status Sage, description: Finished HTTP basics. }
capstone: false               # true only for the final project
```

A lesson is YAML front matter, then Markdown:
```markdown
---
id: status-codes              # unique across the course
title: Status codes
summary: One sentence, the idea of the lesson.
minutes: 5                    # reading time
added: 0.2.0                  # the release that adds the lesson (only for lessons added to a released course)
lab: { … }                    # optional, see below
quiz:                         # usually 3 questions
  - question: What does 404 mean?
    options: [The server crashed, Nothing was found at that address, You must log in]
    answer: 1                 # index of the right option
    explain: Why, in one or two sentences.
---
The reading…
```
A lesson without a lab and without a quiz gets a **Mark as done** button.

**Adding a lesson to a released course:** give it `added:` with the release that ships it. Learners who started before know every lesson without `added` and see the new ones as **New**; from then on, `knownLessons` in their progress remembers what they have seen, so later additions show as new too. Keep lesson ids stable: progress is kept by id, and a renamed id is a new lesson.

### Style
- Plain, friendly words. Short paragraphs. Explain every term the first time it appears.
- 350–700 words. At least one diagram. One `> [!note] Think of it like…` analogy where it helps.
- End with **You'll use this when…** tied to real work.
- Refer to the app as it is: menu names, buttons and shortcuts that exist.

### Markdown the Academy shows
- `##` and `###` headings, paragraphs, `**bold**`, `*italic*`, `` `code` ``, `[links](https://…)` (open in the browser).
- Lists (`-` and `1.`), GitHub tables, fenced code blocks (```` ```json ````, `http`, `bash`, `js`, `yaml`, `text`).
- Callouts: `> [!note] Title`, `> [!tip] Title`, `> [!warning] Title` followed by `>` lines.
- `` `{{name}}` `` shows a variable (its value when a lab runs).
- Diagrams in fenced blocks:

````markdown
```sequence
participants: You, Resolver, Server      (optional; otherwise in order of appearance)
You -> Resolver: Where is shop.example?  (-> a message)
Resolver --> You: 93.184.216.34          (--> an answer, dashed)
Note over Resolver: checks its cache     (a note over one participant, or "Note over A, B:")
```

```flow
Browser -> DNS -> Server                 (boxes joined by arrows; one row per line)
Client -[TLS]-> Server                   (a label on the arrow)
```

```layers
Application | HTTP, DNS, WebSocket       (top to bottom: a layer and what lives there)
Transport | TCP, UDP
```

```anatomy
GET /users?page=2 HTTP/1.1 | request line: method, path, version
Authorization: Bearer eyJ… | a header
```
````

### Labs
```yaml
lab:
  title: Break things on purpose
  goal: Make the server answer 404 and 500, and read what each means.
  minutes: 8
  servers:                    # started for the lab; `{{api}}` holds the address
    api:                      # id: a variable name (letters, digits, _)
      name: Shop API          # saved as "Lab · Shop API" (the learner can open it and see its traffic)
      kind: http              # http, websocket, sse, tcp, udp, dns, tcpProxy
      http:
        routes:
          - { method: GET, path: /products/:id, status: 200, body: '{"id": "{{request.params.id}}"}' }
  playground: true            # or { http: true, tls: true, grpc: true }
  vars: { token: "{{secret.token}}" }        # more variables in the Lab environment
  files: { "data/users.csv": "id,name\n1,Ada\n" }   # written into the workspace
  steps:
    - text: Send `GET {{api}}/products/7`.   # Markdown
      hints: [a nudge, where to click, exactly what to do]
      check: { request: { server: api, method: GET, path: /products/7 } }
      solution:
        - send: { method: GET, url: "{{api}}/products/7" }
```
- **Servers** use the saved-server format (`crates/formats/src/server.rs`; examples in `crates/api/tests/*.rs`). Ports are picked by the lab. Each server `x` also gets `{{x_port}}` and `{{x_host}}` (`127.0.0.1:port`).
- **Placeholders** in servers, vars, files, checks and solutions: server ids, `playground`, `playgroundTls`, `grpc`, the lab's vars, and `{{secret.<name>}}`, a random value such as `falcon-4821` made when the lab starts (so answers can't be copied). Mock templates like `{{request.params.id}}` are left for the server.
- **Binary servers:** reply rules with `hex` encoding match raw bytes; start their regexes with `(?-u)` so `\x82` means the byte, not a Unicode character. `message` checks see text payloads only; check binary exchanges with a `call` check on `socket.send`.
- **The playground** (`crates/testkit`, endpoints in `docs/testing.md`): `/echo`, `/anything/*`, `/status/{code}`, `/redirect/{n}`, `/delay/{ms}`, `/gzip`, `/cookies`, `/cookies/set?name=value`, `/basic-auth/{user}/{pass}`, `/bearer`, `/json`, `/sse`, `/ws`, `/graphql`, OAuth 2.0 at `/oauth/token`. `{{playgroundTls}}` is the same over HTTPS with a certificate from a private authority; its certificate is written to `lab-ca.pem` in the workspace. `{{grpc}}` is `zorvik.test.v1.Echo` with server reflection.

### Checks
Patterns are plain YAML values ([`matcher.rs`](../crates/academy/src/matcher.rs)): objects match when every listed key matches (keys without case; others ignored), arrays when every listed item matches some item, strings without case with `*` wildcards, `re:<regex>`, `>=10`/`<200`/`!=0`, `2xx`/`4xx`, and `!<pattern>` for "not" (`"!*"`: missing).

| Check | Matches | What it looks at |
|---|---|---|
| `request: {server, …}` | an HTTP request a lab server received | `method`, `path`, `query` (object), `headers` (lower-case names), `body`, `json` (parsed body), `status` (the mock's answer), `route`, `httpVersion`; `count: 2` = at least two |
| `message: {server, …}` | a message, connection or DNS query a lab server saw | `kind` (`data`, `open`, `close`, `dns`, `info`, `error`), `direction` (`in`, `out`), `text`, `summary`, `size` |
| `send: {…}` | an HTTP request sent from Zorvik and its answer | `kind`, `method`, `url` (as sent), `finalUrl`, `status`, `httpVersion`, `headers` (sent), `responseHeaders`, `body`, `json`, `tests` (`[{name, passed}]`), `testsPassed`, `testsFailed`, `visualized` (a script called `pm.visualizer.set`), `redirects`, `tls`, `auth` (type), `request` (the request as written, variables unresolved), `error`, `errorCode` (e.g. `skipped`), `errorKind` |
| `call: {method, params, ok, result, error}` | any app action (see `app/src/lib/rpc.ts` for methods and parameters) | e.g. `{method: dns.query, params: {request: {url: "shop.lab.test"}}, ok: true}` |
| `saved: {request \| folder \| environment \| server \| loadTest \| workspace: …}` | something saved in the workspace (the file's fields, plus `path`, `id`, `running` for servers) | e.g. `{request: {name: Login, auth: {type: bearer}}}` |
| `run: {…}` | a collection run that finished during the lab | its summary: `passed`, `requests`, `failed`, `testsPassed`, `testsFailed`, `iterations`, `name` |
| `load: {…}` | a load test run that finished during the lab | `passed`, `requests`, `rps`, `p95`, `errorRate`, `stoppedEarly`, `name` |
| `probe: {kind, name?, method?, path?, headers?, body?, expect}` | the lab calls a server the learner runs (`http`, `tcp`, `udp`; not the lab's own) | `expect` on `{status, headers, body, json}` or `{text}` |
| `answer: "…"` or `answer: [a, b]` | what the learner types in the Lab Guide | e.g. `answer: "{{secret.word}}"` |
| `all: [...]`, `any: [...]` | every / one of the checks | |

`send` and `call` only see actions from the app's UI (not requests a collection run sends: check those with `request` or `run`). A step's check must **not** pass before its solution runs (the course tests fail otherwise), so each step needs something new.

### Solutions
What **Do it for me** runs, and what the course tests run:
- `send: {method, url, headers, body, auth, scripts, settings, kind, …}`: send a request (the saved-request format; `name` and `seq` may be left out).
- `save: {name, method, url, …}`: save a request at the top of the collection.
- `call: {method, params}`: any app method, e.g. `{method: env.create, params: {environment: {name: Staging, variables: []}}}`.
- `answer: "{{secret.word}}"`, `wait: 500` (milliseconds).

Placeholders in solutions are filled in before they run, except in requests, folders and servers they save (`save`, and `call` to `request.*`, `folder.*`, `server.create`/`save`, `mock.addRoute`, `workspace.saveMeta`): those keep `{{variables}}` as a learner's would, and only `{{secret.*}}` is filled in. Placeholders are always text (`"{{x_port}}"` is a string). A `send` goes out without a saved path; to send a saved request (folder auth, OpenAPI checks), `call` `http.send` with its `path`.

Good to know:
- Starting a lab stops the servers the learner ran in earlier labs (they stay saved) and clears values scripts set in the Lab environment, so every lab starts clean. Cookies are shared: a lab that sets one should delete it.
- Lab `files` are written with placeholders filled in.
- A failed send reports the URL as written (`{{api}}/x`); a successful one, the URL as sent. Match both with `*/x`.
- Check results that are big and never useful are not kept: `graphql.schema`, `grpc.describe`, `workspace.*`, `load.run`, `response.save`.
- Every check sees everything since the lab started: make each step ask for something new, or use `count` on server traffic.

### Test a lesson
```bash
cargo test -p zorvik-academy                                   # the course loads, rules
ACADEMY_LESSON=status-codes cargo test -p zorvik-api --test academy   # one lab, step by step
cargo test -p zorvik-api --test academy                         # every lab
```

## RPC
`academy.course`, `academy.progress {utcOffsetMinutes}`, `academy.lesson {id}`, `academy.workspace`, `academy.reset`, `academy.startLab {id}`, `academy.stopLab`, `academy.lab`, `academy.check`, `academy.hint {step}`, `academy.answer {step, value}`, `academy.doStep {step}`, `academy.quiz {id, answers}`, `academy.markRead {id}`, `academy.testOut {id}`, `academy.testOutSubmit {id, answers}`, `academy.saveCertificate {path, png}`. Event: `academy {update: {lab, progress, rewards}}`.

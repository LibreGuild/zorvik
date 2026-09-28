---
title: Scripts overview
description: Where pre-request and post-response scripts live, the order they run in, what they can see and change, the console, and what happens when a script fails.
sidebar:
  order: 1
---

Scripts are small pieces of JavaScript that run around a request:

- A **pre-request script** runs before the request is sent. It can change the request (URL, method, headers, body) and set variables.
- A **post-response script** runs after the response arrives. It reads the response, adds tests with `pm.test`, and saves values (a token, a new id) for later requests.

Scripts use a Postman-compatible `pm` API, so collections imported from Postman keep working. The full API is in the [pm API reference](../pm-reference/), and the assertions are in [Assertions](../assertions/).

```js title="Post-response script"
pm.test("Status is 200", () => {
  pm.response.to.have.status(200);
});

const body = pm.response.json();
pm.environment.set("userId", body.id);
```

## Which requests run scripts

| Where the request is sent | Scripts run? |
|---|---|
| An HTTP or GraphQL request sent from the app | Yes |
| A collection run in the app's Runner tab (HTTP and GraphQL requests) | Yes |
| `zorvik run` (HTTP, GraphQL and Server-Sent Events requests) | Yes |
| HTTP requests an AI agent sends with `send_request`, and runs it starts with `run_collection` | Yes |
| The live Server-Sent Events tab in the app | No, only collection runs run scripts for SSE requests |
| WebSocket, gRPC, TCP, UDP, MQTT and DNS requests | No |

## Where scripts live

Scripts can be set in three places. Each place has a **Pre-request** and a **Post-response** script.

| Level | Where to edit it | Stored in |
|---|---|---|
| Workspace | Workspace menu (the workspace name in the title bar) → **Workspace settings…** → **Scripts** | `zorvik.yaml` |
| Folder | Right-click the folder → **Folder settings…** → **Scripts** | the folder's `_folder.yaml` |
| Request | The request's **Scripts** tab | the request's `.yaml` file |

In the editor, the **Pre-request** / **Post-response** switch picks which script you're editing. A dot on the **Scripts** tab (and on each side of the switch) shows that a script is set. The **Snippets** menu inserts common code, and typing `pm.` offers completions for the API.

Scripts are saved in the workspace files as plain text, so they're versioned in Git with everything else:

```yaml title="requests/Users/Get user.yaml"
name: Get user
method: GET
url: "{{base}}/users/{{userId}}"
scripts:
  preRequest: |
    pm.request.headers.upsert({ key: "X-Request-Id", value: pm.variables.replaceIn("{{$guid}}") });
  postResponse: |
    pm.test("Status is 200", () => pm.response.to.have.status(200));
```

Folders (`_folder.yaml`) and the workspace (`zorvik.yaml`) use the same `scripts:` block. See [Workspace format](../../reference/workspace-format/) for the rest of these files.

:::note
Server-Sent Events requests have no **Scripts** tab in the app. To test an SSE request in a collection run, write its post-response script in the request's `.yaml` file (`scripts.postResponse`), or put it in a folder script. See [Repeat until and event streams](../../testing/repeat-and-streams/).
:::

## The order scripts run in

For every send, Zorvik runs this pipeline:

1. **Pre-request scripts**: workspace, then each folder from the outermost to the innermost, then the request.
2. **Resolve and send**: `{{variables}}` are replaced (with any values the pre-request scripts set), inherited headers and auth are added, and the request is sent.
3. **Post-response scripts**: workspace, then folders from outermost to innermost, then the request.
4. In collection runs only: the request's **"repeat until" condition**, if it has one (see [Repeat until and event streams](../../testing/repeat-and-streams/)).
5. For requests imported from an OpenAPI document: the **"Matches the API spec"** check (see [OpenAPI contract checks](../../testing/openapi-contract-checks/)).

Empty scripts are skipped. Each script runs in its own fresh sandbox: nothing (no local variables, no functions) survives from one script to the next. What does carry over:

- **Variables.** A value set with `pm.variables.set` or `pm.environment.set` in the workspace script is visible to the folder and request scripts that run after it.
- **Request changes.** In the pre-request phase, each script sees the request as the previous script left it.
- **Tests and console output** from every script are collected into one list for the send.

A folder script therefore runs for every request inside the folder, including requests in its subfolders, and a workspace script runs for every request in the workspace.

## What a pre-request script sees

`pm.request` in a pre-request script is the saved request **before** `{{variables}}` are resolved:

| Part | What the script gets |
|---|---|
| `pm.request.url` | The URL exactly as typed, for example `{{base}}/users?page=1` |
| `pm.request.method` | The method, such as `GET` |
| `pm.request.headers` | The request's own enabled headers. Headers inherited from folders and the workspace, auth headers and default headers are added later and are not in this list. |
| `pm.request.body` | The body text for JSON, Text and XML bodies. Empty for other body types (form, multipart, file, GraphQL). |
| `pm.response` | `undefined` (there is no response yet) |

Changes the script makes to the URL, method, headers or body apply to **this send only**. They are never saved to the request file.

:::caution
Setting `pm.request.body` on a request whose body is not JSON, Text or XML (for example a form, a file or a GraphQL query) switches the body for that send: text starting with `{` or `[` is sent as JSON, anything else as Text.
:::

Variables set in a pre-request script are used when the request is resolved, so this works:

```js title="Pre-request script"
pm.variables.set("timestamp", Date.now());
// The URL "{{base}}/events?since={{timestamp}}" now gets the value above.
```

## What a post-response script sees

| Part | What the script gets |
|---|---|
| `pm.response` | The response: status, headers, body, time and size. See [pm.response](../pm-reference/#pmresponse). |
| `pm.request` | What was actually sent: the resolved URL, the headers after inheritance and auth, and the body text (the first 1 MB). Changing it has no effect. |

For Server-Sent Events requests in a collection run, `pm.response.events` also lists the events that were read.

## Variables that scripts set

Scripts can set variables in four scopes. The table shows how long a value lasts.

| API | Scope | In the app | In `zorvik run` |
|---|---|---|---|
| `pm.variables.set` | This send, or the whole collection run | Gone after the send or the run | Gone after the run |
| `pm.environment.set` | The active environment | Kept on this computer | Kept until the run ends |
| `pm.collectionVariables.set` | Workspace variables | Kept on this computer | Kept until the run ends |
| `pm.globals.set` | Global variables | Kept on this computer, shared by all workspaces | Kept until the run ends (starts empty) |

"Kept on this computer" means the value is stored in the app's data folder, never in the workspace files, so tokens that scripts save don't end up in Git. These values win over the value in the file for the same variable. If no environment is active, `pm.environment.set` still works for the rest of the send, but the value is not kept, and the console shows: `No environment is active: values set with pm.environment are not kept.`

Values are always stored as text: numbers and booleans become their text form, objects and arrays become JSON. See [Variables and environments](../../variables/variables-and-environments/) for how scopes are resolved.

## The console

`console.log`, `console.info`, `console.warn`, `console.error` and `console.debug` write to the script console.

- **In the app**, a **Console** tab appears next to the response when there is output. It shows the level of each line and any script errors.
- **In the Runner tab**, expand a result to see its console lines.
- **In `zorvik run`**, the text output doesn't include the console. Use `--json` and read `results[].console`.

```js
console.log("user", pm.response.json().id);
console.warn("%s has %d items", "cart", 3);   // cart has 3 items
console.info({ a: 1, b: [1, 2] });            // {"a":1,"b":[1,2]}
```

Objects are shown as JSON (cycles as `"[Circular]"`), errors as `Name: message`. A script keeps at most 1,000 console messages of up to 10 KB each; see [Sandbox and limits](../sandbox/#limits).

## Tests

Tests are added with `pm.test(name, fn)`, usually in post-response scripts (they work in pre-request scripts too). A test passes when its function doesn't throw.

```js
pm.test("Has an id", () => {
  pm.expect(pm.response.json()).to.have.property("id");
});
```

- **In the app**, a **Tests** tab appears next to the response with the count of passed tests, for example **Tests 3/4**, and lists each test with its error.
- **In a collection run**, a request with tests passes only if all its tests pass (skipped tests don't count). See [Collection runner](../../testing/collection-runner/#when-a-request-passes-or-fails).

## When a script fails

A script fails when it throws an error that isn't caught: a syntax error, a `ReferenceError`, a failed `pm.expect` **outside** of `pm.test`, the time limit, or running out of memory. A failing `pm.expect` **inside** `pm.test` only fails that test; the script keeps running.

| The failing script is… | What happens |
|---|---|
| A pre-request script | The remaining pre-request scripts don't run and **the request is not sent**. The app shows **Pre-request script failed**. In a run, the request fails with that error. |
| A post-response script | The other post-response scripts still run. The app shows the error above the response and in the **Console** tab. In a run, the request fails. |

What the script did before it failed is kept: its tests, its console output, the variables it set and a `setNextRequest` call.

Error messages name the script and, when known, the line:

```text
Pre-request script of request 'Get user' failed at line 3: ReferenceError: foo is not defined
Post-response script of folder 'Users' failed at line 7: AssertionError: expected 404 to equal 200
Pre-request script of workspace failed: The script took longer than 5 s and was stopped
```

Line numbers match the lines in the editor.

## Settings

| Setting | Where | Default |
|---|---|---|
| Script time limit | Settings → Requests → **Script time limit** | 5,000 ms (100 ms to 60 s), for each script |

`zorvik run` doesn't read the app's settings and always uses 5 seconds. See [Sandbox and limits](../sandbox/) for every limit.

## Next steps

- [pm API reference](../pm-reference/): every object and function.
- [Assertions](../assertions/): every `pm.expect` and `pm.response.to` assertion.
- [Script examples](../examples/): recipes for common tasks.
- [Collection runner](../../testing/collection-runner/): run a folder with its scripts and tests.

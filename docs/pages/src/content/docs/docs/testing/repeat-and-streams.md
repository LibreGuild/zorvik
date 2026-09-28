---
title: Repeat until and event streams
description: Poll a request in a collection run until a condition holds, and test Server-Sent Events streams with pm.response.events.
sidebar:
  order: 3
---

Two request settings make collection runs work with APIs that don't answer right away:

- **Repeat until** sends a request again and again until a condition holds, for example until a background job reports `done`.
- **Stream settings** say how long a run reads a **Server-Sent Events** (SSE) request or a **GraphQL subscription**, so its tests can check the events.

Both only apply in [collection runs](../collection-runner/). A single send in the app ignores them.

## Repeat until

### Turn it on

Open the request's **Settings** tab. Under **Repeat until**, turn on **Repeat in collection runs** and fill in:

| Setting | What it means | Default |
|---|---|---|
| **Condition** | A JavaScript expression checked after each send, after the post-response scripts. Empty: until the request's tests pass. | Empty |
| **Every** | Milliseconds to wait between two sends | 1,000 |
| **Give up after** | Milliseconds after the first send; then the request fails | 30,000 |

The setting is available for HTTP (including GraphQL) and SSE requests. In the request file it's `settings.repeat`:

```yaml title="requests/Jobs/Check job.yaml"
name: Check job
method: GET
url: "{{base}}/jobs/{{jobId}}"
settings:
  repeat:
    condition: pm.response.json().status === "done"
    intervalMs: 2000
    timeoutMs: 60000
scripts:
  postResponse: |
    pm.test("The job succeeded", () => {
      pm.expect(pm.response.json().result).to.equal("ok");
    });
```

### How it works

For a request with **Repeat until** on, the runner:

1. Sends the request, with its pre-request and post-response scripts, like any request of the run.
2. Checks the condition.
3. If the condition holds, the request is done: its result is this last send.
4. If not, and waiting **Every** milliseconds more would go past **Give up after** (counted from the first send), the request fails.
5. Otherwise it waits **Every** milliseconds and goes back to step 1.

With the defaults (every 1 s, give up after 30 s), a request is sent up to about 30 times. With **Give up after** set to 0 it's sent once. The run's **Delay** setting is not used between these sends; **Every** is.

Each send runs all the scripts again, and the variables they set carry over from one send to the next.

### The condition

The condition is a JavaScript **expression** with the whole [`pm` API](../../scripting/pm-reference/). It holds when it's truthy.

```js
pm.response.json().status === "done"
```

```js
pm.response.code === 200 && pm.response.json().items.length > 0
```

For more than one statement, wrap them in a function that you call right away:

```js
(() => {
  const job = pm.response.json();
  return job.status === "done" || job.status === "failed";
})()
```

- An error thrown while checking (for example `pm.response.json()` on a body that isn't JSON yet) counts as "not yet". The last such error is shown when the request gives up.
- A syntax error in the condition stops repeating at once and fails the request with a script error.
- A send that fails (a network error, a timeout, a failing pre-request script) also counts as "not yet".

**Empty condition**: the request repeats until its tests pass: every test that isn't skipped, including tests from workspace and folder scripts and the ["Matches the API spec"](../openapi-contract-checks/) check. A request without tests repeats until its status is below 400.

### The result

The run reports the **last** send: its status, tests and console. The result also has:

- **The number of sends**: `3×` in the Runner tab, `attempts` in the [JSON report](../reports/#json-report) (present for every request with **Repeat until** on).
- **The duration** from the first send to the end of the last one.

When the condition never held, the request fails with a message such as:

```text
Repeat until: `pm.response.json().status === "done"` didn't hold after 30 sends in 29.1 s
Repeat until: `pm.response.json().status === "done"` didn't hold after 4 sends in 3.0 s (last: SyntaxError: pm.response.json(): the response body is not valid JSON (…))
Repeat until: its tests didn't pass after 12 sends in 11.0 s (last: status 503)
```

If the last send itself failed (for example with a network error), that error is reported instead.

### A complete polling example

Two requests in a folder, run in order:

```js title="Post-response script of 'Start export'"
pm.test("Export accepted", () => pm.response.to.have.status(202));
pm.variables.set("jobId", pm.response.json().id);
```

`Check export` calls `GET {{base}}/exports/{{jobId}}` with **Repeat until** on, the condition `["done", "failed"].includes(pm.response.json().state)`, and this test:

```js title="Post-response script of 'Check export'"
pm.test("Export finished without errors", () => {
  pm.expect(pm.response.json().state).to.equal("done");
});
```

## Event streams in runs

A Server-Sent Events request sends one HTTP request and reads events from the answer as they arrive. In a collection run, Zorvik reads the stream until a stop condition, then runs the request's post-response scripts with the events.

:::note
SSE requests run in the app's Runner tab, in `zorvik run` and in runs an AI agent starts.
:::

[GraphQL subscriptions](../../protocols/graphql/#subscriptions) run the same way: each result is an event named `next`, and `pm.response.json()` is the list of results (`[{"data": …}, …]`), so a test can check them all:

```js
pm.test("three ticks", () => {
  const ticks = pm.response.json().map((r) => r.data.tick);
  pm.expect(ticks).to.eql([1, 2, 3]);
});
```

### Stream settings

Open the SSE request's **Settings** tab. Under **In collection runs**:

| Setting | What it means | Default |
|---|---|---|
| **Stop at event** | Stop after the first event with this name. Events without an `event:` line are named `message`. Empty: no named event ends the read. | Empty |
| **Stop after** | Stop after this many events. 0: only the time limit (or the server) ends it. | 100 |
| **Time limit** | Milliseconds from the start of the request (connecting included). At most 300,000 (5 minutes). | 10,000 |

Reading stops at the first of these, or when the server closes the stream. In the request file it's `settings.stream`:

```yaml title="requests/Jobs/Watch job.yaml"
name: Watch job
kind: sse
url: "{{base}}/jobs/{{jobId}}/events"
settings:
  stream:
    event: done
    maxEvents: 100
    timeoutMs: 30000
```

Zorvik adds `Accept: text/event-stream` and `Cache-Control: no-cache` unless the request sets these headers.

### What scripts get

| API | For an SSE request |
|---|---|
| `pm.response.events` | The events read, in order: `{ event, data, id }` |
| `pm.response.text()` | The events as text, in wire format (`event:`, `id:` and `data:` lines, a blank line after each event; the `event:` line is left out for `message` events) |
| `pm.response.code`, `status`, `headers` | The stream's HTTP response |
| `pm.response.responseTime` | Milliseconds from the start of the request until reading stopped |
| `pm.response.responseSize` | The size of the text above |

Each event has:

| Field | Value |
|---|---|
| `event` | The event name, `message` when the server sent no `event:` line |
| `data` | The data, with multiple `data:` lines joined by `\n` |
| `id` | The last event ID in effect when the event arrived, or `null`. An `id:` sent with one event carries over to the following events, as in browsers. |

```js title="Post-response script of 'Watch job'"
const events = pm.response.events;

pm.test("Stream opened", () => pm.response.to.have.status(200));

pm.test("Ends with done", () => {
  pm.expect(events).to.not.be.empty;
  pm.expect(events[events.length - 1].event).to.equal("done");
});

pm.test("Progress is valid JSON", () => {
  events
    .filter((e) => e.event === "progress")
    .forEach((e) => pm.expect(JSON.parse(e.data)).to.have.property("pct"));
});
```

The result's console gets a line saying how the read ended, for example `Read 3 events (stopped: the awaited event arrived)`. The possible reasons:

| Reason | Meaning |
|---|---|
| `the awaited event arrived` | The **Stop at event** event arrived (it's included in the events) |
| `enough events arrived` | **Stop after** events arrived |
| `the time limit` | **Time limit** passed |
| `the server ended the stream` | The server closed the connection, or reading failed (events read before are kept) |
| `the answer is not an event stream` | The status wasn't 2xx or the `Content-Type` wasn't `text/event-stream` |

### Passing and failing

An SSE request passes and fails by the [usual rules](../collection-runner/#when-a-request-passes-or-fails): with tests, the tests decide; without tests, a status below 400 passes.

- Reading ending at the time limit is **not** a failure by itself. Add a test for the events you expect.
- When the answer is not an event stream, `pm.response.events` is an empty array and `pm.response.text()` holds the start of the answer (up to 4 KB), so a test can show what the server said.
- When the server doesn't answer at all within the time limit, the request fails with `No answer within N ms`.

### Writing scripts for SSE requests

SSE requests have no **Scripts** tab in the app. Write the post-response script in the request's `.yaml` file (`scripts.postResponse`, as in the examples above), or put it in the post-response script of a folder that holds the SSE requests.

**Repeat until** works with SSE requests too, for example to reconnect until a stream delivers an event:

```yaml
settings:
  stream:
    event: ready
    timeoutMs: 5000
  repeat:
    condition: pm.response.events.some((e) => e.event === "ready")
    intervalMs: 1000
    timeoutMs: 60000
```

### Limits

| Limit | Value |
|---|---|
| Events kept per request | 1,000 (more are counted but not kept) |
| Data per event | 64 K characters (the rest is cut) |
| Time limit | 300,000 ms (5 minutes) |
| Answer kept when it's not an event stream | The first 4 KB |

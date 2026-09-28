---
title: Sandbox and limits
description: The JavaScript runtime scripts run in, its time and memory limits, what is not available, and how Zorvik's script API compares with Postman's.
sidebar:
  order: 5
---

Scripts run in a sandbox that can only compute: it has no access to files, the network, other programs or the clock beyond reading the time. The request, the response and the variables go in as data, and the script's results (tests, console output, variable changes, request changes) come out as data. This makes scripts from shared or imported collections safe to run.

## Runtime

| | |
|---|---|
| Engine | [QuickJS](https://bellard.org/quickjs/), embedded in Zorvik |
| Language | Modern JavaScript: `let`/`const`, arrow functions, classes (with private and static fields), destructuring, template strings, optional chaining (`?.`), `??`, `async`/`await`, generators, `BigInt`, `Map`, `Set`, `WeakRef`, and newer built-ins such as `Array.prototype.at`, `findLast`, `toSorted`, `Object.groupBy` and `String.prototype.replaceAll` |
| Isolation | Every script gets a fresh engine. Nothing (functions, globals) carries over from one script to the next, or from one send to the next. Share data between scripts with [variables](../pm-reference/#variables). |
| Mode | Sloppy (non-strict) mode: assigning an undeclared name creates a global |
| Top level | The script runs inside a function, as in Postman, so a top-level `return` ends it early. `this` is the global object. |
| Line numbers | Error line numbers match the lines in the editor |

```js
// Ends the script early: nothing below runs for 204 responses.
if (pm.response.code === 204) return;
```

### Promises and async code

Promises work. After the script's own code finishes, Zorvik runs the pending promise callbacks until there are none left or the time limit is reached. That's what makes `async` test functions work:

```js
pm.test("async works", async () => {
  const value = await Promise.resolve(pm.response.json());
  pm.expect(value).to.be.an("object");
});
```

There are no timers, so nothing can wait for time to pass. `await` at the top level of a script is a syntax error (the script is not an async function); use `await` inside an `async` function or an async `pm.test`.

## Limits

| Limit | Value | What happens when it's reached |
|---|---|---|
| Time per script | 5 s by default; Settings → Requests → **Script time limit**, 100 ms to 60 s. `zorvik run` always uses 5 s. | The script stops: `The script took longer than 5 s and was stopped`. Promise callbacks count toward the same limit. |
| Memory per script | 64 MB of JavaScript heap | The script stops: `The script ran out of memory (limit 64 MB)` |
| Call stack | 768 KB | Deep recursion throws a `RangeError` (a normal error the script can catch) |
| Collecting the results after the script | 1 s | `Collecting the script's results took too long (did it replace built-in functions?)`, for scripts that break built-ins the report needs |
| Response body seen by scripts | First 16 MB | `pm.response.text()` and `json()` see the start, and the console warns `The response body is larger than 16 MB; scripts see only its start.` |
| Sent body in `pm.request.body` (post-response) | First 1 MB | Cut |
| Console messages per script | 1,000 | Further messages are dropped and counted: `N more console messages were not kept (limit 1000).` |
| Length of one console message | 10 KB | Cut, ending with `… (truncated)` |
| Console lines per send | 2,000 | Further lines are dropped |
| Console lines per result in a collection run | 200, each up to 4,096 characters | `N more console lines not kept.` |
| Tests per script | 10,000 | Further tests are not recorded |
| Length of a test's error message | 10 KB | Cut, ending with `… (truncated)` |
| `replaceIn` nesting | 10 levels | Deeper `{{…}}` are left as they are |
| Events in `pm.response.events` | 1,000 per request, 64 KB of data per event | Further events are counted but not kept |

Collection runs have more limits (iterations, requests per iteration, results kept); see [Collection runner](../../testing/collection-runner/#limits).

## What is not available

The sandbox has no way to reach outside itself.

| Missing | Examples | What you get |
|---|---|---|
| Network | `pm.sendRequest`, `fetch`, `XMLHttpRequest` | `pm.sendRequest` throws `pm.sendRequest is not supported in Zorvik`; the others are `undefined` |
| Modules | `require`, `pm.require`, `import()` | `require` and `pm.require` throw `… is not supported in Zorvik`; `import()` rejects |
| Timers | `setTimeout`, `setInterval`, `setImmediate` and their `clear…` functions | Throw `setTimeout is not supported in Zorvik` (and so on) |
| Files and processes | `process`, `std`, `os` | `undefined` |
| Node.js and web APIs | `Buffer`, `URL`, `URLSearchParams`, `TextEncoder`, `TextDecoder`, `structuredClone`, `crypto`, `Intl` | `undefined` |
| Postman libraries | `CryptoJS`, `xml2Json` | Throw `… is not supported in Zorvik` |
| Other Postman libraries | `lodash` / `_`, `moment`, `cheerio`, `tv4`, `ajv`, `uuid`, `chai` | Not defined (`ReferenceError`) |

What is available beyond the language: the `pm` API, `console`, `atob` and `btoa` (Base64 for Latin-1 text), and `queueMicrotask`.

Some consequences:

- **No hashing or signing.** Without `CryptoJS` or `crypto.subtle`, scripts can't compute HMAC signatures, SHA hashes or JWT signatures.
- **No locale formatting.** Without `Intl`, `toLocaleString` uses a fixed format and `localeCompare` compares plain character codes.
- **No URL parsing class.** Use [`pm.request.url`](../pm-reference/#pmrequesturl) for the request's URL, or a regular expression for other URLs.
- **Build query strings by hand** with `encodeURIComponent`.

```js
const query = Object.entries({ q: "café & co", page: 2 })
  .map(([k, v]) => `${encodeURIComponent(k)}=${encodeURIComponent(v)}`)
  .join("&");
```

### What scripts can read

Scripts can read every variable of the active environment, the workspace and globals, **including secret values**, and the full request and response. Whatever a script writes with `console.log` shows in the Console tab and in exported run reports. Don't log secrets.

## Postman compatibility

Zorvik implements the parts of Postman's sandbox that collections use most, so an imported collection usually runs unchanged. When you import a Postman collection, Zorvik lists the scripts that use APIs it doesn't support (`pm.sendRequest`, `require`, timers, `pm.cookies`, `pm.visualizer`, `CryptoJS`); they're imported anyway and fail with a clear error when they run. Collection-level scripts are put on the folder the import creates. See [Import and export](../../requests/import-export/).

### Supported

| Postman API | In Zorvik |
|---|---|
| `pm.test`, `pm.test.skip` | Yes, including async tests and the `done` callback |
| `pm.expect` | A subset of chai; see [Assertions](../assertions/#differences-from-chai) |
| `pm.response.to.have.*` / `to.be.*` | Yes, except `jsonSchema` |
| `pm.variables`, `pm.environment`, `pm.collectionVariables`, `pm.globals`, `pm.iterationData` | Yes (`pm.collectionVariables` are workspace variables) |
| `pm.request` (URL, method, headers, body, query) | Yes |
| `pm.response` (`code`, `status`, `headers`, `text()`, `json()`, `responseTime`, `responseSize`) | Yes |
| `pm.info` | Yes |
| `pm.execution.setNextRequest`, `postman.setNextRequest` | Yes, in collection runs |
| Legacy `tests[…]`, `postman.*`, `responseBody`, `responseCode`, … | Yes |
| `console` | Yes |
| `atob`, `btoa` | Yes |

### Not supported

`pm.sendRequest`, `pm.require`, `require`, `pm.cookies`, `pm.response.cookies`, `pm.visualizer`, `pm.vault`, `pm.execution.skipRequest`, `pm.response.to.have.jsonSchema`, timers, and the libraries in the table above. See the [list with error messages](../pm-reference/#not-supported).

### Differences

| Topic | Zorvik |
|---|---|
| Stored values | Variable values are stored as text. Numbers become `"5"`, objects become JSON. Within one script, `get` returns what you `set`. |
| Where values are kept | Values scripts set on the environment, workspace or globals are kept in the app's data folder on this computer, never in the workspace files. `zorvik run` keeps them only until the run ends. |
| Global variables | Shared by every workspace on this computer. `zorvik run` starts with none. |
| `pm.variables` in runs | Values last for the whole run, across iterations |
| `pm.info.requestId` | The request's file path under `requests/`, not an id |
| `pm.request` in pre-request scripts | Only the request's own headers; inherited headers and auth are added after the script |
| Unknown `pm.expect` words | A chai word Zorvik doesn't have reads as `undefined` and asserts nothing |
| Scripts on SSE requests | Only in collection runs, with `pm.response.events` (a Zorvik addition) |
| Protocols | Scripts run for HTTP (and GraphQL) requests, and SSE requests in runs; not for WebSocket, gRPC or the other socket kinds |

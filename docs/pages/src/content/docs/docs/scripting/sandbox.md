---
title: Sandbox and limits
description: The JavaScript runtime scripts run in, its time and memory limits, what is not available, and how Zorvik's script API compares with Postman's.
sidebar:
  order: 5
---

Scripts run in a sandbox: no files, no other programs, no way to change the app. The request, the response and the variables go in as data, and the script's results (tests, console output, variable changes, request changes, a visualization) come out as data. The only ways out are the ones Postman has too: [`pm.sendRequest`](../pm-reference/#pmsendrequest), which uses the app's proxy and certificate settings (and, for an AI agent's run, only the hosts the user approved), and [`pm.cookies.jar()`](../pm-reference/#pmcookies), for the request's own site only.

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

Promises work. After the script's own code finishes, Zorvik runs the pending promise callbacks, then the [timers](../pm-reference/#timers) in time order (waiting for them), until there is nothing left or the time limit is reached. That's what makes `async` test functions, `pm.sendRequest` callbacks and `setTimeout` work:

```js
pm.test("async works", async () => {
  const value = await Promise.resolve(pm.response.json());
  pm.expect(value).to.be.an("object");
});
```

`await` works at the top level of a script: a script that uses `await` runs as an async function (`const res = await pm.sendRequest(url)`). An error thrown in a promise callback or a timer, or a promise rejected with nobody handling it, fails the script like any other error.

## Limits

| Limit | Value | What happens when it's reached |
|---|---|---|
| Time per script | 5 s by default; Settings → Requests → **Script time limit**, 100 ms to 60 s. `zorvik run` always uses 5 s. | The script stops: `The script took longer than 5 s and was stopped`. Promise callbacks, timers and `pm.sendRequest` calls count toward the same limit. |
| `pm.sendRequest` per script | 100 requests | Further calls fail with `pm.sendRequest: at most 100 requests per script` |
| `pm.visualizer` result | 5 MB | `pm.visualizer: the result is larger than 5 MB` |
| Memory per script | 64 MB of JavaScript heap | The script stops: `The script ran out of memory (limit 64 MB)` |
| Call stack | 4 MB (each script runs on its own thread) | Deep recursion throws a `RangeError` (a normal error the script can catch) |
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
| Direct network access | `fetch`, `XMLHttpRequest`, sockets | `undefined`: use [`pm.sendRequest`](../pm-reference/#pmsendrequest) |
| Modules | npm packages and files other than the [built-in libraries](#libraries), `import()` | `require('x')` throws `Cannot find module 'x'. Scripts can require only these built-in libraries: …`; `import()` rejects |
| Files and processes | `process`, `std`, `os` | `undefined` |
| Node.js and web APIs | `Buffer`, `URL`, `URLSearchParams`, `TextEncoder`, `TextDecoder`, `structuredClone`, `crypto.subtle`, `Intl` | `undefined` (`Buffer` is in `require('buffer')`) |

What is available beyond the language: the `pm` API, `console`, `atob` and `btoa` (Base64 for Latin-1 text), `queueMicrotask`, `crypto.getRandomValues` and `crypto.randomUUID`, and the [libraries](#libraries).

Some consequences:

- **No locale formatting.** Without `Intl`, `toLocaleString` uses a fixed format and `localeCompare` compares plain character codes. moment formats in English only.
- **No URL class.** Use [`pm.request.url`](../pm-reference/#pmrequesturl) for the request's URL, and `require('url').parse(text, true)` for others.

## Libraries

Scripts can `require` the libraries Postman's sandbox has, and a few of Node's modules. They are part of Zorvik, so nothing is downloaded, and they work offline. A library loads the first time a script requires it (a few milliseconds) and counts toward the script's time and memory limits.

| `require(…)` | Library | Notes |
|---|---|---|
| `"lodash"` | lodash 4.18 | Also the global `_` |
| `"crypto-js"` | crypto-js 4.2 | Also the global `CryptoJS` |
| `"moment"` | moment 2.31 | English only |
| `"ajv"` | Ajv 8 (JSON Schema draft-07) | `"ajv/dist/2019"` and `"ajv/dist/2020"` for the newer drafts, `"ajv-formats"`. As in Postman, unknown keywords are ignored and the standard formats (`date-time`, `email`, `uri`, …) are checked; `new Ajv({ strict: true })` is Ajv 8's strict mode |
| `"tv4"` | tv4 1.3 (JSON Schema draft-04) | Also the global `tv4` |
| `"chai"` | chai 4.5 | The full library: `expect`, `assert`, plugins with `chai.use` |
| `"uuid"` | uuid 14 | `uuid.v4()`, `uuid.v7()`, …; `uuid()` is a v4 UUID, as in Postman |
| `"csv-parse/lib/sync"` | csv-parse 7 | `parse(text, options)`, Postman's form; `"csv-parse/sync"` gives `{ parse }` |
| `"xml2js"` | xml2js 0.6 | `xml2Json(text)` is Postman's shortcut (synchronous) |
| `"cheerio"` | cheerio 1.2 | jQuery-style HTML queries; also the global `cheerio` |
| `"handlebars"` | Handlebars 4.7 | Templates with `Handlebars.compile` |
| `"buffer"`, `"events"`, `"path"`, `"querystring"`, `"url"`, `"util"` | Browser versions of Node's modules | `"node:path"` and the like work too |

```js
const _ = require("lodash");
const moment = require("moment");

pm.test("Newest order is from today", () => {
  const newest = _.maxBy(pm.response.json().orders, "createdAt");
  pm.expect(moment.utc(newest.createdAt).isSame(moment.utc(), "day")).to.be.true;
});
```

`pm.require("npm:lodash@4.17.21")`, Postman's form, gives the same built-in library: the version is ignored. Other npm packages, files and Postman's team package library aren't available.

Within a run, each library is loaded once: two `require("lodash")` calls return the same object. The next script gets fresh copies.

### What scripts can read

Scripts can read every variable of the active environment, the workspace and globals, **including secret values**, and the full request and response. Whatever a script writes with `console.log` shows in the Console tab and in exported run reports. Don't log secrets.

## Postman compatibility

Zorvik implements the parts of Postman's sandbox that collections use most, so an imported collection usually runs unchanged. When you import a Postman collection, Zorvik lists the scripts that use APIs it doesn't support (`pm.vault`, `pm.execution.runRequest`); they're imported anyway and fail with a clear error when they run. Collection-level scripts are put on the folder the import creates. See [Import and export](../../requests/import-export/).

### Supported

| Postman API | In Zorvik |
|---|---|
| `pm.test`, `pm.test.skip` | Yes, including async tests and the `done` callback |
| `pm.expect` | A subset of chai; see [Assertions](../assertions/#differences-from-chai) |
| `pm.response.to.have.*` / `to.be.*` | Yes, `jsonSchema` too (with the bundled Ajv) |
| `pm.variables`, `pm.environment`, `pm.collectionVariables`, `pm.globals`, `pm.iterationData` | Yes (`pm.collectionVariables` are workspace variables) |
| `pm.request` (URL, method, headers, body, query) | Yes |
| `pm.response` (`code`, `status`, `headers`, `text()`, `json()`, `responseTime`, `responseSize`) | Yes |
| `pm.info` | Yes |
| `pm.execution.setNextRequest`, `postman.setNextRequest` | Yes, in collection runs |
| `pm.execution.skipRequest` | Yes |
| `pm.sendRequest` (callback or `await`) | Yes |
| `pm.cookies`, `pm.cookies.jar()`, `pm.response.cookies` | Yes (the jar for the request's own site) |
| `pm.visualizer` | Yes, as HTML and CSS: template scripts don't run |
| `setTimeout`, `setInterval`, `setImmediate` | Yes |
| Legacy `tests[…]`, `postman.*`, `responseBody`, `responseCode`, … | Yes |
| `console` | Yes |
| `atob`, `btoa` | Yes |
| `require`, `pm.require`, `CryptoJS`, `_`, `tv4`, `cheerio`, `xml2Json` | Yes, the [built-in libraries](#libraries) |

### Not supported

`pm.vault` (use secret variables), `pm.execution.runRequest` (use `pm.sendRequest`), scripts inside visualizer templates, and npm packages other than the built-in libraries. See the [list with error messages](../pm-reference/#not-supported).

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

---
title: pm API reference
description: Every object, property and function scripts can use, with signatures, return values and examples.
sidebar:
  order: 2
---

This page lists everything a script can reach: the `pm` object, `console`, the legacy Postman globals, and the few extra functions the sandbox provides. Anything not on this page is not available; the [last section](#not-supported) lists the Postman APIs that exist but throw.

Signatures use TypeScript-style notation. `?` marks an optional argument.

## Global names

| Name | What it is |
|---|---|
| `pm` | The script API described on this page |
| `console` | Script console: `log`, `info`, `warn`, `error`, `debug`, `dir`, `trace`, `clear` |
| `atob(text)`, `btoa(text)` | Base64 decode and encode (Latin-1 text) |
| `postman`, `tests`, `responseBody`, `responseCode`, `responseHeaders`, `responseTime`, `environment`, `globals`, `data`, `iteration` | The [legacy Postman API](#legacy-postman-api) |
| Standard JavaScript | `JSON`, `Math`, `Date`, `RegExp`, `Array`, `Object`, `Map`, `Set`, `Promise`, `queueMicrotask`, typed arrays and the other built-ins of the language |

See [Sandbox and limits](../sandbox/) for what is not available (network, files, timers, modules).

## pm.info

Read-only facts about the current script.

| Property | Type | Description |
|---|---|---|
| `pm.info.eventName` | `"prerequest"` or `"test"` | Which script is running: `"prerequest"` in pre-request scripts, `"test"` in post-response scripts (Postman's names) |
| `pm.info.requestName` | string | The request's name |
| `pm.info.requestId` | string | The request's file path under `requests/`, for example `Users/Get user.yaml`. Empty for a request that isn't saved. |
| `pm.info.iteration` | number | The current iteration of a collection run, starting at `0`. Always `0` for a single send. |
| `pm.info.iterationCount` | number | How many iterations the run has. `1` for a single send. |

```js
if (pm.info.iteration === 0) console.log("first pass of", pm.info.requestName);
```

## Variables

Five objects give access to variables. `pm.variables` looks through every scope; the others read and write one scope each.

| Object | Scope | Writable |
|---|---|---|
| `pm.variables` | Local values for this send (or the whole run) when writing; every scope when reading | Yes |
| `pm.environment` | The active environment | Yes |
| `pm.collectionVariables` | Workspace variables (in `zorvik.yaml`) | Yes |
| `pm.globals` | Global variables | Yes |
| `pm.iterationData` | The current row of the run's data file | No |

### Precedence

`pm.variables.get(name)` and `{{name}}` in a request look through the scopes in this order and use the first one that has the name:

1. Values given with `--var` on the command line (scripts can't change these)
2. `pm.variables` (local values)
3. The current data file row (`pm.iterationData`)
4. The active environment (`pm.environment`)
5. Workspace variables (`pm.collectionVariables`)
6. Global variables (`pm.globals`)

### Methods of pm.environment, pm.collectionVariables and pm.globals

| Method | Returns | Description |
|---|---|---|
| `get(name: string)` | any | The variable's value, or `undefined` when the scope doesn't have it |
| `set(name: string, value: any)` | `undefined` | Sets the variable. A `null` or `undefined` name does nothing. |
| `has(name: string)` | boolean | Whether the scope has the variable |
| `unset(name: string)` | `undefined` | Removes the variable from the scope |
| `clear()` | `undefined` | Unsets every variable of the scope |
| `toObject()` | object | A copy of the scope as `{ name: value }` |
| `toJSON()` | object | Same as `toObject()` |
| `replaceIn(text: string)` | string | `text` with `{{name}}` replaced by this scope's values and [dynamic variables](#dynamic-variables-in-replacein) |

`pm.environment` also has:

| Property | Type | Description |
|---|---|---|
| `pm.environment.name` | string or `undefined` | Name of the active environment, `undefined` when none is active |

```js
pm.environment.set("token", pm.response.json().access_token);
if (!pm.environment.has("userId")) pm.environment.set("userId", "1");
console.log(pm.environment.name, pm.environment.toObject());
```

### Methods of pm.variables

`pm.variables` has the same methods, but reads across all scopes:

| Method | Returns | Description |
|---|---|---|
| `get(name: string)` | any | The value from the highest-precedence scope that has `name`, or `undefined` |
| `has(name: string)` | boolean | Whether any scope has `name` |
| `set(name: string, value: any)` | `undefined` | Sets a local value: it wins over the data row, environment, workspace and globals for the rest of the send, or for the rest of a collection run |
| `unset(name: string)` | `undefined` | Removes a local value (the other scopes are untouched) |
| `clear()` | `undefined` | Removes every local value |
| `toObject()` | object | All scopes merged, higher precedence winning |
| `toJSON()` | object | A copy of the local values only |
| `replaceIn(text: string)` | string | `text` with `{{name}}` replaced using every scope, then dynamic variables |

```js
pm.variables.set("page", 2);
const url = pm.variables.replaceIn("{{base}}/items?page={{page}}");
```

:::note
In a collection run, local values set with `pm.variables.set` last for **the whole run**, across all iterations. Reset them yourself if each iteration should start fresh.
:::

### pm.iterationData

The current row of the run's data file (see [Data files](../../testing/data-files/)). It's read-only and empty in a single send.

| Method | Returns | Description |
|---|---|---|
| `get(name: string)` | any | The column's value, or `undefined` |
| `has(name: string)` | boolean | Whether the row has the column |
| `toObject()` | object | The row as `{ column: value }` |
| `toJSON()` | object | Same as `toObject()` |
| `replaceIn(text: string)` | string | `text` with `{{column}}` replaced by the row's values |

CSV values are always strings. JSON values keep their type: `pm.iterationData.get("count")` returns the number `3` for `{"count": 3}`.

### How values are stored

Variables are text outside of scripts. When a script sets a value, it's stored like this:

| Value passed to `set` | Stored as |
|---|---|
| A string | The string itself |
| A number or boolean | Its text, for example `"5"` or `"true"` |
| An object or array | JSON, for example `{"x":1}` |
| `undefined` | An empty string |
| `null` | The text `"null"` |

Within the same script, `get` returns exactly what you passed to `set`. Later scripts, and `{{name}}` in requests, see the stored text:

```js
pm.environment.set("count", 5);
pm.environment.get("count") + 1;   // 6 in this script
// In the next script: pm.environment.get("count") === "5"
```

To store structured data, store JSON and parse it when reading:

```js
pm.collectionVariables.set("ids", [1, 2, 3]);                // stored as "[1,2,3]"
const ids = JSON.parse(pm.collectionVariables.get("ids"));   // in a later script
```

### What happens to values after the send

| Scope | In the app | In `zorvik run` |
|---|---|---|
| `pm.variables` | Discarded after the send or run | Discarded after the run |
| `pm.environment` | Kept on this computer for the active environment. Without an active environment, not kept (with a console warning). | Kept until the run ends |
| `pm.collectionVariables` | Kept on this computer | Kept until the run ends |
| `pm.globals` | Kept on this computer, shared by every workspace | Kept until the run ends; starts empty |

Values kept on this computer live in the app's data folder, not in the workspace files, and win over the value saved in the file. `unset` removes the value a script set, so the value saved in the file (if any) applies again from the next send. See [Variables and environments](../../variables/variables-and-environments/).

### Dynamic variables in replaceIn

`replaceIn` also fills in these dynamic variables when no scope has the name. Nested variables are resolved up to 10 levels deep; names that aren't found stay as they are.

| Name | Value |
|---|---|
| `{{$guid}}`, `{{$uuid}}`, `{{$randomUUID}}` | A random UUID v4 |
| `{{$timestamp}}` | Unix time in seconds |
| `{{$timestampMs}}` | Unix time in milliseconds |
| `{{$isoTimestamp}}` | The current time in ISO 8601 |
| `{{$randomInt}}` | A whole number from 0 to 1000 |
| `{{$randomBoolean}}` | `true` or `false` |
| `{{$randomAlphaNumeric}}` | One character, `a`–`z` or `0`–`9` |
| `{{$randomEmail}}` | An address like `user4821@example.com` |

These are the same dynamic variables requests support; see [Dynamic variables](../../variables/dynamic-variables/).

## pm.request

The request. In a pre-request script you can change it; in a post-response script it's what was sent (read-only in effect). See [what a pre-request script sees](../overview/#what-a-pre-request-script-sees).

| Member | Type | Description |
|---|---|---|
| `pm.request.url` | [URL object](#pmrequesturl) | The URL. Assign a string to replace it: `pm.request.url = "https://…"`. |
| `pm.request.method` | string | The method. Assigning sets it in upper case: `pm.request.method = "post"` sends `POST`. |
| `pm.request.headers` | [header list](#header-and-query-lists) | The request's headers (names compared without case) |
| `pm.request.body` | [body object](#pmrequestbody) | The body text |
| `pm.request.addHeader(header)` | `undefined` | Same as `headers.add(header)` |
| `pm.request.upsertHeader(header)` | `undefined` | Same as `headers.upsert(header)` |
| `pm.request.removeHeader(name: string)` | `undefined` | Same as `headers.remove(name)` |
| `pm.request.getHeaders()` | object | The headers as `{ name: value }` |
| `pm.request.toJSON()` | object | `{ url, method, header: [{key, value}], body: {mode, raw} }` |

### pm.request.url

| Member | Returns | Description |
|---|---|---|
| `toString()` | string | The whole URL text. Template strings work too: `` `${pm.request.url}` `` |
| `toJSON()` | string | Same as `toString()` |
| `update(url: string)` | `undefined` | Replaces the whole URL |
| `getHost()` | string | The host, for example `api.example.com` |
| `getPath()` | string | The path, `/` when there is none |
| `getQueryString()` | string | The query without `?`, or an empty string |
| `getPathWithQuery()` | string | Path and query, for example `/users?page=1` |
| `getRemote()` | string | Host and port, for example `api.example.com:8443` (no port when none is written) |
| `protocol` | string or `undefined` | For example `https` |
| `host` | string[] | The host split at dots: `["api", "example", "com"]` |
| `port` | string or `undefined` | The port written in the URL |
| `path` | string[] | The path segments: `["users", "42"]` |
| `hash` | string or `undefined` | The part after `#` |
| `query` | [query list](#header-and-query-lists) | The query parameters; changing them rewrites the URL |

In a pre-request script the URL is not resolved yet, so parts may be `{{variables}}`: for `{{base}}/users`, `getHost()` returns `{{base}}`.

```js title="Pre-request script"
pm.request.url.query.upsert({ key: "page", value: "2" });
pm.request.url.query.add("debug=true");
pm.request.url.query.remove("legacy");
console.log(pm.request.url.toString());
```

Query values are written into the URL text as you give them. Encode them yourself with `encodeURIComponent` when they contain `&`, `=`, `#` or other special characters.

### pm.request.body

| Member | Type | Description |
|---|---|---|
| `raw` | string | The body text. Assign to replace it; objects are stored as JSON. |
| `mode` | `"raw"` | Always `"raw"` |
| `update(value: string or {raw})` | `undefined` | Replaces the body: `update("text")` or `update({ raw: "text" })` |
| `isEmpty()` | boolean | Whether the body is empty |
| `toString()` | string | The body text |
| `toJSON()` | object | `{ mode: "raw", raw }` |

```js title="Pre-request script"
const body = JSON.parse(pm.request.body.raw || "{}");
body.sentAt = new Date().toISOString();
pm.request.body.raw = JSON.stringify(body);
```

### Header and query lists

`pm.request.headers`, `pm.response.headers` and `pm.request.url.query` are Postman-style property lists of `{ key, value }` items. Header names are compared without case; query keys with case.

| Method | Returns | Description |
|---|---|---|
| `get(name)` | string or `undefined` | Value of the first item named `name` |
| `one(name)` | `{key, value}` or `undefined` | The first item named `name` |
| `has(name, value?)` | boolean | Whether an item named `name` exists (with exactly `value`, when given) |
| `all()` | `{key, value}[]` | Copies of all items, in order |
| `toObject()` | object | `{ key: value }`; with duplicates the last one wins |
| `count()` | number | Number of items |
| `each(fn)`, `map(fn)`, `filter(fn)`, `find(fn)` | as for arrays | Array methods over `all()` |
| `add(item, value?)` | `undefined` | Adds an item (duplicates allowed) |
| `append(item, value?)` | `undefined` | Same as `add` |
| `upsert(item, value?)` | `undefined` | Sets the value of the first item with that key, or adds it |
| `remove(nameOrPredicate)` | `undefined` | Removes every item with that key, or every item for which `predicate({key, value})` is true |
| `clear()` | `undefined` | Removes all items |
| `toJSON()` | `{key, value}[]` | Same as `all()` |
| `toString()` | string | Headers: `Name: value` lines. Query: the query string. |

An item can be given as `{ key, value }`, `{ name, value }`, a `"Name: value"` string (headers), a `"key=value"` string (query), or as two arguments `(name, value)`. Values are stored as text. A query parameter without `=` has the value `null`.

```js
pm.request.headers.add({ key: "X-Trace", value: "on" });
pm.request.headers.add("X-Client: docs");
pm.request.headers.upsert("Accept", "application/json");
pm.request.headers.remove((h) => h.key.startsWith("X-Debug-"));
```

`pm.response.headers` has the same methods, but it's a copy: changing it does nothing.

## pm.response

Available in post-response scripts. In pre-request scripts `pm.response` is `undefined`.

| Member | Type | Description |
|---|---|---|
| `code` | number | The status code, for example `200` |
| `status` | string | The reason phrase, for example `OK` (the server's, or the standard one when the server sends none, as in HTTP/2) |
| `reason()` | string | Same as `status` |
| `responseTime` | number | Total time in milliseconds (may have decimals) |
| `responseSize` | number | Body size in bytes |
| `headers` | [header list](#header-and-query-lists) | The response headers (read-only copy, names without case) |
| `text()` | string | The body as text (invalid UTF-8 replaced) |
| `json()` | any | The body parsed as JSON. Throws `SyntaxError: pm.response.json(): the response body is not valid JSON (…)` when it isn't. |
| `to` | response assertion | Chai-style assertions on the response; see [Response assertions](../assertions/#response-assertions) |
| `events` | `{event, data, id}[]` | Only for Server-Sent Events requests in a collection run; see below |
| `toJSON()` | object | `{ code, status, header: [{key, value}], body }` |

```js
const json = pm.response.json();
console.log(pm.response.code, pm.response.status, pm.response.responseTime);
console.log(pm.response.headers.get("content-type"));
```

Bodies larger than 16 MB reach scripts cut to their first 16 MB, with a console warning. See [limits](../sandbox/#limits).

### pm.response.events

For an SSE request in a collection run, `pm.response.events` is the list of events read before reading stopped. Each event is:

| Field | Type | Description |
|---|---|---|
| `event` | string | The event name; `"message"` for events sent without an `event:` line |
| `data` | string | The event's data (lines joined with `\n`) |
| `id` | string or `null` | The last event ID in effect when the event arrived (an `id:` from an earlier event carries over), or `null` |

For an SSE request, `pm.response.text()` is the events in wire format (`event:`, `id:` and `data:` lines). For every other request `pm.response.events` is `undefined`. See [Repeat until and event streams](../../testing/repeat-and-streams/#event-streams-in-runs).

```js
const done = pm.response.events.find((e) => e.event === "done");
pm.test("job finished", () => pm.expect(done).to.exist);
```

## pm.test

```ts
pm.test(name: string, fn?: () => void | Promise<void>): void
pm.test(name: string, fn: (done: (error?: any) => void) => void): void
pm.test.skip(name: string, fn?: Function): void
```

Adds a named test. The function runs right away:

- A function without parameters **passes** when it returns without throwing, and fails with the error's message when it throws.
- An `async` function (or one that returns a promise) passes when the promise resolves and fails when it rejects. If it never settles, the test fails with `The test did not finish: its promise never settled`.
- A function with a parameter gets a `done` callback. Call `done()` to pass or `done(error)` to fail. If `done` is never called, the test fails with `The test did not finish: done() was never called`.
- `pm.test(name)` without a function, and `pm.test.skip(name, fn)`, record a **skipped** test. Skipped tests neither pass nor fail.

A failing test doesn't stop the script. `pm.test` returns `undefined`.

```js
pm.test("status is 200", () => pm.response.to.have.status(200));

pm.test("async check", async () => {
  const body = await Promise.resolve(pm.response.json());
  pm.expect(body.items).to.be.an("array");
});

pm.test.skip("pagination (not deployed yet)", () => {
  pm.expect(pm.response.json().next).to.exist;
});
```

There are no timers in the sandbox, so `done` and promises can only wait for other promises, not for time to pass.

## pm.expect

```ts
pm.expect(value: any, message?: string): Assertion
```

Starts a chai-style assertion. When `message` is given, it's put in front of the failure message: `status check: expected 1 to equal 2`. Every supported chain and assertion is listed in [Assertions](../assertions/).

```js
pm.expect(pm.response.json().name).to.be.a("string").and.not.be.empty;
```

Passing `pm.response` to `pm.expect` gives the [response assertions](../assertions/#response-assertions): `pm.expect(pm.response).to.have.status(200)`.

## pm.execution

| Member | Description |
|---|---|
| `pm.execution.setNextRequest(name: string or null)` | In a collection run, which request runs next |
| `pm.execution.skipRequest()` | Not supported: throws `pm.execution.skipRequest is not supported in Zorvik` |

`setNextRequest` takes a request's name, or its path under `requests/` (as in `pm.info.requestId`). After the current request finishes, the run continues with that request instead of the next one in the list. `setNextRequest(null)` ends the current iteration.

- It works in pre-request and post-response scripts. The last call wins; a post-response call wins over a pre-request one.
- A name is looked up among the requests of the run: the first request with that name, then a request with that path.
- If no request of the run has that name or path, the iteration ends, with a console warning: `setNextRequest: no request named 'X' in this run; the iteration ends here.`
- An iteration can send at most 10,000 requests. A loop that goes beyond that stops the run with `Stopped: iteration N sent more than 10000 requests (a setNextRequest loop?)`.
- A single send ignores it.

`postman.setNextRequest(name)` does the same. See [Collection runner](../../testing/collection-runner/#changing-the-order-with-setnextrequest).

```js title="Post-response script of 'Check job'"
if (pm.response.json().status !== "done") {
  pm.execution.setNextRequest("Check job");   // run this request again
}
```

## console

| Method | Level |
|---|---|
| `console.log(...values)` | log |
| `console.info(...values)` | info |
| `console.warn(...values)` | warn |
| `console.error(...values)` | error |
| `console.debug(...values)` | debug |
| `console.dir(...values)` | log |
| `console.trace(...values)` | debug |
| `console.clear()` | Does nothing |

Values are joined with spaces. Strings are shown as they are, objects and arrays as JSON, errors as `Name: message`, functions as `[Function: name]`. When the first argument is a string with placeholders, these are replaced: `%s` (text), `%d` (number), `%i` (whole number), `%f` (decimal number), `%j`, `%o` and `%O` (JSON), and `%%` (a percent sign).

## Legacy Postman API

Older Postman collections use these forms. They keep working.

| Legacy form | Equivalent |
|---|---|
| `tests["name"] = condition` | A test named `name` that passes when `condition` is truthy (added after the `pm.test` results) |
| `postman.setEnvironmentVariable(name, value)` | `pm.environment.set` |
| `postman.getEnvironmentVariable(name)` | `pm.environment.get` |
| `postman.clearEnvironmentVariable(name)` | `pm.environment.unset` |
| `postman.clearEnvironmentVariables()` | `pm.environment.clear` |
| `postman.setGlobalVariable(name, value)` | `pm.globals.set` |
| `postman.getGlobalVariable(name)` | `pm.globals.get` |
| `postman.clearGlobalVariable(name)` | `pm.globals.unset` |
| `postman.clearGlobalVariables()` | `pm.globals.clear` |
| `postman.getResponseHeader(name)` | `pm.response.headers.get(name)` |
| `postman.setNextRequest(name)` | `pm.execution.setNextRequest(name)` |
| `environment` | A copy of the environment's values when the script started |
| `globals` | A copy of the global values when the script started |
| `data` | A copy of the data file row |
| `iteration` | `pm.info.iteration` |
| `responseBody` | `pm.response.text()` (post-response only) |
| `responseCode` | `{ code, name, detail }` where `name` and `detail` are the reason phrase (post-response only) |
| `responseHeaders` | `pm.response.headers.toObject()` (post-response only) |
| `responseTime` | `pm.response.responseTime` (post-response only) |

```js
tests["Status code is 200"] = responseCode.code === 200;
postman.setEnvironmentVariable("token", JSON.parse(responseBody).token);
```

## Not supported

These exist in Postman but not in Zorvik. Calling them (or, for properties, reading them) throws an error `X is not supported in Zorvik`, so a script fails clearly instead of silently doing nothing.

| API | Error message |
|---|---|
| `pm.sendRequest(…)` | `pm.sendRequest is not supported in Zorvik` |
| `pm.require(…)` | `pm.require is not supported in Zorvik` |
| `require(…)` | `require is not supported in Zorvik` |
| `pm.execution.skipRequest()` | `pm.execution.skipRequest is not supported in Zorvik` |
| `pm.cookies` | `pm.cookies is not supported in Zorvik` |
| `pm.response.cookies` | `pm.response.cookies is not supported in Zorvik` |
| `pm.visualizer` | `pm.visualizer is not supported in Zorvik` |
| `pm.vault` | `pm.vault is not supported in Zorvik` |
| `pm.response.to.have.jsonSchema(…)` | `pm.response.to.have.jsonSchema is not supported in Zorvik` |
| `setTimeout`, `setInterval`, `setImmediate`, `clearTimeout`, `clearInterval`, `clearImmediate` | `setTimeout is not supported in Zorvik` (and so on) |
| `CryptoJS` | `CryptoJS is not supported in Zorvik` |
| `xml2Json(…)` | `xml2Json is not supported in Zorvik` |

`pm.cookies`, `pm.response.cookies`, `pm.visualizer`, `pm.vault` and `CryptoJS` throw as soon as they're read, so even `typeof CryptoJS` or `if (pm.cookies)` throws. To read cookies the server set, read the header: `pm.response.headers.get("set-cookie")`.

Other Postman sandbox libraries (`lodash`/`_`, `moment`, `cheerio`, `tv4`, `ajv`, `chai` as a global) are not defined. See [Sandbox and limits](../sandbox/#postman-compatibility) for the full comparison.

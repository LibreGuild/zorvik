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
| `require(name)`, `pm.require(name)` | The [built-in libraries](../sandbox/#libraries): lodash, crypto-js, moment, ajv, … |
| `CryptoJS`, `_`, `tv4`, `cheerio` | Postman's library globals: crypto-js, lodash (with [Postman's lodash 3 names](../sandbox/#postmans-_)), tv4 and cheerio, loaded when first used |
| `Buffer` | Node's `Buffer`, as in Postman's sandbox |
| SugarJS methods | `"text".has("x")`, `(5).hex()`, `Date.create("today")`, …: see [SugarJS](../sandbox/#sugarjs) |
| `xml2Json(text)` | XML to a JSON object, as in Postman (xml2js; a single child element is not an array). Throws for text that isn't XML |
| `crypto.getRandomValues(array)`, `crypto.randomUUID()` | Secure random numbers and UUIDs |
| `postman`, `tests`, `request`, `responseBody`, `responseCode`, `responseHeaders`, `responseTime`, `responseCookies`, `environment`, `globals`, `data`, `iteration` | The [legacy Postman API](#legacy-postman-api) |
| Standard JavaScript | `JSON`, `Math`, `Date`, `RegExp`, `Array`, `Object`, `Map`, `Set`, `Promise`, `queueMicrotask`, typed arrays and the other built-ins of the language |

See [Sandbox and limits](../sandbox/) for what is not available (network, files, timers, npm packages).

## pm.info

Read-only facts about the current script.

| Property | Type | Description |
|---|---|---|
| `pm.info.eventName` | `"prerequest"`, `"test"` or `"message"` | Which script is running: `"prerequest"` in pre-request scripts, `"test"` in post-response scripts (Postman's names), `"message"` in [message scripts](#pmmessage-and-pmconnection) |
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
| `pm.iterationData` | The current row of the run's data file | Only for the script itself |

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

The current row of the run's data file (see [Data files](../../testing/data-files/)). It's empty in a single send. `set`, `unset` and `clear` change it for the rest of the script only; the next script gets the row as it is in the file.

| Method | Returns | Description |
|---|---|---|
| `get(name: string)` | any | The column's value, or `undefined` |
| `has(name: string)` | boolean | Whether the row has the column |
| `toObject()` | object | The row as `{ column: value }` |
| `toJSON()` | object | Same as `toObject()` |
| `replaceIn(text: string)` | string | `text` with `{{column}}` replaced by the row's values |

CSV values are always strings. JSON values keep their type: `pm.iterationData.get("count")` returns the number `3` for `{"count": 3}`.

### How values are stored

Variables are text outside of scripts. When a script sets a value, its text is what `{{name}}` in requests uses and what is kept after the send:

| Value passed to `set` | Text |
|---|---|
| A string | The string itself |
| A number or boolean | Its text, for example `"5"` or `"true"` |
| An object or array | JSON, for example `{"x":1}` |
| `undefined` | An empty string |
| `null` | The text `"null"` |

Scripts get the value back as it was set, as in Postman: in the same script, and in the later scripts of the same send or collection run (as long as nothing else changed the variable). After the send, or in a new run, they get the text:

```js
pm.collectionVariables.set("ids", [1, 2, 3]);
// A later request of the run:
const ids = pm.collectionVariables.get("ids");   // [1, 2, 3], an array
const next = ids.shift();
pm.collectionVariables.set("ids", ids);
```

To be sure of the type in any script, store JSON and parse it when reading: `JSON.parse(pm.collectionVariables.get("ids"))` works with the text too.

### What happens to values after the send

| Scope | In the app | In `zorvik run` |
|---|---|---|
| `pm.variables` | Discarded after the send or run | Discarded after the run |
| `pm.environment` | Kept on this computer for the active environment. Without an active environment, not kept (with a console warning). | Kept until the run ends |
| `pm.collectionVariables` | Kept on this computer | Kept until the run ends |
| `pm.globals` | Kept on this computer, shared by every workspace | Kept until the run ends; starts empty |

Values kept on this computer live in the app's data folder, not in the workspace files, and win over the value saved in the file. `unset` removes the value a script set, so the value saved in the file (if any) applies again from the next send. See [Variables and environments](../../variables/variables-and-environments/).

### Dynamic variables in replaceIn

`replaceIn` also fills in [dynamic variables](../../variables/dynamic-variables/) (`{{$guid}}`, `{{$timestamp}}`, `{{$randomEmail}}`, every one Postman has and more) when no scope has the name. Nested variables are resolved up to 10 levels deep, including names made of variables (`{{host_{{env}}}}`); names that aren't found stay as they are.

## pm.request

The request. In a pre-request script you can change it; in a post-response script it's what was sent (read-only in effect). See [what a pre-request script sees](../overview/#what-a-pre-request-script-sees).

| Member | Type | Description |
|---|---|---|
| `pm.request.url` | [URL object](#pmrequesturl) | The URL. Assign a string to replace it: `pm.request.url = "https://…"`. |
| `pm.request.method` | string | The method. Assigning sets it in upper case: `pm.request.method = "post"` sends `POST`. |
| `pm.request.headers` | [header list](#header-and-query-lists) | The request's headers (names compared without case) |
| `pm.request.body` | [body object](#pmrequestbody) | The body: text, or the fields of a form |
| `pm.request.description` | string or `undefined` | The request's docs |
| `pm.request.to` | request assertion | `pm.request.to.have.header("Accept")`; see [Request assertions](../assertions/#request-assertions) |
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
| `getPath(unresolved?)` | string | The path, `/` when there is none. `:name` path variables are replaced by their values unless `unresolved` is `true`. |
| `getQueryString()` | string | The query without `?`, or an empty string |
| `getPathWithQuery(unresolved?)` | string | Path and query, for example `/users/7?page=1` |
| `getRemote()` | string | Host and port, for example `api.example.com:8443` (no port when none is written) |
| `protocol` | string or `undefined` | For example `https` |
| `host` | string[] | The host split at dots: `["api", "example", "com"]` |
| `port` | string or `undefined` | The port written in the URL |
| `path` | string[] | The path segments: `["users", "42"]` |
| `hash` | string or `undefined` | The part after `#` |
| `query` | [query list](#header-and-query-lists) | The query parameters; changing them rewrites the URL |
| `variables` | [list](#header-and-query-lists) | The `:name` path variables and their values; in a pre-request script, changing them changes what is sent |

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
| `mode` | string or `undefined` | Postman's body mode: `"raw"` (JSON, Text, XML), `"urlencoded"`, `"formdata"`, `"file"` or `"graphql"`; `undefined` without a body |
| `raw` | string or `undefined` | The body text in `raw` mode. Assign to replace it (objects are stored as JSON); that makes it a raw body. |
| `urlencoded` | [list](#header-and-query-lists) or `undefined` | The fields of a URL-encoded body: `{ key, value }` |
| `formdata` | [list](#header-and-query-lists) or `undefined` | The fields of a form-data body: `{ key, value, type: "text" }` or `{ key, src, type: "file" }` (`src` is the file's path) |
| `update(value)` | `undefined` | Replaces the body: `update("text")`, `update({ raw: "text" })`, `update({ mode: "urlencoded", urlencoded: [{ key, value }] })` or `update({ mode: "formdata", formdata: […] })` |
| `isEmpty()` | boolean | Whether the body is empty (no text, or no fields) |
| `toString()` | string | The text; for a URL-encoded body the encoded fields (`a=1&b=2`) |
| `toJSON()` | object | `{ mode: "raw", raw }`, or `{ mode, urlencoded }` / `{ mode, formdata }` |

Only the enabled fields are in the lists. In a pre-request script, changing a field list changes what is sent: switched-off fields stay as they are, and form-data fields you didn't change keep their content type.

```js title="Pre-request script"
const body = JSON.parse(pm.request.body.raw || "{}");
body.sentAt = new Date().toISOString();
pm.request.body.raw = JSON.stringify(body);
```

```js title="Pre-request script: sign a form"
const fields = pm.request.body.urlencoded.all().map((f) => f.key + "=" + f.value).join("&");
pm.request.body.urlencoded.upsert({ key: "sig", value: CryptoJS.HmacSHA256(fields, pm.environment.get("key")).toString() });
```

### Header and query lists

`pm.request.headers`, `pm.response.headers`, `pm.request.url.query`, `pm.request.url.variables` and the body's field lists are Postman-style property lists of `{ key, value }` items (form-data fields also have `type` and `src`). Header names are compared without case; query keys with case.

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
| `cookies` | cookie list | The cookies this response set (`Set-Cookie`); see [pm.cookies](#pmcookies) for its methods |
| `stream` | `Buffer` | The body as a `Buffer`: `res.stream.toString()` is the text |
| `details()` | object | `{ code, name, detail }`, `name` and `detail` being the reason phrase |
| `size()` | object | `{ body, header, total }` in bytes (`header`: the headers' `Name: value` lines) |
| `contentInfo()` | object | `{ mimeType, mimeFormat, charset, fileExtension, fileName }` from the `Content-Type` and `Content-Disposition` headers |
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
| `pm.execution.skipRequest()` | In a pre-request script: don't send this request. A single send shows **Not sent**; a collection run reports it as skipped (with the reason) and goes on. In a post-response script it does nothing. |
| `pm.execution.location` | The names of the folders the request is in, outermost first, then the request's: `["Shop", "Users", "Get user"]`. `pm.execution.location.current` is the name of the folder (or request) whose script is running. |

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

## pm.sendRequest

Sends another request from a script: to get a token before the request, to create test data, or to clean up after it.

```js title="Pre-request script: get a token first"
const res = await pm.sendRequest({
  url: pm.variables.replaceIn("{{baseUrl}}/login"),
  method: "POST",
  header: { "Content-Type": "application/json" },
  body: { mode: "raw", raw: JSON.stringify({ user: pm.environment.get("user"), password: pm.environment.get("password") }) },
});
pm.request.headers.upsert({ key: "Authorization", value: "Bearer " + res.json().token });
```

| Form | |
|---|---|
| `pm.sendRequest(url)` | A `GET` of that URL |
| `pm.sendRequest({ url, method, header, body })` | `header`: `[{key, value, disabled}]`, `{ name: value }` or `"Name: value"` lines. `body.mode`: `raw` (`raw` text; with `options.raw.language: "json"` it gets `Content-Type: application/json`), `urlencoded` (`[{key, value}]`), `formdata` (text fields), `graphql` (`{query, variables}`) |
| `pm.sendRequest(request, (err, res) => …)` | Calls back later with an error or the [response](#pmresponse) (with `code`, `headers`, `json()`, `cookies`…) |
| `await pm.sendRequest(request)` | Without a callback it returns a promise. `await` works at the top level of a script. |

- `{{variables}}` in the request are **not** filled in, as in Postman: use `pm.variables.replaceIn`.
- It uses the app's settings (proxy, certificates, the cookie jar) and the time left of the script's [time limit](../sandbox/#limits). A request an AI agent's run makes may only go to hosts the user approved.
- Each call shows in the Console: `pm.sendRequest POST https://api.test/login → 200 OK`.
- A script can send up to 100 requests. A failure (no connection, a timeout) reaches the callback as `err`, or rejects the promise.

## pm.cookies

The cookies the [cookie jar](../../requests/cookies/) sends to this request's URL (before sending in a pre-request script, after the response in a post-response script). `pm.response.cookies` has the same methods for the cookies the response set.

| Method | Returns |
|---|---|
| `get(name)` | The value, or `undefined` |
| `has(name, value?)` | Whether the cookie is there (with that value) |
| `one(name)` | `{ name, value, domain, path, expires, secure, httpOnly }` |
| `all()`, `toObject()`, `count()`, `each(fn)`, `filter(fn)`, `map(fn)` | The list, `{ name: value }`, how many, … |

`pm.cookies.jar()` reads and changes the jar, for the request's own site only (the same host, or its subdomains or parent domain): another site's cookies are refused with *Scripts can only use the cookies of the request's own site*.

| Method | Does |
|---|---|
| `get(url, name, (err, value) => …)` | One cookie's value |
| `getAll(url, (err, cookies) => …)` | Every cookie for that URL |
| `set(url, name, value, (err) => …)` | Store a cookie (path `/`) |
| `unset(url, name, (err) => …)`, `clear(url, (err) => …)` | Remove one, or all, for that URL |

Without a callback each returns a promise. With the cookie jar switched off (Settings → Requests), the jar methods fail with *The cookie jar is off*.

## pm.message and pm.connection

Only in message scripts of WebSocket and Socket.IO requests (see [WebSocket](../../protocols/websocket/#message-scripts) and [Socket.IO](../../protocols/socketio/#scripts)).

| Name | Description |
|---|---|
| `pm.message.text()` | The message as text (a binary message: its bytes as text) |
| `pm.message.json()` | The message parsed as JSON; Socket.IO: the arguments array |
| `pm.message.args` | Socket.IO: the event's arguments |
| `pm.message.event` | Socket.IO: the event's name |
| `pm.message.direction` | `"received"` or `"sent"` |
| `pm.message.size`, `pm.message.timestamp`, `pm.message.isBinary` | Bytes, Unix milliseconds, and whether it is binary |
| `pm.connection.send(data)` | Sends a message: text as is, anything else as JSON. Socket.IO: emits `message` with `data`. |
| `pm.connection.emit(event, ...args)` | Socket.IO: emits `event` with these arguments |

A script sends at most 100 messages per run; they go out after it ends, in order, even when it fails later.

## pm.visualizer

Shows the response your way: a table, a list, a summary. The response pane gets a **Visualize** tab.

```js title="Post-response script"
const template = `
  <table>
    <tr><th>Name</th><th>Price</th></tr>
    {{#each items}}<tr><td>{{name}}</td><td>{{price}}</td></tr>{{/each}}
  </table>`;
pm.visualizer.set(template, { items: pm.response.json() });
```

| Method | |
|---|---|
| `set(template, data)` | Renders a [Handlebars](https://handlebarsjs.com) template with `data` (values are HTML-escaped; `{{{raw}}}` isn't) |
| `clear()` | Removes it |

The result is plain HTML and CSS in a sandbox: **scripts in the template don't run** and nothing loads from the internet, so Postman templates that draw charts with a JavaScript library show only their HTML. Inline `<svg>` and CSS work. At most 5 MB.

## Timers

`setTimeout`, `setInterval`, `setImmediate` and their `clear…` functions work. The script waits for its timers (and the promises they start) before it ends, within its [time limit](../sandbox/#limits): an interval nobody clears runs until the script is stopped.

```js
setTimeout(() => pm.environment.set("checkedAt", Date.now()), 500);
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
| `tests["name"] = condition` | A test named `name` that passes when `condition` is truthy (added after the `pm.test` results). A failing one says `expected <value> to be truthy`. |
| `postman.setEnvironmentVariable(name, value)` | `pm.environment.set`, storing `value.toString()` as Postman does (an object becomes `[object Object]`) |
| `postman.getEnvironmentVariable(name)` | `pm.environment.get` |
| `postman.clearEnvironmentVariable(name)` | `pm.environment.unset` |
| `postman.clearEnvironmentVariables()` | `pm.environment.clear` |
| `postman.setGlobalVariable(name, value)` | `pm.globals.set`, storing `value.toString()` |
| `postman.getGlobalVariable(name)` | `pm.globals.get` |
| `postman.clearGlobalVariable(name)` | `pm.globals.unset` |
| `postman.clearGlobalVariables()` | `pm.globals.clear` |
| `postman.getResponseHeader(name)` | `pm.response.headers.get(name)` (post-response only) |
| `postman.getResponseCookie(name)` | `pm.cookies.one(name)`: the cookie `{ name, value, … }` (post-response only) |
| `postman.setNextRequest(name)` | `pm.execution.setNextRequest(name)` |
| `request` | `{ id, name, description, method, url, headers, data }`: `headers` as `{ name: value }`, `data` the body text, or the form fields as `{ key: value }` (an array for repeated keys) |
| `environment`, `globals` | The environment's and the global values when the script started; the `postman.*` setters above change them too |
| `data` | A copy of the data file row |
| `iteration` | `pm.info.iteration` |
| `responseBody` | `pm.response.text()` (post-response only) |
| `responseCode` | `{ code, name, detail }` where `name` and `detail` are the reason phrase (post-response only) |
| `responseHeaders` | `pm.response.headers.toObject()` (post-response only) |
| `responseTime` | `pm.response.responseTime` (post-response only) |
| `responseCookies` | `pm.cookies.all()` (post-response only) |

```js
tests["Status code is 200"] = responseCode.code === 200;
tests["Body has the name"] = responseBody.has("name");   // SugarJS, as in Postman
postman.setEnvironmentVariable("token", JSON.parse(responseBody).token);
```

## Not supported

These exist in Postman but not in Zorvik. Using them throws an error, so a script fails clearly instead of silently doing nothing.

| API | Error message |
|---|---|
| `pm.vault` | `pm.vault is not supported in Zorvik` (use [secret variables](../../variables/secrets/)) |
| `pm.execution.runRequest(…)` | `pm.execution.runRequest is not a function` (use [`pm.sendRequest`](#pmsendrequest)) |

`pm.vault` throws as soon as it's read, so even `if (pm.vault)` throws.

`require` of a package that isn't [built in](../sandbox/#libraries) throws `Cannot find module 'name'. Scripts can require only these built-in libraries: …`. `pm.require` of a package from Postman's team package library throws `pm.require: '@team/package' is from a Postman package library, which Zorvik can't reach. Copy its code into the script.` See [Sandbox and limits](../sandbox/#postman-compatibility) for the full comparison.

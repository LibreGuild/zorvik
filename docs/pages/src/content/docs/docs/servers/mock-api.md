---
title: Mock APIs
description: Routes with :params, matching on query, headers, body and JSON values, templated responses, answer sequences, route order, CORS, and forwarding unmatched requests to a real backend (and recording its answers as routes).
sidebar:
  order: 1
---

A mock API is a local HTTP server that answers with the responses you define. Use it to build a front end before the backend exists, to test how a client handles errors and slow answers, or to stand in for a third-party service in tests.

![A mock API: the routes on the left, live traffic on the right](/zorvik/shots/mock-dark.webp)

A mock API is a list of **routes**. Each route says "for this method and path (and, optionally, these query parameters, headers or body), answer with this status, these headers and this body". It speaks HTTP/1.1 and HTTP/2, and can listen with TLS. Like every server, it is saved as a YAML file under `servers/` in the workspace, so it is shared through Git.

## Create a mock API

Open the **Servers** sidebar, click **+** (**New server**) and choose **Mock API (HTTP)**. Give it a name. The new mock listens on `127.0.0.1`, port 3000 (or the next port no other saved server uses), and has one route to start from: `GET /hello` answering `200` with `{"message": "Hello from Zorvik"}`.

Press **Start** (or <kbd>Mod</kbd>+<kbd>Enter</kbd>) and call it at the address shown, for example `http://127.0.0.1:3000/hello`.

You can also build a mock from what you already have: an OpenAPI document, a folder of requests, or a response you just received. See [Mocks from OpenAPI, folders and responses](../from-openapi-and-folders/).

## Routes

The **Routes** section lists the routes. Each row shows an on/off checkbox, the method, the path, the route's name, a clock when it has a delay, a numbered list when it has an [answer sequence](#answer-sequences), a lightning bolt when it has a fault, and the status.

- **Add route** adds `GET /new` (or `/new-2`, …) answering `200` with a JSON body.
- Click a row to edit that route below the list. Arrow keys move through the list.
- Drag a row by its handle to reorder it, or use **Move up** / **Move down** in its **⋯** menu. The menu also has **Duplicate** and **Delete**.
- Unchecking a route turns it off: it never answers and is not listed in 404 answers.

### Route fields

| Field | Description |
|---|---|
| **Method** | `ANY` (saved as `*`), `GET`, `POST`, `PUT`, `PATCH`, `DELETE`, `HEAD` or `OPTIONS`. Another method written in the file (such as `PROPFIND`) is kept and matched. |
| **Path** | The path pattern, see [Paths](#paths). |
| **Status** | The status code, 100 to 999. The field suggests common codes. |
| **Name** | Optional. Shown in the traffic log instead of the method and path. |
| **Response headers** | Header rows. Names and values may use [templates](../templates/). |
| **Response body** | The body, with syntax highlighting chosen from the `Content-Type` header (JSON, HTML, XML, JavaScript or text). **Format JSON** reformats a JSON body. The body may use [templates](../templates/). |
| **Delay (ms)** | Wait this long before answering. See [Faults, delays & CORS](../faults-delays-cors/). |
| **Fault** and **How often (%)** | Answer with an error, drop the connection, or never answer, some or all of the time. See [Faults, delays & CORS](../faults-delays-cors/). |
| **Answer sequence** | Other answers for the first calls, for example `503`, `503`, then the route's own answer. See [Answer sequences](#answer-sequences). |
| **Match only when** | Extra conditions on query parameters, headers, the body and values of a JSON body. See [Conditions](#conditions). |

While the server runs, the route editor shows the route's full address (**Call it at …**) with a copy button.

## Paths

| Pattern | Matches | Parameters |
|---|---|---|
| `/users` | Exactly `/users` | none |
| `/users/:id` | `/users/42`, `/users/abc` | `{{request.params.id}}` |
| `/users/{id}` | Same as `:id` (OpenAPI style) | `{{request.params.id}}` |
| `/orgs/:org/repos/:repo` | `/orgs/acme/repos/api` | `org`, `repo` |
| `/files/*` | `/files`, `/files/a`, `/files/a/b/c.txt` | `{{request.params.*}}` is the rest: `a/b/c.txt` |
| `*` | Every path | `{{request.params.*}}` is the whole path without the leading `/` |
| `/a/*/c` | `/a/b/c`, `/a/x/c` (one segment, any value) | none |
| `/search?type=user` | `/search` with `type=user` in the query | none |

Rules:

- A parameter (`:name` or `{name}`) matches exactly one path segment. Its value is percent-decoded: `/name/j%C3%BCrgen` gives `jürgen`.
- A `*` as the **last** segment matches the rest of the path, including nothing. A `*` anywhere else matches one segment and is not captured.
- Literal segments are **case-sensitive** and compared after percent-decoding.
- A trailing slash doesn't matter, and neither do repeated slashes: `/users/42/` matches `/users/:id`.
- A query in the pattern (`?type=user`) is an extra condition, like a row under **Query parameters**. Anything after `#` is ignored.
- `{{id}}` is **not** a parameter in a route path; it is compared as literal text. Write `:id`.

## Methods

A route's method matches case-insensitively. `ANY` (`*`, or an empty method) matches every method.

`HEAD` requests use `GET` routes when no route matches `HEAD` itself. The answer has the headers and the `Content-Length` a `GET` would get, without a body.

## Conditions

Open **Match only when** to add conditions. A route answers only when **all** its conditions hold.

| Condition | Holds when | Empty value |
|---|---|---|
| **Query parameters** | The request has the parameter with exactly this value (after decoding `%XX` and `+`). Any of its values may match when it repeats. Names are case-sensitive. | The parameter is present, with any value. |
| **Headers** | The request has the header (name in any case) with exactly this value (surrounding spaces ignored; the value is case-sensitive). | The header is present, with any value. |
| **Body contains** | The request body contains this text (case-sensitive). | No condition. |
| **JSON body** | The body is JSON and meets the condition, see below. | No condition. |

Condition values may use variables such as `{{role}}` (from the active environment or the workspace), but not request values. Rows that are unchecked or have no name are ignored. A header name that is not a valid HTTP header name never matches.

### JSON body conditions

A **JSON body** condition looks at one value of a JSON request body, picked with a JSON path:

| Condition | Holds when |
|---|---|
| `$.user.role == "admin"` | The value is `admin`. |
| `$.items[0].qty != 0` | The value is anything but `0` (also when it isn't there). |
| `$.user.id` | The value is there and not `null`. |
| `$.coupon == null` | The value is `null` or isn't there. |

Paths start at `$` (which may be left out) and use `.name`, `['name with spaces']` and `[0]` (`[-1]` is the last item). The value after `==` or `!=` is JSON (`"text"`, `7`, `true`, `null`) or bare text: `$.type == admin` works too. Numbers and booleans compare as text, so `$.id == 7` and `$.id == "7"` both match `{"id": 7}`. A body that isn't JSON never matches. While a condition can't be read (for example `$..id`), the field turns red and says why, and the route doesn't match.

Two routes for the same path can then answer differently by what is sent: `POST /login` with `$.password == "wrong"` answers `401`, and the next `POST /login` route answers `200`.

## Answer sequences

An answer sequence gives a route's first calls other answers, in order. It is how you test a client that retries: the first two calls get `503 Service Unavailable`, the third gets the route's own `200`.

Open **Answer sequence** under the route and click **Add answer**. Each answer has:

| Field | Description |
|---|---|
| **Answer** | **Answer** sends the status, body and headers below; **Error (500)**, **Drop the connection** and **Never answer** work like the route's [faults](../faults-delays-cors/#faults), every time. |
| **Status** | The status of this answer (new answers start with `503`). |
| **Delay** | Wait this long before this answer. Empty: the route's delay. |
| **Body** | Click **Body** to edit the body and **extra headers**. The body may use [templates](../templates/). Extra headers are added to the route's headers; one with the same name (for example `Content-Type`) replaces the route's. `Retry-After` is a common one. |

The last row shows what comes after: **The route's own answer, from then on**. Turn on **Start over after the route's answer** for a backend that keeps failing now and then: with two answers, calls 1 and 2 get the sequence, call 3 the route's answer, call 4 starts over, and so on.

Counting starts again when the server starts (or restarts), and when you change the route's method, path, status, conditions or sequence; editing its own body or headers, or other routes, doesn't reset it. Each route counts its own calls, whatever client sends them. The route's own **Fault** and **How often (%)** apply only to the route's own answer.

The traffic log shows which answer each call got, such as **answer 2 of 3** next to the route, or **reset · answer 2 of 3** for an answer that drops the connection.

## Route order

For each request the mock goes through the routes **from top to bottom** and uses the **first enabled route** whose method, path and conditions all match. Put specific routes above general ones:

| # | Route | Answers |
|---|---|---|
| 1 | `GET /users/me` | the signed-in user |
| 2 | `GET /users/:id` with header `Authorization` = (any value) | a user |
| 3 | `GET /users/:id` | `401` with an error body |
| 4 | `ANY *` | `404` in your API's error format |

With this order, `/users/me` never reaches route 2, requests without an `Authorization` header fall through to route 3, and everything else gets route 4 instead of the built-in 404.

Edits apply to the next request, also while the server runs and before you save.

## The response

**Status.** The route's status is sent as is. A status the server can't send (below 100) answers `500` instead and logs **Route …: status 42 is not valid, answered 500 instead**.

**Headers.** Every checked row with a name is sent, in order; the same name may appear more than once. Names and values may use [templates](../templates/); values are trimmed. `Content-Length` and `Transfer-Encoding` rows are ignored, because the mock sets them from the body. A header that isn't valid after templating is left out and logged as **Route …: header "…" is not valid and was left out**.

**Body.** The body is sent after [templating](../templates/). Answers with status 1xx, `204` or `304` never have a body (the editor says so).

**Content-Type.** When the body is not empty and the route has no `Content-Type` header, the mock picks one:

| The body | Content-Type |
|---|---|
| Starts with `{` or `[` and is valid JSON | `application/json` |
| Starts with `<?xml` | `application/xml` |
| Starts with `<` and has `<html` in its first 512 characters | `text/html; charset=utf-8` |
| Anything else | `text/plain; charset=utf-8` |

## Requests no route matches

The **Requests no route matches** section decides what happens when no route answers.

### Answer 404 (default)

The mock answers `404` with a JSON body that names the request and lists the enabled routes (up to 100), so a typo is easy to spot:

```json
{
  "error": "No route matches GET /user/42",
  "routes": [
    "GET /users/:id",
    "ANY /files/*"
  ]
}
```

To answer unmatched requests yourself, add a last route `ANY *`.

### Forward to a backend

Choose **Forward to a backend** and enter the **Backend URL**, for example `https://api.example.com` or `{{baseUrl}}`. Requests no route matches are then sent there and the backend's answer is passed back. Mock the endpoints you are working on, and let everything else reach the real API.

How forwarding works:

- **Target URL**: the backend URL (variables replaced, trailing `/` removed) followed by the request's path and query. With `https://api.example.com/v2`, a request for `/users?page=2` goes to `https://api.example.com/v2/users?page=2`.
- **Method and body** are passed on unchanged.
- **Headers** are passed on, except `Host`, `Content-Length`, the hop-by-hop headers (`Connection`, `Keep-Alive`, `Proxy-Connection`, `Proxy-Authenticate`, `Proxy-Authorization`, `TE`, `Trailer`, `Transfer-Encoding`, `Upgrade`) and any header named in `Connection`. Zorvik adds `X-Zorvik-Forwarded: 1`, and no headers of its own (no `User-Agent`).
- **The answer** comes back as it is: status, headers (without hop-by-hop headers) and body. Redirects are passed on, not followed, and compressed bodies stay compressed.
- **Network settings**: the proxy, certificate and timeout settings from Settings apply (with `zorvik serve`, `-k` skips certificate checks).

When forwarding fails, the mock answers `502 Bad Gateway` with a JSON `error`:

| `error` | Cause |
|---|---|
| `Could not reach the backend: …` | Connection, TLS or timeout error. |
| `No backend URL is set for requests that match no route` | The **Backend URL** is empty. |
| `The request came back to the mock: the backend URL points at this server` | The backend URL loops back to the mock (the `X-Zorvik-Forwarded` header came back). |
| `The backend's answer is larger than 50 MB` | Answers over 50 MB are not passed on. |

Forwarded exchanges are marked **proxy** in the traffic log. [CORS headers](../faults-delays-cors/#cors), when turned on, are added to forwarded answers too.

### Record the backend's answers as routes

Forwarding also turns a real backend into a mock. Point your app at the mock, use it, then save what the backend answered:

- Hover a forwarded exchange in the traffic log and click **Save as route** (the plus at the end of the row) to add one route for it.
- Click **Save proxied answers as routes** at the top of the traffic panel to add a route for every forwarded exchange at once: one per method, path and query, leaving out the ones a route already answers. Routes with query conditions come first, so the one without them doesn't answer their requests.

A recorded route has the request's method and path, the query as **Query parameters** conditions, and the backend's status, headers and body. Like **Mock this response**, it leaves out `Set-Cookie` (it often holds a session, and server files are shared through Git), `Content-Length`, `Content-Encoding`, `Date` and connection headers. Credentials stay out of the file too: headers such as `Authorization` or `X-Auth-Token` are left out, query values such as `api_key`, `access_token`, `code` or `sig` are left empty (the condition then matches any value), and tokens and codes are removed from `Location` addresses. Zorvik shows a note for each, and one when the body or a header contains `{{…}}`, which the mock fills in when it answers. An answer whose body is binary, compressed or larger than 64 KB can't be recorded: its button is greyed out.

Forwards that fail (the backend is down or too slow) are marked **proxy failed** and can't be saved as routes.

The routes are added to the end of the list and the first one is selected. They answer at once while the server runs, so the next same request no longer reaches the backend. They are unsaved edits: press **Save** to keep them.

## Browser access (CORS)

Turn on **Allow cross-origin requests (CORS)** under **Browser access** when a web app on another origin (for example `http://localhost:5173`) calls the mock. The mock then answers preflight requests and adds `Access-Control-Allow-*` headers to every answer. The details are in [Faults, delays & CORS](../faults-delays-cors/#cors).

## Limits

| Limit | Value | When exceeded |
|---|---|---|
| Request body | 10 MB | `413` with `{"error": "The request body is larger than 10 MB"}` |
| Time to send the body | 30 s | `408` with `{"error": "The request body did not arrive in time"}` |
| Time to send the request head | 30 s | The connection is closed. |
| HTTP/2 streams per connection | 256 at once | |
| HTTP/2 header list | 64 KB | |

HTTP/2 works with TLS (negotiated with ALPN) and without it (clients that start with HTTP/2 directly, "prior knowledge").

A mock API only answers requests: it has no lasting connections, so the traffic panel has no composer to send messages.

## Saved format

```yaml title="servers/Users API.yaml"
name: Users API
kind: http
seq: 0
host: 127.0.0.1
port: 3000
http:
  routes:
    - name: Get user
      method: GET
      path: /users/:id
      status: 200
      headers:
        - key: Content-Type
          value: application/json
      body: |
        {"id": "{{request.params.id}}", "name": "Ada Lovelace"}
    - method: POST
      path: /login
      status: 401
      matchJson: $.password == "wrong"
      body: '{"error": "invalid credentials"}'
      delayMs: 300
    - method: GET
      path: /reports
      status: 200
      body: '{"ready": true}'
      sequence:
        - status: 503
          headers:
            - key: Retry-After
              value: "1"
          body: '{"error": "busy"}'
        - status: 503
          body: '{"error": "busy"}'
      sequenceRepeat: true
  fallback: proxy
  proxyUrl: "{{baseUrl}}"
  cors: true
```

Route fields (`http.routes[]`):

| Field | Default | Description |
|---|---|---|
| `name` | empty | Name for the traffic log. |
| `method` | `*` | HTTP method, or `*` for any. |
| `path` | empty | Path pattern. An empty path matches only `/`. |
| `status` | `200` | Status code. |
| `headers` | none | `key`, `value`, `enabled` (default `true`) rows. |
| `body` | empty | Response body (a template). |
| `delayMs` | `0` | Delay before answering, in ms (at most 24 hours). |
| `matchQuery` | none | Query conditions (`key`/`value` rows; empty value = present). |
| `matchHeaders` | none | Header conditions (`key`/`value` rows; empty value = present). |
| `matchBody` | empty | Text the body must contain. |
| `matchJson` | empty | A condition on a JSON body, such as `$.user.role == "admin"`. |
| `fault` | `none` | `none`, `error`, `reset` or `hang`. |
| `faultPercent` | `100` | How often the fault happens, 0–100. |
| `sequence` | none | Answers for the first calls, in order (at most 100), see below. |
| `sequenceRepeat` | `false` | Start the sequence over after the route's own answer. |
| `enabled` | `true` | Whether the route answers. |

Answers of a sequence (`sequence[]`):

| Field | Default | Description |
|---|---|---|
| `status` | `200` | Status code. |
| `headers` | none | Extra headers (`key`/`value` rows); one with the same name replaces the route's. |
| `body` | empty | Body (a template). |
| `delayMs` | the route's | Delay before this answer, in ms. |
| `fault` | `none` | `none`, `error`, `reset` or `hang`, every time. |

Mock settings (`http`):

| Field | Default | Description |
|---|---|---|
| `routes` | none | The routes, in order. |
| `fallback` | `notFound` | `notFound` or `proxy`. |
| `proxyUrl` | empty | Backend URL for `proxy`. |
| `cors` | `false` | Allow cross-origin requests. |

The listener fields (`host`, `port`, `tls`, `autoStart`) are the same for every kind of server: see [Running servers](../running-servers/#saved-format).

---
title: Mocks from OpenAPI, folders and responses
description: Create a mock API from an OpenAPI or Swagger document, from a folder of saved requests, or from a response you just received.
sidebar:
  order: 4
---

You rarely need to type a mock from scratch. Zorvik builds one from an API description, from the requests you already have, or from a real response. The result is an ordinary [mock API](../mock-api/): edit its routes, add conditions and templates, and start it.

## From an OpenAPI document

1. Open the **Servers** sidebar, click **+** (**New server**) and choose **Mock from OpenAPI…**.
2. In **Mock from OpenAPI**, pick the source:
   - **File**: **Choose…** an OpenAPI or Swagger document (`.json`, `.yaml` or `.yml`).
   - **URL**: the address of the document, for example `https://petstore3.swagger.io/api/v3/openapi.json`. It is downloaded with your proxy and certificate settings (for at most 2 minutes when requests have no timeout).
   - **Paste**: the document itself.
3. Enter the **Server name** (a file name suggests `<file> mock`; empty means **API mock**).
4. Click **Create mock**.

The new mock is saved and opened, but not started. It listens on `127.0.0.1` on the first port from 3000 that no other saved server uses. A message tells you how many routes it got, and, when parts of the document could not be used, how many and which (the first two).

**Supported documents**: OpenAPI 3.0 and 3.1, and Swagger 2.0, as JSON or YAML (YAML anchors and merge keys included), up to 50 MB. Documents that are not OpenAPI or Swagger are refused (**Not an OpenAPI/Swagger document**), and so are documents without operations (**The document has no operations to mock**).

### What each route gets

Every operation becomes one route, in the order of the document. Paths starting with `x-` are skipped.

| Route part | Comes from |
|---|---|
| **Method** | The operation's method (`GET`, `PUT`, `POST`, `DELETE`, `OPTIONS`, `HEAD`, `PATCH`, `TRACE`). |
| **Path** | The operation's path with `{param}` turned into `:param`: `/pets/{petId}` becomes `/pets/:petId`. The `servers` base path is **not** included. |
| **Name** | The operation's `summary`, else its `operationId`, else `METHOD /path`. |
| **Status** | The lowest `2xx` response; else a `2XX` range (answered as `200`); else `default` (answered as `200`); else `200` with no body. |
| **Content-Type** header | The media type of the chosen response. |
| **Body** | The response's example, or one generated from its schema (see below). `204` and `205` have no body. |

**Choosing the media type (OpenAPI 3)**: `application/json` first, then any other JSON type (such as `application/problem+json` or `application/vnd.api+json`), then the first one listed. `*/*` is answered as `application/json`. XML without an example gets an empty body. Other non-text types are mocked as text, with a warning.

**Choosing the media type (Swagger 2)**: the response's `examples` (a JSON media type first); otherwise the schema, with the JSON type from `produces` (or `application/json`).

### Examples and generated bodies

The body is the first of these that exists:

1. The media type's `example`, or the `value` of its first entry in `examples` (Swagger 2: the `examples` entry).
2. A value generated from the schema.

Generating a value from a schema, Zorvik uses, at every level, the schema's own `example`, the first of `examples`, `default`, `const` or the first `enum` value. Where there is none, it builds one:

- **Objects** get every property. Read-only properties are included (they appear in responses); write-only ones are left out.
- **Arrays** get `minItems` items (at least 1, at most 100).
- `allOf` parts are merged; for `oneOf` and `anyOf` the first option that gives a value is used.
- **Strings** follow their `format`: `date-time` gives `2024-01-01T00:00:00Z`, `date` `2024-01-01`, `uuid` `3fa85f64-5717-4562-b3fc-2c963f66afa6`, `email` `user@example.com`, `uri` `https://example.com`, `ipv4` `203.0.113.10`, `ipv6` `2001:db8::1`, `byte` `U29tZSBkYXRh`, and so on. Without a format, the property name gives a realistic value (a `name`, `city` or `id` looks like one); otherwise `example`.
- **Numbers** respect their limits; **booleans** are `true`.

Nesting stops after a few levels, and recursive schemas (a `Node` with `children: [Node]`) stop where they repeat, so every body stays a reasonable size. A document whose `$ref`s would expand to more than 256 MB of examples is cut short, with a warning.

For example, this response:

```yaml
responses:
  "200":
    content:
      application/json:
        schema:
          type: array
          items:
            type: object
            properties:
              id: {type: integer, readOnly: true}
              name: {type: string, example: Tom}
              password: {type: string, writeOnly: true}
```

becomes a route answering `200` with `Content-Type: application/json` and:

```json
[{"id": 1, "name": "Tom"}]
```

### Things to know

- The mock doesn't check requests against the document: any request that matches a route's method and path gets its answer. To check real responses against the document, see [OpenAPI contract checks](../../testing/openapi-contract-checks/).
- Each operation gets one answer (its first success response). To answer errors too, add routes with [conditions](../mock-api/#conditions) above the generated ones.
- Because the base path is left out, an API documented with `servers: [{url: https://api.example.com/v1}]` is mocked at `/pets`, not `/v1/pets`. Point your client at the mock without `/v1`, or add the prefix to the route paths.
- Security schemes are not enforced.

## From a folder of requests

In the **Collection** sidebar:

- Open a folder's menu and choose **Mock this folder…**, or
- open the **New** menu (**+**) of the collection and choose **Mock the collection…**.

Confirm the name (it suggests `<folder> mock`) and click **Create mock**. The mock is saved and opened, not started, on the first free port from 3000.

What you get:

- **One route per HTTP request**, in sidebar order, including the requests in subfolders. Other kinds (WebSocket, gRPC, …) and files that can't be read are skipped. When two requests have the same method and path, the first one wins.
- **Method**: the request's, in capitals (`GET` when empty).
- **Name**: the request's name (or its file name).
- **Answer**: the request's [saved examples](../../requests/responses/#examples), when it has some (below). Otherwise `200` with `Content-Type: application/json` and the body `{}`, ready for you to fill in. `HEAD` and `OPTIONS` routes get no body and no headers.
- **Path**: made from the request URL. The scheme, the host and a leading variable (`{{baseUrl}}`) are dropped, and so are the query and the fragment. A segment containing a variable (`{{id}}`) or an OpenAPI-style `{id}` becomes a parameter; `:param` segments stay.

| Request URL | Route path |
|---|---|
| `https://api.test/users/{{id}}?x=1` | `/users/:id` |
| `{{baseUrl}}/pets/:petId` | `/pets/:petId` |
| `{{ baseUrl }}/a/{{$uuid}}/b#frag` | `/a/:uuid/b` |
| `{{base}}/files/{name}/user-{{id}}` | `/files/:name/:id` |
| `localhost:3000/api/v1/` | `/api/v1` |
| `{{host}}:{{port}}/x` | `/x` |
| `{{baseUrl}}` | `/` |

When the folder has no HTTP requests, you get **There are no HTTP requests here to mock**.

### Requests with examples

A request with saved examples becomes one route per example, answering with the example's status, headers and body (framing headers such as `Content-Length` are left for the mock to set):

- An example saved while the URL had query parameters gets them as **Match query**, so it answers only requests that carry them. A `{{variable}}` value matches any value. Examples with the same query are used once.
- Then one route without conditions answers everything else: the first example saved without query parameters (or the first example, when they all have some).
- Route names are `<request> · <example>`, for example `Get pet · Not found`.

So a request `GET {{baseUrl}}/pets/:id` with the examples *Found* (200) and *Not found* (saved from `…/pets/999?include=owner`, 404) mocks as:

| Route | Match query | Answers |
|---|---|---|
| `Get pet · Not found` | `include=owner` | 404 |
| `Get pet · Found` | | 200 |

## From a response ("Mock this response")

After you send an HTTP request, the response header has a **Mock** button. It adds a route that answers like this response did.

In **Mock this response**:

- **Route**: the method is the request's; edit the path (it is made from the URL as above; use `:name` for segments that change, for example `/users/:id`). The status is the response's.
- **Add to**: one of your mock APIs (listed with their ports), or **New mock API…** with a name.
- Click **Add route** (or **Create mock**).

What is copied:

| Part | Copied |
|---|---|
| Status | Yes. |
| Headers | All, except connection-level ones (`Connection`, `Keep-Alive`, `Proxy-Connection`, `Transfer-Encoding`, `Upgrade`, `TE`, `Trailer`), `Content-Length` and `Content-Encoding` (the mock sets its own), `Date`, `Alt-Svc`, `Strict-Transport-Security`, `Report-To` and `NEL` (they would stick to localhost), and `Set-Cookie`. |
| Body | Text bodies. When the response was too large to show in full, only the part shown is used. Binary bodies can't be mocked yet (the route gets an empty body). A `HEAD` request's route has no body. |
| Name | The request's name. |

Notes in the dialog tell you when `Set-Cookie` headers were left out (they often hold a session; add them to the route if the mock needs them), when the body was cut, or when it was binary.

Adding a route to a running mock applies at once, and open tabs of that mock show the new route. A mock that was allowed to start with the workspace stays allowed (see [Running servers](../running-servers/#start-with-workspace)).

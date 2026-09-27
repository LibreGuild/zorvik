---
id: mock-templates
title: Dynamic answers with templates
summary: Templates fill a mock's answer from the request, so one route can answer /users/1, /users/42 and every user in between.
minutes: 6
lab:
  title: A directory that knows everyone
  goal: Make one mock answer for any user id, echo a search term and "create" users with fresh ids.
  minutes: 10
  steps:
    - text: |
        In **Servers**, create a new **Mock API (HTTP)** named **Directory API** and press **Start**.
      hints:
        - You did this in the last lesson; any free port is fine.
        - Servers → ＋ → Mock API (HTTP), name it, then press Start at the top right of its tab.
        - Servers → ＋ → Mock API (HTTP) → Directory API → Create → Start. If the port is in use, change Port and start again.
      check:
        saved:
          server: { name: Directory API, running: true }
      solution:
        - call:
            method: server.create
            params:
              server: &directory
                name: Directory API
                kind: http
                port: 0
                http:
                  routes:
                    - method: GET
                      path: /hello
                      status: 200
                      headers: [{ key: Content-Type, value: application/json }]
                      body: "{\n  \"message\": \"Hello from Zorvik\"\n}"
        - call:
            method: server.save
            params: { id: Directory API, server: *directory }
        - call:
            method: server.start
            params: { id: Directory API, server: *directory }
    - text: |
        Press **Add route** and make it `GET /users/:id` with this **Response body**:

        ```json
        {"id": "{{request.params.id}}", "name": "User {{request.params.id}}"}
        ```

        The lab asks for `/users/42` and expects to see `42` in the answer.
      hints:
        - The `:id` part of the path matches any value, and the template puts that value into the answer.
        - Add route, set the path to /users/:id, keep status 200 and the Content-Type header, and paste the body. The running mock updates as you type.
        - "Path /users/:id, body {\"id\": \"{{request.params.id}}\", \"name\": \"User {{request.params.id}}\"}. Save with ⌘S."
      check:
        probe:
          kind: http
          name: Directory API
          path: /users/42
          expect:
            status: 200
            json: { id: "42" }
      solution:
        - call:
            method: mock.addRoute
            params:
              serverId: Directory API
              route:
                method: GET
                path: /users/:id
                status: 200
                headers: [{ key: Content-Type, value: application/json }]
                body: "{\"id\": \"{{request.params.id}}\", \"name\": \"User {{request.params.id}}\"}"
    - text: |
        Now a search. Add a route `GET /search` that repeats the search term from the query string:

        ```json
        {"query": "{{request.query.q}}", "results": []}
        ```

        The lab searches for `/search?q=zorvik`.
      hints:
        - The query string is the part after the `?`. Its parameters are not part of the route's path.
        - Add route with path /search (no ?q in the path) and use {{request.query.q}} in the body.
        - "Path /search, status 200, body {\"query\": \"{{request.query.q}}\", \"results\": []}."
      check:
        probe:
          kind: http
          name: Directory API
          path: /search?q=zorvik
          expect:
            status: 200
            json: { query: zorvik }
      solution:
        - call:
            method: mock.addRoute
            params:
              serverId: Directory API
              route:
                method: GET
                path: /search
                status: 200
                headers: [{ key: Content-Type, value: application/json }]
                body: "{\"query\": \"{{request.query.q}}\", \"results\": []}"
    - text: |
        Last one: creating a user. Add a route `POST /users` with status **201** and this body, which gives every new user a fresh id and repeats what was sent:

        ```json
        {"id": "{{$uuid}}", "created": {{request.body}}}
        ```

        The lab sends `{"name": "Ada"}`.
      hints:
        - 201 Created is the usual answer to a POST that makes something new. `{{$uuid}}` is a new random id every time.
        - Add route, pick POST in the method list, path /users, status 201. `{{request.body}}` has no quotes around it, because the request body is already JSON.
        - "Method POST, path /users, status 201, body {\"id\": \"{{$uuid}}\", \"created\": {{request.body}}}."
      check:
        probe:
          kind: http
          name: Directory API
          method: POST
          path: /users
          headers: { Content-Type: application/json }
          body: '{"name": "Ada"}'
          expect:
            status: 201
            json:
              id: "re:^[0-9a-fA-F-]{36}$"
              created: { name: Ada }
      solution:
        - call:
            method: mock.addRoute
            params:
              serverId: Directory API
              route:
                method: POST
                path: /users
                status: 201
                headers: [{ key: Content-Type, value: application/json }]
                body: "{\"id\": \"{{$uuid}}\", \"created\": {{request.body}}}"
quiz:
  - question: A route has the path `/orders/:orderId`. Which placeholder puts `981` into the answer for `GET /orders/981`?
    options:
      - "`{{request.params.orderId}}`"
      - "`{{request.query.orderId}}`"
      - "`{{orderId}}`"
    answer: 0
    explain: Parts of the path written `:name` are route parameters, read with `{{request.params.name}}`. The query string is for values after the `?`.
  - question: Two routes both match `GET /users/0`, first `GET /users/:id`, then `GET /users/0`. Which one answers?
    options:
      - "`GET /users/0`, because it is more exact"
      - Both answers are merged
      - "`GET /users/:id`, because the first enabled route that matches answers"
    answer: 2
    explain: Order matters. Put special cases like `/users/0` above the general `/users/:id` route (drag to reorder).
  - question: Why answer a POST with `{{$uuid}}` instead of a fixed id like `"1"`?
    options:
      - UUIDs are shorter
      - Every created item gets a different id, like a real server would give it
      - Clients refuse fixed ids
    answer: 1
    explain: If every "new" order is order 1, bugs that mix up items stay hidden. A fresh id per call behaves like the real thing.
---

A route with a fixed answer is fine for a menu. But a user profile screen asks for `/users/1`, `/users/2`, `/users/42`… You don't want a thousand routes. You want **one** route whose answer changes with the request. That's what **templates** do.

## Placeholders

A template is an answer with blanks in double curly braces. When a request arrives, the mock fills in each blank from that request.

| Placeholder | Filled with | For `GET /users/42?lang=nl` |
|---|---|---|
| `{{request.params.id}}` | the `:id` part of the route's path | `42` |
| `{{request.query.lang}}` | a query parameter | `nl` |
| `{{request.headers.Accept}}` | a request header | whatever the client sent |
| `{{request.body}}` | the whole request body | (empty for this GET) |
| `{{request.method}}`, `{{request.path}}` | the method and path | `GET`, `/users/42` |
| `{{$uuid}}` | a new random id every time | `3f2b9c1e-…` |

Placeholders work in the response headers and the body. Environment variables such as `{{baseUrl}}` work too.

```sequence
participants: App, Mock
App -> Mock: GET /users/42
Note over Mock: route /users/:id, so id is 42
Mock --> App: 200 {"id": "42", "name": "User 42"}
```

> [!note] Think of it like…
> A form letter: "Dear {{name}}, your order {{number}} has shipped." You write the letter once; the blanks are filled in for each customer. A template route is a form letter for requests.

## Quotes or no quotes?

Placeholders are replaced with plain text. Inside JSON, text needs quotes, so write `"id": "{{request.params.id}}"`. But `{{request.body}}` is usually JSON already, so it goes in *without* quotes: `"created": {{request.body}}`.

## Matching more precisely

Sometimes the same path should answer differently. Open **Match only when** on a route to add conditions: a query parameter, a header, or text the body must contain. For example, a `GET /users/:id` route that only matches when an `Authorization` header is present, followed by a general one that answers `401 Unauthorized`.

Remember the rule from the last lesson: **the first enabled route that matches answers**. Put special cases (like `/users/0` answering `404`) above the general `/users/:id` route. Drag routes in the list to reorder them.

> [!tip] Test your templates in Zorvik
> Send a request to your mock from a request tab and look at the response. If a placeholder comes back empty, check its spelling: `{{request.params.id}}` must match the `:id` in the path exactly.

**You'll use this when…** a screen shows "any user" or "any order", when a create form needs a realistic new id back, or when you want a search box to show that it really sent the term the user typed.

---
id: path-params
title: Path parameters
summary: Many APIs name one specific thing inside the path, like `/users/42`; a `:id` placeholder lets one request, or one mock route, fit them all.
minutes: 5
lab:
  title: Track an order
  goal: Fill in path variables to look up a user and one of their orders.
  minutes: 6
  servers:
    api:
      name: Users API
      kind: http
      http:
        routes:
          - name: One user
            method: GET
            path: /users/:id
            headers:
              - { key: Content-Type, value: application/json }
            body: '{"id": "{{request.params.id}}", "name": "Customer {{request.params.id}}", "orders": "/users/{{request.params.id}}/orders"}'
          - name: One order
            method: GET
            path: /users/:userId/orders/:orderId
            headers:
              - { key: Content-Type, value: application/json }
            body: '{"userId": "{{request.params.userId}}", "orderId": "{{request.params.orderId}}", "status": "shipped", "tracking": "{{secret.tracking}}"}'
  steps:
    - text: |
        Open a new request with the URL `{{api}}/users/:id`. A **Path variables** table appears in the **Params** tab: set `id` to `42`, then send.
      hints:
        - A path segment that starts with `:` is a placeholder. Zorvik fills it in from the Path variables table when it sends.
        - Type the URL with `:id` in it, open **Params** and look under **Path variables**. Type `42` next to `id`.
        - "URL {{api}}/users/:id, path variable id = 42, then Send. The server receives /users/42."
      check:
        all:
          - request: { server: api, method: GET, path: /users/42 }
          - send: { request: { url: "*/users/:id" } }
      solution:
        - send: { method: GET, url: "{{api}}/users/:id", pathParams: [{ key: id, value: "42" }] }
    - text: |
        Now ask for order `7` of user `42` with two path variables: `{{api}}/users/:userId/orders/:orderId`.
      hints:
        - Each `:name` segment gets its own row under **Path variables**.
        - Change the URL to `{{api}}/users/:userId/orders/:orderId`, then fill `userId` = `42` and `orderId` = `7`.
        - "The server should receive GET /users/42/orders/7. Press Send."
      check:
        all:
          - request: { server: api, method: GET, path: /users/42/orders/7 }
          - send: { request: { url: "*/users/:*/orders/:*" } }
      solution:
        - send:
            method: GET
            url: "{{api}}/users/:userId/orders/:orderId"
            pathParams: [{ key: userId, value: "42" }, { key: orderId, value: "7" }]
    - text: The order has shipped. Type its `tracking` code.
      hints:
        - It's in the response body of your last request.
        - Copy the value after `"tracking":`, without the quotes.
      check:
        answer: "{{secret.tracking}}"
      solution:
        - answer: "{{secret.tracking}}"
quiz:
  - question: Which URL names user 42 with a path parameter?
    options:
      - /users?id=42
      - /users/42
      - /users#42
    answer: 1
    explain: The value sits in the path itself. `?id=42` is a query parameter, and anything after `#` never reaches the server.
  - question: A mock route has the path `/users/:id`. Which request does it NOT match?
    options:
      - GET /users/7
      - GET /users/ada
      - GET /users/7/orders
    answer: 2
    explain: "`:id` matches exactly one segment, whatever its value. `/users/7/orders` has one segment too many."
  - question: You set the path variable `id` to `a/b`. What does the server receive?
    options:
      - /users/a%2Fb, one segment
      - /users/a/b, two segments
      - Nothing, Zorvik refuses to send
    answer: 0
    explain: Path variable values are percent-encoded, so a `/` inside a value can't split it into two segments.
---

In the last lesson the *query* said which slice of a list you wanted. Many APIs also put an identity right in the **path**: `/users/42` is "user number 42", and `/users/42/orders/7` is "order 7 of user 42". Each piece that changes is a **path parameter**.

```anatomy
GET /users | a collection: all the users
GET /users/42 | one item in it, picked by its id
GET /users/42/orders | a collection that belongs to that user
GET /users/42/orders/7 | one order of that user
```

This style is common in REST APIs. REST is a popular way of designing HTTP APIs where each URL names a *thing* (a resource) and the method says what to do with it: `GET /users/42` reads the user, `DELETE /users/42` removes it.

> [!note] Think of it like…
> A street address. `/users/42/orders/7` reads like "Users Street, house 42, flat 7": each segment narrows it down until exactly one door is left.

## Path or query?

A rough rule that most APIs follow:

| Use a… | when the value… | Example |
|---|---|---|
| path parameter | *identifies* one thing | `/users/42` |
| query parameter | *filters, sorts or pages* a list | `/users?country=pt&page=2` |

## Path variables in Zorvik

Documentation usually writes paths with placeholders, like `/users/:id` or `/users/{id}`. In Zorvik you can keep the `:id` in the URL: the **Params** tab then shows a **Path variables** table with one row per placeholder, and Zorvik puts the values in when it sends.

```flow
/users/:id -[id = 42]-> /users/42
/users/:userId/orders/:orderId -[userId = 42, orderId = 7]-> /users/42/orders/7
```

Why not just type `42`? Because a saved request with `:id` is reusable: change one cell, or set it to a variable like `{{userId}}`, instead of editing the URL. Values are **percent-encoded**, so a space becomes `%20` and a `/` becomes `%2F` and can't split the segment in two.

## How the mock reads them

The lab's mock has a route with the path `/users/:id`. That route matches `/users/42`, `/users/ada` or any other single segment, and hands the value to the response through a template:

```json
{"id": "{{request.params.id}}", "name": "Customer {{request.params.id}}"}
```

Ask for `/users/42` and the mock answers `"id": "42"`. Open **Servers → Lab · Users API** to see the routes. Mock routes accept the `{id}` style too, the way OpenAPI documents write them, and a trailing `*` matches the rest of the path.

> [!tip] Case matters
> Paths are case-sensitive: `/Users/42` and `/users/42` can be different things, or one of them a 404.

**You'll use this when…** a bug report says "order 7 of customer 42 shows the wrong total". Open your saved `GET /users/:userId/orders/:orderId`, fill in two cells, and you're looking at the same data in seconds.

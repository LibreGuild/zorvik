---
id: chaining-requests
title: Chaining requests
summary: Save a value from one response, like a token or a new id, and use it in the next request.
minutes: 5
lab:
  title: Log in, then use the token
  goal: Save a login token and a new order's id with scripts, and use them in the requests that follow.
  minutes: 9
  servers:
    api:
      name: Account API
      kind: http
      http:
        routes:
          - method: POST
            path: /login
            headers:
              - key: Content-Type
                value: application/json
            body: '{"token": "{{secret.token}}", "expiresIn": 3600}'
          - method: GET
            path: /me
            matchHeaders:
              - key: Authorization
                value: "Bearer {{secret.token}}"
            headers:
              - key: Content-Type
                value: application/json
            body: '{"name": "Ada Lovelace", "plan": "pro"}'
          - method: GET
            path: /me
            status: 401
            headers:
              - key: Content-Type
                value: application/json
            body: '{"error": "Log in first: send a Bearer token"}'
          - method: POST
            path: /orders
            status: 201
            headers:
              - key: Content-Type
                value: application/json
            body: '{"id": "ord-{{secret.order}}", "status": "created"}'
          - method: GET
            path: /orders/:id
            headers:
              - key: Content-Type
                value: application/json
            body: '{"id": "{{request.params.id}}", "status": "paid", "total": 42.5}'
  steps:
    - text: |
        Send `POST {{api}}/login` (this practice server lets anyone in, no body needed). Before you send, give it a **Post-response** script that saves the token from the answer:

        ```js
        const body = pm.response.json();
        pm.environment.set("token", body.token);
        ```
      hints:
        - The script runs after the answer arrives, so it can read body.token and keep it in the active environment.
        - New request, method POST, URL {{api}}/login. Scripts tab → Post-response, paste the two lines, then Send.
        - "After sending, hover {{token}} anywhere, or open ⌘E: the Lab environment shows token under Set by scripts."
      check:
        all:
          - request: { server: api, method: POST, path: /login }
          - send: { url: "*/login", status: 200, request: { scripts: { postResponse: "*.set(*token*" } } }
      solution:
        - send:
            method: POST
            url: "{{api}}/login"
            scripts:
              postResponse: |
                const body = pm.response.json();
                pm.environment.set("token", body.token);
    - text: Now send `GET {{api}}/me`. In its **Auth** tab choose **Bearer token** and type `{{token}}` as the token. Who are you logged in as?
      hints:
        - Without the token the server answers 401. The variable carries the token from the login answer to this request.
        - New request, URL {{api}}/me, Auth tab → Type Bearer token → Token {{token}}. Then Send.
        - "Token field: {{token}} (the variable, not the text of the token). The answer is 200 with Ada Lovelace's account."
      check:
        all:
          - request: { server: api, method: GET, path: /me, status: 200, headers: { authorization: "Bearer {{secret.token}}" } }
          - any:
              - send: { url: "*/me", request: { auth: { token: "*token*" } } }
              - send: { url: "*/me", request: { headers: [{ value: "*token*" }] } }
      solution:
        - send: { method: GET, url: "{{api}}/me", auth: { type: bearer, token: "{{token}}" } }
    - text: |
        One more chain. Send `POST {{api}}/orders` with a Post-response script that saves the new order's id as `orderId`. Then send `GET {{api}}/orders/{{orderId}}` to read that order back.
      hints:
        - It's the same pattern as the token. The answer of POST /orders has an id field.
        - "Post-response of POST /orders: pm.environment.set(\"orderId\", pm.response.json().id);"
        - "Then a new request: GET {{api}}/orders/{{orderId}}. The answer shows the same id, with status paid."
      check:
        all:
          - request: { server: api, method: GET, path: "/orders/ord-{{secret.order}}" }
          - send: { method: GET, request: { url: "*/orders/*orderId*" } }
      solution:
        - send:
            method: POST
            url: "{{api}}/orders"
            scripts:
              postResponse: pm.environment.set("orderId", pm.response.json().id);
        - send: { method: GET, url: "{{api}}/orders/{{orderId}}" }
quiz:
  - question: Where does `pm.environment.set("token", …)` keep the token?
    options:
      - In the environment file, so it's committed to Git
      - With the active environment, on this computer only, listed under Set by scripts
      - Nowhere, it's gone after the script ends
    answer: 1
    explain: Values set by scripts stay on your computer and win over the file's value. They are never written into your workspace files.
  - question: You send `GET /me` before you have ever sent the login request. What happens?
    options:
      - The server gets `Bearer {{token}}` as it is, and answers 401
      - Zorvik logs in for you
      - The request waits until the token exists
    answer: 0
    explain: The variable has no value yet. Order matters in a chain; in a collection run you put the login request first.
  - question: You need a value only while one send or one collection run lasts. Which one do you use?
    options:
      - "`pm.globals.set`"
      - "`pm.environment.set`"
      - "`pm.variables.set`"
    answer: 2
    explain: "`pm.variables` is the most short-lived and the strongest: it lasts for one send or one run, and nothing is kept afterwards."
---

Real APIs are conversations, not single questions. You **log in** and get a token, then use the token for everything else. You **create** an order and get its id, then look up, pay for or cancel that order by its id. Each request needs something from the answer before it. Connecting them is called **chaining**.

## The recipe

1. In the first request's **post-response script**, read the value from the response.
2. Save it in a variable with `pm.environment.set("name", value)`.
3. In the next request, use it like any variable: `{{name}}`.

```sequence
participants: Zorvik, Server
Zorvik -> Server: POST /login
Server --> Zorvik: {"token": "eyJhbGci…"}
Note over Zorvik: pm.environment.set("token", …)
Zorvik -> Server: GET /me, Authorization: Bearer {{token}}
Server --> Zorvik: 200 {"name": "Ada Lovelace"}
```

The next time you log in, the script saves the new token, and every request that says `{{token}}` uses it. No copying and pasting, and no token that expired an hour ago hiding in a header.

> [!note] Think of it like…
> A coat check. You hand over your coat and get a ticket with a number. You don't memorise the number; you keep the ticket and show it when you come back. The script keeps the ticket for you.

## Where saved values go

Scripts can save values at different levels, like the variables you already know:

| Script call | Lasts | Kept where |
|---|---|---|
| `pm.variables.set` | one send, or one collection run | nowhere, gone afterwards |
| `pm.environment.set` | until you change it | with the active environment, on this computer |
| `pm.collectionVariables.set` | until you change it | with the workspace variables, on this computer |
| `pm.globals.set` | until you change it | every workspace, on this computer |

Values that scripts keep show up in **Environments & variables** (**⌘/Ctrl + E**) under **Set by scripts**. They win over the value in the file, but are never written into it, so a token from your login never ends up in Git. **Clear all** there removes them.

> [!tip] Reading values in scripts
> Scripts can read variables too: `pm.environment.get("token")`, or `pm.variables.get("token")`, which looks through every level in order, just like `{{token}}` does.

## Keep chains honest

A chain has an order: the login must run before `/me`. In the next lesson, the **collection runner** sends a whole folder in order, so a chain works from the first request to the last. Add a test to each step too, like `pm.test("Logged in", …)`, so a broken link shows up as a failed test, not as a confusing 401 three requests later.

**You'll use this when…** your API hands out short-lived tokens. You log in once, every request in the collection picks up the fresh token, and an expired token is never again the reason your afternoon disappears.

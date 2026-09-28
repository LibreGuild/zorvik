---
id: requests-and-responses
title: Requests and responses up close
summary: Every HTTP message is a start line, some headers, a blank line and an optional body.
minutes: 6
lab:
  title: Take a response apart
  goal: Send a request, find a value hidden in the response headers, then add a query parameter.
  minutes: 6
  servers:
    api:
      name: Cafe API
      kind: http
      http:
        routes:
          - method: GET
            path: /orders/:id
            headers:
              - key: Content-Type
                value: application/json
              - key: X-Ticket
                value: "{{secret.ticket}}"
            body: '{"id": "{{request.params.id}}", "item": "cappuccino", "size": "large", "status": "baking"}'
  steps:
    - text: Send `GET {{api}}/orders/42` and look at the response.
      hints:
        - The Lab environment is active, so `{{api}}` holds the Cafe API's address.
        - "Press ⌘N (Ctrl+N on Windows), type `{{api}}/orders/42` and press Send."
      check:
        request: { server: api, method: GET, path: /orders/42 }
      solution:
        - send: { method: GET, url: "{{api}}/orders/42" }
    - text: The server added a custom response header called `X-Ticket`. Open the response's **Headers** tab and type its value.
      hints:
        - The response has its own tabs (Body, Headers, Cookies, Timing, Info), separate from the request's tabs. No response on screen? Open the request from History.
        - Click Headers in the response and find the row named X-Ticket.
        - Copy the value next to X-Ticket (a word and a number).
      check:
        answer: "{{secret.ticket}}"
      solution:
        - answer: "{{secret.ticket}}"
    - text: |
        Add a query parameter `fields` with the value `status` and send again. Use the request's **Params** tab, or type `?fields=status` at the end of the URL: they stay in sync.
      hints:
        - A query string starts with ? after the path, then name=value pairs.
        - In the Params tab, add a row with the key fields and the value status. The URL updates by itself.
        - "The URL becomes `{{api}}/orders/42?fields=status`. Press Send."
      check:
        request: { server: api, method: GET, path: /orders/42, query: { fields: status } }
      solution:
        - send: { method: GET, url: "{{api}}/orders/42?fields=status" }
quiz:
  - question: In `GET /users?page=2 HTTP/1.1`, what is `page=2`?
    options:
      - The method
      - A header
      - The query string, an extra option for the server
      - The body
    answer: 2
    explain: Everything after the ? is the query string, name=value pairs joined with &.
  - question: What separates the headers from the body?
    options:
      - An empty line
      - A comma
      - The word BODY
      - Nothing, the body comes first
    answer: 0
    explain: Headers come one per line, and the first empty line says "the body starts here".
  - question: Which header tells the receiver what format the body is in?
    options:
      - Accept
      - Content-Type
      - Host
      - User-Agent
    answer: 1
    explain: Content-Type describes the body that's being sent. Accept says which formats you'd like back.
---

**HTTP** (HyperText Transfer Protocol) is the language web browsers and almost all APIs speak. It's surprisingly readable: in HTTP/1.1 every message is plain lines of text. Newer versions (HTTP/2 and HTTP/3) pack the same parts into a more compact form, but the parts are the same. Let's take them apart.

## A request

```anatomy
GET /orders/42?fields=status HTTP/1.1 | request line: the method, the target and the HTTP version
Host: api.cafe.test | a header: which site you mean (one server can host many)
Accept: application/json | a header: the format you'd like back
(empty line) | the end of the headers
{"note": "extra hot"} | the body: optional data, sent with POST, PUT and PATCH
```

The **target** has two parts:

- the **path**, `/orders/42`: which thing on the server you mean;
- the **query string**, `?fields=status`: extra options as `name=value` pairs, joined with `&`, like `?page=2&size=20`.

## A response

```anatomy
HTTP/1.1 200 OK | status line: the version, the status code and a short reason
Content-Type: application/json | a header: what the body is
X-Ticket: T-1234 | a custom header the server made up (many start with X-)
(empty line) | the end of the headers
{"id": 42, "status": "baking"} | the body: the answer itself
```

The two travel as a pair:

```sequence
participants: Client, Server
Client -> Server: GET /orders/42 plus headers
Note over Server: looks up order 42
Server --> Client: 200 OK plus headers and a JSON body
```

> [!note] Think of it like…
> A parcel. The start line is the address label, the headers are the stickers on the box ("fragile", "this side up", "contains JSON"), and the body is what's inside. The sorting center reads the label and the stickers without ever opening the box.

## Every request stands alone

HTTP is **stateless**: the server doesn't remember your previous request. Each request has to carry everything the server needs to answer it, which is why things like your login token travel in a header on *every* request, not just the first one. The server never speaks first, either: no request, no response.

## Where to find each part in Zorvik

| Part | In the request | In the response |
|---|---|---|
| Method and URL | the URL bar | the **Info** tab |
| Query string | the **Params** tab (in sync with the URL) | |
| Status | | the colored badge at the top of the response |
| Headers | the **Headers** tab | the **Headers** tab |
| Body | the **Body** tab | the **Body** tab (**Pretty** or **Raw**) |

Zorvik also adds a few headers you didn't type, such as `User-Agent` (which program is asking) and `Host`. The response's **Info** tab shows the request exactly as it went out, in its "Request sent" section.

> [!tip] Headers have no order and no case
> `Content-Type` and `content-type` are the same header, and the order of headers doesn't matter. Values, though, are usually case-sensitive.

**You'll use this when…** an API "ignores" what you sent. Nine times out of ten, the answer is in the details: a missing header, a typo in the query string, or a body the server didn't expect.

---
id: http-headers
title: Headers
summary: Headers carry everything about a message except the content itself, from its format to who's asking.
minutes: 6
lab:
  title: The secret handshake
  goal: Get past an API that wants a key in a header, then ask it for a different format.
  minutes: 7
  vars:
    apiKey: "{{secret.key}}"
  servers:
    api:
      name: Weather API
      kind: http
      http:
        routes:
          - name: Forecast as text
            method: GET
            path: /forecast
            matchHeaders:
              - key: X-Api-Key
                value: "{{secret.key}}"
              - key: Accept
                value: text/plain
            headers:
              - key: Content-Type
                value: text/plain; charset=utf-8
            body: Sunny, 24 °C, with a light breeze from the sea.
          - name: Forecast as JSON
            method: GET
            path: /forecast
            matchHeaders:
              - key: X-Api-Key
                value: "{{secret.key}}"
            headers:
              - key: Content-Type
                value: application/json
            body: '{"sky": "sunny", "celsius": 24, "wind": "light breeze"}'
          - name: Missing key
            method: GET
            path: /forecast
            status: 401
            headers:
              - key: Content-Type
                value: application/json
            body: '{"error": "Missing or wrong X-Api-Key header. Your key is the apiKey variable of the Lab environment."}'
  steps:
    - text: Send `GET {{api}}/forecast`. The API refuses. Read the response body to find out why.
      hints:
        - Look at the status badge and the body of the response.
        - "Send `GET {{api}}/forecast`: 401 Unauthorized, because the request has no X-Api-Key header."
      check:
        request: { server: api, method: GET, path: /forecast, status: 401 }
      solution:
        - send: { method: GET, url: "{{api}}/forecast" }
    - text: |
        In the request's **Headers** tab, add a header named `X-Api-Key` with the value `{{apiKey}}`, then send again.
      hints:
        - Headers are name and value pairs. The value can be a variable from the Lab environment.
        - Open the Headers tab of the request (not of the response) and fill in an empty row.
        - "Name `X-Api-Key`, value `{{apiKey}}`, then Send. The answer is 200 with the forecast as JSON."
      check:
        request: { server: api, method: GET, path: /forecast, status: 200, headers: { x-api-key: "{{secret.key}}" } }
      solution:
        - send: { method: GET, url: "{{api}}/forecast", headers: [{ key: X-Api-Key, value: "{{apiKey}}" }] }
    - text: |
        Ask for plain text instead of JSON: keep the key, add a second header `Accept` with the value `text/plain`, and send.
      hints:
        - Accept tells the server which format you'd like back.
        - Add a row below X-Api-Key with the name Accept.
        - "Name `Accept`, value `text/plain`, then Send. The body is now one sentence of text, and the response's Content-Type header says text/plain."
      check:
        request: { server: api, method: GET, path: /forecast, status: 200, headers: { x-api-key: "{{secret.key}}", accept: "text/plain" } }
      solution:
        - send: { method: GET, url: "{{api}}/forecast", headers: [{ key: X-Api-Key, value: "{{apiKey}}" }, { key: Accept, value: text/plain }] }
quiz:
  - question: Are `Content-Type` and `content-type` the same header?
    options:
      - Yes, header names ignore case
      - No, header names are case-sensitive
      - Only in HTTP/2
      - Only for requests, not for responses
    answer: 0
    explain: Header names aren't case-sensitive (HTTP/2 even writes them all in lower case). Their values usually are.
  - question: You want JSON back, but the API sends XML. Which header should you set in your request?
    options:
      - Content-Type
      - Host
      - Accept
      - Cache-Control
    answer: 2
    explain: Accept says which formats you'd like back. Content-Type describes the body you're sending.
  - question: Why put an API key in a header rather than in the URL?
    options:
      - Headers make the request faster
      - A URL can't be longer than 100 characters
      - Servers can't read the query string
      - URLs end up in logs, browser history and screenshots, where a key could leak
    answer: 3
    explain: URLs get copied and logged everywhere. Headers are less exposed, though HTTPS is still what really protects them on the way.
---

Headers are the `Name: value` lines between the start line and the body. They're **metadata**: facts *about* the message rather than the message itself. Header names ignore case, so `Content-Type` and `content-type` are the same header; their values usually don't.

## Headers you'll see all the time

| Header | Sent by | Says |
|---|---|---|
| `Host` | client | which site you want (one server can host many) |
| `User-Agent` | client | which program is asking |
| `Accept` | client | which formats you'd like back, like `application/json` |
| `Content-Type` | both | what the body is: `application/json`, `text/html`, … |
| `Content-Length` | both | how many bytes the body has |
| `Authorization` | client | your credentials, like `Bearer eyJ…` |
| `Cookie`, `Set-Cookie` | client, server | small values a site asks the browser to keep and send back |
| `Cache-Control` | both | whether, and for how long, the answer may be cached |
| `Location` | server | where to go next (with 3xx redirects and 201 Created) |

APIs also invent their own **custom headers**. Many start with `X-`, like `X-Api-Key` or `X-Request-Id`, although newer standards dropped that habit.

```anatomy
Accept: application/json | "I'd like JSON back, please"
Authorization: Bearer eyJhbGciOi… | "here's proof of who I am"
X-Api-Key: 6f1c2a… | a custom header this particular API invented
```

## Asking for a format

One URL can offer the same data in several formats. The client says what it prefers with `Accept`, and the server says what it actually sent with `Content-Type`. This is called **content negotiation**.

```sequence
participants: Client, Weather API
Client -> Weather API: GET /forecast, Accept text/plain
Weather API --> Client: 200, Content-Type text/plain, "Sunny, 24 °C"
Client -> Weather API: GET /forecast, Accept application/json
Weather API --> Client: 200, Content-Type application/json, {"sky" ...}
```

> [!note] Think of it like…
> The notes written on an envelope: "urgent", "reply in English", "do not bend". The letter inside is the body; the notes on the envelope tell everyone who handles it what to do.

## Keys in headers

Many APIs want a key on every request, usually in `Authorization` or a custom header like `X-Api-Key`. Without it, they answer **401 Unauthorized**. A header is a better place for a key than the URL: URLs end up in logs, browser history and screenshots.

> [!tip] In Zorvik
> Add request headers in the request's **Headers** tab, one row per header. Values can use variables, like `{{apiKey}}`, so the real key lives in an environment instead of being copied into every request. The response's own **Headers** tab shows what came back.

**You'll use this when…** an API answers 401 even though "the key is right" (is the header name right?), sends XML when you wanted JSON (did you set `Accept`?), or rejects your JSON (is `Content-Type` missing?).

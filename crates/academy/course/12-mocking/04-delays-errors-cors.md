---
id: mock-failures
title: Delays, errors and CORS
summary: A good mock also has bad days on purpose, so you can see how your app copes with slow answers, errors and browser rules.
minutes: 6
lab:
  title: A checkout with bad days
  goal: Build a checkout mock that a browser page may call, that answers slowly, and that fails on purpose.
  minutes: 10
  steps:
    - text: |
        The web team's checkout page runs in the browser at `http://localhost:5173`. Create a **Mock API (HTTP)** named **Checkout Mock**, switch on **Allow cross-origin requests (CORS)** under **Browser access**, and press **Start**.

        The lab then asks your mock, the way a browser does, whether that page may send it a `POST`.
      hints:
        - Browsers only let a page read answers from another address when the server allows it. That's what the CORS switch does.
        - Scroll down in the server tab, below the routes, to Browser access. The switch works before or after you start the server.
        - Servers → ＋ → Mock API (HTTP) → Checkout Mock → Create; switch on "Allow cross-origin requests (CORS)"; press Start.
      check:
        probe:
          kind: http
          name: Checkout Mock
          method: OPTIONS
          path: /payments
          headers:
            Origin: http://localhost:5173
            Access-Control-Request-Method: POST
          expect:
            status: 204
            headers: { access-control-allow-origin: "http://localhost:5173" }
      solution:
        - call:
            method: server.create
            params:
              server: &checkout
                name: Checkout Mock
                kind: http
                port: 0
                http:
                  cors: true
                  routes:
                    - method: GET
                      path: /hello
                      status: 200
                      headers: [{ key: Content-Type, value: application/json }]
                      body: "{\n  \"message\": \"Hello from Zorvik\"\n}"
        - call:
            method: server.save
            params: { id: Checkout Mock, server: *checkout }
        - call:
            method: server.start
            params: { id: Checkout Mock, server: *checkout }
    - text: |
        A slow cart. Add a route `GET /cart` answering `{"items": 2, "total": 18.5}`, set its **Delay (ms)** to **1500**, and save (**⌘/Ctrl + S**).

        Send a request to it yourself and watch the page's spinner moment: a second and a half of nothing.
      hints:
        - The delay is in the Behavior section of the route. The mock waits that long before it answers.
        - Add route, path /cart, paste the body, then Behavior → Delay (ms) → 1500. Save so the delay is kept in the file.
        - "Route GET /cart, status 200, body {\"items\": 2, \"total\": 18.5}, Delay (ms) 1500, then ⌘S."
      check:
        all:
          - probe:
              kind: http
              name: Checkout Mock
              path: /cart
              expect:
                status: 200
                json: { total: 18.5 }
          - saved:
              server:
                name: Checkout Mock
                http:
                  routes: [{ path: /cart, delayMs: ">=1000" }]
      solution:
        - call:
            method: mock.addRoute
            params:
              serverId: Checkout Mock
              route:
                method: GET
                path: /cart
                status: 200
                headers: [{ key: Content-Type, value: application/json }]
                body: '{"items": 2, "total": 18.5}'
                delayMs: 1500
    - text: |
        Payments are down for maintenance. Add a route `POST /payments` that answers **503** with the header `Retry-After: 30` and the body `{"error": "Payments are down for maintenance"}`.
      hints:
        - 503 Service Unavailable means "not now, try again later". Retry-After says how many seconds later.
        - Add route, choose POST as the method, set the path and type 503 in the status box. Add the header under Response headers.
        - "POST /payments, status 503, header Retry-After: 30, body {\"error\": \"Payments are down for maintenance\"}."
      check:
        probe:
          kind: http
          name: Checkout Mock
          method: POST
          path: /payments
          headers: { Content-Type: application/json }
          body: '{"amount": 18.5}'
          expect:
            status: 503
            headers: { retry-after: "30" }
      solution:
        - call:
            method: mock.addRoute
            params:
              serverId: Checkout Mock
              route:
                method: POST
                path: /payments
                status: 503
                headers:
                  - { key: Content-Type, value: application/json }
                  - { key: Retry-After, value: "30" }
                body: '{"error": "Payments are down for maintenance"}'
    - text: |
        Chaos time. Add a route `GET /stock` and set its **Fault** to **Error (500)**, **How often (%)** 100. Every stock check now fails with an error, whatever its body says.

        Afterwards, try 25%: one request in four fails, like a flaky server.
      hints:
        - A fault replaces the route's normal answer. Error (500) answers 500 Internal Server Error.
        - Add route with path /stock; in Behavior pick Fault → Error (500) and keep How often at 100.
        - "GET /stock, any body, Behavior → Fault: Error (500), How often (%): 100."
      check:
        probe:
          kind: http
          name: Checkout Mock
          path: /stock
          expect:
            status: 500
      solution:
        - call:
            method: mock.addRoute
            params:
              serverId: Checkout Mock
              route:
                method: GET
                path: /stock
                status: 200
                headers: [{ key: Content-Type, value: application/json }]
                body: '{"inStock": true}'
                fault: error
quiz:
  - question: Your request to the mock works in Zorvik, but the same call from a web page in the browser is blocked. What is the most likely cause?
    options:
      - The mock is too slow
      - The route's path is wrong
      - The mock doesn't allow cross-origin requests (CORS)
    answer: 2
    explain: Only browsers enforce CORS. Zorvik, phones and servers don't, which is why "works in Zorvik, fails in the browser" points straight at CORS.
  - question: Which answer tells a client "not now, try again in 30 seconds"?
    options:
      - 404 with the body "30"
      - 503 with the header Retry-After set to 30
      - 200 with a delay of 30 ms
    answer: 1
    explain: 503 Service Unavailable says the server can't answer right now; Retry-After says when to try again. Well-behaved apps wait instead of hammering the server.
  - question: You want to see what your app does when one request in ten fails at random. What do you set on the route?
    options:
      - Fault Error (500), How often (%) 10
      - A delay of 10 ms
      - Status 210
    answer: 0
    explain: A fault with a percentage fails some requests and answers the rest normally, just like a flaky real server.
---

Real servers have bad days. They get slow, they return errors, they drop connections. Your app must survive all of that, and the only way to be sure is to **practise the bad days on purpose**. A mock is the perfect place for it: it misbehaves exactly how and when you tell it to, and nobody else is affected.

> [!note] Think of it like…
> A fire drill. You set off the alarm on a calm Tuesday, on purpose, so that everyone knows what to do when there's a real fire. Delays and errors in a mock are fire drills for your app.

## Slow answers

Every route has a **Delay (ms)** under **Behavior**. The mock waits that many milliseconds (thousandths of a second) before it answers. A delay of `1500` shows you:

- whether the app shows a spinner, or just freezes;
- whether a user can tap **Pay** twice while waiting;
- whether the app gives up (times out) too early.

## Errors on purpose

For errors you *plan*, set the route's status: `404` for "no such order", `401` for "please log in", `503` for "down for maintenance". Add headers such as `Retry-After: 30` exactly as the real API would send them.

For chaos, use the route's **Fault**:

| Fault | What the app sees |
|---|---|
| **Error (500)** | `500 Internal Server Error` instead of the normal answer |
| **Drop the connection** | the connection closes with no answer at all |
| **Never answer** | nothing, until the app gives up waiting |

**How often (%)** makes it random: at `25`, one request in four fails and the others answer normally. That's how flaky networks feel.

## CORS: browsers ask first

A web page may only read answers from its own **origin**: the scheme, host and port it was loaded from, such as `http://localhost:5173`. Your mock at `http://127.0.0.1:3000` is a different origin. Before sending certain requests, the browser asks the server with a **preflight**, an `OPTIONS` request, and only continues if the server answers with `Access-Control-Allow-*` headers. This safety rule is called **CORS** (Cross-Origin Resource Sharing).

```sequence
participants: Browser page, Mock
Browser page -> Mock: OPTIONS /payments (Origin: http://localhost:5173)
Mock --> Browser page: 204 Access-Control-Allow-Origin: http://localhost:5173
Browser page -> Mock: POST /payments
Mock --> Browser page: 503 Retry-After: 30
```

Switch on **Allow cross-origin requests (CORS)** under **Browser access**, and the mock answers preflights and adds those headers to every answer.

> [!warning] Only browsers check CORS
> Zorvik, mobile apps and servers ignore CORS completely. So when a call works in Zorvik but fails in the browser, CORS is the usual suspect.

**You'll use this when…** a designer asks what the loading state looks like, QA wants to test the "payment failed" screen, or a front-end developer says "it works in Zorvik but not in Chrome!"

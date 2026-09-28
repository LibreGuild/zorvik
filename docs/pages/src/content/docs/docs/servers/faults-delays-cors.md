---
title: Faults, delays & CORS
description: Make mock routes slow or unreliable on purpose, delay rule replies, and let browsers call your mocks from other origins.
sidebar:
  order: 3
---

Real services are slow sometimes, fail sometimes, and live on another origin than your web app. Mock APIs can do all three on purpose, so you can test timeouts, retries, error handling and CORS before you meet them in production.

## Delays

### Mock API routes

Set **Delay (ms)** in a route's **Behavior** section. The route waits this long before it answers; a clock icon marks the route in the list.

- The delay is at most 24 hours; longer values are cut to that.
- A client that gives up during the delay ends the wait: nothing is sent, and the traffic log shows the exchange as **client left**.
- The delay also comes before the **Error (500)** and **Drop the connection** faults, so you can make a route fail slowly.
- Each request waits on its own: ten requests with a 2-second delay all answer after about 2 seconds.

### Reply rules

WebSocket, TCP and UDP [reply rules](../websocket-and-sse-servers/#reply-rules) have a **Delay ms** column. The reply is sent that long after the message arrived. On UDP, at most 1,000 delayed replies may wait at once; more are dropped and logged as **Too many delayed replies waiting: this one was dropped**.

### Event streams

An [event stream server](../websocket-and-sse-servers/#event-stream-sse-server) sends its events with the pause set in **Interval (ms)**.

## Faults

A route's **Fault** replaces its normal answer:

| Fault | In the file | What the client sees | Traffic log |
|---|---|---|---|
| **None** | `none` | The normal answer. | |
| **Error (500)** | `error` | `500 Internal Server Error` with `Content-Type: application/json` and `{"error": "Injected fault on route <route>"}`, where `<route>` is the route's name, or its method and path. | status `500`, note **error** |
| **Drop the connection** | `reset` | The connection is closed without an answer (after the delay). | **reset** |
| **Never answer** | `hang` | Nothing. The request stays open until the client gives up or the server stops. The delay doesn't matter. | **hang**, logged right away |

**How often (%)** decides how many requests get the fault: `100` (the default) for every request, `25` for about one in four, picked at random for each request, `0` for none. The route's row shows a lightning bolt with the fault and its percentage.

Faults apply to routes only. Requests that no route matches get the 404 or the forwarded answer as usual.

### What to test with faults

| Goal | Route setup |
|---|---|
| A client's timeout | **Never answer**, or a **Delay** longer than the timeout. |
| Retries | **Error (500)** at 30–50 %, and watch the traffic log for the retries. |
| Handling of a dropped connection | **Drop the connection** at 100 %. |
| A slow, flaky dependency | **Delay** 2000 plus **Error (500)** at 20 %. |
| A specific error body or status (`429`, `503` with `Retry-After`) | No fault: set the **Status**, headers and body of a route instead, possibly with [conditions](../mock-api/#conditions). |

## CORS

Browsers block a web page from reading answers from another origin (scheme, host and port) unless the server allows it with CORS headers. A front end on `http://localhost:5173` calling a mock on `http://127.0.0.1:3000` is such a cross-origin call.

Turn on **Allow cross-origin requests (CORS)** under **Browser access** in the mock's settings. The mock then does two things.

### Preflight requests

A request with method `OPTIONS` and an `Access-Control-Request-Method` header is a browser's preflight. It is answered **before any route is looked at**, with `204 No Content` and:

| Header | Value |
|---|---|
| `Access-Control-Allow-Origin` | The request's `Origin` (or `*` without one) |
| `Access-Control-Allow-Credentials` | `true` (when there is an `Origin`) |
| `Access-Control-Allow-Methods` | `GET, POST, PUT, PATCH, DELETE, HEAD, OPTIONS`, plus the requested method when it is another one |
| `Access-Control-Allow-Headers` | The headers the browser asked for (`Access-Control-Request-Headers`), echoed |
| `Access-Control-Allow-Private-Network` | `true`, when the browser asks (Chrome does before a public site may call a server on your computer) |
| `Access-Control-Max-Age` | `600` (browsers may cache the preflight for 10 minutes) |
| `Vary` | `Origin` (when there is an `Origin`) and `Access-Control-Request-Method, Access-Control-Request-Headers` |

The log marks these exchanges **CORS preflight**.

### Every other answer

Every answer (from routes, the 404, forwarded answers, faults with status 500, and the 408 and 413 errors) gets:

| Header | Value |
|---|---|
| `Access-Control-Allow-Origin` | The request's `Origin`, with `Access-Control-Allow-Credentials: true` and `Vary: Origin`. Without an `Origin` header: `*`. |
| `Access-Control-Expose-Headers` | The names of the answer's headers (other than `Access-Control-*`), so your JavaScript can read them. |

Echoing the origin with credentials allowed means cookies and `Authorization` headers work from any origin. When a route sets its own `Access-Control-Allow-Origin` or `Access-Control-Expose-Headers` header, the mock keeps the route's.

### Without the switch

With CORS off, the mock adds no CORS headers, and `OPTIONS` requests go to your routes like any other request. You can then answer CORS yourself with route headers, and a route with method `OPTIONS` for preflights.

### Other servers

- The [event stream server](../websocket-and-sse-servers/#event-stream-sse-server) always sends `Access-Control-Allow-Origin: *` (enough for `EventSource` without credentials). It has no CORS switch and answers `OPTIONS` with `405`.
- WebSocket connections are not subject to CORS. The [WebSocket server](../websocket-and-sse-servers/#websocket-server) accepts every `Origin`.

## Saved format

```yaml title="servers/Flaky API.yaml"
name: Flaky API
kind: http
seq: 0
host: 127.0.0.1
port: 3000
http:
  cors: true
  routes:
    - method: GET
      path: /orders
      status: 200
      body: '[]'
      delayMs: 1500
      fault: error
      faultPercent: 25
    - method: POST
      path: /payments
      fault: hang
```

| Field | Default | Description |
|---|---|---|
| `http.routes[].delayMs` | `0` | Delay before answering, in milliseconds. |
| `http.routes[].fault` | `none` | `none`, `error`, `reset` or `hang`. |
| `http.routes[].faultPercent` | `100` | Percentage of requests that get the fault. |
| `http.cors` | `false` | Allow cross-origin requests. |

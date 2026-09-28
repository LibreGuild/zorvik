---
id: http2-and-http3
title: HTTP/2 and HTTP/3
summary: Newer HTTP versions keep the same methods, headers and status codes but carry them more efficiently, and HTTP/3 moves from TCP to QUIC.
minutes: 6
lab:
  title: Change gears
  goal: Send the same request as HTTP/1.1 and HTTP/2, then see why HTTP/3 needs HTTPS.
  minutes: 5
  playground: true
  steps:
    - text: |
        Send `GET {{playground}}/echo`. The practice server echoes your request back, including the HTTP version it arrived with. Check the version in the response too.
      hints:
        - The version shows next to the response status (and under **Info**), and in the echo's `httpVersion` field.
        - "GET {{playground}}/echo, then Send. Over plain http:// Zorvik uses HTTP/1.1 unless told otherwise."
      check:
        send: { url: "{{playground}}/echo", status: 200 }
      solution:
        - send: { method: GET, url: "{{playground}}/echo" }
    - text: |
        In the request's **Settings** tab, set **HTTP version** to **HTTP/2 only** and send again.
      hints:
        - The request's own **Settings** tab overrides the app defaults for this one request.
        - "Settings tab → HTTP version → HTTP/2 only, then Send."
        - "The echo now says \"httpVersion\": \"HTTP/2\"."
      check:
        send: { url: "{{playground}}/echo", status: 200, httpVersion: HTTP/2, json: { httpVersion: HTTP/2 }, request: { settings: { httpVersion: http2 } } }
      solution:
        - send: { method: GET, url: "{{playground}}/echo", settings: { httpVersion: http2 } }
    - text: |
        Now pick **HTTP/3 (QUIC)** and send once more. It fails on purpose: read why.
      hints:
        - HTTP/3 runs over a different transport than the first two.
        - "Settings tab → HTTP version → HTTP/3 (QUIC), then Send, and read the error."
        - "The error explains that HTTP/3 needs an https:// URL, because QUIC is always encrypted."
      check:
        send: { url: "*/echo", request: { settings: { httpVersion: http3 } }, error: "*HTTP/3 needs an https*" }
      solution:
        - send: { method: GET, url: "{{playground}}/echo", settings: { httpVersion: http3 } }
quiz:
  - question: What stays the same between HTTP/1.1, HTTP/2 and HTTP/3?
    options:
      - How the bytes travel on the network
      - Methods, headers, status codes and bodies
      - Whether encryption is optional
    answer: 1
    explain: The meaning of a request doesn't change; only the way it's packed and carried does. Your saved requests work with any version.
  - question: What is multiplexing in HTTP/2?
    options:
      - Many requests and responses in flight at once over one connection
      - Sending every request twice for safety
      - Compressing images automatically
    answer: 0
    explain: HTTP/1.1 handles one request at a time per connection. HTTP/2 interleaves many, so one slow answer doesn't hold up the rest.
  - question: HTTP/3 fails at your office but HTTP/2 works. What's a likely reason?
    options:
      - HTTP/3 only works for GET requests
      - The server's certificate is too old for HTTP/3
      - The network blocks UDP, which QUIC runs on
    answer: 2
    explain: HTTP/3 uses QUIC over UDP port 443, which some firewalls and proxies block. Browsers then quietly fall back to HTTP/2 over TCP.
---

The HTTP you've been using is **HTTP/1.1**, from 1997 and still everywhere. Two newer versions carry the *same* requests (same methods, headers, status codes and bodies) in a smarter way. Your requests don't change; only what happens on the wire does.

## HTTP/1.1: one at a time

HTTP/1.1 sends requests as readable text over a **TCP** connection, one request and its response at a time. A slow response blocks the next request on that connection, so browsers open several connections side by side.

## HTTP/2: many at once

**HTTP/2** (2015) keeps one TCP connection and splits every message into small binary **frames**. Frames of different requests are interleaved, so many requests are in flight at once; that's called **multiplexing**. It also compresses headers, which repeat a lot between requests.

```sequence
participants: You, Server
Note over You, Server: one connection
You -> Server: stream 1: GET /users
You -> Server: stream 3: GET /orders
Server --> You: stream 3: 200 (the fast one)
Server --> You: stream 1: 200 (the slow one)
```

On the internet, HTTP/2 is used over HTTPS: the client and server agree on it during the TLS handshake with a feature called **ALPN** (the TLS inspector shows it, like `ALPN h2`). Over plain `http://` it's only used when both sides know in advance, like the practice server in the lab.

> [!note] Think of it like…
> A supermarket checkout. HTTP/1.1 is one cashier serving one customer at a time: one slow customer holds up the queue. HTTP/2 is a cashier who scans items from several baskets in turn, so nobody waits behind a full trolley.

## HTTP/3: a new road

HTTP/2 still has a weak spot: TCP delivers bytes strictly in order, so one lost packet stalls *every* stream until it's resent. **HTTP/3** (2022) replaces TCP with **QUIC**, a transport built on UDP. Each stream is delivered on its own, and connections set up faster.

```flow
HTTP/1.1 or HTTP/2 -> TLS (for https) -> TCP -> IP
HTTP/3 -> QUIC (TLS 1.3 built in) -> UDP -> IP
```

Encryption is part of QUIC itself, so HTTP/3 only exists over `https://`. Some networks block UDP; browsers then quietly fall back to HTTP/2.

## Versions in Zorvik

| Setting | Means |
|---|---|
| Auto (HTTP/2 if offered) | HTTP/2 when an HTTPS server offers it, else HTTP/1.1 (the app default) |
| HTTP/1.1 only | never upgrade |
| HTTP/2 only | fail if the server can't speak HTTP/2 |
| HTTP/3 (QUIC) | QUIC over UDP: https only, and never through a proxy |

Set it for all requests in **Settings → Requests → HTTP version**, or for one request in its own **Settings** tab. The response shows which version was really used. **Tools → HTTP/3 check** tells you whether a site speaks HTTP/3.

> [!tip] When the version matters
> Most of the time it doesn't. It matters when a bug only shows up in one version (a proxy that mangles HTTP/2, a load balancer without HTTP/3), or when you compare performance.

**You'll use this when…** a customer's browser behaves differently from your tests, or an API is fast from your laptop but slow through a proxy. Pin the version, compare, and you'll know whether the transport is to blame.

---
id: why-realtime
title: "Why real-time? Polling versus push"
summary: Asking "anything new?" over and over is slow and wasteful; with push, the server tells you the moment something happens.
minutes: 5
lab:
  title: Where's my pizza?
  goal: Track an order by polling, then by letting the server push updates, and feel the difference.
  minutes: 6
  servers:
    shop:
      name: Pizza shop
      kind: http
      http:
        routes:
          - method: GET
            path: /orders/:id
            headers:
              - key: Content-Type
                value: application/json
            body: '{"order": "{{request.params.id}}", "status": "in the oven"}'
    live:
      name: Pizza tracker
      kind: sse
      sse:
        intervalMs: 600
        events:
          - { event: status, data: in the oven }
          - { event: status, data: boxed }
          - { event: status, data: on the way }
          - { event: status, data: "delivered, door code {{secret.code}}" }
  steps:
    - text: |
        **Polling.** Send `GET {{shop}}/orders/42` three times, as an impatient app would. Look at the `status` each time.
      hints:
        - "Polling means asking the same question again and again. Just press Send three times."
        - "Open a new HTTP request (⌘/Ctrl + N), type {{shop}}/orders/42 and press Send, then press it twice more."
        - "GET {{shop}}/orders/42, Send three times. Every answer says in the oven: nothing new, three times."
      check:
        request: { server: shop, method: GET, path: /orders/42, count: 3 }
      solution:
        - send: { method: GET, url: "{{shop}}/orders/42" }
        - send: { method: GET, url: "{{shop}}/orders/42" }
        - send: { method: GET, url: "{{shop}}/orders/42" }
    - text: |
        **Push.** Click **+** at the end of the tab bar and choose **Event stream (SSE)**. Set the URL to `{{live}}/orders/42` and press **Connect**. Then just watch: the updates arrive by themselves, until the pizza is delivered.
      hints:
        - "This time you ask once and keep the line open. The server sends each change when it happens."
        - "The + menu in the tab bar lists Event stream (SSE). The Connect button sits where Send usually is."
        - "Create an Event stream (SSE) request for {{live}}/orders/42, press Connect and wait a few seconds for the delivered update."
      check:
        message: { server: live, direction: out, text: "delivered*" }
      solution:
        - call: { method: sse.connect, params: { connId: lab-sse, request: { name: Pizza tracker, kind: sse, url: "{{live}}/orders/42" } } }
        - wait: 2000
    - text: The last update came with a door code. Type it here.
      hints:
        - "It's in the data of the last event in the stream log."
        - "Click the last row of the log (the delivered update) to see its full data. The tracker's own Traffic shows it too: open Lab · Pizza tracker under Servers."
        - "The last event's data reads: delivered, door code … Copy the code after door code."
      check:
        answer: "{{secret.code}}"
      solution:
        - answer: "{{secret.code}}"
        - call: { method: sse.close, params: { connId: lab-sse } }
quiz:
  - question: What is the main downside of polling?
    options:
      - Most requests come back with nothing new, and news arrives late (up to one polling interval)
      - It needs a special protocol that browsers don't support
      - Servers are not allowed to answer the same GET twice
    answer: 0
    explain: Polling spends requests on "no change" answers, and you only see a change on your next poll.
  - question: A web page only needs to listen while the server sends updates. Which fits best?
    options:
      - Polling every 100 milliseconds
      - Server-Sent Events (SSE)
      - Sending a UDP datagram per update
    answer: 1
    explain: "SSE is built for exactly this: one HTTP request, then the server pushes events down it."
  - question: Thousands of sensors publish readings, and many apps want some of them. Which fits best?
    options:
      - A REST endpoint per sensor that every app polls
      - A WebSocket from every app to every sensor
      - MQTT, with a broker in the middle
    answer: 2
    explain: "MQTT's broker decouples senders from listeners: sensors publish to topics, apps subscribe to the topics they care about."
---

Some information changes all the time: a chat, a delivery tracker, stock prices, a sensor's temperature. Your app wants to know **the moment** something changes. There are two ways to find out.

## Polling: "Are we there yet?"

The app asks the server again and again, say every five seconds: "anything new?" Usually the answer is "no".

```sequence
App -> Server: GET /orders/42
Server --> App: in the oven
App -> Server: GET /orders/42
Server --> App: in the oven
App -> Server: GET /orders/42
Server --> App: boxed
```

Polling is simple and works with any HTTP API, but it has costs:

- **Wasted work.** Most requests return nothing new, yet each one costs the phone battery, the network and the server.
- **Delay.** A change is noticed only at the next poll. Poll every 30 seconds and you're up to 30 seconds late. Poll faster and the waste grows.

*Long polling* is a trick in between: the server holds each request open until it has news, then the app immediately asks again.

## Push: "I'll tell you"

With **push**, the app opens a connection once and keeps it open. The server sends news down it the moment something happens.

```sequence
App -> Server: open a stream for order 42
Server --> App: in the oven
Note over Server: … minutes pass, nothing is sent
Server --> App: boxed
Server --> App: on the way
Server --> App: delivered
```

No wasted questions, no delay. The price: the server must keep many connections open at once, and the app must notice when a connection drops and open it again. Some office networks and proxies also cut connections that stay quiet for too long, so real apps often combine both: push while connected, and a slow poll as a safety net.

> [!note] Think of it like…
> Polling is a child asking "are we there yet?" every minute of the drive. Push is the driver saying "I'll tell you when we're there", and actually doing it.

## The push technologies in this unit

| Technology | Direction | Typical use |
|---|---|---|
| **WebSocket** | both ways, any time | chat, games, live collaboration |
| **Server-Sent Events** (SSE) | server to client only, over plain HTTP | notifications, live scores, AI answers typed out word by word |
| **MQTT** | publish/subscribe through a broker | sensors, smart homes, fleets of devices |

(Server-to-server push also exists: a **webhook** is the server calling *your* URL when something happens.)

In the lab you'll track a pizza both ways. First you poll a shop's order endpoint. Then you open an event stream and just wait while the tracker pushes each change to you.

**You'll use this when…** a product manager asks "why does the dashboard show new orders a minute late?" You'll recognize polling, measure how often it asks, and know that push is the fix.

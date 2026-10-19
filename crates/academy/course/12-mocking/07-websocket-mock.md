---
id: websocket-mock
title: Mock a WebSocket server
summary: A WebSocket server in Zorvik greets clients, answers by rules, pushes messages on a timer and hangs up on cue, so you can build a real-time app before its back end is ready.
minutes: 6
added: 0.3.0
lab:
  title: A price feed of your own
  goal: Build a WebSocket server that answers by id, pushes a price every second and hangs up on request, then talk to it.
  minutes: 10
  servers:
    sample:
      name: Sample feed
      kind: websocket
      websocket:
        mode: rules
        rules:
          - { match: json, pattern: '$.op == "bye"', reply: '{"type": "bye"}', close: 4001 }
          - { match: json, pattern: $.id, reply: '{"id": {{message.id}}, "price": 42.5}' }
        schedule:
          - { message: '{"type": "tick", "price": {{$randomPrice(5, 50)}}}', everyMs: 1000 }
  steps:
    - text: |
        Open **Servers** on the left rail, click **＋** and choose **WebSocket server**. Name it **Price feed** and press **Create**.

        Under **Replies**, choose **Rules** and press **Add rule**. Set **When** to **JSON where**, **Message** to `$.id`, and **Reply** to:

        ```json
        {"id": {{message.id}}, "price": 42.5}
        ```

        Save with **⌘/Ctrl + S** and press **Start**. (**Lab · Sample feed** under **Servers** is a finished one, if you want to compare later.)
      hints:
        - "A JSON where rule matches JSON messages that meet a condition. $.id on its own means: the message has an id."
        - "In the Price feed tab: Replies → Rules → Add rule. When is a list; Message and Reply are text boxes. Start is at the top right."
        - "Rule: JSON where · $.id · {\"id\": {{message.id}}, \"price\": 42.5}. Then ⌘S and Start: the dot turns green."
      check:
        saved:
          server:
            name: Price feed
            kind: websocket
            running: true
            websocket:
              mode: rules
              rules: [{ match: json, reply: "*{{message.id}}*" }]
      solution:
        - call:
            method: server.create
            params:
              server: &feed
                name: Price feed
                kind: websocket
                port: 0
                websocket:
                  mode: rules
                  rules:
                    - { match: json, pattern: $.id, reply: '{"id": {{message.id}}, "price": 42.5}' }
        - call:
            method: server.save
            params: { id: Price feed, server: *feed }
        - call:
            method: server.start
            params: { id: Price feed, server: *feed }
    - text: |
        Make it push, and hang up on request. Under **Scheduled messages**, press **Add scheduled message**, type this and keep **1000** ms:

        ```json
        {"type": "tick", "price": {{$randomPrice(5, 50)}}}
        ```

        Then press **Add rule** again: **JSON where** `$.op == "bye"`, reply `{"type": "bye"}`, and **Then close** `4001`. Save with **⌘/Ctrl + S**.
      hints:
        - "Scheduled messages go to every connected client on a timer, whatever the rules say. Then close ends the connection after the rule's reply."
        - "The Scheduled messages section is below the Greeting. In the rules table, Then close is the column after Delay ms."
        - "Scheduled: {\"type\": \"tick\", \"price\": {{$randomPrice(5, 50)}}} every 1000 ms. Rule: JSON where · $.op == \"bye\" · {\"type\": \"bye\"} · Then close 4001. Then ⌘S."
      check:
        saved:
          server:
            name: Price feed
            kind: websocket
            running: true
            websocket:
              schedule: [{ message: "*tick*" }]
              rules: [{ match: json, close: 4001 }]
      solution:
        - call:
            method: server.save
            params:
              id: Price feed
              server: &feed2
                name: Price feed
                kind: websocket
                port: 0
                websocket:
                  mode: rules
                  rules:
                    - { match: json, pattern: '$.op == "bye"', reply: '{"type": "bye"}', close: 4001 }
                    - { match: json, pattern: $.id, reply: '{"id": {{message.id}}, "price": 42.5}' }
                  schedule:
                    - { message: '{"type": "tick", "price": {{$randomPrice(5, 50)}}}', everyMs: 1000 }
        - call:
            method: server.start
            params: { id: Price feed, server: *feed2 }
    - text: |
        Talk to it. Click **+** at the end of the tab bar, choose **WebSocket** and type your server's address, which its tab shows (such as `ws://127.0.0.1:3001`). Press **Connect**: a tick arrives every second.

        Switch the composer to **JSON** and send `{"id": 7}`. The answer carries the same `id`.
      hints:
        - "The address is in the Clients section of the Price feed tab while it runs."
        - "WebSocket request, URL ws://127.0.0.1 and your server's port, Connect, then JSON → {\"id\": 7} → Send."
        - "{\"id\": 7, \"price\": 42.5} comes back with a ↩ chip, between the ticks. Lab · Sample feed under Servers is a finished feed to compare with."
      check:
        all:
          - call: { method: ws.connect, ok: true, params: { request: { url: 're:(?i)^(wss?://)?(127\.0\.0\.1|localhost|\[::1\]):\d+/?$' } } }
          - call: { method: ws.send, ok: true, params: { message: { text: '*"id"*' } } }
      solution:
        - call: { method: ws.connect, params: { connId: lab-ws, request: { name: Price feed client, kind: websocket, url: "{{sample}}" } } }
        - call: { method: ws.send, params: { connId: lab-ws, message: { type: text, text: '{"id": 7}' } } }
        - wait: 300
    - text: |
        Now send `{"op": "bye"}`. Your server answers, then hangs up with the code from your rule. The log's last line says what that code means: type the meaning here.
      hints:
        - "Close codes from 4000 to 4999 aren't in the WebSocket standard's list; who decides what they mean?"
        - "Send {\"op\": \"bye\"} as JSON. The last line of the log starts with Disconnected by server (code 4001 · …)."
        - "The log reads Disconnected by server (code 4001 · Defined by the application). Type Defined by the application."
      check:
        all:
          - call: { method: ws.send, ok: true, params: { message: { text: "*bye*" } } }
          - answer: "*application*"
      solution:
        - call: { method: ws.connect, params: { connId: lab-ws, request: { name: Price feed client, kind: websocket, url: "{{sample}}" } } }
        - call: { method: ws.send, params: { connId: lab-ws, message: { type: text, text: '{"op": "bye"}' } } }
        - wait: 300
        - answer: Defined by the application
quiz:
  - question: "A rule's reply is `{\"id\": {{message.id}}, \"ok\": true}`. A client sends `{\"id\": 42, \"op\": \"buy\"}`. What does it get back?"
    options:
      - "`{\"id\": 42, \"ok\": true}`"
      - "`{\"id\": {{message.id}}, \"ok\": true}`"
      - "`{\"id\": \"42\", \"ok\": true}`"
    answer: 0
    explain: "{{message.id}} is replaced by the id from the client's JSON message. Numbers go in as JSON, without quotes, so the answer is valid JSON."
  - question: Every connected client should get a price each second, even if it never sends anything. What do you use?
    options:
      - A rule that matches any message
      - A scheduled message every 1000 ms
      - The greeting
    answer: 1
    explain: Rules only answer messages, and the greeting is sent once. Scheduled messages go to every client on a timer, each filled in fresh.
  - question: "A rule matches `$.op == \"bye\"` and has Then close `4001`. What happens when a client sends `{\"op\": \"bye\"}`?"
    options:
      - The whole server stops
      - The server sends the rule's reply, if it has one, then closes that client's connection with code 4001
      - The client gets an error but stays connected
    answer: 1
    explain: Only that connection closes, the way a real server ends a session. It's how you check that your app shows the right screen, or reconnects.
---

A live price ticker, a chat or a multiplayer lobby needs a WebSocket server to talk to. When that server isn't built yet, is flaky, or only sends prices while the market is open, make a stand-in in Zorvik: a **WebSocket server** that greets clients, answers by rules, pushes messages on a timer and hangs up when told to.

## What you set

| Part | What it does |
|---|---|
| **Replies** | **Echo** (send every message back), **Rules** (the first matching rule answers), **Manual** (you reply from the Traffic panel) or **Discard** |
| **Greeting** | sent to each client as soon as it connects |
| **Scheduled messages** | sent to every connected client every so many milliseconds |
| **Clients** | the address to connect to; any path works |

## Rules

A rule has a **When** (Contains, Is exactly, Matches regex, **JSON where** or Any message), a **Message** pattern, a **Reply**, a **Delay ms** and, on a WebSocket server, **Then close**. The first enabled rule that matches answers; a message no rule matches gets no reply.

**JSON where** reads a JSON message and checks a condition: `$.op == "subscribe"`, `$.id != 0`, or just `$.id` (the message has an id).

Replies are templates. `{{message}}` is the whole message; for a JSON message, `{{message.id}}` or `{{message.params.symbol}}` is one value from it. Numbers and objects go in as JSON and text as it is, so put quotes around text values: `"symbol": "{{message.params.symbol}}"`. `{{$uuid}}`, `{{$isoTimestamp}}` and environment variables work too.

```sequence
participants: App, Mock
App -> Mock: connect
App -> Mock: {"id": 7, "op": "quote"}
Note over Mock: JSON where $.id matches
Mock --> App: {"id": 7, "price": 42.5}
Mock --> App: {"type": "tick", "price": 18.25} (every second)
App -> Mock: {"op": "bye"}
Mock --> App: {"type": "bye"}, then close 4001
```

> [!tip] Order matters
> Put specific rules (`$.op == "bye"`) above general ones (`$.id`, Any message): the first match answers.

## Pushing on a timer

**Scheduled messages** go to every client every so many milliseconds (10 or more), whatever the reply mode. Each one is filled in fresh, so `{"price": {{$randomPrice(5, 50)}}}` brings a new price every time. The first arrives one interval after a client connects. Untick one to pause it.

## Hanging up on cue

Real servers close connections: a session expires, a user is removed, the server restarts. A rule's **Then close** takes a close code (`1000`, or `3000` to `4999`): the server sends the reply, if there is one, then closes that connection with the code. Pair it with a test message such as `{"op": "bye"}` to check that your app shows the right screen, or reconnects.

> [!note] Think of it like…
> An understudy rehearsing with you before the real cast arrives. They say hello when you walk in, have a line ready for each cue, call out the time every minute without being asked, and leave the stage when you say "goodbye".

## Watching it work

The server's **Traffic** panel shows every connection, message and close from the server's side, and sends a message to one client or all of them by hand. A WebSocket request in Zorvik is the quickest client to try it with.

**You'll use this when…** the front end needs live data before the back end exists, when you want to see how your app copes with slow answers or a server that hangs up, or when a demo must show the same ticker every time.

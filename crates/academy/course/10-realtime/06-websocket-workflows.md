---
id: websocket-workflows
title: WebSocket workflows
summary: Save the messages you send often, let Zorvik run the same routine on every connect, pair replies with requests, and turn waits into tests.
minutes: 6
added: 0.3.0
lab:
  title: The ticket desk routine
  goal: Pair a reply with its request, save a connect routine that sends and waits, run it as a test, then keep a heartbeat going and hang up with your own code.
  minutes: 12
  servers:
    desk:
      name: Ticket desk
      kind: websocket
      websocket:
        mode: rules
        greeting: '{"type": "welcome", "desk": "tickets"}'
        rules:
          - { match: json, pattern: '$.op == "bye"', reply: '{"type": "bye"}', close: 4001 }
          - { match: exact, pattern: ping, reply: pong }
          - { match: json, pattern: $.id, reply: '{"id": {{message.id}}, "ticket": "{{secret.word}}"}', delayMs: 150 }
          - { match: any, reply: '{"error": "Send JSON with a number id, such as {\"id\": 1, \"op\": \"open\"}"}' }
  steps:
    - text: |
        The lab's **Ticket desk** answers JSON messages that carry an `id`. Click **+** at the end of the tab bar, choose **WebSocket**, set the URL to `{{desk}}` and press **Connect**. The desk greets you.

        Switch the composer to **JSON** and send `{"id": 1, "op": "open"}`. The answer carries the same `id`, and its green chip shows how long it took.
      hints:
        - "The desk only answers JSON messages with a number id; anything else gets a short error message back."
        - "Connect to {{desk}}, pick JSON in the composer (next to Text), paste the message and press Send."
        - "Send {\"id\": 1, \"op\": \"open\"}. About 150 ms later {\"id\": 1, \"ticket\": …} arrives with a ↩ chip; click it to see which message it answers."
      check:
        message: { server: desk, direction: in, text: '*"id"*' }
      solution:
        - call: { method: ws.connect, params: { connId: lab-ws, request: { name: Ticket desk, kind: websocket, url: "{{desk}}" } } }
        - call: { method: ws.send, params: { connId: lab-ws, message: { type: text, text: '{"id": 1, "op": "open"}' } } }
        - wait: 300
    - text: The desk's answer holds your ticket code. Type it here.
      hints:
        - "Look at the newest message with a blue arrow (received)."
        - "It reads {\"id\": 1, \"ticket\": \"…\"}. Lab · Ticket desk under Servers shows the same answer in its Traffic."
        - "Copy the value of ticket, without the quotes."
      check:
        answer: "{{secret.word}}"
      solution:
        - answer: "{{secret.word}}"
    - text: |
        Make it a routine. In the **Messages** tab press **Add message**, name it `Open ticket`, choose **JSON** and type `{"id": 2, "op": "open"}`.

        In the **On connect** tab, choose **Add step → Send a saved message** (Open ticket), then **Add step → Wait for a message**, set it to **JSON where** and type `$.id == 2`. Save the request as **Ticket desk** (**⌘/Ctrl + S**), press **Disconnect**, then **Connect** again. The log notes each step as it runs.
      hints:
        - "On connect steps run in order after every connect. The wait passes when the desk's answer to message 2 arrives."
        - "Messages → Add message (name Open ticket, JSON). On connect → Add step → Send a saved message, then Add step → Wait for a message, JSON where, $.id == 2."
        - "Save as Ticket desk, reconnect, and look for \"On connect: sent Open ticket\" and \"On connect: got a JSON message where $.id == 2\" in the log."
      check:
        all:
          - saved:
              request:
                kind: websocket
                websocket:
                  messages: [{ text: '*"id"*' }]
                  onConnect: [{ type: send }, { type: wait, match: json }]
          - call: { method: ws.connect, ok: true, params: { request: { websocket: { onConnect: [{ type: send }] } } } }
      solution:
        - save:
            name: Ticket desk
            kind: websocket
            url: "{{desk}}"
            websocket: &routine
              messages:
                - { name: Open ticket, format: json, text: '{"id": 2, "op": "open"}' }
              onConnect:
                - { type: send, message: Open ticket }
                - { type: wait, match: json, pattern: "$.id == 2" }
        - call: { method: ws.connect, params: { connId: lab-ws, request: { name: Ticket desk, kind: websocket, url: "{{desk}}", websocket: *routine } } }
        - wait: 500
    - text: |
        Waits are tests. Open the runner: collection **+** → **Run…**. Press **None** above the list of requests, tick **Ticket desk** and press **Run**. The request connects, runs its steps and closes, and the wait shows as a passed test.
      hints:
        - "In a collection run, a WebSocket request connects, runs its On connect steps and disconnects. Each Wait for step is a test."
        - "The + next to the Filter box of the collection has Run…. The runner opens in a tab of its own; None clears the selection."
        - "Run only Ticket desk: one request, one passed test named Wait for a JSON message where $.id == 2."
      check:
        run: { testsPassed: ">=1", testsFailed: 0 }
      solution:
        - call: { method: folder.create, params: { parent: "", name: Ticket desk checks } }
        - call:
            method: request.create
            params:
              parent: Ticket desk checks
              request: { name: Ticket desk, kind: websocket, url: "{{desk}}", websocket: *routine }
        - call: { method: runner.start, params: { folder: Ticket desk checks } }
        - wait: 1000
    - text: |
        Keep the line alive, then hang up your way. Add a second message, `Ping`, with the text `ping` (format **Text**), and a third On connect step: **Keep sending a message (heartbeat)**, Ping, every `1000` ms. Connect again and watch `ping` and `pong` go back and forth.

        Then hang up with your own code: in the URL bar's **⋯** menu choose **Close with a code…** and type `4001 Done for today`.
      hints:
        - "A heartbeat keeps a quiet connection from being dropped. The desk answers ping with pong."
        - "Messages → Add message: Ping, Text, ping. On connect → Add step → Keep sending a message (heartbeat), pick Ping, every 1000 ms. Then Connect."
        - "After two or three pings, ⋯ → Close with a code… → 4001 Done for today → Close connection. Sending {\"op\": \"bye\"} makes the desk hang up with 4001 too."
      check:
        all:
          - message: { server: desk, direction: in, text: ping, count: 2 }
          - message: { server: desk, kind: close, summary: "*4001*" }
      solution:
        - call:
            method: ws.connect
            params:
              connId: lab-ws
              request:
                name: Ticket desk
                kind: websocket
                url: "{{desk}}"
                websocket:
                  messages:
                    - { name: Open ticket, format: json, text: '{"id": 2, "op": "open"}' }
                    - { name: Ping, text: ping }
                  onConnect:
                    - { type: send, message: Open ticket }
                    - { type: wait, match: json, pattern: "$.id == 2" }
                    - { type: repeat, message: Ping, everyMs: 300 }
        - wait: 1200
        - call: { method: ws.close, params: { connId: lab-ws, code: 4001, reason: Done for today } }
        - wait: 300
quiz:
  - question: A WebSocket request's On connect steps are *Send Login*, then *Wait for a JSON message where $.ok == true*. What happens to it in a collection run?
    options:
      - It is skipped, because runs only send HTTP requests
      - It connects, sends Login, and the wait is a test that passes when the answer arrives in time
      - It keeps sending Login until the run ends
    answer: 1
    explain: Runs connect, run the steps and close. Each Wait for step becomes a test, so a server that stops answering fails your run, in CI too.
  - question: "You sent `{\"id\": 7, \"op\": \"open\"}` and messages keep arriving. Which one does Zorvik show as its reply?"
    options:
      - The next message that arrives after yours
      - The one whose value at the Match replies by path ($.id) is 7 too
      - The longest one
    answer: 1
    explain: Answers can come back in any order, mixed with pushed updates. Pairing by id is how apps (and Zorvik) know which answer belongs to which request.
  - question: Your app closes a WebSocket because the session expired. Which close code fits?
    options:
      - "1006"
      - A code the application defines, from 4000 to 4999, such as 4001
      - "404"
    answer: 1
    explain: 4000 to 4999 are for applications to define, so client and server can agree that 4001 means "session expired". 1006 is never sent; it means the connection dropped without a close message.
---

Real WebSocket APIs rarely stop at "hello". A trading app connects, waits for the welcome, logs in, subscribes, checks each answer, and sends a small "still here" message now and then so the connection isn't dropped. Retyping that on every reconnect gets old fast, so a WebSocket request in Zorvik keeps the routine with it.

## Saved messages

The **Messages** tab keeps the messages you send often, each with a name, a format (**Text**, **JSON**, or **Hex** for binary bytes) and its text. `{{variables}}` are filled in when the message goes out, so `{"op": "login", "token": "{{token}}"}` works in every environment. Each message has **Put in the composer** and **Send now** buttons. The composer's bookmark button, **Saved messages**, lists them too, and **Save as message…** keeps what you just typed.

## Which reply answers what?

Many APIs put an `id` in each request and the same `id` in its answer, because answers can come back in any order, mixed with updates the server pushes. Zorvik pairs them: when a received message has the same value at the **Match replies by** path (`$.id` unless you change it) as a message you sent, it gets a green chip with the time it took, such as **↩ 152 ms**. Click the chip to highlight the message it answers.

```sequence
participants: You, Desk
You -> Desk: {"id": 1, "op": "open"}
You -> Desk: {"id": 2, "op": "open"}
Desk --> You: {"type": "notice"} (pushed, answers nothing)
Desk --> You: {"id": 2, "ticket": "…"} ↩ 120 ms
Desk --> You: {"id": 1, "ticket": "…"} ↩ 160 ms
```

## On connect: the routine, every time

The **On connect** tab holds steps that run in order right after every **Connect**:

| Step | What it does |
|---|---|
| **Send** | sends one of your saved messages |
| **Wait for** | waits for a message that *contains* some text, *is exactly* it, *matches regex*, is *JSON where* a condition holds, or *any message*, within a time limit (5000 ms unless you change it) |
| **Keep sending** | sends a saved message every so many milliseconds: a **heartbeat** |

A *JSON where* condition looks like `$.type == welcome`, `$.id == 2`, or just `$.token` (the value is there). A wait also counts an answer that arrived just before it started; as the first step, it sees the server's greeting. The log notes each step ("On connect: sent Login"). The first step that fails stops the rest, and the connection stays open so you can look around.

```flow
Connect -> Wait for the welcome -> Send Login -> Wait for $.ok == true -> Keep sending Ping
```

> [!note] Think of it like…
> A script for calling a help desk: dial, wait for "Hello, ticket desk", read out your number, wait for "found it", and say "still there?" every minute so nobody hangs up. Write it once, and every call follows it.

## Heartbeats and hanging up

Servers and proxies often drop connections that stay quiet. Besides a **Keep sending** step, the menu on the **Send** button repeats the composer's message (**Send every second**, **Send every 5 seconds**, **Send every…**) until you press **Stop**.

**Disconnect** closes with code `1000`, a normal close. To see how an app copes with other endings, choose **Close with a code…** in the URL bar's **⋯** menu and type a code and a reason, such as `4001 Session expired`. Codes `4000` to `4999` are for applications to define; `1008` means *policy violation*. The log's last line spells out what a code means.

## Waits become tests

A WebSocket request also runs in the collection runner and in `zorvik run`: it connects, runs its On connect steps and closes. Each **Wait for** step is a test, named like *Wait for a JSON message where $.id == 2*, so a server that stops answering fails your run. Keep sending steps are left out of runs.

**You'll use this when…** you test a chat, a trading screen or a game server whose sessions start with the same few messages: save the routine once, reconnect as often as you like, and let the runner check the answers on every build.

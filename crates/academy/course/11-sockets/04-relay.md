---
id: tcp-relay
title: Watch the wire with a relay
summary: A TCP relay sits between a client and a server, passes every byte along unchanged and shows you both directions.
minutes: 5
lab:
  title: Tap the line
  goal: Talk to a backend through a relay and read the conversation, both ways, in the relay's traffic log.
  minutes: 7
  servers:
    backend:
      name: Backend
      kind: tcp
      socket:
        mode: rules
        greeting: BACKEND READY
        lineEnding: lf
        rules:
          - { match: regex, pattern: "(?i)^ping\\s*$", reply: "PONG {{secret.word}}" }
          - { match: any, reply: "GOT: {{message}}" }
    relay:
      name: Wire tap
      kind: tcpProxy
      proxy:
        target: "{{backend_host}}"
  steps:
    - text: |
        Open a **TCP connection** to the **relay**, `{{relay}}` (not to the backend), press **Connect** and send `PING`. You get the backend's greeting and answer, just as if you were talking to it directly.
      hints:
        - "The relay listens on its own port and forwards everything to the backend. Your app only needs the relay's address."
        - "Create a TCP connection from the + menu of the tab bar, put {{relay}} in the URL bar and press Connect."
        - "URL {{relay}}, Connect, type PING and press Send. BACKEND READY and PONG … come back."
      check:
        message: { server: relay, direction: toTarget, text: "PING*" }
      solution:
        - call: { method: socket.connect, params: { connId: lab-tcp, request: { name: Relay, kind: tcp, url: "{{relay}}" } } }
        - call: { method: socket.send, params: { connId: lab-tcp, message: { type: text, text: PING } } }
        - wait: 200
    - text: |
        Now look from the middle. Open **Servers** in the left rail and click **Lab · Wire tap**. Its **Traffic** shows every chunk of bytes: **→ target** went from you to the backend, **← target** came back. What word followed `PONG` on the way back?
      hints:
        - "Servers is one of the icons in the left rail. The relay is listed there as Lab · Wire tap."
        - "In the relay's Traffic, look at the rows marked ← target: those bytes came from the backend."
        - "Find the ← target row with PONG in it and copy the word after PONG, number included."
      check:
        answer: "{{secret.word}}"
      solution:
        - answer: "{{secret.word}}"
    - text: |
        The relay also notes where it forwarded each connection, in a line such as `#1 relayed to 127.0.0.1:…`. On which **port** is the backend listening?
      hints:
        - "Scroll to the top of the relay's Traffic, just after your connection was opened."
        - "The line reads #1 relayed to 127.0.0.1:… (the number after # counts your connections). The number after the colon is the port."
        - "Type only the number after 127.0.0.1: in the relayed to line."
      check:
        answer: "{{backend_port}}"
      solution:
        - answer: "{{backend_port}}"
    - text: |
        Finally, skip the relay: open a TCP connection straight to the backend, `{{backend}}`, and send `DIRECT hello`. Then compare the two traffic logs. The relay never saw this message, and the backend's log shows one connection coming from the relay and one straight from you.
      hints:
        - "A relay only sees what is sent through it."
        - "Use a new TCP connection (or change the URL and reconnect) with {{backend}} as the address."
        - "Connect to {{backend}}, send DIRECT hello, then open Lab · Backend under Servers and look at its Traffic."
      check:
        message: { server: backend, direction: in, text: "DIRECT*" }
      solution:
        - call: { method: socket.connect, params: { connId: lab-tcp, request: { name: Backend, kind: tcp, url: "{{backend}}" } } }
        - call: { method: socket.send, params: { connId: lab-tcp, message: { type: text, text: DIRECT hello } } }
        - wait: 200
        - call: { method: socket.close, params: { connId: lab-tcp } }
quiz:
  - question: In a relay's traffic log, what does a row marked "→ target" hold?
    options:
      - Bytes the relay received from the client and passed on to the target
      - Bytes the target sent back to the client
      - An error the relay ran into
    answer: 0
    explain: The arrow shows the direction. "→ target" is client to target; "← target" is the answer coming back.
  - question: You connect through a relay. Which address does the backend see as its client?
    options:
      - Your app's address
      - The address of the router in your office
      - The relay's address, because the relay opened its own connection to the backend
    answer: 2
    explain: A relay holds two connections, one with you and one with the target. The target only ever talks to the relay.
  - question: Why put a relay between an app and a server?
    options:
      - To make the network faster
      - To see exactly which bytes both sides exchange, without changing either of them
      - To encrypt everything automatically
    answer: 1
    explain: The app just points at the relay's address. The relay passes bytes along unchanged and records them.
---

When an app and a server misbehave together, the best evidence is the conversation itself: the exact bytes each side sent. But you can't always add logging to the app (a mobile game, a device, a database driver), and the server may belong to someone else. The answer is to stand in the middle.

## What a relay does

A **TCP relay** (also called a TCP proxy) listens on a port of its own. When a client connects, the relay opens a second connection to the **target** server and then copies bytes both ways, unchanged, recording everything it passes along.

```flow
Your app -[bytes]-> Relay -[same bytes]-> Target server
Target server -[reply]-> Relay -[same reply]-> Your app
```

The only change on the app's side is the address: it connects to the relay instead of the real server.

```sequence
participants: You, Relay, Backend
You -> Relay: connect
Relay -> Backend: connect (a second connection)
Backend --> Relay: BACKEND READY
Relay --> You: BACKEND READY
You -> Relay: PING
Relay -> Backend: PING
Backend --> Relay: PONG …
Relay --> You: PONG …
Note over Relay: logs every chunk, in both directions
```

Two things follow from the picture:

- The relay holds **two connections**. The backend sees the relay as its client, not you.
- The relay sees only what goes **through** it. Talk to the backend directly and the relay knows nothing.

> [!note] Think of it like…
> An interpreter who repeats every sentence word for word and writes it all down. Neither side needs to change how they speak, and afterwards you have a transcript of the whole conversation.

## Reading the traffic

In a Zorvik relay's **Traffic**, each chunk of bytes is one row:

- **→ target**: from the client, passed on to the target.
- **← target**: from the target, passed back to the client.
- A note such as `#1 relayed to 127.0.0.1:9000` says where each connection went, and `#1` numbers the connections so you can tell them apart.

Rows are *chunks*, the pieces the network happened to deliver, not messages. Framing still applies: one message may span two rows, and one row may hold two messages.

## In Zorvik

Create one under **Servers** with **+** and **TCP relay**, and fill in the **Target server** (`host:port`). The relay listens on its own **Port**. The switch **Connect to the target with TLS** lets a plain-text client reach a TLS server, so you can read traffic that would otherwise be encrypted on its way out.

> [!warning] Only on systems you're allowed to test
> A relay sees every byte, including passwords sent in plain text. Use it on your own apps, test systems and servers you have permission to inspect.

**You'll use this when…** a client library, device or legacy app talks to a server over its own protocol and something goes wrong. Point it at a relay, reproduce the problem, and read the exact bytes in both directions instead of guessing.

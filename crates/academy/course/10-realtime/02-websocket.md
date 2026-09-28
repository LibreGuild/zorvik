---
id: websocket-basics
title: "WebSocket: talk both ways"
summary: A WebSocket starts as an HTTP request, then turns into an open two-way line where either side can send a message at any time.
minutes: 5
lab:
  title: Chat with a server
  goal: Open a WebSocket, send messages, read the server's replies and close the connection properly.
  minutes: 7
  servers:
    chat:
      name: Chat bot
      kind: websocket
      websocket:
        mode: rules
        greeting: Welcome to the lab chat! Say hello.
        rules:
          - { match: regex, pattern: "(?i)hello", reply: "Hello to you too! Today's password is {{secret.word}}." }
          - { match: regex, pattern: "(?i)^ping$", reply: pong }
          - { match: any, reply: "Sorry, I only understand hello and ping. You said: {{message}}" }
  steps:
    - text: |
        Click **+** at the end of the tab bar and choose **WebSocket**. Set the URL to `{{chat}}` and press **Connect**. The server greets you as soon as the connection opens.
      hints:
        - "WebSocket addresses start with ws:// (or wss:// with TLS). The Lab environment holds the chat server's address in chat."
        - "In the + menu of the tab bar, choose WebSocket. Put {{chat}} in the URL bar. The main button says Connect instead of Send."
        - "WebSocket request, URL {{chat}}, press Connect. The log shows Connected, then the greeting with a blue arrow."
      check:
        message: { server: chat, kind: open }
      solution:
        - call: { method: ws.connect, params: { connId: lab-ws, request: { name: Lab chat, kind: websocket, url: "{{chat}}" } } }
    - text: |
        Type `hello` in the message box at the bottom and press **Send**. Your message appears with a green arrow, the reply with a blue one.
      hints:
        - "Once connected, the stream pane has a composer at the bottom: a box for your message and a Send button."
        - "Keep the composer on Text, type hello and press Send (or ⌘/Ctrl + Enter)."
        - "Type hello in the Message to send box and press Send. The server answers right away."
      check:
        message: { server: chat, direction: in, text: "*hello*" }
      solution:
        - call: { method: ws.connect, params: { connId: lab-ws, request: { name: Lab chat, kind: websocket, url: "{{chat}}" } } }
        - call: { method: ws.send, params: { connId: lab-ws, message: { type: text, text: hello } } }
        - wait: 200
    - text: The server's reply contains today's password. Type it here.
      hints:
        - "Look at the newest message with a blue arrow (received)."
        - "It reads: Hello to you too! Today's password is … The server's side shows it too: open Lab · Chat bot under Servers and look at its Traffic."
        - "Copy the word after Today's password is (without the full stop)."
      check:
        answer: "{{secret.word}}"
      solution:
        - answer: "{{secret.word}}"
    - text: |
        Send `ping` and watch `pong` come back. Then press **Disconnect** to close the connection properly.
      hints:
        - "The chat bot has a rule that answers ping with pong."
        - "Type ping, press Send, then use the Disconnect button in the URL bar."
        - "Send ping, see pong arrive, then press Disconnect. The server's log records the closed connection."
      check:
        all:
          - message: { server: chat, direction: in, text: ping }
          - message: { server: chat, kind: close }
      solution:
        - call: { method: ws.connect, params: { connId: lab-ws, request: { name: Lab chat, kind: websocket, url: "{{chat}}" } } }
        - call: { method: ws.send, params: { connId: lab-ws, message: { type: text, text: ping } } }
        - wait: 200
        - call: { method: ws.close, params: { connId: lab-ws } }
        - wait: 300
quiz:
  - question: How does a WebSocket connection begin?
    options:
      - As an HTTP request asking to upgrade, which the server accepts with 101 Switching Protocols
      - With a UDP datagram to port 101
      - With a DNS query of type WS
    answer: 0
    explain: The upgrade handshake lets WebSockets use the same ports, URLs and proxies as the web.
  - question: Once a WebSocket is open, who can send messages?
    options:
      - Only the client
      - Only the server
      - Both sides, at any time, without waiting for each other
    answer: 2
    explain: That's the point of WebSocket. Unlike HTTP, the server doesn't wait to be asked.
  - question: Which address is a WebSocket protected by TLS encryption?
    options:
      - ws://chat.example.com
      - wss://chat.example.com
      - http://chat.example.com
    answer: 1
    explain: wss:// is to ws:// what https:// is to http://. Use it for anything real.
---

HTTP is a strict conversation: the client asks, the server answers, done. The server can't speak until it is spoken to. For a chat app that's a problem: when your friend writes, *their* message should reach *you* without you asking.

**WebSocket** fixes this. It gives you one long-lived connection on which **both sides can send messages at any time**.

## The upgrade handshake

A WebSocket starts life as an ordinary HTTP request that asks the server to switch protocols:

```anatomy
GET /chat HTTP/1.1 | starts as a normal HTTP GET
Upgrade: websocket | "let's switch to WebSocket"
Connection: Upgrade | the connection itself will change
Sec-WebSocket-Key: dGhlIHNhbXBsZQ== | a random key; the server's answer proves it understood
HTTP/1.1 101 Switching Protocols | the server agrees: from now on, WebSocket messages
```

After the `101`, the same TCP connection stays open and carries **messages** (the spec calls them *frames*) in both directions.

```sequence
App -> Server: GET /chat (Upgrade: websocket)
Server --> App: 101 Switching Protocols
Server --> App: Welcome to the lab chat! Say hello.
App -> Server: hello
Server --> App: Hello to you too!
Note over App, Server: either side may send at any moment
App -> Server: close (code 1000, normal)
Server --> App: close
```

## What travels on a WebSocket

- **Text messages**, often JSON such as `{"type": "chat", "text": "hi"}`.
- **Binary messages**: raw bytes, for images, audio or compact formats.
- **Ping and pong**: tiny "are you still there?" checks that keep idle connections alive.
- **Close**: a polite goodbye with a code, such as `1000` (normal) or `1001` (going away). If a connection just vanishes without one, something went wrong.

Addresses use `ws://`, or `wss://` for WebSocket over TLS, the same encryption as `https://`.

> [!note] Think of it like…
> HTTP is ordering at a takeaway counter: you ask, you get your food, you leave, and the cook can't call you back later. A WebSocket is taking a seat at the bar: once you're there, you and the bartender can each speak up whenever there's something to say, until one of you leaves.

## In Zorvik

A **WebSocket** request has a URL, **Params**, **Headers** and **Auth** like HTTP, because the handshake *is* HTTP. **Connect** opens the line; the log then shows every message with a green ↗ (sent) or blue ↙ (received) arrow and a counter such as "3 sent · 4 received". The composer at the bottom sends **Text**, **JSON** or **Binary (hex)**. **Disconnect** closes the connection with a proper close message.

> [!tip] A WebSocket server of your own
> The lab's chat bot is an ordinary Zorvik server: find **Lab · Chat bot** under **Servers** to see its greeting and reply rules, and its traffic log shows the same conversation from the server's side.

**You'll use this when…** you test a chat, a live dashboard or a multiplayer feature: you connect like the app does, send the exact messages you want (including broken ones), and check what the server pushes back.

---
id: socketio
title: "Socket.IO: events, namespaces and acknowledgements"
summary: Socket.IO builds on WebSocket (and falls back to long-polling) and adds named events, namespaces, and replies you can wait for.
minutes: 5
added: 0.2.0
lab:
  title: The game lobby
  goal: Join a Socket.IO namespace, emit events, and get an acknowledgement back from the server.
  minutes: 7
  servers:
    lobby:
      name: Game lobby
      kind: socketio
      socketio:
        mode: rules
        greetingEvent: welcome
        greetingArgs: '{"players": 3, "tables": ["poker", "chess"]}'
        rules:
          - { event: join, ack: '{"ok": true, "table": {{event.arg0}}, "seat": "{{secret.seat}}"}' }
          - { event: chat, replyEvent: chat, replyArgs: '["host", {{event.arg0}}]', broadcast: true }
          - { event: "*", replyEvent: unknown, replyArgs: "{{event.name}}" }
  steps:
    - text: |
        Click **+** at the end of the tab bar and choose **Socket.IO client**. Set the URL to `{{lobby}}/games`: the path is the **namespace** to join, `/games`. Press **Connect**. The server greets you with a `welcome` event.
      hints:
        - "A Socket.IO URL is the server plus the namespace. No path means the main namespace, /."
        - "In the + menu of the tab bar, choose Socket.IO client, put {{lobby}}/games in the URL bar and press Connect."
        - "Socket.IO client, URL {{lobby}}/games, Connect. The log says Joined /games, then shows the welcome event with its arguments."
      check:
        message: { server: lobby, summary: joined /games }
      solution:
        - call: { method: socket.connect, params: { connId: lab-io, path: null, request: { name: Lobby, kind: socketio, url: "{{lobby}}/games" } } }
        - wait: 200
    - text: |
        Join a table: in the composer at the bottom, set **Event** to `join`, turn **Ack** on, type `"poker"` as the argument and press **Emit**. The server acknowledges with your seat number. Type it here.
      hints:
        - "Arguments are JSON: \"poker\" (with quotes) is one text argument. Ack asks the server to answer this very event."
        - "Event join, Ack on, arguments \"poker\" in JSON mode, then Emit. The reply shows as ack #1."
        - "Open the ack #1 row: its arguments contain seat. Copy that value."
      check:
        all:
          - message: { server: lobby, summary: "/games join (asks for acknowledgement*" }
          - answer: "{{secret.seat}}"
      solution:
        - call: { method: socket.connect, params: { connId: lab-io, path: null, request: { name: Lobby, kind: socketio, url: "{{lobby}}/games" } } }
        - call: { method: socket.send, params: { connId: lab-io, message: { type: emit, event: join, args: '"poker"', ack: true } } }
        - wait: 200
        - answer: "{{secret.seat}}"
    - text: |
        Emit an event the lobby doesn't know, such as `dance` (Ack off, no arguments needed). The server answers with an `unknown` event naming it. Then press **Disconnect**.
      hints:
        - "The lobby has a catch-all rule for every event it doesn't handle."
        - "Set Event to dance, clear the arguments, press Emit, then Disconnect in the URL bar."
        - "Emit dance, see unknown [\"dance\"] arrive, then Disconnect. The lobby's traffic shows the client leaving."
      check:
        all:
          - message: { server: lobby, direction: out, summary: /games unknown }
          - message: { server: lobby, kind: close }
      solution:
        - call: { method: socket.connect, params: { connId: lab-io, path: null, request: { name: Lobby, kind: socketio, url: "{{lobby}}/games" } } }
        - call: { method: socket.send, params: { connId: lab-io, message: { type: emit, event: dance, args: "" } } }
        - wait: 200
        - call: { method: socket.close, params: { connId: lab-io } }
        - wait: 300
quiz:
  - question: What does Socket.IO add on top of a plain WebSocket?
    options:
      - Named events, namespaces, acknowledgements and a fallback to long-polling
      - Encryption, which WebSocket doesn't have
      - Nothing; it is another name for WebSocket
    answer: 0
    explain: "A WebSocket only carries messages. Socket.IO gives them names (events), groups connections (namespaces, rooms), lets a sender wait for an answer (acknowledgements) and works where WebSocket is blocked."
  - question: A plain WebSocket client connects to a Socket.IO server. What happens?
    options:
      - It works, but events arrive as JSON
      - It connects, but it doesn't speak Socket.IO's packets, so nothing useful happens
      - The server upgrades it to Socket.IO by itself
    answer: 1
    explain: Socket.IO has its own handshake and packet format on top of the transport. Use a Socket.IO client, like socket.io-client or Zorvik's.
  - question: What is an acknowledgement?
    options:
      - A receipt the browser shows when a message was delivered
      - The receiver's answer to one particular event, sent back to the sender
      - A ping that keeps the connection alive
    answer: 1
    explain: With an acknowledgement, the sender passes a callback (or awaits emitWithAck), and the receiver answers that very event.
---

A plain WebSocket moves messages, and that's all. Real apps quickly want more: messages with names, groups of connections, and a way to know the other side handled something. **Socket.IO** is a library that adds exactly that, on top of WebSocket.

## Events instead of messages

With Socket.IO, you don't send raw text. You **emit an event**: a name and some arguments.

```js
socket.emit("chat", "Hello!", { room: "lobby" });

socket.on("chat", (text, info) => {
  console.log(text, info.room);
});
```

The server listens for `chat` the same way. Arguments can be any JSON, and even binary data.

## Acknowledgements

Sometimes the sender needs an answer to *this* event: "did my order go through?" An **acknowledgement** is that answer.

```sequence
Client -> Server: emit join "poker" (asks for an ack)
Server --> Client: ack {"ok": true, "seat": 4}
```

In code the client passes a callback as the last argument, or awaits `emitWithAck`. In Zorvik, turn on **Ack** in the composer.

## Namespaces and rooms

One server can host separate channels, called **namespaces**: `/chat`, `/games`, `/admin`. A client picks one in its URL (`http://host:3000/games`) and only sees that namespace's events. Inside a namespace, the server can put connections in **rooms** and emit to a whole room at once, which is how chat rooms and multiplayer games work.

> [!note] Think of it like…
> A big office building. The namespace is the floor you go to, rooms are the meeting rooms on it, events are things people say out loud, and an acknowledgement is someone saying "got it" back to you.

## How it connects

Socket.IO usually starts with **HTTP long-polling** (plain requests that wait for news), which works through almost any proxy, then **upgrades** to WebSocket when it can. Heartbeats keep checking the connection, and the client reconnects by itself when it drops.

That's why a plain WebSocket client can't talk to a Socket.IO server, and the other way round: Socket.IO has its own handshake and packet format on top of the transport.

## Mocking a Socket.IO backend

Zorvik can run a **Socket.IO server** too (Servers → **+** → **Socket.IO server**). Point your app's socket.io-client at it, and answer its events by echo, by rules (acknowledgements, replies, broadcasts to a namespace), or by emitting by hand from the traffic panel. The lab's lobby is one of these.

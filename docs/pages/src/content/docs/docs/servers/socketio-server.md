---
title: Socket.IO servers
description: Run a Socket.IO server that socket.io-client apps connect to, over long-polling or WebSocket, and answer their events by echo, by rules with acknowledgements and broadcasts, or by hand.
sidebar:
  order: 6
---

A Socket.IO server in Zorvik is a stand-in for a real one: point your app's socket.io-client at it and watch every event, answer automatically, or emit events yourself. It speaks Socket.IO 3 and 4 (Engine.IO 4), over HTTP long-polling and WebSocket, including the upgrade from polling to WebSocket that socket.io-client makes by default.

## Create one

Open the **Servers** sidebar, click **+** (**New server**) and choose **Socket.IO server**. It listens on `127.0.0.1`, port 3003 (or the next port no other saved server uses), and greets each client with a `welcome` event. Start it, and connect:

```js
import { io } from "socket.io-client";
const socket = io("http://127.0.0.1:3003/chat");
socket.on("welcome", (info) => console.log(info));
socket.emit("hello", "world", (answer) => console.log(answer));
```

Clients may join **any namespace** (`/`, `/chat`, `/admin`, …). One client connection shows as one entry in the traffic log, however many namespaces it joins; the log names the namespace before each event (`/chat hello`).

## Answers

| Mode | In the file | What happens |
|---|---|---|
| **Echo** (default) | `echo` | Every event is emitted back to its sender with the same arguments, and acknowledged with them when the client asks (`emitWithAck`, or a callback as the last argument). |
| **Rules** | `rules` | The first matching rule answers; other events get no answer. |
| **Manual** | `manual` | Nothing is sent automatically: emit from the traffic panel. |
| **Discard** | `discard` | Events are read and dropped. |

## Rules

Each rule reads: **when a client emits** *event* **with** *arguments* → **acknowledge with** … **then emit** … **with** …, optionally **to every client of the namespace**, **after** a delay.

| Part | In the file | Description |
|---|---|---|
| Event | `event` | The event name, or `*` for any. |
| With | `match`, `pattern` | **any arguments** (default), **arguments contain**, **are exactly** or **match regex**. The pattern is checked against the arguments as JSON (`["hi",{"n":1}]`) and, when there is a single text argument, against that text (so **are exactly** `hi` matches `emit("x", "hi")`). |
| Acknowledge with | `ack` | Arguments (JSON) of the acknowledgement, when the client asks for one. Empty: an acknowledgement without arguments. |
| Then emit | `replyEvent`, `replyArgs` | An event to emit back and its arguments (JSON). Empty event: none. |
| To every client of the namespace | `broadcast` | Emit the reply to everyone who joined the namespace, the sender included. |
| After | `delayMs` | Wait before answering. |

Arguments are JSON: an array is several arguments, anything else one. Answers can use the event's values and templates:

| Placeholder | Is |
|---|---|
| `{{event.name}}` | The event name, as JSON (`"hello"`). |
| `{{event.args}}` | All arguments as a JSON array. |
| `{{event.arg0}}`, `{{event.arg1}}`, … | One argument as JSON (`null` when missing). |
| `{{$uuid}}`, `{{$randomFirstName}}`, … | [Dynamic variables](../../variables/dynamic-variables/). |
| `{{token}}` | The active environment's and the workspace's variables. |

The client's values go in after variables are filled in, so a client sending `{{token}}` gets that text back, never the variable's value. Placeholders are JSON values: write `[{{event.arg0}}, "ok"]`, not `["{{event.arg0}}"]`.

```yaml title="Example rules"
- event: join
  ack: '{"ok": true, "room": {{event.arg0}}}'
- event: say
  match: contains
  pattern: hello
  replyEvent: said
  replyArgs: '[{{event.arg0}}, "{{$isoTimestamp}}"]'
  broadcast: true
- event: "*"
  replyEvent: unknown
  replyArgs: "{{event.name}}"
```

A rule with an invalid regex is skipped, and the traffic log says so once.

## Greeting

**Event on joining** is emitted to each client that joins a namespace, with **Arguments** (JSON, templates allowed). Leave the event empty for none.

## Emitting by hand

While the server runs, the traffic panel's composer has an event name, a namespace (`/` by default) and the arguments as JSON. **Emit** sends to the client chosen in **Send to**, or to every client that joined the namespace.

**Disconnect** (in the traffic panel) disconnects the client from its namespaces and ends its connection.

## Clients and browsers

- **Path**: `/socket.io/` unless you change it (socket.io's `path` option; clients must use the same).
- **CORS**: turn on **Allow browsers on other origins** for web apps served from another origin (long-polling requests need it; WebSocket doesn't).
- **TLS**: with TLS on, clients connect to `https://`.
- Heartbeats: the server pings every 25 seconds and drops clients that don't answer within 20.
- Messages and long-polling requests up to 1 MB.
- Socket.IO 2 clients (Engine.IO 3) are refused with **Unsupported protocol version**, as a Socket.IO 4 server does.

## Saved format

```yaml title="servers/Chat.yaml"
name: Chat
kind: socketio
host: 127.0.0.1
port: 3003
socketio:
  mode: rules
  greetingEvent: welcome
  greetingArgs: '{"id": "{{$uuid}}"}'
  rules:
    - event: join
      ack: '{"ok": true}'
  cors: true
```

To talk to it from Zorvik itself, use a [Socket.IO request](../../protocols/socketio/).

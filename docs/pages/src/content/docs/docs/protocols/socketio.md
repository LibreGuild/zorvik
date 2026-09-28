---
title: Socket.IO
description: Connect to Socket.IO 3 and 4 servers over WebSocket or long-polling, emit events with acknowledgements and watch every event live.
sidebar:
  order: 4
---

A Socket.IO request connects to a [Socket.IO](https://socket.io/) server (versions 3 and 4) and joins a namespace. Every event in both directions shows in the log; you emit events from the composer, optionally asking the server to acknowledge them.

## Create a Socket.IO request

In the **Collection** sidebar, open the **New** menu (**+**) and choose **New Socket.IO client**. The request has these tabs: **Connection**, **Params**, **Headers**, **Auth**, **Settings** and **Docs**. There are no scripts; scripts run for HTTP requests.

## URL and namespace

The URL is the server and the namespace to join, as in `io("http://localhost:3000/chat")`:

| URL | Joins |
|---|---|
| `http://localhost:3000` | the main namespace `/` |
| `http://localhost:3000/chat` | `/chat` |
| `https://…`, `ws://…`, `wss://…` | the same, with TLS for `https` and `wss` |

Query parameters (in the URL or the **Params** tab) go with the handshake, like socket.io-client's `query` option; the server reads them from `socket.handshake.query`. `{{variables}}` work everywhere.

## Connection settings

| Setting | Default | What it does |
|---|---|---|
| Path | `/socket.io/` | The server's `path` option. |
| Transport | **WebSocket, else long-polling** | Connect over WebSocket; when the server refuses WebSocket (it only allows polling), use HTTP long-polling instead and say so in the log. **WebSocket only** and **HTTP long-polling only** force one. |
| Auth payload | none | JSON sent when joining the namespace, like socket.io-client's `auth` option (the server reads `socket.handshake.auth`). |

Headers from the **Headers** tab, the folder and the workspace, the **Auth** tab's header, and cookies from the cookie jar go with the handshake (and with every long-polling request).

## Connect

Press **Connect** (or <kbd>Mod</kbd>+<kbd>Enter</kbd>). The log shows **Joined /chat as …** with the socket id, and **Connected · Socket.IO over WebSocket** (or **over HTTP long-polling**). When the server's middleware refuses the connection, the error says why, for example **The server refused to let this client join /chat: Not authorized**. A server running Socket.IO 2 is named as such: Zorvik connects to Socket.IO 3 and 4.

Heartbeats are answered by themselves; when the server stops sending them, the connection ends with **The server stopped answering**.

## Emitting events

The composer has an **Event** field, an **Ack** switch, and the arguments:

| Mode | Arguments |
|---|---|
| **JSON** | JSON: an array is several arguments (`["hi", {"n": 1}]`), anything else one (`{"id": 7}`); empty is none. |
| **Text** | The text as one string argument. |
| **Binary (hex)** | One binary argument, typed as hex bytes. |

With **Ack** on, the server is asked to acknowledge: the sent row shows **wants ack #1**, and the answer arrives as **ack #1** with the acknowledgement's arguments. `{{variables}}` in the event name and arguments are replaced when you emit. Press **Emit** or <kbd>Mod</kbd>+<kbd>Enter</kbd>.

## The event log

Received events show the event name as a badge and their arguments as JSON; click a row for the pretty-printed arguments. Binary arguments are shown as `{"base64": "…", "bytes": 3}`; typing that shape in the JSON arguments sends a binary argument too. Events that ask Zorvik for an acknowledgement are marked **wants ack**. The filter box searches event names and arguments.

## In runs and for agents

Socket.IO requests are live sessions: the collection runner and the CLI skip them, and load tests don't send them.

## Saved format

```yaml title="requests/Chat.yaml"
name: Chat
kind: socketio
url: http://localhost:3000/chat?room=lobby
socketio:
  auth: '{"token": "{{token}}"}'
  event: message
  ack: true
body:
  type: json
  text: '["hello", {"from": "{{userName}}"}]'
```

| Field | Default | Description |
|---|---|---|
| `socketio.path` | `/socket.io/` | The server's path. |
| `socketio.transport` | `auto` | `auto`, `websocket` or `polling`. |
| `socketio.auth` | none | The auth payload as JSON text. |
| `socketio.event`, `socketio.ack` | none, `false` | What the composer emits. |
| `body.text`, `body.type` | | The composer's arguments and mode (`json` or `text`). |

:::tip[A server to talk to]
Zorvik runs Socket.IO servers too: echo, answer by rules with acknowledgements and broadcasts, or emit yourself. See [Socket.IO servers](../../servers/socketio-server/).
:::

---
title: Socket.IO
description: Connect to Socket.IO 3 and 4 servers over WebSocket or long-polling, emit events with acknowledgements, keep saved emits and on-connect steps, watch every event live, and test them in collection runs.
sidebar:
  order: 4
---

A Socket.IO request connects to a [Socket.IO](https://socket.io/) server (versions 3 and 4) and joins a namespace. Every event in both directions shows in the log; you emit events from the composer, optionally asking the server to acknowledge them. Events you emit often can be saved with the request, and **On connect** steps can join a room, wait for the server's event and keep emitting every time you connect, which also makes the request a test in collection runs.

## Create a Socket.IO request

In the **Collection** sidebar, open the **New** menu (**+**) and choose **New Socket.IO client**. The request has these tabs: **Connection**, **Emits**, **On connect**, **Params**, **Headers**, **Auth**, **Settings** and **Docs**; **Emits** and **On connect** show how many they hold. There is no **Scripts** tab: in collection runs, the request's tests are its [wait steps](#in-collection-runs).

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
| Auth payload (JSON) | none | JSON sent when joining the namespace, like socket.io-client's `auth` option (the server reads `socket.handshake.auth`). |
| Answer acknowledgements (JSON) | none | When the server emits an event that asks for an acknowledgement, answer it right away with these arguments: an array is several arguments (`["ok", {"id": 7}]`), anything else one. Empty: don't answer. |

Headers from the **Headers** tab, the folder and the workspace, the **Auth** tab's header, and cookies from the cookie jar go with the handshake (and with every long-polling request). `{{variables}}` work in the auth payload and the acknowledgement arguments. Changes to these settings apply the next time you connect.

### Answer acknowledgements

Some servers emit events with a callback, as in `socket.emit("ping", data, (reply) => …)`, and wait for the client to call it. With **Answer acknowledgements** filled in, Zorvik answers each such event at once with those arguments. The answer shows in the log as a sent row marked **ack #3**, below the event marked **wants ack #3**. Without it, those events stay unanswered and the server's callback never runs.

## Connect

Press **Connect** (or <kbd>Mod</kbd>+<kbd>Enter</kbd>). The log shows **Joined /chat as …** with the socket id, and **Connected · Socket.IO over WebSocket** (or **over HTTP long-polling**). Then the request's [On connect](#on-connect) steps run. When the server's middleware refuses the connection, the error says why, for example **The server refused to let this client join /chat: Not authorized**. A server running Socket.IO 2 is named as such: Zorvik connects to Socket.IO 3 and 4.

Heartbeats are answered by themselves; when the server stops sending them, the connection ends with **The server stopped answering**.

## Emitting events

The composer has an **Event** field, an **Ack** checkbox, and the arguments:

| Mode | Arguments |
|---|---|
| **JSON** | JSON: an array is several arguments (`["hi", {"n": 1}]`), anything else one (`{"id": 7}`); empty is none. |
| **Text** | The text as one string argument. |
| **Binary** | One binary argument, typed as hex bytes. **File** and **Microphone** emit the event once per piece, with the piece as its one binary argument (see [WebSocket](../websocket/#binary-stream-a-file)). |

With **Ack** on, the server is asked to acknowledge: the sent row shows **wants ack #1**, and the answer arrives as **ack #1** with the acknowledgement's arguments and a green chip that says how long the server took (**↩ 12 ms**). Click the chip to jump to the emit it answers. `{{variables}}` in the event name and arguments are replaced when you emit. Press **Emit** or <kbd>Mod</kbd>+<kbd>Enter</kbd>.

The bookmark button at the left of the composer (**Saved emits**) lists the request's [saved emits](#saved-emits): click one to put it in the composer, with its event, **Ack** and arguments. **Save as emit…** saves what is in the composer under a name (the event's name unless you change it), and **Edit saved emits** opens the **Emits** tab.

### Emit repeatedly

The arrow next to **Emit** (**More ways to send**) emits what is in the composer again and again:

| Choice | Does |
|---|---|
| **Emit every second**, **Emit every 5 seconds**, **Emit every 30 seconds** | Emits the event now and then again at that pace. |
| **Emit every…** | The same, at a pace you type in milliseconds (10 or more). |

While it repeats, the composer shows **Emitting every 5 s** and a **Stop** button. Variables are filled in fresh for each emit, so `[{"price": {{$randomPrice(5, 50)}}}]` sends a new price every time. Repeating runs in Zorvik itself, so it keeps its pace when the window is in the background; it stops when you press **Stop** or the connection closes.

## Saved emits

The **Emits** tab keeps events with the request, so you don't type them again: joining a room, a subscription, a typing notice. Each has:

| Part | Description |
|---|---|
| Name | How the composer menu and the **On connect** steps refer to it. Renaming an emit updates the steps that use it. |
| Event | The event to emit, for example `join`. An emit without one is marked. |
| **Ack** | Ask the server to acknowledge it. |
| **Text**, **JSON** or **Hex** | How the arguments are written: **Text** is one string argument, **JSON** the arguments (an array for several, anything else one), **Hex** one binary argument written as hex bytes (`48 65 6c 6c 6f`). |
| The arguments | `{{variables}}` are filled in each time it is emitted (not in hex). |

The buttons on each emit: **Put in the composer**, **Send now** (while connected) and **Remove message**. **Add emit** adds an empty one. **Save as emit…** in the composer's **Saved emits** menu does the same from what you typed; a name that is already taken replaces that emit. Saved emits are part of the request: save the request to keep them.

## On connect

The **On connect** tab holds steps that run in order right after every connect: join rooms, wait for the server's event, keep emitting a heartbeat. **Add step** offers three kinds:

| Step | What it does |
|---|---|
| **Send** a saved emit | Emits it. |
| **Wait for** the event … with arguments that … **within** … ms | Waits until the server emits that event with arguments that match. The time limit is 5,000 ms unless you change it (at most 10 minutes). |
| **Keep sending** a saved emit **every** … ms | Emits it now and then at that pace (10 ms or more) until the connection closes. |

Use the arrows to move a step up or down; the bin removes it. A step that names an emit the request doesn't have is marked.

```text title="Join the lobby, wait until the server confirms, then keep a typing notice going"
1  Send          Join
2  Wait for      the event  joined  with arguments that  JSON where  $[0].room == lobby   within 5000 ms
3  Keep sending  Typing  every 5000 ms
```

**What a wait matches.** A wait names an event, and its condition is checked against the event's arguments, written as a JSON array: an event emitted as `socket.emit("joined", {room: "lobby", users: 3})` has the arguments `[{"room":"lobby","users":3}]`.

| With arguments that | Matches when |
|---|---|
| **contains** | The arguments contain the text, for example `lobby`. Empty: the event with any arguments. |
| **is exactly** | The arguments are exactly the text, for example `["ok"]`. |
| **matches regex** | The regular expression matches anywhere in the arguments (Rust `regex` syntax). |
| **JSON where** | The arguments meet the [JSON condition](../websocket/#json-conditions), with `$[0]` the first argument: `$[0].room == lobby`, `$[1].ok == true`. |
| **any message** | The event arrived, whatever its arguments. |

Leave the event empty to wait for any received event or acknowledgement whose arguments match. Like WebSocket waits, a wait looks at what arrived since the last step that emitted something, so an event that came before the wait started still counts (see [WebSocket: On connect](../websocket/#on-connect)).

The steps report in the event log as notes: **On connect: sent Join**, **On connect: got the event "joined" with arguments where $[0].room == lobby after 12 ms**, **On connect: sending Typing every 5 s**. The first step that fails shows in red, for example **On connect: waited for the event "joined": Nothing matched within 5000 ms**, and the steps after it don't run. The connection stays open, so you can see what the server sent and carry on by hand.

## The event log

Received events show the event name as a badge and their arguments as JSON; click a row for the pretty-printed arguments. Binary arguments are shown as `{"base64": "…", "bytes": 3}`; typing that shape in the JSON arguments sends a binary argument too. Events that ask for an acknowledgement are marked **wants ack**, and acknowledgements **ack** with the same number. The filter box searches event names and arguments; the filter's JSON conditions, the **View** menu and **Export the log…** work as for [WebSocket](../websocket/#the-message-log).

### Event chips

Above the log, a chip for each event name says how often it came up, sent and received, the most frequent first:

- **Click** a chip to show only that event (notes and acknowledgements are hidden too); click it again to show everything.
- <kbd>Mod</kbd>-click a chip to hide that event, for example a noisy `heartbeat` or `price` event. Hidden events are struck through; <kbd>Mod</kbd>-click again to show them.
- **Show all** brings back every event.

The chips work with the filter box: a chip narrows the log to an event, the filter to what is inside it.

## In collection runs

Socket.IO requests run in the Runner tab, in [`zorvik run`](../../cli/run/) and in runs an AI agent starts. Zorvik connects, runs the [On connect](#on-connect) steps, and disconnects when they are done. This works as for [WebSocket requests in runs](../websocket/#in-collection-runs):

- Each **Wait for** step is a **test**, named after what it waits for: `Wait for the event "joined" with arguments where $[0].room == lobby`, `Wait for the event "joined"`. A wait that fails says why, for example `Nothing matched within 5000 ms`. After the first failure, the remaining waits are reported as **skipped**.
- A **Send** step that fails (an emit that isn't saved or has no event, hex that isn't valid) is a failed test named `Send Join`.
- **Keep sending** steps don't run: the connection closes when the steps end.
- **Answer acknowledgements** works in runs too.
- A request without wait steps passes when it connects. One that can't connect (a refused namespace, a timeout) fails with the error.
- The result's status is the handshake's (`101` over WebSocket, `200` over long-polling), its time is the whole conversation, and its response body is the conversation as JSON, with each event's name in `event`. Workspace and folder post-response scripts read it with `pm.response.json()`:

```json
[
  { "direction": "sent", "event": "join", "text": "[\"lobby\"]", "size": 9, "timestamp": 1790000000131.2 },
  { "direction": "received", "event": "joined", "text": "[{\"room\":\"lobby\",\"users\":3}]", "size": 27, "timestamp": 1790000000143.5 }
]
```

In `zorvik run` the request's method shows as `IO`:

```text
✓ IO      Chat  http://localhost:3000/chat  101 96 ms, 248 B
    ✓ Wait for the event "joined" with arguments where $[0].room == lobby
```

Load tests don't send Socket.IO requests.

## AI agents

Coding agents connected through `zorvik mcp` can open Socket.IO connections: `open_connection` connects (running the request's On connect steps), then `send_message` emits (with `event`, and `ack` to ask for an acknowledgement), `wait_for_message` waits for an event (`event`, with the pattern matched against its arguments), `read_messages` reads the log (each message with its `event`) and `close_connection` disconnects. While you follow agents, the connection shows live in a tab. See [Tools](../../agents/tools/#live-connections).

## Saved format

```yaml title="requests/Chat.yaml"
name: Chat
kind: socketio
url: http://localhost:3000/chat?room=lobby
socketio:
  auth: '{"token": "{{token}}"}'
  autoAck: '["ok"]'
  messages:
    - name: Join
      event: join
      text: lobby
      ack: true
    - name: Typing
      event: typing
      format: json
      text: '[{"user": "{{userName}}"}]'
  onConnect:
    - type: send
      message: Join
    - type: wait
      event: joined
      match: json
      pattern: $[0].room == lobby
    - type: repeat
      message: Typing
      everyMs: 5000
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
| `socketio.autoAck` | none | **Answer acknowledgements**: the arguments as JSON text. |
| `socketio.messages` | none | Saved emits: `name`, `event`, `ack` (default `false`), `format` (`text` (default) for one string argument, `json` for the arguments, or `binary` for hex bytes) and `text`. |
| `socketio.onConnect` | none | Steps, in order. `type: send` with `message` (a saved emit's name); `type: wait` with `event`, `match` (`any`, `contains` (default), `exact`, `regex` or `json`), `pattern` and `timeoutMs` (default `5000`); `type: repeat` with `message` and `everyMs`. |
| `socketio.event`, `socketio.ack` | none, `false` | What the composer emits. |
| `body.text`, `body.type` | | The composer's arguments and mode (`json` or `text`). |

:::tip[A server to talk to]
Zorvik runs Socket.IO servers too: echo, answer by rules with acknowledgements and broadcasts, emit scheduled events, or emit yourself. See [Socket.IO servers](../../servers/socketio-server/).
:::

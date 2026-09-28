---
title: WebSocket & SSE servers
description: Run a WebSocket server that echoes, answers by rules or lets you type the replies, and an event stream server that plays a list of events.
sidebar:
  order: 5
---

Zorvik runs two kinds of streaming servers for your clients to connect to: a **WebSocket server** and an **event stream (SSE) server**. Both keep a list of connected clients, show every message in the traffic log, and let you send to one client or all of them by hand.

## WebSocket server

Open the **Servers** sidebar, click **+** (**New server**) and choose **WebSocket server**. It listens on `127.0.0.1`, port 3001 (or the next port no other saved server uses). Start it, and connect a client to its address on **any path**: `ws://127.0.0.1:3001/`, `ws://127.0.0.1:3001/chat?room=1` and so on all work.

### Replies

The **Replies** setting decides what happens with each incoming message:

| Mode | In the file | What happens |
|---|---|---|
| **Echo** (default) | `echo` | Every message is sent back. Text comes back as text, binary as binary. |
| **Rules** | `rules` | The first matching [reply rule](#reply-rules) answers; other messages get no reply. |
| **Manual** | `manual` | Nothing is sent automatically: reply from the traffic panel. |
| **Discard** | `discard` | Messages are read and dropped. |

In **Rules** mode, text and binary messages are both matched (as text), and replies are always sent as text messages.

### Greeting

**Greeting** is sent to each client right after it connects, as a text message. Leave it empty for none. It may use variables and dynamic values (`{{$uuid}}`, `{{$timestamp}}`), not request values:

```json
{"type": "welcome", "session": "{{$uuid}}"}
```

### Connections

- The upgrade must arrive within 10 seconds. A client that isn't a WebSocket client is logged, for example **Not a WebSocket client: the request has no WebSocket upgrade headers (plain HTTP?)**.
- Messages and frames may be up to 16 MB. A larger message closes the connection (**message of … bytes is larger than the limit**).
- Pings are answered automatically.
- Subprotocols are not negotiated: the server never answers with a `Sec-WebSocket-Protocol` header. Clients that ask for a subprotocol may refuse such a connection (Chrome does when you pass protocols to `new WebSocket(url, protocols)`), so connect without one.
- Every `Origin` is accepted.
- With **TLS** on, the address is `wss://`.

**Disconnect** (in the traffic panel) closes a client with code `1000`; stopping the server closes every client with `1001` and the reason `server stopped`. The log records why each connection ended, for example **closed by the client (1001 going away)** or **the client dropped the connection without a close frame**.

## Reply rules

WebSocket, TCP and UDP servers share the same reply rules: "when a message matches, reply with …". Choose **Rules** in **Replies**, then **Add rule**.

| Column | Description |
|---|---|
| Checkbox | Turns the rule on or off. |
| **When** | **Contains**, **Is exactly**, **Matches regex** or **Any message**. |
| **Message** | The pattern (not used for **Any message**). |
| **Reply** | What to send back. May use `{{message}}`, dynamic values and variables (text encoding only). |
| **Delay ms** | Wait this long before replying. |

The rules are checked **from top to bottom**; the **first** enabled rule that matches answers, and a message no rule matches gets no reply.

| When | In the file | Matches when |
|---|---|---|
| **Contains** (default) | `contains` | The message contains the pattern (case-sensitive). An empty pattern matches every message. |
| **Is exactly** | `exact` | The message equals the pattern. Line breaks at the end of either are ignored, so `PING` matches `PING\r\n`. |
| **Matches regex** | `regex` | The regular expression matches anywhere in the message. Use `^` and `$` to anchor it. The syntax is Rust's `regex` crate (like RE2: no look-around or backreferences). |
| **Any message** | `any` | Always. Put it last as a catch-all. |

A rule that can't be used (an invalid regex, or invalid hex on TCP and UDP) is skipped, and the traffic log shows why once, for example **Rule 3: invalid regex: …**.

```text title="Example rules"
Is exactly     PING              →  PONG
Matches regex  ^subscribe (\w+)  →  {"subscribed": "{{message}}", "id": "{{$uuid}}"}   (delay 200)
Contains       error             →  {"type": "error", "code": 500}
Any message                      →  {"type": "ack"}
```

`{{message}}` is the whole message (trailing line breaks removed). Regex groups can't be used in the reply. See [Templates](../templates/#message).

## Event stream (SSE) server

Choose **Event stream (SSE) server** in the **New server** menu. It listens on `127.0.0.1`, port 3002 (or the next free one), and starts with one event, `tick` with the data `{"n": 1}`, sent every second on repeat.

Any `GET` request, on **any path**, opens a stream: `http://127.0.0.1:3002/events`. Other methods get `405 Method Not Allowed` with `Allow: GET` and the text **Event streams are opened with GET**.

Each stream is answered with:

```http
HTTP/1.1 200 OK
Content-Type: text/event-stream
Cache-Control: no-cache
Access-Control-Allow-Origin: *
X-Accel-Buffering: no
```

`X-Accel-Buffering: no` keeps proxies such as nginx from holding events back.

### Events

The **Events** list holds the events every client gets, in order. Each has:

| Field | Description |
|---|---|
| **Event name** | The `event:` field. Empty: no `event:` line, so clients see the default `message`. Not templated. |
| **Id** | The `id:` field (optional). May use templates. |
| **Data** | The data. Each line becomes its own `data:` line. May use templates. |

Drag events by their handle or use the arrows to reorder them; the bin removes one.

Data and id may use [templates](../templates/): the request values of the `GET` that opened the stream (`{{request.query.user}}`, `{{request.headers.authorization}}`, `{{request.path}}`, …), variables and dynamic values. Each client gets its own rendering, so `{{request.query.user}}` greets each client by the name in its URL.

The event `tick` with id `7` and data `{"n": 1}` goes out as:

```text
id: 7
event: tick
data: {"n": 1}

```

Line breaks in names and ids are replaced by spaces.

### Pace

| Setting | Default for a new server | Description |
|---|---|---|
| **Interval (ms)** | 1000 | Pause between events. `0` sends them all at once. |
| **Repeat** | on | Start over after the last event. With an interval of `0`, one round per second. |

Without **Repeat**, a stream stays open after the last event, and only gets what you send from the traffic panel. A comment line (`:keepalive`) is sent every 15 seconds so idle streams (and proxies) stay open. With no events at all, streams stay open and only get what you send.

Edits apply to the next event, also on streams already open.

### Reconnecting clients

A client that reconnects with a `Last-Event-ID` header (as `EventSource` does) continues **after** the event with that id. This works with fixed ids: ids are compared as written, so an id with a placeholder such as `{{$uuid}}` never matches and the client starts from the first event.

### Streams and clients

- Up to 64 events wait for a slow client; after that, sending waits until it reads.
- The server speaks HTTP/1.1 and HTTP/2; with **TLS** on, the address is `https://`.
- It has no CORS switch: it always allows every origin with `*` and doesn't answer `OPTIONS` preflights, which `EventSource` doesn't need.

## Sending from the traffic panel

While the server runs, the composer under the traffic log sends to connected clients:

- **Send to**: **All clients (N)**, or one connection (`#3 127.0.0.1:53422`). The send icon on a log row picks that client.
- WebSocket: **Text** or **Hex** (sent as a binary message, for example `48 65 6c 6c 6f`). `{{variables}}` in text are filled in before sending.
- Event stream: an optional **event name** and the **event data**. The data is rendered on the server for each client, so request values work too. Binary data can't be sent on an event stream.
- Press **Send** or <kbd>Mod</kbd>+<kbd>Enter</kbd>.

A client that stops reading gets at most 256 queued messages; then sends to it fail with **Not sent: client #3 is not reading (256 messages are still waiting)**. The unplug icon on a row (**Disconnect**) closes that client.

See [Running servers](../running-servers/#traffic-log) for the traffic log itself.

## Saved format

```yaml title="servers/Chat echo.yaml"
name: Chat echo
kind: websocket
seq: 0
host: 127.0.0.1
port: 3001
websocket:
  mode: rules
  greeting: '{"type": "welcome", "session": "{{$uuid}}"}'
  rules:
    - match: exact
      pattern: ping
      reply: pong
    - match: regex
      pattern: ^subscribe
      reply: '{"subscribed": "{{message}}"}'
      delayMs: 200
    - match: any
      reply: '{"type": "ack"}'
```

```yaml title="servers/Ticker.yaml"
name: Ticker
kind: sse
seq: 0
host: 127.0.0.1
port: 3002
sse:
  events:
    - event: hello
      data: '{"user": "{{request.query.user}}"}'
      id: "1"
    - event: tick
      data: '{"at": "{{$isoTimestamp}}"}'
      id: "2"
  intervalMs: 1000
  repeat: true
```

| Field | Default | Description |
|---|---|---|
| `websocket.mode` | `echo` | `echo`, `rules`, `manual` or `discard`. |
| `websocket.greeting` | empty | Sent to each client on connect. |
| `websocket.rules` | none | Reply rules (below). |
| `sse.events` | none | `event` (empty: `message`), `data`, `id`. |
| `sse.intervalMs` | `0` | Pause between events in ms. |
| `sse.repeat` | `false` | Start over after the last event. |

Reply rules (`rules[]`, also used by TCP and UDP servers):

| Field | Default | Description |
|---|---|---|
| `match` | `contains` | `contains`, `exact`, `regex` or `any`. |
| `pattern` | empty | The pattern. |
| `reply` | empty | The reply. |
| `delayMs` | `0` | Delay before replying, in ms. |
| `enabled` | `true` | Whether the rule is used. |

To talk to these servers from Zorvik, use a [WebSocket request](../../protocols/websocket/) or an [event stream request](../../protocols/sse/).

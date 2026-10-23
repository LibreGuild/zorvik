---
title: WebSocket
description: Connect to WebSocket servers, send text, JSON or binary messages, keep saved messages and on-connect steps, stream files and the microphone, watch the conversation live, and test it in collection runs.
sidebar:
  order: 3
---

A WebSocket request opens a lasting connection and shows every message in both directions. You connect once, then send as many messages as you like from the composer. Messages you send often can be saved with the request, and **On connect** steps can log in, subscribe and wait for the server's answer every time you connect, which also makes the request a test in collection runs.

## Create a WebSocket request

In the **Collection** sidebar, open the **New** menu (**+**) and choose **New WebSocket**. The request has these tabs: **Messages**, **On connect**, **Params**, **Headers**, **Auth**, **Settings**, **Scripts** and **Docs**; **Messages** and **On connect** show how many they hold. There is no **Body** tab: messages are typed in the composer below the message log. **Scripts** has a script that runs before connecting and [message scripts](#message-scripts) that run on the messages that match.

## URL

| URL | Connection |
|---|---|
| `ws://host[:port]/path` | Plain WebSocket (port 80 by default). |
| `wss://host[:port]/path` | WebSocket over TLS (port 443 by default). |
| `http://…`, `https://…` | Treated as `ws://` and `wss://`. |
| `host/path` | No scheme means `ws://`. |

Anything after `#` is dropped. `{{variables}}` and query parameters work as in HTTP requests.

## Connect and disconnect

Press **Connect** (or <kbd>Mod</kbd>+<kbd>Enter</kbd>). The log shows **Connecting to …**, then **Connected · 101 Switching Protocols · 38 ms**, or the error. When the server agreed to a subprotocol or extensions, they follow: **Connected · 101 Switching Protocols · 38 ms · protocol chat.v1**. Then the request's [On connect](#on-connect) steps run.

**Disconnect** sends a close frame with code `1000`; when the server doesn't answer it within 5 seconds, the connection is dropped anyway. To close with another code, open the **⋯** menu at the end of the URL bar and choose **Close with a code…** (shown while connected). Type the code, then an optional reason, for example `4001 Session expired`. The code is `1000` or one from `3000` to `4999` (`4000` to `4999` are yours to define), and the reason at most 123 bytes, as the protocol allows.

The status next to the log reads **Not connected**, **Connecting…**, **Connected** or **Disconnected**, with counts of sent and received messages. When the server closes the connection, the log shows it with the close code, the reason and what the code means, for example **Disconnected by server (code 1008 · Policy violation)** or **Disconnected by server (code 4001 · Session expired · Defined by the application)**. See [Close codes](#close-codes).

### The upgrade request

Connecting sends an HTTP `GET` upgrade request. Its headers are:

| Header | Value |
|---|---|
| `Host` | The URL's host (and port), or your own `Host` header. |
| `Connection`, `Upgrade` | `Upgrade`, `websocket`. |
| `Sec-WebSocket-Version`, `Sec-WebSocket-Key` | `13` and a fresh key. |
| `Sec-WebSocket-Protocol` | The **Subprotocols** setting, unless you set this header yourself. |
| Your headers | Everything from the **Headers** tab, the folder and workspace headers, and the **Auth** header. `Connection`, `Upgrade`, `Sec-WebSocket-Version`, `Sec-WebSocket-Key` and `Content-Length` from your headers are ignored. |
| `User-Agent` | Zorvik's, unless you set one or default headers are off in Settings. |
| `Cookie` | Cookies from the cookie jar for this host (unless you set a `Cookie` header). `Set-Cookie` on the upgrade response is saved to the jar. |

To send an `Origin`, add an `Origin` header.

### Subprotocols

To ask for a **subprotocol**, open the **Settings** tab and fill in **Subprotocols**: one or more names, comma-separated, the one you prefer first (for example `graphql-transport-ws` or `chat.v2, chat.v1`). They are sent as `Sec-WebSocket-Protocol` and may use `{{variables}}`. The one the server picks shows in the **Connected** line. A `Sec-WebSocket-Protocol` header of your own (in **Headers**) wins over the list.

## Sending messages

The composer at the bottom has three modes:

| Mode | Sends | Notes |
|---|---|---|
| **Text** | A text frame | `{{variables}}` are replaced when you send. |
| **JSON** | A text frame | Like Text, with JSON highlighting. |
| **Binary** | Binary frames | From **Hex** you type, a **File**, or the **Microphone**. See below. |

Press **Send** or <kbd>Mod</kbd>+<kbd>Enter</kbd>. Send is available while connected and the composer is not empty. The composer text is saved with the request, so it is still there next time.

The bookmark button at the left of the composer (**Saved messages**) lists the request's [saved messages](#saved-messages): click one to put it in the composer, in its format. **Save as message…** saves what is in the composer under a name, and **Edit saved messages** opens the **Messages** tab.

### Send repeatedly, or a ping

The arrow next to **Send** (**More ways to send**) has more ways to send what is in the composer:

| Choice | Does |
|---|---|
| **Send every second**, **Send every 5 seconds**, **Send every 30 seconds** | Sends the message now and then again at that pace. |
| **Send every…** | The same, at a pace you type in milliseconds (10 or more). |
| **Send a ping** | Sends a ping frame. The server's **pong** shows in the log. |

While a message repeats, the composer shows **Sending every 5 s** and a **Stop** button. Variables are filled in fresh for each send, so `{"at": {{$timestamp}}, "n": {{$randomInt(1, 9)}}}` sends new values every time. Repeating runs in Zorvik itself, so it keeps its pace when the window is in the background; it stops when you press **Stop** or the connection closes.

### Binary: hex

Type hex bytes such as `48 65 6c 6c 6f`, `48:65:6c` or `0x48,0x65`, and press **Send**. Variables are not replaced.

### Binary: stream a file

**File** sends a file in pieces, one binary message at a time, at the pace you set. Use it to play an audio or video file to a speech or media server, replay a capture, or see how a server copes with a steady upload.

1. Choose **Binary**, then **File**, and press **Choose file…**. Any file works.
2. Set **Send _N_ bytes every _M_ ms**. Below it, Zorvik shows the rate, the number of messages and how long one pass takes.
3. Optionally send only part of the file: **From byte** skips that many bytes (a header, say), and **for _N_ bytes** stops after that many (empty means to the end).
4. Turn on **Loop until stopped** to start over from the same byte after the last message.
5. Press **Start streaming**. A progress bar shows what was sent; **Stop** ends the stream at any time.

**Presets** fills in common paces, such as 1 KB every 100 ms or 1 MB every second.

When the file is a **WAV** file, Zorvik reads its format (shown next to the name, for example **WAV · PCM 16-bit · 16 kHz · mono**) and sets it up to play in real time: 20 ms of samples per message, every 20 ms, with the header skipped so only the samples are sent. **Presets** offers other real-time message lengths (10 to 100 ms), and **Samples only** puts the header back to skipped after you changed the range.

Streaming runs in Zorvik itself, so it keeps its pace when the window is in the background. When the server takes the data more slowly than that pace, the stream waits for it instead of piling messages up, so the progress bar shows the pace the server really gets. It stops when you disconnect, when the connection closes, when the connection reports an error, or when you close the tab. Each message also appears in the log (what was sent shows its first 16 KB; the size is the whole message's).

### Binary: stream the microphone

**Microphone** records from a microphone and sends the audio as it comes, in binary messages. Use it to talk to a speech recognition or voice bot server live.

| Setting | Choices |
|---|---|
| **Microphone** | The system default, or a microphone by name (names show once access is allowed). |
| **Format** | **PCM 16-bit** (`s16le`), **PCM 32-bit float** (`f32le`), **G.711 μ-law** or **A-law** (raw samples), or **Opus** (WebM or Ogg) and **AAC** (MP4), compressed by the system. Compressed formats your system can't record are marked as not available. |
| **Sample rate** | Raw formats: 8 to 48 kHz. Zorvik converts from the microphone's rate. |
| **Bitrate** | Compressed formats: 16 to 128 kbps. |
| **Every … ms** | How much audio each message carries, from 10 ms to 1 second. |
| **Mono** / **Stereo** | Raw formats. Stereo samples are interleaved. |
| **WAV header first** | Raw formats: a 44-byte WAV header goes out before the samples, so the stream plays as a `.wav` file. Its sizes are set to "unknown". |
| **Echo cancellation**, **Noise suppression**, **Auto gain** | The system's voice processing. Turn them off to send the microphone's signal as it is. |

The line under the settings says exactly what goes out, for example **640 B every 20 ms (31.3 KB/s): PCM 16-bit (s16le), 16 kHz, mono.** Raw messages are always whole, the same size each; the last one, when you stop, may be shorter. With a compressed format, the first message carries the container header, and later messages only play after it.

Press **Start recording**. The first time, your system asks whether Zorvik may use the microphone. While recording, a level meter, the time and the bytes sent are shown. **Stop recording** sends what is left and stops. Recording also stops when the connection closes.

:::note
Microphone access is controlled by your system: on macOS in **System Settings → Privacy & Security → Microphone**, on Windows in **Settings → Privacy & security → Microphone**.
:::

## Saved messages

The **Messages** tab keeps messages with the request, so you don't type them again: a login, a subscription, a ping your server understands. Each has:

| Part | Description |
|---|---|
| Name | How the composer menu and the **On connect** steps refer to it. Renaming a message updates the steps that use it. |
| **Text**, **JSON** or **Hex** | How it is sent: a text message (JSON is text with JSON highlighting), or a binary message written as hex bytes (`48 65 6c 6c 6f`). |
| The message | `{{variables}}` are filled in each time it is sent (not in hex). |

The buttons on each message: **Put in the composer**, **Send now** (while connected) and **Remove message**. **Add message** adds an empty one. You can also type a message in the composer and choose **Save as message…** from the **Saved messages** menu; a name that is already taken replaces that message. For a saved request, **Save as message…** writes the message to the request's file at once (other unsaved changes stay unsaved), so teammates and [AI agents](#ai-agents) see it; messages added in the **Messages** tab are kept when you save the request.

**Match replies by** (below the list) is the JSON path that pairs replies with the messages they answer, `$.id` when empty. See [Reply times](#reply-times).

## On connect

The **On connect** tab holds steps that run in order right after every connect: log in, subscribe, wait for the server's answer, keep a heartbeat going. **Add step** offers three kinds:

| Step | What it does |
|---|---|
| **Send** a saved message | Sends that message. |
| **Wait for** a message that … **within** … ms | Waits until a received message matches: one that **contains** a text, **is exactly** a text, **matches regex**, matches a **JSON where** condition, or **any message**. The time limit is 5,000 ms unless you change it (at most 10 minutes). |
| **Keep sending** a saved message **every** … ms | Sends it now and then at that pace (10 ms or more) until the connection closes: a heartbeat. |

Use the arrows to move a step up or down; the bin removes it. A step that names a message the request doesn't have is marked.

```text title="Log in, wait for the welcome, then keep a heartbeat going"
1  Send         Login
2  Wait for     a message that  JSON where  $.type == welcome   within 5000 ms
3  Send         Subscribe
4  Wait for     a message that  contains    subscribed          within 5000 ms
5  Keep sending Heartbeat  every 30000 ms
```

**Which messages a wait sees.** A wait looks at the messages received since the last step that sent something, so a reply that arrived before the wait started still counts. A wait before any send also sees the server's greeting. A wait after another wait looks after the message the first one matched. Pings and pongs are never matched; binary messages are matched by their bytes.

| Wait for a message that | Matches when |
|---|---|
| **contains** | The message contains the text (case-sensitive). Empty: any message. |
| **is exactly** | The message equals the text. A line break at the end of either is ignored. |
| **matches regex** | The regular expression matches anywhere in the message (Rust `regex` syntax: no look-around or backreferences). |
| **JSON where** | The message is JSON and meets the [condition](#json-conditions), for example `$.type == welcome`. |
| **any message** | Always. |

The steps report in the message log as notes: **On connect: sent Login**, **On connect: got a JSON message where $.type == welcome after 34 ms**, **On connect: sending Heartbeat every 30 s**. The first step that fails (nothing matched in time, a message that isn't saved, the connection closed) shows in red, for example **On connect: waited for a message containing "welcome": Nothing matched within 5000 ms**, and the steps after it don't run. The connection stays open, so you can see what the server sent and carry on by hand.

## The message log

Each row shows the direction (sent or received), the time, a **binary**, **ping** or **pong** badge where it applies, the start of the payload and its size. Notes such as **Connected …** and the [On connect](#on-connect) steps' progress appear between the messages. Click a row to see the whole payload: JSON is pretty-printed and binary data is shown as hex. A string that holds JSON itself (a payload sent as `JSON.stringify(…)` inside a message) is shown as JSON too. Notes and errors cut short in the row open the same way: click a connection error to read all of it. The buttons at its top right:

| Button | Does |
|---|---|
| **Pretty** / **Raw** | JSON messages: pretty-printed (the default), or the text exactly as it came. The choice is remembered. |
| **Copy** | Copies the payload. |
| **Save to a file** | Binary messages: saves the bytes to a file you choose. |
| **Edit in the composer** | Sent messages: puts the message back in the composer, as text, JSON or hex, to change it and send it again. |

Pings from the server are answered automatically, and show up in the log. The bin icon (**Clear messages**) clears the log.

While you are scrolled up, new messages don't move the log; a **N new messages** button jumps back to the newest.

### Filter

The filter box above the log shows only the messages that match:

- **Words** find messages that contain them, in any case: `error`, `user-42`.
- A **JSON condition**, when the filter starts with `$`, shows the JSON messages that meet it: `$.type == trade`, `$.price`, `$.status != ok`. See [JSON conditions](#json-conditions).

While a filter is on, the status line adds how many messages are shown (**12 shown**).

### View

The **View** menu (the sliders icon next to the filter) has:

| Choice | Does |
|---|---|
| **All messages**, **Sent only**, **Received only** | Which direction to show. |
| **Time of day** | The clock time of each message (the default). |
| **Time since the previous message** | `+250 ms`: the gap before each message, to see a server's pace. |
| **Time since the first message** | `+3.2 s`: time since the log started. |
| **Export the log…** | Saves every message (not only the shown ones) as a `.jsonl` file. |

The time setting is remembered for every log.

### Reply times

When a server answers each JSON message with one carrying the same id, Zorvik pairs them: a received message whose value at the reply key equals that of a message you sent earlier is shown as its reply, with a green chip that says how long it took (**↩ 34 ms**, or **<1 ms**). Click the chip to jump to the message it answers. Each sent message is paired with its first reply.

The reply key is `$.id` unless you set **Match replies by** in the **Messages** tab, for example `$.request_id` or `$.meta.correlationId`.

### Export

**Export the log…** writes one JSON object per line, oldest first:

```json title="Chat.jsonl"
{"time":"2026-10-19T09:15:42.120Z","direction":"info","kind":"info","text":"Connected · 101 Switching Protocols · 38 ms","size":0}
{"time":"2026-10-19T09:15:42.131Z","direction":"sent","kind":"text","text":"{\"id\": 7, \"op\": \"hello\"}","size":24}
{"time":"2026-10-19T09:15:42.165Z","direction":"received","kind":"text","text":"{\"id\": 7, \"ok\": true}","size":21}
```

| Field | Description |
|---|---|
| `time` | When, in ISO 8601 (UTC). |
| `direction` | `sent`, `received`, `info` (a note) or `error`. |
| `kind` | `text`, `binary`, `ping`, `pong`, … |
| `text` or `base64` | The payload: text, or binary data as base64. |
| `size` | Payload size in bytes. |
| `topic`, `peer`, `detail`, `id` | When the message has them: an MQTT topic, a UDP peer, details such as QoS, an SSE event id. |

The filter, the **View** menu, the export and the **N new messages** button work the same in the logs of Socket.IO, SSE, TCP, UDP and MQTT requests and of GraphQL subscriptions (without **Sent only** and **Received only** where nothing is sent). Reply times are for WebSocket requests.

### Close codes

When a connection closes with a code, the log says what it means:

| Code | Meaning |
|---|---|
| `1000` | Normal closure |
| `1001` | Going away (the server is shutting down, or the page left) |
| `1002` | Protocol error |
| `1003` | Unsupported data (e.g. binary where only text is accepted) |
| `1005` | No close code was given |
| `1006` | The connection dropped without a close frame |
| `1007` | Invalid data (e.g. text that is not UTF-8) |
| `1008` | Policy violation |
| `1009` | Message too big |
| `1010` | A required extension was not agreed |
| `1011` | Internal server error |
| `1012` | Service restart |
| `1013` | Try again later |
| `1014` | Bad gateway |
| `1015` | TLS handshake failed |
| `3000`–`3999` | Registered by a library or framework |
| `4000`–`4999` | Defined by the application |

## JSON conditions

**JSON where** waits, the log filter, [server reply rules](../../servers/websocket-and-sse-servers/#reply-rules) and AI agents' waits share one small syntax for "a JSON message where …":

| Condition | Matches a JSON message where |
|---|---|
| `$.path` | the value is there and isn't `null` |
| `$.path == value` | the value equals `value` |
| `$.path != value` | the value is anything else, or isn't there |

- **Paths**: `$.type`, `$.data.user.id`, `$.items[0]` (the first item), `$.items[-1]` (the last), `$['odd key']` or `$["odd key"]`. A path picks one value: there are no wildcards (`*`), `..` or filters.
- **Values** are JSON (`"welcome"`, `7`, `true`, `null`) or bare text: `$.type == welcome` is the same as `$.type == "welcome"`.
- **Compared as text**: strings as they are, numbers and booleans as written, objects and arrays as compact JSON. So `$.id == 7` matches both `{"id": 7}` and `{"id": "7"}`, and `$.ok == true` matches `{"ok": true}`.
- A message that isn't JSON never matches. A line break at its end is ignored.

## Scripts

The **Scripts** tab has three parts:

| Part | Runs |
|---|---|
| **Before connecting** | Before each connect, after the workspace's and folders' pre-request scripts. It can set variables (`pm.environment.set`, `pm.variables.set`), change the URL and headers (`pm.request`), and stop the connection with `pm.execution.skipRequest()`. An error in it stops the connection and says why. What it logs and tests shows in the message log. |
| **On message** | [Message scripts](#message-scripts), on the messages that match, while connected and in collection runs. |
| **In runs** | In collection runs, after the connection closes: `pm.response.json()` is the [transcript](#in-collection-runs). |

### Message scripts

A message script runs on each message that matches it:

| Setting | Description |
|---|---|
| **Name** | Optional; shown in the log and test names. Without one, the script is named after what it matches: `On a message containing "price"`. |
| **Runs on** | **Received** messages (the default), **Sent** ones (from the composer, saved messages, steps and repeats), or both. Messages the scripts send themselves never run **Sent** scripts, so a script can't loop on its own answers. |
| **Messages that** | **any message**, **contains**, **is exactly**, **matches regex** or **JSON where** a [JSON condition](#json-conditions). |
| The script | JavaScript with the usual [`pm` API](../../scripting/pm-reference/), plus `pm.message` and `pm.connection`. |
| The checkbox | Turns a script off without deleting it. |

```js
pm.test("Trade has a price", () => {
  pm.expect(pm.message.json().price).to.be.above(0);
});
pm.environment.set("lastTrade", pm.message.json().id);
pm.connection.send({ ack: pm.message.json().id });
```

| Name | Description |
|---|---|
| `pm.message.text()` | The message as text (binary: its bytes as text). |
| `pm.message.json()` | The message parsed as JSON. |
| `pm.message.direction` | `received` or `sent`. |
| `pm.message.size`, `.timestamp`, `.isBinary` | Its size in bytes, when it arrived (Unix milliseconds), and whether it is binary. |
| `pm.connection.send(data)` | Sends a text message; objects and arrays go as JSON. At most 100 per run of the script. |

Scripts run one at a time, in the order messages arrive, each with the [script time limit](../../scripting/sandbox/#limits). While connected, what they log and their test results (`✓` or `✗` with the error) show in the message log, and values they set on the environment, the workspace or globals are kept like those of [other scripts](../../scripting/overview/#variables-that-scripts-set) (saved about once a second while messages keep coming). Message scripts work with the variables and the environment as they were when the connection opened: after switching environments, reconnect. `pm.variables` values carry from one message to the next for as long as the connection is open, starting with what **Before connecting** set. When messages come faster than the scripts can run, those beyond 1000 waiting are skipped and the log says how many.

## In collection runs

WebSocket requests run in the Runner tab, in [`zorvik run`](../../cli/run/) and in runs an AI agent starts. Zorvik connects, runs the [On connect](#on-connect) steps, and closes the connection with code `1000` when they are done.

- Each **Wait for** step is a **test**, named after what it waits for: `Wait for a message containing "welcome"`, `Wait for the message "pong"`, `Wait for a message matching /^ack/`, `Wait for a JSON message where $.id == 7`, `Wait for any message`. A wait that fails says why: `Nothing matched within 5000 ms`, or `The server closed the connection (code 1008 · Policy violation) before a message matched`.
- After the first failure, the remaining waits are reported as **skipped**.
- A **Send** step that fails (a message that isn't saved, hex that isn't valid) is a failed test named `Send Login`.
- **Keep sending** steps don't run: the connection closes when the steps end. The request's console says so.
- [Message scripts](#message-scripts) run on the messages of the run; their tests are the request's tests too, named after the script (`On the message "welcome": greeted`), and what they log is in its console. The connection closes once they have answered (at most 5 seconds after the steps).
- A request without wait steps passes when it connects. One that can't connect (a refused upgrade, a timeout) fails with the error.
- The result's status is `101`, its time is the whole conversation, and its response body is the conversation as JSON, which workspace and folder post-response scripts read with `pm.response.json()`:

```json
[
  { "direction": "received", "text": "{\"type\": \"welcome\"}", "size": 19, "timestamp": 1790000000120.5 },
  { "direction": "sent", "text": "{\"op\": \"login\", \"token\": \"…\"}", "size": 31, "timestamp": 1790000000131.2 },
  { "direction": "received", "text": "{\"type\": \"ready\"}", "size": 17, "timestamp": 1790000000165.9 }
]
```

Binary messages are in it as `base64`. In `zorvik run` the request looks like this:

```text
✓ WS      Chat  wss://chat.example.com/socket  101 182 ms, 412 B
    ✓ Wait for a JSON message where $.type == welcome
    ✓ Wait for a message containing "ready"
```

The waits are tests in the [JSON and JUnit reports](../../testing/reports/) too.

## AI agents

Coding agents connected through `zorvik mcp` can open WebSocket connections too: `open_connection` connects (running the request's scripts and On connect steps, and listing its saved messages), then `send_message` (text, JSON, hex, a saved message or a ping), `wait_for_message`, `read_messages` and `close_connection` (with a close code and reason). While you follow agents, the connection shows live in a tab, marked **Connected by an AI agent**. See [Tools](../../agents/tools/#live-connections).

## Limits and network

- **Message size**: incoming messages up to 64 MB, frames up to 16 MB.
- **Timeout**: the request's timeout (in its **Settings** tab, or the app default) limits connecting and the handshake. An open connection has no time limit.
- **Proxy**: when an HTTP proxy applies to the host, both `ws://` and `wss://` go through a `CONNECT` tunnel.
- **TLS**: `wss://` is verified like HTTPS, with this computer's trusted certificates and **Settings → Certificates** (including a client certificate for mutual TLS). See [TLS & certificates](../../requests/tls-and-certificates/).

## Saved format

```yaml title="requests/Chat.yaml"
name: Chat
kind: websocket
url: wss://chat.example.com/socket?room=general
websocket:
  subprotocols:
    - chat.v1
  messages:
    - name: Login
      format: json
      text: '{"op": "login", "token": "{{token}}"}'
    - name: Heartbeat
      text: ping
  onMessage:
    - match: json
      pattern: $.type == trade
      script: pm.environment.set("lastTrade", pm.message.json().id);
  onConnect:
    - type: send
      message: Login
    - type: wait
      match: json
      pattern: $.type == welcome
      timeoutMs: 10000
    - type: repeat
      message: Heartbeat
      everyMs: 30000
  replyKey: $.request_id
body:
  type: text
  text: '{"type": "hello", "user": "{{userName}}"}'
```

| Field | Default | Description |
|---|---|---|
| `websocket.subprotocols` | none | Subprotocols to ask for, the preferred first. |
| `websocket.messages` | none | Saved messages: `name`, `format` (`text`, `json`, or `binary` for hex bytes in `text`) and `text`. |
| `websocket.messages[].format` | `text` | Left out for text. |
| `websocket.onConnect` | none | Steps, in order. `type: send` with `message` (a saved message's name); `type: wait` with `match`, `pattern` and `timeoutMs`; `type: repeat` with `message` and `everyMs`. |
| `websocket.onConnect[].match` | `contains` | Waits: `any`, `contains`, `exact`, `regex` or `json`. |
| `websocket.onConnect[].timeoutMs` | `5000` | Waits: the time limit in milliseconds. |
| `websocket.replyKey` | `$.id` | The JSON path that pairs replies with the messages they answer. |
| `body.text` | empty | The composer text. |
| `body.type` | `text` | The composer mode: `text` or `json`. |

:::tip[A server to talk to]
Zorvik can run a WebSocket server too, one that echoes, answers by rules, or lets you type the answers. See [WebSocket & SSE servers](../../servers/websocket-and-sse-servers/).
:::

---
title: WebSocket
description: Connect to WebSocket servers, send text, JSON or binary messages and watch the conversation live.
sidebar:
  order: 3
---

A WebSocket request opens a lasting connection and shows every message in both directions. You connect once, then send as many messages as you like from the composer.

## Create a WebSocket request

In the **Collection** sidebar, open the **New** menu (**+**) and choose **New WebSocket**. The request has these tabs: **Params**, **Headers**, **Auth**, **Settings** and **Docs**. There is no **Body** tab: messages are typed in the composer below the message log. There are no scripts either; scripts run for HTTP requests only.

## URL

| URL | Connection |
|---|---|
| `ws://host[:port]/path` | Plain WebSocket (port 80 by default). |
| `wss://host[:port]/path` | WebSocket over TLS (port 443 by default). |
| `http://…`, `https://…` | Treated as `ws://` and `wss://`. |
| `host/path` | No scheme means `ws://`. |

Anything after `#` is dropped. `{{variables}}` and query parameters work as in HTTP requests.

## Connect and disconnect

Press **Connect** (or <kbd>Mod</kbd>+<kbd>Enter</kbd>). The log shows **Connecting to …**, then **Connected · 101 Switching Protocols · 38 ms**, or the error. **Disconnect** sends a close frame with code `1000`; when the server doesn't answer it within 5 seconds, the connection is dropped anyway.

The status next to the log reads **Not connected**, **Connecting…**, **Connected** or **Disconnected**, with counts of sent and received messages. When the server closes the connection, the log shows it with the close code and reason, for example **Disconnected by server (code 1001 · going away)**.

### The upgrade request

Connecting sends an HTTP `GET` upgrade request. Its headers are:

| Header | Value |
|---|---|
| `Host` | The URL's host (and port), or your own `Host` header. |
| `Connection`, `Upgrade` | `Upgrade`, `websocket`. |
| `Sec-WebSocket-Version`, `Sec-WebSocket-Key` | `13` and a fresh key. |
| Your headers | Everything from the **Headers** tab, the folder and workspace headers, and the **Auth** header. `Connection`, `Upgrade`, `Sec-WebSocket-Version`, `Sec-WebSocket-Key` and `Content-Length` from your headers are ignored. |
| `User-Agent` | Zorvik's, unless you set one or default headers are off in Settings. |
| `Cookie` | Cookies from the cookie jar for this host (unless you set a `Cookie` header). `Set-Cookie` on the upgrade response is saved to the jar. |

To ask for a **subprotocol**, add a `Sec-WebSocket-Protocol` header, for example `graphql-transport-ws` or `mqtt`. To send an `Origin`, add an `Origin` header.

## Sending messages

The composer at the bottom has three modes:

| Mode | Sends | Notes |
|---|---|---|
| **Text** | A text frame | `{{variables}}` are replaced when you send. |
| **JSON** | A text frame | Like Text, with JSON highlighting. |
| **Binary (hex)** | A binary frame | Type hex bytes such as `48 65 6c 6c 6f`, `48:65:6c` or `0x48,0x65`. Variables are not replaced. |

Press **Send** or <kbd>Mod</kbd>+<kbd>Enter</kbd>. Send is available while connected and the composer is not empty. The composer text is saved with the request, so it is still there next time.

## The message log

Each row shows the direction (sent or received), the time, a **binary**, **ping** or **pong** badge where it applies, the start of the payload and its size. Click a row to see the whole payload: JSON is pretty-printed, binary data is shown as hex, and a copy button copies it. The filter box searches the payloads; the bin icon clears the log.

Pings from the server are answered automatically, and show up in the log.

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
headers:
  - key: Sec-WebSocket-Protocol
    value: chat.v1
body:
  type: text
  text: '{"type": "hello", "user": "{{userName}}"}'
```

`body.text` holds the composer text and `body.type` the composer mode (`text` or `json`).

:::tip[A server to talk to]
Zorvik can run a WebSocket server too, one that echoes, answers by rules, or lets you type the answers. See [WebSocket & SSE servers](../../servers/websocket-and-sse-servers/).
:::

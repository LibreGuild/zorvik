---
title: Server-Sent Events (SSE)
description: Open an event stream, watch events arrive live, and test streams in collection runs.
sidebar:
  order: 4
---

An event stream request opens a Server-Sent Events (SSE) stream, the `text/event-stream` format that `EventSource` in browsers reads, and shows each event as it arrives. Streams only go one way: from the server to you.

## Create an event stream request

In the **Collection** sidebar, open the **New** menu (**+**) and choose **New Event stream (SSE)**. Enter the URL, for example `https://example.com/events`. The request has **Params**, **Headers**, **Auth**, **Settings** and **Docs** tabs, like an HTTP request without a body.

## Connect

Press **Connect** (or <kbd>Mod</kbd>+<kbd>Enter</kbd>). Zorvik sends a `GET` request, resolved like an HTTP request (variables, folder and workspace headers, auth including OAuth 2.0, cookies, proxy and TLS settings), and adds two headers unless you set them yourself:

```http
Accept: text/event-stream
Cache-Control: no-cache
```

When the answer is a 2xx with `Content-Type: text/event-stream`, the log shows **Connected · 200 OK · 45 ms** and events start to appear. Otherwise the start of the body is shown as an error and the stream ends:

- **Response is not an event stream (Content-Type is not text/event-stream). Body: …** for a 2xx answer of another type.
- **Server responded with an error. Body: …** for any other status.

**Disconnect** closes the stream. The log says how a stream ended: **Server closed the stream**, **Connection lost** or **Closed by you**.

:::note[No automatic reconnect]
Unlike a browser's `EventSource`, Zorvik does not reconnect by itself when the stream ends, and it does not act on `retry:` fields. Press **Connect** again. To resume from an event, add a `Last-Event-ID` header yourself.
:::

## How events are read

Events are parsed the way browsers do (the WHATWG "event stream interpretation"):

| Line | Effect |
|---|---|
| `event: name` | The event's name. Events without one are named `message`. |
| `data: text` | A line of data. Several `data:` lines are joined with line breaks. |
| `id: value` | The event id; it stays in effect for later events. |
| `retry: ms` | Read, but not used. |
| `: anything` | A comment (keep-alive), ignored. |
| empty line | Ends the event. |

Line breaks may be LF, CR or CRLF, and a leading byte order mark is ignored. A single line or event larger than 16 MB is cut at 16 MB.

## The event log

Each row shows the time, the event name as a badge, the start of the data and its size. Click a row to see the whole data (JSON pretty-printed) and the event's `id`. The status line counts the events; the filter box searches event data and names; the bin icon clears the log.

## Event streams in collection runs

In collection runs (the runner and `zorvik run`) and for AI agents, a stream can't stay open forever, so the request's **Settings** tab has an **In collection runs** section. Reading stops at the first of these:

| Setting | Default | Meaning |
|---|---|---|
| **Stop at event** | empty | Stop after the first event with this name (`message` for events without a name). Empty: any event may be the last. |
| **Stop after** | 100 | Stop after this many events. 0: only the time limit counts. |
| **Time limit** | 10000 ms | Stop after this long (at least 100 ms). |

The server closing the stream also ends it. The post-response scripts then test the events through `pm.response.events`; see [Scripts & tests](../../scripting/overview/). A run keeps at most 1,000 events and 64 KB of data per event, and never reads for longer than 5 minutes.

The **Repeat until** settings (send again until a condition holds) are available for event streams in collection runs too.

## Saved format

```yaml title="requests/Build log.yaml"
name: Build log
kind: sse
url: "{{baseUrl}}/builds/{{buildId}}/events"
headers:
  - key: Authorization
    value: Bearer {{token}}
settings:
  stream:
    event: done
    maxEvents: 0
    timeoutMs: 60000
```

| Field | Default | Description |
|---|---|---|
| `kind` | | `sse`. |
| `settings.stream.event` | empty | **Stop at event**. |
| `settings.stream.maxEvents` | `100` | **Stop after** (events). |
| `settings.stream.timeoutMs` | `10000` | **Time limit** in milliseconds. |

:::tip[A server to test against]
Zorvik can run an event stream server that plays a list of events, once or on repeat. See [WebSocket & SSE servers](../../servers/websocket-and-sse-servers/#event-stream-sse-server).
:::

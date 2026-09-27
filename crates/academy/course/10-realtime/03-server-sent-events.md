---
id: server-sent-events
title: "Server-Sent Events: news from the server"
summary: One HTTP request, kept open, down which the server writes named events whenever it has news.
minutes: 5
lab:
  title: Tune in to the newsroom
  goal: Read a live event stream, then reconnect and resume where you left off with Last-Event-ID.
  minutes: 7
  servers:
    news:
      name: Newsroom
      kind: sse
      sse:
        intervalMs: 600
        events:
          - { event: headline, id: "1", data: Bootcamp opens its doors }
          - { event: headline, id: "2", data: Local server answers in under a millisecond }
          - { event: weather, id: "3", data: Sunny over 127.0.0.1 }
          - { event: headline, id: "4", data: "Breaking: the code word is {{secret.word}}" }
  steps:
    - text: |
        Click **+** at the end of the tab bar and choose **Event stream (SSE)**. Set the URL to `{{news}}/live` and press **Connect**. Four events arrive, a little apart.
      hints:
        - "An event stream is a GET request that stays open. Zorvik lists each event as it arrives."
        - "Choose Event stream (SSE) in the + menu of the tab bar, put {{news}}/live in the URL bar and press Connect."
        - "URL {{news}}/live, press Connect, and wait about two seconds: the counter reaches 4 events."
      check:
        message: { server: news, kind: open }
      solution:
        - call: { method: sse.connect, params: { connId: lab-sse, request: { name: Newsroom, kind: sse, url: "{{news}}/live" } } }
    - text: The event with id `4` carries a code word. Type it here.
      hints:
        - "Each row in the log shows the event's name as a badge. Click a row to see its id and full data."
        - "The last headline event is the one with id 4. The server's side shows it too: open Lab · Newsroom under Servers and look at its Traffic."
        - "Its data reads: Breaking: the code word is … Copy the word after is."
      check:
        answer: "{{secret.word}}"
      solution:
        - answer: "{{secret.word}}"
    - text: |
        Pretend your connection dropped right after event `2`. Press **Disconnect**, open the **Headers** tab, add the header `Last-Event-ID` with the value `2`, and press **Connect** again.
      hints:
        - "Last-Event-ID tells the server the id of the last event you saw, so it can continue from there."
        - "Headers is one of the tabs under the URL bar. Type Last-Event-ID as the key and 2 as the value."
        - "Disconnect, add the header Last-Event-ID: 2 in the Headers tab, then press Connect."
      check:
        call: { method: sse.connect, params: { request: { headers: [{ key: Last-Event-ID, value: "2" }] } }, ok: true }
      solution:
        - call: { method: sse.connect, params: { connId: lab-sse, request: { name: Newsroom, kind: sse, url: "{{news}}/live", headers: [{ key: Last-Event-ID, value: "2" }] } } }
        - wait: 1000
        - call: { method: sse.close, params: { connId: lab-sse } }
    - text: This time the stream did not start from the beginning. What is the **name** of the first event you received after reconnecting?
      hints:
        - "The server skipped every event up to and including id 2."
        - "Look at the badge on the first event after the second Connected line."
        - "After Last-Event-ID 2, the next event has id 3. Its name is weather."
      check:
        answer: weather
      solution:
        - answer: weather
quiz:
  - question: In which direction do Server-Sent Events flow?
    options:
      - From the client to the server only
      - From the server to the client only
      - Both ways, like a WebSocket
    answer: 1
    explain: The client sends one request. After that only the server writes. To send something back, the client makes a normal HTTP request.
  - question: What is the Last-Event-ID header for?
    options:
      - Resuming a stream after a reconnect without missing or repeating events
      - Logging in to the event stream
      - Choosing which event names you want to receive
    answer: 0
    explain: The client sends the id of the last event it saw, and a server that supports it continues from the next one.
  - question: In the text of an event stream, what marks the end of one event?
    options:
      - A comma
      - Closing the connection
      - An empty line
    answer: 2
    explain: Each event is a few lines such as event and data, followed by a blank line. The stream itself stays open.
---

Sometimes the client doesn't need to *talk*, it only needs to *listen*: notifications, a live score, a build log, a progress bar, or an AI assistant typing its answer word by word. For that there is a simpler tool than WebSocket: **Server-Sent Events** (SSE).

## How it works

The client sends a normal HTTP `GET`. The server answers with `Content-Type: text/event-stream` and then simply **doesn't finish** the response. Whenever it has news, it writes another event into the open response.

```sequence
App -> Server: GET /live (Accept: text/event-stream)
Server --> App: 200 OK, Content-Type: text/event-stream
Server --> App: id: 1, event: headline, data: …
Server --> App: id: 2, event: headline, data: …
Note over App: the connection drops
App -> Server: GET /live (Last-Event-ID: 2)
Server --> App: id: 3, event: weather, data: …
```

## The event format

The stream is plain text. Each event is a few `field: value` lines followed by an empty line:

```anatomy
id: 4 | an id, so the client can say where it stopped
event: headline | the event's name (without one it is called "message")
data: Breaking: the code word is … | the content; several data lines are joined with line breaks
(an empty line) | the end of this event
```

A line starting with `:` is a comment. Servers send one now and then (Zorvik's own SSE servers every 15 seconds) just to keep quiet connections from being cut.

## Reconnecting without losing news

Connections drop: a train enters a tunnel, a laptop sleeps. Browsers reconnect by themselves and send the header **`Last-Event-ID`** with the last id they saw. A server that supports it continues right after that event, so nothing is missed and nothing repeats. In the lab you'll do this by hand.

> [!note] Think of it like…
> A radio station. You tune in once and it keeps broadcasting to you, but you can't talk back through the radio. If you drop out, a good station can tell you what you missed since the last news item you heard.

## SSE or WebSocket?

| | SSE | WebSocket |
|---|---|---|
| Direction | server to client | both ways |
| Built on | plain HTTP | HTTP upgrade, then its own frames |
| Reconnect and resume | built into the standard | up to you |
| Content | text | text or binary |

Because SSE is plain HTTP, it passes through most proxies and firewalls untouched, and headers such as `Authorization` work as usual.

In Zorvik, an **Event stream (SSE)** request has **Params**, **Headers**, **Auth** and **Settings** tabs. **Connect** opens the stream, and every event appears in the log with its name as a badge; click one to see its id and data. The counter shows how many events arrived.

**You'll use this when…** you test a notifications feed or an AI chat API that streams its answer: you connect, watch each event arrive with its name, id and data, and check that a reconnect with `Last-Event-ID` resumes correctly.

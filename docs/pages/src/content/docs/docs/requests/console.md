---
title: The console
description: The console lists every request Zorvik sends with the headers and body that went out and came back, why a request failed, and what scripts log, in the order it happened.
sidebar:
  order: 8.5
---

The console is the log of everything Zorvik sends. Each request is a line you can open to see exactly what went out and what came back, and the lines scripts write with `console.log` sit between the requests in the order they happened. When a request fails, this is where to look.

Open it with the **Console** button at the foot of the left rail, or press <kbd>Mod</kbd>+<kbd>J</kbd>. It sits under the open tab; drag its top border to resize it. While it is closed, a red number on the button counts the requests that failed and the error lines since you last looked.

## What is logged

| What | Shown as |
|---|---|
| Every request you send from a tab: HTTP, GraphQL, gRPC, MCP calls, DNS lookups | A request line: method, URL, status, time and size |
| Requests a [collection run](../../testing/collection-runner/) sends | Request lines marked **Runner** |
| Requests an [AI agent](../../agents/setup/) sends | Request lines marked **Agent** |
| Requests scripts make with `pm.sendRequest` | Request lines marked `pm.sendRequest`, between the script's own lines |
| OAuth 2.0 token requests (fetching or refreshing a token) | Request lines marked `OAuth 2.0 token`, before the request that needed the token |
| The query that reads a [GraphQL](../../protocols/graphql/) schema | A request line marked `GraphQL schema` |
| Connections: WebSocket, Socket.IO, SSE, TCP, UDP, MQTT, gRPC streams, MCP sessions | A line when the connection opens (or fails to), with the handshake for WebSocket and SSE; WebSocket, Socket.IO, TCP, UDP and MQTT connections also leave a line when they close. The messages themselves are in the tab's [message log](../../protocols/websocket/). |
| `console.log`, `info`, `warn`, `error` and `debug` in [scripts](../../scripting/overview/#the-console) | Log lines, as the script writes them, also while it still runs |
| A script that stops with an error, and notes such as **No environment is active** | Error and warning lines |

Each line shows the time and, on its right, the name of the request it belongs to.

Load tests are not logged (they send far too many requests; their own results show what happened), and neither is the traffic of the [servers you run](../../servers/running-servers/), which have their own traffic log.

## Open a request

Click a request line to open it:

- **Network**: the address that answered, the proxy, the TLS version and cipher, each redirect that was followed, and the time each phase took (DNS, connect, TLS, first byte, download).
- **Request headers**: the headers as they went out, including the ones Zorvik adds (`Host`, `User-Agent`, cookies) and the ones inherited from folders and the workspace.
- **Request body**: the body as sent, with variables filled in.
- **Response headers** and **Response body**: what came back. JSON is pretty-printed.

Bodies show their first 64 KB; binary bodies show their size only. Hover a header or a body to copy it.

Values are shown **as they were sent**, secret variables and tokens included, because that is what you need to find out why a request failed. The console is kept in memory only: it is never written to a file, not into the workspace and not into the app's data folder.

## When a request fails

A request that got no response (the server is down, the name doesn't resolve, the TLS handshake failed, it timed out) is shown in red with the reason under it. Open it to see the request that was prepared: its URL, headers and body after variables, inherited headers and auth were applied. The headers Zorvik adds while sending aren't listed in that case, because nothing went out.

A request that can't even be prepared (an undefined `{{variable}}` in the URL, a body file that is missing) is logged as it is written, with the reason.

A request you cancel leaves a **Cancelled** line.

## Filter and clear

- **All**, **Requests**, **Logs** and **Errors** choose what is listed. **Errors** are requests that failed or were answered with a status of 400 or more, and error lines.
- The filter field keeps the lines whose URL, request name, status or text contains what you type.
- The bin icon clears the console.

The console keeps the newest 5000 lines (and at most 64 MB of headers and bodies); older ones go. It shows the newest 300 lines at first: choose **Show earlier lines** at the top for more. It follows new lines while you are at the bottom of the list; scroll up to stay where you are.

It is cleared when you open another workspace, and it starts empty each time the app starts.

## For AI agents

Agents read the console with the [`read_console`](../../agents/tools/#history-console-and-files) tool, to see why a request failed. What they get is [redacted](../../agents/permissions-and-safety/#redaction): secret variable values and credential headers are hidden, and token requests come without their headers and bodies.

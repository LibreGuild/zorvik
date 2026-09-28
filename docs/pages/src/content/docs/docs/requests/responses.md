---
title: Responses
description: Everything the response pane shows, including body views, filters (JSONPath, jq, XPath), headers, cookies, the timing waterfall and connection info, plus saved examples, saving bodies, errors and request history.
sidebar:
  order: 8
---

After you send an HTTP request, the response pane shows what came back and how it got there.

## The status line

At the top of the response:

| Item | Meaning |
|---|---|
| Status, e.g. `200 OK` | Green for 2xx, blue for 3xx, amber for 4xx, red for 5xx. The text is the server's own reason phrase when it sent one. |
| Time | The total time, from starting the request to the last byte of the body |
| Size | The body's size after decompression. Hover it for the **wire size**, the bytes actually received. |
| Protocol | `HTTP/1.1`, `HTTP/2` or `HTTP/3` |
| Lock | The response came over TLS |
| *N redirects* | How many redirects were followed |
| **Save as example** | Keeps this response in the request. See [Examples](#examples). |
| **Mock** | Adds a route that answers like this response to a mock API. See [Mock servers](../../servers/mock-api/). |

Below it, warnings may appear:

- **Sent with undefined values:** the `{{variables}}` (and empty `:path` variables) that weren't defined and were sent as written. See [Variables & environments](../../variables/variables-and-environments/#undefined-variables).
- A red line for each script that failed, with the script and the line. See [Scripts & tests](../../scripting/overview/).

## Response tabs

| Tab | Shown | Contents |
|---|---|---|
| **Body** | Always | The body, in the views below |
| **Tests** | When scripts ran tests | Passed and failed tests. See [Scripts & tests](../../scripting/overview/). |
| **Headers** | Always | Every response header, with a count |
| **Cookies** | Always | Cookies this response set, with a count |
| **Timing** | Always | The timing waterfall |
| **Info** | Always | Connection, TLS certificate, redirects and the request that was sent |
| **Console** | When scripts logged something or failed | `console` output and script errors |

The tab you pick stays selected for that request tab.

## Body

The body toolbar has the view buttons, the content type, and four icons: **Filter**, **Wrap lines** (on by default), **Copy body** and **Save to file…**.

| View | For | Shows |
|---|---|---|
| **Pretty** | JSON (by `Content-Type`, or a body starting with `{` or `[`), up to 5 MB | Indented JSON with syntax highlighting |
| **Raw** | Any text | The body exactly as received (after decompression), highlighted by content type (JSON, HTML, XML, JavaScript) |
| **Preview** | HTML | The page, rendered in a sandbox where its scripts don't run |
| **Preview** | Images (except SVG), up to 8 MB | The image, on a checkerboard for transparency |
| Hex | Other binary bodies | **Binary · hex preview**: offset, hex bytes and ASCII of the first 64 KB |

- Press <kbd>Mod</kbd>+<kbd>F</kbd> in the body to search it.
- Text bodies show the first 10 MB. Beyond that, *Only the first part is shown. Use Save to file to get the whole body.*
- An empty body shows **Empty body**.
- A body cut at the size limit shows *The response was larger than the size limit and was cut. Raise the limit in Settings.* See [Response size limit](../http-versions-timeouts-redirects/#response-size-limit).

### Filter the body

**Filter** (the funnel icon) opens a row above the body. Pick a language, type an expression, and the body shows only what matches, with the number of matches. Close it (or press <kbd>Esc</kbd> in the field) to see the whole body again. The expression stays when you send again, so you can watch the same part of the response.

| Language | For | Example |
|---|---|---|
| **JSONPath** ([RFC 9535](https://www.rfc-editor.org/rfc/rfc9535)) | JSON | `$.items[?@.price > 10].name` |
| **jq** | JSON | `.items[] | select(.price > 10) | .name` |
| **XPath** 1.0 | XML and HTML | `//item[price > 10]/name`, `count(//li)`, `//a/@href` |

- JSON bodies offer **JSONPath** and **jq**; XML and HTML bodies offer **XPath**; other text offers all three.
- JSONPath and jq run on the **whole** body, even when only the first 10 MB are shown. XPath runs on what is shown.
- JSONPath shows its matches as a JSON array. jq shows each result on its own, like the `jq` command (`[.items[].price] | add` gives one number).
- At most 10,000 results are shown. A mistake in the expression shows what is wrong in red, for example *Not a valid jq expression: expected a closing bracket*.
- **Copy body** copies what the filter shows.

AI agents can use the same filters: `send_request` takes a `filter` ([AI agents tools](../../agents/tools/)).

### Save to file

**Save to file…** (the download icon) writes the **whole** body to a file you choose, not just what is shown: all bytes of a large or binary response, after decompression.

Zorvik keeps the bodies of the last 30 responses (up to 512 MB together) for saving. For an older response it says *That response is no longer available; send the request again*.

## Examples

**Save as example** (above the response) keeps the response in the request, as documentation of what it answers and for mocks:

- The example gets the status as its name (`200 OK`, `200 OK (2)`…), the status code, the response headers and the body. Framing headers, `Date` and `Set-Cookie` are left out: cookies can hold a session, and examples are saved in the workspace (and Git).
- Bodies up to 1 MB of text are kept. Binary and larger responses can't be examples; use **Save to file**.
- A saved request with no other unsaved changes is saved at once. Otherwise the example waits with your other changes until you save.

The request's **Examples** tab lists them: pick one to see its headers and body, rename it, change its status or body, or delete it.

[Mock APIs built from a folder](../../servers/from-openapi-and-folders/#from-a-folder-of-requests) answer with the examples: an example saved while the URL had query parameters answers only requests with those parameters, and the first one without answers the rest.

Examples are imported from Postman collections (a request's saved responses) and are written to the request's file:

```yaml
examples:
  - name: 200 OK
    status: 200
    headers:
      - { key: Content-Type, value: application/json }
    body: '{"id": 7, "name": "Rex"}'
  - name: Not found
    status: 404
    url: "{{baseUrl}}/pets/999?include=owner"
    body: '{"error": "not found"}'
```

## Headers and Cookies

**Headers** lists every response header in the order received, one row per header line. Hover a row and choose the copy icon to copy its value.

**Cookies** lists the cookies set by this response's `Set-Cookie` headers: **Name**, **Value**, **Domain**, **Path**, **Expires** (or **Session**) and **Flags** (`Secure`, `HttpOnly`, `SameSite`). The cookies the workspace has stored are in the title bar's **Cookies** dialog. See [Cookies](../cookies/).

## Timing

The **Timing** tab is a waterfall of where the time went. Each phase is a bar placed after the previous one, with its duration on the right:

| Phase | Measures |
|---|---|
| **Redirects** | Time spent on earlier redirect hops (only when there were redirects) |
| **DNS lookup** | Resolving the host name |
| **TCP connect** | Opening the connection, including a proxy tunnel |
| **TLS handshake** | Negotiating encryption (`https://` only) |
| **Waiting (TTFB)** | From sending the request until the first byte of the response: mostly the server's processing time |
| **Download** | Receiving the body |
| **Total** | The whole request |

For HTTP/3, **QUIC handshake** replaces **TCP connect** and **TLS handshake**, because QUIC does both at once.

Every request uses a fresh connection, so DNS, connect and TLS are always measured, never hidden by a reused connection. Behind a proxy, **DNS lookup** is the lookup of the proxy's name. See [Proxies](../proxies/#how-requests-use-the-proxy).

## Info

| Section | Shows |
|---|---|
| **Connection** | **URL** (the final one, after redirects), **Remote address** (IP and port), **Protocol**, **Proxy** (when one was used) and the size of the **Response headers** |
| **Security** | **TLS** version, **Cipher**, **ALPN**, and the server certificate's **Subject**, **Issuer**, **Valid** dates, **Names** and **Serial**. Plain HTTP shows **Not encrypted**. See [TLS & certificates](../tls-and-certificates/#tls-details-of-a-response). |
| **Redirects** | Each hop: status, method, URL and where it pointed |
| **Request sent** | The method, body size, URL and **every header that was really sent**, including those Zorvik added (`User-Agent`, `Content-Type`, `Authorization`, `Cookie`, …) |

**Request sent** is the place to check what your auth, inherited headers and cookie jar really produced.

## When a request fails

If no response arrives, the pane shows what went wrong, the error message and how long it took:

| Title | Typical cause | Hint shown |
|---|---|---|
| **Could not resolve host** | DNS lookup failed | Check the host name, your network or VPN, and DNS settings |
| **Could not connect** | Nothing listening, or blocked | Is the server running and reachable on that port? |
| **TLS / certificate error** | Certificate not trusted, wrong name or expired | Add the CA in **Settings → Certificates**, or turn off verification for this request (with **Open settings**) |
| **Proxy error** | Proxy unreachable, refused or needs credentials | Check **Settings → Proxy** (with **Open settings**) |
| **Request timed out** | A timeout was reached | Increase the timeout in the request's **Settings** tab or in Settings |
| **Too many redirects** | More redirects than **Max redirects** | |
| **Undefined variable** | The URL's host is an undefined `{{variable}}` | **Open environments** |
| **Pre-request script failed** | A pre-request script threw an error; nothing was sent | Fix the script in the request's, its folder's or the workspace's **Scripts** tab |
| **Invalid request** | For example an invalid URL, method or header value | |
| **Request cancelled** | You chose **Cancel** | |

See [Troubleshooting](../../help/troubleshooting/) for more.

## History

Every HTTP request you send from a tab (GraphQL included) is recorded in the workspace's history. Open **History** in the left rail.

- Entries are grouped by day (**Today**, **Yesterday**, then dates), newest first. Each shows the method, the URL, the status (or **Failed**), the time taken and the time of day.
- **Search history** finds entries whose URL or method contains the text. The list shows the newest 300 matching entries; search to reach older ones.
- Click an entry to open that request in a new, unsaved tab, exactly as it was when you sent it (its name, or "From history"). Send it again or save it.
- The trash icon, **Clear history**, deletes all entries of this workspace after you confirm.

| Detail | |
|---|---|
| What is recorded | The request as you wrote it (with its `{{variables}}`, so secret values from environments aren't stored), the URL as sent, the status or error, the duration, the body size and the time. Response bodies are not stored. |
| Secrets in URLs | Values of secret variables in the sent URL (such as an API key in the query) are replaced by `{{name}}` |
| Failed requests | Recorded, with their error. Cancelled requests and requests stopped by a pre-request script are not. |
| Not recorded | Collection runs, requests from the network tools, and other request kinds (WebSocket, SSE, gRPC, TCP, UDP, DNS, MQTT) |
| How many | The newest 500 per workspace. Change it in **Settings → Data & privacy → History size** (10 to 100000). |
| Where | `history.sqlite3` in the app data folder, readable only by your user on macOS and Linux. Never in the workspace. |

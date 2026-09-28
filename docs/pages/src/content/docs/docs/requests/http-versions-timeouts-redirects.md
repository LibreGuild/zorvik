---
title: HTTP versions, timeouts & redirects
description: Choosing HTTP/1.1, HTTP/2 or HTTP/3, how request and connect timeouts work, how redirects are followed, and the other per-request settings with their defaults.
sidebar:
  order: 7
---

How a request travels is set in two places:

- **Settings → Requests**: the defaults for every request in every workspace.
- The request's **Settings** tab: overrides for that request only, saved in its file.

## Defaults and overrides

| Setting | Default (Settings → Requests) | Per request (Settings tab) | In the request file |
|---|---|---|---|
| **Request timeout** | 60000 ms | **Timeout** | `timeoutMs` |
| **Connect timeout** | 15000 ms | — | — |
| **Follow redirects** | On | App default / On / Off | `followRedirects` |
| **Max redirects** | 10 | A number | `maxRedirects` |
| **Verify TLS certificates** | On | App default / On / Off | `verifyTls` |
| **HTTP version** | Auto (HTTP/2 when offered) | App default or a version | `httpVersion` |
| **Decompress responses** | On | App default / On / Off | `decompress` |
| **Default headers** | On | — | — |
| **Max response size** | 100 MB | — | — |

### Per-request settings

The request's **Settings** tab says *Overrides for this request only. Defaults are in Settings.* Every field starts at **App default** (or empty, for numbers). Only the fields you change are written to the request file:

```yaml title="requests/Slow report.yaml"
settings:
  timeoutMs: 300000
  followRedirects: false
  verifyTls: false
  httpVersion: http2      # auto | http1 | http2 | http3
  decompress: false
```

The **Settings** tab of HTTP and event stream requests also has **Repeat until**, which makes collection runs send a request again until a condition holds. See [Collection runner](../../testing/collection-runner/).

## HTTP versions

| Choice | In the file | What happens |
|---|---|---|
| **Auto (HTTP/2 when offered)** | `auto` | For `https://`, Zorvik offers HTTP/2 and HTTP/1.1 during the TLS handshake (ALPN) and uses what the server picks. For `http://`, HTTP/1.1. |
| **HTTP/1.1 only** | `http1` | Always HTTP/1.1 |
| **HTTP/2 only** | `http2` | For `https://`, only HTTP/2 is offered; if the server doesn't agree, the request fails with *Server did not agree to HTTP/2 (ALPN)*. For `http://`, HTTP/2 without TLS ("prior knowledge"), except through a proxy, where HTTP/1.1 is used. |
| **HTTP/3 (QUIC)** | `http3` | HTTP/3 over QUIC. See below. |

The version that was used is shown next to the status (`HTTP/1.1`, `HTTP/2` or `HTTP/3`) and in **Info → Protocol**.

### HTTP/3

HTTP/3 runs over QUIC, on UDP, and is always encrypted with TLS 1.3.

- It is only used when you choose it. **Auto** never switches to HTTP/3, even when the server advertises it (`Alt-Svc`).
- It needs an `https://` URL. Anything else fails with *HTTP/3 needs an https:// URL*.
- It never goes through a proxy. If the host would use one, the request fails with *HTTP/3 can't go through an HTTP proxy*; add the host to the proxy's bypass list, or use HTTP/1.1 or HTTP/2. See [Proxies](../proxies/).
- The **Timing** tab shows one **QUIC handshake** phase instead of **TCP connect** and **TLS handshake**, because QUIC does both at once.

To check whether a site offers HTTP/3 at all, use the **HTTP/3 check** in the **Tools** section of the left rail.

### A fresh connection per request

Every request opens a new connection: DNS lookup, connect and TLS are done and measured each time, so the **Timing** tab always shows the full cost and a stale connection can't affect the result. (Load tests reuse connections; see [Load testing](../../load-testing/overview/).)

## Timeouts

| Timeout | Covers | Default | Range |
|---|---|---|---|
| **Request timeout** | The whole request: connecting, sending, every redirect hop and downloading the body | 60000 ms (60 s) | `0` means no limit |
| **Connect timeout** | Getting connected: DNS lookup, TCP connect, the proxy tunnel and the TLS handshake, together | 15000 ms (15 s) | At least 100 ms |

- Override the request timeout for one request in its **Settings** tab (**Timeout**, in milliseconds). Leave it empty for the app default; `0` means no limit.
- The connect timeout is only in **Settings → Requests**.
- When a timeout hits, the response pane shows **Request timed out** with what timed out, for example *Request timed out after 60s* or *TLS handshake timed out after 15s*.
- Event streams (SSE): the timeout covers connecting and receiving the response head. Once events flow, the stream stays open until you close it.
- **Cancel**, in the URL bar or in the response pane, stops a request at any time.

Scripts have their own **Script time limit**. See [Scripts & tests](../../scripting/overview/).

## Redirects

With **Follow redirects** on, Zorvik follows `301`, `302`, `303`, `307` and `308` responses that have a `Location` header, like a browser:

| Status | Next request |
|---|---|
| `303 See Other` | `GET`, without a body (a `HEAD` stays `HEAD`) |
| `301` and `302` after a `POST` | `GET`, without a body |
| `301` and `302` after other methods, `307`, `308` | The same method and body |

- A relative `Location` is resolved against the current URL. A redirect to something other than `http://` or `https://` fails.
- When the method changes to `GET`, `Content-Type` and `Content-Encoding` are dropped.
- When the redirect goes to another origin (scheme, host or port), `Authorization`, `Cookie`, `Host` and `Proxy-Authorization` are dropped, so your credentials never leak to another site.
- Cookies set on each hop go into the cookie jar, and cookies from the jar are matched for each new URL. See [Cookies](../cookies/).
- **Max redirects** limits how many redirects are followed. One more fails the request with **Too many redirects** (*Stopped after N redirects*). With **Max redirects** `0`, the first redirect fails.

With **Follow redirects** off, the `3xx` response itself is shown.

The status line shows how many redirects were followed. **Info → Redirects** lists each hop (status, method, URL and `Location`), and **Timing → Redirects** shows the time spent on the earlier hops. The response's URL and TLS details are those of the last hop.

## Decompression

With **Decompress responses** on (the default), Zorvik:

- sends `Accept-Encoding: gzip, deflate, br, zstd`, unless you set that header yourself or **Default headers** is off;
- decodes bodies with `Content-Encoding` `gzip`, `x-gzip`, `deflate`, `br` or `zstd`, also several of them in a row.

If a body can't be decoded, the raw bytes are shown with a warning such as *Could not decode 'br' body*. With decompression off, you see the body exactly as it came over the wire.

The size next to the status is the decoded size; hover it for the size on the wire.

## Response size limit

**Settings → Requests → Max response size**, from 1 to 2048 MB (default 100 MB), caps how much of a response body is kept. A larger body is cut, and the response shows *The response was larger than the size limit and was cut.* The limit also bounds decompression, so a small compressed body can't expand without end.

## Default headers

**Settings → Requests → Default headers** (on by default) adds `User-Agent: Zorvik/<version>`, `Accept: */*` and `Accept-Encoding` to requests that don't set them. Turn it off to leave these three out. Headers that come from the body, auth and the cookie jar, and those HTTP itself needs (such as `Host` and `Content-Length`), are still sent. See [HTTP requests](../http/#headers-zorvik-adds).

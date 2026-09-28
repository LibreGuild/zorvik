---
title: Proxies
description: How Zorvik finds and uses an HTTP proxy (system settings, environment variables or a manual proxy), bypass lists, proxy authentication and what is never proxied.
sidebar:
  order: 6
---

Zorvik works behind corporate proxies. By default it uses the same proxy as your system; you can also set one by hand or turn proxies off.

## Proxy modes

Open **Settings → Proxy**:

| Mode | Behaviour |
|---|---|
| **Use system proxy** (default) | The proxy from environment variables, else from the OS settings. See [System proxy](#system-proxy). |
| **No proxy** | Every connection goes directly to the server. |
| **Manual** | The **Proxy URL** and **Bypass** list you enter. |

The proxy setting applies to all workspaces.

## System proxy

With **Use system proxy**, Zorvik looks in two places, in this order:

**1. Environment variables.** When `HTTP_PROXY`, `HTTPS_PROXY` or `ALL_PROXY` is set (upper or lower case), Zorvik uses them and ignores the OS settings:

| Variable | Used for |
|---|---|
| `HTTPS_PROXY` / `https_proxy` | `https://` targets |
| `HTTP_PROXY` / `http_proxy` | `http://` targets |
| `ALL_PROXY` / `all_proxy` | Either, when the specific variable isn't set |
| `NO_PROXY` / `no_proxy` | Hosts that go directly (a [bypass list](#bypass-list)) |

**2. The operating system's settings**, read again at most every 10 seconds:

| System | Where | Notes |
|---|---|---|
| Windows | Internet Options (the per-user proxy settings) | The proxy server and its exceptions. `http=…;https=…` settings per protocol are understood. |
| macOS | System Settings → Network → proxies (read with `scutil --proxy`) | The web proxy (HTTP) and secure web proxy (HTTPS), the bypass list, and **Exclude simple hostnames** |
| Linux | — | Only the environment variables are used |

:::note
Apps started from the macOS Dock or Finder don't see variables exported in your shell profile. There, Zorvik uses the macOS proxy settings.
:::

## Manual proxy

Choose **Manual** and fill in:

| Field | Example | Meaning |
|---|---|---|
| **Proxy URL** | `http://proxy.corp.local:8080` | The proxy for every request. The scheme may be left out; the port defaults to 80. |
| **Bypass** | `*.corp.local, 10.0.0.0/8, <local>` | Hosts that go directly. See [Bypass list](#bypass-list). |

With an empty **Proxy URL**, connections go directly.

### Proxy authentication

Put the user name and password in the proxy URL:

```text
http://alice:s3cret@proxy.corp.local:8080
```

Zorvik sends them as `Proxy-Authorization: Basic …`. Percent-encode special characters: `@` as `%40`, `:` as `%3A`. Credentials work in environment variables the same way.

When the proxy answers `407`, the request fails with *Proxy requires authentication (407). Add credentials to the proxy URL in Settings (only Basic auth is supported).*

## Bypass list

A bypass list names hosts that are reached directly, without the proxy. Entries are separated by commas, semicolons, spaces or new lines, and compared without regard to case.

| Entry | Matches |
|---|---|
| `example.com` | `example.com` and every subdomain, such as `api.example.com` |
| `.example.com` | The same: `example.com` and its subdomains |
| `*.corp.local` | Any host matching the pattern: `*` stands for any text, `?` for one character |
| `10.*` | Addresses and names starting with `10.` |
| `10.0.0.0/8`, `fd00::/8` | IP addresses in that range (IPv4 or IPv6) |
| `<local>` | Host names without a dot, such as `intranet` |
| `*` | Every host |

A port or an `http://` / `https://` prefix in an entry is ignored: `example.com:8443` bypasses all of `example.com`.

**Always direct**, whatever the list says: `localhost`, names ending in `.localhost`, and loopback addresses (`127.0.0.1` and the rest of `127.0.0.0/8`, `::1`).

## How requests use the proxy

| Target | How it goes through the proxy |
|---|---|
| `https://` | A `CONNECT` tunnel to the server; TLS runs end to end inside it, so the proxy can't read the request (unless it inspects TLS with its own CA). |
| `http://` | Sent to the proxy with the full URL in the request line, and `Proxy-Authorization` when the proxy URL has credentials |

Zorvik connects to the proxy, so the proxy resolves the server's name:

- The **Timing** tab's DNS time is the lookup of the proxy's name, and **TCP connect** includes setting up the tunnel.
- The **Info** tab shows **Proxy** (the proxy's host and port), and **Remote address** is the proxy's address.

### What goes through the proxy

| Traffic | Proxied |
|---|---|
| HTTP and HTTPS requests (HTTP/1.1 and HTTP/2), GraphQL, event streams | Yes |
| WebSocket (`ws://` and `wss://`) | Yes, through a `CONNECT` tunnel |
| gRPC | Yes, through a `CONNECT` tunnel |
| TCP connections | Yes, through a `CONNECT` tunnel |
| DNS over HTTPS | Yes |
| OAuth 2.0 token requests, OpenAPI imports from a URL, load tests | Yes |
| HTTP/3 | **No**: QUIC runs over UDP, which HTTP proxies can't carry |
| UDP, MQTT, and DNS over UDP, TCP or TLS | No, always direct |

An HTTP/3 request to a host that would use a proxy fails with *HTTP/3 can't go through an HTTP proxy*. Add the host to the bypass list, or use HTTP/1.1 or HTTP/2. See [HTTP versions](../http-versions-timeouts-redirects/#http3).

## Not supported

- **PAC files** and automatic proxy discovery (WPAD). On Windows and macOS, only a proxy that is set explicitly is found.
- **SOCKS** proxies (`socks5://` and the like are refused).
- **HTTPS proxies**: the connection to the proxy itself must be plain `http://`. (`https://` targets still work, through a tunnel.)
- **NTLM and Kerberos** proxy authentication; only Basic is supported.

If your network needs one of these, a local proxy such as a PAC-aware or NTLM-capable forwarder can sit in between: point **Manual** at it.

## Proxy errors

The response pane shows **Proxy error** with **Open settings**. Common messages:

| Message | Meaning |
|---|---|
| *Could not connect to proxy host:port* | The proxy isn't reachable |
| *Proxy refused tunnel to host:port* | The proxy answered the `CONNECT` with an error status, often because the target is blocked |
| *Proxy requires authentication (407)* | Add credentials to the proxy URL |
| *Proxy scheme '…' is not supported; use an http:// proxy* | The proxy URL isn't `http://` |

:::note[Command line]
The `zorvik` command line doesn't read the app's settings. It always uses the system proxy (environment variables, then the OS settings on Windows and macOS). See [Command line](../../cli/overview/).
:::

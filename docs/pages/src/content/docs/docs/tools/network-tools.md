---
title: Network tools
description: TLS inspector, DNS lookup, port check, ping, network interfaces and HTTP/3 check.
sidebar:
  order: 1
---

The network tools answer questions below the API level: is the certificate right, what does DNS say, is the port open, how far away is the host, which address does this computer have, does the site speak HTTP/3.

Open the **Tools** sidebar and click a tool. Each tool opens in its own tab and keeps its input and results while you switch tabs, so you can run the same tool on two hosts side by side. The tools work without a workspace open.

| Tool | Answers |
|---|---|
| [TLS inspector](#tls-inspector) | Certificate chain, expiry, protocols and ciphers a server accepts. |
| [DNS lookup](#dns-lookup) | Any record type, from the system resolver or a server of your choice. |
| [Port check](#port-check) | Which TCP ports on a host accept connections, and how fast. |
| [Ping](#ping) | Round-trip times to a host (ICMP, or TCP when ICMP is blocked). |
| [Network interfaces](#network-interfaces) | This computer's addresses, to reach its servers from other devices. |
| [HTTP/3 check](#http3-check) | Whether a site speaks HTTP/3 (QUIC). |

## TLS inspector

Enter the **Server** (`example.com`, `example.com:8443` or a URL such as `https://example.com/path`; the port defaults to 443) and, optionally, the **Server name (SNI)** to send when it differs from the server, for example when you connect to an IP address. Press **Inspect** (**Cancel** stops it).

The inspector makes one full TLS handshake to record the certificate chain, and checks the chain the same way requests do: against this computer's trusted certificates plus the extra CA from **Settings → Certificates**. Then it makes more handshakes, a few at a time, to try each protocol version and cipher suite. The HTTP proxy from Settings is used (through a `CONNECT` tunnel) when it applies to the host.

### The report

**Summary**: **Trusted** or **Not trusted** (with the reason), the negotiated TLS version, the ALPN protocol, days until the certificate expires, the handshake time and the time of all checks. Below that: **Server name**, **Connected to** (the address, or the proxy used), **Cipher**, **Key exchange** (for example `X25519`), **Name matches** and **OCSP stapling**.

**Findings**, most severe first:

| Finding | Level | When |
|---|---|---|
| Not trusted | danger | The chain is not trusted for this name; the reason is given. |
| Expired / not valid yet | danger | The server certificate is outside its validity period. |
| Name mismatch | danger | The certificate doesn't cover the server name (the names it covers are listed). |
| Chain certificate expired | danger | An intermediate certificate has expired. |
| Expires soon | warning | The server or an intermediate certificate expires within 30 days. |
| Self-signed | warning | The server certificate is self-signed. |
| Weak key | warning | RSA or DSA below 2048 bits, or EC below 256 bits. |
| Weak signature | warning | A certificate (other than the root) is signed with SHA-1 or MD5. |
| No forward secrecy | warning | The connection uses a TLS 1.2 cipher suite without (EC)DHE. |
| No TLS 1.3 | info | The server doesn't support TLS 1.3. |

**Certificate chain**: every certificate as the server sent it, the server's own first, with subject, issuer, validity, the names it covers, the public key, the signature algorithm, SHA-256 fingerprint and serial number (with copy buttons), and **CA** and self-signed badges.

**Protocol versions**: TLS 1.3 and 1.2 are tried, each **Supported** or **Not supported**. TLS 1.1 and 1.0 show **Can't be tested**: Zorvik's TLS library implements only 1.2 and 1.3.

**Cipher suites**: each TLS 1.3 and 1.2 suite Zorvik knows, with its version, **Accepted** (**Yes**, **No** or **No answer**) and whether it has forward secrecy.

### Timeouts

Connecting and the first handshake use the connect timeout from Settings (at most 30 seconds). Each extra handshake may take 5 seconds, and the whole inspection 45 seconds; checks that couldn't start in time show no answer.

## DNS lookup

The DNS lookup tool asks one DNS question without saving a request:

- The **record type** (**A** by default, or any other, see [record types](../../protocols/dns/#record-types)) and the **name**. An IP address offers **Look up its name (PTR)**; with PTR selected, the tool shows the reverse name it asks for.
- The **resolver**: **System**, the public presets (Cloudflare, Google, Quad9 and their DNS over HTTPS services) or **Custom…** (UDP, TCP, DNS over TLS or DNS over HTTPS).
- **Recursion**: the RD flag.

Press **Look up** (or <kbd>Enter</kbd>). The answer shows every section with the flags and response code, exactly like a [DNS query request](../../protocols/dns/#the-answer). The last 8 lookups appear as chips above the answer; click one to run it again.

## Port check

Checks which TCP ports on a host accept connections.

| Field | Description |
|---|---|
| **Host** | A name or IP address, without a port (list the ports below). |
| **Ports** | **Common web** (`80,443,8080,8443`), **Top 20** (the 20 most common TCP ports, as nmap counts them) or **Custom**. |
| **Custom ports** | Ports and ranges separated by commas, spaces or semicolons: `22,80,8000-8100`. At most 1,024 different ports. |
| **Timeout** | How long to wait for each port: 500 ms, 1 s, 2 s (default) or 5 s. |

Press **Check** (**Stop** ends it early). The host is resolved once (IPv4 preferred when it has both), then up to 64 ports are tried at a time with a plain TCP connect. The proxy is not used. Results stream in, open ports first:

| Result | Meaning |
|---|---|
| **Open** | The connection was accepted; the time is shown. |
| **Closed** | The host refused the connection: nothing listens there. |
| **No answer** | No answer within the timeout; often a firewall dropping packets. |
| **Errors** | Something else went wrong (the message is shown). |

Well-known ports are labelled with their usual service (SSH, HTTPS, PostgreSQL, Redis, …).

:::caution
Only scan hosts you are allowed to test. Scanning other people's servers can be seen as an attack. The tool reminds you when you check more than 100 ports on a host outside your local network.
:::

## Ping

Measures round-trip times to a host.

| Field | Description |
|---|---|
| **Host** | A name or IP address. A port written with it (`example.com:8443`) is used for TCP instead of **TCP port**. |
| **Using** | **Auto** (default): ICMP when allowed, otherwise TCP. **ICMP**: echo requests, like the `ping` command. **TCP**: the time a TCP connection to a port takes. |
| **TCP port** | The port for TCP (default 443). In **Auto** it is the fallback port. |
| **Count** | 4, 10 (default), 50, 100 or **Until stopped**. |
| **Every** | The interval: 500 ms, 1 s (default), 2 s or 5 s. |

Press **Start** (**Stop** ends it). The host is resolved once, IPv4 preferred. One echo is sent at a time: send, wait for the reply (at most 2 seconds), then wait out the rest of the interval.

The results show **Sent**, **Received**, **Loss**, **Min**, **Avg**, **Max** and **Jitter** (the average difference between consecutive round trips), a small chart and a table of replies with their TTL (ICMP). In TCP mode, a refused connection counts as a lost reply (**Connection refused: nothing listens on port …**).

**ICMP without administrator rights**: on macOS and Linux Zorvik uses an unprivileged ICMP socket (on Linux, your user's group must be in `net.ipv4.ping_group_range`); on Windows it uses the system's ICMP API. When ICMP can't be used, **Auto** switches to TCP and a note above the results says why.

## Network interfaces

Lists this computer's network interfaces and their addresses, to reach servers running here from a phone, another computer or a container.

- **Best for other devices** suggests the address to use, with a copy button.
- The table shows each interface with its addresses, prefix length and type (IPv4 or IPv6). Loopback, down and link-local addresses are dimmed and badged.
- **Refresh** reads them again (for example after joining another network).

Remember to start your server on **Other devices too (0.0.0.0)**; see [Running servers](../../servers/running-servers/#address-and-port).

## HTTP/3 check

Checks whether a site speaks HTTP/3 over QUIC. Enter a URL or host and press **Check**. HTTP/3 only exists for `https`, so a missing scheme becomes `https://`, and `http://` is switched to `https://` (with a note).

The check sends the same `GET` twice on fresh connections, one after the other: once as usual (HTTP/1.1 or HTTP/2 over TCP) and once over HTTP/3. Each follows redirects and may take 20 seconds. Your workspace's headers, auth and cookies are **not** sent to the site.

The result:

- A **verdict**: **Speaks HTTP/3**; **Advertises HTTP/3 but the QUIC connection failed (UDP blocked?)** (a firewall, VPN or corporate network may block UDP port 443, and browsers then quietly fall back to HTTP/2); **Advertises HTTP/3 but the HTTP/3 request failed**; **No HTTP/3**; **Could not reach the site**; or **Not checked: a proxy is set for this host** (HTTP/3 can't go through an HTTP proxy; add the host to the proxy bypass list in Settings).
- **Alt-Svc advertised**: the `h3` entries of the site's `Alt-Svc` header with how long browsers may remember them, **No HTTP/3 entry**, **No Alt-Svc header**, or `clear`. A site can answer over QUIC without advertising `h3`; browsers then only use HTTP/3 when a DNS `HTTPS` record announces it.
- **Comparison** of the two requests: result, protocol, total time, DNS lookup, handshake, waiting (TTFB), remote address, TLS version and cipher, ALPN and the final URL.

---
title: DNS
description: Look up any DNS record type with the system resolver or a server of your choice, over UDP, TCP, DNS over TLS or DNS over HTTPS.
sidebar:
  order: 7
---

A DNS query asks one question (a name and a record type) and shows the whole answer the way `dig` does: the response code, the header flags and every section. `NXDOMAIN` and other response codes are results, not errors.

## Create a DNS query

In the **Collection** sidebar, open the **New** menu (**+**) and choose **New DNS query**.

- The **URL field** holds the name to look up, for example `example.com`.
- The **record type** picker sits left of it, where HTTP requests have the method (new queries use `A`).
- **Query** (or <kbd>Mod</kbd>+<kbd>Enter</kbd>) sends it; **Cancel** stops a query in flight.
- The **Resolver** tab chooses the server, recursion and timeout. There is also a **Docs** tab.

The [DNS lookup tool](../../tools/network-tools/#dns-lookup) does the same without saving a request.

## The name

- Type a plain name. When you paste a URL or `host:port`, only the host is used: `https://api.example.com:8443/v1` asks for `api.example.com`.
- Names are absolute: your computer's search domains are never appended.
- International names are converted for you (`bücher.example` becomes `xn--bcher-kva.example`).
- To find the name of an IP address, choose **PTR** and type the address: `192.0.2.10` asks for `10.2.0.192.in-addr.arpa`, IPv6 addresses for their `ip6.arpa` name. An IP address with any other type is refused (**Use PTR to look up an IP address**).
- `{{variables}}` work in the name and in the server. A variable that is not defined stops the query instead of looking up the literal text.

## Record types

The picker offers the common types; **Other type…** at the bottom takes any type name or number.

| Type | Answers with |
|---|---|
| `A` | IPv4 addresses |
| `AAAA` | IPv6 addresses |
| `CNAME` | The name this one is an alias of |
| `MX` | Mail servers, with preference |
| `TXT` | Text, such as SPF, DKIM and site verification |
| `NS` | Name servers of the zone |
| `SOA` | Zone authority and serial |
| `SRV` | Services: priority, weight, port, target |
| `PTR` | The name of an IP address (reverse) |
| `CAA` | Certificate authorities allowed to issue certificates |
| `ANY` | Everything (servers often refuse this) |
| Other | Any type name (`HTTPS`, `DS`, `DNSKEY`, …) or number (`TYPE65`) |

Zone transfers (`AXFR`, `IXFR`) and `OPT` can't be queried.

## Resolver

The **Resolver** tab has one-click presets and a custom field:

| Preset | Server |
|---|---|
| **System** | The DNS servers in this computer's network settings (VPN and corporate DNS included). |
| **Cloudflare** | `1.1.1.1` over UDP (TCP when the answer is large). |
| **Google** | `8.8.8.8` over UDP (TCP when the answer is large). |
| **Quad9** | `9.9.9.9` over UDP; blocks known malicious domains. |
| **Cloudflare DoH** | `https://cloudflare-dns.com/dns-query` (DNS over HTTPS). |
| **Google DoH** | `https://dns.google/dns-query` (DNS over HTTPS). |
| **Custom…** | Any server, see below. |

Custom servers are written like this:

| You type | Transport | Default port |
|---|---|---|
| empty or `system` | The system resolver | |
| `1.1.1.1`, `8.8.8.8:53`, `[2606:4700::1111]`, `dns.example` | UDP, retried over TCP when the answer is truncated | 53 |
| `udp://host[:port]` (or `dns://`) | UDP, as above | 53 |
| `tcp://host[:port]` | TCP | 53 |
| `tls://host[:port]` (or `dot://`) | DNS over TLS (DoT) | 853 |
| `https://host/path` | DNS over HTTPS (DoH), `POST` with `application/dns-message`. A URL without a path uses `/dns-query`. `http://` is accepted for local test servers. | 443 |

The transport shows as a badge next to the field (UDP, TCP, DoT or DoH).

### How the transports differ

| | UDP | TCP | DoT | DoH |
|---|---|---|---|---|
| Encrypted | No | No | Yes | Yes |
| Uses the HTTP proxy | No | No | No | Yes |
| Large answers | Truncated, then asked again over TCP | Up to 65,535 bytes | Up to 65,535 bytes | Up to 65,535 bytes |

DoT and DoH use the same TLS settings as HTTPS requests: this computer's trusted certificates, **Settings → Certificates**, and certificate verification. When a host name has several addresses, UDP tries up to four of them.

### Choosing a resolver

- **System** shows what other apps on this computer see, including VPN and corporate DNS. Use it to debug "works on my machine" problems.
- **A public resolver** (Cloudflare, Google, Quad9) shows what the internet at large sees, bypassing your local DNS.
- **The zone's own name server** (`tcp://ns1.example.com` or its address), with **Ask for recursion (RD)** turned off, shows what the authoritative server says before any cache is involved, for example right after you changed a record.
- **DoT or DoH** checks encrypted DNS, or gets an answer when plain DNS is blocked or tampered with. DoH also works through an HTTP proxy.
- **TCP** is useful for large answers (many `TXT` records, DNSSEC types) or when UDP port 53 is blocked.

### The system resolver

**System** reads this computer's DNS servers from its network settings (`resolv.conf`, the macOS network configuration, or the Windows network adapters) and asks them directly, up to three of them in turn, sharing the time between them. Scoped link-local servers such as `fe80::1%en0` are skipped.

When the DNS settings can't be read at all, `A` and `AAAA` queries fall back to the operating system's own lookup (which also reads the hosts file); those answers show a TTL of 0. Other types then fail with a message suggesting a server in the **Resolver** tab.

Because other apps use the operating system's lookup, not the DNS servers directly, the two can differ (hosts file entries, mDNS names, VPN split DNS). When an `A` or `AAAA` answer is empty or `NXDOMAIN` but the system still resolves the name, a note says so: **This computer still resolves the name to 10.0.0.5 (hosts file, mDNS or VPN settings), which is what other apps connect to.**

## What is sent

- **Recursion desired (RD)** is set unless you turn off **Ask for recursion (RD)**.
- **AD** is set, asking the resolver to say whether it validated the answer with DNSSEC.
- **EDNS** is used with a 1,232-byte UDP payload size. A server that answers `FORMERR` without EDNS is asked again without it (a note says so).
- Over UDP the query is resent after 1, 2, 4… seconds until the timeout.
- **Timeout** (in the **Resolver** tab): the whole query including a retry over TCP. Empty or `0` means 5000 ms.

## The answer

The result shows:

- The **response code** (`NOERROR`, `NXDOMAIN`, `SERVFAIL`, `REFUSED`, …), the total time, the answer's size, and the server and transport that answered (a lock for DoT and DoH).
- The header **flags**: **AA** (authoritative answer), **TC** (truncated), **RD** (recursion desired), **RA** (recursion available), **AD** (DNSSEC-validated), **CD** (checking disabled). Hover a flag for its meaning.
- The **Answer**, **Authority** and **Additional** sections: name, type, class, TTL and data for each record. **Copy all records** copies them.
- **Notes**, for example about a retry over TCP.

## Saved format

```yaml title="requests/DNS/Mail servers.yaml"
name: Mail servers
kind: dns
method: MX
url: example.com
dns:
  server: tls://1.1.1.1
  recursion: true
settings:
  timeoutMs: 3000
```

| Field | Default | Description |
|---|---|---|
| `kind` | | `dns`. |
| `method` | `A` | The record type. |
| `url` | | The name to look up (an IP address for `PTR`). |
| `dns.server` | empty (system) | The resolver, as in the custom field above. |
| `dns.recursion` | `true` | Ask for recursion (RD flag). |
| `settings.timeoutMs` | 5000 | Timeout for the whole query. |

:::tip[Your own DNS answers]
To make names resolve the way you want while testing, run Zorvik's DNS server with your own records. See [TCP, UDP & DNS servers](../../servers/tcp-udp-dns-servers/#dns-server).
:::

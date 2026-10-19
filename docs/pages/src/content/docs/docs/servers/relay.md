---
title: TCP relay
description: Put a relay between a client and a TCP server to see the bytes in both directions, including decrypted TLS traffic, inject your own, and add delays, bandwidth limits and dropped connections.
sidebar:
  order: 8
---

A TCP relay sits between a client and a TCP server. The client connects to the relay instead of the server; the relay connects to the server (the **target**) and passes everything along in both directions, showing each chunk in the traffic log. Use it to see what a database driver, a Redis client, an SMTP library or an IoT device actually sends and receives, without changing the client or the server.

## Create a relay

Open the **Servers** sidebar, click **+** (**New server**) and choose **TCP relay**. A new relay listens on `127.0.0.1`, port 9100 (or the next port no other saved server uses), with `example.com:80` as its target to show the format.

1. Set **Target server** to the server clients should reach, for example `127.0.0.1:5432`.
2. Press **Start**.
3. Point your client at the relay's address (shown under **How to use**, with a copy button) instead of the target.

```text
client  ──►  relay 127.0.0.1:9100  ──►  target db.internal:5432
        ◄──                        ◄──
```

## Target

| You type | Connects to |
|---|---|
| `host:port` | The target over plain TCP. |
| `[::1]:5432` | An IPv6 address, in brackets. |
| `tcp://host:port` | Same as `host:port`. |
| `tls://host:port` | The target with TLS (like turning on the switch below). |

The port is required. The field warns about a missing port, spaces, slashes, or an IPv6 address without brackets.

**Connect to the target with TLS**: the relay speaks TLS to the target while clients still talk plain TCP to the relay, so the traffic log shows the **decrypted** conversation. The target's certificate is checked against this computer's trusted certificates and **Settings → Certificates**, unless **Verify TLS certificates** is off in Settings (with `zorvik serve`, `-k`).

The relay itself has no TLS listener: clients always connect to it with plain TCP.

A relay won't start without a valid target, and stops with **The target … is this relay itself: point it at another server** when the target is its own address and port.

Changing the target applies to the next client connection; open ones keep their target.

## Connections

For each client that connects, the relay:

1. Logs **#1 connected from 127.0.0.1:53110**.
2. Connects to the target directly (the HTTP proxy is not used). Name lookup, TCP connect and TLS must finish within 10 seconds.
3. Logs **#1 relayed to db.internal:5432 (10.0.4.7:5432)**, with the TLS version (for example **over TLS 1.3**) when it uses TLS.
4. Copies bytes both ways until the pair closes.

When the target can't be reached, the log shows **Could not connect to the target …** and the client's connection is closed (**target unreachable: …**).

**Half-close**: when one side stops sending (closes its writing half), the relay passes that on to the other side and keeps the other direction open. The pair closes when both sides are done, on an error, or when you disconnect it. The close reason names who closed first: **client closed** or **target closed**.

## Faults

Open **Faults** to make the connection between client and target slow or unreliable, for example to see how a database driver copes with a slow network, or whether a client reconnects when its connection drops in the middle of a transfer. A field set to `0` is off; the number of faults that are on shows next to **Faults**.

| Field | What it does |
|---|---|
| **Delay per chunk (ms)** | Each chunk waits this long before it is passed on, in both directions. A request and its answer each wait once, so a round trip takes twice the delay longer. |
| **Bandwidth (bytes/s)** | At most this many bytes a second in each direction, sent evenly (in twentieths of a second). The relay reads no faster than it sends, so a fast sender is slowed down too. |
| **Drop after (ms)** | Closes both connections this long after the target connected. |
| **Drop after (bytes)** | Closes both connections once this many bytes went through, both directions together. Exactly that many arrive: a chunk that crosses the limit is cut. |

A dropped connection's close reason says so: **dropped by the relay after 5000 ms** or **dropped by the relay after 1234 bytes**. Delays and dropping times are at most 24 hours. Like the target, faults apply to new connections: change them and connect again.

Data you send from the traffic panel is not delayed or limited, and doesn't count toward **Drop after (bytes)**.

## Traffic

Each chunk the relay passes on is one row in the traffic log:

| Row | Direction |
|---|---|
| **→ target** | From the client, passed to the target. |
| **← target** | From the target, passed to the client. |
| outgoing arrow | Sent by you from the traffic panel, to the client. |

A chunk is whatever one read returned (up to 64 KB), not a protocol message: a message may span rows, or one row may hold several. Payloads show as text when they are valid UTF-8, otherwise as hex; expand a row to see all of it.

## Sending to a client

The composer in the traffic panel sends bytes **to the client, as if the target had sent them**, between the target's own chunks. Pick **All clients** or one connection, choose **Text** or **Hex**, and press **Send**. Nothing is added: no line ending, no framing. You can also send while the relay is still connecting to the target.

At most 256 sends can wait for a client that isn't reading; beyond that, **Not sent: the client is not reading (256 sends are still waiting)**. **Disconnect** (the unplug icon on a row) closes both sides of that connection.

## Saved format

```yaml title="servers/Postgres relay.yaml"
name: Postgres relay
kind: tcpProxy
seq: 0
host: 127.0.0.1
port: 9100
proxy:
  target: db.internal:5432
  upstreamTls: false
  delayMs: 200
  bytesPerSecond: 50000
```

| Field | Default | Description |
|---|---|---|
| `kind` | | `tcpProxy`. |
| `proxy.target` | empty | `host:port` (or `tcp://…`, `tls://…`). Required. |
| `proxy.upstreamTls` | `false` | Connect to the target with TLS. |
| `proxy.delayMs` | `0` | Delay per chunk, in ms. |
| `proxy.bytesPerSecond` | `0` | Bandwidth limit per direction (0: none). |
| `proxy.dropAfterMs` | `0` | Close both connections this long after the target connected (0: never). |
| `proxy.dropAfterBytes` | `0` | Close both connections after this many bytes, both directions together (0: never). |

For TCP servers that answer by themselves, see [TCP, UDP & DNS servers](../tcp-udp-dns-servers/). To talk to a TCP server from Zorvik, use a [TCP connection](../../protocols/tcp-udp/).

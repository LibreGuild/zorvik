---
title: TCP & UDP
description: Raw TCP (with or without TLS) and UDP sockets, with text or hex messages, framing and line endings.
sidebar:
  order: 6
---

TCP and UDP requests talk to a server below HTTP: you connect, send messages from the composer and see every byte that comes back. Use them for line protocols (Redis, SMTP, custom daemons), binary protocols with length prefixes, game or IoT servers, and anything else that isn't HTTP.

## TCP connections

In the **Collection** sidebar, open the **New** menu (**+**) and choose **New TCP connection**.

| Address | Connection |
|---|---|
| `tcp://host:port` | Plain TCP. |
| `tls://host:port` (or `ssl://host:port`) | TCP with TLS. |
| `host:port` | No scheme means `tcp://`. |

The port is required. `{{variables}}` work in the address.

Press **Connect**. The log shows **Connected · TCP · 127.0.0.1:9000 · 2 ms** (with the TLS version for `tls://`, for example `TCP + TLS 1.3`). **Disconnect** first lets messages still being written go out (for up to 5 seconds), then closes the connection. When the server closes it, the log says **Disconnected (Closed by server)**; bytes of an unfinished message are shown as a last message first.

- **Proxy**: when an HTTP proxy applies to the host, the connection goes through a `CONNECT` tunnel.
- **TLS**: verified with this computer's trusted certificates and **Settings → Certificates** (including a client certificate). Turn verification off with **Verify TLS certificates** in the request's **Settings** tab. No ALPN protocol is offered.
- **Timeout**: the connect timeout from Settings applies.

## UDP sockets

Choose **New UDP socket**. The address is `udp://host:port` (or `host:port`).

**Connect** only opens a local socket: nothing is sent until your first message, because UDP has no handshake. When the host name has both IPv4 and IPv6 addresses, IPv4 is used (local servers usually listen on `127.0.0.1` while `localhost` may resolve to `::1` first).

- Each message you send is **one datagram** to the target.
- Every datagram that arrives on the socket is shown **with its sender**, also from other addresses (for example answers to a broadcast).
- **Allow broadcast addresses (e.g. 255.255.255.255)** in the **Options** tab lets you send to broadcast addresses.
- UDP never uses a proxy; proxy settings are ignored.
- A datagram holds at most 65,507 bytes. Your system may allow less (macOS allows 9,216 bytes by default); the error then comes from the system.
- When the target sends back an ICMP "port unreachable", the log shows **Nothing is listening at 127.0.0.1:9001 (port unreachable)**. The socket stays open.

## Sending messages

The composer has three modes:

| Mode | Sends | `{{variables}}` | Line ending added |
|---|---|---|---|
| **Text** | The text as UTF-8 | Replaced | Yes (if set) |
| **JSON** | The text as UTF-8, with JSON highlighting | Replaced | Yes (if set) |
| **Binary (hex)** | The bytes you type in hex | Not replaced | Never |

Hex bytes may be written as `48 65 6c 6c 6f`, `48656c6c6f`, `48:65:6c` or `0x48,0x65`. Press **Send** or <kbd>Mod</kbd>+<kbd>Enter</kbd>.

Received messages are shown as text when they are valid UTF-8, otherwise as hex. Click a row to see the whole message.

## Options

The **Options** tab controls how messages are cut and what is added to them.

### Add to each text message

| Setting | Appended |
|---|---|
| **Nothing** (default) | nothing |
| **\n (LF)** | a line feed |
| **\r\n (CRLF)** | carriage return + line feed |

It applies to Text and JSON messages, on TCP and UDP. Binary messages are sent exactly as typed.

### Message framing (TCP)

TCP is a stream of bytes; it doesn't keep message boundaries. **Message framing** decides how Zorvik cuts incoming bytes into messages, and what it adds to the messages you send.

| Framing | Incoming bytes | Messages you send |
|---|---|---|
| **As they arrive** (default) | Each chunk read from the socket is one message (up to 64 KB). One message from the server may show as several chunks, or several as one. | Sent as they are. |
| **One message per line** | Split at `\n`; a `\r` before it is dropped. A line longer than 16 MB is cut into pieces. | Sent as they are: choose a line ending above, the framing doesn't add one. |
| **Length prefix (big-endian)** | Each message starts with its length as a big-endian number of **Prefix size** bytes (1, 2 or 4). A length over 16 MB closes the connection. | The same prefix is added. A message too long for the prefix (over 255 bytes for 1 byte, 65,535 for 2) is refused with an error. |

The line ending is added before the prefix, so the prefix counts it.

UDP has no framing: one datagram is always one message.

## Saved format

```yaml title="requests/Redis ping.yaml"
name: Redis ping
kind: tcp
url: tcp://localhost:6379
socket:
  framing: line
  lineEnding: crLf
body:
  type: text
  text: PING
```

| Field | Values | Default | Description |
|---|---|---|---|
| `kind` | `tcp`, `udp` | | |
| `socket.framing` | `raw`, `line`, `lengthPrefixed` | `raw` | Message framing (TCP). |
| `socket.lengthBytes` | `1`, `2`, `4` | `2` | Prefix size for `lengthPrefixed`. |
| `socket.lineEnding` | `none`, `lf`, `crLf` | `none` | Added to text messages. |
| `socket.broadcast` | `true`, `false` | `false` | UDP: allow broadcast addresses. |
| `body.text` | | | The composer text. |

:::tip[Servers to test against]
Zorvik runs TCP and UDP servers too (echo, reply rules, or manual replies), and a TCP relay that shows what a client and a server say to each other. See [TCP, UDP & DNS servers](../../servers/tcp-udp-dns-servers/) and [TCP relay](../../servers/relay/).
:::

---
title: TCP, UDP & DNS servers
description: Run TCP and UDP servers that echo or answer by rules in text or hex, and a DNS server that answers with your own records.
sidebar:
  order: 6
---

Below HTTP, Zorvik runs three kinds of servers: **TCP** and **UDP** servers that answer messages (echo, reply rules or by hand), and a **DNS server** that answers with your own records and forwards the rest.

All three are created from the **Servers** sidebar: click **+** (**New server**) and choose **TCP server**, **UDP server** or **DNS server**.

## Reply modes

TCP and UDP servers share the **Replies** setting with the WebSocket server:

| Mode | What happens with each message |
|---|---|
| **Echo** (default) | It is sent back. |
| **Rules** | The first matching reply rule answers; other messages get no reply. |
| **Manual** | Nothing is sent automatically: reply from the traffic panel. |
| **Discard** | It is read and dropped. |

Reply rules (**When**: **Contains**, **Is exactly**, **Matches regex**, **Any message**; the **Reply**; a **Delay ms**) work as described in [Reply rules](../websocket-and-sse-servers/#reply-rules). What TCP and UDP add is a choice of **encoding** and, for TCP, **framing**.

## Text or hex

The **Messages** section sets how the greeting, rule patterns and replies are written: **Rules and greeting as** (TCP) or **Rules and replies as** (UDP).

| Encoding | Patterns and replies | Templates |
|---|---|---|
| **Text** (default) | Text (UTF-8). | Replies and the greeting may use `{{message}}`, dynamic values and variables. See [Templates](../templates/). |
| **Hex bytes** | Hex, for example `48 65 6c 6c 6f`, `0x48,0x65` or `48:65:6c`. **Contains** and **Is exactly** patterns are hex too. | None: hex is sent as written. |

In hex mode, **Matches regex** and **Any message** still use the pattern as a regular expression, matched against the raw bytes (write `\x01` for a byte; for bytes from `\x80` up, use `(?-u:\xff)`). A pattern or reply that isn't valid hex skips the rule, and the traffic log says why, for example **Rule 2: reply Hex needs two digits per byte**.

Incoming messages are always shown as text when they are valid UTF-8, otherwise as hex.

**Line ending on replies** (**None**, **\n (LF)** or **\r\n (CRLF)**) is added to text replies, text greetings and text you send from the traffic panel.

## TCP server

A new TCP server listens on `127.0.0.1`, port 9000 (or the next port no other saved server uses). Its address is `tcp://127.0.0.1:9000`, or `tls://…` with **TLS** on.

### Message framing

TCP delivers a stream of bytes. **Message framing** decides how the server cuts it into messages (for the log and for rules), and what it adds to what it sends:

| Framing | Incoming | Outgoing |
|---|---|---|
| **As they arrive** (default) | Each chunk read from the connection (up to 64 KB) is a message. A message may arrive in pieces, or several in one chunk. | Sent as is. Text gets the line ending. |
| **One message per line** | Split at `\n`; a `\r` before it is dropped. A line over 16 MB is cut into pieces. | **Every** message ends with a line break: the chosen line ending, or `\n` when it is **None**. Echoed lines get their line break back. |
| **Length prefix (big-endian)** | Each message starts with its length as a big-endian number of **Prefix size** bytes (1, 2 or 4). A length over 16 MB closes the connection. | Each message gets the same prefix (counting a line ending, if any). |

What each mode sends:

| Mode | As they arrive | One message per line | Length prefix |
|---|---|---|---|
| **Echo** | The bytes as received | The line + line ending | Prefix + the message |
| Text reply or greeting | Reply + line ending | Reply + line ending | Prefix + reply + line ending |
| Hex reply or greeting | The bytes | The bytes + line ending | Prefix + the bytes |

The framing, prefix size, line ending and greeting are taken when a client connects, so changes to them apply to new connections. The reply mode, the rules and the encoding of the rules apply at once.

### Greeting

**Greeting** is sent to each client right after it connects (after the TLS handshake with **TLS** on). Empty: none. In text encoding it may use variables and dynamic values, for example `220 Welcome {{$uuid}}` (`{{message}}` is empty here); in hex encoding it is hex bytes.

### Connections

- Each client gets a number (`#1`, `#2`, …) in the log, in the order it connected.
- When a client disconnects in the middle of a framed message, the bytes are logged as **unfinished message**.
- With **TLS** on, the handshake must finish within 10 seconds.
- **Disconnect** in the traffic panel closes the client's connection (a TCP shutdown); stopping the server closes every client.

## UDP server

A new UDP server listens on `127.0.0.1`, port 9001 (or the next free one). Its address is `udp://127.0.0.1:9001`. UDP has no TLS.

- **One datagram is one message.** Framing doesn't apply, and there is no greeting.
- **Senders are pseudo-connections.** UDP has no connections, so each sender address gets a number (`#1 connected from 127.0.0.1:55012`) on its first datagram. It can then be picked in the traffic panel.
- A sender that sends nothing for **5 minutes** is forgotten (**idle for 5 minutes**). **Disconnect** forgets it right away (**forgotten by you**). At most 10,000 senders are remembered; beyond that the least active are forgotten, 100 at a time.
- **Echo** sends the datagram back unchanged. Rule replies and text from the traffic panel get the line ending.
- A datagram holds at most 65,507 bytes. Longer sends fail with **The message is … bytes; a UDP datagram holds at most 65507 bytes**.
- At most 1,000 delayed rule replies wait at once; more are dropped (and logged).
- Sending to **All clients** goes to every remembered sender; when there is none yet, you get **No client has sent a datagram yet**.
- An ICMP "port unreachable" for an earlier reply doesn't disturb the server.

## DNS server

A DNS server answers questions for the names you give records, and sends every other question elsewhere, or answers "no such name". Use it to point an app at a test host without editing the hosts file, to test how a client handles `NXDOMAIN`, `CNAME` chains or `SRV` records, or to watch what a device looks up.

A new DNS server listens on `127.0.0.1`, port **1053** (not 5353, which is mDNS and usually taken), with one record (`api.example.test`, `A`, `127.0.0.1`) and **This computer's resolver** for other names. It answers over **UDP and TCP on the same port**. When the TCP port can't be opened, the server still runs, over UDP only, and the log says so.

### Records

Each row of **Records** has a checkbox, **Name**, **Type**, **Value** and **TTL s** (seconds, 60 by default). Unchecked records are ignored.

| Type | Value | Example |
|---|---|---|
| `A` | An IPv4 address | `127.0.0.1` |
| `AAAA` | An IPv6 address | `::1` |
| `CNAME` | The name this one is an alias of | `api.example.test` |
| `TXT` | Text. One plain string, or several `"quoted" "strings"` (`\"` for a quote inside). Strings longer than 255 bytes are split. | `v=spf1 -all` |
| `MX` | `preference host` | `10 mail.example.test` |
| `NS` | A name server | `ns1.example.test` |
| `PTR` | A name. The record's **Name** may be an IP address. | Name `127.0.0.1`, value `api.example.test` |
| `SRV` | `priority weight port target` | `10 5 5060 sip.example.test` |
| `CAA` | `flags tag value` (tag of 1–15 letters or digits, value optionally quoted) | `0 issue letsencrypt.org` |

The editor marks obvious mistakes as you type. A record the server can't use is skipped, and the log says which and why, for example **DNS record 2 (A a.test) skipped: 'not-an-ip' is not an IPv4 address (e.g. 127.0.0.1)**.

**How names match:**

- In any case, with or without a trailing dot. International names are converted to their `xn--` form.
- **Wildcards**: `*.example.test` answers every name below `example.test` at any depth (`a.example.test`, `a.b.example.test`), but not `example.test` itself. A bare `*` answers every name. The `*` must be the whole first label (`a.*.test` is refused).
- A name that has records of its own never uses a wildcard, even for other types. When several wildcards match, the most specific one (with the most labels) wins.
- **CNAME**s are followed within your records, up to 8 steps (a loop is shown once). A `CNAME` that points to a name outside your records is looked up where other names go (see below), and the answer is added.
- Without forwarding, a question for a type the name doesn't have gets `NOERROR` with no answer. So do names above your records: with a record for `api.example.test`, a question for `example.test` gets `NOERROR`, not `NXDOMAIN`. With forwarding, these questions go where other names go (see below).
- `ANY` returns every record of the name. Only class `IN` (or `ANY`) is answered from the records.

### Other names

**Names without a record** decides where other questions go:

| Choice | In the file | Answers |
|---|---|---|
| **Answer NXDOMAIN (no forwarding)** | `upstream` empty | `NXDOMAIN` ("no such name"), marked authoritative. |
| **This computer's resolver** | `system` | Addresses (`A`, `AAAA`, `ANY`) looked up with this computer's DNS settings, with a TTL of 30 seconds. Other types get `NOTIMP` (or `NOERROR` with no answer for names that exist in your records). |
| **Another server…** | an address | The question is forwarded to that server over UDP and its answer passed back. TCP clients get a truncated answer fetched again over TCP. |

The server address is an IP with an optional port: `1.1.1.1`, `192.168.1.1:53`, `2606:4700::1111` or `[2606:4700::1111]:53` (port 53 by default). Host names are not accepted, and neither is the DNS server's own address. A server that doesn't answer within 3 seconds gives `SERVFAIL`; so does an upstream setting that can't be used (the log says why).

Answers from your records are authoritative (**AA**); **RA** (recursion available) is set when there is a resolver or server to forward to.

### Protocol details

| | |
|---|---|
| UDP answer size | 512 bytes, or what the client offers with EDNS, up to 4,096. A larger answer is sent with the **TC** flag and only the question, so the client retries over TCP. |
| TCP | Answers up to 65,535 bytes. At most 64 connections at once; a connection that sends nothing for 10 seconds is closed. |
| Queries at once | 256; more are dropped (and logged). |
| Other opcodes than a standard query | `NOTIMP`. |
| Unreadable queries | `FORMERR`. |
| EDNS version above 0 | `BADVERS`. |
| Responses sent to the server | Dropped. |

### Try it

While the server runs, **Try it** shows commands that query it, for example:

```sh
# macOS / Linux
dig @127.0.0.1 -p 1053 api.example.test
dig @127.0.0.1 -p 1053 api.example.test +tcp

# Windows
nslookup -port=1053 api.example.test 127.0.0.1
```

In Zorvik, a [DNS query](../../protocols/dns/) with the custom server `127.0.0.1:1053` does the same.

Each query shows in the traffic log as one line, such as **A api.example.test → 127.0.0.1**, **AAAA nope.test → NXDOMAIN** or **A example.com → forwarded to 1.1.1.1 · NOERROR · 93.184.215.14** (with **· TCP** for queries over TCP). Expand it for the full answer, the way `dig` prints it.

:::note[Using it for the whole computer]
Operating systems send DNS to port 53. To use this server as a computer's or phone's DNS server, listen on port 53 (ports below 1024 may need administrator rights) and, for other devices, on `0.0.0.0`.
:::

## Saved format

```yaml title="servers/Line echo.yaml"
name: Line echo
kind: tcp
seq: 0
host: 127.0.0.1
port: 9000
socket:
  mode: rules
  greeting: 220 ready
  framing: line
  lineEnding: crLf
  rules:
    - match: exact
      pattern: PING
      reply: PONG
    - match: any
      reply: "you said: {{message}}"
```

```yaml title="servers/Test DNS.yaml"
name: Test DNS
kind: dns
seq: 0
host: 127.0.0.1
port: 1053
dns:
  records:
    - name: api.example.test
      type: A
      value: 127.0.0.1
      ttl: 60
    - name: "*.preview.example.test"
      type: CNAME
      value: api.example.test
      ttl: 60
    - name: example.test
      type: MX
      value: 10 mail.example.test
      ttl: 300
  upstream: system
```

TCP and UDP settings (`socket`):

| Field | Default | Description |
|---|---|---|
| `mode` | `echo` | `echo`, `rules`, `manual` or `discard`. |
| `greeting` | empty | TCP: sent to each client on connect. |
| `rules` | none | Reply rules (`match`, `pattern`, `reply`, `delayMs`, `enabled`); see [Reply rules](../websocket-and-sse-servers/#saved-format). |
| `encoding` | `text` | `text` or `hex`. |
| `framing` | `raw` | TCP: `raw`, `line` or `lengthPrefixed`. |
| `lengthBytes` | `2` | TCP: prefix size, `1`, `2` or `4`. |
| `lineEnding` | `none` | `none`, `lf` or `crLf`. |

DNS settings (`dns`):

| Field | Default | Description |
|---|---|---|
| `records[].name` | | The name, `*.suffix`, `*`, or an IP address for `PTR`. |
| `records[].type` | | `A`, `AAAA`, `CNAME`, `TXT`, `MX`, `NS`, `PTR`, `SRV` or `CAA`. |
| `records[].value` | | The data, as in a zone file. |
| `records[].ttl` | `60` | Seconds. |
| `records[].enabled` | `true` | Whether the record is used. |
| `upstream` | empty | Empty (NXDOMAIN), `system`, or a server address. |

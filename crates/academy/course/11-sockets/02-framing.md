---
id: tcp-framing
title: "Framing: where does a message end?"
summary: A TCP stream has no message boundaries, so protocols mark them, with a line break or with a length in front.
minutes: 6
lab:
  title: Lines and boxes
  goal: Talk to a line-based server and a length-prefixed server, and see what happens when the framing doesn't match.
  minutes: 8
  servers:
    lines:
      name: Line counter
      kind: tcp
      socket:
        mode: rules
        framing: line
        rules:
          - { match: regex, pattern: "(?i)^order ", reply: "ACCEPTED: {{message}} (ticket {{secret.ticket}})" }
          - { match: any, reply: "ERROR: start your line with ORDER, for example ORDER tea" }
    boxes:
      name: Box counter
      kind: tcp
      socket:
        mode: rules
        framing: lengthPrefixed
        lengthBytes: 2
        rules:
          - { match: regex, pattern: "(?i)^ping\\s*$", reply: "PONG {{secret.word}}" }
          - { match: any, reply: "ERROR: send PING" }
  steps:
    - text: |
        Open a **TCP connection** to `{{lines}}`, press **Connect** and send `ORDER tea`. Wait a moment: nothing comes back, because the server is still waiting for the end of the line.

        Now press **Disconnect**, open the **Options** tab, set **Add to each text message** to **\n (LF)**, press **Connect** again and send `ORDER tea` once more.
      hints:
        - "This server reads one line at a time. A line only ends with a line break, and by default Zorvik sends exactly what you type, nothing more."
        - "Add to each text message is in the Options tab of the TCP request. Options apply when you connect, so disconnect and connect again after changing it."
        - "Options, Add to each text message: \\n (LF). Connect to {{lines}}, send ORDER tea. The reply starts with ACCEPTED."
      check:
        message: { server: lines, direction: in, text: "ORDER *", summary: "!unfinished*" }
      solution:
        - call: { method: socket.connect, params: { connId: lab-tcp, request: { name: Line counter, kind: tcp, url: "{{lines}}", socket: { lineEnding: lf } } } }
        - call: { method: socket.send, params: { connId: lab-tcp, message: { type: text, text: ORDER tea } } }
        - wait: 200
    - text: The server accepted your order and gave you a ticket. Type the ticket here.
      hints:
        - "The reply has a blue arrow and starts with ACCEPTED."
        - "The ticket is inside the brackets. The server's side shows it too: open Lab · Line counter under Servers and look at its Traffic."
        - "Copy what comes after the word ticket, without the closing bracket."
      check:
        answer: "{{secret.ticket}}"
      solution:
        - answer: "{{secret.ticket}}"
    - text: |
        The second server wants every message in a box: a 2-byte length, then the bytes. Open a **TCP connection** to `{{boxes}}`. In **Options**, set **Message framing** to **Length prefix (big-endian)** and **Prefix size** to **2 bytes**. Then **Connect** and send `PING`.
      hints:
        - "With a length prefix, Zorvik puts the message's length in front of it (and reads replies the same way). No line break is needed."
        - "Message framing and Prefix size are both in the Options tab. Set them before you press Connect."
        - "Options: Message framing Length prefix (big-endian), Prefix size 2 bytes. Connect to {{boxes}}, send PING. The reply starts with PONG."
      check:
        message: { server: boxes, direction: in, text: PING, summary: "!unfinished*" }
      solution:
        - call: { method: socket.connect, params: { connId: lab-tcp, request: { name: Box counter, kind: tcp, url: "{{boxes}}", socket: { framing: lengthPrefixed, lengthBytes: 2 } } } }
        - call: { method: socket.send, params: { connId: lab-tcp, message: { type: text, text: PING } } }
        - wait: 200
    - text: What word came after `PONG` in the box counter's reply?
      hints:
        - "Zorvik removed the 2-byte length from the reply and shows you only the message."
        - "Look at the newest received message, or at the Traffic of Lab · Box counter under Servers."
        - "The reply reads PONG followed by a word with a number. Type that word, number included."
      check:
        answer: "{{secret.word}}"
      solution:
        - answer: "{{secret.word}}"
        - call: { method: socket.close, params: { connId: lab-tcp } }
quiz:
  - question: You send HELLO twice, quickly, over TCP. What can the server read?
    options:
      - Always two separate pieces of 5 bytes
      - HELLOHELLO in one piece, or the 10 bytes split anywhere
      - Only the second HELLO
    answer: 1
    explain: TCP delivers a stream. The reads the server gets need not match your sends, which is why messages need framing.
  - question: A message starts with the 2-byte length prefix 00 05. How long is the message after it?
    options:
      - 5 bytes
      - 2 bytes
      - 500 bytes
    answer: 0
    explain: 00 05 is the number 5 written in two bytes, most significant byte first (big-endian).
  - question: You send ORDER tea to a line-based server and nothing ever comes back. What's the likely cause?
    options:
      - TCP lost your message
      - The server only accepts binary data
      - Your message didn't end with a line break, so the server is still waiting for the rest of the line
    answer: 2
    explain: TCP doesn't lose bytes silently. A line-based server needs the line break to know your message is complete.
---

In the last lesson you learned that TCP is a **stream of bytes**, not a series of messages. Send `ORDER tea` and then `ORDER cake`, and the server may read `ORDER teaORDER cake` in one go, or `ORD` and then the rest. So how does it know where one message ends and the next begins?

Every protocol on top of TCP answers that question with **framing**: a rule for marking message boundaries. There are three common ones.

## 1. A delimiter, usually a line break

The message ends at a special character, almost always a line break. There are two flavours: **LF** (line feed, written `\n`) and **CRLF** (carriage return plus line feed, `\r\n`). Text protocols such as SMTP (email), Redis and the header part of HTTP/1.1 work this way.

```anatomy
ORDER tea\n | one message: everything up to the line break
ORDER cake\r\n | CRLF works too; many servers accept both
```

Simple and readable, but a message can't contain the delimiter itself, and the receiver must scan every byte looking for it.

## 2. A length prefix

Before each message, a few bytes say how long it is. The receiver reads the length, then exactly that many bytes. Binary protocols love this: gRPC puts a 5-byte header before every message, and MQTT and many databases encode lengths too.

```anatomy
00 04 | length prefix: 2 bytes, big-endian, meaning 4
50 49 4E 47 | the 4 bytes of the message: P I N G
```

**Big-endian** means the most significant byte comes first, the way we write numbers: `01 00` is 256. A message can contain any bytes, and nothing needs scanning.

## 3. A fixed size

Every message is exactly, say, 64 bytes. Rare today, but you'll meet it in old or very simple devices.

Whatever the rule, both sides must use the same one:

```flow
Your messages -[add boundaries]-> TCP byte stream -[find boundaries]-> The same messages
```

> [!note] Think of it like…
> Sentences need full stops. A stream of words without punctuation is hard to split correctly. A line break is a full stop; a length prefix is like saying "the next sentence has seven words" before you speak it.

> [!warning] When framing doesn't match
> If the two sides disagree, nothing crashes: the receiver just waits. A line-based server that never gets a line break waits forever. A length-prefixed server that receives plain `PING` reads `50 49` as the length 20,553 and waits for twenty thousand bytes that never come. When a server "ignores" you, check the framing first.

## In Zorvik

A TCP request's **Options** tab sets both sides of the framing: **Message framing** (**As they arrive**, **One message per line** or **Length prefix (big-endian)**), the **Prefix size** (1, 2 or 4 bytes) and what to **Add to each text message** (nothing, `\n` or `\r\n`). Options apply when you connect. Zorvik servers have the same settings, and their Traffic marks leftover bytes as an **unfinished message** when a connection closes mid-message.

**You'll use this when…** a device or service "doesn't answer" your hand-typed message. Nine times out of ten the message was fine and only the framing was wrong: a missing line break, or a missing length prefix.

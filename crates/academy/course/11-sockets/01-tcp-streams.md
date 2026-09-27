---
id: tcp-streams
title: "TCP: a stream of bytes"
summary: TCP gives two programs a reliable, ordered, two-way stream of bytes; everything else is up to them.
minutes: 5
lab:
  title: Knock on a raw TCP door
  goal: Open a TCP connection by hand, read the server's greeting and speak its tiny text protocol.
  minutes: 6
  servers:
    desk:
      name: Help desk
      kind: tcp
      socket:
        mode: rules
        greeting: WELCOME TO THE LAB HELP DESK. Send HELLO to get a ticket.
        lineEnding: lf
        rules:
          - { match: regex, pattern: "(?i)^hello", reply: "HELLO! Your ticket number is {{secret.ticket}}." }
          - { match: any, reply: "UNKNOWN COMMAND. Send HELLO to get a ticket." }
  steps:
    - text: |
        Click **+** at the end of the tab bar and choose **TCP connection**. Set the URL to `{{desk}}` and press **Connect**. The server speaks first: read its greeting in the log.
      hints:
        - "TCP addresses look like tcp://host:port. The Lab environment holds the help desk's address in desk."
        - "Choose TCP connection in the + menu of the tab bar, put {{desk}} in the URL bar and press Connect."
        - "URL {{desk}}, press Connect. The log shows Connected, then WELCOME TO THE LAB HELP DESK with a blue arrow."
      check:
        message: { server: desk, kind: open }
      solution:
        - call: { method: socket.connect, params: { connId: lab-tcp, request: { name: Help desk, kind: tcp, url: "{{desk}}" } } }
    - text: |
        Type `HELLO` in the message box at the bottom and press **Send**.
      hints:
        - "Once connected, the composer at the bottom sends whatever you type, byte for byte."
        - "Keep the composer on Text, type HELLO and press Send (or ⌘/Ctrl + Enter)."
        - "Type HELLO in the Message to send box and press Send. The server answers with your ticket number."
      check:
        message: { server: desk, direction: in, text: "hello*" }
      solution:
        - call: { method: socket.connect, params: { connId: lab-tcp, request: { name: Help desk, kind: tcp, url: "{{desk}}" } } }
        - call: { method: socket.send, params: { connId: lab-tcp, message: { type: text, text: HELLO } } }
        - wait: 200
    - text: What ticket number did the help desk give you? Type it here.
      hints:
        - "It's in the newest message with a blue arrow (received)."
        - "The reply reads: HELLO! Your ticket number is … The server's side shows it too: open Lab · Help desk under Servers and look at its Traffic."
        - "Copy what comes after Your ticket number is, without the full stop."
      check:
        answer: "{{secret.ticket}}"
      solution:
        - answer: "{{secret.ticket}}"
        - call: { method: socket.close, params: { connId: lab-tcp } }
quiz:
  - question: What does TCP promise the two programs it connects?
    options:
      - The bytes arrive complete and in order, or they learn the connection broke
      - Every message arrives within one millisecond
      - The data is encrypted
    answer: 0
    explain: TCP resends lost pieces and puts them back in order. Speed and encryption are other matters (TLS adds encryption on top).
  - question: Once a TCP connection is open, which side sends first?
    options:
      - Always the client
      - Whichever side the application protocol says; TCP lets both send at any time
      - Always the server
    answer: 1
    explain: The lab's help desk greets you first, while an HTTP server waits for your request. TCP itself allows either.
  - question: You send "HEL" and then "LO" over TCP. What may the other side read?
    options:
      - Always exactly two pieces, HEL and LO
      - Only LO, because TCP keeps the last message
      - HELLO in one piece, or the bytes split in any other way
    answer: 2
    explain: TCP is a stream of bytes with no message boundaries. How your sends are cut into reads is up to the network, which is why protocols need framing (next lesson).
---

In the networks unit you met TCP from a distance: the handshake, and its promise of a reliable, ordered stream of bytes. HTTP, WebSocket, MQTT, databases and email all ride on it. In this unit you drop down a level and pick up that stream yourself, with no HTTP in between.

## One connection, two sockets

A **socket** is what a program holds to use a connection: its end of the line. A TCP connection is identified by four things, the address and port on each side. The server listens on a port everyone knows; your side gets a random, temporary one from the operating system. That's why a server's traffic log says something like `#1 connected from 127.0.0.1:53712`.

```anatomy
tcp:// | a raw TCP connection (tls:// adds TLS encryption)
127.0.0.1 | the server's IP address
:9000 | the server's port: which program on that machine
```

## Who speaks first?

Once the connection is open, TCP doesn't care who talks or when: either side may send at any moment. The **application protocol**, the rules the two programs agreed on, decides. Some servers speak first with a greeting (mail servers announce themselves with `220 …`); others wait for the client (HTTP, Redis). Many protocols are plain lines of text that you can type by hand: send `PING` to a Redis server and it answers `+PONG`.

```sequence
You -> Server: connect (SYN, SYN-ACK, ACK)
Server --> You: WELCOME TO THE LAB HELP DESK…
You -> Server: HELLO
Server --> You: HELLO! Your ticket number is…
You -> Server: FIN: I'm done sending
Server --> You: FIN: me too
```

## A stream, not messages

TCP carries **bytes**, not messages. If you send `HEL` and then `LO`, the other side may read `HELLO` in one go, or `H` and `ELLO`. For TCP these are all the same stream. The next lesson shows how protocols mark where a message ends.

> [!note] Think of it like…
> A garden hose. You pour water in cup by cup, and it all comes out the other end, in order, none of it lost. But at the far end it's just a flow of water: nothing shows where one cup ended and the next began.

## Closing

A polite close is a **FIN** ("I'm done sending"). Each direction closes on its own, so one side can finish sending while it still listens. An abrupt close is a **reset** (RST), which you'll meet as the error "connection reset by peer": the other side dropped the connection without a goodbye, often because it crashed or didn't like what it got.

## In Zorvik

A **TCP connection** request takes a URL such as `tcp://127.0.0.1:9000`. **Connect** opens the connection (the handshake really happens, so a server can greet you before you send anything). The log shows what arrives (blue ↙) and what you send (green ↗), and the composer sends **Text**, **JSON** or **Binary (hex)** bytes. The **Options** tab decides how incoming bytes are split into messages and what is added to each message you send.

**You'll use this when…** you test something that isn't HTTP: a Redis or mail server, a game server, a payment terminal or a device on the factory floor. You connect, type its protocol by hand and see exactly what comes back.

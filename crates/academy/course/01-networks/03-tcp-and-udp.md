---
id: tcp-and-udp
title: TCP vs UDP
summary: TCP delivers a reliable, ordered stream over a connection; UDP sends separate messages with no promises.
minutes: 6
lab:
  title: A phone call and a postcard
  goal: Open a TCP connection and get greeted, then send a UDP datagram without any connection at all.
  minutes: 8
  servers:
    tcp:
      name: TCP Greeter
      kind: tcp
      socket:
        mode: echo
        greeting: "Hello, you are connected! Your code word is {{secret.word}}."
    udp:
      name: UDP Mailbox
      kind: udp
      socket:
        mode: rules
        rules:
          - match: contains
            pattern: ping
            reply: pong
  steps:
    - text: |
        Open a new tab with **+** (right end of the tab bar) → **TCP connection**. Type `{{tcp}}` as the address and press **Connect**.
      hints:
        - The + button at the right end of the tab bar offers every kind of request.
        - Choose "TCP connection", type `{{tcp}}` in the address field (it holds tcp://127.0.0.1 and the lab port).
        - "+ → TCP connection, address `{{tcp}}`, then Connect. The button changes to Disconnect once the connection is open."
      check:
        message: { server: tcp, kind: open }
      solution:
        - call: { method: socket.connect, params: { connId: lab-tcp, request: { name: TCP Greeter, kind: tcp, url: "{{tcp}}" } } }
    - text: The server greeted you the moment the connection opened, before you sent anything. Type the code word from its greeting.
      hints:
        - The greeting is the first message in the connection's message list.
        - Look for "Your code word is" and copy the word and number after it.
        - No connection tab because "Do it for me" connected for you? Open Lab · TCP Greeter in the Servers section. Its traffic log shows the greeting it sent.
      check:
        answer: "{{secret.word}}"
      solution:
        - answer: "{{secret.word}}"
    - text: Type `hello` in the message box at the bottom and press **Send**. This server echoes every message straight back.
      hints:
        - Stay in the TCP connection tab from the first step. The connection is still open.
        - The message box is below the list of messages. You need to be connected first.
        - Type hello, then press Send (or ⌘↩ / Ctrl+Enter). Your message and the echo both appear in the list.
      check:
        message: { server: tcp, kind: data, direction: in, text: "*hello*" }
      solution:
        - call: { method: socket.connect, params: { connId: lab-tcp, request: { name: TCP Greeter, kind: tcp, url: "{{tcp}}" } } }
        - call: { method: socket.send, params: { connId: lab-tcp, message: { type: text, text: hello } } }
    - text: |
        Now a postcard. Open **+** → **UDP socket**, type `{{udp}}` and press **Connect**. For UDP this only gets your side ready: nothing reaches the server yet. Then send `ping`.
      hints:
        - UDP has no handshake, so the server can't greet you. It only hears from you when you send something.
        - "+ → UDP socket, address `{{udp}}`, Connect, then type ping in the message box and press Send."
        - The server answers "pong". Open Lab · UDP Mailbox in the Servers section to see your datagram arrive.
      check:
        message: { server: udp, kind: data, direction: in, text: "*ping*" }
      solution:
        - call: { method: socket.connect, params: { connId: lab-udp, request: { name: UDP Mailbox, kind: udp, url: "{{udp}}" } } }
        - call: { method: socket.send, params: { connId: lab-udp, message: { type: text, text: ping } } }
quiz:
  - question: Which protocol opens a connection with a handshake before any data is sent?
    options:
      - UDP
      - TCP
      - Both
      - Neither
    answer: 1
    explain: TCP starts with the SYN, SYN-ACK, ACK handshake. UDP just sends.
  - question: Why do video calls and games often use UDP?
    options:
      - A frame that arrives late is useless, so waiting for a resend would only add lag
      - UDP is always encrypted
      - UDP packets can never get lost
      - UDP doesn't need IP addresses
    answer: 0
    explain: TCP would hold everything back until a lost packet is sent again. For live audio and video it's better to skip it and move on.
  - question: You send a UDP datagram and nothing comes back. What do you know for sure?
    options:
      - The server is down
      - The port number is wrong
      - Not much. The datagram or the answer may have been lost, or nothing listens on that port
      - The server received it but chose not to answer
    answer: 2
    explain: UDP gives no delivery reports. Silence can mean many things, so watch the server's log or use a timeout when you test.
---

IP gets a packet to the right computer, and the port gets it to the right program. But what if a packet gets lost on the way, or two arrive in the wrong order? That's the job of the **transport protocol**, and almost everything uses one of two: **TCP** or **UDP**.

## Layers

Network protocols stack on top of each other. Each layer uses the one below it and doesn't care how it works:

```layers
Application | HTTP, DNS, WebSocket, gRPC, MQTT
Transport | TCP (reliable stream) and UDP (datagrams)
Internet | IP: addresses and routing between networks
Link | Wi-Fi, Ethernet, mobile networks
```

Your HTTP request is carried by TCP, which is carried by IP, which travels over your Wi-Fi. Along the way, data is cut into **packets**: small pieces of up to about 1,500 bytes each. A big download is thousands of packets, and any of them can be delayed or lost.

## TCP: the careful one

**TCP** (Transmission Control Protocol) first opens a connection with a three-step greeting called the **handshake**:

```sequence
participants: Client, Server
Client -> Server: SYN (can we talk?)
Server --> Client: SYN-ACK (yes, can you hear me?)
Client -> Server: ACK (loud and clear)
Note over Client, Server: connection open, data can flow both ways
```

From then on, TCP numbers every byte. The receiver confirms what arrived; anything lost is sent again, and anything out of order is put back in order. The program on the other side sees one clean **stream** of bytes, exactly as it was sent. The price: the handshake costs a round trip before any data moves, and one lost packet holds up everything behind it.

Used by: HTTP/1.1 and HTTP/2, WebSocket, databases, SSH, email.

## UDP: the quick one

**UDP** (User Datagram Protocol) has no handshake and no connection. You send a **datagram** (one self-contained message) to an address and port, and that's it. It may arrive, arrive twice, arrive out of order, or not arrive at all, and nobody tells you. In return you get speed and simplicity.

Used by: DNS lookups (one small question, one small answer), voice and video calls, games, and QUIC, the protocol under HTTP/3, which builds its own reliability on top of UDP.

> [!note] Think of it like…
> TCP is a phone call: you dial, the other side picks up, you both know you're connected, and if something is garbled you say "sorry, can you repeat that?". UDP is a postcard: quick and cheap, but you never know if or when it arrives.

| | TCP | UDP |
|---|---|---|
| Connection first | Yes, the handshake | No |
| Delivery | Guaranteed, in order | Best effort |
| Data comes as | One stream of bytes | Separate datagrams |
| Typical use | Web, APIs, databases | DNS, voice, video, games |

## In Zorvik

The **+** button in the tab bar opens a **TCP connection** or a **UDP socket**. For TCP, **Connect** really opens a connection (the handshake happens), so a server can greet you before you send anything. For UDP, Connect only gets your side ready: nothing reaches the server until you press **Send**.

> [!warning] Silence is normal for UDP
> When a UDP server doesn't answer, you get silence, not an error. When you test UDP, watch the server's log so you know whether your datagram arrived.

**You'll use this when…** you debug anything below HTTP: a device that speaks its own TCP protocol, a metrics agent that sends UDP, or a DNS lookup that "just times out".

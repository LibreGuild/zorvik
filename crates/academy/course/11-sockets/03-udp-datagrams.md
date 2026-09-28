---
id: udp-datagrams
title: "UDP: fire and forget"
summary: UDP sends single, self-contained datagrams with no connection and no delivery guarantee, which makes it fast and simple.
minutes: 5
lab:
  title: Radio a weather station
  goal: Send UDP datagrams to a weather station, read its answers and notice what UDP doesn't tell you.
  minutes: 6
  servers:
    station:
      name: Weather station
      kind: udp
      socket:
        mode: rules
        rules:
          - { match: regex, pattern: "(?i)^status", reply: "STATION OK. Ask TEMP? for a reading." }
          - { match: regex, pattern: "(?i)^temp", reply: "TEMP 18.4 C, reading id {{secret.reading}}" }
  steps:
    - text: |
        Click **+** at the end of the tab bar and choose **UDP socket**. Set the URL to `{{station}}`, press **Connect**, then send `STATUS`.
      hints:
        - "UDP addresses look like udp://host:port. Connect doesn't contact the station: it only sets where your datagrams go."
        - "Choose UDP socket in the + menu of the tab bar, put {{station}} in the URL bar, press Connect, then type in the composer at the bottom."
        - "URL {{station}}, Connect, type STATUS and press Send. The station answers STATION OK."
      check:
        message: { server: station, direction: in, text: "status*" }
      solution:
        - call: { method: socket.connect, params: { connId: lab-udp, request: { name: Weather station, kind: udp, url: "{{station}}" } } }
        - call: { method: socket.send, params: { connId: lab-udp, message: { type: text, text: STATUS } } }
        - wait: 200
    - text: Now send `TEMP?` and type the **reading id** from the station's answer.
      hints:
        - "Each datagram you send gets its own answer datagram back."
        - "Send TEMP? and look at the newest received message. The station's Traffic (Lab · Weather station under Servers) shows it too."
        - "The answer reads TEMP 18.4 C, reading id … Copy the id at the end."
      check:
        answer: "{{secret.reading}}"
      solution:
        - call: { method: socket.connect, params: { connId: lab-udp, request: { name: Weather station, kind: udp, url: "{{station}}" } } }
        - call: { method: socket.send, params: { connId: lab-udp, message: { type: text, text: "TEMP?" } } }
        - answer: "{{secret.reading}}"
    - text: |
        Fire and forget: send `LOG door opened`. The station stores log lines silently, so no answer comes back. And no error either: from your side, a datagram that arrived looks exactly like one that got lost.
      hints:
        - "Send it like the others. The point is what does not happen afterwards."
        - "Only the station's own Traffic proves your datagram arrived: open Lab · Weather station under Servers."
        - "Type LOG door opened, press Send, then check the station's Traffic for your line."
      check:
        message: { server: station, direction: in, text: "log*" }
      solution:
        - call: { method: socket.connect, params: { connId: lab-udp, request: { name: Weather station, kind: udp, url: "{{station}}" } } }
        - call: { method: socket.send, params: { connId: lab-udp, message: { type: text, text: LOG door opened } } }
        - wait: 200
        - call: { method: socket.close, params: { connId: lab-udp } }
quiz:
  - question: A UDP datagram gets lost on the way. Who notices?
    options:
      - UDP resends it automatically, like TCP
      - Nobody, unless the application itself checks, for example by waiting for a reply and asking again
      - The receiving computer asks for it again
    answer: 1
    explain: UDP has no acknowledgements or resends. Applications that care add their own, like DNS clients that retry after a timeout.
  - question: Why is UDP a good fit for a live video call?
    options:
      - It guarantees that every frame arrives
      - It encrypts the video automatically
      - A frame that arrives late is useless, so skipping it beats waiting for a resend
    answer: 2
    explain: TCP would hold everything back until a lost packet is resent, and the picture would freeze. UDP lets the call move on.
  - question: How do message boundaries work with UDP?
    options:
      - Each datagram arrives whole or not at all, so one send is one message
      - Datagrams are merged into a stream like TCP
      - You need a length prefix, as with TCP
    answer: 0
    explain: A datagram keeps its edges. What UDP doesn't promise is that it arrives, arrives once, or arrives in order.
---

TCP is careful: handshakes, acknowledgements, resends, ordering. You compared it with UDP in the networks unit; now you'll use UDP by hand. First, why would anyone give up all that care? Because sometimes it's in the way. A video call would rather drop a frame than freeze while waiting for it. A DNS lookup is a single question and a single answer, so why set up a connection? For these there is **UDP**, the User Datagram Protocol.

## Datagrams

UDP sends **datagrams**: self-contained packets of bytes, each with a destination address and port. There is no connection and no handshake. Each datagram travels on its own and arrives **whole or not at all**.

What UDP does *not* promise:

- that a datagram arrives at all,
- that it arrives only once,
- that datagrams arrive in the order they were sent.

In return it's fast, has almost no overhead, and every send is exactly one message, so no framing is needed.

```sequence
You -> Station: STATUS
Station --> You: STATION OK
You -> Station: TEMP?
Note over You, Station: this one got lost, and nobody tells you
You -> Station: TEMP? (asked again after a timeout)
Station --> You: TEMP 18.4 C
You -> Station: LOG door opened
Note over Station: stored; no reply, by design
```

If a program needs reliability, it adds its own on top: wait for an answer, and ask again after a timeout. That's exactly what DNS clients do.

> [!note] Think of it like…
> Calling out orders across a busy kitchen. Each call is short and complete ("two soups!"). Most are heard, some get lost in the noise, and the only way to be sure is when the cook calls back. If it matters, you call again.

## Building on UDP

UDP is a bare minimum, so each protocol on top adds exactly what it needs, and nothing more:

| Protocol | What it adds on top of UDP |
|---|---|
| DNS | an id to match each answer to its question, and a retry after a timeout |
| Games | a number on every update, so old positions that arrive late are simply ignored |
| Video and voice calls | nothing for lost packets: a late frame is useless, so it's skipped |
| QUIC (under HTTP/3) | its own resends, ordering and encryption, without TCP's habit of holding everything up for one lost packet |

> [!warning] Keep datagrams small
> A datagram can in theory hold about 64 KB, but large ones get split on the way, and losing any piece loses the whole datagram. Real protocols usually stay below about 1,200 bytes.

## In Zorvik

A **UDP socket** request takes a URL such as `udp://127.0.0.1:9001`. **Connect** only chooses where your datagrams go: no packet is sent until you press **Send**. Each datagram you send and receive shows in the log, received ones with the sender's address. A Zorvik UDP server likewise lists each sender as a pseudo-connection in its Traffic, from the first datagram on.

**You'll use this when…** you test DNS, a game server, a VoIP system or an IoT device that speaks UDP, and when something "just doesn't answer": you'll know that with UDP, silence doesn't tell you whether your message arrived.

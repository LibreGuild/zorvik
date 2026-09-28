---
id: where-the-time-goes
title: Where the time goes
summary: A request's time splits into DNS, connect, TLS, waiting and download, and each phase points to a different fix.
minutes: 6
lab:
  title: Find the slow part
  goal: Compare a fast and a slow request in the Timing tab, then make the client give up with a timeout.
  minutes: 6
  playground: true
  steps:
    - text: Send `GET {{playground}}/delay/0` and open the response's **Timing** tab. Every phase is tiny, because the server is on your own computer and answers at once.
      hints:
        - The playground is a practice server. /delay/0 answers without waiting.
        - "Send `GET {{playground}}/delay/0`, then click Timing among the response's tabs."
      check:
        send: { url: "*/delay/0", status: 200 }
      solution:
        - send: { method: GET, url: "{{playground}}/delay/0" }
    - text: Now send `GET {{playground}}/delay/800`. The server waits 800 milliseconds before it answers.
      hints:
        - Change only the number at the end of the URL.
        - "Send `GET {{playground}}/delay/800` and look at the Timing tab again."
      check:
        send: { url: "*/delay/800", status: 200 }
      solution:
        - send: { method: GET, url: "{{playground}}/delay/800" }
    - text: Compare the two waterfalls. Which phase grew by about 800 ms? Type its name.
      hints:
        - DNS, connect and download stayed tiny. One bar is now much longer than the others.
        - The long bar is the time between sending the request and getting the first byte back.
        - It's "Waiting (TTFB)". Typing waiting or TTFB is enough.
      check:
        answer: ["*waiting*", "*ttfb*", "*first byte*"]
      solution:
        - answer: Waiting (TTFB)
    - text: |
        Make the client give up. In the request's **Settings** tab, set **Timeout** to `500`, then send `GET {{playground}}/delay/2000`.
      hints:
        - The timeout is in milliseconds. The server will take 2000, so the client stops waiting first.
        - Open the request's Settings tab (right after Auth) and type 500 in Timeout.
        - "Timeout `500`, URL `{{playground}}/delay/2000`, Send. You get a timeout error instead of a response."
      check:
        send: { url: "*/delay/*", errorKind: timeout }
      solution:
        - send: { method: GET, url: "{{playground}}/delay/2000", settings: { timeoutMs: 500 } }
quiz:
  - question: Which phase includes the time the server spends working on your request?
    options:
      - DNS lookup
      - TCP connect
      - Download
      - Waiting (TTFB)
    answer: 3
    explain: TTFB, time to first byte, runs from sending the request until the answer starts arriving, so it includes all the server's thinking.
  - question: The Timing tab shows 0 ms for DNS lookup. Why could that be?
    options:
      - The URL uses an IP address, so there was no name to look up
      - The DNS server is broken
      - The request used POST
      - The response had no body
    answer: 0
    explain: With an address like 127.0.0.1 in the URL there's nothing to resolve, so the DNS phase is empty.
  - question: An API call takes 3 seconds and almost all of it is Download. What's the likely cause?
    options:
      - The server's database is slow
      - The DNS resolver is far away
      - The response is very large, or the connection is slow
      - The TLS certificate is expired
    answer: 2
    explain: Download is the time to receive the body. A huge response (or a slow link) makes it long. Paging or asking for fewer fields helps.
---

"The API is slow" can mean five different things. Zorvik measures every request in phases, so you can see which part is slow instead of guessing.

## The phases of a request

```sequence
participants: Zorvik, DNS, Server
Zorvik -> DNS: where is api.cafe.test?
DNS --> Zorvik: 192.0.2.10
Note over Zorvik, Server: TCP connect, the handshake
Note over Zorvik, Server: TLS handshake, for https only
Zorvik -> Server: GET /menu
Note over Server: works on the answer
Server --> Zorvik: the first byte of the response
Server --> Zorvik: the rest of the body
```

| Phase | What happens | It's slow when… |
|---|---|---|
| **DNS lookup** | the name becomes an address | the resolver is slow or far away (0 when the URL has an IP address) |
| **TCP connect** | the handshake opens a connection | the server is far away: it costs about one round trip |
| **TLS handshake** | client and server agree on encryption (https only) | far away (one or two more round trips), or the server is busy |
| **Waiting (TTFB)** | the server works, until the first byte comes back | the server is slow: database queries, calls to other APIs, cold starts |
| **Download** | the rest of the body arrives | the body is big, or the connection is slow |

**TTFB** is short for **time to first byte**. It includes one network round trip plus all the thinking the server does. When an API is slow, it's usually here.

> [!note] Think of it like…
> Ordering a pizza by phone. Looking up the number (DNS), dialing until someone picks up (connect), agreeing on the order (TLS), waiting while it's baked (TTFB), and the delivery ride (download). If you waited an hour, it matters a lot whether the oven or the scooter was slow.

## Reading the waterfall

The response's **Timing** tab draws the phases as bars, one starting where the last one ended, like a waterfall, with the total at the bottom. Zorvik uses a fresh connection for each request, so DNS, connect and TLS are always measured.

- A long **DNS lookup**? Look at the resolver (the DNS unit).
- A long **TCP connect** or **TLS handshake**? Distance, or a struggling server: ping it (the networks unit).
- A long **Waiting (TTFB)**? The server's own work: its logs, its database, the APIs it calls.
- A long **Download**? The response is big: page it, compress it or ask for fewer fields.

> [!warning] Timeouts
> A client doesn't wait forever. When no answer comes in time, it gives up with a **timeout** error. Every request in Zorvik can have its own limit in its **Settings** tab (**Timeout**, in milliseconds).

The practice **playground** has `/delay/<ms>`, which waits that many milliseconds before it answers: a server that's slow on demand, perfect for testing timeouts and loading screens.

**You'll use this when…** someone says "it's slow". You'll answer with a number and a phase, like "the server takes 800 ms before the first byte", instead of a feeling.

---
id: latency-and-ping
title: Latency and ping
summary: Latency is how long one trip takes; ping measures it, over ICMP or by timing a TCP connection.
minutes: 5
lab:
  title: Time the trip
  goal: Measure round trips to a lab server with a TCP ping, then compare with a classic ICMP ping.
  minutes: 5
  servers:
    target:
      name: Ping Target
      kind: tcp
  steps:
    - text: |
        Open **Tools → Ping**. Enter the host `127.0.0.1`, set **Using** to **TCP**, **TCP port** to `{{target_port}}`, and press **Start**.
      hints:
        - A TCP ping times how long it takes to open a connection to one port. The lab server listens on `{{target_port}}`.
        - In Tools → Ping, type 127.0.0.1 as the host, click TCP next to Using, and put the lab port in the TCP port field.
        - "Host `127.0.0.1` · Using: TCP · TCP port `{{target_port}}` · Start. Each reply shows the time in milliseconds."
      check:
        all:
          - call: { method: tools.ping, ok: true, result: { mode: tcp, port: "{{target_port}}" } }
          - message: { server: target, kind: open }
      solution:
        - call: { method: tools.ping, params: { runId: lab-ping, host: "{{target_host}}", mode: tcp, count: 4, intervalMs: 300 } }
    - text: |
        Open **Lab · Ping Target** in the **Servers** section: every TCP ping shows up there as a connection that opened and closed. Now ping `127.0.0.1` again, this time **Using: ICMP**. If your computer doesn't allow ICMP, Zorvik tells you why, and that counts too.
      hints:
        - ICMP ping doesn't use a port. The operating system itself answers it.
        - Go back to the Ping tab, click ICMP next to Using, and press Start.
        - "Host `127.0.0.1` · Using: ICMP · Start. Compare the Avg time with the TCP ping: both are well under a millisecond on your own computer."
      check:
        any:
          - call: { method: tools.ping, params: { mode: icmp } }
          - call: { method: tools.ping, result: { mode: icmp } }
      solution:
        - call: { method: tools.ping, params: { runId: lab-ping, host: 127.0.0.1, mode: icmp, count: 4, intervalMs: 300 } }
quiz:
  - question: What does latency measure?
    options:
      - How much data fits through per second
      - How many routers are on the way
      - How long a message takes to get there (and back)
      - How big one packet can be
    answer: 2
    explain: Latency is time, usually measured as a round trip in milliseconds. "How much per second" is bandwidth.
  - question: A server doesn't answer ICMP ping. What can you conclude?
    options:
      - It's definitely down
      - Not much yet. Many servers block ICMP, so try a TCP ping to the port you need
      - Its disk is full
      - Its name can't be resolved
    answer: 1
    explain: Firewalls often drop ICMP. A TCP ping to the service's port tells you whether the program you need is reachable.
  - question: An app makes 10 calls one after another to a server with an 80 ms round trip. At least how long does it spend waiting on the network?
    options:
      - 0.8 seconds (10 × 80 ms)
      - 80 ms
      - 8 seconds
      - 8 ms
    answer: 0
    explain: Each call pays at least one round trip, so ten in a row pay ten. Fewer calls, or calls in parallel, cut that time.
---

Networks have two kinds of speed. **Bandwidth** is how much data fits through per second, like the number of lanes on a highway. **Latency** is how long one trip takes, like the drive time. A big download mostly cares about bandwidth. An API call that sends a few hundred bytes mostly cares about latency.

## Round trips

The number you'll see most is the **round-trip time** (RTT): the time for a message to reach the other side and for the answer to come back. It's measured in milliseconds (ms, thousandths of a second).

```sequence
participants: You, Server
You -> Server: are you there?
Note over Server: answers right away
Server --> You: yes!
Note over You: round trip took 24 ms
```

Where does the time go?

- **Distance.** Signals in fiber cover about 200 km per millisecond. New York to London and back is about 11,000 km, so at least 55 ms before anything else happens.
- **Hops.** Packets pass through many routers on the way; each one adds a little.
- **Queues.** A busy router or a crowded Wi-Fi network makes packets wait in line.
- **The last meters.** Wi-Fi adds more delay than a cable, especially with many devices around.

Some typical round trips:

| Between | Round trip |
|---|---|
| Two programs on your computer | well under 1 ms |
| Your laptop and your home router | 1–5 ms |
| A data center in your region | 5–30 ms |
| Across an ocean | 70–200 ms |

## Ping

**Ping** is the classic way to measure round trips. It sends a tiny **ICMP echo request** (ICMP is a helper protocol of IP, used for network housekeeping), and the other computer's operating system sends it straight back. Ping reports each round trip, plus:

- **Loss**: how many replies never came back.
- **Jitter**: how much the round trip changes from one reply to the next. Voice and video calls suffer when jitter is high.

Many servers and firewalls block ICMP, so a failed ping doesn't prove a server is down. A **TCP ping** measures something else: how long it takes to open a TCP connection to one port, the handshake from the last lesson. When it works, you know the computer is reachable *and* a program is listening on that port.

> [!note] Think of it like…
> Shouting "hello!" across a canyon and timing the echo. An ICMP ping shouts at the mountain (the computer). A TCP ping knocks on one particular door and times how long it takes someone to open it.

## In Zorvik

**Tools → Ping** has a **Using** switch: **Auto** (ICMP, falling back to TCP when ICMP isn't allowed), **ICMP** or **TCP**, plus a **TCP port**, a **Count** and how often to send (**Every**). It shows each reply, a small chart, and the totals: Sent, Received, Loss, Min, Avg, Max and Jitter.

> [!tip] Latency adds up
> A screen that makes 20 API calls one after another over a 100 ms link waits at least 2 seconds, however fast the server is. That's why good apps make fewer calls, or make them in parallel.

**You'll use this when…** someone says "the API is slow". Ping it first: if the round trip alone is 200 ms, the fix isn't in the server's code.

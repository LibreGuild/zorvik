---
id: ttl-and-caching
title: TTL and caching
summary: Every answer says how long it may be remembered, and that's why DNS changes take a while to reach everyone.
minutes: 5
lab:
  title: How long is an answer good for?
  goal: Read the TTL of two records and work out how long old answers could stick around.
  minutes: 5
  servers:
    dns:
      name: Cafe DNS
      kind: dns
      dns:
        records:
          - { name: menu.cafe.test, type: A, value: 192.0.2.30, ttl: 1800 }
          - { name: status.cafe.test, type: A, value: 192.0.2.31, ttl: 30 }
  steps:
    - text: |
        In **Tools → DNS lookup**, look up `menu.cafe.test` (type **A**) on the lab's DNS server `{{dns_host}}`. How long may this answer be cached? Type the TTL, in seconds or as Zorvik shows it.
      hints:
        - The TTL column is between Type and Data. Hover it to see the value in seconds.
        - "Type A · name `menu.cafe.test` · resolver Custom… → `{{dns_host}}` · Look up."
        - Zorvik shows 30m, which is 1800 seconds. Either answer is fine.
      check:
        all:
          - message: { server: dns, kind: dns, summary: "A menu.cafe.test → *" }
          - answer: ["1800", "1800s", "1800 s", "1800 sec", "1800 seconds", "30m", "30 m", "30 min", "30 minutes"]
      solution:
        - call: { method: dns.query, params: { request: { name: DNS lookup, kind: dns, method: A, url: menu.cafe.test, dns: { server: "{{dns_host}}" } } } }
        - answer: "1800"
    - text: A resolver cached this answer **10 minutes ago**. For how many more **minutes** will it keep handing out its cached copy? Type the number.
      hints:
        - The TTL is 30 minutes, and the countdown started when the resolver cached the answer.
        - 30 minutes of TTL minus the 10 that have already passed.
        - 30 − 10 = 20. Type 20.
      check:
        answer: ["20", "20m", "20 m", "20 min", "20 minutes"]
      solution:
        - answer: "20"
    - text: |
        Now look up `status.cafe.test`. If the cafe moved its status page to a new address right now, what's the longest time, in **seconds**, that anyone could still be sent to the old address?
      hints:
        - Old copies live at most as long as the TTL.
        - "Look up `status.cafe.test` (type A) on `{{dns_host}}` and read its TTL."
        - The TTL is 30 seconds, so the answer is 30.
      check:
        all:
          - message: { server: dns, kind: dns, summary: "A status.cafe.test → *" }
          - answer: ["30", "30s", "30 s", "30 sec", "30 seconds"]
      solution:
        - call: { method: dns.query, params: { request: { name: DNS lookup, kind: dns, method: A, url: status.cafe.test, dns: { server: "{{dns_host}}" } } } }
        - answer: "30"
quiz:
  - question: What does a record's TTL say?
    options:
      - How many seconds a cache may keep the answer
      - How many times the record may be looked up
      - How many servers hold a copy
      - How long the domain is registered for
    answer: 0
    explain: TTL is "time to live", in seconds. When it runs out, the next lookup goes back to the authoritative server.
  - question: You changed your API's A record an hour ago, but some users still reach the old server. Why?
    options:
      - The change failed and must be made again
      - DNS changes need the server to restart
      - Resolvers cached the old address and keep it until its TTL runs out
      - The users typed the wrong URL
    answer: 2
    explain: Nothing "spreads" across the internet. Old copies simply expire, and with a long TTL that takes a while.
  - question: Next week you'll move your API to a new server. What's the smart first step?
    options:
      - Raise the TTL to a week
      - Lower the record's TTL now, so the switch reaches everyone quickly later
      - Delete the record and create it again
      - Nothing, DNS changes are instant
    answer: 1
    explain: Lower the TTL days before the move and let the old, long TTL run out. Then everyone picks up the new address within minutes of the change.
---

If every lookup walked from the root servers to the authoritative server, the internet would crawl. Instead, answers are **cached** (kept for reuse) at almost every step: in the resolver, in your operating system, in your browser and even inside apps.

## TTL: time to live

Every record carries a **TTL** (time to live): how many seconds a cache may keep it. The owner of the domain chooses it.

```sequence
participants: App, Resolver, Authoritative
App -> Resolver: api.cafe.test?
Resolver -> Authoritative: api.cafe.test?
Authoritative --> Resolver: 192.0.2.30, TTL 3600
Resolver --> App: 192.0.2.30, TTL 3600
Note over Resolver: keeps it for an hour
App -> Resolver: api.cafe.test? (10 minutes later)
Resolver --> App: 192.0.2.30, TTL 3000, from its cache
```

A cache counts the TTL down. When you get an answer from a cache, the TTL you see is the time *left*, not the original. When it reaches zero, the copy is thrown away and the next lookup asks the authoritative server again.

> [!note] Think of it like…
> A friend's phone number on a sticky note, with "valid until Friday" written on it. Until Friday you trust the note. If your friend changes number on Wednesday, you'll keep calling the old one until Friday.

## Why "DNS changes take a while"

Say you move your API to a new server and change its A record. Resolvers that cached the old address keep handing it out until their copy expires. With a TTL of 86,400 seconds (a day), some users reach the old server for up to a day. That's what people mean by "waiting for DNS to propagate": nothing is spreading, old copies are running out.

Picking a TTL is a trade-off:

| TTL | Good for | The cost |
|---|---|---|
| 60–300 seconds | things that may move soon, failover | more lookups, a little more waiting |
| 3,600 seconds (1 hour) | most records | changes take up to an hour |
| 86,400 seconds (1 day) | records that almost never change | changes take up to a day |

> [!tip] The moving-day trick
> A day or two before a move, lower the TTL to 60 or 300 seconds. Wait for the old, long TTL to run out, then change the address: everyone gets the new one within minutes. Raise the TTL again afterwards.

Resolvers also remember that a name *doesn't* exist for a while. This is called **negative caching**. If you look up a new name before you create it, you may keep getting NXDOMAIN for a few minutes after it exists.

## In Zorvik

Every DNS answer has a **TTL** column, shown in a short form like `30m`; hover it to see the exact number of seconds. When you ask the authoritative server directly, like the lab's DNS server, you always see the full TTL. A public resolver may show a smaller number that counts down between lookups.

**You'll use this when…** you change where a domain points and some users still reach the old server, or a teammate asks "why does it work on my laptop but not on yours?"

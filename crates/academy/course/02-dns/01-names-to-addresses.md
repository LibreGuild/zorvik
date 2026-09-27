---
id: names-to-addresses
title: Names become addresses
summary: DNS turns a name into an address by asking a chain of servers, and remembers the answer.
minutes: 6
lab:
  title: Look it up
  goal: Ask the lab's DNS server for a name's address, then see what happens with a name that doesn't exist.
  minutes: 6
  servers:
    dns:
      name: Shop DNS
      kind: dns
      dns:
        records:
          - { name: shop.lab.test, type: A, value: 192.0.2.44, ttl: 300 }
          - { name: api.shop.lab.test, type: A, value: 192.0.2.45, ttl: 300 }
  steps:
    - text: |
        Open **Tools → DNS lookup**. Keep the record type **A** and type the name `shop.lab.test`. Under the resolver buttons, choose **Custom…** and enter `{{dns_host}}`, the lab's DNS server. Press **Look up**.
      hints:
        - Lab names like shop.lab.test only exist on the lab's DNS server, so you must ask that server directly.
        - In DNS lookup, click "Custom…" in the row of resolver buttons (System, Cloudflare, …) and type the lab server's address into the field that appears.
        - "Type A · name `shop.lab.test` · resolver Custom… → `{{dns_host}}` · Look up."
      check:
        message: { server: dns, kind: dns, summary: "A shop.lab.test → 192.0.2.44*" }
      solution:
        - call: { method: dns.query, params: { request: { name: DNS lookup, kind: dns, method: A, url: shop.lab.test, dns: { server: "{{dns_host}}" } } } }
    - text: Which IPv4 address does `shop.lab.test` point to? Type it.
      hints:
        - Look at the Answer section of the result, in the Data column.
        - It's four numbers separated by dots, starting with 192.
        - The traffic log of Lab · Shop DNS (in the Servers section) shows every answer the server gave, too.
      check:
        answer: "192.0.2.44"
      solution:
        - answer: "192.0.2.44"
    - text: Now look up a name that doesn't exist on the lab server, such as `shopp.lab.test` (spot the typo), on the same resolver. Read what the server answers.
      hints:
        - Any name the lab server has no record for will do.
        - Change only the name and press Look up again. The resolver stays the same.
        - "Name `shopp.lab.test`, type A, resolver `{{dns_host}}`, Look up. The answer is NXDOMAIN: no such name."
      check:
        message: { server: dns, kind: dns, summary: "* → NXDOMAIN*" }
      solution:
        - call: { method: dns.query, params: { request: { name: DNS lookup, kind: dns, method: A, url: shopp.lab.test, dns: { server: "{{dns_host}}" } } } }
quiz:
  - question: What does a DNS resolver do for you?
    options:
      - It encrypts your traffic
      - It finds the address for a name, asking other DNS servers when it has to
      - It hosts websites
      - It gives your computer its IP address
    answer: 1
    explain: A resolver does the legwork of a lookup and caches the answers. Handing out IP addresses to devices is a different job (DHCP).
  - question: Which server has the final word on the records of shop.example?
    options:
      - A root server
      - The server for the .example top-level domain
      - The authoritative server of shop.example
      - Your own computer
    answer: 2
    explain: The root and TLD servers only point the way. The authoritative server holds the domain's actual records.
  - question: A lookup answers NXDOMAIN. What's the most likely cause?
    options:
      - The name doesn't exist, often because of a typo
      - The server is overloaded
      - The port is closed
      - The certificate has expired
    answer: 0
    explain: NXDOMAIN means "non-existent domain". Check the spelling, and that you're asking a resolver that knows the name.
---

Computers connect using IP addresses, but people remember names. **DNS** (the Domain Name System) is the internet's address book: you give it a name like `api.shop.example`, and it gives you back an address like `93.184.216.34`. It happens before almost every request you send.

## Reading a name

Names are read from right to left, from the most general part to the most specific:

```anatomy
api.shop.example. | the full name (the final dot stands for the root and is usually left out)
example | the top-level domain (TLD), like com, org or uk
shop.example | the domain someone registered
api | a subdomain, chosen by the domain's owner
```

## The resolution walk

No single server knows every name on the internet. Instead, each level knows who is responsible for the next one down:

- The **root servers** know which servers run each TLD (`com`, `org`, `example`, …).
- The **TLD servers** know which servers are in charge of each domain registered under them.
- The **authoritative server** of a domain holds its actual records. It has the final word.

Your computer doesn't do this walk itself. It asks a **resolver**: a DNS server that does the legwork, usually run by your internet provider, your company or a public service. The resolver walks the chain for you:

```sequence
participants: You, Resolver, Root, TLD, Authoritative
You -> Resolver: where is api.shop.example?
Resolver -> Root: api.shop.example?
Root --> Resolver: ask the servers for .example
Resolver -> TLD: api.shop.example?
TLD --> Resolver: ask the servers for shop.example
Resolver -> Authoritative: api.shop.example?
Authoritative --> Resolver: 93.184.216.34
Resolver --> You: 93.184.216.34
Note over Resolver: remembers it for next time
```

Four round trips sound slow, but the resolver **caches** (remembers) what it learns. Most lookups are answered from its memory in a millisecond or two. How long it may remember is the topic of the TTL lesson.

> [!note] Think of it like…
> Asking a well-connected librarian for a book. They don't know where every book is, but they know which building holds which subject, which floor has which shelf, and they remember the books people asked for earlier today.

## Answers you'll see

- **NOERROR** with records: found it.
- **NXDOMAIN** ("non-existent domain"): that name doesn't exist. Often a typo.
- **SERVFAIL**: the resolver tried, but something failed along the way.
- A **timeout**: no DNS server answered at all.

> [!tip] Names ending in .test
> `.test` is reserved for testing and never exists on the real internet, so it's safe for labs. Names like `shop.lab.test` only exist on the lab's own DNS server.

## In Zorvik

**Tools → DNS lookup** asks any DNS server a question. Pick the record type, type the name, and choose who to ask: **System** is the resolver your computer normally uses, and **Custom…** lets you type any server, like `127.0.0.1:5353`. To keep a lookup in your collection, use **+ → DNS query** in the tab bar instead: it works the same way, with the server in its **Resolver** tab.

**You'll use this when…** a request fails with "could not resolve host". Before blaming the server, check that the name turns into an address at all, and into the address you expect.

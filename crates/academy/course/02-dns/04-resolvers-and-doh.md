---
id: resolvers-and-doh
title: Resolvers, public DNS and DNS over HTTPS
summary: Which resolver you ask changes what you see, and DoT and DoH keep your lookups private.
minutes: 6
lab:
  title: Two resolvers, two answers
  goal: Ask an office resolver and a public one the same question, and watch the office resolver forward what it doesn't know.
  minutes: 6
  servers:
    public:
      name: Public DNS
      kind: dns
      dns:
        records:
          - { name: cafe.test, type: A, value: 192.0.2.10, ttl: 300 }
    office:
      name: Office DNS
      kind: dns
      dns:
        records:
          - { name: intranet.cafe.test, type: A, value: 10.20.30.40, ttl: 300 }
        upstream: "{{public_host}}"
  steps:
    - text: |
        In **Tools → DNS lookup**, ask the **Public DNS** at `{{public_host}}` for `intranet.cafe.test` (type **A**).
      hints:
        - Two lab DNS servers are running, Public DNS and Office DNS, and they know different names.
        - Use Custom… as the resolver, with the Public DNS address from this step.
        - "Type A · name `intranet.cafe.test` · resolver Custom… → `{{public_host}}` · Look up. The public resolver has never heard of it: NXDOMAIN."
      check:
        message: { server: public, kind: dns, summary: "A intranet.cafe.test → NXDOMAIN*" }
      solution:
        - call: { method: dns.query, params: { request: { name: DNS lookup, kind: dns, method: A, url: intranet.cafe.test, dns: { server: "{{public_host}}" } } } }
    - text: Ask the **Office DNS** at `{{office_host}}` the very same question. Type the address it answers with.
      hints:
        - The Office DNS plays your company's resolver.
        - Only the resolver changes; keep the name and the type.
        - "Resolver Custom… → `{{office_host}}`, then Look up. Copy the address from the Data column."
      check:
        all:
          - message: { server: office, kind: dns, summary: "A intranet.cafe.test → 10.20.30.40*" }
          - answer: "10.20.30.40"
      solution:
        - call: { method: dns.query, params: { request: { name: DNS lookup, kind: dns, method: A, url: intranet.cafe.test, dns: { server: "{{office_host}}" } } } }
        - answer: "10.20.30.40"
    - text: |
        Now ask the **Office DNS** for `cafe.test`. It has no record for that name, so it **forwards** the question to the public resolver. Open **Lab · Office DNS** in the **Servers** section to see it say so in its traffic log.
      hints:
        - Keep the Office DNS as the resolver and change only the name.
        - "Type A · name `cafe.test` · resolver `{{office_host}}` · Look up. You still get 192.0.2.10, fetched from the Public DNS."
        - In the Office DNS traffic log, the line reads "A cafe.test → forwarded to" the public server's address.
      check:
        message: { server: office, kind: dns, summary: 're:(?i)^[A-Z]+ cafe\.test → forwarded' }
      solution:
        - call: { method: dns.query, params: { request: { name: DNS lookup, kind: dns, method: A, url: cafe.test, dns: { server: "{{office_host}}" } } } }
quiz:
  - question: At the office, intranet.cafe.test works. At home it answers NXDOMAIN. What's the most likely reason?
    options:
      - The server is switched off at night
      - Your home Wi-Fi blocks port 443
      - The record's TTL ran out
      - The office resolver knows private names that public resolvers don't
    answer: 3
    explain: This is split-horizon DNS. Private names only exist on the company's resolvers, which you reach at the office or over the VPN.
  - question: What does DNS over HTTPS (DoH) change?
    options:
      - It encrypts lookups, so others on the network can't read or change them
      - It makes every lookup skip the cache
      - It lets you register new domains
      - It replaces IP addresses with names
    answer: 0
    explain: Classic DNS travels as readable text. DoH (and DoT) wrap the same questions and answers in an encrypted connection.
  - question: Which servers does Zorvik's "System" resolver preset ask?
    options:
      - Cloudflare's 1.1.1.1
      - The authoritative server of each domain, directly
      - The resolvers in your computer's network settings, VPN and company DNS included
      - Google's 8.8.8.8
    answer: 2
    explain: System uses the same resolvers as the rest of your computer, so it sees what your other apps see.
---

The same name can get different answers depending on who you ask. Knowing which resolver answers your lookups explains a lot of "but it works for me" mysteries.

## Who answers your lookups?

Your computer has a tiny **stub resolver** built in. It doesn't walk the DNS tree itself; it passes every question to the **recursive resolvers** listed in your network settings, which do the walk (or answer from their cache).

```flow
Your app -> Stub resolver (your OS) -> Recursive resolver -> Root, TLD and authoritative servers
```

Which recursive resolvers? It depends where you are:

- **At home**, usually your router, which passes questions on to your internet provider.
- **At the office or on a VPN**, the company's resolvers. They often know private names, like `jira.corp.internal`, that exist nowhere else.
- **Public resolvers** that anyone can use: Cloudflare (`1.1.1.1`), Google (`8.8.8.8`) and Quad9 (`9.9.9.9`, which also blocks known malicious domains).

## Why answers differ

- **Split-horizon DNS**: the company resolver answers `intranet.cafe.test` with a private address, while public resolvers say NXDOMAIN.
- **Caches**: one resolver may still hold an old answer (remember the TTL lesson).
- **Location**: big sites give each resolver the address of a data center near it.
- **Filtering**: some resolvers block ads, malware or whole categories of sites on purpose.

A resolver also doesn't have to know everything itself. It can **forward** the questions it can't answer to another resolver. Office resolvers often answer internal names themselves and forward the rest to a public resolver.

> [!note] Think of it like…
> Asking for directions to "the cafe". A colleague in your building points you to the one in the lobby; a stranger on the street sends you to the one downtown. Both are right, from where they stand.

## Keeping lookups private

Classic DNS travels as plain, unencrypted messages over UDP port 53. Anyone on the way, like the Wi-Fi at a coffee shop, can see which names you look up, and could even change the answers. Two newer ways encrypt it:

| | How it travels | Port |
|---|---|---|
| Classic DNS | plain UDP (or TCP for big answers) | 53 |
| **DoT**, DNS over TLS | inside an encrypted TLS connection | 853 |
| **DoH**, DNS over HTTPS | as HTTPS requests to a URL like `https://cloudflare-dns.com/dns-query` | 443 |

DoH looks just like any other web traffic, which also makes it hard to block. Many browsers use it by default.

> [!warning] Private names need the private resolver
> If a request to an internal host fails with "could not resolve", check that you're on the VPN, and that you're asking the company's resolver rather than a public one.

## In Zorvik

The resolver buttons in **Tools → DNS lookup** (and in a DNS query's **Resolver** tab) are exactly these: **System** (your computer's resolvers, VPN included), **Cloudflare**, **Google**, **Quad9**, **Cloudflare DoH** and **Google DoH**. **Custom…** takes any server: an address like `10.0.0.2`, `tls://` for DoT, or an `https://` URL for DoH.

**You'll use this when…** a service works from your laptop but not from a server (or the other way round), when you debug hosts that only exist on the VPN, or when you want to check what the public internet sees for your domain.

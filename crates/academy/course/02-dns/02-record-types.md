---
id: dns-record-types
title: "Record types: A, AAAA, CNAME, MX, TXT"
summary: One name can hold several kinds of records, and each type answers a different question.
minutes: 6
lab:
  title: Read the contact card
  goal: Ask one domain for its mail server, follow an alias, and find a verification code in a TXT record.
  minutes: 7
  servers:
    dns:
      name: Cafe DNS
      kind: dns
      dns:
        records:
          - { name: cafe.test, type: A, value: 192.0.2.10, ttl: 300 }
          - { name: cafe.test, type: AAAA, value: "2001:db8::10", ttl: 300 }
          - { name: www.cafe.test, type: CNAME, value: cafe.test., ttl: 300 }
          - { name: cafe.test, type: MX, value: 10 mail.cafe.test., ttl: 300 }
          - { name: mail.cafe.test, type: A, value: 192.0.2.25, ttl: 300 }
          - { name: cafe.test, type: TXT, value: "v=spf1 mx -all", ttl: 300 }
          - { name: cafe.test, type: TXT, value: "cafe-verification={{secret.word}}", ttl: 300 }
  steps:
    - text: |
        In **Tools → DNS lookup**, ask the lab's DNS server (**Custom…** → `{{dns_host}}`) for the **MX** record of `cafe.test`: which server receives the cafe's email?
      hints:
        - The record type sits left of the name. Click it and pick MX.
        - The resolver is the same kind as in the last lesson, Custom… with the lab server's address.
        - "Type MX · name `cafe.test` · resolver Custom… → `{{dns_host}}` · Look up. The answer is 10 mail.cafe.test."
      check:
        message: { server: dns, kind: dns, summary: "MX cafe.test → *mail.cafe.test*" }
      solution:
        - call: { method: dns.query, params: { request: { name: DNS lookup, kind: dns, method: MX, url: cafe.test, dns: { server: "{{dns_host}}" } } } }
    - text: Now look up `www.cafe.test` with type **A**. Notice that the answer starts with a **CNAME** record, then gives the address it points to.
      hints:
        - An alias (CNAME) points to another name, and that name has the address.
        - Switch the type back to A and change the name to www.cafe.test.
        - "Type A · name `www.cafe.test` · same resolver · Look up. You get two records: CNAME cafe.test, then A 192.0.2.10."
      check:
        message: { server: dns, kind: dns, summary: "A www.cafe.test → CNAME*" }
      solution:
        - call: { method: dns.query, params: { request: { name: DNS lookup, kind: dns, method: A, url: www.cafe.test, dns: { server: "{{dns_host}}" } } } }
    - text: The cafe proved it owns its domain with a **TXT** record. Look up the TXT records of `cafe.test` and type the verification code (the part after `cafe-verification=`).
      hints:
        - Pick TXT as the type and cafe.test as the name.
        - There are two TXT records. One is for email (v=spf1…); the other holds the code.
        - Copy the word and number after "cafe-verification=".
      check:
        all:
          - message: { server: dns, kind: dns, summary: "TXT cafe.test → *" }
          - answer: "{{secret.word}}"
      solution:
        - call: { method: dns.query, params: { request: { name: DNS lookup, kind: dns, method: TXT, url: cafe.test, dns: { server: "{{dns_host}}" } } } }
        - answer: "{{secret.word}}"
quiz:
  - question: Which record type holds an IPv6 address?
    options:
      - A
      - CNAME
      - MX
      - AAAA
    answer: 3
    explain: A is for IPv4 addresses, AAAA ("quad A") for IPv6.
  - question: A service asks you to "add this verification code to your domain". Which record type is that usually?
    options:
      - TXT
      - MX
      - A
      - PTR
    answer: 0
    explain: TXT records hold free text, which is why they're used for ownership checks and email security settings.
  - question: You look up the MX record of api.example.com and get NOERROR with no records. What does that mean?
    options:
      - The name doesn't exist
      - The DNS server is broken
      - The name exists, but it has no MX records
      - The answer came from a cache
    answer: 2
    explain: NOERROR means the name exists. An empty answer just means it has no records of the type you asked for. A missing name would be NXDOMAIN.
---

A DNS name is like a contact card: it can hold several entries, each for a different purpose. Every entry is a **record**, with a **type**, a **value**, and a **TTL** (how many seconds it may be remembered). This is how records look in a *zone file*, the classic way to write them down:

```text
cafe.test.       300  IN  A      192.0.2.10
cafe.test.       300  IN  AAAA   2001:db8::10
www.cafe.test.   300  IN  CNAME  cafe.test.
cafe.test.       300  IN  MX     10 mail.cafe.test.
cafe.test.       300  IN  TXT    "v=spf1 mx -all"
```

Taking one line apart:

```anatomy
www.cafe.test. | name: what the record is about
300 | TTL: how many seconds it may be cached
IN | class: "Internet", practically always
CNAME | type: what kind of record this is
cafe.test. | value: the data, in the format of the type
```

## The five you'll meet every week

- **A**: an IPv4 address. "`cafe.test` is at `192.0.2.10`." A name can have several A records; clients pick one, which spreads the load across servers.
- **AAAA** ("quad A"): the same for an IPv6 address.
- **CNAME** (canonical name): an alias. "`www.cafe.test` is really `cafe.test`, look that up instead." The resolver follows the alias and returns both the CNAME and the final address. A name with a CNAME can't have any other records.
- **MX** (mail exchanger): which server receives email for the domain, with a priority number. Lower numbers are tried first.
- **TXT**: free text. Used to prove you own a domain (the verification codes that Google, Microsoft and many others ask for), and for email security settings such as SPF, DKIM and DMARC.

Following a CNAME looks like this:

```flow
www.cafe.test -[CNAME]-> cafe.test -[A]-> 192.0.2.10
```

A few other types exist too: **NS** (the name servers in charge of a domain), **SRV** (a service's host and port), **PTR** (from an IP address back to a name) and **CAA** (which certificate authorities may issue certificates for the domain).

> [!note] Think of it like…
> A contact card. A and AAAA are the home address, MX says "send letters to the office instead", CNAME says "see my other card", and TXT is the notes field where anything goes.

> [!warning] Ask for the right type
> Asking for a type the name doesn't have isn't an error: the answer is simply empty (NOERROR, no records). When a lookup comes back empty, check the type before anything else.

## In Zorvik

In **Tools → DNS lookup**, the record type sits left of the name; hover it for a short hint on each type. The answer lists every record with its **Name**, **Type**, **TTL** and **Data**. When a CNAME is involved, you see the whole chain, alias first.

**You'll use this when…** you point a custom domain at an API, move email to a new provider, or a service asks you to "add this TXT record to verify your domain".

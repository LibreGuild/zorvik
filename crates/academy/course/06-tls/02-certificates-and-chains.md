---
id: certificates-and-chains
title: Certificates and chains
summary: A certificate ties a server's name to its key, and a chain of signatures leads from it to an authority your computer already trusts.
minutes: 6
lab:
  title: Read a certificate
  goal: Inspect the practice HTTPS server with the TLS inspector and read its certificate and chain.
  minutes: 6
  playground: { tls: true }
  steps:
    - text: |
        Open **Tools → TLS inspector** and inspect the practice HTTPS server at `{{playgroundTls}}`.
      hints:
        - The inspector's **Server** field takes a host, a `host:port` or a whole `https://` address.
        - In the sidebar open **Tools**, then **TLS inspector**. Paste the address shown here into **Server**.
        - "Server = {{playgroundTls}}, then press Inspect. It's fine that it says Not trusted: that's the next lesson."
      check:
        call: { method: tools.tlsInspect, ok: true, result: { chain: [{ commonName: localhost }] } }
      solution:
        - call: { method: tools.tlsInspect, params: { host: "{{playgroundTls}}", sni: null, runId: lab-tls-inspect } }
    - text: Look at the **Server certificate** card. Which authority issued it? Type the issuer's name.
      hints:
        - The **Issuer** row names who signed this certificate. `CN=` means "common name".
        - Type the name after `CN=`, without `CN=`.
      check:
        answer: ["Zorvik Test CA", "CN=Zorvik Test CA"]
      solution:
        - answer: Zorvik Test CA
    - text: How many certificates did the server send? (The title of the chain section tells you.)
      hints:
        - Look for the heading **Certificate chain · … sent by the server**.
        - Count the certificate cards under it.
      check:
        answer: ["1", "one"]
      solution:
        - answer: "1"
quiz:
  - question: What does a certificate authority (CA) do?
    options:
      - Encrypts your traffic for you
      - Signs certificates after checking that the requester controls the name
      - Hosts websites
    answer: 1
    explain: A CA vouches for the link between a name and a key by signing the certificate. Your system trusts a list of CAs.
  - question: A server sends its own certificate but forgets the intermediate. What can happen?
    options:
      - Some clients can't build a chain to a trusted root and reject the connection
      - Nothing, intermediates are optional decoration
      - The connection works but is not encrypted
    answer: 0
    explain: Clients need every link up to a root they trust. Browsers sometimes fill the gap on their own; API clients usually don't.
  - question: Why don't servers send the root certificate?
    options:
      - Roots are secret, so only the CA may hold a copy
      - Roots are too big to fit in a TLS handshake
      - A root only counts if it's already in your trust store
    answer: 2
    explain: Trust comes from your own trust store. A root arriving over the connection could be from anyone.
---

In the last lesson your client checked the server's **certificate** during the handshake. Time to open one up.

## What's in a certificate

A certificate is a small signed document. The important parts:

```anatomy
Subject: CN=shop.example | who the certificate is for
Names: shop.example, www.shop.example | every name it is valid for (Subject Alternative Names)
Public key: EC P-256 | the key the server proves it holds during the handshake
Valid: 1 Mar → 30 May | not before, not after: certificates expire
Issuer: CN=R11, O=Let's Encrypt | who signed it
Signature | the issuer's signature over all of the above
```

The server keeps the matching **private key** secret. In the handshake it signs something with that key, which proves it really owns the certificate and didn't just copy a public one.

## Who vouches for it: the chain

Anyone can *make* a certificate. What makes one believable is who **signed** it. A **certificate authority** (CA) is an organization that checks that you control a domain name, then signs your certificate. Your operating system and browser ship with a list of CAs they trust, the **trust store**. Their certificates are called **root certificates**.

Roots are precious, so they rarely sign server certificates directly. They sign an **intermediate** certificate, and the intermediate signs the server's. That line of signatures is the **chain**:

```flow
Root CA (in your trust store) -[signs]-> Intermediate CA -[signs]-> shop.example
```

To trust `shop.example`, your client checks each signature up the chain until it reaches a root in its trust store. The server sends its own certificate plus the intermediates; it never needs to send the root, because a root only counts if you *already* have it.

> [!note] Think of it like…
> A reference letter. A stranger's letter saying "Ada is great" means little. But if it's signed by her manager, whose signature is certified by a company you already know well, you believe it. The chain is the list of signatures leading back to someone you already trust.

## What a client checks

1. **Chain**: every signature is valid, up to a trusted root.
2. **Name**: the name you connected to is in the certificate's names.
3. **Dates**: today is between *not before* and *not after*.

If any check fails, the connection stops before a single byte of HTTP is sent. The next lesson is about exactly those failures.

## The TLS inspector

**Tools → TLS inspector** connects to a server and shows what it presents: whether the certificate is **Trusted**, whether the **name matches**, how many days are left, the whole chain with each certificate's subject, issuer and names, and which TLS versions and ciphers the server accepts. It uses the same trust settings as your requests, so it's the first place to look when HTTPS fails.

> [!tip] Private CAs
> Companies often run their own CA for internal services, and test servers (like the lab's) use one too. Their certificates are perfectly good, but no computer trusts them until someone adds that CA. You'll do that properly in the next lesson.

**You'll use this when…** a certificate is about to expire, a partner asks which names your new certificate covers, or a client says "it works in my browser but not in my script". The chain is usually the answer.

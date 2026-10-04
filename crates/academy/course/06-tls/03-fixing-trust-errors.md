---
id: fixing-trust-errors
title: Fixing trust errors the right way
summary: When HTTPS fails, find out which check failed, then fix trust by adding the right CA, never by switching verification off.
minutes: 7
lab:
  title: Diagnose a trust error
  goal: Make an HTTPS request fail, read why, and use the TLS inspector to tell a trust problem from a name problem.
  minutes: 7
  playground: { tls: true }
  steps:
    - text: |
        Send `GET {{playgroundTls}}/json`. The practice server's certificate comes from the lab's private CA, which your computer doesn't know, so the request fails.
      hints:
        - It's an ordinary GET request to an `https://` address.
        - "Open a new request, URL {{playgroundTls}}/json, press Send, and read the error that appears instead of a response."
        - "If it works instead, you already trust the lab's CA: that's the fix at the end of this lesson, and the step counts."
      check:
        any:
          - send: { url: "*/json", errorKind: tls }
          - send: { url: "{{playgroundTls}}/json", status: 200 }
      solution:
        - send: { method: GET, url: "{{playgroundTls}}/json" }
    - text: The error says what went wrong in two words (written together in the technical part). Type them.
      hints:
        - Read the error message from the start. After "invalid peer certificate" comes the reason.
        - It means "the certificate was signed by someone I don't know".
        - "Type: unknown issuer."
      check:
        answer: ["re:(?i)unknown\\s*issuer", "re:(?i)not\\s+trusted"]
      solution:
        - answer: unknown issuer
    - text: |
        A trust error isn't the only kind. Use **Tools → TLS inspector** on `{{=playgroundTls}}` again, this time with **Server name (SNI)** set to `shop.example`, as if you'd connected by a name the certificate doesn't cover. Check **Name matches**.
      hints:
        - The SNI field sets the name your client asks for and checks against the certificate.
        - "Server = `{{=playgroundTls}}`, Server name (SNI) = shop.example, then Inspect."
        - "The summary shows Name matches: No. The certificate only lists localhost, 127.0.0.1 and ::1."
      check:
        call: { method: tools.tlsInspect, ok: true, params: { sni: "*" }, result: { hostnameMatches: false } }
      solution:
        - call: { method: tools.tlsInspect, params: { host: "{{playgroundTls}}", sni: shop.example, runId: lab-tls-sni } }
quiz:
  - question: An internal API fails with "unknown issuer". What is the right fix?
    options:
      - Turn off certificate verification for that host
      - Trust your company's CA certificate, from a source you trust
      - Switch the URL to http:// inside the company network
    answer: 1
    explain: The certificate is fine; your client just doesn't know who signed it. Add that CA, and every check stays on.
  - question: You connect to `https://10.0.0.5`, but the certificate only lists `api.internal`. Which check fails?
    options:
      - The name check
      - The date check
      - The chain check
    answer: 0
    explain: The name you connect to must be one of the certificate's names. Use `https://api.internal` instead of the IP address.
  - question: What does switching off certificate verification really do?
    options:
      - Nothing much, the traffic is still encrypted and private
      - It speeds up the handshake and is safe on company networks
      - Anyone in the middle can pose as the server and read what you send
    answer: 2
    explain: Encryption without authentication protects you from nobody who can sit in the middle. Verification is what makes HTTPS trustworthy.
---

Sooner or later an HTTPS request fails before it even starts, with an error about a certificate. It's tempting to make the error go away. This lesson is about making it go away *correctly*.

## Which check failed?

Remember the three checks from the last lesson. Each has its own error, and its own fix:

| Error says | Check that failed | Usual cause | Right fix |
|---|---|---|---|
| unknown issuer, not trusted | chain | a private or company CA; or the server forgot an intermediate | trust that CA; or fix the server's chain |
| not valid for name | name | you used an IP address or an alias the certificate doesn't list | connect with a name the certificate covers; or reissue it |
| expired, not yet valid | dates | the certificate ran out; or your computer's clock is wrong | renew the certificate; or fix the clock |

The **TLS inspector** answers all three at a glance: **Trusted**, **Name matches**, and the days left.

```flow
HTTPS fails -> TLS inspector -[not trusted]-> trust the CA
TLS inspector -[name doesn't match]-> use the right name
TLS inspector -[expired]-> renew it
```

> [!note] Think of it like…
> A security guard who doesn't recognize a visitor's ID card. "Fire the guard" (switching verification off) gets the visitor in, and every impostor after them. The right fix is to tell the guard which card issuer to accept.

## Why "turn off verification" is not a fix

Zorvik's error message mentions turning certificate verification off, and there's a **Verify TLS certificates** switch in **Settings → Requests** and in each request's **Settings** tab. Leave them on. Without verification your client accepts *any* certificate, so anyone between you and the server can answer in its place. Your data is still encrypted, but to the attacker. And a switch flipped "just for testing" has a way of ending up in scripts, shared workspaces and production.

## The right fix: trust the CA

Your company, or a test setup like this Bootcamp, often runs its own **private CA**. Its certificates are perfectly valid; your computer just doesn't know the CA yet. Tell it:

- **Company CAs** are usually installed on your computer by IT. Zorvik uses the system's trust store, so those already work.
- **Any other CA**: in Zorvik open **Settings → Certificates** (the error message points there too) and choose its PEM file as the **Extra CA certificate**. It is trusted *in addition to* the system's CAs, and every other check stays on.

> [!warning] Only trust a CA you got from someone you trust
> A trusted CA can vouch for *any* name. Get the file through a channel you trust (your IT team, the project's repository), never from the server that's failing, and remove it when you no longer need it.

## Try it after the lab

The lab's CA is in the file `lab-ca.pem`, in the Training Bootcamp workspace folder. That's the `bootcamp` folder inside the app data folder shown in **Settings → Data & privacy**.

1. Open **Settings → Certificates**, press **Browse…** next to **Extra CA certificate**, pick `lab-ca.pem` and **Save**.
2. Send `GET {{playgroundTls}}/json` again: `200 OK`, with verification on. The TLS inspector now says **Trusted**.
3. When you're done, come back and press **Clear**, then **Save**. Resetting the Bootcamp deletes `lab-ca.pem`, and while the setting points to a missing file, Zorvik can't verify any HTTPS server.

**You'll use this when…** a new internal service, a staging server or a corporate proxy answers "unknown issuer". Ask for the CA certificate, add it, and keep verification on.

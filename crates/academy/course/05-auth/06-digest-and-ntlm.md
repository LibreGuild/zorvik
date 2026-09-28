---
id: challenge-auth
title: Digest and NTLM
summary: Some servers answer your first request with a challenge; Digest and NTLM prove you know the password without ever sending it.
minutes: 6
added: 0.2.0
lab:
  title: Answer the challenge
  goal: Read a server's Digest challenge, answer it with Digest auth, then log in to a Windows-style server with NTLM.
  minutes: 7
  playground: true
  vars:
    user: ada
    password: "{{secret.pw}}"
    domain: LAB
  steps:
    - text: |
        Knock without a key first. Send `GET {{playground}}/digest-auth/auth/{{user}}/{{password}}` with no auth. The server answers `401 Unauthorized`, and its `WWW-Authenticate` header holds the challenge. Which `realm` does it name? Type it below.
      hints:
        - New requests inherit the workspace's auth, which is No auth here, so just send it. The challenge is in a response header, not in the body.
        - "After sending, open the response's **Headers** tab and find WWW-Authenticate. It starts with Digest, then name=\"value\" pairs."
        - "It reads realm=\"zorvik@test\". Type zorvik@test in the answer box and press Check."
      check:
        all:
          - send:
              url: "*/digest-auth/auth/ada/{{secret.pw}}"
              status: 401
              responseHeaders: { www-authenticate: "Digest *" }
          - answer: zorvik@test
      solution:
        - send: { method: GET, url: "{{playground}}/digest-auth/auth/{{user}}/{{password}}", auth: { type: none } }
        - answer: zorvik@test
    - text: |
        Now answer the challenge. In the **Auth** tab choose **Digest auth**, username `{{user}}`, password `{{password}}`, and send the same request again. Zorvik reads the challenge, works out the answer and sends the request a second time, all in one click.
      hints:
        - You only give the username and password. The realm, the nonce and the algorithm come from the server's challenge.
        - "Auth tab → Type **Digest auth** → Username {{user}}, Password {{password}} (type the variables with their braces)."
        - "Same URL, Digest auth as above, then Send. You get 200 with \"authenticated\": true. The response's **Info** tab, under Request sent, shows the Authorization: Digest … header that went out."
      check:
        send:
          url: "*/digest-auth/auth/ada/{{secret.pw}}"
          status: 200
          auth: digest
          json: { authenticated: true }
          headers: { authorization: "Digest *" }
      solution:
        - send:
            method: GET
            url: "{{playground}}/digest-auth/auth/{{user}}/{{password}}"
            auth: { type: digest, username: "{{user}}", password: "{{password}}" }
    - text: |
        The office file server runs on Windows and wants NTLM. Send `GET {{playground}}/ntlm/{{domain}}/{{user}}/{{password}}` with **NTLM (Windows)** auth: username `{{domain}}\{{user}}`, password `{{password}}`.
      hints:
        - NTLM takes three messages on one connection (hello, challenge, answer). Zorvik sends all three when you press Send.
        - "Auth tab → Type **NTLM (Windows)** → Username {{domain}}\\{{user}} (or {{user}}, with {{domain}} in **Domain**), Password {{password}}."
        - "URL {{playground}}/ntlm/{{domain}}/{{user}}/{{password}}, NTLM auth as above, then Send. The answer says \"authenticated\": true, with your user and domain."
      check:
        send:
          url: "*/ntlm/LAB/ada/{{secret.pw}}"
          status: 200
          auth: ntlm
          json: { authenticated: true, domain: LAB }
      solution:
        - send:
            method: GET
            url: "{{playground}}/ntlm/{{domain}}/{{user}}/{{password}}"
            auth: { type: ntlm, username: "{{domain}}\\{{user}}", password: "{{password}}" }
quiz:
  - question: With Digest auth, how does your password reach the server?
    options:
      - As plain text in the Authorization header
      - As Base64, like Basic auth
      - It doesn't. Only a hash made from it and the server's nonce travels
    answer: 2
    explain: The server sends a fresh random nonce; Zorvik answers with a hash of your username, password, the nonce and the request. The password itself never leaves your computer.
  - question: Why does a Digest request make two round trips?
    options:
      - The first one gets the challenge (a 401), the second one carries the answer
      - Zorvik sends every request twice to be safe
      - The server is slow to wake up
    answer: 0
    explain: Without the challenge there's nothing to answer. The time Zorvik shows includes both round trips.
  - question: NTLM logs in a connection, not a single request. What follows from that?
    options:
      - You must copy the challenge into the next request by hand
      - Its messages go on one connection, so NTLM uses HTTP/1.1 and can't be load tested
      - Any HTTP version works and each message may use a new connection
    answer: 1
    explain: The answer only counts on the connection that got the challenge. That's why Zorvik keeps NTLM on one HTTP/1.1 connection, and why load tests can't use Digest or NTLM.
---

Basic auth sends your username and password with every request. Base64 only disguises them; anyone who sees the request can decode them in a second. Some servers ask for something smarter: a **challenge**. Instead of "tell me the password", they say "here's a random number: prove you know the password by doing a calculation with it".

## Challenge and response

```sequence
participants: Zorvik, Server
Zorvik -> Server: GET /files
Server --> Zorvik: 401, WWW-Authenticate: Digest realm="…", nonce="7f3a…"
Note over Zorvik: hash of user, password, nonce, method and path
Zorvik -> Server: GET /files, Authorization: Digest … response="c81e…"
Note over Server: does the same calculation and compares
Server --> Zorvik: 200 OK
```

The server's first answer is a `401` with a `WWW-Authenticate` header. For **Digest** auth it looks like this:

```anatomy
Digest | the scheme: answer with Digest auth
realm="zorvik@test" | which set of users and passwords to use
nonce="7f3a9c…" | a random number, new for every challenge
qop="auth" | what the answer covers (auth-int: the body too)
algorithm=MD5 | the hash to use (SHA-256 on newer servers)
```

Zorvik answers with a **hash**: a fingerprint calculated from your username, password, the nonce, the method and the path. The server does the same calculation with its copy of the password. If the two match, you're in.

> [!note] Think of it like…
> A sentry and a password of the day. The sentry calls out a random word; you reply with a word you work out from it and the secret you share. Someone listening learns nothing they can reuse, because tomorrow the sentry calls out a different word.

## Digest in Zorvik

In the **Auth** tab choose **Digest auth** and fill in **Username** and **Password**, ideally as `{{variables}}`. Everything else comes from the server's challenge: the algorithm (MD5, SHA-256 and their `-sess` forms), `qop`, `opaque`. When you press **Send**, Zorvik sends the request, reads the challenge and sends it again with the answer. A wrong password gets the server's `401` as the response.

The response's **Info** tab, under **Request sent**, shows the `Authorization: Digest …` header of the second attempt, and the time includes both round trips.

## NTLM: the Windows way

Windows servers (IIS, SharePoint, Exchange and many company intranets) use **NTLM**. It is a challenge too, but in three messages:

```flow
Negotiate -> Challenge -> Authenticate -> 200 OK
```

NTLM logs in the **connection**, not the request, so all three messages must travel on one connection. Zorvik does that for you, over HTTP/1.1. Choose **NTLM (Windows)** and write the username as `user`, `DOMAIN\user` or `user@domain`; **Domain** and **Workstation** are optional. Servers that insist on Kerberos refuse NTLM.

> [!warning] The password is safe, the data isn't
> A challenge protects your password, not the request or the response. Anyone watching can still read what you send and receive, so use `https://` for anything that matters.

Because the answer only counts on the connection that got the challenge, Digest and NTLM can't be used in load tests, where requests share and reuse connections.

**You'll use this when…** you test an older enterprise system, a router's admin page or a Windows intranet API that answers every request with `401` and a `WWW-Authenticate` header. Pick the scheme it names and Zorvik handles the back-and-forth.

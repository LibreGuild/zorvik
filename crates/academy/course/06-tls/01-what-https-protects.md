---
id: what-https-protects
title: What HTTPS protects
summary: HTTPS is HTTP inside TLS, which keeps what you send private, unchanged and going to the right server.
minutes: 6
lab:
  title: Wiretap a login
  goal: Send a login over plain HTTP through a relay and see everything an eavesdropper would see.
  minutes: 5
  vars:
    password: "{{secret.pw}}"
  servers:
    bank:
      name: Piggy Bank API
      kind: http
      http:
        routes:
          - name: Log in
            method: POST
            path: /login
            headers:
              - { key: Content-Type, value: application/json }
            body: '{"session": "{{secret.session}}", "balance": 1250.75}'
    tap:
      name: Wiretap
      kind: tcpProxy
      proxy:
        target: "{{bank_host}}"
  steps:
    - text: |
        The lab put a **Wiretap** between you and the Piggy Bank API: a relay that passes every byte along, and logs it. Log in *through the wiretap* over plain HTTP:

        `POST http://{{tap_host}}/login` with the JSON body `{"user": "ada", "password": "{{password}}"}`
      hints:
        - The address is the wiretap's, not the bank's. The wiretap forwards everything to the bank.
        - "Method POST, URL http://{{tap_host}}/login, Body: JSON."
        - "JSON body {\"user\": \"ada\", \"password\": \"{{password}}\"}, then Send. You should get 200 with a session."
      check:
        all:
          - message: { server: tap, kind: data, direction: toTarget, text: "*POST /login*" }
          - message: { server: tap, kind: data, direction: toTarget, text: "*{{secret.pw}}*" }
          - request: { server: bank, method: POST, path: /login }
      solution:
        - send:
            method: POST
            url: "http://{{tap_host}}/login"
            body: { type: json, text: "{\"user\": \"ada\", \"password\": \"{{password}}\"}" }
    - text: |
        Now be the eavesdropper. Open **Servers → Lab · Wiretap** and read its traffic. Your password is there in plain text, and so is the bank's answer. Type the `session` value the wiretap saw coming back.
      hints:
        - The traffic list shows each chunk of bytes. Click one to see all of it.
        - One entry is your request going to the bank; the next is the bank's answer coming back.
        - "In the answer, copy the value after \"session\":, without the quotes."
      check:
        answer: "{{secret.session}}"
      solution:
        - answer: "{{secret.session}}"
quiz:
  - question: Which of these does HTTPS hide from someone on the same Wi-Fi?
    options:
      - The address and port of the server you connect to
      - The path, headers and body of your requests
      - The fact that you are using the internet at all
    answer: 1
    explain: TLS encrypts the whole HTTP message. The server's address, and usually its name, still show, because the network needs them to deliver your data.
  - question: What does the "integrity" part of TLS protect against?
    options:
      - Someone changing your data on the way without being noticed
      - Someone reading your data
      - The server going down
    answer: 0
    explain: Every TLS record carries a check that fails if even one bit changes, so tampering is detected.
  - question: A website has a valid certificate and the padlock. What does that prove?
    options:
      - The company behind the site is honest and safe to buy from
      - The server has been checked for security holes
      - You are talking privately to the owner of that domain name
    answer: 2
    explain: HTTPS proves who you're connected to and protects the connection. It says nothing about whether that site is trustworthy.
---

Everything you've sent so far traveled as plain text. Anyone on the path between you and the server could read it: someone on the same café Wi-Fi, a compromised router, a nosy network. **HTTPS** fixes that. It's the same HTTP you already know, sent inside a protected tunnel called **TLS** (Transport Layer Security; its old name, SSL, still pops up).

```layers
HTTP | methods, headers, bodies: your request, unchanged
TLS | encrypts, checks and authenticates everything above it
TCP | delivers bytes, in order
IP | moves packets between addresses
```

## What TLS gives you

1. **Privacy** (encryption): only you and the server can read the data. On the wire it looks like random noise.
2. **Integrity**: if anyone changes a single bit on the way, the other side notices and drops the connection.
3. **Authentication**: before sending anything, your client checks the server's **certificate**, a signed document that proves "I really am shop.example". That stops an attacker from simply pretending to be the server.

## The handshake

Before the first byte of HTTP, client and server run a short **TLS handshake**:

```sequence
participants: You, Server
You -> Server: ClientHello: the TLS versions and ciphers I support, my key share
Server --> You: ServerHello: my choices, my key share
Note over You, Server: both work out the same secret keys; from here on, all is encrypted
Server --> You: Certificate, plus a signature proving I hold its private key
Note over You: checks the certificate: trusted issuer? right name? not expired?
You -> Server: Finished
You -> Server: GET /account (encrypted)
Server --> You: 200 OK (encrypted)
```

The two sides agree on fresh, secret keys without ever sending them: each sends a *key share*, and only the two ends can combine them into the same key. This is called a **key exchange**. A **cipher** is the recipe used to encrypt with that key.

> [!note] Think of it like…
> Sending a postcard versus a sealed, tamper-evident envelope handed to a courier who checks the recipient's ID. Plain HTTP is the postcard: every sorter can read it. HTTPS is the envelope: people still see which address it goes to, but not what's inside, and a broken seal shows.

## What HTTPS does *not* hide

- **Where you connect**: the server's IP address and port, and usually its name (the client sends it in the handshake so the server knows which certificate to show).
- **How much and when**: sizes and timing of the traffic.
- **Anything at either end**: the server still sees everything you send, and so does anything running on your own computer.

> [!warning] Plain HTTP is for your own machine only
> The lab servers use plain `http://` because they never leave your computer. Any real API that handles passwords, tokens or personal data must use `https://`.

**You'll use this when…** someone asks "is it OK to send the API key over http, it's only the test environment?" You'll know the answer is no, and be able to explain what an eavesdropper would see.

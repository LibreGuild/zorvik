---
id: clients-and-servers
title: Clients, servers and connections
summary: A client starts the conversation, a server listens and answers, and a connection carries it.
minutes: 5
lab:
  title: Watch from the server's side
  goal: Send requests as a client, then see them arrive in the server's own traffic log.
  minutes: 6
  servers:
    api:
      name: Front Desk
      kind: http
      http:
        routes:
          - name: "{{secret.word}}"
            method: GET
            path: /menu
            headers:
              - key: Content-Type
                value: application/json
            body: '{"today": ["tomato soup", "falafel wrap", "apple pie"]}'
  steps:
    - text: Be the client. Send `GET {{api}}/menu`.
      hints:
        - The Lab environment is active, so `{{api}}` already holds the server's address.
        - "Press ⌘N (Ctrl+N on Windows), type `{{api}}/menu` and press Send."
      check:
        request: { server: api, method: GET, path: /menu }
      solution:
        - send: { method: GET, url: "{{api}}/menu" }
    - text: |
        Now look from the server's side. Open **Lab · Front Desk** in the **Servers** section of the left rail. In its **Traffic** list, click your `GET /menu` request to expand it. The first line ends with **route** and the name of the route that answered (a word and a number). Type that name.
      hints:
        - Servers is one of the icons in the left rail (hover them to see their names). Lab servers are listed there while the lab runs.
        - Click "Lab · Front Desk", then click the GET /menu line in the Traffic list to see its details.
        - The details start with "GET /menu · HTTP/1.1 → 200 · … · route" followed by the name. Copy that name.
      check:
        answer: "{{secret.word}}"
      solution:
        - answer: "{{secret.word}}"
    - text: |
        Go back to your request and send it **two more times**. Then switch to **Lab · Front Desk** again: each request appeared in its Traffic list the moment it arrived.
      hints:
        - Every request that reaches the server adds a line to its Traffic list, even while you watch.
        - Switch back to your request tab and press Send twice.
        - "⌘↩ (Ctrl+Enter on Windows) sends the request in the current tab. Then look at Lab · Front Desk again: three GET /menu lines."
      check:
        request: { server: api, method: GET, path: /menu, count: 3 }
      solution:
        - send: { method: GET, url: "{{api}}/menu" }
        - send: { method: GET, url: "{{api}}/menu" }
quiz:
  - question: Which side starts the conversation?
    options:
      - The client
      - The server
      - Whichever side is faster
      - The router between them
    answer: 0
    explain: The server listens and waits. The client knows its address and port, and contacts it.
  - question: Your app talks to a server at 10.0.0.5:443. Which port does your app use on its side?
    options:
      - "443 as well"
      - Always port 80
      - A free port the operating system picks, called an ephemeral port
      - None, only servers have ports
    answer: 2
    explain: The client needs a port too, so answers can find their way back. The operating system picks a free one for each connection.
  - question: A teammate says "my request never reached the server". Where do you look first?
    options:
      - The response body in your client
      - The server's traffic log, to see if the request arrived at all
      - The request's Docs tab
      - The list of your computer's network interfaces
    answer: 1
    explain: The server's log shows every request that arrived. If it isn't there, the problem is on the way (address, port, network), not in the server's code.
---

Every conversation on a network has two sides, and they play very different roles.

- A **server** is a program that starts first, **listens** on a port and waits. It never calls anyone; it answers.
- A **client** is a program that knows the server's address and port, and contacts it. Your browser, a phone app and Zorvik are all clients.

"Server" can mean the program or the machine it runs on. One computer can run many servers and many clients at the same time, and one program can be both: a web server that asks a database for data is a client of that database.

## Connections

Before most conversations, the client opens a **connection**: a private, two-way channel between two programs. Once it's open, both sides can send data until one of them hangs up.

```sequence
participants: Client, Server
Note over Server: listening on port 5000
Client -> Server: may I connect?
Server --> Client: connection accepted
Client -> Server: GET /menu
Server --> Client: 200 OK and the menu
Client -> Server: goodbye (close)
```

The client needs a port too, so that answers can find their way back. The operating system picks a free one for it, called an **ephemeral port** (short-lived), such as 54012. A connection is known by four numbers: the client's address and port, and the server's address and port.

```flow
Client 127.0.0.1:54012 -[one connection]-> Server 127.0.0.1:5000
```

That's how one server keeps thousands of clients apart: each connection comes from a different address and port.

> [!note] Think of it like…
> A help desk phone line. The desk (the server) has one well-known number and waits for calls. Every caller (a client) calls from their own number, so the desk always knows who it's talking to. Many calls can be open at once, each on its own line.

## The server's view

A client only sees its own requests and the answers it got back. A server sees every request from every client, plus things the client never sees: where each request came from, which part of the server handled it, and how long that took. That's why, when something breaks, developers ask for the **server logs**.

In Zorvik, servers you run appear in the **Servers** section of the left rail. Open one and its **Traffic** list shows every request as it arrives. Click a request to expand it:

```anatomy
From 127.0.0.1:54012 | who sent it: the client's address and ephemeral port
GET /menu · HTTP/1.1 → 200 · 2 ms | what was asked, the status sent back, how long the server took
route Menu | which of the server's routes (its rules for each path) answered
```

Below that line come the request's headers and body, and the answer's headers and body, exactly as they crossed the wire.

> [!tip] Lab servers are real servers
> The "Lab · …" servers are ordinary Zorvik servers saved in the Bootcamp workspace. Later in the Bootcamp you'll build your own mock servers the same way.

**You'll use this when…** an app says "it works on my side". The server's traffic log shows whether the request arrived at all, and exactly what it looked like when it did.

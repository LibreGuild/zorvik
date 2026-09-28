---
id: ip-addresses-and-ports
title: IP addresses and ports
summary: An IP address finds the computer; a port finds the program on it.
minutes: 5
lab:
  title: Knock on a port
  goal: Find your computer's own address, check that a port is open, then talk to the program behind it.
  minutes: 7
  servers:
    api:
      name: Port Lab
      kind: http
      http:
        routes:
          - method: GET
            path: /hello
            headers:
              - key: Content-Type
                value: application/json
            body: '{"message": "You found the program behind this port!", "secretWord": "{{secret.word}}"}'
  steps:
    - text: |
        Open **Tools** in the left rail (the wrench icon) and pick **Network interfaces**. Find the interface marked **loopback**: its address means "this computer".
      hints:
        - The left rail is the column of icons on the far left of the Workbench.
        - Click the wrench icon (Tools), then "Network interfaces" in the list. If it was already open, press Refresh.
        - Tools → Network interfaces. The row marked loopback shows 127.0.0.1.
      check:
        call: { method: tools.interfaces, ok: true }
      solution:
        - call: { method: tools.interfaces }
    - text: |
        The lab started a small server on your computer, listening on port `{{api_port}}`. Open **Port check** (also under Tools), enter the host `127.0.0.1`, choose **Custom** ports, type `{{api_port}}` and press **Check**.
      hints:
        - Port check tries to connect to each port you give it. "Open" means a program is listening.
        - In Port check, type 127.0.0.1 as the host and click Custom next to Ports.
        - "Host: `127.0.0.1` · Ports: Custom → `{{api_port}}` · Check. The port shows as open."
      check:
        call:
          method: tools.portCheck
          ok: true
          params:
            host: 're:(?i)^(127\.0\.0\.1|localhost)$'
            ports: "*{{api_port}}*"
      solution:
        - call: { method: tools.portCheck, params: { runId: lab-port-check, host: 127.0.0.1, ports: "{{api_port}}", timeoutMs: 1000 } }
    - text: |
        Now talk to the program behind that port. Open a new request (**⌘/Ctrl + N**) and send `GET http://localhost:{{api_port}}/hello`. Type it yourself, with the name **localhost** instead of the IP address.
      hints:
        - localhost is a name for your own computer, just like 127.0.0.1.
        - The URL is http://, then localhost, a colon, the port number, and /hello.
        - "Press ⌘N (Ctrl+N on Windows), type `http://localhost:{{api_port}}/hello` and press Send."
      check:
        request: { server: api, method: GET, path: /hello, headers: { host: "localhost:*" } }
      solution:
        - send: { method: GET, url: "http://localhost:{{api_port}}/hello" }
    - text: The program answered with a `secretWord`. Type it here.
      hints:
        - Look at the response body, below or beside the request.
        - Find "secretWord" in the body and copy the value after it (a word and a number).
        - No response on screen because "Do it for me" sent the request? Open History in the left rail and click the request to see its response.
      check:
        answer: "{{secret.word}}"
      solution:
        - answer: "{{secret.word}}"
quiz:
  - question: What does the address 127.0.0.1 mean?
    options:
      - A server somewhere on the internet
      - Your own computer (the loopback address)
      - Your Wi-Fi router
      - The first computer ever connected to the internet
    answer: 1
    explain: 127.0.0.1 is the loopback address. Traffic sent to it never leaves your computer; localhost is its name.
  - question: The URL `https://shop.example/cart` has no port. Which port does it use?
    options:
      - "80"
      - "8080"
      - "443"
      - None, HTTPS doesn't use ports
    answer: 2
    explain: Without a port, the scheme decides. https uses 443 and http uses 80.
  - question: You start a server on port 3000 and get "address already in use". What does that mean?
    options:
      - Another program is already listening on port 3000
      - Your IP address is wrong
      - Port numbers only go up to 1024
      - Your computer is offline
    answer: 0
    explain: Only one program can listen on a port at a time. Stop the other program, or pick another port.
---

Before two programs can talk, one has to find the other. That takes two numbers: an **IP address** to find the right computer, and a **port** to find the right program on it.

## IP addresses

Every device on a network has an **IP address** (IP stands for Internet Protocol, the rules for moving data between computers). The most common kind, **IPv4**, is four numbers from 0 to 255 separated by dots, like `192.168.1.20`. The newer **IPv6** is longer, written with colons, like `2001:db8::1`. It exists because the world ran out of IPv4 addresses.

A few addresses are special:

| Address | What it means |
|---|---|
| `127.0.0.1` | **Loopback**: "this computer". Traffic sent here never leaves your machine. |
| `localhost` | A name for the loopback address (`::1` in IPv6). |
| `192.168.x.x`, `10.x.x.x` | **Private** addresses, used inside homes and offices. |
| `0.0.0.0` | For servers: "listen on every network this computer is connected to". |

> [!note] Think of it like…
> An apartment building. The IP address is the street address: it gets the mail carrier to the right building. The port is the apartment number: it gets the letter to the right person inside.

## Ports

One computer runs many programs that use the network at the same time: a browser, a chat app, a database. A **port** is a number from 1 to 65535 that says which program a message is for. A program that waits for others to contact it **listens** on a port. Some port numbers are famous:

| Port | Usually |
|---|---|
| 80 | HTTP (the web) |
| 443 | HTTPS (the secure web) |
| 53 | DNS |
| 22 | SSH (remote login) |
| 5432 | PostgreSQL (a database) |

Only one program can listen on a given port at a time. That's why starting the same server twice fails with "address already in use".

## Putting them together

An address and a port together, written `127.0.0.1:8080`, point at exactly one program on exactly one computer. You can see both inside every URL:

```anatomy
http:// | scheme: which protocol to speak
127.0.0.1 | IP address: which computer
:8080 | port: which program on that computer
/hello | path: what you want from that program
```

When a URL has no port, the scheme picks the usual one: 80 for `http`, 443 for `https`.

```flow
Your request -[IP address]-> The right computer -[port]-> The right program
```

> [!tip] Names or numbers
> `localhost` and `127.0.0.1` reach the same place. A name needs one extra step, turning it into a number. That step is called DNS, and it has a unit of its own.

## In Zorvik

**Tools** (the wrench in the left rail) has two helpers for this lesson:

- **Network interfaces** lists your computer's addresses. The one marked **loopback** is `127.0.0.1`. The others are what other devices on your network would use to reach you.
- **Port check** tries to connect to ports on a host and tells you which ones have a program listening (**open**) and which don't (**refused**).

In the lab, a practice server listens on a port picked for you. You'll find your own address, check that the port is open, and then talk to the program behind it.

**You'll use this when…** a service "won't connect". The first two questions are always the same: is this the right address, and is anything listening on that port?

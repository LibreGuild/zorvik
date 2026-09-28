---
id: cookies-and-sessions
title: Cookies and sessions
summary: A server can hand you a cookie; your client stores it and sends it back on every later request, which is how websites remember you're logged in.
minutes: 5
lab:
  title: Get a cookie, send it back, throw it away
  goal: Watch the cookie jar store a cookie, send it automatically, and forget it when you delete it.
  minutes: 6
  playground: true
  vars:
    visitor: "{{secret.visitor}}"
  steps:
    - text: |
        Send `GET {{playground}}/cookies/set?visitor={{visitor}}`. The server answers with a `Set-Cookie` header.
      hints:
        - This practice endpoint turns each query parameter into a cookie.
        - After sending, look at the response's **Cookies** tab, or at `Set-Cookie` in its **Headers** tab.
        - "GET {{playground}}/cookies/set?visitor={{visitor}}, then Send."
      check:
        send: { url: "*/cookies/set?*visitor={{secret.visitor}}*", status: 200, responseHeaders: { set-cookie: "visitor={{secret.visitor}}*" } }
      solution:
        - send: { method: GET, url: "{{playground}}/cookies/set?visitor={{visitor}}" }
    - text: |
        Now send `GET {{playground}}/cookies`. You don't add anything: the cookie jar sends the cookie for you, and this endpoint shows which cookies it received.
      hints:
        - Don't add a Cookie header yourself. That's the jar's job.
        - "GET {{playground}}/cookies, then Send. The body lists visitor with your value."
      check:
        send: { url: "*/cookies", status: 200, json: { cookies: { visitor: "{{secret.visitor}}" } } }
      solution:
        - send: { method: GET, url: "{{playground}}/cookies" }
    - text: |
        "Log out": open the cookie jar (the **Cookies** button in the title bar), delete the `visitor` cookie, then send `GET {{playground}}/cookies` again. The server no longer sees it.
      hints:
        - The Cookies button is the cookie icon at the top right of the window, next to the environment picker.
        - Hover the `visitor` row under `127.0.0.1` and click its trash icon. **Clear all** works too.
        - "Then send GET {{playground}}/cookies again: \"cookies\" should be empty."
      check:
        all:
          - call: { method: "re:^cookies\\.(delete|clear)$", ok: true }
          - send: { url: "*/cookies", status: 200, json: { cookies: { visitor: "!*" } } }
      solution:
        - call: { method: cookies.delete, params: { domain: 127.0.0.1, path: /, name: visitor } }
        - send: { method: GET, url: "{{playground}}/cookies" }
quiz:
  - question: Who decides which cookies your client stores?
    options:
      - You, by adding a Cookie header
      - The server, with Set-Cookie headers in its responses
      - The browser picks them at random
    answer: 1
    explain: The server sends Set-Cookie; the client stores the cookie and sends it back in a Cookie header on matching requests.
  - question: A session cookie is stolen. What can the thief do?
    options:
      - Nothing without your password
      - Only read the cookie's name
      - Act as you until the session ends or is revoked
    answer: 2
    explain: A session cookie works like a bearer token. That's why session cookies should be HttpOnly (hidden from page scripts) and Secure (sent over HTTPS only).
  - question: A test passes on your machine but fails on a teammate's. They never ran the login request first. What might be missing?
    options:
      - A session cookie that your cookie jar still holds
      - A faster network connection to the server
      - "An `Accept: application/json` header"
    answer: 0
    explain: A stored cookie can quietly make requests work. Check the cookie jar, or clear it, before trusting a result.
---

HTTP on its own has no memory: every request stands alone, and the server doesn't know it just talked to you. **Cookies** add memory. A **cookie** is a small `name=value` pair that a server asks your client to store and send back on later requests.

```sequence
participants: You, Server
You -> Server: POST /login (username + password)
Server --> You: 200 OK, Set-Cookie: session=8f2c…
Note over You: the cookie jar stores session=8f2c…
You -> Server: GET /account (Cookie: session=8f2c…)
Server --> You: 200 OK: Ada's account
```

The server sets a cookie with a **`Set-Cookie`** response header. From then on, the client adds a **`Cookie`** request header on every request to that site. When a website "remembers you're logged in", this is how.

## Sessions

A **session** is the server's memory of you: who you are, what's in your basket. The server keeps it on its side and gives you only a random **session id** in a cookie. Every request that brings the cookie back gets connected to the session. Logging out deletes the session on the server, and usually the cookie too.

> [!note] Think of it like…
> A coat check. You hand over your coat (log in) and get a numbered ticket (the session cookie). The coat stays behind the counter (the session on the server); whoever shows the ticket gets the coat. Lose the ticket and a stranger can collect it.

## What's in a Set-Cookie header

```anatomy
Set-Cookie: session=8f2c | the name and value
Path=/ | send it for every path on this site
Max-Age=3600 | forget it after an hour (Expires= does the same with a date)
HttpOnly | scripts in a web page can't read it
Secure | only send it over HTTPS
SameSite=Lax | don't send it with most requests started by other websites
```

A cookie without `Max-Age` or `Expires` is a **session cookie**: a browser forgets it when it closes.

## The cookie jar in Zorvik

Zorvik keeps a **cookie jar** for each workspace, like a browser does. Cookies from responses are stored and sent automatically on matching requests. You can see:

- the cookies one response set, in its **Cookies** tab;
- all stored cookies, with the **Cookies** button in the title bar, where you can delete one or **Clear all**.

You can switch the jar off in **Settings → Data & privacy → Cookie jar** when you want every request to stand alone.

> [!warning] Cookies can hide problems
> Cookies ignore port numbers: every server on `127.0.0.1` shares them. And a cookie left over from yesterday can make a request work that would fail for anyone else. When a result surprises you, check the jar.

**You'll use this when…** you test a web app's login flow, reproduce a "logged out after five minutes" bug, or check that a logout really invalidates the session: delete the cookie, or keep it and confirm the server refuses it after logout.

---
id: status-codes
title: Status codes
summary: A three-digit number says how a request went, and its first digit tells you whose problem it is.
minutes: 6
lab:
  title: Break things on purpose
  goal: Make a server answer 404 and 500, read what a good error looks like, and ask the playground for any status you like.
  minutes: 7
  playground: true
  servers:
    api:
      name: Shop API
      kind: http
      http:
        routes:
          - method: GET
            path: /products/1
            headers:
              - key: Content-Type
                value: application/json
            body: '{"id": 1, "name": "Coffee mug", "price": 9.5}'
          - method: GET
            path: /products/:id
            status: 404
            headers:
              - key: Content-Type
                value: application/json
            body: '{"error": "No product with id {{request.params.id}}"}'
          - method: GET
            path: /checkout
            status: 500
            headers:
              - key: Content-Type
                value: application/json
            body: '{"error": "The payment service did not answer", "incident": "{{secret.incident}}"}'
  steps:
    - text: Ask the shop for a product that doesn't exist, for example `GET {{api}}/products/999`.
      hints:
        - Product 1 exists; most other ids don't.
        - "Send `GET {{api}}/products/999`. The status badge turns amber: 404 Not Found."
      check:
        request: { server: api, status: 404 }
      solution:
        - send: { method: GET, url: "{{api}}/products/999" }
    - text: Now make the server fail. Send `GET {{api}}/checkout`.
      hints:
        - This route is broken on purpose, like a server whose payment provider is down.
        - "Send `GET {{api}}/checkout`. The status badge turns red: 500 Internal Server Error."
      check:
        request: { server: api, path: /checkout, status: 500 }
      solution:
        - send: { method: GET, url: "{{api}}/checkout" }
    - text: A helpful error body gives you something to report. Type the **incident** code from the 500 response.
      hints:
        - Look at the response body of the checkout request.
        - Copy the value of "incident" (a word and a number).
        - No response on screen? Open the checkout request from History, or read the response body in Lab · Shop API's traffic log.
      check:
        answer: "{{secret.incident}}"
      solution:
        - answer: "{{secret.incident}}"
    - text: The practice **playground** answers with any status code you ask for. Send `GET {{playground}}/status/418` and read the reason next to the code.
      hints:
        - "`{{playground}}` is another practice server. `/status/<code>` answers with that code."
        - "Send `GET {{playground}}/status/418`. The badge says 418 I'm a teapot: an April Fools' joke from 1998, so popular that the number is now reserved for it."
      check:
        send: { url: "*/status/418", status: 418 }
      solution:
        - send: { method: GET, url: "{{playground}}/status/418" }
quiz:
  - question: Your request gets 401 Unauthorized. What's the most likely fix?
    options:
      - Wait a minute and retry
      - Send valid credentials, for example log in or add the token
      - Fix a bug in the server's code
      - Use POST instead of GET
    answer: 1
    explain: 401 means "who are you?". The server needs credentials it can check. 403 would mean "I know who you are, and the answer is no".
  - question: Which family of status codes means the server itself failed?
    options:
      - 2xx
      - 3xx
      - 4xx
      - 5xx
    answer: 3
    explain: 5xx codes are server errors. 4xx codes mean the request was wrong.
  - question: 'An API answers 200 OK with the body {"error": "out of stock"}. What should your test check?'
    options:
      - Only the status code
      - Only that the body is valid JSON
      - Both the status code and the body
      - Nothing, 200 means success
    answer: 2
    explain: Some APIs report errors inside a 200 response. Checking the body too catches them.
---

Every response starts with a **status code**: three digits that tell the client how things went, before it reads a single byte of the body. There are dozens of codes, but the first digit alone tells you most of the story.

| Family | Means | What to do |
|---|---|---|
| **1xx** | informational: "hold on" | rarely seen directly |
| **2xx** | success | use the answer |
| **3xx** | redirection: "it's over there" | follow the `Location` header (clients do it for you) |
| **4xx** | client error: "you asked wrong" | fix the request |
| **5xx** | server error: "I broke" | not your fault; retry later or tell the server's team |

```flow
Response -[2xx]-> Use the answer
Response -[3xx]-> Follow the new address
Response -[4xx]-> Fix the request
Response -[5xx]-> Retry later or report it
```

## The ones you'll meet every day

- **200 OK**: here you go. **201 Created**: the new thing exists (after a POST). **204 No Content**: done, nothing to send back.
- **301** and **308**: moved for good. **302** and **307**: temporarily somewhere else. **304 Not Modified**: the copy you already have is still fine.
- **400 Bad Request**: the server couldn't make sense of the request, such as broken JSON. **401 Unauthorized**: who are you? Send credentials. **403 Forbidden**: I know who you are, and the answer is no. **404 Not Found**: nothing at that address. **409 Conflict**: it clashes with the current state, like a username that's taken. **422 Unprocessable Content**: understood, but the data is invalid. **429 Too Many Requests**: slow down.
- **500 Internal Server Error**: the server hit a bug. **502 Bad Gateway** and **504 Gateway Timeout**: a server in the middle (a proxy or load balancer) got a bad answer, or none, from the one behind it. **503 Service Unavailable**: overloaded or down for maintenance.

> [!note] Think of it like…
> Ordering at a coffee counter. 2xx: "here's your coffee". 3xx: "we moved, please go to the other counter". 4xx: "we don't sell pizza" (you asked for something wrong). 5xx: "the coffee machine just broke" (not your fault).

> [!warning] The code and the body can disagree
> Some APIs answer `200 OK` with `{"error": "…"}` in the body. Always look at both. Later in the Bootcamp you'll write tests that check both automatically.

## Breaking things on purpose

The unhappy paths matter as much as the happy one. Does your app show a helpful message on a 404? Does it retry after a 503, and stop retrying after a 400? To find out, you need servers that fail on demand. In the lab you'll use a mock server with a broken route, and the **playground**, a practice server where `/status/<code>` answers with any code you ask for.

In Zorvik the status sits in a colored badge at the top of the response: green for 2xx, blue for 3xx, amber for 4xx and red for 5xx.

**You'll use this when…** something fails. The status code tells you where to look first: for a 4xx, read your request again; for a 5xx, check the server's logs.

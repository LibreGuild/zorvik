---
id: first-request
title: Your first request
summary: A request asks a server for something; the response is its answer.
minutes: 4
lab:
  title: Say hello to a server
  goal: Send a request to a practice server and read its answer.
  minutes: 6
  servers:
    api:
      name: Hello API
      kind: http
      http:
        routes:
          - method: GET
            path: /hello
            headers:
              - key: Content-Type
                value: application/json
            body: '{"message": "Hello from your first server!", "secretWord": "{{secret.word}}"}'
  steps:
    - text: |
        Open a new HTTP request (**⌘/Ctrl + N**), type `{{api}}/hello` in the URL bar and press **Send**.
      hints:
        - The "Lab" environment is already active. `{{api}}` is a variable that holds the lab server's address.
        - Press ⌘N (Ctrl+N on Windows), click into the URL bar, type {{api}}/hello, then press Send or ⌘↩.
        - "Method GET, URL {{api}}/hello, then Send. The response appears below or beside the request."
      check:
        request: { server: api, method: GET, path: /hello }
      solution:
        - send: { method: GET, url: "{{api}}/hello" }
    - text: The server's answer contains a `secretWord`. Type it here.
      hints:
        - Look at the response body, the part in curly braces.
        - Find the line that says "secretWord" and copy the word after it.
      check:
        answer: "{{secret.word}}"
      solution:
        - answer: "{{secret.word}}"
quiz:
  - question: What is a request?
    options:
      - A message your app sends to a server, asking for something
      - A file saved on your disk
      - A server's answer
    answer: 0
    explain: A request asks; the response answers.
  - question: What does `{{api}}` in a URL mean?
    options:
      - It is sent to the server exactly like that
      - It is a variable; Zorvik replaces it with its value before sending
      - It is a comment
    answer: 1
    explain: Double curly braces mark variables. Hover one in Zorvik to see its value.
  - question: The response shows `200 OK`. What does that tell you?
    options:
      - The server could not find the page
      - The request worked
      - The server crashed
    answer: 1
    explain: 200 is the "all good" status. You'll meet many more status codes in the HTTP unit.
---

Every time an app shows you something from the internet (a weather forecast, a chat message, a photo) it **asks a server for it**. The question is a **request**. The server's answer is a **response**.

```sequence
You -> Server: GET /hello
Note over Server: finds /hello
Server --> You: 200 OK {"message": "Hello!"}
```

## The parts you will use today

- **Method:** what you want to do. `GET` means "give me this". It never changes anything.
- **URL:** where to ask. For example `http://127.0.0.1:5000/hello`.
- **Status:** a three-digit number in the response. `200` means "OK, here you go".
- **Body:** the content of the response, often **JSON**, a simple text format made of `"name": value` pairs.

```anatomy
GET http://127.0.0.1:5000/hello | method and URL: what and where
200 OK | status: did it work?
{"message": "Hello!"} | body: the answer itself
```

> [!note] Think of it like…
> A restaurant. You (the client) give your order (the request) to the kitchen (the server). The waiter brings back your dish (the response) and tells you if something went wrong ("sorry, we're out of soup").

## Variables

In the lab the server's address is kept in a **variable** called `api`. You write it as `{{api}}` and Zorvik fills in the real address when it sends. Hover a variable in the URL bar to see its value.

**You'll use this when…** you call the same API on your laptop, a test server and production. The requests stay the same; only the variables change.

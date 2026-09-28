---
id: build-a-mock
title: Build a mock API
summary: A mock is a list of routes; each one matches a method and path and answers with a status, headers and a body.
minutes: 5
lab:
  title: Open the café
  goal: Build a mock API on your own computer and make it answer the café app's menu request.
  minutes: 8
  steps:
    - text: |
        Open **Servers** on the left rail, click **＋** and choose **Mock API (HTTP)**. Name it **Cafe API** and press **Create**.
      hints:
        - Servers are saved in the workspace, just like requests. Hover the icons on the left rail to find the Servers section.
        - In the Servers section, the ＋ button next to the filter box lists the kinds of servers you can make.
        - Servers → ＋ → Mock API (HTTP) → type Cafe API → Create.
      check:
        saved:
          server: { name: Cafe API, kind: http }
      solution:
        - call:
            method: server.create
            params:
              server:
                name: Cafe API
                kind: http
                port: 0
                http:
                  routes:
                    - method: GET
                      path: /hello
                      status: 200
                      headers: [{ key: Content-Type, value: application/json }]
                      body: "{\n  \"message\": \"Hello from Zorvik\"\n}"
    - text: |
        Press **Start**. The dot turns green and the tab shows the address your mock listens on, such as `http://127.0.0.1:3000`.
      hints:
        - Starting a server makes it listen for requests on your computer until you stop it.
        - The Start button is at the top right of the Cafe API tab. You can also hover the server in the sidebar and press ▶.
        - If Zorvik says the port is in use, another program already has it. Change **Port** to another number, such as 3100, and press Start again.
      check:
        saved:
          server: { name: Cafe API, running: true }
      solution:
        - call:
            method: server.save
            params:
              id: Cafe API
              server: &cafe
                name: Cafe API
                kind: http
                port: 0
                http:
                  routes:
                    - method: GET
                      path: /hello
                      status: 200
                      headers: [{ key: Content-Type, value: application/json }]
                      body: "{\n  \"message\": \"Hello from Zorvik\"\n}"
        - call:
            method: server.start
            params: { id: Cafe API, server: *cafe }
    - text: |
        Teach it the menu. In **Routes**, select `GET /hello` and change its path to `/menu` (or press **Add route**). Set the **Response body** to:

        ```json
        [
          {"dish": "Tomato soup", "price": 4.5},
          {"dish": "Grilled cheese", "price": 6}
        ]
        ```

        The running mock picks up your edits as you type. Save with **⌘/Ctrl + S** to keep them. The lab then calls `GET /menu`, just like the café app would.
      hints:
        - A route is a method and a path, plus the answer to send back. The app only sees the answer.
        - Click the GET /hello row under Routes. In the Route section, edit the path; paste the JSON into Response body. Keep the server running.
        - "Method GET, path /menu, status 200, header Content-Type: application/json, and a body with a dish called Tomato soup. Then press ⌘S."
      check:
        probe:
          kind: http
          name: Cafe API
          path: /menu
          expect:
            status: 200
            json: [{ dish: Tomato soup }]
      solution:
        - call:
            method: mock.addRoute
            params:
              serverId: Cafe API
              route:
                method: GET
                path: /menu
                status: 200
                headers: [{ key: Content-Type, value: application/json }]
                body: "[\n  {\"dish\": \"Tomato soup\", \"price\": 4.5},\n  {\"dish\": \"Grilled cheese\", \"price\": 6}\n]"
quiz:
  - question: A request arrives for `GET /menu`, but your mock only has a route for `GET /hello`. What does the mock answer?
    options:
      - 200 with an empty body
      - Nothing, it crashes
      - 404, with a list of the routes it knows
    answer: 2
    explain: Requests no route matches get a 404 that lists the mock's routes, so a typo in a path is easy to spot. You can also forward them to a real back end instead.
  - question: What must match for a route to answer a request?
    options:
      - The method and the path
      - The response body
      - The name of the route
    answer: 0
    explain: A route matches on method and path (and optional conditions). The status, headers and body are what it answers with.
  - question: Your mock runs at `http://127.0.0.1:3000`. Who can call it?
    options:
      - Anyone on the internet
      - Only programs on your own computer
      - Only Zorvik's lab
    answer: 1
    explain: 127.0.0.1 means "this computer". To let a phone on your Wi-Fi call the mock, set Listen on to "Other devices too (0.0.0.0)".
---

A mock API in Zorvik is a small web server that runs on your computer. You describe what it should answer, press **Start**, and any app (a browser, a phone emulator, a test script, or Zorvik itself) can call it.

## Routes

A mock is a list of **routes**. Each route says: *when a request with this method and path comes in, answer with this*.

```anatomy
GET /menu | method and path: which requests this route answers
200 | status: how the answer starts
Content-Type: application/json | a response header: what kind of body follows
[{"dish": "Tomato soup"}] | response body: the data the app receives
```

When a request arrives, the mock goes down the list and **the first enabled route that matches answers**. Paths can contain a part that changes, written `:name`, such as `/users/:id`, which matches `/users/7` and `/users/42`. You'll use those in the next lesson.

If no route matches, the mock answers `404 Not Found` with a list of the routes it does know, so a typo like `/meun` is easy to spot.

```sequence
participants: Café app, Mock
Café app -> Mock: GET /menu
Note over Mock: finds the GET /menu route
Mock --> Café app: 200 OK [{"dish": "Tomato soup"}]
Café app -> Mock: GET /drinks
Mock --> Café app: 404 Not Found (known routes: GET /menu)
```

> [!note] Think of it like…
> A receptionist with a cheat sheet. Each line of the sheet is a route: "if someone asks for the menu, hand them this card". Anything not on the sheet gets a polite "sorry, we don't have that here", plus the list of things they can ask for.

## Where your mock lives: address and port

Your mock listens at an **address** such as `http://127.0.0.1:3000`:

- `127.0.0.1` means *this computer*. Only programs on your machine can reach it, which is the safe default. Under **Listen on** you can choose **Other devices too (0.0.0.0)** so a phone on the same Wi-Fi can call it.
- `3000` is the **port**, a number that tells your computer which program should get the request. Only one program can use a port at a time. If Zorvik says the port is in use, pick another one.

## Changing a running mock

You don't need to restart after every change. While the mock runs, edits to its routes apply as you type. Changing the address, port or TLS asks for a **Restart**. Save with **⌘/Ctrl + S** to write the mock to the workspace, so it is still there tomorrow and your teammates get it too.

On the right of the server tab, the **Traffic** list shows every request the mock received and what it answered. When an app "gets the wrong data", this is the first place to look.

> [!tip] Start small
> A route with a hard-coded answer is enough for most screens. Add more routes as the app needs them, not all at once.

**You'll use this when…** a front-end developer asks "can I have something that answers `GET /menu` while the back end is being built?" Five minutes later, they can.

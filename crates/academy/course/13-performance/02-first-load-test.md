---
id: first-load-test
title: Your first load test
summary: A load test sends one saved request from many simulated users at once, and measures how the server copes.
minutes: 5
lab:
  title: Rush hour at the bakery
  goal: Turn a saved request into a small load test, run it and read the results.
  minutes: 8
  servers:
    api:
      name: Bakery API
      kind: http
      http:
        routes:
          - method: GET
            path: /breads
            headers: [{ key: Content-Type, value: application/json }]
            body: '[{"name": "Sourdough", "price": 4.2}, {"name": "Rye", "price": 3.8}, {"name": "Baguette", "price": 2.5}]'
            delayMs: 20
  steps:
    - text: |
        A load test sends **saved** requests. Create a request `GET {{api}}/breads`, send it once to see that it works, and save it (**⌘/Ctrl + S**) with the name **Breads**.
      hints:
        - The "Lab" environment is active, so {{api}} already holds the bakery's address.
        - Press ⌘N (Ctrl+N), type {{api}}/breads, press Send, then ⌘S.
        - Name it Breads and save it at the top of the collection.
      check:
        saved:
          request: { name: Breads, method: GET, url: "*/breads" }
      solution:
        - save: { name: Breads, method: GET, url: "{{api}}/breads" }
    - text: |
        Right-click **Breads** in the collection and choose **Load test…**. In the new load test, under **Stages**, pick **Constant**, set **Peak (users)** to **2** and **Duration (seconds)** to **3**. Save it (**⌘/Ctrl + S**).
      hints:
        - The new load test already lists Breads under Requests. The default plan is 10 users for a whole minute; you only need a few seconds.
        - Stages has three shape buttons. Constant means the full load from the first second to the last.
        - Right-click Breads → Load test…, then Stages → Constant, Peak (users) 2, Duration (seconds) 3, and ⌘S.
      check:
        saved:
          loadTest:
            targets: [{ request: "*Breads.yaml" }]
      solution:
        - call:
            method: load.create
            params:
              test: &bakeryTest
                name: Breads load test
                targets: [{ request: Breads.yaml }]
                model: virtualUsers
                stages:
                  - { durationSecs: 0, target: 2 }
                  - { durationSecs: 3, target: 2 }
                thresholds:
                  - { metric: p95, op: "<", value: 500 }
                  - { metric: errorRate, op: "<", value: 1 }
    - text: |
        Press **Start** (**⌘/Ctrl + Enter**) and watch the charts fill in. After three seconds the run ends and its results stay on screen.
      hints:
        - Load tests only send traffic after you press Start. Zorvik asks first if a test would send load outside your computer or local network; this one stays on 127.0.0.1.
        - The Start button is at the top right of the load test tab.
        - Open Breads load test and press Start, then wait until the progress bar is full.
      check:
        load: { requests: ">0" }
      solution:
        - call:
            method: load.start
            params: { id: Breads load test, test: *bakeryTest }
        - wait: 3500
    - text: |
        Read the results: **Requests / s**, the **p50**, **p95** and **p99 latency**, and the **Error rate**. How many of the requests failed? Type the number.
      hints:
        - Failed requests are network errors or answers with a status of 400 or more.
        - Look at Error rate, or at the Status codes panel below the charts.
        - The bakery never fails, so the answer is 0.
      check:
        answer: ["0", "zero", "none", "no", "0%", "0 %"]
      solution:
        - answer: "0"
quiz:
  - question: What is a virtual user?
    options:
      - A person Zorvik hires to click through your app
      - A user account created in your database
      - A simulated client that sends a request, waits for the answer, and sends the next one
    answer: 2
    explain: Each virtual user is a small loop inside Zorvik. Ten virtual users means ten such loops running at the same time.
  - question: Before running your first load test against a new server, what should you check?
    options:
      - That you own the server or have permission to load test it
      - That the server is in another country
      - That nobody has load tested it before
    answer: 0
    explain: A load test can slow a server down for everyone using it. Only test systems you own or are allowed to test; Zorvik asks before sending load outside your computer or local network.
  - question: Why does a load test use saved requests instead of the open tab?
    options:
      - Unsaved requests are slower
      - The test must send the same, known requests every run, so results can be compared
      - Saved requests skip authentication
    answer: 1
    explain: A load test is a repeatable experiment. Saved requests are stable, so two runs a week apart measure the same thing.
---

Sending one request tells you how fast the API answers *you*. But what happens on Monday morning, when a thousand people open the app at the same time? A **load test** finds out, before those people do.

## What a load test does

A load test takes one or more **saved requests** and sends them again and again, from many simulated clients at the same time, for a while. Each simulated client is a **virtual user**: it sends a request, waits for the answer, and sends the next one, like a person tapping through an app very quickly.

```flow
Saved request -> Load test -> Virtual users -> Server
Server -[answers]-> Results
```

How many users, and for how long, is set by **stages**. The **Constant** shape runs the full load from start to finish. **Ramp up, hold, ramp down** starts gently, holds, and eases off, like a morning rush. **Spike** adds a short burst, like a flash sale.

> [!note] Think of it like…
> A restaurant's soft opening. Before the big night, the owners invite 30 friends to order all at once, to see whether the kitchen keeps up. Better to learn that the fryer is too small in front of friends than in front of paying guests.

## Reading the results

While a test runs, Zorvik draws throughput and latency over time. When it ends, the numbers you learned in the last lesson are waiting:

| Result | What it tells you |
|---|---|
| **Requests / s** | throughput: how much work got done |
| **p50 / p95 / p99 latency** | how fast the typical and the slow requests were |
| **Error rate** | the share of requests that failed |
| **Status codes** | how many of each status came back |

Zorvik keeps each test's recent runs (up to 30) in its history, so you can open earlier runs later.

## Start small, and be kind

Your first run against any system should be small: a couple of users for a few seconds. Then grow step by step. A load test is real traffic; a big one can slow a shared test server down for everyone else.

> [!warning] Only load test what you're allowed to
> Sending heavy traffic to a server you don't own can look like an attack. Zorvik asks for confirmation before a load test sends traffic outside your computer or local network. In this unit, every test stays on `127.0.0.1`.

**You'll use this when…** a new feature is about to go live and someone asks, "Will it hold up when the newsletter goes out to 50,000 people?"

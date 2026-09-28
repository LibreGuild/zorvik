---
id: breaking-point
title: Compare runs and find the breaking point
summary: Raise the load step by step and compare each run with the last; the breaking point is where throughput stops growing and latency takes off.
minutes: 6
lab:
  title: How much can checkout take?
  goal: Run a baseline, run again with four times the users, and compare the two runs.
  minutes: 8
  servers:
    api:
      name: Checkout Service
      kind: http
      http:
        routes:
          - method: GET
            path: /checkout
            headers: [{ key: Content-Type, value: application/json }]
            body: '{"items": 3, "total": 42.9, "currency": "EUR"}'
            delayMs: 100
  files:
    requests/Checkout.yaml: "name: Checkout\nseq: 0\nmethod: GET\nurl: '{{api}}/checkout'\n"
    loadtests/Checkout load test.yaml: |
      name: Checkout load test
      seq: 0
      targets:
        - request: Checkout.yaml
      model: virtualUsers
      stages:
        - durationSecs: 0
          target: 2
        - durationSecs: 3
          target: 2
      thresholds:
        - metric: p95
          op: "<"
          value: 500
        - metric: errorRate
          op: "<"
          value: 1
  steps:
    - text: |
        The lab prepared **Checkout load test**: 2 virtual users for 3 seconds against a checkout that takes about 100 ms per answer. Open it from **Load tests** on the left rail and press **Start**. This first run is your **baseline**.
      hints:
        - A baseline is the run you compare everything else with. Keep it small and simple.
        - Hover the icons on the left rail to find Load tests, then click Checkout load test.
        - Load tests → Checkout load test → Start. Wait until the run finishes.
      check:
        load: { name: Checkout load test, requests: ">0" }
      solution:
        - call:
            method: load.start
            params:
              id: Checkout load test
              test:
                name: Checkout load test
                targets: [{ request: Checkout.yaml }]
                model: virtualUsers
                stages:
                  - { durationSecs: 0, target: 2 }
                  - { durationSecs: 3, target: 2 }
                thresholds:
                  - { metric: p95, op: "<", value: 500 }
                  - { metric: errorRate, op: "<", value: 1 }
        - wait: 3500
    - text: |
        Step up: set **Peak (users)** to **8**, save (**⌘/Ctrl + S**) and press **Start** again.
      hints:
        - Change one thing at a time, so you know what caused any difference.
        - Peak (users) is in the Stages section, next to Duration (seconds).
        - Peak (users) 8, ⌘S, Start, and wait for the run to finish.
      check:
        load: { name: Checkout load test, rps: ">=45" }
      solution:
        - call:
            method: load.save
            params:
              id: Checkout load test
              test: &eightUsers
                name: Checkout load test
                targets: [{ request: Checkout.yaml }]
                model: virtualUsers
                stages:
                  - { durationSecs: 0, target: 8 }
                  - { durationSecs: 3, target: 8 }
                thresholds:
                  - { metric: p95, op: "<", value: 500 }
                  - { metric: errorRate, op: "<", value: 1 }
        - call:
            method: load.start
            params: { id: Checkout load test, test: *eightUsers }
        - wait: 3500
    - text: |
        Press **Compare** above the results and pick the baseline. **Requests / s** grew about four times, while **p95 latency** stayed near 100 ms: no breaking point yet.

        Each checkout takes 100 ms. At most, how many requests per second can **one** virtual user send? Type the number.
      hints:
        - One user sends a request, waits for the answer, then sends the next.
        - One second is 1000 ms. How many 100 ms answers fit in one second?
        - 1000 ms ÷ 100 ms = 10 requests per second, per user.
      check:
        answer: ["10", "ten", "10 req/s", "10 requests per second", "10/s"]
      solution:
        - answer: "10"
quiz:
  - question: You double the users and throughput doubles too, while p95 stays the same. What does that tell you?
    options:
      - The server still has room; you haven't reached the breaking point
      - The server just crashed
      - The test is wrong, because throughput can't double
    answer: 0
    explain: Healthy scaling looks exactly like that. Keep stepping up until throughput stops growing or latency and errors climb.
  - question: Which pattern marks the breaking point?
    options:
      - More users and more throughput at the same latency
      - More users, but throughput stays flat while p95 and errors climb
      - Fewer users and lower latency
    answer: 1
    explain: At the knee the server can't do more work per second, so extra users only wait in line, and waiting shows up as latency, then timeouts and errors.
  - question: Every answer takes 200 ms. Roughly how many virtual users do you need for 50 requests per second?
    options:
      - "5"
      - "50"
      - "10"
    answer: 2
    explain: One user can send at most 1000 ÷ 200 = 5 requests per second, so 50 req/s needs about 10 users (throughput = users ÷ time per request).
---

"How many users can we handle?" is one of the most common questions in software, and one of the most often guessed. You can measure it instead: raise the load step by step and **compare** each run with the one before.

## Step up, compare, repeat

```flow
Baseline run -> More load -> Compare with the last run
Compare with the last run -[still healthy]-> More load
Compare with the last run -[knee found]-> Write it down
```

Start with a small, calm **baseline**. Then double the load, run again, and compare. Change only one thing between runs, and keep the same test, the same data and the same machines, or the comparison means nothing.

In Zorvik, open a run and press **Compare** to pick an earlier run of the same test. You get both runs side by side with the **Change** for each number: throughput, error rate and latency percentiles, colored green when it got better and red when it got worse.

## What the breaking point looks like

Here is a real-world style step-up test of a busy server:

| Users | Requests / s | p95 latency | Error rate |
|---|---|---|---|
| 10 | 98 | 110 ms | 0 % |
| 20 | 195 | 115 ms | 0 % |
| 40 | 310 | 180 ms | 0 % |
| 80 | 330 | 620 ms | 0.4 % |
| 160 | 325 | 1.9 s | 6 % |

Up to 40 users, more users means more work done. After that, throughput stops growing, but latency explodes and errors appear. That bend is called the **knee**, and it's your breaking point: somewhere between 40 and 80 users. The extra users don't get more done; they just wait in line.

> [!note] Think of it like…
> A supermarket with four checkouts. More shoppers means more sales, until all four tills are busy. After that, extra shoppers don't buy any faster; the lines just get longer, and eventually people give up and leave (errors).

## A handy rule of thumb

For virtual users there's a simple relationship (called *Little's law*):

```text
throughput  =  users ÷ time per request
8 users ÷ 0.1 s  =  80 requests per second
```

It tells you what to *expect*. If you measure much less than it predicts, something is waiting: a queue, a lock, a slow database.

> [!warning] Mocks never break
> The checkout in this lab is a mock: it just waits 100 ms, as many times in parallel as you like, so its throughput keeps growing. Real servers run out of workers, database connections or CPU. Find their knee on a test system that looks like production, never on production itself.

**You'll use this when…** a product manager asks "can we survive Black Friday with twice last year's traffic?" and you want to answer with a table of runs instead of a shrug.

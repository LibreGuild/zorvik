---
id: load-thresholds
title: Users vs requests per second, and thresholds
summary: Choose how load is generated (users or a fixed rate), and let thresholds decide automatically whether a run passed.
minutes: 6
lab:
  title: A search that must stay snappy
  goal: Load test a search at a fixed request rate, with thresholds that pass, then tighten one until the run fails.
  minutes: 8
  servers:
    api:
      name: Search API
      kind: http
      http:
        routes:
          - method: GET
            path: /search
            headers: [{ key: Content-Type, value: application/json }]
            body: '{"query": "{{request.query.q}}", "results": [{"name": "Hiking boots"}, {"name": "Rain boots"}]}'
            delayMs: 50
  files:
    requests/Search.yaml: "name: Search\nseq: 0\nmethod: GET\nurl: '{{api}}/search?q=boots'\n"
  steps:
    - text: |
        The lab saved a request **Search** in your collection. Right-click it → **Load test…**. Set **Load model** to **Request rate**, and under **Stages** pick **Constant** with **Peak (req/s)** **20** and **Duration (seconds)** **3**. Keep the two thresholds (p95 latency under 500 ms, error rate under 1 %) and save (**⌘/Ctrl + S**).
      hints:
        - Request rate starts requests on a schedule, however slowly the server answers.
        - Load model is a two-way switch above Stages. With Request rate, Peak is counted in requests per second.
        - Right-click Search → Load test… → Load model Request rate → Stages Constant, Peak 20, Duration 3 → ⌘S.
      check:
        saved:
          loadTest:
            model: arrivalRate
            targets: [{ request: "*Search.yaml" }]
            thresholds: [{ metric: p95, op: "<" }]
      solution:
        - call:
            method: load.create
            params:
              test: &searchTest
                name: Search load test
                targets: [{ request: Search.yaml }]
                model: arrivalRate
                stages:
                  - { durationSecs: 0, target: 20 }
                  - { durationSecs: 3, target: 20 }
                thresholds:
                  - { metric: p95, op: "<", value: 500 }
                  - { metric: errorRate, op: "<", value: 1 }
    - text: |
        Press **Start**. When the run ends, the **Thresholds** panel shows whether each rule held, and the run is marked passed or failed.
      hints:
        - Thresholds are checked once, at the end of the run.
        - The search takes about 50 ms, far below 500 ms, so both thresholds should pass.
        - Press Start in the Search load test tab and wait three seconds.
      check:
        load: { passed: true, requests: ">0" }
      solution:
        - call:
            method: load.start
            params: { id: Search load test, test: *searchTest }
        - wait: 3500
    - text: |
        Now make the rule stricter than the server can manage. Change the p95 threshold to **under 20 ms**, save, and run again. Every search takes about 50 ms, so this run must fail.
      hints:
        - A failing threshold is not a broken test. It's the test doing its job, telling you the goal was missed.
        - In Thresholds, change the value next to "p95 latency <" from 500 to 20.
        - Set the p95 threshold to < 20 ms, press ⌘S, then Start. Let the run finish without stopping it.
      check:
        load: { passed: false, stoppedEarly: false, requests: ">0" }
      solution:
        - call:
            method: load.save
            params:
              id: Search load test
              test: &strictTest
                name: Search load test
                targets: [{ request: Search.yaml }]
                model: arrivalRate
                stages:
                  - { durationSecs: 0, target: 20 }
                  - { durationSecs: 3, target: 20 }
                thresholds:
                  - { metric: p95, op: "<", value: 20 }
                  - { metric: errorRate, op: "<", value: 1 }
        - call:
            method: load.start
            params: { id: Search load test, test: *strictTest }
        - wait: 3500
quiz:
  - question: With virtual users, the server suddenly gets twice as slow. What happens to the number of requests it receives?
    options:
      - It stays the same
      - It drops, because each user waits longer before sending the next request
      - It doubles
    answer: 1
    explain: Virtual users wait for each answer, so a slower server gets fewer requests. The request rate model keeps sending on schedule, so the slowdown shows up as queueing and higher latency instead.
  - question: Your API gets about 200 requests per second from thousands of phones. Which model matches that best?
    options:
      - Virtual users, 1 user
      - Virtual users, 200,000 users with long think times
      - Request rate, at 200 req/s
    answer: 2
    explain: Many independent clients arrive at a rate, whatever your server is doing. That's exactly the request rate model.
  - question: A run finishes with "p95 latency < 300 ms" failed. What does that tell you?
    options:
      - More than 5 % of the requests took longer than 300 ms
      - The load test itself is broken
      - Every request took longer than 300 ms
    answer: 0
    explain: p95 over 300 ms means the slowest 5 % were slower than the goal. Time to find out why, or to agree on a realistic goal.
---

In the last lab, each virtual user sent a request, waited for the answer and sent the next. That's one way to create load. There's a second one, and the difference matters more than you'd think.

## Two ways to create load

**Virtual users** (a *closed* model): each user sends, waits for the answer, maybe pauses (the **think time**), and sends again. The number of users is fixed. If the server slows down, each user waits longer, so fewer requests arrive. Good for "50 people using the app at once".

**Request rate** (an *open* model): requests start on a schedule, say 20 per second, however slow the server gets. If the server slows down, requests pile up and you see it clearly in the latency. Good for "our API receives 200 requests per second from many different clients".

```flow
Virtual users -[send, wait, repeat]-> Server
Request rate -[start on schedule]-> Server
```

> [!note] Think of it like…
> A coffee shop. Virtual users are ten regulars who only order again after their coffee arrives: if the barista is slow, fewer orders come in and the queue looks short. Request rate is the door opening every few seconds, whether or not the queue is moving: a slow barista means a long, visible line.

> [!warning] Virtual users can hide slowness
> Because users wait, a struggling server receives *less* traffic, which makes it look healthier than it is. When you know the traffic you expect per second, prefer **Request rate**.

## Thresholds: pass or fail, automatically

Staring at charts after every run doesn't scale. A **threshold** is a rule the run must meet, checked when it ends:

```text
p95 latency   < 500 ms
Error rate    < 1 %
Throughput    >= 100 req/s
```

If every threshold holds, the run **passed**; if any fails, the run **failed**, and Zorvik shows which rule missed and by how much. A threshold can apply to the whole test or to one request. New load tests start with two sensible ones: p95 under 500 ms and error rate under 1 %.

Thresholds turn a load test into something you can repeat before every release: run it, and if it's green, the performance goal still holds.

> [!tip] Where do goals come from?
> Ask what users need, not what the server does today. "The search box must show results within 300 ms for 95 % of searches" is a goal. Write it as a threshold, and the test keeps you honest.

**You'll use this when…** the team agrees on a performance goal and you want every future run to say "still OK" or "we broke it" without anyone reading charts.

---
title: Models and stages
description: Virtual users or a fixed request rate, stages that ramp the load over time, think time, request weights and the connection options.
sidebar:
  order: 2
---

A load test's shape is set by two things: the **load model** (how load is generated) and the **stages** (how much load, over time). This page also covers think time, weights and the connection options.

## The two models

| | Virtual users | Request rate |
|---|---|---|
| In the app | **Virtual users** (badge `VU`) | **Request rate** (badge `RPS`) |
| In YAML | `model: virtualUsers` (the default) | `model: arrivalRate` |
| Also called | Closed model | Open model, arrival rate |
| Stage target | Number of users | Requests started per second |
| A slower server gets | Fewer requests (users wait for answers) | The same number of requests (they pile up in flight) |
| Latency counts from | When the request is sent | When the request was **scheduled** to start |
| Limit | 5,000 users | 50,000 requests per second |
| Extra option | Think time | Max in flight |

### Virtual users (closed model)

Each virtual user loops: send a request, wait for the whole answer, pause for the [think time](#think-time), send the next one. Throughput therefore depends on the server: roughly *users ÷ (latency + think time)*. Ten users against a server that answers in 50 ms, with no think time, send about 200 requests per second; if the server slows to 500 ms, the same ten users send about 20.

Use it to model a known number of concurrent clients: a mobile app's active sessions, a pool of workers, a batch job with a fixed parallelism.

How users follow the stages:

- Every 50 ms the generator compares the running users with the stage target at that moment (rounded to a whole user) and starts or retires users to match.
- Users are numbered in the order they start (0, 1, 2 …). The number picks their [data file row](../data-and-captures/#data-files) and stays with them.
- A retired user finishes the request it is sending, then stops (it doesn't wait out its think time).
- When users are added again after a ramp down, they are new users with new numbers.

### Request rate (open model)

Requests start on a schedule, whatever the server does. If the server slows down, requests stay in flight longer and more of them overlap; the rate does not drop.

Use it to model traffic that doesn't wait for you: public web traffic, webhooks, a queue consumer at a fixed rate. It is also the honest way to measure latency under overload: because latency counts from each request's *scheduled* start, a request that starts late (the generator or the server fell behind) still counts the time it waited. A closed model can hide that queueing ("coordinated omission").

How the schedule works:

- The target is a rate, in requests per second, that the stages ramp linearly. Request number *k* (from 0) starts when the area under the rate curve reaches *k* + ½. A constant 200 req/s for 3 seconds gives exactly 600 evenly spaced requests; a ramp speeds up smoothly.
- Every request is its own iteration: it takes the [next data file row](../data-and-captures/#data-files), and captured values are not passed to other requests.
- **Max in flight** (`maxInFlight`, default 1,000, 1 to 100,000) caps the requests running at once. A request due while the cap is reached is **dropped**: not started, not queued, and counted as *dropped* (not as a request or an error). Many drops mean the server can't keep up with the rate, or the cap is too low.

:::caution
Dropped requests don't raise the error rate. When a request-rate test drops requests, the `errorRate` threshold can still pass; add an `rps` threshold, or watch the **Requests** tile, which shows the dropped count.
:::

## Stages

Stages describe the load over time. Each stage has a **duration** (seconds) and a **target** (users, or requests per second). The load moves **linearly** from the previous stage's target to this stage's target over the stage's duration:

- The first stage ramps from 0.
- A stage with the same target as the one before holds the load.
- A **0-second** stage jumps straight to its target.
- The run's planned length is the sum of the durations. After the last stage the run ends: no new requests start, and requests in flight get a short grace period (see [Stopping a run](../overview/#stopping-a-run)).

```yaml title="loadtests/Checkout smoke.yaml (stages only)"
stages:
  - { durationSecs: 30, target: 50 }    # ramp up: 0 → 50 over 30 s
  - { durationSecs: 120, target: 50 }   # hold 50 for 2 minutes
  - { durationSecs: 30, target: 0 }     # ramp down to 0
```

More shapes:

```yaml
# Constant: jump to 100 at once, hold for 60 s
stages:
  - { durationSecs: 0, target: 100 }
  - { durationSecs: 60, target: 100 }
```

```yaml
# Steps: find where the server starts to struggle
stages:
  - { durationSecs: 0, target: 50 }
  - { durationSecs: 60, target: 50 }
  - { durationSecs: 0, target: 100 }
  - { durationSecs: 60, target: 100 }
  - { durationSecs: 0, target: 200 }
  - { durationSecs: 60, target: 200 }
```

```yaml
# A pause: nothing for 10 s between two bursts (request rate)
model: arrivalRate
stages:
  - { durationSecs: 0, target: 100 }
  - { durationSecs: 20, target: 100 }
  - { durationSecs: 0, target: 0 }
  - { durationSecs: 10, target: 0 }
  - { durationSecs: 0, target: 100 }
  - { durationSecs: 20, target: 100 }
```

### The stage editor

The **Stages** section shows a preview of the shape (with a marker at the current second while the test runs), three presets, and the stage list.

| Control | What it does |
|---|---|
| **Constant** | A 0-second jump to the peak, then the whole duration at the peak. |
| **Ramp up, hold, ramp down** | Ramp up over a sixth of the duration, hold, ramp down over a sixth. Durations under 3 s become a single ramp. |
| **Spike** | A tenth of the peak (at least 1) for 40 % of the time, a short climb (5 %) to the peak, a hold (10 %), a short fall (5 %), then a tenth again for the rest. Durations under 6 s become Constant. |
| **Peak** | Scales every stage's target so the highest one is this value. A stage above 0 stays above 0. |
| **Duration (seconds)** | Stretches or shrinks every stage so they add up to this value. 0-second stages stay instant. |
| Stage rows | Duration and target of each stage, with a label: *jump*, *hold*, *ramp up* or *ramp down*. Move stages up or down, add or remove them. |

When the stages match a preset, its button is highlighted; otherwise the editor says "Custom shape". A new stage copies the last stage's target and lasts 10 s.

## Think time

`thinkTimeMs` (virtual users only) is the pause each user takes after an answer, before its next request. 0 (the default) sends the next request at once. Real users read and click; adding think time makes a given number of users send fewer requests, closer to what the same number of real clients would do.

The request-rate model has no think time: its rate already says when requests start.

## Requests and weights

`targets` lists the saved requests the test sends. Each has a **weight** (default 1) that sets how often it is picked relative to the others: with weights 3 and 1, the first request gets 75 % of the requests and the second 25 %. The **Share** column in the app shows each one's percent.

Picks are interleaved, not batched (smooth weighted round-robin): weights 3:1 give the order `A A B A`, then again, not `A A A B`. The order is computed once, so picking costs nothing during the run. Very large weight sums are scaled down to a cycle of at most 10,000 picks, keeping every weight above 0 at least once.

- A target with **weight 0** or unticked (`enabled: false`) is not sent, and its request file is not read.
- Without [captures](../data-and-captures/#captures), all users share one order: the next pick goes to whichever user asks next.
- With captures on any sent request, **each virtual user walks the order on its own**, from the start. That keeps a "create" before the "get" that uses its id, for every user.

```yaml
targets:
  - request: Products/List products.yaml
    weight: 8
  - request: Products/Get product.yaml
    weight: 3
  - request: Cart/Add to cart.yaml     # 1 of every 12 requests
  - request: Admin/Reindex.yaml
    enabled: false                     # kept in the file, not sent
```

Request paths are relative to the workspace's `requests/` folder. When you rename or move a request in Zorvik, load tests that send it (and thresholds that name it) are updated to the new path.

## Options

| Option (app) | YAML | Default | What it does |
|---|---|---|---|
| Think time (ms) | `thinkTimeMs` | 0 | Virtual users: pause after each answer, per user. |
| Max in flight | `maxInFlight` | 1000 | Request rate: most requests running at once; more are dropped. 1 to 100,000. |
| Timeout (ms) | `timeoutMs` | the app's request timeout (60,000 ms unless changed) | Per request. 0 means no limit. A timed-out request is a `timeout` error, with the time waited as its latency. |
| HTTP version | `httpVersion` | the app setting | `auto` (HTTP/2 when the server offers it over TLS, else HTTP/1.1), `http1` or `http2` (HTTP/2 only). HTTP/3 is not supported. |
| Reuse connections (keep-alive) | `keepAlive` | true | On: connections are pooled like a real client's. Off: a new connection for every request, which measures DNS, TCP and TLS each time. |

:::caution[Keep-alive off]
Without keep-alive every request opens a new connection and leaves a local port in TIME_WAIT for a while. At high rates you can run out of local ports, especially on Windows (about 16,000 ephemeral ports by default). Use it for short tests of connection setup, not for throughput.
:::

TLS verification, proxy, extra CA and client certificates come from the app settings (Settings → Requests, Proxy and Certificates). In [`zorvik load`](../../cli/load/) they come from the command's defaults and flags (`-k` to skip TLS verification), not from the app.

## Choosing numbers

- Start small: a short constant run at a low target shows whether the requests work at all (look at the status codes and errors) before you push.
- Ramp up rather than jump when you look for a limit: the charts show at which load latency starts to climb.
- Keep an eye on **Generator CPU**. Above 85 % of the computer, Zorvik warns that the laptop may be the bottleneck.
- A threshold on `rps` or latency is checked over the whole run, ramps included (see [Thresholds](../thresholds/)).

## Limits

| What | Limit |
|---|---|
| Virtual users (stage target) | 5,000 |
| Requests per second (stage target) | 50,000 |
| `maxInFlight` | 1 to 100,000 |
| Stage duration in the app's editor | 86,400 s per stage |
| Think time and timeout in the app's editor | 3,600,000 ms |
| Weight in the app's editor | 0 to 1,000 |

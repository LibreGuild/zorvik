---
id: latency-percentiles
title: Latency, throughput and percentiles
summary: Speed has three numbers worth knowing, and the slowest answers matter more than the average.
minutes: 6
quiz:
  - question: Ten requests take 12, 13, 13, 14, 14, 15, 15, 16, 18 and 910 ms. Which number best describes what a typical user felt?
    options:
      - The average, 104 ms
      - The median (p50), about 14 ms
      - The maximum, 910 ms
    answer: 1
    explain: Half the requests were faster than the median and half slower, so it describes the typical request. The average is pulled up by the one slow request, and nobody actually waited 104 ms.
  - question: What does "p95 is 300 ms" mean?
    options:
      - Every request took exactly 300 ms
      - The server handled 95 requests in 300 ms
      - 95 % of the requests took 300 ms or less; the slowest 5 % took longer
    answer: 2
    explain: A percentile cuts the sorted list of times. p95 is the time 95 % of requests stayed under.
  - question: A screen makes 20 API calls before it's ready. Why look at p95, not only the average?
    options:
      - With 20 calls, most screen loads include at least one of the slow 5 %
      - The average is always calculated wrong
      - p95 is easier to compute
    answer: 0
    explain: The chance that all 20 calls are faster than p95 is 0.95 multiplied by itself 20 times, about 36 %. So about two screen loads in three wait for at least one slow call.
---

"Is the API fast?" sounds like a yes-or-no question. It isn't. To answer it honestly you need three numbers, and one of them is sneakier than it looks.

## Latency: how long one answer takes

**Latency** is the time from sending a request until the answer has arrived. It's measured in **milliseconds** (ms): 1000 ms is one second. Zorvik shows it next to every response. Several things add up to it:

```flow
DNS lookup -> Connect -> TLS handshake -> Server works -> Download
```

On your own computer the network parts are almost free, so you mostly see the server's work. Across the internet, the first three steps can take longer than the work itself.

## Throughput and errors

- **Throughput** is how many requests finish per second, written **req/s**. It tells you how much work the server gets done.
- **Error rate** is the share of requests that failed, in percent: network errors plus answers with a status of 400 or higher.

A fast server that fails 10 % of the time is not fast. Always read the three numbers together.

## Why the average lies

Here are ten response times, sorted from fast to slow:

| # | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 |
|---|---|---|---|---|---|---|---|---|---|---|
| ms | 12 | 13 | 13 | 14 | 14 | 15 | 15 | 16 | 18 | 910 |

The **average** is 104 ms. But nobody waited 104 ms: nine people waited about 15 ms, and one waited almost a second. The average describes a request that never happened.

**Percentiles** describe what really happened. Sort all the times, then:

- **p50**, the *median*: half the requests were faster than this. Here, about 14 ms.
- **p95**: 95 % were faster, and the slowest 5 % took longer.
- **p99**: 99 % were faster; one request in a hundred was slower.

The slow end of the list is called the **tail**, and the tail is where users get annoyed.

> [!note] Think of it like…
> A bus line that says "on average, a bus every 10 minutes". If most buses come every 5 minutes but one in twenty is an hour late, the average still looks fine, and you're the one standing in the rain. p95 and p99 tell you about the rain.

## Why the tail matters more than you think

One screen of an app often makes many calls. If a screen needs 20 answers, the chance that *all 20* are faster than the p95 is only about 36 %. So on roughly two loads out of three, the user waits for at least one slow answer. And your most active users, who make the most requests, meet the tail most often.

That's why good performance goals are written with percentiles:

```text
p95 latency   < 300 ms
p99 latency   < 1000 ms
error rate    < 1 %
```

> [!tip] Tiny numbers, big differences
> On a laptop, calls to `127.0.0.1` take a millisecond or two. Don't compare those with numbers from a server across the world; compare runs of the same test on the same setup.

**You'll use this when…** someone says "the API is fast, the average is 104 ms!" and you calmly ask, "Great, and what's the p95?"

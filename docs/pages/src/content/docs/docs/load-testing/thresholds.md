---
title: Thresholds
description: Pass or fail a load test on latency percentiles, error rate or throughput, for the whole test or one request, stop a run early when a threshold keeps failing, and the exit codes of zorvik load.
sidebar:
  order: 3
---

A threshold is a rule such as `p95 < 300 ms` or `errorRate < 1 %`. A run **passes** when every enabled threshold holds at the end; one that doesn't hold **fails** the run. In CI, [`zorvik load`](../../cli/load/) turns that into its exit code.

A new load test starts with two thresholds: `p95 < 500 ms` and `errorRate < 1 %`. A test without thresholds always passes when it completes ("Passed (no thresholds)").

## Fields

```yaml title="loadtests/Checkout smoke.yaml (thresholds only)"
thresholds:
  - { metric: p95, op: "<", value: 300 }
  - { metric: p99, op: "<", value: 1000 }
  - { metric: errorRate, op: "<", value: 1 }
  - { metric: rps, op: ">=", value: 200 }
  - metric: p95
    op: "<"
    value: 150
    target: Products/Get product.yaml   # only this request
  - metric: max
    op: "<"
    value: 5000
    enabled: false                       # kept, not checked
  - metric: errorRate
    op: "<"
    value: 5
    abortAfterSecs: 10                   # stop the run after 10 s of failing
```

| Field | Required | Default | Meaning |
|---|---|---|---|
| `metric` | yes | | What is measured (table below). |
| `op` | yes | | `<`, `<=`, `>` or `>=`. Quote it in YAML (`"<"`). The app shows `≤` and `≥`. |
| `value` | yes | | The limit, in the metric's unit. Decimals are allowed. |
| `target` | no | the whole test | A request path (relative to `requests/`): check only that request's numbers. |
| `enabled` | no | `true` | `false` keeps the threshold in the file but doesn't check it. |
| `abortAfterSecs` | no | none (run to the end) | Stop the run early when this threshold has failed for this many seconds in a row. See [Stopping a run early](#stopping-a-run-early). |

## Metrics

| `metric` | In the app | Unit | Value |
|---|---|---|---|
| `p50` | p50 latency | ms | Median latency: half the requests were faster. |
| `p90` | p90 latency | ms | 90th percentile. |
| `p95` | p95 latency | ms | 95th percentile. |
| `p99` | p99 latency | ms | 99th percentile. |
| `p999` | p99.9 latency | ms | 99.9th percentile. |
| `avg` | Average latency | ms | Mean latency. |
| `max` | Max latency | ms | The slowest request. |
| `errorRate` | Error rate | % | Failed requests (network errors and HTTP status 400 or higher) and dropped ones, as a percent of all requests (completed and dropped), from 0 to 100. `1` means 1 %, not 100 %. |
| `rps` | Throughput | req/s | Completed requests divided by the time requests were being started (the run up to its planned end, or up to a stop). |

Latency is measured as described in the [overview](../overview/#what-it-measures): from send (virtual users) or from the scheduled start (request rate) to the last byte of the response, including timed-out and cut-off requests with the time they waited. Percentiles come from an HdrHistogram with 3 significant digits and are reported in milliseconds with microsecond precision.

Things worth knowing about the numbers:

- **Everything counts**, including the ramp-up and ramp-down stages. An `rps` threshold compares against the average over the whole run, not the peak. The short wait for requests still in flight after the planned end is left out of `rps`. A [warm-up](../models-and-stages/#warm-up) leaves the first seconds out of every threshold.
- **Dropped requests** (request-rate model, over `maxInFlight`) count as failed in `errorRate`: the system couldn't take the load. They have no latency, so they don't change the percentiles. A request whose every run was dropped fails its latency thresholds with "no data".
- **A 3xx** is an answer, not an error (redirects are not followed).
- **Capture misses** are not errors.

## Operators

| `op` | Holds when |
|---|---|
| `<` | actual < value |
| `<=` | actual ≤ value |
| `>` | actual > value |
| `>=` | actual ≥ value |

Use `<` or `<=` for latency and error rate (lower is better) and `>` or `>=` for `rps` (higher is better). In the app, switching a threshold to a metric with another unit resets it to a sensible default: `< 500` for latency, `< 1` for error rate, `>= 100` for throughput.

## One request or the whole test

Without `target`, a threshold checks the totals of every request. With `target`, it checks only that request's numbers, and its label ends with the request's name: `p95 < 150 ms · Get product`.

The target must be a request the test **sends** (enabled, weight above 0). A threshold on a request that isn't sent gets no data and fails; the app refuses to start such a test ("A threshold checks a request this test doesn't send") and marks the threshold. When a request is renamed or moved in Zorvik, thresholds that name it follow.

## When thresholds are checked

- **Live**: every snapshot (about four times a second) shows each threshold's current value and whether it holds. The panel says "Checked live, final at the end". A threshold failing mid-run does not stop the run, unless you ask it to (next section).
- **At the end**: the thresholds are checked once more over the whole run, and that result is the verdict saved in the history and reported by `zorvik load`.

A threshold with **no data** fails at the end: no completed requests (so no error rate or throughput), or no latency at all (for example, every request failed to connect). While the run is live, such a threshold shows "no data yet" instead of failing.

A run **stopped early** with **Stop** is still judged on what it measured. A run stopped by a threshold fails. A run that ends with an **error** (the load generator could not start or stopped because of an internal error) fails whatever its thresholds say.

## Stopping a run early

A load test that has clearly failed doesn't need to run to the end: a system that is down keeps getting load, and a long CI job keeps running for a result you already know. Give a threshold **Stop early** (`abortAfterSecs`) and the run stops once that threshold has failed for that many seconds **in a row**:

1. In a threshold's row, click **Stop early**. It turns into **Stop after [10] s failing**; change the seconds as you like (0 stops at the first failing check). Click **Stop after** again to run to the end instead.
2. While the run is live, the threshold is checked about four times a second, on the numbers so far (the same ones the panel shows). When it passes again, its count starts over.
3. Once it has failed for long enough, the run stops like **Stop**: no new requests start, the ones in flight get a short grace period. The banner says "Stopping: *errorRate < 5 %* failed for longer than it allows."
4. The run **fails**: the verdict card shows **FAILED**, **Stopped by a threshold** and "Stopped early: *label* failed for longer than it allows", the threshold row is marked **Stopped the run**, and the notification says the run was stopped by a threshold. In the JSON summary, `stoppedEarly` is `true` and `stoppedByThreshold` holds the threshold's label.

Things worth knowing:

- A threshold without data (no completed requests yet, or during the [warm-up](../models-and-stages/#warm-up)) is not failing, so it can't stop the run.
- The check uses the numbers **since the start** of the run (after the warm-up): one bad second after a long good stretch moves the error rate only a little. Use a lower `value` or a shorter wait to react faster.
- **Throughput** is low while the load ramps up. Don't stop on an `rps` threshold, or give it a wait longer than the ramp (or a warm-up that covers the ramp).
- Only a running test is stopped this way: during the wait for the last answers after the planned end, the thresholds decide the verdict as usual.
- With [`zorvik load`](../../cli/load/), a run stopped by a threshold prints `Stopped by the threshold … : it failed for longer than it allows` and exits with code `1`.

```yaml title="Stop when the error rate stays above 5 % for 10 s, or p99 above 2 s for 30 s"
thresholds:
  - { metric: errorRate, op: "<", value: 5, abortAfterSecs: 10 }
  - { metric: p99, op: "<", value: 2000, abortAfterSecs: 30 }
  - { metric: p95, op: "<", value: 300 }   # checked at the end only
```

### Verdicts

| Verdict | When |
|---|---|
| **PASSED** | No error and every enabled threshold holds (or there are none). |
| **FAILED** | No error, and at least one enabled threshold doesn't hold, or a threshold [stopped the run](#stopping-a-run-early). The app shows "*n* of *m* thresholds failed" and the failing labels. |
| **ERROR** | The run could not continue; the message says why. |

The verdict appears at the top of the results, in the run history, in the notification when the run ends, and in [reports](../results-and-reports/#export-html-json-and-csv).

## Exit codes of `zorvik load`

`zorvik load <workspace> <test>` runs a saved test in a terminal or CI and exits with:

| Code | Meaning |
|---|---|
| `0` | The run finished and every enabled threshold passed (or the test has none). |
| `1` | The run finished and at least one threshold failed, or a threshold stopped the run early. |
| `2` | The run could not produce a usable result: the workspace, load test or environment was not found, a `--var` was malformed, the plan was invalid (a missing request, a bad capture, a data file problem…), the load generator failed, a `--json` or `--html` file could not be written, or Ctrl+C was pressed twice. |

Pressing Ctrl+C once stops the run like **Stop** in the app: no new requests, a short wait for requests in flight, then the summary is printed and the exit code follows the thresholds (0 or 1). A second Ctrl+C quits at once with exit code 2.

```bash title="A CI step"
zorvik load ./api "Checkout smoke" --env Staging --var token="$API_TOKEN" \
  --quiet --html load-report.html --json load-summary.json
# exit code 0: passed, 1: a threshold failed, 2: the run itself failed
```

The command does not ask before loading outside hosts. See [`zorvik load`](../../cli/load/) for every option.

## Thresholds and AI agents

Agents create load tests with the `save_load_test` tool. It uses the same fields (`abortAfterSecs` too), tells the agent that `errorRate` is in percent (`1` = 1 %, not `0.01`), and refuses an `errorRate` value outside 0 to 100. See [Tools](../../agents/tools/#load-tests).

---
title: Results and reports
description: The run history of a load test, the timing phases and Server-Timing, comparing two runs, and HTML, JSON and CSV reports.
sidebar:
  order: 5
---

Every run of a load test ends with a summary: totals, per-request numbers, one chart point per second, the thresholds and a verdict. The app keeps the summaries of recent runs so you can look back, compare, and export them.

## Run history

When a run ends (it finished, was stopped, or failed with an error), its summary is saved in the app's data folder:

```text
<app data folder>/load-runs/<workspace key>/<load test id>/<run id>.json
```

The history belongs to your computer, not the workspace: it is not committed to Git and teammates don't see it. See [Data locations](../../reference/data-locations/).

- The **newest 30 runs** of each load test are kept; older ones are deleted as new ones arrive.
- Renaming a load test moves its history along, also while it runs.
- Deleting a load test deletes its history.
- The results header has a **runs** menu (for example "12 runs"). Each entry shows PASS, FAIL or ERROR, the date, "stopped" when it was stopped early, and its duration, req/s, p95 and error rate. Pick one to view it; **Back to the latest run** (or **Back to the live run**) returns.
- The trash icon deletes the run on screen from the history. This can't be undone.

With no run open, the results side shows the latest run of the test, from this session or from the history.

### The verdict card

A finished run starts with a card: **PASSED**, **FAILED** or **ERROR**, a line such as "1 of 3 thresholds failed" (or the error message), the start date, the duration, and **Stopped early** when it was stopped (**Stopped by a threshold** when a threshold [stopped it](../thresholds/#stopping-a-run-early), with the threshold named below). When the test has a [warm-up](../models-and-stages/#warm-up), the card says how long it was. See [Thresholds](../thresholds/#verdicts).

## Timing phases

Latency says how long a request took. The **Timing** panel says where that time went, for the whole test:

| Phase | From → to | Measured for |
|---|---|---|
| **Connect** | DNS, TCP, the proxy tunnel and TLS of a new connection | Only requests that opened a new connection. The panel shows "New connection for *x* % of requests". |
| **Time to first byte** | Request sent → first byte of the response | Every answered request. This is the server's time plus one network round trip. |
| **Transfer** | First byte → last byte of the response | Every answered request. |
| **Server-reported** | What the server says it spent, from its `Server-Timing` header | Only responses that had the header. The row is hidden when none had it. |

Each phase shows the number of requests measured, p50, p95, p99 and max in milliseconds (the HTML report adds the average). The per-request table adds **1st byte p95** for each request.

With keep-alive on, most requests reuse a connection, so **Connect** covers only the few that opened one. Turn keep-alive off to measure connection setup on every request (see [Options](../models-and-stages/#options)).

### Server-Timing

Time to first byte always includes the network. To see the server's own time, have it send a [`Server-Timing`](https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Server-Timing) header. Zorvik reads one number per response:

- the `dur` of the metric named `total` (any case), when there is one;
- otherwise the sum of the `dur` of every metric.

| Header | Server-reported time |
|---|---|
| `Server-Timing: total;dur=12.3` | 12.3 ms |
| `Server-Timing: db;dur=53, app;dur=47.2` | 100.2 ms |
| `Server-Timing: db;dur=5, total;dur=20, app;dur=7` | 20 ms |
| `Server-Timing: cache;desc="Hit, fast";dur=2.5, miss` | 2.5 ms |
| `Server-Timing: db;desc="no duration"` | none (not counted) |

Quoted values may contain commas and semicolons; `dur` may be quoted. Negative, non-numeric and missing durations are ignored.

When time to first byte is high but server-reported time is low, the time goes to the network, the proxy or queueing in front of the application.

## Comparing two runs

**Compare** in the results header lists the other runs of the same test (runs that ended with an error are left out). Pick one to put a comparison panel above the charts: the run on screen against the earlier one, row by row.

| Row | Better when |
|---|---|
| Requests / s | Higher |
| Error rate | Lower |
| p50, p90, p95, p99 latency, Max latency | Lower |
| p95 first byte | Lower |
| *Request* p95, one row per request (matched by request path) | Lower |

The change is shown in percent against the earlier run ("+12%", "−3.4%"), green when better and red when worse. Changes under 1 % are noise between runs and have no colour. "new" means the earlier run's value was 0, and "–" means one of the runs has no data for that row. **Stop comparing** in the same menu removes the panel.

Compare runs with the same settings: a different model, stage plan or data file changes the numbers for reasons that have nothing to do with the server.

## Export: HTML, JSON and CSV

**Export** in the results header saves the run on screen:

| Format | What you get |
|---|---|
| **HTML report…** | One self-contained page: inline styles, SVG charts and a small script for their tooltips, no external files. Open it in any browser, attach it to a pull request, or archive it as a CI artifact. |
| **JSON…** | The full summary as JSON, for your own tools. |
| **CSV, one row per second…** | The data of the charts, for a spreadsheet or a notebook. See [CSV files](#csv-files). |
| **CSV, one row per request…** | A row for all requests, then one per request, with status codes and network errors. |

The suggested file name is the test's name with the run's start date and time, for example `Checkout smoke 2026-09-28 14-05.html` or `Checkout smoke 2026-09-28 14-05 per second.csv`.

`zorvik load` writes the same files with `--html <file>`, `--json <file>`, `--csv-seconds <file>` and `--csv-requests <file>`:

```bash
zorvik load ./api "Checkout smoke" --html report.html --json summary.json --csv-requests requests.csv
```

### The HTML report

| Section | Contents |
|---|---|
| Header | Test name, "Load test · Started *2026-09-28 12:05:00 UTC* · ran *duration*", "stopped early" when stopped, and the verdict: ✓ Passed, ✓ Passed (no thresholds), ✗ Failed (*n* of *m* thresholds), ✗ Failed (stopped by a threshold) or ✗ Failed (error). A run stopped by a threshold names it below the header. |
| Tiles | Requests and req/s, error rate, p95 and p99 latency, p95 first byte, data in and out, connections, and when they apply: capture misses, dropped requests, peak generator CPU. With a warm-up, a line below says how long it was. |
| Thresholds | Each threshold, its actual value ("no data" when there was none) and ✓ Passed / ✗ Failed. |
| Over time | Charts of requests per second (completed and failed), latency (p50, p95, p99) and users or requests in flight. Point at a chart, tap it, or focus it with Tab and move with the arrow keys to see the time and every value at that moment. Runs longer than 600 seconds are shown in up to 600 columns: counts and p50 are averaged per column, p95 and p99 take the column's highest value. |
| Latency | min, avg, p50, p90, p95, p99, p99.9 and max of all requests. |
| Timing | The phases above, with a note when no response had `Server-Timing`. |
| Requests | Per request: requests, req/s, error rate, p50, p95, p99, max, p95 first byte, capture misses (when any), data in, and its status codes (the four most frequent, then "other") and network errors, for example `200 × 950, 503 × 12, timeout × 3`. |
| Status codes | Each status with its count and share. |
| Network errors | Each kind of network error with its count. |

### The JSON summary

Latencies are in milliseconds, times since the epoch in milliseconds, rates in requests per second.

```json title="summary.json (shortened)"
{
  "startedAt": 1790517900000,
  "durationMs": 60412,
  "totals": {
    "requests": 118230,
    "errors": 12,
    "errorRate": 0.0101,
    "rps": 1957.1,
    "bytesIn": 88712004,
    "bytesOut": 10403840,
    "latency": { "min": 1.2, "avg": 9.8, "p50": 8.1, "p90": 14.2, "p95": 18.9, "p99": 41.0, "p999": 120.3, "max": 311.7 },
    "statusCodes": [[200, 118218], [503, 12]],
    "errorKinds": [],
    "dropped": 0,
    "connections": 64,
    "timing": {
      "connect":  { "count": 64, "avg": 3.1, "p50": 2.9, "p95": 5.0, "p99": 6.2, "max": 7.0 },
      "ttfb":     { "count": 118230, "avg": 9.1, "p50": 7.6, "p95": 17.8, "p99": 39.2, "max": 310.9 },
      "transfer": { "count": 118230, "avg": 0.7, "p50": 0.4, "p95": 1.9, "p99": 3.3, "max": 12.0 },
      "server":   { "count": 0, "avg": 0, "p50": 0, "p95": 0, "p99": 0, "max": 0 }
    },
    "captureMisses": 0
  },
  "targets": [
    { "name": "List products", "request": "Products/List products.yaml", "metrics": { "requests": 88672, "errors": 0, "rps": 1467.8 } }
  ],
  "points": [
    { "second": 0, "rps": 180, "errors": 0, "p50": 7.9, "p95": 15.0, "p99": 22.4, "active": 2, "target": 2 }
  ],
  "thresholds": [
    { "label": "p95 < 300 ms", "metric": "p95", "op": "<", "value": 300, "target": null, "actual": 18.9, "passed": true }
  ],
  "passed": true,
  "stoppedEarly": false,
  "error": null,
  "peakCpuPercent": 142.0,
  "warmUpMs": 10000
}
```

| Field | Meaning |
|---|---|
| `startedAt` | Start of the run, Unix epoch milliseconds. |
| `durationMs` | How long the run really took, including the wait for requests in flight at the end. |
| `totals` | All requests together (fields below). |
| `targets[]` | The same numbers per request: `name`, `request` (path) and `metrics`. |
| `points[]` | One entry per full second: `second` (from 0), `rps` (requests completed in that second), `errors`, `p50`, `p95`, `p99`, `active` (users, or requests in flight, at the end of the second) and `target` (the stage target: users at the end of the second, or the average rate over the second). The last, partial second is in the totals but not in `points`. |
| `thresholds[]` | Each enabled threshold: `label`, `metric`, `op`, `value`, `target`, `actual` (`null` without data) and `passed`. |
| `passed` | Every threshold passed, there was no error, and no threshold stopped the run. |
| `stoppedEarly` | Stopped before the planned end. |
| `stoppedByThreshold` | The label of the threshold that [stopped the run](../thresholds/#stopping-a-run-early), such as `errorRate < 5 %`. Left out when none did. |
| `warmUpMs` | The [warm-up](../models-and-stages/#warm-up): requests that finished this early are in `points` but not in `totals`, `targets` or `thresholds`. Left out when there was none. |
| `error` | Why the run could not continue, or `null`. |
| `peakCpuPercent` | Highest CPU used by Zorvik, in percent of one core summed over cores (200 = two full cores), or `null` when not measured. |

`totals` and each target's `metrics`:

| Field | Meaning |
|---|---|
| `requests` | Completed requests (answered or failed). |
| `errors` | Network errors plus HTTP status ≥ 400. |
| `errorRate` | `errors` as a percent of `requests` (0 to 100). |
| `rps` | `requests` divided by the seconds requests were being started (after the warm-up, when there is one). |
| `bytesIn`, `bytesOut` | Bytes received and sent over the wire. |
| `latency` | `min`, `avg`, `p50`, `p90`, `p95`, `p99`, `p999`, `max` in ms (all 0 without data). |
| `statusCodes` | `[status, count]` pairs, most frequent first. |
| `errorKinds` | `[kind, count]` pairs for network errors, most frequent first (kinds below). |
| `dropped` | Request rate: requests not started because `maxInFlight` was reached. |
| `connections` | New connections opened. |
| `timing` | `connect`, `ttfb`, `transfer`, `server`: each `count`, `avg`, `p50`, `p95`, `p99`, `max` (`count` 0 means no data). |
| `captureMisses` | Captures that found nothing. |

### Network error kinds

| Kind | Meaning |
|---|---|
| `timeout` | The request timed out. |
| `connect` | Could not connect (refused, unreachable). |
| `dns` | The host name was not found. |
| `tls` | The TLS handshake failed (for example, an untrusted certificate). |
| `proxy` | The proxy refused or failed. |
| `protocol` | The server broke the HTTP protocol. |
| `io` | The connection broke while reading or writing. |
| `cancelled` | Cut off by a stop, or by the end of the run after the grace period. |
| `invalidRequest` | The request could not be built (for example, a rendered URL that isn't valid). |
| `notAllowed` | The request went to a host the run was not started for (a captured host), or one an AI agent was not allowed to reach. |
| `tooManyRedirects` | Listed for completeness; load tests don't follow redirects. |

## CSV files

Both CSV files have a header row, use commas and `.` for decimals, and quote text that holds a comma, quote or line break. Request names come from workspace files: a name that a spreadsheet would run as a formula (it starts with `=`, `+`, `-` or `@`) gets a leading `'`.

**One row per second** (`--csv-seconds`): one row for each full second of the run, the same data as the charts.

```csv
second,requests,errors,p50_ms,p95_ms,p99_ms,active,target,warm_up
0,180,0,7.9,15,22.4,2,2,true
1,410,1,8.2,16.1,25,4,4,false
```

| Column | Meaning |
|---|---|
| `second` | Seconds since the start (0 is the first second). |
| `requests`, `errors` | Requests completed, and failed, in that second. |
| `p50_ms`, `p95_ms`, `p99_ms` | Latency percentiles of that second. |
| `active` | Users (virtual users) or requests in flight (request rate) at the end of the second. |
| `target` | The stage target: users at the end of the second, or the average rate over the second. |
| `warm_up` | `true` for seconds in the [warm-up](../models-and-stages/#warm-up), which the totals leave out. |

**One row per request** (`--csv-requests`): `All requests` first (with an empty `path`), then each request.

| Columns | Meaning |
|---|---|
| `request`, `path` | The request's name and its path under `requests/`. |
| `requests`, `errors`, `error_rate_pct`, `dropped`, `rps` | As in the [JSON summary](#the-json-summary). |
| `min_ms` … `max_ms` | `min`, `avg`, `p50`, `p90`, `p95`, `p99`, `p999` and `max` latency; empty without data. |
| `first_byte_p95_ms`, `connect_p95_ms` | p95 of the [timing phases](#timing-phases); empty without data. |
| `bytes_in`, `bytes_out`, `connections`, `capture_misses` | As in the JSON summary. |
| `status_codes` | `status:count` pairs, most frequent first: `200:950 503:12`. |
| `network_errors` | `kind:count` pairs ([kinds](#network-error-kinds)): `timeout:3 connect:1`. |

## Charts over long runs

Every run keeps one chart point per second (86,400 for a day). While a run is live, the app keeps up to 7,200 points on screen and merges neighbouring seconds beyond that; the saved summary keeps every second.

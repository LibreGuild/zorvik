---
title: zorvik load
description: Run a saved load test from a terminal or CI, see live progress and a summary, save JSON and HTML reports, and fail the build when a threshold fails.
sidebar:
  order: 3
---

`zorvik load` runs a load test saved in the workspace (under `loadtests/`), prints a progress line every second and a summary at the end, and exits with code 1 when one of its thresholds fails. Use it to gate a deployment on "p95 under 300 ms" or "error rate under 1 %".

```text
zorvik load [OPTIONS] <WORKSPACE> <TEST>
```

```bash
zorvik load ./my-api "Checkout smoke" --env Staging --html report.html
```

Load tests are built and tuned in the app. See [Load testing](../../load-testing/overview/) for models, stages, thresholds, data files and captures.

## Arguments

| Argument | Description |
|---|---|
| `<WORKSPACE>` | The workspace folder (the one that contains `zorvik.yaml`) |
| `<TEST>` | The load test's name, or its id (its file name under `loadtests/` without `.yaml`), ignoring case |

## Options

| Option | Value | Default | Description |
|---|---|---|---|
| `-e`, `--env` | `<ENV>` | None | Environment to use, by name or id, ignoring case |
| `--var` | `<KEY=VALUE>` | | Set a variable with the highest precedence. Repeatable. |
| `--json` | `<FILE>` | None | Save the summary as JSON to this file |
| `--html` | `<FILE>` | None | Save an HTML report to this file |
| `-q`, `--quiet` | | Off | Print only the start line and the summary, no progress line every second |
| `-k`, `--insecure` | | Off | Don't verify TLS certificates |
| `--allow-outside-files` | | Off | Let requests send body files, and the data file be, outside the workspace folder |
| `-h`, `--help` | | | Print help |

### Variables

Each request is resolved with, from highest to lowest precedence:

1. `--var` values
2. Values the virtual user captured from earlier responses
3. The virtual user's data file row
4. The environment given with `--env`
5. Workspace variables

Secret values aren't in workspace files, so pass them with `--var` (see [Secrets](../overview/#secrets)). A request that uses a variable nobody defines is still sent, after a warning on standard error:

```text
warning: Get cart: undefined variables: cartId
```

### Data file

A load test's data file is read relative to the **workspace folder** (or from an absolute path). It must be inside the workspace folder unless you pass `--allow-outside-files`; otherwise the command stops with `… is outside the workspace folder …` (exit code 2).

### Settings used

`zorvik load` uses the load test's own **timeout** and **HTTP version**, and otherwise the command line defaults: TLS verification on (`-k` turns it off), the system's proxy settings and the operating system's trusted certificates. See [What the command line doesn't use](../overview/#what-the-command-line-doesnt-use).

## What it does

1. Finds the load test and reads it.
2. Resolves each enabled request with a weight above 0 once: variables, inherited headers and auth, and an OAuth 2.0 token when the request needs one. Requests that use dynamic variables (`{{$uuid}}`…), data file columns or captured values are rendered again for each send.
3. Checks the plan and starts the load generator.
4. Prints a progress line for every finished second, then the summary, and saves the reports you asked for.

Only HTTP requests can be load tested: another kind stops the command with `'X' is not an HTTP request: only HTTP requests can be load tested`. Pre-request and post-response scripts **don't run** during load tests; use captures to pass values between requests, and thresholds to decide pass or fail.

:::caution
The app asks before it load tests a host outside your computer and private networks. The command line doesn't ask: make sure you're allowed to send this load to the target.
:::

## Output

```text
Load test "Checkout smoke": up to 50 virtual users, 60 s, 2 requests
    1s       48.0 req/s  p95   12.40 ms  p99   20.10 ms  errors 0       users 10
    2s       97.0 req/s  p95   13.10 ms  p99   22.80 ms  errors 0       users 20
  …

"Checkout smoke" ran 60.0 s
  Requests   2,880 (48.0 req/s)
  Errors     3 (0.10 %)
  Latency    min 4.10 ms  avg 11.20 ms  p50 9.80 ms  p90 16.40 ms  p95 19.90 ms  p99 31.00 ms  p99.9 58.20 ms  max 102 ms
  First byte p50 8.10 ms  p95 17.30 ms  p99 28.60 ms  (request sent to first byte: server + network)
  Connect    p50 1.20 ms  p95 2.40 ms  p99 3.10 ms  (new connections, 1.74 % of requests)
  Status     200 × 2,877, 503 × 3
  Data       1.2 MB received, 310.4 KB sent, 50 connections
  CPU        23 % peak (100 % = one core)

  Thresholds
  ✓ p95 < 300 ms  (actual 19.90)
  ✗ errorRate < 0.05 %  (actual 0.10)

FAILED
```

- **The start line**: the load test's name, its peak (`up to N virtual users` or `up to N requests/s` for the arrival-rate model), its total duration and the number of requests.
- **A progress line per second** (not with `--quiet`): requests per second, p95 and p99 latency, errors, and the active virtual users (`users`) or requests in flight (`in flight`). When the arrival-rate model couldn't start requests because of its in-flight limit, `dropped so far N` is added.
- **The summary**: totals and latency percentiles. Lines appear only when they have data: `Dropped` (in-flight limit reached), `First byte`, `Connect`, `Server` (from the `Server-Timing` header), `Captures … missed`, `Status`, `Network` (network error kinds such as `timeout` or `connect`) and `CPU` (the load generator's own peak).
- **A table per request** when the test has more than one request, or when a capture missed: requests, requests per second, error rate, p95, p99 and first-byte p95 (and `Missed` captures).
- **Thresholds**, each with `✓` or `✗` and the actual value (`no data` when there was none), then `PASSED` or `FAILED`.

An `error:` line in the summary means the run couldn't continue.

## Reports

| Option | File |
|---|---|
| `--json <FILE>` | The summary as JSON: `startedAt`, `durationMs`, `totals` (requests, errors, `errorRate`, `rps`, latency percentiles, timing, status codes, bytes, connections, `captureMisses`), `targets` (the same per request), `points` (one per second), `thresholds` (`label`, `metric`, `op`, `value`, `target`, `actual`, `passed`), `passed`, `stoppedEarly`, `error`, `peakCpuPercent` |
| `--html <FILE>` | A single self-contained HTML page (no external files) with the verdict, key numbers, thresholds, charts over time and per-request tables, to open in a browser or keep as a CI artifact |

If a file can't be written, the command prints `error: could not save …` and exits with code 2. The summary is still printed.

## Stopping

- Press <kbd>Ctrl</kbd>+<kbd>C</kbd> once to stop early: no new requests start, and the requests in flight get a moment to finish (`Stopping (waiting for requests in flight; Ctrl+C again to quit now)…`). The summary is printed with `(stopped early)`, the reports are saved, and the exit code follows the thresholds.
- Press <kbd>Ctrl</kbd>+<kbd>C</kbd> again to quit at once: `Quit before the run finished.`, no summary, exit code 2.

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Every threshold passed (also when the test has no thresholds) |
| `1` | A threshold failed |
| `2` | The run couldn't start (load test, environment or request not found, a request that isn't HTTP, a data file problem, an invalid plan), the run broke off with an error, a report couldn't be saved, or Ctrl+C was pressed twice |

Plan errors are reported as they are in the app, for example `Add a stage with a duration`, `6000 virtual users is more than the limit of 5000`, or `HTTP/3 isn't supported for load tests yet`.

## Limits

| Limit | Value |
|---|---|
| Virtual users (virtual users model) | 5,000 |
| Requests per second (arrival-rate model) | 50,000 |
| Requests in flight (arrival-rate model) | 100,000 |

## Examples

```bash
# Smoke load before a deployment, quiet output, reports for the artifacts
zorvik load . "Checkout smoke" -e Staging --quiet --json load.json --html load.html

# A token from the CI's secrets
zorvik load . checkout-smoke -e Staging --var token="$API_TOKEN"

# A local server with a self-signed certificate
zorvik load . "Local soak" --var base=https://localhost:8443 -k
```

## CI examples

Install the command line as shown in the [zorvik run CI examples](../run/#ci-examples), then:

### GitHub Actions

```yaml title=".github/workflows/load.yml"
name: Load test
on: workflow_dispatch

jobs:
  load:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Install Zorvik
        run: |
          curl -fsSL -o zorvik.deb https://github.com/LibreGuild/zorvik/releases/latest/download/Zorvik-Linux-amd64.deb
          sudo apt-get update
          sudo apt-get install -y ./zorvik.deb
      - name: Load test
        run: zorvik load ./api "Checkout smoke" --env Staging --var "token=${{ secrets.API_TOKEN }}" --quiet --json load.json --html load.html
      - name: Keep the reports
        if: always()
        uses: actions/upload-artifact@v4
        with:
          name: load-test
          path: |
            load.json
            load.html
```

### GitLab CI

```yaml title=".gitlab-ci.yml"
load-test:
  image: ubuntu:24.04
  when: manual
  variables:
    DEBIAN_FRONTEND: noninteractive
  before_script:
    - apt-get update
    - apt-get install -y curl ca-certificates
    - curl -fsSL -o /tmp/zorvik.deb https://github.com/LibreGuild/zorvik/releases/latest/download/Zorvik-Linux-amd64.deb
    - apt-get install -y /tmp/zorvik.deb
  script:
    - zorvik load ./api "Checkout smoke" --env Staging --var "token=$API_TOKEN" --quiet --json load.json --html load.html
  artifacts:
    when: always
    paths:
      - load.json
      - load.html
```

A shared CI runner is itself a limited machine: its CPU and network can become the bottleneck before your API does. The `CPU` line of the summary shows how busy the load generator was.

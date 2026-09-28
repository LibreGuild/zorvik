---
title: Run reports
description: The text output, JSON report and JUnit XML report of collection runs, how skipped requests and tests are reported, and the exit codes of zorvik run.
sidebar:
  order: 5
---

A finished collection run can be saved in two formats:

| Format | From the app | From `zorvik run` | Use it for |
|---|---|---|---|
| JSON | Runner tab → **Export** → **JSON report…** | `--json` (printed to standard output) | Scripts, dashboards, your own checks with `jq` |
| JUnit XML | Runner tab → **Export** → **JUnit XML…** | `--junit <FILE>` | CI test reports (GitHub, GitLab, Jenkins, Azure DevOps and others) |

Both come from the same data, so a run exported from the app and the same run from the command line have the same shape. In the app, **Export** appears once the run has finished; the last 5 finished runs can be exported (older ones say `That run is no longer available; run it again`). The suggested file name is the run's name and time, such as `Users 2026-09-28 14-05.json`.

## Text output

Without `--json`, `zorvik run` prints one line per request as it finishes, then a summary:

```text
Iteration 1 of 2
✓ POST    Log in  https://api.example.com/login  200 84 ms, 312 B
    ✓ status is 200
✓ GET     Profile  https://api.example.com/me  200 41 ms, 1204 B
    ✓ token works
    ✓ not bob
– WS      Live prices  (skipped: WebSocket connections are live sessions; the runner sends HTTP, GraphQL and SSE requests)

Iteration 2 of 2
✓ POST    Log in  https://api.example.com/login  200 77 ms, 312 B
    ✓ status is 200
✗ GET     Profile  https://api.example.com/me  200 39 ms, 1204 B
    ✓ token works
    ✗ not bob — expected 'bob' to not equal 'bob'
– WS      Live prices  (skipped: WebSocket connections are live sessions; the runner sends HTTP, GraphQL and SSE requests)

Requests  3 passed, 1 failed, 2 skipped (6 total)
Tests     5 passed, 1 failed
Time      0.6 s, 2 iterations
```

- `✓` passed, `✗` failed, `–` skipped. Each line has the method, name, the URL as sent, then the status, time and body size, or the error when nothing came back.
- Tests are listed under their request. Script errors start with `!`, and so does the list of undefined variables (`! undefined variables: token`).
- `Iteration N of M` headers appear when the run has more than one iteration.
- The summary ends with `Stopped (Ctrl+C).` or `Stopped at the first failure (--bail).` when that happened, and the run's error if it had one.
- Console output from scripts is not printed; it's in the JSON report.

Colors are used when standard output is a terminal and the `NO_COLOR` environment variable is not set. Control characters in names, URLs and messages are shown escaped (for example `\u{1b}`), so a response can't take over your terminal.

## JSON report

The JSON report is an object with the run's `summary` and every request's `results`:

```json
{
  "summary": {
    "name": "Users",
    "environment": "Staging",
    "startedAt": 1790571915806.0,
    "durationMs": 1084.75,
    "iterations": 2,
    "requests": 4,
    "failed": 1,
    "skipped": 0,
    "testsPassed": 7,
    "testsFailed": 1,
    "testsSkipped": 0,
    "stopped": false,
    "bailed": false,
    "error": null,
    "passed": false,
    "perIteration": [
      { "iteration": 0, "requests": 2, "failed": 0, "skipped": 0, "testsPassed": 4, "testsFailed": 0, "durationMs": 540.2 },
      { "iteration": 1, "requests": 2, "failed": 1, "skipped": 0, "testsPassed": 3, "testsFailed": 1, "durationMs": 544.5 }
    ],
    "omitted": 0
  },
  "results": [
    {
      "iteration": 1,
      "path": "Users/Get profile.yaml",
      "name": "Get profile",
      "kind": "http",
      "method": "GET",
      "url": "https://api.example.com/me?key={{apiKey}}",
      "status": 200,
      "durationMs": 39.1,
      "size": 1204,
      "tests": [
        { "name": "token works", "passed": true, "skipped": false, "error": null },
        { "name": "not bob", "passed": false, "skipped": false, "error": "expected 'bob' to not equal 'bob'" }
      ],
      "console": [{ "level": "log", "message": "user bob" }],
      "error": null,
      "scriptErrors": [],
      "unresolved": [],
      "passed": false,
      "skipped": false
    }
  ]
}
```

### summary

| Field | Type | Description |
|---|---|---|
| `name` | string | The folder's name, or the workspace's for the whole collection |
| `environment` | string or null | The environment's name |
| `startedAt` | number | Start time, Unix epoch milliseconds |
| `durationMs` | number | Duration of the whole run |
| `iterations` | number | Iterations planned |
| `requests` | number | Requests sent or tried (skipped ones not counted) |
| `failed` | number | Requests that failed |
| `skipped` | number | Requests skipped (kinds the runner doesn't send) |
| `testsPassed`, `testsFailed`, `testsSkipped` | number | Test counts over the whole run |
| `stopped` | boolean | Stopped by the user (Stop, or Ctrl+C) before it finished |
| `bailed` | boolean | Stopped at the first failure (`--bail` / **Stop on the first failure**) |
| `error` | string or null | Why the run ended early otherwise, for example a `setNextRequest` loop |
| `passed` | boolean | No request failed and `error` is null |
| `perIteration` | array | For each iteration that ran: `iteration` (from 0), `requests`, `failed`, `skipped`, `testsPassed`, `testsFailed`, `durationMs` |
| `omitted` | number | Results left out of `results` (see [Limits](#limits)); they are still counted above |

### results

One entry per request per iteration, in the order they ran.

| Field | Type | Description |
|---|---|---|
| `iteration` | number | Iteration, from 0 |
| `path` | string | The request file under `requests/` |
| `name` | string | The request's name |
| `kind` | string | `http`, `sse`, `websocket`, `tcp`, `udp`, `dns`, `mqtt` or `grpc` |
| `method` | string | The method sent |
| `url` | string | The URL as sent, with secret values shown as `{{name}}`; the saved URL when nothing was sent |
| `status` | number or null | HTTP status, null when there was no response |
| `durationMs` | number or null | Time of the request (for "repeat until", from the first send to the end of the last) |
| `size` | number or null | Response body size in bytes |
| `tests` | array | `{ name, passed, skipped, error }` for each test, including "Matches the API spec" |
| `console` | array | `{ level, message }` lines from the scripts; `level` is `log`, `info`, `warn`, `error` or `debug` |
| `error` | string or null | Why there is no usable result: network or resolve error, pre-request script error, "repeat until" timeout, an unreadable request file |
| `scriptErrors` | string[] | Post-response scripts that failed, for example `Post-response script of request 'Get profile' failed at line 3: …` |
| `unresolved` | string[] | Variables the request used but that weren't defined |
| `passed` | boolean | Whether the request passed |
| `skipped` | boolean | Not sent because of its kind |
| `skipReason` | string | Only for skipped requests: why |
| `attempts` | number | Only for requests with "repeat until": how many times it was sent |

A few `jq` examples:

```bash
zorvik run ./api --json > run.json

jq '.summary.passed' run.json                                   # true or false
jq -r '.results[] | select(.passed | not) | .name' run.json     # names of failed requests
jq '[.results[].durationMs | select(. != null)] | add / length' run.json   # average request time
```

## JUnit XML report

The JUnit report follows the layout most CI tools read:

- One `<testsuites>` for the run, named after it, with the totals, the duration in seconds and the start time (`timestamp`, UTC).
- One `<testsuite>` per request, named after its path without `.yaml` (for example `Users/Get profile`), in the order the requests first ran.
- One `<testcase>` per test (per iteration), with `classname` set to the suite's name.
- One extra `<testcase>` for the request itself, named `METHOD Name` (for example `GET Get profile`), when the request was skipped, had an error, or had no tests (then it passes, or fails with `HTTP 404`).
- With more than one iteration, every test case name ends with ` (iteration N)` (N from 1).
- The request's time is on its first test case; the others have `0.000`, so sums stay right.

```xml
<?xml version="1.0" encoding="UTF-8"?>
<testsuites name="Users" tests="7" failures="2" errors="1" skipped="2" time="1.500" timestamp="2026-09-28T14:05:12">
  <testsuite name="Users/Get profile" tests="4" failures="1" errors="0" skipped="1" time="0.020">
    <testcase name="status is 200 (iteration 1)" classname="Users/Get profile" time="0.012"/>
    <testcase name="body (iteration 1)" classname="Users/Get profile" time="0.000">
      <failure message="expected &apos;a&apos; to equal &apos;b&apos;" type="AssertionError">expected &apos;a&apos; to equal &apos;b&apos;</failure>
    </testcase>
    <testcase name="later (iteration 1)" classname="Users/Get profile" time="0.000">
      <skipped/>
    </testcase>
    <testcase name="GET Get profile (iteration 2)" classname="Users/Get profile" time="0.008"/>
  </testsuite>
  <testsuite name="Health" tests="1" failures="0" errors="1" skipped="0" time="0.000">
    <testcase name="GET Health (iteration 1)" classname="Health" time="0.000">
      <error message="Could not connect to 127.0.0.1:8080: connection refused (is the server running?)" type="Error">Could not connect to 127.0.0.1:8080: connection refused (is the server running?)</error>
    </testcase>
  </testsuite>
  <testsuite name="Users/Missing" tests="1" failures="1" errors="0" skipped="0" time="0.000">
    <testcase name="GET Missing (iteration 2)" classname="Users/Missing" time="0.000">
      <failure message="HTTP 404" type="AssertionError">HTTP 404</failure>
    </testcase>
  </testsuite>
  <testsuite name="Live prices" tests="1" failures="0" errors="0" skipped="1" time="0.000">
    <testcase name="Live prices (iteration 2)" classname="Live prices" time="0.000">
      <skipped message="WebSocket connections are live sessions; the runner sends HTTP, GraphQL and SSE requests"/>
    </testcase>
  </testsuite>
</testsuites>
```

How outcomes map:

| Outcome | JUnit element |
|---|---|
| A test passed | `<testcase …/>` |
| A test failed | `<failure type="AssertionError">` with the test's error (`failed` when it has none) |
| A test was skipped (`pm.test.skip`, or `pm.test` without a function) | `<skipped/>` |
| The request was skipped | A request test case with `<skipped message="reason"/>` |
| The request couldn't be sent, a script failed, or "repeat until" timed out | A request test case with `<error type="Error">`; all messages, one per line |
| No tests, and the status failed the request | A request test case with `<failure>` `HTTP <status>` |
| No tests, and it passed | A passing request test case |

Special characters are escaped for XML; control characters that XML can't hold are written as `\u{…}`.

## Skip reasons

A request is **skipped** when it's of a kind the runner doesn't send. It counts as neither passed nor failed. The reason is shown in the text output, in `skipReason` in the JSON report, and in the JUnit `<skipped message>`:

| Request kind | Skip reason |
|---|---|
| WebSocket | `WebSocket connections are live sessions; the runner sends HTTP, GraphQL and SSE requests` |
| TCP | `TCP connections are live sessions; the runner sends HTTP, GraphQL and SSE requests` |
| UDP | `UDP sockets are live sessions; the runner sends HTTP, GraphQL and SSE requests` |
| MQTT | `MQTT clients are live sessions; the runner sends HTTP, GraphQL and SSE requests` |
| gRPC | `gRPC calls don't run in the collection runner; the runner sends HTTP, GraphQL and SSE requests` |
| DNS | `DNS queries don't run in the collection runner; the runner sends HTTP, GraphQL and SSE requests` |

A request file that can't be read is **not** skipped: it fails with `The request can't be read: …`.

A **test** is skipped when it's written with `pm.test.skip(name, fn)` or `pm.test(name)` without a function. Skipped tests are counted in `testsSkipped` and don't affect whether the request passes.

## Exit codes of zorvik run

| Code | Meaning |
|---|---|
| `0` | Every request passed (skipped requests don't count). Also when the run had no requests at all. |
| `1` | A request or test failed, `--bail` stopped the run, or the run ended with an error (such as a `setNextRequest` loop) |
| `2` | The run was stopped with Ctrl+C before it finished, or it couldn't start: a wrong option or value, the workspace, folder or environment not found, a data file problem; also when the JUnit file couldn't be written |

:::caution
A folder with no requests in it runs nothing and exits with `0`. If that matters in your pipeline, check `summary.requests` in the JSON report.
:::

See [zorvik run](../../cli/run/) for the options, and its [CI examples](../../cli/run/#ci-examples).

## Secrets in reports

Reported URLs never show the values of **secret** variables (variables marked secret in the environment or the workspace):

- Every secret value of three characters or more is replaced by `{{name}}` in the URL, both as written and URL-encoded.
- This covers values given with `--var` for a secret variable, and values scripts set for it during the run (for example a token saved by a login request).

Only URLs are masked. Test names, test errors and console output are reported as the scripts wrote them, so keep secret values out of them. Reports don't contain request or response headers or bodies.

## Limits

| Limit | Value |
|---|---|
| Results in `results` | The first 50,000, then failed ones only, up to 100,000 in total. The rest are counted in `summary.omitted` and in all the other counts. |
| Console lines per result | 200 (then `N more console lines not kept.`) |
| Characters per console line | 4,096 (then `… (truncated)`) |

---
title: zorvik run
description: Run the requests of a workspace or folder with their scripts and tests from a terminal or CI, with environments, data files, JSON and JUnit reports.
sidebar:
  order: 2
---

`zorvik run` is the [collection runner](../../testing/collection-runner/) on the command line. It sends the requests of a workspace (or of one folder) in sidebar order, with their [scripts and tests](../../scripting/overview/), once per iteration or per data file row, and reports the results.

```text
zorvik run [OPTIONS] <WORKSPACE>
```

```bash
zorvik run ./my-api --env Staging
zorvik run ./my-api --folder Users -d users.csv --junit report.xml
```

## Arguments

| Argument | Description |
|---|---|
| `<WORKSPACE>` | The workspace folder (the one that contains `zorvik.yaml`) |

## Options

| Option | Value | Default | Description |
|---|---|---|---|
| `-e`, `--env` | `<ENV>` | None | Environment to use, by name or id (file name without `.yaml`), ignoring case |
| `-f`, `--folder` | `<FOLDER>` | The whole workspace | Only run requests under this folder: its path under `requests/`, for example `Users` or `Users/Admin` |
| `--var` | `<KEY=VALUE>` | | Set a variable with the highest precedence. Repeatable. |
| `-d`, `--data` | `<FILE>` | None | Data file: CSV with a header row, or a JSON array of objects. One iteration per row; columns are variables. |
| `-n`, `--iterations` | `<N>` | One per data row, or 1 | How many times to run the requests: 1 to 100,000 |
| `--delay` | `<MS>` | `0` | Pause before each request except the first, in milliseconds: 0 to 600,000 |
| `--allow-http-errors` | | Off | HTTP 4xx/5xx responses don't fail requests that have no tests |
| `--bail` | | Off | Stop at the first failed request |
| `--timeout` | `<TIMEOUT>` | `60000` | Request timeout in milliseconds; `0` means none |
| `-k`, `--insecure` | | Off | Don't verify TLS certificates |
| `--allow-outside-files` | | Off | Let requests send body files from outside the workspace folder |
| `--json` | | Off | Print a JSON report (summary and results) instead of text |
| `--junit` | `<FILE>` | None | Also save a JUnit XML report to this file |
| `-h`, `--help` | | | Print help |

### -e, --env

Uses an environment of the workspace for `{{variables}}` and makes it the environment scripts see (`pm.environment`, `pm.environment.name`). Without it, no environment is active: only workspace variables, data rows and `--var` values are defined, and values scripts set with `pm.environment.set` apply for the rest of the run only (with a console warning).

An unknown name stops with `error: environment 'x' not found` (exit code 2).

### -f, --folder

Runs only the requests under this folder, including its subfolders, in sidebar order. The path is the folder's path under `requests/`, as the directories are named in the workspace. Without it, the whole workspace runs. An unknown folder stops with `error: Folder 'x' not found`.

There's no option to pick single requests or change the order: every request under the folder runs, in sidebar order. Use a folder for each set you want to run, or `pm.execution.setNextRequest` to change the order ([details](../../testing/collection-runner/#changing-the-order-with-setnextrequest)).

### --var

```bash
zorvik run . --var token="$API_TOKEN" --var base=https://staging.example.com
```

`--var` values win over everything else: data rows, environments, workspace variables and values that scripts set (scripts can't change them). Use them for secrets, which workspace files never contain: see [Secrets](../overview/#secrets).

### -d, --data and -n, --iterations

```bash
zorvik run . --folder Signup --data ./fixtures/users.csv           # one iteration per row
zorvik run . --folder Signup --data ./fixtures/users.json -n 1     # only the first row
zorvik run . --folder Smoke -n 20                                  # 20 times, no data file
```

The data file path is relative to the current folder and may be anywhere. With more iterations than rows, the last row is used again. See [Data files](../../testing/data-files/) for the formats and limits. A problem with the file stops the command before anything is sent: `error: data file users.csv: …` (exit code 2).

### --delay

Waits this many milliseconds before each request that is sent, except the first one of the run (also between iterations). Skipped requests don't wait. It doesn't apply between the sends of a request with "repeat until", which uses its own interval.

### --allow-http-errors

By default a request **without tests** fails when its status is 400 or more. With this option such requests pass whatever their status. Requests with tests are always judged by their tests. See [When a request passes or fails](../../testing/collection-runner/#when-a-request-passes-or-fails).

### --bail

Stops the run right after the first failed request (a failed test fails its request). The summary says `Stopped at the first failure (--bail).`, `summary.bailed` is `true`, and the exit code is 1.

### --timeout

Sets the default timeout for the whole request (connecting, sending and receiving), in milliseconds; `0` means no timeout. A request that sets its own timeout in its **Settings** tab keeps it. Without this option the default is 60,000 ms.

### -k, --insecure

Skips TLS certificate verification for every request, as if **Verify TLS certificates** were off. A request whose own settings turn verification on keeps it on. Use it only against servers you trust, for example a local server with a self-signed certificate.

### --allow-outside-files

Requests with file bodies (binary bodies and multipart file fields) may only send files inside the workspace folder, so a shared workspace can't send your private files. This option allows files anywhere. It doesn't concern the data file, which can always be anywhere.

### --json

Prints one JSON document with the summary and every result when the run ends, instead of the text output. Nothing else is printed to standard output, so you can pipe it:

```bash
zorvik run . --env Staging --json > run.json
jq '.summary' run.json
```

The format is described in [Run reports](../../testing/reports/#json-report). It's the only output that includes the scripts' console lines.

### --junit

Saves a JUnit XML report to the file, in addition to the normal output (text or `--json`). If the file can't be written, the command prints `error: could not save …` and exits with 2. See [JUnit XML report](../../testing/reports/#junit-xml-report).

## What runs

- Every request under the workspace or folder, in sidebar order, once per iteration.
- HTTP and GraphQL requests are sent. [Server-Sent Events requests](../../testing/repeat-and-streams/#event-streams-in-runs) are read until their stop settings say to stop.
- WebSocket, TCP, UDP, MQTT, gRPC and DNS requests are listed as skipped, with the reason.
- For each request: pre-request scripts, send, post-response scripts, then "repeat until" and the OpenAPI contract check when they apply.
- Script values and cookies carry from one request to the next during the run, and are forgotten at the end.

`zorvik run` uses the default settings described in [What the command line doesn't use](../overview/#what-the-command-line-doesnt-use), not the app's settings.

## Output

The text output lists each request with its tests as it finishes, then a summary:

```text
✓ POST    Log in  https://staging.example.com/login  200 84 ms, 312 B
    ✓ status is 200
✗ GET     Profile  https://staging.example.com/me  401 39 ms, 58 B
    ✗ is logged in — expected response to have status code 200 but got 401

Requests  1 passed, 1 failed, 0 skipped (2 total)
Tests     1 passed, 1 failed
Time      0.2 s
```

See [Run reports](../../testing/reports/) for every line, the JSON report and the JUnit report.

## Stopping

Press <kbd>Ctrl</kbd>+<kbd>C</kbd> to stop. The request in flight is dropped; the summary of what ran is still printed (and the JSON and JUnit reports still written), ending with `Stopped (Ctrl+C).`, and the exit code is 2.

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Every request passed. Also when there was nothing to run. |
| `1` | A request or test failed, or the run ended with an error (such as a `setNextRequest` loop) |
| `2` | Stopped with Ctrl+C, or couldn't start: wrong options (`--iterations 0`, a `--var` without `=`), workspace, folder or environment not found, a data file problem, or the JUnit file couldn't be written |

## Examples

```bash
# Everything in the workspace against Staging
zorvik run . --env Staging

# One folder, secrets from the environment of the shell
zorvik run . -e Production -f Smoke --var apiKey="$API_KEY"

# Data-driven: one iteration per row, stop at the first failure
zorvik run . -f Signup -d ./fixtures/signups.csv --bail

# Slow down for a rate-limited API: 500 ms between requests
zorvik run . -e Staging --delay 500

# Self-signed certificate on a local server, 5 s timeout
zorvik run . --var base=https://localhost:8443 -k --timeout 5000

# JUnit for CI, JSON for your own checks
zorvik run . -e Staging --junit zorvik-junit.xml --json > zorvik.json
```

## CI examples

The command line is inside every Zorvik download. On Linux CI machines, install the `.deb` package from the latest release, which puts `zorvik` in `/usr/bin`. To pin a version, replace `latest/download` in the URL with `download/v0.1.1` (the release's tag).

Keep secrets in your CI's secret store and pass them with `--var`.

### GitHub Actions

```yaml title=".github/workflows/api-tests.yml"
name: API tests
on: [push, pull_request]

jobs:
  api-tests:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Zorvik
        run: |
          curl -fsSL -o zorvik.deb https://github.com/LibreGuild/zorvik/releases/latest/download/Zorvik-Linux-amd64.deb
          sudo apt-get update
          sudo apt-get install -y ./zorvik.deb

      - name: Run the API tests
        run: >
          zorvik run ./api
          --env Staging
          --var "apiKey=${{ secrets.API_KEY }}"
          --junit zorvik-junit.xml

      - name: Keep the JUnit report
        if: always()
        uses: actions/upload-artifact@v4
        with:
          name: zorvik-junit
          path: zorvik-junit.xml
```

`if: always()` keeps the report when the tests fail (the step before exits with 1). Point any JUnit-reading test reporter action at `zorvik-junit.xml` to see the results in the pull request.

On a Windows runner, the portable zip holds `zorvik.exe`:

```yaml
  api-tests-windows:
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v4
      - name: Install Zorvik
        shell: pwsh
        run: |
          Invoke-WebRequest https://github.com/LibreGuild/zorvik/releases/latest/download/Zorvik-Windows-Portable-x64.zip -OutFile zorvik.zip
          Expand-Archive zorvik.zip -DestinationPath "$env:RUNNER_TEMP\zorvik"
          Add-Content $env:GITHUB_PATH "$env:RUNNER_TEMP\zorvik"
      - name: Run the API tests
        run: zorvik run ./api --env Staging --junit zorvik-junit.xml
```

### GitLab CI

```yaml title=".gitlab-ci.yml"
api-tests:
  image: ubuntu:24.04
  variables:
    DEBIAN_FRONTEND: noninteractive
  before_script:
    - apt-get update
    - apt-get install -y curl ca-certificates
    - curl -fsSL -o /tmp/zorvik.deb https://github.com/LibreGuild/zorvik/releases/latest/download/Zorvik-Linux-amd64.deb
    - apt-get install -y /tmp/zorvik.deb
  script:
    - zorvik run ./api --env Staging --var "apiKey=$API_KEY" --junit zorvik-junit.xml
  artifacts:
    when: always
    reports:
      junit: zorvik-junit.xml
```

Define `API_KEY` as a masked CI/CD variable. GitLab shows the JUnit results in the merge request's test report.

### Against a mock server

Start a saved mock with [`zorvik serve`](../serve/) in the background, run the tests against it, then stop it:

```bash
zorvik serve ./api "Payments mock" --port 3100 > mock.log &
MOCK_PID=$!
until curl -s -o /dev/null http://127.0.0.1:3100/; do sleep 0.2; done   # wait until it answers
zorvik run ./api --folder Checkout --var base=http://127.0.0.1:3100 --junit zorvik-junit.xml
STATUS=$?
kill $MOCK_PID
exit $STATUS
```

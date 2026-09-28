---
title: Collection runner
description: Run the requests of a folder or the whole collection in order, with their scripts and tests, repeated or driven by a data file, in the app or in CI.
sidebar:
  order: 1
---

The collection runner sends the requests of a folder (or of the whole collection) one after the other, with all their [scripts and tests](../../scripting/overview/), and reports which ones passed. It can repeat the list several times, feed each pass with a row of a [data file](../data-files/), poll a request until a condition holds, and test [Server-Sent Events](../repeat-and-streams/#event-streams-in-runs).

The same runner is used in three places, so a folder behaves the same everywhere:

| Where | How |
|---|---|
| The app | A **Runner** tab (this page) |
| A terminal or CI | [`zorvik run`](../../cli/run/) |
| AI agents | The `run_collection` tool (see [AI agents](../../agents/setup/)); the run shows live in a Runner tab |

## Run a folder in the app

1. Open a Runner tab in one of these ways:
   - Right-click a folder in the sidebar → **Run…**
   - The **+** button at the top of the sidebar (or right-click empty space in the collection) → **Run…** for the whole collection
   - The command palette (<kbd>Ctrl</kbd>/<kbd>⌘</kbd> <kbd>K</kbd>) → **Run collection**
2. Choose the requests, iterations, data file and options on the left.
3. Press **Run** (or <kbd>Ctrl</kbd>/<kbd>⌘</kbd> <kbd>Enter</kbd>). Results appear on the right as they arrive.

Press **Stop** (or <kbd>Ctrl</kbd>/<kbd>⌘</kbd> <kbd>Enter</kbd> again) to stop. Only one collection run can be in progress at a time in the app.

## Runner settings

| Setting | What it does | Default |
|---|---|---|
| **Requests** | The HTTP and GraphQL requests under the folder (and its subfolders), in sidebar order; request files that can't be read aren't listed. Untick a request to leave it out (**All** / **None** tick or untick every one). Drag a row by its handle, or press <kbd>Alt</kbd> <kbd>↑</kbd>/<kbd>↓</kbd> on the handle, to change the order. | All, in sidebar order |
| **Iterations** | How many times the list runs. Empty: once per data file row, or once without a data file. | Empty |
| **Delay (ms)** | Pause before each request, except the first one of the run. 0 to 600,000 (10 minutes). | 0 |
| **Data file** | A CSV or JSON file; each row is one iteration. **Choose file…** picks it and shows a preview. See [Data files](../data-files/). | None |
| **Stop on the first failure** | End the run as soon as one request fails | Off |
| **Environment** | Shows the active environment (chosen in the title bar); **Environments…** opens the editor | The active environment |

Changes made while a run is in progress apply to the next run.

## Which requests are sent

| Request kind | In a run |
|---|---|
| HTTP, including GraphQL | Sent |
| Server-Sent Events (SSE) | Sent by `zorvik run` and by runs an AI agent starts: read until the request's stream settings say to stop. See [event streams in runs](../repeat-and-streams/#event-streams-in-runs). |
| GraphQL subscriptions | Read like SSE requests: until the request's stream settings say to stop. See [GraphQL subscriptions](../../protocols/graphql/#subscriptions). |
| WebSocket, Socket.IO, TCP, UDP, MQTT | Skipped: they are live sessions |
| gRPC, DNS | Skipped: they don't run in the collection runner |

:::note
The Runner tab in the app, `zorvik run` and runs an AI agent starts all send HTTP, GraphQL and SSE requests.
:::

Skipped requests are listed in `zorvik run` output and reports with the reason (see [skip reasons](../reports/#skip-reasons)). They don't count as passed or failed, and they don't wait for the delay.

## What happens for each request

For each iteration, the runner goes through the list in order. For each request it:

1. Waits for the **delay**, unless this is the first request the run sends.
2. Runs the pre-request scripts (workspace, folders, request).
3. Resolves `{{variables}}` (with this iteration's data row) and sends the request. An SSE request is read until its stop condition.
4. Runs the post-response scripts.
5. If the request has **Repeat until** turned on, checks the condition and, if it doesn't hold yet, waits and goes back to step 2. See [Repeat until](../repeat-and-streams/).
6. For a request imported from an OpenAPI document, checks the response against it ("Matches the API spec").
7. Records the result, then moves on to the next request, or to the one `setNextRequest` named.

## When a request passes or fails

| Outcome | Result |
|---|---|
| The request is of a kind the runner skips | Skipped (counts as neither) |
| The request file can't be read | **Fails**: `The request can't be read: …` |
| The request can't be sent: network error, timeout, an undefined variable in the URL, a pre-request script error | **Fails** with that error |
| A post-response script throws | **Fails** (the error is listed with the result) |
| "Repeat until" didn't hold before its time limit | **Fails**: `Repeat until: … didn't hold after N sends in X s` |
| The request has tests (not counting skipped ones) | Passes when **all** its tests pass, whatever the status code |
| The request has no tests | Passes when the status is below 400 |

Tests include those added by workspace and folder scripts and the automatic "Matches the API spec" check. So a request with only a skipped test is judged by its status, and a `404` passes if all of its tests pass.

With `zorvik run --allow-http-errors`, a request without tests passes whatever its status.

### When the run passes

A run passes when no request failed and it wasn't cut short by an error (such as a `setNextRequest` loop). A run you stop yourself is marked as stopped.

## Variables during a run

- The run uses the **active environment** in the app, or the environment given with `--env` in `zorvik run`.
- Each iteration gets its data file row, which wins over the environment, workspace and global variables. See [Data files](../data-files/#using-the-values).
- Values set with `pm.variables.set` last for the rest of the run, **across iterations**. Reset them in a script if each iteration must start clean.
- Values scripts set with `pm.environment.set`, `pm.collectionVariables.set` and `pm.globals.set` apply to the requests after them. In the app they are also kept on this computer as they happen, as with single sends. `zorvik run` keeps nothing after the run.

See [Variables that scripts set](../../scripting/overview/#variables-that-scripts-set).

## Changing the order with setNextRequest

By default the requests run in list order. A script can pick the next request:

```js title="Post-response script"
pm.execution.setNextRequest("Get order");   // by name
pm.execution.setNextRequest("Orders/get-order.yaml");   // or by path under requests/
pm.execution.setNextRequest(null);          // end this iteration
```

- The runner looks for the first request **in this run** with that name, then for a request with that path (`pm.info.requestId`).
- After the named request, the run continues in list order from there.
- A name that matches no request of the run ends the iteration, with a warning in the result's console: `setNextRequest: no request named 'X' in this run; the iteration ends here.`
- An iteration can send at most 10,000 requests. Past that, the run stops with the error `Stopped: iteration N sent more than 10000 requests (a setNextRequest loop?)` and fails.
- `postman.setNextRequest` does the same. A single send (not in a run) ignores it.

## Stopping a run

- **Stop** in the app, or <kbd>Ctrl</kbd>+<kbd>C</kbd> for `zorvik run`, stops at once. The request in flight is not reported; everything before it is.
- **Stop on the first failure** (`--bail`) ends the run right after the first failed request.

## Results in the app

The results side shows:

- **A summary line**: **PASS** or **FAIL** (or **STOPPED**), then passed, failed and skipped requests, `Tests passed/total`, and the duration. While the run goes on, a progress bar shows how far it is.
- **All** / **Failed**: show every result, or only the failed ones.
- **Export** (after the run ends): **JSON report…** or **JUnit XML…**. See [Run reports](../reports/).
- **One group per iteration** (named **Iteration N**, or **Requests** for a single iteration), with its counts. Groups with failures open by themselves.
- **One row per request**: a passed, failed or skipped mark, the method and name, the error or skip reason, the number of sends for "repeat until" (for example **3×**), tests passed/total, the status code and the time.
- **Click a row** to see the URL that was sent, the size, errors, undefined variables, each test with its error, and the console output.

When a run finishes, a notification sums it up, for example **"Users" passed** with the request and test counts.

## Cookies and auth

- **Cookies**: in the app, a run uses the workspace's cookie jar when **Settings → Data & privacy → Cookie jar** is on, like single sends. `zorvik run` starts each run with an empty cookie jar and keeps cookies from one request to the next during the run.
- **OAuth 2.0**: tokens are fetched when a request needs one. `zorvik run` keeps them in memory for the run only.

## Limits

| Limit | Value |
|---|---|
| Iterations | 1 to 100,000 |
| Delay between requests | 0 to 600,000 ms (10 minutes) |
| Requests per iteration | 10,000 (a `setNextRequest` loop stops there) |
| Data file | 50 MB and 100,000 rows |
| Results kept in a report | The first 50,000, then failed ones only, up to 100,000. The summary counts all of them; `omitted` says how many were left out. |
| Results listed in the Runner tab | The first 10,000, then failed ones |
| Console lines per result | 200, each up to 4,096 characters |
| Finished runs kept for **Export** in the app | The last 5 |

## The same run from the command line

Every Runner setting has a `zorvik run` option:

| Runner tab | `zorvik run` |
|---|---|
| A folder's **Run…** | `--folder <path>` |
| Requests ticked and ordered | Not available: every request under the folder runs, in sidebar order |
| **Iterations** | `-n`, `--iterations` |
| **Delay (ms)** | `--delay` |
| **Data file** | `-d`, `--data` |
| **Stop on the first failure** | `--bail` |
| The active environment | `-e`, `--env` |
| — | `--allow-http-errors`, `--timeout`, `--insecure`, `--var` |

```bash
zorvik run ./my-api --folder Users --env Staging -d users.csv --bail --junit report.xml
```

See [zorvik run](../../cli/run/) for every option.

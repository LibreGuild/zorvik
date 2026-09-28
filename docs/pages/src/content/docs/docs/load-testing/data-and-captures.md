---
title: Data files and captures
description: Give each virtual user its own row of a CSV or JSON file, and capture values from responses (by JSON path, header or regular expression) for the same user's next requests.
sidebar:
  order: 4
---

Real traffic is not one request repeated with the same values. Two features make a load test look more like real users:

- A **data file** gives each virtual user its own row of values: its own login, product id or search term.
- **Captures** take a value from a response and use it in the same user's later requests: create an order, then fetch `/orders/{{orderId}}`.

Both work through ordinary `{{variables}}` in your requests.

## Variables in a load test

Before the run, each request is resolved with these layers, the first one that defines a name wins:

| In the app | With `zorvik load` |
|---|---|
| 1. Captured values of this user | 1. `--var name=value` |
| 2. This user's data file row | 2. Captured values of this user |
| 3. The active environment (with its secret values and values scripts saved) | 3. This user's data file row |
| 4. Workspace variables (with secret values and values scripts saved) | 4. The environment given with `--env` (none without it) |
| 5. Global variables (set by scripts) | 5. Workspace variables |
| 6. Dynamic values: `{{$uuid}}`, `{{$timestamp}}`, `{{$randomInt}}` … | 6. Dynamic values |

`zorvik load` reads only the workspace files: secret variables (their values stay in the app's data folder), values saved by scripts and globals are not there. Pass what you need with `--var`, for example `--var token=$API_TOKEN`. See [Secret variables](../../variables/secrets/).

A request that uses no dynamic value, no data file column and no captured variable is resolved **once** and sent as is. A request that uses any of them is rendered again **for every request**, with that user's values. Zorvik finds out which is which by rendering each request once with every data column and captured name set to a probe value and comparing.

An undefined variable in a URL's host stops the run before it starts: "{{host}} in the URL is not defined". Undefined variables anywhere else are sent as written (`{{name}}`); `zorvik load` prints a warning listing them.

## Data files

```yaml title="loadtests/Browse as users.yaml"
name: Browse as users
dataFile: data/users.csv
targets:
  - request: Auth/Login.yaml
  - request: Products/Search.yaml
stages:
  - { durationSecs: 10, target: 20 }
  - { durationSecs: 60, target: 20 }
```

```text title="data/users.csv"
email,password,term
ada@example.com,pa55-ada,keyboard
linus@example.com,pa55-linus,monitor
grace@example.com,pa55-grace,mouse
```

A request body such as `{"email": "{{email}}", "password": "{{password}}"}` then sends each user's own credentials.

### Formats

| Format | Rules |
|---|---|
| **CSV** | A header row, then one row per line. Comma-separated, RFC 4180 quoting (`"a, b"`, `"say ""hi"""`, line breaks inside quotes). LF or CRLF line ends; blank lines are skipped. A header column can't be empty or appear twice. Missing values at the end of a row are empty; a trailing comma is ignored; more values than columns is an error. Every value is text. |
| **JSON** | An array of objects, one per row. Columns are the keys, in order of first appearance. Strings are used as they are, `null` as an empty value, numbers, booleans, objects and arrays as JSON text. A key missing from an object leaves that variable to the lower layers for that row. |

The format comes from the extension (`.csv`, `.json`); other names are read as JSON when the content starts with `[`, else as CSV. The file must be UTF-8 (a byte-order mark is fine; in Excel, save as "CSV UTF-8"). It needs at least one row, at most 100,000 rows, and at most 50 MB.

### Where the file can be

`dataFile` is a path relative to the workspace folder, or an absolute path. Choosing a file in the app stores it relative to the workspace when it is inside. The file must be **inside the workspace folder** (links that lead out count as outside) unless:

- the app setting **Files outside the workspace** is on (Settings → Data & privacy), or
- `zorvik load` runs with `--allow-outside-files`.

During an AI agent's call, files outside the workspace are never read, whatever the setting.

Keep the data file in the workspace and commit it with the load test, so the test runs the same on every computer and in CI. The app shows the file's format, row count and first five rows under **Data file**.

### Which row a request gets

| Model | Row |
|---|---|
| Virtual users | User number *N* (users are numbered from 0 in the order they start) takes row *N* mod *rows* and keeps it for its whole life. With more users than rows, they wrap around: with 3 rows, users 0, 3, 6 … share row 0. |
| Request rate | Each request takes the next row, starting over after the last one. |

A data file row wins over the environment and workspace variables of the same name, and a captured value wins over the row.

### Hosts from the data file

A request whose host comes from a column (`https://{{host}}/health`) is rendered with **every row** before the run, and every host found goes through the [outside-host check](../overview/#safety-hosts-outside-your-computer). The run can only send to those hosts.

## Captures

A capture saves one value from a request's response as a variable of the **same virtual user**. That user's later requests use it as `{{variable}}`.

```yaml title="loadtests/Order flow.yaml"
name: Order flow
targets:
  - request: Orders/Create order.yaml     # POST {{baseUrl}}/orders
    captures:
      - { variable: orderId, from: json, path: $.id }
      - { variable: etag, from: header, path: ETag }
  - request: Orders/Get order.yaml        # GET {{baseUrl}}/orders/{{orderId}}
  - request: Orders/Cancel order.yaml     # DELETE … with If-Match: {{etag}}
stages:
  - { durationSecs: 0, target: 25 }
  - { durationSecs: 120, target: 25 }
thinkTimeMs: 200
```

In the app, click the variable icon in a request's row (under **Requests**) to open its captures, then **Add capture**: a variable name, where the value comes from, and the path, header name or pattern.

| Field | Required | Default | Meaning |
|---|---|---|---|
| `variable` | yes | | Variable name used as `{{variable}}`. No spaces or braces. |
| `from` | no | `json` | `json`, `header` or `regex`. |
| `path` | yes | | The JSON path, the header name, or the regular expression. |

### `json`: a JSON path

A subset of JSONPath that picks **one** value:

| Path | Picks |
|---|---|
| `$.id` | The `id` field of the top-level object. |
| `$.data.order.id` | A nested field. |
| `$.items[0].id` | The first item's `id`. |
| `$.items[-1].id` | The last item's `id` (negative indexes count from the end). |
| `$['odd key']["a.b"]` | Keys with spaces, dots or other characters, in single or double quotes. |
| `data.id` | The leading `$` is optional. |
| `$` | The whole body. |

Wildcards (`*`), recursive descent (`..`) and filters are not supported and are refused when the test starts. The value becomes text: strings as they are, numbers and booleans as text (`42`, `true`), objects and arrays as compact JSON. A `null`, a missing field or a body that isn't JSON is a miss.

### `header`: a response header

The header name, in any case: `ETag`, `location`, `X-Request-Id`. The first header with that name gives the value.

### `regex`: a regular expression over the body

The first match's **first group** is the value; a pattern without a group takes the whole match. For example `token=(\w+)` on `...token=abc123...` gives `abc123`, and `[A-Z]{3}-\d+` gives the whole match. The syntax is Rust's `regex` crate (no look-around or back-references). An invalid pattern stops the test before it starts ("capture 'token': invalid regular expression: …").

### How captures behave

- **Only the first 1 MB** of a body is kept for captures (decompressed). A JSON body larger than that can't be parsed, so JSON captures on it miss; regular expressions search the first 1 MB.
- **A miss keeps the old value.** When a capture finds nothing, the variable keeps whatever it had (an earlier capture, the data row, the environment). The miss is counted: **Capture misses** in the totals, a **Missed** column per request, and a warning under **Errors**. Misses are **not** errors and don't affect the error rate.
- **Captures change the order of requests.** With captures on any sent request, each virtual user goes through the requests in their weighted order by itself (for weights 1:1:1: create, get, cancel, create, get, cancel …), so a create always comes before the requests that use its value. Without captures, users share one order.
- **Captured values can't move the load.** A request rendered with a captured value must go to a host known when the run started, or it fails with "*host* is not a host this run was started for".
- **Request rate: checked, not reused.** In the request-rate model every request is its own iteration, so captured values are checked (misses are counted) but no later request uses them. The app says so under the captures. Use virtual users for chains like create, then get.

:::tip
Look at the **Missed** column after a first short run. A capture that misses on every response usually has a wrong path, or reads a response that failed (a 401 has no `$.id`).
:::

## What doesn't carry between requests

Load tests don't run scripts, so `pm.environment.set(...)` in a post-response script has no effect during a load test, and there is no cookie jar. Captures are the way to carry a value, a token or a session id from one response to the next request.

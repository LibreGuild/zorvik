---
title: Data files
description: Drive a collection run with a CSV or JSON file, one iteration per row, and use each row's values as variables in requests and scripts.
sidebar:
  order: 2
---

A data file runs the same requests with different inputs: each row is one iteration of the run, and each column is a variable for that iteration. Use it to try a list of users, products or edge cases without copying requests.

```csv title="users.csv"
username,password,expectedStatus
ada,correct-horse,200
bob,wrong,401
"carol, jr",s3cret,200
```

With this file, a run has three iterations. In the first one, `{{username}}` is `ada`; in the second, `bob`; and so on.

## Choosing a data file

**In the app**: in the Runner tab, under **Data file**, press **Choose file…** and pick a `.csv` or `.json` file. The runner shows the format, the number of rows and a preview of the first five rows. **Change** picks another file; the **×** button removes it.

- A file inside the workspace folder is remembered by its path relative to the workspace, so the setting works on every computer that has the workspace.
- A file outside the workspace folder is refused unless **Settings → Data & privacy → Files outside the workspace** is on. The message says so: `Data file '…' is outside the workspace folder. Move it into the workspace, or allow files outside the workspace in Settings → Data & privacy.`

**With `zorvik run`**: pass the file with `-d` or `--data`. The path is relative to the current folder, and the file may be anywhere.

```bash
zorvik run ./my-api --folder Login --data ./test-data/users.csv
```

**For AI agents**: `run_collection` takes a `dataFile` inside the workspace folder.

## Formats

Zorvik picks the format from the file name: `.json` is JSON and `.csv` is CSV. For other names, a file whose first non-space character is `[` is JSON, and anything else is CSV. A UTF-8 byte order mark at the start is ignored.

The file must be UTF-8 text. From Excel, save as **CSV UTF-8**.

### CSV

- The **first row is the header**: it names the columns. Names are trimmed of spaces; a column without a name, or the same name twice, is an error.
- Each following row is one iteration. **Values are kept exactly**, spaces included, and are always **text**.
- Values are separated by commas. A value in double quotes can contain commas, line breaks and quotes (written as `""`), as in RFC 4180.
- Lines may end with `\n` or `\r\n`. Blank lines are skipped. A line with just `""` is a row with one empty value.
- A row with fewer values than columns gets empty values for the missing ones. Extra empty values at the end of a row (a trailing comma, for example) are ignored; any other row with more values than columns is an error.
- A quote inside an unquoted value is kept as a normal character: `5"x` is the text `5"x`.

```csv title="orders.csv"
id,note,total
1,"likes ""fast"" delivery, please",12.50
2,,8
```

Row 1 has `note` = `likes "fast" delivery, please`; row 2 has an empty `note`. `total` is the text `"12.50"` and `"8"`.

### JSON

The file is an **array of objects**; each object is one iteration and its keys are the columns. Objects may have different keys: the columns are every key, in the order they first appear.

```json title="users.json"
[
  { "id": 1, "name": "Ada", "admin": true, "tags": ["math"] },
  { "id": 2, "name": "Grace", "admin": false, "manager": null }
]
```

JSON values keep their type in scripts: `pm.iterationData.get("id")` is the number `1`, `get("admin")` is `true`. As `{{variables}}` in a request, they become text:

| JSON value | `{{column}}` becomes |
|---|---|
| String | The string |
| Number, boolean | Its text: `1`, `true` |
| Object, array | Its JSON: `["math"]` |
| `null` | An empty string |
| Key missing in this row | Not set by the row: other scopes are used, or the variable is undefined |

## Iterations and rows

| Iterations setting | What runs |
|---|---|
| Empty (app) or no `--iterations` (CLI) | One iteration per row |
| Fewer than the rows | The first rows only |
| More than the rows | Row by row, then the **last row** again for every extra iteration |

Iteration `i` (starting at 0) uses row `i`. `pm.info.iteration` tells a script which iteration it's in, and `pm.info.iterationCount` how many there are.

## Using the values

### In requests

Every column is a variable for its iteration. Use it anywhere variables work: URL, query, headers, body, auth.

```text
POST {{base}}/login
{"username": "{{username}}", "password": "{{password}}"}
```

A row's values win over the environment, workspace and global variables. They lose to values set with `pm.variables.set` and to `--var` values on the command line:

1. `--var` values (command line)
2. `pm.variables` (set by scripts during the run)
3. **The data file row**
4. The active environment
5. Workspace variables
6. Global variables

### In scripts

`pm.iterationData` is the current row. It's read-only.

```js title="Post-response script"
const expected = Number(pm.iterationData.get("expectedStatus"));
pm.test(`${pm.iterationData.get("username")} gets ${expected}`, () => {
  pm.response.to.have.status(expected);
});
```

| Method | Returns |
|---|---|
| `pm.iterationData.get(column)` | The value (a string for CSV; the JSON type for JSON), or `undefined` |
| `pm.iterationData.has(column)` | Whether the row has the column |
| `pm.iterationData.toObject()` | The whole row |
| `pm.iterationData.replaceIn(text)` | `text` with `{{column}}` filled from the row |

`pm.variables.get(column)` also finds the value, following the precedence above. The legacy `data` global is a copy of the row.

Outside a run (a single send), the row is empty.

:::tip
Name test cases after the row, as above, so the report says which input failed: `bob gets 401`. In reports of runs with more than one iteration, JUnit test names also end with `(iteration N)`.
:::

## Limits

| Limit | Value |
|---|---|
| File size | 50 MB |
| Rows | 100,000 (the most iterations a run can have) |
| Rows needed | At least 1 |

## Errors

The runner checks the file before the run starts. In the app, the error shows under the file and the **Run** button stays disabled; `zorvik run` prints it and exits with code 2.

| Message | Cause |
|---|---|
| `The data file has no rows` | An empty file, a CSV with only a header, or `[]` |
| `The data file is not UTF-8 text (line N). Save it as UTF-8, e.g. "CSV UTF-8" in Excel.` | The file uses another encoding |
| `Column N of the CSV header has no name` | An empty column name in the header |
| `The CSV header has the column 'x' twice` | A duplicate column name |
| `Line N of the CSV data has X values, but the header has Y columns` | A row with too many values |
| `The CSV data has a quote on line N that is never closed` | An opening `"` without its closing one |
| `The data file is not valid JSON: …` | A JSON syntax error |
| `JSON data must be an array of objects, one per iteration` | The JSON file is not an array |
| `Item N of the JSON data is not an object` | An array item that isn't an object |
| `The data file has more than 100000 rows (the most iterations a run can have)` | Too many rows |
| `Data file '…' is larger than 50 MB` | The file is too big (the CLI says `larger than 50 MB`) |

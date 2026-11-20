---
title: Scenarios
description: Run saved requests one after the other as steps, pass values from one response to the next request, check what comes back, branch, loop and clean up, in the app, in CI and from AI agents.
sidebar:
  order: 2
---

A scenario runs saved requests one after the other as **steps**, the way you test by hand: create a cart, add an item, read the cart back and check the item is there. A step saves values of its response as **outputs**, and the steps after it use them as `{{name}}`. No scripts are needed, and the requests' own [scripts and tests](../../scripting/overview/) still run.

The same request can be a step as often as you like, each time with its own values. Steps can also branch, loop, run at the same time, wait, ask you for a value and run another scenario.

A scenario is one file in the workspace (`scenarios/<name>.yaml`, or in a [folder](#folders)) and runs the same in three places:

| Where | How |
|---|---|
| The app | A scenario tab (this page) |
| A terminal or CI | [`zorvik run --scenario`](../../cli/run/#-s-scenario) |
| AI agents | The `run_scenario` tool (see [Tools](../../agents/tools/#scenarios)); the scenario's tab shows the run live |

## Make a scenario

- The **Scenarios** section of the left rail → **+** → **New scenario** (no steps yet), or **Scenario from the collection…** (a step for every request).
- Right-click a folder in the collection → **New scenario…**: a step for every request under it that a run can send, in sidebar order. The **+** at the top of the collection does the same for the whole collection.
- In a [Runner tab](../collection-runner/), **Save as a scenario** (the button next to **Run**): the ticked requests, in their order.
- The command palette (<kbd>Ctrl</kbd>/<kbd>⌘</kbd> <kbd>K</kbd>) → **New scenario**.

Each asks for a name and opens the scenario in a tab. To rename, duplicate, move or delete a scenario, right-click it in the **Scenarios** section; drag scenarios there to reorder them.

## Folders

Keep scenarios organized in folders, and folders in folders, as in the collection.

- **New folder**: the **+** at the top of the **Scenarios** section, or right-click a folder → **New folder** for one inside it. Right-click a folder → **New scenario** makes a scenario in it.
- **Move** a scenario or a folder by dragging it: onto a folder to put it inside, onto the top edge of a row to put it before that row, or onto the empty space below the list for the top level. Right-click → **Move to…** picks the folder from a list.
- **Rename** or **delete** a folder by right-clicking it. Deleting a folder moves it, and every scenario in it, to the trash.
- The **Filter** at the top shows the scenarios whose name matches, in their folders.

A scenario's tab shows the folders it is in before its name. A folder is a folder on disk (`scenarios/Shop/Checkout.yaml`), and the scenario's **id** is its path without `.yaml`: `Shop/Checkout`. [Run scenario](#run-scenario) steps, [`zorvik run --scenario`](../../cli/run/#-s-scenario) and AI agents use that id. When you rename or move a scenario or a folder in Zorvik, the steps that run the scenarios in it follow, and so do the values their last runs left.

## The scenario tab

The header has **Run** (or **Stop**), also <kbd>Ctrl</kbd>/<kbd>⌘</kbd> <kbd>Enter</kbd>, and **Save** (<kbd>Ctrl</kbd>/<kbd>⌘</kbd> <kbd>S</kbd>). Under the name, a line says how many steps there are, or what keeps the scenario from running (for example *Add a step*, or a step and what it is missing).

The left side has four sections:

| Section | Holds |
|---|---|
| **Steps** | The steps, in order, and the steps under **Always at the end** |
| **Variables** | The scenario's **Starting values**, and the **Values of the last run** |
| **Settings** | The **Data file**, how many **Times** it runs, and what happens **When a step fails** |
| **Docs** | Notes about the scenario, in Markdown |

The right side shows the results of the latest run. In a narrow window the two sides become **Scenario** and **Results**.

**Run** uses the scenario as it is on screen, saved or not. Only one run can be in progress at a time in the app, a scenario or a [collection run](../collection-runner/).

## Steps

### Add and arrange steps

At the end of every list of steps:

- **+ Request** opens a search over the saved requests. Pick one; the list stays open to add more.
- **+ Step** adds a step of another kind: **Run scenario**, **If**, **For each**, **Repeat**, **At the same time**, **Set values**, **Wait**, **Stop**, **Script** or **Ask for a value**.

You can also drag a request, or a folder (every request under it), from the collection onto the steps.

Each step is a row: a checkbox (clear it to leave the step out of runs), its kind, and its name. Click the row to open the step's fields.

- **Move** a step by dragging its row: onto the upper half of another step to land before it, onto the lower half to land after it, or onto the end of a list. Steps move into and out of the lists inside **If**, **For each**, **Repeat** and **At the same time** the same way. The **⋯** menu has **Move up**, **Move down**, **Duplicate** and **Delete**.
- **Name**: at the foot of an open step. Without one, a step is called by what it does: the request's name, *If {{total}} is greater than 100*, *Wait 500 ms*.
- **Go on when it fails**: at the foot of an open step. See [When a step fails](#when-a-step-fails).
- A warning sign on a row says what the step still needs (*Choose a request*, *An output: give the variable a name*).
- After a run, a mark on each row shows whether the step passed or failed, and how often it ran (*3×*) when it is in a loop.

### How a value goes from one step to the next

A scenario has its own set of named values. Every step can read them, and steps add to them.

1. **A step saves a value.** On the step **Create cart**, add an output: variable `cartId`, from the JSON path `$.id`. After the step runs, the scenario has `cartId`.
2. **A later step uses it.** Anywhere in a saved request (URL, headers, body) `{{cartId}}` now takes that value, the same way [variables](../../variables/variables-and-environments/) work everywhere else. If the request already says `{{cartId}}`, there is nothing more to do.
3. **The request uses another name.** If it says `{{id}}`, give the step an input: `id` = `{{cartId}}`.
4. **Several values in one.** A value is text with `{{variables}}`: `{{userId}}-{{cartId}}` joins the values of two earlier steps.
5. **Work a value out.** A value that starts with `=` is JavaScript: `= Number(total) + 1`. See [Values](#values).

Typing `{{` in a step's fields suggests the values that step can use: the scenario's starting values, what the steps before it save, and the item and index of the loops it is in. Hover a `{{name}}` to see where it comes from.

### Run to here, and one step alone

Two buttons show on a step's row when you hover it (they are also in its **⋯** menu), so you can build a scenario one step at a time:

- **Run to here** runs the scenario from the start up to and including this step, then ends. The steps under **Always at the end** don't run (unless the step you run to is one of them), and a [data file](#data-file-and-times)'s first row is used. If the run never gets to the step (it is in a branch that isn't taken), the steps after it run too.
- **Run only this step** runs just this step (with the steps inside it), starting from the [values the last run left](#values-of-the-last-run), or from the starting values when there are none. Nothing else runs.

A typical way to work: add a request, **Run to here**, open the step and **Pick from the response**, then use the new value in the next step.

## The kinds of step

### Request

Sends a saved request, with its scripts and tests. Open the step to see the request it sends (**Open** shows the request in its own tab, **Change** picks another one) and four groups of fields.

**Inputs** are values for this step only. They win over the scenario's values, the environment and every other variable of the same name, and are gone after the step. **Add input** adds a row (name and value). Under the rows, Zorvik offers the `{{variables}}` the request uses that nothing gives a value yet, as **+ name** buttons.

**Outputs** save values of the response for the steps after this one:

| From | Path | Saves |
|---|---|---|
| **JSON path** | A JSONPath into the body: `$.id`, `$.items[0].id`, `$.items[-1].id`. The leading `$` can be left out. | The value there. A number, list or object stays what it is. A path that can match several values (`$.items[*].id`, `..`, a filter `[?@.price > 10]`, a slice) saves a list, also when it matches one value or none. |
| **Header** | The header's name, in any case | The first header with that name |
| **Cookie** | The cookie's name | The value of a cookie the response set |
| **Regex** | A regular expression over the body | Its first group, or the whole match when it has no group |
| **Status** | | The status code |
| **Body** | | The whole body as text |
| **Time (ms)** | | The response time in milliseconds |

Each output has three options:

| Option | Meaning |
|---|---|
| **Secret** | The value is hidden: `••••••` in the results and under **Variables**, and `{{name}}` in the URLs, headers and bodies that later results show, in values built from it, and in what AI agents read of the console |
| **May be missing** | Finding nothing is fine: the variable is then not set. Without it, an output that finds nothing fails the step. |
| **Add to a list** | The value is added to a list under this name instead of replacing it. A step in a loop then collects one value per round. |

A JSON `null` counts as nothing found. A value larger than 1 MB is refused.

**Pick from the response** lists every value of the step's last response with the path that picks it, and its headers; search and click one to add the output, with a name suggested from the path (`$.data.user.id` becomes `userId`). Run the step once first.

XML and HTML bodies have no path of their own: use **Regex**, or a script.

**Checks** say what the response must be. Each check is a test in the results, named by what it checks (*Status equals 201*, *$.items[0].productId equals 42*). A check reads a value like an output does (**JSON path**, **Header**, **Cookie**, **Regex**, **Status**, **Body**, **Time (ms)**) and compares it:

| Comparison | Holds when |
|---|---|
| **equals**, **does not equal** | The two are the same: as numbers when both are numbers (`201` equals `"201"`), else as text |
| **contains**, **does not contain** | The text contains the other text; a list has an item equal to it; an object has that key |
| **matches (regex)** | The regular expression finds a match in the text |
| **is greater than**, **is at least**, **is less than**, **is at most** | Both are numbers and compare that way. Something that isn't a number fails the check: *'abc' is not a number*. |
| **is one of**, **is none of** | The value equals (or doesn't) one of a list: a JSON array, or values separated by commas |
| **exists**, **does not exist** | There is a value: not missing, `null` or empty text. `0`, `false` and an empty list exist. |
| **is true**, **is false** | The value counts as true: anything but `false`, `0`, `null` and empty |

The value a check compares with is a [value](#values) like any other: text with `{{variables}}`, or JavaScript after `=`. A failed check says what it found: *It is "7"*, or *Nothing was found*.

**Try again when it fails** sends the step again when it fails, up to 99 more times, after **Pause before each try (ms)**. The result is the last try's, and shows the number of tries. Only the last try saves its outputs: a try that failed leaves none behind.

### If

Runs the steps under **Then** when its condition holds. **Add else if** adds another condition with its own steps; the first one that holds runs. The steps under **Else** run when none holds.

A condition is one or more rules: a value, a comparison (the same ones as for checks) and, for most comparisons, a value to compare with. With more than one rule, choose whether **All** or **Any** of them must hold. Several **else if** branches on the same value work as a switch.

A rule on a variable that isn't defined has no value: **does not exist** holds for it. For a condition in JavaScript, write it as the value and compare with **is true**: `= total > 100 && items.length > 0`.

### For each

Runs its steps once per item of a **List**:

- `{{items}}`, a scenario value that is a list (an output with a path like `$.items[*].id`, or one with **Add to a list**),
- a JSON array (`["a", "b"]`),
- values separated by commas (`ada, grace`),
- or JavaScript after `=` (`= items.filter(i => i.active)`).

Inside, `{{item}}` is the current item, `{{item.id}}` one of its fields when it is an object, and `{{index}}` counts from 0. **Each item as** gives the item another name (`product` gives `{{product}}` and `{{product.id}}`). After the loop, these names have the values they had before it.

### Repeat

Runs its steps several times: **Times** rounds, with `{{index}}` counting from 0.

With **Until a condition holds**, the condition is checked after each round and the loop ends when it holds: use it to wait for a job to finish. **At most (rounds)** then limits the rounds; if the condition never holds within them, the step fails (*{{status}} equals done didn't hold after 5 rounds*). **Pause between rounds (ms)** waits before every round but the first.

To poll with a single request, the request's own [Repeat until](../repeat-and-streams/) works in a scenario too.

### At the same time

Starts its steps together, for example two requests that must not interfere with each other. Each step gets its own copy of the scenario's values; when all have finished, what they saved is put together, and for a name two of them saved, the later step in the list wins. Their results show together, in the steps' order.

A step inside can't **ask for a value**: ask before it.

### Set values

Sets scenario values without sending anything: rows of name and [value](#values). Use it to join values, to work one out (`= Number(total) + 1`), or to give a value a name of its own. A row whose value is a single `{{variable}}` that isn't defined removes the value.

### Wait

Pauses for a number of milliseconds (at most 10 minutes).

### Stop

| What | Does |
|---|---|
| **Leave the loop it is in** | Ends the **For each** or **Repeat** around it; the steps after the loop go on |
| **End the scenario (passed)** | Ends the scenario here, without failing it |
| **Fail the scenario** | Ends the scenario here and fails it |

**Why** is shown in the results and may use `{{variables}}`. Put a **Stop** inside an **If** to end early on a value. The steps under **Always at the end** still run.

### Run scenario

Runs another scenario as one step, for example a login that several scenarios share.

- The other scenario has **its own values**: it starts from its own starting values and does not see this scenario's.
- **Give it**: rows of name and value, read with this scenario's values and given to the other one over its starting values.
- **Bring back**: rows of a name here and a value there, read with the other scenario's values when it has finished: `token` = `{{token}}`. A value that is secret there stays secret here. Nothing else comes back: what the other scenario saved, and what its scripts set, ends with it.

A failed step of the other scenario counts as this step failing, also one the other scenario went on after. Its own steps under **Always at the end** run before it returns. Scenarios can run each other at most 8 deep, so one that runs itself ends with an error.

### Script

JavaScript with the [`pm` API](../../scripting/pm-reference/), for whatever the other steps don't cover. `pm.variables.set(name, value)` saves a scenario value; `pm.test` and `pm.expect` add tests, and `pm.sendRequest` sends a request. The step fails when the script throws or one of its tests fails. `pm.request` is empty and there is no `pm.response`: a script step is not attached to a request.

### Ask for a value

Pauses the run and asks you for a value, such as a one-time code sent to your phone. **Save the answer as** names the variable, **Question** is shown in the dialog, and **Secret** hides the answer as you type it and in the results, also when the value comes with the run. **Cancel** gives no value, and the step fails.

Nobody can be asked in a run from the command line or by an AI agent. There, give the value when starting the run (`--var otp=123456`, or the tool's `variables`). A value the scenario already has under that name, such as a starting value, is used too. Without one the step fails: *Nobody can be asked for 'otp' in this run: give it a value when starting the run*.

## Values

Every value field of a scenario (inputs, starting values, **Set values**, what a check or a rule compares with, a **For each** list) takes one of two forms:

- **Text with `{{variables}}`**: `{{baseUrl}}/carts/{{cartId}}`, `{{userId}}-{{cartId}}`. [Dynamic variables](../../variables/dynamic-variables/) such as `{{$uuid}}` work too.
- **JavaScript, after `=`**: `= Number(total) + 1`, `= items.map(i => i.id)`, `= new Date().toISOString()`. It is one expression. Variables whose names can be written in JavaScript are there by name; all of them are in `vars` (`vars["my-id"]`). The scenario's lists, numbers and objects come as what they are; values from the environment are text.

A scenario value that is JSON can be read into with dots:

| Written | Gives |
|---|---|
| `{{user.address.city}}` | A field of the object in `user` |
| `{{items.0.id}}` | A field of the first item of the list in `items` |
| `{{items.length}}` | How many items the list has |

A variable that is itself named `user.id` wins over the field.

## Variables

### A scenario's values are its own

A scenario's values are its **starting values**, what its steps save (outputs, **Set values**, answers, the item of a loop) and what its scripts set. They belong to the scenario:

- The **environment**, the **workspace variables** and **globals** are read, never changed. A request you send on its own afterwards behaves as before.
- What scripts set with `pm.environment.set`, `pm.collectionVariables.set` and `pm.globals.set` during a scenario stays in the scenario too, as a scenario value, and is seen by the steps after it. Nothing is kept on this computer as it is after a single send.
- Every scenario has its own values; a scenario that [runs another one](#run-scenario) passes values in and out by name.

When the same name is defined in more than one place, the higher one in this table wins:

| # | Value | Set by |
|---|---|---|
| 1 | Given for the run | `--var` on the command line, `variables` of an agent's `run_scenario` |
| 2 | The step's inputs | **Inputs** of a request step, for that step only |
| 3 | The scenario's values | Starting values, the data file's row (over a starting value of the same name), outputs, **Set values**, loops, answers, scripts |
| 4 | The active environment | [Environments](../../variables/variables-and-environments/) |
| 5 | Workspace variables | |
| 6 | Globals | |
| 7 | Dynamic variables | `{{$uuid}}`, `{{$timestamp}}`, … |

So a name the scenario doesn't set comes from the environment, as in any request.

**Starting values** are set in order when a run starts; one may use an earlier one or the environment (`{{baseUrl}}/v2`).

:::note[A scenario's values are data]
What a step saves is used as it is. If a server answers with text that contains `{{token}}`, the next request sends exactly that text: it is not filled in again as a variable.
:::

### Values of the last run

**Variables → Values of the last run** lists what the scenario ended with, with **Clear** for one value and **Clear all**. Secret values show as `••••••`, and a secret inside another value (a header built from a secret key) as `{{name}}`, as in the results.

- A whole run, and **Run to here**, start from the starting values again. **Run only this step** starts from these values.
- They are kept in memory on this computer: until you clear them, close the workspace or quit. They are never written to the workspace's files.

## Data file and times

Under **Settings**:

- **Data file**: a CSV or JSON file; the scenario runs once per row, and the row's columns are values of that run. They win over starting values of the same name. The formats, limits and where the file can be are those of the collection runner: see [Data files](../data-files/).
- **Times**: how many times the scenario runs. Empty: once per data row, or once. With more times than rows, the last row is used again.

Every run of the scenario starts from the starting values again; the results show one group per iteration.

## When a step fails

A request step passes or fails like a request in the [collection runner](../collection-runner/#when-a-request-passes-or-fails), with its checks and outputs counted as tests:

| Outcome | Result |
|---|---|
| The request can't be sent, or its file can't be read | **Fails** with that error |
| It has checks or tests (not counting skipped ones) | Passes when **all** of them pass, whatever the status code |
| An output finds nothing (and isn't marked **May be missing**) | **Fails**, with a failed test: *Save cartId from $.id* (*Nothing was found*) |
| It has no checks and no tests | Passes when the status is below 400 |
| A kind of request a run doesn't send | Skipped: neither passed nor failed |

Other steps fail when they can't do what they say: a value in JavaScript that throws, a comparison of something that isn't a number, a **Repeat** whose condition never holds, a script that throws.

By default **the scenario ends at the first failed step**: the steps after it depend on it. Two switches change that:

- **Go on when it fails**, on a step: the next step runs anyway. On a step that holds others, it applies when a step inside it fails.
- **Settings → When a step fails → Go on with the next step**: every step runs.

Either way a failed step fails the scenario. With a data file, the next row still runs.

### Always at the end

The steps under **Always at the end** run after the others whatever happened: after a failed step, and after a **Stop** step. Put the clean-up there, for example deleting what the scenario created. A clean-up step that fails doesn't keep the next one from running.

They don't run when you stop the run yourself, with **Run to here** on a step that isn't one of them, or with **Run only this step**.

## Which requests run

| Request kind | As a step |
|---|---|
| HTTP, including GraphQL | Sent |
| Server-Sent Events, GraphQL subscriptions | Read until the request's stream settings say to stop. See [event streams in runs](../repeat-and-streams/#event-streams-in-runs). |
| MCP calls | Sent. A program (stdio server) starts only if you allowed it in the app; `zorvik run` needs `--allow-programs`. |
| WebSocket, Socket.IO | Connected: the request's **On connect** steps run (each wait is a test), then it closes |
| gRPC | Called; streams run their **On connect** steps and end |
| TCP, UDP, MQTT, DNS | Skipped, with the reason. **+ Request** doesn't offer them. |

For the kinds that answer with a transcript or a list of messages, outputs and checks read that body.

A script's `pm.execution.setNextRequest` does nothing in a scenario (the result's console says so): use **If** and **Stop** steps to change what runs next.

## Results

The results side lists what ran, in the order it ran:

- **A summary line**: **PASS**, **FAIL** or **STOPPED**, the steps that passed and failed, `Tests passed/total`, the time, and *ended at the first failed step* when it did.
- **One row per step**, indented as the steps are nested. A request shows its method, status and time; other steps show their kind. **If**, the loops, **At the same time** and **Run scenario** show what they decided (*yes*, *else*, *3 items*, *up to 5 rounds*), and each round of a loop has a row (*item = 42 · 2 of 3*). These rows aren't counted as steps: the counts are the steps that did something.
- **Saved values** show at the right of the step that saved them (`cartId = 7`).
- **Click a row** for its tests and checks, errors, console output and saved values; for a request, **Request** shows what was sent and **Response** what came back, as in the Runner tab.
- **All** / **Failed**, and **Export** (**JSON report…**, **JUnit XML…**) once the run has ended. In the reports each result has a `step` with its place in the scenario (`steps/2/else/0`), kind and saved values; in JUnit, request steps are suites by their request and the other steps by their name. See [Run reports](../reports/).

**Stop** ends the run at once: the step in progress is not reported.

## Limits

| Limit | Value |
|---|---|
| Steps one iteration runs (a round of a loop counts as one) | 10,000; past that the run stops with *Stopped: iteration N ran more than 10000 steps (a loop that never ends?)* |
| Times (iterations) | 1 to 100,000 |
| Steps nested inside each other | 16 levels |
| Scenarios running each other | 8 deep |
| A saved value | 1 MB |
| **Wait**, and the pauses of **Repeat** and **Try again** | 10 minutes |
| **Try again when it fails** | 99 more tries |
| Rounds of a **Repeat** | 10,000 |
| Saved values shown in a result | The first 300 characters of each |

## The file

A scenario is `scenarios/<name>.yaml` in the workspace. Renaming or moving a request in Zorvik updates the scenarios that send it. Every field is described in [Workspace format](../../reference/workspace-format/#scenarios-scenariosyaml).

```yaml title="scenarios/Checkout.yaml"
name: Checkout
seq: 0
docs: Buys what is in stock and deletes the cart afterwards.
variables:
  - { key: quantity, value: 2 }
steps:
  - type: scenario
    scenario: Log in                      # scenarios/Log in.yaml
    inputs:
      - { key: user, value: "{{shopper}}" }
    outputs:
      - { key: token, value: "{{token}}" }
  - type: request
    request: Cart/Create cart.yaml
    outputs:
      - { variable: cartId, path: $.id }
    checks:
      - { from: status, value: 201 }
  - type: request
    request: Products/List products.yaml
    outputs:
      - { variable: products, path: "$.items[*].id" }   # a list
  - type: forEach
    list: "{{products}}"
    as: productId
    steps:
      - type: request
        request: Cart/Add item.yaml
        inputs:
          - { key: qty, value: "{{quantity}}" }   # the request uses {{cartId}} and {{productId}} as they are
        retry: { times: 2, delayMs: 500 }
  - type: request
    request: Cart/Get cart.yaml
    outputs:
      - { variable: total, path: $.total }
    checks:
      - { path: $.items, op: exists }
  - type: if
    when:
      rules:
        - { value: "{{total}}", op: greater, to: 100 }
    steps:
      - type: request
        request: Cart/Apply coupon.yaml
    else:
      - type: set
        values:
          - { key: coupon, value: standard }
  - name: Pay
    type: request
    request: Cart/Checkout.yaml
    inputs:
      - { key: reference, value: "{{cartId}}-{{$timestamp}}" }
always:                                   # clean up, whatever happened
  - type: request
    request: Cart/Delete cart.yaml
```

## From the command line and AI agents

```bash
zorvik run ./my-api --scenario Checkout --env Staging --var shopper=ada --junit report.xml
```

`zorvik run --scenario` runs a saved scenario with the same steps and reports, and exits with 1 when a step fails. See [zorvik run](../../cli/run/#-s-scenario).

AI agents list, read, save and run scenarios with `list_scenarios`, `read_scenario`, `save_scenario` and `run_scenario`, also up to a step or one step alone. See [Tools](../../agents/tools/#scenarios).

---
title: Variables & environments
description: The {{variable}} syntax, where variables work, environments, workspace variables and globals, how scopes take precedence, and values set by scripts.
sidebar:
  order: 1
---

Variables let you write a request once and run it anywhere. Instead of `https://staging.example.com/users/42`, write:

```text
{{baseUrl}}/users/{{userId}}
```

Then switch the active **environment** in the title bar to send the same request to your laptop, staging or production.

## Syntax

| Rule | Example |
|---|---|
| A variable is a name in double braces | `{{baseUrl}}` |
| Spaces around the name are ignored | `{{ baseUrl }}` is `{{baseUrl}}` |
| Names are case-sensitive | `{{baseURL}}` is not `{{baseUrl}}` |
| A name can't contain braces or a line break; anything else goes | `{{api-key}}`, `{{user.id}}` |
| Names starting with `$` are [dynamic variables](../dynamic-variables/) | `{{$uuid}}`, `{{$timestamp}}` |
| Values may contain variables themselves | `baseUrl` = `https://{{host}}/{{apiVersion}}` |
| Braces that don't form a variable are sent as they are | `{"a":{"b":1}}`, `{{}}`, `a {{ b` |

Variables are replaced as plain text before sending. In a JSON body, `"{{name}}"` produces a string and `{{count}}` a number, depending on the quotes you write.

### Where variables work

| Part of the request | Variables |
|---|---|
| URL, query parameters, path variable values | Yes |
| Header names and values | Yes |
| JSON, Text and XML bodies | Yes, anywhere in the text |
| Form fields, multipart part names and values, file paths | Yes |
| GraphQL query, variables and operation name | Yes |
| Auth fields (username, password, token, prefix, API key, every OAuth 2.0 field) | Yes |
| The method, and the content type of a Text body | No |

Scripts read variables with `pm.variables.get("name")` and friends. See [Scripts & tests](../../scripting/overview/). Mock servers can use environment and workspace variables in their answers too, but never secret ones. See [Mock servers](../../servers/mock-api/).

### In the editor

- Known variables are highlighted. **Undefined ones turn red** before you send.
- Type `{{` to get suggestions: every defined variable and every dynamic variable. <kbd>↑</kbd>/<kbd>↓</kbd> choose, <kbd>Enter</kbd> or <kbd>Tab</kbd> inserts, <kbd>Esc</kbd> closes.
- **Hover a variable** to see its value and where it comes from: *From the active environment*, *workspace variables* or *globals*, and *set by a script (this computer only)* for [values set by scripts](#values-set-by-scripts). Secret values show as `••••••`. An undefined one says *Not defined in the active environment or workspace*.

## Scopes and precedence

A variable can be defined in several places. When the same name is defined in more than one, the one higher in this table wins:

| # | Scope | Set by | Lasts |
|---|---|---|---|
| 1 | `--var KEY=VALUE` | The `zorvik` command line only | That command |
| 2 | Request variables | `pm.variables.set()` in a script | This send, or the whole collection run |
| 3 | Data row | A data file in the [collection runner](../../testing/collection-runner/) (CSV column or JSON key) | That iteration |
| 4 | **Active environment** | The environment chosen in the title bar | Saved in `environments/` |
| 5 | **Workspace variables** | **Environments & variables → Workspace variables** | Saved in `zorvik.yaml` |
| 6 | **Globals** | `pm.globals.set()` in a script | On this computer, for every workspace |
| 7 | Dynamic variables | Built in: `{{$uuid}}`, `{{$timestamp}}`, … | Generated each time they're used |

So an environment's `baseUrl` overrides a workspace `baseUrl`, and a defined variable named `$timestamp` overrides the dynamic one.

In the environment, workspace and global scopes, a [value set by a script](#values-set-by-scripts) replaces the value from the file.

Within one scope, a variable that is switched off (its checkbox cleared) doesn't exist, and if the same name appears twice, the last one counts.

## Environments

An environment is a named set of variables, such as `Local`, `Staging` or `Production`. One environment is **active** at a time, or none.

### Choosing the active environment

The environment picker in the title bar shows the active environment, or **No environment**. Open it to choose another one; each shows how many variables it has. **Manage environments…** opens the editor.

The active environment is remembered per workspace on this computer, not in the workspace files, so each teammate can pick their own.

### Editing environments

Press <kbd>Mod</kbd>+<kbd>E</kbd>, or choose **Manage environments…** in the picker, to open **Environments & variables**. On the left are **Workspace variables**, **Globals** and your **Environments**, sorted by name; the active one has a highlighted icon.

| To | Do |
|---|---|
| Create an environment | **+** next to **Environments**. It starts as "New environment" with no variables. |
| Rename it | Edit the name at the top. Its file is renamed when you save. |
| Copy it | **Duplicate**: "*name* copy", with all its variables, secret values included |
| Delete it | **Delete**, then confirm. The file goes to your system's trash; its secret values and values set by scripts on this computer are deleted. |
| Add a variable | Type in the empty last row: **Variable** and **Value** |
| Switch a variable off | Clear its checkbox. It stays in the file but isn't defined. |
| Make it secret | Choose the lock icon. See [Secrets](../secrets/). |
| Reorder or remove | Drag the handle; the trash icon removes a row |

Changes aren't saved until you choose **Save changes**. A dot marks every environment with unsaved changes; **Save changes** saves them all. Closing with unsaved changes asks **Discard changes?** Rows without a name are dropped when saving.

### Environment files

Each environment is a file in `environments/`, named after the environment:

```yaml title="environments/Staging.yaml"
name: Staging
variables:
  - key: baseUrl
    value: https://staging.api.example.com
  - key: apiVersion
    value: v2
  - key: token
    value: ""
    secret: true
  - key: debug
    value: "true"
    enabled: false
```

`enabled` and `secret` are only written when they differ from the default (on, and not secret). A secret variable's value is always empty in the file.

## Workspace variables

Workspace variables apply whichever environment is active, and are the fallback when no environment is active or the environment doesn't define a name. Use them for values that are the same everywhere, such as an API version or a page size.

Edit them in **Environments & variables → Workspace variables**, with the same table as environments. They are stored in `zorvik.yaml`:

```yaml title="zorvik.yaml"
version: 1
id: 3b1d8c2e-5f4a-4e0b-9a8f-2d7c1e6b9f00
name: Payments API
variables:
  - key: apiVersion
    value: v2
  - key: pageSize
    value: "50"
```

Scripts see workspace variables as `pm.collectionVariables`. Postman collection variables are imported into them. See [Import & export](../../requests/import-export/#postman-collections).

## Globals

Globals apply to **every workspace** on this computer, below environments and workspace variables. They are only ever set by scripts, with `pm.globals.set("name", "value")`, and never written to any workspace file.

**Environments & variables → Globals** lists them; clear one with **×**, or all with **Clear all**. With none set, it says *No global values yet. Scripts set them with pm.globals.set; they apply to every workspace.*

## Values set by scripts

Scripts can change variables while they run. Where the change goes depends on the call:

| Call | Changes | Kept after the send |
|---|---|---|
| `pm.variables.set()` | Request variables | No: only for this send (or this collection run) |
| `pm.environment.set()` | The active environment | Yes, on this computer. Without an active environment, only for this send. |
| `pm.collectionVariables.set()` | Workspace variables | Yes, on this computer |
| `pm.globals.set()` | Globals | Yes, on this computer |

The matching `unset()` calls remove a kept value again.

Kept values are **current values**: they live in the app data folder (`local-values.json`), **never in the workspace files**, so a token a login script stores never ends up in Git. In their scope they win over the file's value (and switch on a variable whose checkbox is cleared); a name the file doesn't have is added.

Under the variable table of each environment and of the workspace variables, **Set by scripts** lists these values: *Current values from scripts. They override the values above, stay on this computer and are never written to workspace files.*

- **×** next to a value forgets it; **Clear all** forgets all of that scope. The file's value applies again.
- Values of variables marked secret stay hidden until you choose the eye icon.
- Renaming an environment keeps its values; deleting it deletes them.

All values set by scripts together may take at most 32 MB. A script that would go past that keeps its values for the current send only, and the console says *Values set by scripts were not kept*.

See [Scripts & tests](../../scripting/overview/) and the [pm API reference](../../scripting/pm-reference/).

## Undefined variables

A variable that isn't defined in any scope is **not** replaced by an empty string:

- **In the URL's host** (or at the very start of the URL), the request isn't sent. The response pane shows **Undefined variable**, for example *`{{baseUrl}}` in the URL is not defined. Select an environment that defines it, or add it under Environments. Names are case-sensitive.*, with **Open environments**.
- **Anywhere else**, it is sent as written, braces included (such as `?id={{userId}}`), and a warning above the response lists it: **Sent with undefined values: `{{userId}}`**. Empty [path variables](../../requests/http/#path-variables) are listed there too, as `:name`.

In a collection run, each request's result lists them under **Undefined variables**.

## Variables inside variables

A value can reference other variables, which are resolved in turn, with the full precedence each time:

| Variable | Value | Resolves to |
|---|---|---|
| `host` (environment) | `api.example.com` | |
| `apiVersion` (workspace) | `v2` | |
| `baseUrl` (workspace) | `https://{{host}}/{{apiVersion}}` | `https://api.example.com/v2` |

Limits keep a shared workspace from hanging the app:

- References are followed at most 10 levels deep. Deeper, typically a cycle such as `a` = `{{b}}` and `b` = `{{a}}`, the rest is left unresolved and reported as undefined.
- Variables may add at most 16 MB to one field. Beyond that, the remaining variables stay as written and are reported.

## Variables on the command line

`zorvik run`, `zorvik load` and `zorvik serve` read the same environment and workspace files:

```bash
zorvik run ./my-api --env Staging --var token=$API_TOKEN
```

- `--env` (`-e`) takes an environment's name or file name, ignoring case.
- `--var KEY=VALUE` (repeatable) wins over every other scope.
- Secret values live only in the app's data folder, so the command line doesn't have them: a secret variable is undefined there unless you pass it with `--var`. See [Secrets](../secrets/#on-the-command-line).
- Values set by scripts in the app and the app's globals are not used.

See [Command line](../../cli/overview/).

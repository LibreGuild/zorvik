---
title: Secrets
description: Mark variables as secret so tokens and passwords stay on your computer and never enter the workspace files or Git, and pass them to the command line in CI.
sidebar:
  order: 3
---

A workspace is meant to be committed to Git, so it must not contain your passwords, API keys or tokens. **Secret variables** solve this: the variable's name is in the workspace, but its value stays on your computer.

## Make a variable secret

1. Press <kbd>Mod</kbd>+<kbd>E</kbd> to open **Environments & variables**.
2. Select an environment, or **Workspace variables**.
3. In the variable's row, choose the open **lock icon** (*Plain: saved in the workspace file*). It turns into a closed lock: *Secret: kept on this computer only, never written to workspace files*.
4. Enter the value and choose **Save changes**.

The value is now shown as dots. Choose the **eye icon** to show or hide it while editing.

Use it like any other variable, for example as `{{token}}` in a Bearer token's **Token** field.

## What is stored where

```yaml title="environments/Production.yaml (in Git)"
name: Production
variables:
  - key: baseUrl
    value: https://api.example.com
  - key: token
    value: ""
    secret: true
```

The workspace file only says that `token` exists and is secret. Its value is in `secrets.json` in the app data folder on your computer (**Settings → Data & privacy → App data folder** shows where):

| | |
|---|---|
| Keyed by | The workspace (its `id` **and** its folder on this computer), then the environment or the workspace variables |
| Format | A JSON file, not encrypted. On macOS and Linux only your user can read it; on Windows it is inside your user profile. Protect your computer with disk encryption. |
| Not in | The workspace files, history, the OS keychain |

Because the key includes the folder, a copy of the workspace in another folder, or another workspace that reuses the same `id`, can't read your secrets.

:::caution[Moving the workspace folder]
If you move or rename the workspace folder, Zorvik sees a different workspace: enter the secret values again. Cookies, OAuth tokens and values set by scripts start fresh too.
:::

### Changing a variable

| Action | What happens to the secret value |
|---|---|
| Rename the environment | Kept |
| **Duplicate** the environment | Copied into the new one |
| Delete the variable, or the environment | Deleted from this computer |
| Make it plain again (lock icon) | Written into the workspace file on the next save. Check before you commit. |

## Sharing a workspace with secrets

When a teammate clones the workspace, secret variables arrive with no value. Each person enters their own values once, in **Environments & variables**, and saves.

Until then, the variable is defined but **empty**: requests that use it are sent with an empty value (an empty token, for example), and the variable doesn't show up as undefined. If a request fails with `401 Unauthorized` right after cloning, check the secret values first.

:::tip
Put a note in the workspace's docs or README that says which secret variables each environment needs and where to get them.
:::

## Where secret values stay hidden

| Place | Behaviour |
|---|---|
| The environment editor | Shown as dots until you choose the eye icon |
| Hovering `{{token}}` in a request | Shows `••••••` |
| **Set by scripts** list | Values of secret variables stay hidden until you choose the eye icon |
| History | Secret values found in the sent URL (such as an API key in the query) are replaced by `{{name}}`. Request headers and bodies are stored with their `{{variables}}`, not the values. |
| Mock servers and other servers | Never see secret variables; `{{token}}` stays as written in their answers |
| AI agents | See secret values masked |
| Postman environment import | Postman's **secret** variables become secret here, and their values go straight to your computer's store |

Secret values are **not** hidden from:

- **The request itself.** That's the point: they are sent to the server.
- **Scripts.** `pm.environment.get("token")` returns the real value, and `console.log` prints it in the Console.
- **Copy as cURL or code** with **Substitute variables** on. The dialog warns that the result contains secrets. See [Copy as cURL or code](../../requests/import-export/#copy-as-curl-or-code).
- **Responses** that echo them back.

## Values set by scripts

A login script that stores a token with `pm.environment.set("token", …)` keeps it on this computer, in `local-values.json` in the app data folder, never in the workspace files, whether or not the variable is marked secret. See [Values set by scripts](../variables-and-environments/#values-set-by-scripts).

## Don't type secrets into fields

Fields such as a Basic auth **Password** or an OAuth 2.0 **Client secret** show dots, but what you type there is saved **as plain text in the request, folder or workspace file**. Always put a secret variable there instead:

| Field | Write | With |
|---|---|---|
| Bearer **Token** | `{{token}}` | `token` marked secret |
| Basic auth **Password** | `{{password}}` | `password` marked secret |
| OAuth 2.0 **Client secret** | `{{clientSecret}}` | `clientSecret` marked secret |
| A header `X-API-Key` | `{{apiKey}}` | `apiKey` marked secret |

The same goes for any header value or body text you commit. (A proxy password in **Settings → Proxy** is not in the workspace: it is stored as plain text in the app's `settings.json` on this computer.)

## On the command line

The `zorvik` command line reads the workspace files, but not the app's data folder, so it doesn't have your secret values. A secret variable with no value is **undefined** on the command line (not empty): a request that uses it in the URL's host stops, and elsewhere it is reported as undefined.

Pass secret values with `--var`, typically from your CI system's secret store:

```bash
zorvik run ./my-api --env Production --var token="$API_TOKEN" --var clientSecret="$CLIENT_SECRET"
```

```yaml title=".github/workflows/api-tests.yml (excerpt)"
- name: API tests
  run: zorvik run ./my-api --env Staging --var token="$API_TOKEN" --junit report.xml
  env:
    API_TOKEN: ${{ secrets.STAGING_API_TOKEN }}
```

`--var` wins over every other scope. Values of variables declared secret, also when given with `--var`, are hidden in the URLs that `zorvik run` reports. See [Command line](../../cli/overview/).

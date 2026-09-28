---
title: Data locations
description: Where Zorvik keeps its data on Windows, macOS and Linux, what each file in the app data folder holds, and where the logs are.
sidebar:
  order: 4
---

Zorvik keeps two kinds of data apart:

- **Your workspace folder** holds what you share: requests, folders, environments, servers, load tests and kept OpenAPI documents (see [Workspace format](../workspace-format/)). Commit it to Git.
- **The app data folder** holds what belongs to this computer and this user: settings, history, secret values, cookies, OAuth tokens, load test results and the Training Bootcamp. It is never written into a workspace.

## The app data folder

| System | Folder |
|---|---|
| Windows | `%APPDATA%\org.libreguild.zorvik` (for example `C:\Users\ada\AppData\Roaming\org.libreguild.zorvik`) |
| macOS | `~/Library/Application Support/org.libreguild.zorvik` |
| Linux | `$XDG_DATA_HOME/org.libreguild.zorvik`, by default `~/.local/share/org.libreguild.zorvik` |

The exact path on your computer is shown in **Settings → Data & privacy → App data folder**.

### What is in it

| Path | What it holds |
|---|---|
| `settings.json` | Your [settings](../settings/). |
| `state.json` | Recent workspaces, the last workspace opened, and the active environment of each workspace. |
| `history.sqlite3` | Request history of every workspace (SQLite): the request as written (variables not filled in, so environment secrets are not stored) and a summary of its response. The newest entries per workspace are kept (**History size**, 500 by default). |
| `secrets.json` | Values of secret variables, per workspace and environment. Owner-only permissions on macOS and Linux. |
| `local-values.json` | Values that scripts set with `pm.environment.set`, `pm.collectionVariables.set` and `pm.globals.set` (global variables live only here). Owner-only. At most 32 MB. |
| `oauth-tokens.json` | OAuth 2.0 access and refresh tokens. Owner-only. |
| `cookies/<workspace key>.json` | The cookie jar of each workspace. |
| `trusted-programs.json` | Fingerprints of the programs (command, folder and environment) you allowed each workspace's MCP requests to start. |
| `trusted-servers.json` | Fingerprints of the server configurations this computer has started or saved, so only those start with their workspace. |
| `load-runs/<workspace key>/<load test id>/<run id>.json` | Load test run history: the newest 30 runs per load test. See [Results and reports](../../load-testing/results-and-reports/#run-history). |
| `agent.json` | While the app runs: the local port and token that `zorvik mcp` uses to reach it (owner-only on macOS and Linux). Removed when the app quits. |
| `bootcamp/` | The Training Bootcamp workspace. |
| `academy-progress.json` | Training Bootcamp progress: XP, completed lessons, badges, streak. |
| `academy-lab-servers.json` | While a lab runs: the servers it saved, so a lab cut short is cleaned up next time. |

The **workspace key** combines the workspace's `id` from `zorvik.yaml` with a hash of its folder path. A copied workspace (same `id`, another folder) therefore doesn't share secrets, cookies, tokens or load test history with the original.

:::note[Secrets are stored per computer, not encrypted]
Secret values, tokens and values set by scripts are plain JSON files that only your user account can read. They are kept out of the workspace so they are never committed; protect the data folder like the rest of your home folder.
:::

If `secrets.json` can't be read (for example, after a disk error), Zorvik moves it aside as `secrets.corrupt-<timestamp>.json` instead of overwriting it, and starts with an empty secret store.

### Other things kept per computer

- The window layout (sidebar width, split positions, **Response position**) is kept by the app window itself, not in these files.
- Deleted requests, folders, environments, servers and load tests go to the **system trash** (Recycle Bin on Windows), where you can restore them.

## Logs

The desktop app writes a log file per day and keeps the last 7:

| System | Folder |
|---|---|
| Windows | `%LOCALAPPDATA%\org.libreguild.zorvik\logs` |
| macOS | `~/Library/Logs/org.libreguild.zorvik` |
| Linux | `$XDG_DATA_HOME/org.libreguild.zorvik/logs`, by default `~/.local/share/org.libreguild.zorvik/logs` |

Files are named `zorvik.<date>.log`, for example `zorvik.2026-09-28.log`. At the default level (`info`) the log records start-up (version and data folder), the port the app listens on for AI agents, warnings such as "AI agents can't connect" or files that could not be saved, and errors. Attach it to a bug report after reading it through.

For more detail, start the app from a terminal with the `RUST_LOG` environment variable, for example `RUST_LOG=debug`:

```bash
# macOS
RUST_LOG=debug /Applications/Zorvik.app/Contents/MacOS/zorvik-desktop
# Linux (.deb or .rpm)
RUST_LOG=debug zorvik-desktop
```

```powershell
# Windows (PowerShell), from the install folder
$env:RUST_LOG = "debug"; .\zorvik-desktop.exe
```

The `zorvik` command line writes no log files: it prints to the terminal (`zorvik mcp` prints its problems to standard error, since standard output carries MCP messages).

## Start fresh

To reset Zorvik on a computer, quit it and delete (or rename) the app data folder. You lose settings, history, secret values, cookies, tokens, load test history and Bootcamp progress; your workspaces are untouched. To reset only the Training Bootcamp's workspace, use **Reset Bootcamp workspace…** in the Academy menu instead: progress stays.

---
title: Command line overview
description: Install the zorvik command line and put it on your PATH on Windows, macOS and Linux, and learn the behaviour, options and exit codes all its commands share.
sidebar:
  order: 1
---

`zorvik` is Zorvik's command line. It runs the same workspace files as the app, with the same engine, so a request, a script or a load test behaves the same in a terminal, in CI and in the app. It needs no account and doesn't need the app to be running (except `zorvik mcp`, which talks to the app).

| Command | What it does |
|---|---|
| [`zorvik run`](../run/) | Runs the HTTP requests of a workspace or folder with their scripts and tests, and reports the results (text, JSON, JUnit) |
| [`zorvik load`](../load/) | Runs a saved load test and checks its thresholds |
| [`zorvik serve`](../serve/) | Starts a saved mock API or server and prints its traffic until you stop it |
| [`zorvik mcp`](../mcp/) | The MCP server AI agents start to control the Zorvik app |

```bash
zorvik run ./my-api --env Staging --junit report.xml
zorvik load ./my-api "Checkout smoke" --html report.html
zorvik serve ./my-api "Payments mock" --port 3100
```

## Install

Every download of Zorvik contains the app **and** the command line. How it gets on your `PATH` depends on the system:

| System | Download | Where `zorvik` is | On `PATH` |
|---|---|---|---|
| Windows 10/11 | `Zorvik-Windows-Setup-x64.exe` | The install folder, next to the app | Yes: the installer adds the folder to your user `PATH` (no admin rights) and the uninstaller removes it |
| Windows 10/11 | `Zorvik-Windows-Portable-x64.zip` | `zorvik.exe`, next to `zorvik-desktop.exe` | No: add the folder yourself, or call it by its full path |
| macOS 11+ | `Zorvik-macOS-universal.dmg` | `Zorvik.app/Contents/MacOS/zorvik` | After **Add zorvik to PATH…** (see below) |
| Ubuntu 22.04+, Debian 12+ | `Zorvik-Linux-amd64.deb` | `/usr/bin/zorvik` | Yes |
| Fedora, RHEL, openSUSE | `Zorvik-Linux-x86_64.rpm` | `/usr/bin/zorvik` | Yes |
| Any Linux (x86-64) | `Zorvik-Linux-x86_64.AppImage` | Not included | Use the standalone command line below |

Downloads are on the [releases page](https://github.com/LibreGuild/zorvik/releases/latest). See [Install Zorvik](../../getting-started/install/) for installing the app.

### Only the command line (CI, servers, containers)

When a machine only runs `zorvik`, download it on its own. Each archive holds the one program and its licenses, nothing else to install:

| System | File |
|---|---|
| Windows (x64) | `zorvik-cli-windows-x64.zip` (`zorvik.exe`) |
| macOS 11+ (Apple silicon and Intel) | `zorvik-cli-macos-universal.tar.gz` |
| Linux (x86-64, glibc 2.35+: Ubuntu 22.04+, Debian 12+, Fedora 36+) | `zorvik-cli-linux-x86_64.tar.gz` |

The `releases/latest/download/` links always give the newest version, which suits pipelines:

```bash title="Linux (or macOS with the macos-universal file)"
curl -fsSL https://github.com/LibreGuild/zorvik/releases/latest/download/zorvik-cli-linux-x86_64.tar.gz \
  | sudo tar -xz -C /usr/local/bin zorvik
zorvik --version
```

```yaml title=".github/workflows/api-tests.yml (a step)"
- name: Install zorvik
  run: |
    curl -fsSL https://github.com/LibreGuild/zorvik/releases/latest/download/zorvik-cli-linux-x86_64.tar.gz \
      | tar -xz -C "$RUNNER_TEMP" zorvik
    echo "$RUNNER_TEMP" >> "$GITHUB_PATH"
```

```powershell title="Windows (PowerShell)"
Invoke-WebRequest https://github.com/LibreGuild/zorvik/releases/latest/download/zorvik-cli-windows-x64.zip -OutFile zorvik.zip
Expand-Archive zorvik.zip -DestinationPath "$env:LOCALAPPDATA\zorvik"
& "$env:LOCALAPPDATA\zorvik\zorvik.exe" --version
```

To pin a version instead, use `releases/download/v0.2.0/…`. The standalone command line doesn't update itself.

### Windows

The installer adds its folder to your user `PATH`. Open a **new** terminal afterwards: terminals that were already open don't see the change.

If your `PATH` is very long (more than 1,024 characters), the installer leaves it alone and says so in its log: `Your PATH is too long to change safely: add … to it to use zorvik in a terminal.` Add the install folder to `PATH` yourself in that case (Settings → System → About → Advanced system settings → Environment Variables).

### macOS

The app bundle can't put a command on `PATH` by itself. Once:

1. Move **Zorvik** to **Applications** and open it from there.
2. Open **Settings → AI agents**. Next to **Command-line tool** you see where `zorvik` is.
3. Press **Add zorvik to PATH…** and enter your password. This links `/usr/local/bin/zorvik` to the command line inside the app.
4. Open a new terminal and run `zorvik --version`.

If Settings says **Not found next to the app. Move Zorvik to Applications and open it from there.**, the app is running from the disk image or a temporary copy macOS made; move it to Applications first.

The same link by hand:

```bash
sudo mkdir -p /usr/local/bin
sudo ln -sfn /Applications/Zorvik.app/Contents/MacOS/zorvik /usr/local/bin/zorvik
```

The builds are not code-signed yet. If macOS blocks the app, allow it in System Settings → Privacy & Security → **Open Anyway**, or run `xattr -dr com.apple.quarantine /Applications/Zorvik.app`.

### Linux

The `.deb` and `.rpm` packages install `zorvik` to `/usr/bin`:

```bash
sudo apt install ./Zorvik-Linux-amd64.deb      # Ubuntu, Debian
sudo dnf install ./Zorvik-Linux-x86_64.rpm     # Fedora, RHEL
```

The AppImage has no command line on `PATH`. Settings → AI agents then says **Not found next to the app (the AppImage has none: use the .deb or .rpm for the command line)**.

### Check the installation

```bash
zorvik --version     # zorvik 0.1.1
zorvik --help
```

For CI machines, see the [GitHub Actions and GitLab CI examples](../run/#ci-examples).

## How commands work

### The workspace argument

`run`, `load` and `serve` take the **workspace folder** as their first argument: the folder that contains `zorvik.yaml`. A relative path is relative to the current folder.

```bash
zorvik run .                  # the workspace is the current folder
zorvik run ./api-tests        # a workspace inside the current folder
```

A folder without `zorvik.yaml` is refused: `error: … is not a Zorvik workspace (no zorvik.yaml)`.

### Names and ids

Environments, load tests and servers are found by **id** (the file name without `.yaml`) or by **name**, ignoring case. All of these find `environments/staging.yaml` named "Staging":

```bash
zorvik run . --env Staging
zorvik run . --env staging
zorvik run . -e STAGING
```

Names with spaces need quotes: `zorvik load . "Checkout smoke"`. When a name isn't found, the error lists what exists: `load test 'nope' not found (load tests: Smoke, Checkout smoke)`.

### Variables

`--var KEY=VALUE` sets a variable with the **highest precedence**: it wins over data files, environments, workspace variables and anything scripts set. Repeat it for more variables. The text is split at the first `=`, so values may contain `=`; spaces around the key are ignored.

```bash
zorvik run . --env Staging --var token="$API_TOKEN" --var region=eu-west-1
```

A value without `=` is refused: `error: --var expects KEY=VALUE, got 'token'`.

### Secrets

Secret variable values are stored only in the app's data folder on each computer, never in the workspace files. The command line reads only the files, so a secret variable has no value there: it is **undefined** (and reported as such), not sent as an empty string. Pass secrets with `--var`:

```bash
zorvik run . --env Production --var apiKey="$API_KEY"
```

In reports, values of variables declared secret are still shown as `{{name}}`, also when they come from `--var`.

### What the command line doesn't use

The command line works from the workspace files and doesn't use the app's data folder, so:

- **App settings don't apply.** The defaults are used: request timeout 60 s (`zorvik run --timeout` changes it), connect timeout 15 s, redirects followed (at most 10), TLS certificates verified (`--insecure` turns it off), responses up to 100 MB, script time limit 5 s, the system's proxy settings (proxy environment variables, then the operating system's settings), and the operating system's trusted certificates. Custom CA and client certificates set in the app's Settings are not used.
- **Nothing is kept.** Cookies and OAuth 2.0 tokens live in memory for one command. Values scripts set are forgotten when `zorvik run` ends. The workspace files are never changed.
- **No history.** Commands don't add to the app's history.

A request's own settings (its **Settings** tab: timeout, redirects, TLS verification, HTTP version) still apply.

### Output

- Results go to standard output; errors go to standard error, starting with `error:`.
- Colors are used when standard output is a terminal, unless the `NO_COLOR` environment variable is set. `--json` output never has colors.
- Control characters from responses, traffic and workspace files are printed escaped (for example `\u{1b}`), so they can't change your terminal.
- Writing to a closed pipe is fine: `zorvik serve … | head` doesn't crash.

### Help and version

| Option | What it does |
|---|---|
| `-h`, `--help` | Help for `zorvik` or a command: `zorvik run --help` |
| `-V`, `--version` | Prints the version, for example `zorvik 0.1.1` |
| `zorvik help <command>` | Same as `zorvik <command> --help` |

## Exit codes

| Code | `zorvik run` | `zorvik load` | `zorvik serve` | `zorvik mcp` |
|---|---|---|---|---|
| `0` | Every request passed | Every threshold passed | Stopped with Ctrl+C | The agent closed the connection |
| `1` | A request or test failed | A threshold failed | — | No data folder, or an input/output error |
| `2` | Stopped with Ctrl+C, or couldn't start | The run couldn't start or broke off, a report couldn't be saved, or Ctrl+C twice | Couldn't start, or the server stopped with an error | — |

Wrong options or values (a missing argument, `--iterations 0`, an unknown flag) exit with `2` for every command. See each command's page for the details.

## Environment variables

| Variable | Used by | Effect |
|---|---|---|
| `NO_COLOR` | `run`, `load`, `serve` | Any value turns colors off |
| `ZORVIK_DATA_DIR` | `mcp` | The app's data folder, instead of the default location |
| `ZORVIK_APP` | `mcp` | The program to start when the app isn't running, instead of the installed app |

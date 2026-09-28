---
title: Install Zorvik
description: Download and install the Zorvik desktop app and the zorvik command line on Windows, macOS or Linux.
sidebar:
  order: 1
---

Zorvik is a desktop app for Windows, macOS and Linux. Every download contains two programs:

- **The app** (Zorvik), where you build, send, mock and load test requests.
- **The `zorvik` command line**, which runs the same workspaces in a terminal or a CI pipeline.

There is no account to create and nothing to sign in to. The app works offline.

## Downloads

Get the newest version from the [latest release](https://github.com/LibreGuild/zorvik/releases/latest) on GitHub.

| System | File | Notes |
|---|---|---|
| Windows 10/11 (x64) | `Zorvik-Windows-Setup-x64.exe` | Installs for the current user, no administrator rights needed. Puts `zorvik` on `PATH`; the uninstaller removes it again. |
| Windows 10/11 (x64) | `Zorvik-Windows-Portable-x64.zip` | No installation: unzip and run `zorvik-desktop.exe` (the app). `zorvik.exe` (the command line) is next to it. |
| macOS 11 or newer | `Zorvik-macOS-universal.dmg` | One build for Apple silicon and Intel. |
| Ubuntu 22.04+, Debian 12+ | `Zorvik-Linux-amd64.deb` | Installs `zorvik` to `/usr/bin`. |
| Fedora, RHEL, openSUSE | `Zorvik-Linux-x86_64.rpm` | Installs `zorvik` to `/usr/bin`. |
| Any Linux (x86-64) | `Zorvik-Linux-x86_64.AppImage` | Runs anywhere, but has no `zorvik` command line. |

:::note[Nightly builds]
Want the newest changes before a release? The [nightly build](https://github.com/LibreGuild/zorvik/releases/tag/nightly) is built from `main` after every merge and replaced each time. Settings shows it as "nightly N · commit" next to the version.
:::

## Windows

1. Download `Zorvik-Windows-Setup-x64.exe` and run it.
2. The builds are not code-signed yet, so Windows SmartScreen may warn you. Choose **More info**, then **Run anyway**.
3. Start Zorvik from the Start menu.

The installer adds `zorvik` to your `PATH`. Open a **new** terminal window to use it.

To run Zorvik without installing it, use the portable zip instead. It contains `zorvik-desktop.exe` (the app) and `zorvik.exe` (the command line). Nothing is added to `PATH`; call `zorvik.exe` with its full path, or add its folder to `PATH` yourself.

## macOS

1. Download `Zorvik-macOS-universal.dmg` and open it.
2. Drag **Zorvik** to **Applications**.
3. Open Zorvik from **Applications**. The builds are not code-signed yet, so macOS blocks the first open: open **System Settings → Privacy & Security** and choose **Open Anyway**.

Instead of **Open Anyway**, you can remove the quarantine flag in a terminal:

```bash
xattr -dr com.apple.quarantine /Applications/Zorvik.app
```

The app is ad-hoc signed so it runs on Apple silicon.

:::caution[Run it from Applications]
Start Zorvik from the Applications folder, not from the mounted disk image. When the app runs from the disk image (or from a temporary copy macOS makes for quarantined apps), it can't find its own `zorvik` command line, and **Settings → AI agents** says so.
:::

### Put `zorvik` on your PATH (macOS)

A macOS app can't add itself to `PATH`. To use `zorvik` in a terminal:

1. Open **Settings** (<kbd>⌘</kbd>+<kbd>,</kbd>) and choose **AI agents**.
2. Under **Command-line tool**, choose **Add zorvik to PATH…**.
3. Enter your password when macOS asks.

This links `zorvik` into `/usr/local/bin`. Open a new terminal window afterwards. When it's done, the same place says **On PATH: type zorvik in a terminal.**

## Linux

**Debian and Ubuntu:**

```bash
sudo apt install ./Zorvik-Linux-amd64.deb
```

**Fedora, RHEL and openSUSE:**

```bash
sudo dnf install ./Zorvik-Linux-x86_64.rpm
```

Both packages install `zorvik` to `/usr/bin`. They are built on Ubuntu 22.04, the oldest supported system, so they also run on newer distributions.

**AppImage (any x86-64 distribution):**

```bash
chmod +x Zorvik-Linux-x86_64.AppImage
./Zorvik-Linux-x86_64.AppImage
```

The AppImage runs from a temporary mount, so it doesn't include a usable `zorvik` command line. Use the `.deb` or `.rpm` package when you need the command line.

## Check the command line

In a new terminal:

```bash
zorvik --version
zorvik --help
```

The command line has four commands: `run` (collections and tests), `load` (load tests), `serve` (mock servers and servers) and `mcp` (the connection for AI agents). See [Command line](../../cli/overview/).

| Download | Where `zorvik` is |
|---|---|
| Windows installer | Installed next to the app and added to `PATH` |
| Windows portable zip | `zorvik.exe`, next to `zorvik-desktop.exe` |
| macOS | Inside `Zorvik.app`; **Add zorvik to PATH…** links it to `/usr/local/bin` |
| `.deb` and `.rpm` | `/usr/bin/zorvik` |
| AppImage | Not available |

**Settings → AI agents → Command-line tool** always shows where the app found its `zorvik`, and whether it is on your `PATH`.

## First start

Zorvik opens on the welcome screen:

- **Training Bootcamp**: a hands-on course inside the app, for when you're new to APIs and networks. See [Training Bootcamp](../../bootcamp/training-bootcamp/).
- **New workspace**: create a folder for your requests.
- **Open workspace**: open a folder that already contains `zorvik.yaml`, for example one you cloned from Git.
- **Recent**: workspaces you opened before.

Continue with [Your first request](../first-request/).

## Where Zorvik keeps your data

Zorvik keeps two kinds of data apart:

- **Your workspace folder** holds requests, folders, environments, mock servers and load tests as YAML files. You choose where it is, and you can commit it to Git.
- **The app data folder** holds what belongs to this computer only: settings, history, cookies, OAuth tokens, secret variable values and values set by scripts. **Settings → Data & privacy → App data folder** shows its path.

| System | App data folder |
|---|---|
| Windows | `%APPDATA%\org.libreguild.zorvik` |
| macOS | `~/Library/Application Support/org.libreguild.zorvik` |
| Linux | `~/.local/share/org.libreguild.zorvik` (or `$XDG_DATA_HOME/org.libreguild.zorvik`) |

## Updating

Zorvik doesn't update itself. To update, download the newest release and install it the same way. Your workspaces and the app data folder are kept.

Versions follow [SemVer](https://semver.org). While Zorvik is `0.x`, a minor release may change behaviour; patch releases only fix bugs.

## Build from source

You need:

- **Rust** stable, 1.88 or newer
- **Node.js** 22 or newer
- **macOS:** Xcode command line tools (`xcode-select --install`)
- **Windows:** Visual Studio Build Tools with "Desktop development with C++", and WebView2 (already on Windows 10 and 11)
- **Debian/Ubuntu:** `sudo apt install libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev libssl-dev patchelf file`

Then:

```bash
git clone https://github.com/LibreGuild/zorvik.git
cd zorvik/app
npm ci
npm run tauri dev                   # run the app from source
npm run package                     # build the installers for your system
cargo build --release -p zorvik-cli # just the command line
```

[CONTRIBUTING.md](https://github.com/LibreGuild/zorvik/blob/main/CONTRIBUTING.md) has the details.

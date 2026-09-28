---
title: Updates
description: How Zorvik keeps itself up to date, what the update check sends (nothing about you), and how to turn it off.
sidebar:
  order: 5
---

The installed app keeps itself up to date from the project's releases on GitHub. It never restarts without asking and never interrupts your work.

## What happens

1. **A check.** A little after Zorvik starts, and then every six hours, it reads one file from GitHub: `latest.json` in the project's [releases](https://github.com/LibreGuild/zorvik/releases). The file says which version is newest.
2. **A download in the background.** When there is a newer version, Zorvik downloads it and checks its signature against the project's update key, which is built into the app. A download that doesn't match is thrown away.
3. **A small notice.** "Zorvik 0.2.0 is ready", with **What's new**, **Restart now** and a close button. A dot on the settings button stays until you update.
4. **Install on quit.** If you close the notice, the update installs the next time you quit Zorvik, so your next start is the new version.

**Restart now** first tells you what a restart would stop: running servers, a load test, a Bootcamp lab, unsaved tabs or a connected AI agent. Nothing restarts until you agree.

## What is sent

The update check is the only connection Zorvik makes by itself, and it only goes to GitHub:

- It asks for `latest.json` from `github.com/LibreGuild/zorvik`, then downloads the new version from the same place.
- No account, no ID, nothing about you, this computer, your settings or your work is sent. No other server is involved, and nothing is collected.
- The requests go through the proxy you set in **Settings → Proxy**.

As with any download, GitHub counts how many times a release file was downloaded. That count is all the project sees.

## Settings

**Settings → Updates**:

| Setting | Choices | What it does |
|---|---|---|
| **Updates** | **Automatic** (default) | Checks, downloads in the background, installs when you restart or quit. |
| | **Tell me, don't download** | Checks, and shows a notice with **Download** when a new version is out. |
| | **Off** | Never checks. **Check for updates** still works when you click it. |
| **Channel** | **Stable releases** (default) | Versioned releases. |
| | **Nightly builds** | A build of the newest code, once a day when there are changes. Less tested. |

The section also shows this version, where the update stands, and **Check for updates**. In `settings.json` these are `updates.mode` (`automatic`, `notify`, `off`) and `updates.channel` (`stable`, `nightly`).

:::tip[Switching back from nightly]
Moving from the nightly channel back to stable doesn't downgrade you: you stay on your nightly until the next stable release, which then updates you.
:::

## Which downloads update themselves

| Download | Updates |
|---|---|
| Windows installer | Itself. The installer runs with a small progress window and needs no administrator rights. |
| macOS app | Itself, when it is in a folder you can write to (for example `/Applications`). A copy running from the disk image, or in a folder you can't write to, tells you when a new version is out. |
| Linux AppImage | Itself. |
| Windows portable zip, `.deb`, `.rpm` | They tell you when a new version is out, with a link to the download. `.deb` and `.rpm` belong to your package manager; install the new package the same way as the first one. |
| The standalone command line | Doesn't check. Download the new archive from the [latest release](https://github.com/LibreGuild/zorvik/releases/latest) (in CI, the `latest/download/` links always give the newest). |

The `zorvik` command line inside the app updates with the app.

## If an update can't install itself

Sometimes Zorvik can't update itself: the download was interrupted, a network filter changed the file on the way (its signature then doesn't match, so it is never installed), or the app's folder can't be written to. Zorvik doesn't show an error for that. The notice says the new version is out, with **Open downloads**, which opens its release page on GitHub: download and install it the same way as the first time.

- **"Could not reach GitHub"** (after **Check for updates**): you are offline, or a proxy or firewall blocks `github.com`. Set the proxy in **Settings → Proxy**, or download the new version by hand.
- **On macOS**, keep Zorvik in `/Applications`: a copy opened from the disk image can't replace itself.
- The reason is in the [log file](../../reference/data-locations/#logs).

Your workspaces and the app data folder are never touched by an update.

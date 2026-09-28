---
title: FAQ
description: Short answers to common questions about Zorvik, its data, security, load tests, AI agents and the command line.
sidebar:
  order: 2
---

## General

### Is Zorvik free?

Yes. Zorvik is free and open source, licensed under either the MIT license or the Apache License 2.0, at your option. The source is on [GitHub](https://github.com/LibreGuild/zorvik).

### Do I need an account? Does it work offline?

No account, no sign-in, no cloud sync. Zorvik works fully offline; it talks to the servers you send requests to (and the OAuth providers and proxies you configure). The only connection it makes by itself is the [update check](../../getting-started/updates/): it reads one file from the project's releases on GitHub and sends nothing about you, this computer or your work. There is no tracking or telemetry. You can turn the update check off in **Settings → Updates**.

### Is every feature free? Will there be paid plans?

Every feature is free, for everyone, and there is no paid plan, no "team edition" and no cloud sync on the roadmap. Your work stays in files you own; share it through Git.

### Which systems does it run on?

Windows 10 and 11 (x64), macOS 11 or later (Apple silicon and Intel) and Linux x86-64 (`.deb` for Ubuntu 22.04+ and Debian 12+, `.rpm` for Fedora, RHEL and openSUSE, and an AppImage). See [Install](../../getting-started/install/).

### Why does Windows or macOS warn me when I open it?

The builds are not code-signed yet. See [Troubleshooting](../troubleshooting/#installing-and-opening) for the one-time step.

### Can I run two Zorvik windows?

Zorvik runs once per user. Opening it again brings the running window to the front. It works on one workspace at a time; switch from the workspace menu. Running servers and a running load test keep going when you switch.

## Workspaces and data

### Where are my requests saved?

In your workspace folder, as YAML files: one file per request, folders as folders, plus environments, servers and load tests. Commit the folder to Git to share it. See [Workspace format](../../reference/workspace-format/).

### Are my secrets committed to Git?

No. A variable marked **secret** has an empty value in the workspace file; the real value is kept on your computer, in the app's data folder. The same goes for cookies, OAuth tokens, values set by scripts and the history. Each teammate enters their own secrets. See [Secret variables](../../variables/secrets/) and [Data locations](../../reference/data-locations/).

### Can a workspace I cloned do something harmful?

Workspace files are treated as untrusted input. Symbolic links are not followed, file sizes and depths are capped, body and data files outside the workspace folder can't be used unless you allow it, mock servers from Git don't start by themselves until you start them once, and scripts run in a sandbox without file, network, process or timer access. Secrets are keyed by the workspace's id **and** its folder, so a workspace that copies another's id doesn't get its secrets. Do read the scripts of a collection you don't know before you run it.

### Can I edit the YAML by hand?

Yes. Zorvik picks up changes on disk, fills in defaults for missing fields, and shows a broken file with a warning instead of failing. Renaming a file by hand works too; the display name is the `name:` inside it.

### How do I move from Postman?

Choose **Import…** and drop in your collection or environment export. Scripts and tests come along: the `pm` API is Postman-compatible. OpenAPI 3, Swagger 2 and cURL commands import too.

## Load testing

### Virtual users or request rate?

Virtual users for a known number of concurrent clients that wait for each answer; request rate to send a fixed number of requests per second whatever the server does, and to see honest latency when it falls behind. See [Models and stages](../../load-testing/models-and-stages/).

### How much load can Zorvik generate?

Up to 5,000 virtual users or 50,000 requests per second per test, and up to 100,000 requests in flight. What your computer really reaches depends on its CPU, its network and the requests; the dashboard shows the generator's CPU and warns above 85 %.

### Do my scripts run during a load test?

No. Pre-request and post-response scripts, tests and spec checks are not run, redirects are not followed and there is no cookie jar. Use a [data file](../../load-testing/data-and-captures/#data-files) for per-user values and [captures](../../load-testing/data-and-captures/#captures) to carry values from one response to the next request.

### Can I load test WebSocket, gRPC or HTTP/3?

Not yet: only HTTP/1.1 and HTTP/2 requests (GraphQL included).

### Can I run two load tests at once?

No, one at a time. Run more load from several machines with `zorvik load`.

### Why did Zorvik ask before running my load test?

It targets a host outside your computer and private networks. Load only systems you own or are allowed to test. `zorvik load` doesn't ask. See [Safety](../../load-testing/overview/#safety-hosts-outside-your-computer).

### Are load test results shared with my team?

No. The run history stays on your computer (the newest 30 runs per test). Export an [HTML or JSON report](../../load-testing/results-and-reports/#export-html-and-json) to share one.

## AI agents

### Which agents work with Zorvik?

Any MCP client that can start a command over stdio: Claude Code, Codex, Gemini CLI, Cursor, Windsurf, Antigravity, VS Code, Zed and others. See [Connect an AI agent](../../agents/setup/).

### Can an agent delete my work or read my secrets?

Deleting always asks you, and goes to the trash. Agents never see secret values (they come back as `{{name}}` or `••••••`), settings, cookies or tokens. Load tests, starting servers and opening workspaces always ask too. See [Permissions and safety](../../agents/permissions-and-safety/).

### Does an agent need the app open?

By default, `zorvik mcp` opens the app when the agent first uses it. With **Work without the app** on, agents use Zorvik in the background while it's closed, but anything that would ask you is refused.

### Does Zorvik send my code or data to an AI service?

No. Zorvik has no AI model of its own and calls no AI service. Your agent runs `zorvik mcp` on your computer; what the agent sends to its model is up to the agent.

## Command line

### Is the command line a separate download?

No. Every download includes the `zorvik` command (except the Linux AppImage). It runs collections (`zorvik run`), load tests (`zorvik load`), servers (`zorvik serve`) and the MCP server for agents (`zorvik mcp`).

### Does `zorvik` use my app settings and secrets?

No. It reads only the workspace files and uses default settings plus its flags. Secret values are not in the files: pass them with `--var name=value` (for example from your CI's secret store). See [`zorvik load`](../../cli/load/).

### What do `zorvik load`'s exit codes mean?

`0`: every threshold passed. `1`: a threshold failed. `2`: the run couldn't produce a result. See [Thresholds](../../load-testing/thresholds/#exit-codes-of-zorvik-load).

## Learning

### Is there a tutorial?

Yes, inside the app: the [Training Bootcamp](../../bootcamp/training-bootcamp/), a course from "what is a network?" to load testing, with hands-on labs in the real workbench.

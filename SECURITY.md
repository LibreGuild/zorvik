# Security policy

## Supported versions
Security fixes go into the latest release and the nightly build. Please update to the [latest release](https://github.com/LibreGuild/zorvik/releases/latest) before reporting.

With automatic updates on (Settings → Updates, the default), the app gets fixes by itself. It installs an update only when the download is signed with the project's update key, which is built into the app, and it only ever downloads from this repository's releases on GitHub.

## Reporting a vulnerability
**Please don't open a public issue, discussion or pull request for a security problem.**

Report it privately on GitHub: [**Security → Report a vulnerability**](https://github.com/LibreGuild/zorvik/security/advisories/new). Only the maintainers can see it.

Please include:
- what an attacker can do, and what they need first (for example a shared workspace, a malicious server, or local access)
- the steps or a proof of concept to reproduce it
- the Zorvik version (Settings shows it) and your operating system

## What happens next
- We acknowledge the report within 7 days and keep you updated as we investigate.
- Once it is fixed, we publish a release and a security advisory. We credit you unless you prefer not to be named.
- Please give us a reasonable time to ship the fix before you disclose it publicly.

## What counts
Zorvik handles untrusted input on purpose, so these are especially relevant:
- **Workspace files** (they come from Git): reading or writing files outside the workspace, running code, or leaking secrets when a workspace is opened or a request is sent.
- **Secrets:** secret variable values, cookies or OAuth tokens ending up in workspace files, logs, history, exports or AI agent responses.
- **Servers and responses:** a malicious server or response that crashes the app, exhausts memory without bound, or escapes the script sandbox.
- **AI agents (MCP):** an agent getting past a confirmation, sending traffic to hosts that weren't approved, reading files outside the workspace, or another local program talking to the app's agent listener without its token.

Not in scope: load tests or port scans a user runs on purpose, and problems that need an attacker who already controls your user account.

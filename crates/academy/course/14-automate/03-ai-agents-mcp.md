---
id: ai-agents-mcp
title: AI agents over MCP
summary: Coding agents like Claude Code, Codex and Gemini CLI can work in Zorvik for you, while you watch and approve anything risky.
minutes: 6
quiz:
  - question: What is `zorvik mcp`?
    options:
      - A chat window inside Zorvik
      - A cloud service that runs your tests
      - The MCP server an AI agent starts to use Zorvik's tools, which then work through the Zorvik app
    answer: 2
    explain: The agent starts `zorvik mcp` on your computer. It connects to the running app (or starts it), so everything the agent does happens in the app you're looking at.
  - question: An agent wants to send a request to `api.example.com`, a host outside your computer and network. What happens with the default settings?
    options:
      - The request is sent without asking
      - Zorvik asks you once for that host while the agent stays connected
      - Agents can never reach the internet
    answer: 1
    explain: Requests to your own computer and private networks are allowed. An outside host asks first, once per host and agent session, so an agent can't quietly send your data somewhere new.
  - question: Can an agent read the value of your secret `apiKey`?
    options:
      - No, secret values are masked in everything an agent sees, and settings and cookies are off-limits
      - Yes, agents see every variable as it is
      - Only if it asks nicely
    answer: 0
    explain: Agents work with `{{apiKey}}`, never its value. Credentials are also removed from history, server traffic and everything else the agent reads.
---

An **AI coding agent** is an AI assistant that works in your project, not just in a chat: Claude Code, Codex, Gemini CLI, Cursor and others can read code, run commands and use tools. With Zorvik connected, they can use *your* workbench too.

## What agents can do in Zorvik

Ask in plain words, and an agent can:

- **map the APIs in your code** to a collection: it reads your routes and saves one request per endpoint,
- **send requests**, including GraphQL, gRPC, DNS and event streams, and read the answers,
- **add tests and run collections**, then fix what fails,
- **build mock servers**, start them and read what they received,
- create and run **load tests**, import OpenAPI documents, and export requests as code.

Try asking: *"Map the APIs in this repo to a Zorvik collection and run them."* In Claude Code, the ready-made command `/mcp__zorvik__map_apis` starts a guided mapping of your codebase.

## How it connects

Agents talk to tools through **MCP**, the Model Context Protocol, an open standard for plugging tools into AI assistants. The `zorvik mcp` command is Zorvik's MCP server:

```sequence
participants: You, Agent, Zorvik
You -> Agent: Run the Users folder and fix what fails
Agent -> Zorvik: run_collection, through zorvik mcp
Note over Zorvik: shows it live, asks you if it's risky
Zorvik --> Agent: 12 passed, 2 failed
Agent --> You: Two tests failed; here is a fix
```

`zorvik mcp` talks to the app over a private connection on your own computer, so the agent works in the same Zorvik you're looking at.

Connect an agent once. **Settings → AI agents → Connect an agent** shows the exact command for your computer, for example:

```bash
claude mcp add --scope user zorvik -- zorvik mcp       # Claude Code
codex mcp add zorvik -- zorvik mcp                     # Codex
gemini mcp add --scope user zorvik zorvik mcp          # Gemini CLI
```

Other tools, like Cursor or VS Code, take a short JSON snippet in their MCP settings; the same page shows it.

> [!note] Think of it like…
> A new assistant with a key card. They can walk into the rooms they need for their work, but the vault, the boss's office and the exit to the street need someone to buzz them through, and everything they do is on camera.

## You stay in charge

Everything an agent does shows up live: the title bar names the connected agent, the **AI agents** section of the left rail lists every action, and with **Follow agents** on, Zorvik opens whatever the agent is working on.

Some actions ask you first, in a dialog that names the agent and exactly what it wants to do, with **Deny** and a button to allow it (for some actions also **Allow for this session**). If you don't answer within a minute, the answer is no.

| Action | What happens |
|---|---|
| reading requests, environments, history | allowed |
| creating and changing requests, folders, environments | allowed, or asks you (**Edits by agents**) |
| requests to your computer or private network | allowed |
| requests to outside hosts | asks once per host (**Requests sent by agents**) |
| deleting, load tests, starting servers, opening workspaces | always asks |

Secret values, settings and cookies are never available to agents, and credentials are removed from everything they read. With **Work without the app** switched on, agents can use Zorvik in the background while the app is closed, but then nobody can approve anything, so every action that would ask is refused.

> [!warning] Read before you approve
> Agents can be wrong, and a web page or API answer can even contain instructions that try to trick them. An approval dialog is where you catch that: check the host, the item and the action, and choose **Deny** when something looks off. Afterwards, review what the agent saved like any teammate's change, in Git.

**You'll use this when…** you join a project with forty undocumented endpoints. Your agent maps them into a collection in minutes, adds tests and runs them, while you approve the few steps that matter and review the result.

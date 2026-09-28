---
id: why-mcp
title: Why MCP exists
summary: The Model Context Protocol is one standard way for AI apps to reach tools and data, so a tool built once works in every AI app.
minutes: 6
added: 0.2.0
quiz:
  - question: What problem does MCP solve?
    options:
      - AI models are too slow, and MCP makes them faster
      - Every AI app needed its own connector for every tool; with one protocol, a tool built once works in all of them
      - It replaces HTTP for web APIs
    answer: 1
    explain: Before MCP, connecting 5 AI apps to 10 tools meant up to 50 custom integrations. With MCP, each app speaks MCP once and each tool offers MCP once.
  - question: Which part decides that a tool gets called?
    options:
      - The MCP server, on its own schedule
      - The AI model inside the host app, based on the tool's name, description and input schema
      - The user must type every call by hand
    answer: 1
    explain: The server only offers tools and answers calls. The model reads what the tools say about themselves and chooses one, which is why clear descriptions and schemas matter so much.
  - question: A server runs on your laptop and the AI app starts it as a program. Which transport is that?
    options:
      - stdio, messages over the program's standard input and output
      - Streamable HTTP
      - DNS
    answer: 0
    explain: Local servers are usually programs the app starts and talks to over stdin and stdout. Remote servers use Streamable HTTP (or the older HTTP+SSE) at a URL.
---

An AI model on its own can only write text. To check the weather, read a ticket or query a database, it needs **tools**: small functions it can ask an app to run. The **Model Context Protocol (MCP)** is the open standard for offering those tools to AI apps such as Claude, ChatGPT, Cursor or VS Code.

## The problem it solves

Before MCP, every AI app had its own way to plug in tools. A team with a useful API had to write a separate connector for each app, and each app had to learn each tool. Five apps and ten tools meant up to fifty integrations, all a little different, all breaking in their own ways.

MCP turns that into *one* connection per side: an app speaks MCP once, a tool speaks MCP once, and any app can use any tool.

> [!note] Think of it like…
> A USB-C port. Your laptop doesn't need a special socket for each brand of charger, drive or screen. Anything with the plug works. MCP is that plug for AI apps.

## Who is who

```flow
AI model -> Host app -> MCP client -[JSON-RPC]-> MCP server -> Your API or data
```

- The **host** is the AI app you use (Claude Desktop, an IDE, a coding agent).
- Inside it, an **MCP client** keeps one connection to each **MCP server**.
- The **server** offers what it can do and answers calls. It is often a thin layer over an API you already have.

## What a server offers

| Offers | What it is | Example |
|---|---|---|
| **Tools** | Functions the model may call, each with an input schema (JSON Schema) | `get_forecast(city)` |
| **Resources** | Documents the app can read by URI | `docs://readme`, `users://42` |
| **Prompts** | Message templates a user picks | `code_review(language)` |

## How they talk

MCP messages are **JSON-RPC**: small JSON objects with a `method`, `params` and an `id` that the answer repeats.

```sequence
participants: Client, Server
Client -> Server: initialize (versions, capabilities)
Server --> Client: its name, version and capabilities
Client -> Server: tools/list
Server --> Client: tools with names, descriptions, input schemas
Client -> Server: tools/call get_forecast {city: "Oslo"}
Server --> Client: content: "Sunny, 21 °C"
```

They travel over one of two **transports**:
- **stdio**: the app starts the server as a program on your computer and exchanges lines of JSON on its standard input and output. Most local servers work this way.
- **Streamable HTTP**: the server lives at a URL (`https://…/mcp`); each message is an HTTP POST, and answers come back as JSON or as a stream of events. Older servers use **HTTP+SSE**.

## Why test MCP servers?

A person reads an API's docs and tries again when something is off. A model doesn't: it picks tools from their **descriptions** and fills in arguments from their **schemas**, often with nobody watching. So a vague description, a schema that allows nonsense, an error that says only "failed", or a tool that hangs, turns straight into an agent that does the wrong thing.

Zorvik lets you see exactly what an agent would see: connect to a server, list what it offers, make calls with your own arguments, watch every message, and test the answers like any other API. And when you build an MCP *client*, a mock MCP server lets you test it without the real one.

## You'll use this when…

- you build an MCP server for your product and want to check each tool before an AI app does,
- an agent misbehaves and you need to see what the server really answered,
- you write an AI app or agent and need a server that answers predictably, including failures.

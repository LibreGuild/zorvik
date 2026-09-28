---
title: MCP in Zorvik
description: Test the MCP servers AI apps and agents rely on, mock MCP servers for your AI clients, and let coding agents work in Zorvik, all with the Model Context Protocol.
sidebar:
  order: 1
---

The **Model Context Protocol (MCP)** is how AI apps and agents (Claude, ChatGPT, Cursor, VS Code, coding agents) reach tools and data: an **MCP server** offers **tools** the model can call, **resources** it can read and **prompts** a person can pick, and the app's **MCP client** talks to it over JSON-RPC.

Zorvik works on both sides of that conversation:

| You want to… | Use | See |
|---|---|---|
| Call and test an MCP server: see its tools, call them with your arguments, check the answers in tests and CI | an **MCP call** (request kind `mcp`) | [MCP client](../client/) |
| Give an AI app or your own MCP client a server whose tools and answers you control | an **MCP server** (server kind `mcp`) | [MCP servers](../servers/) |
| Let a coding agent use Zorvik: map APIs, send requests, write tests, run collections | `zorvik mcp` | [AI agents](../../agents/setup/) |

## Why test MCP servers

A person reads an API's documentation and tries again when something looks off. A model picks tools from their **names, descriptions and input schemas**, fills in the arguments itself and acts on the answer, often with nobody watching. So problems a person would shrug off become an agent doing the wrong thing:

- a description that doesn't say when to use the tool, so the model picks the wrong one,
- a schema that allows nonsense, or misses a required argument,
- an error that says "failed" instead of what to fix (tools should answer with `isError` and words the model can act on),
- a result that doesn't match the tool's output schema, or a tool that takes too long.

With Zorvik you see exactly what an AI app would see, make the same calls, watch every JSON-RPC message, and turn the calls into tests that run in the collection runner and in CI with `zorvik run`.

## What Zorvik supports

- **Transports:** Streamable HTTP (with sessions and the server's own event stream), the older HTTP+SSE transport (chosen by itself for servers that need it), and **stdio**: programs Zorvik starts on your computer, such as `npx -y @modelcontextprotocol/server-everything`.
- **Everything a server offers:** tools (with input and output schemas, structured content, images, audio and embedded resources), resources and resource templates, prompts, server instructions and capabilities.
- **Protocol versions** 2025-11-25, 2025-06-18, 2025-03-26 and 2024-11-05.
- **Auth** from the request's Auth tab (bearer tokens, API keys, OAuth 2.0 and the rest) and headers for remote servers; environment variables for programs.
- **Safety:** a workspace can only start a program once you allowed that exact command, folder and environment on your computer; `zorvik run` needs `--allow-programs`; AI agents are asked every time.

:::tip[Learn it hands-on]
The Training Bootcamp's unit **MCP: tools for AI agents** explains why MCP exists and walks you through calling, testing and building MCP servers in labs.
:::

---
id: build-an-mcp-server
title: Build an MCP server for AI apps
summary: Make an MCP server in Zorvik with tools, resources and prompts, to try an idea with a real AI app or to test an MCP client against it.
minutes: 5
added: 0.2.0
lab:
  title: A notes server
  goal: Build an MCP server with a tool and a resource, start it, and let the lab connect to it like an AI app would.
  minutes: 8
  steps:
    - text: |
        Open **Servers** on the left rail, click **＋** and choose **MCP server**. Name it **Notes** and press **Create**. It comes with an example tool, resource and prompt.
      hints:
        - "Servers are saved in the workspace, like requests. The MCP server kind offers tools, resources and prompts."
        - "Servers → ＋ → MCP server → type Notes → Create."
        - "The new server opens in a tab with a get_weather tool, a docs://readme resource and a plan_trip prompt."
      check:
        saved:
          server: { name: Notes, kind: mcp }
      solution:
        - call:
            method: server.create
            params:
              server:
                name: Notes
                kind: mcp
                port: 0
                mcp:
                  tools:
                    - name: get_weather
                      description: The current weather in a city.
                      inputSchema: '{"type": "object", "properties": {"city": {"type": "string"}}, "required": ["city"]}'
                      result: '{"city": "{{args.city}}", "forecast": "sunny"}'
    - text: |
        Make it a notes server. Change the tool's name to `add_note`, its description to `Save a note for later`, and its **Input schema** to:

        ```json
        {"type": "object", "properties": {"text": {"type": "string"}}, "required": ["text"]}
        ```

        Set its **Result** to `Saved: {{args.text}}`. Press **Start**: the lab connects to your server over MCP, lists its tools and calls `add_note`, like an AI app would.
      hints:
        - "{{args.text}} in a result is replaced by the argument the client sent."
        - "Edit the first tool card: name add_note, the schema above, result Saved: {{args.text}}. Then Start (top right)."
        - "Tool add_note, input schema with a required text, result Saved: {{args.text}}, server running. The lab calls it with the text hello."
      check:
        probe:
          kind: mcp
          name: Notes
          method: tools/call
          body: '{"name": "add_note", "arguments": {"text": "hello"}}'
          expect:
            result: { content: [{ text: "Saved: hello" }] }
      solution:
        - call:
            method: server.save
            params:
              id: Notes
              server: &notes2
                name: Notes
                kind: mcp
                port: 0
                mcp:
                  tools:
                    - name: add_note
                      description: Save a note for later
                      inputSchema: '{"type": "object", "properties": {"text": {"type": "string"}}, "required": ["text"]}'
                      result: "Saved: {{args.text}}"
        - call:
            method: server.start
            params: { id: Notes, server: *notes2 }
    - text: |
        Give AI apps something to read. Under **Resources**, set the URI to `notes://today` and the text to `Buy milk`. The running server picks the change up at once and tells connected clients that its list changed.
      hints:
        - "A resource is a URI and its content. Clients list resources and read the ones they need."
        - "Resources section: URI notes://today, text Buy milk. Keep the server running."
        - "The lab reads notes://today from your server and expects Buy milk."
      check:
        probe:
          kind: mcp
          name: Notes
          method: resources/read
          body: '{"uri": "notes://today"}'
          expect:
            result: { contents: [{ text: "*milk*" }] }
      solution:
        - call:
            method: server.save
            params:
              id: Notes
              server:
                name: Notes
                kind: mcp
                port: 0
                mcp:
                  tools:
                    - name: add_note
                      description: Save a note for later
                      inputSchema: '{"type": "object", "properties": {"text": {"type": "string"}}, "required": ["text"]}'
                      result: "Saved: {{args.text}}"
                  resources:
                    - { uri: "notes://today", name: today, text: Buy milk }
    - text: |
        An AI app on your computer can also start the server as a program and talk to it over stdio. Look at the server's **Clients** section: which `zorvik` command does that? Type it here (the part before the workspace path is enough).
      hints:
        - "Local AI apps usually start MCP servers as programs. Zorvik's command line can serve an MCP server that way."
        - "The Clients section shows a command starting with zorvik serve."
        - "zorvik serve <workspace> \"Notes\" --stdio"
      check:
        answer: ["zorvik serve*", "*--stdio*"]
      solution:
        - answer: "zorvik serve"
quiz:
  - question: Why build an MCP server in Zorvik instead of writing code first?
    options:
      - To try tools, descriptions and answers with a real AI app in minutes, and to test an MCP client against predictable answers
      - Because MCP servers can't be written in code
      - To make the AI model faster
    answer: 0
    explain: A mock server lets you shape tools before building them, and gives a client (your app or agent) answers you control, including failures and delays.
  - question: How can a tool's answer use the arguments it was called with?
    options:
      - It can't; answers are fixed
      - "With templates such as {{args.city}}, or {{args}} for all of them as JSON"
      - Only with a script
    answer: 1
    explain: "Results, resource texts and prompt messages are templates. {{args.name}} is one argument; resource templates use {{params.name}}."
  - question: You change a running server's tools. What happens to connected clients?
    options:
      - They must reconnect to see the change
      - The server tells them the tool list changed, and they list it again
      - Nothing, until the server restarts
    answer: 1
    explain: "The server sends notifications/tools/list_changed to clients with an open stream. Zorvik's own MCP client refreshes its Server tab when it gets one."
---

An **MCP server made in Zorvik** answers like a real one, from settings instead of code: tools with input schemas and answers, resources, and prompts. It speaks every transport, so any AI app or MCP client can use it: **Streamable HTTP** at `http://127.0.0.1:<port>/mcp`, the older **HTTP+SSE** at `/sse`, and **stdio** when an app starts it as a program.

## Why mock an MCP server?

- **Try an idea with a real AI app.** Write a tool's description and a sample answer, connect Claude or your IDE, and see whether the model picks the tool and fills the arguments the way you hoped. Change the description and try again, without writing code.
- **Test an MCP client or agent.** Your app talks to a server that answers exactly what you set: the happy path, a failed call (`isError`), a slow tool (a delay), a missing argument.
- **Keep working while the real server is down**, or before it exists.

```flow
AI app or your client -[MCP]-> Zorvik MCP server
Zorvik MCP server -> Traffic panel: every request and answer
```

## What you set

| Part | What it holds |
|---|---|
| **Tools** | name, description, input schema, result (text or JSON), an optional output schema, "answer as a failed call", a delay |
| **Resources** | URI (with `{parts}` for a template), name, MIME type, text |
| **Prompts** | name, arguments, messages (user or assistant) |
| **Introduction** | the name and version clients see, and instructions for the model |
| **Clients** | the endpoint path, CORS for browser-based clients |

Answers are **templates**: `{{args.city}}` is one argument, `{{args}}` all of them as JSON, and `{{params.id}}` a part of a resource template's URI. Dynamic variables (`{{$uuid}}`) and environment variables work too. A missing required argument is answered as a failed call, just as a careful server would.

> [!note] Think of it like…
> A stage set: from the audience (the AI app), it looks like a real room. Behind it there's nothing to build or break, and you can move the walls between scenes.

## Connect an AI app

Most AI apps take an MCP server as a URL or a command. With the server running, give an app its address:

```json
{ "mcpServers": { "notes": { "url": "http://127.0.0.1:3004/mcp" } } }
```

or let the app start it as a program, over stdio:

```json
{ "mcpServers": { "notes": { "command": "zorvik", "args": ["serve", "/path/to/workspace", "Notes", "--stdio"] } } }
```

Every message shows in the server's **Traffic** panel, the same as when a person uses it, so you can see exactly what the model asked for. Change a tool while it runs and connected clients are told the list changed.

## You'll use this when…

- you design tools for an AI feature and want to try the descriptions with a real model first,
- you build an agent and need a server whose answers (and failures) you control,
- a demo or a workshop needs an MCP server that always answers the same way.

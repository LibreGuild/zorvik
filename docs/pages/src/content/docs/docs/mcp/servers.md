---
title: MCP servers
description: Run an MCP server with the tools, resources and prompts you set up, for AI apps and MCP clients over Streamable HTTP, HTTP+SSE or stdio, and watch every call.
sidebar:
  order: 3
---

An **MCP server** in Zorvik answers like a real one, from settings instead of code. Use it to:

- **try tools with a real AI app** before you build them: write a description and a sample answer, connect Claude, Cursor or VS Code, and see whether the model picks the tool and fills in the arguments as you hoped,
- **test an MCP client or an agent** against answers you control, including failed calls and slow tools,
- **stand in for a server** that is down, not built yet, or costs money per call.

## Create one

Open the **Servers** sidebar, click **+** (**New server**) and choose **MCP server**. It listens on `127.0.0.1`, port 3004 (or the next port no other saved server uses), and comes with an example tool, resource and prompt. Press **Start**: the address it shows, such as `http://127.0.0.1:3004/mcp`, is the one to give MCP clients.

It speaks every transport:

| Transport | Address | For |
|---|---|---|
| **Streamable HTTP** | `http://127.0.0.1:3004/mcp` | Current clients. Sessions (`Mcp-Session-Id`), answers as JSON, a `GET` stream for the server's notifications, `DELETE` to end a session. |
| **HTTP+SSE** | `http://127.0.0.1:3004/sse` | Clients of the 2024-11-05 protocol: `GET /sse`, then `POST /messages?sessionId=…` |
| **stdio** | `zorvik serve <workspace> "<server>" --stdio` | AI apps that start servers as programs |

It answers protocol versions 2025-11-25, 2025-06-18, 2025-03-26 and 2024-11-05 (the client's, when it's one of these), and `ping`, logging levels, resource subscriptions and completions (with no suggestions).

## Tools

| Part | In the file | Description |
|---|---|---|
| Name | `name` | What clients call. Unique among the enabled tools. |
| Description | `description` | What the tool does and when to use it: the model reads it to decide. |
| Title | `title` | A display name for people (optional). |
| Input schema | `inputSchema` | JSON Schema of the arguments, as JSON text (empty: any object). A call without a **required** argument is answered as a failed call that names it, like a careful server would. |
| Result | `result` | The answer's text. JSON text is fine. |
| Output schema | `outputSchema` | JSON Schema of a structured result (optional). Then the result must be JSON: it is sent as text and as `structuredContent`. |
| Answer as a failed call | `isError` | Send the result with `isError: true`: the call worked, the tool "couldn't do it". |
| After | `delayMs` | Wait before answering (at most an hour), to test timeouts and progress. |
| On/off | `enabled` | Switched-off tools aren't offered. |

An unknown tool gets a JSON-RPC error (`-32602 Unknown tool`), as the protocol says.

## Resources

| Part | In the file | Description |
|---|---|---|
| URI | `uri` | Such as `docs://readme`. With `{parts}` it's a **template** (`users://{id}`): it's listed under resource templates, and any URI that fits is read, with the parts available as `{{params.id}}`. |
| Name, description | `name`, `description` | What clients list. |
| MIME type | `mimeType` | Default `text/plain`. |
| Text | `text` | The content. |

## Prompts

A prompt has a **name**, a **description**, **arguments** (each with a name, a description and whether it's required) and **messages**: each a **role** (`user` or `assistant`) and a **text**. A missing required argument is answered with a JSON-RPC error.

## Answers are templates

Results, resource texts and prompt messages can use:

| Placeholder | Is |
|---|---|
| `{{args.city}}` | One argument of the call. Text is inserted as it is; numbers, objects and arrays as JSON. |
| `{{args}}` | All the arguments, as JSON. |
| `{{params.id}}` | A part of a resource template's URI. |
| `{{$uuid}}`, `{{$randomFirstName}}`, … | [Dynamic variables](../../variables/dynamic-variables/). |
| `{{token}}` | The active environment's and the workspace's variables. |

The client's values go in after variables are filled in, so a client sending `{{token}}` gets that text back, never the variable's value.

## Introduction and clients

| Setting | In the file | Default | Description |
|---|---|---|---|
| Name clients see | `serverName` | the server's name | `serverInfo.name` in the `initialize` answer |
| Version | `version` | `1.0.0` | `serverInfo.version` |
| Instructions | `instructions` | none | Sent with `initialize`: how to use the server. AI apps pass them to the model. |
| Endpoint path | `path` | `/mcp` | The Streamable HTTP endpoint (not `/sse` or `/messages`, which the older transport uses) |
| CORS | `cors` | off | Answer browsers on other origins, for web-based MCP clients and inspectors |

The server's **Clients** section shows its address and the `zorvik serve … --stdio` command to copy.

## Connect an AI app

Most AI apps take MCP servers in a JSON configuration. Give them the running server's address:

```json
{
  "mcpServers": {
    "weather-mock": { "url": "http://127.0.0.1:3004/mcp" }
  }
}
```

or let them start it as a program over stdio (the `zorvik` command line comes with every download; see [Install](../../getting-started/install/)):

```json
{
  "mcpServers": {
    "weather-mock": {
      "command": "zorvik",
      "args": ["serve", "/path/to/workspace", "Weather mock", "--stdio"]
    }
  }
}
```

Over stdio, only MCP messages go to standard output; the traffic log goes to standard error, as JSON lines with `--json`. See [zorvik serve](../../cli/serve/#stdio).

## Traffic and live changes

Every message shows in the server's **Traffic** panel: sessions opening and closing, each request with a short summary (`tools/call get_weather`, `resources/read docs://readme`) and each answer (`result of tools/call get_weather`); click one for its JSON. Settings that would confuse clients, such as two tools with the same name or a schema that isn't a JSON object, show there as errors.

Edits reach a running server at once. When its tools, resources or prompts change, clients with an open stream are told (`notifications/tools/list_changed`, and the same for resources and prompts) and list them again; Zorvik's own MCP client does.

## Saved format

```yaml title="servers/Weather mock.yaml"
name: Weather mock
kind: mcp
port: 3004
mcp:
  instructions: Use get_weather for the weather in a city.
  tools:
    - name: get_weather
      description: The current weather in a city.
      inputSchema: '{"type": "object", "properties": {"city": {"type": "string"}}, "required": ["city"]}'
      result: '{"city": "{{args.city}}", "forecast": "sunny", "temperatureC": 21}'
    - name: slow_report
      description: A report that takes a while.
      result: Done.
      delayMs: 5000
  resources:
    - uri: docs://readme
      name: readme
      mimeType: text/markdown
      text: "# Weather service"
    - uri: stations://{name}
      name: station
      text: "Station {{params.name}}: 21 °C"
  prompts:
    - name: plan_trip
      description: Plan a day out
      arguments:
        - { name: city, required: true }
      messages:
        - { role: user, text: "Plan a day in {{args.city}} that suits the weather." }
```

AI agents can build, start and read the traffic of MCP servers too (`save_server` with kind `mcp`): see [Agent tools](../../agents/tools/).

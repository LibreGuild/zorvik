---
id: call-an-mcp-server
title: Call an MCP server
summary: Connect to an MCP server, see its tools, resources and prompts, and call each one with your own arguments.
minutes: 5
added: 0.2.0
lab:
  title: The weather desk
  goal: Connect to a weather MCP server, call its tool, read a resource and fetch a prompt, like an AI app would.
  minutes: 8
  servers:
    weather:
      name: Weather tools
      kind: mcp
      mcp:
        instructions: Use get_forecast for the weather in a city. Station readings are resources.
        tools:
          - name: get_forecast
            description: The weather forecast for a city.
            inputSchema: '{"type": "object", "properties": {"city": {"type": "string", "description": "City name"}}, "required": ["city"]}'
            result: '{"city": "{{args.city}}", "forecast": "sunny", "ticket": "{{secret.ticket}}"}'
        resources:
          - { uri: "docs://stations", name: stations, mimeType: text/plain, text: "Stations: Oslo, Lisbon, Nairobi." }
          - { uri: "stations://{name}", name: station reading, text: "Station {{params.name}} reads {{secret.reading}}." }
        prompts:
          - name: packing_list
            description: What to pack for a trip
            arguments: [{ name: city, required: true }]
            messages: [{ role: user, text: "What should I pack for {{args.city}}?" }]
  steps:
    - text: |
        Click **+** at the end of the tab bar and choose **MCP call (AI tools)**. Put `{{weather}}` in the URL bar. In the result pane, press **Connect**, then open its **Server** tab: the server's tools, resources and prompts are listed, as an AI app would see them.
      hints:
        - "The address of an MCP server over HTTP ends with its endpoint, here /mcp. Connect opens a session: the initialize handshake, then the lists."
        - "+ in the tab bar → MCP call (AI tools), paste {{weather}}, then Connect at the top of the result pane."
        - "After Connect, the pane shows Weather tools with its version and transport. Its Server tab lists get_forecast, the stations resources and packing_list."
      check:
        call: { method: mcp.connect, ok: true }
      solution:
        - call: { method: mcp.connect, params: { connId: lab-mcp, path: null, request: { name: Weather, kind: mcp, url: "{{weather}}" } } }
    - text: |
        In the **Server** tab, click **get_forecast**. The **Call** tab now shows its description and arguments, and the arguments start from its input schema. Set `"city"` to `"Oslo"` and press **Send**. The answer contains a `ticket`: type it here.
      hints:
        - "Tools take their arguments as a JSON object. The input schema says city is required and is text."
        - "Click get_forecast in the Server tab, edit the arguments to {\"city\": \"Oslo\"} and press Send (⌘/Ctrl + Enter)."
        - "The Result tab shows the tool's text: {\"city\": \"Oslo\", \"forecast\": \"sunny\", \"ticket\": …}. Copy the ticket value."
      check:
        all:
          - message: { server: weather, summary: tools/call get_forecast }
          - answer: "{{secret.ticket}}"
      solution:
        - send: { kind: mcp, url: "{{weather}}", mcp: { call: tool, name: get_forecast, arguments: '{"city": "Oslo"}' } }
        - answer: "{{secret.ticket}}"
    - text: |
        Resources are read by URI. `stations://{name}` is a **template**: switch the Call tab to **Resource**, pick it, set the arguments to `{"name": "Lisbon"}` and press **Send**. Zorvik fills in the URI (`stations://Lisbon`). Type the reading.
      hints:
        - "A template's {parts} are filled in from the arguments before the resources/read call goes out."
        - "Call → Resource, name stations://{name} (the field suggests it), arguments {\"name\": \"Lisbon\"}, Send."
        - "The Result tab shows the resource's text: Station Lisbon reads …. Type what comes after reads."
      check:
        all:
          - message: { server: weather, summary: "resources/read stations://*" }
          - answer: "{{secret.reading}}"
      solution:
        - send: { kind: mcp, url: "{{weather}}", mcp: { call: resource, name: "stations://{name}", arguments: '{"name": "Lisbon"}' } }
        - answer: "{{secret.reading}}"
    - text: |
        Prompts are templates a person picks in an AI app. Switch to **Prompt**, choose **packing_list**, give it a `city` and press **Send**: the answer is the messages the app would hand to the model. Then open the **Messages** tab to see every JSON-RPC message of your session, both ways.
      hints:
        - "Prompt arguments are text. The answer holds messages with a role and content."
        - "Call → Prompt, name packing_list, arguments {\"city\": \"Nairobi\"}, Send."
        - "The Result tab shows a user message: What should I pack for Nairobi?"
      check:
        message: { server: weather, summary: prompts/get packing_list }
      solution:
        - send: { kind: mcp, url: "{{weather}}", mcp: { call: prompt, name: packing_list, arguments: '{"city": "Nairobi"}' } }
quiz:
  - question: What does an MCP client do right after connecting, before any tool call?
    options:
      - Nothing; it calls tools straight away
      - "initialize: both sides agree on a protocol version and say what they support"
      - It downloads the server's source code
    answer: 1
    explain: Every session starts with initialize. The server answers with its name, version, capabilities and optional instructions; then the client lists what it needs.
  - question: A tool call needs `city`, but the arguments you send are `{}`. What should a well-built server answer?
    options:
      - A result that says the call failed (isError) and names the missing argument, so the model can fix it and try again
      - Nothing; it should wait
      - A random city
    answer: 0
    explain: Tool failures come back as a result with isError, in words the model can act on. That's how agents recover from their own mistakes.
  - question: When is a resource URI a template?
    options:
      - When it starts with https
      - "When it has {parts}, like users://{id}, that are filled in before reading"
      - Never; resources can't take arguments
    answer: 1
    explain: "Templates describe many resources at once. The client fills in the parts, then reads the concrete URI."
---

You don't need an AI app to find out what an MCP server does. Zorvik is an **MCP client** too: it connects to a server, shows what it offers, and makes the same calls an AI app would, with arguments you choose.

## Anatomy of an MCP request

An **MCP call** in Zorvik is a request like any other: saved in your collection, sent with **Send**, tested with scripts.

```anatomy
http://localhost:3000/mcp | the address: a URL, or the command that starts a local server
Call: Tool get_forecast | what to call: a tool, a resource URI or a prompt
{"city": "{{city}}"} | the arguments, as JSON, with variables
```

- **The address** is the server's URL, or, for a server that runs on your computer, the command that starts it (such as `npx -y @modelcontextprotocol/server-everything`). Zorvik picks the transport from it: HTTP for `http(s)://`, a program over stdio otherwise.
- **Call** is what to do: call a **tool**, read a **resource**, or get a **prompt**.
- **Arguments** are JSON. A tool's input schema says which ones it takes.

## A session, or one call

Press **Connect** in the result pane to open a **session**. Zorvik runs the handshake and lists everything the server offers in the **Server** tab. Click a tool there and the Call tab fills in its name, shows its description and arguments, and starts the arguments from its schema, so you don't have to guess.

While the session is open, **Send** calls on it; without one, Send connects, calls and disconnects. That's also what a collection run or `zorvik run` does.

```sequence
participants: Zorvik, Server
Zorvik -> Server: initialize
Server --> Zorvik: Weather tools 1.0.0, tools + resources + prompts
Zorvik -> Server: tools/list, resources/list, prompts/list
Server --> Zorvik: the catalog
Zorvik -> Server: tools/call get_forecast {"city": "Oslo"}
Server --> Zorvik: content, or isError
```

## Reading the answer

- **Result** shows the answer as an AI app would get it: text (JSON in it is pretty-printed), images, embedded resources and `structuredContent`. **JSON** shows the raw result.
- A tool that failed answers with **isError**: a normal result whose text explains what went wrong. A request the server can't handle at all (an unknown tool, bad parameters) gets a **JSON-RPC error** with a code; Zorvik shows it as status 500.
- **Messages** lists every JSON-RPC message of the session, both ways, with the program's own log lines for local servers.

> [!tip] Server instructions
> Some servers send **instructions** when you connect: hints for the model on how to use them. You'll find them at the top of the Server tab.

## You'll use this when…

- you want to know what a server really offers before you plug it into an AI app,
- an agent says a tool failed and you want to make the same call yourself,
- you need a reproducible call to put in a test or a bug report.

---
id: test-mcp-servers
title: Test an MCP server
summary: Check that tools fail helpfully, that results match their output schema, and run the checks as a collection, in CI too.
minutes: 6
added: 0.2.0
lab:
  title: Currency checks
  goal: Find how a currency tool fails, test its structured result, and run a folder of MCP checks.
  minutes: 8
  servers:
    currency:
      name: Currency tools
      kind: mcp
      mcp:
        tools:
          - name: convert
            description: Convert an amount of money from one currency to another.
            inputSchema: '{"type": "object", "properties": {"amount": {"type": "number"}, "from": {"type": "string"}, "to": {"type": "string"}}, "required": ["amount", "from", "to"]}'
            outputSchema: '{"type": "object", "properties": {"amount": {"type": "number"}, "currency": {"type": "string"}}, "required": ["amount", "currency"]}'
            result: '{"amount": 1150.5, "currency": "{{args.to}}"}'
  files:
    requests/MCP checks/_folder.yaml: |
      name: MCP checks
      seq: 93
    requests/MCP checks/Convert.yaml: |
      name: Convert
      seq: 1
      kind: mcp
      url: {{currency}}
      mcp:
        name: convert
        arguments: '{"amount": 100, "from": "EUR", "to": "NOK"}'
      scripts:
        postResponse: |
          const answer = pm.response.json();
          pm.test("The call worked", () => pm.expect(answer.isError).to.not.eql(true));
          pm.test("Structured result in NOK", () => pm.expect(answer.structuredContent.currency).to.eql("NOK"));
    requests/MCP checks/Missing argument.yaml: |
      name: Missing argument
      seq: 2
      kind: mcp
      url: {{currency}}
      mcp:
        name: convert
        arguments: '{"amount": 100}'
      scripts:
        postResponse: |
          const answer = pm.response.json();
          pm.test("Fails as a tool result", () => pm.expect(answer.isError).to.eql(true));
          pm.test("Says what is missing", () => pm.expect(answer.content[0].text).to.include("from"));
    requests/MCP checks/Unknown tool.yaml: |
      name: Unknown tool
      seq: 3
      kind: mcp
      url: {{currency}}
      mcp:
        name: convert_all
      scripts:
        postResponse: |
          pm.test("A JSON-RPC error", () => {
            pm.response.to.have.status(500);
            pm.expect(pm.response.json().error.code).to.eql(-32602);
          });
  steps:
    - text: |
        Open a new **MCP call (AI tools)** to `{{currency}}` and call the tool `convert` with only `{"amount": 100, "from": "EUR"}`. Look at the Result: the tool answers, but as a **failed call** (isError) whose text names what is missing.
      hints:
        - "Type convert in the Tool field (or Connect and pick it in the Server tab), then set the arguments."
        - "Arguments {\"amount\": 100, \"from\": \"EUR\"}, then Send. The Result shows a red note: the tool reported that it failed."
        - "The result is {\"content\": [{\"type\": \"text\", \"text\": \"Missing required argument: to\"}], \"isError\": true}."
      check:
        send: { kind: mcp, json: { isError: true } }
      solution:
        - send: { kind: mcp, url: "{{currency}}", mcp: { call: tool, name: convert, arguments: '{"amount": 100, "from": "EUR"}' } }
    - text: |
        Add `"to": "NOK"` and a test. In **Scripts → Post-response**, write:

        ```js
        pm.test("Structured result in NOK", () => {
          pm.expect(pm.response.json().structuredContent.currency).to.eql("NOK");
        });
        ```

        Press **Send**. The tool has an **output schema**, so its answer also comes as `structuredContent`, JSON a program (or your test) can read without parsing text.
      hints:
        - "For MCP calls, pm.response.json() is the call's result: content, structuredContent and isError."
        - "Arguments {\"amount\": 100, \"from\": \"EUR\", \"to\": \"NOK\"}, paste the test in Scripts → Post-response, Send."
        - "The Tests tab next to Result shows Structured result in NOK passing."
      check:
        send: { kind: mcp, testsPassed: ">=1", testsFailed: 0, json: { structuredContent: { currency: NOK } } }
      solution:
        - send:
            kind: mcp
            url: "{{currency}}"
            mcp: { call: tool, name: convert, arguments: '{"amount": 100, "from": "EUR", "to": "NOK"}' }
            scripts:
              postResponse: |
                pm.test("Structured result in NOK", () => {
                  pm.expect(pm.response.json().structuredContent.currency).to.eql("NOK");
                });
    - text: |
        The lab added a folder **MCP checks** with three calls and their tests: a good call, a missing argument, and a tool that doesn't exist. Right-click the folder → **Run…** and press **Run**. All tests should pass.
      hints:
        - "A collection run makes each MCP call in one go (connect, call, disconnect) and runs its tests, like zorvik run does in CI."
        - "Right-click MCP checks in the sidebar, choose Run…, then Run."
        - "Three requests, six tests, all green: the server fails the way it should."
      check:
        run: { name: MCP checks, requests: 3, failed: 0, testsFailed: 0 }
      solution:
        - call: { method: runner.start, params: { folder: MCP checks } }
        - wait: 800
quiz:
  - question: A tool gets arguments it can't use. How should it answer?
    options:
      - With a result marked isError whose text explains the problem, so the model can correct itself
      - By closing the connection
      - With an empty result
    answer: 0
    explain: "isError results reach the model like any answer, so it can read what went wrong and try again. Protocol errors (like an unknown tool) are for mistakes in the conversation itself."
  - question: What is `structuredContent` for?
    options:
      - Pictures
      - "JSON that matches the tool's output schema, for programs and tests to read without parsing text"
      - The tool's description
    answer: 1
    explain: A tool with an output schema sends its result both as text (for the model) and as structured JSON that must match the schema.
  - question: How do you run MCP checks on every change in CI?
    options:
      - It can't be done; MCP needs an AI app
      - "zorvik run on the folder; add --allow-programs when the server is a program the run starts"
      - By opening Zorvik on the CI machine
    answer: 1
    explain: "zorvik run makes each MCP call and runs its tests like any request. Starting programs from a workspace is only allowed with --allow-programs, because the workspace could come from anyone."
---

Testing an MCP server is testing an API whose most important user is a model. It won't read your docs, it can't ask a colleague, and it will happily try again with slightly different arguments. So a good MCP test asks: **does each tool succeed with good input, fail helpfully with bad input, and answer in the shape it promised?**

## Two ways to fail

MCP has two kinds of failure, and they mean different things:

| | Looks like | Means | The model… |
|---|---|---|---|
| **Tool error** | a result with `isError: true` and text | the tool ran and couldn't do it (missing argument, not found, rate limited) | reads the text and can fix its call |
| **Protocol error** | a JSON-RPC `error` with a `code` | the request itself is wrong (unknown tool, bad parameters) | usually can't recover |

Zorvik shows a protocol error as status **500** (with the code and message in the body) and a tool error as status **200** with a red note, so tests can tell them apart.

> [!note] Think of it like…
> A shop assistant saying "we're out of size 42, would 43 do?" (a tool error the customer can act on) versus a locked door (a protocol error).

## Test what the model depends on

- **Good input works**: the call succeeds and the text says what a model needs.
- **Bad input fails in words**: a missing or wrong argument gives `isError` and a message that names the problem.
- **The shape holds**: when a tool declares an **output schema**, its answer carries `structuredContent` that matches it.
- **It's quick**: slow tools make agents time out. Each call's time shows next to its status.

For MCP calls, `pm.response.json()` in a post-response script is the call's **result** (`content`, `structuredContent`, `isError`) or, for a protocol error, `{"error": {code, message}}`:

```js
const answer = pm.response.json();
pm.test("Fails as a tool result", () => pm.expect(answer.isError).to.eql(true));
pm.test("Names the problem", () => pm.expect(answer.content[0].text).to.include("to"));
```

## Run them everywhere

Save your calls in a folder and run it like any collection: right-click → **Run…**, or in CI:

```bash
zorvik run . --folder "MCP checks"
zorvik run . --folder "Local MCP" --allow-programs   # when the server is a program
```

Each call connects, makes its call, disconnects and runs its tests. `--allow-programs` is needed when a request starts a program (a local stdio server): the workspace comes from Git, so Zorvik never starts programs from it unless you say so.

## You'll use this when…

- you ship an MCP server and want CI to catch a broken tool before users' agents do,
- a model keeps calling a tool wrong, and you want to check what the tool's errors actually say,
- you change a tool's schema and need to be sure old calls still fail helpfully.

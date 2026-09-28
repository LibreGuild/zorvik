import { randomUUID } from "node:crypto";
import fs from "node:fs";
import http from "node:http";
import { createRequire } from "node:module";
import os from "node:os";
import path from "node:path";
import { expect, type Page, test } from "@playwright/test";

// MCP both ways against the official SDK (@modelcontextprotocol/sdk): Zorvik's MCP client with
// SDK servers over Streamable HTTP and over stdio (a program Zorvik starts once the user allows
// it), and the SDK's client with an MCP server made in Zorvik.
const require = createRequire(import.meta.url);
const { Client } = require("@modelcontextprotocol/sdk/client/index.js");
const { StreamableHTTPClientTransport } = require("@modelcontextprotocol/sdk/client/streamableHttp.js");
const { Server } = require("@modelcontextprotocol/sdk/server/index.js");
const { StreamableHTTPServerTransport } = require("@modelcontextprotocol/sdk/server/streamableHttp.js");
const types = require("@modelcontextprotocol/sdk/types.js");

const SDK_SERVER_PORT = 18931;
const MOCK_PORT = 18932;

test.beforeEach(async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("workspace-menu")).toContainText("E2E Workspace");
});

/** An SDK server with one tool (add), one resource and one prompt. */
function sdkServer() {
  const server = new Server({ name: "sdk-calculator", version: "1.2.3" }, { capabilities: { tools: {}, resources: {}, prompts: {} } });
  server.setRequestHandler(types.ListToolsRequestSchema, async () => ({
    tools: [
      {
        name: "add",
        description: "Add two numbers",
        inputSchema: { type: "object", properties: { a: { type: "number" }, b: { type: "number" } }, required: ["a", "b"] },
      },
    ],
  }));
  server.setRequestHandler(types.CallToolRequestSchema, async (request: { params: { name: string; arguments?: Record<string, number> } }) => {
    const { a = 0, b = 0 } = request.params.arguments ?? {};
    return { content: [{ type: "text", text: `The sum is ${a + b}` }] };
  });
  server.setRequestHandler(types.ListResourcesRequestSchema, async () => ({ resources: [{ uri: "note://hello", name: "hello", mimeType: "text/plain" }] }));
  server.setRequestHandler(types.ReadResourceRequestSchema, async (request: { params: { uri: string } }) => ({
    contents: [{ uri: request.params.uri, mimeType: "text/plain", text: "Hello from the SDK" }],
  }));
  server.setRequestHandler(types.ListPromptsRequestSchema, async () => ({ prompts: [{ name: "greet", arguments: [{ name: "who", required: true }] }] }));
  return server;
}

/** The SDK's Streamable HTTP server with sessions, on `port`. */
async function startSdkHttpServer(port: number): Promise<http.Server> {
  const sessions: Record<string, InstanceType<typeof StreamableHTTPServerTransport>> = {};
  const httpServer = http.createServer(async (req, res) => {
    const chunks: Buffer[] = [];
    for await (const c of req) chunks.push(c as Buffer);
    const body = chunks.length ? JSON.parse(Buffer.concat(chunks).toString()) : undefined;
    const id = req.headers["mcp-session-id"] as string | undefined;
    let transport = id ? sessions[id] : undefined;
    if (!transport && req.method === "POST" && types.isInitializeRequest(body)) {
      transport = new StreamableHTTPServerTransport({ sessionIdGenerator: () => randomUUID(), onsessioninitialized: (sid: string) => (sessions[sid] = transport!) });
      await sdkServer().connect(transport);
    }
    if (!transport) {
      res.writeHead(404).end();
      return;
    }
    await transport.handleRequest(req, res, body);
  });
  await new Promise<void>((resolve) => httpServer.listen(port, "127.0.0.1", resolve));
  return httpServer;
}

async function newMcpCall(page: Page, address: string) {
  await page.getByRole("button", { name: "New tab" }).click();
  await page.getByRole("menuitem", { name: "MCP call (AI tools)" }).click();
  await page.getByLabel("URL", { exact: true }).fill(address);
}

async function setArguments(page: Page, json: string) {
  await page.getByTestId("mcp-arguments").locator(".cm-content").click();
  await page.keyboard.press("ControlOrMeta+a");
  await page.keyboard.insertText(json);
}

test("the MCP client connects to an SDK server, lists what it offers and calls a tool", async ({ page }) => {
  const httpServer = await startSdkHttpServer(SDK_SERVER_PORT);
  try {
    await newMcpCall(page, `http://127.0.0.1:${SDK_SERVER_PORT}/mcp`);
    const session = page.getByTestId("mcp-session");
    await session.getByRole("button", { name: "Connect" }).click();
    await expect(session).toContainText("sdk-calculator");
    await expect(session).toContainText("Streamable HTTP");

    // Pick the tool from the catalog: its arguments start from the input schema.
    await page.getByRole("tab", { name: "Server" }).click();
    const catalog = page.getByTestId("mcp-catalog");
    await expect(catalog).toContainText("note://hello");
    await expect(catalog).toContainText("greet");
    await catalog.getByTestId("mcp-item-tool").filter({ hasText: "add" }).click();
    await expect(page.getByTestId("mcp-selected")).toContainText("Add two numbers");
    await expect(page.getByTestId("mcp-arguments")).toContainText('"a": 0');

    await setArguments(page, '{"a": 2, "b": 40}');
    await page.getByRole("button", { name: "Send", exact: true }).click();
    await page.getByRole("tab", { name: "Result" }).click();
    await expect(page.getByTestId("mcp-text")).toHaveText("The sum is 42");

    // A resource, on the same session.
    await page.getByRole("tab", { name: "Server" }).click();
    await catalog.getByTestId("mcp-item-resource").filter({ hasText: "note://hello" }).click();
    await page.getByRole("button", { name: "Send", exact: true }).click();
    await page.getByRole("tab", { name: "Result" }).click();
    await expect(page.getByTestId("mcp-result")).toContainText("Hello from the SDK");

    // Every message, both ways.
    await page.getByRole("tab", { name: /Messages/ }).click();
    const messages = page.getByTestId("mcp-messages");
    await expect(messages).toContainText("initialize");
    await expect(messages).toContainText("tools/call");
    await expect(messages).toContainText("answer to resources/read");

    await session.getByRole("button", { name: "Disconnect" }).click();
    await expect(session).toContainText("Not connected");
  } finally {
    httpServer.close();
  }
});

test("a program is started over stdio only once the user allows it", async ({ page }) => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "zorvik-mcp-"));
  const script = path.join(dir, "server.cjs");
  const resolve = (p: string) => JSON.stringify(require.resolve(p));
  fs.writeFileSync(
    script,
    `const { Server } = require(${resolve("@modelcontextprotocol/sdk/server/index.js")});
const { StdioServerTransport } = require(${resolve("@modelcontextprotocol/sdk/server/stdio.js")});
const types = require(${resolve("@modelcontextprotocol/sdk/types.js")});
const server = new Server({ name: "sdk-stdio", version: "0.1.0" }, { capabilities: { tools: {} } });
server.setRequestHandler(types.ListToolsRequestSchema, async () => ({ tools: [{ name: "shout", inputSchema: { type: "object", properties: { text: { type: "string" } } } }] }));
server.setRequestHandler(types.CallToolRequestSchema, async (r) => ({ content: [{ type: "text", text: String(r.params.arguments.text).toUpperCase() }] }));
console.error("sdk-stdio ready");
server.connect(new StdioServerTransport());
`,
  );
  try {
    await newMcpCall(page, `node "${script}"`);
    await page.getByRole("tab", { name: "Call", exact: true }).click();
    await page.getByLabel("Tool", { exact: true }).fill("shout");
    await setArguments(page, '{"text": "hello"}');

    // Sending asks first; cancelling sends nothing.
    await page.getByRole("button", { name: "Send", exact: true }).click();
    const dialog = page.getByRole("dialog", { name: "Start this program?" });
    await expect(dialog.getByTestId("confirm-details")).toContainText(`Command: node "${script}"`);
    await dialog.getByRole("button", { name: "Cancel" }).click();
    await expect(page.getByTestId("mcp-pane")).toContainText("starts a program");

    // Allowed: it starts (and isn't asked about again).
    await page.getByRole("button", { name: "Send", exact: true }).click();
    await page.getByRole("dialog", { name: "Start this program?" }).getByRole("button", { name: "Allow and start" }).click();
    await expect(page.getByTestId("mcp-text")).toHaveText("HELLO");

    const session = page.getByTestId("mcp-session");
    await session.getByRole("button", { name: "Connect" }).click();
    await expect(session).toContainText("sdk-stdio");
    await expect(session).toContainText("stdio");
    await page.getByRole("tab", { name: /Messages/ }).click();
    await expect(page.getByTestId("mcp-messages")).toContainText("sdk-stdio ready");
    await session.getByRole("button", { name: "Disconnect" }).click();
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("the SDK's client uses an MCP server made in Zorvik", async ({ page }) => {
  await page.getByTestId("rail-servers").click();
  await page.getByRole("button", { name: "New server" }).first().click();
  await page.getByRole("menuitem", { name: "MCP server" }).click();
  await page.getByRole("textbox").last().fill("E2E MCP");
  await page.getByRole("button", { name: "Create" }).click();
  await page.getByLabel("Port").fill(String(MOCK_PORT));
  await page.keyboard.press("ControlOrMeta+s");
  await page.getByTestId("server-start").click();
  await expect(page.getByTestId("server-status")).toContainText(`http://127.0.0.1:${MOCK_PORT}/mcp`);

  const client = new Client({ name: "sdk-e2e", version: "1.0.0" });
  await client.connect(new StreamableHTTPClientTransport(new URL(`http://127.0.0.1:${MOCK_PORT}/mcp`)));
  try {
    expect(client.getServerVersion()).toMatchObject({ name: "E2E MCP" });
    const { tools } = await client.listTools();
    expect(tools.map((t: { name: string }) => t.name)).toEqual(["get_weather"]);
    const result = await client.callTool({ name: "get_weather", arguments: { city: "Lisbon" } });
    expect(JSON.parse(result.content[0].text)).toMatchObject({ city: "Lisbon", forecast: "sunny" });
    const missing = await client.callTool({ name: "get_weather", arguments: {} });
    expect(missing.isError).toBe(true);
    const readme = await client.readResource({ uri: "docs://readme" });
    expect(readme.contents[0].text).toContain("Weather service");
    const prompt = await client.getPrompt({ name: "plan_trip", arguments: { city: "Lisbon" } });
    expect(prompt.messages[0].content.text).toBe("Plan a day in Lisbon that suits the weather.");

    const log = page.getByTestId("traffic-log");
    await expect(log).toContainText("tools/call get_weather");
    await expect(log).toContainText("prompts/get plan_trip");
  } finally {
    await client.close();
    await page.getByTestId("server-stop").click();
  }
});

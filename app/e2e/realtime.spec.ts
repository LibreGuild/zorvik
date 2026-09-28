import { createRequire } from "node:module";
import { expect, test } from "@playwright/test";
import type { Server as IoServer } from "socket.io";
import type { io as IoClient } from "socket.io-client";

// Loaded as CommonJS: Playwright's ESM loader breaks engine.io's imports.
const require = createRequire(import.meta.url);
const { Server } = require("socket.io") as { Server: typeof IoServer };
const { io: connectIo } = require("socket.io-client") as { io: typeof IoClient };

// GraphQL subscriptions against the test servers, and Socket.IO both ways against the
// official socket.io libraries: Zorvik's client with a socket.io server, socket.io-client
// (default transports: long-polling, then the upgrade to WebSocket) with Zorvik's server.
const HTTP = "http://127.0.0.1:18787";
const IO_SERVER_PORT = 18921;
const MOCK_PORT = 18922;

test.beforeEach(async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("workspace-menu")).toContainText("E2E Workspace");
});

async function newGraphql(page: import("@playwright/test").Page, url: string, query: string, variables: string) {
  await page.getByRole("button", { name: "New tab" }).click();
  await page.getByRole("menuitem", { name: "GraphQL request" }).click();
  await page.getByLabel("URL", { exact: true }).fill(url);
  await page.getByTestId("graphql-query").locator(".cm-content").click();
  await page.keyboard.insertText(query);
  await page.getByTestId("graphql-variables").locator(".cm-content").click();
  await page.keyboard.insertText(variables);
}

test("a GraphQL subscription streams its results over WebSocket and SSE", async ({ page }) => {
  await newGraphql(page, `${HTTP}/graphql-ws`, "subscription Ticks { tick }", '{"count": 2}');
  await page.getByRole("button", { name: "Subscribe", exact: true }).click();
  const log = page.getByTestId("message-log");
  await expect(log).toContainText('{"data":{"tick":2}}');
  await expect(log).toContainText("The server completed the subscription");
  await expect(page.getByTestId("stream-status")).toHaveText("Disconnected");

  // The same over Server-Sent Events.
  await page.getByLabel("URL", { exact: true }).fill(`${HTTP}/graphql-sse`);
  await page.getByRole("button", { name: "Subscription", exact: true }).click();
  await page.getByTestId("graphql-subscription").getByRole("button", { name: "SSE", exact: true }).click();
  await page.getByRole("button", { name: "Clear messages" }).click();
  await page.getByRole("button", { name: "Subscribe", exact: true }).click();
  await expect(log).toContainText('{"data":{"tick":2}}');
  await expect(log).toContainText("GraphQL over SSE");

  // A query is plain HTTP again.
  await page.getByTestId("graphql-query").locator(".cm-content").click();
  await page.keyboard.press("ControlOrMeta+a");
  await page.keyboard.insertText("{ hello }");
  await expect(page.getByRole("button", { name: "Send", exact: true })).toBeVisible();
});

test("the Socket.IO client talks to a socket.io server", async ({ page }) => {
  const server = new Server(IO_SERVER_PORT);
  server.of("/chat").use((socket, next) => (socket.handshake.auth.token === "e2e" ? next() : next(new Error("Not authorized"))));
  server.of("/chat").on("connection", (socket) => {
    socket.emit("hello", { room: socket.handshake.query.room });
    socket.on("say", (text: string, ack?: (answer: unknown) => void) => ack?.({ heard: text }));
  });
  try {
    await page.getByRole("button", { name: "New tab" }).click();
    await page.getByRole("menuitem", { name: "Socket.IO client" }).click();
    await page.getByLabel("URL", { exact: true }).fill(`http://127.0.0.1:${IO_SERVER_PORT}/chat?room=lobby`);
    await page.getByTestId("socketio-auth").locator(".cm-content").click();
    await page.keyboard.insertText('{"token": "e2e"}');
    await page.getByRole("button", { name: "Connect", exact: true }).click();
    await expect(page.getByTestId("stream-status")).toHaveText("Connected");
    const log = page.getByTestId("message-log");
    await expect(log).toContainText('[{"room":"lobby"}]');

    await page.getByLabel("Event", { exact: true }).fill("say");
    await page.getByRole("checkbox", { name: "Ask for acknowledgement" }).click();
    await page.locator('[data-testid="stream-pane"] .cm-content').click();
    await page.keyboard.insertText('"hi there"');
    await page.getByRole("button", { name: "Emit", exact: true }).click();
    await expect(log).toContainText("ack #1");
    await expect(log).toContainText('[{"heard":"hi there"}]');
  } finally {
    server.close();
  }
});

test("socket.io-client talks to a Socket.IO server made in Zorvik", async ({ page }) => {
  await page.getByTestId("rail-servers").click();
  await page.getByRole("button", { name: "New server" }).first().click();
  await page.getByRole("menuitem", { name: "Socket.IO server" }).click();
  await page.getByRole("textbox").last().fill("E2E socket.io");
  await page.getByRole("button", { name: "Create" }).click();
  await page.getByLabel("Port").fill(String(MOCK_PORT));
  await page.keyboard.press("ControlOrMeta+s");
  await page.getByTestId("server-start").click();
  await expect(page.getByTestId("server-status")).toContainText(`http://127.0.0.1:${MOCK_PORT}`);

  // Default transports: long-polling first, then the upgrade to WebSocket.
  const client = connectIo(`http://127.0.0.1:${MOCK_PORT}/news`, { reconnection: false, auth: { token: "t" } });
  try {
    const welcome = await new Promise<unknown>((resolve, reject) => {
      client.on("welcome", resolve);
      client.on("connect_error", reject);
    });
    expect(welcome).toHaveProperty("id");
    const ack = await client.timeout(5000).emitWithAck("chat", "hello", { n: 1 });
    expect(ack).toBe("hello");
    await expect.poll(() => client.io.engine.transport.name).toBe("websocket");

    const log = page.getByTestId("traffic-log");
    await expect(log).toContainText("joined /news");
    await expect(log).toContainText("/news chat (asks for acknowledgement #0)");

    // An emit from the traffic panel reaches the client.
    const news = new Promise((resolve) => client.on("breaking", (...args: unknown[]) => resolve(args)));
    const composer = page.getByTestId("server-composer");
    await composer.getByLabel("Event name").fill("breaking");
    await composer.getByLabel("Namespace").fill("/news");
    await composer.getByLabel("Message to send").fill('[1, "two"]');
    await composer.getByRole("button", { name: "Emit" }).click();
    expect(await news).toEqual([1, "two"]);
  } finally {
    client.close();
    await page.getByTestId("server-stop").click();
  }
});

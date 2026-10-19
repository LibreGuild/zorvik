import { expect, test, type Page } from "@playwright/test";

// WebSocket workflows against the test server's echo (`welcome` first, then everything back;
// "close" makes it close with 4000) and against a WebSocket server made in Zorvik.
const WS = "ws://127.0.0.1:18787";
const MOCK_PORT = 18931;

test.beforeEach(async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("workspace-menu")).toContainText("E2E Workspace");
});

async function newWebSocket(page: Page, url: string) {
  await pick(page, "New tab", "WebSocket");
  await page.getByLabel("URL", { exact: true }).fill(url);
}

/** Open a menu and pick an item (again if the click landed while the last menu was closing). */
async function pick(page: Page, button: string, item: string) {
  await expect(async () => {
    await page.getByRole("button", { name: button, exact: true }).click();
    await page.getByRole("menuitem", { name: item }).click({ timeout: 1000 });
  }).toPass();
}

/** Type into the composer (replacing what's there) and send. */
async function compose(page: Page, text: string) {
  const editor = page.getByTestId("composer").locator(".cm-content");
  await editor.click();
  await page.keyboard.press("ControlOrMeta+a");
  await page.keyboard.insertText(text);
  await page.getByTestId("composer").getByRole("button", { name: "Send", exact: true }).click();
}

test("saved messages and on-connect steps, reply times, repeating sends, ping and close codes", async ({ page }) => {
  await newWebSocket(page, `${WS}/ws`);

  // A saved message.
  await page.getByRole("tab", { name: "Messages" }).click();
  await page.getByRole("button", { name: "Add message" }).click();
  const card = page.getByTestId("ws-message");
  await card.getByLabel("Message name").fill("Ask");
  await card.getByRole("button", { name: "JSON", exact: true }).click();
  await card.locator(".cm-content").click();
  await page.keyboard.insertText('{"id": 7, "q": "hi"}');

  // Steps: wait for the greeting, send it, wait for the answer.
  await page.getByRole("tab", { name: "On connect" }).click();
  await pick(page, "Add step", "Wait for a message");
  const steps = page.getByTestId("ws-step");
  await steps.nth(0).getByLabel("Match").selectOption("exact");
  await steps.nth(0).getByLabel("Pattern").fill("welcome");
  await pick(page, "Add step", "Send a saved message");
  await pick(page, "Add step", "Wait for a message");
  await steps.nth(2).getByLabel("Match").selectOption("json");
  await steps.nth(2).getByLabel("Pattern").fill("$.id == 7");
  await expect(page.getByRole("tab", { name: /On connect/ })).toContainText("3");

  await page.getByRole("button", { name: "Connect", exact: true }).click();
  const log = page.getByTestId("message-log");
  await expect(log).toContainText("On connect: sent Ask");
  await expect(log).toContainText("On connect: got a JSON message where $.id == 7 after");
  // The echo has the same id: its reply time shows, and jumps to what it answers.
  await expect(page.getByTestId("reply-time")).toHaveCount(1);

  await compose(page, '{"id": 9}');
  await expect(page.getByTestId("reply-time")).toHaveCount(2);

  // Only sent messages; then a JSON filter.
  await pick(page, "View", "Sent only");
  await expect(log.getByText('{"id": 9}')).toHaveCount(1);
  await pick(page, "View", "All messages");
  await page.getByLabel("Filter messages").fill("$.id == 9");
  await expect(log.getByText('{"id": 9}')).toHaveCount(2);
  await page.getByLabel("Filter messages").fill("");

  // Save the composer's message, then send it every second.
  await pick(page, "Saved messages", "Save as message…");
  await page.getByRole("dialog").getByRole("textbox").fill("Nine");
  await page.getByRole("dialog").getByRole("button", { name: "Save" }).click();
  await expect(page.getByRole("tab", { name: /Messages/ })).toContainText("2");
  await pick(page, "More ways to send", "Send every second");
  await expect(page.getByTestId("repeating")).toContainText("Sending every 1 s");
  await expect(log.getByText('{"id": 9}')).toHaveCount(6, { timeout: 5000 });
  await page.getByTestId("composer").getByRole("button", { name: "Stop", exact: true }).click();
  await expect(page.getByTestId("repeating")).toHaveCount(0);

  await pick(page, "More ways to send", "Send a ping");
  await expect(log.getByText("pong", { exact: true })).toBeVisible();

  await page.getByRole("button", { name: "Disconnect" }).click();
  await expect(page.getByTestId("stream-status")).toHaveText("Disconnected");
});

test("a WebSocket mock answers JSON with the same id, sends on a timer and closes by rule", async ({ page }) => {
  await page.getByTestId("rail-servers").click();
  await page.getByRole("button", { name: "New server" }).first().click();
  await page.getByRole("menuitem", { name: "WebSocket server" }).click();
  await page.getByRole("textbox").last().fill("E2E ws mock");
  await page.getByRole("button", { name: "Create" }).click();
  await page.getByLabel("Port").fill(String(MOCK_PORT));
  await page.getByRole("button", { name: "Rules", exact: true }).click();
  await page.getByRole("button", { name: "Add rule" }).click();
  await page.getByRole("button", { name: "Add rule" }).click();
  const rules = page.getByRole("table", { name: "Reply rules" }).getByRole("row");
  await rules.nth(1).getByLabel("Match").selectOption("json");
  await rules.nth(1).getByLabel("Pattern").fill('$.op == "bye"');
  await rules.nth(1).getByLabel("Close code").fill("4001");
  await rules.nth(2).getByLabel("Match").selectOption("json");
  await rules.nth(2).getByLabel("Pattern").fill("$.id");
  await rules.nth(2).getByLabel("Reply").fill('{"id": {{message.id}}, "ok": true}');
  await page.getByRole("button", { name: "Add scheduled message" }).click();
  await page.getByLabel("Scheduled message", { exact: true }).fill("tick");
  await page.getByTestId("ws-schedule").getByLabel("Every (ms)").fill("300");
  await page.keyboard.press("ControlOrMeta+s");
  await page.getByTestId("server-start").click();
  await expect(page.getByTestId("server-status")).toContainText(`ws://127.0.0.1:${MOCK_PORT}`);

  await newWebSocket(page, `ws://127.0.0.1:${MOCK_PORT}/`);
  await page.getByRole("button", { name: "Connect", exact: true }).click();
  const log = page.getByTestId("message-log");
  await expect(log.getByText("tick", { exact: true }).nth(1)).toBeVisible();
  await compose(page, '{"id": 5, "q": 1}');
  await expect(log).toContainText('{"id": 5, "ok": true}');
  await expect(page.getByTestId("reply-time")).toHaveCount(1);
  await compose(page, '{"op": "bye"}');
  await expect(log).toContainText("Disconnected by server (code 4001 · Defined by the application)");

  // Closing with a code of our own: the server sees the code and reason.
  await page.getByRole("button", { name: "Connect", exact: true }).click();
  await expect(page.getByTestId("stream-status")).toHaveText("Connected");
  await pick(page, "More actions", "Close with a code…");
  await page.getByRole("dialog").getByRole("textbox").fill("4002 done here");
  await page.getByRole("dialog").getByRole("button", { name: "Close connection" }).click();
  await expect(log).toContainText("code 4002 · done here · Defined by the application");

  await page.getByTestId("rail-servers").click();
  await page.getByTestId("server-row-E2E ws mock").click();
  await expect(page.getByTestId("traffic-log")).toContainText("closed by the client (4002 done here)");
  await page.getByTestId("server-stop").click();
});

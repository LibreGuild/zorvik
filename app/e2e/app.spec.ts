import { expect, test, type Page } from "@playwright/test";

const HTTP = "http://127.0.0.1:18787";
const WS = "ws://127.0.0.1:18787";

async function newTab(page: Page, kind: "HTTP request" | "WebSocket" | "Event stream (SSE)") {
  await page.getByRole("button", { name: "New tab" }).click();
  await page.getByRole("menuitem", { name: kind }).click();
}

async function setUrl(page: Page, url: string) {
  const input = page.getByLabel("URL", { exact: true });
  await input.fill(url);
}

test.beforeEach(async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("workspace-menu")).toContainText("E2E Workspace");
});

test("sends an HTTP request and shows status, body and timing", async ({ page }) => {
  await newTab(page, "HTTP request");
  await setUrl(page, `${HTTP}/json`);
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await expect(page.getByTestId("response-status")).toHaveText("200 OK");
  await expect(page.getByTestId("response")).toContainText('"name": "Zorvik"');
  await page.getByRole("tab", { name: "Timing" }).click();
  await expect(page.getByText("DNS lookup")).toBeVisible();
  await page.getByRole("tab", { name: /^Headers/ }).last().click();
  await expect(page.getByTestId("response")).toContainText("Content-Type");
});

test("query params table stays in sync with the URL", async ({ page }) => {
  await newTab(page, "HTTP request");
  await setUrl(page, `${HTTP}/echo?alpha=1&beta=two`);
  await expect(page.getByPlaceholder("Parameter").first()).toHaveValue("alpha");
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await expect(page.getByTestId("response")).toContainText('"beta": "two"');
});

test("shows a helpful error when the server is down", async ({ page }) => {
  await newTab(page, "HTTP request");
  await setUrl(page, "http://127.0.0.1:1/");
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await expect(page.getByText("Could not connect", { exact: true })).toBeVisible();
});

test("environment variables are substituted", async ({ page }) => {
  await page.getByTestId("env-picker").click();
  await page.getByRole("menuitem", { name: "Manage environments…" }).click();
  await page.getByRole("button", { name: "New environment" }).click();
  await page.getByLabel("Environment name").fill("Local");
  await page.getByPlaceholder("Variable").last().fill("base");
  await page.getByPlaceholder("Value").first().fill(HTTP);
  await page.getByRole("button", { name: "Save changes" }).click();
  await page.getByRole("button", { name: "Close", exact: true }).last().click();
  await page.getByTestId("env-picker").click();
  await page.getByRole("menuitem", { name: /Local/ }).click();
  await expect(page.getByTestId("env-picker")).toContainText("Local");

  await newTab(page, "HTTP request");
  await setUrl(page, "{{base}}/echo?from=env");
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await expect(page.getByTestId("response-status")).toHaveText("200 OK");
  await expect(page.getByTestId("response")).toContainText('"from": "env"');
});

test("variable suggestions stay within the field, long examples cut short", async ({ page }) => {
  await newTab(page, "HTTP request");
  const input = page.getByLabel("URL", { exact: true });
  await input.pressSequentially("{{$randomLoremP");
  const item = page.getByRole("button", { name: /\$randomLoremParagraphs/ });
  await expect(item).toBeVisible();
  const field = (await input.boundingBox())!;
  const menu = (await item.locator("..").boundingBox())!;
  expect(menu.x + menu.width).toBeLessThanOrEqual(field.x + field.width + 1);
});

test("saves a request into the collection", async ({ page }) => {
  await newTab(page, "HTTP request");
  await setUrl(page, `${HTTP}/status/204`);
  await page.getByRole("button", { name: /^Save/ }).first().click();
  await page.getByLabel("Name").fill("Saved status check");
  await page.getByRole("button", { name: "Save", exact: true }).click();
  await expect(page.getByTestId("collection-tree")).toContainText("Saved status check");
});

test("WebSocket connects, sends and receives", async ({ page }) => {
  await newTab(page, "WebSocket");
  await setUrl(page, `${WS}/ws`);
  await page.getByRole("button", { name: "Connect" }).click();
  await expect(page.getByTestId("stream-status")).toHaveText("Connected");
  await expect(page.getByTestId("message-log")).toContainText("welcome");
  await page.getByTestId("stream-pane").locator(".cm-content").click();
  await page.keyboard.type("hello from e2e");
  await page.getByTestId("stream-pane").getByRole("button", { name: "Send" }).click();
  await expect(page.getByTestId("message-log").getByText("hello from e2e")).toHaveCount(2);
  await page.getByRole("button", { name: "Disconnect" }).click();
  await expect(page.getByTestId("stream-status")).toHaveText("Disconnected");
});

test("Server-Sent Events are listed", async ({ page }) => {
  await newTab(page, "Event stream (SSE)");
  await setUrl(page, `${HTTP}/sse?count=3&interval=20`);
  await page.getByRole("button", { name: "Connect" }).click();
  await expect(page.getByTestId("message-log")).toContainText("line one 1");
  await expect(page.getByTestId("stream-status")).toHaveText("Disconnected");
});

test("imports a cURL command", async ({ page }) => {
  await page.getByRole("button", { name: "New", exact: true }).click();
  await page.getByRole("menuitem", { name: "Import…" }).click();
  await page.getByRole("tab", { name: "cURL" }).click();
  await page.locator(".cm-content").last().click();
  await page.keyboard.insertText(`curl -X PUT '${HTTP}/echo' -H 'X-Imported: yes' --data-raw '{"ok":true}'`);
  await page.getByRole("button", { name: "Open as new request" }).click();
  await expect(page.getByLabel("HTTP method")).toHaveText(/PUT/);
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await expect(page.getByTestId("response")).toContainText("x-imported");
});

test("history lists sent requests", async ({ page }) => {
  await newTab(page, "HTTP request");
  await setUrl(page, `${HTTP}/echo?history=1`);
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await expect(page.getByTestId("response-status")).toHaveText("200 OK");
  await page.getByRole("button", { name: "History" }).click();
  await expect(page.getByText("127.0.0.1:18787/echo?history=1").first()).toBeVisible();
});

test("drag and drop moves a request into a folder, and Move to… works", async ({ page }) => {
  const tree = page.getByTestId("collection-tree");
  await page.getByRole("button", { name: "New", exact: true }).click();
  await page.getByRole("menuitem", { name: "New folder" }).click();
  await page.getByRole("textbox").last().fill("Target folder");
  await page.getByRole("button", { name: "Create" }).click();
  await page.getByRole("button", { name: "New", exact: true }).click();
  await page.getByRole("menuitem", { name: "New HTTP request" }).click();
  await page.getByRole("textbox").last().fill("Dragged request");
  await page.getByRole("button", { name: "Create" }).click();
  await expect(tree.getByText("Dragged request")).toBeVisible();

  await tree.getByText("Dragged request").dragTo(tree.getByText("Target folder"), { targetPosition: { x: 20, y: 18 } });
  await expect(tree.getByRole("treeitem", { name: /Target folder/ })).toHaveAttribute("aria-expanded", "true");
  const folder = tree.locator("div", { has: page.getByRole("treeitem", { name: /Target folder/ }) }).first();
  await expect(folder).toContainText("Dragged request");

  await tree.getByText("Dragged request").click({ button: "right" });
  await page.getByRole("menuitem", { name: "Move to…" }).click();
  await page.getByRole("combobox").selectOption("");
  await page.getByRole("button", { name: "Move", exact: true }).click();
  await expect(tree.getByRole("treeitem", { name: /Dragged request/ })).toBeVisible();
});

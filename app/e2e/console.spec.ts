import { expect, test, type Page } from "@playwright/test";

const HTTP = "http://127.0.0.1:18787";

async function newHttpTab(page: Page, url: string) {
  await page.getByRole("button", { name: "New tab" }).click();
  await page.getByRole("menuitem", { name: "HTTP request" }).click();
  await page.getByLabel("URL", { exact: true }).fill(url);
}

async function writeScript(page: Page, which: "Pre-request" | "Post-response", code: string) {
  await page.getByRole("tab", { name: /^Scripts/ }).click();
  const editor = page.getByTestId("scripts-editor");
  await editor.getByRole("button", { name: which }).click();
  await editor.locator(".cm-content").click();
  await page.keyboard.press("ControlOrMeta+a");
  await page.keyboard.insertText(code);
}

async function sendRequest(page: Page) {
  await page.getByRole("button", { name: "Send", exact: true }).click();
}

test.beforeEach(async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("workspace-menu")).toContainText("E2E Workspace");
  // The app keeps the console while it runs: start each test with an empty one.
  await page.getByTestId("rail-console").click();
  const panel = page.getByTestId("console-panel");
  await expect(panel).toBeVisible();
  const clear = panel.getByRole("button", { name: "Clear the console" });
  if (await clear.isEnabled()) await clear.click();
  await expect(panel).toContainText("Nothing sent yet");
});

test("a request shows in the console with what it sent and what came back", async ({ page }) => {
  const panel = page.getByTestId("console-panel");
  await newHttpTab(page, `${HTTP}/echo?probe=console`);
  await writeScript(page, "Pre-request", "console.log('about to send');");
  await writeScript(page, "Post-response", "console.warn('answered', pm.response.code);");
  await sendRequest(page);
  await expect(page.getByTestId("response-status")).toHaveText("200 OK");

  // In order: the script's line, the request, the script's line.
  await expect(panel.getByTestId("console-count")).toHaveText("3 lines");
  const rows = panel.getByTestId("console-lines").locator("> div");
  await expect(rows.nth(0)).toContainText("about to send");
  await expect(rows.nth(1)).toContainText(`${HTTP}/echo?probe=console`);
  await expect(rows.nth(1)).toContainText("200");
  await expect(rows.nth(2)).toContainText("answered 200");

  // Opened: the headers that went out and the body that came back.
  await rows.nth(1).getByRole("button").first().click();
  const details = panel.getByTestId("console-details");
  await expect(details).toContainText("Request headers");
  await expect(details).toContainText("User-Agent");
  await expect(details).toContainText("Response headers");
  await expect(details).toContainText("Response body");
  await expect(details).toContainText('"probe": "console"');
  await expect(details).toContainText("Address");

  // Filters.
  await panel.getByRole("button", { name: "Logs", exact: true }).click();
  await expect(panel.getByTestId("console-request")).toHaveCount(0);
  await expect(panel.getByTestId("console-log")).toHaveCount(2);
  await panel.getByRole("button", { name: "All", exact: true }).click();
  await panel.getByLabel("Filter the console").fill("probe=console");
  await expect(panel.getByTestId("console-count")).toHaveText("1 of 3");
  await panel.getByLabel("Filter the console").fill("");

  // It is still there after the window reloads: the app keeps it, not the page.
  await page.reload();
  await expect(page.getByTestId("console-panel").getByTestId("console-count")).toHaveText("3 lines");
});

test("a request that fails says why, and the rail counts failures while the console is closed", async ({ page }) => {
  const panel = page.getByTestId("console-panel");
  await panel.getByRole("button", { name: "Close the console" }).click();
  await expect(panel).toBeHidden();

  // A port nothing listens on.
  await newHttpTab(page, "http://127.0.0.1:9/down");
  await page.getByRole("tab", { name: /^Headers/ }).click();
  await sendRequest(page);
  await expect(page.getByTestId("rail-console-errors")).toHaveText("1");

  // The keyboard opens it; the count is seen.
  await page.keyboard.press("ControlOrMeta+j");
  await expect(panel).toBeVisible();
  await expect(page.getByTestId("rail-console-errors")).toHaveCount(0);
  const row = panel.getByTestId("console-request");
  await expect(row).toContainText("http://127.0.0.1:9/down");
  await expect(row).toContainText("ERROR");
  await panel.getByRole("button", { name: "Errors", exact: true }).click();
  await expect(panel.getByTestId("console-count")).toHaveText("1 line");
  await row.getByRole("button").first().click();
  await expect(panel.getByTestId("console-details")).toContainText("No response.");
  await expect(panel.getByTestId("console-details")).toContainText("as prepared");

  await panel.getByRole("button", { name: "Clear the console" }).click();
  await expect(panel).toContainText("Nothing sent yet");
});

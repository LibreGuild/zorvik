// Docs (Markdown with Preview / Edit) and saved examples (indented JSON, editable status
// and headers, Beautify).
import { expect, test, type Page } from "@playwright/test";

const HTTP = "http://127.0.0.1:18787";

test.beforeEach(async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("workspace-menu")).toContainText("E2E Workspace");
});

async function newHttpTab(page: Page) {
  await page.getByRole("button", { name: "New tab" }).click();
  await page.getByRole("menuitem", { name: "HTTP request" }).click();
}

test("request docs show as Markdown, and Edit changes them", async ({ page }) => {
  await newHttpTab(page);
  await page.getByRole("tab", { name: "Docs" }).click();
  const docs = page.getByTestId("docs-editor");
  // Empty docs open for typing, with the cursor in the editor.
  await expect(docs.getByRole("button", { name: "Edit" })).toHaveAttribute("aria-pressed", "true");
  await page.keyboard.type("# Users\n\nLists **every** user, see [the guide](https://example.com/guide).\n\n- one\n- two\n\n<script>window.hacked = 1</script>");

  await docs.getByRole("button", { name: "Preview" }).click();
  const preview = docs.getByTestId("docs-preview");
  await expect(preview.getByRole("heading", { name: "Users" })).toBeVisible();
  await expect(preview.locator("strong")).toHaveText("every");
  await expect(preview.getByRole("link", { name: "the guide" })).toHaveAttribute("href", "https://example.com/guide");
  await expect(preview.locator("li")).toHaveCount(2);
  // HTML stays text.
  await expect(preview.locator("script")).toHaveCount(0);
  await expect(preview).toContainText("<script>window.hacked = 1</script>");
  expect(await page.evaluate(() => (window as { hacked?: number }).hacked)).toBeUndefined();

  // Back to the docs later: they open in Preview.
  await page.getByRole("tab", { name: /^Params/ }).click();
  await page.getByRole("tab", { name: "Docs" }).click();
  await expect(preview.getByRole("heading", { name: "Users" })).toBeVisible();

  // A double-click (or Enter on the preview) edits them.
  await preview.getByRole("heading", { name: "Users" }).dblclick();
  await expect(docs.locator(".cm-content")).toContainText("# Users");
  await docs.getByRole("button", { name: "Preview" }).click();
  await preview.focus();
  await page.keyboard.press("Enter");
  await expect(docs.locator(".cm-content")).toBeFocused();
});

test("a saved example keeps JSON indented, and its status, headers and body can change", async ({ page }) => {
  await newHttpTab(page);
  await page.getByLabel("URL", { exact: true }).fill(`${HTTP}/json`);
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await expect(page.getByTestId("response-status")).toHaveText("200 OK");
  await page.getByRole("button", { name: /^Save/ }).first().click();
  await page.getByLabel("Name").fill("Examples to edit");
  await page.getByRole("button", { name: "Save", exact: true }).click();
  await expect(page.getByTestId("collection-tree")).toContainText("Examples to edit");
  await page.getByTestId("save-example").click();
  await expect(page.getByText("Saved as an example")).toBeVisible();
  await page.getByRole("tab", { name: /^Examples/ }).click();

  const examples = page.getByTestId("examples");
  await expect(examples).toContainText('"name": "Zorvik"');

  // The status takes any code from 100 to 599, typed as usual.
  const status = examples.getByLabel("Status");
  await status.fill("404");
  await expect(status).toHaveValue("404");
  await status.fill("99");
  await expect(status).toHaveAttribute("aria-invalid", "true");
  await status.blur();
  await expect(status).toHaveValue("404");

  // Headers: add one, switch it off.
  await examples.getByRole("button", { name: /^Headers/ }).click();
  const headers = examples.getByTestId("example-headers");
  await headers.getByPlaceholder("Header").last().fill("X-Mock");
  await headers.getByPlaceholder("Value").nth(1).fill("yes");
  await expect(examples.getByRole("button", { name: "Headers (2)" })).toBeVisible();
  await headers.getByTitle("Disable").nth(1).click();
  await expect(examples.getByRole("button", { name: "Headers (1)" })).toBeVisible();

  // Beautify indents a JSON body and keeps its numbers.
  await examples.getByRole("button", { name: "Body", exact: true }).click();
  const body = examples.locator(".cm-content");
  await body.click();
  await page.keyboard.press("ControlOrMeta+a");
  await page.keyboard.insertText('{"big":12345678901234567890,"list":[1,2]}');
  await examples.getByRole("button", { name: "Beautify" }).click();
  await expect(body).toContainText('"big": 12345678901234567890,');
  await expect(body.locator(".cm-line")).toHaveCount(7);

  await body.click();
  await page.keyboard.press("ControlOrMeta+a");
  await page.keyboard.insertText('{"broken": ');
  await examples.getByRole("button", { name: "Beautify" }).click();
  await expect(page.getByText("Body is not valid JSON")).toBeVisible();
});

import fs from "node:fs";
import path from "node:path";
import { expect, test, type Page } from "@playwright/test";

// Collection runner against the local test server, with a data file in the E2E workspace.
const HTTP = "http://127.0.0.1:18787";
const WORKSPACE = process.env.ZV_E2E_WORKSPACE!;

test.beforeEach(async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("workspace-menu")).toContainText("E2E Workspace");
});

/** A saved request in `folder` (right-click menu) with a URL and a post-response script. */
async function requestIn(page: Page, folder: string, name: string, url: string, post = "") {
  await page.getByTestId("collection-tree").getByRole("treeitem", { name: new RegExp(folder) }).click({ button: "right" });
  await page.getByRole("menuitem", { name: "New HTTP request" }).click();
  await page.getByRole("textbox").last().fill(name);
  await page.getByRole("button", { name: "Create" }).click();
  // Wait for the new request's tab (an empty URL), or the URL lands in the previous tab.
  await expect(page.getByRole("tab", { name: new RegExp(name), selected: true })).toBeVisible();
  await expect(page.getByLabel("URL", { exact: true })).toHaveValue("");
  await page.getByLabel("URL", { exact: true }).fill(url);
  if (post) {
    await page.getByRole("tab", { name: /^Scripts/ }).click();
    const editor = page.getByTestId("scripts-editor");
    await editor.getByRole("button", { name: "Post-response" }).click();
    await editor.locator(".cm-content").click();
    await page.keyboard.press("ControlOrMeta+a");
    await page.keyboard.insertText(post);
  }
  await page.keyboard.press("ControlOrMeta+s");
  await expect(page.getByTestId("collection-tree")).toContainText(name);
}

test("a folder runs with a data file: results per iteration, filter, details, export", async ({ page }) => {
  test.setTimeout(90_000);
  fs.writeFileSync(path.join(WORKSPACE, "runner-users.csv"), "user\r\nada\r\n\"gr,ace\"\r\n");

  await page.getByRole("button", { name: "New", exact: true }).click();
  await page.getByRole("menuitem", { name: "New folder" }).click();
  await page.getByRole("textbox").last().fill("E2E runner");
  await page.getByRole("button", { name: "Create" }).click();
  await requestIn(
    page,
    "E2E runner",
    "Runner echo",
    `${HTTP}/echo?user={{user}}`,
    "pm.test('user echoed', () => pm.expect(pm.response.json().args.user).to.equal(pm.iterationData.get('user')));",
  );
  await requestIn(page, "E2E runner", "Runner missing", `${HTTP}/status/404`);

  // "Run…" on the folder opens a runner tab with its requests in order.
  await page.getByTestId("collection-tree").getByRole("treeitem", { name: /E2E runner/ }).click({ button: "right" });
  await page.getByRole("menuitem", { name: "Run…" }).click();
  const view = page.getByTestId("runner-view");
  await expect(view).toBeVisible();
  await expect(page.getByTestId("runner-request")).toHaveCount(2);
  await expect(page.getByTestId("runner-request").first()).toContainText("Runner echo");

  // The data file (the browser build asks for the path in a prompt), with a preview.
  page.once("dialog", (d) => void d.accept(path.join(WORKSPACE, "runner-users.csv")));
  await page.getByRole("button", { name: "Choose file…" }).click();
  await expect(page.getByTestId("runner-data")).toContainText("runner-users.csv");
  await expect(page.getByTestId("runner-data")).toContainText("CSV · 2 rows");
  await expect(page.getByTestId("runner-data-preview")).toContainText("gr,ace");
  await expect(page.getByTestId("runner-status")).toContainText("2 requests × 2 iterations");

  // Run: the 404 without tests fails.
  await page.getByTestId("runner-start").click();
  await expect(page.getByText("“E2E runner” failed")).toBeVisible({ timeout: 20_000 });
  await expect(page.getByTestId("runner-iteration")).toHaveCount(2);
  await expect(page.getByTestId("runner-result")).toHaveCount(4);
  await expect(page.getByTestId("runner-summary")).toContainText("2 passed · 2 failed");
  await expect(page.getByTestId("runner-summary")).toContainText("Tests 2/2");

  // Only failed results; a row opens to its tests and URL.
  const results = page.getByTestId("runner-results");
  await results.getByRole("button", { name: "Failed", exact: true }).click();
  await expect(page.getByTestId("runner-result")).toHaveCount(2);
  await results.getByRole("button", { name: "All", exact: true }).click();
  await page.getByTestId("runner-result").first().getByRole("button").first().click();
  await expect(page.getByTestId("runner-result-details").first()).toContainText("user echoed");
  await expect(page.getByTestId("runner-result-details").first()).toContainText("/echo?user=ada");

  // Leave the 404 out and run again with Mod+Enter: everything passes.
  await page.getByRole("checkbox", { name: "Run Runner missing" }).click();
  await page.keyboard.press("ControlOrMeta+Enter");
  await expect(page.getByText("“E2E runner” passed")).toBeVisible({ timeout: 20_000 });
  await expect(page.getByTestId("runner-result")).toHaveCount(2);

  // Export as JUnit XML (the browser build asks for the path in a prompt).
  const out = path.join(WORKSPACE, "runner-report.xml");
  page.once("dialog", (d) => void d.accept(out));
  await page.getByRole("button", { name: "Export run" }).click();
  await page.getByRole("menuitem", { name: "JUnit XML…" }).click();
  await expect(page.getByText("JUnit report saved")).toBeVisible();
  expect(fs.readFileSync(out, "utf8")).toContain('<testcase name="user echoed (iteration 2)"');
});

test("the command palette opens the runner for the whole collection", async ({ page }) => {
  await page.keyboard.press("ControlOrMeta+k");
  await page.getByPlaceholder(/Search requests/).fill("run collection");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("runner-view")).toBeVisible();
  await expect(page.getByTestId("runner-view")).toContainText("E2E Workspace");
  await expect(page.getByRole("tab", { selected: true })).toContainText("E2E Workspace");
});

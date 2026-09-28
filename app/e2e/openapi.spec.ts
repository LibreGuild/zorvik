import { expect, test, type Page } from "@playwright/test";

const HTTP = "http://127.0.0.1:18787";

/** An OpenAPI document for the test server's `/json`; version 2 adds an operation, drops one and requires a field. */
function spec(version: 1 | 2): string {
  const required = version === 1 ? ["id", "name"] : ["id", "name", "missing"];
  const paths: Record<string, unknown> = {
    "/json": {
      get: {
        summary: "Get sample",
        responses: {
          "200": {
            content: {
              "application/json": {
                schema: { type: "object", required, properties: { id: { type: "integer" }, name: { type: "string" } } },
              },
            },
          },
        },
      },
    },
  };
  if (version === 1) paths["/old"] = { get: { summary: "Old thing", responses: { "200": { description: "ok" } } } };
  else paths["/status/{code}"] = { get: { summary: "Status code", responses: { "204": { description: "empty" } } } };
  // A relative server: the import asks where the API runs.
  return JSON.stringify({ openapi: "3.0.0", info: { title: "Spec E2E", version: String(version) }, servers: [{ url: "/" }], paths });
}

/** Drop `text` as a file on the import dialog's drop zone. */
async function dropFile(page: Page, name: string, text: string) {
  const data = await page.evaluateHandle(
    ([n, t]) => {
      const dt = new DataTransfer();
      dt.items.add(new File([t], n, { type: "application/json" }));
      return dt;
    },
    [name, text] as const,
  );
  await page.getByRole("button", { name: /Choose a file, or drop it here/ }).dispatchEvent("drop", { dataTransfer: data });
}

test.beforeEach(async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("workspace-menu")).toContainText("E2E Workspace");
});

test("OpenAPI import asks for the base URL, checks responses and updates from a new version", async ({ page }) => {
  const tree = page.getByTestId("collection-tree");
  await page.getByRole("button", { name: "New", exact: true }).click();
  await page.getByRole("menuitem", { name: "Import…" }).click();
  await dropFile(page, "spec.json", spec(1));
  // No full server URL: the dialog asks for it, then imports.
  const ask = page.getByTestId("import-base-url");
  await expect(ask).toContainText("base URL");
  await ask.getByLabel("Base URL").fill(HTTP);
  await page.getByRole("button", { name: "Import", exact: true }).click();
  await expect(page.getByText("Imported “Spec E2E”")).toBeVisible();
  await page.getByRole("button", { name: "Done" }).click();

  // The imported request's response is checked against the spec.
  const folder = tree.getByRole("treeitem", { name: /Spec E2E/ });
  if ((await folder.getAttribute("aria-expanded")) !== "true") await folder.click();
  await page.getByTestId("env-picker").click();
  await page.getByRole("menu").getByText("Spec E2E", { exact: true }).click();
  await tree.getByText("Get sample").click();
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await expect(page.getByTestId("response-status")).toHaveText("200 OK");
  await page.getByRole("tab", { name: /Tests/ }).click();
  await expect(page.getByText("Matches the API spec (GET /json → 200)")).toBeVisible();
  await expect(page.getByRole("tab", { name: /Tests 1\/1/ })).toBeVisible();

  // A new version of the spec: preview, then update.
  await tree.getByRole("treeitem", { name: /Spec E2E/ }).click({ button: "right" });
  await page.getByRole("menuitem", { name: "Update from API spec…" }).click();
  await page.getByRole("tab", { name: "Paste" }).click();
  await page.getByRole("dialog").locator(".cm-content").click();
  await page.keyboard.insertText(spec(2));
  await page.getByTestId("spec-update-preview").click();
  const plan = page.getByTestId("spec-update-plan");
  await expect(plan).toContainText("1 added");
  await expect(plan).toContainText("GET /status/{code}");
  await expect(plan).toContainText("GET /old");
  await page.getByTestId("spec-update-apply").click();
  await expect(plan).toContainText("Updated.");
  await page.getByRole("button", { name: "Done" }).click();
  await expect(tree.getByText("Status code")).toBeVisible();
  // Kept, but crossed out.
  await expect(tree.getByText("Old thing")).toHaveClass(/line-through/);

  // The response no longer matches the new version.
  await tree.getByText("Get sample").click();
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await expect(page.getByRole("tab", { name: /Tests 0\/1/ })).toBeVisible();
});

test("a long menu fits a short window and scrolls", async ({ page }) => {
  await page.setViewportSize({ width: 1200, height: 420 });
  const tree = page.getByTestId("collection-tree");
  await page.getByRole("button", { name: "New", exact: true }).click();
  await page.getByRole("menuitem", { name: "New folder" }).click();
  await page.getByRole("textbox").last().fill("Menu folder");
  await page.getByRole("button", { name: "Create" }).click();
  await tree.getByText("Menu folder").click({ button: "right" });
  const menu = page.getByRole("menu");
  await expect(menu).toBeVisible();
  const box = await menu.boundingBox();
  expect(box!.y + box!.height).toBeLessThanOrEqual(420);
  const scrolls = await menu.evaluate((el) => el.scrollHeight > el.clientHeight);
  expect(scrolls).toBe(true);
  // The last item can be reached.
  const remove = page.getByRole("menuitem", { name: "Delete" });
  await remove.scrollIntoViewIfNeeded();
  await expect(remove).toBeInViewport();
  await page.keyboard.press("Escape");
});

test("copies a request as code", async ({ page }) => {
  await page.getByRole("button", { name: "New tab" }).click();
  await page.getByRole("menuitem", { name: "HTTP request" }).click();
  await page.getByLabel("URL", { exact: true }).fill(`${HTTP}/echo?q=1`);
  await page.getByRole("button", { name: "More actions" }).click();
  await page.getByRole("menuitem", { name: "Copy as cURL or code…" }).click();
  const dialog = page.getByRole("dialog", { name: "Copy as cURL or code" });
  const code = dialog.getByTestId("export-code").locator(".cm-content");
  await expect(code).toContainText("curl");
  await dialog.getByRole("option", { name: "Kotlin" }).click();
  await expect(code).toContainText("OkHttpClient");
  await dialog.getByRole("option", { name: "Python" }).click();
  await expect(code).toContainText("requests.request");
  await dialog.getByRole("button", { name: "HTTPX", exact: true }).click();
  await expect(code).toContainText("httpx.request");
  // Found by platform; Up and Down pick from the search box.
  await dialog.getByLabel("Search languages").fill("flutter");
  await expect(dialog.getByRole("option")).toHaveCount(1);
  await page.keyboard.press("ArrowDown");
  await expect(code).toContainText("package:http");
});

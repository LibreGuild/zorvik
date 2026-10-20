import { expect, test, type Locator, type Page, type Route } from "@playwright/test";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

// The API catalog in the Import window. Anthropic (a description written for Zorvik) and
// GitHub GraphQL (one request) import without the network; downloads are answered here.

/** Call the API the way the UI does (dev bridge). */
async function rpc(page: Page, method: string, params: unknown = {}) {
  const res = await page.request.post("/bridge/rpc", { data: { method, params }, headers: { "x-zorvik-bridge": "1" } });
  return ((await res.json()) as { result?: unknown }).result;
}

/** Answer the RPC methods in `answers` here (recording their params); everything else goes to the API. */
async function answer(page: Page, answers: Record<string, (params: Record<string, unknown>) => unknown | Promise<unknown>>) {
  const calls: { method: string; params: Record<string, unknown> }[] = [];
  await page.route("**/bridge/rpc", async (route: Route) => {
    const { method, params } = route.request().postDataJSON() as { method: string; params: Record<string, unknown> };
    const reply = answers[method];
    if (!reply) return route.continue();
    calls.push({ method, params });
    await route.fulfill({ json: await reply(params) });
  });
  return calls;
}

/** A reply held back until `release` is called. */
function held() {
  let release!: () => void;
  const gate = new Promise<void>((resolve) => (release = resolve));
  return { gate, release };
}

/** Drop a file with `text` on `target`. */
async function dropText(page: Page, target: Locator, name: string, text: string) {
  const data = await page.evaluateHandle(
    ([file, content]) => {
      const dt = new DataTransfer();
      dt.items.add(new File([content], file, { type: "application/json" }));
      return dt;
    },
    [name, text],
  );
  await target.dispatchEvent("drop", { dataTransfer: data });
}

async function openImport(page: Page) {
  await page.getByRole("button", { name: "New", exact: true }).click();
  await page.getByRole("menuitem", { name: "Import…" }).click();
  const dialog = page.getByRole("dialog", { name: "Import" });
  // The window opens on File; the catalog is the last tab.
  await expect(dialog.getByRole("tab", { name: "File" })).toHaveAttribute("aria-selected", "true");
  const tabs = dialog.getByRole("tab");
  await expect(tabs.last()).toHaveText("API catalog");
  await dialog.getByRole("tab", { name: "API catalog" }).click();
  return dialog;
}

test.beforeEach(async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("workspace-menu")).toContainText("E2E Workspace");
});

// Later tests expect the E2E workspace, with no environment active.
test.afterEach(async ({ page }) => {
  await rpc(page, "workspace.open", { path: process.env.ZV_E2E_WORKSPACE });
  await rpc(page, "env.setActive", { id: null });
});

test("imports Anthropic from the API catalog: folder, first request and active environment", async ({ page }) => {
  const dialog = await openImport(page);
  await dialog.getByLabel("Search APIs").fill("anthropic");
  await expect(dialog.getByRole("listbox", { name: "APIs" }).getByRole("option", { name: /Anthropic/ })).toHaveAttribute("aria-selected", "true");
  const detail = dialog.getByTestId("catalog-detail");
  await expect(detail.getByRole("heading", { name: "Anthropic" })).toBeVisible();
  await expect(detail).toContainText("API key in the x-api-key header");
  await expect(detail).toContainText("Written for Zorvik");
  await detail.getByRole("button", { name: "Import", exact: true }).click();

  await expect(dialog.getByText("Imported “Anthropic”")).toBeVisible();
  await expect(dialog).toContainText(/The environment “Anthropic.*” is now active/);
  await expect(page.getByTestId("env-picker")).toContainText("Anthropic");

  // "Add your API key" opens the new environment.
  await dialog.getByRole("button", { name: "Add your API key" }).click();
  const environments = page.getByRole("dialog", { name: "Environments & variables" });
  await expect(environments.getByLabel("Environment name")).toHaveValue(/^Anthropic/);
  await environments.getByRole("button", { name: "Close", exact: true }).last().click();
  await expect(environments).toHaveCount(0);

  // Its first request is open, and shown in the sidebar with the folders above it open.
  const tree = page.getByTestId("collection-tree");
  await expect(page.getByRole("tab", { name: /Create a message/ })).toHaveAttribute("aria-selected", "true");
  await expect(tree.getByRole("treeitem", { name: /Create a message/, selected: true })).toBeVisible();
  await expect(page.getByLabel("URL", { exact: true })).toHaveValue(/\/v1\/messages$/);
});

test("an API that can't be imported says why and links to its docs", async ({ page, context }) => {
  await context.route("https://developers.hubspot.com/**", (route) => route.fulfill({ body: "HubSpot docs" }));
  const dialog = await openImport(page);
  await dialog.getByLabel("Search APIs").fill("hubspot");
  const option = dialog.getByRole("listbox", { name: "APIs" }).getByRole("option", { name: /HubSpot/ });
  await expect(option).toContainText("Link only");
  await option.click();
  const detail = dialog.getByTestId("catalog-detail");
  await expect(detail.getByTestId("catalog-notice")).toContainText("not licensed");
  await expect(detail.getByRole("button", { name: "Import", exact: true })).toHaveCount(0);
  const popup = page.waitForEvent("popup");
  await detail.getByRole("button", { name: "Open API docs" }).click();
  await expect(await popup).toHaveURL(/developers\.hubspot\.com/);
});

test("categories, search and the keyboard pick an API", async ({ page }) => {
  const dialog = await openImport(page);
  const list = dialog.getByRole("listbox", { name: "APIs" });
  const options = list.getByRole("option");
  await dialog.getByRole("button", { name: /^Developer tools/ }).click();
  await expect(dialog.getByRole("button", { name: /^Developer tools/ })).toHaveAttribute("aria-pressed", "true");
  await expect(list.getByRole("option", { name: /^GitHub GraphQL/ })).toBeVisible();
  await expect(list.getByRole("option", { name: /^Anthropic/ })).toHaveCount(0);

  await dialog.getByRole("button", { name: /^All/ }).click();
  const search = dialog.getByLabel("Search APIs");
  await search.fill("github");
  // Names that start with the search come first.
  await expect(options.first()).toHaveAttribute("aria-selected", "true");
  await expect(options.first()).toContainText("GitHub");
  const detail = dialog.getByTestId("catalog-detail");
  // Down in the search box moves through the list; the details follow.
  const second = (await options.nth(1).locator("span").first().innerText()).trim();
  await search.press("ArrowDown");
  await expect(options.nth(1)).toHaveAttribute("aria-selected", "true");
  await expect(detail.getByRole("heading", { name: second, exact: true })).toBeVisible();
  // And in the list itself.
  await options.nth(1).press("ArrowUp");
  await expect(options.first()).toBeFocused();
  await expect(options.first()).toHaveAttribute("aria-selected", "true");
  await options.first().press("End");
  await expect(options.last()).toBeFocused();

  await search.fill("github graphql");
  await expect(detail.getByRole("heading", { name: "GitHub GraphQL" })).toBeVisible();
  await expect(detail).toContainText("One GraphQL request; the editor completes from the schema");

  await search.fill("no api is called this");
  await expect(options).toHaveCount(0);
  await expect(dialog).toContainText("No API matches “no api is called this”");
});

test("Import into folder… puts the API in that folder", async ({ page }) => {
  const tree = page.getByTestId("collection-tree");
  const target = `Catalog target ${Date.now()}`;
  await page.getByRole("button", { name: "New", exact: true }).click();
  await page.getByRole("menuitem", { name: "New folder" }).click();
  await page.getByRole("textbox").last().fill(target);
  await page.getByRole("button", { name: "Create" }).click();
  await tree.getByText(target).click({ button: "right" });
  await page.getByRole("menuitem", { name: "Import into folder…" }).click();

  const dialog = page.getByRole("dialog", { name: "Import" });
  await expect(dialog.getByRole("tab", { name: "File" })).toHaveAttribute("aria-selected", "true");
  await dialog.getByRole("tab", { name: "API catalog" }).click();
  await expect(dialog.getByLabel("Import into")).toHaveValue(target);
  await dialog.getByLabel("Search APIs").fill("github graphql");
  await dialog.getByTestId("catalog-detail").getByRole("button", { name: "Import", exact: true }).click();
  await expect(dialog.getByText("Imported “GitHub GraphQL”")).toBeVisible();
  await dialog.getByRole("button", { name: "Done" }).click();

  // Shown open in the folder: the API's folder is the one right after it.
  const folder = tree.getByRole("treeitem", { name: new RegExp(`^${target}`) });
  await expect(folder).toHaveAttribute("aria-expanded", "true");
  const inside = folder.locator("xpath=following-sibling::div[1]");
  await expect(inside.getByRole("treeitem", { name: /^GitHub GraphQL Item/ })).toHaveAttribute("aria-expanded", "true");
  await expect(inside.getByText("GitHub GraphQL query")).toBeVisible();
});

test("a failed download explains itself, offers the docs, and the File tab imports instead", async ({ page }) => {
  const message =
    "The Stripe API description is no longer at https://example.com/openapi.json (HTTP 404): the publisher moved or removed it. " +
    "You can download the Stripe API description from https://docs.stripe.com/api yourself and import the file (Import → File).";
  const calls = await answer(page, {
    // Slow, like a download: a second click comes while it runs.
    "catalog.preview": async () => {
      await new Promise((resolve) => setTimeout(resolve, 500));
      return { error: { code: "catalogUnavailable", message, networkKind: null } };
    },
  });
  const dialog = await openImport(page);
  await dialog.getByLabel("Search APIs").fill("stripe");
  const detail = dialog.getByTestId("catalog-detail");
  await expect(detail).toContainText("Downloads from raw.githubusercontent.com");
  // A second click while it runs starts nothing.
  await detail.getByRole("button", { name: "Import", exact: true }).dblclick();
  const notice = detail.getByRole("alert");
  await expect(notice).toContainText("Couldn't import Stripe");
  await expect(notice).toContainText("no longer at");
  expect(calls.filter((c) => c.method === "catalog.preview")).toHaveLength(1);
  await expect(notice.getByRole("button", { name: "Open API docs" })).toBeVisible();

  await notice.getByRole("button", { name: "Import a file instead" }).click();
  await expect(dialog.getByRole("tab", { name: "File" })).toHaveAttribute("aria-selected", "true");
  // The File tab imports as before.
  const collection = JSON.stringify({
    info: { name: "Catalog fallback", schema: "https://schema.getpostman.com/json/collection/v2.1.0/collection.json" },
    item: [{ name: "Fallback request", request: { method: "GET", url: "http://127.0.0.1:18787/json" } }],
  });
  const data = await page.evaluateHandle((text) => {
    const dt = new DataTransfer();
    dt.items.add(new File([text], "collection.json", { type: "application/json" }));
    return dt;
  }, collection);
  await dialog.getByRole("button", { name: /Choose a file, or drop it here/ }).dispatchEvent("drop", { dataTransfer: data });
  await expect(dialog.getByText("Imported “Catalog fallback”")).toBeVisible();
  await dialog.getByRole("button", { name: "Done" }).click();
  await expect(page.getByTestId("collection-tree").getByText("Fallback request")).toBeVisible();
});

test("a large API asks which groups to import", async ({ page }) => {
  const calls = await answer(page, {
    "catalog.preview": () => ({
      result: {
        operations: 420,
        groups: [
          { tag: "repos", operations: 200 },
          { tag: "issues", operations: 150 },
          { tag: "users", operations: 60 },
          { tag: "", operations: 10 },
        ],
        selected: ["repos", "users"],
      },
    }),
    "catalog.import": () => ({
      result: {
        summary: { name: "GitHub", requests: 210, folders: 2, environments: 0, workspaceVariables: 0, warnings: ["2 operations had no example"], folderPath: null },
      },
    }),
  });
  const dialog = await openImport(page);
  await dialog.getByLabel("Search APIs").fill("github");
  const detail = dialog.getByTestId("catalog-detail");
  await expect(detail.getByRole("heading", { name: "GitHub", exact: true })).toBeVisible();
  await detail.getByRole("button", { name: "Import", exact: true }).click();

  const groups = detail.getByTestId("catalog-groups");
  const total = groups.getByTestId("catalog-groups-total");
  // The suggested groups are ticked to begin with.
  await expect(total).toHaveText("260 requests selected");
  await expect(groups.getByRole("checkbox", { name: "repos" })).toHaveAttribute("aria-checked", "true");
  await expect(groups.getByRole("checkbox", { name: "issues" })).toHaveAttribute("aria-checked", "false");
  await expect(groups.getByRole("checkbox", { name: "Other" })).toBeVisible();

  await groups.getByRole("button", { name: "None" }).click();
  await expect(total).toHaveText("0 requests selected");
  await expect(groups.getByRole("button", { name: "Import", exact: true })).toBeDisabled();
  // Select all applies to the groups the search shows.
  await groups.getByLabel("Search groups").fill("iss");
  await expect(groups.getByRole("checkbox")).toHaveCount(1);
  await groups.getByRole("button", { name: "Select all" }).click();
  await groups.getByLabel("Search groups").fill("");
  await groups.getByText("users", { exact: true }).click();
  await expect(total).toHaveText("210 requests selected");

  await groups.getByRole("button", { name: "Import", exact: true }).click();
  await expect(dialog.getByText("Imported “GitHub”")).toBeVisible();
  await expect(dialog).toContainText("2 operations had no example");
  const sent = calls.find((c) => c.method === "catalog.import")!;
  expect(sent.params).toMatchObject({ id: "github", parent: "", groups: ["issues", "users"] });
});

test("the welcome screen browses the catalog and makes a workspace to import into", async ({ page }) => {
  const folder = fs.mkdtempSync(path.join(os.tmpdir(), "zorvik-e2e-catalog-"));
  await page.getByTestId("workspace-menu").click();
  await page.getByRole("menuitem", { name: "Close workspace" }).click();
  await page.getByTestId("welcome-catalog").click();

  const dialog = page.getByRole("dialog", { name: "Import" });
  // Without a workspace only the catalog is offered.
  await expect(dialog.getByRole("tab")).toHaveCount(1);
  await dialog.getByLabel("Search APIs").fill("github graphql");
  const detail = dialog.getByTestId("catalog-detail");
  await expect(detail).toContainText("you'll choose its folder first");
  page.once("dialog", (d) => void d.accept(folder));
  await detail.getByRole("button", { name: "Import", exact: true }).click();
  await page.getByRole("button", { name: "Create workspace" }).click();

  await expect(dialog.getByText("Imported “GitHub GraphQL”")).toBeVisible();
  await expect(page.getByTestId("workspace-menu")).toContainText(path.basename(folder));
  await dialog.getByRole("button", { name: "Done" }).click();
  await expect(page.getByTestId("collection-tree").getByText("GitHub GraphQL query")).toBeVisible();
});

test("one import at a time: the other tabs wait for the catalog, and the catalog for them", async ({ page }) => {
  const preview = held();
  const file = held();
  const calls = await answer(page, {
    "catalog.preview": async () => {
      await preview.gate;
      return { error: { code: "catalogUnavailable", message: "The download failed.", networkKind: null } };
    },
    "import.file": async () => {
      await file.gate;
      return { result: { name: "Held file", requests: 1, folders: 0, environments: 0, workspaceVariables: 0, warnings: [], folderPath: null } };
    },
  });
  const dialog = await openImport(page);
  await dialog.getByLabel("Search APIs").fill("stripe");
  const detail = dialog.getByTestId("catalog-detail");
  await detail.getByRole("button", { name: "Import", exact: true }).click();
  await expect(detail.getByRole("status")).toContainText("Downloading from");
  // Where it goes can't change while it runs.
  await expect(dialog.getByLabel("Import into")).toBeDisabled();

  const waiting = dialog.getByTestId("import-waiting");
  await dialog.getByRole("tab", { name: "File" }).click();
  await expect(waiting).toContainText("An API from the catalog is being imported");
  const dropZone = dialog.getByRole("button", { name: /Choose a file, or drop it here/ });
  await expect(dropZone).toBeDisabled();
  await dropText(page, dropZone, "early.json", "{}");
  await dialog.getByRole("tab", { name: "OpenAPI URL" }).click();
  await dialog.getByLabel("OpenAPI / Swagger URL").fill("https://example.com/openapi.json");
  await expect(dialog.getByRole("button", { name: "Import", exact: true })).toBeDisabled();

  preview.release();
  await expect(waiting).toHaveCount(0);
  await dialog.getByRole("tab", { name: "API catalog" }).click();
  await expect(detail.getByRole("alert")).toContainText("Couldn't import Stripe");
  expect(calls.filter((c) => c.method === "import.file")).toHaveLength(0);

  // A file import runs: the catalog waits for it.
  await dialog.getByRole("tab", { name: "File" }).click();
  await dropText(page, dropZone, "held.json", "{}");
  await dialog.getByRole("tab", { name: "API catalog" }).click();
  await expect(waiting).toContainText("Another import is running");
  await expect(detail.getByRole("button", { name: "Import", exact: true })).toBeDisabled();
  file.release();
  await expect(dialog.getByText("Imported “Held file”")).toBeVisible();
  // A file, not an API from the catalog: nothing about its environment.
  await expect(dialog).not.toContainText("is now active");
  expect(calls.filter((c) => c.method === "import.file")).toHaveLength(1);
  expect(calls.filter((c) => c.method === "catalog.preview")).toHaveLength(1);
});

test("closing the window while the description downloads cancels the import", async ({ page }) => {
  const preview = held();
  let answered = false;
  const calls = await answer(page, {
    "catalog.preview": async () => {
      await preview.gate;
      answered = true;
      return { result: { operations: 3, groups: [], selected: [] } };
    },
    "catalog.import": () => ({ error: { code: "internal", message: "Not expected", networkKind: null } }),
  });
  const dialog = await openImport(page);
  await dialog.getByLabel("Search APIs").fill("stripe");
  const detail = dialog.getByTestId("catalog-detail");
  await detail.getByRole("button", { name: "Import", exact: true }).click();
  await expect(detail.getByRole("status")).toContainText("Downloading from");
  await dialog.getByRole("button", { name: "Cancel" }).click();
  await expect(dialog).toHaveCount(0);

  preview.release();
  await expect.poll(() => answered).toBe(true);
  // Give a late import the time it would take to start.
  await page.waitForTimeout(500);
  expect(calls.filter((c) => c.method === "catalog.import")).toHaveLength(0);
});

test("an import whose workspace can't reload afterwards still says what happened", async ({ page }) => {
  await answer(page, {
    "catalog.preview": () => ({ result: { operations: 3, groups: [], selected: [] } }),
    "catalog.import": () => ({
      result: { summary: { name: "Stripe", requests: 3, folders: 1, environments: 0, workspaceVariables: 0, warnings: [], folderPath: null } },
    }),
    "workspace.reload": () => ({ error: { code: "io", message: "The workspace folder can't be read.", networkKind: null } }),
  });
  const dialog = await openImport(page);
  await dialog.getByLabel("Search APIs").fill("stripe");
  await dialog.getByTestId("catalog-detail").getByRole("button", { name: "Import", exact: true }).click();
  await expect(dialog.getByText("Imported “Stripe”")).toBeVisible();
  await expect(page.getByText("Imported Stripe, but couldn't reload the workspace")).toBeVisible();
  await expect(page.getByText("The workspace folder can't be read.")).toBeVisible();
});

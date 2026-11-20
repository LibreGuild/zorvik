import fs from "node:fs";
import path from "node:path";
import { expect, test, type Page } from "@playwright/test";

// Scenarios against the local test server: steps from a folder, a value passed from one
// step to the next, checks, running a part, and the values a run leaves.
const HTTP = "http://127.0.0.1:18787";
const WORKSPACE = process.env.ZV_E2E_WORKSPACE!;

test.beforeEach(async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("workspace-menu")).toContainText("E2E Workspace");
});

/** A saved request in `folder` (right-click menu) with a URL. */
async function requestIn(page: Page, folder: string, name: string, url: string) {
  await page.getByTestId("collection-tree").getByRole("treeitem", { name: new RegExp(folder) }).click({ button: "right" });
  await page.getByRole("menuitem", { name: "New HTTP request" }).click();
  await page.getByRole("textbox").last().fill(name);
  await page.getByRole("button", { name: "Create" }).click();
  // Wait for the new request's tab (an empty URL), or the URL lands in the previous tab.
  await expect(page.getByRole("tab", { name: new RegExp(name), selected: true })).toBeVisible();
  await expect(page.getByLabel("URL", { exact: true })).toHaveValue("");
  await page.getByLabel("URL", { exact: true }).fill(url);
  await page.keyboard.press("ControlOrMeta+s");
  await expect(page.getByTestId("collection-tree")).toContainText(name);
}

test("a scenario passes a value from one step to the next, checks it, and keeps its values to itself", async ({ page }) => {
  test.setTimeout(120_000);
  await page.getByRole("button", { name: "New", exact: true }).click();
  await page.getByRole("menuitem", { name: "New folder" }).click();
  await page.getByRole("textbox").last().fill("E2E scenario");
  await page.getByRole("button", { name: "Create" }).click();
  await requestIn(page, "E2E scenario", "Create cart", `${HTTP}/echo?id=7`);
  await requestIn(page, "E2E scenario", "Get cart", `${HTTP}/echo?cart={{cartId}}&who={{shopper}}`);

  // "New scenario…" on the folder: a step per request, in order.
  await page.getByTestId("collection-tree").getByRole("treeitem", { name: /E2E scenario/ }).click({ button: "right" });
  await page.getByRole("menuitem", { name: "New scenario…" }).click();
  await page.getByRole("textbox").last().fill("Cart flow");
  await page.getByRole("button", { name: "Create" }).click();
  const view = page.getByTestId("scenario-view");
  const sections = page.getByTestId("scenario-sections");
  await expect(view).toBeVisible();
  await expect(page.getByTestId("scenarios-list")).toContainText("Cart flow");
  const steps = page.getByTestId("scenario-step");
  await expect(steps).toHaveCount(2);
  await expect(steps.first()).toContainText("Create cart");
  await expect(page.getByTestId("scenario-status")).toContainText("2 steps");

  // Run up to the first step, then pick an output from what came back.
  await steps.first().hover();
  await steps.first().getByTestId("scenario-run-to").click();
  await expect(page.getByText("“Cart flow” passed")).toBeVisible({ timeout: 20_000 });
  await expect(page.getByTestId("runner-result")).toHaveCount(1);
  await expect(steps.first().getByTestId("scenario-step-outcome")).toBeVisible();
  await steps.first().getByTestId("scenario-step-title").click();
  await steps.first().getByTestId("scenario-pick-output").click();
  await page.getByPlaceholder("Search the response").fill("args.id");
  await page.getByTestId("scenario-pick-row").first().click();
  await expect(steps.first().getByTestId("scenario-output")).toHaveCount(1);
  await expect(steps.first().getByLabel(/Output 1 of Create cart: json path/)).toHaveValue("$.args.id");
  await steps.first().getByLabel("Output 1 of Create cart: variable").fill("cartId");
  await steps.first().getByRole("button", { name: "Add check" }).click();
  await expect(steps.first().getByTestId("scenario-check")).toHaveCount(1);

  // The second step: what the request uses and nothing gives yet is offered as an input.
  await steps.nth(1).getByTestId("scenario-step-title").click();
  await expect(steps.nth(1).getByRole("button", { name: "+ shopper" })).toBeVisible();
  await expect(steps.nth(1).getByRole("button", { name: "+ cartId" })).toHaveCount(0);
  await steps.nth(1).getByRole("button", { name: "+ shopper" }).click();
  await steps.nth(1).getByLabel("Input 1 value").fill("ada-{{cartId}}");

  // A step that sends nothing: a value worked out with JavaScript.
  await page.getByTestId("scenario-add-step-steps").click();
  await page.getByRole("menuitem", { name: "Set values" }).click();
  await expect(steps).toHaveCount(3);
  await steps.nth(2).getByLabel("Value 1 name").fill("total");
  await steps.nth(2).getByLabel("Value 1 value").fill("= Number(cartId) * 6");

  // The whole scenario (Mod+Enter).
  await page.keyboard.press("ControlOrMeta+Enter");
  await expect(page.getByText("“Cart flow” passed").last()).toBeVisible({ timeout: 20_000 });
  const results = page.getByTestId("runner-result");
  await expect(results).toHaveCount(3);
  await expect(page.getByTestId("runner-summary")).toContainText("3 passed · 0 failed");
  await expect(page.getByTestId("runner-summary")).toContainText("Tests 1/1");
  await expect(results.first().getByTestId("result-saved")).toContainText("cartId = 7");
  await expect(results.nth(2)).toContainText("Set total");
  await expect(results.nth(2).getByTestId("result-saved")).toContainText("total = 42");
  await results.nth(1).getByRole("button").first().click();
  await expect(page.getByTestId("runner-result-details").first()).toContainText("/echo?cart=7&who=ada-7");

  // The values the run left; they are the scenario's own (the environment has none of them).
  await sections.getByRole("button", { name: /^Variables/ }).click();
  await expect(page.getByTestId("scenario-value")).toHaveCount(2);
  await expect(page.getByTestId("scenario-values")).toContainText("cartId");
  await expect(page.getByTestId("scenario-values")).toContainText("42");
  await page.getByTestId("scenario-clear-values").click();
  await expect(page.getByTestId("scenario-value")).toHaveCount(0);

  // Docs, and saving: a plain file in the workspace.
  await sections.getByRole("button", { name: /^Docs/ }).click();
  await view.getByRole("button", { name: "Edit" }).click();
  await view.locator(".cm-content").click();
  await page.keyboard.insertText("Creates a cart and reads it back.");
  await page.keyboard.press("ControlOrMeta+s");
  const file = path.join(WORKSPACE, "scenarios", "Cart flow.yaml");
  await expect.poll(() => (fs.existsSync(file) ? fs.readFileSync(file, "utf8") : "")).toContain("variable: cartId");
  const saved = fs.readFileSync(file, "utf8");
  expect(saved).toContain("request: E2E scenario/Create cart.yaml");
  expect(saved).toContain("docs: Creates a cart and reads it back.");
  expect(saved).toContain("value: = Number(cartId) * 6");

  // A failing check ends the run at that step; the step shows it.
  await sections.getByRole("button", { name: /^Steps/ }).click();
  await steps.first().getByLabel("Check 1 of Create cart: value").fill("201");
  await page.getByTestId("scenario-start").click();
  await expect(page.getByText("“Cart flow” failed")).toBeVisible({ timeout: 20_000 });
  await expect(page.getByTestId("runner-result")).toHaveCount(1);
  await expect(page.getByTestId("runner-summary")).toContainText("ended at the first failed step");
});

test("the command palette makes a scenario; steps are added from the collection and nest in an if", async ({ page }) => {
  test.setTimeout(90_000);
  await page.keyboard.press("ControlOrMeta+k");
  await page.getByPlaceholder(/Search requests/).fill("new scenario");
  await page.keyboard.press("Enter");
  await page.getByRole("textbox").last().fill("Branches");
  await page.getByRole("button", { name: "Create" }).click();
  const view = page.getByTestId("scenario-view");
  await expect(view).toBeVisible();
  await expect(page.getByTestId("scenario-status")).toContainText("Add a step");
  await expect(page.getByTestId("scenario-start")).toBeDisabled();

  // An "if" whose branch holds a request picked from the collection.
  await page.getByTestId("scenario-add-step-steps").click();
  await page.getByRole("menuitem", { name: "If", exact: true }).click();
  const branch = page.getByTestId("scenario-step").first();
  await branch.getByLabel("Rule 1 value").fill("= 2 > 1");
  await branch.getByLabel("Rule 1 comparison").selectOption("isTrue");
  await page.getByTestId("scenario-add-request-steps/0/steps").click();
  await page.getByPlaceholder("Search requests").fill("Create cart");
  await page.getByRole("option", { name: /Create cart/ }).click();
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("scenario-list-steps/0/steps").getByTestId("scenario-step")).toHaveCount(1);
  await expect(page.getByTestId("scenario-status")).toContainText("2 steps");

  await page.getByTestId("scenario-start").click();
  await expect(page.getByText("“Branches” passed")).toBeVisible({ timeout: 20_000 });
  const results = page.getByTestId("runner-result");
  await expect(results).toHaveCount(2);
  await expect(results.first()).toContainText("If = 2 > 1 is true · yes");
  await expect(results.nth(1)).toContainText("Create cart");
  // The "if" only directs: one step did something.
  await expect(page.getByTestId("runner-summary")).toContainText("1 passed · 0 failed");

  // Steps move by their menu and by dragging, also into another step's list.
  const top = page.getByTestId("scenario-list-steps").locator("> [data-testid=scenario-step]");
  await page.getByTestId("scenario-add-step-steps").click();
  await page.getByRole("menuitem", { name: "Wait", exact: true }).click();
  await expect(top).toHaveCount(2);
  await expect(top.nth(1)).toContainText("Wait 1000 ms");
  await top.nth(1).getByRole("button", { name: "Actions of Wait 1000 ms" }).click();
  await page.getByRole("menuitem", { name: "Move up" }).click();
  await expect(top.first()).toContainText("Wait 1000 ms");
  // The last run's results stay with their steps: the new step has none, the "if" and the step in it keep theirs.
  await expect(top.first().getByTestId("scenario-step-outcome")).toHaveCount(0);
  await expect(top.nth(1).getByTestId("scenario-step-outcome")).toHaveCount(2);
  await top.first().getByTestId("scenario-step-title").dragTo(page.getByTestId("scenario-list-end-steps/1/else"));
  await expect(top).toHaveCount(1);
  const moved = page.getByTestId("scenario-list-steps/0/else").getByTestId("scenario-step");
  await expect(moved).toContainText("Wait 1000 ms");
  // Onto the lower half of a step's row: after that step.
  const then = page.getByTestId("scenario-list-steps/0/steps");
  const target = then.getByTestId("scenario-step-row");
  const box = (await target.boundingBox())!;
  await moved.getByTestId("scenario-step-title").dragTo(target, { targetPosition: { x: box.width / 2, y: box.height - 4 } });
  await expect(then.getByTestId("scenario-step")).toHaveCount(2);
  await expect(then.getByTestId("scenario-step").nth(1)).toContainText("Wait 1000 ms");

  // Only the step inside the branch, with the values the last run left.
  const inner = then.getByTestId("scenario-step").first();
  await inner.hover();
  await inner.getByTestId("scenario-run-only").click();
  await expect(page.getByText("“Branches” passed").last()).toBeVisible({ timeout: 20_000 });
  await expect(page.getByTestId("runner-result")).toHaveCount(1);
  await expect(page.getByTestId("runner-result")).toContainText("Create cart");
});

test("scenarios are kept in folders, inside each other, and still run each other", async ({ page }) => {
  test.setTimeout(90_000);
  const file = (rel: string) => path.join(WORKSPACE, "scenarios", rel);
  const read = (rel: string) => (fs.existsSync(file(rel)) ? fs.readFileSync(file(rel), "utf8") : "");
  const name = async (value: string, confirm: string) => {
    await page.getByRole("textbox").last().fill(value);
    await page.getByRole("button", { name: confirm, exact: true }).click();
  };
  await page.getByTestId("rail-scenarios").click();
  const list = page.getByTestId("scenarios-list");

  // A folder, a folder in it, and a scenario in that one.
  await list.getByRole("button", { name: "New scenario" }).click();
  await page.getByRole("menuitem", { name: "New folder" }).click();
  await name("E2E Shop", "Create");
  await list.getByTestId("scenario-folder-E2E Shop").click({ button: "right" });
  await page.getByRole("menuitem", { name: "New folder" }).click();
  await name("Auth", "Create");
  const auth = list.getByTestId("scenario-folder-E2E Shop/Auth");
  await expect(auth).toBeVisible();
  await auth.click({ button: "right" });
  await page.getByRole("menuitem", { name: "New scenario" }).click();
  await name("Log in", "Create");
  await expect(page.getByTestId("scenario-folders")).toHaveText("E2E Shop / Auth /");
  await expect(list.getByTestId("scenario-row-E2E Shop/Auth/Log in")).toBeVisible();
  await page.getByTestId("scenario-add-step-steps").click();
  await page.getByRole("menuitem", { name: "Set values" }).click();
  await page.getByTestId("scenario-step").first().getByLabel("Value 1 name").fill("token");
  await page.getByTestId("scenario-step").first().getByLabel("Value 1 value").fill("abc");
  await page.keyboard.press("ControlOrMeta+s");
  await expect.poll(() => read("E2E Shop/Auth/Log in.yaml")).toContain("key: token");

  // A scenario at the top level runs it, chosen by its folders and name.
  await list.getByRole("button", { name: "New scenario" }).click();
  await page.getByRole("menuitem", { name: "New scenario" }).click();
  await name("E2E Main", "Create");
  await page.getByTestId("scenario-add-step-steps").click();
  await page.getByRole("menuitem", { name: "Run scenario" }).click();
  await page.getByLabel("Scenario to run").selectOption({ label: "E2E Shop / Auth / Log in" });
  await page.keyboard.press("ControlOrMeta+s");
  await expect.poll(() => read("E2E Main.yaml")).toContain("scenario: E2E Shop/Auth/Log in");
  await page.keyboard.press("ControlOrMeta+Enter");
  await expect(page.getByText("“E2E Main” passed").last()).toBeVisible({ timeout: 20_000 });
  await expect(page.getByTestId("runner-result").first()).toContainText("Run scenario Log in");

  // A folder renamed: the step that runs the scenario in it follows, and so does its tab.
  await auth.click({ button: "right" });
  await page.getByRole("menuitem", { name: "Rename…" }).click();
  await name("Sign in", "Rename");
  await expect(list.getByTestId("scenario-row-E2E Shop/Sign in/Log in")).toBeVisible();
  await expect.poll(() => read("E2E Main.yaml")).toContain("scenario: E2E Shop/Sign in/Log in");
  await page.getByRole("tab", { name: /Log in/ }).click();
  await expect(page.getByTestId("scenario-folders")).toHaveText("E2E Shop / Sign in /");

  // Dragged onto a folder: into it.
  const shop = list.getByTestId("scenario-folder-E2E Shop");
  const box = (await shop.boundingBox())!;
  await list.getByTestId("scenario-row-E2E Main").dragTo(shop, { targetPosition: { x: box.width / 2, y: box.height - 4 } });
  await expect(list.getByTestId("scenario-row-E2E Shop/E2E Main")).toBeVisible();
  expect(fs.existsSync(file("E2E Shop/E2E Main.yaml"))).toBe(true);

  // Moved with "Move to…" to the top level; it still runs.
  await list.getByTestId("scenario-row-E2E Shop/Sign in/Log in").click({ button: "right" });
  await page.getByRole("menuitem", { name: "Move to…" }).click();
  await page.getByLabel("Destination folder").selectOption("");
  await page.getByRole("button", { name: "Move", exact: true }).click();
  await expect(list.getByTestId("scenario-row-Log in")).toBeVisible();
  await expect.poll(() => read("E2E Shop/E2E Main.yaml")).toContain("scenario: Log in");
  await list.getByTestId("scenario-row-E2E Shop/E2E Main").click();
  await page.getByTestId("scenario-start").click();
  await expect(page.getByText("“E2E Main” passed").last()).toBeVisible({ timeout: 20_000 });

  // The filter shows a scenario with the folders it is in.
  await list.getByLabel("Filter scenarios").fill("e2e main");
  await expect(list.getByRole("treeitem")).toHaveCount(2);
  await expect(list.getByTestId("scenario-folder-E2E Shop")).toBeVisible();
});

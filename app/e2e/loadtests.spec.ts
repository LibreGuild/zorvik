import { expect, test } from "@playwright/test";

// Load tests against the local test server (localhost: no "another host" confirmation).
const HTTP = "http://127.0.0.1:18787";

test.beforeEach(async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("workspace-menu")).toContainText("E2E Workspace");
});

test("a request's load test runs, shows live numbers and passes its thresholds", async ({ page }) => {
  test.setTimeout(90_000);

  // A saved request to the local test server.
  await page.getByRole("button", { name: "New tab" }).click();
  await page.getByRole("menuitem", { name: "HTTP request" }).click();
  await page.getByLabel("URL", { exact: true }).fill(`${HTTP}/json`);
  await page.getByRole("button", { name: /^Save/ }).first().click();
  await page.getByLabel("Name").fill("E2E load target");
  await page.getByRole("button", { name: "Save", exact: true }).click();
  const row = page.getByTestId("collection-tree").getByRole("treeitem", { name: /E2E load target/ });
  await expect(row).toBeVisible();

  // "Load test…" from its row creates a load test with it and opens it.
  await row.click({ button: "right" });
  await page.getByRole("menuitem", { name: "Load test…" }).click();
  const view = page.getByTestId("loadtest-view");
  await expect(view).toBeVisible();
  await expect(view).toContainText("E2E load target load test");
  await expect(page.getByTestId("load-target")).toHaveCount(1);
  await expect(page.getByTestId("load-target")).toContainText("E2E load target");
  await expect(page.getByTestId("loadtests-list")).toContainText("E2E load target load test");

  // A short constant load: 5 users for 3 seconds.
  await page.getByRole("button", { name: "Constant" }).click();
  await page.getByLabel("Peak load").fill("5");
  await page.getByLabel("Total duration in seconds").fill("3");
  await expect(page.getByTestId("load-stage")).toHaveCount(2);

  // Thresholds: the defaults (p95 < 500 ms, errors < 1 %) plus p99 < 2000 ms.
  await page.getByRole("button", { name: "Add threshold" }).click();
  const added = page.getByTestId("load-threshold").last();
  await added.getByLabel("Threshold metric").selectOption("p99");
  await added.getByLabel("Threshold value").fill("2000");
  await expect(page.getByTestId("load-threshold")).toHaveCount(3);
  await page.keyboard.press("ControlOrMeta+s");
  await expect(page.getByTestId("loadtests-list")).toContainText("3 s");

  // Start: localhost needs no confirmation.
  await page.getByTestId("loadtest-start").click();
  await expect(page.getByTestId("running-load-test")).toBeVisible();
  await expect(page.getByTestId("rail-loadtests-running")).toBeVisible();
  await expect(page.getByTestId("loadtest-status")).toContainText(/Starting|Running/);

  // Live numbers arrive while it runs.
  await expect
    .poll(async () => Number((await page.getByTestId("stat-requests-value").getAttribute("data-value")) ?? 0), { timeout: 15_000 })
    .toBeGreaterThan(0);

  // It finishes on its own: a toast, the verdict and the history.
  await expect(page.getByText("“E2E load target load test” passed")).toBeVisible({ timeout: 20_000 });
  await expect(page.getByTestId("running-load-test")).toHaveCount(0);
  await expect(page.getByTestId("load-verdict")).toContainText("PASSED");
  await expect(page.getByTestId("load-threshold-results")).toContainText("p99");
  await expect(page.getByTestId("load-status-codes")).toContainText("200");
  expect(Number(await page.getByTestId("stat-requests-value").getAttribute("data-value"))).toBeGreaterThan(0);
  await expect(page.getByTestId("load-runs")).toContainText("1 run");
  await expect(page.getByTestId("loadtest-start")).toBeEnabled();
});

test("a load test can be stopped early from the title bar", async ({ page }) => {
  test.setTimeout(90_000);
  await page.getByTestId("rail-loadtests").click();
  // The "+" menu (an empty list also has a "New load test" link, after it).
  await page.getByRole("button", { name: "New load test" }).first().click();
  await page.getByRole("menuitem", { name: "New load test" }).click();
  await page.getByRole("textbox").last().fill("E2E stop early");
  await page.getByRole("button", { name: "Create" }).click();
  const view = page.getByTestId("loadtest-view");
  await expect(view).toBeVisible();
  await expect(page.getByTestId("loadtest-status")).toContainText("Add a request to send");
  await expect(page.getByTestId("loadtest-start")).toBeDisabled();

  // Pick the saved request from the picker (created by the first test, or create it here).
  await page.getByRole("button", { name: "Add requests" }).first().click();
  const picker = page.getByRole("listbox", { name: "Requests" });
  if (!(await picker.getByText("E2E load target").count())) {
    await page.keyboard.press("Escape");
    test.skip(true, "needs the request saved by the first test");
  }
  await picker.getByText("E2E load target").click();
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("load-target")).toHaveCount(1);

  await page.getByRole("button", { name: "Constant" }).click();
  await page.getByLabel("Peak load").fill("2");
  await page.getByLabel("Total duration in seconds").fill("60");
  await page.keyboard.press("ControlOrMeta+Enter");
  await expect(page.getByTestId("running-load-test")).toBeVisible();
  await expect
    .poll(async () => Number((await page.getByTestId("stat-requests-value").getAttribute("data-value")) ?? 0), { timeout: 15_000 })
    .toBeGreaterThan(0);

  await page.getByTestId("running-load-test-stop").click();
  await expect(page.getByTestId("running-load-test")).toHaveCount(0, { timeout: 15_000 });
  await expect(page.getByTestId("load-verdict")).toContainText("Stopped early");
});

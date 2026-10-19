import { expect, type Page, test } from "@playwright/test";

// Load tests: a random think time, a warm-up, a threshold that stops the run early,
// each request's status codes, and the CSV exports (local test server: no confirmation).
const HTTP = "http://127.0.0.1:18787";

test.beforeEach(async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("workspace-menu")).toContainText("E2E Workspace");
});

/** Save a GET request to `url` under `name` and return its row in the collection. */
async function saveRequest(page: Page, name: string, url: string) {
  await page.getByRole("button", { name: "New tab" }).click();
  await page.getByRole("menuitem", { name: "HTTP request" }).click();
  await page.getByLabel("URL", { exact: true }).fill(url);
  await page.getByRole("button", { name: /^Save/ }).first().click();
  await page.getByLabel("Name").fill(name);
  await page.getByRole("button", { name: "Save", exact: true }).click();
  const row = page.getByTestId("collection-tree").getByRole("treeitem", { name: new RegExp(name) });
  await expect(row).toBeVisible();
  return row;
}

test("a threshold that keeps failing stops the run early, after the warm-up", async ({ page }) => {
  test.setTimeout(90_000);
  const row = await saveRequest(page, "E2E outage target", `${HTTP}/status/503`);
  await row.click({ button: "right" });
  await page.getByRole("menuitem", { name: "Load test…" }).click();
  await expect(page.getByTestId("loadtest-view")).toContainText("E2E outage target load test");

  // Two users for a minute, pausing 20–80 ms at random, with a one-second warm-up.
  await page.getByRole("button", { name: "Constant" }).click();
  await page.getByLabel("Peak load").fill("2");
  await page.getByLabel("Total duration in seconds").fill("60");
  await page.getByLabel("Think time in milliseconds", { exact: true }).fill("20");
  await page.getByLabel("Longest think time in milliseconds").fill("10");
  await expect(page.getByLabel("Longest think time in milliseconds")).toHaveAttribute("aria-invalid", "true");
  await expect(page.getByTestId("loadtest-status")).toContainText("The longest think time is shorter than the shortest");
  await page.getByLabel("Longest think time in milliseconds").fill("80");
  await expect(page.getByLabel("Longest think time in milliseconds")).not.toHaveAttribute("aria-invalid", "true");
  await page.getByLabel("Warm-up in seconds").fill("1");

  // The default "errorRate < 1 %" threshold stops the run after 1 s of failing.
  const errors = page.getByTestId("load-threshold").nth(1);
  await expect(errors.getByLabel("Threshold metric")).toHaveValue("errorRate");
  await errors.getByRole("button", { name: "Stop the run early when this threshold keeps failing" }).click();
  await errors.getByLabel("Seconds failing before the run stops").fill("1");
  await page.keyboard.press("ControlOrMeta+s");

  const started = Date.now();
  await page.getByTestId("loadtest-start").click();
  await expect(page.getByTestId("running-load-test")).toBeVisible();
  await expect(page.getByText("“E2E outage target load test” was stopped by a threshold")).toBeVisible({ timeout: 20_000 });
  expect(Date.now() - started).toBeLessThan(30_000);

  const verdict = page.getByTestId("load-verdict");
  await expect(verdict).toContainText("FAILED");
  await expect(verdict).toContainText("Stopped by a threshold");
  await expect(verdict).toContainText("Stopped early: errorRate < 1 % failed for longer than it allows.");
  await expect(verdict).toContainText("The first 1 s (warm-up) are charted but left out");
  await expect(page.getByTestId("load-threshold-stopped")).toBeVisible();
  await expect(page.getByTestId("load-status-codes")).toContainText("503");

  // The exports include both CSV files.
  await page.getByRole("button", { name: "Export run" }).click();
  await expect(page.getByRole("menuitem", { name: "CSV, one row per second…" })).toBeVisible();
  await expect(page.getByRole("menuitem", { name: "CSV, one row per request…" })).toBeVisible();
  await page.keyboard.press("Escape");
});

test("status codes and errors can be shown for one request", async ({ page }) => {
  test.setTimeout(90_000);
  await saveRequest(page, "E2E fine target", `${HTTP}/status/200`);
  const broken = await saveRequest(page, "E2E broken target", `${HTTP}/status/500`);
  await broken.click({ button: "right" });
  await page.getByRole("menuitem", { name: "Load test…" }).click();
  await expect(page.getByTestId("load-target")).toHaveCount(1);
  await page.getByRole("button", { name: "Add requests" }).first().click();
  await page.getByRole("listbox", { name: "Requests" }).getByText("E2E fine target").click();
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("load-target")).toHaveCount(2);

  await page.getByRole("button", { name: "Constant" }).click();
  await page.getByLabel("Peak load").fill("2");
  await page.getByLabel("Total duration in seconds").fill("2");
  await page.getByLabel("Think time in milliseconds", { exact: true }).fill("20");
  await page.keyboard.press("ControlOrMeta+s");
  await page.getByTestId("loadtest-start").click();
  await expect(page.getByTestId("load-verdict")).toBeVisible({ timeout: 20_000 });

  const codes = page.getByTestId("load-status-codes");
  await expect(codes).toContainText("200");
  await expect(codes).toContainText("500");
  await expect(page.getByTestId("load-status-classes")).toContainText("2xx");
  await page.getByLabel("Status codes and errors of").selectOption({ label: "E2E broken target" });
  await expect(codes).toContainText("500");
  await expect(codes).not.toContainText("200");
  await expect(page.getByTestId("load-errors")).toContainText("HTTP status ≥ 400");
  await page.getByLabel("Status codes and errors of").selectOption({ label: "All requests" });
  await expect(codes).toContainText("200");
});

import { expect, test } from "@playwright/test";

// The in-app docs: the rail opens the topic list; a topic opens the Docs tab at it.
test("docs open from the rail and jump to a topic", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("workspace-menu")).toContainText("E2E Workspace");
  await page.getByTestId("rail-docs").click();
  await expect(page.getByTestId("docs-list")).toBeVisible();

  await page.getByTestId("docs-start").click();
  const docs = page.getByTestId("docs");
  await expect(docs.getByRole("heading", { name: "Everything Zorvik can do" })).toBeVisible();
  // Every feature card has its illustration.
  const cards = docs.locator("[data-testid^='docs-card-']");
  await expect(cards).toHaveCount(16);
  const broken = await docs.locator("img").evaluateAll((imgs) => imgs.filter((i) => !(i as HTMLImageElement).complete || (i as HTMLImageElement).naturalWidth === 0).length);
  expect(broken).toBe(0);

  await page.getByTestId("docs-topic-load").click();
  await expect(docs.getByTestId("doc-load").getByRole("heading", { name: "Load testing" })).toBeInViewport();
  // One Docs tab, reused.
  await page.getByTestId("docs-topic-agents").click();
  await expect(docs.getByTestId("doc-agents")).toBeInViewport();
  await expect(page.getByRole("tab", { name: /Docs/ })).toHaveCount(1);
});

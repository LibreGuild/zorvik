import { expect, test, type Page } from "@playwright/test";

/** Call the API the way the UI does (dev bridge). */
async function rpc(page: Page, method: string, params: unknown = {}) {
  const res = await page.request.post("/bridge/rpc", { data: { method, params }, headers: { "x-zorvik-bridge": "1" } });
  return ((await res.json()) as { result?: unknown }).result;
}

test.beforeEach(async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("workspace-menu")).toContainText("E2E Workspace");
});

// Later tests expect the E2E workspace: go back to it, without a lab running.
test.afterEach(async ({ page }) => {
  await rpc(page, "academy.stopLab");
  await rpc(page, "workspace.open", { path: process.env.ZV_E2E_WORKSPACE });
});

test("the Academy: a lesson, its lab in the workbench, the quiz and the rewards", async ({ page }) => {
  // The Workbench | Academy switch belongs to the Bootcamp workspace; the pinned menu entry opens the course.
  await expect(page.getByTestId("mode-switch")).toHaveCount(0);
  await page.getByTestId("workspace-menu").click();
  await page.getByRole("menuitem", { name: /Training Bootcamp/ }).click();
  await expect(page.getByTestId("academy")).toBeVisible();
  await expect(page.getByTestId("workspace-menu")).toContainText("Training Bootcamp");
  await expect(page.getByTestId("mode-switch")).toBeVisible();
  await expect(page.getByTestId("course-map")).toBeVisible();

  await page.getByTestId("lesson-first-request").click();
  const lesson = page.getByTestId("lesson");
  await expect(lesson.getByRole("heading", { name: "Your first request" })).toBeVisible();
  // Diagrams render from the lesson's text.
  await expect(lesson.getByLabel("Sequence diagram")).toContainText("GET /hello");

  // The lab starts its server, fills the Lab environment and docks the guide beside the workbench.
  await lesson.getByTestId("lab-start").last().click();
  const guide = page.getByTestId("lab-guide");
  await expect(guide).toBeVisible();
  await expect(page.getByTestId("env-picker")).toContainText("Lab");
  await expect(page.getByTestId("running-servers")).toBeVisible();

  // Step 1 is ticked off as soon as the lab server sees the request.
  await page.getByRole("button", { name: "New tab" }).click();
  await page.getByRole("menuitem", { name: "HTTP request" }).click();
  await page.getByLabel("URL", { exact: true }).fill("{{api}}/hello");
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await expect(page.getByTestId("response-status")).toHaveText("200 OK");
  await expect(guide.getByTestId("lab-step-0")).toHaveAttribute("data-done", "true");

  // A wrong answer is refused; hints open one by one; "Do it for me" finishes the step.
  await guide.getByTestId("lab-answer").fill("not-the-word");
  await guide.getByRole("button", { name: "Check" }).click();
  await expect(guide).toContainText("Not that one");
  await guide.getByTestId("lab-hint").click();
  await expect(guide).toContainText("curly braces");
  await guide.getByTestId("lab-do-it").click();
  await expect(guide.getByTestId("lab-finished")).toBeVisible();

  // Back in the lesson, the quick check completes it.
  await guide.getByRole("button", { name: "Back to the lesson" }).click();
  const quiz = page.getByTestId("quiz");
  for (const [q, option] of [
    [0, "A message your app sends to a server, asking for something"],
    [1, "It is a variable; Zorvik replaces it with its value before sending"],
    [2, "The request worked"],
  ] as const) {
    await quiz.getByTestId(`question-${q}`).getByText(option).click();
  }
  await quiz.getByTestId("quiz-submit").click();
  await expect(quiz.getByTestId("quiz-score")).toContainText("3 of 3");
  // A perfect quiz earns a badge.
  await expect(page.getByTestId("celebration")).toContainText("Perfect Score");
  await page.getByTestId("celebration-close").click();
  await expect(lesson).toContainText("Completed");

  // Progress shows on the course map and the badges page.
  await page.getByTestId("lesson-back").click();
  await expect(page.getByTestId("academy-xp")).not.toContainText(/^0 /);
  await page.getByTestId("academy-badges").click();
  await expect(page.getByTestId("badge-perfect-score")).toContainText("Earned");

  // The workbench is one click away, and the guide folds.
  await page.getByTestId("mode-workbench").click();
  await expect(guide).toBeVisible();
  await page.getByRole("button", { name: "Fold the Lab Guide" }).click();
  await expect(page.getByTestId("lab-guide-unfold")).toBeVisible();
  await page.getByTestId("lab-guide-unfold").click();
  await expect(guide).toBeVisible();
});

test("the Bootcamp is on the welcome screen and pinned in the workspace menu", async ({ page }) => {
  await page.getByTestId("workspace-menu").click();
  const bootcamp = page.getByRole("menuitem", { name: /Training Bootcamp/ });
  await expect(bootcamp).toBeVisible();
  await bootcamp.click();
  await expect(page.getByTestId("workspace-menu")).toContainText("Training Bootcamp");
  await expect(page.getByTestId("academy")).toBeVisible();
  // It is never listed among the recent workspaces, only pinned.
  await page.getByTestId("workspace-menu").click();
  await expect(page.getByRole("menuitem", { name: /Training Bootcamp/ })).toHaveCount(1);
  await page.getByRole("menuitem", { name: "Close workspace" }).click();
  await expect(page.getByTestId("welcome-bootcamp")).toBeVisible();
  await expect(page.getByTestId("mode-switch")).toHaveCount(0);
  // The welcome art is not cut off: the page starts at the top.
  const art = await page.getByTestId("welcome-art").boundingBox();
  expect(art!.y).toBeGreaterThanOrEqual(0);
  await page.getByTestId("welcome-bootcamp").click();
  await expect(page.getByTestId("academy")).toBeVisible();
});

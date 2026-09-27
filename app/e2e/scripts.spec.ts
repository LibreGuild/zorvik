import { expect, test, type Page } from "@playwright/test";

const HTTP = "http://127.0.0.1:18787";

async function newHttpTab(page: Page, url: string) {
  await page.getByRole("button", { name: "New tab" }).click();
  await page.getByRole("menuitem", { name: "HTTP request" }).click();
  await page.getByLabel("URL", { exact: true }).fill(url);
}

/** Type into the Scripts tab's editor for one script. */
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
});

test("pre-request script sets a variable and post-response tests are listed", async ({ page }) => {
  await newHttpTab(page, `${HTTP}/echo?from={{fromScript}}`);
  await writeScript(page, "Pre-request", "pm.variables.set('fromScript', 'e2e');\nconsole.log('pre ran');");
  await expect(page.getByTestId("scripts-dot")).toBeVisible();
  await writeScript(
    page,
    "Post-response",
    "pm.test('status is 200', () => pm.response.to.have.status(200));\npm.test('echoed', () => pm.expect(pm.response.json().args.from).to.equal('nope'));",
  );
  await sendRequest(page);
  await expect(page.getByTestId("response-status")).toHaveText("200 OK");

  await page.getByRole("tab", { name: /^Tests/ }).click();
  await expect(page.getByRole("tab", { name: /^Tests/ })).toContainText("Tests 1/2");
  await expect(page.getByTestId("tests")).toContainText("status is 200");
  await expect(page.getByTestId("tests")).toContainText("expected 'e2e' to equal 'nope'");

  await page.getByRole("tab", { name: /^Console/ }).click();
  await expect(page.getByTestId("console")).toContainText("pre ran");
});

test("a pre-request error stops the send", async ({ page }) => {
  await newHttpTab(page, `${HTTP}/echo`);
  await writeScript(page, "Pre-request", "const a = 1;\nnotDefined();");
  await sendRequest(page);
  await expect(page.getByText("Pre-request script failed", { exact: true })).toBeVisible();
  await expect(page.getByText(/failed at line 2: ReferenceError/)).toBeVisible();
});

test("pm.environment values show under Set by scripts and can be cleared", async ({ page }) => {
  // An active environment is needed for pm.environment.set values to be kept.
  await page.getByTestId("env-picker").click();
  await page.getByRole("menuitem", { name: "Manage environments…" }).click();
  await page.getByRole("button", { name: "New environment" }).click();
  await page.getByLabel("Environment name").fill("Scripted");
  await page.getByRole("button", { name: "Save changes" }).click();
  await page.getByRole("button", { name: "Close", exact: true }).last().click();
  await page.getByTestId("env-picker").click();
  await page.getByRole("menuitem", { name: /Scripted/ }).click();

  await newHttpTab(page, `${HTTP}/json`);
  await writeScript(page, "Post-response", "pm.environment.set('seenName', pm.response.json().name);");
  await sendRequest(page);
  await expect(page.getByTestId("response-status")).toHaveText("200 OK");

  await page.getByTestId("env-picker").click();
  await page.getByRole("menuitem", { name: "Manage environments…" }).click();
  await page.getByRole("dialog").getByRole("button", { name: /Scripted/ }).click();
  const local = page.getByTestId("local-values");
  await expect(local).toContainText("seenName");
  await expect(local).toContainText("Zorvik");
  await local.getByRole("button", { name: "Clear seenName" }).click();
  await expect(page.getByTestId("local-values")).toHaveCount(0);
});

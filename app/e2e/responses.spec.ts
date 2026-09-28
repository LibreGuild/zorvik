// Response filters (JSONPath, jq, XPath), saved examples and the new auth types' forms.
import { expect, test, type Page } from "@playwright/test";

const HTTP = "http://127.0.0.1:18787";

async function send(page: Page, url: string) {
  await page.getByRole("button", { name: "New tab" }).click();
  await page.getByRole("menuitem", { name: "HTTP request" }).click();
  await page.getByLabel("URL", { exact: true }).fill(url);
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await expect(page.getByTestId("response-status")).toHaveText("200 OK");
}

test.beforeEach(async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("workspace-menu")).toContainText("E2E Workspace");
});

test("filters a JSON body with JSONPath and jq, over the whole body", async ({ page }) => {
  await send(page, `${HTTP}/big-json?n=2000`);
  await page.getByRole("button", { name: "Filter (JSONPath, jq, XPath)" }).click();
  const filter = page.getByTestId("response-filter");
  await filter.getByLabel("JSONPath expression").fill("$.items[?@.index > 1997].name");
  await expect(page.getByTestId("response-filter-status")).toHaveText("2 matches");
  await expect(page.getByTestId("response")).toContainText('"item-1999"');

  await filter.getByRole("button", { name: "jq" }).click();
  await filter.getByLabel("jq expression").fill("[.items[] | select(.active)] | length");
  await expect(page.getByTestId("response-filter-status")).toHaveText("1 match");
  await expect(page.getByTestId("response")).toContainText("1000");

  await filter.getByLabel("jq expression").fill(".items[");
  await expect(page.getByTestId("response-filter-status")).toContainText("Not a valid jq expression");

  await filter.getByRole("button", { name: "Close filter" }).click();
  await expect(page.getByTestId("response")).toContainText('"count": 2000');
});

test("filters an XML body with XPath", async ({ page }) => {
  await send(page, `${HTTP}/xml`);
  await page.getByRole("button", { name: "Filter (JSONPath, jq, XPath)" }).click();
  await page.getByLabel("XPath expression").fill("/note/from/text()");
  await expect(page.getByTestId("response-filter-status")).toHaveText("1 match");
  await expect(page.getByTestId("response")).toContainText("Zorvik");
});

test("saves a response as an example of the request", async ({ page }) => {
  await send(page, `${HTTP}/json`);
  await page.getByRole("button", { name: /^Save/ }).first().click();
  await page.getByLabel("Name").fill("Example source");
  await page.getByRole("button", { name: "Save", exact: true }).click();
  await expect(page.getByTestId("collection-tree")).toContainText("Example source");

  await page.getByTestId("save-example").click();
  await expect(page.getByText("Saved as an example")).toBeVisible();
  await page.getByRole("tab", { name: /^Examples/ }).click();
  const examples = page.getByTestId("examples");
  await expect(examples.getByLabel("Example name")).toHaveValue("200 OK");
  await expect(examples).toContainText("Content-Type");
  await expect(examples).not.toContainText("Date:");
  await expect(examples).toContainText('"name":"Zorvik"');
});

test("the auth editor has every auth type", async ({ page }) => {
  await page.getByRole("button", { name: "New tab" }).click();
  await page.getByRole("menuitem", { name: "HTTP request" }).click();
  await page.getByRole("tab", { name: /^Auth/ }).click();
  const type = page.locator("select").first();
  for (const [value, field] of [
    ["awsSigV4", "Region"],
    ["oauth1", "Consumer key"],
    ["jwt", "Payload"],
    ["digest", "Username"],
    ["ntlm", "Domain"],
    ["hawk", "Hawk ID"],
    ["akamaiEdgeGrid", "Client token"],
    ["asap", "Key ID"],
  ] as const) {
    await type.selectOption(value);
    await expect(page.getByText(field, { exact: true }).first()).toBeVisible();
  }
});

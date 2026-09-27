import { expect, test, type Page } from "@playwright/test";

const HTTP = "http://127.0.0.1:18787";

async function newGraphql(page: Page, url: string) {
  await page.getByRole("button", { name: "New tab" }).click();
  await page.getByRole("menuitem", { name: "GraphQL request" }).click();
  await page.getByLabel("URL", { exact: true }).fill(url);
}

/** Replace the query editor's text (no key events: no auto-closed brackets or completions). */
async function setQuery(page: Page, text: string) {
  const editor = page.getByTestId("graphql-query").locator(".cm-content");
  await editor.click();
  await page.keyboard.press("ControlOrMeta+A");
  await page.keyboard.insertText(text);
}

test.beforeEach(async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("workspace-menu")).toContainText("E2E Workspace");
});

test("GraphQL: schema loads by itself, completes, validates and sends", async ({ page }) => {
  await newGraphql(page, `${HTTP}/graphql`);
  // Opens on the Body tab, and the schema comes from introspection.
  await expect(page.getByTestId("graphql-query")).toBeVisible();
  await expect(page.getByRole("button", { name: /^Schema: Schema loaded · 9 types/ })).toBeVisible();

  const editor = page.getByTestId("graphql-query").locator(".cm-content");
  await editor.click();
  await page.keyboard.type("{ hel");
  // Click, not Enter: CodeMirror ignores Enter for 75 ms after the list opens,
  // and a fast test can press it inside that window (a newline instead).
  await page.locator(".cm-tooltip-autocomplete li", { hasText: "hello" }).first().click();
  await expect(editor).toContainText("{ hello");

  // Unknown fields are flagged.
  await setQuery(page, "{ nope }");
  await expect(page.locator(".cm-lintRange-error").first()).toBeVisible();

  // Two operations: pick one, with variables.
  await setQuery(page, "query A { hello } query B($id: ID!) { user(id: $id) { name email } }");
  await page.getByLabel("Operation").selectOption("B");
  const variables = page.getByTestId("graphql-variables").locator(".cm-content");
  await variables.click();
  await page.keyboard.insertText('{"id": "2"}');
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await expect(page.getByTestId("response-status")).toHaveText("200 OK");
  await expect(page.getByTestId("response")).toContainText("Alan Turing");

  // Prettify reformats the query.
  await page.getByRole("button", { name: "Prettify" }).click();
  await expect(page.getByTestId("graphql-query")).toContainText("query B($id: ID!) {");
});

test("GraphQL: schema panel, errors and the sidebar badge", async ({ page }) => {
  await newGraphql(page, `${HTTP}/graphql`);
  await page.getByRole("button", { name: /^Schema: / }).click();
  const panel = page.getByTestId("graphql-schema");
  await expect(panel.getByTestId("graphql-schema-status")).toContainText("Schema loaded");
  await panel.getByRole("button", { name: "Query", exact: true }).first().click();
  await expect(panel.getByTestId("graphql-schema-field").first()).toContainText("hello(name: String");
  await expect(panel).toContainText("Deprecated: Use `hello`.");
  await panel.getByRole("button", { name: "User", exact: true }).first().click();
  await expect(panel.getByTestId("graphql-schema-type")).toContainText("posts");
  await panel.getByLabel("Search schema").fill("echo");
  await expect(panel.getByRole("button", { name: /Mutation\.echo/ })).toBeVisible();

  // An endpoint that wants credentials: the error is shown, then auth fixes it.
  await page.getByLabel("URL", { exact: true }).fill(`${HTTP}/graphql-auth`);
  await expect(panel.getByTestId("graphql-schema-status")).toContainText("Not authenticated");

  await setQuery(page, "{ hello }");
  await page.getByRole("button", { name: /^Save/ }).first().click();
  await page.getByLabel("Name").fill("Hello GraphQL");
  await page.getByRole("button", { name: "Save", exact: true }).click();
  const row = page.getByTestId("collection-tree").getByText("Hello GraphQL").locator("..");
  await expect(row).toContainText("GQL");
});

// The offline tools: JSON compare (with a response from history), the filter playground,
// the regex tester and the certificate decoder (pasted, and from a file).
import { expect, test, type Locator, type Page } from "@playwright/test";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

// The test servers (playwright.config.ts starts them on 18787).
const HTTP = process.env.ZV_E2E_HTTP ?? "http://127.0.0.1:18787";

// A throwaway self-signed CA certificate (public; no key), valid until 2036.
const ROOT_PEM = `-----BEGIN CERTIFICATE-----
MIIBhDCCASmgAwIBAgIUFWiG9dmpUvyzQ9ssto29ICNtywcwCgYIKoZIzj0EAwIw
FzEVMBMGA1UEAwwMRGVtbyBSb290IENBMB4XDTI2MTAxOTE4MTUyN1oXDTM2MTAx
NjE4MTUyN1owFzEVMBMGA1UEAwwMRGVtbyBSb290IENBMFkwEwYHKoZIzj0CAQYI
KoZIzj0DAQcDQgAE7XGVy81rv87vcu99n4Z5SAgfSXNY3lBTalO1PSf8tbAoluZI
VKzvkqbuubOhmzTOEoKyByHdiFCZFjwU9fFfZ6NTMFEwHQYDVR0OBBYEFMLmbtw1
5jtJvo4qEjYCkcTj8cmBMB8GA1UdIwQYMBaAFMLmbtw15jtJvo4qEjYCkcTj8cmB
MA8GA1UdEwEB/wQFMAMBAf8wCgYIKoZIzj0EAwIDSQAwRgIhANPTPrDBrgia4hj2
/7j8RJxJlZDXXKFBnfboUu5FfRa4AiEAxtzpNQUiaPIJQ/hWeOKDW7FnxExZXTmB
JjkWAxXFujQ=
-----END CERTIFICATE-----
`;

async function openTool(page: Page, name: RegExp) {
  await page.getByTestId("rail-tools").click();
  await page.getByTestId("tools-list").getByRole("button", { name }).click();
}

/** Replace an editor's text (no key events: no auto-closed brackets). */
async function setEditor(page: Page, editor: Locator, text: string) {
  const content = editor.locator(".cm-content");
  await content.click();
  await page.keyboard.press("ControlOrMeta+A");
  await page.keyboard.insertText(text);
}

test.beforeEach(async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("workspace-menu")).toContainText("E2E Workspace");
});

test("JSON compare lists what changed between a response and an edited copy", async ({ page }) => {
  await page.getByRole("button", { name: "New tab" }).click();
  await page.getByRole("menuitem", { name: "HTTP request" }).click();
  await page.getByLabel("URL", { exact: true }).fill(`${HTTP}/json`);
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await expect(page.getByTestId("response-status")).toHaveText("200 OK");

  await openTool(page, /JSON compare/);
  const tool = page.getByTestId("json-compare");
  await expect(tool.getByText("Compare two JSON documents")).toBeVisible();
  // The left side from history.
  await tool.getByRole("button", { name: "From history" }).first().click();
  await page.getByRole("menuitem", { name: /\/json/ }).first().click();
  await expect(page.getByTestId("json-left").locator(".cm-content")).toContainText('"name": "Zorvik"');
  // The right side typed: key order, formatting and 19.90 vs 19.9 don't count.
  await setEditor(
    page,
    page.getByTestId("json-right"),
    '{"unicode": "héllo ✓", "nested": {"nothing": null, "ok": false}, "tags": ["api"], "price": 19.9, "name": "Zorvik", "id": 42}',
  );
  await expect(page.getByTestId("json-diff-summary")).toHaveText("2 differences");
  const result = page.getByTestId("json-diff-result");
  await expect(result.locator('[data-kind="changed"]')).toContainText("$.nested.ok");
  await expect(result.locator('[data-kind="removed"]')).toContainText("$.tags[1]");

  // Swapping sides turns the removal into an addition.
  await tool.getByRole("button", { name: "Swap sides" }).click();
  await expect(result.locator('[data-kind="added"]')).toContainText("$.tags[1]");
  await tool.getByRole("button", { name: /^Added \d/ }).click();
  await expect(result.getByRole("row")).toHaveCount(2); // header + the addition
});

test("filter playground runs JSONPath, jq and XPath as you type", async ({ page }) => {
  await openTool(page, /Filter playground/);
  const tool = page.getByTestId("filter-playground");
  await setEditor(page, page.getByTestId("filter-input"), '{"items": [{"id": 1, "price": 9.5, "name": "Dune"}, {"id": 2, "price": 12, "name": "Emma"}]}');
  await tool.getByRole("button", { name: "$.items[?@.price > 10].name" }).click();
  await expect(page.getByTestId("filter-status")).toHaveText("1 match");
  await expect(page.getByTestId("filter-output")).toContainText('"Emma"');

  await tool.getByRole("button", { name: "jq", exact: true }).click();
  await tool.getByLabel("Expression").fill("[.items[].price] | add");
  await expect(page.getByTestId("filter-output")).toContainText("21.5");
  await tool.getByLabel("Expression").fill(".items[");
  await expect(page.getByTestId("filter-output").getByRole("alert")).toContainText("Not a valid jq expression");

  await tool.getByRole("button", { name: "XPath", exact: true }).click();
  await setEditor(page, page.getByTestId("filter-input"), "<list><item>a</item><item>b</item></list>");
  await tool.getByLabel("Expression").fill("count(//item)");
  await expect(page.getByTestId("filter-status")).toHaveText("1 match");
  await expect(page.getByTestId("filter-output")).toContainText("2");
});

test("regex tester highlights matches and shows what a capture takes", async ({ page }) => {
  await openTool(page, /Regex tester/);
  const tool = page.getByTestId("regex-tester");
  await setEditor(page, page.getByTestId("regex-text"), "token=Ab12\nTOKEN=zz99\nnothing here");
  await tool.getByLabel("Pattern", { exact: true }).fill(String.raw`token=(?P<value>\w+)`);
  await expect(page.getByTestId("regex-status")).toHaveText("1 match");
  await expect(page.getByTestId("regex-capture")).toHaveText("Ab12");
  await expect(page.getByTestId("regex-text").locator(".cm-rx-match")).toHaveCount(1);

  await tool.getByRole("button", { name: "Ignore case" }).click();
  await expect(page.getByTestId("regex-status")).toHaveText("2 matches");
  await expect(tool.getByText(String.raw`(?i)token=(?P<value>\w+)`)).toBeVisible();
  await expect(page.getByTestId("regex-matches").locator("tbody tr")).toHaveCount(2);

  // Rust syntax: look-around is refused with the regex crate's explanation.
  await tool.getByLabel("Pattern", { exact: true }).fill("(?=token)");
  await expect(tool.getByRole("alert")).toContainText("look-around");
});

test("certificate decoder reads pasted PEM and a file", async ({ page }) => {
  await openTool(page, /Certificate decoder/);
  const tool = page.getByTestId("cert-decoder");
  await tool.getByLabel("PEM text").fill(ROOT_PEM);
  const report = page.getByTestId("cert-report");
  await expect(report).toContainText("1 · Root certificate");
  await expect(report).toContainText("Demo Root CA");
  await expect(report).toContainText("EC · P-256 · 256 bits");

  await tool.getByLabel("PEM text").fill("not a certificate");
  await expect(tool.getByRole("alert")).toContainText("PEM format");

  // A file (the browser build asks for its path).
  const file = path.join(os.tmpdir(), `zorvik-e2e-root-${process.pid}.pem`);
  fs.writeFileSync(file, ROOT_PEM);
  page.once("dialog", (d) => void d.accept(file));
  await tool.getByRole("button", { name: "Open file…" }).click();
  await expect(tool.getByText(path.basename(file))).toBeVisible();
  await expect(report).toContainText("Demo Root CA");
  await tool.getByRole("button", { name: "Paste text instead" }).click();
  await expect(tool.getByLabel("PEM text")).toBeVisible();
  fs.rmSync(file, { force: true });
});

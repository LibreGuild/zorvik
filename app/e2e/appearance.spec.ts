import { expect, test } from "@playwright/test";

async function rpc<T>(method: string, params: unknown = {}): Promise<T> {
  const res = await fetch("http://127.0.0.1:18799/bridge/rpc", {
    method: "POST",
    headers: { "content-type": "application/json", "x-zorvik-bridge": "1" },
    body: JSON.stringify({ method, params }),
  });
  const data = (await res.json()) as { result?: T; error?: { message: string } };
  if (data.error) throw new Error(data.error.message);
  return data.result as T;
}

type Settings = Record<string, any>;

// ⌘+ / ⌘− / ⌘0 zoom (outside Tauri the page uses CSS zoom) and the fonts in Settings → General.
test("zoom keys and font settings", async ({ page }) => {
  const before = await rpc<Settings>("settings.get");
  try {
    await page.goto("/");
    await expect(page.getByTestId("workspace-menu")).toContainText("E2E Workspace");
    const zoom = () => page.evaluate(() => document.documentElement.style.zoom);
    const cssVar = (name: string) => page.evaluate((n) => document.documentElement.style.getPropertyValue(n), name);
    const saved = async () => (await rpc<Settings>("settings.get")).appearance;

    await page.keyboard.press("ControlOrMeta+Equal");
    await expect.poll(zoom).toBe("1.1");
    await page.keyboard.press("ControlOrMeta+Minus");
    await page.keyboard.press("ControlOrMeta+Minus");
    await expect.poll(zoom).toBe("0.9");
    await expect.poll(async () => (await saved()).zoom).toBe(90);

    // The dialog shows the zoom and changes it at once.
    await page.keyboard.press("ControlOrMeta+Comma");
    const dialog = page.getByRole("dialog", { name: "Settings" });
    await expect(dialog.getByRole("combobox", { name: "Zoom" })).toHaveValue("90");
    await dialog.getByRole("button", { name: "Reset" }).click();
    await expect.poll(zoom).toBe("");
    await expect.poll(async () => (await saved()).zoom).toBe(100);

    // Fonts preview while the dialog is open; Cancel puts the saved ones back.
    await dialog.getByLabel("Code font").fill("Courier New");
    await expect.poll(() => cssVar("--code-font-pick")).toBe('"Courier New"');
    await dialog.getByRole("button", { name: "Cancel" }).click();
    await expect.poll(() => cssVar("--code-font-pick")).toBe("");

    await page.keyboard.press("ControlOrMeta+Comma");
    await dialog.getByLabel("Code text size").selectOption("16");
    await dialog.getByRole("button", { name: "Save" }).click();
    await expect(dialog).toBeHidden();
    await expect.poll(() => cssVar("--code-size")).toBe("16px");
    expect((await saved()).codeFontSize).toBe(16);

    // Remembered for the next start, before settings load.
    await page.reload();
    await expect.poll(() => cssVar("--code-size")).toBe("16px");
  } finally {
    await rpc("settings.save", { settings: before });
  }
});

// A short window: the dialog's panes shrink, so only the open section scrolls (one scrollbar, not two).
test("settings scroll in one place on a short window", async ({ page }) => {
  await page.setViewportSize({ width: 1100, height: 600 });
  await page.goto("/");
  await expect(page.getByTestId("workspace-menu")).toContainText("E2E Workspace");
  await page.keyboard.press("ControlOrMeta+,");
  const dialog = page.getByRole("dialog", { name: "Settings" });
  await expect(dialog).toBeVisible();
  const scrolls = (testId: string) =>
    dialog.getByTestId(testId).evaluate((el) => {
      // The element itself, and the dialog's body around the panes.
      const overflows = (e: Element) => e.scrollHeight > e.clientHeight + 1;
      return { self: overflows(el), around: el.closest('[data-testid="settings-panes"]')!.parentElement!.scrollHeight > el.closest('[data-testid="settings-panes"]')!.parentElement!.clientHeight + 1 };
    });
  // General is longer than the window leaves room for: it scrolls, the dialog around it doesn't.
  expect(await scrolls("settings-section")).toEqual({ self: true, around: false });
  // The whole dialog fits the window, with its buttons.
  const box = (await dialog.boundingBox())!;
  expect(box.y + box.height).toBeLessThanOrEqual(600);
  await expect(dialog.getByRole("button", { name: "Save", exact: true })).toBeInViewport();
  // The section scrolls to its end.
  const section = dialog.getByTestId("settings-section");
  await section.evaluate((el) => el.scrollTo(0, el.scrollHeight));
  expect(await section.evaluate((el) => el.scrollTop)).toBeGreaterThan(0);
});

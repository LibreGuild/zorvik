import { expect, type Page, test } from "@playwright/test";

// Mock API answer sequences, JSON body conditions and templates, recording a backend's
// answers as routes, relay faults, event stream reconnects and Socket.IO scheduled events.
// Fixed, uncommon ports.
const SEQUENCE_PORT = 18941;
const BACKEND_PORT = 18942;
const FRONT_PORT = 18943;

test.beforeEach(async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("workspace-menu")).toContainText("E2E Workspace");
});

test.afterEach(async ({ page }) => {
  const running = page.getByTestId("running-servers");
  if (await running.count()) {
    await running.click();
    await page.getByRole("menuitem", { name: "Stop all", exact: true }).click();
    // Several running servers ask first.
    const confirm = page.getByRole("dialog").filter({ hasText: /Stop \d+ servers\?/ });
    await expect(async () => {
      if (await confirm.isVisible()) await confirm.getByRole("button", { name: "Stop all" }).click();
      await expect(running).toHaveCount(0, { timeout: 500 });
    }).toPass({ timeout: 10_000 });
  }
});

/** Create a server (not started) and save it on `port`. */
async function createServer(page: Page, kind: string, name: string, port?: number) {
  await page.getByTestId("rail-servers").click();
  await page.getByRole("button", { name: "New server" }).first().click();
  await page.getByRole("menuitem", { name: kind }).click();
  await page.getByRole("textbox").last().fill(name);
  await page.getByRole("button", { name: "Create" }).click();
  // The new server's view, not the one open before.
  await expect(page.getByRole("tab", { selected: true })).toContainText(name);
  await expect(page.getByTestId("server-view")).toBeVisible();
  if (port !== undefined) {
    await page.getByLabel("Port").fill(String(port));
    await page.keyboard.press("ControlOrMeta+s");
    await expect(page.getByTestId(`server-row-${name}`)).toContainText(`:${port}`);
  }
}

async function start(page: Page, port: number) {
  await page.getByTestId("server-start").click();
  await expect(page.getByTestId("server-status")).toContainText(`127.0.0.1:${port}`);
}

test("an answer sequence fails the first call, then the route answers", async ({ page }) => {
  await createServer(page, "Mock API (HTTP)", "E2E sequence", SEQUENCE_PORT);
  await page.getByRole("button", { name: "Answer sequence" }).click();
  await page.getByRole("button", { name: "Add answer" }).click();
  await expect(page.getByLabel("Call 1 status")).toHaveValue("503");
  await expect(page.getByTestId("route-sequence")).toContainText("The route's own answer (200), from then on");
  await expect(page.getByTestId("mock-route-0").locator("svg.lucide-list-ordered")).toBeVisible();

  // A JSON body condition that can't be read says why; a good one is kept.
  await page.getByRole("button", { name: "Match only when" }).click();
  await page.getByLabel("JSON body condition").fill("$..id");
  await expect(page.getByText(/wildcards and \.\. are not supported/)).toBeVisible();
  await page.getByLabel("JSON body condition").fill("");
  await page.keyboard.press("ControlOrMeta+s");
  await start(page, SEQUENCE_PORT);

  const url = `http://127.0.0.1:${SEQUENCE_PORT}/hello`;
  expect((await page.request.get(url)).status()).toBe(503);
  const ok = await page.request.get(url);
  expect(ok.status()).toBe(200);
  expect(await ok.text()).toContain("Hello from Zorvik");

  // The traffic says which answer each call got.
  const log = page.getByTestId("traffic-log");
  await expect(log.getByText("GET /hello")).toHaveCount(2);
  await log.getByText("GET /hello").first().click();
  await expect(log).toContainText("answer 1 of 2");
});

test("proxied answers are saved as routes", async ({ page }) => {
  // The "real" backend: the default route answers GET /hello.
  await createServer(page, "Mock API (HTTP)", "E2E backend", BACKEND_PORT);
  await start(page, BACKEND_PORT);

  // A mock without routes that forwards everything to it.
  await createServer(page, "Mock API (HTTP)", "E2E front", FRONT_PORT);
  await page.getByTestId("mock-route-0").getByRole("button", { name: "Route actions" }).click();
  await page.getByRole("menuitem", { name: "Delete" }).click();
  await page.getByRole("button", { name: "Forward to a backend" }).click();
  await page.getByLabel("Backend URL").fill(`http://127.0.0.1:${BACKEND_PORT}`);
  await page.keyboard.press("ControlOrMeta+s");
  await start(page, FRONT_PORT);

  const answer = await page.request.get(`http://127.0.0.1:${FRONT_PORT}/hello?lang=en`);
  expect(answer.status()).toBe(200);
  const log = page.getByTestId("traffic-log");
  await expect(log).toContainText("GET /hello?lang=en");
  await log.getByRole("button", { name: "Save as route" }).first().click();

  // The route is added and selected, with the query as a condition, and answers from now on.
  await expect(page.getByTestId("mock-route-0")).toContainText("/hello");
  await expect(page.getByLabel("Path", { exact: true })).toHaveValue("/hello");
  await page.keyboard.press("ControlOrMeta+s");
  await expect(async () => {
    const again = await page.request.get(`http://127.0.0.1:${FRONT_PORT}/hello?lang=en`);
    expect(await again.text()).toContain("Hello from Zorvik");
    await log.getByRole("button", { name: /GET \/hello\?lang=en/ }).last().click();
    await expect(log).toContainText("route GET /hello", { timeout: 1000 });
  }).toPass({ timeout: 10_000 });

  // Everything forwarded already has a route now.
  await page.getByRole("button", { name: "Save proxied answers as routes" }).click();
  await expect(page.getByText("No new answers to save")).toBeVisible();
});

test("relay faults, event stream reconnects and Socket.IO scheduled events are set in their editors", async ({ page }) => {
  await createServer(page, "TCP relay", "E2E slow relay");
  await page.getByRole("button", { name: "Faults" }).click();
  await page.getByLabel("Delay per chunk (ms)").fill("250");
  await page.getByLabel("Drop after (bytes)").fill("1000");
  await page.keyboard.press("ControlOrMeta+s");
  await page.getByRole("button", { name: "Faults" }).click();
  await expect(page.getByRole("button", { name: "Faults" })).toContainText("2");

  await createServer(page, "Event stream (SSE) server", "E2E reconnects");
  // New event streams repeat; ending after the last event needs Repeat off.
  await page.getByRole("switch", { name: "Repeat" }).click();
  await page.getByLabel("Retry in milliseconds").fill("1500");
  await page.getByRole("switch", { name: "End after the last event" }).click();
  await expect(page.getByRole("switch", { name: "End after the last event" })).toHaveAttribute("aria-checked", "true");
  await page.keyboard.press("ControlOrMeta+s");
  await expect(page.getByLabel("Retry in milliseconds")).toHaveValue("1500");

  await createServer(page, "Socket.IO server", "E2E ticker");
  await page.getByRole("button", { name: "Add scheduled event" }).click();
  await page.getByLabel("Scheduled event", { exact: true }).fill("price");
  await page.getByLabel("Scheduled event arguments").fill('{"price": {{$randomPrice(5, 50)}}}');
  await page.getByLabel("Scheduled event namespace").fill("/prices");
  await page.getByLabel("Every (ms)").fill("500");
  await page.keyboard.press("ControlOrMeta+s");
  await expect(page.getByLabel("Scheduled event namespace")).toHaveValue("/prices");
});

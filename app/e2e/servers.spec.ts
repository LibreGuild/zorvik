import { expect, test } from "@playwright/test";

// A fixed, uncommon port: the server is created and started through the UI.
const PORT = 18911;

test.beforeEach(async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("workspace-menu")).toContainText("E2E Workspace");
});

test("a TCP server runs in the background and a TCP client talks to it", async ({ page }) => {
  // Create the server from the Servers section.
  await page.getByTestId("rail-servers").click();
  await page.getByRole("button", { name: "New server" }).first().click();
  await page.getByRole("menuitem", { name: "TCP server" }).click();
  await page.getByRole("textbox").last().fill("E2E echo");
  await page.getByRole("button", { name: "Create" }).click();
  await expect(page.getByTestId("server-view")).toBeVisible();
  await page.getByLabel("Port").fill(String(PORT));
  await page.keyboard.press("ControlOrMeta+s");
  await expect(page.getByTestId("server-row-E2E echo")).toContainText(`:${PORT}`);
  await page.getByTestId("server-start").click();
  await expect(page.getByTestId("server-status")).toContainText(`tcp://127.0.0.1:${PORT}`);
  await expect(page.getByTestId("running-servers")).toHaveText(/1 running/);

  // Closing the server's tab keeps it running.
  await page.getByRole("tab", { name: /E2E echo/ }).getByRole("button", { name: "Close tab" }).click();
  await expect(page.getByTestId("running-servers")).toHaveText(/1 running/);

  // A TCP client request.
  await page.getByRole("button", { name: "New tab" }).click();
  await page.getByRole("menuitem", { name: "TCP connection" }).click();
  await page.getByLabel("URL", { exact: true }).fill(`tcp://127.0.0.1:${PORT}`);
  await page.getByRole("button", { name: "Connect" }).click();
  await expect(page.getByTestId("stream-status")).toHaveText("Connected");
  await page.locator('[data-testid="stream-pane"] .cm-content').click();
  await page.keyboard.type("hello over tcp");
  await page.getByRole("button", { name: "Send", exact: true }).click();
  const log = page.getByTestId("message-log");
  await expect(log.getByText("hello over tcp")).toHaveCount(2); // sent + echoed

  // The server shows the traffic.
  await page.getByTestId("rail-servers").click();
  await page.getByTestId("server-row-E2E echo").click();
  await expect(page.getByTestId("traffic-log")).toContainText("hello over tcp");
  await expect(page.getByTestId("server-stats")).toContainText("1 open");

  // Stop everything from the title bar.
  await page.getByTestId("running-servers").click();
  await page.getByRole("menuitem", { name: "Stop all", exact: true }).click();
  await expect(page.getByTestId("running-servers")).toHaveCount(0);
  await expect(page.getByTestId("server-status")).toContainText("Stopped");
});

async function createServer(page: import("@playwright/test").Page, kind: string, name: string, port: number) {
  await page.getByTestId("rail-servers").click();
  await page.getByRole("button", { name: "New server" }).first().click();
  await page.getByRole("menuitem", { name: kind }).click();
  await page.getByRole("textbox").last().fill(name);
  await page.getByRole("button", { name: "Create" }).click();
  await expect(page.getByTestId("server-view")).toBeVisible();
  await page.getByLabel("Port").fill(String(port));
  await page.keyboard.press("ControlOrMeta+s");
  await expect(page.getByTestId(`server-row-${name}`)).toContainText(`:${port}`);
  await page.getByTestId("server-start").click();
  await expect(page.getByTestId("server-status")).toContainText(`127.0.0.1:${port}`);
}

test("a mock API answers requests and logs them", async ({ page }) => {
  await createServer(page, "Mock API (HTTP)", "E2E mock", 18912);
  await page.getByRole("button", { name: "New tab" }).click();
  await page.getByRole("menuitem", { name: "HTTP request" }).click();
  await page.getByLabel("URL", { exact: true }).fill("http://127.0.0.1:18912/hello");
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await expect(page.getByTestId("response-status")).toHaveText("200 OK");
  await expect(page.getByTestId("response")).toContainText("Hello from Zorvik");
  await page.getByTestId("server-row-E2E mock").click();
  await expect(page.getByTestId("traffic-log")).toContainText("GET /hello");
  await page.getByTestId("server-stop").click();
  await expect(page.getByTestId("server-status")).toContainText("Stopped");
});

test("the DNS lookup tool queries a local DNS server", async ({ page }) => {
  await createServer(page, "DNS server", "E2E dns", 18913);
  await page.getByTestId("rail-tools").click();
  await page.getByRole("button", { name: /DNS lookup/ }).click();
  await page.getByLabel("Name to look up").fill("api.example.test");
  await page.getByRole("radio", { name: "Custom…" }).click();
  await page.getByLabel("DNS server").fill("127.0.0.1:18913");
  await page.getByRole("button", { name: "Look up", exact: true }).click();
  await expect(page.getByTestId("dns-rcode")).toHaveText("NOERROR");
  await expect(page.getByTestId("dns-result")).toContainText("127.0.0.1");
  await page.getByTestId("running-servers").click();
  await page.getByRole("menuitem", { name: "Stop all", exact: true }).click();
  await expect(page.getByTestId("running-servers")).toHaveCount(0);
});

test("the encoders tool converts base64", async ({ page }) => {
  await page.getByTestId("rail-tools").click();
  await page.getByRole("button", { name: /Encoders/ }).click();
  await page.getByLabel("Input", { exact: true }).fill("hello");
  await expect(page.getByLabel("Output", { exact: true })).toHaveValue("aGVsbG8=");
});

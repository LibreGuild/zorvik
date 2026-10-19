import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { expect, test, type Page } from "@playwright/test";

// Binary streams from the composer to the test server's WebSocket echo: a file in paced
// messages, and the microphone (Chromium's fake device, a beeping tone).
const WS = "ws://127.0.0.1:18787";

test.use({
  launchOptions: [
    async ({ browserName }, use) =>
      use(browserName === "chromium" ? { args: ["--use-fake-device-for-media-stream", "--use-fake-ui-for-media-stream"] } : {}),
    { scope: "worker" },
  ],
  permissions: async ({ browserName }, use) => use(browserName === "chromium" ? ["microphone"] : []),
});

test.beforeEach(async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("workspace-menu")).toContainText("E2E Workspace");
});

/** A 16 kHz mono 16-bit WAV file of `ms` milliseconds. */
function wavFile(ms: number): string {
  const data = 32 * ms;
  const header = Buffer.alloc(44);
  header.write("RIFF", 0);
  header.writeUInt32LE(36 + data, 4);
  header.write("WAVEfmt ", 8);
  header.writeUInt32LE(16, 16);
  header.writeUInt16LE(1, 20);
  header.writeUInt16LE(1, 22);
  header.writeUInt32LE(16000, 24);
  header.writeUInt32LE(32000, 28);
  header.writeUInt16LE(2, 32);
  header.writeUInt16LE(16, 34);
  header.write("data", 36);
  header.writeUInt32LE(data, 40);
  const file = path.join(fs.mkdtempSync(path.join(os.tmpdir(), "zorvik-e2e-stream-")), "voice.wav");
  fs.writeFileSync(file, Buffer.concat([header, Buffer.alloc(data, 1)]));
  return file;
}

async function connectedWebSocket(page: Page) {
  await page.getByRole("button", { name: "New tab" }).click();
  await page.getByRole("menuitem", { name: "WebSocket" }).click();
  await page.getByLabel("URL", { exact: true }).fill(`${WS}/ws`);
  await page.getByRole("button", { name: "Connect", exact: true }).click();
  await expect(page.getByTestId("stream-status")).toHaveText("Connected");
  const composer = page.getByTestId("composer");
  await composer.getByRole("button", { name: "Binary", exact: true }).click();
  return composer;
}

test("a WAV file streams in real-time audio messages, once or in a loop", async ({ page }) => {
  const composer = await connectedWebSocket(page);
  await composer.getByRole("button", { name: "File", exact: true }).click();
  const file = wavFile(100);
  page.once("dialog", (d) => void d.accept(file));
  await composer.getByRole("button", { name: "Choose file…" }).click();
  const panel = page.getByTestId("file-stream");
  await expect(panel).toContainText("WAV · PCM 16-bit · 16 kHz · mono");
  // Paced as audio: 20 ms of samples per message, header skipped.
  await expect(panel.getByLabel("Bytes per message")).toHaveValue("640");
  await expect(panel.getByLabel("Interval in milliseconds")).toHaveValue("20");
  await expect(panel.getByLabel("Start byte")).toHaveValue("44");
  await expect(panel).toContainText("31.3 KB/s · 5 messages");

  await composer.getByRole("button", { name: "Start streaming" }).click();
  const progress = page.getByTestId("file-stream-progress");
  await expect(progress).toContainText("Sent");
  await expect(progress).toContainText("3.13 KB in 5 messages");
  // Each message went out and came back from the echo server.
  await expect(page.getByTestId("message-log").getByText("640 B")).toHaveCount(10);

  await panel.getByRole("checkbox", { name: "Loop until stopped" }).click();
  await composer.getByRole("button", { name: "Start streaming" }).click();
  await expect(progress).toContainText("pass 3");
  await composer.getByRole("button", { name: "Stop", exact: true }).click();
  await expect(progress).toContainText("Stopped");
  await expect(composer.getByRole("button", { name: "Start streaming" })).toBeEnabled();
});

test("the microphone streams G.711 μ-law until stopped or disconnected", async ({ page, browserName }) => {
  test.skip(browserName !== "chromium", "needs Chromium's fake microphone");
  const composer = await connectedWebSocket(page);
  await composer.getByRole("button", { name: "Microphone", exact: true }).click();
  await page.getByLabel("Audio format").selectOption("mulaw");
  await expect(page.getByLabel("Sample rate")).toHaveValue("8000");
  const panel = page.getByTestId("mic-stream");
  await expect(panel).toContainText("160 B every 20 ms");

  await composer.getByRole("button", { name: "Start recording" }).click();
  await expect(page.getByTestId("mic-status")).toContainText("Recording");
  await expect(page.getByTestId("message-log").getByText("160 B").nth(5)).toBeVisible();
  await composer.getByRole("button", { name: "Stop recording" }).click();
  await expect(page.getByTestId("mic-status")).toHaveCount(0);

  await composer.getByRole("button", { name: "Start recording" }).click();
  await expect(page.getByTestId("mic-status")).toContainText("Recording");
  await page.getByRole("button", { name: "Disconnect" }).click();
  await expect(page.getByTestId("mic-status")).toHaveCount(0);
  await expect(composer.getByRole("button", { name: "Start recording" })).toBeDisabled();
});

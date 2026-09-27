import { defineConfig } from "@playwright/test";
import os from "node:os";
import path from "node:path";

// Real stack: test servers + Rust API (dev bridge) + Vite UI.
const workspace = process.env.ZV_E2E_WORKSPACE ?? path.join(os.tmpdir(), `zorvik-e2e-${process.pid}`);
process.env.ZV_E2E_WORKSPACE = workspace;
// The dev bridge's data dir: AI agent tests start `zorvik mcp` with it (it holds agent.json).
const dataDir = process.env.ZV_E2E_DATA_DIR ?? path.join(os.tmpdir(), `zorvik-e2e-data-${process.pid}`);
process.env.ZV_E2E_DATA_DIR = dataDir;
const ci = !!process.env.CI;
// Uncommon ports so a developer's own servers don't collide.
const PORTS = { http: 18787, bridge: 18799, ui: 15420 };

export default defineConfig({
  testDir: "e2e",
  timeout: 45_000,
  expect: { timeout: 10_000 },
  fullyParallel: false,
  workers: 1,
  retries: ci ? 1 : 0,
  reporter: ci ? [["list"], ["html", { open: "never" }]] : "list",
  use: {
    baseURL: `http://localhost:${PORTS.ui}`,
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
    viewport: { width: 1400, height: 880 },
  },
  // Chromium matches WebView2 (Windows). WebKit matches WKWebView (macOS): run with E2E_WEBKIT=1.
  projects: [
    { name: "chromium", use: { browserName: "chromium" } },
    ...(process.env.E2E_WEBKIT ? [{ name: "webkit", use: { browserName: "webkit" as const } }] : []),
  ],
  webServer: [
    {
      command: `cargo run -q -p zorvik-testkit -- --port ${PORTS.http}`,
      cwd: "..",
      url: `http://127.0.0.1:${PORTS.http}/`,
      timeout: 300_000,
      reuseExistingServer: !ci,
    },
    {
      command: `cargo run -q -p zorvik-devbridge -- --port ${PORTS.bridge} --workspace "${workspace}" --data-dir "${dataDir}"`,
      cwd: "..",
      url: `http://127.0.0.1:${PORTS.bridge}/bridge/health`,
      timeout: 300_000,
      reuseExistingServer: !ci,
    },
    {
      command: `npx vite --port ${PORTS.ui} --strictPort`,
      env: { ZORVIK_BRIDGE: `http://127.0.0.1:${PORTS.bridge}` },
      url: `http://localhost:${PORTS.ui}`,
      timeout: 120_000,
      reuseExistingServer: !ci,
    },
  ],
});

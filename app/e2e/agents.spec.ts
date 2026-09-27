import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import path from "node:path";
import readline from "node:readline";
import { expect, test } from "@playwright/test";

// An AI agent drives the app through the real `zorvik mcp` (stdio) and the dev bridge's
// agent listener; the user answers its questions in the UI.
const HTTP = "http://127.0.0.1:18787";
const DATA_DIR = process.env.ZV_E2E_DATA_DIR!;
// Playwright runs from app/; the CLI is built into the Cargo target dir (`cargo build -p zorvik-cli`).
const CLI = path.resolve("../target/debug", process.platform === "win32" ? "zorvik.exe" : "zorvik");

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

/** A minimal MCP client over the bridge's stdio. */
class Agent {
  private proc: ChildProcessWithoutNullStreams;
  private next = 1;
  private waiting = new Map<number, (v: Record<string, unknown>) => void>();

  constructor() {
    this.proc = spawn(CLI, ["mcp", "--data-dir", DATA_DIR]);
    readline.createInterface({ input: this.proc.stdout }).on("line", (line) => {
      const msg = JSON.parse(line) as { id?: number };
      if (typeof msg.id === "number") this.waiting.get(msg.id)?.(msg);
    });
  }

  request(method: string, params: unknown = {}): Promise<Record<string, any>> {
    const id = this.next++;
    return new Promise((resolve) => {
      this.waiting.set(id, resolve);
      this.proc.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", id, method, params })}\n`);
    });
  }

  /** A tool call: [isError, text]. */
  async call(name: string, args: unknown = {}): Promise<[boolean, string]> {
    const r = await this.request("tools/call", { name, arguments: args });
    return [r.result.isError === true, r.result.content[0].text as string];
  }

  close() {
    this.proc.stdin.end();
  }
}

test("an agent maps, sends and asks; the user sees it and answers", async ({ page }) => {
  // Start from "agents off, following on", whatever an earlier run left.
  const settings = await rpc<Record<string, any>>("settings.get");
  await rpc("settings.save", { settings: { ...settings, agents: { ...settings.agents, enabled: false, follow: true, changes: "allow", traffic: "askOutside" } } });
  await page.goto("/");
  await expect(page.getByTestId("workspace-menu")).toContainText("E2E Workspace");

  const agent = new Agent();
  await agent.request("initialize", { protocolVersion: "2025-06-18", capabilities: {}, clientInfo: { name: "claude-code", version: "1" } });

  // Off: the first call asks to allow agents.
  const first = agent.call("get_workspace");
  await expect(page.getByRole("dialog")).toContainText("Claude Code wants to control Zorvik");
  await page.getByTestId("agent-allow").click();
  const [err, text] = await first;
  expect(err, text).toBe(false);
  expect(JSON.parse(text).name).toBe("E2E Workspace");
  await expect(page.getByTestId("running-agents")).toContainText("Claude Code");

  // Edits show up in the sidebar, and the saved request opens.
  const folder = `Agent ${Date.now()}`;
  const [saveErr, saved] = await agent.call("save_requests", {
    requests: [{ folder, name: "Echo", url: `${HTTP}/echo?from=agent`, scripts: { postResponse: "pm.test('ok', () => pm.response.to.have.status(200));" } }],
  });
  expect(saveErr, saved).toBe(false);
  await expect(page.getByTestId("collection-tree")).toContainText(folder);
  await expect(page.getByRole("tab", { name: /Echo/, selected: true })).toBeVisible();

  // A local request needs no approval; its response shows in the tab.
  const [sendErr, sent] = await agent.call("send_request", { path: `${folder}/Echo.yaml` });
  expect(sendErr, sent).toBe(false);
  expect(JSON.parse(sent).status).toBe(200);
  await expect(page.getByTestId("response-status")).toHaveText("200 OK");

  // Deletes always ask: the user says no.
  const del = agent.call("delete_items", { paths: [folder] });
  await expect(page.getByTestId("agent-confirm")).toContainText(`Folder ${folder}`);
  await page.getByTestId("agent-deny").click();
  const [delErr, delText] = await del;
  expect(delErr).toBe(true);
  expect(delText).toContain("declined");
  await expect(page.getByTestId("collection-tree")).toContainText(folder);

  // The activity log lists what happened.
  await page.getByTestId("rail-agents").click();
  await expect(page.getByTestId("agent-activity").first()).toContainText("Delete");
  await expect(page.getByTestId("agents-panel")).toContainText("Send a request");

  agent.close();
  await expect(page.getByTestId("running-agents")).toHaveCount(0);
});

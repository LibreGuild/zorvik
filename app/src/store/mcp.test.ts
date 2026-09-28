import { beforeEach, describe, expect, it, vi } from "vitest";
import type { McpCatalog } from "../bindings/McpCatalog";
import type { McpEvent } from "../bindings/McpEvent";
import type { McpServerInfo } from "../bindings/McpServerInfo";
import type { StreamEvent } from "../bindings/StreamEvent";

const handlers: ((e: StreamEvent) => void)[] = [];
vi.mock("../lib/events", () => ({
  onEvent: (h: (e: StreamEvent) => void) => {
    handlers.push(h);
    return () => {};
  },
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("../lib/rpc", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/rpc")>();
  const fn = () => vi.fn((..._args: unknown[]): Promise<unknown> => Promise.resolve(null));
  return { ...actual, api: { mcpConnect: fn(), mcpCatalog: fn(), mcpClose: fn(), mcpProgram: fn(), mcpTrust: fn(), send: fn(), cancel: fn() } };
});
vi.mock("./dialogs", () => ({ confirm: vi.fn(() => Promise.resolve(true)) }));

const { api, RpcError } = await import("../lib/rpc");
const { confirm } = await import("./dialogs");
const { closeTab, newRequestDraft, openDraft, resetTabs, send, updateDraft, useTabs } = await import("./tabs");
const mcp = await import("./mcp");
const { argumentSkeleton, argumentsOf, blankArguments, catalogItems, connectMcp, isProgram, selectCall, templateParams, useMcp } = mcp;
const mocked = api as unknown as Record<string, ReturnType<typeof vi.fn>>;
type Tab = import("./tabs").Tab;

const flush = () => new Promise((r) => setTimeout(r, 70));
const emit = (connId: string, event: McpEvent) => handlers.forEach((h) => h({ type: "mcp", connId, event }));

const info: McpServerInfo = {
  transport: "Streamable HTTP",
  protocolVersion: "2025-11-25",
  name: "weather",
  title: null,
  version: "1.0.0",
  instructions: null,
  capabilities: { tools: {} },
  sessionId: "s1",
  pid: null,
  connectMs: 12,
};

const catalog: McpCatalog = {
  tools: [
    {
      name: "get_weather",
      description: "The weather",
      inputSchema: {
        type: "object",
        properties: { city: { type: "string", description: "City" }, days: { type: "integer", default: 3 }, unit: { enum: ["c", "f"] } },
        required: ["city", "days", "unit"],
      },
    },
    { name: "ping", inputSchema: { type: "object", properties: { loud: { type: "boolean" }, tags: { type: "array", items: { type: "string" } } } } },
  ],
  resources: [{ uri: "docs://readme", name: "readme" }],
  resourceTemplates: [{ uriTemplate: "users://{id}/posts{?page,size}", name: "posts" }],
  prompts: [{ name: "plan", arguments: [{ name: "city", required: true }, { name: "mood" }] }],
  problems: [],
};

function mcpTab(url = "http://localhost:3004/mcp"): Tab {
  const id = openDraft({ ...newRequestDraft("mcp"), url });
  return useTabs.getState().tabs.find((t) => t.id === id) as Tab;
}

const tabOf = (id: string) => useTabs.getState().tabs.find((t) => t.id === id) as Tab;
const draftOf = (id: string) => tabOf(id).draft;

beforeEach(() => {
  resetTabs();
  useMcp.setState({ sessions: {} });
  Object.values(mocked).forEach((m) => m.mockReset());
  mocked.mcpConnect.mockResolvedValue({ info, unresolved: [] });
  mocked.mcpCatalog.mockResolvedValue(catalog);
  mocked.mcpClose.mockResolvedValue(null);
});

describe("helpers", () => {
  it("tells programs from URLs like the backend", () => {
    expect(isProgram("http://localhost:3000/mcp", undefined)).toBe(false);
    expect(isProgram("HTTPS://x/mcp", "auto")).toBe(false);
    expect(isProgram("npx -y server", "auto")).toBe(true);
    expect(isProgram("http://x", "stdio")).toBe(true);
    expect(isProgram("./server", "sse")).toBe(false);
  });

  it("lists what a call can name, with templates", () => {
    expect(catalogItems(catalog, "tool").map((i) => i.name)).toEqual(["get_weather", "ping"]);
    const resources = catalogItems(catalog, "resource");
    expect(resources.map((i) => [i.name, i.template])).toEqual([
      ["docs://readme", false],
      ["users://{id}/posts{?page,size}", true],
    ]);
    expect(catalogItems(null, "prompt")).toEqual([]);
  });

  it("reads arguments from schemas, prompts and templates", () => {
    const tool = catalogItems(catalog, "tool")[0];
    expect(argumentsOf(tool, "tool")).toEqual([
      { name: "city", type: "string", required: true, description: "City" },
      { name: "days", type: "integer", required: true, description: "" },
      { name: "unit", type: '"c" | "f"', required: true, description: "" },
    ]);
    expect(argumentsOf(catalogItems(catalog, "tool")[1], "tool")[1].type).toBe("string[]");
    expect(argumentsOf(catalogItems(catalog, "prompt")[0], "prompt").map((a) => [a.name, a.required])).toEqual([
      ["city", true],
      ["mood", false],
    ]);
    expect(templateParams("users://{id}/posts{?page,size}")).toEqual(["id", "page", "size"]);
    expect(templateParams("{+path}/{name*}")).toEqual(["path", "name"]);
  });

  it("starts arguments from the schema", () => {
    const [weather, ping] = catalogItems(catalog, "tool");
    expect(JSON.parse(argumentSkeleton(weather, "tool"))).toEqual({ city: "", days: 3, unit: "c" });
    // Nothing required: every argument.
    expect(JSON.parse(argumentSkeleton(ping, "tool"))).toEqual({ loud: false, tags: [] });
    expect(JSON.parse(argumentSkeleton(catalogItems(catalog, "prompt")[0], "prompt"))).toEqual({ city: "" });
    expect(blankArguments(" { } ")).toBe(true);
    expect(blankArguments('{"a": 1}')).toBe(false);
  });
});

describe("sessions", () => {
  it("connects, reads the catalog and logs messages", async () => {
    const tab = mcpTab();
    await connectMcp(tab);
    expect(mocked.mcpConnect).toHaveBeenCalledWith(tab.id, tab.draft, null);
    const session = useMcp.getState().sessions[tab.id];
    expect(session.status).toBe("open");
    expect(session.catalog?.tools).toHaveLength(2);

    emit(tab.id, { type: "message", direction: "sent", text: '{"id":1}', method: "tools/call", id: "1", size: 8, timestamp: 1 });
    emit("another-tab", { type: "stderr", text: "not mine", timestamp: 1 });
    await flush();
    expect(useMcp.getState().sessions[tab.id].log.map((e) => e.event.type)).toEqual(["message"]);

    // A list change reloads the catalog.
    mocked.mcpCatalog.mockClear();
    emit(tab.id, { type: "message", direction: "received", text: "{}", method: "notifications/tools/list_changed", id: null, size: 2, timestamp: 2 });
    await flush();
    expect(mocked.mcpCatalog).toHaveBeenCalledTimes(1);

    emit(tab.id, { type: "closed", reason: "The server ended the session" });
    await flush();
    expect(useMcp.getState().sessions[tab.id]).toMatchObject({ status: "closed", error: "The server ended the session" });
  });

  it("asks before starting a program, then connects", async () => {
    const tab = mcpTab("npx -y some-server");
    mocked.mcpConnect.mockRejectedValueOnce(new RpcError({ code: "untrustedProgram", message: "starts a program", networkKind: null }));
    mocked.mcpProgram.mockResolvedValue({ command: "npx -y some-server", cwd: "/ws", env: [{ key: "TOKEN", value: "{{token}}" }], trusted: false });
    await connectMcp(tab);
    expect(confirm).toHaveBeenCalledWith(expect.objectContaining({ danger: true, details: ["Command: npx -y some-server", "Folder: /ws", "Environment: TOKEN={{token}}"] }));
    expect(mocked.mcpTrust).toHaveBeenCalledWith(tab.draft, null);
    expect(mocked.mcpConnect).toHaveBeenCalledTimes(2);
    expect(useMcp.getState().sessions[tab.id].status).toBe("open");
  });

  it("ignores the replaced session's end while connecting, and stops when disconnected meanwhile", async () => {
    const tab = mcpTab("npx -y some-server");
    let answer!: (ok: boolean) => void;
    vi.mocked(confirm).mockImplementationOnce(() => new Promise((resolve) => (answer = resolve)));
    mocked.mcpConnect.mockRejectedValueOnce(new RpcError({ code: "untrustedProgram", message: "starts a program", networkKind: null }));
    mocked.mcpProgram.mockResolvedValue({ command: "npx -y some-server", cwd: "/ws", env: [], trusted: false });
    const connecting = connectMcp(tab);
    await flush();
    // The old program exits while the question is open: still connecting.
    emit(tab.id, { type: "closed", reason: "the program exited" });
    await flush();
    expect(useMcp.getState().sessions[tab.id].status).toBe("connecting");
    // Disconnect, then allow: nothing is started.
    await mcp.disconnectMcp(tab.id);
    answer(true);
    await connecting;
    expect(mocked.mcpConnect).toHaveBeenCalledTimes(1);
    expect(useMcp.getState().sessions[tab.id].status).toBe("closed");
  });

  it("sends again once the program is trusted, and not when refused", async () => {
    const tab = mcpTab("npx -y some-server");
    const untrusted = new RpcError({ code: "untrustedProgram", message: "starts a program", networkKind: null });
    mocked.mcpProgram.mockResolvedValue({ command: "npx -y some-server", cwd: "/ws", env: [], trusted: false });
    mocked.send.mockRejectedValueOnce(untrusted).mockResolvedValueOnce({ meta: { status: 200 }, body: {}, unresolved: [] });
    await send(tab.id);
    expect(mocked.send).toHaveBeenCalledTimes(2);
    expect(tabOf(tab.id).response).toMatchObject({ status: "done" });

    vi.mocked(confirm).mockResolvedValueOnce(false);
    mocked.send.mockRejectedValueOnce(untrusted);
    await send(tab.id);
    expect(tabOf(tab.id).response).toMatchObject({ status: "error", code: "untrustedProgram" });
  });

  it("tries the trust question once per send", async () => {
    const tab = mcpTab("npx -y some-server");
    const untrusted = new RpcError({ code: "untrustedProgram", message: "starts a program", networkKind: null });
    // Already trusted as written, yet refused (a script changes it): no endless retries.
    mocked.mcpProgram.mockResolvedValue({ command: "npx -y some-server", cwd: "/ws", env: [], trusted: true });
    mocked.send.mockRejectedValue(untrusted);
    await send(tab.id);
    expect(mocked.send).toHaveBeenCalledTimes(2);
    expect(tabOf(tab.id).response).toMatchObject({ status: "error", code: "untrustedProgram" });
  });

  it("picks a call and starts its arguments from the schema", () => {
    const tab = mcpTab();
    const [weather] = catalogItems(catalog, "tool");
    selectCall(tab.id, "tool", weather);
    expect(draftOf(tab.id).mcp).toMatchObject({ call: "tool", name: "get_weather" });
    expect(JSON.parse(draftOf(tab.id).mcp?.arguments ?? "")).toEqual({ city: "", days: 3, unit: "c" });
    // Picking it again keeps what was typed.
    updateDraft(tab.id, (r) => ({ ...r, mcp: { ...r.mcp, arguments: '{"city": "Oslo"}' } }));
    selectCall(tab.id, "tool", weather);
    expect(draftOf(tab.id).mcp?.arguments).toBe('{"city": "Oslo"}');
    // A resource without parameters.
    selectCall(tab.id, "resource", catalogItems(catalog, "resource")[0]);
    expect(draftOf(tab.id).mcp).toMatchObject({ call: "resource", name: "docs://readme", arguments: "{}" });
  });

  it("closes a tab's session with the tab", async () => {
    const tab = mcpTab();
    await connectMcp(tab);
    closeTab(tab.id);
    await flush();
    expect(mocked.mcpClose).toHaveBeenCalledWith(tab.id);
    expect(useMcp.getState().sessions[tab.id]).toBeUndefined();
  });
});

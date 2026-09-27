import { beforeEach, describe, expect, it, vi } from "vitest";
import type { StreamEvent } from "../bindings/StreamEvent";
import type { ToolEvent } from "../bindings/ToolEvent";

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
  return { ...actual, api: { portCheck: fn(), ping: fn(), tlsInspect: fn(), netInterfaces: fn(), cancelTool: fn() } };
});

const { api, RpcError } = await import("../lib/rpc");
const { openTool, updateToolState, useTabs } = await import("./tabs");
const tools = await import("./tools");
const { applyPingEvents, applyPortEvents, countPorts, isLocalHost, MAX_REPLIES, PING_DEFAULTS, PORT_DEFAULTS, portSpec, toolState } = tools;
const mocked = api as unknown as Record<string, ReturnType<typeof vi.fn>>;
type PingState = import("./tools").PingState;

const emit = (runId: string, event: ToolEvent) => handlers.forEach((h) => h({ type: "tool", runId, event }));
const flush = () => new Promise((r) => setTimeout(r, 70));

function toolTab(tool: string) {
  openTool(tool);
  const tab = useTabs.getState().tabs.find((t) => t.type === "tool" && t.tool === tool);
  if (!tab || tab.type !== "tool") throw new Error("no tab");
  return tab.id;
}

const tabState = <T extends object>(id: string, defaults: T): T => {
  const tab = useTabs.getState().tabs.find((t) => t.id === id);
  if (!tab || tab.type !== "tool") throw new Error("no tab");
  return toolState(tab, defaults);
};

beforeEach(() => {
  useTabs.setState({ tabs: [], activeId: null });
  Object.values(mocked).forEach((m) => m.mockClear());
});

describe("port lists", () => {
  it("counts distinct ports and explains bad input", () => {
    expect(countPorts("80,443 8000-8010;443")).toBe(13);
    expect(countPorts("1-1024")).toBe(1024);
    expect(countPorts("1-1025")).toMatch(/more than 1024/);
    expect(countPorts("")).toMatch(/at least one/);
    expect(countPorts("0")).toMatch(/out of range/);
    expect(countPorts("70000")).toMatch(/out of range/);
    expect(countPorts("10-5")).toMatch(/start is after/);
    expect(countPorts("http")).toMatch(/not a port/);
    expect(countPorts("1-65535")).toMatch(/more than/);
  });

  it("uses presets unless custom", () => {
    expect(portSpec({ preset: "web", custom: "1" })).toBe("80,443,8080,8443");
    expect(countPorts(portSpec({ preset: "top20", custom: "" }))).toBe(20);
    expect(portSpec({ preset: "custom", custom: "22" })).toBe("22");
  });

  it("knows which hosts are local", () => {
    for (const h of ["localhost", "127.0.0.1", "10.1.2.3", "172.20.0.1", "192.168.1.5", "169.254.0.9", "100.64.1.1", "::1", "[fd00::1]", "fe80::1%en0", "nas.local", "router", "api.internal"]) {
      expect(isLocalHost(h), h).toBe(true);
    }
    for (const h of ["example.com", "8.8.8.8", "172.32.0.1", "2001:4860::8888", "", "11.0.0.1"]) {
      expect(isLocalHost(h), h).toBe(false);
    }
  });
});

describe("interfaces", () => {
  it("suggests an up, routable IPv4 address", () => {
    const iface = (name: string, ip: string, extra: Partial<{ loopback: boolean; up: boolean; linkLocal: boolean }> = {}) => ({
      name,
      loopback: extra.loopback ?? false,
      up: extra.up ?? true,
      addresses: [{ ip, prefix: 24, family: ip.includes(":") ? ("ipv6" as const) : ("ipv4" as const), linkLocal: extra.linkLocal ?? false }],
    });
    expect(tools.bestAddress([iface("lo0", "127.0.0.1", { loopback: true }), iface("en5", "10.0.0.2", { up: false }), iface("awdl0", "fe80::1"), iface("en0", "192.168.1.20")])).toBe(
      "192.168.1.20",
    );
    expect(tools.bestAddress([iface("en0", "169.254.1.1", { linkLocal: true })])).toBeNull();
  });
});

describe("reducers", () => {
  it("applies port events of the current run only", () => {
    const s = { ...PORT_DEFAULTS, runId: "a", status: "running" as const };
    const result: ToolEvent = { type: "portResult", port: 80, open: true, ms: 1, error: null, message: null };
    expect(applyPortEvents(s, "old", [result])).toBe(s);
    const next = applyPortEvents(s, "a", [result, { ...result, port: 81, open: false, error: "refused" }]);
    expect(next.results.map((r) => r.port)).toEqual([80, 81]);
    expect(s.results).toEqual([]);
    const done = applyPortEvents(next, "a", [
      { type: "portsDone", address: "10.0.0.1", open: [80], refused: 1, timedOut: 0, errors: 0, checked: 2, total: 2, durationMs: 5, cancelled: false },
    ]);
    expect(done.status).toBe("done");
    expect(done.address).toBe("10.0.0.1");
    expect(done.summary?.open).toEqual([80]);
  });

  it("keeps ping totals while bounding the replies", () => {
    let s: PingState = { ...PING_DEFAULTS, runId: "p", status: "running" };
    s = applyPingEvents(s, "p", [{ type: "pingStarted", mode: "icmp", address: "1.1.1.1", port: null, note: null }]);
    const replies: ToolEvent[] = [];
    for (let seq = 1; seq <= MAX_REPLIES + 10; seq++) {
      replies.push({ type: "pingReply", seq, ms: seq % 5 === 0 ? null : seq, ttl: 64, error: seq % 5 === 0 ? "Request timed out" : null });
    }
    s = applyPingEvents(s, "p", replies);
    expect(s.started?.mode).toBe("icmp");
    expect(s.replies).toHaveLength(MAX_REPLIES);
    expect(s.replies[0].seq).toBe(11);
    expect(s.stats.sent).toBe(MAX_REPLIES + 10);
    expect(s.stats.received).toBe(MAX_REPLIES + 10 - 102);
    expect(s.stats.min).toBe(1);
    expect(s.stats.max).toBe(509);
    expect(applyPingEvents(s, "other", replies)).toBe(s);
  });
});

describe("runs", () => {
  it("routes streamed port results to the tab", async () => {
    const id = toolTab("ports");
    updateToolState(id, (s) => ({ ...s, host: "127.0.0.1", preset: "custom", custom: "80,81" }));
    mocked.portCheck.mockResolvedValueOnce({ address: "127.0.0.1", total: 2 });
    await tools.startPortCheck(id);
    const [runId, host, ports, timeoutMs] = mocked.portCheck.mock.calls[0] as [string, string, string, number];
    expect([host, ports, timeoutMs]).toEqual(["127.0.0.1", "80,81", 2000]);
    expect(tabState(id, PORT_DEFAULTS)).toMatchObject({ status: "running", address: "127.0.0.1", total: 2 });

    emit(runId, { type: "portResult", port: 80, open: true, ms: 1, error: null, message: null });
    emit("someone-else", { type: "portResult", port: 99, open: true, ms: 1, error: null, message: null });
    emit(runId, { type: "portsDone", address: "127.0.0.1", open: [80], refused: 0, timedOut: 0, errors: 0, checked: 1, total: 2, durationMs: 3, cancelled: false });
    await flush();
    const s = tabState(id, PORT_DEFAULTS);
    expect(s.results.map((r) => r.port)).toEqual([80]);
    expect(s.status).toBe("done");
  });

  it("asks before scanning many ports on a public host", async () => {
    const { useDialogs } = await import("./dialogs");
    const id = toolTab("ports");
    updateToolState(id, (s) => ({ ...s, host: "example.com", preset: "custom", custom: "1-200" }));
    const started = tools.startPortCheck(id);
    await Promise.resolve();
    const dialog = useDialogs.getState().current;
    expect(dialog?.kind).toBe("confirm");
    if (dialog?.kind === "confirm") expect(dialog.message).toMatch(/allowed to test/);
    if (dialog?.kind === "confirm") dialog.resolve(false);
    await started;
    expect(mocked.portCheck).not.toHaveBeenCalled();
  });

  it("reports input errors without calling the backend", async () => {
    const id = toolTab("ports");
    updateToolState(id, (s) => ({ ...s, host: "localhost", preset: "custom", custom: "abc" }));
    await tools.startPortCheck(id);
    expect(mocked.portCheck).not.toHaveBeenCalled();
    expect(tabState(id, PORT_DEFAULTS)).toMatchObject({ status: "error", error: "'abc' is not a port or range" });
  });

  it("cancels a run when its tab closes and treats cancellation as idle", async () => {
    const id = toolTab("ping");
    updateToolState(id, (s) => ({ ...s, host: "example.com" }));
    let reject!: (e: unknown) => void;
    mocked.ping.mockReturnValueOnce(new Promise((_, rej) => (reject = rej)));
    const run = tools.startPing(id);
    const runId = mocked.ping.mock.calls[0][0] as string;
    await tools.stopRun(id);
    expect(mocked.cancelTool).toHaveBeenCalledWith(runId);
    reject(new RpcError({ code: "network", message: "Request cancelled", networkKind: "cancelled" }));
    await run;
    expect(tabState(id, PING_DEFAULTS)).toMatchObject({ status: "idle", error: null });

    mocked.cancelTool.mockClear();
    mocked.ping.mockResolvedValueOnce({ mode: "tcp", address: "1.2.3.4", port: 443, note: "ICMP is not permitted" });
    await tools.startPing(id);
    const second = mocked.ping.mock.calls[1][0] as string;
    expect(tabState(id, PING_DEFAULTS).started?.note).toMatch(/ICMP/);
    useTabs.setState({ tabs: [], activeId: null });
    expect(mocked.cancelTool).toHaveBeenCalledWith(second);
  });

  it("stores TLS reports and errors", async () => {
    const id = toolTab("tls");
    await tools.inspectTls(id);
    expect(tabState(id, tools.TLS_DEFAULTS).error).toMatch(/Enter/);
    updateToolState(id, (s) => ({ ...s, host: "example.com:8443", sni: " " }));
    mocked.tlsInspect.mockRejectedValueOnce(new RpcError({ code: "network", message: "TLS handshake failed", networkKind: "tls" }));
    await tools.inspectTls(id);
    expect(mocked.tlsInspect.mock.calls[0].slice(0, 2)).toEqual(["example.com:8443", null]);
    expect(tabState(id, tools.TLS_DEFAULTS)).toMatchObject({ status: "error", error: "TLS handshake failed" });
  });
});

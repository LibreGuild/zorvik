import { beforeEach, describe, expect, it, vi } from "vitest";
import type { RunningServerInfo } from "../bindings/RunningServerInfo";
import type { Server } from "../bindings/Server";
import type { ServerNode } from "../bindings/ServerNode";
import type { StreamEvent } from "../bindings/StreamEvent";
import type { TrafficEntry } from "../bindings/TrafficEntry";
import type { WorkspaceInfo } from "../bindings/WorkspaceInfo";

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
  return {
    ...actual,
    api: {
      startServer: fn(),
      stopServer: fn(),
      stopAllServers: fn(),
      serverLog: fn(),
      runningServers: fn(),
      servers: fn(),
      readServer: fn(),
      saveServer: fn(),
      createServer: fn(),
      deleteServer: fn(),
    },
  };
});

const { api, RpcError } = await import("../lib/rpc");
const { useWorkspace } = await import("./workspace");
const { useDialogs } = await import("./dialogs");
const { deleteServer, refreshRunning, renameServer, serverKey, startServer, stopAllServers, stopServer, useServers } = await import("./servers");
const { useTabs, resetTabs, isDirty, saveTab, openServer, onServerDeleted, updateServerDraft } = await import("./tabs");
type ServerTab = import("./tabs").ServerTab;
const mocked = api as unknown as Record<string, ReturnType<typeof vi.fn>>;

const WS = "/ws";
const server: Server = { name: "Echo", kind: "tcp", seq: 0, host: "127.0.0.1", port: 9000 };
const stats = { connectionsOpen: 0, connectionsTotal: 0, requests: 0, bytesIn: 0, bytesOut: 0, errors: 0 };
const info = (runId: string, serverId: string, workspacePath = WS): RunningServerInfo => ({
  runId,
  workspacePath,
  workspaceName: "ws",
  serverId,
  name: serverId,
  kind: "tcp",
  url: "tcp://127.0.0.1:9000",
  host: "127.0.0.1",
  port: 9000,
  tls: false,
  startedAt: 0,
  stats,
});
const node = (id: string): ServerNode => ({ id, name: id, kind: "tcp", host: "127.0.0.1", port: 9000, tls: false, seq: 0, autoStart: false });
const entry = (id: number): TrafficEntry => ({
  id,
  timestamp: 0,
  kind: "data",
  conn: 1,
  peer: null,
  direction: "in",
  summary: "",
  text: `m${id}`,
  base64: null,
  size: 2,
  truncated: false,
  http: null,
});
const emit = (e: StreamEvent) => handlers.forEach((h) => h(e));
const traffic = (runId: string, ...ids: number[]) => ids.forEach((id) => emit({ type: "server", runId, event: { type: "traffic", entry: entry(id) } }));
const flush = () => new Promise((r) => setTimeout(r, 70));
const logIds = (serverId: string) => useServers.getState().logs[serverKey(WS, serverId)]?.map((e) => e.id);

function deferred<T>() {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((res) => (resolve = res));
  return { promise, resolve };
}

/** Start `serverId` as run `runId` with an empty backlog. */
async function started(serverId: string, runId: string) {
  mocked.startServer.mockResolvedValueOnce(info(runId, serverId));
  mocked.serverLog.mockResolvedValueOnce([]);
  expect(await startServer(serverId, server)).toBe(true);
}

function answerDialog(value: boolean | string) {
  const dialog = useDialogs.getState().current;
  if (dialog?.kind === "confirm") dialog.resolve(value as boolean);
  else if (dialog?.kind === "prompt") dialog.resolve(value as string);
  else throw new Error("no dialog");
}

beforeEach(() => {
  useWorkspace.setState({ info: { path: WS } as WorkspaceInfo });
  useServers.setState({ saved: [], running: [], logs: {}, busy: {} });
  useDialogs.setState({ current: null });
  vi.clearAllMocks();
  mocked.servers.mockResolvedValue([]);
  mocked.runningServers.mockImplementation(() => Promise.resolve(useServers.getState().running));
});

describe("traffic log", () => {
  it("keeps each entry once when it also arrived while the backlog loaded", async () => {
    mocked.startServer.mockResolvedValueOnce(info("r1", "a"));
    const backlog = deferred<TrafficEntry[]>();
    mocked.serverLog.mockReturnValueOnce(backlog.promise);
    const starting = startServer("a", server);
    await vi.waitFor(() => expect(mocked.serverLog).toHaveBeenCalled());
    traffic("r1", 1, 2, 3); // 1 and 2 are in the backlog too
    backlog.resolve([entry(1), entry(2)]);
    await starting;
    await flush();
    expect(logIds("a")).toEqual([1, 2, 3]);
  });

  it("keeps the last run's traffic when a start fails", async () => {
    await started("b", "r2");
    traffic("r2", 1, 2);
    await flush();
    await stopServer("r2");
    mocked.startServer.mockRejectedValueOnce(new RpcError({ code: "invalid", message: "Port 9000 is in use", networkKind: null }));
    expect(await startServer("b", server)).toBe(false);
    expect(logIds("b")).toEqual([1, 2]);
  });

  it("starts over for a new run found running (ids start at 1 again)", async () => {
    await started("c", "r3");
    traffic("r3", 1, 2);
    await flush();
    emit({ type: "server", runId: "r3", event: { type: "stopped", error: null } });
    // Started again elsewhere (e.g. with the workspace).
    mocked.runningServers.mockResolvedValueOnce([info("r4", "c")]);
    mocked.serverLog.mockResolvedValueOnce([entry(1)]);
    await refreshRunning();
    traffic("r4", 2);
    await flush();
    expect(logIds("c")).toEqual([1, 2]);
    expect(useServers.getState().logs[serverKey(WS, "c")]?.[0].text).toBe("m1");
  });

  it("follows a rename, also for traffic arriving before the running list is refreshed", async () => {
    await started("d", "r5");
    traffic("r5", 1);
    await flush();
    mocked.readServer.mockResolvedValueOnce({ ...server, name: "d" });
    mocked.saveServer.mockResolvedValueOnce("d2");
    const list = deferred<RunningServerInfo[]>();
    mocked.runningServers.mockReturnValueOnce(list.promise);
    const renaming = renameServer(node("d"));
    answerDialog("d2");
    await vi.waitFor(() => expect(mocked.runningServers).toHaveBeenCalled());
    traffic("r5", 2);
    list.resolve([info("r5", "d2")]);
    await renaming;
    await flush();
    expect(logIds("d2")).toEqual([1, 2]);
    expect(logIds("d")).toBeUndefined();
  });

  it("drops a deleted server's traffic", async () => {
    await started("e", "r6");
    traffic("r6", 1);
    await flush();
    const deleting = deleteServer(node("e"));
    answerDialog(true);
    await deleting;
    traffic("r6", 2); // late event of the stopped copy
    await flush();
    expect(logIds("e")).toBeUndefined();
    expect(useServers.getState().running).toEqual([]);
  });
});

describe("start / stop", () => {
  it("stops once on a double click", async () => {
    await started("f", "r7");
    const stopping = deferred<null>();
    mocked.stopServer.mockReturnValueOnce(stopping.promise);
    const first = stopServer("r7");
    const second = stopServer("r7");
    stopping.resolve(null);
    await Promise.all([first, second]);
    expect(mocked.stopServer).toHaveBeenCalledTimes(1);
    expect(useServers.getState().running).toEqual([]);
  });

  it("asks before stopping several servers", async () => {
    useServers.setState({ running: [info("r8", "g"), info("r9", "h", "/other")] });
    const stopping = stopAllServers();
    const dialog = useDialogs.getState().current;
    expect(dialog?.kind === "confirm" && dialog.message).toMatch(/another workspace/);
    answerDialog(false);
    await stopping;
    expect(mocked.stopAllServers).not.toHaveBeenCalled();

    useServers.setState({ running: [info("r8", "g")] });
    await stopAllServers();
    expect(mocked.stopAllServers).toHaveBeenCalledTimes(1);
    expect(useServers.getState().running).toEqual([]);
  });
});

describe("server tabs", () => {
  beforeEach(() => resetTabs());

  it("saving a tab whose file was deleted writes it again", async () => {
    mocked.readServer.mockResolvedValueOnce(server);
    await openServer("echo");
    const id = useTabs.getState().activeId!;
    onServerDeleted("echo");
    mocked.saveServer.mockRejectedValueOnce(new RpcError({ code: "notFound", message: "Server 'echo' not found", networkKind: null }));
    mocked.createServer.mockResolvedValueOnce("Echo");
    expect(await saveTab(id)).toBe(true);
    expect(mocked.createServer).toHaveBeenCalledWith(server);
    expect(useTabs.getState().tabs[0]).toMatchObject({ serverId: "Echo", orphaned: false });
  });

  it("keeps the sidebar order in open tabs, so saving does not undo a reorder", async () => {
    mocked.readServer.mockResolvedValueOnce(server);
    await openServer("echo");
    const id = useTabs.getState().activeId!;
    updateServerDraft(id, (s) => ({ ...s, port: 9100 }));
    // Reordered in the sidebar: the file changes only its seq.
    mocked.readServer.mockResolvedValueOnce({ ...server, seq: 4 });
    emit({ type: "workspaceChanged", paths: ["servers/echo.yaml"] });
    await vi.waitFor(() => expect((useTabs.getState().tabs[0] as ServerTab).draft.seq).toBe(4));
    const tab = useTabs.getState().tabs[0] as ServerTab;
    expect(tab.draft.port).toBe(9100);
    expect(isDirty(tab)).toBe(true);
  });
});

import { beforeEach, describe, expect, it, vi } from "vitest";
import type { RunResult } from "../bindings/RunResult";
import type { RunSummary } from "../bindings/RunSummary";
import type { StreamEvent } from "../bindings/StreamEvent";
import type { TreeNode } from "../bindings/TreeNode";
import type { WorkspaceInfo } from "../bindings/WorkspaceInfo";

const handlers: ((e: StreamEvent) => void)[] = [];
vi.mock("../lib/events", () => ({
  onEvent: (h: (e: StreamEvent) => void) => {
    handlers.push(h);
    return () => {};
  },
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("../lib/platform", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/platform")>()),
  pickFile: vi.fn(),
  pickSavePath: vi.fn(),
}));
vi.mock("../lib/rpc", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/rpc")>();
  const fn = () => vi.fn((..._args: unknown[]): Promise<unknown> => Promise.resolve(null));
  return { ...actual, api: { startRun: fn(), stopRun: fn(), previewRunData: fn(), exportRun: fn() } };
});

const { api, RpcError } = await import("../lib/rpc");
const platform = await import("../lib/platform");
const { useWorkspace } = await import("./workspace");
const { useToasts } = await import("./toasts");
const { useTabs, resetTabs, openRunner, closeTab } = await import("./tabs");
type RunnerTab = import("./tabs").RunnerTab;
const {
  DEFAULT_SETTINGS,
  exportRun,
  finishedMessage,
  flushRunnerEvents,
  groupResults,
  liveCounts,
  moveInOrder,
  NO_COUNTS,
  orderRequests,
  pickDataFile,
  progress,
  runnableRequests,
  sanitizeSettings,
  selectedPaths,
  setDataFile,
  startProblem,
  startRun,
  stopRun,
  toggleRun,
  updateSettings,
  useRunner,
  withResults,
} = await import("./runner");
const mocked = api as unknown as Record<string, ReturnType<typeof vi.fn>>;

const WS = "/ws";
const req = (path: string, name: string, extra: Partial<TreeNode> = {}): TreeNode => ({ kind: "request", path, name, seq: 0, method: "GET", children: [], ...extra });
const folder = (path: string, name: string, children: TreeNode[]): TreeNode => ({ kind: "folder", path, name, seq: 0, children });
const tree: TreeNode[] = [
  folder("users", "Users", [
    req("users/list.yaml", "List"),
    req("users/socket.yaml", "Socket", { requestKind: "websocket" }),
    req("users/events.yaml", "Events", { requestKind: "sse" }),
    folder("users/admin", "Admin", [req("users/admin/ban.yaml", "Ban", { method: "POST" })]),
    req("users/broken.yaml", "Broken", { error: "bad yaml" }),
  ]),
  req("health.yaml", "Health"),
];
const result = (iteration: number, name: string, extra: Partial<RunResult> = {}): RunResult => ({
  iteration,
  path: `${name}.yaml`,
  name,
  kind: "http",
  method: "GET",
  url: "http://x.test",
  status: 200,
  durationMs: 5,
  size: 10,
  tests: [],
  console: [],
  error: null,
  scriptErrors: [],
  unresolved: [],
  passed: true,
  skipped: false,
  ...extra,
});
const test = (name: string, passed: boolean, skipped = false) => ({ name, passed, skipped, error: passed ? null : "nope" });
const summary = (extra: Partial<RunSummary> = {}): RunSummary => ({
  name: "Users",
  environment: null,
  startedAt: Date.UTC(2026, 8, 27, 10, 30),
  durationMs: 1200,
  iterations: 1,
  requests: 2,
  failed: 0,
  skipped: 0,
  testsPassed: 3,
  testsFailed: 0,
  testsSkipped: 0,
  stopped: false,
  bailed: false,
  error: null,
  passed: true,
  perIteration: [],
  omitted: 0,
  ...extra,
});
const emit = (e: StreamEvent) => handlers.forEach((h) => h(e));
const runnerTab = (id: string): RunnerTab => useTabs.getState().tabs.find((t) => t.id === id) as RunnerTab;

beforeEach(() => {
  resetTabs();
  localStorage.clear();
  useWorkspace.setState({ info: { path: WS, tree, environments: [], activeEnvironment: null, meta: { name: "Shop" } } as unknown as WorkspaceInfo });
  useRunner.setState({ settings: {}, data: {}, runs: {}, filter: {}, active: null, starting: null });
  useToasts.setState({ toasts: [] });
  vi.clearAllMocks();
});

describe("requests of a run", () => {
  it("lists a folder's HTTP and event-stream requests in sidebar order, with their folder trail", () => {
    const users = runnableRequests(tree, "users")!;
    expect(users.map((r) => [r.path, r.trail])).toEqual([
      ["users/list.yaml", "Users"],
      ["users/events.yaml", "Users"],
      ["users/admin/ban.yaml", "Users / Admin"],
    ]);
    expect(runnableRequests(tree, "users/admin")!.map((r) => r.name)).toEqual(["Ban"]);
    expect(runnableRequests(tree, "")!.map((r) => r.name)).toEqual(["List", "Events", "Ban", "Health"]);
    expect(runnableRequests(tree, "gone")).toBeNull();
  });

  it("orders, selects and moves requests", () => {
    const entries = runnableRequests(tree, "")!;
    const order = orderRequests(entries, ["health.yaml", "missing.yaml", "users/list.yaml"]).map((e) => e.name);
    expect(order).toEqual(["Health", "List", "Events", "Ban"]);
    const settings = { ...DEFAULT_SETTINGS, order: ["health.yaml"], excluded: ["users/list.yaml"] };
    expect(selectedPaths(entries, settings)).toEqual(["health.yaml", "users/events.yaml", "users/admin/ban.yaml"]);
    expect(moveInOrder(["a", "b", "c"], 0, 2)).toEqual(["b", "c", "a"]);
    expect(moveInOrder(["a", "b", "c"], 2, 0)).toEqual(["c", "a", "b"]);
    expect(moveInOrder(["a", "b", "c"], 0, -1)).toEqual(["a", "b", "c"]);
    expect(moveInOrder(["a", "b", "c"], 2, 5)).toEqual(["a", "b", "c"]);
  });

  it("explains why a run can't start", () => {
    const entries = runnableRequests(tree, "users")!;
    expect(startProblem(null, DEFAULT_SETTINGS, undefined)).toBe("This folder no longer exists");
    expect(startProblem([], DEFAULT_SETTINGS, undefined)).toBe("No HTTP requests to run");
    expect(startProblem(entries, { ...DEFAULT_SETTINGS, excluded: entries.map((e) => e.path) }, undefined)).toBe("Select a request to run");
    const withData = { ...DEFAULT_SETTINGS, dataFile: "d.csv" };
    expect(startProblem(entries, withData, { status: "error", file: "d.csv", message: "x" })).toBe("The data file can't be used");
    expect(startProblem(entries, withData, { status: "loading", file: "d.csv" })).toBe("Reading the data file…");
    expect(startProblem(entries, DEFAULT_SETTINGS, undefined)).toBeNull();
  });
});

describe("results", () => {
  const results = [
    result(0, "A", { tests: [test("t1", true), test("t2", false), test("t3", false, true)], passed: false }),
    result(0, "B", { skipped: true, status: null }),
    result(1, "A", { tests: [test("t1", true)] }),
    result(1, "B", { skipped: true, status: null }),
  ];

  it("groups by iteration with counts, and filters failed ones", () => {
    const groups = groupResults(results);
    expect(groups.map((g) => [g.iteration, g.results.length, g.failed, g.testsPassed, g.testsFailed])).toEqual([
      [0, 2, 1, 1, 1],
      [1, 2, 0, 1, 0],
    ]);
    const failed = groupResults(results, "failed");
    expect(failed.map((g) => [g.iteration, g.results.map((r) => r.name)])).toEqual([[0, ["A"]]]);
  });

  const empty = { runId: "r", name: "Users", environment: null, total: 8, iterations: 2, startedAt: 0, results: [], received: 0, counts: NO_COUNTS, summary: null, stopping: false };

  it("counts a live run and its progress", () => {
    const run = withResults(empty, results);
    expect(run.results).toEqual(results);
    expect(liveCounts(run)).toEqual({ requests: 2, failed: 1, skipped: 2, testsPassed: 2, testsFailed: 1 });
    expect(progress(run)).toBe(0.5);
    expect(progress(withResults(run, [...results, ...results]))).toBe(1);
    expect(progress({ ...run, summary: summary() })).toBe(1);
    expect(liveCounts({ ...run, summary: summary({ requests: 9 }) }).requests).toBe(9);
  });

  it("keeps the first results, then failed ones, and counts them all", () => {
    const failed = { passed: false, status: 500 };
    const many = ["A", "B", "C", "D", "E", "F", "G"].map((n) => result(0, n, "DFG".includes(n) ? failed : {}));
    const run = withResults(withResults(empty, many.slice(0, 3), 2), many.slice(3), 2);
    // Two (the limit), then failed ones up to twice the limit.
    expect(run.results.map((r) => r.name)).toEqual(["A", "B", "D", "F"]);
    expect(run.received).toBe(7);
    expect(liveCounts(run)).toMatchObject({ requests: 7, failed: 3 });
  });

  it("describes a finished run", () => {
    expect(finishedMessage("Users", summary())).toEqual({ tone: "success", title: "“Users” passed", detail: "2 requests · 3/3 tests passed" });
    const failed = finishedMessage("Users", summary({ passed: false, failed: 1, testsFailed: 1, iterations: 3, perIteration: [{} as never, {} as never] }));
    expect(failed).toEqual({ tone: "error", title: "“Users” failed", detail: "2 requests, 1 failed · 3/4 tests passed · 2 of 3 iterations" });
    expect(finishedMessage("Users", summary({ stopped: true })).tone).toBe("info");
    expect(finishedMessage("Users", summary({ error: "loop", passed: false }))).toMatchObject({ tone: "error", detail: "loop" });
  });
});

describe("runs", () => {
  it("starts with the tab's settings, applies results per frame and finishes with a toast", async () => {
    const id = openRunner("users", "Users");
    updateSettings(id, (s) => ({ ...s, order: ["users/admin/ban.yaml"], iterations: 2, delayMs: 50, stopOnFailure: true }));
    let resolveStart: (v: unknown) => void = () => {};
    mocked.startRun.mockReturnValueOnce(new Promise((r) => (resolveStart = r)));
    const starting = startRun(runnerTab(id), runnableRequests(tree, "users")!);
    expect(mocked.startRun).toHaveBeenCalledWith({
      folder: "users",
      requests: ["users/admin/ban.yaml", "users/list.yaml", "users/events.yaml"],
      iterations: 2,
      delayMs: 50,
      dataFile: undefined,
      stopOnFailure: true,
    });
    // Events can arrive before the start call returns: they are kept for it.
    emit({ type: "runner", runId: "r1", event: { type: "started", name: "Users", total: 4, iterations: 2 } });
    emit({ type: "runner", runId: "r1", event: { type: "result", result: result(0, "Ban") } });
    resolveStart({ runId: "r1", name: "Users", total: 4, iterations: 2, environment: "Local" });
    expect(await starting).toBe(true);
    expect(useRunner.getState().active).toBe(id);

    emit({ type: "runner", runId: "r1", event: { type: "result", result: result(0, "List", { passed: false, status: 500 }) } });
    emit({ type: "runner", runId: "other", event: { type: "result", result: result(0, "Nope") } });
    flushRunnerEvents();
    const run = useRunner.getState().runs[id];
    expect(run.results.map((r) => r.name)).toEqual(["Ban", "List"]);
    expect(run.environment).toBe("Local");

    // A second start is refused while one runs.
    expect(await startRun(runnerTab(id), runnableRequests(tree, "users")!)).toBe(false);
    expect(mocked.startRun).toHaveBeenCalledTimes(1);

    emit({ type: "runner", runId: "r1", event: { type: "result", result: result(1, "Ban") } });
    emit({ type: "runner", runId: "r1", event: { type: "finished", summary: summary({ passed: false, failed: 1 }) } });
    const s = useRunner.getState();
    expect(s.active).toBeNull();
    expect(s.runs[id].results).toHaveLength(3);
    expect(s.runs[id].summary?.failed).toBe(1);
    expect(useToasts.getState().toasts.at(-1)).toMatchObject({ tone: "error", title: "“Users” failed" });
  });

  it("stops, toggles with Mod+Enter and toasts start errors", async () => {
    const id = openRunner("", "Shop");
    mocked.startRun.mockResolvedValueOnce({ runId: "r2", name: "Shop", total: 3, iterations: 1, environment: null });
    expect(await toggleRun(runnerTab(id))).toBe(true);
    expect(mocked.startRun.mock.calls[0][0]).toMatchObject({ folder: "", requests: ["users/list.yaml", "users/events.yaml", "users/admin/ban.yaml", "health.yaml"] });
    await toggleRun(runnerTab(id));
    expect(mocked.stopRun).toHaveBeenCalledWith("r2");
    expect(useRunner.getState().runs[id].stopping).toBe(true);
    await stopRun(id);
    expect(mocked.stopRun).toHaveBeenCalledTimes(1);
    emit({ type: "runner", runId: "r2", event: { type: "finished", summary: summary({ stopped: true }) } });
    expect(useRunner.getState().runs[id].stopping).toBe(false);

    mocked.startRun.mockRejectedValueOnce(new RpcError({ code: "invalidInput", message: "Iterations must be between 1 and 100000", networkKind: null }));
    expect(await toggleRun(runnerTab(id))).toBe(false);
    expect(useToasts.getState().toasts.at(-1)).toMatchObject({ tone: "error", detail: "Iterations must be between 1 and 100000" });

    // Nothing selected: not even asked.
    updateSettings(id, (s) => ({ ...s, excluded: ["users/list.yaml", "users/events.yaml", "users/admin/ban.yaml", "health.yaml"] }));
    expect(await toggleRun(runnerTab(id))).toBe(false);
    expect(mocked.startRun).toHaveBeenCalledTimes(2);
  });

  it("previews a data file, relative to the workspace when picked inside it", async () => {
    const id = openRunner("users", "Users");
    const preview = { format: "csv", columns: ["user"], rows: [["ada"]], count: 3 };
    vi.mocked(platform.pickFile).mockResolvedValueOnce("/ws/data/users.csv");
    mocked.previewRunData.mockResolvedValueOnce(preview);
    await pickDataFile(id);
    expect(mocked.previewRunData).toHaveBeenCalledWith("data/users.csv");
    expect(useRunner.getState().settings[id].dataFile).toBe("data/users.csv");
    expect(useRunner.getState().data[id]).toEqual({ status: "ready", file: "data/users.csv", preview });

    // A slow answer for a file no longer chosen is dropped.
    let late: (v: unknown) => void = () => {};
    mocked.previewRunData.mockReturnValueOnce(new Promise((r) => (late = r)));
    const first = setDataFile(id, "old.csv");
    mocked.previewRunData.mockRejectedValueOnce(new RpcError({ code: "invalidInput", message: "The data file has no rows", networkKind: null }));
    await setDataFile(id, "/elsewhere/new.csv");
    late(preview);
    await first;
    expect(useRunner.getState().data[id]).toEqual({ status: "error", file: "/elsewhere/new.csv", message: "The data file has no rows" });

    await setDataFile(id, null);
    expect(useRunner.getState().data[id]).toBeUndefined();
    expect(useRunner.getState().settings[id].dataFile).toBeNull();
  });

  it("exports a finished run", async () => {
    const id = openRunner("users", "Users");
    useRunner.setState({
      runs: {
        [id]: { runId: "r3", name: "Users/ok", environment: null, total: 1, iterations: 1, startedAt: 0, results: [], received: 0, counts: NO_COUNTS, summary: summary(), stopping: false },
      },
    });
    vi.mocked(platform.pickSavePath).mockResolvedValueOnce("/tmp/out.xml");
    await exportRun(id, "junit");
    expect(vi.mocked(platform.pickSavePath).mock.calls[0][1]).toMatch(/^Users-ok \d{4}-\d{2}-\d{2} \d{2}-\d{2}\.xml$/);
    expect(mocked.exportRun).toHaveBeenCalledWith("r3", "/tmp/out.xml", "junit");
    expect(useToasts.getState().toasts.at(-1)).toMatchObject({ tone: "success", title: "JUnit report saved" });
  });

  it("closing the tab drops its state and stops its run; switching workspace resets", async () => {
    const id = openRunner("users", "Users");
    mocked.startRun.mockResolvedValueOnce({ runId: "r4", name: "Users", total: 2, iterations: 1, environment: null });
    await startRun(runnerTab(id), runnableRequests(tree, "users")!);
    await closeTab(id);
    expect(mocked.stopRun).toHaveBeenCalledWith("r4");
    expect(useRunner.getState()).toMatchObject({ runs: {}, settings: {}, active: null });

    const other = openRunner("", "Shop");
    updateSettings(other, (s) => ({ ...s, delayMs: 10 }));
    useWorkspace.setState({ info: { path: "/other", tree: [], environments: [], meta: { name: "Other" } } as unknown as WorkspaceInfo });
    expect(useRunner.getState().settings).toEqual({});
  });

  it("stops a run whose tab closed while it started, and runs no tab shows", async () => {
    const id = openRunner("users", "Users");
    let resolveStart: (v: unknown) => void = () => {};
    mocked.startRun.mockReturnValueOnce(new Promise((r) => (resolveStart = r)));
    const starting = startRun(runnerTab(id), runnableRequests(tree, "users")!);
    await closeTab(id);
    resolveStart({ runId: "r5", name: "Users", total: 2, iterations: 1, environment: null });
    expect(await starting).toBe(false);
    expect(mocked.stopRun).toHaveBeenCalledWith("r5");
    expect(useRunner.getState()).toMatchObject({ runs: {}, active: null, starting: null });

    // E.g. after the window reloaded: it would block every other run until it ends.
    emit({ type: "runner", runId: "lost", event: { type: "result", result: result(0, "A") } });
    expect(mocked.stopRun).toHaveBeenCalledWith("lost");
  });

  it("keeps a tab's settings when its tabs are restored, and follows renamed requests", async () => {
    const id = openRunner("users", "Users");
    updateSettings(id, (s) => ({ ...s, order: ["users/admin/ban.yaml", "users/list.yaml"], excluded: ["users/admin/ban.yaml"], delayMs: 25 }));
    const { onItemMoved } = await import("./tabs");
    onItemMoved("users/admin", "users/admins");
    expect(useRunner.getState().settings[id]).toMatchObject({ order: ["users/admins/ban.yaml", "users/list.yaml"], excluded: ["users/admins/ban.yaml"] });

    // Tabs dropped and restored (reload, or another workspace and back): the left-out request stays out.
    const tab = runnerTab(id);
    useTabs.setState({ tabs: [], activeId: null });
    expect(useRunner.getState().settings[id]).toBeUndefined();
    mocked.previewRunData.mockResolvedValue({ format: "csv", columns: ["a"], rows: [], count: 1 });
    useTabs.setState({ tabs: [tab], activeId: id });
    expect(useRunner.getState().settings[id]).toEqual({ ...DEFAULT_SETTINGS, order: ["users/admins/ban.yaml", "users/list.yaml"], excluded: ["users/admins/ban.yaml"], delayMs: 25 });

    // A data file's preview loads again.
    updateSettings(id, (s) => ({ ...s, dataFile: "d.csv" }));
    useTabs.setState({ tabs: [], activeId: null });
    useTabs.setState({ tabs: [tab], activeId: id });
    expect(mocked.previewRunData).toHaveBeenCalledWith("d.csv");

    expect(sanitizeSettings({ order: ["a", 1], excluded: "x", iterations: 0, delayMs: -1, dataFile: 5, stopOnFailure: "yes" })).toEqual({ ...DEFAULT_SETTINGS, order: ["a"] });
    expect(sanitizeSettings(null)).toBeNull();
  });

  it("opens one tab per folder and follows folder moves", async () => {
    const a = openRunner("users", "Users");
    expect(openRunner("users", "Users")).toBe(a);
    const { onItemMoved } = await import("./tabs");
    onItemMoved("users", "people");
    expect(runnerTab(a).folder).toBe("people");
    onItemMoved("people/admin", "admins");
    expect(runnerTab(a).folder).toBe("people");
  });
});

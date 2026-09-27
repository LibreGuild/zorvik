import { beforeEach, describe, expect, it, vi } from "vitest";
import type { LoadTest } from "../bindings/LoadTest";
import type { MetricsSummary } from "../bindings/MetricsSummary";
import type { Snapshot } from "../bindings/Snapshot";
import type { StreamEvent } from "../bindings/StreamEvent";
import type { Summary } from "../bindings/Summary";
import type { TimePoint } from "../bindings/TimePoint";
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
      loadTests: fn(),
      readLoadTest: fn(),
      createLoadTest: fn(),
      saveLoadTest: fn(),
      deleteLoadTest: fn(),
      startLoadTest: fn(),
      stopLoadTest: fn(),
      activeLoadTest: fn(),
    },
  };
});

const { api, RpcError } = await import("../lib/rpc");
const { useWorkspace } = await import("./workspace");
const { useDialogs } = await import("./dialogs");
const { useToasts } = await import("./toasts");
const { useTabs, resetTabs, isDirty, sameLoadTest, saveTab, openLoadTest, onLoadTestDeleted, updateLoadTestDraft } = await import("./tabs");
type LoadTestTab = import("./tabs").LoadTestTab;
const {
  appendPoints,
  applySnapshot,
  compactPoints,
  deleteLoadTest,
  finishedMessage,
  flushLoadEvents,
  forgetResult,
  loadTestFolder,
  loadTestRequest,
  MAX_POINTS,
  renameLoadTest,
  restoreActiveRun,
  runFor,
  setCompareRun,
  startLoadRun,
  stopLoadRun,
  useLoadTests,
} = await import("./loadtests");
const mocked = api as unknown as Record<string, ReturnType<typeof vi.fn>>;

const WS = "/ws";
const test: LoadTest = { name: "Smoke", seq: 0, targets: [{ request: "a.yaml" }], model: "virtualUsers", stages: [{ durationSecs: 3, target: 5 }] };
const latency = { min: 1, avg: 2, p50: 2, p90: 3, p95: 4, p99: 5, p999: 6, max: 7 };
const phase = { count: 0, avg: 0, p50: 0, p95: 0, p99: 0, max: 0 };
const totals = (requests: number): MetricsSummary => ({
  requests,
  errors: 0,
  errorRate: 0,
  rps: requests / 3,
  bytesIn: 0,
  bytesOut: 0,
  latency,
  statusCodes: [[200, requests]],
  errorKinds: [],
  dropped: 0,
  connections: 5,
  timing: { connect: phase, ttfb: phase, transfer: phase, server: phase },
  captureMisses: 0,
});
const point = (second: number, rps = 10): TimePoint => ({ second, rps, errors: 0, p50: 2, p95: 4, p99: 5, active: 5, target: 5 });
const snapshot = (elapsedMs: number, points: TimePoint[], requests = 10): Snapshot => ({
  phase: "running",
  elapsedMs,
  plannedMs: 3000,
  active: 5,
  target: 5,
  totals: totals(requests),
  targets: [],
  points,
  thresholds: [],
  cpuPercent: 12,
});
const summary = (passed = true, extra: Partial<Summary> = {}): Summary => ({
  startedAt: 1,
  durationMs: 3000,
  totals: totals(30),
  targets: [],
  points: [point(0), point(1), point(2)],
  thresholds: passed ? [] : [{ label: "p95 < 1 ms", metric: "p95", op: "<", value: 1, target: null, actual: 4, passed: false }],
  passed,
  stoppedEarly: false,
  error: null,
  peakCpuPercent: 20,
  ...extra,
});
const emit = (e: StreamEvent) => handlers.forEach((h) => h(e));
const answerDialog = (value: boolean | string) => {
  const dialog = useDialogs.getState().current;
  if (dialog?.kind === "confirm") dialog.resolve(value as boolean);
  else if (dialog?.kind === "prompt") dialog.resolve(value as string);
  else throw new Error("no dialog");
};

beforeEach(() => {
  resetTabs();
  useWorkspace.setState({ info: { path: WS, tree: [] } as unknown as WorkspaceInfo });
  useLoadTests.setState({ saved: [], active: null, starting: null, last: {}, historyVersion: {}, compare: {} });
  useDialogs.setState({ current: null });
  useToasts.setState({ toasts: [] });
  vi.clearAllMocks();
  mocked.loadTests.mockResolvedValue([]);
  mocked.activeLoadTest.mockResolvedValue(null);
});

describe("chart points", () => {
  it("appends new seconds only, in order", () => {
    const pts = appendPoints([point(0), point(1)], [point(1), point(2), point(2), point(3)]);
    expect(pts.map((p) => p.second)).toEqual([0, 1, 2, 3]);
    const same = [point(0)];
    expect(appendPoints(same, [point(0)])).toBe(same);
  });

  it("stays bounded by merging neighbouring seconds (keeping the worse latency)", () => {
    const many = Array.from({ length: 9 }, (_, i) => ({ ...point(i, i * 2), p99: i }));
    const bounded = appendPoints([], many, 8);
    expect(bounded.length).toBeLessThanOrEqual(8);
    expect(bounded[0]).toMatchObject({ second: 0, rps: 1, p99: 1 });
    expect(compactPoints([point(0, 4), point(1, 8), point(2, 6)])).toEqual([{ ...point(0, 6) }, point(2, 6)]);
    const long = appendPoints([], Array.from({ length: MAX_POINTS + 10 }, (_, i) => point(i)));
    expect(long.length).toBeLessThanOrEqual(MAX_POINTS);
    expect(long[long.length - 1].second).toBeGreaterThan(MAX_POINTS - 10);
  });

  it("applies a snapshot: latest numbers, planned time, points", () => {
    const run = { runId: "r", testId: "t", workspacePath: WS, name: "n", startedAt: 0, plannedMs: 0, model: null, snapshot: null, points: [], stopping: false };
    const next = applySnapshot(run, snapshot(1200, [point(0)]));
    expect(next.plannedMs).toBe(3000);
    expect(next.points).toHaveLength(1);
    expect(next.snapshot?.totals.requests).toBe(10);
    expect(applySnapshot(next, { ...snapshot(2500, []), phase: "stopping" }).stopping).toBe(true);
  });

  it("describes a finished run", () => {
    expect(finishedMessage("Smoke", summary(true))).toMatchObject({ tone: "success", title: "“Smoke” finished" });
    const failed = finishedMessage("Smoke", summary(false));
    expect(failed.tone).toBe("error");
    expect(failed.title).toBe("“Smoke” failed a threshold");
    expect(failed.detail).toContain("p95 < 1 ms");
    expect(failed.detail).toContain("30 requests");
    expect(finishedMessage("Smoke", summary(false, { error: "boom" }))).toMatchObject({ tone: "error", detail: "boom" });
  });
});

describe("runs", () => {
  it("starts, applies coalesced snapshots, finishes with a toast and keeps the result", async () => {
    mocked.startLoadTest.mockResolvedValueOnce({ runId: "r1", plannedMs: 3000, targets: [] });
    expect(await startLoadRun("smoke", test)).toBe(true);
    expect(mocked.startLoadTest).toHaveBeenCalledWith("smoke", test, false);
    expect(runFor("smoke")?.runId).toBe("r1");

    emit({ type: "load", runId: "r1", event: { type: "snapshot", snapshot: snapshot(500, []) } });
    emit({ type: "load", runId: "r1", event: { type: "snapshot", snapshot: snapshot(1100, [point(0)], 12) } });
    emit({ type: "load", runId: "r1", event: { type: "snapshot", snapshot: snapshot(2100, [point(1)], 25) } });
    // Nothing rendered yet: one update per frame.
    expect(useLoadTests.getState().active?.snapshot).toBeNull();
    flushLoadEvents();
    const active = useLoadTests.getState().active!;
    expect(active.snapshot?.totals.requests).toBe(25);
    expect(active.points.map((p) => p.second)).toEqual([0, 1]);

    emit({ type: "load", runId: "r1", event: { type: "finished", summary: summary(true) } });
    const s = useLoadTests.getState();
    expect(s.active).toBeNull();
    expect(s.last.smoke?.runId).toBe("r1");
    expect(s.historyVersion.smoke).toBe(1);
    expect(useToasts.getState().toasts.at(-1)).toMatchObject({ tone: "success", title: "“Smoke” finished" });
  });

  it("asks before sending load to another host, then starts confirmed", async () => {
    mocked.startLoadTest
      .mockRejectedValueOnce(new RpcError({ code: "confirmTarget", message: "This sends load to example.com.", networkKind: null }))
      .mockResolvedValueOnce({ runId: "r2", plannedMs: 3000, targets: [] });
    const starting = startLoadRun("smoke", test);
    await vi.waitFor(() => expect(useDialogs.getState().current).not.toBeNull());
    const dialog = useDialogs.getState().current;
    expect(dialog?.kind === "confirm" && dialog.message).toBe("This sends load to example.com.");
    expect(dialog?.kind === "confirm" && dialog.danger).toBe(true);
    answerDialog(true);
    expect(await starting).toBe(true);
    expect(mocked.startLoadTest).toHaveBeenLastCalledWith("smoke", test, true);
  });

  it("does not start when the confirmation is declined, and toasts other errors", async () => {
    mocked.startLoadTest.mockRejectedValueOnce(new RpcError({ code: "confirmTarget", message: "outside", networkKind: null }));
    const starting = startLoadRun("smoke", test);
    await vi.waitFor(() => expect(useDialogs.getState().current).not.toBeNull());
    answerDialog(false);
    expect(await starting).toBe(false);
    expect(mocked.startLoadTest).toHaveBeenCalledTimes(1);
    expect(useLoadTests.getState().starting).toBeNull();

    mocked.startLoadTest.mockRejectedValueOnce(new RpcError({ code: "invalid", message: "The load generator is not available yet", networkKind: null }));
    expect(await startLoadRun("smoke", test)).toBe(false);
    expect(useToasts.getState().toasts.at(-1)).toMatchObject({ tone: "error", detail: "The load generator is not available yet" });
    expect(useLoadTests.getState().active).toBeNull();
  });

  it("runs one test at a time", async () => {
    mocked.startLoadTest.mockResolvedValueOnce({ runId: "r3", plannedMs: 3000, targets: [] });
    await startLoadRun("smoke", test);
    expect(await startLoadRun("other", { ...test, name: "Other" })).toBe(false);
    expect(mocked.startLoadTest).toHaveBeenCalledTimes(1);
    expect(runFor("other")).toBeNull();
  });

  it("keeps events that arrive before the start call returns", async () => {
    let resolve!: (v: unknown) => void;
    mocked.startLoadTest.mockReturnValueOnce(new Promise((r) => (resolve = r)));
    const starting = startLoadRun("smoke", test);
    emit({ type: "load", runId: "r4", event: { type: "snapshot", snapshot: snapshot(300, [point(0)], 3) } });
    flushLoadEvents();
    expect(mocked.activeLoadTest).not.toHaveBeenCalled(); // a start is in flight: it claims them
    resolve({ runId: "r4", plannedMs: 3000, targets: [] });
    await starting;
    expect(useLoadTests.getState().active?.snapshot?.totals.requests).toBe(3);
    expect(useLoadTests.getState().active?.points).toHaveLength(1);
  });

  it("picks up a run found running (window reloaded)", async () => {
    mocked.activeLoadTest.mockResolvedValueOnce({ runId: "r5", testId: "smoke", workspacePath: WS, name: "Smoke", startedAt: 5, plannedMs: 3000, snapshot: null, points: [] });
    emit({ type: "load", runId: "r5", event: { type: "snapshot", snapshot: snapshot(800, [point(0)], 7) } });
    flushLoadEvents();
    await vi.waitFor(() => expect(useLoadTests.getState().active?.runId).toBe("r5"));
    expect(useLoadTests.getState().active?.snapshot?.totals.requests).toBe(7);
  });

  it("restores the run so far from the backend", async () => {
    mocked.activeLoadTest.mockResolvedValueOnce({
      runId: "r9",
      testId: "smoke",
      workspacePath: WS,
      name: "Smoke",
      startedAt: 5,
      plannedMs: 3000,
      snapshot: snapshot(2000, [], 40),
      points: [point(0), point(1)],
    });
    await restoreActiveRun();
    expect(useLoadTests.getState().active?.snapshot?.totals.requests).toBe(40);
    expect(useLoadTests.getState().active?.points).toHaveLength(2);
    expect(useLoadTests.getState().active?.plannedMs).toBe(3000);
  });

  it("stops, and catches up when the backend says it already ended", async () => {
    mocked.startLoadTest.mockResolvedValueOnce({ runId: "r6", plannedMs: 3000, targets: [] });
    await startLoadRun("smoke", test);
    await stopLoadRun();
    expect(mocked.stopLoadTest).toHaveBeenCalledWith("r6");
    expect(useLoadTests.getState().active?.stopping).toBe(true);
    await restoreActiveRun();
    expect(useLoadTests.getState().active).toBeNull();
    expect(useLoadTests.getState().historyVersion.smoke).toBe(1);
    // A late result still counts.
    emit({ type: "load", runId: "r6", event: { type: "finished", summary: summary(false) } });
    expect(useLoadTests.getState().last.smoke?.runId).toBe("r6");
    expect(useToasts.getState().toasts.at(-1)?.tone).toBe("error");
  });
});

describe("saved load tests and tabs", () => {
  it("deletes a running test once its run has ended (its result would recreate the history)", async () => {
    mocked.startLoadTest.mockResolvedValueOnce({ runId: "r8", plannedMs: 3000, targets: [] });
    await startLoadRun("smoke", test);
    const deleting = deleteLoadTest({ id: "smoke", name: "Smoke", model: "virtualUsers", targets: 1, durationSecs: 3, seq: 0 });
    const dialog = useDialogs.getState().current;
    expect(dialog?.kind === "confirm" && dialog.message).toMatch(/and stopped/);
    answerDialog(true);
    await vi.waitFor(() => expect(mocked.stopLoadTest).toHaveBeenCalledWith("r8"));
    expect(mocked.deleteLoadTest).not.toHaveBeenCalled();
    emit({ type: "load", runId: "r8", event: { type: "finished", summary: summary(true, { stoppedEarly: true }) } });
    await deleting;
    expect(mocked.deleteLoadTest).toHaveBeenCalledWith("smoke");
    expect(useLoadTests.getState().last.smoke).toBeUndefined();
    expect(useLoadTests.getState().active).toBeNull();
  });

  it("remembers the run to compare with until it is deleted", () => {
    setCompareRun("smoke", "r1");
    setCompareRun("other", "r2");
    expect(useLoadTests.getState().compare).toEqual({ smoke: "r1", other: "r2" });
    forgetResult("smoke", "r9");
    expect(useLoadTests.getState().compare.smoke).toBe("r1");
    forgetResult("smoke", "r1");
    expect(useLoadTests.getState().compare).toEqual({ other: "r2" });
    setCompareRun("other", null);
    expect(useLoadTests.getState().compare).toEqual({});
  });

  it("treats file-format defaults as unchanged", () => {
    const withDefaults: LoadTest = {
      ...test,
      targets: [{ request: "a.yaml", weight: 1, enabled: true }],
      keepAlive: true,
      maxInFlight: 1000,
      thinkTimeMs: 0,
      thresholds: [],
    };
    expect(sameLoadTest(test, withDefaults)).toBe(true);
    expect(sameLoadTest(test, { ...test, targets: [{ request: "a.yaml", weight: 2 }] })).toBe(false);
    expect(sameLoadTest(test, { ...test, keepAlive: false })).toBe(false);
    // No data file and no captures are what a file without them means.
    expect(sameLoadTest(test, { ...test, dataFile: "", targets: [{ request: "a.yaml", captures: [] }] })).toBe(true);
    expect(sameLoadTest(test, { ...test, dataFile: "users.csv" })).toBe(false);
  });

  it("creates a load test for a request and opens it", async () => {
    mocked.createLoadTest.mockResolvedValueOnce("Get users load test");
    mocked.readLoadTest.mockImplementation(() => Promise.resolve(mocked.createLoadTest.mock.calls[0][0]));
    await loadTestRequest("Users/Get users.yaml", "Get users");
    const created = mocked.createLoadTest.mock.calls[0][0] as LoadTest;
    expect(created.name).toBe("Get users load test");
    expect(created.targets).toEqual([{ request: "Users/Get users.yaml" }]);
    const tab = useTabs.getState().tabs[0];
    expect(tab).toMatchObject({ type: "loadTest", testId: "Get users load test" });
  });

  it("creates one for a folder with every HTTP request in it, or explains why not", async () => {
    const tree = [
      {
        kind: "folder" as const,
        path: "Users",
        name: "Users",
        seq: 0,
        children: [
          { kind: "request" as const, path: "Users/A.yaml", name: "A", seq: 0, method: "GET", requestKind: "http" as const, children: [] },
          { kind: "request" as const, path: "Users/WS.yaml", name: "WS", seq: 1, method: "GET", requestKind: "websocket" as const, children: [] },
        ],
      },
    ];
    mocked.createLoadTest.mockResolvedValueOnce("Users load test");
    mocked.readLoadTest.mockResolvedValueOnce(test);
    await loadTestFolder(tree, "Users", "Users");
    expect((mocked.createLoadTest.mock.calls[0][0] as LoadTest).targets).toEqual([{ request: "Users/A.yaml" }]);

    await loadTestFolder([], "Empty", "Empty");
    expect(mocked.createLoadTest).toHaveBeenCalledTimes(1);
    expect(useToasts.getState().toasts.at(-1)?.title).toBe("No HTTP requests to load test");
  });

  it("saves a load test tab and follows a rename of its file", async () => {
    mocked.readLoadTest.mockResolvedValueOnce(test);
    await openLoadTest("smoke");
    const id = useTabs.getState().activeId!;
    updateLoadTestDraft(id, (t) => ({ ...t, name: "Smoke 2" }));
    expect(isDirty(useTabs.getState().tabs[0])).toBe(true);
    useLoadTests.setState({ last: { smoke: { runId: "r", summary: summary() } } });
    mocked.saveLoadTest.mockResolvedValueOnce("Smoke 2");
    expect(await saveTab(id)).toBe(true);
    const tab = useTabs.getState().tabs[0];
    expect(tab).toMatchObject({ testId: "Smoke 2" });
    expect(isDirty(tab)).toBe(false);
    expect(useLoadTests.getState().last["Smoke 2"]?.runId).toBe("r");
    expect(useLoadTests.getState().last.smoke).toBeUndefined();
  });

  it("saving a tab whose file was deleted writes it again", async () => {
    mocked.readLoadTest.mockResolvedValueOnce(test);
    await openLoadTest("smoke");
    const id = useTabs.getState().activeId!;
    onLoadTestDeleted("smoke");
    mocked.saveLoadTest.mockRejectedValueOnce(new RpcError({ code: "notFound", message: "Load test 'smoke' not found", networkKind: null }));
    mocked.createLoadTest.mockResolvedValueOnce("Smoke");
    expect(await saveTab(id)).toBe(true);
    expect(mocked.createLoadTest).toHaveBeenCalledWith(test);
    expect(useTabs.getState().tabs[0]).toMatchObject({ testId: "Smoke", orphaned: false });
  });

  it("keeps the sidebar order in open tabs, so saving does not undo a reorder", async () => {
    mocked.readLoadTest.mockResolvedValueOnce(test);
    await openLoadTest("smoke");
    const id = useTabs.getState().activeId!;
    updateLoadTestDraft(id, (t) => ({ ...t, stages: [{ durationSecs: 9, target: 1 }] }));
    // Reordered in the sidebar: the file changes only its seq.
    mocked.readLoadTest.mockResolvedValueOnce({ ...test, seq: 4 });
    emit({ type: "workspaceChanged", paths: ["loadtests/smoke.yaml"] });
    await vi.waitFor(() => expect((useTabs.getState().tabs[0] as LoadTestTab).draft.seq).toBe(4));
    const tab = useTabs.getState().tabs[0] as LoadTestTab;
    expect(tab.draft.stages).toEqual([{ durationSecs: 9, target: 1 }]);
    expect(isDirty(tab)).toBe(true);
  });

  it("renames from the sidebar: tabs, results and the run in progress follow", async () => {
    mocked.readLoadTest.mockResolvedValue(test);
    await openLoadTest("smoke");
    mocked.startLoadTest.mockResolvedValueOnce({ runId: "r7", plannedMs: 3000, targets: [] });
    await startLoadRun("smoke", test);
    mocked.saveLoadTest.mockResolvedValueOnce("Renamed");
    const renaming = renameLoadTest({ id: "smoke", name: "Smoke", model: "virtualUsers", targets: 1, durationSecs: 3, seq: 0 });
    answerDialog("Renamed");
    await renaming;
    expect(useTabs.getState().tabs[0]).toMatchObject({ testId: "Renamed" });
    expect(runFor("Renamed")?.runId).toBe("r7");
  });
});

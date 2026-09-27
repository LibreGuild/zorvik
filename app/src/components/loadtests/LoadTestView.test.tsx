// Render a load test tab with realistic state (no backend): idle with no runs,
// live with snapshots, and a finished run; editing the settings updates the draft.
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { LoadTest } from "../../bindings/LoadTest";
import type { MetricsSummary } from "../../bindings/MetricsSummary";
import type { Snapshot } from "../../bindings/Snapshot";
import type { Summary } from "../../bindings/Summary";
import type { TimePoint } from "../../bindings/TimePoint";
import type { TreeNode } from "../../bindings/TreeNode";
import type { WorkspaceInfo } from "../../bindings/WorkspaceInfo";
import type { LoadTestTab } from "../../store/tabs";

vi.mock("../../lib/events", () => ({ onEvent: () => () => {} }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("../../lib/rpc", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../lib/rpc")>();
  const loadRuns = vi.fn(() => Promise.resolve([]));
  const loadRun = vi.fn(() => Promise.resolve(null));
  const previewRunData = vi.fn(() => Promise.resolve(null));
  return { ...actual, api: { loadRuns, loadRun, previewRunData, startLoadTest: vi.fn(() => Promise.resolve(null)) } };
});

const { api } = await import("../../lib/rpc");
const { TooltipProvider } = await import("../ui");
const { LoadTestView, startProblem } = await import("./LoadTestView");
const { useTabs, updateLoadTestDraft } = await import("../../store/tabs");
const { useLoadTests } = await import("../../store/loadtests");
const { useWorkspace } = await import("../../store/workspace");
const mocked = api as unknown as Record<string, ReturnType<typeof vi.fn>>;

const WS = "/ws";
const tree: TreeNode[] = [
  {
    kind: "folder",
    path: "Users",
    name: "Users",
    seq: 0,
    children: [
      { kind: "request", path: "Users/List.yaml", name: "List users", seq: 0, method: "GET", requestKind: "http", children: [] },
      { kind: "request", path: "Users/Create.yaml", name: "Create user", seq: 1, method: "POST", requestKind: "http", children: [] },
    ],
  },
];
const test: LoadTest = {
  name: "Users load test",
  seq: 0,
  targets: [{ request: "Users/List.yaml" }, { request: "Users/Gone.yaml", weight: 3 }],
  model: "virtualUsers",
  stages: [
    { durationSecs: 10, target: 10 },
    { durationSecs: 40, target: 10 },
    { durationSecs: 10, target: 0 },
  ],
  thresholds: [{ metric: "p95", op: "<", value: 500 }],
};
const latency = { min: 1, avg: 12, p50: 10, p90: 20, p95: 25, p99: 40, p999: 60, max: 80 };
const phase = (p95: number, count = 100) => ({ count, avg: p95 / 2, p50: p95 / 2, p95, p99: p95 * 2, max: p95 * 3 });
const totals = (requests: number, errors = 0, ttfb = 20): MetricsSummary => ({
  requests,
  errors,
  errorRate: requests ? (errors * 100) / requests : 0,
  rps: requests / 10,
  bytesIn: 123456,
  bytesOut: 2345,
  latency,
  statusCodes: [
    [200, requests - errors],
    [503, errors],
  ],
  errorKinds: errors ? [["timeout", 1]] : [],
  dropped: 0,
  connections: 10,
  timing: { connect: phase(3, 10), ttfb: phase(ttfb), transfer: phase(1), server: phase(0, 0) },
  captureMisses: 0,
});
const points = (n: number): TimePoint[] => Array.from({ length: n }, (_, i) => ({ second: i, rps: 100 + (i % 7), errors: i % 5 === 0 ? 1 : 0, p50: 10, p95: 25, p99: 40, active: 10, target: 10 }));
const snapshot: Snapshot = {
  phase: "running",
  elapsedMs: 32_000,
  plannedMs: 60_000,
  active: 10,
  target: 10,
  totals: totals(3200, 4),
  targets: [{ name: "List users", request: "Users/List.yaml", metrics: totals(3200, 4) }],
  points: [],
  thresholds: [{ label: "p95 < 500 ms", metric: "p95", op: "<", value: 500, target: null, actual: 25, passed: true }],
  // 95% of every core: above the warning level.
  cpuPercent: 95 * Math.max(1, navigator.hardwareConcurrency || 1),
};
const summary: Summary = {
  startedAt: Date.UTC(2026, 8, 27, 12, 0),
  durationMs: 60_000,
  totals: totals(6000),
  targets: [{ name: "List users", request: "Users/List.yaml", metrics: totals(6000) }],
  points: points(60),
  thresholds: [{ label: "p95 < 500 ms", metric: "p95", op: "<", value: 500, target: null, actual: 25, passed: true }],
  passed: true,
  stoppedEarly: false,
  error: null,
  peakCpuPercent: 30,
};

function renderView() {
  const tab: LoadTestTab = { type: "loadTest", id: "tab-1", testId: "users", draft: structuredClone(test), saved: test };
  useTabs.setState({ tabs: [tab], activeId: tab.id });
  const Current = () => {
    const current = useTabs((s) => s.tabs.find((t) => t.id === tab.id)) as LoadTestTab;
    return <LoadTestView tab={current} />;
  };
  return render(
    <TooltipProvider>
      <Current />
    </TooltipProvider>,
  );
}

const draft = () => (useTabs.getState().tabs[0] as LoadTestTab).draft;

beforeEach(() => {
  useWorkspace.setState({ info: { path: WS, tree } as unknown as WorkspaceInfo });
  useLoadTests.setState({ saved: [], active: null, starting: null, last: {}, historyVersion: {}, compare: {} });
  mocked.loadRuns.mockResolvedValue([]);
});
afterEach(cleanup);

describe("load test tab", () => {
  it("shows the settings, a missing request and the empty results", async () => {
    renderView();
    expect(screen.getByText("Users load test")).toBeTruthy();
    const rows = screen.getAllByTestId("load-target");
    expect(rows).toHaveLength(2);
    expect(within(rows[0]).getByText("List users")).toBeTruthy();
    expect(within(rows[0]).getByText("25%")).toBeTruthy();
    expect(within(rows[1]).getByText("Not found in the collection")).toBeTruthy();
    expect(screen.getByTestId("loadtest-status").textContent).toContain("A request is missing from the collection");
    expect(await screen.findByText("No runs yet")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Ramp up, hold, ramp down" }).getAttribute("aria-pressed")).toBe("true");
  });

  it("edits stages through the shape, peak and duration", () => {
    renderView();
    // Typing "100" passes through "1" and "10" (and an empty field): the shape survives.
    fireEvent.click(screen.getByRole("button", { name: "Spike" }));
    const shape = draft().stages;
    const peak = screen.getByLabelText("Peak load");
    fireEvent.focus(peak);
    for (const value of ["", "1", "10", "100"]) fireEvent.change(peak, { target: { value } });
    fireEvent.blur(peak);
    expect(draft().stages).toEqual(shape.map((s) => ({ ...s, target: s.target * 10 })));
    expect(screen.getByRole("button", { name: "Spike" }).getAttribute("aria-pressed")).toBe("true");

    fireEvent.click(screen.getByRole("button", { name: "Constant" }));
    expect(draft().stages).toEqual([
      { durationSecs: 0, target: 100 },
      { durationSecs: 60, target: 100 },
    ]);
    fireEvent.change(screen.getByLabelText("Peak load"), { target: { value: "5" } });
    fireEvent.change(screen.getByLabelText("Total duration in seconds"), { target: { value: "3" } });
    expect(draft().stages).toEqual([
      { durationSecs: 0, target: 5 },
      { durationSecs: 3, target: 5 },
    ]);
    fireEvent.click(screen.getByRole("button", { name: "Add threshold" }));
    expect(draft().thresholds?.map((t) => t.metric)).toEqual(["p95", "errorRate"]);
    fireEvent.click(screen.getByRole("button", { name: "Request rate" }));
    expect(draft().model).toBe("arrivalRate");
    expect(screen.getByLabelText("Most requests in flight")).toBeTruthy();
  });

  it("shows a live run: numbers, thresholds, charts and a CPU warning", async () => {
    renderView();
    const button = screen.getByTestId("loadtest-start");
    act(() => {
      useLoadTests.setState({
        active: {
          runId: "r1",
          testId: "users",
          workspacePath: WS,
          name: "Users load test",
          startedAt: Date.now() - 32_000,
          plannedMs: 60_000,
          model: "virtualUsers",
          snapshot,
          points: points(32),
          stopping: false,
        },
      });
    });
    expect(screen.getByTestId("loadtest-status").textContent).toContain("Running");
    expect(screen.getByTestId("loadtest-status").textContent).toContain("32 s of 1m");
    expect(screen.getByTestId("stat-requests-value").getAttribute("data-value")).toBe("3200");
    expect(screen.getByTestId("stat-requests-value").textContent).toBe("3,200");
    expect(screen.getByTestId("stat-errors-value").textContent).toBe("0.13%");
    // The same button turns into Stop: keyboard focus stays on it.
    expect(screen.getByTestId("loadtest-stop")).toBe(button);
    expect(button.textContent).toBe("Stop");
    expect(screen.getByRole("progressbar").getAttribute("aria-valuenow")).toBe("53");
    expect(within(screen.getByTestId("load-threshold-results")).getByText("PASS")).toBeTruthy();
    expect(screen.getByText(/the laptop may be the bottleneck/)).toBeTruthy();
    expect(within(screen.getByTestId("load-errors")).getByText("Timed out")).toBeTruthy();
  });

  it("shows the latest finished run with its verdict", async () => {
    mocked.loadRuns.mockResolvedValue([
      { runId: "r9", startedAt: summary.startedAt, durationMs: 60_000, passed: true, stoppedEarly: false, requests: 6000, rps: 100, p95: 25, errorRate: 0, error: null },
    ]);
    mocked.loadRun.mockResolvedValue(summary);
    renderView();
    const verdict = await screen.findByTestId("load-verdict");
    expect(verdict.textContent).toContain("PASSED");
    expect(verdict.textContent).toContain("The threshold passed.");
    expect(screen.getByTestId("load-runs").textContent).toContain("1 run");
    expect(screen.getByRole("button", { name: "Export run" })).toBeTruthy();
  });

  it("counts a threshold without data as failed once the run is over", async () => {
    const noData = { label: "p95 < 500 ms · Gone", metric: "p95" as const, op: "<" as const, value: 500, target: "Users/Gone.yaml", actual: null, passed: false };
    mocked.loadRuns.mockResolvedValue([
      { runId: "r10", startedAt: summary.startedAt, durationMs: 60_000, passed: false, stoppedEarly: false, requests: 6000, rps: 100, p95: 25, errorRate: 0, error: null },
    ]);
    mocked.loadRun.mockResolvedValue({ ...summary, passed: false, thresholds: [...summary.thresholds, noData] });
    renderView();
    const panel = await screen.findByTestId("load-threshold-results");
    expect(panel.textContent).toContain("1 failing");
    expect(panel.textContent).toContain("no data");
    expect(within(panel).getByText("FAIL")).toBeTruthy();
  });

  it("keeps the per-request table's first column in view when it scrolls", async () => {
    mocked.loadRuns.mockResolvedValue([
      { runId: "r11", startedAt: summary.startedAt, durationMs: 60_000, passed: true, stoppedEarly: false, requests: 6000, rps: 100, p95: 25, errorRate: 0, error: null },
    ]);
    mocked.loadRun.mockResolvedValue(summary);
    renderView();
    const table = await screen.findByTestId("load-targets-table");
    expect(table.querySelector("th")?.className).toContain("sticky");
    expect(table.querySelector("td")?.className).toContain("sticky");
    expect(table.querySelector("table")?.className).not.toContain("min-w-");
  });

  it("keeps option values within what the file and the generator accept", () => {
    renderView();
    fireEvent.change(screen.getByLabelText("Think time in milliseconds"), { target: { value: "99999999999" } });
    expect(draft().thinkTimeMs).toBe(3_600_000);
    fireEvent.change(screen.getByLabelText("Timeout in milliseconds"), { target: { value: "-5" } });
    expect(draft().timeoutMs).toBe(0);
    fireEvent.change(screen.getByLabelText("Timeout in milliseconds"), { target: { value: "" } });
    expect(draft().timeoutMs).toBeUndefined();
    // Above the limit is kept (and explained) but never beyond a whole number the file can hold.
    const target = screen.getByLabelText("Stage 1 target");
    fireEvent.change(target, { target: { value: "1e20" } });
    expect(draft().stages[0].target).toBe(4_294_967_295);
    expect(target.getAttribute("aria-invalid")).toBe("true");
  });

  it("explains why a test can't start", () => {
    expect(startProblem({ ...test, targets: [] }, tree)).toBe("Add a request to send");
    expect(startProblem({ ...test, targets: [{ request: "Users/List.yaml", enabled: false }] }, tree)).toBe("Enable a request with a weight above 0");
    expect(startProblem({ ...test, targets: [{ request: "Users/List.yaml" }], stages: [{ durationSecs: 0, target: 5 }] }, tree)).toBe("Give a stage a duration");
    expect(startProblem({ ...test, targets: [{ request: "Users/List.yaml" }], stages: [{ durationSecs: 5, target: 6000 }] }, tree)).toBe("Up to 5,000 users");
    expect(startProblem({ ...test, targets: [{ request: "Users/List.yaml" }] }, tree)).toBeNull();
    // A threshold on a request that is not sent would fail every run.
    const listOnly = { ...test, targets: [{ request: "Users/List.yaml" }, { request: "Users/Create.yaml", enabled: false }] };
    const onCreate = { metric: "p95" as const, op: "<" as const, value: 500, target: "Users/Create.yaml" };
    expect(startProblem({ ...listOnly, thresholds: [onCreate] }, tree)).toBe("A threshold checks a request this test doesn't send");
    expect(startProblem({ ...listOnly, thresholds: [{ ...onCreate, enabled: false }] }, tree)).toBeNull();
    expect(startProblem({ ...listOnly, thresholds: [{ ...onCreate, target: "Users/List.yaml" }] }, tree)).toBeNull();
    const capture = { variable: "id", from: "json" as const, path: "" };
    expect(startProblem({ ...test, targets: [{ request: "Users/List.yaml", captures: [capture] }] }, tree)).toBe("A capture needs a variable name and a path");
    expect(startProblem({ ...test, targets: [{ request: "Users/List.yaml", captures: [{ ...capture, path: "$.id" }] }] }, tree)).toBeNull();
  });

  it("edits a request's captures and the data file", async () => {
    mocked.previewRunData.mockResolvedValue({ format: "csv", columns: ["user"], rows: [["ada"], ["grace"]], count: 2 });
    renderView();
    const toggle = screen.getByRole("button", { name: "Captures of List users" });
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
    fireEvent.click(toggle);
    fireEvent.click(screen.getByRole("button", { name: "Add capture" }));
    expect(draft().targets[0].captures).toEqual([{ variable: "", from: "json", path: "" }]);
    expect(screen.getByText("Name the variable")).toBeTruthy();
    fireEvent.change(screen.getByLabelText("Variable 1 of List users"), { target: { value: "orderId" } });
    fireEvent.change(screen.getByLabelText("JSON path of capture 1 of List users"), { target: { value: "$.id" } });
    fireEvent.change(screen.getByLabelText("Where capture 1 of List users comes from"), { target: { value: "header" } });
    expect(draft().targets[0].captures).toEqual([{ variable: "orderId", from: "header", path: "$.id" }]);
    expect(toggle.textContent).toBe("1");
    fireEvent.click(screen.getByRole("button", { name: "Remove capture 1 of List users" }));
    expect(draft().targets[0].captures).toBeUndefined();

    // The data file shows its rows once read.
    act(() => updateLoadTestDraft("tab-1", (t) => ({ ...t, dataFile: "users.csv" })));
    const preview = await screen.findByTestId("load-data-preview");
    expect(mocked.previewRunData).toHaveBeenCalledWith("users.csv");
    expect(within(preview).getByText("grace")).toBeTruthy();
    expect(screen.getByText("CSV · 2 rows")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Remove the data file" }));
    expect(draft().dataFile).toBeUndefined();
  });

  it("shows the timing and compares with an earlier run", async () => {
    const earlier: Summary = { ...summary, startedAt: summary.startedAt - 86_400_000, totals: { ...totals(4800, 48, 40), rps: 480 } };
    mocked.loadRuns.mockResolvedValue([
      { runId: "r2", startedAt: summary.startedAt, durationMs: 60_000, passed: true, stoppedEarly: false, requests: 6000, rps: 100, p95: 25, errorRate: 0, error: null },
      { runId: "r1", startedAt: earlier.startedAt, durationMs: 60_000, passed: true, stoppedEarly: false, requests: 5000, rps: 80, p95: 25, errorRate: 1, error: null },
    ]);
    mocked.loadRun.mockImplementation((_id: string, runId: string) => Promise.resolve(runId === "r1" ? earlier : summary));
    renderView();
    const timing = await screen.findByTestId("load-timing");
    expect(within(timing).getByText("Time to first byte")).toBeTruthy();
    // No Server-Timing header in this run: no server-reported row, and a note that says so.
    expect(within(timing).queryByText("Server-reported")).toBeNull();
    expect(timing.textContent).toContain("have it send a Server-Timing header");
    expect(timing.textContent).toContain("New connection for 0.17% of requests");

    act(() => useLoadTests.setState({ compare: { users: "r1" } }));
    const panel = await screen.findByTestId("load-compare-panel");
    await within(panel).findByText("Requests / s");
    // Throughput up 25%: better. First byte 40 → 20 ms: better. Errors 1% → 0%: better.
    expect(screen.getByTestId("compare-change-rps").textContent).toBe("+25%");
    expect(screen.getByTestId("compare-change-rps").className).toContain("text-success");
    expect(screen.getByTestId("compare-change-ttfb").textContent).toBe("−50%");
    expect(screen.getByTestId("compare-change-errorRate").className).toContain("text-success");
    // The same latency: no change, no colour.
    expect(screen.getByTestId("compare-change-p95").textContent).toBe("0%");
    expect(screen.getByTestId("compare-change-p95").className).toContain("text-muted");
    expect(within(panel).getByText("List users p95")).toBeTruthy();
    fireEvent.click(within(panel).getByRole("button", { name: "Stop comparing" }));
    expect(useLoadTests.getState().compare).toEqual({});
  });
});

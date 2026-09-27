// Load tests: the saved list of the open workspace, the run in progress (one at
// a time, app-wide), its live snapshots, and the latest result per test.
//
// Snapshots arrive about 4×/s; they are coalesced per animation frame so a
// burst (e.g. after the window was hidden) renders once.
import { create } from "zustand";
import type { LoadEvent } from "../bindings/LoadEvent";
import type { LoadModel } from "../bindings/LoadModel";
import type { LoadStarted } from "../bindings/LoadStarted";
import type { LoadTest } from "../bindings/LoadTest";
import type { LoadTestNode } from "../bindings/LoadTestNode";
import type { Snapshot } from "../bindings/Snapshot";
import type { Summary } from "../bindings/Summary";
import type { TimePoint } from "../bindings/TimePoint";
import type { TreeNode } from "../bindings/TreeNode";
import { onEvent } from "../lib/events";
import { api, errorMessage, RpcError } from "../lib/rpc";
import { formatDuration, formatLatency, formatPercent, formatRate, httpRequestsUnder, nameFromPath } from "../components/loadtests/model";
import { confirm, prompt } from "./dialogs";
import { onLoadTestDeleted, onLoadTestFilesChanged, onLoadTestRenamed, onLoadTestSaved, openLoadTest } from "./tabs";
import { toast } from "./toasts";
import { useUi } from "./ui";
import { useWorkspace } from "./workspace";

/** Chart points kept for a run; older seconds are merged in pairs beyond this (2 h at 1 s). */
export const MAX_POINTS = 7200;

/** The run in progress. */
export interface LiveRun {
  runId: string;
  testId: string;
  workspacePath: string;
  name: string;
  /** Unix epoch milliseconds. */
  startedAt: number;
  plannedMs: number;
  /** Unknown for a run found running (until its test is read). */
  model: LoadModel | null;
  snapshot: Snapshot | null;
  /** Every second so far (bounded by MAX_POINTS). */
  points: TimePoint[];
  stopping: boolean;
}

export interface FinishedRun {
  runId: string;
  summary: Summary;
}

interface LoadTestsState {
  saved: LoadTestNode[];
  active: LiveRun | null;
  /** Test being started (the start call is in flight, maybe waiting for a confirmation). */
  starting: string | null;
  /** Latest finished run per test id (of the open workspace). */
  last: Record<string, FinishedRun>;
  /** Bumped when a test's run history changes (a run finished or was deleted). */
  historyVersion: Record<string, number>;
  /** The earlier run each test's results are compared with (test id → run id). */
  compare: Record<string, string>;
}

export const useLoadTests = create<LoadTestsState>(() => ({ saved: [], active: null, starting: null, last: {}, historyVersion: {}, compare: {} }));
const set = useLoadTests.setState;
const get = useLoadTests.getState;

const currentWorkspace = () => useWorkspace.getState().info?.path ?? "";

/** The run in progress when it belongs to this saved test of the open workspace. */
export function runFor(testId: string, active: LiveRun | null = get().active): LiveRun | null {
  return active && active.testId === testId && active.workspacePath === currentWorkspace() ? active : null;
}

// ---- pure reducers (exported for tests) --------------------------------------------

/** Merge neighbouring seconds pairwise: rates and counts averaged, latency kept at the worse of the two. */
export function compactPoints(points: TimePoint[]): TimePoint[] {
  const out: TimePoint[] = [];
  for (let i = 0; i < points.length; i += 2) {
    const a = points[i];
    const b = points[i + 1];
    if (!b) {
      out.push(a);
      continue;
    }
    out.push({
      second: a.second,
      rps: (a.rps + b.rps) / 2,
      errors: Math.round((a.errors + b.errors) / 2),
      p50: Math.max(a.p50, b.p50),
      p95: Math.max(a.p95, b.p95),
      p99: Math.max(a.p99, b.p99),
      active: Math.max(a.active, b.active),
      target: b.target,
    });
  }
  return out;
}

/** Append new seconds (skipping ones already there) and stay under `max` points. */
export function appendPoints(points: TimePoint[], extra: TimePoint[], max = MAX_POINTS): TimePoint[] {
  const lastSecond = points.length ? points[points.length - 1].second : -1;
  const fresh = extra.filter((p, i) => p.second > lastSecond && (i === 0 || p.second > extra[i - 1].second));
  if (!fresh.length) return points;
  let next = points.concat(fresh);
  while (next.length > max) next = compactPoints(next);
  return next;
}

/** A run with the latest snapshot and its new seconds applied. */
export function applySnapshot(run: LiveRun, snapshot: Snapshot, points: TimePoint[] = snapshot.points): LiveRun {
  return {
    ...run,
    snapshot,
    plannedMs: snapshot.plannedMs || run.plannedMs,
    points: appendPoints(run.points, points),
    stopping: run.stopping || snapshot.phase === "stopping",
  };
}

/** Toast text for a finished run: `{title, detail, tone}`. */
export function finishedMessage(name: string, summary: Summary): { tone: "success" | "error" | "info"; title: string; detail: string } {
  const t = summary.totals;
  const numbers = [
    `${t.requests.toLocaleString("en-US")} requests`,
    `${formatRate(t.rps)} req/s`,
    `p95 ${formatLatency(t.latency.p95)}`,
    `${formatPercent(t.errorRate)} errors`,
  ].join(" · ");
  if (summary.error) return { tone: "error", title: `“${name}” could not finish`, detail: summary.error };
  const failed = summary.thresholds.filter((x) => !x.passed);
  const early = summary.stoppedEarly ? `Stopped after ${formatDuration(summary.durationMs / 1000)} · ` : "";
  if (failed.length) {
    return {
      tone: "error",
      title: `“${name}” failed ${failed.length === 1 ? "a threshold" : `${failed.length} thresholds`}`,
      detail: `${early}${failed.map((x) => x.label).join(", ")} · ${numbers}`,
    };
  }
  return {
    tone: summary.stoppedEarly ? "info" : "success",
    title: `“${name}” ${summary.thresholds.length ? "passed" : "finished"}`,
    detail: `${early}${numbers}`,
  };
}

// ---- saved list ----------------------------------------------------------------------

export async function refreshLoadTests() {
  if (!useWorkspace.getState().info) {
    set({ saved: [] });
    return;
  }
  const ws = currentWorkspace();
  try {
    const saved = await api.loadTests();
    if (currentWorkspace() === ws) set({ saved });
  } catch {
    if (currentWorkspace() === ws) set({ saved: [] });
  }
}

/** The open workspace changed: results of the previous one no longer apply. */
export function resetLoadTestResults() {
  set({ last: {}, historyVersion: {} });
}

onLoadTestFilesChanged(() => void refreshLoadTests());
onLoadTestSaved((oldId, newId) => {
  if (oldId !== newId) moveResults(oldId, newId);
  void refreshLoadTests();
});

/** A new load test with the file format's defaults: ramp to 10 users over 10 s, hold 40 s, ramp down. */
export function newLoadTest(name: string, requests: string[] = []): LoadTest {
  return {
    name,
    seq: 0,
    targets: requests.map((request) => ({ request })),
    model: "virtualUsers",
    stages: [
      { durationSecs: 10, target: 10 },
      { durationSecs: 40, target: 10 },
      { durationSecs: 10, target: 0 },
    ],
    thresholds: [
      { metric: "p95", op: "<", value: 500 },
      { metric: "errorRate", op: "<", value: 1 },
    ],
  };
}

/** Create a load test and open it; returns its id. */
export async function createLoadTest(test: LoadTest): Promise<string | null> {
  try {
    const id = await api.createLoadTest(test);
    await refreshLoadTests();
    await openLoadTest(id);
    useUi.setState({ sidebarTab: "loadtests" });
    return id;
  } catch (e) {
    toast("error", "Could not create load test", errorMessage(e));
    return null;
  }
}

/** "New load test" (asks for a name; no requests yet). */
export async function newLoadTestPrompt() {
  const name = await prompt({ title: "New load test", value: "Load test", confirmLabel: "Create" });
  if (name) await createLoadTest(newLoadTest(name));
}

/** "Load test…" on a saved request. */
export async function loadTestRequest(path: string, requestName?: string) {
  const name = requestName || nameFromPath(path);
  await createLoadTest(newLoadTest(`${name} load test`, [path]));
}

/** "Load test this folder…": every HTTP request under it, weight 1 each. */
export async function loadTestFolder(tree: TreeNode[], folder: string, folderName: string) {
  const requests = httpRequestsUnder(tree, folder);
  if (!requests.length) {
    toast("info", "No HTTP requests to load test", `“${folderName}” has no HTTP requests (WebSocket, TCP and other kinds can't be load tested).`);
    return;
  }
  await createLoadTest(newLoadTest(`${folderName} load test`, requests));
}

export async function renameLoadTest(node: LoadTestNode) {
  const name = await prompt({ title: "Rename load test", value: node.name, confirmLabel: "Rename" });
  if (!name || name === node.name) return;
  try {
    const test = await api.readLoadTest(node.id);
    const saved = { ...test, name };
    const newId = await api.saveLoadTest(node.id, saved);
    onLoadTestRenamed(node.id, newId, saved);
    moveResults(node.id, newId);
    await refreshLoadTests();
  } catch (e) {
    toast("error", "Could not rename load test", errorMessage(e));
  }
}

export async function duplicateLoadTest(node: LoadTestNode) {
  try {
    const id = await api.duplicateLoadTest(node.id);
    await refreshLoadTests();
    await openLoadTest(id);
  } catch (e) {
    toast("error", "Could not duplicate load test", errorMessage(e));
  }
}

export async function deleteLoadTest(node: LoadTestNode) {
  const ok = await confirm({
    title: "Delete load test?",
    message: `“${node.name}” will be moved to the trash${runFor(node.id) ? " and stopped" : ""}. Its run history is deleted.`,
    confirmLabel: "Delete",
    danger: true,
  });
  if (!ok) return;
  // Checked again: the run may have ended (and another one started) while the dialog was open.
  const running = runFor(node.id);
  try {
    if (running) {
      // Its result is written to the history when it ends: let that happen before the history goes.
      await stopLoadRun();
      await waitForRunEnd(running.runId);
    }
    await api.deleteLoadTest(node.id);
    onLoadTestDeleted(node.id);
    set((s) => {
      const last = { ...s.last };
      delete last[node.id];
      return { last, historyVersion: { ...s.historyVersion, [node.id]: (s.historyVersion[node.id] ?? 0) + 1 } };
    });
    await refreshLoadTests();
  } catch (e) {
    toast("error", "Could not delete load test", errorMessage(e));
  }
}

export async function reorderLoadTests(ids: string[]) {
  set((s) => ({ saved: ids.map((id) => s.saved.find((n) => n.id === id)!).filter(Boolean) }));
  await api.reorderLoadTests(ids).catch((e) => toast("error", "Could not reorder", errorMessage(e)));
  await refreshLoadTests();
}

/** A rename: results and the run in progress follow the new id (the backend moves the history). */
function moveResults(oldId: string, newId: string) {
  if (oldId === newId) return;
  const ws = currentWorkspace();
  set((s) => {
    const last = { ...s.last };
    if (oldId in last) {
      last[newId] = last[oldId];
      delete last[oldId];
    }
    const historyVersion = { ...s.historyVersion, [newId]: (s.historyVersion[oldId] ?? 0) + 1 };
    delete historyVersion[oldId];
    const active = s.active && s.active.testId === oldId && s.active.workspacePath === ws ? { ...s.active, testId: newId } : s.active;
    return { last, historyVersion, active };
  });
}

/** Remember a run's result as the test's latest (e.g. loaded from history). */
export function rememberResult(testId: string, run: FinishedRun) {
  set((s) => (s.last[testId]?.runId === run.runId ? s : { last: { ...s.last, [testId]: run } }));
}

/** A run was deleted from the history. */
export function forgetResult(testId: string, runId: string) {
  set((s) => {
    const last = { ...s.last };
    if (last[testId]?.runId === runId) delete last[testId];
    const compare = { ...s.compare };
    if (compare[testId] === runId) delete compare[testId];
    return { last, compare, historyVersion: { ...s.historyVersion, [testId]: (s.historyVersion[testId] ?? 0) + 1 } };
  });
}

/** Compare a test's results with an earlier run (`null`: stop comparing). */
export function setCompareRun(testId: string, runId: string | null) {
  set((s) => {
    const compare = { ...s.compare };
    if (runId) compare[testId] = runId;
    else delete compare[testId];
    return { compare };
  });
}

// ---- start / stop ------------------------------------------------------------------------

/** Start a load test with the given configuration (a tab's draft, possibly unsaved). */
export async function startLoadRun(testId: string, test: LoadTest): Promise<boolean> {
  const { active, starting } = get();
  if (starting) return false;
  if (active) {
    toast("info", "A load test is already running", `Stop “${active.name}” first: one load test runs at a time.`);
    return false;
  }
  const ws = currentWorkspace();
  set({ starting: testId });
  try {
    let started: LoadStarted;
    try {
      started = await api.startLoadTest(testId, test, false);
    } catch (e) {
      if (!(e instanceof RpcError) || e.code !== "confirmTarget") throw e;
      const ok = await confirm({ title: "Send load to another host?", message: e.message, confirmLabel: "Run load test", danger: true });
      if (!ok) return false;
      started = await api.startLoadTest(testId, test, true);
    }
    const run: LiveRun = {
      runId: started.runId,
      testId,
      workspacePath: ws,
      name: test.name,
      startedAt: Date.now(),
      plannedMs: started.plannedMs,
      model: test.model,
      snapshot: null,
      points: [],
      stopping: false,
    };
    // A catch-up with the backend may have picked this run up already (with its points so far).
    if (get().active?.runId !== run.runId) set({ active: run });
    claimPending(run.runId);
    return true;
  } catch (e) {
    const message = errorMessage(e);
    toast("error", `Could not start “${test.name}”`, message);
    // Out of step with the backend (e.g. a run started before the window reloaded).
    if (/already running/i.test(message)) void restoreActiveRun();
    return false;
  } finally {
    set({ starting: null });
  }
}

/** Stop the run in progress early (it reports its results as usual). */
export async function stopLoadRun() {
  const run = get().active;
  if (!run || run.stopping) return;
  set((s) => (s.active?.runId === run.runId ? { active: { ...s.active, stopping: true } } : s));
  try {
    await api.stopLoadTest(run.runId);
  } catch (e) {
    toast("error", "Could not stop the load test", errorMessage(e));
    set((s) => (s.active?.runId === run.runId ? { active: { ...s.active, stopping: false } } : s));
    return;
  }
  // The run ends with a "finished" event; if it never comes (the run had already ended), catch up.
  setTimeout(() => {
    if (get().active?.runId === run.runId) void restoreActiveRun();
  }, 10_000);
}

/** Resolves when `runId` is no longer the run in progress (or after `ms`). */
export function waitForRunEnd(runId: string, ms = 10_000): Promise<void> {
  return new Promise((resolve) => {
    if (get().active?.runId !== runId) return resolve();
    let unsubscribe = () => {};
    const done = () => {
      clearTimeout(timer);
      unsubscribe();
      resolve();
    };
    const timer = setTimeout(done, ms);
    unsubscribe = useLoadTests.subscribe((s) => {
      if (s.active?.runId !== runId) done();
    });
  });
}

/** Start or stop a test (Mod+Enter, the Start/Stop button). */
export async function toggleLoadRun(testId: string, test: LoadTest) {
  const active = get().active;
  if (active && runFor(testId, active)) return stopLoadRun();
  return startLoadRun(testId, test);
}

let restoring: Promise<void> | null = null;

/** Pick up the run in progress from the backend (startup, window reload, out of step). */
export function restoreActiveRun(): Promise<void> {
  if (restoring) return restoring;
  restoring = (async () => {
    try {
      const info = await api.activeLoadTest();
      const current = get().active;
      if (!info) {
        // Ended without us seeing its result: the view reads it from the history (a late result still counts).
        if (current && !get().starting) {
          ended.set(current.runId, current);
          set((s) => ({
            active: null,
            historyVersion: { ...s.historyVersion, [current.testId]: (s.historyVersion[current.testId] ?? 0) + 1 },
          }));
        }
        return;
      }
      if (current?.runId === info.runId) return;
      const node = get().saved.find((n) => n.id === info.testId);
      set({
        active: {
          runId: info.runId,
          testId: info.testId,
          workspacePath: info.workspacePath,
          name: info.name,
          startedAt: info.startedAt,
          plannedMs: info.plannedMs || (node ? node.durationSecs * 1000 : 0),
          model: info.workspacePath === currentWorkspace() ? (node?.model ?? null) : null,
          // The backend keeps the run so far: a reloaded window shows the whole chart.
          snapshot: info.snapshot ?? null,
          points: info.points ?? [],
          stopping: false,
        },
      });
      claimPending(info.runId);
    } catch {
      /* no workspace or backend unavailable */
    } finally {
      restoring = null;
    }
  })();
  return restoring;
}

// ---- events -------------------------------------------------------------------------------

interface Pending {
  snapshot: Snapshot | null;
  points: TimePoint[];
  finished: Summary | null;
  at: number;
}

/** Events not yet applied, by run id (also runs the UI doesn't know yet: a start in flight). */
const pending = new Map<string, Pending>();
/** Runs we already asked the backend about. */
const asked = new Set<string>();
/** Runs dropped by a catch-up with the backend before their result arrived. */
const ended = new Map<string, LiveRun>();
let flushScheduled = false;

function scheduleFlush() {
  if (flushScheduled) return;
  flushScheduled = true;
  const run = () => {
    if (flushScheduled) flushLoadEvents();
  };
  if (typeof requestAnimationFrame === "function") requestAnimationFrame(run);
  // Hidden windows get no animation frames.
  setTimeout(run, 250);
}

function queue(runId: string, event: LoadEvent) {
  let p = pending.get(runId);
  if (!p) {
    p = { snapshot: null, points: [], finished: null, at: Date.now() };
    pending.set(runId, p);
  }
  if (event.type === "snapshot") {
    p.snapshot = event.snapshot;
    p.points = appendPoints(p.points, event.snapshot.points);
    scheduleFlush();
  } else {
    p.finished = event.summary;
    // Results right away (toast, history) even when the window is hidden.
    flushLoadEvents();
  }
}

/** Apply queued events of the run in progress (exported for tests). */
export function flushLoadEvents() {
  flushScheduled = false;
  const active = get().active;
  for (const [runId, p] of pending) {
    if (active?.runId === runId) {
      pending.delete(runId);
      applyPending(active, p);
      continue;
    }
    const gone = ended.get(runId);
    if (gone && p.finished) {
      pending.delete(runId);
      ended.delete(runId);
      finishRun(gone, p.finished);
      continue;
    }
    // Not known yet: a start call still in flight claims it; otherwise ask the backend once.
    if (!get().starting && !asked.has(runId)) {
      asked.add(runId);
      void restoreActiveRun();
    }
    if (Date.now() - p.at > 30_000) pending.delete(runId);
  }
  // Bounded: at most a few unknown runs.
  while (pending.size > 4) pending.delete(pending.keys().next().value!);
}

function claimPending(runId: string) {
  const p = pending.get(runId);
  const active = get().active;
  if (!p || active?.runId !== runId) return;
  pending.delete(runId);
  applyPending(active, p);
}

function applyPending(run: LiveRun, p: Pending) {
  let next = run;
  if (p.snapshot) next = applySnapshot(run, p.snapshot, p.points);
  if (p.finished) {
    finishRun(next, p.finished);
    return;
  }
  if (next !== run) set((s) => (s.active?.runId === run.runId ? { active: next } : s));
}

function finishRun(run: LiveRun, summary: Summary) {
  const here = run.workspacePath === currentWorkspace();
  set((s) => ({
    active: s.active?.runId === run.runId ? null : s.active,
    ...(here
      ? {
          last: { ...s.last, [run.testId]: { runId: run.runId, summary } },
          historyVersion: { ...s.historyVersion, [run.testId]: (s.historyVersion[run.testId] ?? 0) + 1 },
        }
      : {}),
  }));
  const { tone, title, detail } = finishedMessage(run.name, summary);
  toast(tone, title, detail);
}

onEvent((e) => {
  if (e.type === "load") queue(e.runId, e.event);
});

// Collection runs: the settings of each runner tab, the run in progress (one at a
// time, app-wide) and the latest run of each tab with its results as they come.
//
// A fast collection sends results in bursts; they are applied once per animation
// frame so a burst renders once. A long run keeps its counts exact but only the
// first MAX_RESULTS results (then failed ones), so memory and the list stay bounded.
// Settings are kept per workspace (localStorage), so a restored tab runs what it
// ran before (not requests the user left out).
import { create } from "zustand";
import type { DataPreview } from "../bindings/DataPreview";
import type { RunEvent } from "../bindings/RunEvent";
import type { RunResult } from "../bindings/RunResult";
import type { RunStarted } from "../bindings/RunStarted";
import type { RunSummary } from "../bindings/RunSummary";
import type { TreeNode } from "../bindings/TreeNode";
import { flattenRequests, type RequestEntry } from "../components/loadtests/model";
import { onEvent } from "../lib/events";
import { pickFile, pickSavePath } from "../lib/platform";
import { api, errorMessage } from "../lib/rpc";
import { workspaceRelative } from "./grpc";
import { isRunnerTab, onItemsMoved, type RunnerTab, useTabs } from "./tabs";
import { toast } from "./toasts";
import { useSettings } from "./settings";
import { useWorkspace } from "./workspace";

export interface RunnerSettings {
  /** Request paths in the order they run; requests of the folder not listed follow in sidebar order. */
  order: string[];
  /** Paths left out of the run. */
  excluded: string[];
  /** Null: one per data row, or 1. */
  iterations: number | null;
  delayMs: number;
  /** Relative to the workspace folder, or absolute. */
  dataFile: string | null;
  stopOnFailure: boolean;
}

/** Limits the backend enforces too. */
export const MAX_ITERATIONS = 100_000;
export const MAX_DELAY_MS = 600_000;
/** Results a tab keeps: all up to this many, then failed ones only up to twice as many. */
export const MAX_RESULTS = 10_000;

export const DEFAULT_SETTINGS: RunnerSettings = { order: [], excluded: [], iterations: null, delayMs: 0, dataFile: null, stopOnFailure: false };

/** The chosen data file's preview. */
export type DataState = { status: "loading"; file: string } | { status: "ready"; file: string; preview: DataPreview } | { status: "error"; file: string; message: string };

/** A run as the tab shows it: in progress (no summary yet) or finished. */
export interface RunView {
  runId: string;
  name: string;
  environment: string | null;
  /** Requests if every iteration goes through the list once (setNextRequest may change it). */
  total: number;
  iterations: number;
  /** Unix epoch milliseconds. */
  startedAt: number;
  /** The results kept (see MAX_RESULTS). */
  results: RunResult[];
  /** Results received so far, kept or not. */
  received: number;
  /** Counts of every result received. */
  counts: RunCounts;
  summary: RunSummary | null;
  stopping: boolean;
}

export interface RunCounts {
  requests: number;
  failed: number;
  skipped: number;
  testsPassed: number;
  testsFailed: number;
}

export const NO_COUNTS: RunCounts = { requests: 0, failed: 0, skipped: 0, testsPassed: 0, testsFailed: 0 };

export type ResultFilter = "all" | "failed";

interface RunnerState {
  /** By tab id. */
  settings: Record<string, RunnerSettings>;
  data: Record<string, DataState>;
  /** The latest run of each tab. */
  runs: Record<string, RunView>;
  filter: Record<string, ResultFilter>;
  /** The tab whose run is in progress. */
  active: string | null;
  /** The tab whose start call is in flight. */
  starting: string | null;
}

export const useRunner = create<RunnerState>(() => ({ settings: {}, data: {}, runs: {}, filter: {}, active: null, starting: null }));
const set = useRunner.setState;
const get = useRunner.getState;

export const settingsOf = (tabId: string, state: RunnerState = get()): RunnerSettings => state.settings[tabId] ?? DEFAULT_SETTINGS;

// ---- pure helpers (exported for tests) -------------------------------------------------

/** The requests a run sends (HTTP, GraphQL and event streams) under a folder ("" = the whole collection) in sidebar order; null when the folder is gone. */
export function runnableRequests(tree: TreeNode[], folder: string): RequestEntry[] | null {
  let nodes = tree;
  let trail: string[] = [];
  if (folder) {
    const find = (list: TreeNode[], names: string[]): [TreeNode, string[]] | null => {
      for (const n of list) {
        if (n.kind !== "folder") continue;
        if (n.path === folder) return [n, names];
        if (folder.startsWith(`${n.path}/`)) return find(n.children, [...names, n.name]);
      }
      return null;
    };
    const found = find(tree, []);
    if (!found) return null;
    nodes = found[0].children;
    trail = [...found[1], found[0].name];
  }
  return flattenRequests(nodes, trail).filter((r) => (r.kind === "http" || r.kind === "sse") && !r.error);
}

/** Requests in run order: the saved order first, then the others in sidebar order. */
export function orderRequests(entries: RequestEntry[], order: string[]): RequestEntry[] {
  const byPath = new Map(entries.map((e) => [e.path, e]));
  const first = order.map((p) => byPath.get(p)).filter((e): e is RequestEntry => !!e);
  const listed = new Set(order);
  return [...first, ...entries.filter((e) => !listed.has(e.path))];
}

/** Paths that run, in order. */
export function selectedPaths(entries: RequestEntry[], settings: RunnerSettings): string[] {
  const excluded = new Set(settings.excluded);
  return orderRequests(entries, settings.order)
    .map((e) => e.path)
    .filter((p) => !excluded.has(p));
}

/** The order after moving the request at `from` to `to` (indexes in the ordered list). */
export function moveInOrder(ordered: string[], from: number, to: number): string[] {
  if (from === to || from < 0 || from >= ordered.length) return ordered;
  const next = ordered.slice();
  const [moved] = next.splice(from, 1);
  next.splice(Math.max(0, Math.min(to, next.length)), 0, moved);
  return next;
}

export interface IterationGroup {
  iteration: number;
  results: RunResult[];
  failed: number;
  testsPassed: number;
  testsFailed: number;
}

/** Results by iteration (in order), with counts; `filter` keeps failed results only. */
export function groupResults(results: RunResult[], filter: ResultFilter = "all"): IterationGroup[] {
  const groups: IterationGroup[] = [];
  for (const r of results) {
    let g = groups[groups.length - 1];
    if (!g || g.iteration !== r.iteration) {
      g = { iteration: r.iteration, results: [], failed: 0, testsPassed: 0, testsFailed: 0 };
      groups.push(g);
    }
    const t = testCounts(r);
    g.testsPassed += t.passed;
    g.testsFailed += t.failed;
    if (!r.passed) g.failed++;
    if (filter === "all" || !r.passed) g.results.push(r);
  }
  return filter === "all" ? groups : groups.filter((g) => g.results.length);
}

export function testCounts(r: RunResult): { passed: number; failed: number; total: number } {
  const counted = r.tests.filter((t) => !t.skipped);
  const passed = counted.filter((t) => t.passed).length;
  return { passed, failed: counted.length - passed, total: counted.length };
}

/** Counts of the run so far (the summary once it finished). */
export function liveCounts(run: RunView): RunCounts {
  if (run.summary) {
    const s = run.summary;
    return { requests: s.requests, failed: s.failed, skipped: s.skipped, testsPassed: s.testsPassed, testsFailed: s.testsFailed };
  }
  return run.counts;
}

/** `run` with more results: all counted, kept while there's room (see MAX_RESULTS). */
export function withResults(run: RunView, extra: RunResult[], limit = MAX_RESULTS): RunView {
  const counts = { ...run.counts };
  const results = run.results.slice();
  for (const r of extra) {
    if (r.skipped) counts.skipped++;
    else counts.requests++;
    if (!r.passed) counts.failed++;
    const t = testCounts(r);
    counts.testsPassed += t.passed;
    counts.testsFailed += t.failed;
    if (results.length < limit || (!r.passed && results.length < 2 * limit)) results.push(r);
  }
  return { ...run, results, counts, received: run.received + extra.length };
}

/** 0–1: results so far of the planned total (a setNextRequest loop may run past it). */
export function progress(run: RunView): number {
  if (run.summary) return 1;
  return run.total > 0 ? Math.min(1, run.received / run.total) : 0;
}

/** Toast text for a finished run. */
export function finishedMessage(name: string, s: RunSummary): { tone: "success" | "error" | "info"; title: string; detail: string } {
  const tests = s.testsPassed + s.testsFailed;
  const detail = [
    `${s.requests} ${s.requests === 1 ? "request" : "requests"}${s.failed ? `, ${s.failed} failed` : ""}`,
    tests ? `${s.testsPassed}/${tests} tests passed` : null,
    s.iterations > 1 ? `${s.perIteration.length} of ${s.iterations} iterations` : null,
  ]
    .filter(Boolean)
    .join(" · ");
  if (s.error) return { tone: "error", title: `“${name}” stopped`, detail: s.error };
  if (s.stopped) return { tone: "info", title: `“${name}” stopped`, detail };
  if (!s.passed) return { tone: "error", title: `“${name}” failed`, detail };
  return { tone: "success", title: `“${name}” passed`, detail };
}

// ---- settings ----------------------------------------------------------------------------

export function updateSettings(tabId: string, fn: (s: RunnerSettings) => RunnerSettings) {
  set((s) => ({ settings: { ...s.settings, [tabId]: fn(settingsOf(tabId, s)) } }));
  persistSettings();
}

const storageKey = (wsPath: string) => `zv:runner:${wsPath}`;

/** Settings saved for the workspace's runner tabs, by tab id. */
function storedSettings(wsPath: string): Record<string, unknown> {
  try {
    const v: unknown = JSON.parse(localStorage.getItem(storageKey(wsPath)) ?? "null");
    return v && typeof v === "object" ? (v as Record<string, unknown>) : {};
  } catch {
    return {};
  }
}

/** Saved settings as RunnerSettings (anything unexpected: the default). */
export function sanitizeSettings(v: unknown): RunnerSettings | null {
  if (!v || typeof v !== "object") return null;
  const s = v as Record<string, unknown>;
  const paths = (a: unknown) => (Array.isArray(a) ? a.filter((p): p is string => typeof p === "string") : []);
  const int = (n: unknown, min: number, max: number) => (typeof n === "number" && Number.isInteger(n) && n >= min && n <= max ? n : null);
  return {
    order: paths(s.order),
    excluded: paths(s.excluded),
    iterations: int(s.iterations, 1, MAX_ITERATIONS),
    delayMs: int(s.delayMs, 0, MAX_DELAY_MS) ?? 0,
    dataFile: typeof s.dataFile === "string" && s.dataFile ? s.dataFile : null,
    stopOnFailure: s.stopOnFailure === true,
  };
}

/** Save the settings of the open runner tabs (those of closed tabs go). */
function persistSettings() {
  const wsPath = useWorkspace.getState().info?.path;
  if (!wsPath) return;
  const open = new Set(useTabs.getState().tabs.filter(isRunnerTab).map((t) => t.id));
  const kept = Object.fromEntries(Object.entries(get().settings).filter(([id]) => open.has(id)));
  try {
    localStorage.setItem(storageKey(wsPath), JSON.stringify(kept));
  } catch {
    /* storage full or unavailable */
  }
}

/** Runner tabs whose saved settings were looked up (restored tabs get theirs once). */
const hydrated = new Set<string>();

function hydrateSettings(tabs: RunnerTab[]) {
  const wsPath = useWorkspace.getState().info?.path;
  const fresh = tabs.filter((t) => !hydrated.has(t.id));
  if (!wsPath || !fresh.length) return;
  const stored = storedSettings(wsPath);
  const found: [string, RunnerSettings][] = [];
  for (const t of fresh) {
    hydrated.add(t.id);
    const s = get().settings[t.id] ? null : sanitizeSettings(stored[t.id]);
    if (s) found.push([t.id, s]);
  }
  if (!found.length) return;
  set((st) => ({ settings: { ...st.settings, ...Object.fromEntries(found) } }));
  for (const [id, s] of found) if (s.dataFile) void setDataFile(id, s.dataFile);
}

export function setFilter(tabId: string, filter: ResultFilter) {
  set((s) => ({ filter: { ...s.filter, [tabId]: filter } }));
}

/** Choose a data file (null: none) and load its preview. */
export async function setDataFile(tabId: string, file: string | null) {
  updateSettings(tabId, (s) => ({ ...s, dataFile: file }));
  if (!file) {
    set((s) => {
      const data = { ...s.data };
      delete data[tabId];
      return { data };
    });
    return;
  }
  set((s) => ({ data: { ...s.data, [tabId]: { status: "loading", file } } }));
  let next: DataState;
  try {
    next = { status: "ready", file, preview: await api.previewRunData(file) };
  } catch (e) {
    next = { status: "error", file, message: errorMessage(e) };
  }
  // Another file may have been chosen meanwhile.
  set((s) => (s.data[tabId]?.file === file ? { data: { ...s.data, [tabId]: next } } : s));
}

/** "Choose…": a CSV or JSON file, kept relative to the workspace when inside it. */
export async function pickDataFile(tabId: string) {
  const file = await pickFile("Choose a data file (CSV or JSON)", ["csv", "json"]);
  if (file) await setDataFile(tabId, workspaceRelative(file, useWorkspace.getState().info?.path));
}

// ---- start / stop / export -----------------------------------------------------------------

/** Why this tab can't start a run, if it can't (the backend checks again). */
export function startProblem(entries: RequestEntry[] | null, settings: RunnerSettings, data: DataState | undefined): string | null {
  if (!entries) return "This folder no longer exists";
  if (!entries.length) return "No HTTP requests to run";
  if (!selectedPaths(entries, settings).length) return "Select a request to run";
  if (settings.dataFile && data?.status === "error") return "The data file can't be used";
  if (settings.dataFile && data?.status === "loading") return "Reading the data file…";
  return null;
}

export async function startRun(tab: RunnerTab, entries: RequestEntry[]): Promise<boolean> {
  const { active, starting, runs } = get();
  if (starting) return false;
  if (active) {
    const other = runs[active]?.name ?? "Another run";
    toast("info", "A run is in progress", `Stop “${other}” first: one collection run at a time.`);
    return false;
  }
  const settings = settingsOf(tab.id);
  const wsPath = useWorkspace.getState().info?.path;
  set({ starting: tab.id });
  try {
    const started = await api.startRun({
      folder: tab.folder,
      requests: selectedPaths(entries, settings),
      iterations: settings.iterations ?? undefined,
      delayMs: settings.delayMs,
      dataFile: settings.dataFile ?? undefined,
      stopOnFailure: settings.stopOnFailure,
    });
    // The tab closed or the workspace changed meanwhile: nobody shows this run.
    if (useWorkspace.getState().info?.path !== wsPath || !useTabs.getState().tabs.some((t) => t.id === tab.id)) {
      early.delete(started.runId);
      void api.stopRun(started.runId).catch(() => {});
      return false;
    }
    attachRun(tab.id, started);
    return true;
  } catch (e) {
    toast("error", `Could not run “${tab.name}”`, errorMessage(e));
    return false;
  } finally {
    set({ starting: null });
  }
}

/** Show a started run in a runner tab (also one an AI agent started). */
export function attachRun(tabId: string, started: RunStarted) {
  const run: RunView = {
    runId: started.runId,
    name: started.name,
    environment: started.environment,
    total: started.total,
    iterations: started.iterations,
    startedAt: Date.now(),
    results: [],
    received: 0,
    counts: NO_COUNTS,
    summary: null,
    stopping: false,
  };
  set((s) => ({ runs: { ...s.runs, [tabId]: run }, active: tabId }));
  claimEarly(started.runId);
}

export async function stopRun(tabId: string) {
  const run = get().runs[tabId];
  if (!run || run.summary || run.stopping) return;
  patchRun(tabId, run.runId, { stopping: true });
  try {
    await api.stopRun(run.runId);
  } catch (e) {
    toast("error", "Could not stop the run", errorMessage(e));
    patchRun(tabId, run.runId, { stopping: false });
  }
}

/** Start or stop (Mod+Enter, the Run/Stop button). */
export async function toggleRun(tab: RunnerTab) {
  if (get().active === tab.id) return stopRun(tab.id);
  const entries = runnableRequests(useWorkspace.getState().info?.tree ?? [], tab.folder);
  const settings = settingsOf(tab.id);
  const problem = startProblem(entries, settings, get().data[tab.id]);
  if (problem || !entries) {
    toast("info", `Can't run “${tab.name}”`, problem ?? "");
    return false;
  }
  return startRun(tab, entries);
}

/** Save the tab's finished run as a JSON report or JUnit XML. */
export async function exportRun(tabId: string, format: "json" | "junit") {
  const run = get().runs[tabId];
  if (!run?.summary) return;
  const d = new Date(run.summary.startedAt);
  const pad = (n: number) => String(n).padStart(2, "0");
  const stamp = `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}-${pad(d.getMinutes())}`;
  const safe = run.name.replace(/[\\/:*?"<>|]+/g, "-").trim() || "run";
  const ext = format === "junit" ? "xml" : "json";
  const path = await pickSavePath(format === "junit" ? "Save JUnit XML" : "Save the results as JSON", `${safe} ${stamp}.${ext}`);
  if (!path) return;
  try {
    await api.exportRun(run.runId, path, format);
    toast("success", format === "junit" ? "JUnit report saved" : "Results saved", path);
  } catch (e) {
    toast("error", "Could not export", errorMessage(e));
  }
}

function patchRun(tabId: string, runId: string, patch: Partial<RunView>) {
  set((s) => {
    const run = s.runs[tabId];
    return run?.runId === runId ? { runs: { ...s.runs, [tabId]: { ...run, ...patch } } } : s;
  });
}

// ---- events -------------------------------------------------------------------------------

/** Results not yet applied, by run id. */
const incoming = new Map<string, RunResult[]>();
/** Events of runs not known yet (their start call is still in flight), by run id. */
const early = new Map<string, RunEvent[]>();
/** Runs AI agents started: shown in a tab when following the agent, otherwise left alone. */
const agentRuns = new Set<string>();
let flushScheduled = false;

const followingAgents = () => useSettings.getState().settings?.agents.follow ?? true;

const tabOfRun = (runId: string): string | null => Object.entries(get().runs).find(([, r]) => r.runId === runId)?.[0] ?? null;

function handle(runId: string, event: RunEvent, agent = false) {
  const tabId = tabOfRun(runId);
  if (!tabId) {
    // Every event of an agent's run says so (also after a window reload).
    if (agent && !agentRuns.has(runId)) {
      agentRuns.add(runId);
      while (agentRuns.size > 20) agentRuns.delete(agentRuns.values().next().value!);
    }
    if (agentRuns.has(runId)) {
      // Its tab opens as soon as the agent's "run" event arrives (when following).
      if (event.type === "finished") agentRuns.delete(runId);
      if (followingAgents()) buffer(runId, event);
      return;
    }
    // A start in flight claims these when it returns.
    if (get().starting) {
      buffer(runId, event);
    } else if (event.type !== "finished") {
      // A run no tab shows (the window reloaded, or its stop got lost) would block
      // every other run until it ends: stop it.
      void api.stopRun(runId).catch(() => {});
    }
    return;
  }
  if (event.type === "started") {
    patchRun(tabId, runId, { total: event.total, iterations: event.iterations });
  } else if (event.type === "result") {
    const list = incoming.get(runId) ?? [];
    list.push(event.result);
    incoming.set(runId, list);
    scheduleFlush();
  } else {
    flushRunnerEvents();
    finish(tabId, runId, event.summary);
  }
}

function buffer(runId: string, event: RunEvent) {
  const list = early.get(runId) ?? [];
  list.push(event);
  early.set(runId, list);
  while (early.size > 4) early.delete(early.keys().next().value!);
}

function claimEarly(runId: string) {
  const events = early.get(runId);
  early.delete(runId);
  for (const e of events ?? []) handle(runId, e);
}

function scheduleFlush() {
  if (flushScheduled) return;
  flushScheduled = true;
  const run = () => {
    if (flushScheduled) flushRunnerEvents();
  };
  if (typeof requestAnimationFrame === "function") requestAnimationFrame(run);
  // Hidden windows get no animation frames.
  setTimeout(run, 250);
}

/** Apply queued results (exported for tests). */
export function flushRunnerEvents() {
  flushScheduled = false;
  if (!incoming.size) return;
  const batches = new Map(incoming);
  incoming.clear();
  set((s) => {
    const runs = { ...s.runs };
    for (const [tabId, run] of Object.entries(s.runs)) {
      const extra = batches.get(run.runId);
      if (extra) runs[tabId] = withResults(run, extra);
    }
    return { runs };
  });
}

function finish(tabId: string, runId: string, summary: RunSummary) {
  const run = get().runs[tabId];
  if (!run || run.runId !== runId) return;
  set((s) => ({
    runs: { ...s.runs, [tabId]: { ...run, summary, stopping: false } },
    active: s.active === tabId ? null : s.active,
  }));
  const { tone, title, detail } = finishedMessage(run.name, summary);
  toast(tone, title, detail);
}

onEvent((e) => {
  if (e.type === "runner") handle(e.runId, e.event, e.agent ?? false);
});

// ---- lifecycle ------------------------------------------------------------------------------

/** Forget everything (workspace switch); a run in progress is stopped. */
export function resetRunner() {
  const { active, runs } = get();
  const run = active ? runs[active] : null;
  if (run && !run.summary) void api.stopRun(run.runId).catch(() => {});
  incoming.clear();
  early.clear();
  hydrated.clear();
  set({ settings: {}, data: {}, runs: {}, filter: {}, active: null, starting: null });
}

// A closed runner tab takes its state along (and stops its run); a restored one gets its settings back.
useTabs.subscribe((s, prev) => {
  if (s.tabs === prev.tabs) return;
  const runnerTabs = s.tabs.filter(isRunnerTab);
  const open = new Set(runnerTabs.map((t) => t.id));
  // A tab that comes back (tabs dropped and restored) looks its settings up again.
  for (const id of hydrated) if (!open.has(id)) hydrated.delete(id);
  hydrateSettings(runnerTabs);
  const state = get();
  const gone = Object.keys({ ...state.settings, ...state.runs, ...state.data }).filter((id) => !open.has(id));
  if (!gone.length) return;
  const drop = <T,>(rec: Record<string, T>) => Object.fromEntries(Object.entries(rec).filter(([id]) => open.has(id)));
  for (const id of gone) {
    const run = state.runs[id];
    if (run && !run.summary) void api.stopRun(run.runId).catch(() => {});
  }
  set((st) => ({
    settings: drop(st.settings),
    data: drop(st.data),
    runs: drop(st.runs),
    filter: drop(st.filter),
    active: st.active && open.has(st.active) ? st.active : null,
  }));
});

useWorkspace.subscribe((s, prev) => {
  if (s.info?.path !== prev.info?.path) resetRunner();
});

// Renamed or moved requests keep their place and their checkbox.
onItemsMoved((oldPath, newPath) => {
  const moved = (p: string) => (p === oldPath ? newPath : p.startsWith(`${oldPath}/`) ? newPath + p.slice(oldPath.length) : p);
  const changed = (list: string[]) => list.some((p) => moved(p) !== p);
  const settings = get().settings;
  if (!Object.values(settings).some((s) => changed(s.order) || changed(s.excluded))) return;
  set({ settings: Object.fromEntries(Object.entries(settings).map(([id, s]) => [id, { ...s, order: s.order.map(moved), excluded: s.excluded.map(moved) }])) });
  persistSettings();
});

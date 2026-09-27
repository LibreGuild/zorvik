// Network tools' state and runs. Each tool keeps its state in its tab
// (updateToolState), so switching tabs keeps inputs and results. Streaming
// runs (port check, ping) are routed by run id from "tool" events to their
// tab and applied in small batches; closing a tab cancels its run.
import type { NetInterface } from "../bindings/NetInterface";
import type { PingMode } from "../bindings/PingMode";
import type { PingReply } from "../bindings/PingReply";
import type { PingStarted } from "../bindings/PingStarted";
import type { PingSummary } from "../bindings/PingSummary";
import type { PortCheckStarted } from "../bindings/PortCheckStarted";
import type { PortResult } from "../bindings/PortResult";
import type { PortScanSummary } from "../bindings/PortScanSummary";
import type { TlsReport } from "../bindings/TlsReport";
import type { ToolEvent } from "../bindings/ToolEvent";
import { onEvent } from "../lib/events";
import { newId } from "../lib/ids";
import { api, errorMessage, RpcError } from "../lib/rpc";
import { confirm } from "./dialogs";
import { isToolTab, type ToolTab, updateToolState, useTabs } from "./tabs";

export type RunStatus = "idle" | "running" | "done" | "error";

// ---- state shapes -------------------------------------------------------------

export interface TlsState {
  host: string;
  sni: string;
  status: RunStatus;
  runId: string | null;
  report: TlsReport | null;
  error: string | null;
}

export type PortPreset = "web" | "top20" | "custom";

export interface PortCheckState {
  host: string;
  preset: PortPreset;
  custom: string;
  timeoutMs: number;
  status: RunStatus;
  runId: string | null;
  address: string | null;
  total: number;
  /** In arrival order. */
  results: PortResult[];
  summary: PortScanSummary | null;
  error: string | null;
}

export interface PingStats {
  sent: number;
  received: number;
  sum: number;
  min: number | null;
  max: number | null;
}

export interface PingState {
  host: string;
  /** 0 = until stopped. */
  count: number;
  mode: PingMode;
  port: number;
  intervalMs: number;
  status: RunStatus;
  runId: string | null;
  started: PingStarted | null;
  /** The latest replies (bounded). */
  replies: PingReply[];
  /** Totals over the whole run (replies may have been dropped). */
  stats: PingStats;
  summary: PingSummary | null;
  error: string | null;
}

export interface InterfacesState {
  status: "idle" | "loading" | "done" | "error";
  list: NetInterface[];
  error: string | null;
}

export const TLS_DEFAULTS: TlsState = { host: "", sni: "", status: "idle", runId: null, report: null, error: null };

export const PORT_DEFAULTS: PortCheckState = {
  host: "",
  preset: "web",
  custom: "",
  timeoutMs: 2000,
  status: "idle",
  runId: null,
  address: null,
  total: 0,
  results: [],
  summary: null,
  error: null,
};

const EMPTY_STATS: PingStats = { sent: 0, received: 0, sum: 0, min: null, max: null };

export const PING_DEFAULTS: PingState = {
  host: "",
  count: 10,
  mode: "auto",
  port: 443,
  intervalMs: 1000,
  status: "idle",
  runId: null,
  started: null,
  replies: [],
  stats: EMPTY_STATS,
  summary: null,
  error: null,
};

export const INTERFACES_DEFAULTS: InterfacesState = { status: "idle", list: [], error: null };

/** A tool tab's state with defaults for anything missing. */
export function toolState<T extends object>(tab: ToolTab, defaults: T): T {
  return { ...defaults, ...(tab.state as Partial<T>) };
}

function patch<T extends object>(tabId: string, defaults: T, fn: (s: T) => Partial<T>) {
  updateToolState(tabId, (raw) => {
    const s = { ...defaults, ...(raw as Partial<T>) };
    return { ...s, ...fn(s) } as Record<string, unknown>;
  });
}

// Typed per tool so literal values (status: "running") keep their types.
const patchTls = (id: string, fn: (s: TlsState) => Partial<TlsState>) => patch(id, TLS_DEFAULTS, fn);
const patchPorts = (id: string, fn: (s: PortCheckState) => Partial<PortCheckState>) => patch(id, PORT_DEFAULTS, fn);
const patchPing = (id: string, fn: (s: PingState) => Partial<PingState>) => patch(id, PING_DEFAULTS, fn);
const patchInterfaces = (id: string, fn: (s: InterfacesState) => Partial<InterfacesState>) => patch(id, INTERFACES_DEFAULTS, fn);

function stateOf<T extends object>(tabId: string, defaults: T): T | null {
  const tab = useTabs.getState().tabs.find((t) => t.id === tabId);
  return isToolTab(tab) ? toolState(tab, defaults) : null;
}

// ---- ports --------------------------------------------------------------------

export const PORT_PRESETS: { id: PortPreset; label: string; ports: string }[] = [
  { id: "web", label: "Common web", ports: "80,443,8080,8443" },
  // nmap's 20 most common TCP ports.
  { id: "top20", label: "Top 20", ports: "21,22,23,25,53,80,110,111,135,139,143,443,445,993,995,1723,3306,3389,5900,8080" },
  { id: "custom", label: "Custom", ports: "" },
];

export const MAX_PORTS = 1024;

export function portSpec(s: Pick<PortCheckState, "preset" | "custom">): string {
  return s.preset === "custom" ? s.custom : (PORT_PRESETS.find((p) => p.id === s.preset)?.ports ?? "");
}

/** Number of distinct ports in `80,443,8000-8010`, or an error message (the backend validates too). */
export function countPorts(spec: string): number | string {
  const ports = new Set<number>();
  const tokens = spec.split(/[\s,;]+/).filter(Boolean);
  if (tokens.length === 0) return "Enter at least one port, e.g. 80,443 or 8000-8010";
  for (const token of tokens) {
    const m = token.match(/^(\d{1,5})(?:-(\d{1,5}))?$/);
    if (!m) return `'${token}' is not a port or range`;
    const a = Number(m[1]);
    const b = m[2] === undefined ? a : Number(m[2]);
    if (a < 1 || b < 1 || a > 65535 || b > 65535) return `Port '${token}' is out of range (1–65535)`;
    if (a > b) return `Invalid range '${token}': the start is after the end`;
    if (b - a >= MAX_PORTS * 64) return `That is more than ${MAX_PORTS} ports`;
    for (let p = a; p <= b; p++) ports.add(p);
    if (ports.size > MAX_PORTS) return `That is more than ${MAX_PORTS} ports; check at most ${MAX_PORTS} at a time`;
  }
  return ports.size;
}

/**
 * Loopback, private (RFC 1918 / ULA), link-local and CGNAT addresses and
 * local-only names. Other hosts may belong to someone else.
 */
export function isLocalHost(host: string): boolean {
  const h = host.trim().toLowerCase().replace(/^\[|\]$/g, "").replace(/%.*$/, "");
  if (!h) return false;
  // Single-label names ("router", "intranet") only resolve on the local network.
  if (h === "localhost" || /\.(localhost|local|lan|home\.arpa|internal|test)$/.test(h) || (!h.includes(".") && !h.includes(":"))) return true;
  const v4 = h.match(/^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$/);
  if (v4) {
    const [a, b] = [Number(v4[1]), Number(v4[2])];
    return a === 10 || a === 127 || (a === 172 && b >= 16 && b <= 31) || (a === 192 && b === 168) || (a === 169 && b === 254) || (a === 100 && b >= 64 && b <= 127);
  }
  if (h.includes(":")) return h === "::1" || /^f[cd][0-9a-f]{2}:/.test(h) || /^fe[89ab][0-9a-f]:/.test(h);
  return false;
}

/** Apply port check events of `runId` (events of an older run are ignored). */
export function applyPortEvents(s: PortCheckState, runId: string, events: ToolEvent[]): PortCheckState {
  if (s.runId !== runId) return s;
  let results = s.results;
  let next = s;
  for (const e of events) {
    if (e.type === "portResult") {
      if (results === s.results) results = [...results];
      results.push(e);
    } else if (e.type === "portsDone") {
      next = { ...next, status: "done", summary: e, address: e.address, total: e.total };
    }
  }
  return results === s.results && next === s ? s : { ...next, results };
}

export async function startPortCheck(tabId: string) {
  const s = stateOf(tabId, PORT_DEFAULTS);
  if (!s || s.status === "running") return;
  const host = s.host.trim();
  const spec = portSpec(s);
  const count = countPorts(spec);
  if (!host || typeof count === "string") {
    patchPorts(tabId, () => ({ status: "error", error: !host ? "Enter a host name or IP address" : (count as string) }));
    return;
  }
  if (count > 100 && !isLocalHost(host)) {
    const ok = await confirm({
      title: `Check ${count} ports on ${host}?`,
      message: "Only scan hosts you're allowed to test. Scanning other people's servers can be seen as an attack.",
      confirmLabel: "Check ports",
    });
    if (!ok) return;
  }
  const runId = newId();
  track(runId, tabId);
  patchPorts(tabId, () => ({ status: "running", runId, address: null, total: count, results: [], summary: null, error: null }));
  try {
    const started: PortCheckStarted = await api.portCheck(runId, host, spec, s.timeoutMs);
    patchPorts(tabId, (cur) => (cur.runId === runId ? { address: started.address, total: started.total } : {}));
  } catch (e) {
    untrack(runId);
    patchPorts(tabId, (cur) => (cur.runId === runId ? failed(e) : {}));
  }
}

// ---- ping ---------------------------------------------------------------------

/** Replies kept per tab (stats cover the whole run). */
export const MAX_REPLIES = 500;

export function addReply(stats: PingStats, r: PingReply): PingStats {
  if (r.ms == null) return { ...stats, sent: stats.sent + 1 };
  return {
    sent: stats.sent + 1,
    received: stats.received + 1,
    sum: stats.sum + r.ms,
    min: stats.min == null ? r.ms : Math.min(stats.min, r.ms),
    max: stats.max == null ? r.ms : Math.max(stats.max, r.ms),
  };
}

export function applyPingEvents(s: PingState, runId: string, events: ToolEvent[]): PingState {
  if (s.runId !== runId) return s;
  let next = s;
  let replies: PingReply[] | null = null;
  for (const e of events) {
    if (e.type === "pingStarted") {
      next = { ...next, started: e };
    } else if (e.type === "pingReply") {
      replies ??= [...next.replies];
      replies.push(e);
      next = { ...next, stats: addReply(next.stats, e) };
    } else if (e.type === "pingDone") {
      next = { ...next, status: "done", summary: e };
    }
  }
  if (replies) next = { ...next, replies: replies.length > MAX_REPLIES ? replies.slice(-MAX_REPLIES) : replies };
  return next;
}

export async function startPing(tabId: string) {
  const s = stateOf(tabId, PING_DEFAULTS);
  if (!s || s.status === "running") return;
  const host = s.host.trim();
  if (!host) {
    patchPing(tabId, () => ({ status: "error", error: "Enter a host name or IP address" }));
    return;
  }
  const runId = newId();
  track(runId, tabId);
  patchPing(tabId, () => ({ status: "running", runId, started: null, replies: [], stats: EMPTY_STATS, summary: null, error: null }));
  try {
    const started = await api.ping(runId, host, s.count, s.intervalMs, s.mode, s.port);
    patchPing(tabId, (cur) => (cur.runId === runId && !cur.started ? { started } : {}));
  } catch (e) {
    untrack(runId);
    patchPing(tabId, (cur) => (cur.runId === runId ? failed(e) : {}));
  }
}

// ---- TLS inspector ------------------------------------------------------------

export async function inspectTls(tabId: string) {
  const s = stateOf(tabId, TLS_DEFAULTS);
  if (!s || s.status === "running") return;
  const host = s.host.trim();
  if (!host) {
    patchTls(tabId, () => ({ status: "error", error: "Enter a host name, host:port or URL" }));
    return;
  }
  const runId = newId();
  track(runId, tabId);
  patchTls(tabId, () => ({ status: "running", runId, error: null }));
  try {
    const report = await api.tlsInspect(host, s.sni.trim() || null, runId);
    patchTls(tabId, (cur) => (cur.runId === runId ? { status: "done", report, error: null } : {}));
  } catch (e) {
    patchTls(tabId, (cur) => (cur.runId === runId ? failed(e) : {}));
  } finally {
    untrack(runId);
  }
}

// ---- interfaces ---------------------------------------------------------------

/** The address most likely to work from another device: an up, non-loopback, routable IPv4. */
export function bestAddress(list: NetInterface[]): string | null {
  for (const i of list) {
    if (i.loopback || !i.up) continue;
    const a = i.addresses.find((x) => x.family === "ipv4" && !x.linkLocal);
    if (a) return a.ip;
  }
  return null;
}

export async function loadInterfaces(tabId: string) {
  patchInterfaces(tabId, () => ({ status: "loading", error: null }));
  try {
    const list = await api.netInterfaces();
    patchInterfaces(tabId, () => ({ status: "done", list, error: null }));
  } catch (e) {
    patchInterfaces(tabId, () => ({ status: "error", error: errorMessage(e) }));
  }
}

// ---- runs ---------------------------------------------------------------------

/** Run id → tab id, for routing events and cancelling. */
const runs = new Map<string, string>();

function track(runId: string, tabId: string) {
  runs.set(runId, tabId);
}

function untrack(runId: string) {
  runs.delete(runId);
}

/** A cancelled call is not an error to show; other failures are. */
function failed(e: unknown): { status: RunStatus; error: string | null } {
  if (e instanceof RpcError && e.networkKind === "cancelled") return { status: "idle", error: null };
  return { status: "error", error: errorMessage(e) };
}

/** Stop the tab's current run; the run reports its own end (port check / ping). */
export async function stopRun(tabId: string) {
  for (const [runId, tab] of runs) {
    if (tab === tabId) await api.cancelTool(runId).catch(() => {});
  }
}

let queue: { runId: string; event: ToolEvent }[] = [];
let flushTimer: ReturnType<typeof setTimeout> | null = null;

function flush() {
  flushTimer = null;
  const batch = queue;
  queue = [];
  const byRun = new Map<string, ToolEvent[]>();
  for (const { runId, event } of batch) {
    const list = byRun.get(runId);
    if (list) list.push(event);
    else byRun.set(runId, [event]);
  }
  for (const [runId, events] of byRun) {
    const tabId = runs.get(runId);
    if (!tabId) continue;
    const tab = useTabs.getState().tabs.find((t) => t.id === tabId);
    if (!isToolTab(tab)) continue;
    if (tab.tool === "ports") patchPorts(tabId, (s) => applyPortEvents(s, runId, events));
    else if (tab.tool === "ping") patchPing(tabId, (s) => applyPingEvents(s, runId, events));
    if (events.some((e) => e.type === "portsDone" || e.type === "pingDone")) untrack(runId);
  }
}

onEvent((e) => {
  if (e.type !== "tool" || !runs.has(e.runId)) return;
  queue.push({ runId: e.runId, event: e.event });
  // Floods (a fast port check) are applied at most every 50 ms.
  flushTimer ??= setTimeout(flush, 50);
});

// A closed tab's run is stopped.
useTabs.subscribe((state, prev) => {
  if (state.tabs === prev.tabs || runs.size === 0) return;
  const open = new Set(state.tabs.map((t) => t.id));
  for (const [runId, tabId] of runs) {
    if (!open.has(tabId)) {
      runs.delete(runId);
      void api.cancelTool(runId).catch(() => {});
    }
  }
});

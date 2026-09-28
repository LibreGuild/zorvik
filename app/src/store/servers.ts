// Servers: the saved list of the open workspace, what is running (in any
// workspace), and the traffic of running servers (kept per saved server so a
// stopped server still shows its last run until it starts again).
import { create } from "zustand";
import type { RunningServerInfo } from "../bindings/RunningServerInfo";
import type { Server } from "../bindings/Server";
import type { ServerKind } from "../bindings/ServerKind";
import type { ServerNode } from "../bindings/ServerNode";
import type { TrafficEntry } from "../bindings/TrafficEntry";
import { onEvent } from "../lib/events";
import { quitApp } from "../lib/platform";
import { api, errorMessage } from "../lib/rpc";
import { confirm, prompt } from "./dialogs";
import { onServerDeleted, onServerFilesChanged, onServerRenamed, onServerSaved, openServer, type ServerTab } from "./tabs";
import { toast } from "./toasts";
import { useWorkspace } from "./workspace";

/** Traffic entries kept per server in the UI (the backend keeps its own bounded log). */
const MAX_ENTRIES = 5000;

interface ServersState {
  saved: ServerNode[];
  running: RunningServerInfo[];
  /** Traffic by server key (see `serverKey`). */
  logs: Record<string, TrafficEntry[]>;
  /** Servers being started or stopped (by server key). */
  busy: Record<string, boolean>;
}

export const useServers = create<ServersState>(() => ({ saved: [], running: [], logs: {}, busy: {} }));
const set = useServers.setState;
const get = useServers.getState;

/** Identifies a saved server across workspaces. */
export const serverKey = (workspacePath: string, serverId: string) => `${workspacePath}\u0000${serverId}`;

const currentWorkspace = () => useWorkspace.getState().info?.path ?? "";

/** The running instance of a saved server in the open workspace, if any. */
export function runningFor(serverId: string, running: RunningServerInfo[] = get().running): RunningServerInfo | undefined {
  const ws = currentWorkspace();
  return running.find((r) => r.workspacePath === ws && r.serverId === serverId);
}

export async function refreshServers() {
  if (!useWorkspace.getState().info) {
    set({ saved: [] });
    return;
  }
  try {
    set({ saved: await api.servers() });
  } catch {
    set({ saved: [] });
  }
}

export async function refreshRunning() {
  const running = await api.runningServers().catch(() => []);
  set({ running });
  // Servers that were already running (the window reloaded) or started elsewhere (with the
  // workspace): their log starts over, with what they logged so far.
  for (const info of running) {
    const key = serverKey(info.workspacePath, info.serverId);
    claimLog(key, info.runId);
    await loadBacklog(key, info.runId);
  }
}

onServerFilesChanged(() => void refreshServers());
onServerSaved((oldId, newId) => {
  if (oldId !== newId) moveLog(oldId, newId);
  void refreshServers();
  void refreshRunning(); // the backend renames a running copy too
});

// ---- create / rename / delete -----------------------------------------------------

export const SERVER_KIND_NAMES: Record<ServerKind, string> = {
  http: "Mock API",
  websocket: "WebSocket server",
  sse: "Event stream server",
  tcp: "TCP server",
  udp: "UDP server",
  dns: "DNS server",
  tcpProxy: "TCP relay",
  socketio: "Socket.IO server",
};

const DEFAULT_PORTS: Record<ServerKind, number> = { http: 3000, websocket: 3001, sse: 3002, tcp: 9000, udp: 9001, dns: 1053, tcpProxy: 9100, socketio: 3003 };

export function newServerDraft(kind: ServerKind, name = SERVER_KIND_NAMES[kind]): Server {
  // A port not used by another saved server, so a new one starts right away.
  const used = new Set(get().saved.map((s) => s.port));
  let port = DEFAULT_PORTS[kind];
  while (used.has(port)) port++;
  const server: Server = { name, kind, seq: 0, host: "127.0.0.1", port };
  if (kind === "http") server.http = { routes: [{ method: "GET", path: "/hello", status: 200, headers: [{ key: "Content-Type", value: "application/json" }], body: '{\n  "message": "Hello from Zorvik"\n}' }] };
  if (kind === "sse") server.sse = { events: [{ event: "tick", data: '{"n": 1}' }], intervalMs: 1000, repeat: true };
  if (kind === "dns") server.dns = { records: [{ name: "api.example.test", type: "A", value: "127.0.0.1", ttl: 60 }], upstream: "system" };
  if (kind === "tcpProxy") server.proxy = { target: "example.com:80" };
  if (kind === "socketio") server.socketio = { greetingEvent: "welcome", greetingArgs: '{"id": "{{$uuid}}"}' };
  return server;
}

export async function createServer(kind: ServerKind, template?: Server): Promise<string | null> {
  const name = await prompt({ title: `New ${SERVER_KIND_NAMES[kind]}`, value: template?.name ?? SERVER_KIND_NAMES[kind], confirmLabel: "Create" });
  if (!name) return null;
  try {
    const id = await api.createServer({ ...(template ?? newServerDraft(kind)), name });
    await refreshServers();
    await openServer(id);
    return id;
  } catch (e) {
    toast("error", "Could not create server", errorMessage(e));
    return null;
  }
}

export async function renameServer(node: ServerNode) {
  const name = await prompt({ title: "Rename server", value: node.name, confirmLabel: "Rename" });
  if (!name || name === node.name) return;
  try {
    const server = await api.readServer(node.id);
    const newId = await api.saveServer(node.id, { ...server, name });
    onServerRenamed(node.id, newId, { ...server, name });
    moveLog(node.id, newId);
    await Promise.all([refreshServers(), refreshRunning()]);
  } catch (e) {
    toast("error", "Could not rename server", errorMessage(e));
  }
}

export async function duplicateServer(node: ServerNode) {
  try {
    const id = await api.duplicateServer(node.id);
    await refreshServers();
    await openServer(id);
  } catch (e) {
    toast("error", "Could not duplicate server", errorMessage(e));
  }
}

export async function deleteServer(node: ServerNode) {
  const running = runningFor(node.id);
  const ok = await confirm({
    title: "Delete server?",
    message: `"${node.name}" will be moved to the trash${running ? " and stopped" : ""}.`,
    confirmLabel: "Delete",
    danger: true,
  });
  if (!ok) return;
  try {
    await api.deleteServer(node.id);
    // The backend stopped its running copy; its traffic goes too (a new server may get the same id).
    const ws = currentWorkspace();
    set((s) => ({ running: s.running.filter((r) => !(r.workspacePath === ws && r.serverId === node.id)) }));
    dropLog(serverKey(ws, node.id));
    onServerDeleted(node.id);
    await Promise.all([refreshServers(), refreshRunning()]);
  } catch (e) {
    toast("error", "Could not delete server", errorMessage(e));
  }
}

export async function reorderServers(ids: string[]) {
  set((s) => ({ saved: ids.map((id) => s.saved.find((n) => n.id === id)!).filter(Boolean) }));
  await api.reorderServers(ids).catch((e) => toast("error", "Could not reorder", errorMessage(e)));
  await refreshServers();
}

/** A rename: the log (and the running copy, which the backend renames too) follow the new id. */
function moveLog(oldId: string, newId: string) {
  const ws = currentWorkspace();
  const from = serverKey(ws, oldId);
  const to = serverKey(ws, newId);
  if (from === to) return;
  const run = logRuns.get(from);
  logRuns.delete(from);
  if (run) logRuns.set(to, run);
  if (pendingTraffic[from]) {
    pendingTraffic[to] = [...(pendingTraffic[to] ?? []), ...pendingTraffic[from]];
    delete pendingTraffic[from];
  }
  set((s) => {
    // Updated here rather than by the next refresh, so traffic arriving meanwhile goes to the new key.
    const running = s.running.map((r) => (r.workspacePath === ws && r.serverId === oldId ? { ...r, serverId: newId } : r));
    const logs = { ...s.logs };
    if (from in logs) {
      logs[to] = logs[from];
      delete logs[from];
    }
    return { running, logs };
  });
}

function dropLog(key: string) {
  logRuns.delete(key);
  delete pendingTraffic[key];
  set((s) => {
    if (!(key in s.logs)) return s;
    const logs = { ...s.logs };
    delete logs[key];
    return { logs };
  });
}

// ---- start / stop / live updates --------------------------------------------------------

function setBusy(key: string, value: boolean) {
  set((s) => ({ busy: { ...s.busy, [key]: value } }));
}

/** Start a saved server with the given configuration (a tab's draft, possibly unsaved). */
export async function startServer(serverId: string, server: Server): Promise<boolean> {
  const key = serverKey(currentWorkspace(), serverId);
  if (get().busy[key] || runningFor(serverId)) return false;
  setBusy(key, true);
  try {
    const info = await api.startServer(serverId, server);
    // Only now: a start that fails (e.g. the port is taken) keeps the last run's traffic.
    claimLog(key, info.runId);
    set((s) => ({ running: [...s.running.filter((r) => r.runId !== info.runId), info] }));
    // Traffic that arrived before the list knew the run id.
    await loadBacklog(key, info.runId);
    return true;
  } catch (e) {
    toast("error", `Could not start "${server.name}"`, errorMessage(e));
    return false;
  } finally {
    setBusy(key, false);
  }
}

export async function stopServer(runId: string) {
  const info = get().running.find((r) => r.runId === runId);
  const key = info ? serverKey(info.workspacePath, info.serverId) : runId;
  // Already starting or stopping (a double click): a second stop would fail with "not running".
  if (get().busy[key]) return;
  setBusy(key, true);
  try {
    await api.stopServer(runId);
    set((s) => ({ running: s.running.filter((r) => r.runId !== runId) }));
  } catch (e) {
    toast("error", "Could not stop server", errorMessage(e));
    await refreshRunning();
  } finally {
    setBusy(key, false);
  }
}

export async function stopAllServers() {
  const running = get().running;
  // One server is like its own Stop button; several may include other workspaces' servers.
  if (running.length > 1) {
    const ws = currentWorkspace();
    const elsewhere = running.filter((r) => r.workspacePath !== ws).length;
    const ok = await confirm({
      title: `Stop ${running.length} servers?`,
      message: `Their clients are disconnected${elsewhere ? `, including ${elsewhere === 1 ? "a server" : `${elsewhere} servers`} of another workspace` : ""}.`,
      confirmLabel: "Stop all",
      danger: true,
    });
    if (!ok) return;
  }
  await api.stopAllServers().catch((e) => toast("error", "Could not stop servers", errorMessage(e)));
  set({ running: [] });
}

/** Restart with the current configuration (needed after changing address, port or TLS). */
export async function restartServer(serverId: string, server: Server) {
  const run = runningFor(serverId);
  if (run) await stopServer(run.runId);
  return startServer(serverId, server);
}

/** Apply edits to a running server without a restart when possible; returns false when a restart is needed. */
export async function applyToRunning(serverId: string, server: Server): Promise<boolean> {
  const run = runningFor(serverId);
  if (!run) return true;
  try {
    const { applied } = await api.updateServer(run.runId, server);
    if (applied) set((s) => ({ running: s.running.map((r) => (r.runId === run.runId ? { ...r, name: server.name } : r)) }));
    return applied;
  } catch {
    return true;
  }
}

/** Start or restart a server tab (Mod+Enter / the Start button). */
export async function runServerTab(tab: ServerTab) {
  const run = runningFor(tab.serverId);
  if (run) await restartServer(tab.serverId, tab.draft);
  else await startServer(tab.serverId, tab.draft);
}

export async function clearServerLog(serverId: string) {
  const key = serverKey(currentWorkspace(), serverId);
  const run = runningFor(serverId);
  if (run) await api.clearServerLog(run.runId).catch(() => {});
  delete pendingTraffic[key];
  set((s) => ({ logs: { ...s.logs, [key]: [] } }));
}

/** Servers set to start with the workspace. */
export async function autoStartServers() {
  try {
    const { started, errors } = await api.autoStartServers();
    if (started.length) await refreshRunning();
    for (const error of errors) toast("error", "A server could not start", error);
  } catch {
    /* no workspace */
  }
}

function mergeLog(current: TrafficEntry[], extra: TrafficEntry[]): TrafficEntry[] {
  const seen = new Set(current.map((e) => e.id));
  const merged = [...extra.filter((e) => !seen.has(e.id)), ...current].sort((a, b) => a.id - b.id);
  return merged.length > MAX_ENTRIES ? merged.slice(merged.length - MAX_ENTRIES) : merged;
}

/**
 * Per log (server key): the run it shows, and the ids its backlog (`server.log`) brought in.
 * Entry ids start at 1 for every run, so a log never mixes runs; and entries in the backlog
 * may also arrive as events (null: the backlog was not loaded yet).
 */
const logRuns = new Map<string, { runId: string; backlog: Set<number> | null }>();

/** Show `runId`'s traffic in the log of `key`, starting it over when it showed another run. */
function claimLog(key: string, runId: string) {
  if (logRuns.get(key)?.runId === runId) return;
  logRuns.set(key, { runId, backlog: null });
  delete pendingTraffic[key];
  set((s) => ({ logs: { ...s.logs, [key]: [] } }));
}

/** What the run logged before the UI listened to it (once per run). */
async function loadBacklog(key: string, runId: string) {
  const run = logRuns.get(key);
  if (run?.runId !== runId || run.backlog) return;
  run.backlog = new Set();
  const log = await api.serverLog(runId).catch(() => [] as TrafficEntry[]);
  if (logRuns.get(key) !== run) return; // another run, a rename or a delete took the log meanwhile
  run.backlog = new Set(log.map((e) => e.id));
  set((s) => ({ logs: { ...s.logs, [key]: mergeLog(s.logs[key] ?? [], log) } }));
}

// ---- events (batched per animation frame: servers can log thousands of entries a second) ----

let pendingTraffic: Record<string, TrafficEntry[]> = {};
let flushTimer: ReturnType<typeof setTimeout> | null = null;

function flushTraffic() {
  flushTimer = null;
  const batch = pendingTraffic;
  pendingTraffic = {};
  set((s) => {
    const logs = { ...s.logs };
    for (const [key, entries] of Object.entries(batch)) {
      const backlog = logRuns.get(key)?.backlog;
      const fresh = backlog?.size ? entries.filter((e) => !backlog.has(e.id)) : entries;
      if (!fresh.length) continue;
      let next = [...(logs[key] ?? []), ...fresh];
      if (next.length > MAX_ENTRIES) next = next.slice(next.length - MAX_ENTRIES);
      logs[key] = next;
    }
    return { logs };
  });
}

onEvent((e) => {
  if (e.type === "quitRequested") {
    void (async () => {
      const parts = [
        ...(e.running ? [e.running === 1 ? "1 server" : `${e.running} servers`] : []),
        ...(e.loadTest ? ["a load test"] : []),
      ];
      const ok = await confirm({
        title: "Quit Zorvik?",
        message: `${parts.join(" and ")} ${parts.length > 1 || e.running > 1 ? "are" : "is"} running. Quitting stops ${parts.length > 1 || e.running > 1 ? "them" : "it"}.`,
        confirmLabel: "Stop and quit",
        danger: true,
      });
      if (ok) await quitApp();
    })();
    return;
  }
  if (e.type !== "server") return;
  const info = get().running.find((r) => r.runId === e.runId);
  const ev = e.event;
  if (ev.type === "stats") {
    if (info) set((s) => ({ running: s.running.map((r) => (r.runId === e.runId ? { ...r, stats: ev.stats } : r)) }));
    return;
  }
  if (ev.type === "stopped") {
    if (ev.error) toast("error", `${info?.name ?? "A server"} stopped`, ev.error);
    set((s) => ({ running: s.running.filter((r) => r.runId !== e.runId) }));
    return;
  }
  if (!info) return; // started from elsewhere and not listed yet; `startServer` loads its log
  const key = serverKey(info.workspacePath, info.serverId);
  claimLog(key, e.runId);
  (pendingTraffic[key] ??= []).push(ev.entry);
  if (!flushTimer) flushTimer = setTimeout(flushTraffic, 50);
});

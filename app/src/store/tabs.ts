// Open tabs: requests (drafts, responses, live WebSocket/SSE/TCP/UDP/MQTT streams),
// servers (their saved configuration; running state lives in store/servers.ts), load tests
// (configuration; runs live in store/loadtests.ts), collection runs (settings and results
// live in store/runner.ts) and tools.
import { create } from "zustand";
import type { ErrorKind } from "../bindings/ErrorKind";
import type { LoadTest } from "../bindings/LoadTest";
import type { Request } from "../bindings/Request";
import type { RequestKind } from "../bindings/RequestKind";
import type { SendResult } from "../bindings/SendResult";
import type { Server } from "../bindings/Server";
import type { SocketOpened } from "../bindings/SocketOpened";
import type { StreamEvent } from "../bindings/StreamEvent";
import type { StreamOpened } from "../bindings/StreamOpened";
import { onEvent } from "../lib/events";
import { SOCKET_KINDS, STREAM_KINDS } from "../lib/http";
import { newId } from "../lib/ids";
import { api, errorMessage, RpcError } from "../lib/rpc";
import { confirm } from "./dialogs";
import { toast } from "./toasts";
import { openModal } from "./ui";
import { refreshEnvironments, refreshTree, refreshVariables, reloadWorkspace } from "./workspace";

export type ResponseState =
  | { status: "idle" }
  | { status: "loading"; startedAt: number }
  | { status: "done"; result: SendResult; at: number }
  /** A one-shot result that is not an HTTP response (e.g. a DNS answer); `kind` says which. */
  | { status: "other"; kind: string; result: unknown; at: number }
  | { status: "error"; message: string; code: string; kind: ErrorKind | null; at: number; durationMs: number };

export interface StreamMessage {
  id: number;
  direction: "sent" | "received" | "info" | "error";
  /** text | binary | ping | pong | event name for SSE */
  kind: string;
  text: string | null;
  base64: string | null;
  size: number;
  timestamp: number;
  eventId?: string | null;
  /** UDP: the other side's address. */
  peer?: string | null;
  /** MQTT: the topic. */
  topic?: string | null;
  /** MQTT: QoS / retained. */
  detail?: string | null;
}

export interface StreamState {
  status: "idle" | "connecting" | "open" | "closed";
  connId: string;
  opened: StreamOpened | SocketOpened | null;
  messages: StreamMessage[];
  error: string | null;
}

/** A request tab (the default kind of tab). */
export interface Tab {
  type?: "request";
  id: string;
  /** Saved location (relative to requests/), null for unsaved drafts. */
  path: string | null;
  draft: Request;
  /** Last saved/loaded content, null when never saved. */
  saved: Request | null;
  response: ResponseState;
  stream: StreamState;
  requestTab: string;
  responseTab: string;
  /** The saved file disappeared (deleted or moved outside the app). */
  orphaned?: boolean;
}

/** A saved server's configuration (running state: store/servers.ts). */
export interface ServerTab {
  type: "server";
  id: string;
  /** File stem under `servers/`. */
  serverId: string;
  draft: Server;
  saved: Server;
  /** The file disappeared or could not be reloaded. */
  orphaned?: boolean;
  /** Selected section of the server editor. */
  section?: string;
}

/** A network tool (TLS inspector, DNS lookup, …); `state` is the tool's own. */
export interface ToolTab {
  type: "tool";
  id: string;
  tool: string;
  state: Record<string, unknown>;
}

/** A saved load test's configuration (runs and results: store/loadtests.ts). */
export interface LoadTestTab {
  type: "loadTest";
  id: string;
  /** File stem under `loadtests/`. */
  testId: string;
  draft: LoadTest;
  saved: LoadTest;
  /** The file disappeared or could not be reloaded. */
  orphaned?: boolean;
  /** An earlier run shown in the results (unset: the latest). */
  runId?: string | null;
  /** Narrow windows show one pane at a time. */
  pane?: "settings" | "results";
}

/** A collection run of a folder or the whole collection (settings and results: store/runner.ts). */
export interface RunnerTab {
  type: "runner";
  id: string;
  /** Folder path relative to requests/ ("" = the whole collection). */
  folder: string;
  /** The folder's (or workspace's) name when the tab opened. */
  name: string;
  /** Narrow windows show one pane at a time. */
  pane?: "settings" | "results";
}

export type AnyTab = Tab | ServerTab | ToolTab | LoadTestTab | RunnerTab;

export const isRequestTab = (t: AnyTab | undefined | null): t is Tab => !!t && (t.type === undefined || t.type === "request");
export const isServerTab = (t: AnyTab | undefined | null): t is ServerTab => t?.type === "server";
export const isToolTab = (t: AnyTab | undefined | null): t is ToolTab => t?.type === "tool";
export const isLoadTestTab = (t: AnyTab | undefined | null): t is LoadTestTab => t?.type === "loadTest";
export const isRunnerTab = (t: AnyTab | undefined | null): t is RunnerTab => t?.type === "runner";

interface TabsState {
  tabs: AnyTab[];
  activeId: string | null;
}

export const useTabs = create<TabsState>(() => ({ tabs: [], activeId: null }));
const set = useTabs.setState;
const get = useTabs.getState;

/** The request tab with this id (undefined for other kinds of tab). */
function reqTab(id: string): Tab | undefined {
  const t = get().tabs.find((x) => x.id === id);
  return isRequestTab(t) ? t : undefined;
}

const requestTabs = (): Tab[] => get().tabs.filter(isRequestTab);

const MAX_MESSAGES = 5000;

/** Bumped by resetTabs so async work started for the previous tabs (restore, open) is dropped. */
let epoch = 0;

function emptyStream(): StreamState {
  return { status: "idle", connId: newId(), opened: null, messages: [], error: null };
}

/** What "New …" menus create: a request kind, or GraphQL (an HTTP request with a GraphQL body). */
export type NewRequestType = RequestKind | "graphql";

export const REQUEST_KIND_NAMES: Record<NewRequestType, string> = {
  http: "New request",
  graphql: "New GraphQL request",
  websocket: "New WebSocket",
  sse: "New event stream",
  tcp: "New TCP connection",
  udp: "New UDP socket",
  dns: "New DNS query",
  mqtt: "New MQTT client",
  grpc: "New gRPC request",
};

export function newRequestDraft(type: NewRequestType = "http"): Request {
  if (type === "graphql") {
    return { name: REQUEST_KIND_NAMES.graphql, kind: "http", seq: 0, method: "POST", url: "", body: { type: "graphql", graphql: { query: "" } } };
  }
  if (type === "grpc") {
    return { name: REQUEST_KIND_NAMES.grpc, kind: "grpc", seq: 0, method: "", url: "", body: { type: "json", text: "{}" } };
  }
  const composer = type === "websocket" || type === "tcp" || type === "udp" || type === "mqtt";
  return {
    name: REQUEST_KIND_NAMES[type],
    kind: type,
    seq: 0,
    method: type === "dns" ? "A" : "GET",
    url: "",
    ...(composer ? { body: { type: "text", text: "" } } : {}),
  };
}

function makeTab(draft: Request, path: string | null, saved: Request | null): Tab {
  return {
    id: newId(),
    path,
    draft,
    saved,
    response: { status: "idle" },
    stream: emptyStream(),
    // GraphQL requests open on their query.
    requestTab: draft.kind && draft.kind !== "http" && draft.kind !== "sse" ? "options" : draft.body?.type === "graphql" ? "body" : "params",
    responseTab: "body",
  };
}

// ---- dirty tracking ---------------------------------------------------------

function canonical(value: unknown, key?: string): unknown {
  if (Array.isArray(value)) return value.length ? value.map((v) => canonical(v)) : undefined;
  if (value && typeof value === "object") {
    const out: Record<string, unknown> = {};
    for (const k of Object.keys(value).sort()) {
      if (k === "seq") continue;
      const raw = (value as Record<string, unknown>)[k];
      // A server's TLS is off unless enabled; rows (headers, routes, records…) are on unless disabled.
      const v = key === "tls" && k === "enabled" ? (raw === true ? true : undefined) : canonical(raw, k);
      if (v !== undefined) out[k] = v;
    }
    return Object.keys(out).length ? out : undefined;
  }
  if (value === null || value === undefined || value === "") return undefined;
  if (key === "enabled" && value === true) return undefined;
  if (key === "kind" && value === "http") return undefined;
  return value;
}

export function sameRequest(a: Request, b: Request): boolean {
  return JSON.stringify(canonical(a)) === JSON.stringify(canonical(b));
}

/** A load test with the file format's defaults filled in (a weight of 1 equals no weight). */
function loadTestDefaults(t: LoadTest): LoadTest {
  return {
    ...t,
    targets: t.targets.map((x) => ({ ...x, weight: x.weight ?? 1, enabled: x.enabled ?? true })),
    thinkTimeMs: t.thinkTimeMs ?? 0,
    maxInFlight: t.maxInFlight ?? 1000,
    keepAlive: t.keepAlive ?? true,
    thresholds: (t.thresholds ?? []).map((x) => ({ ...x, enabled: x.enabled ?? true, target: x.target || undefined })),
  };
}

export function sameLoadTest(a: LoadTest, b: LoadTest): boolean {
  return JSON.stringify(canonical(loadTestDefaults(a))) === JSON.stringify(canonical(loadTestDefaults(b)));
}

export function isDirty(tab: AnyTab): boolean {
  if (isServerTab(tab)) return JSON.stringify(canonical(tab.draft)) !== JSON.stringify(canonical(tab.saved));
  if (isLoadTestTab(tab)) return !sameLoadTest(tab.draft, tab.saved);
  if (isToolTab(tab) || isRunnerTab(tab)) return false;
  if (!tab.saved) return tab.draft.url.trim() !== "" || tab.path === null;
  return !sameRequest(tab.draft, tab.saved);
}

/** Name shown on the tab. */
export function tabTitle(tab: AnyTab): string {
  if (isToolTab(tab)) return tab.tool;
  if (isRunnerTab(tab)) return tab.name;
  return tab.draft.name || "Untitled";
}

// ---- tab lifecycle ----------------------------------------------------------

export function activeTab(): AnyTab | undefined {
  const { tabs, activeId } = get();
  return tabs.find((t) => t.id === activeId);
}

/** Update a request tab (other kinds of tab are left alone). */
export function updateTab(id: string, fn: (t: Tab) => Partial<Tab>) {
  set((s) => ({ tabs: s.tabs.map((t) => (t.id === id && isRequestTab(t) ? { ...t, ...fn(t) } : t)) }));
}

export function updateServerTab(id: string, fn: (t: ServerTab) => Partial<ServerTab>) {
  set((s) => ({ tabs: s.tabs.map((t) => (t.id === id && isServerTab(t) ? { ...t, ...fn(t) } : t)) }));
}

export function updateServerDraft(id: string, fn: (s: Server) => Server) {
  updateServerTab(id, (t) => ({ draft: fn(t.draft) }));
}

export function updateLoadTestTab(id: string, fn: (t: LoadTestTab) => Partial<LoadTestTab>) {
  set((s) => ({ tabs: s.tabs.map((t) => (t.id === id && isLoadTestTab(t) ? { ...t, ...fn(t) } : t)) }));
}

export function updateLoadTestDraft(id: string, fn: (t: LoadTest) => LoadTest) {
  updateLoadTestTab(id, (t) => ({ draft: fn(t.draft) }));
}

export function updateRunnerTab(id: string, fn: (t: RunnerTab) => Partial<RunnerTab>) {
  set((s) => ({ tabs: s.tabs.map((t) => (t.id === id && isRunnerTab(t) ? { ...t, ...fn(t) } : t)) }));
}

export function updateToolState(id: string, fn: (state: Record<string, unknown>) => Record<string, unknown>) {
  set((s) => ({ tabs: s.tabs.map((t) => (t.id === id && isToolTab(t) ? { ...t, state: fn(t.state) } : t)) }));
}

export function updateDraft(id: string, fn: (r: Request) => Request) {
  updateTab(id, (t) => ({ draft: fn(t.draft) }));
}

export function activate(id: string) {
  set({ activeId: id });
}

/** Open a saved request in a tab (or find its tab); `activate: false` leaves the current tab in front. */
export async function openRequest(path: string, { activate = true } = {}) {
  const existing = requestTabs().find((t) => t.path === path);
  if (existing) {
    if (activate) set({ activeId: existing.id });
    return;
  }
  const started = epoch;
  try {
    const req = await api.readRequest(path);
    if (epoch !== started) return; // workspace switched meanwhile
    // Opened by a concurrent call (e.g. a double click) while this one was reading.
    const opened = requestTabs().find((t) => t.path === path);
    if (opened) {
      if (activate) set({ activeId: opened.id });
      return;
    }
    const tab = makeTab(structuredClone(req), path, req);
    set((s) => ({ tabs: [...s.tabs, tab], activeId: activate || !s.activeId ? tab.id : s.activeId }));
  } catch (e) {
    toast("error", "Could not open request", errorMessage(e));
  }
}

export function openDraft(request: Request, { activate = true } = {}) {
  const tab = makeTab(request, null, null);
  set((s) => ({ tabs: [...s.tabs, tab], activeId: activate || !s.activeId ? tab.id : s.activeId }));
  return tab.id;
}

export function newRequest(type: NewRequestType = "http") {
  return openDraft(newRequestDraft(type));
}

/** Open a saved server in a tab (or switch to it). */
export async function openServer(serverId: string) {
  const existing = get().tabs.find((t) => isServerTab(t) && t.serverId === serverId);
  if (existing) {
    set({ activeId: existing.id });
    return;
  }
  const started = epoch;
  try {
    const server = await api.readServer(serverId);
    if (epoch !== started) return;
    const opened = get().tabs.find((t) => isServerTab(t) && t.serverId === serverId);
    if (opened) {
      set({ activeId: opened.id });
      return;
    }
    const tab: ServerTab = { type: "server", id: newId(), serverId, draft: structuredClone(server), saved: server };
    set((s) => ({ tabs: [...s.tabs, tab], activeId: tab.id }));
  } catch (e) {
    toast("error", "Could not open server", errorMessage(e));
  }
}

/** Open a saved load test in a tab (or switch to it). */
export async function openLoadTest(testId: string) {
  const existing = get().tabs.find((t) => isLoadTestTab(t) && t.testId === testId);
  if (existing) {
    set({ activeId: existing.id });
    return;
  }
  const started = epoch;
  try {
    const test = await api.readLoadTest(testId);
    if (epoch !== started) return;
    const opened = get().tabs.find((t) => isLoadTestTab(t) && t.testId === testId);
    if (opened) {
      set({ activeId: opened.id });
      return;
    }
    const tab: LoadTestTab = { type: "loadTest", id: newId(), testId, draft: structuredClone(test), saved: test };
    set((s) => ({ tabs: [...s.tabs, tab], activeId: tab.id }));
  } catch (e) {
    toast("error", "Could not open load test", errorMessage(e));
  }
}

/** Open a tool in a tab (one tab per tool). */
export function openTool(tool: string) {
  const existing = get().tabs.find((t) => isToolTab(t) && t.tool === tool);
  if (existing) {
    set({ activeId: existing.id });
    return;
  }
  const tab: ToolTab = { type: "tool", id: newId(), tool, state: {} };
  set((s) => ({ tabs: [...s.tabs, tab], activeId: tab.id }));
}

/** Open the docs (one tab), at `topic` or at the top. */
export function openDocs(topic: string | null = null) {
  openTool("docs");
  const tab = get().tabs.find((t) => isToolTab(t) && t.tool === "docs");
  if (tab) updateToolState(tab.id, (s) => ({ ...s, topic, at: Date.now() }));
}

/** Open the runner for a folder ("" = the whole collection), or switch to its tab. */
export function openRunner(folder: string, name: string, { activate = true } = {}) {
  const existing = get().tabs.find((t) => isRunnerTab(t) && t.folder === folder);
  if (existing) {
    if (activate) set({ activeId: existing.id });
    return existing.id;
  }
  const tab: RunnerTab = { type: "runner", id: newId(), folder, name };
  set((s) => ({ tabs: [...s.tabs, tab], activeId: activate || !s.activeId ? tab.id : s.activeId }));
  return tab.id;
}

/** Keep server tabs in step with a rename (new file id) or delete. */
export function onServerRenamed(oldId: string, newId: string, saved: Server) {
  set((s) => ({
    tabs: s.tabs.map((t) =>
      isServerTab(t) && t.serverId === oldId ? { ...t, serverId: newId, saved, draft: { ...t.draft, name: saved.name } } : t,
    ),
  }));
}

export function onServerDeleted(serverId: string) {
  set((s) => ({ tabs: s.tabs.map((t) => (isServerTab(t) && t.serverId === serverId ? { ...t, orphaned: true } : t)) }));
}

/** Keep load test tabs in step with a rename from the sidebar (new file id) or a delete. */
export function onLoadTestRenamed(oldId: string, newId: string, saved: LoadTest) {
  set((s) => ({
    tabs: s.tabs.map((t) =>
      isLoadTestTab(t) && t.testId === oldId ? { ...t, testId: newId, saved, draft: { ...t.draft, name: saved.name } } : t,
    ),
  }));
}

export function onLoadTestDeleted(testId: string) {
  set((s) => ({ tabs: s.tabs.map((t) => (isLoadTestTab(t) && t.testId === testId ? { ...t, orphaned: true } : t)) }));
}

export async function closeTab(id: string, force = false): Promise<boolean> {
  const any = get().tabs.find((t) => t.id === id);
  if (!any) return true;
  if (!isRequestTab(any)) {
    if (!force && isDirty(any) && (isServerTab(any) || isLoadTestTab(any))) {
      const ok = await confirm({
        title: "Discard unsaved changes?",
        message: `"${any.draft.name}" has changes that are not saved. ${isServerTab(any) ? "A running server keeps its current setup." : "A running load test keeps running."}`,
        confirmLabel: "Discard",
        danger: true,
      });
      if (!ok) return false;
    }
    removeTab(id);
    return true;
  }
  const tab = any;
  // Unsaved drafts only ask when something was entered (a body or headers count, not just a URL).
  if (!force && isDirty(tab) && (tab.saved || !sameRequest(tab.draft, newRequestDraft(tab.draft.kind ?? "http")))) {
    const ok = await confirm({
      title: "Discard unsaved changes?",
      message: `"${tab.draft.name}" has changes that are not saved.`,
      confirmLabel: "Discard",
      danger: true,
    });
    if (!ok) return false;
  }
  stopTabActivity(tab);
  removeTab(id);
  return true;
}

function removeTab(id: string) {
  set((s) => {
    const index = s.tabs.findIndex((t) => t.id === id);
    const tabs = s.tabs.filter((t) => t.id !== id);
    let activeId = s.activeId;
    if (activeId === id) activeId = tabs[Math.min(index, tabs.length - 1)]?.id ?? null;
    return { tabs, activeId };
  });
}

export async function closeOtherTabs(keepId: string) {
  for (const t of [...get().tabs]) {
    if (t.id !== keepId && !(await closeTab(t.id))) return;
  }
}

function stopTabActivity(tab: Tab) {
  if (tab.response.status === "loading") void api.cancel(tab.id).catch(() => {});
  if (tab.stream.status === "open" || tab.stream.status === "connecting") closeStream(tab.draft.kind, tab.stream.connId);
}

function closeStream(kind: RequestKind | null | undefined, connId: string) {
  if (kind === "websocket") void api.wsClose(connId).catch(() => {});
  else if (kind === "sse") void api.sseClose(connId).catch(() => {});
  else if (kind && SOCKET_KINDS.includes(kind)) void api.socketClose(connId).catch(() => {});
}

/** Save the tab; unsaved drafts open the "Save as" dialog. */
export async function saveTab(id: string): Promise<boolean> {
  const any = get().tabs.find((t) => t.id === id);
  if (isServerTab(any)) return saveServerTab(any);
  if (isLoadTestTab(any)) return saveLoadTestTab(any);
  const tab = reqTab(id);
  if (!tab) return false;
  if (!tab.path) {
    openModal({ type: "saveAs", tabId: id });
    return false;
  }
  const draft = tab.draft;
  try {
    await api.saveRequest(tab.path, draft);
    // Mark what was written as saved: edits made while saving stay dirty.
    updateTab(id, () => ({ saved: structuredClone(draft), orphaned: false }));
    return true;
  } catch (e) {
    toast("error", "Could not save", errorMessage(e));
    return false;
  }
}

async function saveServerTab(tab: ServerTab): Promise<boolean> {
  const draft = tab.draft;
  try {
    let newId: string;
    try {
      newId = await api.saveServer(tab.serverId, draft);
    } catch (e) {
      // Its file was deleted: "Save to write it again" creates it anew.
      if (!(tab.orphaned && e instanceof RpcError && e.code === "notFound")) throw e;
      newId = await api.createServer(draft);
    }
    updateServerTab(tab.id, () => ({ serverId: newId, saved: structuredClone(draft), orphaned: false }));
    for (const h of serverSavedHandlers) h(tab.serverId, newId);
    return true;
  } catch (e) {
    toast("error", "Could not save", errorMessage(e));
    return false;
  }
}

async function saveLoadTestTab(tab: LoadTestTab): Promise<boolean> {
  const draft = tab.draft;
  if (!draft.name.trim()) {
    toast("info", "Give the load test a name first");
    return false;
  }
  try {
    let newId: string;
    try {
      newId = await api.saveLoadTest(tab.testId, draft);
    } catch (e) {
      // Its file was deleted: "Save to write it again" creates it anew.
      if (!(tab.orphaned && e instanceof RpcError && e.code === "notFound")) throw e;
      newId = await api.createLoadTest(draft);
    }
    updateLoadTestTab(tab.id, () => ({ testId: newId, saved: structuredClone(draft), orphaned: false }));
    for (const h of loadTestSavedHandlers) h(tab.testId, newId);
    return true;
  } catch (e) {
    toast("error", "Could not save", errorMessage(e));
    return false;
  }
}

export async function saveTabAs(id: string, parent: string, name: string): Promise<boolean> {
  const tab = reqTab(id);
  if (!tab) return false;
  try {
    const draft = { ...tab.draft, name };
    const path = await api.createRequest(parent, draft);
    const saved = await api.readRequest(path);
    updateTab(id, () => ({ path, draft: structuredClone(saved), saved, orphaned: false }));
    await refreshTree();
    return true;
  } catch (e) {
    toast("error", "Could not save", errorMessage(e));
    return false;
  }
}

// ---- sending ----------------------------------------------------------------

/** Handlers for one-shot request kinds other than HTTP (e.g. DNS), registered by their modules. */
const oneShot: Partial<Record<RequestKind, (tab: Tab) => Promise<{ kind: string; result: unknown }>>> = {};

export function registerOneShot(kind: RequestKind, run: (tab: Tab) => Promise<{ kind: string; result: unknown }>) {
  oneShot[kind] = run;
}

export async function send(id: string) {
  const tab = reqTab(id);
  if (!tab) return;
  const kind = tab.draft.kind ?? "http";
  if (STREAM_KINDS.includes(kind)) return connect(id);
  // Already sending, like the Send button (hidden while loading): cancel first to resend. This also
  // stops one key press that reaches both a field's Enter handler and the global Mod+Enter from sending twice.
  if (tab.response.status === "loading") return;
  if (!tab.draft.url.trim()) {
    toast("info", kind === "dns" ? "Enter a name to look up" : "Enter a URL first");
    return;
  }
  const startedAt = Date.now();
  const loading: ResponseState = { status: "loading", startedAt };
  updateTab(id, () => ({ response: loading }));
  // Only the send that set this loading state may replace it (the tab may be gone or restored by now).
  const settle = (response: ResponseState) => {
    if (reqTab(id)?.response === loading) updateTab(id, () => ({ response }));
  };
  try {
    const run = oneShot[kind];
    if (run) {
      const { kind: resultKind, result } = await run(tab);
      settle({ status: "other", kind: resultKind, result, at: Date.now() });
      return;
    }
    const result = await api.send(id, tab.draft, tab.path);
    settle({ status: "done", result, at: Date.now() });
    // Scripts may have set variables (kept as local values).
    if (result.scripts) void refreshVariables();
  } catch (e) {
    const err = e instanceof RpcError ? e : new RpcError({ code: "internal", message: errorMessage(e), networkKind: null });
    if (err.code === "script") void refreshVariables();
    settle({ status: "error", message: err.message, code: err.code, kind: err.networkKind, at: Date.now(), durationMs: Date.now() - startedAt });
  }
}

export async function cancel(id: string) {
  await api.cancel(id).catch(() => {});
}

export async function connect(id: string) {
  const tab = reqTab(id);
  if (!tab) return;
  // Already connected or connecting (e.g. Mod+Enter): disconnect first to reconnect.
  if (tab.stream.status === "open" || tab.stream.status === "connecting") return;
  if (!tab.draft.url.trim()) {
    toast("info", "Enter a URL first");
    return;
  }
  const connId = newId();
  const kind = tab.draft.kind;
  updateTab(id, (t) => ({
    stream: { ...t.stream, status: "connecting", connId, error: null, opened: null },
  }));
  pushMessage(id, { direction: "info", kind: "info", text: `Connecting to ${tab.draft.url}…` });
  try {
    let opened: StreamOpened | SocketOpened;
    let summary: string;
    if (kind && SOCKET_KINDS.includes(kind)) {
      const r = await api.socketConnect(connId, tab.draft, tab.path);
      opened = r.opened;
      summary = [r.opened.protocol, r.opened.remoteAddr, `${Math.round(r.opened.timing.totalMs)} ms`].filter(Boolean).join(" · ");
    } else {
      const r = kind === "websocket" ? await api.wsConnect(connId, tab.draft, tab.path) : await api.sseConnect(connId, tab.draft, tab.path);
      opened = r;
      summary = `${r.meta.status} ${r.meta.statusText} · ${Math.round(r.timing.totalMs)} ms`;
    }
    // Disconnected, closed or replaced while connecting: close it rather than leak it (closing
    // before it opened was a no-op on the backend).
    if (reqTab(id)?.stream.connId !== connId) {
      closeStream(kind, connId);
      return;
    }
    updateTab(id, (t) => ({ stream: { ...t.stream, status: "open", opened } }));
    pushMessage(id, { direction: "info", kind: "info", text: `Connected · ${summary}` });
  } catch (e) {
    if (reqTab(id)?.stream.connId !== connId) return;
    const message = errorMessage(e);
    updateTab(id, (t) => ({ stream: { ...t.stream, status: "closed", error: message } }));
    pushMessage(id, { direction: "error", kind: "error", text: message });
  }
}

export async function disconnect(id: string) {
  const tab = reqTab(id);
  if (!tab) return;
  if (tab.draft.kind === "websocket") await api.wsClose(tab.stream.connId).catch(() => {});
  else if (tab.draft.kind === "sse") await api.sseClose(tab.stream.connId).catch(() => {});
  else await api.socketClose(tab.stream.connId).catch(() => {});
  if (tab.stream.status === "connecting") {
    updateTab(id, (t) => ({ stream: { ...t.stream, status: "closed", connId: newId() } }));
  }
}

/** Send a composed message on an open WebSocket/TCP/UDP connection (text with variables, or hex). */
export async function wsSend(id: string, text: string, binary: boolean) {
  const tab = reqTab(id);
  if (!tab || tab.stream.status !== "open") return;
  const socket = SOCKET_KINDS.includes(tab.draft.kind ?? "http");
  try {
    const base64 = binary ? hexToBase64(text) : null;
    if (binary && base64 === null) {
      toast("error", "Invalid hex", "Binary messages are entered as hex bytes, e.g. 48 65 6c 6c 6f");
      return;
    }
    if (tab.draft.kind === "mqtt") {
      // Text or bytes, always as a publish (the session has no plain "send").
      const mqtt = tab.draft.mqtt;
      const topic = mqtt?.topic?.includes("{{") ? await api.renderVariables(mqtt.topic) : (mqtt?.topic ?? "");
      if (!topic.trim()) {
        toast("info", "Enter a topic to publish to");
        return;
      }
      const rendered = base64 !== null ? null : text.includes("{{") ? await api.renderVariables(text) : text;
      await api.socketSend(tab.stream.connId, { type: "publish", topic, text: rendered, base64, qos: mqtt?.qos ?? 0, retain: mqtt?.retain ?? false });
    } else if (base64 !== null) {
      if (socket) await api.socketSend(tab.stream.connId, { type: "binary", base64 });
      else await api.wsSend(tab.stream.connId, { type: "binary", base64 });
    } else {
      const rendered = text.includes("{{") ? await api.renderVariables(text) : text;
      if (socket) await api.socketSend(tab.stream.connId, { type: "text", text: rendered });
      else await api.wsSend(tab.stream.connId, { type: "text", text: rendered });
    }
  } catch (e) {
    toast("error", "Send failed", errorMessage(e));
  }
}

/** Hex bytes (`48 65 6c`) as base64, or null when not valid hex. */
export function hexToBase64(text: string): string | null {
  const bytes = hexToBytes(text);
  if (!bytes) return null;
  let bin = "";
  bytes.forEach((b) => (bin += String.fromCharCode(b)));
  return btoa(bin);
}

function hexToBytes(text: string): number[] | null {
  const clean = text.replace(/0x/gi, "").replace(/[\s,:]/g, "");
  if (clean.length % 2 !== 0 || /[^0-9a-f]/i.test(clean)) return null;
  const out: number[] = [];
  for (let i = 0; i < clean.length; i += 2) out.push(parseInt(clean.slice(i, i + 2), 16));
  return out;
}

export function clearMessages(id: string) {
  updateTab(id, (t) => ({ stream: { ...t.stream, messages: [] } }));
}

// ---- stream events (batched to keep rendering cheap under load) -------------

let messageSeq = 0;
let pending: { tabId: string; msg: StreamMessage; close?: { reason: string; error?: string } }[] = [];
let flushTimer: ReturnType<typeof setTimeout> | null = null;

function pushMessage(tabId: string, msg: Omit<StreamMessage, "id" | "size" | "timestamp" | "base64"> & Partial<StreamMessage>, close?: { reason: string; error?: string }) {
  pending.push({
    tabId,
    msg: { id: ++messageSeq, size: 0, timestamp: Date.now(), base64: null, ...msg } as StreamMessage,
    close,
  });
  if (!flushTimer) flushTimer = setTimeout(flush, 40);
}

function flush() {
  flushTimer = null;
  const batch = pending;
  pending = [];
  if (!batch.length) return;
  set((s) => ({
    tabs: s.tabs.map((t) => {
      if (!isRequestTab(t)) return t;
      const mine = batch.filter((b) => b.tabId === t.id);
      if (!mine.length) return t;
      let messages = [...t.stream.messages, ...mine.map((m) => m.msg)];
      if (messages.length > MAX_MESSAGES) messages = messages.slice(messages.length - MAX_MESSAGES);
      const closing = mine.find((m) => m.close);
      return {
        ...t,
        stream: {
          ...t.stream,
          messages,
          ...(closing ? { status: "closed" as const, error: closing.close?.error ?? t.stream.error } : {}),
        },
      };
    }),
  }));
}

function tabForConn(connId: string): Tab | undefined {
  return requestTabs().find((t) => t.stream.connId === connId);
}

let fileChangeTimer: ReturnType<typeof setTimeout> | null = null;
let changedPaths = new Set<string>();

function handleEvent(e: StreamEvent) {
  switch (e.type) {
    case "ws": {
      const tab = tabForConn(e.connId);
      if (!tab) return;
      const ev = e.event;
      if (ev.type === "message") {
        pushMessage(tab.id, {
          direction: ev.direction,
          kind: ev.kind,
          text: ev.text,
          base64: ev.base64,
          size: ev.size,
          timestamp: ev.timestamp,
        });
      } else if (ev.type === "error") {
        pushMessage(tab.id, { direction: "error", kind: "error", text: ev.message });
      } else {
        const reason = [ev.code != null ? `code ${ev.code}` : null, ev.reason || null].filter(Boolean).join(" · ");
        pushMessage(
          tab.id,
          { direction: "info", kind: "info", text: `Disconnected${ev.byClient ? "" : " by server"}${reason ? ` (${reason})` : ""}` },
          { reason },
        );
      }
      return;
    }
    case "sse": {
      const tab = tabForConn(e.connId);
      if (!tab) return;
      const ev = e.event;
      if (ev.type === "event") {
        pushMessage(tab.id, {
          direction: "received",
          kind: ev.event.event,
          text: ev.event.data,
          size: ev.event.data.length,
          timestamp: ev.timestamp,
          eventId: ev.event.id,
        });
      } else if (ev.type === "error") {
        pushMessage(tab.id, { direction: "error", kind: "error", text: ev.message });
      } else {
        pushMessage(tab.id, { direction: "info", kind: "info", text: ev.reason }, { reason: ev.reason });
      }
      return;
    }
    case "socket": {
      const tab = tabForConn(e.connId);
      if (!tab) return;
      const ev = e.event;
      if (ev.type === "message") {
        pushMessage(tab.id, {
          direction: ev.direction,
          kind: ev.text !== null ? "text" : "binary",
          text: ev.text,
          base64: ev.base64,
          size: ev.size,
          timestamp: ev.timestamp,
          peer: ev.peer,
          topic: ev.topic,
          detail: ev.detail,
        });
      } else if (ev.type === "info") {
        pushMessage(tab.id, { direction: "info", kind: "info", text: ev.text, timestamp: ev.timestamp });
      } else if (ev.type === "error") {
        pushMessage(tab.id, { direction: "error", kind: "error", text: ev.message });
      } else {
        pushMessage(tab.id, { direction: "info", kind: "info", text: `Disconnected${ev.reason ? ` (${ev.reason})` : ""}` }, { reason: ev.reason });
      }
      return;
    }
    case "openUrl":
      // Dev bridge only (Tauri opens it natively). The URL comes from workspace config: never run javascript: here.
      if (/^https?:\/\//i.test(e.url)) window.open(e.url, "_blank", "noopener");
      return;
    case "workspaceChanged":
      for (const p of e.paths) changedPaths.add(p);
      if (fileChangeTimer) clearTimeout(fileChangeTimer);
      fileChangeTimer = setTimeout(() => {
        const paths = changedPaths;
        changedPaths = new Set();
        void handleFileChanges([...paths]);
      }, 250);
      return;
  }
}

/** Sync the UI with edits made outside the app (Git pull, editor). */
async function handleFileChanges(paths: string[]) {
  if (paths.some((p) => p === "zorvik.yaml")) await reloadWorkspace().catch(() => {});
  if (paths.some((p) => p.startsWith("environments/"))) await refreshEnvironments().catch(() => {});
  const loadTestIds = paths.filter((p) => p.startsWith("loadtests/")).map((p) => p.slice("loadtests/".length).replace(/\.yaml$/i, ""));
  if (loadTestIds.length) {
    for (const h of loadTestFileHandlers) h();
    for (const tab of get().tabs) {
      if (!isLoadTestTab(tab) || !loadTestIds.includes(tab.testId)) continue;
      try {
        const fresh = await api.readLoadTest(tab.testId);
        // The sidebar order (`seq`) is not an edit, but a save writes the draft's: keep it current
        // so saving does not undo a reorder.
        if (sameLoadTest(fresh, tab.saved)) {
          if (tab.orphaned || tab.draft.seq !== fresh.seq) {
            updateLoadTestTab(tab.id, (t) => ({ saved: fresh, draft: t.draft.seq === fresh.seq ? t.draft : { ...t.draft, seq: fresh.seq }, orphaned: false }));
          }
          continue;
        }
        updateLoadTestTab(tab.id, (t) =>
          isDirty(t) ? { saved: fresh, draft: { ...t.draft, seq: fresh.seq }, orphaned: false } : { saved: fresh, draft: structuredClone(fresh), orphaned: false },
        );
      } catch {
        updateLoadTestTab(tab.id, () => ({ orphaned: true }));
      }
    }
  }
  const serverIds = paths.filter((p) => p.startsWith("servers/")).map((p) => p.slice("servers/".length).replace(/\.yaml$/i, ""));
  if (serverIds.length) {
    for (const h of serverFileHandlers) h();
    for (const tab of get().tabs) {
      if (!isServerTab(tab) || !serverIds.includes(tab.serverId)) continue;
      try {
        const fresh = await api.readServer(tab.serverId);
        // Like load tests: keep the draft's sidebar order current so saving does not undo a reorder.
        if (JSON.stringify(canonical(fresh)) === JSON.stringify(canonical(tab.saved))) {
          if (tab.orphaned || tab.draft.seq !== fresh.seq) {
            updateServerTab(tab.id, (t) => ({ saved: fresh, draft: t.draft.seq === fresh.seq ? t.draft : { ...t.draft, seq: fresh.seq }, orphaned: false }));
          }
          continue;
        }
        updateServerTab(tab.id, (t) =>
          isDirty(t) ? { saved: fresh, draft: { ...t.draft, seq: fresh.seq }, orphaned: false } : { saved: fresh, draft: structuredClone(fresh), orphaned: false },
        );
      } catch {
        updateServerTab(tab.id, () => ({ orphaned: true }));
      }
    }
  }
  const requestPaths = paths.filter((p) => p.startsWith("requests/")).map((p) => p.slice("requests/".length));
  if (!requestPaths.length) return;
  await refreshTree().catch(() => {});
  for (const tab of requestTabs()) {
    if (!tab.path) continue;
    const affected = requestPaths.some((p) => p === tab.path || tab.path!.startsWith(`${p}/`));
    if (!affected) continue;
    try {
      const fresh = await api.readRequest(tab.path);
      // Readable again: drop an earlier "could not be reloaded" warning.
      if (tab.saved && sameRequest(fresh, tab.saved)) {
        if (tab.orphaned) updateTab(tab.id, () => ({ orphaned: false }));
        continue;
      }
      updateTab(tab.id, (t) =>
        isDirty(t) ? { saved: fresh, orphaned: false } : { saved: fresh, draft: structuredClone(fresh), orphaned: false },
      );
    } catch {
      updateTab(tab.id, () => ({ orphaned: true }));
    }
  }
}

/** Called when files under servers/ change on disk (the servers store refreshes its list). */
const serverFileHandlers = new Set<() => void>();
export function onServerFilesChanged(handler: () => void) {
  serverFileHandlers.add(handler);
}

/** Called after a server tab is saved (the id changes when the name does). */
const serverSavedHandlers = new Set<(oldId: string, newId: string) => void>();
export function onServerSaved(handler: (oldId: string, newId: string) => void) {
  serverSavedHandlers.add(handler);
}

/** Called when files under loadtests/ change on disk (the load tests store refreshes its list). */
const loadTestFileHandlers = new Set<() => void>();
export function onLoadTestFilesChanged(handler: () => void) {
  loadTestFileHandlers.add(handler);
}

/** Called after a load test tab is saved (the id changes when the name does). */
const loadTestSavedHandlers = new Set<(oldId: string, newId: string) => void>();
export function onLoadTestSaved(handler: (oldId: string, newId: string) => void) {
  loadTestSavedHandlers.add(handler);
}

// ---- keep tabs in sync with sidebar operations -------------------------------

/** Called after a request or folder was moved or renamed (the runner's request lists follow). */
const itemMovedHandlers = new Set<(oldPath: string, newPath: string) => void>();
export function onItemsMoved(handler: (oldPath: string, newPath: string) => void) {
  itemMovedHandlers.add(handler);
}

export function onItemMoved(oldPath: string, newPath: string) {
  const moved = (path: string) => (path === oldPath ? newPath : path.startsWith(`${oldPath}/`) ? newPath + path.slice(oldPath.length) : path);
  set((s) => ({
    tabs: s.tabs.map((t) => {
      if (isRunnerTab(t) && t.folder) return moved(t.folder) === t.folder ? t : { ...t, folder: moved(t.folder) };
      if (!isRequestTab(t) || !t.path) return t;
      if (t.path === oldPath) return { ...t, path: newPath };
      if (t.path.startsWith(`${oldPath}/`)) return { ...t, path: newPath + t.path.slice(oldPath.length) };
      return t;
    }),
  }));
  for (const handler of itemMovedHandlers) handler(oldPath, newPath);
}

export async function onItemRenamed(oldPath: string, newPath: string) {
  onItemMoved(oldPath, newPath);
  // The display name lives inside the file; refresh clean tabs.
  for (const t of requestTabs()) {
    if (t.path === newPath) {
      const fresh = await api.readRequest(newPath).catch(() => null);
      if (fresh) updateTab(t.id, (tab) => ({ saved: fresh, draft: { ...tab.draft, name: fresh.name } }));
    }
  }
}

export function onItemDeleted(path: string) {
  set((s) => ({
    tabs: s.tabs.map((t) =>
      isRequestTab(t) && t.path && (t.path === path || t.path.startsWith(`${path}/`)) ? { ...t, path: null, saved: null, orphaned: true } : t,
    ),
  }));
}

// ---- persistence of open tabs per workspace --------------------------------------

type PersistedTab =
  | { type?: "request"; id: string; path: string | null; draft?: Request }
  | { type: "server"; id: string; serverId: string; draft?: Server }
  | { type: "loadTest"; id: string; testId: string; draft?: LoadTest }
  | { type: "runner"; id: string; folder: string; name: string }
  | { type: "tool"; id: string; tool: string };

const storageKey = (wsPath: string) => `zv:tabs:${wsPath}`;

/** Workspace the open tabs belong to (set once they're restored, cleared by resetTabs); only it is persisted. */
let owner: string | null = null;

export async function restoreTabs(wsPath: string) {
  const started = epoch;
  let data: { tabs: PersistedTab[]; activeId: string | null } | null = null;
  try {
    data = JSON.parse(localStorage.getItem(storageKey(wsPath)) ?? "null");
  } catch {
    data = null;
  }
  if (!data?.tabs?.length) {
    owner = wsPath;
    return;
  }
  const tabs: AnyTab[] = [];
  for (const p of data.tabs) {
    if (p.type === "tool") {
      tabs.push({ type: "tool", id: p.id, tool: p.tool, state: {} });
      continue;
    }
    if (p.type === "runner") {
      tabs.push({ type: "runner", id: p.id, folder: p.folder, name: p.name });
      continue;
    }
    if (p.type === "server") {
      const saved = await api.readServer(p.serverId).catch(() => null);
      if (saved) tabs.push({ type: "server", id: p.id, serverId: p.serverId, draft: p.draft ? { ...p.draft, seq: saved.seq } : structuredClone(saved), saved });
      continue;
    }
    if (p.type === "loadTest") {
      const saved = await api.readLoadTest(p.testId).catch(() => null);
      if (saved) tabs.push({ type: "loadTest", id: p.id, testId: p.testId, draft: p.draft ? { ...p.draft, seq: saved.seq } : structuredClone(saved), saved });
      continue;
    }
    if (p.path) {
      const saved = await api.readRequest(p.path).catch(() => null);
      if (saved) {
        tabs.push({ ...makeTab(p.draft ?? structuredClone(saved), p.path, saved), id: p.id });
      } else if (p.draft) {
        tabs.push({ ...makeTab(p.draft, null, null), id: p.id, orphaned: true });
      }
    } else if (p.draft) {
      tabs.push({ ...makeTab(p.draft, null, null), id: p.id });
    }
  }
  if (epoch !== started) return; // workspace switched or closed while restoring
  owner = wsPath;
  // Merge: keep anything the user opened while restoring (don't clobber it).
  set((s) => {
    const fresh = s.tabs.filter((t) => !tabs.some((r) => r.id === t.id || sameTarget(r, t)));
    const restoredActive = tabs.find((t) => t.id === data!.activeId)?.id ?? tabs[0]?.id ?? null;
    // The user's active tab may have been replaced by the restored tab for the same request.
    const current = s.tabs.find((t) => t.id === s.activeId);
    const kept = current && [...tabs, ...fresh].find((t) => t.id === current.id || sameTarget(t, current));
    return { tabs: [...tabs, ...fresh], activeId: kept?.id ?? restoredActive };
  });
}

/** Two tabs showing the same saved request, server, load test, tool or runner folder. */
function sameTarget(a: AnyTab, b: AnyTab): boolean {
  if (isRequestTab(a) && isRequestTab(b)) return !!a.path && a.path === b.path;
  if (isServerTab(a) && isServerTab(b)) return a.serverId === b.serverId;
  if (isLoadTestTab(a) && isLoadTestTab(b)) return a.testId === b.testId;
  if (isToolTab(a) && isToolTab(b)) return a.tool === b.tool;
  if (isRunnerTab(a) && isRunnerTab(b)) return a.folder === b.folder;
  return false;
}

let persistTimer: ReturnType<typeof setTimeout> | null = null;

function persistTabs() {
  if (persistTimer) clearTimeout(persistTimer);
  persistTimer = null;
  if (!owner) return;
  const { tabs, activeId } = get();
  const persisted: PersistedTab[] = tabs.map((t): PersistedTab => {
    if (isToolTab(t)) return { type: "tool", id: t.id, tool: t.tool };
    if (isRunnerTab(t)) return { type: "runner", id: t.id, folder: t.folder, name: t.name };
    if (isServerTab(t)) return { type: "server", id: t.id, serverId: t.serverId, ...(isDirty(t) ? { draft: t.draft } : {}) };
    if (isLoadTestTab(t)) return { type: "loadTest", id: t.id, testId: t.testId, ...(isDirty(t) ? { draft: t.draft } : {}) };
    return { id: t.id, path: t.path, ...(isDirty(t) ? { draft: t.draft } : {}) };
  });
  try {
    localStorage.setItem(storageKey(owner), JSON.stringify({ tabs: persisted, activeId }));
  } catch {
    /* storage full or unavailable */
  }
}

useTabs.subscribe((s, prev) => {
  // Only what is persisted counts: stream traffic every 40 ms must not keep postponing the write.
  const changed =
    s.activeId !== prev.activeId ||
    s.tabs.length !== prev.tabs.length ||
    s.tabs.some((t, i) => {
      const p = prev.tabs[i];
      if (t.id !== p.id || t.type !== p.type) return true;
      if (isToolTab(t)) return false;
      if (isRunnerTab(t)) return t.folder !== (p as RunnerTab).folder;
      const q = p as Tab | ServerTab | LoadTestTab;
      return (
        ("path" in t && t.path !== (q as Tab).path) ||
        ("serverId" in t && t.serverId !== (q as ServerTab).serverId) ||
        ("testId" in t && t.testId !== (q as LoadTestTab).testId) ||
        t.draft !== q.draft ||
        t.saved !== q.saved
      );
    });
  if (!changed || !owner) return;
  if (persistTimer) clearTimeout(persistTimer);
  persistTimer = setTimeout(persistTabs, 300);
});

/** Drop all tabs (workspace switch/close). Their list and unsaved drafts stay saved for that workspace. */
export function resetTabs() {
  persistTabs();
  owner = null;
  epoch++;
  for (const t of requestTabs()) stopTabActivity(t);
  set({ tabs: [], activeId: null });
}

onEvent(handleEvent);

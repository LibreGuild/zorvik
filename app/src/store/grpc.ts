// gRPC requests: services per source (server reflection through the URL, or the request's
// .proto files) for the method picker, and the call of each tab. Send runs a unary call
// (grpc.invoke) or a stream (grpc.start, then grpc.send / grpc.end), registered as the kind's
// one-shot sender so the URL bar's Send / Cancel and the loading state work like other requests.
// Stream events (`grpc {sessionId, event}`) are batched like other stream traffic.
import { create } from "zustand";
import type { GrpcEvent } from "../bindings/GrpcEvent";
import type { GrpcInvokeResult } from "../bindings/GrpcInvokeResult";
import type { GrpcMessage } from "../bindings/GrpcMessage";
import type { GrpcMethod } from "../bindings/GrpcMethod";
import type { GrpcOpened } from "../bindings/GrpcOpened";
import type { GrpcService } from "../bindings/GrpcService";
import type { GrpcStatus } from "../bindings/GrpcStatus";
import type { Header } from "../bindings/Header";
import type { Request } from "../bindings/Request";
import type { StreamEvent } from "../bindings/StreamEvent";
import type { Timing } from "../bindings/Timing";
import type { Tone } from "../lib/format";
import { onEvent } from "../lib/events";
import { newId } from "../lib/ids";
import { api, errorMessage, RpcError } from "../lib/rpc";
import { registerOneShot, type Tab, updateDraft, useTabs } from "./tabs";
import { toast } from "./toasts";
import { useWorkspace } from "./workspace";

export interface ServicesEntry {
  status: "loading" | "loaded" | "error";
  /** Last list that loaded (kept while reloading and after a failed reload). */
  services: GrpcService[];
  /** `reflection v1`, `reflection v1alpha` or `proto files`. */
  source: string | null;
  error: string | null;
}

/** One call of a tab (the latest), live while it runs and kept afterwards. */
export interface GrpcCall {
  /** Session id of a stream; a fresh id per call so late events of an earlier one are ignored. */
  id: string;
  method: GrpcMethod | null;
  streaming: boolean;
  state: "starting" | "open" | "ended";
  startedAt: number;
  /** Sent and received, in order (the newest MAX_MESSAGES). */
  messages: GrpcMessage[];
  /** Messages dropped from the front of `messages`: `dropped + index` is a message's stable number. */
  dropped: number;
  headers: Header[];
  trailers: Header[];
  status: GrpcStatus | null;
  timing: Timing | null;
  requestHeaders: Header[];
  remoteAddr: string | null;
  /** Messages that could not be decoded, send failures (the newest MAX_ERRORS). */
  errors: string[];
  /** The client side was ended (half-close). */
  clientEnded: boolean;
  unresolved: string[];
}

interface GrpcState {
  /** By sourceKey(). */
  services: Record<string, ServicesEntry>;
  /** By tab id. */
  calls: Record<string, GrpcCall>;
}

export const useGrpc = create<GrpcState>(() => ({ services: {}, calls: {} }));
const set = useGrpc.setState;
const get = useGrpc.getState;

export const MAX_MESSAGES = 5000;
const MAX_ERRORS = 20;

// ---- services ---------------------------------------------------------------

/** Where a request's services come from: its .proto files, else reflection through the URL (per environment). */
export function sourceKey(request: Request, environment: string | null | undefined): string {
  const files = (request.grpc?.protoFiles ?? []).filter((f) => f.trim());
  if (files.length) return `files\n${files.join("\n")}\n${(request.grpc?.importPaths ?? []).join("\n")}`;
  return `url\n${environment ?? ""}\n${request.url.trim()}`;
}

export function usesProtoFiles(request: Request): boolean {
  return (request.grpc?.protoFiles ?? []).some((f) => f.trim());
}

function currentKey(request: Request): string {
  return sourceKey(request, useWorkspace.getState().info?.activeEnvironment);
}

/** Load (or reload) the services of a tab's request. */
export async function loadServices(tab: Tab, refresh = false): Promise<ServicesEntry> {
  const key = currentKey(tab.draft);
  set((s) => {
    const previous = s.services[key];
    const entry: ServicesEntry = { services: previous?.services ?? [], source: previous?.source ?? null, status: "loading", error: null };
    return { services: { ...s.services, [key]: entry } };
  });
  try {
    const r = await api.grpcDescribe(tab.draft, tab.path, refresh);
    const entry: ServicesEntry = { status: "loaded", services: r.services, source: r.source, error: null };
    set((s) => ({ services: { ...s.services, [key]: entry } }));
    return entry;
  } catch (e) {
    const entry: ServicesEntry = { ...(get().services[key] ?? { services: [], source: null }), status: "error", error: errorMessage(e) };
    set((s) => ({ services: { ...s.services, [key]: entry } }));
    return entry;
  }
}

export function servicesFor(request: Request, environment: string | null | undefined): ServicesEntry | undefined {
  return get().services[sourceKey(request, environment)];
}

export function findMethod(services: GrpcService[], path: string): GrpcMethod | null {
  const want = path.trim().replace(/^\//, "");
  for (const s of services) {
    const m = s.methods.find((x) => x.path === want);
    if (m) return m;
  }
  return null;
}

/** Services with the methods matching `query` (method, service or type names; all words must match). */
export function filterServices(services: GrpcService[], query: string): GrpcService[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (!words.length) return services;
  return services
    .map((s) => ({
      ...s,
      methods: s.methods.filter((m) => {
        const text = `${m.path} ${m.inputType} ${m.outputType}`.toLowerCase();
        return words.every((w) => text.includes(w));
      }),
    }))
    .filter((s) => s.methods.length > 0);
}

export type CallKind = "unary" | "server" | "client" | "bidi";

export function callKind(m: Pick<GrpcMethod, "clientStreaming" | "serverStreaming">): CallKind {
  if (m.clientStreaming && m.serverStreaming) return "bidi";
  if (m.clientStreaming) return "client";
  return m.serverStreaming ? "server" : "unary";
}

export const CALL_KIND_LABEL: Record<CallKind, string> = {
  unary: "Unary",
  server: "Server stream",
  client: "Client stream",
  bidi: "Bidi stream",
};

/** `package.Service/Method` → `Service/Method` for compact display. */
export function shortMethod(path: string): string {
  const [service, method] = path.split("/");
  if (!method) return path;
  return `${service.split(".").pop()}/${method}`;
}

/** Pick a method; an empty (or untouched) message becomes the method's example. */
export function selectMethod(tabId: string, method: GrpcMethod) {
  updateDraft(tabId, (r) => {
    const text = r.body?.text?.trim() ?? "";
    const blank = text === "" || text === "{}";
    return { ...r, method: method.path, body: { ...(r.body ?? { type: "json" }), type: "json", text: blank ? method.example : (r.body?.text ?? "") } };
  });
}

/** A file path relative to the workspace root when it is inside it (portable through Git), else as given. */
export function workspaceRelative(file: string, root: string | null | undefined): string {
  if (!root) return file;
  const norm = (p: string) => p.replace(/\\/g, "/").replace(/\/+$/, "");
  const f = norm(file);
  const r = norm(root);
  // Windows paths (drive letters, UNC shares) compare case-insensitively.
  const windows = /^([a-z]:\/|\/\/[^/])/i.test(r);
  const inside = windows ? f.toLowerCase().startsWith(`${r.toLowerCase()}/`) : f.startsWith(`${r}/`);
  return inside ? f.slice(r.length + 1) : file;
}

// ---- status -----------------------------------------------------------------

/** Tone of a status: OK green, caller mistakes amber, server/transport failures red. */
export function grpcTone(code: number): Tone {
  if (code === 0) return "success";
  if (code === 1) return "muted";
  // INVALID_ARGUMENT, NOT_FOUND, ALREADY_EXISTS, PERMISSION_DENIED, FAILED_PRECONDITION, OUT_OF_RANGE, UNAUTHENTICATED
  if ([3, 5, 6, 7, 9, 11, 16].includes(code)) return "warning";
  return "danger";
}

export const STATUS_MEANING: Record<string, string> = {
  OK: "The call succeeded.",
  CANCELLED: "The call was cancelled (usually by the caller).",
  UNKNOWN: "An unknown error, or an answer that is not gRPC.",
  INVALID_ARGUMENT: "The request is invalid (independent of the server's state).",
  DEADLINE_EXCEEDED: "The deadline passed before the call finished (Settings → timeout).",
  NOT_FOUND: "Something the request refers to was not found.",
  ALREADY_EXISTS: "What the request wants to create already exists.",
  PERMISSION_DENIED: "The caller may not do this.",
  RESOURCE_EXHAUSTED: "Out of a resource: a quota, rate limit or message size.",
  FAILED_PRECONDITION: "The system is not in a state that allows this.",
  ABORTED: "Aborted, typically because of a concurrency conflict.",
  OUT_OF_RANGE: "A value is out of the valid range.",
  UNIMPLEMENTED: "The server does not implement this method.",
  INTERNAL: "An internal error in the server (or a broken answer).",
  UNAVAILABLE: "The service is unavailable (connection failed or was lost); retrying may help.",
  DATA_LOSS: "Unrecoverable data loss or corruption.",
  UNAUTHENTICATED: "Missing or invalid credentials (check Auth and Metadata).",
};

// ---- calls ------------------------------------------------------------------

function newCall(id: string, method: GrpcMethod | null, streaming: boolean): GrpcCall {
  return {
    id,
    method,
    streaming,
    state: "starting",
    startedAt: Date.now(),
    messages: [],
    dropped: 0,
    headers: [],
    trailers: [],
    status: null,
    timing: null,
    requestHeaders: [],
    remoteAddr: null,
    errors: [],
    clientEnded: false,
    unresolved: [],
  };
}

/** A finished unary call. */
export function callFromInvoke(call: GrpcCall, r: GrpcInvokeResult): GrpcCall {
  return {
    ...call,
    state: "ended",
    messages: r.messages,
    headers: r.headers,
    trailers: r.trailers,
    status: r.status,
    timing: r.timing,
    requestHeaders: r.requestHeaders,
    remoteAddr: r.remoteAddr,
    errors: r.warnings,
    clientEnded: true,
    unresolved: r.unresolved,
  };
}

/** Show a unary call made elsewhere (by an AI agent) in a tab. */
export function showInvokeResult(tabId: string, r: GrpcInvokeResult): GrpcCall {
  const call = callFromInvoke(newCall(newId(), null, false), r);
  set((s) => ({ calls: { ...s.calls, [tabId]: call } }));
  return call;
}

/** Apply stream events to a call; a burst of messages copies the list once (fast streams send thousands). */
export function applyEvents(call: GrpcCall, events: GrpcEvent[]): GrpcCall {
  let next = call;
  let messages: GrpcMessage[] | null = null;
  let errors: string[] | null = null;
  for (const event of events) {
    switch (event.type) {
      case "headers":
        next = { ...next, headers: event.headers };
        break;
      case "message":
        (messages ??= [...call.messages]).push(event.message);
        break;
      case "error":
        (errors ??= [...call.errors]).push(event.message);
        break;
      case "end":
        next = { ...next, state: "ended", status: event.status, trailers: event.trailers, timing: event.timing };
        break;
    }
  }
  if (messages) {
    const over = Math.max(0, messages.length - MAX_MESSAGES);
    next = { ...next, messages: over ? messages.slice(over) : messages, dropped: call.dropped + over };
  }
  if (errors) next = { ...next, errors: errors.slice(-MAX_ERRORS) };
  return next;
}

export function applyEvent(call: GrpcCall, event: GrpcEvent): GrpcCall {
  return applyEvents(call, [event]);
}

function setCall(tabId: string, fn: (c: GrpcCall) => GrpcCall) {
  set((s) => (s.calls[tabId] ? { calls: { ...s.calls, [tabId]: fn(s.calls[tabId]) } } : s));
}

/** Resolves the running stream's Send (its loading state) when its `end` event arrives. */
const endWaiters = new Map<string, (call: GrpcCall) => void>();

function methodFor(tab: Tab): GrpcMethod | null {
  const entry = get().services[currentKey(tab.draft)];
  return entry ? findMethod(entry.services, tab.draft.method) : null;
}

/** Send: a unary call, or a stream that stays "loading" until it ends. */
export async function runCall(tab: Tab): Promise<GrpcCall> {
  if (!tab.draft.method.trim()) {
    throw new RpcError({ code: "invalidInput", message: "Pick a method first (left of the URL).", networkKind: null });
  }
  let method = methodFor(tab);
  if (!method) {
    // Unknown streaming type: load the services (usually cached by the backend).
    const entry = await loadServices(tab);
    method = findMethod(entry.services, tab.draft.method);
  }
  const streaming = !!method && (method.clientStreaming || method.serverStreaming);
  const id = newId();
  set((s) => ({ calls: { ...s.calls, [tab.id]: newCall(id, method, streaming) } }));
  if (!streaming) {
    const r = await api.grpcInvoke(tab.id, tab.draft, tab.path);
    const call = callFromInvoke(get().calls[tab.id] ?? newCall(id, method, false), r);
    if (get().calls[tab.id]?.id === id) set((s) => ({ calls: { ...s.calls, [tab.id]: call } }));
    return call;
  }
  const ended = new Promise<GrpcCall>((resolve) => endWaiters.set(id, resolve));
  try {
    const r = await api.grpcStart(id, tab.id, tab.draft, tab.path);
    setCall(tab.id, (c) =>
      c.id === id ? { ...c, method: r.method, state: c.state === "ended" ? "ended" : "open", clientEnded: !r.method.clientStreaming, ...opened(r.opened), unresolved: r.unresolved } : c,
    );
  } catch (e) {
    endWaiters.delete(id);
    setCall(tab.id, (c) => (c.id === id ? { ...c, state: "ended" } : c));
    throw e;
  }
  return ended;
}

function opened(o: GrpcOpened): Pick<GrpcCall, "requestHeaders" | "remoteAddr"> {
  return { requestHeaders: o.requestHeaders, remoteAddr: o.remoteAddr };
}

registerOneShot("grpc", async (tab) => ({ kind: "grpc", result: await runCall(tab) }));

/** The open stream of a tab (client-streaming calls take more messages). */
export function openCall(tabId: string): GrpcCall | null {
  const call = get().calls[tabId];
  return call && call.streaming && call.state !== "ended" ? call : null;
}

/** Send the message editor's JSON on the tab's open stream. */
export async function sendMessage(tabId: string, text: string) {
  const call = openCall(tabId);
  if (!call || call.clientEnded) return;
  try {
    await api.grpcSend(call.id, text);
  } catch (e) {
    toast("error", "Send failed", errorMessage(e));
  }
}

/** Half-close: tell the server no more messages follow. */
export async function endStream(tabId: string) {
  const call = openCall(tabId);
  if (!call || call.clientEnded) return;
  setCall(tabId, (c) => (c.id === call.id ? { ...c, clientEnded: true } : c));
  await api.grpcEnd(call.id).catch((e) => toast("error", "Could not end the stream", errorMessage(e)));
}

/** Stop the tab's call. A stream whose `end` event never comes is closed here after a while. */
export async function cancelCall(tabId: string) {
  const call = get().calls[tabId];
  await api.cancel(tabId).catch(() => {});
  if (!call?.streaming || call.state === "ended") return;
  await api.grpcCancel(call.id).catch(() => {});
  setTimeout(() => {
    const waiter = endWaiters.get(call.id);
    if (!waiter) return;
    endWaiters.delete(call.id);
    const status: GrpcStatus = { code: 1, name: "CANCELLED", message: "Cancelled", details: null, local: true };
    setCall(tabId, (c) => (c.id === call.id ? { ...c, state: "ended", status } : c));
    waiter(get().calls[tabId] ?? call);
  }, 3000);
}

// ---- stream events (batched) -------------------------------------------------

let pending: { sessionId: string; event: GrpcEvent }[] = [];
let flushTimer: ReturnType<typeof setTimeout> | null = null;

function flush() {
  flushTimer = null;
  const batch = pending;
  pending = [];
  if (!batch.length) return;
  const ended: GrpcCall[] = [];
  set((s) => {
    const calls = { ...s.calls };
    for (const [tabId, call] of Object.entries(calls)) {
      const mine = batch.filter((b) => b.sessionId === call.id);
      if (!mine.length) continue;
      const next = applyEvents(call, mine.map((b) => b.event));
      calls[tabId] = next;
      if (next.state === "ended") ended.push(next);
    }
    return { calls };
  });
  for (const call of ended) {
    const waiter = endWaiters.get(call.id);
    endWaiters.delete(call.id);
    waiter?.(call);
  }
}

function handleEvent(e: StreamEvent) {
  if (e.type !== "grpc") return;
  pending.push({ sessionId: e.sessionId, event: e.event });
  if (!flushTimer) flushTimer = setTimeout(flush, 40);
}

onEvent(handleEvent);

// A closed tab's stream is cancelled (the URL bar's Cancel is gone with it). Any change of the tab
// list counts: a tab can be replaced without the count going down (e.g. while tabs are restored).
useTabs.subscribe((s, prev) => {
  if (s.tabs === prev.tabs) return;
  const calls = get().calls;
  const open = new Set(s.tabs.map((t) => t.id));
  const gone = Object.keys(calls).filter((tabId) => !open.has(tabId));
  if (!gone.length) return;
  for (const tabId of gone) {
    const call = calls[tabId];
    if (call.streaming && call.state !== "ended") void api.grpcCancel(call.id).catch(() => {});
    // Its Send promise would otherwise wait forever: the end event finds no call.
    const waiter = endWaiters.get(call.id);
    endWaiters.delete(call.id);
    waiter?.(call);
  }
  set((st) => ({ calls: Object.fromEntries(Object.entries(st.calls).filter(([tabId]) => open.has(tabId))) }));
});

// Loaded services belong to a workspace (its relative .proto paths and environments).
useWorkspace.subscribe((s, prev) => {
  if (s.info?.path !== prev.info?.path) set({ services: {} });
});

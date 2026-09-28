// MCP requests: the session a tab opens (Connect) to see what the server offers and every
// message, and the trust a program needs before a workspace may start it. A call (Send) is sent
// like any request (scripts, tests, history); the backend makes it on the tab's session while
// that is open to the same server (the session's id is the tab's id), otherwise in one go.
import { create } from "zustand";
import type { McpCallKind } from "../bindings/McpCallKind";
import type { McpCatalog } from "../bindings/McpCatalog";
import type { McpEvent } from "../bindings/McpEvent";
import type { McpServerInfo } from "../bindings/McpServerInfo";
import type { McpTransport } from "../bindings/McpTransport";
import type { Request } from "../bindings/Request";
import type { StreamEvent } from "../bindings/StreamEvent";
import { onEvent } from "../lib/events";
import { api, errorMessage, RpcError } from "../lib/rpc";
import { confirm } from "./dialogs";
import { registerRecovery, type Tab, updateDraft, useTabs } from "./tabs";

export interface McpLogEntry {
  /** Stable number (for keys and expanding). */
  seq: number;
  event: McpEvent;
  /** When it arrived here (events without their own time). */
  at: number;
}

export interface McpSession {
  status: "connecting" | "open" | "closed";
  info: McpServerInfo | null;
  catalog: McpCatalog | null;
  catalogLoading: boolean;
  /** Why connecting failed or the session ended. */
  error: string | null;
  /** Messages, program log lines and notices (the newest MAX_LOG). */
  log: McpLogEntry[];
  /** Entries dropped from the front of `log`. */
  dropped: number;
  /** The address and transport it was opened for: sends go elsewhere once they change. */
  target: string;
}

export const useMcp = create<{ sessions: Record<string, McpSession> }>(() => ({ sessions: {} }));
const set = useMcp.setState;
const get = useMcp.getState;

export const MAX_LOG = 2000;

// ---- helpers (pure) ----------------------------------------------------------

/** Whether a request's server is a program the app starts (stdio), like the backend decides. */
export function isProgram(address: string, transport: McpTransport | undefined): boolean {
  const t = transport ?? "auto";
  return t === "stdio" || (t === "auto" && !/^https?:\/\//i.test(address.trim()));
}

/** What a session is opened for: sends reuse it while the request still goes there. */
export function targetOf(request: Request): string {
  return `${request.mcp?.transport ?? "auto"}\n${request.url.trim()}`;
}

export interface CatalogItem {
  /** The tool or prompt name, the resource URI or URI template. */
  name: string;
  title: string;
  description: string;
  /** A resource template (its `{parts}` come from the arguments). */
  template: boolean;
  raw: Record<string, unknown>;
}

const str = (v: unknown): string => (typeof v === "string" ? v : "");

/** What a call of `kind` can name, from the catalog. */
export function catalogItems(catalog: McpCatalog | null | undefined, kind: McpCallKind): CatalogItem[] {
  if (!catalog) return [];
  const item = (raw: Record<string, unknown>, name: string, template = false): CatalogItem => ({
    name,
    title: str(raw.title) || str((raw.annotations as Record<string, unknown> | undefined)?.title),
    description: str(raw.description),
    template,
    raw,
  });
  if (kind === "tool") return catalog.tools.map((t) => item(t, str(t.name)));
  if (kind === "prompt") return catalog.prompts.map((p) => item(p, str(p.name)));
  // Resources are named by their URI; their name reads as the title.
  const resource = (raw: Record<string, unknown>, uri: string, template = false) => {
    const found = item(raw, uri, template);
    return { ...found, title: found.title || (str(raw.name) !== uri ? str(raw.name) : "") };
  };
  return [...catalog.resources.map((r) => resource(r, str(r.uri))), ...catalog.resourceTemplates.map((r) => resource(r, str(r.uriTemplate), true))];
}

export interface ArgumentInfo {
  name: string;
  type: string;
  required: boolean;
  description: string;
}

/** The arguments a tool (from its input schema), prompt or resource template takes. */
export function argumentsOf(item: CatalogItem | undefined, kind: McpCallKind): ArgumentInfo[] {
  if (!item) return [];
  if (kind === "prompt") {
    const args = Array.isArray(item.raw.arguments) ? (item.raw.arguments as Record<string, unknown>[]) : [];
    return args.map((a) => ({ name: str(a.name), type: "string", required: a.required === true, description: str(a.description) }));
  }
  if (kind === "resource") {
    return templateParams(item.name).map((name) => ({ name, type: "string", required: true, description: "" }));
  }
  const schema = (item.raw.inputSchema ?? {}) as Record<string, unknown>;
  const properties = (schema.properties ?? {}) as Record<string, Record<string, unknown>>;
  const required = Array.isArray(schema.required) ? (schema.required as string[]) : [];
  return Object.entries(properties).map(([name, p]) => ({
    name,
    type: typeName(p),
    required: required.includes(name),
    description: str(p.description),
  }));
}

function typeName(p: Record<string, unknown>): string {
  if (Array.isArray(p.enum)) return p.enum.map((v) => JSON.stringify(v)).join(" | ");
  if (Array.isArray(p.type)) return p.type.join(" | ");
  if (p.type === "array" && p.items && typeof p.items === "object") return `${typeName(p.items as Record<string, unknown>)}[]`;
  return str(p.type) || "any";
}

/** The `{name}` parts of a URI template (RFC 6570 operators dropped). */
export function templateParams(template: string): string[] {
  return [...template.matchAll(/\{([^}]+)\}/g)].flatMap((m) => m[1].replace(/^[+#./;?&]/, "").split(",")).map((n) => n.replace(/\*$/, "").trim()).filter(Boolean);
}

/** Arguments to start from: the required ones (all of them when none is required), with a
 *  value of their type (a schema's default or first enum value when it has one). */
export function argumentSkeleton(item: CatalogItem | undefined, kind: McpCallKind): string {
  const args = argumentsOf(item, kind);
  const picked = args.some((a) => a.required) ? args.filter((a) => a.required) : args;
  const properties = ((item?.raw.inputSchema as Record<string, unknown> | undefined)?.properties ?? {}) as Record<string, Record<string, unknown>>;
  const value = (a: ArgumentInfo): unknown => {
    const p = properties[a.name] ?? {};
    if (p.default !== undefined) return p.default;
    if (Array.isArray(p.enum) && p.enum.length) return p.enum[0];
    const type = Array.isArray(p.type) ? p.type[0] : p.type;
    switch (kind === "tool" ? type : "string") {
      case "number":
      case "integer":
        return 0;
      case "boolean":
        return false;
      case "array":
        return [];
      case "object":
        return {};
      default:
        return "";
    }
  };
  return JSON.stringify(Object.fromEntries(picked.map((a) => [a.name, value(a)])), null, 2);
}

/** Whether arguments text is still untouched (empty, or an empty object). */
export function blankArguments(text: string | undefined): boolean {
  const t = (text ?? "").trim();
  return t === "" || /^\{\s*\}$/.test(t);
}

// ---- sessions ------------------------------------------------------------------

function blank(target: string): McpSession {
  return { status: "connecting", info: null, catalog: null, catalogLoading: false, error: null, log: [], dropped: 0, target };
}

function patch(tabId: string, fn: (s: McpSession) => Partial<McpSession>) {
  set((st) => (st.sessions[tabId] ? { sessions: { ...st.sessions, [tabId]: { ...st.sessions[tabId], ...fn(st.sessions[tabId]) } } } : st));
}

/** Connect attempts per tab: only the latest one may change the tab's session. */
const attempts = new Map<string, number>();

/** Open the tab's session and read what the server offers. */
export async function connectMcp(tab: Tab): Promise<void> {
  if (!tab.draft.url.trim()) throw new RpcError({ code: "invalidInput", message: "Enter the server's URL or the command that starts it", networkKind: null });
  const target = targetOf(tab.draft);
  const attempt = (attempts.get(tab.id) ?? 0) + 1;
  attempts.set(tab.id, attempt);
  const current = () => attempts.get(tab.id) === attempt;
  const previous = get().sessions[tab.id];
  // The log carries on across reconnects (it shows what happened before).
  set((st) => ({ sessions: { ...st.sessions, [tab.id]: { ...blank(target), log: previous?.log ?? [], dropped: previous?.dropped ?? 0 } } }));
  try {
    let opened;
    try {
      opened = await api.mcpConnect(tab.id, tab.draft, tab.path);
    } catch (e) {
      if (!(e instanceof RpcError && e.code === "untrustedProgram" && current() && (await allowProgram(tab.draft, tab.path)))) throw e;
      // Disconnected, reconnected or closed while the question was open: don't start it.
      if (!current() || get().sessions[tab.id]?.status !== "connecting") return;
      opened = await api.mcpConnect(tab.id, tab.draft, tab.path);
    }
    if (!current()) return; // a newer connect replaced this session
    if (get().sessions[tab.id]?.status !== "connecting") {
      // Disconnected (or the tab closed) while connecting: end what just opened.
      void api.mcpClose(tab.id).catch(() => {});
      return;
    }
    patch(tab.id, () => ({ status: "open", info: opened.info }));
    await refreshCatalog(tab.id);
  } catch (e) {
    if (current() && get().sessions[tab.id]?.status === "connecting") patch(tab.id, () => ({ status: "closed", error: errorMessage(e) }));
    throw e;
  }
}

export async function disconnectMcp(tabId: string) {
  if (!get().sessions[tabId]) return;
  patch(tabId, () => ({ status: "closed" }));
  await api.mcpClose(tabId).catch(() => {});
}

export async function refreshCatalog(tabId: string) {
  if (get().sessions[tabId]?.status !== "open") return;
  patch(tabId, () => ({ catalogLoading: true }));
  try {
    const catalog = await api.mcpCatalog(tabId);
    patch(tabId, () => ({ catalog, catalogLoading: false }));
  } catch (e) {
    patch(tabId, (s) => ({ catalogLoading: false, catalog: { ...(s.catalog ?? { tools: [], resources: [], resourceTemplates: [], prompts: [] }), problems: [errorMessage(e)] } }));
  }
}

export function clearLog(tabId: string) {
  patch(tabId, (s) => ({ log: [], dropped: s.dropped + s.log.length }));
}

/** Call this tool, resource or prompt: the arguments start from its schema when still blank. */
export function selectCall(tabId: string, kind: McpCallKind, item: CatalogItem) {
  updateDraft(tabId, (r) => {
    const mcp = r.mcp ?? {};
    const sameKind = (mcp.call ?? "tool") === kind;
    // Picking what is already chosen keeps what was typed (and leaves the request unchanged).
    if (sameKind && mcp.name === item.name && !blankArguments(mcp.arguments)) return r;
    const args = argumentsOf(item, kind).length ? argumentSkeleton(item, kind) : "{}";
    return { ...r, mcp: { ...mcp, call: kind, name: item.name, arguments: args } };
  });
}

// ---- trusting programs -----------------------------------------------------------

/** Ask the user whether this workspace may start the request's program; remembers a yes. */
export async function allowProgram(request: Request, path: string | null): Promise<boolean> {
  const program = await api.mcpProgram(request, path);
  if (program.trusted) return true;
  const ok = await confirm({
    title: "Start this program?",
    message:
      "This MCP request starts a program on your computer. Workspaces can come from other people through Git, so allow it only if you know what it runs. Zorvik asks again if the command, folder or environment changes.",
    details: [`Command: ${program.command}`, `Folder: ${program.cwd}`, ...program.env.map((e) => `Environment: ${e.key}=${e.value}`)],
    confirmLabel: "Allow and start",
    danger: true,
  });
  if (!ok) return false;
  await api.mcpTrust(request, path);
  return true;
}

registerRecovery("untrustedProgram", (tab) => allowProgram(tab.draft, tab.path));

// ---- events (batched) ------------------------------------------------------------

let seq = 0;
let pending: { tabId: string; event: McpEvent }[] = [];
let flushTimer: ReturnType<typeof setTimeout> | null = null;
const LIST_CHANGED = /^notifications\/(tools|resources|prompts)\/list_changed$/;

function flush() {
  flushTimer = null;
  const batch = pending;
  pending = [];
  if (!batch.length) return;
  const refresh = new Set<string>();
  set((st) => {
    const sessions = { ...st.sessions };
    for (const tabId of new Set(batch.map((b) => b.tabId))) {
      const session = sessions[tabId];
      if (!session) continue;
      const mine = batch.filter((b) => b.tabId === tabId).map((b) => b.event);
      const log = [...session.log, ...mine.map((event) => ({ seq: ++seq, event, at: Date.now() }))];
      const over = Math.max(0, log.length - MAX_LOG);
      let next: McpSession = { ...session, log: over ? log.slice(over) : log, dropped: session.dropped + over };
      for (const event of mine) {
        // While connecting, a "closed" is the replaced session's.
        if (event.type === "closed" && next.status !== "connecting") next = { ...next, status: "closed", error: event.reason };
        if (event.type === "message" && event.direction === "received" && event.method && LIST_CHANGED.test(event.method)) refresh.add(tabId);
      }
      sessions[tabId] = next;
    }
    return { sessions };
  });
  for (const tabId of refresh) void refreshCatalog(tabId);
}

onEvent((e: StreamEvent) => {
  if (e.type !== "mcp") return;
  pending.push({ tabId: e.connId, event: e.event });
  if (!flushTimer) flushTimer = setTimeout(flush, 40);
});

// A closed tab's session ends with it.
useTabs.subscribe((s, prev) => {
  if (s.tabs === prev.tabs) return;
  const open = new Set(s.tabs.map((t) => t.id));
  const gone = Object.keys(get().sessions).filter((tabId) => !open.has(tabId));
  if (!gone.length) return;
  for (const tabId of gone) {
    attempts.delete(tabId);
    void api.mcpClose(tabId).catch(() => {});
  }
  set((st) => ({ sessions: Object.fromEntries(Object.entries(st.sessions).filter(([tabId]) => open.has(tabId))) }));
});

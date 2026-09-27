// AI agents (docs/architecture.md, "AI agents"): who is connected, what they did, the questions waiting
// for the user, and following what they work on (settings.agents.follow).
import { getCurrentWindow, UserAttentionType } from "@tauri-apps/api/window";
import { create } from "zustand";
import type { AgentActivity } from "../bindings/AgentActivity";
import type { AgentConfirm } from "../bindings/AgentConfirm";
import type { AgentSession } from "../bindings/AgentSession";
import type { AgentState } from "../bindings/AgentState";
import type { AgentTarget } from "../bindings/AgentTarget";
import type { GrpcInvokeResult } from "../bindings/GrpcInvokeResult";
import type { Request } from "../bindings/Request";
import type { SendResult } from "../bindings/SendResult";
import { onEvent } from "../lib/events";
import { api, errorMessage, isTauri } from "../lib/rpc";
import { openCall, showInvokeResult } from "./grpc";
import { attachRun } from "./runner";
import { useSettings } from "./settings";
import {
  activate as activateTab,
  isRequestTab,
  openDraft,
  openLoadTest,
  openRequest,
  openRunner,
  openServer,
  resetTabs,
  restoreTabs,
  updateTab,
  useTabs,
} from "./tabs";
import { toast } from "./toasts";
import { openModal, toggleExpanded, useUi } from "./ui";
import { adoptWorkspace, refreshVariables, reloadWorkspace, useWorkspace } from "./workspace";

interface AgentsState {
  sessions: AgentSession[];
  /** Oldest first. */
  activity: AgentActivity[];
  /** Questions for the user, oldest first (the dialog shows the first). */
  pending: AgentConfirm[];
}

/** Entries kept (the backend keeps as many). */
const MAX_ACTIVITY = 200;

export const useAgents = create<AgentsState>(() => ({ sessions: [], activity: [], pending: [] }));
const set = useAgents.setState;

/** Questions closed while `loadAgents` waits for the snapshot (it must not bring them back). */
let closedDuringLoad: Set<string> | null = null;

/** Catch up with the backend (startup, window reload); events that arrive meanwhile are kept. */
export async function loadAgents() {
  const closed = (closedDuringLoad = new Set());
  try {
    const state = await api.call<AgentState>("agent.state");
    set((s) => {
      const known = new Set(state.pending.map((p) => p.id));
      const pending = [...state.pending, ...s.pending.filter((p) => !known.has(p.id))].filter((p) => !closed.has(p.id));
      // Entries from events are newer than the snapshot's.
      const updated = new Map(s.activity.map((a) => [a.id, a]));
      const activity = state.activity.map((a) => updated.get(a.id) ?? a);
      const inSnapshot = new Set(activity.map((a) => a.id));
      activity.push(...s.activity.filter((a) => !inSnapshot.has(a.id)));
      return { sessions: state.sessions, activity: activity.slice(-MAX_ACTIVITY), pending };
    });
  } catch {
    /* backend unavailable */
  } finally {
    if (closedDuringLoad === closed) closedDuringLoad = null;
  }
}

/** Answer the agent's question (the dialog closes for every window with `confirmClosed`). */
export async function answer(id: string, allow: boolean, forSession = false) {
  set((s) => ({ pending: s.pending.filter((p) => p.id !== id) }));
  try {
    await api.call("agent.answer", { id, allow, forSession });
  } catch (e) {
    toast("info", "That question had already expired", errorMessage(e));
  }
}

/** Disconnect one agent, or all when no id is given. */
export async function disconnectAgent(sessionId?: string) {
  try {
    await api.call("agent.disconnect", { sessionId: sessionId ?? null });
  } catch (e) {
    toast("error", "Could not disconnect", errorMessage(e));
  }
}

const following = () => useSettings.getState().settings?.agents.follow ?? true;

/** The user typed a moment ago: following opens tabs behind the current one then. */
let lastKey = 0;
if (typeof window !== "undefined") window.addEventListener("keydown", () => (lastKey = Date.now()), true);
const typing = () => Date.now() - lastKey < 2500;

/** The tab unsaved requests of agents show in (one, reused while the user leaves it alone). */
let scratch: { tabId: string; draft: string } | null = null;

/** The tab is busy with the user's own request (a send in flight, an open gRPC stream). */
function busy(tabId: string) {
  const tab = useTabs.getState().tabs.find((t) => t.id === tabId);
  return !isRequestTab(tab) || tab.response.status === "loading" || !!openCall(tabId);
}

/** Expand the folders above `path` (and `path` itself when it is a folder). */
function reveal(path: string, self: boolean) {
  const parts = path.split("/").filter(Boolean);
  const upTo = self ? parts.length : parts.length - 1;
  for (let i = 1; i <= upTo; i++) toggleExpanded(parts.slice(0, i).join("/"), true);
}

/** Open what an agent worked on. `explicit`: the agent or the user asked for it. */
export async function openTarget(target: AgentTarget, explicit = false) {
  const activate = explicit || !typing();
  switch (target.type) {
    case "request":
      reveal(target.path, false);
      if (explicit) useUi.setState({ sidebarTab: "collection" });
      return openRequest(target.path, { activate });
    case "folder":
      if (target.path) reveal(target.path, true);
      if (explicit) useUi.setState({ sidebarTab: "collection" });
      return;
    case "loadTest":
      if (explicit) useUi.setState({ sidebarTab: "loadtests" });
      return openLoadTest(target.id);
    case "server":
      if (explicit) useUi.setState({ sidebarTab: "servers" });
      return openServer(target.id);
    case "runner":
      openRunner(target.folder, target.name, { activate });
      return;
    case "environments":
      // A dialog only when asked for: following must not pop one up mid-typing.
      if (explicit) openModal({ type: "environments" });
      return;
  }
}

/** A request the agent sent: its tab (a new unsaved one for an unsaved request) shows the response. */
async function showResponse(path: string | null, request: Request, kind: string, result: unknown) {
  const activate = !typing();
  let tabId: string | undefined;
  if (path) {
    reveal(path, false);
    await openRequest(path, { activate });
    tabId = useTabs.getState().tabs.find((t) => isRequestTab(t) && t.path === path)?.id;
  }
  // An unsaved request, or the saved one's tab is busy with the user's own send: the scratch tab.
  if (!tabId || busy(tabId)) {
    const draft = structuredClone(request);
    const reuse = scratch && useTabs.getState().tabs.find((t) => t.id === scratch!.tabId);
    // Reused only while the user hasn't edited it (or sent from it).
    if (reuse && isRequestTab(reuse) && !reuse.path && JSON.stringify(reuse.draft) === scratch!.draft && !busy(reuse.id)) {
      tabId = reuse.id;
      updateTab(tabId, () => ({ draft }));
      if (activate) activateTab(tabId);
    } else {
      tabId = openDraft(draft, { activate });
    }
    scratch = { tabId, draft: JSON.stringify(draft) };
  }
  const at = Date.now();
  if (kind === "http") updateTab(tabId, () => ({ response: { status: "done", result: result as SendResult, at } }));
  else if (kind === "grpc") {
    const call = showInvokeResult(tabId, result as GrpcInvokeResult);
    updateTab(tabId, () => ({ response: { status: "other", kind: "grpc", result: call, at } }));
  } else updateTab(tabId, () => ({ response: { status: "other", kind, result, at } }));
  // Its scripts may have set variables.
  void refreshVariables();
}

/** The agent opened another workspace, or switched the environment. */
async function workspaceChanged() {
  const info = await api.currentWorkspace().catch(() => null);
  const current = useWorkspace.getState().info?.path ?? null;
  if (info && info.path !== current) {
    // Open tabs are kept for when the user comes back to the other workspace.
    resetTabs();
    await adoptWorkspace(info);
    await restoreTabs(info.path);
  } else if (info) {
    await reloadWorkspace().catch(() => {});
  }
  await refreshVariables();
}

/** A question is waiting: bounce the Dock icon / flash the taskbar button when the window is in the background. */
function attention() {
  if (!isTauri || document.hasFocus()) return;
  void getCurrentWindow()
    .requestUserAttention(UserAttentionType.Informational)
    .catch(() => {});
}

onEvent((e) => {
  if (e.type !== "agent") return;
  const ev = e.event;
  switch (ev.type) {
    case "sessions":
      set({ sessions: ev.sessions });
      return;
    case "activity":
      set((s) => {
        const i = s.activity.findIndex((a) => a.id === ev.entry.id);
        const activity = i >= 0 ? s.activity.map((a, j) => (j === i ? ev.entry : a)) : [...s.activity, ev.entry].slice(-MAX_ACTIVITY);
        return { activity };
      });
      return;
    case "confirm":
      set((s) => (s.pending.some((p) => p.id === ev.request.id) ? s : { pending: [...s.pending, ev.request] }));
      attention();
      return;
    case "confirmClosed":
      closedDuringLoad?.add(ev.id);
      set((s) => ({ pending: s.pending.filter((p) => p.id !== ev.id) }));
      return;
    case "show":
      if (ev.explicit || following()) void openTarget(ev.target, ev.explicit);
      return;
    case "response":
      if (following()) void showResponse(ev.path, ev.request, ev.kind, ev.result);
      else void refreshVariables();
      return;
    case "run":
      if (following()) attachRun(openRunner(ev.folder, ev.run.name, { activate: !typing() }), ev.run);
      return;
    case "workspaceChanged":
      void workspaceChanged();
      return;
  }
});

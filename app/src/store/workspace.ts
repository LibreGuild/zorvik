import { useMemo } from "react";
import { create } from "zustand";
import type { DynamicVarInfo } from "../bindings/DynamicVarInfo";
import type { AppInfo } from "../bindings/AppInfo";
import type { RecentWorkspace } from "../bindings/RecentWorkspace";
import type { TreeNode } from "../bindings/TreeNode";
import type { VariableInfo } from "../bindings/VariableInfo";
import type { WorkspaceInfo } from "../bindings/WorkspaceInfo";
import { api, RpcError } from "../lib/rpc";
import { confirm } from "./dialogs";

interface WorkspaceState {
  appInfo: AppInfo | null;
  info: WorkspaceInfo | null;
  variables: VariableInfo[];
  recent: RecentWorkspace[];
  ready: boolean;
}

export const useWorkspace = create<WorkspaceState>(() => ({
  appInfo: null,
  info: null,
  variables: [],
  recent: [],
  ready: false,
}));

const set = useWorkspace.setState;
const get = useWorkspace.getState;

export async function initWorkspace() {
  const [appInfo, info, recent] = await Promise.all([
    api.appInfo(),
    api.currentWorkspace().catch(() => null),
    api.recentWorkspaces().catch(() => []),
  ]);
  set({ appInfo, info, recent, ready: true });
  if (info) await refreshVariables();
}

/** A workspace the backend opened already (an AI agent opened it). */
export async function adoptWorkspace(info: WorkspaceInfo) {
  set({ info, recent: await api.recentWorkspaces().catch(() => get().recent) });
  await refreshVariables();
}

export async function openWorkspace(path: string) {
  const info = await api.openWorkspace(path);
  set({ info, recent: await api.recentWorkspaces() });
  await refreshVariables();
}

/** Open a folder; offer to turn it into a workspace when it isn't one yet. Returns false if cancelled. */
export async function openOrInitWorkspace(path: string): Promise<boolean> {
  try {
    await openWorkspace(path);
    return true;
  } catch (e) {
    if (!(e instanceof RpcError) || e.code !== "notAWorkspace") throw e;
    const name = path.split(/[\\/]/).filter(Boolean).pop() ?? "My Workspace";
    const ok = await confirm({
      title: "Create a workspace here?",
      message: `“${path}” is not a Zorvik workspace yet. Create one in this folder? Existing files are left untouched.`,
      confirmLabel: "Create workspace",
    });
    if (!ok) return false;
    await createWorkspace(path, name);
    return true;
  }
}

export async function createWorkspace(path: string, name: string) {
  const info = await api.createWorkspace(path, name);
  set({ info, recent: await api.recentWorkspaces() });
  await refreshVariables();
}

export async function closeWorkspace() {
  await api.closeWorkspace();
  set({ info: null, variables: [], recent: await api.recentWorkspaces() });
}

// Results below are applied only if the workspace they were fetched for is still the open one
// (it may have been switched or closed while awaiting; spreading a null info would crash the UI).
const isOpen = (path: string) => get().info?.path === path;

export async function refreshTree() {
  const info = get().info;
  if (!info) return;
  const tree = await api.tree();
  if (isOpen(info.path)) set({ info: { ...get().info!, tree } });
}

export async function reloadWorkspace() {
  const current = get().info;
  if (!current) return;
  const info = await api.reloadWorkspace();
  if (!isOpen(current.path)) return;
  set({ info });
  await refreshVariables();
}

export async function refreshEnvironments() {
  const info = get().info;
  if (!info) return;
  const environments = await api.listEnvironments();
  if (!isOpen(info.path)) return;
  const active = environments.some((e) => e.id === get().info?.activeEnvironment) ? get().info!.activeEnvironment : null;
  set({ info: { ...get().info!, environments, activeEnvironment: active } });
  await refreshVariables();
}

export async function refreshVariables() {
  const info = get().info;
  if (!info) return;
  const variables = await api.variables().catch(() => []);
  if (isOpen(info.path)) set({ variables });
}

export async function setActiveEnvironment(id: string | null) {
  const path = get().info?.path;
  await api.setActiveEnvironment(id);
  if (path && isOpen(path)) set({ info: { ...get().info!, activeEnvironment: id } });
  await refreshVariables();
}

export function findNode(nodes: TreeNode[], path: string): TreeNode | null {
  for (const n of nodes) {
    if (n.path === path) return n;
    const inner = findNode(n.children, path);
    if (inner) return inner;
  }
  return null;
}

/** Dynamic variables (`$randomEmail` …) by name: description, example and the `(args)` form. */
export function useDynamicCatalog(): Map<string, DynamicVarInfo> {
  const catalog = useWorkspace((s) => s.appInfo?.dynamicCatalog);
  return useMemo(() => new Map((catalog ?? []).map((v) => [v.name, v])), [catalog]);
}

/** Variable names for highlighting/autocomplete, including dynamic ones. */
export function useVariableNames(): { names: string[]; known: Set<string> } {
  const variables = useWorkspace((s) => s.variables);
  const dynamic = useWorkspace((s) => s.appInfo?.dynamicVariables);
  return useMemo(() => {
    const names = [...variables.map((v) => v.key), ...(dynamic ?? [])];
    return { names, known: new Set(names) };
  }, [variables, dynamic]);
}

// Scenarios: the saved ones of the open workspace and their folders, the values each one's
// last run left, and starting a run (the run itself and its results live in
// store/runner.ts, like a collection run's).
import { create } from "zustand";
import type { Scenario } from "../bindings/Scenario";
import type { ScenarioNode } from "../bindings/ScenarioNode";
import type { ScenarioValue } from "../bindings/ScenarioValue";
import type { TreeNode } from "../bindings/TreeNode";
import { flattenRequests } from "../components/loadtests/model";
import { flattenScenarios, movedId, newScenario, retarget, retargetCalls, scenarioParent, startProblem } from "../components/scenarios/model";
import { api, errorMessage } from "../lib/rpc";
import { confirm, prompt } from "./dialogs";
import { attachRun, onRunFinished, runnableRequests, stopRun, useRunner } from "./runner";
import {
  isScenarioTab,
  onItemsMoved,
  onScenarioDeleted,
  onScenarioFilesChanged,
  onScenarioRenamed,
  onScenarioSaved,
  openScenario,
  type ScenarioTab,
  updateScenarioTab,
  useTabs,
} from "./tabs";
import { toast } from "./toasts";
import { toggleExpanded, useUi } from "./ui";
import { useWorkspace } from "./workspace";

interface ScenariosState {
  /** The scenarios and their folders, as the sidebar shows them. */
  tree: ScenarioNode[];
  /** Every scenario (not the folders), in sidebar order. */
  saved: ScenarioNode[];
  /** The values the last run of each scenario left, by scenario id. */
  values: Record<string, ScenarioValue[]>;
}

export const useScenarios = create<ScenariosState>(() => ({ tree: [], saved: [], values: {} }));
const set = useScenarios.setState;

const currentWorkspace = () => useWorkspace.getState().info?.path ?? "";

// ---- saved list ----------------------------------------------------------------------

export async function refreshScenarios() {
  if (!useWorkspace.getState().info) {
    set({ tree: [], saved: [] });
    return;
  }
  const ws = currentWorkspace();
  try {
    const tree = await api.scenarios();
    if (currentWorkspace() === ws) set({ tree, saved: flattenScenarios(tree) });
  } catch {
    if (currentWorkspace() === ws) set({ tree: [], saved: [] });
  }
}

/** The open workspace changed: the values of the previous one's scenarios no longer apply. */
export function resetScenarioValues() {
  set({ values: {} });
}

onScenarioFilesChanged(() => void refreshScenarios());
onScenarioSaved((oldId, newId) => {
  scenariosMoved(oldId, newId, false);
  void refreshScenarios();
});

/** Create a scenario in `folder` ("" the top level) and open it; returns its id. */
export async function createScenario(scenario: Scenario, folder = ""): Promise<string | null> {
  try {
    const id = await api.createScenario(scenario, folder);
    showScenarioFolder(folder);
    await refreshScenarios();
    await openScenario(id);
    useUi.setState({ sidebarTab: "scenarios" });
    return id;
  } catch (e) {
    toast("error", "Could not create scenario", errorMessage(e));
    return null;
  }
}

/** "New scenario" (asks for a name; no steps yet), at the top level or in `folder`. */
export async function newScenarioPrompt(folder = "") {
  const name = await prompt({ title: "New scenario", value: "Scenario", confirmLabel: "Create" });
  if (name) await createScenario(newScenario(name), folder);
}

/** "New scenario from this folder…": a step for every request under it the runner can send, in sidebar order. */
export async function scenarioFromFolder(tree: TreeNode[], folder: string, folderName: string) {
  const requests = (runnableRequests(tree, folder) ?? []).map((r) => r.path);
  if (!requests.length) {
    toast("info", "No requests to make steps of", `“${folderName}” has no requests a scenario can send.`);
    return;
  }
  await scenarioFromRequests(folderName, requests);
}

/** A new scenario that sends `requests` in order (asks for its name). */
export async function scenarioFromRequests(suggested: string, requests: string[]) {
  const name = await prompt({ title: "New scenario", value: suggested, confirmLabel: "Create" });
  if (name) await createScenario(newScenario(name, requests));
}

export async function renameScenario(node: ScenarioNode) {
  const name = await prompt({ title: "Rename scenario", value: node.name, confirmLabel: "Rename" });
  if (!name || name === node.name) return;
  try {
    const scenario = await api.readScenario(node.id);
    const saved = { ...scenario, name };
    const newId = await api.saveScenario(node.id, saved);
    onScenarioRenamed(node.id, newId, saved);
    scenariosMoved(node.id, newId, false);
    await refreshScenarios();
  } catch (e) {
    toast("error", "Could not rename scenario", errorMessage(e));
  }
}

export async function duplicateScenario(node: ScenarioNode) {
  try {
    const id = await api.duplicateScenario(node.id);
    await refreshScenarios();
    await openScenario(id);
  } catch (e) {
    toast("error", "Could not duplicate scenario", errorMessage(e));
  }
}

export async function deleteScenario(node: ScenarioNode) {
  const ok = await confirm({ title: "Delete scenario?", message: `“${node.name}” will be moved to the trash.`, confirmLabel: "Delete", danger: true });
  if (!ok) return;
  try {
    const tab = tabOf(node.id);
    if (tab && useRunner.getState().active === tab.id) await stopRun(tab.id);
    await api.deleteScenario(node.id);
    onScenarioDeleted(node.id);
    set((s) => {
      const values = { ...s.values };
      delete values[node.id];
      return { values };
    });
    await refreshScenarios();
  } catch (e) {
    toast("error", "Could not delete scenario", errorMessage(e));
  }
}

const tabOf = (scenarioId: string): ScenarioTab | undefined => useTabs.getState().tabs.find((t): t is ScenarioTab => isScenarioTab(t) && t.scenarioId === scenarioId);

// ---- folders ---------------------------------------------------------------------------

/** What `useUi.expanded` keeps a folder of scenarios open under (request folders use their path). */
export const scenarioFolderKey = (id: string) => `scenarios:${id}`;

/** Open a folder of scenarios in the sidebar, and the folders it is in. */
export function showScenarioFolder(id: string) {
  const parts = id.split("/").filter(Boolean);
  for (let i = 1; i <= parts.length; i++) toggleExpanded(scenarioFolderKey(parts.slice(0, i).join("/")), true);
}

/** "New folder", at the top level or in `parent`. */
export async function newScenarioFolder(parent = "") {
  const name = await prompt({ title: "New folder", placeholder: "Folder name", confirmLabel: "Create" });
  if (!name) return;
  try {
    const id = await api.createScenarioFolder(parent, name);
    showScenarioFolder(id);
    await refreshScenarios();
  } catch (e) {
    toast("error", "Could not create folder", errorMessage(e));
  }
}

export async function renameScenarioFolder(node: ScenarioNode) {
  const name = await prompt({ title: "Rename folder", value: node.name, confirmLabel: "Rename" });
  if (!name || name === node.name) return;
  try {
    const newId = await api.renameScenarioFolder(node.id, name);
    scenariosMoved(node.id, newId, true);
    await refreshScenarios();
  } catch (e) {
    toast("error", "Could not rename folder", errorMessage(e));
  }
}

export async function deleteScenarioFolder(node: ScenarioNode) {
  const inside = flattenScenarios(node.children ?? []);
  const what = inside.length ? ` and ${inside.length === 1 ? "the scenario" : `the ${inside.length} scenarios`} in it` : "";
  const ok = await confirm({ title: "Delete folder?", message: `“${node.name}”${what} will be moved to the trash.`, confirmLabel: "Delete", danger: true });
  if (!ok) return;
  try {
    const active = useRunner.getState().active;
    const running = useTabs.getState().tabs.find((t) => t.id === active);
    if (isScenarioTab(running) && movedId(running.scenarioId, node.id, node.id, true)) await stopRun(running.id);
    await api.deleteScenarioFolder(node.id);
    for (const s of inside) onScenarioDeleted(s.id);
    set((s) => ({ values: Object.fromEntries(Object.entries(s.values).filter(([id]) => !movedId(id, node.id, node.id, true))) }));
    await refreshScenarios();
  } catch (e) {
    toast("error", "Could not delete folder", errorMessage(e));
  }
}

/**
 * Move a scenario or a folder into the folder `parent` ("" the top level), at place `index`
 * among what is there now (last: null).
 */
export async function moveScenarioNode(node: ScenarioNode, parent: string, index: number | null) {
  const folder = !!node.folder;
  if (folder && (parent === node.id || parent.startsWith(`${node.id}/`))) return;
  const from = scenarioParent(node.id);
  if (from === parent && index === null) return;
  // Within its own folder, the places after it move up once it leaves its own.
  if (from === parent && index !== null) {
    const level = parent ? findScenarioNode(useScenarios.getState().tree, parent)?.children ?? [] : useScenarios.getState().tree;
    const at = level.findIndex((n) => n.id === node.id && !!n.folder === folder);
    if (at === index || at + 1 === index) return;
    if (at >= 0 && at < index) index -= 1;
  }
  try {
    const newId = await api.moveScenario(node.id, folder, parent, index);
    scenariosMoved(node.id, newId, folder);
    showScenarioFolder(parent);
    await refreshScenarios();
  } catch (e) {
    toast("error", "Could not move", errorMessage(e));
  }
}

/** The node with this id in the tree (a folder when `folder`). */
export function findScenarioNode(nodes: ScenarioNode[], id: string, folder = true): ScenarioNode | undefined {
  for (const n of nodes) {
    if (n.id === id && !!n.folder === folder) return n;
    const inside = n.folder ? findScenarioNode(n.children ?? [], id, folder) : undefined;
    if (inside) return inside;
  }
  return undefined;
}

/**
 * The scenario `oldId`, or (`folder`) everything in the folder `oldId`, has a new id: what is
 * kept by id follows. Their tabs, the values their last runs left, the steps of open
 * scenarios that run them (the files were changed already) and open folders.
 */
export function scenariosMoved(oldId: string, newId: string, folder: boolean) {
  if (oldId === newId) return;
  const moved = (id: string) => movedId(id, oldId, newId, folder);
  for (const tab of useTabs.getState().tabs) {
    if (!isScenarioTab(tab)) continue;
    const id = moved(tab.scenarioId);
    const draft = retargetCalls(tab.draft, moved);
    const saved = retargetCalls(tab.saved, moved);
    if (id || draft !== tab.draft || saved !== tab.saved) updateScenarioTab(tab.id, () => ({ ...(id ? { scenarioId: id, orphaned: false } : {}), draft, saved }));
  }
  set((s) => ({ values: Object.fromEntries(Object.entries(s.values).map(([id, v]) => [moved(id) ?? id, v])) }));
  if (folder) {
    const key = scenarioFolderKey(oldId);
    useUi.setState((s) => ({
      expanded: Object.fromEntries(
        Object.entries(s.expanded).map(([k, v]) => [k === key ? scenarioFolderKey(newId) : k.startsWith(`${key}/`) ? scenarioFolderKey(newId) + k.slice(key.length) : k, v]),
      ),
    }));
  }
}

// ---- values ----------------------------------------------------------------------------

/** Read the values the last run of a scenario left. */
export async function refreshScenarioValues(scenarioId: string) {
  const ws = currentWorkspace();
  try {
    const values = await api.scenarioValues(scenarioId);
    if (currentWorkspace() === ws) set((s) => ({ values: { ...s.values, [scenarioId]: values } }));
  } catch {
    /* no workspace or backend unavailable */
  }
}

/** Forget the values the last run left: all of them, or the one named `key`. */
export async function clearScenarioValues(scenarioId: string, key?: string) {
  try {
    await api.clearScenarioValues(scenarioId, key);
    await refreshScenarioValues(scenarioId);
  } catch (e) {
    toast("error", "Could not clear the values", errorMessage(e));
  }
}

onRunFinished((tabId) => {
  const tab = useTabs.getState().tabs.find((t) => t.id === tabId);
  if (isScenarioTab(tab)) void refreshScenarioValues(tab.scenarioId);
});

// ---- running ---------------------------------------------------------------------------

/**
 * Run a scenario tab's draft: all of it, up to and including the step at `to`, or `only`
 * that step with the values the last run left.
 */
export async function runScenario(tab: ScenarioTab, part: { to?: string; only?: string } = {}): Promise<boolean> {
  const { active, starting, runs } = useRunner.getState();
  if (starting) return false;
  if (active) {
    const other = runs[active]?.name ?? "Another run";
    toast("info", "A run is in progress", `Stop “${other}” first: one run at a time.`);
    return false;
  }
  const wsPath = currentWorkspace();
  useRunner.setState({ starting: tab.id });
  try {
    const started = await api.runScenario(tab.scenarioId, tab.draft, part);
    // The tab closed or the workspace changed meanwhile: nobody shows this run.
    if (currentWorkspace() !== wsPath || !useTabs.getState().tabs.some((t) => t.id === tab.id)) {
      void api.stopRun(started.runId).catch(() => {});
      return false;
    }
    attachRun(tab.id, started);
    return true;
  } catch (e) {
    toast("error", `Could not run “${tab.draft.name}”`, errorMessage(e));
    return false;
  } finally {
    useRunner.setState({ starting: null });
  }
}

/** Start or stop (Mod+Enter, the Run/Stop button). */
export async function toggleScenarioRun(tab: ScenarioTab) {
  if (useRunner.getState().active === tab.id) return stopRun(tab.id);
  const requests = new Map(flattenRequests(useWorkspace.getState().info?.tree ?? []).map((r) => [r.path, r]));
  const problem = startProblem(tab.draft, requests, useScenarios.getState().saved, tab.scenarioId);
  if (problem) {
    toast("info", `Can't run “${tab.draft.name}”`, problem);
    return false;
  }
  return runScenario(tab);
}

// Renamed or moved requests: the steps that send them follow, also in drafts with unsaved changes.
onItemsMoved((oldPath, newPath) => {
  for (const tab of useTabs.getState().tabs) {
    if (!isScenarioTab(tab)) continue;
    const draft = retarget(tab.draft, oldPath, newPath);
    const saved = retarget(tab.saved, oldPath, newPath);
    if (draft !== tab.draft || saved !== tab.saved) updateScenarioTab(tab.id, () => ({ draft, saved }));
  }
});

// The values shown belong to the open workspace's scenarios.
useWorkspace.subscribe((s, prev) => {
  if (s.info?.path !== prev.info?.path) resetScenarioValues();
});

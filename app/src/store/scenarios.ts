// Scenarios: the saved list of the open workspace, the values each one's last run left,
// and starting a run (the run itself and its results live in store/runner.ts, like a
// collection run's).
import { create } from "zustand";
import type { Scenario } from "../bindings/Scenario";
import type { ScenarioNode } from "../bindings/ScenarioNode";
import type { ScenarioValue } from "../bindings/ScenarioValue";
import type { TreeNode } from "../bindings/TreeNode";
import { flattenRequests } from "../components/loadtests/model";
import { newScenario, retarget, startProblem } from "../components/scenarios/model";
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
import { useUi } from "./ui";
import { useWorkspace } from "./workspace";

interface ScenariosState {
  saved: ScenarioNode[];
  /** The values the last run of each scenario left, by scenario id. */
  values: Record<string, ScenarioValue[]>;
}

export const useScenarios = create<ScenariosState>(() => ({ saved: [], values: {} }));
const set = useScenarios.setState;

const currentWorkspace = () => useWorkspace.getState().info?.path ?? "";

// ---- saved list ----------------------------------------------------------------------

export async function refreshScenarios() {
  if (!useWorkspace.getState().info) {
    set({ saved: [] });
    return;
  }
  const ws = currentWorkspace();
  try {
    const saved = await api.scenarios();
    if (currentWorkspace() === ws) set({ saved });
  } catch {
    if (currentWorkspace() === ws) set({ saved: [] });
  }
}

/** The open workspace changed: the values of the previous one's scenarios no longer apply. */
export function resetScenarioValues() {
  set({ values: {} });
}

onScenarioFilesChanged(() => void refreshScenarios());
onScenarioSaved((oldId, newId) => {
  if (oldId !== newId) moveValues(oldId, newId);
  void refreshScenarios();
});

/** Create a scenario and open it; returns its id. */
export async function createScenario(scenario: Scenario): Promise<string | null> {
  try {
    const id = await api.createScenario(scenario);
    await refreshScenarios();
    await openScenario(id);
    useUi.setState({ sidebarTab: "scenarios" });
    return id;
  } catch (e) {
    toast("error", "Could not create scenario", errorMessage(e));
    return null;
  }
}

/** "New scenario" (asks for a name; no steps yet). */
export async function newScenarioPrompt() {
  const name = await prompt({ title: "New scenario", value: "Scenario", confirmLabel: "Create" });
  if (name) await createScenario(newScenario(name));
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
    moveValues(node.id, newId);
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

export async function reorderScenarios(ids: string[]) {
  set((s) => ({ saved: ids.map((id) => s.saved.find((n) => n.id === id)!).filter(Boolean) }));
  await api.reorderScenarios(ids).catch((e) => toast("error", "Could not reorder", errorMessage(e)));
  await refreshScenarios();
}

const tabOf = (scenarioId: string): ScenarioTab | undefined => useTabs.getState().tabs.find((t): t is ScenarioTab => isScenarioTab(t) && t.scenarioId === scenarioId);

/** A rename: the values the last run left follow the new id. */
function moveValues(oldId: string, newId: string) {
  if (oldId === newId) return;
  set((s) => {
    const values = { ...s.values };
    if (oldId in values) {
      values[newId] = values[oldId];
      delete values[oldId];
    }
    return { values };
  });
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

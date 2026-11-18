// Environments and variables in the sidebar and in tabs: opening them, making, copying
// and deleting environments, and keeping open tabs in step with the workspace.
import type { EnvironmentEntry } from "../bindings/EnvironmentEntry";
import { api, errorMessage } from "../lib/rpc";
import { confirm } from "./dialogs";
import { closeTab, isDirty, isVariablesTab, openVariables, sameVariables, updateVariablesTab, useTabs, variableSet } from "./tabs";
import { toast } from "./toasts";
import { useUi } from "./ui";
import { refreshEnvironments, setActiveEnvironment, useWorkspace } from "./workspace";

/**
 * Show the environments: the sidebar section, and in a tab `focus` (an environment's id),
 * else the active environment, else the first one, else the workspace's variables.
 */
export function showEnvironments(focus?: string) {
  const info = useWorkspace.getState().info;
  if (!info) return;
  useUi.setState({ sidebarTab: "environments" });
  const known = (id: string | null | undefined) => (id && info.environments.some((e) => e.id === id) ? id : undefined);
  const id = known(focus) ?? known(info.activeEnvironment) ?? info.environments[0]?.id;
  if (id) openVariables("environment", id);
  else openVariables("workspace");
}

/** A new environment (or a copy of `from`), opened in a tab to name and fill in. */
export async function newEnvironment(from?: EnvironmentEntry) {
  try {
    const id = await api.createEnvironment(
      from ? { name: `${from.environment.name} copy`, variables: from.environment.variables } : { name: "New environment", variables: [] },
    );
    await refreshEnvironments();
    useUi.setState({ sidebarTab: "environments" });
    openVariables("environment", id);
  } catch (e) {
    toast("error", "Could not create environment", errorMessage(e));
  }
}

export async function deleteEnvironment(env: EnvironmentEntry) {
  if (!(await confirm({ title: "Delete environment?", message: `“${env.environment.name}” will be moved to the trash.`, confirmLabel: "Delete", danger: true }))) return;
  try {
    await api.deleteEnvironment(env.id);
    const tab = useTabs.getState().tabs.find((t) => isVariablesTab(t) && t.envId === env.id);
    if (tab) await closeTab(tab.id, true);
    await refreshEnvironments();
  } catch (e) {
    toast("error", "Could not delete", errorMessage(e));
  }
}

/** Make an environment the active one, or none when it already is. */
export async function toggleActiveEnvironment(id: string) {
  const active = useWorkspace.getState().info?.activeEnvironment === id;
  await setActiveEnvironment(active ? null : id).catch((e) => toast("error", "Could not switch the environment", errorMessage(e)));
}

// Open tabs follow the workspace: a change made elsewhere (a Git pull, an AI agent, an import)
// shows in a tab without edits; one with edits keeps them. A deleted environment's tab says so.
useWorkspace.subscribe((s, prev) => {
  if (!s.info || (s.info.environments === prev.info?.environments && s.info.meta === prev.info?.meta)) return;
  for (const tab of useTabs.getState().tabs) {
    if (!isVariablesTab(tab) || tab.scope === "globals") continue;
    const fresh = variableSet(tab.scope, tab.envId);
    if (!fresh) {
      if (!tab.orphaned) updateVariablesTab(tab.id, () => ({ orphaned: true }));
      continue;
    }
    if (sameVariables(fresh, tab.saved) && !tab.orphaned) continue;
    updateVariablesTab(tab.id, (t) => (isDirty(t) ? { saved: fresh, orphaned: false } : { saved: fresh, draft: structuredClone(fresh), orphaned: false }));
  }
});

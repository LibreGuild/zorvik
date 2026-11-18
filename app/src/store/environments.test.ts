import { beforeEach, describe, expect, it, vi } from "vitest";
import type { EnvironmentEntry } from "../bindings/EnvironmentEntry";
import type { WorkspaceInfo } from "../bindings/WorkspaceInfo";

vi.mock("../lib/events", () => ({ onEvent: () => () => {} }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("../lib/rpc", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/rpc")>();
  const fn = () => vi.fn((..._args: unknown[]): Promise<unknown> => Promise.resolve(null));
  return {
    ...actual,
    api: {
      listEnvironments: fn(),
      createEnvironment: fn(),
      saveEnvironment: fn(),
      deleteEnvironment: fn(),
      setActiveEnvironment: fn(),
      saveWorkspaceMeta: fn(),
      currentWorkspace: fn(),
      variables: vi.fn((): Promise<unknown> => Promise.resolve([])),
    },
  };
});

const { api, RpcError } = await import("../lib/rpc");
const { useWorkspace } = await import("./workspace");
const { useUi } = await import("./ui");
const { useDialogs } = await import("./dialogs");
const { useTabs, resetTabs, restoreTabs, leaveTabs, openVariables, isDirty, saveTab, updateVariablesTab, sameVariables } = await import("./tabs");
type VariablesTab = import("./tabs").VariablesTab;
const { deleteEnvironment, newEnvironment, showEnvironments, toggleActiveEnvironment } = await import("./environments");
const mocked = api as unknown as Record<string, ReturnType<typeof vi.fn>>;

const WS = "/ws";
const env = (id: string, variables: EnvironmentEntry["environment"]["variables"] = []): EnvironmentEntry => ({ id, environment: { name: id, variables } });
const workspace = (environments: EnvironmentEntry[], activeEnvironment: string | null = null) =>
  useWorkspace.setState({
    info: { path: WS, tree: [], environments, activeEnvironment, meta: { name: "Shop", variables: [{ key: "base", value: "http://x.test" }] } } as unknown as WorkspaceInfo,
  });
const tab = (id: string | null) => useTabs.getState().tabs.find((t) => t.id === id) as VariablesTab;

beforeEach(() => {
  resetTabs();
  localStorage.clear();
  useUi.setState({ sidebarTab: "collection" });
  useDialogs.setState({ current: null });
  workspace([env("Local", [{ key: "token", value: "s3cret", secret: true }]), env("Staging")], "Staging");
  vi.clearAllMocks();
});

describe("environments in the sidebar and in tabs", () => {
  it("show the active environment, the one asked for, or the workspace's variables", () => {
    showEnvironments();
    expect(useUi.getState().sidebarTab).toBe("environments");
    expect(tab(useTabs.getState().activeId)).toMatchObject({ scope: "environment", envId: "Staging" });
    showEnvironments("Local");
    expect(tab(useTabs.getState().activeId)).toMatchObject({ envId: "Local", draft: { name: "Local" } });
    // One tab per environment.
    showEnvironments("Local");
    expect(useTabs.getState().tabs).toHaveLength(2);
    // None at all: the workspace's variables.
    resetTabs();
    workspace([]);
    showEnvironments("gone");
    expect(tab(useTabs.getState().activeId)).toMatchObject({ scope: "workspace", draft: { name: "Workspace variables", variables: [{ key: "base" }] } });
    expect(openVariables("environment", "gone")).toBeNull();
  });

  it("save an environment under its new name, and the workspace's variables into the workspace", async () => {
    const id = openVariables("environment", "Local")!;
    expect(isDirty(tab(id))).toBe(false);
    // A row not filled in is no change.
    updateVariablesTab(id, (t) => ({ draft: { ...t.draft, variables: [...t.draft.variables, { key: "", value: "" }] } }));
    expect(isDirty(tab(id))).toBe(false);
    updateVariablesTab(id, (t) => ({ draft: { name: " Dev ", variables: [...t.draft.variables, { key: "base", value: "http://dev.test" }, { key: " ", value: "no name" }] } }));
    expect(isDirty(tab(id))).toBe(true);
    mocked.saveEnvironment.mockResolvedValueOnce("Dev");
    const saved = { name: "Dev", variables: [{ key: "token", value: "s3cret", secret: true }, { key: "base", value: "http://dev.test" }] };
    mocked.listEnvironments.mockResolvedValueOnce([{ id: "Dev", environment: saved }, env("Staging")]);
    expect(await saveTab(id)).toBe(true);
    expect(mocked.saveEnvironment).toHaveBeenCalledWith("Local", saved);
    expect(tab(id)).toMatchObject({ envId: "Dev", draft: saved, saved });
    expect(isDirty(tab(id))).toBe(false);

    const ws = openVariables("workspace")!;
    updateVariablesTab(ws, (t) => ({ draft: { ...t.draft, variables: [{ key: "base", value: "http://y.test" }] } }));
    mocked.currentWorkspace.mockResolvedValueOnce(null);
    expect(await saveTab(ws)).toBe(true);
    expect(mocked.saveWorkspaceMeta).toHaveBeenCalledWith({ name: "Shop", variables: [{ key: "base", value: "http://y.test" }] });
    // Globals have nothing to save.
    const globals = openVariables("globals")!;
    expect(isDirty(tab(globals))).toBe(false);
    expect(await saveTab(globals)).toBe(true);
  });

  it("follow changes made elsewhere, keep edits, and say when the file is gone", async () => {
    const id = openVariables("environment", "Local")!;
    workspace([env("Local", [{ key: "token", value: "rotated", secret: true }]), env("Staging")]);
    expect(tab(id).draft.variables[0].value).toBe("rotated");
    updateVariablesTab(id, (t) => ({ draft: { ...t.draft, name: "Mine" } }));
    workspace([env("Local", [{ key: "token", value: "again", secret: true }]), env("Staging")]);
    expect(tab(id)).toMatchObject({ draft: { name: "Mine", variables: [{ value: "rotated" }] }, saved: { variables: [{ value: "again" }] } });
    workspace([env("Staging")]);
    expect(tab(id).orphaned).toBe(true);
    // Saving writes it again.
    mocked.saveEnvironment.mockRejectedValueOnce(new RpcError({ code: "notFound", message: "Environment 'Local' not found" } as ConstructorParameters<typeof RpcError>[0]));
    mocked.createEnvironment.mockResolvedValueOnce("Mine");
    mocked.listEnvironments.mockResolvedValueOnce([env("Staging"), { id: "Mine", environment: tab(id).draft }]);
    expect(await saveTab(id)).toBe(true);
    expect(tab(id)).toMatchObject({ envId: "Mine", orphaned: false });
    expect(sameVariables(tab(id).draft, tab(id).saved)).toBe(true);
  });

  it("make, copy, activate and delete environments", async () => {
    mocked.createEnvironment.mockResolvedValueOnce("New environment");
    mocked.listEnvironments.mockResolvedValueOnce([env("Local"), env("Staging"), env("New environment")]);
    await newEnvironment();
    expect(mocked.createEnvironment).toHaveBeenCalledWith({ name: "New environment", variables: [] });
    expect(tab(useTabs.getState().activeId)).toMatchObject({ envId: "New environment" });
    expect(useUi.getState().sidebarTab).toBe("environments");

    mocked.createEnvironment.mockResolvedValueOnce("Local copy");
    mocked.listEnvironments.mockResolvedValueOnce([env("Local"), env("Staging"), env("New environment"), env("Local copy")]);
    await newEnvironment(env("Local", [{ key: "a", value: "1" }]));
    expect(mocked.createEnvironment).toHaveBeenLastCalledWith({ name: "Local copy", variables: [{ key: "a", value: "1" }] });

    await toggleActiveEnvironment("Local");
    expect(mocked.setActiveEnvironment).toHaveBeenLastCalledWith("Local");
    expect(useWorkspace.getState().info?.activeEnvironment).toBe("Local");
    await toggleActiveEnvironment("Local");
    expect(mocked.setActiveEnvironment).toHaveBeenLastCalledWith(null);

    // Deleting asks, then closes its tab (edits and all).
    const id = openVariables("environment", "Staging")!;
    updateVariablesTab(id, (t) => ({ draft: { ...t.draft, name: "Edited" } }));
    mocked.listEnvironments.mockResolvedValueOnce([env("Local")]);
    const deleting = deleteEnvironment(env("Staging"));
    await vi.waitFor(() => expect(useDialogs.getState().current).not.toBeNull());
    const dialog = useDialogs.getState().current!;
    useDialogs.setState({ current: null });
    if (dialog.kind === "confirm") dialog.resolve(true);
    await deleting;
    expect(mocked.deleteEnvironment).toHaveBeenCalledWith("Staging");
    expect(tab(id)).toBeUndefined();
  });

  it("come back after a restart without their unsaved edits, so no secret is kept in the window's storage", async () => {
    await restoreTabs(WS);
    const id = openVariables("environment", "Local")!;
    updateVariablesTab(id, (t) => ({ draft: { ...t.draft, variables: [{ key: "token", value: "typed-secret", secret: true }] } }));
    await new Promise((r) => setTimeout(r, 350));
    const stored = localStorage.getItem(`zv:tabs:${WS}`) ?? "";
    expect(stored).toContain('"envId":"Local"');
    expect(stored).not.toContain("typed-secret");
    expect(stored).not.toContain("s3cret");
    // So leaving the workspace asks first; without unsaved variables it doesn't.
    const leaving = leaveTabs();
    const dialog = useDialogs.getState().current!;
    expect(dialog).toMatchObject({ kind: "confirm", title: "Discard unsaved changes?" });
    expect(dialog.kind === "confirm" && dialog.message).toContain('"Local" has changes that are not saved');
    useDialogs.setState({ current: null });
    if (dialog.kind === "confirm") dialog.resolve(false);
    expect(await leaving).toBe(false);
    resetTabs();
    await restoreTabs(WS);
    expect(useTabs.getState().tabs[0]).toMatchObject({ type: "variables", scope: "environment", envId: "Local", draft: { variables: [{ value: "s3cret" }] } });
    expect(await leaveTabs()).toBe(true);
  });
});

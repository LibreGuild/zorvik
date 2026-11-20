import { beforeEach, describe, expect, it, vi } from "vitest";
import type { RunResult } from "../bindings/RunResult";
import type { RunSummary } from "../bindings/RunSummary";
import type { Scenario } from "../bindings/Scenario";
import type { ScenarioNode } from "../bindings/ScenarioNode";
import type { StreamEvent } from "../bindings/StreamEvent";
import type { TreeNode } from "../bindings/TreeNode";
import type { WorkspaceInfo } from "../bindings/WorkspaceInfo";

const handlers: ((e: StreamEvent) => void)[] = [];
vi.mock("../lib/events", () => ({
  onEvent: (h: (e: StreamEvent) => void) => {
    handlers.push(h);
    return () => {};
  },
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("../lib/rpc", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/rpc")>();
  const fn = () => vi.fn((..._args: unknown[]): Promise<unknown> => Promise.resolve(null));
  return {
    ...actual,
    api: {
      scenarios: fn(),
      readScenario: fn(),
      createScenario: fn(),
      saveScenario: fn(),
      deleteScenario: fn(),
      createScenarioFolder: fn(),
      renameScenarioFolder: fn(),
      deleteScenarioFolder: fn(),
      moveScenario: fn(),
      runScenario: fn(),
      answerScenario: fn(),
      scenarioValues: fn(),
      clearScenarioValues: fn(),
      stopRun: fn(),
    },
  };
});

const { api } = await import("../lib/rpc");
const { useWorkspace } = await import("./workspace");
const { useToasts } = await import("./toasts");
const { useDialogs } = await import("./dialogs");
const { useTabs, resetTabs, openScenario, isDirty, onItemMoved, sameScenario, saveTab, updateScenarioDraft, closeTab } = await import("./tabs");
type ScenarioTab = import("./tabs").ScenarioTab;
const { flushRunnerEvents, useRunner } = await import("./runner");
const {
  clearScenarioValues,
  deleteScenarioFolder,
  moveScenarioNode,
  newScenarioFolder,
  refreshScenarios,
  renameScenarioFolder,
  runScenario,
  scenarioFolderKey,
  scenarioFromFolder,
  toggleScenarioRun,
  useScenarios,
} = await import("./scenarios");
const { useUi } = await import("./ui");
const mocked = api as unknown as Record<string, ReturnType<typeof vi.fn>>;

const WS = "/ws";
const req = (path: string, name: string, extra: Partial<TreeNode> = {}): TreeNode => ({ kind: "request", path, name, seq: 0, method: "GET", children: [], ...extra });
const tree: TreeNode[] = [
  { kind: "folder", path: "cart", name: "Cart", seq: 0, children: [req("cart/create.yaml", "Create"), req("cart/socket.yaml", "Socket", { requestKind: "tcp" }), req("cart/get.yaml", "Get")] },
];
const scenario = (extra: Partial<Scenario> = {}): Scenario => ({
  name: "Cart flow",
  seq: 0,
  steps: [
    { type: "request", request: "cart/create.yaml", outputs: [{ variable: "cartId", path: "$.id" }] },
    { type: "request", request: "cart/get.yaml" },
  ],
  ...extra,
});
const result = (name: string, step: RunResult["step"], extra: Partial<RunResult> = {}): RunResult => ({
  iteration: 0,
  path: "",
  name,
  kind: "http",
  method: "GET",
  url: "",
  status: null,
  durationMs: null,
  size: null,
  tests: [],
  console: [],
  error: null,
  scriptErrors: [],
  unresolved: [],
  passed: true,
  skipped: false,
  seq: 0,
  step,
  ...extra,
});
const summary = (extra: Partial<RunSummary> = {}): RunSummary => ({
  name: "Cart flow",
  environment: null,
  startedAt: 0,
  durationMs: 10,
  iterations: 1,
  requests: 2,
  failed: 0,
  skipped: 0,
  testsPassed: 0,
  testsFailed: 0,
  testsSkipped: 0,
  stopped: false,
  bailed: false,
  error: null,
  passed: true,
  perIteration: [],
  omitted: 0,
  ...extra,
});
const emit = (e: StreamEvent) => handlers.forEach((h) => h(e));
const tab = (id: string): ScenarioTab => useTabs.getState().tabs.find((t) => t.id === id) as ScenarioTab;

async function opened(saved = scenario()): Promise<string> {
  mocked.readScenario.mockResolvedValueOnce(saved);
  return (await openScenario("Cart flow"))!;
}

beforeEach(() => {
  resetTabs();
  localStorage.clear();
  useWorkspace.setState({ info: { path: WS, tree, environments: [], activeEnvironment: null, meta: { name: "Shop" } } as unknown as WorkspaceInfo });
  useRunner.setState({ settings: {}, data: {}, runs: {}, filter: {}, active: null, starting: null });
  useScenarios.setState({ tree: [], saved: [], values: {} });
  useToasts.setState({ toasts: [] });
  useDialogs.setState({ current: null });
  vi.clearAllMocks();
});

describe("scenario tabs", () => {
  it("open once, know when they changed, and save under the name's file", async () => {
    const id = await opened();
    expect(await openScenario("Cart flow")).toBe(id);
    expect(mocked.readScenario).toHaveBeenCalledTimes(1);
    expect(isDirty(tab(id))).toBe(false);
    // Defaults written out are the same scenario.
    updateScenarioDraft(id, (s) => ({ ...s, steps: s.steps.map((step) => (step.type === "request" ? { ...step, continueOnFailure: false, outputs: step.outputs?.map((o) => ({ ...o, from: "json" as const, secret: false })) } : step)) }));
    expect(isDirty(tab(id))).toBe(false);
    updateScenarioDraft(id, (s) => ({ ...s, name: "Checkout" }));
    expect(isDirty(tab(id))).toBe(true);
    mocked.saveScenario.mockResolvedValueOnce("Checkout");
    mocked.scenarios.mockResolvedValue([]);
    useScenarios.setState({ values: { "Cart flow": [{ key: "cartId", value: "7", kind: "text", secret: false }] } });
    expect(await saveTab(id)).toBe(true);
    expect(mocked.saveScenario).toHaveBeenCalledWith("Cart flow", expect.objectContaining({ name: "Checkout" }));
    expect(tab(id).scenarioId).toBe("Checkout");
    expect(isDirty(tab(id))).toBe(false);
    // The values the last run left follow the new name.
    expect(Object.keys(useScenarios.getState().values)).toEqual(["Checkout"]);
  });

  it("compares scenarios without the defaults", () => {
    const a = scenario();
    expect(sameScenario(a, { ...a, seq: 5, docs: "", variables: [], always: [] })).toBe(true);
    expect(sameScenario(a, { ...a, continueOnFailure: true })).toBe(false);
    expect(sameScenario(a, { ...a, steps: [...a.steps].reverse() })).toBe(false);
    const off = { ...a, steps: [{ ...a.steps[0], enabled: false }, a.steps[1]] };
    expect(sameScenario(a, off)).toBe(false);
  });

  it("follow requests that are renamed or moved, in the draft too", async () => {
    const id = await opened();
    updateScenarioDraft(id, (s) => ({ ...s, docs: "edited" }));
    onItemMoved("cart", "basket");
    const requests = (s: Scenario) => s.steps.map((step) => (step.type === "request" ? step.request : ""));
    expect(requests(tab(id).draft)).toEqual(["basket/create.yaml", "basket/get.yaml"]);
    expect(requests(tab(id).saved)).toEqual(["basket/create.yaml", "basket/get.yaml"]);
    expect(tab(id).draft.docs).toBe("edited");
  });

  it("are made from a folder's requests a run can send", async () => {
    mocked.createScenario.mockResolvedValueOnce("Cart");
    mocked.scenarios.mockResolvedValue([{ id: "Cart", name: "Cart", steps: 2, seq: 0 }]);
    mocked.readScenario.mockResolvedValueOnce(scenario({ name: "Cart" }));
    const making = scenarioFromFolder(tree, "cart", "Cart");
    await vi.waitFor(() => expect(useDialogs.getState().current).not.toBeNull());
    const dialog = useDialogs.getState().current!;
    useDialogs.setState({ current: null });
    if (dialog.kind === "prompt") dialog.resolve("Cart");
    await making;
    expect(mocked.createScenario).toHaveBeenCalledWith(
      {
        name: "Cart",
        seq: 0,
        steps: [
          { type: "request", request: "cart/create.yaml" },
          { type: "request", request: "cart/get.yaml" },
        ],
      },
      "",
    );
    await refreshScenarios();
    expect(useScenarios.getState().saved).toHaveLength(1);
  });
});

describe("scenario folders", () => {
  const node = (id: string, extra: Partial<ScenarioNode> = {}): ScenarioNode => ({ id, name: id.split("/").pop()!, steps: 1, seq: 0, ...extra });
  const shop = (): ScenarioNode[] => [
    node("Smoke"),
    node("Shop", { folder: true, children: [node("Shop/Auth", { folder: true, children: [node("Shop/Auth/Log in")] }), node("Shop/Checkout")] }),
    node("Last"),
  ];
  const answer = async (value: string | boolean) => {
    await vi.waitFor(() => expect(useDialogs.getState().current).not.toBeNull());
    const dialog = useDialogs.getState().current!;
    useDialogs.setState({ current: null });
    if (dialog.kind === "prompt" && typeof value === "string") dialog.resolve(value);
    if (dialog.kind === "confirm" && typeof value === "boolean") dialog.resolve(value);
    return dialog;
  };

  it("list the scenarios in their folders, and every scenario for the pickers", async () => {
    mocked.scenarios.mockResolvedValue(shop());
    await refreshScenarios();
    expect(useScenarios.getState().tree[1].children).toHaveLength(2);
    expect(useScenarios.getState().saved.map((n) => n.id)).toEqual(["Smoke", "Shop/Auth/Log in", "Shop/Checkout", "Last"]);
  });

  it("are made, opened in the sidebar, renamed, and what is in them follows", async () => {
    mocked.scenarios.mockResolvedValue(shop());
    mocked.createScenarioFolder.mockResolvedValueOnce("Shop/Auth/Tokens");
    const making = newScenarioFolder("Shop/Auth");
    await answer("Tokens");
    await making;
    expect(mocked.createScenarioFolder).toHaveBeenCalledWith("Shop/Auth", "Tokens");
    const open = useUi.getState().expanded;
    expect([open[scenarioFolderKey("Shop")], open[scenarioFolderKey("Shop/Auth")], open[scenarioFolderKey("Shop/Auth/Tokens")]]).toEqual([true, true, true]);

    // A tab of a scenario in the folder, another whose step runs it, and the values its last run left.
    mocked.readScenario.mockResolvedValueOnce(scenario({ name: "Log in" }));
    const login = (await openScenario("Shop/Auth/Log in"))!;
    mocked.readScenario.mockResolvedValueOnce(scenario({ name: "Main", steps: [{ type: "scenario", scenario: "Shop/Auth/Log in" }] }));
    const main = (await openScenario("Main"))!;
    useScenarios.setState({ values: { "Shop/Auth/Log in": [{ key: "token", value: "t", kind: "text", secret: false }] } });
    mocked.renameScenarioFolder.mockResolvedValueOnce("Shop/Sign in");
    const auth = useScenarios.getState().tree[1].children![0];
    const renaming = renameScenarioFolder(auth);
    await answer("Sign in");
    await renaming;
    expect(mocked.renameScenarioFolder).toHaveBeenCalledWith("Shop/Auth", "Sign in");
    expect(tab(login).scenarioId).toBe("Shop/Sign in/Log in");
    const calls = (s: Scenario) => s.steps.map((step) => (step.type === "scenario" ? step.scenario : ""));
    expect([calls(tab(main).draft), calls(tab(main).saved)]).toEqual([["Shop/Sign in/Log in"], ["Shop/Sign in/Log in"]]);
    expect(isDirty(tab(main))).toBe(false);
    expect(Object.keys(useScenarios.getState().values)).toEqual(["Shop/Sign in/Log in"]);
    expect(useUi.getState().expanded[scenarioFolderKey("Shop/Sign in/Tokens")]).toBe(true);
  });

  it("move scenarios and folders: into a folder, and to a place in their own", async () => {
    mocked.scenarios.mockResolvedValue(shop());
    await refreshScenarios();
    const [smoke, shopFolder, last] = useScenarios.getState().tree;
    mocked.moveScenario.mockResolvedValueOnce("Shop/Smoke");
    await moveScenarioNode(smoke, "Shop", null);
    expect(mocked.moveScenario).toHaveBeenLastCalledWith("Smoke", false, "Shop", null);
    // Dropped before the row after it, or on itself: nowhere new.
    await moveScenarioNode(smoke, "", 1);
    await moveScenarioNode(smoke, "", 0);
    expect(mocked.moveScenario).toHaveBeenCalledTimes(1);
    // Down its own folder: the places after it move up once it leaves its own.
    mocked.moveScenario.mockResolvedValueOnce("Smoke");
    await moveScenarioNode(smoke, "", 3);
    expect(mocked.moveScenario).toHaveBeenLastCalledWith("Smoke", false, "", 2);
    mocked.moveScenario.mockResolvedValueOnce("Last");
    await moveScenarioNode(last, "", 0);
    expect(mocked.moveScenario).toHaveBeenLastCalledWith("Last", false, "", 0);
    // Never into itself.
    await moveScenarioNode(shopFolder, "Shop/Auth", null);
    expect(mocked.moveScenario).toHaveBeenCalledTimes(3);
    mocked.moveScenario.mockRejectedValueOnce(new Error("disk full"));
    await moveScenarioNode(shopFolder, "Archive", null);
    expect(useToasts.getState().toasts.at(-1)).toMatchObject({ title: "Could not move" });
  });

  it("are deleted with what is in them, after asking", async () => {
    mocked.scenarios.mockResolvedValue(shop());
    await refreshScenarios();
    mocked.readScenario.mockResolvedValueOnce(scenario({ name: "Checkout" }));
    const checkout = (await openScenario("Shop/Checkout"))!;
    useScenarios.setState({ values: { "Shop/Checkout": [], Smoke: [] } });
    const shopFolder = useScenarios.getState().tree[1];
    let deleting = deleteScenarioFolder(shopFolder);
    await answer(false);
    await deleting;
    expect(mocked.deleteScenarioFolder).not.toHaveBeenCalled();
    deleting = deleteScenarioFolder(shopFolder);
    const dialog = await answer(true);
    await deleting;
    expect(dialog.kind === "confirm" && dialog.message).toBe("“Shop” and the 2 scenarios in it will be moved to the trash.");
    expect(mocked.deleteScenarioFolder).toHaveBeenCalledWith("Shop");
    expect(tab(checkout).orphaned).toBe(true);
    expect(Object.keys(useScenarios.getState().values)).toEqual(["Smoke"]);
  });
});

describe("scenario runs", () => {
  it("run the draft, count the steps that did something, ask for values and read what the run left", async () => {
    const id = await opened();
    updateScenarioDraft(id, (s) => ({ ...s, docs: "draft" }));
    mocked.runScenario.mockResolvedValueOnce({ runId: "r1", name: "Cart flow", total: 0, iterations: 1, environment: null });
    expect(await runScenario(tab(id), { to: "steps/0" })).toBe(true);
    expect(mocked.runScenario).toHaveBeenCalledWith("Cart flow", expect.objectContaining({ docs: "draft" }), { to: "steps/0" });
    expect(useRunner.getState().active).toBe(id);
    // Another run can't start meanwhile.
    expect(await runScenario(tab(id))).toBe(false);
    expect(useToasts.getState().toasts.at(-1)).toMatchObject({ title: "A run is in progress" });

    const run = (event: Extract<StreamEvent, { type: "runner" }>["event"]) => emit({ type: "runner", runId: "r1", event });
    run({ type: "result", result: result("Create", { path: "steps/0", kind: "request", depth: 0, saved: [{ key: "cartId", value: "7" }] }, { status: 201 }) });
    run({ type: "result", result: result("For each item in a,b", { path: "steps/1", kind: "forEach", depth: 0, note: "2 items" }) });
    run({ type: "result", result: result("item = a", { path: "steps/1", kind: "round", depth: 1, note: "1 of 2" }) });
    run({ type: "result", result: result("Get", { path: "steps/1/steps/0", kind: "request", depth: 1 }, { status: 500, passed: false }) });
    run({ type: "result", result: result("Repeat until {{x}} exists", { path: "steps/2", kind: "repeat", depth: 0 }, { passed: false, error: "never" }) });
    flushRunnerEvents();
    // Two requests and the failed repeat count; the loop and its round don't.
    expect(useRunner.getState().runs[id].counts).toMatchObject({ requests: 3, failed: 2 });
    expect(useRunner.getState().runs[id].results).toHaveLength(5);

    // An "ask" step: the user's answer goes back to the run; Cancel gives none.
    run({ type: "ask", variable: "otp", message: "The code", secret: true });
    await vi.waitFor(() => expect(useDialogs.getState().current).not.toBeNull());
    const dialog = useDialogs.getState().current!;
    expect(dialog).toMatchObject({ kind: "prompt", title: "Value for otp", message: "The code", secret: true });
    useDialogs.setState({ current: null });
    if (dialog.kind === "prompt") dialog.resolve("123456");
    await vi.waitFor(() => expect(mocked.answerScenario).toHaveBeenCalledWith("r1", "123456"));

    mocked.scenarioValues.mockResolvedValueOnce([{ key: "cartId", value: "7", kind: "text", secret: false }]);
    run({ type: "finished", summary: summary({ passed: false, failed: 2, requests: 3, bailed: true }) });
    expect(useRunner.getState().active).toBeNull();
    expect(useToasts.getState().toasts.at(-1)).toMatchObject({ tone: "error", title: "“Cart flow” failed", detail: "3 steps, 2 failed" });
    await vi.waitFor(() => expect(useScenarios.getState().values["Cart flow"]).toEqual([{ key: "cartId", value: "7", kind: "text", secret: false }]));

    mocked.scenarioValues.mockResolvedValueOnce([]);
    await clearScenarioValues("Cart flow", "cartId");
    expect(mocked.clearScenarioValues).toHaveBeenCalledWith("Cart flow", "cartId");
    expect(useScenarios.getState().values["Cart flow"]).toEqual([]);
  });

  it("toggle with Mod+Enter, toast start errors, and stop with their tab", async () => {
    const id = await opened();
    mocked.runScenario.mockRejectedValueOnce(new Error("'Cart flow' has no steps to run"));
    expect(await toggleScenarioRun(tab(id))).toBe(false);
    expect(useToasts.getState().toasts.at(-1)).toMatchObject({ tone: "error", title: "Could not run “Cart flow”" });
    expect(useRunner.getState().starting).toBeNull();

    // What can't run says why instead of starting.
    updateScenarioDraft(id, (s) => ({ ...s, steps: [...s.steps, { type: "request", request: "gone.yaml" }] }));
    expect(await toggleScenarioRun(tab(id))).toBe(false);
    expect(useToasts.getState().toasts.at(-1)).toMatchObject({ title: "Can't run “Cart flow”", detail: "gone: the request is no longer in the collection" });
    expect(mocked.runScenario).toHaveBeenCalledTimes(1);
    updateScenarioDraft(id, (s) => ({ ...s, steps: s.steps.slice(0, 2) }));

    mocked.runScenario.mockResolvedValueOnce({ runId: "r2", name: "Cart flow", total: 0, iterations: 1, environment: null });
    expect(await toggleScenarioRun(tab(id))).toBe(true);
    await toggleScenarioRun(tab(id));
    expect(mocked.stopRun).toHaveBeenCalledWith("r2");
    // Closing the tab stops a run still in progress and drops its results.
    mocked.stopRun.mockClear();
    useRunner.setState((s) => ({ runs: { ...s.runs, [id]: { ...s.runs[id], stopping: false } } }));
    await closeTab(id, true);
    expect(mocked.stopRun).toHaveBeenCalledWith("r2");
    expect(useRunner.getState().runs[id]).toBeUndefined();
  });
});

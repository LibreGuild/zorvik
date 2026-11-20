import { describe, expect, it } from "vitest";
import type { Request } from "../../bindings/Request";
import type { RunResult } from "../../bindings/RunResult";
import type { Scenario } from "../../bindings/Scenario";
import type { ScenarioNode } from "../../bindings/ScenarioNode";
import type { Step } from "../../bindings/Step";
import type { RequestEntry } from "../loadtests/model";
import {
  conditionText,
  countSteps,
  duplicateStep,
  editStep,
  filterScenarios,
  flattenScenarios,
  folderNames,
  followSteps,
  insertSteps,
  jsonLeaves,
  listAt,
  movedId,
  moveStep,
  nameForPath,
  newScenario,
  newStep,
  outcomes,
  removeStep,
  requestVariables,
  retarget,
  retargetCalls,
  scenarioFolders,
  scenarioParent,
  startProblem,
  stepAt,
  stepProblem,
  stepsMoved,
  stepTitle,
  valuesAt,
  walkSteps,
} from "./model";

const request = (path: string, extra: Partial<Extract<Step, { type: "request" }>> = {}): Step => ({ type: "request", request: path, ...extra });
const entry = (path: string, name: string, kind: RequestEntry["kind"] = "http"): RequestEntry => ({ path, name, method: "GET", kind, trail: "" });
const requests = new Map([entry("a.yaml", "A"), entry("b.yaml", "B"), entry("c.yaml", "C"), entry("d.yaml", "D")].map((r) => [r.path, r]));

/** The request paths (and other kinds) in walk order: what the scenario looks like. */
const shape = (s: Scenario) => {
  const out: string[] = [];
  walkSteps(s, (path, step) => out.push(`${path}=${step.type === "request" ? step.request : step.type}`));
  return out;
};

const nested = (): Scenario => ({
  name: "S",
  seq: 0,
  variables: [{ key: "base", value: "1" }, { key: "off", value: "1", enabled: false }],
  steps: [
    request("a.yaml", { outputs: [{ variable: "cartId", path: "$.id" }] }),
    {
      type: "if",
      when: { rules: [{ value: "{{cartId}}", op: "greater", to: "5" }] },
      steps: [request("b.yaml")],
      elseIf: [{ when: { any: true, rules: [{ value: "{{x}}", op: "exists" }, { value: "{{y}}" }] }, steps: [request("c.yaml")] }],
      else: [{ type: "forEach", list: "{{items}}", as: "row", steps: [request("d.yaml"), { type: "set", values: [{ key: "seen", value: "1" }] }] }],
    },
    { type: "ask", variable: "otp" },
  ],
  always: [request("a.yaml")],
});

describe("steps by path", () => {
  it("walks every list, outer steps first", () => {
    const s = nested();
    expect(shape(s)).toEqual([
      "steps/0=a.yaml",
      "steps/1=if",
      "steps/1/steps/0=b.yaml",
      "steps/1/elseIf/0/0=c.yaml",
      "steps/1/else/0=forEach",
      "steps/1/else/0/steps/0=d.yaml",
      "steps/1/else/0/steps/1=set",
      "steps/2=ask",
      "always/0=a.yaml",
    ]);
    expect(countSteps(s)).toBe(9);
    expect(stepAt(s, "steps/1/else/0/steps/1")?.type).toBe("set");
    expect(stepAt(s, "steps/9")).toBeUndefined();
    expect(listAt(s, "steps/1/elseIf/0").length).toBe(1);
    expect(listAt(s, "always").length).toBe(1);
  });

  it("changes, removes, inserts and duplicates a step where it is", () => {
    const s = nested();
    const renamed = editStep(s, "steps/1/elseIf/0/0", (step) => ({ ...step, name: "Third" }));
    expect(stepAt(renamed, "steps/1/elseIf/0/0")?.name).toBe("Third");
    // The rest is the same objects (React re-renders only what changed).
    expect(renamed.steps[0]).toBe(s.steps[0]);
    expect(shape(removeStep(s, "steps/1/else/0/steps/0"))).toContain("steps/1/else/0/steps/0=set");
    expect(shape(insertSteps(s, "steps/1/steps", 0, [newStep("wait")]))).toContain("steps/1/steps/0=wait");
    expect(shape(insertSteps(s, "always", 99, [request("b.yaml")]))).toContain("always/1=b.yaml");
    const twice = duplicateStep(s, "steps/0");
    expect(shape(twice).slice(0, 2)).toEqual(["steps/0=a.yaml", "steps/1=a.yaml"]);
    expect(twice.steps[1]).not.toBe(twice.steps[0]);
  });

  it("moves a step within its list, into another list and out of one", () => {
    const flat = newScenario("S", ["a.yaml", "b.yaml", "c.yaml"]);
    const order = (s: Scenario) => s.steps.map((x) => (x.type === "request" ? x.request : x.type));
    expect(order(moveStep(flat, "steps/0", "steps", 3))).toEqual(["b.yaml", "c.yaml", "a.yaml"]);
    expect(order(moveStep(flat, "steps/2", "steps", 0))).toEqual(["c.yaml", "a.yaml", "b.yaml"]);
    expect(order(moveStep(flat, "steps/1", "steps", 0))).toEqual(["b.yaml", "a.yaml", "c.yaml"]);
    // Dropped where it is: nothing changes.
    expect(moveStep(flat, "steps/1", "steps", 1)).toBe(flat);
    expect(moveStep(flat, "steps/1", "steps", 2)).toBe(flat);
    // Two steps that are the same: the one that was dragged moves.
    const same = newScenario("S", ["a.yaml", "b.yaml", "a.yaml"]);
    expect(order(moveStep(same, "steps/2", "steps", 1))).toEqual(["a.yaml", "a.yaml", "b.yaml"]);

    const s = nested();
    // Into a list of a later step: that step's place changes when the moved one leaves.
    expect(shape(moveStep(s, "steps/0", "steps/1/steps", 1)).slice(0, 3)).toEqual(["steps/0=if", "steps/0/steps/0=b.yaml", "steps/0/steps/1=a.yaml"]);
    // Out of a nested list to before the step it was in.
    const out = moveStep(s, "steps/1/else/0/steps/0", "steps", 1);
    expect(shape(out).slice(0, 3)).toEqual(["steps/0=a.yaml", "steps/1=d.yaml", "steps/2=if"]);
    expect(shape(out)).toContain("steps/2/else/0/steps/0=set");
    // To the clean-up list.
    expect(shape(moveStep(s, "steps/2", "always", 0)).slice(-2)).toEqual(["always/0=ask", "always/1=a.yaml"]);
    // Never into itself.
    expect(moveStep(s, "steps/1", "steps/1/else/0/steps", 0)).toBe(s);
    expect(moveStep(s, "steps/9", "steps", 0)).toBe(s);
  });

  it("knows where steps were after they are added, removed or moved", () => {
    const s = nested();
    const moved = (after: Scenario) => Object.fromEntries(stepsMoved(s, after) ?? []);
    // A step that only changed is where it was, and so is everything else.
    expect(stepsMoved(s, s)).toBeNull();
    expect(stepsMoved(s, editStep(s, "steps/1/else/0/steps/0", (step) => ({ ...step, name: "D" })))).toBeNull();
    expect(stepsMoved(s, { ...s, docs: "x" })).toBeNull();
    // Removed: the steps after it move up, those inside it are gone.
    expect(moved(removeStep(s, "steps/1"))).toEqual({ "steps/0": "steps/0", "steps/1": "steps/2", "always/0": "always/0" });
    // Added: the new step was nowhere.
    const added = moved(insertSteps(s, "steps", 0, [newStep("wait")]));
    expect(added["steps/0"]).toBeUndefined();
    expect(added["steps/2/else/0/steps/1"]).toBe("steps/1/else/0/steps/1");
    expect(moved(duplicateStep(s, "steps/0"))).toMatchObject({ "steps/0": "steps/0", "steps/2": "steps/1", "steps/3": "steps/2" });
    expect(moved(duplicateStep(s, "steps/0"))["steps/1"]).toBeUndefined();
    // Moved into a later step: that step and what it holds move up with it.
    expect(moved(moveStep(s, "steps/0", "steps/1/steps", 1))).toMatchObject({
      "steps/0": "steps/1",
      "steps/0/steps/0": "steps/1/steps/0",
      "steps/0/steps/1": "steps/0",
      "steps/0/elseIf/0/0": "steps/1/elseIf/0/0",
      "steps/0/else/0": "steps/1/else/0",
      "steps/1": "steps/2",
    });
    // Out of a nested list: the steps that held it stay known.
    expect(moved(moveStep(s, "steps/1/else/0/steps/0", "steps", 1))).toMatchObject({
      "steps/1": "steps/1/else/0/steps/0",
      "steps/2": "steps/1",
      "steps/2/else/0": "steps/1/else/0",
      "steps/2/else/0/steps/0": "steps/1/else/0/steps/1",
    });
    // An "else if" that goes takes its steps along; the ones after it follow.
    const two = editStep(s, "steps/1", (step) => (step.type === "if" ? { ...step, elseIf: [{ when: { rules: [] }, steps: [request("x.yaml")] }, ...(step.elseIf ?? [])] } : step));
    const one = editStep(two, "steps/1", (step) => (step.type === "if" ? { ...step, elseIf: step.elseIf?.slice(1) } : step));
    expect(Object.fromEntries(stepsMoved(two, one) ?? [])).toMatchObject({ "steps/1": "steps/1", "steps/1/elseIf/0/0": "steps/1/elseIf/1/0" });

    // What is kept by path follows.
    const after = moveStep(s, "steps/0", "steps/1/steps", 1);
    expect(followSteps({ "steps/0": true, "steps/1": false, "steps/9": true }, stepsMoved(s, after)!)).toEqual({ "steps/0/steps/1": true, "steps/0": false });
  });

  it("follows requests that were renamed or moved", () => {
    const s = nested();
    const moved = retarget(s, "d.yaml", "Carts/d.yaml");
    expect(shape(moved)).toContain("steps/1/else/0/steps/0=Carts/d.yaml");
    expect(retarget(s, "nope.yaml", "x.yaml")).toBe(s);
    const inFolder = newScenario("S", ["Users/a.yaml", "Users/Admin/b.yaml", "Other/c.yaml"]);
    expect(retarget(inFolder, "Users", "People").steps.map((x) => (x.type === "request" ? x.request : ""))).toEqual(["People/a.yaml", "People/Admin/b.yaml", "Other/c.yaml"]);
  });
});

describe("what steps are called", () => {
  it("their name, or what they do", () => {
    const s = nested();
    expect(stepTitle(s.steps[0], requests)).toBe("A");
    expect(stepTitle({ ...s.steps[0], name: " Create " }, requests)).toBe("Create");
    expect(stepTitle(request("x/Missing one.yaml"), requests)).toBe("Missing one");
    expect(stepTitle(request(""), requests)).toBe("Choose a request");
    expect(stepTitle(s.steps[1])).toBe("If {{cartId}} is greater than 5");
    expect(conditionText((s.steps[1] as Extract<Step, { type: "if" }>).elseIf![0].when)).toBe("{{x}} exists or {{y}} equals ");
    expect(conditionText({ rules: [{ value: "" }] })).toBe("always");
    expect(stepTitle({ type: "forEach", list: " {{items}} ", steps: [] })).toBe("For each item in {{items}}");
    expect(stepTitle({ type: "repeat", times: 1, steps: [] })).toBe("Repeat 1 time");
    expect(stepTitle({ type: "repeat", times: 5, until: { rules: [{ value: "{{done}}", op: "isTrue" }] }, steps: [] })).toBe("Repeat until {{done}} is true");
    expect(stepTitle({ type: "set", values: [{ key: "a", value: "1" }, { key: "b", value: "", enabled: false }, { key: "c", value: "" }] })).toBe("Set a, c");
    expect(stepTitle({ type: "stop", what: "loop" })).toBe("Leave the loop");
    expect(stepTitle({ type: "scenario", scenario: "login" }, requests, [{ id: "login", name: "Log in", steps: 2, seq: 0 }])).toBe("Run scenario Log in");
    expect(stepTitle({ type: "ask", variable: "otp" })).toBe("Ask for otp");
  });
});

describe("the values a step can use", () => {
  it("the scenario's variables, what earlier steps save, and the loops it is in", () => {
    const s = nested();
    const names = (path: string | null) => valuesAt(s, path, requests).map((v) => v.name);
    // The first step: only the starting values (not its own output).
    expect(names("steps/0")).toEqual(["base"]);
    expect(names("steps/1/steps/0")).toEqual(["base", "cartId"]);
    // Inside the loop: its item and index; a later step of the loop also what an earlier one set.
    expect(names("steps/1/else/0/steps/0")).toEqual(["base", "cartId", "row", "index"]);
    expect(names("steps/1/else/0/steps/1")).toEqual(["base", "cartId", "row", "index"]);
    // After the loop: what its steps saved, not its item.
    expect(names("steps/2")).toEqual(["base", "cartId", "seen"]);
    expect(names("always/0")).toEqual(["base", "cartId", "seen", "otp"]);
    expect(valuesAt(s, "steps/2", requests).find((v) => v.name === "cartId")?.from).toBe("A");
    expect(valuesAt(s, "steps/2", requests).find((v) => v.name === "base")?.from).toBe("Variables");
    // What a switched-off step would save isn't there.
    const off = editStep(s, "steps/0", (step) => ({ ...step, enabled: false }));
    expect(valuesAt(off, "steps/2", requests).map((v) => v.name)).toEqual(["base", "seen"]);
  });

  it("the variables a request uses", () => {
    const r = {
      name: "R",
      seq: 0,
      method: "POST",
      url: "{{base}}/carts/{{cartId}}?t={{$timestamp}}",
      headers: [{ key: "Authorization", value: "Bearer {{token}}" }],
      body: { type: "json", text: '{"id": "{{ cartId }}", "who": "{{user.name}}"}' },
      docs: "Uses {{notThisOne}}",
    } as Request;
    expect(requestVariables(r)).toEqual(["base", "cartId", "token", "user.name"]);
  });
});

describe("picking an output from a response", () => {
  it("lists every value with a path that picks it", () => {
    const leaves = jsonLeaves('{"id": 7, "user": {"name": "Ada", "odd key": true}, "items": [{"id": 1}, "x"], "none": null}');
    expect(leaves.map((l) => `${l.path} → ${l.preview}`)).toEqual([
      "$.id → 7",
      '$.user → {"name":"Ada","odd key":true}',
      "$.user.name → Ada",
      '$.user["odd key"] → true',
      '$.items → [{"id":1},"x"]',
      '$.items[0] → {"id":1}',
      "$.items[0].id → 1",
      "$.items[1] → x",
      "$.none → null",
    ]);
    expect(jsonLeaves("not json")).toEqual([]);
    expect(jsonLeaves(JSON.stringify({ list: Array.from({ length: 50 }, (_, i) => i) }), 10).length).toBe(10);
  });

  it("suggests a name for it", () => {
    expect(nameForPath("$.token")).toBe("token");
    expect(nameForPath("$.data.user.id")).toBe("userId");
    expect(nameForPath("$.items[0].id")).toBe("itemId");
    expect(nameForPath("$.id")).toBe("id");
    expect(nameForPath("$.id", ["id", "id2"])).toBe("id3");
    expect(nameForPath('$["odd key"]')).toBe("oddkey");
    expect(nameForPath("$[0]")).toBe("value");
  });
});

describe("problems", () => {
  it("a step's own", () => {
    const check = (step: Step) => stepProblem(step, requests, [{ id: "login", name: "Log in", steps: 1, seq: 0 }], "self");
    expect(check(request("a.yaml"))).toBeNull();
    expect(check(request(""))).toBe("Choose a request");
    expect(check(request("gone.yaml"))).toBe("The request is no longer in the collection");
    expect(check(request("a.yaml", { outputs: [{ variable: "", path: "$.id" }] }))).toBe("An output: give the variable a name");
    expect(check(request("a.yaml", { outputs: [{ variable: "two words", path: "$.id" }] }))).toBe("An output: a variable name has no spaces or braces");
    expect(check(request("a.yaml", { outputs: [{ variable: "x", from: "header", path: " " }] }))).toBe("An output: give the header");
    expect(check(request("a.yaml", { outputs: [{ variable: "x", from: "status" }] }))).toBeNull();
    expect(check(request("a.yaml", { checks: [{ path: "" }] }))).toBe("A check needs its path, header or pattern");
    expect(check(request("a.yaml", { checks: [{ from: "status", value: "200" }, { path: "", enabled: false }] }))).toBeNull();
    expect(check(request("a.yaml", { inputs: [{ key: "a b", value: "1" }] }))).toBe("An input needs a name without spaces or braces");
    expect(check({ type: "forEach", list: "", steps: [] })).toBe("Give the list to go through");
    expect(check({ type: "scenario", scenario: "" })).toBe("Choose a scenario");
    expect(check({ type: "scenario", scenario: "self" })).toBe("A scenario can't run itself");
    expect(check({ type: "scenario", scenario: "gone" })).toBe("That scenario is no longer there");
    expect(check({ type: "scenario", scenario: "login" })).toBeNull();
    expect(check({ type: "ask", variable: "" })).toBe("Give the variable the answer goes in");
    expect(check({ type: "set", values: [{ key: "", value: "" }] })).toBeNull();
  });

  it("why a scenario can't run", () => {
    expect(startProblem(newScenario("S"), requests, [])).toBe("Add a step");
    expect(startProblem(newScenario("S", ["a.yaml"]), requests, [])).toBeNull();
    const off = { ...newScenario("S", ["a.yaml"]), steps: [{ ...request("a.yaml"), enabled: false }] };
    expect(startProblem(off, requests, [])).toBe("Switch a step on");
    expect(startProblem(newScenario("S", ["a.yaml", "gone.yaml"]), requests, [])).toBe("gone: the request is no longer in the collection");
    // A problem inside a switched-off step doesn't count.
    const inside: Scenario = { ...newScenario("S", ["a.yaml"]), steps: [request("a.yaml"), { type: "parallel", enabled: false, steps: [request("gone.yaml")] }] };
    expect(startProblem(inside, requests, [])).toBeNull();
    expect(startProblem({ ...newScenario("S", ["a.yaml"]), variables: [{ key: "a b", value: "1" }] }, requests, [])).toBe("A variable needs a name without spaces or braces");
  });
});

describe("results by step", () => {
  it("counts the runs of each of the scenario's own steps", () => {
    const result = (path: string, kind: string, passed: boolean, seq: number) => ({ step: { path, kind, depth: 0 }, passed, seq, skipped: false }) as unknown as RunResult;
    const map = outcomes([
      result("steps/0", "request", true, 1),
      result("steps/1", "forEach", true, 2),
      result("steps/1", "round", true, 3),
      result("steps/1/steps/0", "request", true, 4),
      result("steps/1", "round", true, 5),
      result("steps/1/steps/0", "request", false, 6),
      result("steps/2>steps/0", "request", true, 7),
      { passed: true, seq: 8 } as RunResult,
    ]);
    expect([...map.keys()]).toEqual(["steps/0", "steps/1", "steps/1/steps/0"]);
    expect(map.get("steps/1/steps/0")).toMatchObject({ runs: 2, failed: 1 });
    expect(map.get("steps/1/steps/0")?.last.seq).toBe(6);
    expect(map.get("steps/1")).toMatchObject({ runs: 1, failed: 0 });
    // Steps moved since the run: each result is with its step, a new step has none.
    const moved = outcomes([result("steps/0", "request", true, 1), result("steps/1", "request", false, 2)], { "steps/0": "steps/1", "steps/2": "steps/0" });
    expect([...moved].map(([path, o]) => [path, o.last.seq])).toEqual([
      ["steps/0", 2],
      ["steps/2", 1],
    ]);
  });
});

describe("folders of scenarios", () => {
  const node = (id: string, extra: Partial<ScenarioNode> = {}): ScenarioNode => ({ id, name: id.split("/").pop()!, steps: 1, seq: 0, ...extra });
  const tree: ScenarioNode[] = [
    node("Smoke"),
    node("Shop-", { name: "Shop: EU", folder: true, children: [node("Shop-/Auth", { folder: true, children: [node("Shop-/Auth/Log in")] }), node("Shop-/Checkout")] }),
    node("Empty", { folder: true }),
  ];

  it("list every scenario, every folder, and the folders a scenario is in", () => {
    expect(flattenScenarios(tree).map((n) => n.id)).toEqual(["Smoke", "Shop-/Auth/Log in", "Shop-/Checkout"]);
    expect(scenarioFolders(tree)).toEqual([
      { id: "Shop-", name: "Shop: EU", depth: 0 },
      { id: "Shop-/Auth", name: "Auth", depth: 1 },
      { id: "Empty", name: "Empty", depth: 0 },
    ]);
    expect(folderNames("Shop-/Auth/Log in", tree)).toEqual(["Shop: EU", "Auth"]);
    expect(folderNames("Smoke", tree)).toEqual([]);
    expect(folderNames("Gone/Away", tree)).toEqual(["Gone"]);
    expect([scenarioParent("Shop-/Auth/Log in"), scenarioParent("Smoke")]).toEqual(["Shop-/Auth", ""]);
  });

  it("filter by name, keeping the folders on the way", () => {
    const shape = (nodes: ScenarioNode[]): string[] => nodes.flatMap((n) => [n.id, ...shape(n.children ?? [])]);
    expect(shape(filterScenarios(tree, "log"))).toEqual(["Shop-", "Shop-/Auth", "Shop-/Auth/Log in"]);
    // A folder whose name matches comes whole.
    expect(shape(filterScenarios(tree, "eu"))).toEqual(["Shop-", "Shop-/Auth", "Shop-/Auth/Log in", "Shop-/Checkout"]);
    expect(filterScenarios(tree, "")).toBe(tree);
    expect(filterScenarios(tree, "nothing")).toEqual([]);
  });

  it("follow a scenario or a folder to its new id, in the steps that run them too", () => {
    expect(movedId("Shop/Log in", "Shop/Log in", "Auth/Log in", false)).toBe("Auth/Log in");
    expect(movedId("Shop/Log in", "Shop", "Store", true)).toBe("Store/Log in");
    expect(movedId("Shopping/Log in", "Shop", "Store", true)).toBeNull();
    expect(movedId("Shop", "Shop", "Store", true)).toBeNull();
    const s: Scenario = {
      name: "Main",
      seq: 0,
      steps: [{ type: "scenario", scenario: "Shop/Log in" }, { type: "if", when: { rules: [] }, steps: [{ type: "scenario", scenario: "Shop/Pay" }] }, request("a.yaml")],
    };
    const moved = retargetCalls(s, (id) => movedId(id, "Shop", "Store", true));
    expect(moved.steps[0]).toEqual({ type: "scenario", scenario: "Store/Log in" });
    expect(moved.steps[1].type === "if" && moved.steps[1].steps).toEqual([{ type: "scenario", scenario: "Store/Pay" }]);
    expect(moved.steps[2]).toBe(s.steps[2]);
    expect(retargetCalls(s, (id) => movedId(id, "Other", "Else", true))).toBe(s);
  });
});

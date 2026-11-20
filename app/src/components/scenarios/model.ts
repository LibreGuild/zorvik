// What the scenario editor works out without the app: steps by their path, the lists of
// steps a step holds, moving and changing steps, the values a step can use, what a step
// is called, what keeps a scenario from running, and the folders scenarios are in.
import type { CompareOp } from "../../bindings/CompareOp";
import type { Condition } from "../../bindings/Condition";
import type { Request } from "../../bindings/Request";
import type { RunResult } from "../../bindings/RunResult";
import type { Scenario } from "../../bindings/Scenario";
import type { ScenarioNode } from "../../bindings/ScenarioNode";
import type { Step } from "../../bindings/Step";
import type { StepKind } from "../../bindings/StepKind";
import type { StepOutput } from "../../bindings/StepOutput";
import type { StepValue } from "../../bindings/StepValue";
import type { ValueFrom } from "../../bindings/ValueFrom";
import { variableNames } from "../../lib/vars";
import type { RequestEntry } from "../loadtests/model";

export type StepType = Step["type"];

/** The kinds of step, in the order the "Add step" menu lists them. */
export const STEP_TYPES: { type: StepType; label: string; about: string; group: "send" | "flow" | "other" }[] = [
  { type: "request", label: "Request", about: "Send a saved request", group: "send" },
  { type: "scenario", label: "Run scenario", about: "Run another scenario with its own values", group: "send" },
  { type: "if", label: "If", about: "Run steps only when a condition holds", group: "flow" },
  { type: "forEach", label: "For each", about: "Run steps once per item of a list", group: "flow" },
  { type: "repeat", label: "Repeat", about: "Run steps several times, or until a condition holds", group: "flow" },
  { type: "parallel", label: "At the same time", about: "Run steps together", group: "flow" },
  { type: "set", label: "Set values", about: "Set or work out values", group: "other" },
  { type: "wait", label: "Wait", about: "Pause", group: "other" },
  { type: "stop", label: "Stop", about: "Leave a loop or end the scenario", group: "other" },
  { type: "script", label: "Script", about: "JavaScript with the pm API", group: "other" },
  { type: "ask", label: "Ask for a value", about: "Pause and ask you (a one-time code)", group: "other" },
];

export const stepLabel = (type: StepType | StepKind) => (type === "round" ? "Round" : (STEP_TYPES.find((t) => t.type === type)?.label ?? type));

/** Where an output or a check reads its value. */
export const SOURCES: { id: ValueFrom; label: string; placeholder: string; path: boolean }[] = [
  { id: "json", label: "JSON path", placeholder: "$.id", path: true },
  { id: "header", label: "Header", placeholder: "ETag", path: true },
  { id: "cookie", label: "Cookie", placeholder: "session", path: true },
  { id: "regex", label: "Regex", placeholder: "token=(\\w+)", path: true },
  { id: "status", label: "Status", placeholder: "", path: false },
  { id: "body", label: "Body", placeholder: "", path: false },
  { id: "time", label: "Time (ms)", placeholder: "", path: false },
];

/** How two values are compared; `value`: whether there is a right-hand side. */
export const OPS: { id: CompareOp; label: string; value: boolean }[] = [
  { id: "equals", label: "equals", value: true },
  { id: "notEquals", label: "does not equal", value: true },
  { id: "contains", label: "contains", value: true },
  { id: "notContains", label: "does not contain", value: true },
  { id: "matches", label: "matches (regex)", value: true },
  { id: "greater", label: "is greater than", value: true },
  { id: "greaterOrEqual", label: "is at least", value: true },
  { id: "less", label: "is less than", value: true },
  { id: "lessOrEqual", label: "is at most", value: true },
  { id: "in", label: "is one of", value: true },
  { id: "notIn", label: "is none of", value: true },
  { id: "exists", label: "exists", value: false },
  { id: "notExists", label: "does not exist", value: false },
  { id: "isTrue", label: "is true", value: false },
  { id: "isFalse", label: "is false", value: false },
];

const opInfo = (op: CompareOp | undefined) => OPS.find((o) => o.id === (op ?? "equals")) ?? OPS[0];
export const opTakesValue = (op: CompareOp | undefined) => opInfo(op).value;

export function newScenario(name: string, requests: string[] = []): Scenario {
  return { name, seq: 0, steps: requests.map((request) => ({ type: "request", request })) };
}

/** A new step of a kind, with what it needs to be filled in. */
export function newStep(type: StepType): Step {
  switch (type) {
    case "request":
      return { type, request: "" };
    case "if":
      return { type, when: { rules: [{ value: "" }] }, steps: [] };
    case "forEach":
      return { type, list: "", steps: [] };
    case "repeat":
      return { type, times: 3, steps: [] };
    case "parallel":
      return { type, steps: [] };
    case "set":
      return { type, values: [{ key: "", value: "" }] };
    case "wait":
      return { type, ms: 1000 };
    case "stop":
      return { type, what: "scenario" };
    case "scenario":
      return { type, scenario: "" };
    case "script":
      return { type, code: "" };
    case "ask":
      return { type, variable: "" };
  }
}

// ---- paths ---------------------------------------------------------------------------------

/** A list of steps inside a step: its part of a path (`steps`, `elseIf/0`, `else`) and what it is. */
export interface StepList {
  part: string;
  label: string;
  steps: Step[];
}

/** The lists of steps a step holds (none for steps that hold no others). */
export function stepLists(step: Step): StepList[] {
  switch (step.type) {
    case "if":
      return [
        { part: "steps", label: "Then", steps: step.steps ?? [] },
        ...(step.elseIf ?? []).map((b, i) => ({ part: `elseIf/${i}`, label: `Else if ${conditionText(b.when)}`, steps: b.steps ?? [] })),
        { part: "else", label: "Else", steps: step.else ?? [] },
      ];
    case "forEach":
    case "repeat":
    case "parallel":
      return [{ part: "steps", label: "", steps: step.steps ?? [] }];
    default:
      return [];
  }
}

/** Every step with its path (`steps/2`, `steps/2/else/0`, `always/0`), outer steps first. */
export function walkSteps(scenario: Scenario, visit: (path: string, step: Step) => void) {
  const go = (prefix: string, steps: Step[]) =>
    steps.forEach((step, i) => {
      const path = `${prefix}/${i}`;
      visit(path, step);
      for (const list of stepLists(step)) go(`${path}/${list.part}`, list.steps);
    });
  go("steps", scenario.steps ?? []);
  go("always", scenario.always ?? []);
}

export function stepAt(scenario: Scenario, path: string): Step | undefined {
  let found: Step | undefined;
  walkSteps(scenario, (p, step) => {
    if (p === path) found = step;
  });
  return found;
}

export function countSteps(scenario: Scenario): number {
  let n = 0;
  walkSteps(scenario, () => n++);
  return n;
}

/** The list a step is in, and its place there: `steps/2/else/0` → [`steps/2/else`, 0]. */
export function parentOf(path: string): [string, number] {
  const cut = path.lastIndexOf("/");
  return [path.slice(0, cut), Number(path.slice(cut + 1))];
}

function inList(list: Step[], tokens: string[], fn: (list: Step[]) => Step[]): Step[] {
  if (!tokens.length) return fn(list);
  const at = Number(tokens[0]);
  return list.map((step, i) => (i === at ? inStep(step, tokens.slice(1), fn) : step));
}

function inStep(step: Step, tokens: string[], fn: (list: Step[]) => Step[]): Step {
  const [part, ...rest] = tokens;
  if (step.type === "if") {
    if (part === "steps") return { ...step, steps: inList(step.steps ?? [], rest, fn) };
    if (part === "else") return { ...step, else: inList(step.else ?? [], rest, fn) };
    if (part === "elseIf") {
      const at = Number(rest[0]);
      return { ...step, elseIf: (step.elseIf ?? []).map((b, i) => (i === at ? { ...b, steps: inList(b.steps ?? [], rest.slice(1), fn) } : b)) };
    }
    return step;
  }
  if ((step.type === "forEach" || step.type === "repeat" || step.type === "parallel") && part === "steps") {
    return { ...step, steps: inList(step.steps ?? [], rest, fn) };
  }
  return step;
}

/** The scenario with the list at `listPath` (`steps`, `always`, `steps/2/else`) changed. */
export function editList(scenario: Scenario, listPath: string, fn: (list: Step[]) => Step[]): Scenario {
  const [root, ...rest] = listPath.split("/");
  if (root === "always") return { ...scenario, always: inList(scenario.always ?? [], rest, fn) };
  return { ...scenario, steps: inList(scenario.steps ?? [], rest, fn) };
}

/** The scenario with the step at `path` changed. */
export function editStep(scenario: Scenario, path: string, fn: (step: Step) => Step): Scenario {
  const [list, at] = parentOf(path);
  return editList(scenario, list, (steps) => steps.map((step, i) => (i === at ? fn(step) : step)));
}

export function removeStep(scenario: Scenario, path: string): Scenario {
  const [list, at] = parentOf(path);
  return editList(scenario, list, (steps) => steps.filter((_, i) => i !== at));
}

export function insertSteps(scenario: Scenario, listPath: string, index: number, steps: Step[]): Scenario {
  return editList(scenario, listPath, (list) => {
    const at = Math.max(0, Math.min(index, list.length));
    return [...list.slice(0, at), ...steps, ...list.slice(at)];
  });
}

export function duplicateStep(scenario: Scenario, path: string): Scenario {
  const step = stepAt(scenario, path);
  if (!step) return scenario;
  const [list, at] = parentOf(path);
  return insertSteps(scenario, list, at + 1, [structuredClone(step)]);
}

/** Every list of the scenario, the innermost first, changed by `fn`. */
function mapLists(scenario: Scenario, fn: (list: Step[]) => Step[]): Scenario {
  const list = (steps: Step[] | undefined): Step[] => fn((steps ?? []).map(step));
  const step = (s: Step): Step => {
    if (s.type === "if") {
      return { ...s, steps: list(s.steps), ...(s.elseIf ? { elseIf: s.elseIf.map((b) => ({ ...b, steps: list(b.steps) })) } : {}), ...(s.else ? { else: list(s.else) } : {}) };
    }
    if (s.type === "forEach" || s.type === "repeat" || s.type === "parallel") return { ...s, steps: list(s.steps) };
    return s;
  };
  return { ...scenario, steps: list(scenario.steps), ...(scenario.always ? { always: list(scenario.always) } : {}) };
}

/** The steps of the list at `listPath` (none when there is no such list). */
export function listAt(scenario: Scenario, listPath: string): Step[] {
  let found: Step[] = [];
  editList(scenario, listPath, (list) => (found = list));
  return found;
}

/**
 * Move the step at `from` to place `index` of the list at `toList`. A step can't move into
 * itself; the scenario comes back unchanged then.
 */
export function moveStep(scenario: Scenario, from: string, toList: string, index: number): Scenario {
  const step = stepAt(scenario, from);
  if (!step || toList === from || toList.startsWith(`${from}/`)) return scenario;
  const [fromList, fromIndex] = parentOf(from);
  const at = Math.max(0, Math.min(index, listAt(scenario, toList).length));
  if (fromList === toList && (at === fromIndex || at === fromIndex + 1)) return scenario;
  // Put it in first, then take the original out where it is now: one place further down
  // its list when the new one went in before it (or before a step it is inside).
  let source = from;
  const prefix = `${toList}/`;
  if (from.startsWith(prefix)) {
    const rest = from.slice(prefix.length);
    const slash = rest.indexOf("/");
    const place = Number(slash < 0 ? rest : rest.slice(0, slash));
    if (place >= at) source = `${prefix}${place + 1}${slash < 0 ? "" : rest.slice(slash)}`;
  }
  return removeStep(insertSteps(scenario, toList, at, [step]), source);
}

/** The step that holds the list a path is in: `steps/1/elseIf/0/2` → `steps/1` (none for the scenario's own lists). */
function holderOf(path: string): string | null {
  const tokens = path.split("/").slice(0, -1);
  tokens.splice(tokens[tokens.length - 2] === "elseIf" ? -2 : -1);
  return tokens.length ? tokens.join("/") : null;
}

/**
 * Where the steps of `after` were in `before` (path now → path then), so that what is known
 * about a step by its path (its result, whether it is open) follows it when steps are added,
 * removed or moved. A step is the same object until it changes: one in a new place is found
 * by itself, one that changed by its place, and one that holds others by the steps it still
 * holds. A new step is in none. `null`: every step is where it was.
 */
export function stepsMoved(before: Scenario, after: Scenario): Map<string, string> | null {
  if (before.steps === after.steps && before.always === after.always) return null;
  const was = new Map<Step, string>();
  const old = new Map<string, Step>();
  walkSteps(before, (path, step) => {
    was.set(step, path);
    old.set(path, step);
  });
  const now = new Set<Step>();
  walkSteps(after, (_, step) => now.add(step));
  const gone = (path: string | null | undefined) => {
    const step = path ? old.get(path) : undefined;
    return !!step && !now.has(step);
  };
  const from = new Map<string, string>();
  walkSteps(after, (path, step) => {
    const known = was.get(step);
    if (known !== undefined) return void from.set(path, known);
    if (gone(path)) return void from.set(path, path);
    for (const list of stepLists(step)) {
      for (const child of list.steps) {
        const held = was.get(child);
        const holder = held === undefined ? null : holderOf(held);
        if (holder && gone(holder)) return void from.set(path, holder);
      }
    }
  });
  const same = from.size === old.size && now.size === old.size && [...from].every(([to, path]) => to === path);
  return same ? null : from;
}

/** What was kept by step path, after the steps moved (see `stepsMoved`). */
export function followSteps<T>(byPath: Record<string, T>, moved: Map<string, string>): Record<string, T> {
  const out: Record<string, T> = {};
  for (const [to, path] of moved) if (path in byPath) out[to] = byPath[path];
  return out;
}

/** Requests renamed or moved in the collection: the steps that send them follow. */
export function retarget(scenario: Scenario, oldPath: string, newPath: string): Scenario {
  const moved = (p: string) => (p === oldPath ? newPath : p.startsWith(`${oldPath}/`) ? newPath + p.slice(oldPath.length) : p);
  let changed = false;
  const next = mapLists(scenario, (list) =>
    list.map((s) => {
      if (s.type !== "request" || moved(s.request) === s.request) return s;
      changed = true;
      return { ...s, request: moved(s.request) };
    }),
  );
  return changed ? next : scenario;
}

// ---- folders -------------------------------------------------------------------------------

/** The folder a scenario or folder is in, by id (`Shop/Auth/Log in` → `Shop/Auth`; "" the top level). */
export const scenarioParent = (id: string) => (id.includes("/") ? id.slice(0, id.lastIndexOf("/")) : "");

/**
 * `id` once the scenario `oldId` got the id `newId`, or (`folder`) once the folder `oldId` did and
 * everything in it moved along; null when `id` is not one of them.
 */
export function movedId(id: string, oldId: string, newId: string, folder: boolean): string | null {
  if (folder) return id.startsWith(`${oldId}/`) ? newId + id.slice(oldId.length) : null;
  return id === oldId ? newId : null;
}

/** The steps that run a scenario follow it to its new id (`moved`: the new id, or null when it didn't move). */
export function retargetCalls(scenario: Scenario, moved: (id: string) => string | null): Scenario {
  let changed = false;
  const next = mapLists(scenario, (list) =>
    list.map((s) => {
      const to = s.type === "scenario" ? moved(s.scenario) : null;
      if (s.type !== "scenario" || to === null || to === s.scenario) return s;
      changed = true;
      return { ...s, scenario: to };
    }),
  );
  return changed ? next : scenario;
}

/** Every scenario of the tree (not the folders), in sidebar order. */
export function flattenScenarios(nodes: ScenarioNode[]): ScenarioNode[] {
  return nodes.flatMap((n) => (n.folder ? flattenScenarios(n.children ?? []) : [n]));
}

/** The folders of the tree, outer ones first, each with how deep it is. */
export function scenarioFolders(nodes: ScenarioNode[], depth = 0): { id: string; name: string; depth: number }[] {
  return nodes.filter((n) => n.folder).flatMap((n) => [{ id: n.id, name: n.name, depth }, ...scenarioFolders(n.children ?? [], depth + 1)]);
}

/** The tree with the scenarios whose name has `q` (lower case), and the folders they are in; a folder whose name has it, whole. */
export function filterScenarios(nodes: ScenarioNode[], q: string): ScenarioNode[] {
  if (!q) return nodes;
  return nodes.flatMap((n) => {
    if (!n.folder) return n.name.toLowerCase().includes(q) ? [n] : [];
    if (n.name.toLowerCase().includes(q)) return [n];
    const children = filterScenarios(n.children ?? [], q);
    return children.length ? [{ ...n, children }] : [];
  });
}

/** The names of the folders the scenario `id` is in, outer ones first (`Shop/Auth/Log in` → Shop, Auth). */
export function folderNames(id: string, tree: ScenarioNode[]): string[] {
  const names: string[] = [];
  let level = tree;
  for (const part of scenarioParent(id).split("/").filter(Boolean)) {
    const folder = level.find((n) => n.folder && n.id.split("/").pop() === part);
    names.push(folder?.name ?? part);
    level = folder?.children ?? [];
  }
  return names;
}

// ---- what steps are called -----------------------------------------------------------------

const short = (text: string, max = 60) => (text.length > max ? `${text.slice(0, max)}…` : text);

export function ruleText(rule: { value: string; op?: CompareOp; to?: string }): string {
  const op = opInfo(rule.op);
  const label = op.id === "matches" ? "matches" : op.label;
  return op.value ? `${short(rule.value.trim())} ${label} ${short((rule.to ?? "").trim())}` : `${short(rule.value.trim())} ${label}`;
}

export function conditionText(condition: Condition | undefined): string {
  const rules = (condition?.rules ?? []).filter((r) => r.value.trim() || (r.to ?? "").trim());
  if (!rules.length) return "always";
  return rules.map(ruleText).join(condition?.any ? " or " : " and ");
}

export const nameFromPath = (path: string) => path.split("/").pop()?.replace(/\.yaml$/i, "") ?? path;

/** What a step is called when it has no name of its own (as the results call it). */
export function stepTitle(step: Step, requests?: Map<string, RequestEntry>, scenarios?: ScenarioNode[]): string {
  if (step.name?.trim()) return step.name.trim();
  switch (step.type) {
    case "request":
      return step.request ? (requests?.get(step.request)?.name ?? nameFromPath(step.request)) : "Choose a request";
    case "if":
      return `If ${conditionText(step.when)}`;
    case "forEach":
      return `For each ${step.as?.trim() || "item"} in ${short(step.list.trim()) || "…"}`;
    case "repeat":
      return step.until ? `Repeat until ${conditionText(step.until)}` : `Repeat ${step.times} ${step.times === 1 ? "time" : "times"}`;
    case "parallel":
      return "At the same time";
    case "set": {
      const keys = (step.values ?? []).filter((v) => v.enabled !== false && v.key.trim()).map((v) => v.key.trim());
      return keys.length ? `Set ${keys.join(", ")}` : "Set values";
    }
    case "wait":
      return `Wait ${step.ms} ms`;
    case "stop":
      return step.what === "loop" ? "Leave the loop" : step.what === "fail" ? "Fail the scenario" : "End the scenario";
    case "scenario": {
      const found = scenarios?.find((s) => s.id === step.scenario);
      return step.scenario ? `Run scenario ${found?.name ?? step.scenario}` : "Choose a scenario";
    }
    case "script":
      return "Script";
    case "ask":
      return `Ask for ${step.variable.trim() || "a value"}`;
  }
}

// ---- values a step can use -----------------------------------------------------------------

export interface KnownValue {
  name: string;
  /** Where it comes from: "Variables", or the step that saves it. */
  from: string;
}

/** The names a step makes available to the steps after it. */
function savedBy(step: Step): string[] {
  const names = (values: StepValue[] | undefined) => (values ?? []).filter((v) => v.enabled !== false).map((v) => v.key.trim());
  switch (step.type) {
    case "request":
      return (step.outputs ?? []).map((o) => o.variable.trim());
    case "set":
      return names(step.values);
    case "scenario":
      return names(step.outputs);
    case "ask":
      return [step.variable.trim()];
    default:
      return [];
  }
}

/**
 * The scenario's values a step at `path` can use: its variables, what the steps before it
 * save, and the item and index of the loops it is in. A step's own outputs are not listed.
 * With no `path`: everything any step saves.
 */
export function valuesAt(scenario: Scenario, path: string | null, requests?: Map<string, RequestEntry>, scenarios?: ScenarioNode[]): KnownValue[] {
  const out = new Map<string, KnownValue>();
  const add = (name: string, from: string) => {
    if (name && !out.has(name)) out.set(name, { name, from });
  };
  for (const v of scenario.variables ?? []) if (v.enabled !== false) add(v.key.trim(), "Variables");
  let reached = false;
  walkSteps(scenario, (p, step) => {
    if (p === path) reached = true;
    if (reached) return;
    const title = stepTitle(step, requests, scenarios);
    // A loop this step is in gives its item and index; one it is not in gives nothing after it.
    const inside = path !== null && path.startsWith(`${p}/`);
    if (step.type === "forEach" && (inside || path === null)) {
      add(step.as?.trim() || "item", title);
      add("index", title);
    }
    if (step.type === "repeat" && (inside || path === null)) add("index", title);
    if (step.enabled !== false) for (const name of savedBy(step)) add(name, title);
  });
  return [...out.values()];
}

/** The `{{variables}}` a saved request uses (not the dynamic `{{$…}}` ones), each once. */
export function requestVariables(request: Request): string[] {
  const names = variableNames(JSON.stringify({ ...request, docs: undefined, examples: undefined, scripts: undefined }));
  return [...new Set(names.filter((n) => !n.startsWith("$") && /^[\w.-]+$/.test(n)))];
}

// ---- picking an output from a response -------------------------------------------------------

export interface JsonLeaf {
  /** A JSONPath that picks it: `$.items[0].id`. */
  path: string;
  /** The value as text (objects and lists as JSON, cut). */
  preview: string;
}

/** Entries listed at most (a big response has too many to pick from). */
export const MAX_LEAVES = 400;

const plainKey = /^[A-Za-z_][\w]*$/;

/** Every value of a JSON document with the path that picks it; lists and objects come before their items. */
export function jsonLeaves(text: string, max = MAX_LEAVES): JsonLeaf[] {
  let root: unknown;
  try {
    root = JSON.parse(text);
  } catch {
    return [];
  }
  const out: JsonLeaf[] = [];
  const go = (value: unknown, path: string) => {
    if (out.length >= max) return;
    if (value !== null && typeof value === "object") {
      if (path !== "$") out.push({ path, preview: short(JSON.stringify(value), 80) });
      if (Array.isArray(value)) value.forEach((item, i) => go(item, `${path}[${i}]`));
      else for (const [key, item] of Object.entries(value)) go(item, plainKey.test(key) ? `${path}.${key}` : `${path}[${JSON.stringify(key)}]`);
      return;
    }
    out.push({ path, preview: short(typeof value === "string" ? value : String(value), 80) });
  };
  go(root, "$");
  return out;
}

/** A variable name for the value at a JSON path: `$.data.user.id` → `userId`, `$.token` → `token`. */
export function nameForPath(path: string, taken: string[] = []): string {
  const keys = [...path.matchAll(/\.([A-Za-z_]\w*)|\["((?:[^"\\]|\\.)*)"\]/g)].map((m) => (m[1] ?? m[2]).replace(/[^\w]/g, ""));
  const last = keys[keys.length - 1] || "value";
  const before = keys[keys.length - 2];
  // `id`, `name` and the like say little alone: `user.id` is `userId`.
  const generic = ["id", "name", "value", "key", "type", "status", "url"].includes(last.toLowerCase());
  const singular = (word: string) => (word.length > 3 && word.endsWith("s") ? word.slice(0, -1) : word);
  const base = generic && before ? `${singular(before)}${last[0].toUpperCase()}${last.slice(1)}` : last;
  let name = base;
  for (let n = 2; taken.includes(name); n++) name = `${base}${n}`;
  return name;
}

// ---- problems --------------------------------------------------------------------------------

const badName = (name: string) => !name.trim() || /[\s{}]/.test(name.trim());

export function outputProblem(output: StepOutput): string | null {
  if (badName(output.variable)) return output.variable.trim() ? "A variable name has no spaces or braces" : "Give the variable a name";
  const source = SOURCES.find((s) => s.id === (output.from ?? "json")) ?? SOURCES[0];
  if (source.path && !(output.path ?? "").trim()) return `Give the ${source.label.toLowerCase()}`;
  return null;
}

/** What is wrong with one step itself (not with the steps inside it), if anything. */
export function stepProblem(step: Step, requests: Map<string, RequestEntry>, scenarios: ScenarioNode[], ownId?: string): string | null {
  const values = (list: StepValue[] | undefined) => (list ?? []).filter((v) => v.enabled !== false && (v.key.trim() || v.value.trim()));
  switch (step.type) {
    case "request": {
      if (!step.request) return "Choose a request";
      const entry = requests.get(step.request);
      if (!entry) return "The request is no longer in the collection";
      if (entry.error) return "The request's file can't be read";
      if (values(step.inputs).some((v) => badName(v.key))) return "An input needs a name without spaces or braces";
      for (const output of step.outputs ?? []) {
        const problem = outputProblem(output);
        if (problem) return `An output: ${problem.toLowerCase()}`;
      }
      const check = (step.checks ?? []).find((c) => c.enabled !== false && (SOURCES.find((s) => s.id === (c.from ?? "json")) ?? SOURCES[0]).path && !(c.path ?? "").trim());
      if (check) return "A check needs its path, header or pattern";
      return null;
    }
    case "if":
      return null;
    case "forEach":
      if (!step.list.trim()) return "Give the list to go through";
      return badName(step.as ?? "item") ? "The item's name has no spaces or braces" : null;
    case "repeat":
      return step.times < 1 ? "Repeat at least once" : null;
    case "set":
      return values(step.values).some((v) => badName(v.key)) ? "A value needs a name without spaces or braces" : null;
    case "scenario":
      if (!step.scenario) return "Choose a scenario";
      if (step.scenario === ownId) return "A scenario can't run itself";
      if (!scenarios.some((s) => s.id === step.scenario)) return "That scenario is no longer there";
      return values(step.inputs).concat(values(step.outputs)).some((v) => badName(v.key)) ? "A value needs a name without spaces or braces" : null;
    case "ask":
      return badName(step.variable) ? "Give the variable the answer goes in" : null;
    default:
      return null;
  }
}

/** Why the scenario can't run, if it can't (the app checks again when it starts). */
export function startProblem(scenario: Scenario, requests: Map<string, RequestEntry>, scenarios: ScenarioNode[], ownId?: string): string | null {
  const top = [...(scenario.steps ?? []), ...(scenario.always ?? [])];
  if (!top.length) return "Add a step";
  if (top.every((s) => s.enabled === false)) return "Switch a step on";
  if ((scenario.variables ?? []).some((v) => v.enabled !== false && (v.key.trim() || v.value.trim()) && badName(v.key))) return "A variable needs a name without spaces or braces";
  let problem: string | null = null;
  // A switched-off step is left out, with everything inside it.
  const off: string[] = [];
  walkSteps(scenario, (path, step) => {
    if (step.enabled === false) off.push(`${path}/`);
    if (problem || step.enabled === false || off.some((p) => path.startsWith(p))) return;
    const own = stepProblem(step, requests, scenarios, ownId);
    if (own) problem = `${stepTitle(step, requests, scenarios)}: ${own.toLowerCase()}`;
  });
  return problem;
}

// ---- results ---------------------------------------------------------------------------------

export interface StepOutcome {
  /** Results of the step in the latest run (a step in a loop has one per round). */
  runs: number;
  failed: number;
  /** The latest result (its response is what an output can be picked from). */
  last: RunResult;
}

/**
 * The latest run's results by the path of the step they belong to (the scenario's own steps
 * only). `ranAt`: where the steps were when the run started (path now → path then), once
 * steps were added, removed or moved since.
 */
export function outcomes(results: RunResult[], ranAt?: Record<string, string>): Map<string, StepOutcome> {
  const out = new Map<string, StepOutcome>();
  for (const r of results) {
    const step = r.step;
    if (!step || step.kind === "round" || step.path.includes(">")) continue;
    const seen = out.get(step.path);
    out.set(step.path, { runs: (seen?.runs ?? 0) + 1, failed: (seen?.failed ?? 0) + (r.passed ? 0 : 1), last: r });
  }
  if (!ranAt) return out;
  const moved = new Map<string, StepOutcome>();
  for (const [to, path] of Object.entries(ranAt)) {
    const outcome = out.get(path);
    if (outcome) moved.set(to, outcome);
  }
  return moved;
}

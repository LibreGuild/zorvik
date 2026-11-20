// The steps of a scenario: a list of cards, each a step with its fields; steps that hold
// others (if, loops, at the same time) show their lists inside. Steps are added from the
// collection or a menu, reordered and moved between lists by dragging, and run one at a
// time or up to a point.
import { createContext, useContext, useEffect, useMemo, useState } from "react";
import {
  ArrowDown,
  ArrowUp,
  ChevronRight,
  CircleCheck,
  CircleX,
  CodeXml,
  Copy,
  GitBranch,
  GripVertical,
  ListOrdered,
  MessageCircleQuestionMark,
  MoreHorizontal,
  OctagonX,
  Play,
  Plus,
  Repeat,
  Split,
  StepForward,
  Timer,
  Trash2,
  TriangleAlert,
  Variable,
  Workflow,
} from "lucide-react";
import type { Scenario } from "../../bindings/Scenario";
import type { ScenarioNode } from "../../bindings/ScenarioNode";
import type { Step } from "../../bindings/Step";
import type { TreeNode } from "../../bindings/TreeNode";
import { methodColor, methodLabel } from "../../lib/http";
import { api } from "../../lib/rpc";
import { useScenarios } from "../../store/scenarios";
import { openRequest } from "../../store/tabs";
import { useWorkspace } from "../../store/workspace";
import { CodeEditor } from "../CodeEditor";
import { flattenRequests, type RequestEntry } from "../loadtests/model";
import { RequestPicker } from "../loadtests/TargetsEditor";
import { LocalVariables } from "../VarInput";
import { Button, Checkbox, cx, IconButton, Input, Menu, type MenuEntry, NumberInput } from "../ui";
import {
  duplicateStep,
  editStep,
  folderNames,
  insertSteps,
  listAt,
  moveStep,
  newStep,
  parentOf,
  removeStep,
  requestVariables,
  STEP_TYPES,
  stepLists,
  type StepOutcome,
  stepProblem,
  stepTitle,
  type StepType,
  valuesAt,
} from "./model";
import { ChecksEditor, ConditionEditor, FieldLabel, Option, OutputsEditor, selectClass, ValueInput, ValuesEditor } from "./StepFields";

/** A step being dragged (its path), and a request or folder dragged from the collection. */
const STEP_DRAG = "application/x-zv-step";
const ITEM_DRAG = "application/x-zv-item";
/** The kinds of request a run sends (the others are live sessions). */
const RUNNABLE = new Set(["http", "sse", "websocket", "socketio", "grpc", "mcp"]);
const runnable = (r: RequestEntry) => RUNNABLE.has(r.kind);
const PICKER_WORDS = {
  search: "Search requests",
  none: "No saved requests yet. Save a request to the collection first.",
  note: "A request can be a step as often as you like. TCP, UDP, MQTT and DNS requests can't be steps.",
};
/** Ten minutes: the longest pause. */
const MAX_WAIT = 600_000;

const KIND_ICON: Record<Exclude<StepType, "request">, React.ReactNode> = {
  if: <GitBranch size={13} />,
  forEach: <ListOrdered size={13} />,
  repeat: <Repeat size={13} />,
  parallel: <Split size={13} />,
  set: <Variable size={13} />,
  wait: <Timer size={13} />,
  stop: <OctagonX size={13} />,
  scenario: <Workflow size={13} />,
  script: <CodeXml size={13} />,
  ask: <MessageCircleQuestionMark size={13} />,
};

interface Editor {
  scenario: Scenario;
  scenarioId: string;
  change: (fn: (s: Scenario) => Scenario) => void;
  all: RequestEntry[];
  requests: Map<string, RequestEntry>;
  scenarios: ScenarioNode[];
  /** The latest run's results by step path, and that run. */
  outcomes: Map<string, StepOutcome>;
  runId: string | null;
  run: (part: { to?: string; only?: string }) => void;
  /** Why a part can't run now, if it can't. */
  blocked: string | null;
  open: Record<string, boolean>;
  setOpen: (path: string, open: boolean) => void;
}

const EditorContext = createContext<Editor | null>(null);
const useEditor = () => useContext(EditorContext)!;

const NO_NODES: TreeNode[] = [];

export function StepsEditor({
  scenario,
  scenarioId,
  change,
  scenarios,
  outcomes,
  runId,
  run,
  blocked,
  open,
  setOpen,
}: {
  scenario: Scenario;
  scenarioId: string;
  change: (fn: (s: Scenario) => Scenario) => void;
  scenarios: ScenarioNode[];
  outcomes: Map<string, StepOutcome>;
  runId: string | null;
  run: (part: { to?: string; only?: string }) => void;
  blocked: string | null;
  /** Which steps show their fields (kept with the tab). */
  open: Record<string, boolean>;
  setOpen: (path: string, open: boolean) => void;
}) {
  const tree = useWorkspace((s) => s.info?.tree ?? NO_NODES);
  const all = useMemo(() => flattenRequests(tree), [tree]);
  const requests = useMemo(() => new Map(all.map((r) => [r.path, r])), [all]);
  const editor: Editor = {
    scenario,
    scenarioId,
    change,
    all,
    requests,
    scenarios,
    outcomes,
    runId,
    run,
    blocked,
    open,
    setOpen,
  };
  const steps = scenario.steps ?? [];
  const always = scenario.always ?? [];
  return (
    <EditorContext.Provider value={editor}>
      <div className="flex flex-col gap-4 px-4 pb-8 pt-3" data-testid="scenario-steps">
        {steps.length === 0 && (
          <div className="rounded-xl border border-dashed border-line-strong px-4 py-4 text-center text-[12.5px] leading-relaxed text-muted">
            Add the saved requests this scenario sends, in order. Then save a value from one step's response as an <b className="font-medium text-fg">output</b> and
            use it in the next one as <code className="font-mono">{"{{name}}"}</code>.
          </div>
        )}
        <StepList listPath="steps" steps={steps} />
        <div className="flex flex-col gap-1">
          <FieldLabel hint="Clean up: these run whatever happened before, also after a failed step.">Always at the end</FieldLabel>
          <StepList listPath="always" steps={always} />
        </div>
      </div>
    </EditorContext.Provider>
  );
}

function StepList({ listPath, steps }: { listPath: string; steps: Step[] }) {
  return (
    <div className="flex flex-col gap-1.5" data-testid={`scenario-list-${listPath}`}>
      {steps.map((step, i) => (
        <StepCard key={i} step={step} path={`${listPath}/${i}`} />
      ))}
      <AddStep listPath={listPath} />
    </div>
  );
}

const draggedOver = (e: React.DragEvent) => e.dataTransfer.types.includes(STEP_DRAG) || e.dataTransfer.types.includes(ITEM_DRAG);

/** What was dropped goes to place `index` of the list: a step that was dragged, or a request (or a folder's requests) from the collection. */
function dropInto(ed: Editor, e: React.DragEvent, listPath: string, index: number) {
  const from = e.dataTransfer.getData(STEP_DRAG);
  if (from) {
    ed.change((s) => moveStep(s, from, listPath, index));
    return;
  }
  // A request, or a folder: every request under it a run sends, in sidebar order.
  const item = e.dataTransfer.getData(ITEM_DRAG);
  const paths = ed.all.filter((r) => (r.path === item || r.path.startsWith(`${item}/`)) && runnable(r) && !r.error).map((r) => r.path);
  if (paths.length) ed.change((s) => insertSteps(s, listPath, index, paths.map((request) => ({ type: "request", request }))));
}

function AddStep({ listPath }: { listPath: string }) {
  const ed = useEditor();
  const add = (steps: Step[], open: boolean) => {
    const at = listAt(ed.scenario, listPath).length;
    ed.change((s) => insertSteps(s, listPath, at, steps));
    if (open) ed.setOpen(`${listPath}/${at}`, true);
  };
  const entries: MenuEntry[] = STEP_TYPES.filter((t) => t.type !== "request").flatMap((t, i, list) => [
    ...(i > 0 && list[i - 1].group !== t.group ? [{ separator: true } as MenuEntry] : []),
    { label: t.label, icon: KIND_ICON[t.type as Exclude<StepType, "request">], onSelect: () => add([newStep(t.type)], true) },
  ]);
  // The end of the list takes what is dropped on it (the only place to drop in an empty list).
  const [over, setOver] = useState(false);
  return (
    <div
      className={cx("relative flex items-center gap-1 rounded-lg", over && "bg-accent-soft")}
      data-testid={`scenario-list-end-${listPath}`}
      onDragOver={(e) => {
        if (!draggedOver(e)) return;
        e.preventDefault();
        e.stopPropagation();
        setOver(true);
      }}
      onDragLeave={() => setOver(false)}
      onDrop={(e) => {
        if (!draggedOver(e)) return;
        e.preventDefault();
        e.stopPropagation();
        setOver(false);
        dropInto(ed, e, listPath, listAt(ed.scenario, listPath).length);
      }}
    >
      <RequestPicker
        all={ed.all}
        accept={runnable}
        words={PICKER_WORDS}
        onAdd={(paths) => add(paths.map((request) => ({ type: "request", request })), false)}
        trigger={
          <Button size="sm" variant="ghost" icon={<Plus size={13} />} data-testid={`scenario-add-request-${listPath}`}>
            Request
          </Button>
        }
      />
      <Menu
        entries={entries}
        trigger={
          <Button size="sm" variant="ghost" icon={<Plus size={13} />} data-testid={`scenario-add-step-${listPath}`}>
            Step
          </Button>
        }
      />
    </div>
  );
}

function StepCard({ step, path }: { step: Step; path: string }) {
  const ed = useEditor();
  const title = stepTitle(step, ed.requests, ed.scenarios);
  const problem = stepProblem(step, ed.requests, ed.scenarios, ed.scenarioId);
  const outcome = ed.outcomes.get(path);
  const holds = stepLists(step).length > 0;
  // Steps that hold others open by themselves: what is inside them is the scenario.
  const open = ed.open[path] ?? holds;
  const enabled = step.enabled !== false;
  const update = (fn: (s: Step) => Step) => ed.change((sc) => editStep(sc, path, fn));
  const [list, at] = parentOf(path);
  const siblings = listAt(ed.scenario, list).length;
  const entry = step.type === "request" ? ed.requests.get(step.request) : undefined;
  const known = useMemo(
    () => new Map(valuesAt(ed.scenario, path, ed.requests, ed.scenarios).map((v) => [v.name, v.from])),
    [ed.scenario, path, ed.requests, ed.scenarios],
  );
  const menu: MenuEntry[] = [
    { label: "Run to here", icon: <StepForward size={14} />, disabled: !!ed.blocked || !enabled, onSelect: () => ed.run({ to: path }) },
    { label: "Run only this step", icon: <Play size={14} />, disabled: !!ed.blocked, onSelect: () => ed.run({ only: path }) },
    { separator: true },
    { label: "Duplicate", icon: <Copy size={14} />, onSelect: () => ed.change((s) => duplicateStep(s, path)) },
    { label: "Move up", icon: <ArrowUp size={14} />, disabled: at === 0, onSelect: () => ed.change((s) => moveStep(s, path, list, at - 1)) },
    { label: "Move down", icon: <ArrowDown size={14} />, disabled: at >= siblings - 1, onSelect: () => ed.change((s) => moveStep(s, path, list, at + 2)) },
    { separator: true },
    { label: "Delete", icon: <Trash2 size={14} />, danger: true, onSelect: () => ed.change((s) => removeStep(s, path)) },
  ];
  // Dropped on the upper half of the step's row: before it; on the lower half: after it.
  const [over, setOver] = useState<"before" | "after" | null>(null);
  const half = (e: React.DragEvent): "before" | "after" => {
    const box = e.currentTarget.getBoundingClientRect();
    return e.clientY < box.top + box.height / 2 ? "before" : "after";
  };
  return (
    <div className={cx("relative rounded-xl border bg-bg", problem && enabled ? "border-warning/50" : "border-line")} data-testid="scenario-step" data-path={path}>
      {over && <div className={cx("pointer-events-none absolute inset-x-1 h-[2px] rounded bg-accent", over === "before" ? "-top-1" : "-bottom-1")} />}
      <div
        draggable
        onDragStart={(e) => {
          e.stopPropagation();
          e.dataTransfer.setData(STEP_DRAG, path);
          e.dataTransfer.effectAllowed = "move";
        }}
        onDragOver={(e) => {
          if (!draggedOver(e)) return;
          e.preventDefault();
          e.stopPropagation();
          setOver(half(e));
        }}
        onDragLeave={() => setOver(null)}
        onDrop={(e) => {
          if (!draggedOver(e)) return;
          e.preventDefault();
          e.stopPropagation();
          setOver(null);
          dropInto(ed, e, list, half(e) === "before" ? at : at + 1);
        }}
        className="group flex min-h-[36px] items-center gap-1.5 px-1.5"
        data-testid="scenario-step-row"
      >
        <GripVertical size={13} className="shrink-0 cursor-grab text-faint" aria-hidden />
        <Checkbox checked={enabled} onChange={(v) => update((s) => ({ ...s, enabled: v ? undefined : false }))} label={`Run ${title}`} />
        <button
          type="button"
          onClick={() => ed.setOpen(path, !open)}
          aria-expanded={open}
          className={cx("flex min-w-0 flex-1 items-center gap-2 py-1.5 text-left", !enabled && "opacity-50")}
          data-testid="scenario-step-title"
        >
          <ChevronRight size={13} className={cx("shrink-0 text-faint transition-transform", open && "rotate-90")} />
          {step.type === "request" ? (
            <span className="w-[34px] shrink-0 text-right font-mono text-[10px] font-bold" style={{ color: methodColor(entry?.method, entry?.kind) }}>
              {entry ? methodLabel(entry.method, entry.kind) : "?"}
            </span>
          ) : (
            <span className="flex w-[34px] shrink-0 justify-end text-accent">{KIND_ICON[step.type]}</span>
          )}
          <span className="min-w-0 truncate text-[12.5px] text-fg" title={title}>
            {title}
          </span>
          {step.type === "request" && entry?.trail && <span className="min-w-0 shrink-[4] truncate text-[11px] text-faint">{entry.trail}</span>}
        </button>
        {problem && enabled && (
          <span title={problem} className="flex shrink-0 items-center gap-1 text-[11px] text-warning" data-testid="scenario-step-problem">
            <TriangleAlert size={13} />
          </span>
        )}
        {outcome && <OutcomeMark outcome={outcome} />}
        <span className="flex shrink-0 items-center opacity-0 focus-within:opacity-100 group-hover:opacity-100">
          <IconButton label={ed.blocked ?? "Run to here: from the start up to this step"} disabled={!!ed.blocked || !enabled} onClick={() => ed.run({ to: path })} size={24} data-testid="scenario-run-to">
            <StepForward size={13} />
          </IconButton>
          <IconButton label={ed.blocked ?? "Run only this step, with the values of the last run"} disabled={!!ed.blocked} onClick={() => ed.run({ only: path })} size={24} data-testid="scenario-run-only">
            <Play size={12} />
          </IconButton>
        </span>
        <Menu
          align="end"
          entries={menu}
          trigger={
            <button aria-label={`Actions of ${title}`} className="flex h-6 w-6 shrink-0 items-center justify-center rounded-lg text-faint hover:bg-hover hover:text-fg">
              <MoreHorizontal size={14} />
            </button>
          }
        />
      </div>
      {open && (
        <LocalVariables.Provider value={known}>
          <div className="flex flex-col gap-3 border-t border-line/60 px-3 pb-3 pt-2.5">
            <StepBody step={step} path={path} update={update} title={title} />
            <div className="flex flex-wrap items-center gap-x-4 gap-y-1.5 border-t border-line/40 pt-2">
              <label className="flex min-w-[180px] flex-1 items-center gap-2 text-[11.5px] text-muted">
                Name
                <Input
                  aria-label={`Name of ${title}`}
                  value={step.name ?? ""}
                  placeholder={stepTitle({ ...step, name: undefined }, ed.requests, ed.scenarios)}
                  onChange={(e) => update((s) => ({ ...s, name: e.target.value || undefined }))}
                  className="h-7 text-[12px]"
                />
              </label>
              <Option
                checked={!!step.continueOnFailure}
                onChange={(v) => update((s) => ({ ...s, continueOnFailure: v || undefined }))}
                label="Go on when it fails"
                title="The next step runs even when this one fails (the scenario still fails)"
              />
            </div>
          </div>
        </LocalVariables.Provider>
      )}
    </div>
  );
}

/** How the step did in the latest run. */
function OutcomeMark({ outcome }: { outcome: StepOutcome }) {
  const failed = outcome.failed > 0;
  const skipped = outcome.last.skipped;
  return (
    <span
      className={cx("flex shrink-0 items-center gap-1 text-[11px] tabular-nums", failed ? "text-danger" : "text-success")}
      title={failed ? (outcome.last.error ?? "Failed in the last run") : skipped ? "Skipped in the last run" : "Passed in the last run"}
      data-testid="scenario-step-outcome"
    >
      {outcome.runs > 1 && `${outcome.runs}×`}
      {failed ? <CircleX size={13} aria-label="Failed" /> : <CircleCheck size={13} aria-label="Passed" className={cx(skipped && "text-faint")} />}
    </span>
  );
}

/** A list of steps inside a step. */
function Nested({ label, listPath, steps }: { label?: React.ReactNode; listPath: string; steps: Step[] }) {
  return (
    <div className="flex flex-col gap-0.5 border-l-2 border-line/80 pl-2.5">
      {label && <div className="text-[11px] font-semibold uppercase tracking-wide text-faint">{label}</div>}
      <StepList listPath={listPath} steps={steps} />
    </div>
  );
}

function StepBody({ step, path, update, title }: { step: Step; path: string; update: (fn: (s: Step) => Step) => void; title: string }) {
  const ed = useEditor();
  const tree = useScenarios((s) => s.tree);
  // Typed updates: the step keeps its kind.
  const set = (patch: Record<string, unknown>) => update((s) => ({ ...s, ...patch }) as Step);
  switch (step.type) {
    case "request":
      return <RequestBody step={step} path={path} set={set} title={title} />;
    case "if":
      return (
        <>
          <ConditionEditor condition={step.when} onChange={(when) => set({ when })} label="Rule" />
          <Nested label="Then" listPath={`${path}/steps`} steps={step.steps ?? []} />
          {(step.elseIf ?? []).map((branch, i) => (
            <div key={i} className="flex flex-col gap-1.5">
              <div className="flex items-center gap-2">
                <FieldLabel>Else if</FieldLabel>
                <button className="text-[11.5px] text-muted hover:text-danger" onClick={() => set({ elseIf: (step.elseIf ?? []).filter((_, j) => j !== i) })}>
                  Remove
                </button>
              </div>
              <ConditionEditor condition={branch.when} onChange={(when) => set({ elseIf: (step.elseIf ?? []).map((b, j) => (j === i ? { ...b, when } : b)) })} label={`Else if ${i + 1} rule`} />
              <Nested listPath={`${path}/elseIf/${i}`} steps={branch.steps ?? []} />
            </div>
          ))}
          <Nested label="Else" listPath={`${path}/else`} steps={step.else ?? []} />
          <div>
            <Button size="sm" variant="ghost" icon={<Plus size={12} />} onClick={() => set({ elseIf: [...(step.elseIf ?? []), { when: { rules: [{ value: "" }] }, steps: [] }] })}>
              Add else if
            </Button>
          </div>
        </>
      );
    case "forEach":
      return (
        <>
          <div className="grid grid-cols-[minmax(0,1fr)_140px] gap-2">
            <label className="flex min-w-0 flex-col gap-1">
              <FieldLabel>List</FieldLabel>
              <ValueInput value={step.list} onChange={(list) => set({ list })} placeholder="{{items}}, a, b, c or = JavaScript" label="List to go through" />
            </label>
            <label className="flex min-w-0 flex-col gap-1">
              <FieldLabel>Each item as</FieldLabel>
              <Input aria-label="Name of each item" value={step.as ?? ""} placeholder="item" onChange={(e) => set({ as: e.target.value || undefined })} className="h-7 font-mono text-[12px]" />
            </label>
          </div>
          <p className="text-[11px] leading-snug text-faint">
            Inside, <code className="font-mono">{`{{${step.as?.trim() || "item"}}}`}</code> is the item (<code className="font-mono">{`{{${step.as?.trim() || "item"}.id}}`}</code> one of its
            fields) and <code className="font-mono">{"{{index}}"}</code> counts from 0.
          </p>
          <Nested listPath={`${path}/steps`} steps={step.steps ?? []} />
        </>
      );
    case "repeat":
      return (
        <>
          <div className="flex flex-wrap items-end gap-3">
            <label className="flex flex-col gap-1">
              <FieldLabel>{step.until ? "At most (rounds)" : "Times"}</FieldLabel>
              <NumberInput aria-label="Rounds" value={step.times} min={1} max={10_000} onChange={(v) => set({ times: v ?? 1 })} className="h-7 w-[96px]" />
            </label>
            <label className="flex flex-col gap-1">
              <FieldLabel>Pause between rounds (ms)</FieldLabel>
              <NumberInput aria-label="Pause between rounds in milliseconds" value={step.delayMs ?? 0} min={0} max={MAX_WAIT} onChange={(v) => set({ delayMs: v || undefined })} className="h-7 w-[120px]" />
            </label>
            <Option
              checked={!!step.until}
              onChange={(v) => set({ until: v ? { rules: [{ value: "" }] } : undefined })}
              label="Until a condition holds"
              title="Stop after the round that makes it hold; the step fails when it never does"
            />
          </div>
          {step.until && <ConditionEditor condition={step.until} onChange={(until) => set({ until })} label="Until rule" />}
          <Nested listPath={`${path}/steps`} steps={step.steps ?? []} />
        </>
      );
    case "parallel":
      return (
        <>
          <p className="text-[11px] leading-snug text-faint">
            These steps start together. Each has its own copy of the values; what they save is put together when all have finished.
          </p>
          <Nested listPath={`${path}/steps`} steps={step.steps ?? []} />
        </>
      );
    case "set":
      return <ValuesEditor values={step.values ?? []} onChange={(values) => set({ values })} what="Value" />;
    case "wait":
      return (
        <label className="flex flex-col gap-1">
          <FieldLabel>Milliseconds</FieldLabel>
          <NumberInput aria-label="Milliseconds to wait" value={step.ms} min={0} max={MAX_WAIT} onChange={(v) => set({ ms: v ?? 0 })} className="h-7 w-[120px]" />
        </label>
      );
    case "stop":
      return (
        <div className="grid grid-cols-[200px_minmax(0,1fr)] gap-2">
          <label className="flex flex-col gap-1">
            <FieldLabel>What</FieldLabel>
            <select aria-label="What to stop" value={step.what ?? "scenario"} onChange={(e) => set({ what: e.target.value })} className={selectClass}>
              <option value="loop">Leave the loop it is in</option>
              <option value="scenario">End the scenario (passed)</option>
              <option value="fail">Fail the scenario</option>
            </select>
          </label>
          <label className="flex min-w-0 flex-col gap-1">
            <FieldLabel>Why (shown in the results)</FieldLabel>
            <ValueInput value={step.message ?? ""} onChange={(message) => set({ message: message || undefined })} placeholder="Optional" label="Why it stops" />
          </label>
        </div>
      );
    case "scenario":
      return (
        <>
          <label className="flex flex-col gap-1">
            <FieldLabel>Scenario</FieldLabel>
            <select aria-label="Scenario to run" value={step.scenario} onChange={(e) => set({ scenario: e.target.value })} className={selectClass}>
              <option value="">Choose…</option>
              {ed.scenarios
                .filter((s) => s.id !== ed.scenarioId)
                .map((s) => (
                  <option key={s.id} value={s.id}>
                    {[...folderNames(s.id, tree), s.name].join(" / ")}
                  </option>
                ))}
              {step.scenario && !ed.scenarios.some((s) => s.id === step.scenario) && <option value={step.scenario}>{step.scenario} (not found)</option>}
            </select>
          </label>
          <div className="flex flex-col gap-1">
            <FieldLabel hint="The values it starts with. It has its own values and does not see this scenario's.">Give it</FieldLabel>
            <ValuesEditor values={step.inputs ?? []} onChange={(inputs) => set({ inputs: inputs.length ? inputs : undefined })} what="Input" add="Add input" />
          </div>
          <div className="flex flex-col gap-1">
            <FieldLabel hint="Name here, and its value there: token = {{token}}.">Bring back</FieldLabel>
            <ValuesEditor
              values={step.outputs ?? []}
              onChange={(outputs) => set({ outputs: outputs.length ? outputs : undefined })}
              what="Output"
              add="Add output"
              valuePlaceholder="{{name}} in the other scenario"
            />
          </div>
        </>
      );
    case "script":
      return (
        <>
          <div className="h-44 overflow-hidden rounded-lg border border-line">
            <CodeEditor value={step.code ?? ""} onChange={(code) => set({ code })} language="javascript" placeholder={'pm.variables.set("total", Number(pm.variables.get("count")) + 1);'} />
          </div>
          <p className="text-[11px] leading-snug text-faint">
            <code className="font-mono">pm.variables.set</code> saves a value of the scenario; <code className="font-mono">pm.test</code>, <code className="font-mono">pm.expect</code> and{" "}
            <code className="font-mono">pm.sendRequest</code> work as in request scripts.
          </p>
        </>
      );
    case "ask":
      return (
        <>
          <div className="grid grid-cols-[180px_minmax(0,1fr)] gap-2">
            <label className="flex flex-col gap-1">
              <FieldLabel>Save the answer as</FieldLabel>
              <Input aria-label="Variable for the answer" value={step.variable} placeholder="otp" onChange={(e) => set({ variable: e.target.value })} className="h-7 font-mono text-[12px]" />
            </label>
            <label className="flex min-w-0 flex-col gap-1">
              <FieldLabel>Question</FieldLabel>
              <ValueInput value={step.message ?? ""} onChange={(message) => set({ message: message || undefined })} placeholder="The code sent to your phone" label="Question" />
            </label>
          </div>
          <div className="flex flex-wrap items-center gap-x-4 gap-y-1">
            <Option checked={!!step.secret} onChange={(v) => set({ secret: v || undefined })} label="Secret" title="Hide the answer as it is typed and in the results" />
            <span className="text-[11px] leading-snug text-faint">The run waits for you. From the command line or an AI agent, give the value when starting the run.</span>
          </div>
        </>
      );
  }
}

/** The `{{variables}}` a saved request uses (read when its step opens). */
function useRequestVariables(path: string): string[] {
  const [names, setNames] = useState<string[]>([]);
  useEffect(() => {
    let alive = true;
    setNames([]);
    if (path) {
      api
        .readRequest(path)
        .then((request) => alive && setNames(requestVariables(request)))
        .catch(() => {});
    }
    return () => {
      alive = false;
    };
  }, [path]);
  return names;
}

function RequestBody({ step, path, set, title }: { step: Extract<Step, { type: "request" }>; path: string; set: (patch: Record<string, unknown>) => void; title: string }) {
  const ed = useEditor();
  const entry = ed.requests.get(step.request);
  const uses = useRequestVariables(step.request);
  const defined = useWorkspace((s) => s.variables);
  const known = useContext(LocalVariables);
  // What the request uses that nothing gives a value yet: offered as inputs.
  const unset = useMemo(() => {
    const names = new Set(defined.map((v) => v.key));
    return uses.filter((name) => !names.has(name) && !known?.has(name.split(".")[0]));
  }, [uses, defined, known]);
  const outcome = ed.outcomes.get(path);
  const pick = ed.runId && outcome && outcome.last.seq > 0 && outcome.last.status != null ? { runId: ed.runId, seq: outcome.last.seq } : undefined;
  return (
    <>
      <div className="flex items-center gap-2 rounded-lg bg-panel-2/60 px-2.5 py-1.5">
        <div className="min-w-0 flex-1">
          <div className="truncate text-[12.5px] text-fg">{entry?.name ?? (step.request || "No request chosen")}</div>
          <div className="truncate text-[11px] text-faint">{entry ? entry.trail || "Collection" : step.request ? "Not in the collection" : "Choose the saved request this step sends"}</div>
        </div>
        {entry && (
          <button className="shrink-0 text-[12px] text-accent hover:underline" onClick={() => void openRequest(step.request)}>
            Open
          </button>
        )}
        <RequestPicker
          all={ed.all}
          accept={runnable}
          words={PICKER_WORDS}
          onAdd={(paths) => set({ request: paths[0] })}
          trigger={
            <Button size="sm" variant={step.request ? "ghost" : "primary"}>
              {step.request ? "Change" : "Choose…"}
            </Button>
          }
        />
      </div>
      <div className="flex flex-col gap-1">
        <FieldLabel hint="Values for this step only. They win over every other variable of the same name.">Inputs</FieldLabel>
        <ValuesEditor values={step.inputs ?? []} onChange={(inputs) => set({ inputs: inputs.length ? inputs : undefined })} what="Input" add="Add input" suggest={unset} />
      </div>
      <div className="flex flex-col gap-1">
        <FieldLabel hint="Values of the response, saved for the steps after this one.">Outputs</FieldLabel>
        <OutputsEditor outputs={step.outputs ?? []} onChange={(outputs) => set({ outputs: outputs.length ? outputs : undefined })} name={title} pick={pick} />
      </div>
      <div className="flex flex-col gap-1">
        <FieldLabel hint="What the response must be for the step to pass.">Checks</FieldLabel>
        <ChecksEditor checks={step.checks ?? []} onChange={(checks) => set({ checks: checks.length ? checks : undefined })} name={title} />
      </div>
      <div className="flex flex-wrap items-end gap-3">
        <label className="flex flex-col gap-1">
          <FieldLabel>Try again when it fails</FieldLabel>
          <NumberInput
            aria-label="More tries when the step fails"
            optional
            placeholder="No"
            value={step.retry?.times || undefined}
            min={0}
            max={99}
            onChange={(v) => set({ retry: v ? { ...step.retry, times: v } : undefined })}
            className="h-7 w-[96px]"
          />
        </label>
        {step.retry && (
          <label className="flex flex-col gap-1">
            <FieldLabel>Pause before each try (ms)</FieldLabel>
            <NumberInput
              aria-label="Pause before each try in milliseconds"
              value={step.retry.delayMs ?? 0}
              min={0}
              max={MAX_WAIT}
              onChange={(v) => set({ retry: { ...step.retry!, delayMs: v || undefined } })}
              className="h-7 w-[120px]"
            />
          </label>
        )}
      </div>
    </>
  );
}

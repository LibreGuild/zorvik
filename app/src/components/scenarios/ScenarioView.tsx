// A scenario tab: header (run/stop, save, status), on the left its steps, variables,
// settings and docs, on the right the results of its latest run; one pane at a time when
// narrow.
import { useCallback, useEffect, useMemo } from "react";
import { Eraser, Loader2, Play, Save, Square, Workflow } from "lucide-react";
import type { Scenario } from "../../bindings/Scenario";
import type { ScenarioValue } from "../../bindings/ScenarioValue";
import type { TreeNode } from "../../bindings/TreeNode";
import { modKey } from "../../lib/platform";
import { stopRun, useRunner } from "../../store/runner";
import { clearScenarioValues, refreshScenarioValues, runScenario, useScenarios } from "../../store/scenarios";
import { isDirty, saveTab, type ScenarioTab, updateScenarioTab } from "../../store/tabs";
import { useUi } from "../../store/ui";
import { useWorkspace } from "../../store/workspace";
import { DocsEditor } from "../DocsEditor";
import { DataFileField } from "../loadtests/DataFileField";
import { flattenRequests } from "../loadtests/model";
import { useElementWidth } from "../loadtests/parts";
import { RunnerResults } from "../runner/RunnerResults";
import { EditorSection } from "../servers/ServerView";
import { Splitter } from "../Splitter";
import { Banner, Button, cx, IconButton, NumberInput, Segmented, Switch, Tooltip } from "../ui";
import { countSteps, folderNames, followSteps, outcomes, startProblem, stepsMoved } from "./model";
import { ValuesEditor } from "./StepFields";
import { StepsEditor } from "./StepsEditor";

const NO_NODES: TreeNode[] = [];
const NO_VALUES: ScenarioValue[] = [];
const NO_OPEN: Record<string, boolean> = {};
/** Below this width the editor and the results share the space as two panes. */
const NARROW = 820;
const MAX_ITERATIONS = 100_000;

type Section = NonNullable<ScenarioTab["section"]>;

export function ScenarioView({ tab }: { tab: ScenarioTab }) {
  const scenario = tab.draft;
  const tree = useWorkspace((s) => s.info?.tree ?? NO_NODES);
  const scenarios = useScenarios((s) => s.saved);
  const folders = useScenarios((s) => folderNames(tab.scenarioId, s.tree).join(" / "));
  const running = useRunner((s) => s.active === tab.id);
  const other = useRunner((s) => (s.active && s.active !== tab.id ? (s.runs[s.active]?.name ?? "Another run") : null));
  const starting = useRunner((s) => s.starting === tab.id);
  const busy = useRunner((s) => s.starting !== null);
  const stopping = useRunner((s) => !!s.runs[tab.id]?.stopping);
  const run = useRunner((s) => s.runs[tab.id]);
  const split = useUi((s) => s.loadSplit);
  const [area, width] = useElementWidth<HTMLDivElement>();
  const requests = useMemo(() => new Map(flattenRequests(tree).map((r) => [r.path, r])), [tree]);
  const problem = useMemo(() => startProblem(scenario, requests, scenarios, tab.scenarioId), [scenario, requests, scenarios, tab.scenarioId]);
  const ranAt = run && tab.ranAt?.runId === run.runId ? tab.ranAt.paths : undefined;
  const results = useMemo(() => outcomes(run?.results ?? [], ranAt), [run?.results, ranAt]);
  const dirty = useMemo(() => isDirty(tab), [tab]);
  // Steps that are added, removed or moved change the paths of others: what is kept by
  // path (open steps, where the latest run's results belong) follows the steps.
  const change = useCallback(
    (fn: (s: Scenario) => Scenario) =>
      updateScenarioTab(tab.id, (t) => {
        const draft = fn(t.draft);
        const moved = stepsMoved(t.draft, draft);
        if (!moved) return { draft };
        const runId = useRunner.getState().runs[tab.id]?.runId;
        const paths = runId && t.ranAt?.runId === runId ? followSteps(t.ranAt.paths, moved) : Object.fromEntries(moved);
        return { draft, open: t.open && followSteps(t.open, moved), ranAt: runId ? { runId, paths } : undefined };
      }),
    [tab.id],
  );
  const narrow = width > 0 && width < NARROW;
  const pane = tab.pane ?? "settings";
  const section = tab.section ?? "steps";
  // Why no run can start now. A part can run while another step still misses something:
  // that is how a scenario is built, one step at a time.
  const blocked = running ? "This scenario is running" : other ? `“${other}” is running. One run at a time.` : busy ? "A run is starting" : null;

  useEffect(() => {
    void refreshScenarioValues(tab.scenarioId);
  }, [tab.scenarioId]);

  const setOpen = useCallback((path: string, open: boolean) => updateScenarioTab(tab.id, (t) => ({ open: { ...t.open, [path]: open } })), [tab.id]);

  const start = useCallback(
    (part: { to?: string; only?: string } = {}) => {
      if (narrow) updateScenarioTab(tab.id, () => ({ pane: "results" }));
      void runScenario(tab, part);
    },
    [tab, narrow],
  );

  return (
    <div className="flex h-full min-h-0 flex-col bg-bg" data-testid="scenario-view">
      {tab.orphaned && <Banner tone="warning">This scenario's file was deleted or could not be reloaded. Save to write it again.</Banner>}
      <div className="flex shrink-0 flex-wrap items-center gap-2 px-3 pb-2 pt-2">
        <span className="flex h-9 items-center gap-2 rounded-xl bg-panel-2 px-3 text-[12px] font-bold text-accent">
          <Workflow size={15} />
          Scenario
        </span>
        <div className="min-w-0 flex-1">
          <div className="truncate text-[14px] font-semibold text-fg" title={folders ? `${folders} / ${scenario.name}` : scenario.name}>
            {folders && (
              <span className="font-normal text-faint" data-testid="scenario-folders">
                {folders} /{" "}
              </span>
            )}
            {scenario.name}
          </div>
          <StatusLine tab={tab} problem={problem} other={other} />
        </div>
        {/* One button for Run and Stop with a tooltip that always has content, so keyboard focus
            stays on it when a run starts or ends (see LoadTestView). */}
        <Tooltip content={running ? `Stop (${modKey}+Enter)` : (blocked ?? problem ?? `Run (${modKey}+Enter)`)}>
          <span className="inline-flex">
            <Button
              variant={running ? "secondary" : "primary"}
              icon={starting || stopping ? <Loader2 size={14} className="zv-spin" /> : running ? <Square size={13} /> : <Play size={14} />}
              disabled={!running && ((busy && !starting) || !!other || !!problem)}
              aria-disabled={starting || stopping || undefined}
              onClick={running ? () => void stopRun(tab.id) : () => start()}
              className="h-9 w-[104px] rounded-xl"
              data-testid={running ? "scenario-stop" : "scenario-start"}
            >
              {running ? (stopping ? "Stopping" : "Stop") : "Run"}
            </Button>
          </span>
        </Tooltip>
        <IconButton label={`${dirty ? "Save unsaved changes" : "Save"} (${modKey}+S)`} onClick={() => void saveTab(tab.id)} size={36} className={cx("rounded-xl", dirty && "text-accent")}>
          <Save size={16} />
        </IconButton>
      </div>
      {running && (
        <div className="h-[3px] shrink-0 overflow-hidden bg-line/60" role="progressbar" aria-label="Scenario run in progress">
          <div className="zv-indeterminate h-full w-1/3 bg-accent" />
        </div>
      )}
      {narrow && (
        <div className="flex shrink-0 justify-center border-t border-line/60 py-1.5">
          <Segmented
            items={[
              { id: "settings", label: "Scenario" },
              { id: "results", label: running ? "Results ●" : "Results" },
            ]}
            value={pane}
            onChange={(p) => updateScenarioTab(tab.id, () => ({ pane: p }))}
          />
        </div>
      )}
      <div ref={area} className={cx("flex min-h-0 flex-1", !running && "border-t border-line/60")}>
        {(!narrow || pane === "settings") && (
          <div style={narrow ? undefined : { width: `${split * 100}%` }} className={cx("flex min-h-0 min-w-0 flex-col", narrow ? "flex-1" : "shrink-0")}>
            <div className="flex shrink-0 items-center gap-2 border-b border-line/60 px-4 py-1.5" data-testid="scenario-sections">
              <Segmented<Section>
                items={[
                  { id: "steps", label: `Steps${countSteps(scenario) ? ` ${countSteps(scenario)}` : ""}` },
                  { id: "variables", label: `Variables${scenario.variables?.length ? ` ${scenario.variables.length}` : ""}` },
                  { id: "settings", label: "Settings" },
                  { id: "docs", label: scenario.docs?.trim() ? "Docs ●" : "Docs" },
                ]}
                value={section}
                onChange={(s) => updateScenarioTab(tab.id, () => ({ section: s }))}
              />
            </div>
            <div className="min-h-0 flex-1 overflow-auto">
              {section === "steps" ? (
                <StepsEditor
                  scenario={scenario}
                  scenarioId={tab.scenarioId}
                  change={change}
                  scenarios={scenarios}
                  outcomes={results}
                  runId={run?.runId ?? null}
                  run={start}
                  blocked={blocked}
                  open={tab.open ?? NO_OPEN}
                  setOpen={setOpen}
                />
              ) : section === "variables" ? (
                <Variables tab={tab} change={change} running={running} />
              ) : section === "settings" ? (
                <Settings scenario={scenario} change={change} />
              ) : (
                <EditorSection title="Docs">
                  <DocsEditor
                    key={tab.scenarioId}
                    variant="section"
                    value={scenario.docs ?? ""}
                    onChange={(docs) => change((s) => ({ ...s, docs: docs || undefined }))}
                    placeholder="What this scenario checks and what it needs, in Markdown"
                    emptyText="No docs for this scenario yet."
                  />
                </EditorSection>
              )}
            </div>
          </div>
        )}
        {!narrow && (
          <Splitter
            direction="horizontal"
            onResize={(delta) => {
              const el = area.current;
              if (!el) return;
              useUi.setState((s) => ({ loadSplit: Math.min(0.7, Math.max(0.3, s.loadSplit + delta / el.clientWidth)) }));
            }}
          />
        )}
        {(!narrow || pane === "results") && (
          <div className="min-h-0 min-w-0 flex-1 overflow-auto bg-panel/40">
            <RunnerResults tabId={tab.id} scenario />
          </div>
        )}
      </div>
    </div>
  );
}

/** "Running · 4 steps so far", why it can't run, or what it will do. */
function StatusLine({ tab, problem, other }: { tab: ScenarioTab; problem: string | null; other: string | null }) {
  const run = useRunner((s) => s.runs[tab.id]);
  const running = useRunner((s) => s.active === tab.id);
  const starting = useRunner((s) => s.starting === tab.id);
  const steps = countSteps(tab.draft);
  let dot = "bg-faint";
  let text: React.ReactNode;
  if (running && run) {
    dot = "bg-accent animate-pulse";
    text = (
      <>
        <span className="font-medium text-fg">{run.stopping ? "Stopping" : "Running"}</span>
        <span className="tabular-nums">
          · {run.counts.requests} {run.counts.requests === 1 ? "step" : "steps"} so far
        </span>
      </>
    );
  } else if (starting) {
    dot = "bg-accent";
    text = <span>Starting…</span>;
  } else if (problem) {
    dot = "bg-warning";
    text = <span className="text-warning">{problem}</span>;
  } else {
    text = (
      <span className="truncate">
        {steps} {steps === 1 ? "step" : "steps"}
        {tab.draft.dataFile ? " · once per row of the data file" : tab.draft.iterations && tab.draft.iterations > 1 ? ` · ${tab.draft.iterations} times` : ""}
        {other ? <span className="text-faint"> · “{other}” is running</span> : null}
      </span>
    );
  }
  return (
    <div className="flex min-w-0 items-center gap-1.5 text-[12px] text-muted" data-testid="scenario-status">
      <span className={cx("h-2 w-2 shrink-0 rounded-full", dot)} />
      {text}
    </div>
  );
}

/** The values a run starts with, and the ones the last run left. */
function Variables({ tab, change, running }: { tab: ScenarioTab; change: (fn: (s: Scenario) => Scenario) => void; running: boolean }) {
  const values = useScenarios((s) => s.values[tab.scenarioId] ?? NO_VALUES);
  return (
    <div className="flex flex-col pb-8" data-testid="scenario-values">
      <EditorSection title="Starting values">
        <p className="text-[12px] leading-relaxed text-muted">
          A scenario has its own values: these, what its steps save and what its scripts set. They win over the environment and the workspace's variables, which a
          scenario reads but never changes. Use one as <code className="font-mono">{"{{name}}"}</code>.
        </p>
        <ValuesEditor
          values={tab.draft.variables ?? []}
          onChange={(variables) => change((s) => ({ ...s, variables: variables.length ? variables : undefined }))}
          what="Variable"
          add="Add variable"
        />
      </EditorSection>
      <EditorSection
        title="Values of the last run"
        right={
          values.length > 0 ? (
            <Button size="sm" variant="ghost" icon={<Eraser size={12} />} disabled={running} onClick={() => void clearScenarioValues(tab.scenarioId)} data-testid="scenario-clear-values">
              Clear all
            </Button>
          ) : undefined
        }
      >
        {values.length === 0 ? (
          <p className="text-[12px] leading-relaxed text-muted">
            Nothing yet. After a run, what the scenario ended with shows here. “Run only this step” starts from these values; a whole run starts from the starting
            values again.
          </p>
        ) : (
          <>
            <div className="overflow-hidden rounded-xl border border-line">
              {values.map((v) => (
                <div key={v.key} className="flex items-baseline gap-3 border-b border-line/60 px-2.5 py-1.5 last:border-b-0" data-testid="scenario-value">
                  <span className="w-[34%] shrink-0 truncate font-mono text-[12px] text-fg" title={v.key}>
                    {v.key}
                  </span>
                  <span className={cx("min-w-0 flex-1 truncate font-mono text-[12px]", v.secret ? "text-faint" : "text-accent")} title={v.secret ? "Secret" : v.value}>
                    {v.value === "" ? <span className="italic text-faint">empty</span> : v.value}
                  </span>
                  {v.kind !== "text" && <span className="shrink-0 text-[10.5px] uppercase tracking-wide text-faint">{v.kind}</span>}
                  <button
                    aria-label={`Clear ${v.key}`}
                    disabled={running}
                    onClick={() => void clearScenarioValues(tab.scenarioId, v.key)}
                    className="shrink-0 text-[11.5px] text-muted hover:text-danger disabled:opacity-40"
                  >
                    Clear
                  </button>
                </div>
              ))}
            </div>
            <p className="text-[11.5px] leading-snug text-faint">Kept in memory on this computer until you clear them, close the workspace or quit. They are never written to the workspace's files.</p>
          </>
        )}
      </EditorSection>
    </div>
  );
}

function Settings({ scenario, change }: { scenario: Scenario; change: (fn: (s: Scenario) => Scenario) => void }) {
  return (
    <div className="flex flex-col pb-8" data-testid="scenario-settings">
      <EditorSection title="Data file">
        <DataFileField
          file={scenario.dataFile}
          rows={{ why: "to run the scenario once per row", each: "The scenario runs once per row. A row's values win over the starting values of the same name." }}
          onChange={(dataFile) => change((s) => ({ ...s, dataFile }))}
        />
      </EditorSection>
      <EditorSection title="Runs">
        <label className="flex max-w-[220px] flex-col gap-1">
          <span className="text-[11.5px] font-medium text-muted">Times</span>
          <NumberInput
            aria-label="How many times the scenario runs"
            optional
            placeholder={scenario.dataFile ? "Once per row" : "Once"}
            value={scenario.iterations}
            min={1}
            max={MAX_ITERATIONS}
            onChange={(v) => change((s) => ({ ...s, iterations: v }))}
          />
          <span className="text-[11px] leading-snug text-faint">Each time starts from the starting values again.</span>
        </label>
      </EditorSection>
      <EditorSection title="When a step fails">
        <Switch checked={!!scenario.continueOnFailure} onChange={(v) => change((s) => ({ ...s, continueOnFailure: v || undefined }))} label="Go on with the next step" />
        <p className="text-[11.5px] leading-snug text-faint">
          {scenario.continueOnFailure
            ? "Every step runs; the scenario fails when one of them did."
            : "The scenario ends at the first failed step (a step can say otherwise). The steps under “Always at the end” still run."}
        </p>
      </EditorSection>
    </div>
  );
}

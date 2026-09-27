// A load test tab: header (start/stop, save, status), settings on the left and
// results (live or the latest run) on the right; one pane at a time when narrow.
import { useCallback, useMemo } from "react";
import { Activity, Loader2, Play, Save, Square } from "lucide-react";
import type { LoadTest } from "../../bindings/LoadTest";
import type { TreeNode } from "../../bindings/TreeNode";
import { modKey } from "../../lib/platform";
import { runFor, startLoadRun, stopLoadRun, useLoadTests } from "../../store/loadtests";
import { isDirty, type LoadTestTab, saveTab, updateLoadTestDraft, updateLoadTestTab } from "../../store/tabs";
import { useUi } from "../../store/ui";
import { useWorkspace } from "../../store/workspace";
import { Splitter } from "../Splitter";
import { Banner, Button, cx, IconButton, Segmented, Tooltip } from "../ui";
import { LoadResults } from "./LoadResults";
import { LoadSettings } from "./LoadSettings";
import { flattenRequests, formatDuration, MODELS, peakTarget, sendingTargets, totalDuration } from "./model";
import { useElementWidth, useTicker } from "./parts";

const NO_NODES: TreeNode[] = [];
/** Below this width the settings and results share the space as two panes. */
const NARROW = 780;

/** Why this test can't start, if it can't (the backend checks again). */
export function startProblem(test: LoadTest, tree: TreeNode[]): string | null {
  const sending = sendingTargets(test.targets);
  if (!sending.length) return test.targets.length ? "Enable a request with a weight above 0" : "Add a request to send";
  const known = new Map(flattenRequests(tree).map((r) => [r.path, r]));
  const missing = sending.find((t) => !known.has(t.request));
  if (missing) return "A request is missing from the collection";
  if (sending.some((t) => known.get(t.request)?.error)) return "A request's file can't be read";
  if (sending.some((t) => known.get(t.request)?.kind !== "http")) return "Only HTTP requests can be load tested";
  if (totalDuration(test.stages) <= 0) return "Give a stage a duration";
  if (peakTarget(test.stages) <= 0) return "Set a stage target above 0";
  if (test.stages.some((s) => s.target > MODELS[test.model].limit)) return `Up to ${MODELS[test.model].noun(MODELS[test.model].limit)}`;
  // It would never get data, so the run would fail it.
  const sent = new Set(sending.map((t) => t.request));
  if ((test.thresholds ?? []).some((t) => t.enabled !== false && t.target && !sent.has(t.target))) return "A threshold checks a request this test doesn't send";
  return null;
}

export function LoadTestView({ tab }: { tab: LoadTestTab }) {
  const test = tab.draft;
  const running = useLoadTests((s) => !!runFor(tab.testId, s.active));
  const other = useLoadTests((s) => (s.active && !runFor(tab.testId, s.active) ? s.active.name : null));
  const starting = useLoadTests((s) => s.starting === tab.testId);
  const busy = useLoadTests((s) => s.starting !== null);
  const stopping = useLoadTests((s) => !!runFor(tab.testId, s.active)?.stopping);
  const tree = useWorkspace((s) => s.info?.tree ?? NO_NODES);
  const split = useUi((s) => s.loadSplit);
  const [area, width] = useElementWidth<HTMLDivElement>();
  const dirty = useMemo(() => isDirty(tab), [tab]);
  const problem = useMemo(() => startProblem(test, tree), [test, tree]);
  const change = useCallback((fn: (t: LoadTest) => LoadTest) => updateLoadTestDraft(tab.id, fn), [tab.id]);
  const narrow = width > 0 && width < NARROW;
  const pane = tab.pane ?? "settings";

  const start = () => {
    // Show the new run, not an earlier one picked from the history.
    updateLoadTestTab(tab.id, () => ({ runId: null, ...(narrow ? { pane: "results" as const } : {}) }));
    void startLoadRun(tab.testId, test);
  };

  return (
    <div className="flex h-full min-h-0 flex-col bg-bg" data-testid="loadtest-view">
      {tab.orphaned && <Banner tone="warning">This load test's file was deleted or could not be reloaded. Save to write it again.</Banner>}
      <div className="flex shrink-0 flex-wrap items-center gap-2 px-3 pb-2 pt-2">
        <span className="flex h-9 items-center gap-2 rounded-xl bg-panel-2 px-3 text-[12px] font-bold text-accent" title={MODELS[test.model].label}>
          <Activity size={15} />
          {MODELS[test.model].short}
        </span>
        <div className="min-w-0 flex-1">
          <div className="truncate text-[14px] font-semibold text-fg">{test.name}</div>
          <StatusLine tab={tab} problem={problem} other={other} />
        </div>
        {/* One button for Start and Stop, and a tooltip that always has content (same element tree),
            so keyboard focus stays on it when a run starts or ends. Starting and stopping are not
            `disabled` for the same reason: they ignore clicks. */}
        <Tooltip
          content={
            running ? `Stop (${modKey}+Enter)` : other ? `“${other}” is running. One load test runs at a time.` : (problem ?? `Start (${modKey}+Enter)`)
          }
        >
          {/* A wrapper: tooltips don't show on disabled buttons. */}
          <span className="inline-flex">
            <Button
              variant={running ? "secondary" : "primary"}
              icon={
                starting || stopping ? <Loader2 size={14} className="zv-spin" /> : running ? <Square size={13} /> : <Play size={14} />
              }
              disabled={!running && ((busy && !starting) || !!other || !!problem)}
              aria-disabled={starting || stopping || undefined}
              onClick={running ? () => void stopLoadRun() : start}
              className="h-9 w-[104px] rounded-xl"
              data-testid={running ? "loadtest-stop" : "loadtest-start"}
            >
              {running ? (stopping ? "Stopping" : "Stop") : "Start"}
            </Button>
          </span>
        </Tooltip>
        <IconButton label={`${dirty ? "Save unsaved changes" : "Save"} (${modKey}+S)`} onClick={() => void saveTab(tab.id)} size={36} className={cx("rounded-xl", dirty && "text-accent")}>
          <Save size={16} />
        </IconButton>
      </div>
      <Progress testId={tab.testId} />
      {narrow && (
        <div className="flex shrink-0 justify-center border-t border-line/60 py-1.5">
          <Segmented
            items={[
              { id: "settings", label: "Settings" },
              {
                id: "results",
                label: running ? (
                  <>
                    Results<span aria-hidden> ●</span>
                    <span className="sr-only"> (running)</span>
                  </>
                ) : (
                  "Results"
                ),
              },
            ]}
            value={pane}
            onChange={(p) => updateLoadTestTab(tab.id, () => ({ pane: p }))}
          />
        </div>
      )}
      <div ref={area} className={cx("flex min-h-0 flex-1", !running && "border-t border-line/60")}>
        {(!narrow || pane === "settings") && (
          <div style={narrow ? undefined : { width: `${split * 100}%` }} className={cx("min-h-0 min-w-0 overflow-auto", narrow ? "flex-1" : "shrink-0")}>
            <LoadSettings test={test} testId={tab.testId} onChange={change} running={running} />
          </div>
        )}
        {!narrow && (
          <Splitter
            direction="horizontal"
            onResize={(delta) => {
              const el = area.current;
              if (!el) return;
              useUi.setState((s) => ({ loadSplit: Math.min(0.65, Math.max(0.28, s.loadSplit + delta / el.clientWidth)) }));
            }}
          />
        )}
        {(!narrow || pane === "results") && (
          <div className="min-h-0 min-w-0 flex-1 overflow-auto bg-panel/40">
            <LoadResults tab={tab} />
          </div>
        )}
      </div>
    </div>
  );
}

/** "Running · 32 s of 60 s", or what the test will do. */
function StatusLine({ tab, problem, other }: { tab: LoadTestTab; problem: string | null; other: string | null }) {
  const run = useLoadTests((s) => runFor(tab.testId, s.active));
  const starting = useLoadTests((s) => s.starting === tab.testId);
  useTicker(!!run && !run.snapshot);
  const test = tab.draft;
  let dot = "bg-faint";
  let text: React.ReactNode;
  if (run) {
    const elapsed = run.snapshot ? run.snapshot.elapsedMs / 1000 : Math.max(0, (Date.now() - run.startedAt) / 1000);
    const planned = (run.snapshot?.plannedMs || run.plannedMs) / 1000;
    dot = "bg-accent animate-pulse";
    text = (
      <>
        <span className="font-medium text-fg">{run.stopping ? "Stopping" : run.snapshot?.phase === "starting" || !run.snapshot ? "Starting" : "Running"}</span>
        <span className="tabular-nums">
          · {formatDuration(elapsed)}
          {planned > 0 ? ` of ${formatDuration(planned)}` : ""}
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
    const n = sendingTargets(test.targets).length;
    const info = MODELS[test.model];
    text = (
      <span className="truncate">
        {n} {n === 1 ? "request" : "requests"} · {formatDuration(totalDuration(test.stages))} · up to {info.noun(peakTarget(test.stages))}
        {other ? <span className="text-faint"> · “{other}” is running</span> : null}
      </span>
    );
  }
  return (
    <div className="flex min-w-0 items-center gap-1.5 text-[12px] text-muted" data-testid="loadtest-status">
      <span className={cx("h-2 w-2 shrink-0 rounded-full", dot)} />
      {text}
    </div>
  );
}

/** A thin progress bar under the header while this test runs. */
function Progress({ testId }: { testId: string }) {
  const run = useLoadTests((s) => runFor(testId, s.active));
  useTicker(!!run && !run.snapshot);
  if (!run) return null;
  const planned = run.snapshot?.plannedMs || run.plannedMs;
  const elapsed = run.snapshot ? run.snapshot.elapsedMs : Date.now() - run.startedAt;
  const pct = planned > 0 ? Math.min(100, (elapsed * 100) / planned) : 0;
  return (
    <div
      className="h-[3px] shrink-0 bg-line/60"
      role="progressbar"
      aria-label="Load test progress"
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(pct)}
    >
      <div className="h-full bg-accent transition-[width] duration-300 ease-linear" style={{ width: `${pct}%` }} />
    </div>
  );
}

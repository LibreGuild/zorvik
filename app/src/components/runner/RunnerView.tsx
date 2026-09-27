// A runner tab: header (run/stop, status), settings on the left (requests, iterations,
// data file, options) and results on the right; one pane at a time when narrow.
import { useMemo } from "react";
import { ListChecks, Loader2, Play, Square } from "lucide-react";
import type { TreeNode } from "../../bindings/TreeNode";
import { modKey } from "../../lib/platform";
import { liveCounts, progress, runnableRequests, selectedPaths, settingsOf, startProblem, startRun, stopRun, useRunner } from "../../store/runner";
import { type RunnerTab, updateRunnerTab } from "../../store/tabs";
import { useUi } from "../../store/ui";
import { useWorkspace } from "../../store/workspace";
import { useElementWidth } from "../loadtests/parts";
import { Splitter } from "../Splitter";
import { Banner, Button, cx, Segmented, Tooltip } from "../ui";
import { RunnerResults } from "./RunnerResults";
import { RunnerSettingsPane } from "./RunnerSettings";

const NO_NODES: TreeNode[] = [];
/** Below this width the settings and results share the space as two panes. */
const NARROW = 760;

export function RunnerView({ tab }: { tab: RunnerTab }) {
  const tree = useWorkspace((s) => s.info?.tree ?? NO_NODES);
  const title = useWorkspace((s) => (tab.folder ? null : s.info?.meta.name)) ?? tab.name;
  const settings = useRunner((s) => settingsOf(tab.id, s));
  const data = useRunner((s) => s.data[tab.id]);
  const running = useRunner((s) => s.active === tab.id);
  const other = useRunner((s) => (s.active && s.active !== tab.id ? (s.runs[s.active]?.name ?? "Another run") : null));
  const starting = useRunner((s) => s.starting === tab.id);
  const busy = useRunner((s) => s.starting !== null);
  const stopping = useRunner((s) => !!s.runs[tab.id]?.stopping);
  const hasRun = useRunner((s) => !!s.runs[tab.id]);
  const split = useUi((s) => s.loadSplit);
  const [area, width] = useElementWidth<HTMLDivElement>();
  const entries = useMemo(() => runnableRequests(tree, tab.folder), [tree, tab.folder]);
  const problem = startProblem(entries, settings, data);
  const narrow = width > 0 && width < NARROW;
  const pane = tab.pane ?? "settings";

  const start = () => {
    if (!entries) return;
    if (narrow) updateRunnerTab(tab.id, () => ({ pane: "results" }));
    void startRun(tab, entries);
  };

  return (
    <div className="flex h-full min-h-0 flex-col bg-bg" data-testid="runner-view">
      {!entries && <Banner tone="warning">This folder was deleted or moved outside Zorvik.</Banner>}
      <div className="flex shrink-0 flex-wrap items-center gap-2 px-3 pb-2 pt-2">
        <span className="flex h-9 items-center gap-2 rounded-xl bg-panel-2 px-3 text-[12px] font-bold text-accent">
          <ListChecks size={15} />
          Run
        </span>
        <div className="min-w-0 flex-1">
          <div className="truncate text-[14px] font-semibold text-fg">{title}</div>
          <StatusLine tab={tab} problem={problem} other={other} selected={entries ? selectedPaths(entries, settings).length : 0} />
        </div>
        {/* One button for Run and Stop with a tooltip that always has content, so keyboard focus
            stays on it when a run starts or ends (see LoadTestView). */}
        <Tooltip content={running ? `Stop (${modKey}+Enter)` : other ? `“${other}” is running. One run at a time.` : (problem ?? `Run (${modKey}+Enter)`)}>
          <span className="inline-flex">
            <Button
              variant={running ? "secondary" : "primary"}
              icon={starting || stopping ? <Loader2 size={14} className="zv-spin" /> : running ? <Square size={13} /> : <Play size={14} />}
              disabled={!running && ((busy && !starting) || !!other || !!problem)}
              aria-disabled={starting || stopping || undefined}
              onClick={running ? () => void stopRun(tab.id) : start}
              className="h-9 w-[104px] rounded-xl"
              data-testid={running ? "runner-stop" : "runner-start"}
            >
              {running ? (stopping ? "Stopping" : "Stop") : "Run"}
            </Button>
          </span>
        </Tooltip>
      </div>
      <Progress tabId={tab.id} />
      {narrow && (
        <div className="flex shrink-0 justify-center border-t border-line/60 py-1.5">
          <Segmented
            items={[
              { id: "settings", label: "Settings" },
              { id: "results", label: running ? "Results ●" : "Results" },
            ]}
            value={pane}
            onChange={(p) => updateRunnerTab(tab.id, () => ({ pane: p }))}
          />
        </div>
      )}
      <div ref={area} className={cx("flex min-h-0 flex-1", !running && "border-t border-line/60")}>
        {(!narrow || pane === "settings") && (
          <div style={narrow ? undefined : { width: `${split * 100}%` }} className={cx("min-h-0 min-w-0 overflow-auto", narrow ? "flex-1" : "shrink-0")}>
            <RunnerSettingsPane tab={tab} entries={entries ?? []} running={running} />
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
          <div className={cx("min-h-0 min-w-0 flex-1 overflow-auto", hasRun && "bg-panel/40")}>
            <RunnerResults tabId={tab.id} />
          </div>
        )}
      </div>
    </div>
  );
}

/** "Running · 12 of 40" or what a run would do. */
function StatusLine({ tab, problem, other, selected }: { tab: RunnerTab; problem: string | null; other: string | null; selected: number }) {
  const run = useRunner((s) => (s.active === tab.id ? s.runs[tab.id] : null));
  const starting = useRunner((s) => s.starting === tab.id);
  const settings = useRunner((s) => settingsOf(tab.id, s));
  const rows = useRunner((s) => {
    const d = s.data[tab.id];
    return d?.status === "ready" ? d.preview.count : null;
  });
  const environment = useWorkspace((s) => s.info?.environments.find((e) => e.id === s.info?.activeEnvironment)?.environment.name ?? null);
  let dot = "bg-faint";
  let text: React.ReactNode;
  if (run) {
    const counts = liveCounts(run);
    dot = "bg-accent animate-pulse";
    text = (
      <>
        <span className="font-medium text-fg">{run.stopping ? "Stopping" : "Running"}</span>
        <span className="tabular-nums">
          · {run.received} of {run.total}
          {counts.failed ? <span className="text-danger"> · {counts.failed} failed</span> : null}
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
    const iterations = settings.iterations ?? rows ?? 1;
    text = (
      <span className="truncate">
        {selected} {selected === 1 ? "request" : "requests"} × {iterations} {iterations === 1 ? "iteration" : "iterations"} · {environment ? `environment ${environment}` : "no environment"}
        {other ? <span className="text-faint"> · “{other}” is running</span> : null}
      </span>
    );
  }
  return (
    <div className="flex min-w-0 items-center gap-1.5 text-[12px] text-muted" data-testid="runner-status">
      <span className={cx("h-2 w-2 shrink-0 rounded-full", dot)} />
      {text}
    </div>
  );
}

/** A thin progress bar under the header while this tab runs. */
function Progress({ tabId }: { tabId: string }) {
  const run = useRunner((s) => (s.active === tabId ? s.runs[tabId] : null));
  if (!run) return null;
  const pct = progress(run) * 100;
  return (
    <div className="h-[3px] shrink-0 bg-line/60" role="progressbar" aria-label="Run progress" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(pct)}>
      <div className="h-full bg-accent transition-[width] duration-300 ease-linear" style={{ width: `${pct}%` }} />
    </div>
  );
}

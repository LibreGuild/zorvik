// The results side of a load test tab: the live run, or the latest / an earlier
// finished run, with its history, a comparison with another run, exports and deletion.
import { memo, useCallback, useEffect, useMemo, useState } from "react";
import { DropdownMenu } from "radix-ui";
import { Activity, Check, ChevronDown, CircleAlert, CircleCheck, CircleDashed, CircleX, Cpu, Download, History, Loader2, Trash2 } from "lucide-react";
import type { LoadModel } from "../../bindings/LoadModel";
import type { LoadRunRecord } from "../../bindings/LoadRunRecord";
import type { MetricsSummary } from "../../bindings/MetricsSummary";
import type { Summary } from "../../bindings/Summary";
import type { TargetSummary } from "../../bindings/TargetSummary";
import type { ThresholdResult } from "../../bindings/ThresholdResult";
import type { TimePoint } from "../../bindings/TimePoint";
import type { TreeNode } from "../../bindings/TreeNode";
import { formatBytes, statusTone, toneText } from "../../lib/format";
import { methodColor, methodLabel } from "../../lib/http";
import { modKey, pickSavePath } from "../../lib/platform";
import { api, errorMessage } from "../../lib/rpc";
import { confirm } from "../../store/dialogs";
import { forgetResult, type LiveRun, rememberResult, runFor, setCompareRun, useLoadTests } from "../../store/loadtests";
import { type LoadTestTab, updateLoadTestTab } from "../../store/tabs";
import { toast } from "../../store/toasts";
import { useWorkspace } from "../../store/workspace";
import { Banner, cx, EmptyState, IconButton, Menu } from "../ui";
import {
  CPU_WARNING,
  cpuShare,
  flattenRequests,
  formatCount,
  formatDuration,
  formatLatency,
  formatLatencyNumber,
  formatMetricValue,
  formatPercent,
  formatRate,
  MODELS,
  nameFromPath,
} from "./model";
import { CompareMenu, ComparePanel } from "./CompareRuns";
import { Panel, Stat, useTicker, Verdict } from "./parts";
import { type ChartSeries, Legend, TimeChart } from "./TimeChart";
import { TimingPanel } from "./TimingPanel";

const NO_NODES: TreeNode[] = [];
const cores = typeof navigator !== "undefined" ? Math.max(1, navigator.hardwareConcurrency || 1) : 1;

const runDate = (ms: number) => new Date(ms).toLocaleString(undefined, { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });

/** What the dashboard shows: a live run or a finished one. */
interface ViewData {
  live: LiveRun | null;
  summary: Summary | null;
  totals: MetricsSummary | null;
  targets: TargetSummary[];
  points: TimePoint[];
  thresholds: ThresholdResult[];
  elapsedSecs: number;
  plannedSecs: number;
  /** CPU (percent of one core, summed) now or at its peak. */
  cpu: number | null;
}

function liveData(run: LiveRun): ViewData {
  const s = run.snapshot;
  const elapsed = s ? s.elapsedMs : Math.max(0, Date.now() - run.startedAt);
  return {
    live: run,
    summary: null,
    totals: s?.totals ?? null,
    targets: s?.targets ?? [],
    points: run.points,
    thresholds: s?.thresholds ?? [],
    elapsedSecs: elapsed / 1000,
    plannedSecs: (s?.plannedMs || run.plannedMs) / 1000,
    cpu: s?.cpuPercent ?? null,
  };
}

function summaryData(summary: Summary): ViewData {
  return {
    live: null,
    summary,
    totals: summary.totals,
    targets: summary.targets,
    points: summary.points,
    thresholds: summary.thresholds,
    elapsedSecs: summary.durationMs / 1000,
    plannedSecs: summary.durationMs / 1000,
    cpu: summary.peakCpuPercent,
  };
}

export function LoadResults({ tab }: { tab: LoadTestTab }) {
  const testId = tab.testId;
  const live = useLoadTests((s) => runFor(testId, s.active));
  const latest = useLoadTests((s) => s.last[testId]);
  const version = useLoadTests((s) => s.historyVersion[testId] ?? 0);
  const [runs, setRuns] = useState<LoadRunRecord[] | null>(null);
  const [cache, setCache] = useState<Record<string, Summary>>({});
  const selected = tab.runId ?? null;
  const compareId = useLoadTests((s) => s.compare[testId] ?? null);
  // Until the first snapshot, the elapsed time comes from the clock.
  const tick = useTicker(!!live && !live.snapshot);

  useEffect(() => {
    let alive = true;
    api
      .loadRuns(testId)
      .then((r) => alive && setRuns(r))
      .catch(() => alive && setRuns([]));
    return () => {
      alive = false;
    };
  }, [testId, version]);

  // The latest result comes from the history when this session has none.
  useEffect(() => {
    const newest = runs?.[0];
    if (!newest || (latest && latest.summary.startedAt >= newest.startedAt)) return;
    let alive = true;
    api
      .loadRun(testId, newest.runId)
      .then((summary) => alive && rememberResult(testId, { runId: newest.runId, summary }))
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, [runs, latest, testId]);

  const viewing = selected ? (latest?.runId === selected ? latest.summary : cache[selected]) : undefined;
  useEffect(() => {
    if (!selected || viewing) return;
    let alive = true;
    api
      .loadRun(testId, selected)
      .then((summary) => alive && setCache((c) => ({ ...c, [selected]: summary })))
      .catch((e) => {
        if (!alive) return;
        toast("error", "Could not open that run", errorMessage(e));
        updateLoadTestTab(tab.id, () => ({ runId: null }));
      });
    return () => {
      alive = false;
    };
  }, [selected, viewing, testId, tab.id]);

  const shownRunId = selected ?? (live ? null : (latest?.runId ?? null));
  const data = useMemo<ViewData | null>(() => {
    if (selected) return viewing ? summaryData(viewing) : null;
    if (live) return liveData(live);
    return latest ? summaryData(latest.summary) : null;
    // `tick` moves the clock before the first snapshot arrives.
  }, [selected, viewing, live, latest, tick]);

  const select = useCallback((runId: string | null) => updateLoadTestTab(tab.id, () => ({ runId })), [tab.id]);

  const removeRun = async (runId: string) => {
    const record = runs?.find((r) => r.runId === runId);
    const ok = await confirm({
      title: "Delete this run?",
      message: `The run${record ? ` of ${runDate(record.startedAt)}` : ""} is removed from the history. This cannot be undone.`,
      confirmLabel: "Delete run",
      danger: true,
    });
    if (!ok) return;
    try {
      await api.deleteLoadRun(testId, runId);
      if (selected === runId) select(null);
      setCache((c) => {
        const next = { ...c };
        delete next[runId];
        return next;
      });
      forgetResult(testId, runId);
    } catch (e) {
      toast("error", "Could not delete the run", errorMessage(e));
    }
  };

  const model: LoadModel = live?.model ?? tab.draft.model;

  return (
    <div className="flex min-h-full flex-col" data-testid="load-results">
      <div className="sticky top-0 z-20 flex min-h-11 flex-wrap items-center gap-2 border-b border-line/60 bg-bg/95 px-4 py-1.5 backdrop-blur">
        <ResultsTitle live={!selected ? live : null} summary={data?.summary ?? null} selected={!!selected} />
        <div className="flex-1" />
        {selected && (
          <button className="text-[12px] text-accent hover:underline" onClick={() => select(null)}>
            {live ? "Back to the live run" : "Back to the latest run"}
          </button>
        )}
        {data?.totals && <CompareMenu runs={runs} shown={shownRunId} selected={compareId} onSelect={(runId) => setCompareRun(testId, runId)} />}
        <RunsMenu runs={runs} selected={shownRunId} live={!!live} onSelect={select} />
        {shownRunId && data?.summary && (
          <>
            <Menu
              align="end"
              trigger={
                <button aria-label="Export run" className="flex h-7 items-center gap-1 rounded-lg px-2 text-[12px] text-muted hover:bg-hover hover:text-fg">
                  <Download size={13} /> Export
                </button>
              }
              entries={[
                { label: "HTML report…", onSelect: () => void exportRun(testId, shownRunId, tab.draft.name, data.summary!, "html") },
                { label: "JSON…", onSelect: () => void exportRun(testId, shownRunId, tab.draft.name, data.summary!, "json") },
              ]}
            />
            <IconButton label="Delete this run" onClick={() => void removeRun(shownRunId)}>
              <Trash2 size={13} />
            </IconButton>
          </>
        )}
      </div>

      {data ? (
        <Dashboard data={data} model={model} testId={testId} compareId={compareId && compareId !== shownRunId ? compareId : null} />
      ) : selected || (runs === null && !latest) ? (
        <div className="flex flex-1 items-center justify-center p-8">
          <Loader2 size={18} className="zv-spin text-muted" />
        </div>
      ) : (
        <div className="flex flex-1 items-center justify-center">
          <EmptyState icon={<Activity size={28} strokeWidth={1.5} />} title="No runs yet">
            Press <b className="font-medium text-fg">Start</b> ({modKey}+Enter) to run this test. Throughput, latency percentiles (p50 to p99), errors and status codes show
            here as it runs, and every run is kept in the history.
          </EmptyState>
        </div>
      )}
    </div>
  );
}

function ResultsTitle({ live, summary, selected }: { live: LiveRun | null; summary: Summary | null; selected: boolean }) {
  if (live) {
    const phase = live.stopping ? "Stopping" : !live.snapshot || live.snapshot.phase === "starting" ? "Starting" : "Live";
    return (
      <div className="flex items-center gap-2 text-[12.5px] font-semibold text-fg">
        <span className="relative flex h-2 w-2">
          <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-accent opacity-60" />
          <span className="relative inline-flex h-2 w-2 rounded-full bg-accent" />
        </span>
        {phase}
      </div>
    );
  }
  if (!summary) return <div className="text-[12.5px] font-semibold text-fg">Results</div>;
  return (
    <div className="flex min-w-0 items-center gap-2 text-[12.5px]">
      <span className="font-semibold text-fg">{selected ? "Earlier run" : "Last run"}</span>
      <span className="truncate text-muted">{runDate(summary.startedAt)}</span>
    </div>
  );
}

function RunsMenu({ runs, selected, live, onSelect }: { runs: LoadRunRecord[] | null; selected: string | null; live: boolean; onSelect: (runId: string | null) => void }) {
  const count = runs?.length ?? 0;
  return (
    <DropdownMenu.Root modal={false}>
      <DropdownMenu.Trigger asChild>
        <button
          aria-label="Run history"
          disabled={!count && !live}
          className="flex h-7 items-center gap-1.5 rounded-lg px-2 text-[12px] text-muted outline-none hover:bg-hover hover:text-fg disabled:opacity-40"
          data-testid="load-runs"
        >
          <History size={13} />
          {count} {count === 1 ? "run" : "runs"}
          <ChevronDown size={12} />
        </button>
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content align="end" sideOffset={6} collisionPadding={12} className="zv-pop z-[90] max-h-[60vh] w-[min(380px,calc(100vw-24px))] overflow-auto rounded-xl border border-line bg-elev p-1.5 shadow-pop">
          <div className="px-2 pb-1 pt-0.5 text-[11px] font-semibold uppercase tracking-wide text-faint">Run history</div>
          {live && (
            <DropdownMenu.Item onSelect={() => onSelect(null)} className="flex items-center gap-2 rounded-lg px-2 py-1.5 text-[12.5px] outline-none data-[highlighted]:bg-hover">
              <span className="flex w-4 justify-center">{selected === null && <Check size={13} />}</span>
              <span className="h-2 w-2 rounded-full bg-accent" />
              <span className="font-medium text-fg">Live run</span>
            </DropdownMenu.Item>
          )}
          {runs?.map((r) => (
            <DropdownMenu.Item
              key={r.runId}
              onSelect={() => onSelect(r === runs[0] && !live ? null : r.runId)}
              className="flex items-center gap-2 rounded-lg px-2 py-1.5 outline-none data-[highlighted]:bg-hover"
            >
              <span className="flex w-4 shrink-0 justify-center text-fg">{selected === r.runId && <Check size={13} />}</span>
              {r.error ? (
                <span className="shrink-0 rounded-md bg-danger/14 px-1.5 py-px text-[10px] font-bold tracking-wide text-danger">ERROR</span>
              ) : (
                <Verdict passed={r.passed} />
              )}
              <div className="min-w-0 flex-1">
                <div className="truncate text-[12.5px] text-fg">
                  {runDate(r.startedAt)}
                  {r.stoppedEarly && <span className="text-faint"> · stopped</span>}
                </div>
                <div className="truncate text-[11px] tabular-nums text-muted">
                  {formatDuration(r.durationMs / 1000)} · {formatRate(r.rps)} req/s · p95 {formatLatency(r.p95)} · {formatPercent(r.errorRate)} errors
                </div>
              </div>
            </DropdownMenu.Item>
          ))}
          {!count && <div className="px-2 py-3 text-center text-[12px] text-muted">No finished runs yet</div>}
          {count >= 30 && <div className="px-2 pt-1 text-[11px] text-faint">The 30 most recent runs are kept.</div>}
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  );
}

async function exportRun(testId: string, runId: string, name: string, summary: Summary, format: "json" | "html") {
  const d = new Date(summary.startedAt);
  const pad = (n: number) => String(n).padStart(2, "0");
  const stamp = `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}-${pad(d.getMinutes())}`;
  const safe = name.replace(/[\\/:*?"<>|]+/g, "-").trim() || "load test";
  const path = await pickSavePath(format === "html" ? "Save the HTML report" : "Save the results as JSON", `${safe} ${stamp}.${format}`);
  if (!path) return;
  try {
    await api.exportLoadRun(testId, runId, path, format);
    toast("success", format === "html" ? "Report saved" : "Results saved", path);
  } catch (e) {
    toast("error", "Could not export", errorMessage(e));
  }
}

// ---- dashboard ----------------------------------------------------------------------------

const Dashboard = memo(function Dashboard({ data, model, testId, compareId }: { data: ViewData; model: LoadModel; testId: string; compareId: string | null }) {
  const tree = useWorkspace((s) => s.info?.tree ?? NO_NODES);
  const share = cpuShare(data.cpu, cores);
  const hot = share != null && share > CPU_WARNING;
  const current = useMemo(() => (data.totals ? { totals: data.totals, targets: data.targets } : null), [data.totals, data.targets]);
  const timing = data.totals?.timing;
  return (
    <div className="flex flex-col gap-3 p-4">
      {data.summary && <VerdictCard summary={data.summary} />}
      {data.live?.stopping && <Banner tone="info">Stopping: no new requests start; waiting briefly for the ones in flight.</Banner>}
      <Stats data={data} model={model} share={share} />
      {hot && (
        <div className="flex items-start gap-2 rounded-xl border border-warning/40 bg-warning/10 px-3 py-2 text-[12.5px] text-warning" role="status">
          <Cpu size={14} className="mt-0.5 shrink-0" />
          <div>
            Zorvik {data.live ? "uses" : "used"} {Math.round(share)}% of this computer's CPU: the laptop may be the bottleneck, not the server. Numbers above this
            level can understate what the server handles; lower the load or use fewer users.
          </div>
        </div>
      )}
      {compareId && current && <ComparePanel testId={testId} runId={compareId} current={current} onClose={() => setCompareRun(testId, null)} />}
      {data.thresholds.length > 0 && <ThresholdsPanel thresholds={data.thresholds} live={!!data.live} />}
      <Charts points={data.points} plannedSecs={data.plannedSecs} model={model} />
      {data.totals && timing && (timing.ttfb.count > 0 || timing.connect.count > 0) && <TimingPanel totals={data.totals} />}
      {data.targets.length > 0 && <TargetsTable targets={data.targets} tree={tree} />}
      {data.totals && (
        <div className="grid grid-cols-[repeat(auto-fit,minmax(240px,1fr))] gap-3">
          <StatusCodes totals={data.totals} />
          <Errors totals={data.totals} model={model} />
        </div>
      )}
    </div>
  );
});

function VerdictCard({ summary }: { summary: Summary }) {
  const failed = summary.thresholds.filter((t) => !t.passed).length;
  const ok = summary.passed && !summary.error;
  return (
    <div
      className={cx("flex flex-wrap items-center gap-x-4 gap-y-2 rounded-xl border px-4 py-3", ok ? "border-success/35 bg-success/8" : "border-danger/35 bg-danger/7")}
      data-testid="load-verdict"
    >
      <div className={cx("flex items-center gap-2 text-[15px] font-bold tracking-wide", ok ? "text-success" : "text-danger")}>
        {ok ? <CircleCheck size={18} /> : <CircleX size={18} />}
        {summary.error ? "ERROR" : ok ? "PASSED" : "FAILED"}
      </div>
      <div className="min-w-0 flex-1 text-[12.5px] text-muted">
        {summary.error ? (
          <span className="selectable text-danger">{summary.error}</span>
        ) : summary.thresholds.length === 0 ? (
          "No thresholds set: the run passes when it completes."
        ) : failed ? (
          `${failed} of ${summary.thresholds.length} thresholds failed.`
        ) : summary.thresholds.length === 1 ? (
          "The threshold passed."
        ) : (
          `All ${summary.thresholds.length} thresholds passed.`
        )}
      </div>
      <div className="flex flex-wrap items-center gap-2 text-[11.5px] text-muted">
        <span>{runDate(summary.startedAt)}</span>
        <span>·</span>
        <span>{formatDuration(summary.durationMs / 1000)}</span>
        {summary.stoppedEarly && <span className="rounded-md bg-hover px-1.5 py-px font-medium text-muted">Stopped early</span>}
      </div>
    </div>
  );
}

function Stats({ data, model, share }: { data: ViewData; model: LoadModel; share: number | null }) {
  const t = data.totals;
  const lat = t?.latency;
  const last = data.points.length ? data.points[data.points.length - 1] : null;
  const peakActive = useMemo(() => data.points.reduce((m, p) => Math.max(m, p.active), 0), [data.points]);
  const peakRps = useMemo(() => data.points.reduce((m, p) => Math.max(m, p.rps), 0), [data.points]);
  const info = MODELS[model];
  const live = data.live;
  const snap = live?.snapshot;
  const progress = live && data.plannedSecs > 0 ? `${formatDuration(data.elapsedSecs)} of ${formatDuration(data.plannedSecs)}` : null;
  return (
    <div className="grid grid-cols-[repeat(auto-fill,minmax(132px,1fr))] gap-2" data-testid="load-stats">
      <Stat
        label="Requests / s"
        value={t ? formatRate(live ? (last?.rps ?? t.rps) : t.rps) : "–"}
        sub={t ? (live ? `avg ${formatRate(t.rps)}` : `peak ${formatRate(peakRps)}`) : progress}
        tone="accent"
        testId="stat-rps"
        raw={t?.rps}
      />
      <Stat label="p50 latency" value={formatLatency(lat?.p50)} sub={lat ? `avg ${formatLatency(lat.avg)}` : undefined} testId="stat-p50" raw={lat?.p50} />
      <Stat label="p95 latency" value={formatLatency(lat?.p95)} sub={lat ? `p90 ${formatLatency(lat.p90)}` : undefined} testId="stat-p95" raw={lat?.p95} />
      <Stat label="p99 latency" value={formatLatency(lat?.p99)} sub={lat ? `max ${formatLatency(lat.max)}` : undefined} testId="stat-p99" raw={lat?.p99} />
      <Stat
        label="Error rate"
        value={t ? formatPercent(t.errorRate) : "–"}
        sub={
          t
            ? t.dropped > 0
              ? `${formatCount(t.errors)} failed + ${formatCount(t.dropped)} dropped of ${formatCount(t.requests + t.dropped)}`
              : `${formatCount(t.errors)} of ${formatCount(t.requests)}`
            : undefined
        }
        tone={t && t.errors + t.dropped > 0 ? "danger" : undefined}
        testId="stat-errors"
        raw={t?.errorRate}
      />
      <Stat
        label={info.activeLabel}
        value={snap ? formatCount(snap.active) : data.points.length ? formatCount(live ? (last?.active ?? 0) : peakActive) : "–"}
        sub={snap ? `target ${formatCount(snap.target)}${model === "arrivalRate" ? " req/s" : ""}` : live ? progress : data.points.length ? "peak" : undefined}
        testId="stat-active"
      />
      <Stat
        label="Requests"
        value={t ? formatCount(t.requests) : "–"}
        sub={t ? (t.dropped > 0 ? `${formatCount(t.dropped)} dropped` : `${formatCount(t.connections)} connections`) : undefined}
        tone={t && t.dropped > 0 ? "warning" : undefined}
        testId="stat-requests"
        raw={t?.requests ?? 0}
      />
      <Stat label="Data in / out" value={t ? formatBytes(t.bytesIn) : "–"} sub={t ? `sent ${formatBytes(t.bytesOut)}` : undefined} testId="stat-data" />
      <Stat
        label={live ? "Generator CPU" : "Peak generator CPU"}
        value={share != null ? `${Math.round(share)}%` : "–"}
        sub={share != null ? `of ${cores} ${cores === 1 ? "core" : "cores"}` : "not measured"}
        tone={share != null && share > CPU_WARNING ? "warning" : undefined}
        testId="stat-cpu"
        raw={share ?? undefined}
      />
    </div>
  );
}

function ThresholdsPanel({ thresholds, live }: { thresholds: ThresholdResult[]; live: boolean }) {
  // Without data a threshold is pending while live, and failed once the run is over.
  const failed = thresholds.filter((t) => (t.actual != null || !live) && !t.passed).length;
  return (
    <Panel
      title="Thresholds"
      testId="load-threshold-results"
      right={
        <span className={cx("text-[11.5px]", failed ? "text-danger" : "text-muted")}>
          {failed ? `${failed} failing` : live ? "Checked live, final at the end" : "All passed"}
        </span>
      }
    >
      <div className="flex flex-col">
        {thresholds.map((t, i) => {
          const pending = live && t.actual == null;
          return (
            <div key={i} className="flex min-h-8 items-center gap-2.5 border-b border-line/50 py-1 text-[12.5px] last:border-b-0">
              {pending ? (
                <CircleDashed size={15} className="shrink-0 text-faint" />
              ) : t.passed ? (
                <CircleCheck size={15} className="shrink-0 text-success" />
              ) : (
                <CircleX size={15} className="shrink-0 text-danger" />
              )}
              <span className="min-w-0 flex-1 truncate font-mono text-[12px] text-fg" title={t.label}>
                {t.label}
              </span>
              <span className="shrink-0 font-mono text-[12px] tabular-nums text-muted">
                {t.actual == null ? (live ? "no data yet" : "no data") : formatMetricValue(t.metric, t.actual)}
              </span>
              {pending ? <span className="w-[38px]" /> : <Verdict passed={t.passed} />}
            </div>
          );
        })}
      </div>
    </Panel>
  );
}

// ---- charts -----------------------------------------------------------------------------

const C = {
  rps: "var(--accent)",
  errors: "var(--danger)",
  target: "var(--muted)",
  p50: "var(--info)",
  p95: "var(--warning)",
  p99: "var(--danger)",
  active: "var(--success)",
};

const fmtRate = (v: number) => `${formatRate(v)}/s`;
const fmtCount = (v: number) => formatCount(v);

const Charts = memo(function Charts({ points, plannedSecs, model }: { points: TimePoint[]; plannedSecs: number; model: LoadModel }) {
  const [hover, setHover] = useState<number | null>(null);
  const cols = useMemo(() => {
    const n = points.length;
    const col = (f: (p: TimePoint) => number) => {
      const out = new Array<number>(n);
      for (let i = 0; i < n; i++) out[i] = f(points[i]);
      return out;
    };
    return {
      xs: col((p) => p.second),
      rps: col((p) => p.rps),
      errors: col((p) => p.errors),
      p50: col((p) => p.p50),
      p95: col((p) => p.p95),
      p99: col((p) => p.p99),
      active: col((p) => p.active),
      target: col((p) => p.target),
    };
  }, [points]);
  const rate = model === "arrivalRate";
  const info = MODELS[model];
  const throughput = useMemo<ChartSeries[]>(
    () => [
      { key: "rps", label: "Requests", color: C.rps, values: cols.rps, area: true },
      ...(rate ? [{ key: "target", label: "Target rate", color: C.target, values: cols.target, dashed: true }] : []),
      { key: "errors", label: "Errors", color: C.errors, values: cols.errors },
    ],
    [cols, rate],
  );
  const latency = useMemo<ChartSeries[]>(
    () => [
      { key: "p50", label: "p50", color: C.p50, values: cols.p50 },
      { key: "p95", label: "p95", color: C.p95, values: cols.p95 },
      { key: "p99", label: "p99", color: C.p99, values: cols.p99 },
    ],
    [cols],
  );
  const load = useMemo<ChartSeries[]>(
    () => [
      { key: "active", label: info.activeLabel, color: C.active, values: cols.active, area: true },
      ...(!rate ? [{ key: "target", label: "Target", color: C.target, values: cols.target, dashed: true }] : []),
    ],
    [cols, rate, info.activeLabel],
  );
  const last = points.length ? points[points.length - 1] : null;
  const xMax = Math.max(plannedSecs, last?.second ?? 0);
  const empty = points.length === 0;
  return (
    <div className="grid grid-cols-[repeat(auto-fit,minmax(min(100%,340px),1fr))] gap-3">
      <Panel
        title="Throughput"
        className="col-span-full"
        right={
          <Legend
            items={[
              { label: "Requests/s", color: C.rps, value: last ? formatRate(last.rps) : undefined },
              ...(rate ? [{ label: "Target", color: C.target, dashed: true }] : []),
              { label: "Errors/s", color: C.errors, value: last ? formatCount(last.errors) : undefined },
            ]}
          />
        }
      >
        {empty ? <NoPoints /> : <TimeChart label="Requests per second over time" xs={cols.xs} series={throughput} xMax={xMax} format={fmtRate} hover={hover} onHover={setHover} />}
      </Panel>
      <Panel
        title="Latency (ms)"
        right={
          <Legend
            items={[
              { label: "p50", color: C.p50, value: last ? formatLatencyNumber(last.p50) : undefined },
              { label: "p95", color: C.p95, value: last ? formatLatencyNumber(last.p95) : undefined },
              { label: "p99", color: C.p99, value: last ? formatLatencyNumber(last.p99) : undefined },
            ]}
          />
        }
      >
        {empty ? <NoPoints /> : <TimeChart label="Latency percentiles over time" xs={cols.xs} series={latency} xMax={xMax} format={formatLatency} hover={hover} onHover={setHover} />}
      </Panel>
      <Panel
        title={rate ? "In flight" : "Users"}
        right={
          <Legend
            items={[
              { label: rate ? "In flight" : "Active", color: C.active, value: last ? formatCount(last.active) : undefined },
              ...(!rate ? [{ label: "Target", color: C.target, dashed: true, value: last ? formatCount(last.target) : undefined }] : []),
            ]}
          />
        }
      >
        {empty ? <NoPoints /> : <TimeChart label={rate ? "Requests in flight over time" : "Active and target users over time"} xs={cols.xs} series={load} xMax={xMax} format={fmtCount} hover={hover} onHover={setHover} />}
      </Panel>
    </div>
  );
});

function NoPoints() {
  return <div className="flex h-[150px] items-center justify-center rounded-lg bg-panel-2/50 text-[12px] text-faint">The chart starts after the first second</div>;
}

// ---- tables -----------------------------------------------------------------------------

function TargetsTable({ targets, tree }: { targets: TargetSummary[]; tree: TreeNode[] }) {
  const entries = useMemo(() => new Map(flattenRequests(tree).map((r) => [r.path, r])), [tree]);
  const th = "whitespace-nowrap px-1.5 py-1.5 text-right font-medium";
  const td = "whitespace-nowrap px-1.5 py-1.5 text-right font-mono tabular-nums";
  // Narrow panes scroll the numbers sideways under the request names.
  const first = "sticky left-0 z-[1] bg-bg px-2 py-1.5";
  const ttfb = targets.some((t) => (t.metrics.timing?.ttfb.count ?? 0) > 0);
  const misses = targets.some((t) => (t.metrics.captureMisses ?? 0) > 0);
  return (
    <Panel title="Per request" testId="load-targets-table">
      <div className="-mx-1 overflow-x-auto">
        <table className="w-full border-collapse text-[12px]">
          <thead>
            <tr className="border-b border-line text-[11px] text-faint">
              <th className={cx(first, "text-left font-medium")}>Request</th>
              <th className={th}>Requests</th>
              <th className={th}>Errors</th>
              <th className={th}>req/s</th>
              <th className={th}>p50 ms</th>
              <th className={th}>p95 ms</th>
              <th className={th}>p99 ms</th>
              <th className={th}>max ms</th>
              {ttfb && (
                <th className={th} title="Time to first byte: the server's time plus one network round trip">
                  1st byte p95
                </th>
              )}
              {misses && (
                <th className={th} title="Captures that found nothing in a response (the variable kept its value)">
                  Missed
                </th>
              )}
            </tr>
          </thead>
          <tbody>
            {targets.map((t) => {
              const m = t.metrics;
              const entry = entries.get(t.request);
              return (
                <tr key={t.request} className="border-b border-line/50 last:border-b-0">
                  <td className={first}>
                    <div className="flex min-w-[120px] max-w-[240px] items-center gap-2" title={t.request}>
                      <span className="w-[30px] shrink-0 text-right font-mono text-[10px] font-bold" style={{ color: methodColor(entry?.method, entry?.kind) }}>
                        {entry ? methodLabel(entry.method, entry.kind) : ""}
                      </span>
                      <span className="truncate text-fg">{t.name || nameFromPath(t.request)}</span>
                    </div>
                  </td>
                  <td className={cx(td, "text-fg")}>{formatCount(m.requests)}</td>
                  <td className={cx(td, m.errors ? "text-danger" : "text-muted")}>{formatPercent(m.errorRate)}</td>
                  <td className={cx(td, "text-fg")}>{formatRate(m.rps)}</td>
                  <td className={cx(td, "text-fg")}>{formatLatencyNumber(m.latency.p50)}</td>
                  <td className={cx(td, "text-fg")}>{formatLatencyNumber(m.latency.p95)}</td>
                  <td className={cx(td, "text-fg")}>{formatLatencyNumber(m.latency.p99)}</td>
                  <td className={cx(td, "text-muted")}>{formatLatencyNumber(m.latency.max)}</td>
                  {ttfb && <td className={cx(td, "text-fg")}>{m.timing?.ttfb.count ? formatLatencyNumber(m.timing.ttfb.p95) : "–"}</td>}
                  {misses && <td className={cx(td, m.captureMisses ? "text-warning" : "text-muted")}>{formatCount(m.captureMisses ?? 0)}</td>}
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </Panel>
  );
}

function StatusCodes({ totals }: { totals: MetricsSummary }) {
  const total = totals.statusCodes.reduce((s, [, n]) => s + n, 0);
  const shown = totals.statusCodes.slice(0, 8);
  const rest = totals.statusCodes.slice(8).reduce((s, [, n]) => s + n, 0);
  return (
    <Panel title="Status codes" testId="load-status-codes">
      {total === 0 ? (
        <div className="py-3 text-[12px] text-faint">No responses yet</div>
      ) : (
        <div className="flex flex-col gap-1.5">
          {shown.map(([code, n]) => (
            <Bar key={code} label={String(code)} labelClass={cx("font-mono font-semibold", toneText[statusTone(code)])} count={n} total={total} color={`var(--${toneVar(code)})`} />
          ))}
          {rest > 0 && <Bar label="Other" labelClass="text-muted" count={rest} total={total} color="var(--faint)" />}
        </div>
      )}
    </Panel>
  );
}

const toneVar = (code: number) => {
  const t = statusTone(code);
  return t === "muted" ? "faint" : t;
};

const ERROR_KINDS: Record<string, string> = {
  timeout: "Timed out",
  connect: "Could not connect",
  refused: "Connection refused",
  reset: "Connection reset",
  dns: "DNS lookup failed",
  tls: "TLS handshake failed",
  proxy: "Proxy error",
  protocol: "Protocol error",
  body: "Response body error",
  closed: "Connection closed",
};

function Errors({ totals, model }: { totals: MetricsSummary; model: LoadModel }) {
  const network = totals.errorKinds.reduce((s, [, n]) => s + n, 0);
  const http = Math.max(0, totals.errors - network);
  const denominator = Math.max(1, totals.requests);
  return (
    <Panel title="Errors" testId="load-errors">
      {totals.errors === 0 && totals.dropped === 0 ? (
        <div className="flex items-center gap-2 py-2 text-[12px] text-muted">
          <CircleCheck size={14} className="text-success" /> No errors{totals.requests ? "" : " yet"}
        </div>
      ) : (
        <div className="flex flex-col gap-1.5">
          {http > 0 && <Bar label="HTTP status ≥ 400" labelClass="text-fg" count={http} total={denominator} color="var(--warning)" />}
          {totals.errorKinds.map(([kind, n]) => (
            <Bar key={kind} label={ERROR_KINDS[kind] ?? kind.charAt(0).toUpperCase() + kind.slice(1)} labelClass="text-fg" count={n} total={denominator} color="var(--danger)" />
          ))}
          {totals.dropped > 0 && (
            <div className="mt-1 flex items-start gap-1.5 text-[11.5px] text-warning">
              <CircleAlert size={13} className="mt-px shrink-0" />
              <span>
                {formatCount(totals.dropped)} {model === "arrivalRate" ? "requests were not started: the in-flight limit was reached (the server could not keep up)." : "iterations were dropped."}
              </span>
            </div>
          )}
        </div>
      )}
      {(totals.captureMisses ?? 0) > 0 && (
        <div className="mt-2 flex items-start gap-1.5 text-[11.5px] text-warning" data-testid="load-capture-misses">
          <CircleAlert size={13} className="mt-px shrink-0" />
          <span>
            {formatCount(totals.captureMisses)} capture {totals.captureMisses === 1 ? "miss" : "misses"}: a capture found nothing in a response (or a value over 64 KB), so the user's next
            requests used the value it had before. These are not counted as errors.
          </span>
        </div>
      )}
    </Panel>
  );
}

function Bar({ label, labelClass, count, total, color }: { label: string; labelClass: string; count: number; total: number; color: string }) {
  const pct = total > 0 ? (count * 100) / total : 0;
  return (
    <div className="grid grid-cols-[minmax(0,9rem)_minmax(40px,1fr)_auto] items-center gap-2 text-[12px]">
      <span className={cx("truncate", labelClass)} title={label}>
        {label}
      </span>
      <div className="h-1.5 overflow-hidden rounded-full bg-panel-2">
        <div className="h-full rounded-full" style={{ width: `${Math.max(pct, 0.5)}%`, background: color }} />
      </div>
      <span className="whitespace-nowrap text-right font-mono text-[11.5px] tabular-nums text-muted">
        {formatCount(count)} <span className="text-faint">· {formatPercent(pct)}</span>
      </span>
    </div>
  );
}

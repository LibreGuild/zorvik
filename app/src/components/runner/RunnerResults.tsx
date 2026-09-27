// The results side of a runner tab: a summary header, then every request of every
// iteration (status, time, tests), expandable to its tests, errors and console.
import { memo, useMemo, useState } from "react";
import { ChevronRight, CircleCheck, CircleMinus, CircleX, Download, ListChecks } from "lucide-react";
import type { ConsoleLevel } from "../../bindings/ConsoleLevel";
import type { RunResult } from "../../bindings/RunResult";
import { formatBytes, formatMs, statusTone, toneText } from "../../lib/format";
import { methodColor, methodLabel } from "../../lib/http";
import { modKey } from "../../lib/platform";
import { exportRun, groupResults, type IterationGroup, liveCounts, MAX_RESULTS, type RunView, setFilter, testCounts, useRunner } from "../../store/runner";
import { cx, EmptyState, Menu, Segmented } from "../ui";
import { useTicker, Verdict } from "../loadtests/parts";

/** Rows shown per iteration before "Show all" (a setNextRequest loop can make thousands). */
const ROW_LIMIT = 300;
/** Iterations listed before "Show more" (a run can have 100,000). */
const GROUP_LIMIT = 200;
/** Iterations with failures that open by themselves (each can list ROW_LIMIT rows). */
const OPEN_FAILED = 10;

export function RunnerResults({ tabId }: { tabId: string }) {
  const run = useRunner((s) => s.runs[tabId]);
  const filter = useRunner((s) => s.filter[tabId] ?? "all");
  const groups = useMemo(() => (run ? groupResults(run.results, filter) : []), [run, filter]);
  const [limit, setLimit] = useState(GROUP_LIMIT);
  if (!run) {
    return (
      <div className="flex h-full items-center justify-center">
        <EmptyState icon={<ListChecks size={28} strokeWidth={1.5} />} title="No runs yet">
          Press <b className="font-medium text-fg">Run</b> ({modKey}+Enter) to send the selected requests in order with their scripts and tests. Results show here
          as they come.
        </EmptyState>
      </div>
    );
  }
  const live = !run.summary;
  const hidden = groups.length - Math.min(limit, groups.length);
  const omitted = run.received - run.results.length;
  let failedOpen = 0;
  return (
    <div className="flex min-h-full flex-col" data-testid="runner-results">
      <div className="sticky top-0 z-20 flex min-h-11 flex-wrap items-center gap-x-3 gap-y-1.5 border-b border-line/60 bg-bg/95 px-4 py-1.5 backdrop-blur">
        <SummaryLine run={run} />
        <div className="flex-1" />
        <Segmented
          items={[
            { id: "all", label: "All" },
            { id: "failed", label: "Failed" },
          ]}
          value={filter}
          onChange={(f) => setFilter(tabId, f)}
        />
        {!live && (
          <Menu
            align="end"
            trigger={
              <button aria-label="Export run" className="flex h-7 items-center gap-1 rounded-lg px-2 text-[12px] text-muted hover:bg-hover hover:text-fg">
                <Download size={13} /> Export
              </button>
            }
            entries={[
              { label: "JSON report…", onSelect: () => void exportRun(tabId, "json") },
              { label: "JUnit XML…", onSelect: () => void exportRun(tabId, "junit") },
            ]}
          />
        )}
      </div>
      {run.summary?.error && <div className="border-b border-line/60 bg-danger/6 px-4 py-2 text-[12px] text-danger">{run.summary.error}</div>}
      <div className="flex flex-col gap-2 p-3">
        {groups.slice(0, limit).map((g, i) => {
          const open = groups.length <= 3 || (live && i === groups.length - 1) || (g.failed > 0 && failedOpen++ < OPEN_FAILED);
          // Keyed by the filter too: rows are listed by position, their open state must not move to another result.
          return <Iteration key={`${filter}:${g.iteration}`} group={g} count={run.iterations} open={open} />;
        })}
        {hidden > 0 && (
          <button className="rounded-xl border border-dashed border-line-strong py-1.5 text-[12px] text-accent hover:underline" onClick={() => setLimit((l) => l + GROUP_LIMIT)}>
            {`Show ${Math.min(GROUP_LIMIT, hidden)} more iterations (${hidden} not shown)`}
          </button>
        )}
        {omitted > 0 && (
          <p className="px-1 text-[12px] text-faint" data-testid="runner-omitted">
            {`${omitted.toLocaleString()} more results aren't listed: a run keeps the first ${MAX_RESULTS.toLocaleString()}, then failed ones. The counts above include them.`}
          </p>
        )}
        {!groups.length && (
          <div className="py-8 text-center text-[12.5px] text-muted">{filter === "failed" ? "Nothing failed." : live ? "Waiting for the first result…" : "No requests ran."}</div>
        )}
      </div>
    </div>
  );
}

/** Verdict, counts and time of the run. */
function SummaryLine({ run }: { run: RunView }) {
  useTicker(!run.summary);
  const c = liveCounts(run);
  const s = run.summary;
  const tests = c.testsPassed + c.testsFailed;
  const duration = s ? s.durationMs : Date.now() - run.startedAt;
  return (
    <div className="flex min-w-0 flex-wrap items-center gap-x-2.5 gap-y-1 text-[12.5px]" data-testid="runner-summary">
      {s ? (
        s.stopped || s.error ? (
          <span className="rounded-md bg-warning/14 px-1.5 py-px text-[10px] font-bold tracking-wide text-warning">STOPPED</span>
        ) : (
          <Verdict passed={s.passed} />
        )
      ) : (
        <span className="flex items-center gap-1.5 font-semibold text-fg">
          <span className="relative flex h-2 w-2">
            <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-accent opacity-60" />
            <span className="relative inline-flex h-2 w-2 rounded-full bg-accent" />
          </span>
          {run.stopping ? "Stopping" : "Running"}
        </span>
      )}
      <span className="tabular-nums text-muted">
        <span className="text-success">{c.requests - c.failed} passed</span>
        {" · "}
        <span className={c.failed ? "text-danger" : undefined}>{c.failed} failed</span>
        {c.skipped ? ` · ${c.skipped} skipped` : ""}
      </span>
      {tests > 0 && (
        <span className="tabular-nums text-muted">
          Tests <span className={c.testsFailed ? "text-danger" : "text-success"}>{`${c.testsPassed}/${tests}`}</span>
        </span>
      )}
      <span className="tabular-nums text-faint">{formatMs(duration)}</span>
      {s?.bailed && <span className="text-faint">stopped at the first failure</span>}
    </div>
  );
}

const Iteration = memo(function Iteration({ group, count, open: initiallyOpen }: { group: IterationGroup; count: number; open: boolean }) {
  const [open, setOpen] = useState<boolean | null>(null);
  const [all, setAll] = useState(false);
  const shown = open ?? initiallyOpen;
  const tests = group.testsPassed + group.testsFailed;
  const rows = all ? group.results : group.results.slice(0, ROW_LIMIT);
  return (
    <section className="overflow-hidden rounded-xl border border-line/80 bg-bg" data-testid="runner-iteration">
      <button className="flex h-9 w-full items-center gap-2 px-3 text-left text-[12.5px] hover:bg-hover/60" onClick={() => setOpen(!shown)} aria-expanded={shown}>
        <ChevronRight size={14} className={cx("shrink-0 text-faint transition-transform", shown && "rotate-90")} />
        <span className="font-semibold text-fg">{count > 1 ? `Iteration ${group.iteration + 1}` : "Requests"}</span>
        <span className="tabular-nums text-faint">
          {group.results.length} {group.results.length === 1 ? "request" : "requests"}
          {group.failed ? <span className="text-danger"> · {group.failed} failed</span> : null}
          {tests ? ` · tests ${group.testsPassed}/${tests}` : ""}
        </span>
      </button>
      {shown && (
        <div className="border-t border-line/60">
          {rows.map((r, i) => (
            <ResultRow key={i} result={r} />
          ))}
          {rows.length < group.results.length && (
            <button className="w-full border-t border-line/60 py-1.5 text-[12px] text-accent hover:underline" onClick={() => setAll(true)}>
              Show all {group.results.length}
            </button>
          )}
        </div>
      )}
    </section>
  );
});

const ResultRow = memo(function ResultRow({ result: r }: { result: RunResult }) {
  const [open, setOpen] = useState(false);
  const t = testCounts(r);
  const problem = r.error ?? r.scriptErrors[0] ?? null;
  const expandable = !r.skipped;
  return (
    <div className="border-b border-line/50 last:border-b-0" data-testid="runner-result">
      <button
        className={cx("flex min-h-[34px] w-full items-center gap-2 px-3 py-1 text-left", expandable && "hover:bg-hover/60")}
        onClick={() => expandable && setOpen(!open)}
        aria-expanded={expandable ? open : undefined}
      >
        {r.skipped ? (
          <CircleMinus size={14} className="shrink-0 text-faint" aria-label="Skipped" />
        ) : r.passed ? (
          <CircleCheck size={14} className="shrink-0 text-success" aria-label="Passed" />
        ) : (
          <CircleX size={14} className="shrink-0 text-danger" aria-label="Failed" />
        )}
        <span className="w-[38px] shrink-0 text-right font-mono text-[10px] font-bold" style={{ color: methodColor(r.method, r.kind) }}>
          {methodLabel(r.method, r.kind)}
        </span>
        <div className="min-w-0 flex-1">
          <div className="truncate text-[12.5px] text-fg" title={r.path}>
            {r.name}
          </div>
          {r.skipped ? (
            <div className="truncate text-[11px] text-faint">Skipped: only HTTP requests run</div>
          ) : problem ? (
            <div className="truncate text-[11px] text-danger" title={problem}>
              {problem}
            </div>
          ) : null}
        </div>
        {t.total > 0 && (
          <span className={cx("shrink-0 text-[11.5px] tabular-nums", t.failed ? "text-danger" : "text-success")} title="Tests passed">
            {`${t.passed}/${t.total}`}
          </span>
        )}
        {r.status != null && <span className={cx("w-9 shrink-0 text-right font-mono text-[12px] font-semibold", toneText[statusTone(r.status)])}>{r.status}</span>}
        <span className="w-14 shrink-0 text-right text-[11.5px] tabular-nums text-faint">{r.durationMs != null ? formatMs(r.durationMs) : ""}</span>
      </button>
      {open && <ResultDetails result={r} />}
    </div>
  );
});

const LEVEL_CLASS: Record<ConsoleLevel, string> = {
  log: "text-fg",
  info: "text-fg",
  debug: "text-faint",
  warn: "text-warning",
  error: "text-danger",
};

function ResultDetails({ result: r }: { result: RunResult }) {
  return (
    <div className="selectable flex flex-col gap-2 bg-panel-2/40 px-3 pb-3 pt-1.5 pl-[70px] text-[12px]" data-testid="runner-result-details">
      <div className="break-all font-mono text-[11.5px] text-muted">
        {r.method} {r.url}
        {r.size != null && <span className="text-faint"> · {formatBytes(r.size)}</span>}
      </div>
      {r.error && <div className="whitespace-pre-wrap break-words text-danger">{r.error}</div>}
      {r.scriptErrors.map((e, i) => (
        <div key={i} className="whitespace-pre-wrap break-words text-danger">
          {e}
        </div>
      ))}
      {r.unresolved.length > 0 && <div className="text-warning">Undefined variables: {r.unresolved.join(", ")}</div>}
      {r.tests.length > 0 && (
        <div className="flex flex-col gap-1">
          {r.tests.map((t, i) => (
            <div key={i} className="flex items-start gap-2">
              {t.skipped ? (
                <CircleMinus size={13} className="mt-0.5 shrink-0 text-faint" aria-label="Skipped" />
              ) : t.passed ? (
                <CircleCheck size={13} className="mt-0.5 shrink-0 text-success" aria-label="Passed" />
              ) : (
                <CircleX size={13} className="mt-0.5 shrink-0 text-danger" aria-label="Failed" />
              )}
              <div className="min-w-0 flex-1">
                <div className={cx("break-words", t.skipped ? "text-faint" : "text-fg")}>{t.name}</div>
                {t.error && <div className="break-words font-mono text-[11.5px] text-danger">{t.error}</div>}
              </div>
            </div>
          ))}
        </div>
      )}
      {r.console.length > 0 && (
        <div className="overflow-hidden rounded-lg border border-line/60 font-mono text-[11.5px]" data-testid="runner-console">
          {r.console.map((c, i) => (
            <div key={i} className={cx("flex gap-3 border-b border-line/50 px-2 py-0.5 last:border-b-0", LEVEL_CLASS[c.level])}>
              <span className="w-10 shrink-0 text-[10px] uppercase text-faint">{c.level}</span>
              <span className="min-w-0 flex-1 whitespace-pre-wrap break-words">{c.message}</span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

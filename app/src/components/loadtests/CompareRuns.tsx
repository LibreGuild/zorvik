// Compare the run on screen with an earlier one of the same test: throughput,
// error rate, latency percentiles, p95 time to first byte and each request's p95.
import { useEffect, useMemo, useState } from "react";
import { DropdownMenu } from "radix-ui";
import { Check, ChevronDown, GitCompareArrows, Loader2, X } from "lucide-react";
import type { LoadRunRecord } from "../../bindings/LoadRunRecord";
import type { Summary } from "../../bindings/Summary";
import { api, errorMessage } from "../../lib/rpc";
import { cx, IconButton } from "../ui";
import { type CompareRow, compareRuns, formatChange, formatLatency, formatPercent, formatRate, type RunNumbers } from "./model";
import { Panel } from "./parts";

const runDate = (ms: number) => new Date(ms).toLocaleString(undefined, { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });

/** "Compare": pick an earlier run of this test (not the one on screen). */
export function CompareMenu({
  runs,
  shown,
  selected,
  onSelect,
}: {
  runs: LoadRunRecord[] | null;
  /** The run on screen (`null`: the live run). */
  shown: string | null;
  selected: string | null;
  onSelect: (runId: string | null) => void;
}) {
  const others = (runs ?? []).filter((r) => r.runId !== shown && !r.error);
  if (!others.length) return null;
  return (
    <DropdownMenu.Root modal={false}>
      <DropdownMenu.Trigger asChild>
        <button
          aria-label="Compare with an earlier run"
          className={cx(
            "flex h-7 items-center gap-1.5 rounded-lg px-2 text-[12px] outline-none hover:bg-hover hover:text-fg",
            selected ? "text-accent" : "text-muted",
          )}
          data-testid="load-compare"
        >
          <GitCompareArrows size={13} />
          Compare
          <ChevronDown size={12} />
        </button>
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content align="end" sideOffset={6} collisionPadding={12} className="zv-pop z-[90] max-h-[60vh] w-[min(360px,calc(100vw-24px))] overflow-auto rounded-xl border border-line bg-elev p-1.5 shadow-pop">
          <div className="px-2 pb-1 pt-0.5 text-[11px] font-semibold uppercase tracking-wide text-faint">Compare with</div>
          {selected && (
            <DropdownMenu.Item onSelect={() => onSelect(null)} className="flex items-center gap-2 rounded-lg px-2 py-1.5 text-[12.5px] text-muted outline-none data-[highlighted]:bg-hover">
              <span className="flex w-4 justify-center">
                <X size={13} />
              </span>
              Stop comparing
            </DropdownMenu.Item>
          )}
          {others.map((r) => (
            <DropdownMenu.Item key={r.runId} onSelect={() => onSelect(r.runId)} className="flex items-center gap-2 rounded-lg px-2 py-1.5 outline-none data-[highlighted]:bg-hover">
              <span className="flex w-4 shrink-0 justify-center text-fg">{selected === r.runId && <Check size={13} />}</span>
              <div className="min-w-0 flex-1">
                <div className="truncate text-[12.5px] text-fg">{runDate(r.startedAt)}</div>
                <div className="truncate text-[11px] tabular-nums text-muted">
                  {formatRate(r.rps)} req/s · p95 {formatLatency(r.p95)} · {formatPercent(r.errorRate)} errors
                </div>
              </div>
            </DropdownMenu.Item>
          ))}
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  );
}

const value = (r: CompareRow, v: number | null) => {
  if (v == null) return "–";
  if (r.unit === "req/s") return formatRate(v);
  if (r.unit === "%") return formatPercent(v);
  return formatLatency(v);
};

/** Side by side: the run on screen and the earlier one, with the change colored good or bad. */
export function ComparePanel({ testId, runId, current, onClose }: { testId: string; runId: string; current: RunNumbers; onClose: () => void }) {
  const [baseline, setBaseline] = useState<{ runId: string; summary: Summary } | { runId: string; error: string } | null>(null);
  useEffect(() => {
    let alive = true;
    api
      .loadRun(testId, runId)
      .then((summary) => alive && setBaseline({ runId, summary }))
      .catch((e) => alive && setBaseline({ runId, error: errorMessage(e) }));
    return () => {
      alive = false;
    };
  }, [testId, runId]);
  const loaded = baseline?.runId === runId ? baseline : null;
  const summary = loaded && "summary" in loaded ? loaded.summary : null;
  const rows = useMemo(() => (summary ? compareRuns(current, summary) : []), [current, summary]);
  const totals = rows.filter((r) => !r.target);
  const targets = rows.filter((r) => r.target);
  const th = "whitespace-nowrap px-1.5 py-1.5 text-right font-medium";
  const td = "whitespace-nowrap px-1.5 py-1.5 text-right font-mono tabular-nums";
  const line = (r: CompareRow) => (
    <tr key={r.key} className="border-b border-line/50 last:border-b-0" data-testid="compare-row">
      <td className={cx("px-2 py-1.5 text-fg", r.target && "pl-4")}>
        <div className="max-w-[240px] truncate" title={r.label}>
          {r.label}
        </div>
      </td>
      <td className={cx(td, "text-fg")}>{value(r, r.current)}</td>
      <td className={cx(td, "text-muted")}>{value(r, r.baseline)}</td>
      <td className={cx(td, "font-semibold", r.better === true ? "text-success" : r.better === false ? "text-danger" : "text-muted")} data-testid={`compare-change-${r.key}`}>
        {formatChange(r)}
      </td>
    </tr>
  );
  return (
    <Panel
      title={summary ? `Compared with the run of ${runDate(summary.startedAt)}` : "Compared with an earlier run"}
      testId="load-compare-panel"
      right={
        <IconButton label="Stop comparing" onClick={onClose} size={22}>
          <X size={13} />
        </IconButton>
      }
    >
      {!loaded ? (
        <div className="flex justify-center py-3">
          <Loader2 size={16} className="zv-spin text-muted" />
        </div>
      ) : "error" in loaded ? (
        <p className="py-2 text-[12px] text-danger">{`That run can't be opened: ${loaded.error}`}</p>
      ) : (
        <div className="-mx-1 overflow-x-auto">
          <table className="w-full border-collapse text-[12px]">
            <thead>
              <tr className="border-b border-line text-[11px] text-faint">
                <th className="px-2 py-1.5 text-left font-medium">Metric</th>
                <th className={th}>This run</th>
                <th className={th}>Earlier</th>
                <th className={th}>Change</th>
              </tr>
            </thead>
            <tbody>
              {totals.map(line)}
              {targets.length > 0 && (
                <tr className="border-b border-line/50">
                  <td colSpan={4} className="px-2 pb-1 pt-2.5 text-[11px] font-medium text-faint">
                    Per request (p95 latency)
                  </td>
                </tr>
              )}
              {targets.map(line)}
            </tbody>
          </table>
        </div>
      )}
    </Panel>
  );
}

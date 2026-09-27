// Pass/fail rules: "p95 < 500 ms", "error rate < 1 %" (for the whole test or one request).
import { memo, useMemo } from "react";
import { Plus, X } from "lucide-react";
import type { LoadTarget } from "../../bindings/LoadTarget";
import type { Threshold } from "../../bindings/Threshold";
import type { ThresholdMetric } from "../../bindings/ThresholdMetric";
import type { ThresholdOp } from "../../bindings/ThresholdOp";
import type { TreeNode } from "../../bindings/TreeNode";
import { Button, Checkbox, cx, IconButton } from "../ui";
import { changeMetric, defaultThreshold, flattenRequests, metricInfo, METRICS, nameFromPath, OP_LABELS, OPS, sendingTargets, thresholdLabel } from "./model";
import { NumberInput } from "./parts";

const selectClass =
  "h-7 min-w-0 rounded-lg border border-line bg-input px-1.5 text-[12px] text-fg outline-none hover:border-line-strong focus:border-accent focus:ring-2 focus:ring-accent-soft";

export const ThresholdsEditor = memo(function ThresholdsEditor({
  thresholds,
  targets,
  tree,
  onChange,
}: {
  thresholds: Threshold[];
  targets: LoadTarget[];
  tree: TreeNode[];
  onChange: (fn: (list: Threshold[]) => Threshold[]) => void;
}) {
  const names = useMemo(() => new Map(flattenRequests(tree).map((r) => [r.path, r.name])), [tree]);
  const sent = useMemo(() => new Set(sendingTargets(targets).map((t) => t.request)), [targets]);
  const targetName = (path: string) => names.get(path) ?? nameFromPath(path);
  const update = (i: number, fn: (t: Threshold) => Threshold) => onChange((list) => list.map((t, j) => (j === i ? fn(t) : t)));

  return (
    <div className="flex flex-col gap-2" data-testid="load-thresholds">
      {thresholds.length === 0 ? (
        <p className="rounded-xl border border-dashed border-line-strong px-3 py-3 text-[12px] text-muted">
          No thresholds: every run passes. Add one to fail a run that is too slow or has errors (the <span className="font-mono">zorvik load</span> command exits with 1 then, for CI).
        </p>
      ) : (
        <div className="overflow-hidden rounded-xl border border-line">
          {thresholds.map((t, i) => {
            const enabled = t.enabled !== false;
            const unit = metricInfo(t.metric).unit;
            const missingTarget = !!t.target && !targets.some((x) => x.request === t.target);
            // Removed, disabled or weight 0: no data, so the run would fail it.
            const unsent = !!t.target && !sent.has(t.target);
            return (
              <div
                key={i}
                className="flex flex-wrap items-center gap-1.5 border-b border-line/60 px-2.5 py-1.5 last:border-b-0"
                data-testid="load-threshold"
                title={`${thresholdLabel(t, t.target ? targetName(t.target) : null)}${unsent && enabled ? "\nThat request isn't sent: turn it on, pick another or uncheck this threshold." : ""}`}
              >
                <Checkbox checked={enabled} onChange={(v) => update(i, (x) => ({ ...x, enabled: v }))} label="Check this threshold" />
                <div className={cx("flex min-w-0 flex-1 flex-wrap items-center gap-1.5", !enabled && "opacity-50")}>
                  <select aria-label="Threshold metric" value={t.metric} onChange={(e) => update(i, (x) => changeMetric(x, e.target.value as ThresholdMetric))} className={cx(selectClass, "w-[132px]")}>
                    {METRICS.map((m) => (
                      <option key={m.id} value={m.id}>
                        {m.label}
                        {m.unit === "ms" ? " (ms)" : m.unit === "%" ? " (%)" : ""}
                      </option>
                    ))}
                  </select>
                  <select aria-label="Threshold comparison" value={t.op} onChange={(e) => update(i, (x) => ({ ...x, op: e.target.value as ThresholdOp }))} className={cx(selectClass, "w-[46px] text-center font-mono")}>
                    {OPS.map((op) => (
                      <option key={op} value={op}>
                        {OP_LABELS[op]}
                      </option>
                    ))}
                  </select>
                  <div className="flex items-center gap-1">
                    <NumberInput
                      aria-label="Threshold value"
                      value={t.value}
                      integer={false}
                      min={0}
                      onChange={(v) => update(i, (x) => ({ ...x, value: v ?? 0 }))}
                      className="h-7 w-[76px] text-right"
                    />
                    <span className="w-9 text-[11.5px] text-faint">{unit}</span>
                  </div>
                  <select
                    aria-label="Threshold applies to"
                    value={t.target ?? ""}
                    onChange={(e) => update(i, (x) => ({ ...x, target: e.target.value || undefined }))}
                    aria-invalid={(unsent && enabled) || undefined}
                    className={cx(selectClass, "w-[140px] flex-1", unsent && enabled && "border-warning")}
                  >
                    <option value="">All requests</option>
                    {targets.map((x) => (
                      <option key={x.request} value={x.request}>
                        {targetName(x.request)}
                        {sent.has(x.request) ? "" : " (not sent)"}
                      </option>
                    ))}
                    {missingTarget && <option value={t.target}>{targetName(t.target!)} (not in this test)</option>}
                  </select>
                </div>
                <IconButton label="Remove threshold" onClick={() => onChange((list) => list.filter((_, j) => j !== i))} size={24}>
                  <X size={13} />
                </IconButton>
              </div>
            );
          })}
        </div>
      )}
      <div>
        <Button size="sm" variant="ghost" icon={<Plus size={13} />} onClick={() => onChange((list) => [...list, nextThreshold(list)])}>
          Add threshold
        </Button>
      </div>
    </div>
  );
});

/** A threshold on a metric not checked yet (p95, then errors, p99, throughput…). */
function nextThreshold(list: Threshold[]): Threshold {
  const order: ThresholdMetric[] = ["p95", "errorRate", "p99", "rps", "p50", "p90", "avg", "max", "p999"];
  const metric = order.find((m) => !list.some((t) => t.metric === m && !t.target)) ?? "p95";
  return defaultThreshold(metric);
}

// The load over time: a shape (preset), peak and duration, and the stage list for
// fine control, with a preview of the shape.
import { memo, useMemo, useRef } from "react";
import { ArrowDown, ArrowUp, Plus, X } from "lucide-react";
import type { LoadModel } from "../../bindings/LoadModel";
import type { LoadStage } from "../../bindings/LoadStage";
import { useLoadTests, runFor } from "../../store/loadtests";
import { Button, cx, IconButton, Tooltip } from "../ui";
import {
  formatCount,
  formatDuration,
  matchPreset,
  MODELS,
  peakTarget,
  presetStages,
  scaleDurations,
  scaleTargets,
  STAGE_PRESETS,
  stageShape,
  type StagePreset,
  timeLabel,
  totalDuration,
} from "./model";
import { NumberInput, useElementWidth } from "./parts";

const MAX_STAGE_SECS = 86_400;

export const StagesEditor = memo(function StagesEditor({
  stages,
  model,
  testId,
  onChange,
}: {
  stages: LoadStage[];
  model: LoadModel;
  testId: string;
  onChange: (fn: (stages: LoadStage[]) => LoadStage[]) => void;
}) {
  const info = MODELS[model];
  const total = totalDuration(stages);
  const peak = peakTarget(stages);
  const preset = useMemo(() => matchPreset(stages), [stages]);
  // Peak and duration scale the stages as they were when editing began: typing "100" passes
  // through "1" and "10", which must not flatten the shape on the way.
  const base = useRef<LoadStage[] | null>(null);
  const scaling = {
    onFocus: () => (base.current = stages),
    onBlur: () => (base.current = null),
  };
  const overLimit = stages.some((s) => s.target > info.limit);
  const update = (i: number, patch: Partial<LoadStage>) => onChange((list) => list.map((s, j) => (j === i ? { ...s, ...patch } : s)));
  const rows = useRef<HTMLDivElement>(null);
  const move = (i: number, by: number) => {
    onChange((list) => {
      const next = [...list];
      const [s] = next.splice(i, 1);
      next.splice(i + by, 0, s);
      return next;
    });
    // Rows are keyed by position: keep the keyboard on the stage that moved (its button in that
    // direction, or the other one at the top or bottom).
    requestAnimationFrame(() => {
      const row = rows.current?.querySelectorAll('[data-testid="load-stage"]')[i + by];
      const button = row?.querySelector<HTMLButtonElement>(`[data-move="${by < 0 ? "up" : "down"}"]:not(:disabled)`) ?? row?.querySelector<HTMLButtonElement>("[data-move]:not(:disabled)");
      button?.focus();
    });
  };

  return (
    <div className="flex flex-col gap-3" data-testid="load-stages">
      <StagePreview stages={stages} model={model} testId={testId} />

      <div className="flex flex-wrap gap-1.5" role="group" aria-label="Load shape">
        {STAGE_PRESETS.map((p) => (
          <Tooltip key={p.id} content={p.description}>
            <button
              type="button"
              aria-pressed={preset === p.id}
              onClick={() => onChange(() => presetStages(p.id, peak || 10, total || 60))}
              className={cx(
                "flex h-8 items-center gap-2 rounded-lg border px-2.5 text-[12px] font-medium transition-colors",
                preset === p.id ? "border-accent bg-accent-soft text-fg" : "border-line text-muted hover:border-line-strong hover:text-fg",
              )}
            >
              <ShapeIcon preset={p.id} />
              {p.label}
            </button>
          </Tooltip>
        ))}
        {!preset && stages.length > 0 && <span className="flex h-8 items-center px-1 text-[11.5px] text-faint">Custom shape</span>}
      </div>

      <div className="flex flex-wrap items-end gap-3">
        <label className="flex flex-col gap-1">
          <span className="text-[11.5px] font-medium text-muted">Peak ({info.unit})</span>
          <NumberInput
            aria-label="Peak load"
            value={peak}
            min={0}
            max={info.limit}
            {...scaling}
            onChange={(v) => onChange((list) => scaleTargets(base.current ?? list, v ?? 0))}
            className="w-28"
          />
        </label>
        <label className="flex flex-col gap-1">
          <span className="text-[11.5px] font-medium text-muted">Duration (seconds)</span>
          <NumberInput
            aria-label="Total duration in seconds"
            value={total}
            min={0}
            max={MAX_STAGE_SECS}
            {...scaling}
            onChange={(v) => onChange((list) => scaleDurations(base.current ?? list, Math.min(MAX_STAGE_SECS, v ?? 0)))}
            className="w-28"
          />
        </label>
        <span className="pb-2 text-[11.5px] text-faint">{total > 0 ? formatDuration(total) : ""}</span>
      </div>
      {overLimit && <p className="text-[11.5px] text-danger">Up to {info.noun(info.limit)} per test: lower the stage targets.</p>}
      {total === 0 && <p className="text-[11.5px] text-warning">Give at least one stage a duration.</p>}

      <div ref={rows} className="overflow-hidden rounded-xl border border-line">
        <div className="grid h-7 grid-cols-[22px_minmax(0,1fr)_minmax(0,1fr)_70px] items-center gap-2 border-b border-line bg-panel-2/60 px-2.5 text-[11px] font-medium text-faint">
          <span>#</span>
          <span>Duration (s)</span>
          <span>Target ({info.unit})</span>
          <span />
        </div>
        {stages.map((s, i) => {
          const prev = i === 0 ? 0 : stages[i - 1].target;
          const what = s.durationSecs === 0 ? "jump" : s.target === prev ? "hold" : s.target > prev ? "ramp up" : "ramp down";
          return (
            <div key={i} className="grid min-h-[38px] grid-cols-[22px_minmax(0,1fr)_minmax(0,1fr)_70px] items-center gap-2 border-b border-line/60 px-2.5 py-1 last:border-b-0" data-testid="load-stage">
              <span className="text-[11.5px] tabular-nums text-faint">{i + 1}</span>
              <div className="flex min-w-0 items-center gap-1.5">
                <NumberInput aria-label={`Stage ${i + 1} duration in seconds`} value={s.durationSecs} min={0} max={MAX_STAGE_SECS} onChange={(v) => update(i, { durationSecs: Math.min(MAX_STAGE_SECS, v ?? 0) })} className="h-7 min-w-0" />
                <span className="hidden w-[52px] shrink-0 truncate text-[10.5px] text-faint @min-[400px]:inline">{what}</span>
              </div>
              <div className="flex min-w-0 items-center gap-1.5">
                <NumberInput aria-label={`Stage ${i + 1} target`} value={s.target} min={0} max={info.limit} onChange={(v) => update(i, { target: v ?? 0 })} className="h-7 min-w-0" />
              </div>
              <div className="flex items-center justify-end gap-0.5">
                <IconButton label={`Move stage ${i + 1} up`} data-move="up" size={22} disabled={i === 0} onClick={() => move(i, -1)}>
                  <ArrowUp size={12} />
                </IconButton>
                <IconButton label={`Move stage ${i + 1} down`} data-move="down" size={22} disabled={i === stages.length - 1} onClick={() => move(i, 1)}>
                  <ArrowDown size={12} />
                </IconButton>
                <IconButton label={`Remove stage ${i + 1}`} size={22} disabled={stages.length === 1} onClick={() => onChange((list) => list.filter((_, j) => j !== i))}>
                  <X size={12} />
                </IconButton>
              </div>
            </div>
          );
        })}
      </div>
      <div className="flex flex-wrap items-center gap-2">
        <Button
          size="sm"
          variant="ghost"
          icon={<Plus size={13} />}
          onClick={() => onChange((list) => [...list, { durationSecs: 10, target: list.length ? list[list.length - 1].target : 10 }])}
        >
          Add stage
        </Button>
        <span className="flex-1" />
        <span className="text-[11.5px] text-faint">
          Each stage moves linearly from the previous target to its own; 0 s jumps.
        </span>
      </div>
    </div>
  );
});

/** Tiny icon of a preset's shape. */
function ShapeIcon({ preset }: { preset: StagePreset }) {
  const d = preset === "constant" ? "M1 11V3H17" : preset === "ramp" ? "M1 11L5 3H13L17 11" : "M1 11V9H7L8 2H10L11 9H17";
  return (
    <svg width="18" height="13" viewBox="0 0 18 13" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinejoin="round" strokeLinecap="round" aria-hidden>
      <path d={d} />
    </svg>
  );
}

const H = 76;
const PAD = { left: 6, right: 6, top: 10, bottom: 16 };

/** The planned load over time; while this test runs, a marker shows where it is. */
function StagePreview({ stages, model, testId }: { stages: LoadStage[]; model: LoadModel; testId: string }) {
  const [ref, width] = useElementWidth<HTMLDivElement>();
  const elapsed = useLoadTests((s) => {
    const run = runFor(testId, s.active);
    return run?.snapshot ? Math.floor(run.snapshot.elapsedMs / 1000) : null;
  });
  const shape = useMemo(() => stageShape(stages), [stages]);
  const total = shape[shape.length - 1][0];
  const peak = Math.max(1, ...shape.map((p) => p[1]));
  const w = Math.max(10, width - PAD.left - PAD.right);
  const h = H - PAD.top - PAD.bottom;
  const x = (t: number) => PAD.left + (total > 0 ? (t / total) * w : 0);
  const y = (v: number) => PAD.top + h - (v / peak) * h;
  const line = shape.map((p, i) => `${i ? "L" : "M"}${x(p[0]).toFixed(1)} ${y(p[1]).toFixed(1)}`).join("");
  const area = `${line}L${x(total).toFixed(1)} ${PAD.top + h}L${PAD.left} ${PAD.top + h}Z`;
  const info = MODELS[model];
  return (
    <div ref={ref} className="relative rounded-xl bg-panel-2/70" style={{ height: H }} data-testid="stage-preview">
      {width > 0 && total > 0 && (
        <svg width={width} height={H} className="block" role="img" aria-label={`Load shape: up to ${info.noun(peakTarget(stages))} over ${formatDuration(total)}`}>
          <line x1={PAD.left} x2={PAD.left + w} y1={PAD.top + h} y2={PAD.top + h} stroke="var(--border-strong)" />
          <path d={area} fill="var(--accent)" opacity={0.12} />
          <path d={line} fill="none" stroke="var(--accent)" strokeWidth={1.75} strokeLinejoin="round" />
          {elapsed != null && elapsed <= total && (
            <g>
              <line x1={x(elapsed)} x2={x(elapsed)} y1={PAD.top - 4} y2={PAD.top + h} stroke="var(--text)" strokeWidth={1} strokeDasharray="2 2" />
              <circle cx={x(elapsed)} cy={PAD.top + h} r={2.5} fill="var(--text)" />
            </g>
          )}
          <text x={PAD.left + 2} y={H - 3} className="fill-[var(--faint)] text-[10px]">
            0
          </text>
          <text x={PAD.left + w - 2} y={H - 3} textAnchor="end" className="fill-[var(--faint)] text-[10px] tabular-nums">
            {timeLabel(total)}
          </text>
        </svg>
      )}
      {total > 0 ? (
        <span className="absolute left-2.5 top-1.5 rounded bg-panel-2/80 px-1 text-[10.5px] font-medium tabular-nums text-muted">
          peak {formatCount(peakTarget(stages))} {info.unit}
        </span>
      ) : (
        <span className="absolute inset-0 flex items-center justify-center text-[11.5px] text-faint">No duration yet</span>
      )}
      {elapsed != null && total > 0 && (
        <span className="absolute right-2.5 top-1.5 rounded bg-panel-2/80 px-1 text-[10.5px] tabular-nums text-muted">{timeLabel(elapsed)}</span>
      )}
    </div>
  );
}

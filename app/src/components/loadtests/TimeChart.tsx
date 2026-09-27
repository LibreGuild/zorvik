// A lightweight SVG line chart over time (seconds). Series are decimated to the
// pixel width (min/max per pixel column), so hours of 1-second points draw fast;
// paths are memoized and only the hover overlay follows the pointer.
import { memo, useMemo } from "react";
import { compactNumber, decimate, niceMax, timeLabel, timeTicks } from "./model";
import { useElementWidth } from "./parts";

export interface ChartSeries {
  key: string;
  label: string;
  color: string;
  values: number[];
  dashed?: boolean;
  /** Fill under the line. */
  area?: boolean;
}

const PAD = { left: 40, right: 10, top: 8, bottom: 20 };

interface Props {
  /** Seconds since the start, ascending (one per value). */
  xs: number[];
  series: ChartSeries[];
  /** Right end of the time axis (the planned duration while running). */
  xMax: number;
  height?: number;
  /** Tooltip value format. */
  format: (v: number) => string;
  /** Hovered second, shared by the charts of a run. */
  hover: number | null;
  onHover: (second: number | null) => void;
  label: string;
}

export const TimeChart = memo(function TimeChart({ xs, series, xMax, height = 150, format, hover, onHover, label }: Props) {
  const [ref, width] = useElementWidth<HTMLDivElement>();
  const plotW = Math.max(10, width - PAD.left - PAD.right);
  const plotH = Math.max(10, height - PAD.top - PAD.bottom);
  const domainX = Math.max(1, xMax, xs.length ? xs[xs.length - 1] : 0);

  const yMax = useMemo(() => {
    let m = 0;
    for (const s of series) for (const v of s.values) if (v > m && Number.isFinite(v)) m = v;
    return niceMax(m);
  }, [series]);

  const paths = useMemo(() => {
    if (!width || xs.length === 0) return [];
    const sx = (x: number) => PAD.left + (x / domainX) * plotW;
    const sy = (y: number) => PAD.top + plotH - (Math.max(0, y) / yMax) * plotH;
    return series.map((s) => {
      const idx = decimate(xs, s.values, Math.max(1, Math.round(plotW)));
      let d = "";
      for (let k = 0; k < idx.length; k++) {
        const i = idx[k];
        const v = s.values[i];
        if (!Number.isFinite(v)) continue;
        d += `${d ? "L" : "M"}${sx(xs[i]).toFixed(1)} ${sy(v).toFixed(1)}`;
      }
      const first = idx.length ? sx(xs[idx[0]]).toFixed(1) : "0";
      const last = idx.length ? sx(xs[idx[idx.length - 1]]).toFixed(1) : "0";
      const base = (PAD.top + plotH).toFixed(1);
      const area = s.area && d ? `${d}L${last} ${base}L${first} ${base}Z` : null;
      return { key: s.key, color: s.color, dashed: s.dashed, d, area };
    });
  }, [series, xs, width, plotW, plotH, domainX, yMax]);

  const ticksX = useMemo(() => timeTicks(domainX, Math.max(2, Math.floor(plotW / 90))), [domainX, plotW]);
  const ticksY = [0, yMax / 2, yMax];

  // Hover: the nearest second with data.
  const hoverIndex = useMemo(() => {
    if (hover == null || xs.length === 0) return -1;
    let lo = 0;
    let hi = xs.length - 1;
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      if (xs[mid] < hover) lo = mid + 1;
      else hi = mid;
    }
    if (lo > 0 && Math.abs(xs[lo - 1] - hover) < Math.abs(xs[lo] - hover)) lo--;
    return Math.abs(xs[lo] - hover) <= Math.max(2, domainX / 100) ? lo : -1;
  }, [hover, xs, domainX]);
  const hx = hoverIndex >= 0 ? PAD.left + (xs[hoverIndex] / domainX) * plotW : 0;

  return (
    <div ref={ref} className="relative w-full select-none" style={{ height }}>
      {width > 0 && (
        <svg width={width} height={height} className="block" role="img" aria-label={label}>
          {ticksY.map((t) => {
            const y = PAD.top + plotH - (t / yMax) * plotH;
            return (
              <g key={t}>
                <line x1={PAD.left} x2={PAD.left + plotW} y1={y} y2={y} stroke="var(--border)" strokeDasharray={t === 0 ? undefined : "3 3"} />
                <text x={PAD.left - 6} y={y + 3.5} textAnchor="end" className="fill-[var(--faint)] text-[10px] tabular-nums">
                  {compactNumber(t)}
                </text>
              </g>
            );
          })}
          {ticksX.map((t) => {
            const x = PAD.left + (t / domainX) * plotW;
            // Labels at the ends stay inside the chart.
            const anchor = t === 0 ? "start" : x > PAD.left + plotW - 16 ? "end" : "middle";
            return (
              <text key={t} x={anchor === "end" ? PAD.left + plotW : x} y={height - 5} textAnchor={anchor} className="fill-[var(--faint)] text-[10px] tabular-nums">
                {timeLabel(t)}
              </text>
            );
          })}
          {paths.map((p) =>
            p.area ? <path key={`${p.key}-area`} d={p.area} fill={p.color} opacity={0.1} stroke="none" /> : null,
          )}
          {paths.map((p) => (
            <path
              key={p.key}
              d={p.d}
              fill="none"
              stroke={p.color}
              strokeWidth={p.dashed ? 1.25 : 1.6}
              strokeDasharray={p.dashed ? "4 3" : undefined}
              strokeLinejoin="round"
              strokeLinecap="round"
              opacity={p.dashed ? 0.8 : 1}
            />
          ))}
          {hoverIndex >= 0 && (
            <g pointerEvents="none">
              <line x1={hx} x2={hx} y1={PAD.top} y2={PAD.top + plotH} stroke="var(--border-strong)" />
              {series.map((s) => {
                const v = s.values[hoverIndex];
                if (!Number.isFinite(v)) return null;
                return <circle key={s.key} cx={hx} cy={PAD.top + plotH - (Math.max(0, v) / yMax) * plotH} r={3} fill="var(--bg)" stroke={s.color} strokeWidth={1.6} />;
              })}
            </g>
          )}
          <rect
            x={PAD.left}
            y={PAD.top}
            width={plotW}
            height={plotH}
            fill="transparent"
            onPointerMove={(e) => {
              const box = (e.currentTarget as SVGRectElement).getBoundingClientRect();
              const second = ((e.clientX - box.left) / box.width) * domainX;
              onHover(Math.max(0, Math.round(second)));
            }}
            onPointerLeave={() => onHover(null)}
          />
        </svg>
      )}
      {hoverIndex >= 0 && (
        <div
          className="pointer-events-none absolute top-1 z-10 min-w-[120px] rounded-lg border border-line bg-elev px-2 py-1.5 text-[11px] shadow-pop"
          style={hx > width / 2 ? { right: width - hx + 10 } : { left: hx + 10 }}
        >
          <div className="mb-0.5 font-medium text-muted">{timeLabel(xs[hoverIndex])}</div>
          {series.map((s) => (
            <div key={s.key} className="flex items-center gap-1.5 tabular-nums">
              <span className="h-2 w-2 shrink-0 rounded-full" style={{ background: s.color }} />
              <span className="text-muted">{s.label}</span>
              <span className="ml-auto pl-2 font-mono text-fg">{Number.isFinite(s.values[hoverIndex]) ? format(s.values[hoverIndex]) : "–"}</span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
});

/** Colored dots with labels (and optional latest values) for a chart's header. */
export function Legend({ items }: { items: { label: string; color: string; value?: string; dashed?: boolean }[] }) {
  return (
    <div className="flex flex-wrap items-center gap-x-3 gap-y-0.5 text-[11px] text-muted">
      {items.map((i) => (
        <span key={i.label} className="inline-flex items-center gap-1.5 whitespace-nowrap">
          {i.dashed ? (
            <span className="h-0 w-3 border-t-[1.5px] border-dashed" style={{ borderColor: i.color }} />
          ) : (
            <span className="h-2 w-2 rounded-full" style={{ background: i.color }} />
          )}
          {i.label}
          {i.value != null && <span className="font-mono tabular-nums text-fg">{i.value}</span>}
        </span>
      ))}
    </div>
  );
}

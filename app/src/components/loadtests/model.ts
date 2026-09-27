// Pure helpers for load tests: labels, stage shapes, target weights, request
// lookup, number formatting and chart decimation. No React, no store.
import type { LoadModel } from "../../bindings/LoadModel";
import type { LoadStage } from "../../bindings/LoadStage";
import type { LoadTarget } from "../../bindings/LoadTarget";
import type { RequestKind } from "../../bindings/RequestKind";
import type { Threshold } from "../../bindings/Threshold";
import type { ThresholdMetric } from "../../bindings/ThresholdMetric";
import type { ThresholdOp } from "../../bindings/ThresholdOp";
import type { TreeNode } from "../../bindings/TreeNode";

/** Same limits as the generator (crates/load). */
export const MAX_USERS = 5_000;
export const MAX_RATE = 50_000;

export interface ModelInfo {
  label: string;
  /** Badge in lists and tabs. */
  short: string;
  /** What a stage target counts. */
  unit: string;
  /** Unit in a sentence ("10 users"). */
  noun: (n: number) => string;
  description: string;
  /** What `active` counts. */
  activeLabel: string;
  limit: number;
}

export const MODELS: Record<LoadModel, ModelInfo> = {
  virtualUsers: {
    label: "Virtual users",
    short: "VU",
    unit: "users",
    noun: (n) => `${formatCount(n)} ${n === 1 ? "user" : "users"}`,
    description: "Each user sends a request, waits for the answer, then sends the next. A slower server gets fewer requests.",
    activeLabel: "Active users",
    limit: MAX_USERS,
  },
  arrivalRate: {
    label: "Request rate",
    short: "RPS",
    unit: "req/s",
    noun: (n) => `${formatCount(n)} req/s`,
    description: "Requests start on schedule however slow the server gets, so queueing shows up in latency instead of being hidden.",
    activeLabel: "In flight",
    limit: MAX_RATE,
  },
};

// ---- thresholds -------------------------------------------------------------

export type MetricUnit = "ms" | "%" | "req/s";

export const METRICS: { id: ThresholdMetric; label: string; short: string; unit: MetricUnit }[] = [
  { id: "p50", label: "p50 latency", short: "p50", unit: "ms" },
  { id: "p90", label: "p90 latency", short: "p90", unit: "ms" },
  { id: "p95", label: "p95 latency", short: "p95", unit: "ms" },
  { id: "p99", label: "p99 latency", short: "p99", unit: "ms" },
  { id: "p999", label: "p99.9 latency", short: "p99.9", unit: "ms" },
  { id: "avg", label: "Average latency", short: "avg", unit: "ms" },
  { id: "max", label: "Max latency", short: "max", unit: "ms" },
  { id: "errorRate", label: "Error rate", short: "errors", unit: "%" },
  { id: "rps", label: "Throughput", short: "throughput", unit: "req/s" },
];

export const OPS: ThresholdOp[] = ["<", "<=", ">", ">="];
export const OP_LABELS: Record<ThresholdOp, string> = { "<": "<", "<=": "≤", ">": ">", ">=": "≥" };

export const metricInfo = (m: ThresholdMetric) => METRICS.find((x) => x.id === m) ?? METRICS[0];

/** A sensible new threshold for a metric ("lower is better" except throughput). */
export function defaultThreshold(metric: ThresholdMetric = "p95"): Threshold {
  if (metric === "errorRate") return { metric, op: "<", value: 1 };
  if (metric === "rps") return { metric, op: ">=", value: 100 };
  return { metric, op: "<", value: 500 };
}

/** Keep the value/op meaningful when the metric changes (e.g. p95 < 500 → throughput ≥ 100). */
export function changeMetric(t: Threshold, metric: ThresholdMetric): Threshold {
  const from = metricInfo(t.metric).unit;
  const to = metricInfo(metric).unit;
  if (from === to) return { ...t, metric };
  const d = defaultThreshold(metric);
  return { ...t, metric, op: d.op, value: d.value };
}

export function formatMetricValue(metric: ThresholdMetric, value: number): string {
  const unit = metricInfo(metric).unit;
  if (unit === "ms") return formatLatency(value);
  if (unit === "%") return formatPercent(value);
  return `${formatRate(value)} req/s`;
}

/** "p95 < 500 ms", "Error rate ≤ 1 %", "Throughput ≥ 100 req/s", plus " · Get users" for one target. */
export function thresholdLabel(t: Pick<Threshold, "metric" | "op" | "value">, targetName?: string | null): string {
  const m = metricInfo(t.metric);
  const name = m.unit === "ms" ? m.short : m.label;
  const value = `${formatNumber(t.value)} ${m.unit === "%" ? "%" : m.unit}`;
  return `${name} ${OP_LABELS[t.op]} ${value}${targetName ? ` · ${targetName}` : ""}`;
}

// ---- stages -----------------------------------------------------------------

export const totalDuration = (stages: LoadStage[]) => stages.reduce((sum, s) => sum + Math.max(0, s.durationSecs), 0);
export const peakTarget = (stages: LoadStage[]) => stages.reduce((max, s) => Math.max(max, s.target), 0);

export type StagePreset = "constant" | "ramp" | "spike";

export const STAGE_PRESETS: { id: StagePreset; label: string; description: string }[] = [
  { id: "constant", label: "Constant", description: "The full load for the whole run" },
  { id: "ramp", label: "Ramp up, hold, ramp down", description: "Ramp up over a sixth of the run, hold, ramp down" },
  { id: "spike", label: "Spike", description: "A tenth of the load, a short burst at full load, back to a tenth" },
];

/** Stages with this shape at `peak` for `total` seconds. */
export function presetStages(preset: StagePreset, peak: number, total: number): LoadStage[] {
  const p = Math.max(0, Math.round(peak));
  const t = Math.max(1, Math.round(total));
  if (preset === "constant") return [{ durationSecs: 0, target: p }, { durationSecs: t, target: p }];
  if (preset === "ramp") {
    if (t < 3) return [{ durationSecs: t, target: p }];
    const r = Math.max(1, Math.round(t / 6));
    return [
      { durationSecs: r, target: p },
      { durationSecs: t - 2 * r, target: p },
      { durationSecs: r, target: 0 },
    ];
  }
  // Spike: baseline, a quick burst at full load, baseline again.
  const base = p > 0 ? Math.max(1, Math.round(p / 10)) : 0;
  if (t < 6) return [{ durationSecs: 0, target: p }, { durationSecs: t, target: p }];
  const warm = Math.round(t * 0.4);
  const edge = Math.max(1, Math.round(t * 0.05));
  const hold = Math.max(1, Math.round(t * 0.1));
  const rest = Math.max(0, t - warm - 2 * edge - hold);
  return [
    { durationSecs: 0, target: base },
    { durationSecs: warm, target: base },
    { durationSecs: edge, target: p },
    { durationSecs: hold, target: p },
    { durationSecs: edge, target: base },
    { durationSecs: rest, target: base },
  ];
}

const sameStages = (a: LoadStage[], b: LoadStage[]) =>
  a.length === b.length && a.every((s, i) => s.durationSecs === b[i].durationSecs && s.target === b[i].target);

/** The preset these stages were made with, if any. */
export function matchPreset(stages: LoadStage[]): StagePreset | null {
  const peak = peakTarget(stages);
  const total = totalDuration(stages);
  if (!stages.length || total <= 0) return null;
  return STAGE_PRESETS.find((p) => sameStages(presetStages(p.id, peak, total), stages))?.id ?? null;
}

/** Stretch or shrink every stage so they add up to `total` seconds (zero-length stages stay instant). */
export function scaleDurations(stages: LoadStage[], total: number): LoadStage[] {
  const want = Math.max(0, Math.round(total));
  const current = totalDuration(stages);
  if (!stages.length) return [{ durationSecs: want, target: 0 }];
  if (current === 0) return stages.map((s, i) => (i === stages.length - 1 ? { ...s, durationSecs: want } : s));
  const scaled = stages.map((s) => ({ ...s, durationSecs: s.durationSecs > 0 ? Math.max(want > 0 ? 1 : 0, Math.round((s.durationSecs * want) / current)) : 0 }));
  // Rounding: settle the difference on the longest stages.
  const longest = () => scaled.reduce((best, s, i) => (s.durationSecs > scaled[best].durationSecs ? i : best), 0);
  let diff = want - totalDuration(scaled);
  while (diff !== 0) {
    const i = longest();
    const fixed = Math.max(0, scaled[i].durationSecs + diff);
    diff -= fixed - scaled[i].durationSecs;
    scaled[i] = { ...scaled[i], durationSecs: fixed };
    if (fixed === 0 && diff !== 0 && totalDuration(scaled) === 0) break;
  }
  return scaled;
}

/** Scale every stage target so the highest is `peak` (a stage above zero stays above zero). */
export function scaleTargets(stages: LoadStage[], peak: number): LoadStage[] {
  const want = Math.max(0, Math.round(peak));
  const current = peakTarget(stages);
  if (current === 0) return stages.map((s) => ({ ...s, target: want }));
  return stages.map((s) => ({ ...s, target: s.target > 0 ? Math.max(want > 0 ? 1 : 0, Math.round((s.target * want) / current)) : 0 }));
}

/** Corner points `[second, target]` of the load shape, starting at `[0, 0]` (linear between them). */
export function stageShape(stages: LoadStage[]): [number, number][] {
  const out: [number, number][] = [[0, 0]];
  let t = 0;
  for (const s of stages) {
    t += Math.max(0, s.durationSecs);
    out.push([t, Math.max(0, s.target)]);
  }
  return out;
}

/** Planned target at `second` (users or req/s). */
export function targetAt(stages: LoadStage[], second: number): number {
  let t = 0;
  let prev = 0;
  for (const s of stages) {
    const d = Math.max(0, s.durationSecs);
    if (second < t + d) return prev + ((s.target - prev) * (second - t)) / d;
    t += d;
    prev = s.target;
  }
  return prev;
}

// ---- targets ----------------------------------------------------------------

/** Share (percent) of requests each target gets; 0 for disabled targets or weight 0. */
export function targetShares(targets: LoadTarget[]): number[] {
  const weights = targets.map((t) => (t.enabled === false ? 0 : Math.max(0, t.weight ?? 1)));
  const sum = weights.reduce((a, b) => a + b, 0);
  return weights.map((w) => (sum > 0 ? (w * 100) / sum : 0));
}

/** Targets that will send (enabled with a weight above 0). */
export const sendingTargets = (targets: LoadTarget[]) => targets.filter((t) => t.enabled !== false && (t.weight ?? 1) > 0);

export const clampInt = (value: number, min: number, max: number) => Math.min(max, Math.max(min, Math.trunc(Number.isFinite(value) ? value : 0)));

export interface RequestEntry {
  path: string;
  name: string;
  method: string;
  kind: RequestKind;
  /** Folder names above it, "Users / Admin". */
  trail: string;
  error?: string;
}

const isHttp = (n: TreeNode) => n.kind === "request" && (n.requestKind ?? "http") === "http";

/** Every request in the tree, depth first in sidebar order. */
export function flattenRequests(nodes: TreeNode[], trail: string[] = []): RequestEntry[] {
  return nodes.flatMap((n) =>
    n.kind === "folder"
      ? flattenRequests(n.children, [...trail, n.name])
      : [{ path: n.path, name: n.name, method: n.method ?? "GET", kind: n.requestKind ?? "http", trail: trail.join(" / "), error: n.error }],
  );
}

/** HTTP requests only (the ones a load test can send). */
export const httpRequests = (nodes: TreeNode[]) => flattenRequests(nodes).filter((r) => r.kind === "http" && !r.error);

/** Paths of every HTTP request inside a folder, recursively ("" = the whole collection). */
export function httpRequestsUnder(nodes: TreeNode[], folder: string): string[] {
  const walk = (list: TreeNode[]): string[] => list.flatMap((n) => (n.kind === "folder" ? walk(n.children) : isHttp(n) && !n.error ? [n.path] : []));
  if (!folder) return walk(nodes);
  const find = (list: TreeNode[]): TreeNode | null => {
    for (const n of list) {
      if (n.path === folder) return n;
      if (n.kind === "folder" && folder.startsWith(`${n.path}/`)) return find(n.children);
    }
    return null;
  };
  const node = find(nodes);
  return node?.kind === "folder" ? walk(node.children) : [];
}

/** A request's display name from its path when it is not in the tree ("Users/List users.yaml" → "List users"). */
export const nameFromPath = (path: string) => (path.split("/").pop() ?? path).replace(/\.ya?ml$/i, "");

// ---- formatting -------------------------------------------------------------

export function formatNumber(n: number): string {
  if (!Number.isFinite(n)) return "–";
  if (Number.isInteger(n)) return n.toLocaleString("en-US");
  return n.toLocaleString("en-US", { maximumFractionDigits: Math.abs(n) < 10 ? 2 : 1 });
}

export const formatCount = (n: number) => (Number.isFinite(n) ? Math.round(n).toLocaleString("en-US") : "–");

/** Requests per second: one decimal below 100, whole numbers above. */
export function formatRate(n: number): string {
  if (!Number.isFinite(n)) return "–";
  if (n === 0) return "0";
  if (n < 100) return n.toLocaleString("en-US", { minimumFractionDigits: 1, maximumFractionDigits: 1 });
  return Math.round(n).toLocaleString("en-US");
}

/** Latency in milliseconds with a precision that fits its size. */
export function formatLatency(ms: number | null | undefined): string {
  if (ms == null || !Number.isFinite(ms)) return "–";
  if (ms < 1) return `${ms.toFixed(2)} ms`;
  if (ms < 10) return `${ms.toFixed(1)} ms`;
  if (ms < 1000) return `${Math.round(ms)} ms`;
  if (ms < 60_000) return `${(ms / 1000).toFixed(ms < 10_000 ? 2 : 1)} s`;
  return formatDuration(ms / 1000);
}

/** A latency number without unit, for tables with the unit in the header. */
export function formatLatencyNumber(ms: number | null | undefined): string {
  if (ms == null || !Number.isFinite(ms)) return "–";
  if (ms < 1) return ms.toFixed(2);
  if (ms < 10) return ms.toFixed(1);
  return Math.round(ms).toLocaleString("en-US");
}

export function formatPercent(n: number | null | undefined): string {
  if (n == null || !Number.isFinite(n)) return "–";
  if (n === 0) return "0%";
  if (n < 0.01) return "<0.01%";
  if (n < 1) return `${n.toFixed(2)}%`;
  if (n < 10) return `${n.toFixed(1)}%`;
  return `${Math.round(n)}%`;
}

/** "45 s", "1m 30s", "1h 5m". */
export function formatDuration(totalSecs: number): string {
  const s = Math.max(0, Math.round(totalSecs));
  if (s < 60) return `${s} s`;
  if (s < 3600) return `${Math.floor(s / 60)}m${s % 60 ? ` ${s % 60}s` : ""}`;
  const m = Math.floor((s % 3600) / 60);
  return `${Math.floor(s / 3600)}h${m ? ` ${m}m` : ""}`;
}

/** Share of the whole computer from the generator's CPU (percent of one core, summed over cores). */
export function cpuShare(cpuPercent: number | null | undefined, cores: number): number | null {
  if (cpuPercent == null || !Number.isFinite(cpuPercent)) return null;
  return Math.min(100, Math.max(0, cpuPercent / Math.max(1, cores)));
}

/** Above this share of the computer the generator itself may limit the results. */
export const CPU_WARNING = 85;

// ---- charts -----------------------------------------------------------------

/**
 * Reduce a series to about `buckets` × 2 points for drawing: per bucket (equal x ranges)
 * the lowest and the highest point, in x order, so spikes survive. Returns indexes into
 * the input. Inputs up to `2 × buckets` points come back unchanged.
 */
export function decimate(xs: ArrayLike<number>, ys: ArrayLike<number>, buckets: number): number[] {
  const n = xs.length;
  const all = () => Array.from({ length: n }, (_, i) => i);
  if (n <= Math.max(2, buckets * 2)) return all();
  const x0 = xs[0];
  const span = xs[n - 1] - x0;
  if (!(span > 0)) return all();
  const out: number[] = [];
  let i = 0;
  for (let b = 0; b < buckets && i < n; b++) {
    const end = x0 + (span * (b + 1)) / buckets;
    let lo = -1;
    let hi = -1;
    for (; i < n && (xs[i] <= end || b === buckets - 1); i++) {
      if (!Number.isFinite(ys[i])) continue;
      if (lo < 0 || ys[i] < ys[lo]) lo = i;
      if (hi < 0 || ys[i] > ys[hi]) hi = i;
    }
    if (lo < 0) continue;
    if (lo === hi) out.push(lo);
    else out.push(Math.min(lo, hi), Math.max(lo, hi));
  }
  return out;
}

/** A round axis maximum at or above `v` (1, 2, 2.5, 5 × 10^k). */
export function niceMax(v: number): number {
  if (!(v > 0) || !Number.isFinite(v)) return 1;
  const exp = Math.floor(Math.log10(v));
  const base = 10 ** exp;
  for (const m of [1, 2, 2.5, 5, 10]) if (m * base >= v * 0.9999) return m * base;
  return 10 * base;
}

/** Round seconds for time-axis ticks (about `count` of them) up to `max` seconds. */
export function timeTicks(max: number, count = 5): number[] {
  if (!(max > 0)) return [0];
  const raw = max / Math.max(1, count);
  const steps = [1, 2, 5, 10, 15, 30, 60, 120, 300, 600, 900, 1800, 3600, 7200];
  const step = steps.find((s) => s >= raw) ?? Math.ceil(raw / 3600) * 3600;
  const ticks: number[] = [];
  for (let t = 0; t <= max + 1e-9; t += step) ticks.push(t);
  return ticks;
}

/** Short time-axis label: "0", "30s", "2m", "1m30s", "1h". */
export function timeLabel(secs: number): string {
  const s = Math.round(secs);
  if (s < 60) return `${s}s`;
  if (s < 3600) return s % 60 ? `${Math.floor(s / 60)}m${s % 60}s` : `${s / 60}m`;
  const m = Math.floor((s % 3600) / 60);
  return m ? `${Math.floor(s / 3600)}h${m}m` : `${s / 3600}h`;
}

/** Compact axis number: 950, 1.25k, 12.5k, 2.5M (three significant digits, so half of a 25 axis reads 12.5, not 13). */
export function compactNumber(n: number): string {
  const a = Math.abs(n);
  if (a >= 1e6) return `${+(n / 1e6).toPrecision(3)}M`;
  if (a >= 1e3) return `${+(n / 1e3).toPrecision(3)}k`;
  return `${+n.toPrecision(3)}`;
}

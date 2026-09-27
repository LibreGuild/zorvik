import { describe, expect, it } from "vitest";
import type { LoadStage } from "../../bindings/LoadStage";
import type { TreeNode } from "../../bindings/TreeNode";
import {
  changeMetric,
  compactNumber,
  cpuShare,
  decimate,
  defaultThreshold,
  flattenRequests,
  formatDuration,
  formatLatency,
  formatPercent,
  formatRate,
  httpRequests,
  httpRequestsUnder,
  matchPreset,
  nameFromPath,
  niceMax,
  peakTarget,
  presetStages,
  scaleDurations,
  scaleTargets,
  sendingTargets,
  stageShape,
  targetAt,
  targetShares,
  thresholdLabel,
  timeLabel,
  timeTicks,
  totalDuration,
} from "./model";

const stage = (durationSecs: number, target: number): LoadStage => ({ durationSecs, target });

describe("weighted targets", () => {
  it("splits requests by weight, ignoring disabled targets and weight 0", () => {
    const shares = targetShares([{ request: "a" }, { request: "b", weight: 3 }, { request: "c", enabled: false, weight: 5 }, { request: "d", weight: 0 }]);
    expect(shares).toEqual([25, 75, 0, 0]);
    expect(targetShares([{ request: "a", weight: 0 }])).toEqual([0]);
    expect(targetShares([])).toEqual([]);
  });

  it("lists the targets that send", () => {
    const targets = [{ request: "a" }, { request: "b", enabled: false }, { request: "c", weight: 0 }, { request: "d", weight: 2, enabled: true }];
    expect(sendingTargets(targets).map((t) => t.request)).toEqual(["a", "d"]);
  });
});

describe("stages", () => {
  it("sums durations and finds the peak", () => {
    const stages = [stage(10, 10), stage(40, 10), stage(10, 0)];
    expect(totalDuration(stages)).toBe(60);
    expect(peakTarget(stages)).toBe(10);
    expect(totalDuration([])).toBe(0);
  });

  it("builds presets that add up to the duration and reach the peak", () => {
    for (const preset of ["constant", "ramp", "spike"] as const) {
      for (const [peak, total] of [
        [10, 60],
        [5, 3],
        [1000, 3600],
        [1, 7],
      ]) {
        const stages = presetStages(preset, peak, total);
        expect(totalDuration(stages), `${preset} ${peak}/${total}`).toBe(total);
        expect(peakTarget(stages), `${preset} ${peak}/${total}`).toBe(peak);
        expect(stages.every((s) => s.durationSecs >= 0 && s.target >= 0)).toBe(true);
      }
    }
    expect(presetStages("constant", 5, 3)).toEqual([stage(0, 5), stage(3, 5)]);
    expect(presetStages("ramp", 10, 60)).toEqual([stage(10, 10), stage(40, 10), stage(10, 0)]);
  });

  it("recognizes a preset, and a custom shape", () => {
    expect(matchPreset(presetStages("spike", 200, 120))).toBe("spike");
    expect(matchPreset(presetStages("constant", 5, 3))).toBe("constant");
    expect(matchPreset([stage(10, 10), stage(40, 10), stage(10, 0)])).toBe("ramp");
    expect(matchPreset([stage(5, 3), stage(5, 9)])).toBeNull();
    expect(matchPreset([])).toBeNull();
  });

  it("scales durations to a new total, keeping instant stages instant", () => {
    const scaled = scaleDurations([stage(0, 5), stage(10, 5), stage(20, 0)], 60);
    expect(scaled).toEqual([stage(0, 5), stage(20, 5), stage(40, 0)]);
    // Rounding is settled so the total is exact.
    for (const total of [1, 7, 61, 1000]) expect(totalDuration(scaleDurations([stage(3, 1), stage(3, 1), stage(3, 1)], total))).toBe(total);
    expect(scaleDurations([], 30)).toEqual([stage(30, 0)]);
    expect(scaleDurations([stage(0, 4)], 12)).toEqual([stage(12, 4)]);
    expect(totalDuration(scaleDurations([stage(5, 1), stage(5, 2)], 0))).toBe(0);
  });

  it("scales targets to a new peak, keeping non-zero stages above zero", () => {
    expect(scaleTargets([stage(10, 10), stage(40, 10), stage(10, 0)], 50)).toEqual([stage(10, 50), stage(40, 50), stage(10, 0)]);
    expect(scaleTargets([stage(1, 1), stage(1, 100)], 10)).toEqual([stage(1, 1), stage(1, 10)]);
    expect(scaleTargets([stage(5, 0), stage(5, 0)], 7)).toEqual([stage(5, 7), stage(5, 7)]);
  });

  it("describes the load shape over time", () => {
    const stages = [stage(10, 10), stage(0, 20), stage(10, 0)];
    expect(stageShape(stages)).toEqual([
      [0, 0],
      [10, 10],
      [10, 20],
      [20, 0],
    ]);
    expect(targetAt(stages, 5)).toBe(5);
    expect(targetAt(stages, 10)).toBe(20);
    expect(targetAt(stages, 15)).toBe(10);
    expect(targetAt(stages, 99)).toBe(0);
  });
});

describe("thresholds", () => {
  it("labels thresholds with units", () => {
    expect(thresholdLabel({ metric: "p95", op: "<", value: 500 })).toBe("p95 < 500 ms");
    expect(thresholdLabel({ metric: "p999", op: "<=", value: 1200 })).toBe("p99.9 ≤ 1,200 ms");
    expect(thresholdLabel({ metric: "errorRate", op: "<", value: 0.5 })).toBe("Error rate < 0.5 %");
    expect(thresholdLabel({ metric: "rps", op: ">=", value: 100 }, "Get users")).toBe("Throughput ≥ 100 req/s · Get users");
  });

  it("keeps a threshold sensible when its metric changes", () => {
    const t = { metric: "p95" as const, op: "<" as const, value: 250 };
    expect(changeMetric(t, "p99")).toEqual({ metric: "p99", op: "<", value: 250 });
    expect(changeMetric(t, "rps")).toEqual({ metric: "rps", op: ">=", value: 100 });
    expect(changeMetric(t, "errorRate")).toEqual({ metric: "errorRate", op: "<", value: 1 });
    expect(defaultThreshold("max")).toEqual({ metric: "max", op: "<", value: 500 });
  });
});

describe("requests in the tree", () => {
  const req = (path: string, requestKind: TreeNode["requestKind"] = "http", method = "GET"): TreeNode => ({
    kind: "request",
    path,
    name: nameFromPath(path),
    seq: 0,
    method,
    requestKind,
    children: [],
  });
  const folder = (path: string, children: TreeNode[]): TreeNode => ({ kind: "folder", path, name: path.split("/").pop()!, seq: 0, children });
  const tree = [
    folder("Users", [req("Users/List.yaml"), req("Users/Socket.yaml", "websocket"), folder("Users/Admin", [req("Users/Admin/Delete.yaml", "http", "DELETE")])]),
    folder("Users2", [req("Users2/Other.yaml")]),
    req("Health.yaml"),
    { ...req("Broken.yaml"), error: "bad yaml" },
  ];

  it("collects HTTP requests under a folder, recursively", () => {
    expect(httpRequestsUnder(tree, "Users")).toEqual(["Users/List.yaml", "Users/Admin/Delete.yaml"]);
    expect(httpRequestsUnder(tree, "Users/Admin")).toEqual(["Users/Admin/Delete.yaml"]);
    expect(httpRequestsUnder(tree, "")).toEqual(["Users/List.yaml", "Users/Admin/Delete.yaml", "Users2/Other.yaml", "Health.yaml"]);
    expect(httpRequestsUnder(tree, "Nope")).toEqual([]);
  });

  it("flattens with folder trails and keeps only sendable HTTP requests for pickers", () => {
    const all = flattenRequests(tree);
    expect(all.find((r) => r.path === "Users/Admin/Delete.yaml")).toMatchObject({ trail: "Users / Admin", method: "DELETE", kind: "http" });
    expect(httpRequests(tree).map((r) => r.path)).toEqual(["Users/List.yaml", "Users/Admin/Delete.yaml", "Users2/Other.yaml", "Health.yaml"]);
    expect(nameFromPath("Users/List users.yaml")).toBe("List users");
  });
});

describe("formatting", () => {
  it("formats rates, latencies, percentages and durations", () => {
    expect(formatRate(0)).toBe("0");
    expect(formatRate(12.345)).toBe("12.3");
    expect(formatRate(1234.6)).toBe("1,235");
    expect(formatLatency(0.456)).toBe("0.46 ms");
    expect(formatLatency(4.26)).toBe("4.3 ms");
    expect(formatLatency(250.4)).toBe("250 ms");
    expect(formatLatency(1500)).toBe("1.50 s");
    expect(formatLatency(null)).toBe("–");
    expect(formatPercent(0)).toBe("0%");
    expect(formatPercent(0.004)).toBe("<0.01%");
    expect(formatPercent(0.5)).toBe("0.50%");
    expect(formatPercent(12.4)).toBe("12%");
    expect(formatDuration(45)).toBe("45 s");
    expect(formatDuration(90)).toBe("1m 30s");
    expect(formatDuration(120)).toBe("2m");
    expect(formatDuration(3900)).toBe("1h 5m");
  });

  it("turns CPU (percent of one core, summed) into a share of the computer", () => {
    expect(cpuShare(400, 8)).toBe(50);
    expect(cpuShare(900, 8)).toBe(100);
    expect(cpuShare(null, 8)).toBeNull();
  });
});

describe("chart helpers", () => {
  it("keeps short series as they are", () => {
    expect(decimate([0, 1, 2], [5, 6, 7], 10)).toEqual([0, 1, 2]);
  });

  it("reduces long series to about two points per pixel column, keeping spikes", () => {
    const n = 3600;
    const xs = Array.from({ length: n }, (_, i) => i);
    const ys = xs.map((i) => (i === 1234 ? 999 : i === 2345 ? -5 : 10));
    const idx = decimate(xs, ys, 300);
    expect(idx.length).toBeLessThanOrEqual(600);
    expect(idx).toContain(1234);
    expect(idx).toContain(2345);
    // Indexes stay in x order (a path never goes back).
    expect([...idx].sort((a, b) => a - b)).toEqual(idx);
    expect(idx[0]).toBe(0);
  });

  it("picks round axis maximums and time ticks", () => {
    expect(niceMax(0)).toBe(1);
    expect(niceMax(7)).toBe(10);
    expect(niceMax(180)).toBe(200);
    expect(niceMax(2400)).toBe(2500);
    expect(timeTicks(60, 4)).toEqual([0, 15, 30, 45, 60]);
    expect(timeTicks(3, 5)).toEqual([0, 1, 2, 3]);
    expect(timeLabel(90)).toBe("1m30s");
    expect(timeLabel(3600)).toBe("1h");
    expect(compactNumber(1500)).toBe("1.5k");
    expect(compactNumber(25_000)).toBe("25k");
    expect(compactNumber(2.5)).toBe("2.5");
    // Half of a 25 or 2,500 axis: not rounded to a wrong gridline label.
    expect(compactNumber(12.5)).toBe("12.5");
    expect(compactNumber(1250)).toBe("1.25k");
    expect(compactNumber(0)).toBe("0");
    expect(compactNumber(0.25)).toBe("0.25");
  });
});

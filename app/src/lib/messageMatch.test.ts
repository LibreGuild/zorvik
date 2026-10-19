import { describe, expect, it } from "vitest";
import { conditionHolds, logFilter, pairAcks, pairReplies, parseCondition, parsePath, patternProblem, reusePairs, valueAt, valueText } from "./messageMatch";

describe("JSON paths", () => {
  it("pick one value like the backend", () => {
    const doc = { a: [{ "b c": 1 }, { id: "x" }], n: null, t: true };
    const text = (p: string) => {
      const steps = parsePath(p);
      if (typeof steps === "string") throw new Error(steps);
      return valueText(valueAt(doc, steps));
    };
    expect(text("$.a[0]['b c']")).toBe("1");
    expect(text("a[-1].id")).toBe("x");
    expect(text("$.t")).toBe("true");
    expect(text("$.n")).toBeNull();
    expect(text("$.missing")).toBeNull();
    for (const bad of ["", "$.a[", "$.a[x]", "$..a", "$.*", "$a"]) expect(typeof parsePath(bad)).toBe("string");
    expect(parsePath("$..a")).toContain("wildcards and .. are not supported");
  });

  it("conditions compare text, numbers and null", () => {
    const msg = { type: "welcome", id: 7, gone: null };
    const holds = (p: string) => {
      const c = parseCondition(p);
      if (typeof c === "string") throw new Error(c);
      return conditionHolds(c, msg);
    };
    expect(holds('$.type == "welcome"')).toBe(true);
    expect(holds("$.type == welcome")).toBe(true);
    expect(holds("$.type == 'welcome'")).toBe(true);
    expect(holds("$.constructor")).toBe(false);
    expect(holds("$.toString")).toBe(false);
    expect(holds("$.id == 7")).toBe(true);
    expect(holds("$.id != 8")).toBe(true);
    expect(holds("$.gone == null")).toBe(true);
    expect(holds("$.gone")).toBe(false);
    expect(patternProblem("json", "$..x == 1")).toMatch(/not a JSON path/);
    expect(patternProblem("contains", "(")).toBeNull();
  });
});

describe("the log filter", () => {
  it("finds words, or JSON values after $", () => {
    const words = logFilter("Trade")!;
    expect(words('{"type":"trade"}', () => undefined)).toBe(true);
    const json = logFilter("$.type == trade")!;
    expect(json("x", () => ({ type: "trade" }))).toBe(true);
    expect(json("x", () => ({ type: "quote" }))).toBe(false);
    expect(logFilter("  ")).toBeNull();
  });
});

describe("pairReplies", () => {
  it("pairs each reply with the first unanswered message with its id", () => {
    const messages = [
      { id: 1, direction: "sent", timestamp: 100, json: { id: 1 } },
      { id: 2, direction: "sent", timestamp: 110, json: { id: 2 } },
      { id: 3, direction: "received", timestamp: 150, json: { id: 2, ok: true } },
      { id: 4, direction: "received", timestamp: 190, json: { id: 1 } },
      { id: 5, direction: "received", timestamp: 200, json: { id: 1 } },
      { id: 6, direction: "info", timestamp: 210, json: { id: 1 } },
    ];
    const byId = new Map(messages.map((m) => [m.id, m.json]));
    const pairs = pairReplies(messages, (id) => byId.get(id), "");
    expect([...pairs.entries()]).toEqual([
      [3, { to: 2, ms: 40 }],
      [4, { to: 1, ms: 90 }],
    ]);
    const custom = pairReplies(
      [
        { id: 1, direction: "sent", timestamp: 0 },
        { id: 2, direction: "received", timestamp: 5 },
      ],
      (id) => ({ meta: { ref: "a" }, id }),
      "$.meta.ref",
    );
    expect(custom.get(2)).toEqual({ to: 1, ms: 5 });
  });
});

describe("pairAcks", () => {
  it("pairs acknowledgements with the emits that asked", () => {
    const pairs = pairAcks([
      { id: 1, direction: "sent", timestamp: 10, detail: "asks for an acknowledgement (#1)" },
      { id: 2, direction: "received", timestamp: 15, detail: "asks for an acknowledgement (#1)" },
      { id: 3, direction: "received", timestamp: 42, detail: "acknowledgement of #1" },
      { id: 4, direction: "received", timestamp: 50, detail: "acknowledgement of #9" },
    ]);
    expect([...pairs.entries()]).toEqual([[3, { to: 1, ms: 32 }]]);
  });
});

describe("reusePairs", () => {
  it("keeps the earlier object for a pair that did not change", () => {
    const messages = [
      { id: 1, direction: "sent", timestamp: 10, detail: "asks for an acknowledgement (#1)" },
      { id: 2, direction: "received", timestamp: 42, detail: "acknowledgement of #1" },
    ];
    const first = reusePairs(new Map(), pairAcks(messages));
    const before = first.get(2)!;
    // The next flush pairs a new emit too.
    const more = [
      ...messages,
      { id: 3, direction: "sent", timestamp: 50, detail: "asks for an acknowledgement (#2)" },
      { id: 4, direction: "received", timestamp: 55, detail: "acknowledgement of #2" },
    ];
    const next = reusePairs(first, pairAcks(more));
    expect(next.get(2)).toBe(before);
    expect(next.get(4)).toEqual({ to: 3, ms: 5 });
  });

  it("takes the new object when a pair changed, and drops pairs that are gone", () => {
    const old = { to: 1, ms: 30 };
    const moved = { to: 5, ms: 30 };
    const slower = { to: 1, ms: 31 };
    const next = reusePairs(
      new Map([
        [2, old],
        [3, { to: 1, ms: 9 }],
        [9, { to: 8, ms: 1 }],
      ]),
      new Map([
        [2, moved],
        [3, slower],
      ]),
    );
    expect(next.get(2)).toBe(moved);
    expect(next.get(3)).toBe(slower);
    expect(next.has(9)).toBe(false);
  });
});

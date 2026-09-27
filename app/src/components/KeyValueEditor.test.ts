import { describe, expect, it } from "vitest";
import { parseBulk } from "./KeyValueEditor";

describe("bulk key/value editing", () => {
  it("parses lines and disabled rows", () => {
    expect(parseBulk("A: 1\n  // B : two words \n\nC", [])).toEqual([
      { key: "A", value: "1" },
      { key: "B", value: "two words", enabled: false },
      { key: "C", value: "" },
    ]);
  });

  it("keeps descriptions of existing rows, matched by key in order", () => {
    const prev = [
      { key: "X", value: "1", description: "first" },
      { key: "Y", value: "2", enabled: false, description: "why" },
      { key: "X", value: "3", description: "second" },
    ];
    expect(parseBulk("X: 1\nX: 30\nY: 2\nZ: 4", prev)).toEqual([
      { key: "X", value: "1", description: "first" },
      { key: "X", value: "30", description: "second" },
      { key: "Y", value: "2", description: "why" },
      { key: "Z", value: "4" },
    ]);
  });
});

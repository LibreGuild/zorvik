import { describe, expect, it } from "vitest";
import { formatMs } from "./format";

describe("formatMs", () => {
  it("formats durations", () => {
    expect(formatMs(12.4)).toBe("12 ms");
    expect(formatMs(1234)).toBe("1.23 s");
    expect(formatMs(61_000)).toBe("1m 1s");
    expect(formatMs(119_600)).toBe("2m 0s");
  });
});

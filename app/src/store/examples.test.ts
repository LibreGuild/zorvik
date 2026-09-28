import { describe, expect, it } from "vitest";
import { exampleHeaders, exampleName } from "./examples";

describe("examples", () => {
  it("keeps useful headers and drops framing, dates and cookies", () => {
    const headers = [
      { name: "Content-Type", value: "application/json" },
      { name: "Content-Length", value: "12" },
      { name: "Date", value: "Mon" },
      { name: "Set-Cookie", value: "sid=secret" },
      { name: "X-Request-Id", value: "abc" },
    ];
    expect(exampleHeaders(headers)).toEqual([
      { key: "Content-Type", value: "application/json", enabled: true },
      { key: "X-Request-Id", value: "abc", enabled: true },
    ]);
  });

  it("gives each example its own name", () => {
    expect(exampleName("200 OK", [])).toBe("200 OK");
    expect(exampleName("200 OK", ["200 OK", "200 OK (2)"])).toBe("200 OK (3)");
  });
});

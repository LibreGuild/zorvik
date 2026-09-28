import { describe, expect, it } from "vitest";
import type { ScriptReport } from "../../bindings/ScriptReport";
import { completeMembers, hasScripts, SNIPPETS, testSummary } from "./scriptsModel";

const labels = (before: string) => completeMembers(before)?.options.map((o) => o.label) ?? null;

describe("script helpers", () => {
  it("knows when there are scripts", () => {
    expect(hasScripts(undefined)).toBe(false);
    expect(hasScripts({ preRequest: "  \n" })).toBe(false);
    expect(hasScripts({ postResponse: "pm.test('x', () => {})" })).toBe(true);
  });

  it("completes pm members after a dot", () => {
    expect(labels("pm.")).toEqual(expect.arrayContaining(["test", "expect", "environment", "variables", "request", "response", "info"]));
    expect(labels("pm.environment.")).toEqual(["get", "set", "has", "unset", "clear", "replaceIn", "toObject", "name"]);
    expect(labels("  const t = pm.collectionVariables.g")).toContain("get");
    expect(labels("pm.request.headers.")).toContain("upsert");
    expect(labels("pm.response.headers.")).not.toContain("add");
    expect(labels("pm.response.to.have.")).toEqual(["status", "header", "jsonBody", "body"]);
    expect(labels("pm.response.to.not.be.")).toContain("ok");
    expect(labels("console.")).toEqual(["log", "info", "warn", "error", "debug"]);
    expect(labels("foo.bar.")).toBeNull();
    expect(labels("pm.nope.")).toBeNull();
  });

  it("offers the completed word's start", () => {
    const before = "x = pm.environment.se";
    expect(completeMembers(before)?.from).toBe(before.length - 2);
    expect(completeMembers("pm")?.from).toBe(0);
    expect(labels("pm")).toEqual(["pm", "require", "setTimeout", "setInterval", "clearTimeout", "clearInterval", "console"]);
    expect(completeMembers("")).toBeNull();
  });

  it("completes chai chains after pm.expect(…)", () => {
    expect(labels("pm.expect(json.id).")).toEqual(expect.arrayContaining(["to", "not", "equal"]));
    expect(labels("pm.test('a', () => pm.expect(pm.response.code).to.be.")).toEqual(expect.arrayContaining(["above", "oneOf", "ok"]));
    expect(labels("pm.response.json().")).toBeNull();
  });

  it("sums up tests", () => {
    const report: ScriptReport = {
      tests: [
        { name: "a", passed: true, skipped: false, error: null },
        { name: "b", passed: false, skipped: false, error: "expected 1 to equal 2" },
        { name: "c", passed: false, skipped: true, error: null },
        { name: "d", passed: true, skipped: false, error: null },
      ],
      console: [],
      errors: [],
    };
    expect(testSummary(report)).toEqual({ passed: 2, failed: 1, skipped: 1, total: 3 });
    expect(testSummary(null)).toEqual({ passed: 0, failed: 0, skipped: 0, total: 0 });
  });

  it("has snippets for both scripts", () => {
    expect(SNIPPETS.preRequest.length).toBeGreaterThan(0);
    expect(SNIPPETS.postResponse.some((s) => s.code.includes("pm.test"))).toBe(true);
  });
});

// Script results in the response pane: tests with their errors, console lines and script errors.
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import type { ScriptReport } from "../../bindings/ScriptReport";
import { ConsoleView, describeFailure, TestsLabel, TestsView } from "./ScriptResults";

const report: ScriptReport = {
  tests: [
    { name: "Status is 200", passed: true, skipped: false, error: null },
    { name: "Has an id", passed: false, skipped: false, error: "expected { name: 'a' } to have property 'id'" },
    { name: "Later", passed: false, skipped: true, error: null },
  ],
  console: [
    { level: "log", message: "hello" },
    { level: "warn", message: "careful" },
  ],
  errors: [{ script: "Post-response script of folder 'Users'", message: "ReferenceError: x is not defined", line: 3 }],
};

afterEach(cleanup);

describe("script results", () => {
  it("lists tests with a summary", () => {
    render(<TestsView report={report} />);
    expect(screen.getByText("1 passed · 1 failed · 1 skipped")).toBeTruthy();
    expect(screen.getByText("expected { name: 'a' } to have property 'id'")).toBeTruthy();
    expect(screen.getByLabelText("Skipped")).toBeTruthy();
  });

  it("labels the tab with passed/total", () => {
    const { container } = render(<TestsLabel report={report} />);
    expect(container.textContent).toBe("Tests 1/2");
  });

  it("shows console lines and script errors", () => {
    render(<ConsoleView report={report} />);
    expect(screen.getByText("careful")).toBeTruthy();
    expect(screen.getByText("Post-response script of folder 'Users' failed at line 3: ReferenceError: x is not defined")).toBeTruthy();
    expect(describeFailure({ script: "Pre-request script of workspace", message: "boom", line: null })).toBe("Pre-request script of workspace failed: boom");
  });

  it("says when there is nothing to show", () => {
    render(<ConsoleView report={{ tests: [], console: [], errors: [] }} />);
    expect(screen.getByText("No console output")).toBeTruthy();
  });
});

// Reply rules say under the pattern when the server couldn't use it: a regex its engine
// refuses, or a JSON condition that doesn't parse.
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ReplyRule } from "../../bindings/ReplyRule";
import { patternProblem } from "../../lib/messageMatch";

vi.mock("../../lib/rpc", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/rpc")>()),
  api: { regexTest: vi.fn((pattern: string) => (pattern.includes("(?=") ? Promise.reject(new Error("look-around is not supported")) : Promise.resolve({}))) },
}));

const { ReplyRulesEditor } = await import("./ReplyRulesEditor");

afterEach(cleanup);

const show = (rules: ReplyRule[]) => render(<ReplyRulesEditor rules={rules} onChange={() => {}} closing />);

describe("ReplyRulesEditor", () => {
  it("explains a JSON condition that doesn't parse", () => {
    show([{ match: "json", pattern: "$.items[0 == 1", reply: "" }]);
    expect(screen.getByText(patternProblem("json", "$.items[0 == 1")!)).toBeTruthy();
  });

  it("explains a regex the server's engine refuses", async () => {
    show([
      { match: "regex", pattern: "^a(?=b)", reply: "" },
      { match: "regex", pattern: "^GET (\\w+)", reply: "" },
    ]);
    expect(await screen.findByText("invalid regex: look-around is not supported")).toBeTruthy();
    expect(screen.getAllByText(/invalid regex/)).toHaveLength(1);
  });

  it("marks a close code a server can't send", () => {
    show([{ match: "any", pattern: "", reply: "", close: 2000 }]);
    expect(screen.getByLabelText("Close code").getAttribute("aria-invalid")).toBe("true");
  });
});

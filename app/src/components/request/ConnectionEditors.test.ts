// Renaming a saved message keeps the on-connect steps that sent it on it, and only those.
import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(() => Promise.resolve(null)) }));
vi.mock("../../lib/events", () => ({ onEvent: () => () => {} }));

const { renameMessage } = await import("./ConnectionEditors");

const options = {
  messages: [
    { name: "Login", text: "a" },
    { name: "Ping", text: "b" },
  ],
  onConnect: [
    { type: "send" as const, message: "Login" },
    { type: "repeat" as const, message: "Ping", everyMs: 1000 },
    { type: "wait" as const, match: "contains" as const, pattern: "Login" },
  ],
};

describe("renameMessage", () => {
  it("re-points the steps that sent the renamed message", () => {
    const next = renameMessage(options, 0, "Sign in")!;
    expect(next.messages?.map((m) => m.name)).toEqual(["Sign in", "Ping"]);
    expect(next.onConnect).toEqual([{ type: "send", message: "Sign in" }, options.onConnect[1], options.onConnect[2]]);
  });

  it("refuses an empty name or another message's", () => {
    expect(renameMessage(options, 0, "Ping")).toBeNull();
    expect(renameMessage(options, 0, "  ")).toBeNull();
  });

  it("leaves steps alone that sent another message of the same name", () => {
    // Steps send the first message with their name: renaming the second copy moves nothing.
    const dupes = { ...options, messages: [...options.messages, { name: "Login", text: "c" }] };
    const next = renameMessage(dupes, 2, "Login 2")!;
    expect(next.onConnect).toEqual(options.onConnect);
    expect(renameMessage(dupes, 0, "Sign in")!.onConnect![0]).toEqual({ type: "send", message: "Sign in" });
  });
});

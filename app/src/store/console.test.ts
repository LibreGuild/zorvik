import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ConsoleLine } from "../bindings/ConsoleLine";
import type { StreamEvent } from "../bindings/StreamEvent";

const handlers: ((e: StreamEvent) => void)[] = [];
vi.mock("../lib/events", () => ({
  onEvent: (h: (e: StreamEvent) => void) => {
    handlers.push(h);
    return () => {};
  },
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("../lib/rpc", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/rpc")>();
  return { ...actual, api: { consoleList: vi.fn(), consoleClear: vi.fn(() => Promise.resolve(null)) } };
});

const { api } = await import("../lib/rpc");
const { useUi } = await import("./ui");
const { appendLines, clearConsole, filterLines, flushConsole, initConsole, isProblem, toggleConsole, useConsole } = await import("./console");

const log = (id: number, level: "log" | "warn" | "error", message: string, request: string | null = null): ConsoleLine => ({
  id,
  at: 1_700_000_000_000 + id,
  source: "user",
  request,
  item: { type: "log", level, message },
});

const sent = (id: number, url: string, status: number | null, error: string | null = null, request: string | null = null): ConsoleLine => ({
  id,
  at: 1_700_000_000_000 + id,
  source: "runner",
  request,
  item: { type: "exchange", protocol: "HTTP", method: "GET", url, status, statusText: status ? "OK" : null, error, durationMs: 3, size: 10, origin: null },
});

const emit = (line: ConsoleLine) => handlers.forEach((h) => h({ type: "console", event: { type: "line", line } }));

beforeEach(() => {
  flushConsole();
  useConsole.setState({ lines: [], unseen: 0 });
  useUi.setState({ consoleOpen: false });
});

describe("console lines", () => {
  it("keeps the newest lines and never a line twice", () => {
    const first = [log(1, "log", "a"), log(2, "log", "b")];
    expect(appendLines(first, [log(2, "log", "b"), log(3, "log", "c")]).map((l) => l.id)).toEqual([1, 2, 3]);
    expect(appendLines(first, [log(1, "log", "a")])).toBe(first);
    expect(appendLines(first, [log(3, "log", "c"), log(4, "log", "d")], 3).map((l) => l.id)).toEqual([2, 3, 4]);
    expect(appendLines([], []).length).toBe(0);
  });

  it("knows a failure when it sees one", () => {
    expect(isProblem(sent(1, "http://x.test/a", 200))).toBe(false);
    expect(isProblem(sent(2, "http://x.test/a", 404))).toBe(true);
    expect(isProblem(sent(3, "http://x.test/a", null, "Connection refused"))).toBe(true);
    expect(isProblem(log(4, "warn", "careful"))).toBe(false);
    expect(isProblem(log(5, "error", "boom"))).toBe(true);
  });

  it("filters by kind and by text", () => {
    const lines = [sent(1, "http://x.test/users", 200, null, "List users"), log(2, "log", "token saved", "Login"), sent(3, "http://x.test/orders", 500), log(4, "error", "boom")];
    expect(filterLines(lines, "all", "")).toBe(lines);
    expect(filterLines(lines, "requests", "").map((l) => l.id)).toEqual([1, 3]);
    expect(filterLines(lines, "logs", "").map((l) => l.id)).toEqual([2, 4]);
    expect(filterLines(lines, "errors", "").map((l) => l.id)).toEqual([3, 4]);
    expect(filterLines(lines, "all", "ORDERS").map((l) => l.id)).toEqual([3]);
    expect(filterLines(lines, "all", "login").map((l) => l.id)).toEqual([2]);
    expect(filterLines(lines, "all", " 500 ").map((l) => l.id)).toEqual([3]);
    expect(filterLines(lines, "requests", "users").map((l) => l.id)).toEqual([1]);
    expect(filterLines(lines, "logs", "users")).toEqual([]);
  });
});

describe("console events", () => {
  it("loads what the app has, then follows events in batches", async () => {
    vi.mocked(api.consoleList).mockResolvedValue([log(1, "log", "kept"), sent(2, "http://x.test/a", 200)]);
    await initConsole();
    expect(useConsole.getState().lines.map((l) => l.id)).toEqual([1, 2]);

    emit(sent(3, "http://x.test/b", 502));
    emit(log(4, "error", "boom"));
    emit(log(5, "log", "fine"));
    // Not yet: lines wait a moment for company.
    expect(useConsole.getState().lines.length).toBe(2);
    flushConsole();
    expect(useConsole.getState().lines.map((l) => l.id)).toEqual([1, 2, 3, 4, 5]);
    // Closed: the two failures are counted until it opens.
    expect(useConsole.getState().unseen).toBe(2);
    toggleConsole();
    expect(useUi.getState().consoleOpen).toBe(true);
    expect(useConsole.getState().unseen).toBe(0);
    emit(log(6, "error", "seen as it happens"));
    flushConsole();
    expect(useConsole.getState().unseen).toBe(0);

    // A line that came both in the list and as an event shows once.
    emit(log(6, "error", "seen as it happens"));
    flushConsole();
    expect(useConsole.getState().lines.length).toBe(6);
  });

  it("drops a list that was asked for before the console was cleared", async () => {
    let answer: (lines: ConsoleLine[]) => void = () => {};
    vi.mocked(api.consoleList).mockReturnValue(new Promise((resolve) => (answer = resolve)));
    const loading = initConsole();
    // Another workspace opened meanwhile: the app cleared its console.
    handlers.forEach((h) => h({ type: "console", event: { type: "cleared" } }));
    answer([log(1, "log", "of the workspace before")]);
    await loading;
    expect(useConsole.getState().lines).toEqual([]);
  });

  it("empties when the app says the console was cleared", async () => {
    useConsole.setState({ lines: [log(1, "log", "a")], unseen: 1 });
    emit(log(2, "log", "waiting"));
    await clearConsole();
    expect(api.consoleClear).toHaveBeenCalled();
    // The app answers with the event.
    handlers.forEach((h) => h({ type: "console", event: { type: "cleared" } }));
    flushConsole();
    expect(useConsole.getState()).toMatchObject({ lines: [], unseen: 0 });
  });
});

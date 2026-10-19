import { describe, expect, it, vi } from "vitest";

vi.mock("../lib/events", () => ({ onEvent: () => () => {} }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

type StreamMessage = import("./tabs").StreamMessage;
const { appendMessages, MAX_LOG_BYTES } = await import("./tabs");

let seq = 0;
const msg = (text: string | null, base64: string | null = null): StreamMessage => ({
  id: ++seq,
  direction: "received",
  kind: "text",
  text,
  base64,
  size: (text ?? base64 ?? "").length,
  timestamp: seq,
});
const ids = (messages: StreamMessage[]) => messages.map((m) => m.id);

describe("the message log's caps", () => {
  it("keeps the newest messages by count", () => {
    const a = [msg("a"), msg("b"), msg("c")];
    const kept = appendMessages(a.slice(0, 2), [a[2]], 2);
    expect(ids(kept)).toEqual(ids(a.slice(1)));
  });

  it("drops the oldest messages once the text and base64 pass the size cap", () => {
    // 4 characters each, 10 at most: two fit.
    let log: StreamMessage[] = [];
    const all: StreamMessage[] = [];
    for (let flush = 0; flush < 6; flush++) {
      const added = [msg("abcd"), msg(null, "QUJD")];
      all.push(...added);
      log = appendMessages(log, added, 5000, 10);
      // The running size matches the newest messages that fit, flush after flush.
      expect(ids(log)).toEqual(ids(all.slice(-2)));
    }
    // A bigger message pushes out what no longer fits; small ones fill up again after it.
    log = appendMessages(log, [msg("123456789")], 5000, 10);
    expect(log.map((m) => m.text)).toEqual(["123456789"]);
    log = appendMessages(log, [msg("x")], 5000, 10);
    expect(log.map((m) => m.text)).toEqual(["123456789", "x"]);
    log = appendMessages(log, [msg("y")], 5000, 10);
    expect(log.map((m) => m.text)).toEqual(["x", "y"]);
  });

  it("keeps the newest message even when it alone is over the cap", () => {
    const log = appendMessages([msg("ab")], [msg("far too long")], 5000, 4);
    expect(log.map((m) => m.text)).toEqual(["far too long"]);
  });

  it("measures a log it did not build (cleared or new) from its messages", () => {
    const outside = [msg("abcd"), msg("efgh")];
    expect(ids(appendMessages(outside, [msg("ijkl")], 5000, 10))).toEqual([outside[1].id, seq]);
    expect(appendMessages([], [msg("abcd")], 5000, 10)).toHaveLength(1);
  });

  it("caps the log at about 64 MB by default", () => {
    expect(MAX_LOG_BYTES).toBe(64 * 1024 * 1024);
    // One string shared by both messages: 2 × 33 MB is over the cap, one is not.
    const big = "x".repeat(33 * 1024 * 1024);
    const first = appendMessages([], [msg(big)]);
    expect(first).toHaveLength(1);
    const second = appendMessages(first, [msg(big)]);
    expect(ids(second)).toEqual([seq]);
  });
});

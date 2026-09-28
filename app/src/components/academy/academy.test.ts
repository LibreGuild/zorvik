import { describe, expect, it } from "vitest";
import type { CourseView } from "../../bindings/CourseView";
import type { ProgressView } from "../../bindings/ProgressView";
import { parseFlow, parseSequence } from "./Diagrams";
import { parseMarkdown } from "./Markdown";
import { levelFraction, nextLesson } from "./parts";

describe("lesson markdown", () => {
  it("reads headings, lists, tables, callouts and code", () => {
    const blocks = parseMarkdown(
      [
        "## Title",
        "Some *text* that",
        "wraps.",
        "",
        "- one",
        "- two",
        "  continued",
        "1. first",
        "",
        "| a | b |",
        "|---|---|",
        "| 1 | 2 |",
        "",
        "> [!tip] Remember",
        "> This part.",
        "",
        "```sequence",
        "You -> Server: GET /",
        "```",
      ].join("\n"),
    );
    expect(blocks.map((b) => b.type)).toEqual(["heading", "paragraph", "list", "list", "table", "callout", "code"]);
    expect(blocks[1]).toEqual({ type: "paragraph", text: "Some *text* that wraps." });
    expect(blocks[2]).toEqual({ type: "list", ordered: false, items: ["one", "two continued"] });
    expect(blocks[4]).toEqual({ type: "table", head: ["a", "b"], rows: [["1", "2"]] });
    const callout = blocks[5];
    expect(callout.type === "callout" && callout.kind).toBe("tip");
    expect(callout.type === "callout" && callout.body).toEqual([{ type: "paragraph", text: "This part." }]);
    expect(blocks[6]).toEqual({ type: "code", lang: "sequence", code: "You -> Server: GET /" });
  });
});

describe("diagrams", () => {
  it("parses sequence diagrams", () => {
    const d = parseSequence("participants: You, DNS\nYou -> Server: GET /\nServer --> You: 200 OK\nNote over You, Server: done\nbad line");
    expect(d.participants).toEqual(["You", "DNS", "Server"]);
    expect(d.rows).toEqual([
      { kind: "message", from: 0, to: 2, text: "GET /", reply: false },
      { kind: "message", from: 2, to: 0, text: "200 OK", reply: true },
      { kind: "note", from: 0, to: 2, text: "done" },
    ]);
  });

  it("parses flows with labels", () => {
    expect(parseFlow("A -> B -[TLS]-> C")).toEqual([{ nodes: ["A", "B", "C"], labels: ["", "TLS"] }]);
  });
});

describe("course helpers", () => {
  const lesson = (id: string) => ({ id, title: id, summary: "", minutes: 1, labMinutes: null, labSteps: 0, questions: 0 });
  const course: CourseView = {
    units: [
      { id: "u1", title: "U1", summary: "", color: "#000", image: "x", capstone: false, badge: { id: "b1", name: "B", description: "" }, lessons: [lesson("a"), lesson("b")] },
      { id: "u2", title: "U2", summary: "", color: "#000", image: "y", capstone: false, badge: { id: "b2", name: "B", description: "" }, lessons: [lesson("c")] },
    ],
    extraBadges: [],
  };
  const progress = (done: string[], last: string | null): ProgressView => ({
    xp: 50,
    level: 2,
    rank: "Newbie Node",
    levelXp: 40,
    nextLevelXp: 120,
    streak: 0,
    bestStreak: 0,
    activeToday: false,
    badges: [],
    lessons: done.map((id) => ({ id, completed: true, stepsDone: 0, labDone: false, quizBest: null })),
    units: [],
    lastLesson: last,
    graduatedAt: null,
    completedLessons: done.length,
    totalLessons: 3,
  });

  it("continues the last lesson, else the next unfinished one", () => {
    expect(nextLesson(course, progress([], null))?.lesson.id).toBe("a");
    expect(nextLesson(course, progress([], "b"))?.lesson.id).toBe("b");
    expect(nextLesson(course, progress(["b"], "b"))?.lesson.id).toBe("c");
    expect(nextLesson(course, progress(["b", "c"], "c"))?.lesson.id).toBe("a");
    expect(nextLesson(course, progress(["a", "b", "c"], "c"))).toBeNull();
  });

  it("measures progress through a level", () => {
    expect(levelFraction(progress([], null))).toBeCloseTo(10 / 80);
  });
});

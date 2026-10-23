import { EditorState } from "@codemirror/state";
import { describe, expect, it } from "vitest";
import { LONG_LINE, longLines, showMore, variableAt } from "./CodeEditor";
import { variableHint } from "./VarInput";
import { useWorkspace } from "../store/workspace";

/** The parts of each shortened line that aren't drawn, as [from, to]. */
function hidden(state: EditorState): [number, number][] {
  const out: [number, number][] = [];
  state.field(longLines).decorations.between(0, state.doc.length, (from, to) => {
    out.push([from, to]);
  });
  return out;
}

describe("very long lines", () => {
  const base64 = "iVBORw0KGgo".repeat(250_000); // 2.75 MB on one line
  const doc = `{\n  "data": [{ "b64_json": "${base64}" }],\n  "created": 1\n}`;

  it("draw only their start and end; the document keeps all the text", () => {
    const state = EditorState.create({ doc, extensions: longLines });
    const parts = hidden(state);
    expect(parts).toHaveLength(1);
    const line = state.doc.line(2);
    expect(parts[0][0]).toBe(line.from + 1_000);
    expect(parts[0][1]).toBe(line.to - 80);
    expect(state.doc.toString()).toBe(doc);
  });

  it("leave ordinary lines and documents alone", () => {
    const short = EditorState.create({ doc: '{"a": 1}', extensions: longLines });
    expect(hidden(short)).toEqual([]);
    const many = EditorState.create({ doc: "x".repeat(100) + "\n".repeat(5_000), extensions: longLines });
    expect(hidden(many)).toEqual([]);
    const justUnder = EditorState.create({ doc: "y".repeat(LONG_LINE), extensions: longLines });
    expect(hidden(justUnder)).toEqual([]);
  });

  it("show more on request, and follow edits", () => {
    let state = EditorState.create({ doc, extensions: longLines });
    const line = state.doc.line(2);
    state = state.update({ effects: showMore.of(line.from) }).state;
    expect(hidden(state)[0][0]).toBe(line.from + 51_000);
    // Typing above the line moves it; it stays shortened, with what was shown.
    state = state.update({ changes: { from: 0, insert: "// note\n" } }).state;
    const moved = state.doc.line(3);
    expect(hidden(state)).toEqual([[moved.from + 51_000, moved.to - 80]]);
    // Cutting the line short makes it an ordinary line again.
    state = state.update({ changes: { from: moved.from + 100, to: moved.to - 10 } }).state;
    expect(hidden(state)).toEqual([]);
    // A long line typed or pasted is shortened.
    state = state.update({ changes: { from: state.doc.length, insert: `\n${"z".repeat(20_000)}` } }).state;
    expect(hidden(state)).toHaveLength(1);
  });
});

describe("hovering {{variables}} in an editor", () => {
  it("finds the variable under the pointer", () => {
    const line = '{"user": "{{ userId }}", "at": {{$timestamp}}}';
    expect(variableAt(line, line.indexOf("userId"))).toEqual({ name: "userId", from: 10, to: 22 });
    expect(variableAt(line, line.indexOf("$timestamp"))?.name).toBe("$timestamp");
    expect(variableAt(line, 2)).toBeNull();
  });

  it("tells the value and where it comes from, or that it isn't defined", () => {
    useWorkspace.setState({ variables: [{ key: "userId", value: "42", source: "environment", secret: false }] } as never);
    expect(variableHint("userId").map((l) => l.text)).toEqual(["{{userId}}", "42", "From the active environment"]);
    expect(variableHint("nope")[1]).toEqual({ text: "Not defined in the active environment or workspace", tone: "danger" });
  });
});

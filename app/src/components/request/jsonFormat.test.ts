import { describe, expect, it } from "vitest";
import { beautifyJson, prettyMessage } from "./jsonFormat";

describe("beautifyJson", () => {
  it("matches JSON.stringify indentation for plain JSON", () => {
    const src = '{"a":1,"b":[true,null,{"c":"x"}],"e":{},"f":[ ]}';
    expect(beautifyJson(src)).toBe(JSON.stringify(JSON.parse(src), null, 2));
  });

  it("keeps variables inside and outside strings", () => {
    expect(beautifyJson('{"name":"{{name}}","id":{{id}},"tag":"a-{{ t }}-b"}')).toBe(
      '{\n  "name": "{{name}}",\n  "id": {{id}},\n  "tag": "a-{{ t }}-b"\n}',
    );
    expect(beautifyJson("{{{key}}:1}")).toBe("{\n  {{key}}: 1\n}");
  });

  it("keeps numbers and escapes exactly as typed", () => {
    expect(beautifyJson('{"id":12345678901234567890,"f":1.50,"e":1e5,"s":"a\\"b\\u00e9"}')).toBe(
      '{\n  "id": 12345678901234567890,\n  "f": 1.50,\n  "e": 1e5,\n  "s": "a\\"b\\u00e9"\n}',
    );
  });

  it("leaves whitespace and braces inside strings alone", () => {
    expect(beautifyJson('[ "a , b", "{ x : [ ] }" ]')).toBe('[\n  "a , b",\n  "{ x : [ ] }"\n]');
  });

  it("rejects invalid JSON", () => {
    expect(beautifyJson('{"a":}')).toBeNull();
    expect(beautifyJson("{a:1}")).toBeNull();
    expect(beautifyJson("")).toBeNull();
  });
});

describe("prettyMessage", () => {
  it("re-indents JSON and keeps it exact when nothing is nested", () => {
    expect(prettyMessage('{"id":12345678901234567890}')).toBe('{\n  "id": 12345678901234567890\n}');
    expect(prettyMessage("not json")).toBeNull();
  });

  it("expands string values that hold JSON (a Socket.IO argument sent as a JSON string)", () => {
    const args = JSON.stringify(["update", JSON.stringify({ price: 12.5, tags: ["a"] })]);
    expect(prettyMessage(args)).toBe(JSON.stringify(["update", { price: 12.5, tags: ["a"] }], null, 2));
    // JSON inside JSON inside JSON, and strings that only look like JSON stay as they are.
    const twice = JSON.stringify({ data: JSON.stringify({ inner: JSON.stringify({ ok: true }) }), note: "[not json]" });
    expect(prettyMessage(twice)).toBe(JSON.stringify({ data: { inner: { ok: true } }, note: "[not json]" }, null, 2));
  });
});

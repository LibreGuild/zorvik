import { describe, expect, it } from "vitest";
import { beautifyJson } from "./jsonFormat";

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

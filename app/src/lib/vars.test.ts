import { describe, expect, it } from "vitest";
import { segments, variableNames } from "./vars";

describe("variable segments", () => {
  it("splits text around variables", () => {
    expect(segments("a {{ x }} b {{$uuid}}")).toEqual([
      { text: "a " },
      { text: "{{ x }}", variable: "x" },
      { text: " b " },
      { text: "{{$uuid}}", variable: "$uuid" },
    ]);
    expect(segments("{not} {{}}")).toEqual([{ text: "{not} {{}}" }]);
    expect(variableNames("{{a}}/{{b}}")).toEqual(["a", "b"]);
  });
});

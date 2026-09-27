import { describe, expect, it } from "vitest";
import { applyParamRows, paramRows, pathParamNames, splitUrl, syncPathParams } from "./url";

describe("url params", () => {
  it("splits URL parts", () => {
    expect(splitUrl("https://h/p?a=1#x")).toEqual({ base: "https://h/p", query: "a=1", hash: "#x" });
    expect(splitUrl("h/p")).toEqual({ base: "h/p", query: null, hash: "" });
  });

  it("builds rows from URL and disabled params", () => {
    const rows = paramRows("http://h/?a=1&flag&b={{v}}", [{ key: "off", value: "x", enabled: false }]);
    expect(rows).toEqual([
      { key: "a", value: "1" },
      { key: "flag", value: "" },
      { key: "b", value: "{{v}}" },
      { key: "off", value: "x", enabled: false },
    ]);
  });

  it("round-trips edits back into the URL", () => {
    const out = applyParamRows("http://h/p?a=1#frag", [
      { key: "a", value: "1&2" },
      { key: "b", value: "x y" },
      { key: "c", value: "3", enabled: false },
      { key: "", value: "" },
    ]);
    expect(out.url).toBe("http://h/p?a=1%262&b=x y#frag");
    expect(out.disabledParams).toEqual([{ key: "c", value: "3", enabled: false }]);
    expect(applyParamRows("http://h/p?a=1", []).url).toBe("http://h/p");
  });

  it("finds path params outside the host", () => {
    expect(pathParamNames("http://localhost:8080/users/:id/posts/:post?x=:no")).toEqual(["id", "post"]);
    expect(pathParamNames("{{base}}/:id")).toEqual(["id"]);
    expect(syncPathParams("http://h/:a/:b", [{ key: "b", value: "2" }, { key: "gone", value: "x" }])).toEqual([
      { key: "a", value: "" },
      { key: "b", value: "2" },
    ]);
  });
});

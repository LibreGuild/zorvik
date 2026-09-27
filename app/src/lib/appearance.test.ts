import { describe, expect, it } from "vitest";
import { fontFamily, stepZoom } from "./appearance";

describe("appearance", () => {
  it("steps zoom through the browser levels and stops at the ends", () => {
    expect(stepZoom(100, 1)).toBe(110);
    expect(stepZoom(100, -1)).toBe(90);
    expect(stepZoom(200, 1)).toBe(200);
    expect(stepZoom(50, -1)).toBe(50);
    expect(stepZoom(133, 1)).toBe(150);
    expect(stepZoom(133, -1)).toBe(125);
    expect(stepZoom(175, 0)).toBe(100);
  });

  it("quotes a font name and drops what could break out of the CSS value", () => {
    expect(fontFamily("  Fira Code ")).toBe('"Fira Code"');
    expect(fontFamily('Evil"; color: red')).toBe('"Evil color: red"');
    expect(fontFamily("A, monospace")).toBe('"A monospace"');
    expect(fontFamily("   ")).toBeNull();
  });
});

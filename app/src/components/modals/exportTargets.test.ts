import { beforeEach, describe, expect, it } from "vitest";
import type { SnippetLanguage } from "../../bindings/SnippetLanguage";
import { EXPORT_LANGUAGES, findVariant, initialVariant, rememberVariant, searchLanguages } from "./exportTargets";

// Every language the backend writes (a type error here means one is missing from this list).
const ALL: Record<SnippetLanguage, true> = {
  kotlin: true,
  swift: true,
  javascript: true,
  javascriptAxios: true,
  python: true,
  pythonHttpx: true,
  go: true,
  java: true,
  csharp: true,
  php: true,
  ruby: true,
  rust: true,
  dart: true,
  c: true,
  powerShell: true,
  httpie: true,
  wget: true,
};

describe("export targets", () => {
  beforeEach(() => localStorage.clear());

  it("offers every language and every cURL shell once, with unique ids", () => {
    const variants = EXPORT_LANGUAGES.flatMap((l) => l.variants);
    const code = variants.flatMap((v) => (v.target.kind === "code" ? [v.target.language] : []));
    expect(code.sort()).toEqual(Object.keys(ALL).sort());
    expect(variants.filter((v) => v.target.kind === "curl")).toHaveLength(3);
    expect(new Set(variants.map((v) => v.id)).size).toBe(variants.length);
    // The PowerShell snippet and cURL for PowerShell are different targets.
    expect(findVariant("powerShell")?.variant.target).toEqual({ kind: "code", language: "powerShell" });
    expect(findVariant("curl-powershell")?.variant.target).toEqual({ kind: "curl", flavor: "powerShell" });
  });

  it("finds languages by name, library or platform", () => {
    const ids = (q: string) => searchLanguages(q).map((l) => l.id);
    expect(ids("")).toHaveLength(EXPORT_LANGUAGES.length);
    expect(ids("axios")).toEqual(["javascript"]);
    expect(ids("android")).toEqual(["kotlin"]);
    expect(ids("flutter")).toEqual(["dart"]);
    expect(ids(".net")).toEqual(["csharp"]);
    expect(ids("windows cmd")).toEqual(["curl"]);
    expect(ids("nothing like this")).toEqual([]);
  });

  it("remembers the last choice, else cURL for the computer's shell", () => {
    expect(initialVariant(false)).toBe("curl-bash");
    expect(initialVariant(true)).toBe("curl-cmd");
    rememberVariant("pythonHttpx");
    expect(initialVariant(true)).toBe("pythonHttpx");
    localStorage.setItem("zorvik.export.variant", "cobol");
    expect(initialVariant(false)).toBe("curl-bash");
  });
});

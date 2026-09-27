import { describe, expect, it } from "vitest";
import { rebase } from "./SettingsModal";

describe("rebase", () => {
  it("keeps the user's edits and takes other changes", () => {
    const base = { theme: "system", agents: { enabled: false, follow: true }, historyLimit: 500 };
    const edited = { ...base, historyLimit: 900 };
    const latest = { ...base, agents: { enabled: true, follow: true } };
    expect(rebase(base, edited, latest)).toEqual({ theme: "system", agents: { enabled: true, follow: true }, historyLimit: 900 });
  });

  it("a field changed on both sides keeps the user's value", () => {
    const base = { agents: { enabled: false, follow: true } };
    const edited = { agents: { enabled: false, follow: false } };
    const latest = { agents: { enabled: true, follow: true } };
    expect(rebase(base, edited, latest)).toEqual({ agents: { enabled: true, follow: false } });
  });
});

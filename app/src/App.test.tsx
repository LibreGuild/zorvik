// App-wide shortcuts: the command palette must not replace another dialog (and its unsaved edits).
import { cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(() => Promise.resolve(null)) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));
vi.mock("./lib/events", () => ({ onEvent: () => () => {} }));

const { useShortcuts } = await import("./App");
const { useUi } = await import("./store/ui");

/** Mod+key (⌘ on macOS, Ctrl elsewhere: both are held here). */
const press = (key: string) => window.dispatchEvent(new KeyboardEvent("keydown", { key, ctrlKey: true, metaKey: true, cancelable: true }));
const modal = () => useUi.getState().modal.type;

beforeEach(() => {
  useUi.setState({ modal: { type: "none" } });
  renderHook(() => useShortcuts(true));
});
afterEach(cleanup);

describe("command palette shortcut", () => {
  it("opens the palette", () => {
    press("k");
    expect(modal()).toBe("palette");
    useUi.setState({ modal: { type: "none" } });
    press("p");
    expect(modal()).toBe("palette");
  });

  it("leaves another open dialog alone", () => {
    for (const type of ["settings", "environments", "workspaceSettings"] as const) {
      useUi.setState({ modal: { type } });
      press("k");
      press("p");
      expect(modal()).toBe(type);
    }
  });

  it("keeps the palette open when pressed again", () => {
    press("k");
    press("k");
    press("p");
    expect(modal()).toBe("palette");
  });
});

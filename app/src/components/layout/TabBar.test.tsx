// The tab bar from the keyboard: one tab stop (the active tab), arrows move, Enter or Space opens.
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(() => Promise.resolve(null)) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));
vi.mock("../../lib/events", () => ({ onEvent: () => () => {} }));

const { TooltipProvider } = await import("../ui");
const { openDraft, resetTabs, useTabs } = await import("../../store/tabs");
const { TabBar } = await import("./TabBar");

const draft = (name: string) => ({ name, seq: 0, method: "GET", url: `http://h/${name}` });
const tab = (name: string) => screen.getByRole("tab", { name: new RegExp(name) });

beforeEach(() => {
  resetTabs();
  openDraft(draft("one"));
  openDraft(draft("two"));
  openDraft(draft("three"));
  useTabs.setState((s) => ({ activeId: s.tabs[1].id }));
  render(
    <TooltipProvider>
      <TabBar />
    </TooltipProvider>,
  );
});
afterEach(cleanup);

describe("tab bar keyboard", () => {
  it("has one tab stop, the active tab", () => {
    expect(screen.getAllByRole("tab").map((t) => t.tabIndex)).toEqual([-1, 0, -1]);
  });

  it("moves focus with the arrows and opens the focused tab with Enter or Space", () => {
    tab("two").focus();
    fireEvent.keyDown(tab("two"), { key: "ArrowRight" });
    expect(document.activeElement).toBe(tab("three"));
    fireEvent.keyDown(tab("three"), { key: "ArrowRight" }); // wraps around
    expect(document.activeElement).toBe(tab("one"));
    expect(useTabs.getState().activeId).toBe(useTabs.getState().tabs[1].id); // focus alone doesn't switch
    fireEvent.keyDown(tab("one"), { key: "Enter" });
    expect(useTabs.getState().activeId).toBe(useTabs.getState().tabs[0].id);
    expect(tab("one").tabIndex).toBe(0);
    fireEvent.keyDown(tab("one"), { key: "ArrowLeft" });
    expect(document.activeElement).toBe(tab("three"));
    fireEvent.keyDown(tab("three"), { key: " " });
    expect(useTabs.getState().activeId).toBe(useTabs.getState().tabs[2].id);
  });

  it("leaves keys on the close button to it", () => {
    const close = tab("one").querySelector("button")!;
    fireEvent.keyDown(close, { key: "Enter" });
    expect(useTabs.getState().activeId).toBe(useTabs.getState().tabs[1].id);
  });
});

// A request's Settings tab: every field is named by its label and described by its hint.
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { Request } from "../../bindings/Request";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(() => Promise.resolve(null)) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));
vi.mock("../../lib/events", () => ({ onEvent: () => () => {} }));
// The tab strip measures itself; jsdom has no layout.
vi.stubGlobal(
  "ResizeObserver",
  class {
    observe() {}
    disconnect() {}
  },
);

const { TooltipProvider } = await import("../ui");
const { RequestEditor } = await import("./RequestEditor");
type Tab = import("../../store/tabs").Tab;

const settingsOf = (draft: Request): Tab => ({
  id: "t1",
  path: null,
  draft,
  saved: null,
  response: { status: "idle" },
  stream: { status: "idle", connId: "c1", opened: null, messages: [], error: null },
  requestTab: "settings",
  responseTab: "body",
});

const show = (draft: Request) =>
  render(
    <TooltipProvider>
      <RequestEditor tab={settingsOf(draft)} />
    </TooltipProvider>,
  );

afterEach(cleanup);

describe("request settings", () => {
  it("names each field by its label", () => {
    show({ name: "r", seq: 0, method: "GET", url: "http://h/a", settings: { repeat: { condition: "", intervalMs: 1000, timeoutMs: 30000 } } });
    for (const name of ["Follow redirects", "Verify TLS certificates", "HTTP version", "Decompress responses"]) {
      expect(screen.getByRole("combobox", { name })).toBeTruthy();
    }
    for (const name of ["Timeout", "Max redirects", "Every", "Give up after"]) expect(screen.getByRole("spinbutton", { name })).toBeTruthy();
    expect(screen.getByRole("textbox", { name: "Condition" })).toBeTruthy();
    expect(screen.getByRole("switch", { name: /^Repeat in collection runs:\s*On$/ })).toBeTruthy();
  });

  it("describes a field by its hint", () => {
    show({ name: "r", seq: 0, method: "GET", url: "http://h/a" });
    const timeout = screen.getByRole("spinbutton", { name: "Timeout" });
    const hint = document.getElementById(timeout.getAttribute("aria-describedby") ?? "");
    expect(hint?.textContent).toMatch(/^Milliseconds for the whole request/);
    expect(screen.getByRole("combobox", { name: "Follow redirects" }).getAttribute("aria-describedby")).toBeNull();
  });
});

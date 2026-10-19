// An example's name, status, headers and body are all editable; Beautify indents a JSON
// body without touching its numbers.
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Example } from "../../bindings/Example";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(() => Promise.resolve(null)) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));
vi.mock("../../lib/events", () => ({ onEvent: () => () => {} }));

const { TooltipProvider } = await import("../ui");
const { ExamplesTab } = await import("./ExamplesTab");
const { useTabs } = await import("../../store/tabs");
const { useToasts } = await import("../../store/toasts");
type Tab = import("../../store/tabs").Tab;

function setExamples(examples: Example[]) {
  const tab: Tab = {
    id: "t1",
    path: null,
    draft: { name: "r", seq: 0, method: "GET", url: "http://h/a", examples },
    saved: null,
    response: { status: "idle" },
    stream: { status: "idle", connId: "c1", opened: null, messages: [], error: null },
    requestTab: "examples",
    responseTab: "body",
  } as Tab;
  useTabs.setState({ tabs: [tab], activeId: tab.id });
}

function Shown() {
  const tab = useTabs((s) => s.tabs[0]) as Tab;
  return (
    <TooltipProvider>
      <ExamplesTab tab={tab} />
    </TooltipProvider>
  );
}

const example = () => (useTabs.getState().tabs[0] as Tab).draft.examples![0];

beforeEach(() => useToasts.setState({ toasts: [] }));
afterEach(cleanup);

describe("examples", () => {
  it("keeps a status while it is typed and saves it once it is a code", () => {
    setExamples([{ name: "OK", status: 200 }]);
    render(<Shown />);
    const status = screen.getByRole("textbox", { name: "Status" });
    fireEvent.change(status, { target: { value: "4" } });
    expect((status as HTMLInputElement).value).toBe("4");
    expect(example().status).toBe(200);
    fireEvent.change(status, { target: { value: "404" } });
    expect(example().status).toBe(404);
    fireEvent.change(status, { target: { value: "700" } });
    expect(status.getAttribute("aria-invalid")).toBe("true");
    fireEvent.blur(status);
    expect((status as HTMLInputElement).value).toBe("404");
  });

  it("edits, switches off and removes headers", () => {
    setExamples([{ name: "OK", status: 200, headers: [{ key: "Content-Type", value: "application/json", enabled: true }] }]);
    render(<Shown />);
    fireEvent.click(screen.getByRole("button", { name: "Headers (1)" }));
    const values = screen.getAllByPlaceholderText("Value");
    fireEvent.change(values[0], { target: { value: "application/problem+json" } });
    expect(example().headers![0].value).toBe("application/problem+json");
    // The empty row adds one.
    fireEvent.change(screen.getAllByPlaceholderText("Header")[1], { target: { value: "ETag" } });
    expect(example().headers!.map((h) => h.key)).toEqual(["Content-Type", "ETag"]);
    fireEvent.click(screen.getAllByTitle("Disable")[1]);
    expect(example().headers![1].enabled).toBe(false);
    fireEvent.click(screen.getAllByRole("button", { name: "Remove row" })[0]);
    expect(example().headers!.map((h) => h.key)).toEqual(["ETag"]);
  });

  it("beautifies a JSON body, keeping big numbers as written", () => {
    setExamples([{ name: "OK", status: 200, headers: [{ key: "Content-Type", value: "application/json" }], body: '{"id":12345678901234567890,"n":1.50}' }]);
    render(<Shown />);
    fireEvent.click(screen.getByRole("button", { name: "Beautify" }));
    expect(example().body).toBe('{\n  "id": 12345678901234567890,\n  "n": 1.50\n}');
  });

  it("says when the body isn't valid JSON", () => {
    setExamples([{ name: "OK", status: 200, body: '{"id": ' }]);
    render(<Shown />);
    act(() => fireEvent.click(screen.getByRole("button", { name: "Beautify" })));
    expect(useToasts.getState().toasts.map((t) => t.title)).toEqual(["Body is not valid JSON"]);
    expect(example().body).toBe('{"id": ');
  });

  it("offers Beautify only for JSON", () => {
    setExamples([{ name: "OK", status: 200, headers: [{ key: "Content-Type", value: "text/plain" }], body: "pong" }]);
    render(<Shown />);
    expect(screen.queryByRole("button", { name: "Beautify" })).toBeNull();
  });
});

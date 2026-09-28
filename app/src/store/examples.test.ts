import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Request } from "../bindings/Request";
import type { SendResult } from "../bindings/SendResult";

vi.mock("../lib/events", () => ({ onEvent: () => () => {} }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("../lib/rpc", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/rpc")>();
  const done = () => vi.fn((..._args: unknown[]): Promise<unknown> => Promise.resolve(null));
  return { ...actual, api: { readRequest: done(), saveRequest: done(), responseText: done() } };
});

const { api } = await import("../lib/rpc");
const { exampleHeaders, exampleName, saveAsExample } = await import("./examples");
const { isDirty, openRequest, resetTabs, updateDraft, useTabs } = await import("./tabs");
type Tab = import("./tabs").Tab;
const mocked = api as unknown as Record<string, ReturnType<typeof vi.fn>>;

beforeEach(() => {
  resetTabs();
  vi.clearAllMocks();
});

describe("examples", () => {
  it("keeps useful headers and drops framing, dates and cookies", () => {
    const headers = [
      { name: "Content-Type", value: "application/json" },
      { name: "Content-Length", value: "12" },
      { name: "Date", value: "Mon" },
      { name: "Set-Cookie", value: "sid=secret" },
      { name: "X-Request-Id", value: "abc" },
    ];
    expect(exampleHeaders(headers)).toEqual([
      { key: "Content-Type", value: "application/json", enabled: true },
      { key: "X-Request-Id", value: "abc", enabled: true },
    ]);
  });

  it("gives each example its own name", () => {
    expect(exampleName("200 OK", [])).toBe("200 OK");
    expect(exampleName("200 OK", ["200 OK", "200 OK (2)"])).toBe("200 OK (3)");
  });
});

describe("save as example", () => {
  const request: Request = { name: "r", seq: 1, method: "GET", url: "http://h/a" };
  const result = { responseId: "resp", meta: { status: 200, statusText: "OK", headers: [] } } as unknown as SendResult;
  const tab = () => useTabs.getState().tabs[0] as Tab;

  async function openSaved() {
    mocked.readRequest.mockResolvedValueOnce(request);
    await openRequest("a.yaml");
    return tab().id;
  }

  it("saves a request with no other changes at once", async () => {
    const id = await openSaved();
    mocked.responseText.mockResolvedValueOnce({ text: "{}" });
    expect(await saveAsExample(id, result)).toBe(true);
    expect(mocked.saveRequest).toHaveBeenCalledWith("a.yaml", expect.objectContaining({ examples: [expect.objectContaining({ name: "200 OK", body: "{}" })] }));
  });

  it("does not save edits typed while the response body loads", async () => {
    const id = await openSaved();
    let answer!: (v: { text: string }) => void;
    mocked.responseText.mockReturnValueOnce(new Promise((r) => (answer = r)));
    const saving = saveAsExample(id, result);
    updateDraft(id, (r) => ({ ...r, url: "http://h/typed" }));
    answer({ text: "{}" });
    expect(await saving).toBe(false);
    expect(mocked.saveRequest).not.toHaveBeenCalled();
    expect(tab().draft.url).toBe("http://h/typed");
    expect(tab().draft.examples).toHaveLength(1);
    expect(isDirty(tab())).toBe(true);
  });
});

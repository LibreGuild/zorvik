import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Request } from "../bindings/Request";

vi.mock("../lib/events", () => ({ onEvent: () => () => {} }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("../lib/rpc", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/rpc")>();
  const done = () => vi.fn((..._args: unknown[]): Promise<unknown> => Promise.resolve(null));
  return {
    ...actual,
    api: { readRequest: done(), saveRequest: done(), send: done(), cancel: done(), wsConnect: done(), wsClose: done(), sseConnect: done(), sseClose: done(), socketConnect: done(), socketSend: done(), socketClose: done(), renderVariables: done() },
  };
});

const { api, RpcError } = await import("../lib/rpc");
type Tab = import("./tabs").Tab;
type ServerTab = import("./tabs").ServerTab;
type Server = import("../bindings/Server").Server;
const { useDialogs } = await import("./dialogs");
const { closeTab, connect, disconnect, isDirty, openDraft, openRequest, resetTabs, restoreTabs, sameRequest, saveTab, send, updateDraft, useTabs, wsSend } =
  await import("./tabs");

const mocked = api as unknown as Record<keyof typeof api, ReturnType<typeof vi.fn>>;

function deferred<T>() {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

const request = (url = "http://h/a"): Request => ({ name: "r", seq: 1, method: "GET", url });
const tabById = (id: string) => useTabs.getState().tabs.find((t) => t.id === id)! as Tab;
const settle = () => new Promise((r) => setTimeout(r, 0));

beforeEach(() => {
  resetTabs();
  localStorage.clear();
  useDialogs.setState({ current: null });
  vi.clearAllMocks();
});

describe("dirty tracking", () => {
  const base = { name: "r", seq: 1, method: "GET", url: "http://h" };
  it("ignores defaults, empty arrays and seq", () => {
    expect(sameRequest(base, { ...base, seq: 7, headers: [], kind: "http", docs: "" })).toBe(true);
    expect(sameRequest({ ...base, headers: [{ key: "a", value: "1", enabled: true }] }, { ...base, headers: [{ key: "a", value: "1" }] })).toBe(true);
  });
  it("detects real changes", () => {
    expect(sameRequest(base, { ...base, url: "http://x" })).toBe(false);
    expect(sameRequest(base, { ...base, headers: [{ key: "a", value: "1", enabled: false }] })).toBe(false);
  });
  it("sees a server's TLS switched on (off by default, unlike rows)", () => {
    const saved: Server = { name: "s", kind: "tcp", seq: 0, host: "127.0.0.1", port: 9000 };
    const tab = (draft: Server): ServerTab => ({ type: "server", id: "t", serverId: "s", saved, draft });
    expect(isDirty(tab({ ...saved, tls: { enabled: true } }))).toBe(true);
    expect(isDirty(tab({ ...saved, tls: { enabled: false } }))).toBe(false);
    const rules: Server = { ...saved, socket: { rules: [{ match: "contains", pattern: "a", reply: "b" }] } };
    expect(isDirty({ ...tab({ ...saved, socket: { rules: [{ match: "contains", pattern: "a", reply: "b", enabled: true }] } }), saved: rules })).toBe(false);
  });
});

describe("tab lifecycle", () => {
  it("opens one tab when the same request is opened twice at once", async () => {
    const read = deferred<Request>();
    mocked.readRequest.mockReturnValueOnce(read.promise).mockReturnValueOnce(read.promise);
    const a = openRequest("a.yaml");
    const b = openRequest("a.yaml");
    read.resolve(request());
    await Promise.all([a, b]);
    expect(useTabs.getState().tabs.filter((t) => (t as Tab).path === "a.yaml")).toHaveLength(1);
  });

  it("keeps edits made while saving dirty", async () => {
    mocked.readRequest.mockResolvedValueOnce(request());
    await openRequest("a.yaml");
    const id = useTabs.getState().activeId!;
    updateDraft(id, (r) => ({ ...r, url: "http://h/b" }));
    const write = deferred<null>();
    mocked.saveRequest.mockReturnValueOnce(write.promise);
    const saving = saveTab(id);
    updateDraft(id, (r) => ({ ...r, url: "http://h/c" }));
    write.resolve(null);
    await saving;
    expect(tabById(id).saved?.url).toBe("http://h/b");
    expect(isDirty(tabById(id))).toBe(true);
  });

  it("asks before closing a new draft only when something was entered", async () => {
    const blank = openDraft({ name: "New request", kind: "http", seq: 0, method: "GET", url: "" });
    expect(await closeTab(blank)).toBe(true);
    const typed = openDraft({ name: "New request", kind: "http", seq: 0, method: "GET", url: "", headers: [{ key: "a", value: "1" }] });
    const closing = closeTab(typed);
    const dialog = useDialogs.getState().current;
    expect(dialog?.kind).toBe("confirm");
    if (dialog?.kind === "confirm") dialog.resolve(false);
    expect(await closing).toBe(false);
  });
});

describe("sending", () => {
  it("sends once when two handlers fire for one key press", async () => {
    const id = openDraft(request());
    const reply = deferred<never>();
    mocked.send.mockReturnValueOnce(reply.promise);
    void send(id);
    void send(id);
    expect(mocked.send).toHaveBeenCalledTimes(1);
  });

  it("ignores a late result for a tab that was reset and restored", async () => {
    await restoreTabs("/ws"); // nothing saved yet; tabs now belong to /ws
    const id = openDraft(request());
    const reply = deferred<never>();
    mocked.send.mockReturnValueOnce(reply.promise);
    const sending = send(id);
    resetTabs(); // workspace switch
    await restoreTabs("/ws"); // switched back: same tab id
    expect(tabById(id).response.status).toBe("idle");
    reply.reject(new RpcError({ code: "network", message: "cancelled", networkKind: "cancelled" }));
    await sending;
    expect(tabById(id).response.status).toBe("idle");
  });

  it("closes a connection that opens after it was disconnected", async () => {
    const id = openDraft({ ...request("ws://h/s"), kind: "websocket" });
    const opened = deferred<unknown>();
    mocked.wsConnect.mockReturnValueOnce(opened.promise);
    const connecting = connect(id);
    void connect(id); // second press while connecting
    expect(mocked.wsConnect).toHaveBeenCalledTimes(1);
    const connId = tabById(id).stream.connId;
    await disconnect(id);
    opened.resolve({ meta: { status: 101, statusText: "" }, timing: { totalMs: 1 }, unresolved: [] });
    await connecting;
    expect(mocked.wsClose).toHaveBeenLastCalledWith(connId);
    expect(tabById(id).stream.status).toBe("closed");
  });

  it("publishes MQTT bytes (hex) to the topic", async () => {
    const id = openDraft({ ...request("mqtt://h"), kind: "mqtt", mqtt: { keepAliveSecs: 30, qos: 1, topic: "t/1" } });
    mocked.socketConnect.mockResolvedValueOnce({ opened: { protocol: "MQTT 3.1.1", remoteAddr: "h:1883", timing: { totalMs: 1 } } });
    await connect(id);
    await wsSend(id, "01 02", true);
    expect(mocked.socketSend).toHaveBeenLastCalledWith(tabById(id).stream.connId, {
      type: "publish",
      topic: "t/1",
      text: null,
      base64: "AQI=",
      qos: 1,
      retain: false,
    });
  });
});

describe("persistence per workspace", () => {
  it("keeps open tabs and drafts when switching workspaces", async () => {
    await restoreTabs("/a");
    const id = openDraft(request("http://h/draft"));
    resetTabs(); // switch away before the debounced write ran
    expect(useTabs.getState().tabs).toHaveLength(0);
    await restoreTabs("/a");
    expect(tabById(id).draft.url).toBe("http://h/draft");
  });

  it("drops a restore that finishes after the workspace was switched", async () => {
    localStorage.setItem("zv:tabs:/b", JSON.stringify({ tabs: [{ id: "t1", path: "x.yaml" }], activeId: "t1" }));
    const read = deferred<Request>();
    mocked.readRequest.mockReturnValueOnce(read.promise);
    const restoring = restoreTabs("/b");
    resetTabs();
    read.resolve(request());
    await restoring;
    await settle();
    expect(useTabs.getState().tabs).toHaveLength(0);
  });
});

import { beforeEach, describe, expect, it, vi } from "vitest";
import type { GrpcEvent } from "../bindings/GrpcEvent";
import type { GrpcInvokeResult } from "../bindings/GrpcInvokeResult";
import type { GrpcMethod } from "../bindings/GrpcMethod";
import type { GrpcService } from "../bindings/GrpcService";
import type { GrpcStatus } from "../bindings/GrpcStatus";
import type { Request } from "../bindings/Request";
import type { StreamEvent } from "../bindings/StreamEvent";

const handlers: ((e: StreamEvent) => void)[] = [];
vi.mock("../lib/events", () => ({
  onEvent: (h: (e: StreamEvent) => void) => {
    handlers.push(h);
    return () => {};
  },
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("../lib/rpc", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/rpc")>();
  const fn = () => vi.fn((..._args: unknown[]): Promise<unknown> => Promise.resolve(null));
  return {
    ...actual,
    api: { grpcDescribe: fn(), grpcInvoke: fn(), grpcStart: fn(), grpcSend: fn(), grpcEnd: fn(), grpcCancel: fn(), cancel: fn(), send: fn() },
  };
});

const { api } = await import("../lib/rpc");
const { closeTab, openDraft, resetTabs, send, useTabs } = await import("./tabs");
const grpc = await import("./grpc");
const { useWorkspace } = await import("./workspace");
const { applyEvent, applyEvents, callKind, endStream, filterServices, findMethod, grpcTone, loadServices, MAX_MESSAGES, selectMethod, sendMessage, shortMethod, sourceKey, useGrpc, workspaceRelative } =
  grpc;
const mocked = api as unknown as Record<string, ReturnType<typeof vi.fn>>;
type Tab = import("./tabs").Tab;

const flush = () => new Promise((r) => setTimeout(r, 70));
const emit = (sessionId: string, event: GrpcEvent) => handlers.forEach((h) => h({ type: "grpc", sessionId, event }));

const method = (name: string, clientStreaming = false, serverStreaming = false): GrpcMethod => ({
  path: `shop.v1.Orders/${name}`,
  name,
  clientStreaming,
  serverStreaming,
  inputType: "shop.v1.OrderRequest",
  outputType: "shop.v1.Order",
  example: '{\n  "id": ""\n}',
});

const SERVICES: GrpcService[] = [
  { name: "shop.v1.Orders", methods: [method("Get"), method("Watch", false, true), method("Upload", true, false), method("Chat", true, true)] },
  { name: "shop.v1.Users", methods: [{ ...method("Find"), path: "shop.v1.Users/Find", inputType: "shop.v1.UserQuery", outputType: "shop.v1.User" }] },
];

const OK: GrpcStatus = { code: 0, name: "OK", message: "", details: null, local: false };
const timing = { redirectMs: 0, dnsMs: 1, connectMs: 1, tlsMs: 0, ttfbMs: 2, downloadMs: 0, totalMs: 5 };

const grpcRequest = (m = ""): Request => ({ name: "g", kind: "grpc", seq: 0, method: m, url: "grpc://h:1", body: { type: "json", text: "{}" } });
const tabById = (id: string) => useTabs.getState().tabs.find((t) => t.id === id)! as Tab;

function withServices(request: Request) {
  useGrpc.setState((s) => ({ services: { ...s.services, [sourceKey(request, undefined)]: { status: "loaded", services: SERVICES, source: "reflection v1", error: null } } }));
}

beforeEach(() => {
  resetTabs();
  useGrpc.setState({ services: {}, calls: {} });
  vi.clearAllMocks();
});

describe("services", () => {
  it("keys sources by files, else URL and environment", () => {
    const r = grpcRequest();
    expect(sourceKey(r, "dev")).not.toBe(sourceKey(r, "prod"));
    const files = { ...r, grpc: { protoFiles: ["a.proto"] } };
    expect(sourceKey(files, "dev")).toBe(sourceKey({ ...files, url: "other" }, "prod"));
    expect(sourceKey({ ...r, grpc: { protoFiles: ["  "] } }, null)).toBe(sourceKey(r, null));
  });

  it("finds and filters methods", () => {
    expect(findMethod(SERVICES, "/shop.v1.Orders/Watch")?.name).toBe("Watch");
    expect(findMethod(SERVICES, "shop.v1.Orders/Nope")).toBeNull();
    expect(filterServices(SERVICES, "user").map((s) => s.name)).toEqual(["shop.v1.Users"]);
    expect(filterServices(SERVICES, "orders chat")[0].methods.map((m) => m.name)).toEqual(["Chat"]);
    expect(filterServices(SERVICES, "  ")).toBe(SERVICES);
    expect(filterServices(SERVICES, "zzz")).toEqual([]);
  });

  it("names call kinds and methods", () => {
    expect(SERVICES[0].methods.map(callKind)).toEqual(["unary", "server", "client", "bidi"]);
    expect(shortMethod("shop.v1.Orders/Get")).toBe("Orders/Get");
    expect(shortMethod("Get")).toBe("Get");
  });

  it("loads services and keeps the last list after a failed reload", async () => {
    const id = openDraft(grpcRequest());
    mocked.grpcDescribe.mockResolvedValueOnce({ services: SERVICES, source: "reflection v1" });
    const entry = await loadServices(tabById(id));
    expect(entry.status).toBe("loaded");
    expect(mocked.grpcDescribe).toHaveBeenCalledWith(tabById(id).draft, null, false);
    mocked.grpcDescribe.mockRejectedValueOnce(new Error("server gone"));
    const failed = await loadServices(tabById(id), true);
    expect(failed.status).toBe("error");
    expect(failed.error).toContain("server gone");
    expect(failed.services).toHaveLength(2);
  });

  it("services are forgotten when the workspace changes", () => {
    const request = grpcRequest();
    withServices(request);
    useWorkspace.setState({ info: { ...(useWorkspace.getState().info ?? {}), path: "/other" } as never });
    expect(useGrpc.getState().services).toEqual({});
  });

  it("picking a method fills a blank message with its example", () => {
    const id = openDraft(grpcRequest());
    selectMethod(id, SERVICES[0].methods[0]);
    expect(tabById(id).draft.method).toBe("shop.v1.Orders/Get");
    expect(tabById(id).draft.body?.text).toBe('{\n  "id": ""\n}');
    const typed = openDraft({ ...grpcRequest(), body: { type: "json", text: '{"id": "7"}' } });
    selectMethod(typed, SERVICES[0].methods[1]);
    expect(tabById(typed).draft.body?.text).toBe('{"id": "7"}');
  });
});

describe("helpers", () => {
  it("makes paths inside the workspace relative", () => {
    expect(workspaceRelative("/ws/protos/a.proto", "/ws")).toBe("protos/a.proto");
    expect(workspaceRelative("/ws/protos/a.proto", "/ws/")).toBe("protos/a.proto");
    expect(workspaceRelative("/other/a.proto", "/ws")).toBe("/other/a.proto");
    expect(workspaceRelative("/wsx/a.proto", "/ws")).toBe("/wsx/a.proto");
    expect(workspaceRelative("C:\\Work\\WS\\protos\\a.proto", "c:\\work\\ws")).toBe("protos/a.proto");
    expect(workspaceRelative("/ws/a.proto", null)).toBe("/ws/a.proto");
    expect(workspaceRelative("\\\\Server\\Share\\WS\\a.proto", "\\\\server\\share\\ws")).toBe("a.proto");
  });

  it("tones statuses", () => {
    expect(grpcTone(0)).toBe("success");
    expect(grpcTone(1)).toBe("muted");
    expect(grpcTone(5)).toBe("warning");
    expect(grpcTone(16)).toBe("warning");
    expect(grpcTone(14)).toBe("danger");
    expect(grpcTone(4)).toBe("danger");
  });

  it("applies stream events", () => {
    type Call = import("./grpc").GrpcCall;
    let call: Call = {
      id: "s",
      method: null,
      streaming: true,
      state: "open",
      startedAt: 0,
      messages: [],
      dropped: 0,
      headers: [],
      trailers: [],
      status: null,
      timing: null,
      requestHeaders: [],
      remoteAddr: null,
      errors: [],
      clientEnded: false,
      unresolved: [],
    };
    const start = call;
    call = applyEvent(call, { type: "headers", headers: [{ name: "content-type", value: "application/grpc" }], timestamp: 1 });
    call = applyEvent(call, { type: "message", message: { direction: "received", json: "{}", size: 0, timestamp: 2 } });
    call = applyEvent(call, { type: "error", message: "bad message" });
    expect(call.headers).toHaveLength(1);
    expect(call.messages).toHaveLength(1);
    expect(call.errors).toEqual(["bad message"]);
    expect(call.state).toBe("open");
    call = applyEvent(call, { type: "end", status: OK, trailers: [{ name: "grpc-status", value: "0" }], timing });
    expect(call.state).toBe("ended");
    expect(call.trailers).toHaveLength(1);

    // A flood keeps the newest messages (numbered on through `dropped`) and the newest problems.
    const message = (i: number): GrpcEvent => ({ type: "message", message: { direction: "received", json: `{"i": ${i}}`, size: 0, timestamp: i } });
    let flood = applyEvents(start, Array.from({ length: MAX_MESSAGES + 10 }, (_, i) => message(i)));
    flood = applyEvents(flood, [message(MAX_MESSAGES + 10), ...Array.from({ length: 50 }, (_, i): GrpcEvent => ({ type: "error", message: `bad ${i}` }))]);
    expect(flood.messages).toHaveLength(MAX_MESSAGES);
    expect(flood.dropped).toBe(11);
    expect(flood.messages[0].json).toBe('{"i": 11}');
    expect(flood.errors).toHaveLength(20);
    expect(flood.errors.at(-1)).toBe("bad 49");
    expect(start.messages).toHaveLength(0);
  });
});

describe("calls", () => {
  it("unary: Send invokes with the tab id (so Cancel works) and keeps the answer", async () => {
    const request = grpcRequest("shop.v1.Orders/Get");
    withServices(request);
    const id = openDraft(request);
    const result: GrpcInvokeResult = {
      status: OK,
      messages: [{ direction: "received", json: '{"id": "1"}', size: 3, timestamp: 1 }],
      headers: [],
      trailers: [],
      timing,
      remoteAddr: "127.0.0.1:1",
      tls: null,
      requestHeaders: [],
      warnings: [],
      unresolved: ["x"],
    };
    mocked.grpcInvoke.mockResolvedValueOnce(result);
    await send(id);
    expect(mocked.grpcInvoke).toHaveBeenCalledWith(id, request, null);
    expect(mocked.grpcStart).not.toHaveBeenCalled();
    const tab = tabById(id);
    expect(tab.response.status).toBe("other");
    const call = useGrpc.getState().calls[id];
    expect(call.state).toBe("ended");
    expect(call.messages[0].json).toContain('"1"');
    expect(call.unresolved).toEqual(["x"]);
  });

  it("unknown methods load the services first; no method is an error", async () => {
    const id = openDraft(grpcRequest("shop.v1.Orders/Get"));
    mocked.grpcDescribe.mockResolvedValueOnce({ services: SERVICES, source: "reflection v1" });
    mocked.grpcInvoke.mockResolvedValueOnce({ status: OK, messages: [], headers: [], trailers: [], timing, remoteAddr: null, tls: null, requestHeaders: [], warnings: [], unresolved: [] });
    await send(id);
    expect(mocked.grpcDescribe).toHaveBeenCalledTimes(1);
    expect(mocked.grpcInvoke).toHaveBeenCalledTimes(1);

    const none = openDraft(grpcRequest(""));
    await send(none);
    const r = tabById(none).response;
    expect(r.status).toBe("error");
    expect(r.status === "error" && r.message).toContain("Pick a method");
  });

  it("streams: Send starts a session, stays loading until the end event, ignores other sessions", async () => {
    const request = grpcRequest("shop.v1.Orders/Chat");
    withServices(request);
    const id = openDraft(request);
    mocked.grpcStart.mockResolvedValueOnce({ opened: { remoteAddr: "127.0.0.1:1", tls: null, timing, requestHeaders: [] }, method: method("Chat", true, true), unresolved: [] });
    const sending = send(id);
    await flush();
    const [sessionId, requestId] = mocked.grpcStart.mock.calls[0] as [string, string];
    expect(requestId).toBe(id);
    expect(sessionId).not.toBe(id);
    expect(tabById(id).response.status).toBe("loading");
    expect(useGrpc.getState().calls[id].state).toBe("open");

    await sendMessage(id, '{"id": "a"}');
    expect(mocked.grpcSend).toHaveBeenCalledWith(sessionId, '{"id": "a"}');
    emit(sessionId, { type: "message", message: { direction: "sent", json: '{"id": "a"}', size: 3, timestamp: Date.now() } });
    emit(sessionId, { type: "message", message: { direction: "received", json: '{"id": "a"}', size: 3, timestamp: Date.now() } });
    // A late event of an earlier call is ignored.
    emit("old-session", { type: "end", status: { ...OK, code: 1, name: "CANCELLED" }, trailers: [], timing });
    await flush();
    expect(useGrpc.getState().calls[id].messages).toHaveLength(2);
    expect(tabById(id).response.status).toBe("loading");

    await endStream(id);
    expect(mocked.grpcEnd).toHaveBeenCalledWith(sessionId);
    expect(useGrpc.getState().calls[id].clientEnded).toBe(true);
    await sendMessage(id, "{}");
    expect(mocked.grpcSend).toHaveBeenCalledTimes(1);

    emit(sessionId, { type: "end", status: OK, trailers: [], timing });
    await sending;
    expect(tabById(id).response.status).toBe("other");
    expect(useGrpc.getState().calls[id].status?.name).toBe("OK");
  });

  it("a stream that fails to start is an error; closing a tab cancels its stream", async () => {
    const request = grpcRequest("shop.v1.Orders/Watch");
    withServices(request);
    const id = openDraft(request);
    mocked.grpcStart.mockRejectedValueOnce(new Error("connection refused"));
    await send(id);
    expect(tabById(id).response.status).toBe("error");

    mocked.grpcStart.mockResolvedValueOnce({ opened: { remoteAddr: null, tls: null, timing, requestHeaders: [] }, method: method("Watch", false, true), unresolved: [] });
    const sending = send(id);
    await flush();
    const sessionId = mocked.grpcStart.mock.calls[1][0] as string;
    await closeTab(id, true);
    expect(mocked.grpcCancel).toHaveBeenCalledWith(sessionId);
    expect(useGrpc.getState().calls[id]).toBeUndefined();
    // Its Send settles instead of waiting for an end event that finds no call.
    await expect(Promise.race([sending.then(() => "settled"), flush().then(() => "pending")])).resolves.toBe("settled");
  });

  it("a tab replaced without the count going down still cancels its stream", async () => {
    const request = grpcRequest("shop.v1.Orders/Watch");
    withServices(request);
    const id = openDraft(request);
    mocked.grpcStart.mockResolvedValueOnce({ opened: { remoteAddr: null, tls: null, timing, requestHeaders: [] }, method: method("Watch", false, true), unresolved: [] });
    void send(id);
    await flush();
    const sessionId = mocked.grpcStart.mock.calls[0][0] as string;
    const count = useTabs.getState().tabs.length;
    useTabs.setState((s) => ({ tabs: s.tabs.map((t) => (t.id === id ? { ...t, id: "restored" } : t)) }));
    expect(useTabs.getState().tabs).toHaveLength(count);
    expect(mocked.grpcCancel).toHaveBeenCalledWith(sessionId);
    expect(useGrpc.getState().calls[id]).toBeUndefined();
  });
});

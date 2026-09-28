// The URL bar's Cancel: a gRPC stream is cancelled on its session too, not only the pending Send.
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Request } from "../../bindings/Request";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(() => Promise.resolve(null)) }));
vi.mock("../../lib/events", () => ({ onEvent: () => () => {} }));
vi.mock("../../lib/rpc", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../lib/rpc")>();
  const fn = () => vi.fn((..._args: unknown[]): Promise<unknown> => Promise.resolve(null));
  return { ...actual, api: { cancel: fn(), grpcCancel: fn(), grpcDescribe: vi.fn(() => Promise.resolve({ services: [], source: null })) } };
});

const { api } = await import("../../lib/rpc");
const { TooltipProvider } = await import("../ui");
const { useGrpc } = await import("../../store/grpc");
const { UrlBar } = await import("./UrlBar");
type Tab = import("../../store/tabs").Tab;
type GrpcCall = import("../../store/grpc").GrpcCall;
const mocked = api as unknown as Record<string, ReturnType<typeof vi.fn>>;

const loadingTab = (draft: Request): Tab => ({
  id: "t1",
  path: null,
  draft,
  saved: null,
  response: { status: "loading", startedAt: 0 },
  stream: { status: "idle", connId: "c1", opened: null, messages: [], error: null },
  requestTab: "params",
  responseTab: "body",
});

const call: GrpcCall = {
  id: "s1",
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

const clickCancel = (tab: Tab) => {
  render(
    <TooltipProvider>
      <UrlBar tab={tab} />
    </TooltipProvider>,
  );
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
};

beforeEach(() => {
  vi.clearAllMocks();
  useGrpc.setState({ calls: { t1: call } });
});
afterEach(cleanup);

describe("Cancel in the URL bar", () => {
  it("cancels a gRPC stream on its session", async () => {
    clickCancel(loadingTab({ name: "g", kind: "grpc", seq: 0, method: "shop.v1.Orders/Watch", url: "grpc://h:1", body: { type: "json", text: "{}" } }));
    await vi.waitFor(() => expect(mocked.grpcCancel).toHaveBeenCalledWith("s1"));
    expect(mocked.cancel).toHaveBeenCalledWith("t1");
  });

  it("cancels an HTTP request", async () => {
    clickCancel(loadingTab({ name: "r", seq: 0, method: "GET", url: "http://h/a" }));
    await vi.waitFor(() => expect(mocked.cancel).toHaveBeenCalledWith("t1"));
    expect(mocked.grpcCancel).not.toHaveBeenCalled();
  });
});

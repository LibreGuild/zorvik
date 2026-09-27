// Transport to the Rust API: Tauri IPC in the desktop app, the dev bridge
// (HTTP) when running in a browser. Both call the same `zorvik_api::Api::call`.
import { invoke } from "@tauri-apps/api/core";
import type { ApiError } from "../bindings/ApiError";
import type { ActiveLoadRun } from "../bindings/ActiveLoadRun";
import type { AppInfo } from "../bindings/AppInfo";
import type { AutoStartResult } from "../bindings/AutoStartResult";
import type { Auth } from "../bindings/Auth";
import type { CookieInfo } from "../bindings/CookieInfo";
import type { CurlFlavor } from "../bindings/CurlFlavor";
import type { CurlImportResult } from "../bindings/CurlImportResult";
import type { DataPreview } from "../bindings/DataPreview";
import type { DnsQueryResult } from "../bindings/DnsQueryResult";
import type { Environment } from "../bindings/Environment";
import type { EnvironmentEntry } from "../bindings/EnvironmentEntry";
import type { FolderMeta } from "../bindings/FolderMeta";
import type { GraphqlSchema } from "../bindings/GraphqlSchema";
import type { GrpcDescribeResult } from "../bindings/GrpcDescribeResult";
import type { GrpcInvokeResult } from "../bindings/GrpcInvokeResult";
import type { GrpcStartResult } from "../bindings/GrpcStartResult";
import type { HistoryEntry } from "../bindings/HistoryEntry";
import type { ImportSummary } from "../bindings/ImportSummary";
import type { LoadRunRecord } from "../bindings/LoadRunRecord";
import type { LocalValue } from "../bindings/LocalValue";
import type { LoadStarted } from "../bindings/LoadStarted";
import type { LoadTest } from "../bindings/LoadTest";
import type { LoadTestNode } from "../bindings/LoadTestNode";
import type { MockCreated } from "../bindings/MockCreated";
import type { MockRoute } from "../bindings/MockRoute";
import type { NetInterface } from "../bindings/NetInterface";
import type { OutgoingMessage } from "../bindings/OutgoingMessage";
import type { PingMode } from "../bindings/PingMode";
import type { PingStarted } from "../bindings/PingStarted";
import type { PortCheckStarted } from "../bindings/PortCheckStarted";
import type { RecentWorkspace } from "../bindings/RecentWorkspace";
import type { Request } from "../bindings/Request";
import type { RunningServerInfo } from "../bindings/RunningServerInfo";
import type { RunStarted } from "../bindings/RunStarted";
import type { SendResult } from "../bindings/SendResult";
import type { Server } from "../bindings/Server";
import type { ServerNode } from "../bindings/ServerNode";
import type { Settings } from "../bindings/Settings";
import type { Summary } from "../bindings/Summary";
import type { SocketConnectResult } from "../bindings/SocketConnectResult";
import type { SocketOutgoing } from "../bindings/SocketOutgoing";
import type { StreamOpened } from "../bindings/StreamOpened";
import type { TlsReport } from "../bindings/TlsReport";
import type { TokenStatus } from "../bindings/TokenStatus";
import type { TrafficEntry } from "../bindings/TrafficEntry";
import type { TreeNode } from "../bindings/TreeNode";
import type { VariableInfo } from "../bindings/VariableInfo";
import type { WorkspaceInfo } from "../bindings/WorkspaceInfo";
import type { WorkspaceMeta } from "../bindings/WorkspaceMeta";
import type { WsOutgoing } from "../bindings/WsOutgoing";

export const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export class RpcError extends Error {
  code: string;
  networkKind: ApiError["networkKind"];
  constructor(e: ApiError) {
    super(e.message);
    this.code = e.code;
    this.networkKind = e.networkKind;
  }
}

function toRpcError(e: unknown): RpcError {
  if (e instanceof RpcError) return e;
  if (e && typeof e === "object" && "message" in e && "code" in e) return new RpcError(e as ApiError);
  return new RpcError({ code: "internal", message: String((e as Error)?.message ?? e), networkKind: null });
}

async function call<T>(method: string, params: unknown = {}): Promise<T> {
  try {
    if (isTauri) return await invoke<T>("rpc", { method, params });
    const res = await fetch("/bridge/rpc", {
      method: "POST",
      headers: { "content-type": "application/json", "x-zorvik-bridge": "1" },
      body: JSON.stringify({ method, params }),
    });
    if (!res.ok) throw new RpcError({ code: "internal", message: `Bridge error ${res.status}`, networkKind: null });
    const data = (await res.json()) as { result?: T; error?: ApiError };
    if (data.error) throw new RpcError(data.error);
    return data.result as T;
  } catch (e) {
    throw toRpcError(e);
  }
}

export const api = {
  appInfo: () => call<AppInfo>("app.info"),
  getSettings: () => call<Settings>("settings.get"),
  saveSettings: (settings: Settings) => call<null>("settings.save", { settings }),

  recentWorkspaces: () => call<RecentWorkspace[]>("workspace.recent"),
  removeRecent: (path: string) => call<null>("workspace.removeRecent", { path }),
  createWorkspace: (path: string, name: string) => call<WorkspaceInfo>("workspace.create", { path, name }),
  openWorkspace: (path: string) => call<WorkspaceInfo>("workspace.open", { path }),
  currentWorkspace: () => call<WorkspaceInfo | null>("workspace.current"),
  reloadWorkspace: () => call<WorkspaceInfo>("workspace.reload"),
  closeWorkspace: () => call<null>("workspace.close"),
  tree: () => call<TreeNode[]>("workspace.tree"),
  saveWorkspaceMeta: (meta: WorkspaceMeta) => call<WorkspaceMeta>("workspace.saveMeta", { meta }),

  readRequest: (path: string) => call<Request>("request.read", { path }),
  saveRequest: (path: string, request: Request) => call<null>("request.save", { path, request }),
  createRequest: (parent: string, request: Request) => call<string>("request.create", { parent, request }),
  createFolder: (parent: string, name: string) => call<string>("folder.create", { parent, name }),
  readFolder: (path: string) => call<FolderMeta>("folder.read", { path }),
  saveFolder: (path: string, meta: FolderMeta) => call<null>("folder.save", { path, meta }),
  renameItem: (path: string, name: string) => call<string>("item.rename", { path, name }),
  deleteItem: (path: string) => call<null>("item.delete", { path }),
  duplicateItem: (path: string) => call<string>("item.duplicate", { path }),
  moveItem: (path: string, parent: string, index: number | null) => call<string>("item.move", { path, parent, index }),

  listEnvironments: () => call<EnvironmentEntry[]>("env.list"),
  createEnvironment: (environment: Environment) => call<string>("env.create", { environment }),
  saveEnvironment: (id: string, environment: Environment) => call<string>("env.save", { id, environment }),
  deleteEnvironment: (id: string) => call<null>("env.delete", { id }),
  setActiveEnvironment: (id: string | null) => call<null>("env.setActive", { id }),
  variables: () => call<VariableInfo[]>("vars.list"),
  renderVariables: (text: string) => call<string>("vars.render", { text }),
  /** Values scripts set (environments of the open workspace, the workspace, globals). */
  localValues: () => call<LocalValue[]>("vars.local"),
  /** Forget one script-set value, or all of a scope when `key` is null. */
  clearLocalValues: (scope: "environment" | "workspace" | "globals", environmentId: string | null, key: string | null) =>
    call<null>("vars.clearLocal", { scope, environmentId, key }),

  send: (requestId: string, request: Request, path: string | null, opts?: { standalone?: boolean }) =>
    call<SendResult>("http.send", { requestId, request, path, standalone: opts?.standalone ?? false }),
  cancel: (requestId: string) => call<null>("http.cancel", { requestId }),
  saveResponse: (responseId: string, path: string) => call<null>("response.save", { responseId, path }),

  wsConnect: (connId: string, request: Request, path: string | null) =>
    call<StreamOpened>("ws.connect", { connId, request, path }),
  wsSend: (connId: string, message: WsOutgoing) => call<null>("ws.send", { connId, message }),
  wsClose: (connId: string) => call<null>("ws.close", { connId, code: 1000, reason: "" }),
  sseConnect: (connId: string, request: Request, path: string | null) =>
    call<StreamOpened>("sse.connect", { connId, request, path }),
  sseClose: (connId: string) => call<null>("sse.close", { connId }),

  socketConnect: (connId: string, request: Request, path: string | null) =>
    call<SocketConnectResult>("socket.connect", { connId, request, path }),
  socketSend: (connId: string, message: SocketOutgoing) => call<null>("socket.send", { connId, message }),
  socketClose: (connId: string) => call<null>("socket.close", { connId }),

  /** DNS query of a request (name = url, type = method). `requestId` lets `cancel` stop it. */
  dnsQuery: (requestId: string | null, request: Request, path: string | null) =>
    call<DnsQueryResult>("dns.query", { requestId, request, path }),
  /** Introspect a GraphQL request's schema with its URL, headers and auth (cached until `refresh`). */
  graphqlSchema: (request: Request, path: string | null, refresh = false) =>
    call<GraphqlSchema>("graphql.schema", { request, path, refresh }),

  /** gRPC services from the request's .proto files or server reflection (cached until `refresh`). */
  grpcDescribe: (request: Request, path: string | null, refresh = false) =>
    call<GrpcDescribeResult>("grpc.describe", { request, path, refresh }),
  /** Unary gRPC call; `cancel(requestId)` stops it. */
  grpcInvoke: (requestId: string, request: Request, path: string | null) =>
    call<GrpcInvokeResult>("grpc.invoke", { requestId, request, path }),
  /** Open a streaming call; events `grpc {sessionId}`. `cancel(requestId)` stops it too. */
  grpcStart: (sessionId: string, requestId: string, request: Request, path: string | null) =>
    call<GrpcStartResult>("grpc.start", { sessionId, requestId, request, path }),
  grpcSend: (sessionId: string, message: string) => call<null>("grpc.send", { sessionId, message }),
  grpcEnd: (sessionId: string) => call<null>("grpc.end", { sessionId }),
  grpcCancel: (sessionId: string) => call<null>("grpc.cancel", { sessionId }),

  servers: () => call<ServerNode[]>("server.list"),
  readServer: (id: string) => call<Server>("server.read", { id }),
  createServer: (server: Server) => call<string>("server.create", { server }),
  saveServer: (id: string, server: Server) => call<string>("server.save", { id, server }),
  duplicateServer: (id: string) => call<string>("server.duplicate", { id }),
  deleteServer: (id: string) => call<null>("server.delete", { id }),
  reorderServers: (ids: string[]) => call<null>("server.reorder", { ids }),
  startServer: (id: string, server: Server) => call<RunningServerInfo>("server.start", { id, server }),
  autoStartServers: () => call<AutoStartResult>("server.autoStart"),
  stopServer: (runId: string) => call<null>("server.stop", { runId }),
  stopAllServers: () => call<null>("server.stopAll"),
  runningServers: () => call<RunningServerInfo[]>("server.running"),
  updateServer: (runId: string, server: Server) => call<{ applied: boolean }>("server.update", { runId, server }),
  serverSend: (runId: string, conn: number | null, message: OutgoingMessage) => call<number>("server.send", { runId, conn, message }),
  serverDisconnect: (runId: string, conn: number) => call<null>("server.disconnect", { runId, conn }),
  serverLog: (runId: string) => call<TrafficEntry[]>("server.log", { runId }),
  clearServerLog: (runId: string) => call<null>("server.clearLog", { runId }),

  /** New mock API from the HTTP requests in a folder ("" = the whole collection). */
  mockFromFolder: (folder: string, name: string) => call<MockCreated>("mock.fromFolder", { folder, name }),
  /** New mock API from an OpenAPI/Swagger document: pasted text, a file path or a URL. */
  mockFromOpenApi: (source: { text: string } | { path: string } | { url: string }, name: string) =>
    call<MockCreated>("mock.fromOpenApi", { ...source, name }),
  /** Append a route to a saved mock API (a running copy picks it up); returns the server id. */
  mockAddRoute: (serverId: string, route: MockRoute) => call<string>("mock.addRoute", { serverId, route }),

  loadTests: () => call<LoadTestNode[]>("load.list"),
  readLoadTest: (id: string) => call<LoadTest>("load.read", { id }),
  createLoadTest: (test: LoadTest) => call<string>("load.create", { test }),
  saveLoadTest: (id: string, test: LoadTest) => call<string>("load.save", { id, test }),
  duplicateLoadTest: (id: string) => call<string>("load.duplicate", { id }),
  deleteLoadTest: (id: string) => call<null>("load.delete", { id }),
  reorderLoadTests: (ids: string[]) => call<null>("load.reorder", { ids }),
  /** Fails with code `confirmTarget` when the plan hits hosts outside this computer/network, unless `confirmed`. */
  startLoadTest: (id: string, test: LoadTest, confirmed = false) => call<LoadStarted>("load.start", { id, test, confirmed }),
  stopLoadTest: (runId: string) => call<null>("load.stop", { runId }),
  activeLoadTest: () => call<ActiveLoadRun | null>("load.active"),
  loadRuns: (id: string) => call<LoadRunRecord[]>("load.runs", { id }),
  loadRun: (id: string, runId: string) => call<Summary>("load.run", { id, runId }),
  deleteLoadRun: (id: string, runId: string) => call<null>("load.deleteRun", { id, runId }),
  exportLoadRun: (id: string, runId: string, path: string, format: "json" | "html") =>
    call<null>("load.export", { id, runId, path, format }),

  /** Run requests of a folder ("" = the collection) or `requests` in that order; events `runner {runId}`. */
  startRun: (params: RunnerStartParams) => call<RunStarted>("runner.start", params),
  stopRun: (runId: string) => call<null>("runner.stop", { runId }),
  /** The first rows of a data file (relative to the workspace or absolute) and its row count. */
  previewRunData: (dataFile: string) => call<DataPreview>("runner.preview", { dataFile }),
  /** Save one of the last finished runs as a JSON report or JUnit XML. */
  exportRun: (runId: string, path: string, format: "json" | "junit") => call<null>("runner.export", { runId, path, format }),

  /** Generic call for feature modules (DNS, tools) that own their method names. */
  call: <T>(method: string, params: unknown = {}) => call<T>(method, params),

  // Network tools. Port check and ping stream "tool" events tagged with `runId`.
  tlsInspect: (host: string, sni: string | null, runId: string) => call<TlsReport>("tools.tlsInspect", { host, sni, runId }),
  portCheck: (runId: string, host: string, ports: string, timeoutMs: number) =>
    call<PortCheckStarted>("tools.portCheck", { runId, host, ports, timeoutMs }),
  ping: (runId: string, host: string, count: number, intervalMs: number, mode: PingMode, port: number) =>
    call<PingStarted>("tools.ping", { runId, host, count, intervalMs, mode, port }),
  netInterfaces: () => call<NetInterface[]>("tools.interfaces"),
  cancelTool: (runId: string) => call<null>("tools.cancel", { runId }),

  oauthGetToken: (auth: Auth, path: string | null) => call<TokenStatus>("oauth2.getToken", { auth, path }),
  oauthStatus: (auth: Auth, path: string | null) => call<TokenStatus>("oauth2.status", { auth, path }),
  oauthClear: (auth: Auth, path: string | null) => call<null>("oauth2.clear", { auth, path }),

  history: (search: string, limit = 300, offset = 0) => call<HistoryEntry[]>("history.list", { search, limit, offset }),
  deleteHistory: (id: number) => call<null>("history.delete", { id }),
  clearHistory: () => call<null>("history.clear"),

  cookies: () => call<CookieInfo[]>("cookies.list"),
  deleteCookie: (c: Pick<CookieInfo, "domain" | "path" | "name">) =>
    call<null>("cookies.delete", { domain: c.domain, path: c.path, name: c.name }),
  clearCookies: () => call<null>("cookies.clear"),

  importCurl: (text: string) => call<CurlImportResult>("import.curl", { text }),
  importFile: (path: string, parent: string) => call<ImportSummary>("import.file", { path, parent }),
  importText: (text: string, parent: string) => call<ImportSummary>("import.file", { text, parent }),
  importUrl: (url: string, parent: string) => call<ImportSummary>("import.url", { url, parent }),
  exportCurl: (request: Request, path: string | null, flavor: CurlFlavor, resolveVariables: boolean) =>
    call<string>("export.curl", { request, path, flavor, resolveVariables }),
};

export interface RunnerStartParams {
  folder: string;
  requests?: string[];
  /** Default: one per data row, or 1. */
  iterations?: number;
  delayMs?: number;
  dataFile?: string;
  stopOnFailure?: boolean;
}

export function errorMessage(e: unknown): string {
  return toRpcError(e).message;
}

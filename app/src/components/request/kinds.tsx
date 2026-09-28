// Per-kind parts of the request workbench for kinds that don't use the HTTP
// response pane or the stream log (e.g. DNS), and extra editor tabs per kind.
import type { ComponentType } from "react";
import type { RequestKind } from "../../bindings/RequestKind";
import type { Tab } from "../../store/tabs";
import { DnsOptions } from "./DnsOptions";
import { DnsRecordPrefix } from "./DnsRecordPicker";
// Also registers the DNS one-shot sender (dns.query).
import { DnsResultPane } from "./DnsResultPane";
import { GrpcMessageTab, GrpcMetadataTab, GrpcProtoFilesTab } from "./GrpcEditors";
import { GrpcMethodPrefix } from "./GrpcMethodPicker";
// Its store registers the gRPC sender (grpc.invoke / grpc.start).
import { GrpcResultPane } from "./GrpcResultPane";
// Its store registers the trust question for MCP programs.
import { McpCallTab, McpConnectionTab, McpResultPane } from "./Mcp";
import { MqttOptions } from "./MqttOptions";
import { MqttSubscriptions } from "./MqttSubscriptions";
import { MqttTopicField } from "./MqttTopicField";
import { SocketIoEventField, SocketIoOptions } from "./SocketIo";
import { SocketOptions } from "./SocketOptions";

export interface KindPaneProps {
  tab: Tab;
}

/** Result pane replacing the HTTP response pane / stream log for a kind. */
export const RESULT_PANES: Partial<Record<RequestKind, ComponentType<KindPaneProps>>> = {
  dns: DnsResultPane,
  grpc: GrpcResultPane,
  mcp: McpResultPane,
};

/** Editor tabs of a kind: shown instead of Params/Headers/Body/Auth. */
export interface KindEditorTab {
  id: string;
  label: string;
  View: ComponentType<KindPaneProps>;
}

export const KIND_EDITOR_TABS: Partial<Record<RequestKind, KindEditorTab[]>> = {
  tcp: [{ id: "options", label: "Options", View: SocketOptions }],
  udp: [{ id: "options", label: "Options", View: SocketOptions }],
  dns: [{ id: "resolver", label: "Resolver", View: DnsOptions }],
  mqtt: [
    { id: "connection", label: "Connection", View: MqttOptions },
    { id: "subscriptions", label: "Subscriptions", View: MqttSubscriptions },
  ],
  grpc: [
    { id: "message", label: "Message", View: GrpcMessageTab },
    { id: "metadata", label: "Metadata", View: GrpcMetadataTab },
    { id: "protos", label: "Proto files", View: GrpcProtoFilesTab },
  ],
  // Shown before the HTTP-like tabs (Params, Headers, Auth, …).
  socketio: [{ id: "connection", label: "Connection", View: SocketIoOptions }],
  // Followed by Headers, Auth, Settings, Scripts and Docs.
  mcp: [
    { id: "call", label: "Call", View: McpCallTab },
    { id: "connection", label: "Connection", View: McpConnectionTab },
  ],
};

/** Kinds whose editor tabs come before the usual Params, Headers, Auth and Settings. */
export const WITH_HTTP_TABS: RequestKind[] = ["socketio"];

/** Replaces the method picker left of the URL (e.g. the DNS record type). */
export const URL_PREFIXES: Partial<Record<RequestKind, ComponentType<KindPaneProps>>> = {
  dns: DnsRecordPrefix,
  grpc: GrpcMethodPrefix,
};

/** Extra controls in the message composer (e.g. the MQTT topic). */
export const COMPOSER_EXTRAS: Partial<Record<RequestKind, ComponentType<KindPaneProps>>> = {
  mqtt: MqttTopicField,
  socketio: SocketIoEventField,
};

/** Placeholder text of the URL field per kind. */
export const URL_PLACEHOLDERS: Partial<Record<RequestKind, string>> = {
  websocket: "ws://localhost:8080/socket",
  sse: "https://example.com/events",
  tcp: "tcp://localhost:9000",
  udp: "udp://localhost:9001",
  dns: "example.com",
  mqtt: "mqtt://localhost:1883",
  grpc: "grpc://localhost:50051",
  socketio: "http://localhost:3000/chat",
  mcp: "http://localhost:3000/mcp  or  npx -y @modelcontextprotocol/server-everything",
};

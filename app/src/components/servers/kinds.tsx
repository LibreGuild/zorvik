// Server kinds: how each is labelled and which editor configures it.
import type { ComponentType, ReactNode } from "react";
import { ArrowLeftRight, Globe, Network, Radio, Router, Server as ServerIcon, Zap } from "lucide-react";
import type { RunningServerInfo } from "../../bindings/RunningServerInfo";
import type { Server } from "../../bindings/Server";
import type { ServerKind } from "../../bindings/ServerKind";
import { DnsServerEditor } from "./DnsServerEditor";
import { HttpMockEditor } from "./HttpMockEditor";
import { RelayEditor } from "./RelayEditor";
import { SocketServerEditor } from "./SocketServerEditor";
import { SseServerEditor } from "./SseServerEditor";
import { WsServerEditor } from "./WsServerEditor";

export interface ServerEditorProps {
  server: Server;
  onChange: (fn: (s: Server) => Server) => void;
  /** Set while the server runs. */
  running?: RunningServerInfo;
  /** Open server tab id (for tab-local UI state). */
  tabId: string;
}

export interface ServerKindInfo {
  label: string;
  /** Short badge in lists and tabs. */
  short: string;
  color: string;
  icon: (size: number) => ReactNode;
  description: string;
  Editor: ComponentType<ServerEditorProps>;
  /** Clients connect over a lasting connection the UI can send to. */
  connections: boolean;
}

export const SERVER_KINDS: Record<ServerKind, ServerKindInfo> = {
  http: {
    label: "Mock API (HTTP)",
    short: "HTTP",
    color: "var(--m-get)",
    icon: (size) => <Globe size={size} />,
    description: "Routes with canned responses, delays and faults. Build it from a collection or an OpenAPI spec.",
    Editor: HttpMockEditor,
    connections: false,
  },
  websocket: {
    label: "WebSocket server",
    short: "WS",
    color: "var(--m-ws)",
    icon: (size) => <Zap size={size} />,
    description: "Echo, reply by rules, or send messages to connected clients yourself.",
    Editor: WsServerEditor,
    connections: true,
  },
  sse: {
    label: "Event stream (SSE) server",
    short: "SSE",
    color: "var(--m-sse)",
    icon: (size) => <Radio size={size} />,
    description: "Streams a list of events to each client, once or on repeat, plus events you send.",
    Editor: SseServerEditor,
    connections: true,
  },
  tcp: {
    label: "TCP server",
    short: "TCP",
    color: "var(--m-tcp)",
    icon: (size) => <ServerIcon size={size} />,
    description: "Raw TCP listener: echo, rules, manual replies. Lines, length prefixes or raw bytes.",
    Editor: SocketServerEditor,
    connections: true,
  },
  udp: {
    label: "UDP server",
    short: "UDP",
    color: "var(--m-udp)",
    icon: (size) => <Network size={size} />,
    description: "Receives datagrams and answers each sender: echo, rules or manual.",
    Editor: SocketServerEditor,
    connections: true,
  },
  dns: {
    label: "DNS server",
    short: "DNS",
    color: "var(--m-dns)",
    icon: (size) => <Router size={size} />,
    description: "Answers with your records (A, AAAA, CNAME, TXT, MX, …) and forwards the rest.",
    Editor: DnsServerEditor,
    connections: false,
  },
  tcpProxy: {
    label: "TCP relay",
    short: "RELAY",
    color: "var(--m-options)",
    icon: (size) => <ArrowLeftRight size={size} />,
    description: "Sits between a client and a TCP server and shows the traffic in both directions.",
    Editor: RelayEditor,
    connections: true,
  },
};

export const SERVER_KIND_ORDER: ServerKind[] = ["http", "websocket", "sse", "tcp", "udp", "dns", "tcpProxy"];

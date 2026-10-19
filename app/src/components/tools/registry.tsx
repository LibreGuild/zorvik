// Tools: each opens in its own tab.
import type { ComponentType, ReactNode } from "react";
import { Binary, BookOpen, Cable, FileBadge, Gauge, GitCompareArrows, Globe, ListFilter, Lock, Network, Regex, Search } from "lucide-react";
import { DocsView } from "../docs/DocsView";
import type { ToolTab } from "../../store/tabs";
import { CertDecoderTool } from "./CertDecoderTool";
import { DnsLookupTool } from "./DnsLookupTool";
import { EncodersTool } from "./EncodersTool";
import { FilterPlaygroundTool } from "./FilterPlaygroundTool";
import { Http3CheckTool } from "./Http3CheckTool";
import { InterfacesTool } from "./InterfacesTool";
import { JsonCompareTool } from "./JsonCompareTool";
import { PingTool } from "./PingTool";
import { PortCheckTool } from "./PortCheckTool";
import { RegexTesterTool } from "./RegexTesterTool";
import { TlsInspectorTool } from "./TlsInspectorTool";

export interface ToolProps {
  tab: ToolTab;
}

export interface ToolInfo {
  id: string;
  label: string;
  description: string;
  icon: (size: number) => ReactNode;
  View: ComponentType<ToolProps>;
  /** Draws its own header (no tool header above it). */
  bare?: boolean;
}

export const TOOLS: ToolInfo[] = [
  { id: "tls", label: "TLS inspector", description: "Certificate chain, expiry, protocols and ciphers a server accepts.", icon: (s) => <Lock size={s} />, View: TlsInspectorTool },
  { id: "dns", label: "DNS lookup", description: "Look up any record type with the system resolver or a server of your choice.", icon: (s) => <Search size={s} />, View: DnsLookupTool },
  { id: "ports", label: "Port check", description: "Which TCP ports on a host accept connections, and how fast.", icon: (s) => <Cable size={s} />, View: PortCheckTool },
  { id: "ping", label: "Ping", description: "Round-trip times to a host (ICMP, or TCP when ICMP is blocked).", icon: (s) => <Gauge size={s} />, View: PingTool },
  { id: "interfaces", label: "Network interfaces", description: "This computer's addresses, to reach its servers from other devices.", icon: (s) => <Network size={s} />, View: InterfacesTool },
  { id: "encoders", label: "Encoders", description: "Base64, URL, hex, JWT decoding, hashes and timestamps.", icon: (s) => <Binary size={s} />, View: EncodersTool },
  { id: "http3", label: "HTTP/3 check", description: "Whether a site speaks HTTP/3 (QUIC).", icon: (s) => <Globe size={s} />, View: Http3CheckTool },
  { id: "certs", label: "Certificate decoder", description: "What a PEM certificate, chain, CSR or key holds, and which certificate a key belongs to.", icon: (s) => <FileBadge size={s} />, View: CertDecoderTool },
  { id: "jsonCompare", label: "JSON compare", description: "What was added, removed or changed between two JSON documents or responses.", icon: (s) => <GitCompareArrows size={s} />, View: JsonCompareTool },
  { id: "filter", label: "Filter playground", description: "Try JSONPath, jq or XPath on JSON or XML as you type.", icon: (s) => <ListFilter size={s} />, View: FilterPlaygroundTool },
  { id: "regex", label: "Regex tester", description: "Matches and groups of a pattern, with the syntax rules and captures use.", icon: (s) => <Regex size={s} />, View: RegexTesterTool },
];

/** The in-app docs open in a tab like a tool, but aren't listed with the tools. */
export const DOCS_TOOL: ToolInfo = {
  id: "docs",
  label: "Docs",
  description: "Everything Zorvik can do.",
  icon: (s) => <BookOpen size={s} />,
  View: DocsView,
  bare: true,
};

export const toolInfo = (id: string) => TOOLS.find((t) => t.id === id) ?? (id === DOCS_TOOL.id ? DOCS_TOOL : undefined);

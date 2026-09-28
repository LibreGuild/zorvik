import type { RequestKind } from "../bindings/RequestKind";

export const METHODS = ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] as const;

/** Label and color of the non-HTTP request kinds (HTTP shows its method). */
export const KIND_BADGE: Partial<Record<RequestKind, { label: string; color: string }>> = {
  websocket: { label: "WS", color: "var(--m-ws)" },
  sse: { label: "SSE", color: "var(--m-sse)" },
  tcp: { label: "TCP", color: "var(--m-tcp)" },
  udp: { label: "UDP", color: "var(--m-udp)" },
  dns: { label: "DNS", color: "var(--m-dns)" },
  mqtt: { label: "MQTT", color: "var(--m-mqtt)" },
  grpc: { label: "gRPC", color: "var(--m-grpc)" },
  socketio: { label: "SIO", color: "var(--m-socketio)" },
  mcp: { label: "MCP", color: "var(--m-mcp)" },
};

/** Request kinds that keep a connection open (message log + composer). */
export const STREAM_KINDS: RequestKind[] = ["websocket", "sse", "tcp", "udp", "mqtt", "socketio"];
/** Stream kinds handled by the generic socket session API (GraphQL subscriptions too). */
export const SOCKET_KINDS: RequestKind[] = ["tcp", "udp", "mqtt", "socketio"];

export function methodColor(method: string | null | undefined, kind?: RequestKind | null): string {
  const badge = kind ? KIND_BADGE[kind] : undefined;
  if (badge) return badge.color;
  switch ((method ?? "").toUpperCase()) {
    case "GET":
      return "var(--m-get)";
    case "POST":
      return "var(--m-post)";
    case "PUT":
      return "var(--m-put)";
    case "PATCH":
      return "var(--m-patch)";
    case "DELETE":
      return "var(--m-delete)";
    case "HEAD":
      return "var(--m-head)";
    case "OPTIONS":
      return "var(--m-options)";
    default:
      return "var(--muted)";
  }
}

export function methodLabel(method: string | null | undefined, kind?: RequestKind | null): string {
  const badge = kind ? KIND_BADGE[kind] : undefined;
  if (badge) return badge.label;
  const m = (method ?? "GET").toUpperCase();
  if (m === "DELETE") return "DEL";
  if (m === "OPTIONS") return "OPT";
  if (m === "PATCH") return "PTCH";
  return m.length > 5 ? `${m.slice(0, 4)}…` : m;
}

export const COMMON_HEADERS = [
  "Accept",
  "Accept-Encoding",
  "Accept-Language",
  "Authorization",
  "Cache-Control",
  "Connection",
  "Content-Encoding",
  "Content-Type",
  "Cookie",
  "If-Match",
  "If-Modified-Since",
  "If-None-Match",
  "Origin",
  "Referer",
  "User-Agent",
  "X-API-Key",
  "X-Correlation-ID",
  "X-Forwarded-For",
  "X-Request-ID",
];

export const CONTENT_TYPES = [
  "application/json",
  "application/xml",
  "application/x-www-form-urlencoded",
  "multipart/form-data",
  "text/plain",
  "text/html",
  "text/csv",
  "application/javascript",
  "application/octet-stream",
];

/** Shown instead of the method for HTTP requests with a GraphQL body. */
export const GRAPHQL_BADGE = { label: "GQL", color: "var(--m-gql)" };

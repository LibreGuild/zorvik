// DNS request helpers without React: record types, resolver presets, reverse names.
import type { Tone } from "../../lib/format";

export const DNS_RECORD_TYPES = ["A", "AAAA", "CNAME", "MX", "TXT", "NS", "SOA", "SRV", "PTR", "CAA", "ANY"] as const;

export const DNS_TYPE_HINTS: Record<string, string> = {
  A: "IPv4 addresses",
  AAAA: "IPv6 addresses",
  CNAME: "Alias of another name",
  MX: "Mail servers",
  TXT: "Text: SPF, DKIM, site verification",
  NS: "Name servers of the zone",
  SOA: "Zone authority and serial",
  SRV: "Services: priority, weight, port, target",
  PTR: "Name of an IP address (reverse)",
  CAA: "Certificate authorities allowed to issue",
  ANY: "Everything (servers often refuse this)",
};

export interface ResolverPreset {
  id: string;
  label: string;
  /** Value of `request.dns.server`; empty = the system's resolver. */
  server: string;
  hint: string;
}

export const RESOLVER_PRESETS: ResolverPreset[] = [
  { id: "system", label: "System", server: "", hint: "The DNS servers in this computer's network settings (VPN and corporate DNS included)." },
  { id: "cloudflare", label: "Cloudflare", server: "1.1.1.1", hint: "1.1.1.1 over UDP (TCP when the answer is large)." },
  { id: "google", label: "Google", server: "8.8.8.8", hint: "8.8.8.8 over UDP (TCP when the answer is large)." },
  { id: "quad9", label: "Quad9", server: "9.9.9.9", hint: "9.9.9.9 over UDP; blocks known malicious domains." },
  { id: "cloudflare-doh", label: "Cloudflare DoH", server: "https://cloudflare-dns.com/dns-query", hint: "DNS over HTTPS: encrypted, uses the proxy settings." },
  { id: "google-doh", label: "Google DoH", server: "https://dns.google/dns-query", hint: "DNS over HTTPS: encrypted, uses the proxy settings." },
];

/** The preset a server value matches, or "custom". */
export function presetFor(server: string | undefined | null): string {
  const s = (server ?? "").trim().toLowerCase();
  if (s === "" || s === "system") return "system";
  const found = RESOLVER_PRESETS.find((p) => p.server.toLowerCase() === s || (p.server && `${p.server}:53` === s));
  return found?.id ?? "custom";
}

export type ResolverProtocol = "System" | "UDP" | "TCP" | "DoT" | "DoH";

/** How a server value is reached, or null when its scheme is not supported. */
export function resolverProtocol(server: string | undefined | null): ResolverProtocol | null {
  const s = (server ?? "").trim();
  if (s === "" || s.toLowerCase() === "system") return "System";
  const scheme = /^([a-z][a-z0-9+.-]*):\/\//i.exec(s)?.[1]?.toLowerCase();
  if (!scheme) return "UDP";
  switch (scheme) {
    case "udp":
    case "dns":
      return "UDP";
    case "tcp":
      return "TCP";
    case "tls":
    case "dot":
      return "DoT";
    case "https":
    case "http":
      return "DoH";
    default:
      return null;
  }
}

/** A quick check of a custom server (the backend has the final word). */
export function resolverProblem(server: string | undefined | null): string | null {
  const s = (server ?? "").trim();
  if (!s || s.includes("{{")) return null;
  if (resolverProtocol(s) === null) return "Use an IP address or host, or udp://, tcp://, tls:// or https://";
  if (/\s/.test(s)) return "Server addresses cannot contain spaces";
  const port = /^(?:[a-z]+:\/\/)?(?:\[[^\]]*\]|[^/:[\]]+):(\d+)(?:\/.*)?$/i.exec(s)?.[1];
  if (port !== undefined && (Number(port) < 1 || Number(port) > 65535)) return "Ports go from 1 to 65535";
  return null;
}

function ipv4Parts(text: string): number[] | null {
  const m = /^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$/.exec(text);
  if (!m) return null;
  const parts = m.slice(1).map(Number);
  return parts.every((p) => p <= 255) ? parts : null;
}

/** The 32 hex digits of an IPv6 address, or null when it is not one. */
function ipv6Nibbles(text: string): string | null {
  let s = text.toLowerCase();
  if (!s.includes(":") || /[^0-9a-f:.]/.test(s)) return null;
  // Embedded IPv4 (::ffff:1.2.3.4) becomes two groups.
  const v4 = /^(.*:)(\d+\.\d+\.\d+\.\d+)$/.exec(s);
  if (v4) {
    const p = ipv4Parts(v4[2]);
    if (!p) return null;
    s = `${v4[1]}${((p[0] << 8) | p[1]).toString(16)}:${((p[2] << 8) | p[3]).toString(16)}`;
  }
  const halves = s.split("::");
  if (halves.length > 2) return null;
  const head = halves[0] ? halves[0].split(":") : [];
  const tail = halves.length === 2 && halves[1] ? halves[1].split(":") : [];
  const missing = 8 - head.length - tail.length;
  if (halves.length === 1 ? missing !== 0 : missing < 1) return null;
  const groups = [...head, ...Array<string>(halves.length === 2 ? missing : 0).fill("0"), ...tail];
  if (groups.some((g) => !/^[0-9a-f]{1,4}$/.test(g))) return null;
  return groups.map((g) => g.padStart(4, "0")).join("");
}

/** The name to look up as typed: no scheme, user, port or path (like the backend). */
export function bareName(raw: string): string {
  let s = raw.trim();
  const scheme = s.indexOf("://");
  if (scheme >= 0) s = s.slice(scheme + 3);
  s = s.split(/[/?#]/)[0];
  const at = s.lastIndexOf("@");
  if (at >= 0) s = s.slice(at + 1);
  if (s.startsWith("[")) return s.slice(1).split("]")[0];
  const colon = s.split(":");
  if (colon.length === 2 && /^\d*$/.test(colon[1])) return colon[0];
  return s;
}

export function isIpAddress(raw: string): boolean {
  const s = bareName(raw);
  return ipv4Parts(s) !== null || ipv6Nibbles(s) !== null;
}

/** The PTR name of an IP address (`1.2.3.4` -> `4.3.2.1.in-addr.arpa`), or null. */
export function reverseName(raw: string): string | null {
  const s = bareName(raw);
  const v4 = ipv4Parts(s);
  if (v4) return `${[...v4].reverse().join(".")}.in-addr.arpa`;
  const v6 = ipv6Nibbles(s);
  if (v6) return `${[...v6].reverse().join(".")}.ip6.arpa`;
  return null;
}

/** Badge tone of a response code. */
export function rcodeTone(rcode: string): Tone {
  if (rcode === "NOERROR") return "success";
  if (rcode === "NXDOMAIN" || rcode === "NXRRSET") return "warning";
  if (rcode === "SERVFAIL" || rcode === "REFUSED" || rcode === "FORMERR" || rcode === "NOTIMP") return "danger";
  return "muted";
}

/** What a response code means, for tooltips and empty states. */
export const RCODE_MEANING: Record<string, string> = {
  NOERROR: "The server answered.",
  NXDOMAIN: "The name does not exist.",
  SERVFAIL: "The server could not get an answer (often a broken or DNSSEC-failing zone).",
  REFUSED: "The server refused to answer (it may not allow recursion for you).",
  FORMERR: "The server could not understand the question.",
  NOTIMP: "The server does not support this kind of question.",
};

/** `3600` -> `1h`, `90` -> `1m 30s`. */
export function formatTtl(seconds: number): string {
  if (seconds < 60) return `${seconds}s`;
  const units: [number, string][] = [
    [86400, "d"],
    [3600, "h"],
    [60, "m"],
    [1, "s"],
  ];
  const parts: string[] = [];
  let rest = seconds;
  for (const [size, unit] of units) {
    if (rest >= size) {
      parts.push(`${Math.floor(rest / size)}${unit}`);
      rest %= size;
    }
    if (parts.length === 2) break;
  }
  return parts.join(" ");
}

/** Records as zone-file lines (for "copy all"). */
export function zoneText(records: { name: string; type: string; class: string; ttl: number; data: string }[]): string {
  return records.map((r) => `${r.name}\t${r.ttl}\t${r.class}\t${r.type}\t${r.data}`).join("\n");
}

// DNS answers: response code, flags and the answer/authority/additional sections.
// Used as the result pane of DNS requests and by the DNS lookup tool.
import { memo, useEffect, useState } from "react";
import { AlertTriangle, Braces, Clock, Copy, Globe, Info, Lock, Search, ShieldAlert, Unplug } from "lucide-react";
import type { DnsQueryResult } from "../../bindings/DnsQueryResult";
import type { DnsResultRecord } from "../../bindings/DnsResultRecord";
import type { ErrorKind } from "../../bindings/ErrorKind";
import { formatBytes, formatMs, toneBg } from "../../lib/format";
import { copyText, modKey } from "../../lib/platform";
import { api } from "../../lib/rpc";
import { cancel, registerOneShot, type Tab, updateDraft } from "../../store/tabs";
import { toast } from "../../store/toasts";
import { openModal } from "../../store/ui";
import { Badge, Banner, Button, cx, EmptyState, IconButton, Kbd, Spinner, Tooltip } from "../ui";
import { bareName, formatTtl, isIpAddress, RCODE_MEANING, rcodeTone, reverseName, zoneText } from "./dnsModel";
import type { KindPaneProps } from "./kinds";

// Send (Mod+Enter / the Query button) runs DNS requests through dns.query; `tab.id` makes Cancel work.
registerOneShot("dns", async (tab) => ({ kind: "dns", result: await api.dnsQuery(tab.id, tab.draft, tab.path) }));

export function DnsResultPane({ tab }: KindPaneProps) {
  const r = tab.response;
  if (r.status === "loading") return <DnsLoading startedAt={r.startedAt} onCancel={() => cancel(tab.id)} />;
  if (r.status === "error") return <DnsError kind={r.kind} code={r.code} message={r.message} durationMs={r.durationMs} />;
  if (r.status === "other" && r.kind === "dns") return <DnsResultView result={r.result as DnsQueryResult} />;
  return <DnsIdle tab={tab} />;
}

function DnsIdle({ tab }: { tab: Tab }) {
  const name = tab.draft.url;
  const type = (tab.draft.method || "A").toUpperCase();
  const ip = name.trim() !== "" && !name.includes("{{") && isIpAddress(name);
  return (
    <EmptyState
      icon={<Search size={30} strokeWidth={1.5} />}
      title="Query to see the answer"
      action={
        ip && type !== "PTR" ? (
          <Button size="sm" onClick={() => updateDraft(tab.id, (d) => ({ ...d, method: "PTR" }))}>
            Look up the name of {bareName(name)} (PTR)
          </Button>
        ) : undefined
      }
    >
      {ip && type === "PTR" ? (
        <>
          Asks for <span className="font-mono text-fg">{reverseName(name)}</span>.
        </>
      ) : ip ? (
        "This is an IP address: use the PTR type to find its name."
      ) : (
        <span>
          <Kbd>{modKey}</Kbd> <Kbd>Enter</Kbd> query · the resolver is set in the Resolver tab
        </span>
      )}
    </EmptyState>
  );
}

export function DnsLoading({ startedAt, onCancel }: { startedAt: number; onCancel: () => void }) {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), 100);
    return () => clearInterval(t);
  }, []);
  return (
    <div className="flex h-full flex-col items-center justify-center gap-3" data-testid="dns-loading">
      <Spinner size={22} />
      <div className="font-mono text-[13px] tabular-nums text-muted">{formatMs(now - startedAt)}</div>
      <Button size="sm" onClick={onCancel}>
        Cancel
      </Button>
    </div>
  );
}

const ERRORS: Partial<Record<ErrorKind, { title: string; icon: React.ReactNode; hint?: string }>> = {
  timeout: { title: "No answer in time", icon: <Clock size={28} />, hint: "The server did not answer. Try another resolver, or raise the timeout in the Resolver tab." },
  connect: { title: "Could not reach the DNS server", icon: <Unplug size={28} />, hint: "Check the server address and port; a firewall may block DNS (UDP/TCP 53, 853 for DoT)." },
  dns: { title: "Could not resolve", icon: <Globe size={28} />, hint: "Pick a DNS server in the Resolver tab to query it directly." },
  tls: { title: "TLS / certificate error", icon: <ShieldAlert size={28} />, hint: "For corporate or self-signed certificates, add the CA in Settings → Certificates." },
  protocol: { title: "Invalid answer", icon: <AlertTriangle size={28} />, hint: "The server's reply was not a valid DNS message." },
  proxy: { title: "Proxy error", icon: <Unplug size={28} />, hint: "DNS over HTTPS uses the proxy from Settings → Proxy." },
  invalidRequest: { title: "Invalid query", icon: <AlertTriangle size={28} /> },
  cancelled: { title: "Query cancelled", icon: <AlertTriangle size={28} /> },
};

export function DnsError({ kind, code, message, durationMs }: { kind: ErrorKind | null; code: string; message: string; durationMs?: number }) {
  const undefinedVar = code === "undefinedVariable";
  const info = undefinedVar
    ? { title: "Undefined variable", icon: <Braces size={28} />, hint: undefined }
    : ((kind && ERRORS[kind]) ?? { title: "Query failed", icon: <AlertTriangle size={28} />, hint: undefined });
  const cancelled = kind === "cancelled";
  return (
    <div className="flex h-full flex-col items-center justify-center gap-3 p-8 text-center" data-testid="dns-error">
      <div className={cancelled ? "text-faint" : "text-danger"}>{info.icon}</div>
      <div className="text-[14px] font-semibold text-fg">{info.title}</div>
      {!cancelled && (
        <div className="selectable max-w-xl whitespace-pre-wrap break-words rounded-xl border border-line bg-panel-2 px-3.5 py-2.5 text-left font-mono text-[12px] text-muted">
          {message}
        </div>
      )}
      {info.hint && <div className="max-w-md text-[12.5px] text-muted">{info.hint}</div>}
      <div className="flex items-center gap-2">
        {!undefinedVar && durationMs !== undefined && <span className="text-[11.5px] text-faint">after {formatMs(durationMs)}</span>}
        {undefinedVar && (
          <Button size="sm" onClick={() => openModal({ type: "environments" })}>
            Open environments
          </Button>
        )}
        {kind === "tls" && (
          <Button size="sm" variant="ghost" onClick={() => openModal({ type: "settings" })}>
            Open settings
          </Button>
        )}
      </div>
    </div>
  );
}

const FLAGS: { key: "aa" | "tc" | "rd" | "ra" | "ad" | "cd"; label: string; meaning: string }[] = [
  { key: "aa", label: "AA", meaning: "Authoritative answer: from a server that owns the zone" },
  { key: "tc", label: "TC", meaning: "Truncated: the answer did not fit" },
  { key: "rd", label: "RD", meaning: "Recursion desired (asked for)" },
  { key: "ra", label: "RA", meaning: "Recursion available on this server" },
  { key: "ad", label: "AD", meaning: "Authentic data: DNSSEC-validated by the resolver" },
  { key: "cd", label: "CD", meaning: "Checking disabled: DNSSEC validation was skipped" },
];

function serverLabel(result: DnsQueryResult): string {
  if (result.protocol === "System") return "Operating system lookup";
  if (result.system) return `System resolver ${result.server} · ${result.protocol}`;
  return result.protocol === "DoH" ? `DoH · ${result.server}` : `${result.protocol} · ${result.server}`;
}

// Memoized: the request tab re-renders on every keystroke; the tables only need to when the result changes.
export const DnsResultView = memo(function DnsResultView({ result }: { result: DnsQueryResult }) {
  const tone = rcodeTone(result.rcode);
  const all = [...result.answers, ...result.authority, ...result.additional];
  const sections: { title: string; records: DnsResultRecord[] }[] = [
    { title: "Answer", records: result.answers },
    { title: "Authority", records: result.authority },
    { title: "Additional", records: result.additional },
  ];
  const q = result.question;
  return (
    <div className="flex h-full min-h-0 flex-col" data-testid="dns-result">
      <div className="flex min-h-11 shrink-0 flex-wrap items-center gap-x-3 gap-y-1 px-3 pt-1">
        <Tooltip content={RCODE_MEANING[result.rcode] ?? `Response code ${result.rcodeValue}`}>
          <span>
            <Badge className={cx("text-[12px]", toneBg[tone])}>
              <span data-testid="dns-rcode">{result.rcode}</span>
            </Badge>
          </span>
        </Tooltip>
        <span className="text-[12px] tabular-nums text-muted" title="Total time">
          {formatMs(result.durationMs)}
        </span>
        {result.size > 0 && <span className="text-[12px] tabular-nums text-muted">{formatBytes(result.size)}</span>}
        <span className="flex min-w-0 items-center gap-1.5 truncate text-[12px] text-faint" title={serverLabel(result)}>
          {result.tls && <Lock size={12} className="shrink-0 text-success" aria-label={result.tls.version} />}
          <span className="truncate">{serverLabel(result)}</span>
        </span>
        <div className="flex-1" />
        {result.protocol !== "System" && (
          <div className="flex items-center gap-1" aria-label="Flags">
            {FLAGS.map((f) => (
              <Tooltip key={f.key} content={`${f.meaning}${result.flags[f.key] ? "" : " (not set)"}`}>
                <span
                  className={cx(
                    "rounded px-1 font-mono text-[10.5px] font-semibold",
                    result.flags[f.key] ? "bg-accent-soft text-accent" : "text-faint/60 line-through decoration-faint/40",
                  )}
                >
                  {f.label}
                </span>
              </Tooltip>
            ))}
          </div>
        )}
        {all.length > 0 && (
          <IconButton
            label="Copy all records"
            onClick={() => {
              void copyText(zoneText(all));
              toast("success", "Copied to clipboard");
            }}
          >
            <Copy size={14} />
          </IconButton>
        )}
      </div>
      {result.unresolved.length > 0 && (
        <Banner tone="warning">
          Sent with undefined values: <span className="font-mono">{result.unresolved.map((v) => `{{${v}}}`).join(", ")}</span>
        </Banner>
      )}
      {result.notes.map((note, i) => (
        <Banner key={i} tone="info">
          {note}
        </Banner>
      ))}
      <div className="min-h-0 flex-1 overflow-auto pb-3">
        <div className="px-3 pb-1 pt-2 text-[12px] text-muted">
          <span className="font-mono text-fg">{q.name}</span> <span className="font-mono">{q.class}</span>{" "}
          <span className="font-mono font-semibold" style={{ color: "var(--m-dns)" }}>
            {q.type}
          </span>
        </div>
        {result.answers.length === 0 && (
          <div className="mx-3 my-2 flex items-start gap-2 rounded-lg border border-line bg-panel-2 px-3 py-2.5 text-[12.5px] text-muted">
            <Info size={14} className="mt-0.5 shrink-0 text-faint" />
            <div>
              {result.rcode === "NOERROR"
                ? `The name exists but has no ${q.type} records.`
                : (RCODE_MEANING[result.rcode] ?? `The server answered ${result.rcode}.`)}
              {result.authority.some((r) => r.type === "SOA") && " The SOA record below says how long resolvers may cache this."}
            </div>
          </div>
        )}
        {sections
          .filter((s) => s.records.length > 0)
          .map((s) => (
            <RecordTable key={s.title} title={s.title} records={s.records} />
          ))}
      </div>
    </div>
  );
});

function RecordTable({ title, records }: { title: string; records: DnsResultRecord[] }) {
  return (
    <section className="mt-2">
      <div className="px-3 pb-1.5 pt-1 text-[11px] font-semibold uppercase tracking-wide text-faint">
        {title} <span className="font-normal">({records.length})</span>
      </div>
      <div className="mx-3 overflow-hidden rounded-lg border border-line">
        <table className="selectable w-full table-fixed border-collapse text-[12.5px]">
          <thead>
            <tr className="border-b border-line/60 text-left text-[11px] uppercase tracking-wide text-faint">
              <th className="w-[30%] px-3 py-1.5 font-semibold">Name</th>
              <th className="w-[76px] px-2 py-1.5 font-semibold">Type</th>
              <th className="w-[88px] px-2 py-1.5 font-semibold">TTL</th>
              <th className="px-2 py-1.5 font-semibold">Data</th>
              <th className="w-8" />
            </tr>
          </thead>
          <tbody>
            {records.map((r, i) => (
              <tr key={i} className="group border-b border-line/60 align-top last:border-b-0 hover:bg-hover/40">
                <td className="truncate px-3 py-1.5 font-mono text-muted" title={r.name}>
                  {r.name}
                </td>
                <td className="px-2 py-1.5 font-mono font-semibold" style={{ color: "var(--m-dns)" }}>
                  {r.type}
                </td>
                <td className="px-2 py-1.5 tabular-nums text-muted" title={`${r.ttl} seconds`}>
                  {formatTtl(r.ttl)}
                </td>
                <td className="break-all px-2 py-1.5 font-mono text-fg">{r.data}</td>
                <td className="pr-2 pt-1">
                  <button
                    aria-label="Copy data"
                    onClick={() => {
                      void copyText(r.data);
                      toast("success", "Copied to clipboard");
                    }}
                    className="rounded p-1 text-faint opacity-0 hover:bg-hover hover:text-fg focus:opacity-100 group-hover:opacity-100"
                  >
                    <Copy size={12} />
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </section>
  );
}

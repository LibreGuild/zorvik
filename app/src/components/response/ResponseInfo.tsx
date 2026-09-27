import { ArrowRight, Lock, LockOpen } from "lucide-react";
import type { ResponseMeta } from "../../bindings/ResponseMeta";
import type { Timing } from "../../bindings/Timing";
import { formatBytes, formatMs } from "../../lib/format";
import { HeaderTable } from "./ResponsePane";

const PHASES: { key: keyof Timing; label: string; color: string; hint: string }[] = [
  { key: "redirectMs", label: "Redirects", color: "var(--m-head)", hint: "Earlier redirect hops" },
  { key: "dnsMs", label: "DNS lookup", color: "var(--m-ws)", hint: "Resolving the host name" },
  { key: "connectMs", label: "TCP connect", color: "var(--m-put)", hint: "Opening the connection (incl. proxy tunnel)" },
  { key: "tlsMs", label: "TLS handshake", color: "var(--m-patch)", hint: "Negotiating encryption" },
  { key: "ttfbMs", label: "Waiting (TTFB)", color: "var(--m-post)", hint: "Server processing until the first byte" },
  { key: "downloadMs", label: "Download", color: "var(--m-get)", hint: "Receiving the body" },
];

/** HTTP/3: the QUIC handshake includes TLS, so it is one "connect" phase. */
const QUIC_CONNECT = { label: "QUIC handshake", hint: "Opening the QUIC connection, TLS 1.3 included" };

export function TimingView({ timing, protocol }: { timing: Timing; protocol?: string }) {
  const total = Math.max(timing.totalMs, 0.001);
  const quic = protocol === "HTTP/3";
  let offset = 0;
  return (
    <div className="max-w-3xl p-4">
      <div className="flex flex-col gap-2.5">
        {PHASES.map((p) => {
          const value = timing[p.key];
          const left = (offset / total) * 100;
          offset += value;
          const width = Math.max((value / total) * 100, value > 0 ? 0.6 : 0);
          if (p.key === "redirectMs" && value === 0) return null;
          if (quic && p.key === "tlsMs") return null;
          const { label, hint } = quic && p.key === "connectMs" ? QUIC_CONNECT : p;
          return (
            <div key={p.key} className="grid grid-cols-[150px_1fr_80px] items-center gap-3" title={hint}>
              <div className="text-[12.5px] text-muted">{label}</div>
              <div className="relative h-3.5 rounded bg-panel-2">
                <div className="absolute inset-y-0 rounded" style={{ left: `${left}%`, width: `${width}%`, background: p.color }} />
              </div>
              <div className="text-right font-mono text-[12px] tabular-nums text-fg">{formatMs(value)}</div>
            </div>
          );
        })}
        <div className="mt-1 grid grid-cols-[150px_1fr_80px] items-center gap-3 border-t border-line pt-2.5">
          <div className="text-[12.5px] font-semibold text-fg">Total</div>
          <div />
          <div className="text-right font-mono text-[12px] font-semibold tabular-nums text-fg">{formatMs(timing.totalMs)}</div>
        </div>
      </div>
      <p className="mt-4 text-[11.5px] text-faint">
        Each request uses a fresh connection, so DNS, connect and TLS are always measured.
      </p>
    </div>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="pb-2">
      <div className="px-3 pb-1.5 pt-3 text-[11px] font-semibold uppercase tracking-wide text-faint">{title}</div>
      {children}
    </div>
  );
}

function KV({ label, value, mono = true }: { label: string; value: React.ReactNode; mono?: boolean }) {
  return (
    <div className="grid grid-cols-[160px_1fr] gap-3 px-3 py-1 text-[12.5px]">
      <div className="text-muted">{label}</div>
      <div className={mono ? "selectable break-all font-mono text-fg" : "selectable text-fg"}>{value}</div>
    </div>
  );
}

export function InfoView({ meta }: { meta: ResponseMeta }) {
  const req = meta.request;
  const cert = meta.tls?.certificate;
  return (
    <div className="pb-6">
      <Section title="Connection">
        <KV label="URL" value={meta.url} />
        <KV label="Remote address" value={meta.remoteAddr ?? "–"} />
        <KV label="Protocol" value={meta.httpVersion} />
        {req.proxy && <KV label="Proxy" value={req.proxy} />}
        <KV label="Response headers" value={formatBytes(meta.headersSize)} mono={false} />
      </Section>
      <Section title="Security">
        {meta.tls ? (
          <>
            <KV label="TLS" value={<span className="inline-flex items-center gap-1.5 text-success"><Lock size={12} />{meta.tls.version}</span>} />
            <KV label="Cipher" value={meta.tls.cipher} />
            <KV label="ALPN" value={meta.tls.alpn ?? "–"} />
            {cert && (
              <>
                <KV label="Subject" value={cert.subject} />
                <KV label="Issuer" value={cert.issuer} />
                <KV label="Valid" value={`${new Date(cert.notBefore).toLocaleDateString()} → ${new Date(cert.notAfter).toLocaleDateString()}`} />
                <KV label="Names" value={cert.subjectAltNames.join(", ") || "–"} />
                <KV label="Serial" value={cert.serial} />
              </>
            )}
          </>
        ) : (
          <KV label="TLS" value={<span className="inline-flex items-center gap-1.5 text-warning"><LockOpen size={12} />Not encrypted</span>} />
        )}
      </Section>
      {meta.redirects.length > 0 && (
        <Section title="Redirects">
          {meta.redirects.map((r, i) => (
            <div key={i} className="flex items-center gap-2 px-3 py-1 font-mono text-[12px]">
              <span className="text-info">{r.status}</span>
              <span className="text-muted">{r.method}</span>
              <span className="min-w-0 truncate text-fg" title={r.url}>
                {r.url}
              </span>
              <ArrowRight size={12} className="shrink-0 text-faint" />
              <span className="min-w-0 truncate text-muted" title={r.location}>
                {r.location}
              </span>
            </div>
          ))}
        </Section>
      )}
      <Section title={`Request sent · ${req.method} · ${formatBytes(req.bodySize)} body`}>
        <KV label="URL" value={req.url} />
        <HeaderTable headers={req.headers} />
      </Section>
    </div>
  );
}

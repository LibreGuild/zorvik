// HTTP/3 check: does a site speak HTTP/3 (QUIC)? Sends the same GET twice on
// fresh connections, once as usual (HTTP/1.1 or HTTP/2) and once over HTTP/3,
// and reads the Alt-Svc header the site advertises.
import { Ban, CheckCircle2, CircleAlert, Send, Square, TriangleAlert, WifiOff } from "lucide-react";
import type { ReactNode } from "react";
import type { ErrorKind } from "../../bindings/ErrorKind";
import type { Request } from "../../bindings/Request";
import type { SendResult } from "../../bindings/SendResult";
import type { Timing } from "../../bindings/Timing";
import type { TlsInfo } from "../../bindings/TlsInfo";
import { formatMs, statusTone, toneBg } from "../../lib/format";
import { newId } from "../../lib/ids";
import { api, RpcError, errorMessage } from "../../lib/rpc";
import { isToolTab, updateToolState, useTabs } from "../../store/tabs";
import { Badge, Button, Input, Spinner, cx } from "../ui";
import type { ToolProps } from "./registry";

/** Result of one of the two requests (only what the view needs, not the body). */
export type Outcome =
  | {
      ok: true;
      status: number;
      statusText: string;
      httpVersion: string;
      url: string;
      timing: Timing;
      tls: TlsInfo | null;
      remoteAddr: string | null;
      altSvc: string | null;
    }
  | { ok: false; kind: ErrorKind | null; message: string };

interface CheckResult {
  url: string;
  /** The URL was changed before checking (scheme added or switched to https). */
  note: string | null;
  auto: Outcome | null;
  h3: Outcome | null;
}

interface CheckState {
  url: string;
  /** Set while a check runs; results of an older (cancelled) run are ignored. */
  runId: string | null;
  result: CheckResult | null;
}

const TIMEOUT_MS = 20_000;

function readState(state: Record<string, unknown>): CheckState {
  return {
    url: typeof state.url === "string" ? state.url : "",
    runId: typeof state.runId === "string" ? state.runId : null,
    result: (state.result as CheckResult | undefined) ?? null,
  };
}

// ---- Alt-Svc (RFC 7838) -------------------------------------------------

export interface AltSvcEntry {
  protocol: string;
  /** `:443` (same host) or `host:port`. */
  authority: string;
  /** Seconds the client may remember it (default 24 h). */
  maxAge: number;
}

/** Split on `sep` outside double quotes. */
function splitOutsideQuotes(text: string, sep: string): string[] {
  const parts: string[] = [];
  let current = "";
  let quoted = false;
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (c === "\\" && quoted && i + 1 < text.length) {
      current += c + text[++i];
      continue;
    }
    if (c === '"') quoted = !quoted;
    if (c === sep && !quoted) {
      parts.push(current);
      current = "";
    } else {
      current += c;
    }
  }
  parts.push(current);
  return parts;
}

function unquote(value: string): string {
  const v = value.trim();
  return v.length >= 2 && v.startsWith('"') && v.endsWith('"') ? v.slice(1, -1).replace(/\\(.)/g, "$1") : v;
}

/** Parse Alt-Svc header values (several header lines may be joined with ", "). */
export function parseAltSvc(value: string): { clear: boolean; entries: AltSvcEntry[] } {
  const entries: AltSvcEntry[] = [];
  let clear = false;
  for (const raw of splitOutsideQuotes(value, ",")) {
    const [alternative, ...params] = splitOutsideQuotes(raw, ";");
    const text = alternative.trim();
    if (!text) continue;
    if (text.toLowerCase() === "clear") {
      clear = true;
      continue;
    }
    const eq = text.indexOf("=");
    if (eq <= 0) continue;
    let protocol = text.slice(0, eq).trim();
    try {
      protocol = decodeURIComponent(protocol);
    } catch {
      // keep as sent
    }
    let maxAge = 86_400;
    for (const p of params) {
      const [k, v = ""] = p.split("=");
      const seconds = Number(unquote(v));
      if (k.trim().toLowerCase() === "ma" && Number.isFinite(seconds) && seconds >= 0) maxAge = seconds;
    }
    entries.push({ protocol, authority: unquote(text.slice(eq + 1)), maxAge });
  }
  return { clear, entries };
}

/** `h3`, or a draft version such as `h3-29`. */
export const isH3Protocol = (protocol: string) => /^h3(-\w+)?$/i.test(protocol);

function formatAge(seconds: number): string {
  if (seconds < 60) return `${seconds} s`;
  if (seconds < 3600) return `${Math.round(seconds / 60)} min`;
  if (seconds < 86_400) return `${Math.round(seconds / 3600)} h`;
  const days = Math.round(seconds / 86_400);
  return `${days} day${days === 1 ? "" : "s"}`;
}

// ---- Running the check --------------------------------------------------

/** Add `https://` when missing; HTTP/3 only exists for https. */
export function normalizeCheckUrl(input: string): { url: string; note: string | null } {
  const trimmed = input.trim();
  if (/^https:\/\//i.test(trimmed)) return { url: trimmed, note: null };
  if (/^http:\/\//i.test(trimmed)) {
    return { url: `https://${trimmed.slice(7)}`, note: "HTTP/3 only works over https, so https:// was checked." };
  }
  return { url: `https://${trimmed}`, note: null };
}

function checkRequest(url: string, httpVersion: "auto" | "http3"): Request {
  return {
    name: "HTTP/3 check",
    seq: 0,
    method: "GET",
    url,
    // Never send the workspace's credentials to whatever site is being checked.
    auth: { type: "none" },
    settings: { httpVersion, timeoutMs: TIMEOUT_MS, followRedirects: true },
  };
}

function toOutcome(r: SendResult): Outcome {
  const altSvc = r.meta.headers.filter((h) => h.name.toLowerCase() === "alt-svc").map((h) => h.value);
  return {
    ok: true,
    status: r.meta.status,
    statusText: r.meta.statusText,
    httpVersion: r.meta.httpVersion,
    url: r.meta.url,
    timing: r.timing,
    tls: r.meta.tls,
    remoteAddr: r.meta.remoteAddr,
    altSvc: altSvc.length ? altSvc.join(", ") : null,
  };
}

function toFailure(e: unknown): Outcome {
  return { ok: false, kind: e instanceof RpcError ? e.networkKind : null, message: errorMessage(e) };
}

const requestIds = (runId: string) => ({ auto: `http3-check:${runId}:auto`, h3: `http3-check:${runId}:h3` });

async function runCheck(tabId: string, input: string) {
  const { url, note } = normalizeCheckUrl(input);
  const runId = newId();
  const ids = requestIds(runId);
  updateToolState(tabId, (s) => ({ ...s, runId, result: { url, note, auto: null, h3: null } }));
  // False once the check was cancelled, restarted or its tab closed.
  const current = () => {
    const tab = useTabs.getState().tabs.find((t) => t.id === tabId);
    return isToolTab(tab) && readState(tab.state).runId === runId;
  };
  const store = (part: Partial<CheckResult>, done = false) =>
    updateToolState(tabId, (s) => {
      const st = readState(s);
      if (st.runId !== runId || !st.result) return s;
      return { ...s, runId: done ? null : runId, result: { ...st.result, ...part } };
    });
  // One after the other, so the timings don't compete for the network.
  // Standalone: the site being checked gets none of the workspace's headers, auth or cookies.
  const auto = await api.send(ids.auto, checkRequest(url, "auto"), null, { standalone: true }).then(toOutcome, toFailure);
  if (!current()) return;
  store({ auto });
  const h3 = await api.send(ids.h3, checkRequest(url, "http3"), null, { standalone: true }).then(toOutcome, toFailure);
  store({ h3 }, true);
}

function cancelCheck(tabId: string, runId: string) {
  const ids = requestIds(runId);
  void api.cancel(ids.auto).catch(() => {});
  void api.cancel(ids.h3).catch(() => {});
  updateToolState(tabId, (s) => (readState(s).runId === runId ? { ...s, runId: null, result: null } : s));
}

// ---- Verdict ------------------------------------------------------------

interface Verdict {
  tone: "success" | "warning" | "danger" | "muted";
  icon: ReactNode;
  title: string;
  detail: string;
}

export function verdictFor(auto: Outcome, h3: Outcome, advertised: boolean): Verdict {
  if (h3.ok) {
    return {
      tone: "success",
      icon: <CheckCircle2 size={18} />,
      title: "Speaks HTTP/3",
      detail: advertised
        ? `Advertised in Alt-Svc and answered over QUIC with ${h3.status} ${h3.statusText}.`.trim()
        : "Answered over QUIC, but its Alt-Svc header has no h3 entry, so browsers only use HTTP/3 if a DNS HTTPS record announces it.",
    };
  }
  if (h3.kind === "proxy") {
    return {
      tone: "muted",
      icon: <Ban size={18} />,
      title: "Not checked: a proxy is set for this host",
      detail: "HTTP/3 (UDP) can't go through an HTTP proxy. Add the host to the proxy bypass list in Settings to check it.",
    };
  }
  if (!auto.ok && !advertised) {
    return { tone: "danger", icon: <CircleAlert size={18} />, title: "Could not reach the site", detail: auto.message };
  }
  if (advertised && (h3.kind === "connect" || h3.kind === "timeout" || h3.kind === "io")) {
    return {
      tone: "warning",
      icon: <WifiOff size={18} />,
      title: "Advertises HTTP/3 but the QUIC connection failed (UDP blocked?)",
      detail:
        "The site offers HTTP/3, but no QUIC connection could be made from this computer. A firewall, VPN or corporate network may block UDP port 443; browsers then quietly fall back to HTTP/2.",
    };
  }
  if (advertised) {
    return {
      tone: "warning",
      icon: <TriangleAlert size={18} />,
      title: "Advertises HTTP/3 but the HTTP/3 request failed",
      detail: h3.message,
    };
  }
  return {
    tone: "muted",
    icon: <Ban size={18} />,
    title: "No HTTP/3",
    detail: "The site does not advertise HTTP/3 (no h3 in Alt-Svc) and a direct QUIC connection did not work.",
  };
}

const verdictStyle: Record<Verdict["tone"], string> = {
  success: "border-success/30 bg-success/8 text-success",
  warning: "border-warning/30 bg-warning/8 text-warning",
  danger: "border-danger/30 bg-danger/8 text-danger",
  muted: "border-line bg-panel-2 text-muted",
};

// ---- View ---------------------------------------------------------------

export function Http3CheckTool({ tab }: ToolProps) {
  const { url, runId, result } = readState(tab.state);
  const running = runId !== null;
  const canRun = url.trim().length > 0 && !running;
  const start = () => {
    if (canRun) void runCheck(tab.id, url);
  };

  return (
    <div className="flex max-w-4xl flex-col gap-4 px-4 pb-6 pt-2">
      <div className="flex items-center gap-2">
        <Input
          aria-label="URL to check"
          value={url}
          placeholder="https://example.com"
          className="font-mono"
          onChange={(e) => updateToolState(tab.id, (s) => ({ ...s, url: e.target.value }))}
          onKeyDown={(e) => {
            if (e.key === "Enter") start();
          }}
        />
        {running ? (
          <Button icon={<Square size={13} />} onClick={() => cancelCheck(tab.id, runId)}>
            Cancel
          </Button>
        ) : (
          <Button variant="primary" icon={<Send size={13} />} disabled={!canRun} onClick={start}>
            Check
          </Button>
        )}
      </div>
      <p className="-mt-2 text-[11.5px] text-faint">
        Sends a GET twice on fresh connections: as usual (HTTP/1.1 or HTTP/2 over TCP), then over HTTP/3 (QUIC on UDP).
        Both use your environment variables, proxy and TLS settings, and appear in History.
      </p>
      {result && <ResultView result={result} running={running} />}
    </div>
  );
}

function ResultView({ result, running }: { result: CheckResult; running: boolean }) {
  const altSvc = result.auto?.ok ? result.auto.altSvc : null;
  const h3AltSvc = result.h3?.ok ? result.h3.altSvc : null;
  const parsed = parseAltSvc([altSvc, h3AltSvc].filter(Boolean).join(", "));
  const h3Entries = parsed.entries.filter((e) => isH3Protocol(e.protocol));
  // Dedupe entries repeated on both responses.
  const shown = h3Entries.filter((e, i) => h3Entries.findIndex((o) => o.protocol === e.protocol && o.authority === e.authority) === i);
  const verdict = result.auto && result.h3 ? verdictFor(result.auto, result.h3, shown.length > 0) : null;

  return (
    <>
      {result.note && <div className="text-[12px] text-muted">{result.note}</div>}
      {verdict ? (
        <div className={cx("flex items-start gap-3 rounded-xl border px-3.5 py-3", verdictStyle[verdict.tone])} data-testid="http3-verdict">
          <div className="mt-px shrink-0">{verdict.icon}</div>
          <div className="min-w-0">
            <div className="text-[13.5px] font-semibold">{verdict.title}</div>
            <div className="selectable mt-0.5 break-words text-[12.5px] text-muted">{verdict.detail}</div>
          </div>
        </div>
      ) : (
        running && (
          <div className="flex items-center gap-2 text-[12.5px] text-muted">
            <Spinner /> {result.auto ? "Trying HTTP/3 (QUIC)…" : "Requesting over HTTP/1.1 / HTTP/2…"}
          </div>
        )
      )}

      <section>
        <SectionTitle>Alt-Svc advertised</SectionTitle>
        {!result.auto ? (
          <div className="text-[12.5px] text-faint">…</div>
        ) : !result.auto.ok && !h3AltSvc ? (
          <div className="text-[12.5px] text-faint">Unknown (the HTTP/1.1 / HTTP/2 request failed).</div>
        ) : shown.length > 0 ? (
          <div className="flex flex-wrap gap-1.5">
            {shown.map((e) => (
              <span key={`${e.protocol}${e.authority}`} className="rounded-lg border border-line bg-panel-2 px-2 py-1 font-mono text-[12px] text-fg">
                {e.protocol} → {e.authority.startsWith(":") ? `port ${e.authority.slice(1)}` : e.authority}
                <span className="ml-1.5 font-sans text-faint">for {formatAge(e.maxAge)}</span>
              </span>
            ))}
          </div>
        ) : (
          <div className="text-[12.5px] text-muted">
            {parsed.clear ? "Alt-Svc: clear (the site withdrew its alternatives)." : altSvc ? "No HTTP/3 entry." : "No Alt-Svc header."}
          </div>
        )}
        {altSvc && <div className="selectable mt-1.5 break-all font-mono text-[11.5px] text-faint">Alt-Svc: {altSvc}</div>}
      </section>

      <section>
        <SectionTitle>Comparison</SectionTitle>
        <Comparison auto={result.auto} h3={result.h3} running={running} />
      </section>
    </>
  );
}

function SectionTitle({ children }: { children: ReactNode }) {
  return <div className="pb-1.5 text-[11px] font-semibold uppercase tracking-wide text-faint">{children}</div>;
}

interface Row {
  label: string;
  hint?: string;
  value: (o: Extract<Outcome, { ok: true }>) => ReactNode;
}

const ROWS: Row[] = [
  {
    label: "Result",
    value: (o) => (
      <Badge className={toneBg[statusTone(o.status)]}>
        {o.status} {o.statusText}
      </Badge>
    ),
  },
  { label: "Protocol", value: (o) => o.httpVersion },
  { label: "Total time", value: (o) => formatMs(o.timing.totalMs) },
  { label: "DNS lookup", value: (o) => formatMs(o.timing.dnsMs) },
  {
    label: "Handshake",
    hint: "TCP connect + TLS for HTTP/1.1 and HTTP/2; the single QUIC handshake (which includes TLS 1.3) for HTTP/3.",
    value: (o) => formatMs(o.timing.connectMs + o.timing.tlsMs),
  },
  { label: "Waiting (TTFB)", value: (o) => formatMs(o.timing.ttfbMs) },
  { label: "Remote address", value: (o) => o.remoteAddr ?? "–" },
  { label: "TLS", value: (o) => (o.tls ? [o.tls.version, o.tls.cipher].filter(Boolean).join(" · ") : "Not encrypted") },
  { label: "ALPN", value: (o) => o.tls?.alpn ?? "–" },
  { label: "Final URL", value: (o) => o.url },
];

function Comparison({ auto, h3, running }: { auto: Outcome | null; h3: Outcome | null; running: boolean }) {
  const cell = (o: Outcome | null, row: Row) => {
    if (!o) return running ? <Spinner size={12} /> : <span className="text-faint">–</span>;
    if (!o.ok) return <span className="text-faint">–</span>;
    return row.value(o);
  };
  const faster = auto?.ok && h3?.ok ? auto.timing.totalMs - h3.timing.totalMs : null;
  return (
    <div className="overflow-hidden rounded-xl border border-line">
      <div className="grid grid-cols-[150px_1fr_1fr] gap-3 border-b border-line bg-panel-2 px-3 py-2 text-[12px] font-semibold text-fg">
        <div />
        <div>HTTP/1.1 / HTTP/2 (TCP)</div>
        <div>HTTP/3 (QUIC)</div>
      </div>
      {[auto, h3].some((o) => o && !o.ok) && (
        <div className="grid grid-cols-[150px_1fr_1fr] gap-3 border-b border-line/60 px-3 py-2 text-[12.5px]">
          <div className="text-muted">Error</div>
          {[auto, h3].map((o, i) => (
            <div key={i} className="selectable min-w-0 break-words font-mono text-[12px] text-danger">
              {o && !o.ok ? o.message : ""}
            </div>
          ))}
        </div>
      )}
      {ROWS.map((row) => (
        <div key={row.label} className="grid grid-cols-[150px_1fr_1fr] items-center gap-3 px-3 py-1.5 text-[12.5px]" title={row.hint}>
          <div className="text-muted">{row.label}</div>
          <div className="selectable min-w-0 break-all font-mono text-[12px] text-fg">{cell(auto, row)}</div>
          <div className="selectable min-w-0 break-all font-mono text-[12px] text-fg">{cell(h3, row)}</div>
        </div>
      ))}
      {faster !== null && (
        <div className="border-t border-line/60 px-3 py-2 text-[12px] text-muted">
          {Math.abs(faster) < 1
            ? "Both took about the same time."
            : `HTTP/3 was ${formatMs(Math.abs(faster))} ${faster > 0 ? "faster" : "slower"} in total (one sample each, fresh connections).`}
        </div>
      )}
    </div>
  );
}

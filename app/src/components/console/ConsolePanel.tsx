// The console under the open tab: every request the app sent (what went out, what came back,
// why it failed) and what scripts logged, newest at the bottom.
import { memo, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { ChevronRight, Copy, Search, Terminal, Trash2, X } from "lucide-react";
import type { ConsoleBody } from "../../bindings/ConsoleBody";
import type { ConsoleDetails } from "../../bindings/ConsoleDetails";
import type { ConsoleItem } from "../../bindings/ConsoleItem";
import type { ConsoleLevel } from "../../bindings/ConsoleLevel";
import type { ConsoleLine } from "../../bindings/ConsoleLine";
import type { Header } from "../../bindings/Header";
import { formatBytes, formatClock, formatMs, statusTone, toneText } from "../../lib/format";
import { methodColor } from "../../lib/http";
import { copyText } from "../../lib/platform";
import { api } from "../../lib/rpc";
import { clearConsole, type ConsoleFilter, filterLines, useConsole } from "../../store/console";
import { useUi } from "../../store/ui";
import { prettyMessage } from "../request/jsonFormat";
import { HeaderTable } from "../response/ResponsePane";
import { cx, EmptyState, IconButton, Segmented } from "../ui";

/** Lines drawn at first, and added by "Show earlier lines" (the list keeps up to 5000). */
const PAGE = 300;

const LEVEL_CLASS: Record<ConsoleLevel, string> = {
  log: "text-fg",
  info: "text-muted",
  debug: "text-faint",
  warn: "text-warning",
  error: "text-danger",
};

const SOURCE_LABEL = { user: null, runner: "Runner", agent: "Agent" } as const;

export function ConsolePanel() {
  const lines = useConsole((s) => s.lines);
  const [filter, setFilter] = useState<ConsoleFilter>("all");
  const [search, setSearch] = useState("");
  const [shown, setShown] = useState(PAGE);
  const matching = useMemo(() => filterLines(lines, filter, search), [lines, filter, search]);
  const visible = matching.length > shown ? matching.slice(matching.length - shown) : matching;
  const list = useRef<HTMLDivElement>(null);
  // Follows new lines while the list is scrolled to its end.
  const follow = useRef(true);
  useLayoutEffect(() => {
    const el = list.current;
    if (el && follow.current) el.scrollTop = el.scrollHeight;
  }, [visible]);

  return (
    <section aria-label="Console" className="flex h-full min-h-0 flex-col bg-bg" data-testid="console-panel">
      <header className="flex h-9 shrink-0 items-center gap-2 border-b border-line/70 px-3">
        <Terminal size={14} className="shrink-0 text-muted" />
        <span className="text-[12.5px] font-semibold text-fg">Console</span>
        <Segmented<ConsoleFilter>
          items={[
            { id: "all", label: "All" },
            { id: "requests", label: "Requests" },
            { id: "logs", label: "Logs" },
            { id: "errors", label: "Errors" },
          ]}
          value={filter}
          onChange={(f) => {
            setFilter(f);
            setShown(PAGE);
          }}
        />
        <label className="flex h-6 min-w-0 max-w-64 flex-1 items-center gap-1.5 rounded-md border border-line bg-panel-2 px-2 focus-within:border-accent">
          <Search size={12} className="shrink-0 text-faint" />
          <input
            aria-label="Filter the console"
            value={search}
            onChange={(e) => {
              setSearch(e.target.value);
              setShown(PAGE);
            }}
            placeholder="Filter by URL, name or text"
            className="min-w-0 flex-1 bg-transparent text-[12px] text-fg outline-none placeholder:text-faint"
          />
        </label>
        <span className="ml-auto shrink-0 text-[11.5px] tabular-nums text-faint" data-testid="console-count">
          {matching.length === lines.length ? `${lines.length} ${lines.length === 1 ? "line" : "lines"}` : `${matching.length} of ${lines.length}`}
        </span>
        <IconButton label="Clear the console" onClick={() => void clearConsole()} disabled={!lines.length}>
          <Trash2 size={13} />
        </IconButton>
        <IconButton label="Close the console" onClick={() => useUi.setState({ consoleOpen: false })}>
          <X size={14} />
        </IconButton>
      </header>
      <div
        ref={list}
        className="min-h-0 flex-1 overflow-auto"
        onScroll={(e) => {
          const el = e.currentTarget;
          follow.current = el.scrollHeight - el.scrollTop - el.clientHeight < 32;
        }}
        data-testid="console-lines"
      >
        {!lines.length ? (
          <EmptyState title="Nothing sent yet">Every request Zorvik sends shows here with what went out and what came back, next to what scripts log.</EmptyState>
        ) : !matching.length ? (
          <EmptyState title="No matching lines" />
        ) : (
          <>
            {matching.length > visible.length && (
              <button className="w-full border-b border-line/60 py-1.5 text-[12px] text-accent hover:underline" onClick={() => setShown(shown + PAGE)}>
                Show earlier lines ({matching.length - visible.length} more)
              </button>
            )}
            {visible.map((line) => (line.item.type === "log" ? <LogRow key={line.id} line={line} item={line.item} /> : <ExchangeRow key={line.id} line={line} item={line.item} />))}
          </>
        )}
      </div>
    </section>
  );
}

function Time({ at }: { at: number }) {
  return <span className="w-[84px] shrink-0 font-mono text-[10.5px] tabular-nums text-faint">{formatClock(at)}</span>;
}

/** Who sent it, when not the user, and for which request. */
function Origin({ line }: { line: ConsoleLine }) {
  const source = SOURCE_LABEL[line.source];
  return (
    <>
      {source && <span className="shrink-0 rounded bg-hover px-1 text-[10px] font-semibold text-muted">{source}</span>}
      {line.request && (
        <span className="max-w-[28%] shrink-0 truncate text-[11.5px] text-faint" title={line.request}>
          {line.request}
        </span>
      )}
    </>
  );
}

const LogRow = memo(function LogRow({ line, item }: { line: ConsoleLine; item: Extract<ConsoleItem, { type: "log" }> }) {
  return (
    <div className={cx("selectable flex items-start gap-2 border-b border-line/50 px-3 py-1 font-mono text-[12px]", LEVEL_CLASS[item.level])} data-testid="console-log">
      <Time at={line.at} />
      <span className="w-10 shrink-0 text-[10px] uppercase leading-[18px] text-faint">{item.level}</span>
      <span className="min-w-0 flex-1 whitespace-pre-wrap break-words">{item.message}</span>
      <Origin line={line} />
    </div>
  );
});

const ExchangeRow = memo(function ExchangeRow({ line, item }: { line: ConsoleLine; item: Extract<ConsoleItem, { type: "exchange" }> }) {
  const [open, setOpen] = useState(false);
  const label = item.method || item.protocol;
  return (
    <div className="border-b border-line/50" data-testid="console-request">
      <button className="flex min-h-[28px] w-full items-center gap-2 px-3 py-0.5 text-left hover:bg-hover/60" onClick={() => setOpen(!open)} aria-expanded={open}>
        <Time at={line.at} />
        <ChevronRight size={12} className={cx("shrink-0 text-faint transition-transform", open && "rotate-90")} />
        <span className="max-w-[36%] shrink-0 truncate font-mono text-[11px] font-bold" style={{ color: item.method ? methodColor(item.method) : "var(--muted)" }} title={label}>
          {label}
        </span>
        {item.method && item.protocol !== "HTTP" && <span className="shrink-0 rounded bg-hover px-1 text-[10px] font-semibold text-muted">{item.protocol}</span>}
        <span className={cx("min-w-0 flex-1 truncate font-mono text-[12px]", item.error ? "text-danger" : "text-fg")} title={item.url}>
          {item.url}
        </span>
        {item.origin && <span className="shrink-0 rounded bg-accent/12 px-1 text-[10px] font-semibold text-accent">{item.origin}</span>}
        <Origin line={line} />
        {item.status != null ? (
          <span className={cx("shrink-0 font-mono text-[12px] font-semibold", toneText[statusTone(item.status)])}>{item.status}</span>
        ) : item.error ? (
          <span className="shrink-0 font-mono text-[11px] font-bold text-danger">ERROR</span>
        ) : item.statusText ? (
          <span className="max-w-[30%] shrink-0 truncate text-[11.5px] text-success" title={item.statusText}>
            {item.statusText}
          </span>
        ) : null}
        <span className="w-14 shrink-0 text-right text-[11.5px] tabular-nums text-faint">{item.durationMs != null ? formatMs(item.durationMs) : ""}</span>
        <span className="w-14 shrink-0 text-right text-[11.5px] tabular-nums text-faint">{item.size != null ? formatBytes(item.size) : ""}</span>
      </button>
      {!open && item.error && <div className="selectable truncate px-3 pb-1 pl-[116px] text-[11.5px] text-danger">{item.error}</div>}
      {open && <Details id={line.id} item={item} />}
    </div>
  );
});

function Details({ id, item }: { id: number; item: Extract<ConsoleItem, { type: "exchange" }> }) {
  // What it sent and got, fetched when the line opens (the app keeps it, not the UI).
  const [details, setDetails] = useState<ConsoleDetails | "loading" | "gone">("loading");
  useEffect(() => {
    let current = true;
    api
      .consoleDetails(id)
      .then((d) => current && setDetails(d ?? "gone"))
      .catch(() => current && setDetails("gone"));
    return () => {
      current = false;
    };
  }, [id]);
  const res = details === "loading" || details === "gone" ? null : details.response;
  return (
    <div className="selectable flex flex-col gap-2.5 bg-panel-2/40 px-3 pb-3 pl-[116px] pt-1.5 text-[12px]" data-testid="console-details">
      <div className="break-all font-mono text-[11.5px] text-muted">
        {item.method} {item.url}
      </div>
      {item.error && <div className="whitespace-pre-wrap break-words text-danger">{item.error}</div>}
      {details === "loading" ? (
        <div className="text-faint">Loading…</div>
      ) : details === "gone" ? (
        <div className="text-faint">No longer kept: the console keeps the newest lines. Send it again to see what it sent and got.</div>
      ) : (
        <>
          <Network details={details} />
          <Section title="Request headers" note={details.prepared ? "as prepared: nothing came back, so the headers added while sending are not known" : undefined}>
            <Headers headers={details.requestHeaders} />
          </Section>
          {details.requestBody && (
            <Section title="Request body" note={formatBytes(details.requestBody.size)}>
              <Body body={details.requestBody} />
            </Section>
          )}
          {res ? (
            <>
              {res.status > 0 && (
                <Section title="Response">
                  <div className="flex flex-wrap items-center gap-x-3 font-mono text-[11.5px]">
                    <span className={cx("font-semibold", toneText[statusTone(res.status)])}>
                      {res.status} {res.statusText}
                    </span>
                    <span className="text-faint">{res.httpVersion}</span>
                  </div>
                </Section>
              )}
              <Section title="Response headers">
                <Headers headers={res.headers} />
              </Section>
              {res.body && (
                <Section title="Response body" note={formatBytes(res.body.size)}>
                  <Body body={res.body} />
                </Section>
              )}
            </>
          ) : (
            item.error && <div className="text-muted">No response.</div>
          )}
        </>
      )}
    </div>
  );
}

function Section({ title, note, children }: { title: string; note?: string; children: React.ReactNode }) {
  return (
    <div className="flex flex-col gap-1">
      <div className="text-[11px] font-semibold uppercase tracking-wide text-faint">
        {title}
        {note && <span className="ml-2 font-normal normal-case tracking-normal">{note}</span>}
      </div>
      {children}
    </div>
  );
}

function Headers({ headers }: { headers: Header[] }) {
  if (!headers.length) return <div className="text-faint">None</div>;
  return (
    <div className="overflow-hidden rounded-lg border border-line/60 bg-bg">
      <HeaderTable headers={headers} />
    </div>
  );
}

function Body({ body }: { body: ConsoleBody }) {
  const pretty = useMemo(() => (body.text == null ? null : prettyMessage(body.text)), [body.text]);
  if (body.text == null) return <div className="text-faint">Binary data ({formatBytes(body.size)}), not shown.</div>;
  const text = pretty ?? body.text;
  return (
    <div className="flex flex-col gap-1">
      <div className="group relative">
        <pre className="max-h-80 overflow-auto whitespace-pre-wrap break-all rounded-lg border border-line/60 bg-bg px-2 py-1.5 font-mono text-[11.5px] text-fg">{text}</pre>
        <button
          aria-label="Copy body"
          onClick={() => void copyText(body.text ?? "")}
          className="absolute right-1.5 top-1.5 rounded bg-panel-2 p-1 text-faint opacity-0 hover:text-fg group-hover:opacity-100"
        >
          <Copy size={12} />
        </button>
      </div>
      {body.truncated && <span className="text-[11px] text-faint">Only the first 64 KB are kept here.</span>}
    </div>
  );
}

/** Where it went and how long each part took. */
function Network({ details: d }: { details: ConsoleDetails }) {
  const t = d.timing;
  const parts: [string, string][] = [];
  if (d.remoteAddr) parts.push(["Address", d.remoteAddr]);
  if (d.proxy) parts.push(["Proxy", d.proxy]);
  if (d.tls) parts.push(["TLS", [d.tls.version, d.tls.cipher, d.tls.alpn].filter(Boolean).join(" · ")]);
  if (d.finalUrl) parts.push(["Final URL", d.finalUrl]);
  if (t) {
    const phases: [string, number][] = [
      ["redirects", t.redirectMs],
      ["DNS", t.dnsMs],
      ["connect", t.connectMs],
      ["TLS", t.tlsMs],
      ["first byte", t.ttfbMs],
      ["download", t.downloadMs],
    ];
    const text = phases.filter(([, ms]) => ms > 0).map(([name, ms]) => `${name} ${formatMs(ms)}`);
    if (text.length) parts.push(["Timing", `${text.join(" · ")} · total ${formatMs(t.totalMs)}`]);
  }
  if (!parts.length && !d.redirects.length) return null;
  return (
    <Section title="Network">
      <div className="flex flex-col gap-0.5 font-mono text-[11.5px]">
        {parts.map(([name, value]) => (
          <div key={name} className="flex gap-2">
            <span className="w-20 shrink-0 text-faint">{name}</span>
            <span className="min-w-0 break-all text-muted">{value}</span>
          </div>
        ))}
        {d.redirects.map((hop, i) => (
          <div key={i} className="flex gap-2">
            <span className="w-20 shrink-0 text-faint">{i === 0 ? "Redirects" : ""}</span>
            <span className="min-w-0 break-all text-muted">
              {hop.status} {hop.method} {hop.url} → {hop.location}
            </span>
          </div>
        ))}
      </div>
    </Section>
  );
}

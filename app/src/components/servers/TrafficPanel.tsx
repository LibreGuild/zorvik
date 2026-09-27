// Live traffic of a server: counters, a filterable log, and a composer to
// send to one client or all of them.
import { memo, useEffect, useMemo, useRef, useState } from "react";
import {
  ArrowDownLeft,
  ArrowLeftRight,
  ArrowUpRight,
  Ban,
  ChevronRight,
  CircleAlert,
  Copy,
  Globe,
  Info,
  Link2,
  Link2Off,
  Search,
  Send,
  Trash2,
  Unplug,
} from "lucide-react";
import type { OutgoingMessage } from "../../bindings/OutgoingMessage";
import type { RunningServerInfo } from "../../bindings/RunningServerInfo";
import type { Server } from "../../bindings/Server";
import type { TrafficEntry } from "../../bindings/TrafficEntry";
import { formatBytes, formatClock } from "../../lib/format";
import { copyText, modKey } from "../../lib/platform";
import { api, errorMessage } from "../../lib/rpc";
import { clearServerLog, serverKey, useServers } from "../../store/servers";
import { hexToBase64 } from "../../store/tabs";
import { toast } from "../../store/toasts";
import { useWorkspace } from "../../store/workspace";
import { beautifyJson } from "../request/jsonFormat";
import { Button, cx, EmptyState, IconButton, Segmented } from "../ui";
import { SERVER_KINDS } from "./kinds";

const EMPTY: TrafficEntry[] = [];

const plural = (n: number, word: string) => (n === 1 ? word : word === "query" ? "queries" : `${word}s`);

export function TrafficPanel({ serverId, server, running }: { serverId: string; server: Server; running?: RunningServerInfo }) {
  const wsPath = useWorkspace((s) => s.info?.path ?? "");
  const entries = useServers((s) => s.logs[serverKey(wsPath, serverId)] ?? EMPTY);
  const [filter, setFilter] = useState("");
  const [target, setTarget] = useState<number | null>(null);
  const stats = running?.stats;

  const visible = useMemo(() => {
    const q = filter.trim();
    if (!q) return entries;
    // Runs again with every batch of traffic: match case-insensitively without copying each
    // payload (up to 64 KB) into lower case.
    const re = new RegExp(q.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"), "i");
    return entries.filter(
      (e) => re.test(e.summary) || (e.text != null && re.test(e.text)) || (e.peer != null && re.test(e.peer)) || (e.conn != null && `#${e.conn}` === q),
    );
  }, [entries, filter]);

  // Connections still open (from the log): targets for the composer.
  const open = useMemo(() => {
    const conns = new Map<number, string>();
    for (const e of entries) {
      if (e.conn == null) continue;
      if (e.kind === "open") conns.set(e.conn, e.peer ?? "");
      else if (e.kind === "close") conns.delete(e.conn);
    }
    return conns;
  }, [entries]);
  useEffect(() => {
    if (target !== null && !open.has(target)) setTarget(null);
  }, [open, target]);

  const kindInfo = SERVER_KINDS[server.kind];
  // Relays send to the client (to inject data into the conversation).
  const canSend = !!running && kindInfo.connections;

  return (
    <div className="flex h-full min-h-0 flex-col" data-testid="traffic-panel">
      <div className="flex h-11 shrink-0 items-center gap-3 px-3 pt-1">
        <span className="text-[12.5px] font-medium text-fg">Traffic</span>
        <div className="flex-1" />
        <div className="flex h-7 w-44 items-center gap-1.5 rounded-lg border border-transparent bg-panel-2 px-2 focus-within:border-accent">
          <Search size={12} className="text-faint" />
          <input
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            placeholder="Filter traffic"
            aria-label="Filter traffic"
            className="min-w-0 flex-1 bg-transparent text-[12px] outline-none placeholder:text-faint"
          />
        </div>
        <IconButton label="Clear traffic" onClick={() => void clearServerLog(serverId)}>
          <Trash2 size={14} />
        </IconButton>
      </div>
      <div className="flex shrink-0 flex-wrap gap-x-3 gap-y-0.5 px-3 pb-2 text-[11.5px] text-muted" data-testid="server-stats">
        {stats ? (
          <>
            {kindInfo.connections && (
              <span>
                <b className="font-semibold text-fg">{stats.connectionsOpen}</b> open · {stats.connectionsTotal} total
              </span>
            )}
            <span>
              <b className="font-semibold text-fg">{stats.requests}</b> {plural(stats.requests, server.kind === "http" ? "request" : server.kind === "dns" ? "query" : "message")}
            </span>
            <span>
              ↓ {formatBytes(stats.bytesIn)} · ↑ {formatBytes(stats.bytesOut)}
            </span>
            {stats.errors > 0 && <span className="text-danger">{stats.errors} errors</span>}
          </>
        ) : (
          <span className="text-faint">{entries.length ? "Stopped · showing the last run" : "Start the server to see traffic"}</span>
        )}
      </div>
      <TrafficLog
        entries={visible}
        empty={entries.length === 0}
        runId={running?.runId}
        canReply={canSend}
        // Stable: the panel re-renders with every stats update, the log's rows must not.
        onReplyTo={setTarget}
      />
      {canSend && running && <Composer runId={running.runId} server={server} target={target} setTarget={setTarget} open={open} />}
    </div>
  );
}

const TrafficLog = memo(function TrafficLog({
  entries,
  empty,
  runId,
  canReply,
  onReplyTo,
}: {
  entries: TrafficEntry[];
  empty: boolean;
  runId?: string;
  canReply: boolean;
  onReplyTo: (conn: number) => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const stick = useRef(true);
  useEffect(() => {
    const el = ref.current;
    if (el && stick.current) el.scrollTop = el.scrollHeight;
  }, [entries]);
  if (empty) {
    return (
      <div className="min-h-0 flex-1">
        <EmptyState icon={<Ban size={26} strokeWidth={1.5} />} title="No traffic yet">
          Connections, messages and requests show up here as they happen.
        </EmptyState>
      </div>
    );
  }
  return (
    <div
      ref={ref}
      onScroll={(e) => {
        const el = e.currentTarget;
        stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
      }}
      className="mx-3 mb-3 min-h-0 flex-1 overflow-auto rounded-lg border border-line font-mono text-[12px]"
      data-testid="traffic-log"
    >
      {entries.map((e) => (
        <TrafficRow key={e.id} e={e} runId={runId} canReply={canReply} onReplyTo={onReplyTo} />
      ))}
    </div>
  );
});

function toHex(b64: string): string {
  try {
    return [...atob(b64)].map((c) => c.charCodeAt(0).toString(16).padStart(2, "0")).join(" ");
  } catch {
    return b64;
  }
}

/** The start of a payload for its one-line row (the details show all of it). */
function preview(e: TrafficEntry): string {
  if (e.text != null) return e.text.slice(0, 300);
  // 400 base64 characters are 300 bytes.
  return e.base64 ? toHex(e.base64.slice(0, 400)) : "";
}

function entryIcon(e: TrafficEntry) {
  switch (e.kind) {
    case "open":
      return <Link2 size={13} className="text-success" />;
    case "close":
      return <Link2Off size={13} className="text-faint" />;
    case "error":
      return <CircleAlert size={13} className="text-danger" />;
    case "info":
      return <Info size={13} className="text-faint" />;
    case "http":
    case "dns":
      return <Globe size={13} className="text-info" />;
    default:
      if (e.direction === "in") return <ArrowDownLeft size={13} className="text-info" />;
      if (e.direction === "out") return <ArrowUpRight size={13} className="text-success" />;
      return <ArrowLeftRight size={13} className={e.direction === "toTarget" ? "text-success" : "text-info"} />;
  }
}

const DIRECTION_LABEL: Record<string, string> = { toTarget: "→ target", fromTarget: "← target" };

const TrafficRow = memo(function TrafficRow({
  e,
  runId,
  canReply,
  onReplyTo,
}: {
  e: TrafficEntry;
  runId?: string;
  canReply: boolean;
  onReplyTo: (conn: number) => void;
}) {
  const [open, setOpen] = useState(false);
  const expandable = e.kind === "data" || e.kind === "http" || e.kind === "dns";
  const status = e.http?.status;
  // HTTP rows show the status as a colored badge and the time on the right, not in the text.
  const line = e.http && status ? `${e.http.method} ${e.http.path}` : e.kind === "data" ? preview(e) || e.summary : e.summary;
  return (
    <div
      className={cx("group/row border-b border-line/60 last:border-b-0", e.kind === "error" && "bg-danger/5")}
      style={{ contentVisibility: "auto", containIntrinsicSize: "auto 30px" }}
    >
      <div className="flex w-full items-center gap-2 px-3 py-1.5">
        <button
          onClick={() => expandable && setOpen(!open)}
          className={cx("flex min-w-0 flex-1 items-center gap-2 text-left", expandable && "cursor-pointer")}
          aria-expanded={expandable ? open : undefined}
        >
          {expandable ? <ChevronRight size={11} className={cx("shrink-0 text-faint transition-transform", open && "rotate-90")} /> : <span className="w-[11px] shrink-0" />}
          {entryIcon(e)}
          <span className="shrink-0 text-[11px] tabular-nums text-faint">{formatClock(e.timestamp)}</span>
          {e.conn != null && <span className="shrink-0 rounded bg-hover px-1.5 text-[10.5px] font-semibold text-muted">#{e.conn}</span>}
          {e.direction && DIRECTION_LABEL[e.direction] && <span className="shrink-0 text-[10.5px] text-faint">{DIRECTION_LABEL[e.direction]}</span>}
          {e.kind === "data" && e.summary && <span className="shrink-0 rounded bg-accent-soft px-1.5 text-[10.5px] font-semibold text-accent">{e.summary}</span>}
          {status != null && status > 0 && (
            <span className={cx("shrink-0 text-[11px] font-semibold", status < 300 ? "text-success" : status < 400 ? "text-info" : status < 500 ? "text-warning" : "text-danger")}>{status}</span>
          )}
          <span className={cx("min-w-0 flex-1 truncate", e.kind === "data" || e.kind === "http" || e.kind === "dns" ? "text-fg" : "font-sans text-muted")}>
            {line || <i className="text-faint">empty</i>}
          </span>
          {e.kind === "data" && <span className="shrink-0 text-[11px] text-faint">{formatBytes(e.size)}</span>}
          {e.http && status ? <span className="shrink-0 text-[11px] tabular-nums text-faint">{Math.round(e.http.durationMs)} ms</span> : null}
        </button>
        {canReply && runId && e.conn != null && e.kind !== "close" && (
          <span className="flex shrink-0 gap-0.5 opacity-0 focus-within:opacity-100 group-hover/row:opacity-100">
            <button aria-label={`Reply to #${e.conn}`} title={`Send to #${e.conn}`} onClick={() => onReplyTo(e.conn!)} className="rounded p-1 text-faint hover:bg-hover hover:text-fg">
              <Send size={12} />
            </button>
            <button
              aria-label={`Disconnect #${e.conn}`}
              title={`Disconnect #${e.conn}`}
              onClick={() => void api.serverDisconnect(runId, e.conn!).catch(() => {})}
              className="rounded p-1 text-faint hover:bg-hover hover:text-danger"
            >
              <Unplug size={12} />
            </button>
          </span>
        )}
      </div>
      {open && <EntryDetails e={e} />}
    </div>
  );
});

function Block({ title, text }: { title: string; text: string }) {
  const pretty = useMemo(() => beautifyJson(text) ?? text, [text]);
  return (
    <div className="group relative">
      <div className="mb-1 font-sans text-[11px] font-semibold uppercase tracking-wide text-faint">{title}</div>
      <pre className="selectable max-h-72 overflow-auto whitespace-pre-wrap break-all text-fg">{pretty || <i className="text-faint">empty</i>}</pre>
      {text && (
        <button aria-label={`Copy ${title}`} onClick={() => void copyText(text)} className="absolute right-0 top-0 rounded p-1 text-faint opacity-0 hover:bg-hover hover:text-fg group-hover:opacity-100">
          <Copy size={12} />
        </button>
      )}
    </div>
  );
}

function EntryDetails({ e }: { e: TrafficEntry }) {
  const h = e.http;
  const payload = useMemo(() => e.text ?? (e.base64 ? toHex(e.base64) : ""), [e]);
  return (
    <div className="flex flex-col gap-3 bg-panel-2/60 px-8 py-2.5">
      {e.peer && <div className="font-sans text-[11px] text-faint">From {e.peer}</div>}
      {h ? (
        <>
          <div className="font-sans text-[12px] text-muted">
            {h.method} {h.path} · {h.httpVersion} → {h.status || h.note} · {Math.round(h.durationMs)} ms
            {h.route ? ` · route ${h.route}` : " · no route matched"}
            {h.note && h.status ? ` · ${h.note}` : ""}
          </div>
          <Block title="Request headers" text={h.requestHeaders.map((x) => `${x.name}: ${x.value}`).join("\n")} />
          {h.requestBody && <Block title="Request body" text={h.requestBody} />}
          <Block title="Response headers" text={h.responseHeaders.map((x) => `${x.name}: ${x.value}`).join("\n")} />
          {h.responseBody && <Block title="Response body" text={h.responseBody} />}
        </>
      ) : (
        <Block title={e.base64 ? "Payload (hex)" : "Payload"} text={payload} />
      )}
      {e.truncated && <div className="font-sans text-[11px] text-faint">Only the first 64 KB of {formatBytes(e.size)} is shown.</div>}
    </div>
  );
}

function Composer({
  runId,
  server,
  target,
  setTarget,
  open,
}: {
  runId: string;
  server: Server;
  target: number | null;
  setTarget: (t: number | null) => void;
  open: Map<number, string>;
}) {
  const sse = server.kind === "sse";
  const [mode, setMode] = useState<"text" | "hex">("text");
  const [text, setText] = useState("");
  const [event, setEvent] = useState("");
  const [sending, setSending] = useState(false);
  const submit = async () => {
    if (!text || sending) return;
    const base64 = !sse && mode === "hex" ? hexToBase64(text) : null;
    if (!sse && mode === "hex" && base64 === null) {
      toast("error", "Invalid hex", "Enter bytes like 48 65 6c 6c 6f");
      return;
    }
    // Busy while variables render too, so a second Mod+Enter doesn't send twice.
    setSending(true);
    try {
      const message: OutgoingMessage = sse
        ? { type: "event", event, data: text, id: "" }
        : base64 !== null
          ? { type: "binary", base64 }
          : { type: "text", text: text.includes("{{") ? await api.renderVariables(text) : text };
      await api.serverSend(runId, target, message);
    } catch (e) {
      toast("error", "Not sent", errorMessage(e));
    } finally {
      setSending(false);
    }
  };
  return (
    <div className="mx-3 mb-3 flex shrink-0 flex-col gap-2 rounded-xl border border-line bg-input p-2 has-[textarea:focus]:border-accent" data-testid="server-composer">
      <div className="flex items-center gap-2">
        <select
          aria-label="Send to"
          value={target === null ? "all" : String(target)}
          onChange={(e) => setTarget(e.target.value === "all" ? null : Number(e.target.value))}
          className="h-7 rounded-md border border-line bg-panel-2 px-1.5 text-[12px] text-fg outline-none focus:border-accent"
        >
          <option value="all">All clients ({open.size})</option>
          {[...open.entries()].map(([conn, peer]) => (
            <option key={conn} value={conn}>
              #{conn} {peer}
            </option>
          ))}
        </select>
        {sse ? (
          <input
            aria-label="Event name"
            value={event}
            onChange={(e) => setEvent(e.target.value)}
            placeholder="event name (optional)"
            className="h-7 w-44 rounded-md border border-line bg-panel-2 px-2 font-mono text-[12px] outline-none focus:border-accent"
          />
        ) : (
          <Segmented items={[{ id: "text", label: "Text" }, { id: "hex", label: "Hex" }]} value={mode} onChange={setMode} />
        )}
        <div className="flex-1" />
        <span className="text-[11px] text-faint">{modKey}+Enter</span>
        <Button size="sm" variant="primary" icon={<Send size={13} />} disabled={!text || sending} onClick={() => void submit()}>
          Send
        </Button>
      </div>
      <textarea
        aria-label="Message to send"
        value={text}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
            e.preventDefault();
            e.stopPropagation();
            void submit();
          }
        }}
        rows={3}
        spellCheck={false}
        placeholder={sse ? "Event data" : mode === "hex" ? "48 65 6c 6c 6f" : "Message to send"}
        className="min-h-[60px] resize-y rounded-md bg-transparent px-1.5 py-1 font-mono text-[12px] text-fg outline-none placeholder:text-faint"
      />
    </div>
  );
}

// Live message log of WebSocket / SSE / TCP / UDP / MQTT / Socket.IO connections and GraphQL
// subscriptions, with a composer.
import { memo, useEffect, useMemo, useRef, useState } from "react";
import { ArrowDownLeft, ArrowUpRight, Ban, ChevronRight, CircleAlert, Copy, Info, Search, Send, Trash2 } from "lucide-react";
import { formatBytes, formatClock } from "../../lib/format";
import { copyText, modKey } from "../../lib/platform";
import { clearMessages, type StreamMessage, type Tab, updateDraft, wsSend } from "../../store/tabs";
import { useVariableNames } from "../../store/workspace";
import { CodeEditor } from "../CodeEditor";
import { beautifyJson } from "../request/jsonFormat";
import { COMPOSER_EXTRAS } from "../request/kinds";
import { Button, cx, EmptyState, IconButton, Segmented } from "../ui";

function statusDot(status: Tab["stream"]["status"]) {
  const map = {
    idle: ["bg-faint", "Not connected"],
    connecting: ["bg-warning animate-pulse", "Connecting…"],
    open: ["bg-success", "Connected"],
    closed: ["bg-faint", "Disconnected"],
  } as const;
  return map[status];
}

/** Kinds whose messages are typed in the composer (SSE and subscriptions only receive). */
const COMPOSER_KINDS = ["websocket", "tcp", "udp", "mqtt", "socketio"];

export function StreamPane({ tab }: { tab: Tab }) {
  const kind = tab.draft.kind ?? "http";
  const isWs = COMPOSER_KINDS.includes(kind);
  const { status, messages } = tab.stream;
  const [filter, setFilter] = useState("");
  const [dot, label] = statusDot(status);
  const sent = messages.filter((m) => m.direction === "sent").length;
  const received = messages.filter((m) => m.direction === "received").length;

  const visible = useMemo(() => {
    const q = filter.trim().toLowerCase();
    if (!q) return messages;
    return messages.filter(
      (m) => (m.text ?? "").toLowerCase().includes(q) || m.kind.toLowerCase().includes(q) || (m.topic ?? "").toLowerCase().includes(q) || (m.peer ?? "").includes(q),
    );
  }, [messages, filter]);

  return (
    <div className="flex h-full min-h-0 flex-col" data-testid="stream-pane">
      <div className="flex h-11 shrink-0 items-center gap-3 px-3 pt-1">
        <span className={cx("h-2 w-2 rounded-full", dot)} />
        <span className="whitespace-nowrap text-[12.5px] font-medium text-fg" data-testid="stream-status">
          {label}
        </span>
        <span className="truncate whitespace-nowrap text-[12px] text-muted">
          {isWs ? `${sent} sent · ${received} received` : `${received} ${kind === "http" ? "results" : "events"}`}
        </span>
        <div className="flex-1" />
        <div className="flex h-7 w-52 items-center gap-1.5 rounded-lg border border-transparent bg-panel-2 px-2 focus-within:border-accent">
          <Search size={12} className="text-faint" />
          <input
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            placeholder="Filter messages"
            className="min-w-0 flex-1 bg-transparent text-[12px] outline-none placeholder:text-faint"
          />
        </div>
        <IconButton label="Clear messages" onClick={() => clearMessages(tab.id)}>
          <Trash2 size={14} />
        </IconButton>
      </div>
      <MessageLog messages={visible} empty={messages.length === 0} isWs={isWs} kind={kind} />
      {isWs && <Composer tab={tab} />}
    </div>
  );
}

// Memoized: the tab (and so StreamPane) re-renders on every keystroke in the composer; the log
// only needs to when its messages change.
const EMPTY_HINT: Record<string, string> = {
  websocket: "Connect, then send messages below. Incoming messages appear here.",
  tcp: "Connect, then send messages below. Bytes from the server appear here.",
  udp: "Send a datagram below. Replies from the host appear here.",
  mqtt: "Connect to receive messages on your subscriptions, and publish below.",
  socketio: "Connect, then emit events below. Events from the server appear here.",
  http: "Subscribe to receive the subscription's results here.",
};

const MessageLog = memo(function MessageLog({ messages, empty, isWs, kind }: { messages: StreamMessage[]; empty: boolean; isWs: boolean; kind: string }) {
  const ref = useRef<HTMLDivElement>(null);
  const stick = useRef(true);
  useEffect(() => {
    const el = ref.current;
    if (el && stick.current) el.scrollTop = el.scrollHeight;
  }, [messages]);
  if (empty) {
    return (
      <div className="min-h-0 flex-1">
        <EmptyState icon={<Ban size={26} strokeWidth={1.5} />} title="No messages yet">
          {EMPTY_HINT[kind] ?? "Connect to start receiving events."}
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
      className="mx-3 min-h-0 flex-1 overflow-auto rounded-lg border border-line font-mono text-[12px]"
      data-testid="message-log"
    >
      {messages.map((m) => (
        <MessageRow key={m.id} m={m} isWs={isWs} sse={kind === "sse"} />
      ))}
    </div>
  );
});

function decodeBase64(b64: string): string {
  try {
    const bin = atob(b64);
    return [...bin].map((c) => c.charCodeAt(0).toString(16).padStart(2, "0")).join(" ");
  } catch {
    return b64;
  }
}

/** A Socket.IO acknowledgement note in a few words: "ack #3" or "wants ack #3". */
function ackChip(detail: string | null | undefined): string | null {
  const id = detail?.match(/#(\d+)/)?.[1];
  if (!id) return null;
  if (detail?.startsWith("acknowledgement")) return `ack #${id}`;
  if (detail?.startsWith("asks for")) return `wants ack #${id}`;
  return null;
}

const MessageRow = memo(function MessageRow({ m, isWs, sse }: { m: StreamMessage; isWs: boolean; sse: boolean }) {
  const [open, setOpen] = useState(false);
  // The one-line row shows the start: a TCP message can be megabytes (400 base64 characters are 300 bytes).
  const line = m.text != null ? m.text.slice(0, 300) : m.base64 ? decodeBase64(m.base64.slice(0, 400)) : "";
  const content = useMemo(() => (open ? (m.text ?? (m.base64 ? decodeBase64(m.base64) : "")) : ""), [open, m.text, m.base64]);
  // Token-based, so large integer ids aren't rounded the way JSON.parse would.
  const pretty = useMemo(() => (open && m.text ? beautifyJson(m.text) : null), [open, m.text]);
  const icon =
    m.direction === "sent" ? (
      <ArrowUpRight size={13} className="text-success" />
    ) : m.direction === "received" ? (
      <ArrowDownLeft size={13} className="text-info" />
    ) : m.direction === "error" ? (
      <CircleAlert size={13} className="text-danger" />
    ) : (
      <Info size={13} className="text-faint" />
    );
  const meta = m.direction === "info" || m.direction === "error";
  return (
    <div className={cx("border-b border-line/60 last:border-b-0", m.direction === "error" && "bg-danger/5")} style={{ contentVisibility: "auto", containIntrinsicSize: "auto 30px" }}>
      <button
        onClick={() => !meta && setOpen(!open)}
        className={cx("flex w-full items-center gap-2 px-3 py-1.5 text-left", !meta && "hover:bg-hover/40")}
      >
        {!meta ? <ChevronRight size={11} className={cx("shrink-0 text-faint transition-transform", open && "rotate-90")} /> : <span className="w-[11px]" />}
        {icon}
        <span className="shrink-0 text-[11px] tabular-nums text-faint">{formatClock(m.timestamp)}</span>
        {sse && m.direction === "received" && (
          <span className="shrink-0 rounded bg-accent-soft px-1.5 text-[10.5px] font-semibold text-accent">{m.kind}</span>
        )}
        {isWs && (m.kind === "binary" || m.kind === "ping" || m.kind === "pong") && (
          <span className="shrink-0 rounded bg-hover px-1.5 text-[10.5px] font-semibold uppercase text-muted">{m.kind}</span>
        )}
        {m.topic && <span className="max-w-[40%] shrink-0 truncate rounded bg-accent-soft px-1.5 text-[10.5px] font-semibold text-accent" title={m.topic}>{m.topic}</span>}
        {ackChip(m.detail) && <span className="shrink-0 rounded bg-hover px-1.5 text-[10.5px] font-semibold text-muted">{ackChip(m.detail)}</span>}
        {m.peer && <span className="shrink-0 text-[11px] text-faint">{m.peer}</span>}
        <span className={cx("min-w-0 flex-1 truncate", meta ? "font-sans text-muted" : "text-fg")}>{line || <i className="text-faint">empty</i>}</span>
        {!meta && <span className="shrink-0 text-[11px] text-faint">{formatBytes(m.size)}</span>}
      </button>
      {open && (
        <div className="group relative bg-panel-2/60 px-8 py-2">
          {m.eventId && <div className="mb-1 font-sans text-[11px] text-faint">id: {m.eventId}</div>}
          {m.detail && <div className="mb-1 font-sans text-[11px] text-faint">{m.detail}</div>}
          <pre className="selectable max-h-80 overflow-auto whitespace-pre-wrap break-all text-fg">{pretty ?? content}</pre>
          <button
            aria-label="Copy message"
            onClick={() => void copyText(content)}
            className="absolute right-2 top-2 rounded p-1 text-faint opacity-0 hover:bg-hover hover:text-fg group-hover:opacity-100"
          >
            <Copy size={12} />
          </button>
        </div>
      )}
    </div>
  );
});

function Composer({ tab }: { tab: Tab }) {
  const { names } = useVariableNames();
  const Extra = COMPOSER_EXTRAS[tab.draft.kind ?? "http"];
  const [mode, setMode] = useState<"text" | "json" | "binary">(() => (tab.draft.body?.type === "json" ? "json" : "text"));
  const text = tab.draft.body?.text ?? "";
  const open = tab.stream.status === "open";
  // A Socket.IO event may have no arguments.
  const ready = open && (text.length > 0 || tab.draft.kind === "socketio");
  const submit = () => {
    if (ready) void wsSend(tab.id, text, mode === "binary");
  };
  return (
    <div className="m-3 flex h-48 shrink-0 flex-col overflow-hidden rounded-xl border border-line bg-input">
      <div className="flex min-h-9 flex-wrap items-center gap-x-2 gap-y-1 px-3 py-1">
        {Extra && <Extra tab={tab} />}
        <Segmented
          items={[
            { id: "text", label: "Text" },
            { id: "json", label: "JSON" },
            { id: "binary", label: "Binary (hex)" },
          ]}
          value={mode}
          onChange={(m) => {
            setMode(m);
            updateDraft(tab.id, (r) => ({ ...r, body: { ...(r.body ?? { type: "text" }), type: m === "json" ? "json" : "text" } }));
          }}
        />
        <div className="flex-1" />
        <span className="text-[11px] text-faint">
          {modKey}+Enter
        </span>
        <Button size="sm" variant="primary" icon={<Send size={13} />} disabled={!ready} onClick={submit}>
          {tab.draft.kind === "socketio" ? "Emit" : "Send"}
        </Button>
      </div>
      <div className="min-h-0 flex-1">
        <CodeEditor
          value={text}
          onChange={(v) => updateDraft(tab.id, (r) => ({ ...r, body: { ...(r.body ?? { type: "text" }), text: v } }))}
          language={mode === "json" ? "json" : "text"}
          variables={names}
          onSubmit={submit}
          lineNumbers={false}
          placeholder={
            mode === "binary"
              ? "48 65 6c 6c 6f"
              : tab.draft.kind === "socketio"
                ? mode === "json"
                  ? 'Arguments as JSON, e.g. ["hi", {"n": 1}]'
                  : "Text, sent as one argument"
                : "Message to send"
          }
        />
      </div>
    </div>
  );
}

// The message log of a live connection: one row per message (expand for the whole payload),
// reply times paired by the request's reply key, times as clock or deltas, and a "new messages"
// button while scrolled up.
import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ArrowDown, ArrowDownLeft, ArrowUpRight, Ban, ChevronRight, CircleAlert, Copy, Download, Info, Pencil, Reply } from "lucide-react";
import { formatBytes, formatClock, formatMs } from "../../lib/format";
import type { ReplyPair } from "../../lib/messageMatch";
import { copyText, pickSavePath } from "../../lib/platform";
import { api, errorMessage } from "../../lib/rpc";
import type { StreamMessage } from "../../store/tabs";
import { toast } from "../../store/toasts";
import { beautifyJson } from "../request/jsonFormat";
import { cx, EmptyState } from "../ui";

export type TimeMode = "clock" | "delta" | "elapsed";

const EMPTY_HINT: Record<string, string> = {
  websocket: "Connect, then send messages below. Incoming messages appear here.",
  tcp: "Connect, then send messages below. Bytes from the server appear here.",
  udp: "Send a datagram below. Replies from the host appear here.",
  mqtt: "Connect to receive messages on your subscriptions, and publish below.",
  socketio: "Connect, then emit events below. Events from the server appear here.",
  http: "Subscribe to receive the subscription's results here.",
};

export function decodeBase64(b64: string): string {
  try {
    const bin = atob(b64);
    return [...bin].map((c) => c.charCodeAt(0).toString(16).padStart(2, "0")).join(" ");
  } catch {
    return b64;
  }
}

// The one-line row shows the start: a TCP message can be megabytes (400 base64 characters are
// 300 bytes). Worked out once per message (messages never change), not on every render.
const previews = new WeakMap<StreamMessage, string>();
function preview(m: StreamMessage): string {
  let line = previews.get(m);
  if (line === undefined) {
    line = m.text != null ? m.text.slice(0, 300) : m.base64 ? decodeBase64(m.base64.slice(0, 400)) : "";
    previews.set(m, line);
  }
  return line;
}

/** A Socket.IO acknowledgement note in a few words: "ack #3" or "wants ack #3". */
function ackChip(detail: string | null | undefined): string | null {
  const id = detail?.match(/#(\d+)/)?.[1];
  if (!id) return null;
  if (detail?.startsWith("acknowledgement")) return `ack #${id}`;
  if (detail?.startsWith("asks for")) return `wants ack #${id}`;
  return null;
}

function timeText(mode: TimeMode, m: StreamMessage, previous: StreamMessage | undefined, first: StreamMessage | undefined): string {
  if (mode === "clock" || !first) return formatClock(m.timestamp);
  const from = mode === "delta" ? previous : first;
  if (!from) return "+0 ms";
  return `+${formatMs(Math.max(0, m.timestamp - from.timestamp))}`;
}

interface LogProps {
  messages: StreamMessage[];
  empty: boolean;
  isWs: boolean;
  kind: string;
  time: TimeMode;
  replies: Map<number, ReplyPair>;
  /** Put a sent message back in the composer. */
  onEdit?: (m: StreamMessage) => void;
}

// Memoized: the tab (and so the pane) re-renders on every keystroke in the composer; the log
// only needs to when its messages change.
export const MessageLog = memo(function MessageLog({ messages, empty, isWs, kind, time, replies, onEdit }: LogProps) {
  const ref = useRef<HTMLDivElement>(null);
  const stick = useRef(true);
  const [behind, setBehind] = useState(0);
  // The newest message id seen (ids only grow; the list itself is capped, so its length stops).
  const seen = useRef(0);
  const newest = messages.length ? messages[messages.length - 1].id : 0;
  const [flash, setFlash] = useState<number | null>(null);
  useEffect(() => {
    const el = ref.current;
    if (el && stick.current) {
      el.scrollTop = el.scrollHeight;
      seen.current = newest;
    }
    setBehind(stick.current ? 0 : messages.filter((m) => m.id > seen.current).length);
  }, [messages, newest]);
  // Stable, so the memoized rows don't all re-render when a message arrives.
  const jump = useCallback((id: number) => {
    ref.current?.querySelector(`[data-msg-id="${id}"]`)?.scrollIntoView({ block: "center" });
    setFlash(id);
    setTimeout(() => setFlash((f) => (f === id ? null : f)), 1200);
  }, []);
  const toEnd = () => {
    const el = ref.current;
    if (!el) return;
    stick.current = true;
    seen.current = newest;
    setBehind(0);
    el.scrollTop = el.scrollHeight;
  };
  if (empty) {
    return (
      <div className="min-h-0 flex-1">
        <EmptyState icon={<Ban size={26} strokeWidth={1.5} />} title="No messages yet">
          {EMPTY_HINT[kind] ?? "Connect to start receiving events."}
        </EmptyState>
      </div>
    );
  }
  const first = messages[0];
  return (
    <div className="relative mx-3 min-h-0 flex-1">
      <div
        ref={ref}
        onScroll={(e) => {
          const el = e.currentTarget;
          const atEnd = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
          if (atEnd && !stick.current) {
            seen.current = newest;
            setBehind(0);
          }
          if (!atEnd && stick.current) seen.current = newest;
          stick.current = atEnd;
        }}
        className="h-full overflow-auto rounded-lg border border-line font-mono text-[12px]"
        data-testid="message-log"
      >
        {messages.map((m, i) => (
          <MessageRow
            key={m.id}
            m={m}
            isWs={isWs}
            sse={kind === "sse"}
            time={timeText(time, m, messages[i - 1], first)}
            reply={replies.get(m.id)}
            onJump={jump}
            flash={flash === m.id}
            onEdit={onEdit && m.direction === "sent" ? onEdit : undefined}
          />
        ))}
      </div>
      {behind > 0 && (
        <button
          onClick={toEnd}
          className="absolute bottom-3 left-1/2 flex -translate-x-1/2 items-center gap-1.5 rounded-full border border-line bg-elev px-3 py-1 font-sans text-[11.5px] font-medium text-fg shadow-pop hover:bg-hover"
        >
          <ArrowDown size={12} />
          {behind} new {behind === 1 ? "message" : "messages"}
        </button>
      )}
    </div>
  );
});

async function saveBinary(m: StreamMessage) {
  if (!m.base64) return;
  const path = await pickSavePath("Save the message", "message.bin");
  if (!path) return;
  try {
    await api.saveFile(path, { base64: m.base64 });
    toast("success", "Saved", path);
  } catch (e) {
    toast("error", "Could not save", errorMessage(e));
  }
}

const MessageRow = memo(function MessageRow({
  m,
  isWs,
  sse,
  time,
  reply,
  onJump,
  flash,
  onEdit,
}: {
  m: StreamMessage;
  isWs: boolean;
  sse: boolean;
  time: string;
  reply?: ReplyPair;
  onJump: (id: number) => void;
  flash: boolean;
  onEdit?: (m: StreamMessage) => void;
}) {
  const [open, setOpen] = useState(false);
  const line = preview(m);
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
  const chip = ackChip(m.detail);
  return (
    <div
      data-msg-id={m.id}
      className={cx("border-b border-line/60 transition-colors last:border-b-0", m.direction === "error" && "bg-danger/5", flash && "bg-accent-soft")}
      style={{ contentVisibility: "auto", containIntrinsicSize: "auto 30px" }}
    >
      <div className={cx("flex w-full items-center gap-2 px-3 py-1.5", !meta && "hover:bg-hover/40")}>
        <button
          onClick={() => !meta && setOpen(!open)}
          aria-expanded={meta ? undefined : open}
          className="flex min-w-0 flex-1 items-center gap-2 text-left"
        >
          {!meta ? <ChevronRight size={11} className={cx("shrink-0 text-faint transition-transform", open && "rotate-90")} /> : <span className="w-[11px]" />}
          {icon}
          <span className="shrink-0 text-[11px] tabular-nums text-faint">{time}</span>
          {sse && m.direction === "received" && (
            <span className="shrink-0 rounded bg-accent-soft px-1.5 text-[10.5px] font-semibold text-accent">{m.kind}</span>
          )}
          {isWs && (m.kind === "binary" || m.kind === "ping" || m.kind === "pong") && (
            <span className="shrink-0 rounded bg-hover px-1.5 text-[10.5px] font-semibold uppercase text-muted">{m.kind}</span>
          )}
          {m.topic && (
            <span className="max-w-[40%] shrink-0 truncate rounded bg-accent-soft px-1.5 text-[10.5px] font-semibold text-accent" title={m.topic}>
              {m.topic}
            </span>
          )}
          {chip && <span className="shrink-0 rounded bg-hover px-1.5 text-[10.5px] font-semibold text-muted">{chip}</span>}
          {m.peer && <span className="shrink-0 text-[11px] text-faint">{m.peer}</span>}
          <span className={cx("min-w-0 flex-1 truncate", meta ? "font-sans text-muted" : "text-fg")}>{line || <i className="text-faint">empty</i>}</span>
        </button>
        {reply && (
          <button
            onClick={() => onJump(reply.to)}
            title="A reply to a message you sent (same reply key, or the acknowledgement of an emit): show that message"
            className="flex shrink-0 items-center gap-1 rounded bg-success/10 px-1.5 font-sans text-[10.5px] font-semibold tabular-nums text-success hover:bg-success/20"
            data-testid="reply-time"
          >
            <Reply size={11} />
            {reply.ms < 1 ? "<1 ms" : formatMs(reply.ms)}
          </button>
        )}
        {!meta && <span className="shrink-0 text-[11px] text-faint">{formatBytes(m.size)}</span>}
      </div>
      {open && (
        <div className="group relative bg-panel-2/60 px-8 py-2">
          {m.eventId && <div className="mb-1 font-sans text-[11px] text-faint">id: {m.eventId}</div>}
          {m.detail && <div className="mb-1 font-sans text-[11px] text-faint">{m.detail}</div>}
          <pre className="selectable max-h-80 overflow-auto whitespace-pre-wrap break-all text-fg">{pretty ?? content}</pre>
          <div className="absolute right-2 top-2 flex gap-0.5 opacity-0 group-hover:opacity-100">
            {onEdit && (
              <button aria-label="Edit in the composer" title="Edit in the composer" onClick={() => onEdit(m)} className="rounded p-1 text-faint hover:bg-hover hover:text-fg">
                <Pencil size={12} />
              </button>
            )}
            {m.base64 && (
              <button aria-label="Save to a file" title="Save to a file" onClick={() => void saveBinary(m)} className="rounded p-1 text-faint hover:bg-hover hover:text-fg">
                <Download size={12} />
              </button>
            )}
            <button aria-label="Copy message" title="Copy" onClick={() => void copyText(content)} className="rounded p-1 text-faint hover:bg-hover hover:text-fg">
              <Copy size={12} />
            </button>
          </div>
        </div>
      )}
    </div>
  );
});

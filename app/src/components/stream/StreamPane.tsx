// Live message log of WebSocket / SSE / TCP / UDP / MQTT / Socket.IO connections and GraphQL
// subscriptions, with a composer.
import { useCallback, useMemo, useRef, useState } from "react";
import { Download, Search, SlidersHorizontal, Trash2 } from "lucide-react";
import { logFilter, pairAcks, pairReplies, parseJson, type ReplyPair, reusePairs } from "../../lib/messageMatch";
import { modKey, pickSavePath } from "../../lib/platform";
import { api, errorMessage } from "../../lib/rpc";
import { loadIntoComposer } from "../../store/composer";
import { clearMessages, type StreamMessage, type Tab } from "../../store/tabs";
import { toast } from "../../store/toasts";
import { cx, IconButton, Menu } from "../ui";
import { Composer } from "./Composer";
import { decodeBase64, MessageLog, type TimeMode } from "./MessageLog";

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

type Show = "all" | "sent" | "received";

const TIME_KEY = "zorvik.log.time";

function rememberedTime(): TimeMode {
  try {
    const t = localStorage.getItem(TIME_KEY);
    return t === "delta" || t === "elapsed" ? t : "clock";
  } catch {
    return "clock";
  }
}

// Parsed once per message (messages never change): the filter and reply pairing read them often.
const parsed = new WeakMap<StreamMessage, unknown>();
function jsonOf(m: StreamMessage): unknown {
  if (!parsed.has(m)) parsed.set(m, parseJson(m.text));
  return parsed.get(m);
}

// What the word filter looks in: the text, or the first bytes of binary as hex (decoded once).
const bodies = new WeakMap<StreamMessage, string | null>();
function bodyOf(m: StreamMessage): string | null {
  if (m.text != null) return m.text;
  let body = bodies.get(m);
  if (body === undefined) {
    body = m.base64 ? decodeBase64(m.base64.slice(0, 4000)) : null;
    bodies.set(m, body);
  }
  return body;
}

async function exportLog(tab: Tab) {
  const path = await pickSavePath("Export the message log", `${tab.draft.name || "messages"}.jsonl`, [{ name: "JSON Lines", extensions: ["jsonl"] }]);
  if (!path) return;
  const lines = tab.stream.messages.map((m) =>
    JSON.stringify({
      time: new Date(m.timestamp).toISOString(),
      direction: m.direction,
      kind: m.kind,
      ...(m.text != null ? { text: m.text } : {}),
      ...(m.base64 ? { base64: m.base64 } : {}),
      size: m.size,
      ...(m.topic ? { topic: m.topic } : {}),
      ...(m.peer ? { peer: m.peer } : {}),
      ...(m.detail ? { detail: m.detail } : {}),
      ...(m.eventId ? { id: m.eventId } : {}),
    }),
  );
  try {
    await api.saveFile(path, { text: lines.join("\n") + "\n" });
    toast("success", `Exported ${lines.length} ${lines.length === 1 ? "message" : "messages"}`, path);
  } catch (e) {
    toast("error", "Could not export the log", errorMessage(e));
  }
}

export function StreamPane({ tab }: { tab: Tab }) {
  const kind = tab.draft.kind ?? "http";
  const isWs = COMPOSER_KINDS.includes(kind);
  const { status, messages } = tab.stream;
  const [filter, setFilter] = useState("");
  const [show, setShow] = useState<Show>("all");
  const [time, setTimeMode] = useState<TimeMode>(rememberedTime);
  const [dot, label] = statusDot(status);
  const sent = messages.filter((m) => m.direction === "sent").length;
  const received = messages.filter((m) => m.direction === "received").length;
  const replyKey = tab.draft.websocket?.replyKey ?? "";

  // Socket.IO events and MQTT topics: one at a time, or some hidden.
  const [only, setOnly] = useState<string | null>(null);
  const [hidden, setHidden] = useState<ReadonlySet<string>>(new Set());
  const topics = useMemo(() => {
    if (kind !== "socketio" && kind !== "mqtt") return [];
    const counts = new Map<string, number>();
    for (const m of messages) if (m.topic) counts.set(m.topic, (counts.get(m.topic) ?? 0) + 1);
    return [...counts.entries()].sort((a, b) => b[1] - a[1]);
  }, [messages, kind]);

  // The filter's answer for each message, kept while the filter text stays (messages never
  // change): a flush tests only the new messages.
  const matches = useMemo(() => {
    const test = logFilter(filter);
    if (!test) return null;
    const words = !filter.trim().startsWith("$");
    const answers = new WeakMap<StreamMessage, boolean>();
    return (m: StreamMessage) => {
      let yes = answers.get(m);
      if (yes === undefined) {
        const meta = [m.kind, m.topic ?? "", m.peer ?? ""].join(" ");
        yes = test(bodyOf(m), () => jsonOf(m)) || (words && test(meta, () => undefined));
        answers.set(m, yes);
      }
      return yes;
    };
  }, [filter]);

  const visible = useMemo(
    () =>
      messages.filter((m) => {
        if (show !== "all" && m.direction !== show) return false;
        if (only !== null && m.topic !== only) return false;
        if (m.topic && hidden.has(m.topic)) return false;
        return !matches || matches(m);
      }),
    [messages, matches, show, only, hidden],
  );

  // Replies paired with the messages they answer (same value at the reply key). Pairs that did
  // not change keep their object, so the log's rows don't re-render on every flush.
  const lastReplies = useRef(new Map<number, ReplyPair>());
  const replies = useMemo(() => {
    let next = new Map<number, ReplyPair>();
    if (kind === "socketio") next = pairAcks(messages);
    else if (kind === "websocket") {
      const byId = new Map(messages.map((m) => [m.id, m]));
      next = pairReplies(messages, (id) => jsonOf(byId.get(id)!), replyKey);
    }
    lastReplies.current = reusePairs(lastReplies.current, next);
    return next;
  }, [messages, kind, replyKey]);

  const setTime = (t: TimeMode) => {
    setTimeMode(t);
    try {
      localStorage.setItem(TIME_KEY, t);
    } catch {
      // Not remembered.
    }
  };
  // Stable, so the log and its rows (memoized) don't re-render on every keystroke in the composer.
  const edit = useCallback(
    (m: StreamMessage) => {
      const binary = m.text == null;
      const json = !binary && jsonOf(m) !== undefined;
      loadIntoComposer(tab.id, { name: "", format: binary ? "binary" : json ? "json" : "text", text: binary ? decodeBase64(m.base64 ?? "") : (m.text ?? "") });
    },
    [tab.id],
  );
  const filtered = filter.trim() !== "" || show !== "all" || only !== null || hidden.size > 0;

  return (
    <div className="flex h-full min-h-0 flex-col" data-testid="stream-pane">
      <div className="flex h-11 shrink-0 items-center gap-3 px-3 pt-1">
        <span className={cx("h-2 w-2 rounded-full", dot)} />
        <span className="whitespace-nowrap text-[12.5px] font-medium text-fg" data-testid="stream-status">
          {label}
        </span>
        <span className="truncate whitespace-nowrap text-[12px] text-muted">
          {isWs ? `${sent} sent · ${received} received` : `${received} ${kind === "http" ? "results" : "events"}`}
          {filtered && messages.length > 0 && ` · ${visible.length} shown`}
        </span>
        <div className="flex-1" />
        <div className="flex h-7 w-56 items-center gap-1.5 rounded-lg border border-transparent bg-panel-2 px-2 focus-within:border-accent">
          <Search size={12} className="text-faint" />
          <input
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            placeholder="Filter, or $.type == trade"
            aria-label="Filter messages"
            className="min-w-0 flex-1 bg-transparent text-[12px] outline-none placeholder:text-faint"
          />
        </div>
        <Menu
          align="end"
          trigger={
            <IconButton label="View" active={show !== "all"}>
              <SlidersHorizontal size={14} />
            </IconButton>
          }
          entries={[
            ...(isWs
              ? ([
                  { label: "All messages", checked: show === "all", onSelect: () => setShow("all") },
                  { label: "Sent only", checked: show === "sent", onSelect: () => setShow("sent") },
                  { label: "Received only", checked: show === "received", onSelect: () => setShow("received") },
                  { separator: true },
                ] as const)
              : []),
            { label: "Time of day", checked: time === "clock", onSelect: () => setTime("clock") },
            { label: "Time since the previous message", checked: time === "delta", onSelect: () => setTime("delta") },
            { label: "Time since the first message", checked: time === "elapsed", onSelect: () => setTime("elapsed") },
            { separator: true },
            { label: "Export the log…", icon: <Download size={13} />, disabled: messages.length === 0, onSelect: () => void exportLog(tab) },
          ]}
        />
        <IconButton label="Clear messages" onClick={() => clearMessages(tab.id)}>
          <Trash2 size={14} />
        </IconButton>
      </div>
      {topics.length > 0 && (
        <div className="mx-3 mb-1.5 flex shrink-0 items-center gap-1 overflow-x-auto pb-0.5" data-testid="event-chips" aria-label={kind === "mqtt" ? "Topics" : "Events"}>
          {topics.slice(0, 30).map(([name, count]) => {
            const off = hidden.has(name);
            return (
              <button
                key={name}
                aria-pressed={only === name}
                title={`${only === name ? "Show every" : "Show only this"} ${kind === "mqtt" ? "topic" : "event"} · ${modKey}-click: ${off ? "show" : "hide"} it`}
                onClick={(e) => {
                  if (e.metaKey || e.ctrlKey) {
                    setHidden((h) => {
                      const next = new Set(h);
                      if (next.has(name)) next.delete(name);
                      else next.add(name);
                      return next;
                    });
                    if (only === name) setOnly(null);
                  } else setOnly(only === name ? null : name);
                }}
                className={cx(
                  "flex shrink-0 items-center gap-1 rounded-full border px-2 py-0.5 font-mono text-[11px] transition-colors",
                  only === name ? "border-accent bg-accent-soft text-accent" : "border-line text-muted hover:bg-hover hover:text-fg",
                  off && "line-through opacity-60",
                )}
              >
                {name}
                <span className="tabular-nums text-faint">{count}</span>
              </button>
            );
          })}
          {(only !== null || hidden.size > 0) && (
            <button
              className="shrink-0 px-1.5 text-[11px] text-accent hover:underline"
              onClick={() => {
                setOnly(null);
                setHidden(new Set());
              }}
            >
              Show all
            </button>
          )}
        </div>
      )}
      <MessageLog
        messages={visible}
        empty={messages.length === 0}
        isWs={isWs}
        kind={kind}
        time={time}
        replies={replies}
        onEdit={kind === "websocket" ? edit : undefined}
      />
      {isWs && <Composer tab={tab} />}
    </div>
  );
}

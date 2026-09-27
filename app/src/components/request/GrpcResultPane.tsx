// Result pane of gRPC requests: status (code, name, message, details), timing, the messages
// (the answer of a unary call; every sent and received message of a stream, with time
// offsets), response headers, trailers and the request metadata as sent.
import { memo, useEffect, useMemo, useState } from "react";
import { AlertTriangle, ArrowDownLeft, ArrowUpRight, Braces, ChevronRight, CircleAlert, Clock, Copy, Globe, ShieldAlert, Square, Unplug, WifiOff, Zap } from "lucide-react";
import type { ErrorKind } from "../../bindings/ErrorKind";
import type { GrpcMessage } from "../../bindings/GrpcMessage";
import type { GrpcStatus } from "../../bindings/GrpcStatus";
import { formatBytes, formatMs, toneBg } from "../../lib/format";
import { copyText, modKey } from "../../lib/platform";
import { CALL_KIND_LABEL, callKind, cancelCall, endStream, type GrpcCall, grpcTone, MAX_MESSAGES, shortMethod, STATUS_MEANING, useGrpc } from "../../store/grpc";
import { type Tab, updateTab } from "../../store/tabs";
import { toast } from "../../store/toasts";
import { openModal } from "../../store/ui";
import { CodeEditor } from "../CodeEditor";
import { HeaderTable } from "../response/ResponsePane";
import { TimingView } from "../response/ResponseInfo";
import { Badge, Banner, Button, cx, EmptyState, IconButton, Kbd, Spinner, Tabs, Tooltip } from "../ui";
import type { KindPaneProps } from "./kinds";

export function GrpcResultPane({ tab }: KindPaneProps) {
  const call = useGrpc((s) => s.calls[tab.id]);
  const r = tab.response;
  if (r.status === "error") return <GrpcError kind={r.kind} code={r.code} message={r.message} durationMs={r.durationMs} />;
  if (!call || (r.status === "loading" && !call.streaming)) {
    if (r.status === "loading") return <Waiting startedAt={r.startedAt} onCancel={() => void cancelCall(tab.id)} />;
    return <GrpcIdle tab={tab} />;
  }
  return <CallView tab={tab} call={call} running={r.status === "loading"} />;
}

function GrpcIdle({ tab }: { tab: Tab }) {
  const method = tab.draft.method.trim();
  return (
    <EmptyState icon={<Zap size={30} strokeWidth={1.5} />} title={method ? `Send to call ${shortMethod(method)}` : "Pick a method, then send"}>
      <span>
        <Kbd>{modKey}</Kbd> <Kbd>Enter</Kbd> send · services come from server reflection or the Proto files tab
      </span>
    </EmptyState>
  );
}

function Waiting({ startedAt, onCancel }: { startedAt: number; onCancel: () => void }) {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), 100);
    return () => clearInterval(t);
  }, []);
  return (
    <div className="flex h-full flex-col items-center justify-center gap-3" data-testid="grpc-loading">
      <Spinner size={22} />
      <div className="font-mono text-[13px] tabular-nums text-muted">{formatMs(now - startedAt)}</div>
      <Button size="sm" onClick={onCancel}>
        Cancel
      </Button>
    </div>
  );
}

const ERRORS: Partial<Record<ErrorKind, { title: string; icon: React.ReactNode; hint?: string }>> = {
  connect: { title: "Could not connect", icon: <Unplug size={28} />, hint: "Is the server running on that port? grpc:// is plaintext HTTP/2, grpcs:// uses TLS." },
  dns: { title: "Could not resolve host", icon: <Globe size={28} />, hint: "Check the host name, your network/VPN, and DNS settings." },
  tls: { title: "TLS / certificate error", icon: <ShieldAlert size={28} />, hint: "For corporate or self-signed certificates, add the CA in Settings → Certificates. A plaintext server needs grpc://." },
  proxy: { title: "Proxy error", icon: <WifiOff size={28} />, hint: "gRPC goes through the proxy from Settings → Proxy (CONNECT tunnel); localhost is always direct." },
  timeout: { title: "Timed out", icon: <Clock size={28} />, hint: "Connecting took too long, or reflection did not answer in time (Settings → timeout)." },
  protocol: { title: "Could not talk gRPC", icon: <AlertTriangle size={28} /> },
  invalidRequest: { title: "Invalid request", icon: <AlertTriangle size={28} /> },
  cancelled: { title: "Call cancelled", icon: <AlertTriangle size={28} /> },
};

function GrpcError({ kind, code, message, durationMs }: { kind: ErrorKind | null; code: string; message: string; durationMs: number }) {
  const undefinedVar = code === "undefinedVariable";
  const info = undefinedVar
    ? { title: "Undefined variable", icon: <Braces size={28} />, hint: undefined }
    : ((kind && ERRORS[kind]) ?? { title: "Call failed", icon: <AlertTriangle size={28} />, hint: undefined });
  const cancelled = kind === "cancelled";
  return (
    <div className="flex h-full flex-col items-center justify-center gap-3 p-8 text-center" data-testid="grpc-error">
      <div className={cancelled ? "text-faint" : "text-danger"}>{info.icon}</div>
      <div className="text-[14px] font-semibold text-fg">{info.title}</div>
      {!cancelled && (
        <div className="selectable max-w-xl whitespace-pre-wrap break-words rounded-xl border border-line bg-panel-2 px-3.5 py-2.5 text-left font-mono text-[12px] text-muted">
          {message}
        </div>
      )}
      {info.hint && <div className="max-w-md text-[12.5px] text-muted">{info.hint}</div>}
      <div className="flex items-center gap-2">
        {!undefinedVar && <span className="text-[11.5px] text-faint">after {formatMs(durationMs)}</span>}
        {undefinedVar && (
          <Button size="sm" onClick={() => openModal({ type: "environments" })}>
            Open environments
          </Button>
        )}
      </div>
    </div>
  );
}

function StatusBadge({ status }: { status: GrpcStatus }) {
  return (
    <Tooltip content={`${STATUS_MEANING[status.name] ?? `Status code ${status.code}`}${status.local ? " (set by Zorvik, not sent by the server)" : ""}`}>
      <span>
        <Badge className={cx("text-[12px]", toneBg[grpcTone(status.code)])}>
          <span data-testid="grpc-status">
            {status.code} {status.name}
          </span>
        </Badge>
      </span>
    </Tooltip>
  );
}

function CallView({ tab, call, running }: { tab: Tab; call: GrpcCall; running: boolean }) {
  const sent = call.messages.filter((m) => m.direction === "sent").length;
  const received = call.messages.length - sent;
  const kind = call.method ? callKind(call.method) : "unary";
  const view = ["body", "headers", "trailers", "request", "timing"].includes(tab.responseTab) ? tab.responseTab : "body";
  const items = [
    { id: "body", label: "Messages", badge: call.streaming ? call.messages.length : undefined },
    { id: "headers", label: "Headers", badge: call.headers.length },
    { id: "trailers", label: "Trailers", badge: call.trailers.length },
    { id: "request", label: "Request metadata" },
    ...(call.timing ? [{ id: "timing", label: "Timing" }] : []),
  ];
  return (
    <div className="flex h-full min-h-0 flex-col" data-testid="grpc-result">
      <div className="flex min-h-11 shrink-0 flex-wrap items-center gap-x-3 gap-y-1 px-3 pt-1">
        {call.status ? (
          <StatusBadge status={call.status} />
        ) : (
          <span className="flex items-center gap-2 text-[12.5px] font-medium text-fg" data-testid="grpc-stream-state">
            <span className={cx("h-2 w-2 rounded-full", call.state === "open" ? "bg-success" : "bg-warning animate-pulse")} />
            {call.state === "open" ? (call.clientEnded ? "Waiting for the server…" : "Stream open") : "Starting…"}
          </span>
        )}
        {call.timing && (
          <span className="text-[12px] tabular-nums text-muted" title="Total time">
            {formatMs(call.timing.totalMs)}
          </span>
        )}
        {call.streaming && (
          <span className="text-[12px] text-muted">
            {CALL_KIND_LABEL[kind]} · {sent} sent · {received} received
          </span>
        )}
        {call.status?.message && (
          <span className="min-w-0 flex-1 truncate text-[12px] text-muted" title={call.status.message} data-testid="grpc-status-message">
            {call.status.message}
          </span>
        )}
        <div className="flex-1" />
        {running && call.streaming && call.method?.clientStreaming && !call.clientEnded && (
          <Button size="sm" icon={<Square size={12} />} onClick={() => void endStream(tab.id)}>
            End stream
          </Button>
        )}
        {running && (
          <Button size="sm" variant="ghost" onClick={() => void cancelCall(tab.id)}>
            Cancel
          </Button>
        )}
      </div>
      {call.unresolved.length > 0 && (
        <Banner tone="warning">
          Sent with undefined values: <span className="font-mono">{call.unresolved.map((v) => `{{${v}}}`).join(", ")}</span>
        </Banner>
      )}
      {/* The newest few: a stream whose messages all fail to decode must not push the messages away. */}
      {call.errors.slice(-3).map((e, i) => (
        <Banner key={i} tone="danger">
          {e}
        </Banner>
      ))}
      {call.errors.length > 3 && <div className="mx-3 mt-1 text-[11.5px] text-faint">and {call.errors.length - 3} earlier problems</div>}
      <Tabs items={items} value={view} onChange={(responseTab) => updateTab(tab.id, () => ({ responseTab }))} />
      <div className="min-h-0 flex-1 overflow-auto">
        {view === "body" && <MessagesView call={call} running={running} />}
        {view === "headers" && <HeaderTable headers={call.headers} />}
        {view === "trailers" && <HeaderTable headers={call.trailers} />}
        {view === "request" && (
          <div>
            {call.remoteAddr && <div className="px-3 pb-1 pt-2 text-[12px] text-muted">Connected to <span className="font-mono text-fg">{call.remoteAddr}</span></div>}
            <HeaderTable headers={call.requestHeaders} />
          </div>
        )}
        {view === "timing" && call.timing && <TimingView timing={call.timing} />}
      </div>
    </div>
  );
}

function MessagesView({ call, running }: { call: GrpcCall; running: boolean }) {
  const failed = call.status && call.status.code !== 0 ? call.status : null;
  const received = call.messages.filter((m) => m.direction === "received");
  return (
    <div className="flex h-full min-h-0 flex-col">
      {failed && <StatusPanel status={failed} />}
      {!call.streaming ? (
        received.length === 1 ? (
          <div className="min-h-0 flex-1">
            <CodeEditor value={received[0].json} readOnly language="json" />
          </div>
        ) : received.length === 0 ? (
          !failed && <EmptyState title="No message in the answer" />
        ) : (
          <MessageLog messages={call.messages} dropped={call.dropped} startedAt={call.startedAt} />
        )
      ) : call.messages.length === 0 ? (
        <EmptyState icon={running ? <Spinner size={20} /> : undefined} title={running ? "Waiting for messages…" : "No messages"}>
          {running && call.method?.clientStreaming && !call.clientEnded ? "Send messages from the Message tab; End stream when you are done." : undefined}
        </EmptyState>
      ) : (
        <MessageLog messages={call.messages} dropped={call.dropped} startedAt={call.startedAt} />
      )}
    </div>
  );
}

function StatusPanel({ status }: { status: GrpcStatus }) {
  return (
    <div className="mx-3 mb-2 mt-2 shrink-0 rounded-lg border border-line bg-panel-2 px-3 py-2 text-[12.5px]" data-testid="grpc-status-panel">
      <div className="flex items-start gap-2">
        <CircleAlert size={14} className={cx("mt-0.5 shrink-0", grpcTone(status.code) === "danger" ? "text-danger" : "text-warning")} />
        <div className="min-w-0 flex-1">
          <div className="font-semibold text-fg">
            {status.name}
            {status.local && <span className="ml-2 text-[11px] font-normal text-faint">set by Zorvik</span>}
          </div>
          {status.message && <div className="selectable whitespace-pre-wrap break-words text-fg">{status.message}</div>}
          <div className="mt-0.5 text-[11.5px] text-muted">{STATUS_MEANING[status.name]}</div>
          {status.details && (
            <pre className="selectable mt-2 max-h-60 overflow-auto whitespace-pre-wrap break-all rounded-md bg-panel px-2 py-1.5 font-mono text-[11.5px] text-fg">
              {status.details}
            </pre>
          )}
        </div>
      </div>
    </div>
  );
}

function offset(ms: number): string {
  return `+${formatMs(Math.max(0, ms))}`;
}

// Memoized: the tab re-renders on every keystroke in the editor. Rows are keyed by their stable
// number (`dropped + index`): once the oldest messages are dropped, the others keep their rows
// (and open state) instead of all re-rendering on every batch.
const MessageLog = memo(function MessageLog({ messages, dropped, startedAt }: { messages: GrpcMessage[]; dropped: number; startedAt: number }) {
  return (
    <div className="mx-3 mb-3 min-h-0 flex-1 overflow-auto rounded-lg border border-line font-mono text-[12px]" data-testid="grpc-messages">
      {dropped > 0 && (
        <div className="border-b border-line/60 px-3 py-1.5 font-sans text-[11.5px] text-faint" data-testid="grpc-messages-dropped">
          {dropped.toLocaleString()} earlier {dropped === 1 ? "message is" : "messages are"} not shown (only the newest {MAX_MESSAGES.toLocaleString()} are kept).
        </div>
      )}
      {messages.map((m, i) => (
        <MessageRow key={dropped + i} m={m} at={m.timestamp - startedAt} defaultOpen={messages.length <= 3} />
      ))}
    </div>
  );
});

const MessageRow = memo(function MessageRow({ m, at, defaultOpen }: { m: GrpcMessage; at: number; defaultOpen: boolean }) {
  const [open, setOpen] = useState(defaultOpen);
  // The one-line preview: compact JSON (the full text is pretty-printed).
  const line = useMemo(() => {
    try {
      return JSON.stringify(JSON.parse(m.json)).slice(0, 300);
    } catch {
      return m.json.slice(0, 300);
    }
  }, [m.json]);
  return (
    <div className="border-b border-line/60 last:border-b-0" style={{ contentVisibility: "auto", containIntrinsicSize: "auto 30px" }}>
      <button onClick={() => setOpen(!open)} aria-expanded={open} className="flex w-full items-center gap-2 px-3 py-1.5 text-left hover:bg-hover/40">
        <ChevronRight size={11} className={cx("shrink-0 text-faint transition-transform", open && "rotate-90")} />
        {m.direction === "sent" ? <ArrowUpRight size={13} className="text-success" /> : <ArrowDownLeft size={13} className="text-info" />}
        <span className="w-16 shrink-0 text-[11px] tabular-nums text-faint" title="Since the call started">
          {offset(at)}
        </span>
        <span className="min-w-0 flex-1 truncate text-fg" data-testid="grpc-message">
          {line}
        </span>
        <span className="shrink-0 text-[11px] text-faint">{formatBytes(m.size)}</span>
      </button>
      {open && (
        <div className="group relative bg-panel-2/60 px-8 py-2">
          <pre className="selectable max-h-80 overflow-auto whitespace-pre-wrap break-all text-fg">{m.json}</pre>
          <IconButton
            label="Copy message"
            onClick={() => {
              void copyText(m.json);
              toast("success", "Copied to clipboard");
            }}
            className="absolute right-2 top-2 opacity-0 group-hover:opacity-100"
            size={24}
          >
            <Copy size={12} />
          </IconButton>
        </div>
      )}
    </div>
  );
});

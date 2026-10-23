// WebSocket, Socket.IO and gRPC stream requests: saved messages (Socket.IO: emits, with their
// event and acknowledgement; gRPC: JSON messages), the on-connect steps (send, wait for a reply,
// keep sending), and WebSocket settings (subprotocols). Steps run in the backend on every connect
// (gRPC: once a stream is open); in collection runs each wait is a test.
import { useEffect, useState } from "react";
import { ArrowDown, ArrowUp, Bookmark, Hourglass, Plus, Repeat, Send, Trash2, Upload } from "lucide-react";
import type { ConnectStep } from "../../bindings/ConnectStep";
import type { MatchKind } from "../../bindings/MatchKind";
import type { MessageFormat } from "../../bindings/MessageFormat";
import type { SavedMessage } from "../../bindings/SavedMessage";
import type { WebSocketOptions } from "../../bindings/WebSocketOptions";
import { Checkbox } from "../ui";
import { patternProblem } from "../../lib/messageMatch";
import { useRegexProblem } from "../../lib/regexCheck";
import { freeName, loadIntoComposer, sendSaved } from "../../store/composer";
import { sendMessage as sendGrpcMessage, useGrpc } from "../../store/grpc";
import { type Tab, updateDraft, updateTab } from "../../store/tabs";
import { useVariableNames } from "../../store/workspace";
import { CodeEditor } from "../CodeEditor";
import { Button, cx, Field, IconButton, Input, Menu, NumberInput, Segmented, Select } from "../ui";
import type { KindPaneProps } from "./kinds";

export const WAIT_TIMEOUT = 5000;

export const MATCH_KINDS: { id: MatchKind; label: string; placeholder: string }[] = [
  { id: "contains", label: "contains", placeholder: "welcome" },
  { id: "exact", label: "is exactly", placeholder: "pong" },
  { id: "regex", label: "matches regex", placeholder: "^ack:\\d+$" },
  { id: "json", label: "JSON where", placeholder: '$.type == "welcome"' },
  { id: "any", label: "any message", placeholder: "" },
];

const FORMATS: { id: MessageFormat; label: string }[] = [
  { id: "text", label: "Text" },
  { id: "json", label: "JSON" },
  { id: "binary", label: "Hex" },
];

/** What saved messages and steps share between the WebSocket and Socket.IO options. */
export interface Options {
  messages?: SavedMessage[];
  onConnect?: ConnectStep[];
}

const isSocketIo = (tab: Tab) => tab.draft.kind === "socketio";
const isGrpc = (tab: Tab) => tab.draft.kind === "grpc";

export function wsOptions(tab: Tab): WebSocketOptions {
  return tab.draft.websocket ?? {};
}

/** The request's saved messages and steps (WebSocket, Socket.IO or gRPC). */
export function liveOptions(tab: Tab): Options {
  if (isGrpc(tab)) return tab.draft.grpc ?? {};
  return isSocketIo(tab) ? (tab.draft.socketio ?? {}) : (tab.draft.websocket ?? {});
}

function setOptions(tab: Tab, fn: (o: Options) => Options) {
  updateDraft(tab.id, (r) => {
    if (r.kind === "grpc") return { ...r, grpc: { ...(r.grpc ?? {}), ...fn(r.grpc ?? {}) } };
    return r.kind === "socketio" ? { ...r, socketio: { ...(r.socketio ?? {}), ...fn(r.socketio ?? {}) } } : { ...r, websocket: { ...(r.websocket ?? {}), ...fn(r.websocket ?? {}) } };
  });
}

function setWsOptions(tab: Tab, fn: (o: WebSocketOptions) => WebSocketOptions) {
  updateDraft(tab.id, (r) => ({ ...r, websocket: fn(r.websocket ?? {}) }));
}

// ---- Messages ----------------------------------------------------------------------------------

/**
 * Rename saved message `i`. Steps name the message they send (the first one with that name), so
 * the steps that sent this one follow it. A name that's empty or another message's is refused (null).
 */
export function renameMessage(o: Options, i: number, name: string): Options | null {
  const messages = o.messages ?? [];
  const old = messages[i];
  if (!old || !name.trim() || messages.some((m, j) => j !== i && m.name === name)) return null;
  if (name === old.name) return o;
  const sentThis = messages.findIndex((m) => m.name === old.name) === i;
  return {
    ...o,
    messages: messages.map((m, j) => (j === i ? { ...m, name } : m)),
    onConnect: sentThis ? o.onConnect?.map((s) => (s.type !== "wait" && s.message === old.name ? { ...s, message: name } : s)) : o.onConnect,
  };
}

/** A saved message's name. One that can't be used (empty, another message's) isn't taken: the field shows why until it's left. */
function MessageName({ name, taken, onRename }: { name: string; taken: (name: string) => boolean; onRename: (name: string) => void }) {
  // What's typed while it can't be the name.
  const [text, setText] = useState<string | null>(null);
  const shown = text ?? name;
  const problem = !shown.trim() ? "Give the message a name" : taken(shown) ? "Another message has this name" : null;
  return (
    <div className="flex min-w-0 flex-1 flex-col">
      <input
        aria-label="Message name"
        value={shown}
        aria-invalid={!!problem || undefined}
        onChange={(e) => {
          const next = e.target.value;
          if (next.trim() && !taken(next)) {
            onRename(next);
            setText(null);
          } else setText(next);
        }}
        onBlur={() => setText(null)}
        className={cx("h-7 min-w-0 rounded-md bg-transparent px-1.5 text-[12.5px] font-medium text-fg outline-none focus:bg-panel-2", problem && "text-danger")}
        placeholder="Name"
      />
      {problem && <span className="px-1.5 text-[11px] text-warning">{problem}</span>}
    </div>
  );
}

export function MessagesTab({ tab }: KindPaneProps) {
  const { names } = useVariableNames();
  const io = isSocketIo(tab);
  const grpc = isGrpc(tab);
  const o = liveOptions(tab);
  const messages = o.messages ?? [];
  const open = tab.stream.status === "open";
  const call = useGrpc((st) => st.calls[tab.id]);
  const grpcOpen = !!call && call.streaming && call.state !== "ended" && !call.clientEnded && !!call.method?.clientStreaming;
  const update = (i: number, patch: Partial<Omit<SavedMessage, "name">>) =>
    setOptions(tab, (w) => {
      const list = [...(w.messages ?? [])];
      const old = list[i];
      list[i] = { ...old, ...patch, format: (patch.format ?? old.format) === "text" ? undefined : (patch.format ?? old.format) };
      return { ...w, messages: list };
    });
  const rename = (i: number, name: string) => setOptions(tab, (w) => renameMessage(w, i, name) ?? w);
  const add = () =>
    setOptions(tab, (w) => ({
      ...w,
      messages: [
        ...(w.messages ?? []),
        { name: freeName((w.messages ?? []).map((m) => m.name), io ? "Emit" : "Message"), text: "", ...(io ? { event: "" } : {}), ...(grpc ? { format: "json" as const } : {}) },
      ],
    }));
  const remove = (i: number) => setOptions(tab, (w) => ({ ...w, messages: (w.messages ?? []).filter((_, j) => j !== i) }));
  return (
    <div className="flex max-w-3xl flex-col gap-3 p-4" data-testid="ws-messages">
      {grpc ? (
        <p className="text-[12.5px] text-muted">
          Messages (JSON of the method's input type) kept with this request, for streaming calls: sent by <b>On connect</b> steps once the stream is open,
          or from the Message tab while it is. <code className="font-mono">{"{{variables}}"}</code> are filled in when one is sent.
        </p>
      ) : (
      <p className="text-[12.5px] text-muted">
        {io ? "Emits" : "Messages"} kept with this request. Send them from the composer's <Bookmark size={12} className="inline align-[-2px]" /> menu, or
        from <b>On connect</b> steps. <code className="font-mono">{"{{variables}}"}</code> are filled in when {io ? "one is emitted" : "a message is sent"}.
        {io && " JSON is the arguments (an array for several); Text is one string argument; Hex is one binary argument."}
      </p>
      )}
      {messages.map((m, i) => (
        <div key={i} className="overflow-hidden rounded-xl border border-line bg-input" data-testid="ws-message">
          <div className="flex items-center gap-2 border-b border-line px-2 py-1.5">
            <MessageName name={m.name} taken={(name) => messages.some((x, j) => j !== i && x.name === name)} onRename={(name) => rename(i, name)} />
            {io && (
              <>
                <input
                  aria-label="Event"
                  value={m.event ?? ""}
                  onChange={(e) => update(i, { event: e.target.value })}
                  placeholder="event"
                  className={cx("h-7 w-36 rounded-md border border-line bg-panel-2 px-2 font-mono text-[12px] text-fg outline-none focus:border-accent", !m.event?.trim() && "border-warning/60")}
                />
                <label className="flex shrink-0 cursor-pointer items-center gap-1.5 text-[12px] text-muted" title="Ask the server to acknowledge">
                  <Checkbox checked={m.ack ?? false} onChange={(ack) => update(i, { ack: ack || undefined })} label="Ask for acknowledgement" />
                  Ack
                </label>
              </>
            )}
            {!grpc && <Segmented items={FORMATS} value={m.format ?? "text"} onChange={(format) => update(i, { format })} />}
            {!grpc && (
              <IconButton label="Put in the composer" onClick={() => loadIntoComposer(tab.id, m)}>
                <Upload size={13} />
              </IconButton>
            )}
            <IconButton
              label={grpc ? (grpcOpen ? "Send on the open stream" : "Open a client or bidirectional stream to send") : open ? "Send now" : "Connect to send"}
              disabled={grpc ? !grpcOpen : !open}
              onClick={() => void (grpc ? sendGrpcMessage(tab.id, m.text) : sendSaved(tab.id, m))}
            >
              <Send size={13} />
            </IconButton>
            <IconButton label="Remove message" onClick={() => remove(i)}>
              <Trash2 size={13} />
            </IconButton>
          </div>
          <div style={{ height: Math.min(200, Math.max(56, 22 + m.text.split("\n").length * 18)) }}>
            <CodeEditor
              value={m.text}
              onChange={(text) => update(i, { text })}
              language={m.format === "json" || grpc ? "json" : "text"}
              variables={m.format === "binary" ? undefined : names}
              lineNumbers={false}
              placeholder={
                m.format === "binary" ? "48 65 6c 6c 6f" : m.format === "json" ? (io ? '["lobby", {"user": "{{user}}"}]' : '{"type": "subscribe", "id": 1}') : io ? "One text argument" : "Message"
              }
            />
          </div>
        </div>
      ))}
      <div className="flex items-center gap-3">
        <Button size="sm" icon={<Plus size={13} />} onClick={add}>
          {io ? "Add emit" : "Add message"}
        </Button>
        {messages.length === 0 && !grpc && <span className="text-[12px] text-faint">Or type one in the composer and choose “Save as message…”.</span>}
      </div>
      {!io && !grpc && (
      <Field
        label="Match replies by"
        hint="A reply whose value at this JSON path equals that of a message you sent shows how long it took, in the log."
        className="mt-2 max-w-sm"
      >
        <Input
          aria-label="Match replies by"
          className="font-mono text-[12.5px]"
          value={wsOptions(tab).replyKey ?? ""}
          placeholder="$.id"
          onChange={(e) => setWsOptions(tab, (w) => ({ ...w, replyKey: e.target.value || undefined }))}
        />
      </Field>
      )}
    </div>
  );
}

// ---- On connect --------------------------------------------------------------------------------

const STEP_KINDS: { id: ConnectStep["type"]; label: string }[] = [
  { id: "send", label: "Send" },
  { id: "wait", label: "Wait for" },
  { id: "repeat", label: "Keep sending" },
];

function newStep(type: ConnectStep["type"], first: string): ConnectStep {
  if (type === "send") return { type, message: first };
  if (type === "repeat") return { type, message: first, everyMs: 30000 };
  return { type, match: "contains", pattern: "" };
}

export function OnConnectTab({ tab }: KindPaneProps) {
  const io = isSocketIo(tab);
  const grpc = isGrpc(tab);
  const o = liveOptions(tab);
  const steps = o.onConnect ?? [];
  const names = (o.messages ?? []).map((m) => m.name);
  const setSteps = (fn: (s: ConnectStep[]) => ConnectStep[]) => setOptions(tab, (w) => ({ ...w, onConnect: fn(w.onConnect ?? []) }));
  const update = (i: number, step: ConnectStep) => setSteps((s) => s.map((x, j) => (j === i ? step : x)));
  const move = (i: number, by: number) =>
    setSteps((s) => {
      const next = [...s];
      [next[i], next[i + by]] = [next[i + by], next[i]];
      return next;
    });
  const add = (type: ConnectStep["type"]) => setSteps((s) => [...s, newStep(type, names[0] ?? "")]);
  return (
    <div className="flex max-w-3xl flex-col gap-3 p-4" data-testid="ws-on-connect">
      {grpc ? (
        <>
          <p className="text-[12.5px] text-muted">
            Runs in order once a streaming call is open: send messages, wait for the server's answers (matched as JSON, e.g.{" "}
            <code className="font-mono">$.status == READY</code>), keep sending one. For server streams the Message tab's message goes first. In collection runs,{" "}
            <code className="font-mono">zorvik run</code> and for AI agents the call runs to its end and each wait is a test.
          </p>
          <label className="flex items-center gap-2 text-[12.5px] text-muted">
            <Checkbox
              checked={tab.draft.grpc?.endAfterSteps ?? false}
              onChange={(endAfterSteps) => updateDraft(tab.id, (r) => ({ ...r, grpc: { ...(r.grpc ?? {}), endAfterSteps: endAfterSteps || undefined } }))}
              label="End the stream after the steps"
            />
            End the stream after the steps (client and bidirectional streams; runs always do)
          </label>
        </>
      ) : (
      <p className="text-[12.5px] text-muted">
        Runs in order every time you connect: {io ? "join rooms, wait for the server's event" : "log in, subscribe, wait for the server's answer"}, keep a heartbeat going. A step that fails stops
        the rest (the connection stays open). In collection runs and <code className="font-mono">zorvik run</code>, each wait is a test.
      </p>
      )}
      {steps.length > 0 && (
        <ol className="flex flex-col gap-1.5">
          {steps.map((step, i) => (
            <StepRow
              key={i}
              index={i}
              step={step}
              names={names}
              onChange={(s) => update(i, s)}
              onMove={(by) => move(i, by)}
              onRemove={() => setSteps((s) => s.filter((_, j) => j !== i))}
              last={i === steps.length - 1}
              io={io}
            />
          ))}
        </ol>
      )}
      <div className="flex items-center gap-3">
        <Menu
          trigger={
            <Button size="sm" icon={<Plus size={13} />}>
              Add step
            </Button>
          }
          entries={[
            { label: "Send a saved message", icon: <Send size={13} />, onSelect: () => add("send") },
            { label: "Wait for a message", icon: <Hourglass size={13} />, onSelect: () => add("wait") },
            { label: "Keep sending a message (heartbeat)", icon: <Repeat size={13} />, onSelect: () => add("repeat") },
          ]}
        />
        {names.length === 0 && (
          <button className="text-[12px] text-accent hover:underline" onClick={() => updateTab(tab.id, () => ({ requestTab: "messages" }))}>
            Save a message first to send it here
          </button>
        )}
      </div>
    </div>
  );
}

const numberField = "h-7 w-24 rounded-md border border-line bg-panel-2 px-2 text-right font-mono text-[12px] text-fg outline-none focus:border-accent";

function StepRow({
  index,
  step,
  names,
  onChange,
  onMove,
  onRemove,
  last,
  io,
}: {
  io: boolean;
  index: number;
  step: ConnectStep;
  names: string[];
  onChange: (s: ConnectStep) => void;
  onMove: (by: number) => void;
  onRemove: () => void;
  last: boolean;
}) {
  const messagePicker = (value: string, set: (message: string) => void) => (
    <Select aria-label="Saved message" value={value} onChange={(e) => set(e.target.value)} className="h-7 min-w-40 [&_select]:h-7 [&_select]:text-[12px]">
      {!names.includes(value) && <option value={value}>{value ? `${value} (not saved)` : "Choose a message"}</option>}
      {names.map((n) => (
        <option key={n} value={n}>
          {n}
        </option>
      ))}
    </Select>
  );
  const regexProblem = useRegexProblem(step.type === "wait" && step.match === "regex" ? (step.pattern ?? "") : null);
  const problem =
    step.type === "wait"
      ? (regexProblem ?? patternProblem(step.match, step.pattern ?? ""))
      : names.includes(step.message)
        ? null
        : "There's no saved message with this name";
  const kind = MATCH_KINDS.find((k) => step.type === "wait" && k.id === step.match);
  return (
    <li className="flex items-start gap-2 rounded-lg border border-line bg-input px-2 py-1.5" data-testid="ws-step">
      <span className="w-5 pt-1.5 text-center text-[11px] tabular-nums text-faint">{index + 1}</span>
      <div className="flex min-w-0 flex-1 flex-col gap-1">
        <div className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1.5 text-[12px] text-muted">
          <Select
            aria-label="Step"
            value={step.type}
            onChange={(e) => onChange(newStep(e.target.value as ConnectStep["type"], step.type !== "wait" ? step.message : (names[0] ?? "")))}
            className="h-7 w-32 [&_select]:h-7 [&_select]:text-[12px]"
          >
            {STEP_KINDS.map((k) => (
              <option key={k.id} value={k.id}>
                {k.label}
              </option>
            ))}
          </Select>
          {step.type === "send" && messagePicker(step.message, (message) => onChange({ ...step, message }))}
          {step.type === "repeat" && (
            <>
              {messagePicker(step.message, (message) => onChange({ ...step, message }))}
              <span className="flex items-center gap-2 whitespace-nowrap">
                every
                <NumberInput bare min={10} aria-label="Every (ms)" className={numberField} value={step.everyMs} onChange={(everyMs) => onChange({ ...step, everyMs: everyMs ?? 10 })} />
                ms
              </span>
            </>
          )}
          {step.type === "wait" && (
            <>
              {io && (
                <span className="flex items-center gap-2 whitespace-nowrap">
                  the event
                  <input
                    aria-label="Event"
                    value={step.event ?? ""}
                    placeholder="any event"
                    onChange={(e) => onChange({ ...step, event: e.target.value || undefined })}
                    className="h-7 w-32 rounded-md border border-line bg-panel-2 px-2 font-mono text-[12px] text-fg outline-none focus:border-accent"
                  />
                </span>
              )}
              <span className="flex items-center gap-2 whitespace-nowrap">
                {io ? "with arguments that" : "a message that"}
                <Select
                  aria-label="Match"
                  value={step.match}
                  onChange={(e) => onChange({ ...step, match: e.target.value as MatchKind })}
                  className="h-7 w-36 [&_select]:h-7 [&_select]:text-[12px]"
                >
                  {MATCH_KINDS.map((k) => (
                    <option key={k.id} value={k.id}>
                      {k.label}
                    </option>
                  ))}
                </Select>
              </span>
              {step.match !== "any" && (
                <input
                  aria-label="Pattern"
                  value={step.pattern ?? ""}
                  placeholder={io && step.match === "json" ? "$[0].room == lobby" : kind?.placeholder}
                  onChange={(e) => onChange({ ...step, pattern: e.target.value || undefined })}
                  className="h-7 min-w-48 flex-1 rounded-md border border-line bg-panel-2 px-2 font-mono text-[12px] text-fg outline-none focus:border-accent"
                />
              )}
              <span className="flex items-center gap-2 whitespace-nowrap">
                within
                <NumberInput
                  bare
                  min={1}
                  aria-label="Timeout (ms)"
                  className={numberField}
                  value={step.timeoutMs ?? WAIT_TIMEOUT}
                  onChange={(ms) => onChange({ ...step, timeoutMs: ms === undefined || ms === WAIT_TIMEOUT ? undefined : ms })}
                />
                ms
              </span>
            </>
          )}
        </div>
        {problem && <div className="text-[11.5px] text-warning">{problem}</div>}
      </div>
      <div className="flex shrink-0 items-center">
        <IconButton label="Move up" disabled={index === 0} onClick={() => onMove(-1)}>
          <ArrowUp size={13} />
        </IconButton>
        <IconButton label="Move down" disabled={last} onClick={() => onMove(1)}>
          <ArrowDown size={13} />
        </IconButton>
        <IconButton label="Remove step" onClick={onRemove}>
          <Trash2 size={13} />
        </IconButton>
      </div>
    </li>
  );
}

// ---- Settings ----------------------------------------------------------------------------------

function splitList(text: string): string[] {
  return text
    .split(",")
    .map((p) => p.trim())
    .filter(Boolean);
}

export function WebSocketSettings({ tab }: KindPaneProps) {
  const joined = (wsOptions(tab).subprotocols ?? []).join(", ");
  // Typed as text ("a, b"); the list is what's saved.
  const [text, setText] = useState(joined);
  useEffect(() => {
    setText((t) => (splitList(t).join(", ") === joined ? t : joined));
  }, [joined]);
  return (
    <div className="flex max-w-2xl flex-col gap-4 px-4 pb-4">
      <Field
        label="Subprotocols"
        hint="Sent as Sec-WebSocket-Protocol, the preferred first (comma-separated). The one the server picks shows when you connect. A Sec-WebSocket-Protocol header of your own wins."
      >
        <Input
          aria-label="Subprotocols"
          className="font-mono text-[12.5px]"
          placeholder="graphql-transport-ws, chat.v2"
          value={text}
          onChange={(e) => {
            setText(e.target.value);
            const list = splitList(e.target.value);
            setOptions(tab, (w) => ({ ...w, subprotocols: list.length ? list : undefined }));
          }}
        />
      </Field>
    </div>
  );
}

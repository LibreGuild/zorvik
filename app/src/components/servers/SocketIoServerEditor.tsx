// Socket.IO server settings: how events are answered (echo or rules with acknowledgements,
// replies and broadcasts), a greeting event, events on a timer, the path and CORS.
import { Plus, Trash2 } from "lucide-react";
import type { MatchKind } from "../../bindings/MatchKind";
import type { SocketIoRule } from "../../bindings/SocketIoRule";
import type { SocketIoScheduled } from "../../bindings/SocketIoScheduled";
import type { SocketIoServerConfig } from "../../bindings/SocketIoServerConfig";
import { Button, Checkbox, cx, Field, Input, NumberInput, Segmented, Select, Switch } from "../ui";
import type { ServerEditorProps } from "./kinds";
import { EditorSection } from "./ServerView";
import { PatternProblem } from "./ReplyRulesEditor";
import { REPLY_MODES } from "./SocketServerEditor";

const HINTS: Record<string, string> = {
  echo: "Every event is emitted back to its sender, and acknowledged with its own arguments when the client asks.",
  rules: "The first matching rule answers; other events get no answer.",
  manual: "Nothing is sent automatically: emit from the traffic panel.",
  discard: "Events are read and dropped.",
};

const MATCHERS: { id: MatchKind; label: string }[] = [
  { id: "any", label: "any arguments" },
  { id: "contains", label: "arguments contain" },
  { id: "exact", label: "arguments are exactly" },
  { id: "regex", label: "arguments match regex" },
  { id: "json", label: "arguments (JSON) where" },
];

const mono = "font-mono text-[12px]";

export function SocketIoServerEditor({ server, onChange, running }: ServerEditorProps) {
  const io: SocketIoServerConfig = server.socketio ?? {};
  const set = (patch: Partial<SocketIoServerConfig>) => onChange((s) => ({ ...s, socketio: { ...(s.socketio ?? {}), ...patch } }));
  const mode = io.mode ?? "echo";
  const path = io.path ?? "/socket.io/";

  return (
    <>
      <EditorSection title="Answers">
        <div>
          <Segmented items={REPLY_MODES.map((m) => ({ id: m.id, label: m.label }))} value={mode} onChange={(mode) => set({ mode })} />
        </div>
        <p className="text-[12px] text-muted">{HINTS[mode]}</p>
        {mode === "rules" && <RulesEditor rules={io.rules ?? []} onChange={(rules) => set({ rules })} />}
      </EditorSection>

      <EditorSection title="Greeting">
        <div className="grid grid-cols-[minmax(0,1fr)_minmax(0,1.6fr)] gap-3">
          <Field label="Event on joining" hint="Emitted to each client that joins. Empty: none.">
            <Input aria-label="Greeting event" className={mono} value={io.greetingEvent ?? ""} placeholder="welcome" onChange={(e) => set({ greetingEvent: e.target.value })} />
          </Field>
          <Field label="Arguments (JSON)" hint="An array for several arguments. Can use {{$uuid}} and environment variables.">
            <Input
              aria-label="Greeting arguments"
              className={mono}
              value={io.greetingArgs ?? ""}
              placeholder={'{"id": "{{$uuid}}"}'}
              onChange={(e) => set({ greetingArgs: e.target.value })}
            />
          </Field>
        </div>
      </EditorSection>

      <EditorSection title="Scheduled events">
        <ScheduleEditor schedule={io.schedule ?? []} onChange={(schedule) => set({ schedule: schedule.length ? schedule : undefined })} />
      </EditorSection>

      <EditorSection title="Clients">
        <div className="grid grid-cols-2 gap-3">
          <Field label="Path" hint="The path option of socket.io and socket.io-client.">
            <Input aria-label="Socket.IO path" className={mono} value={path} placeholder="/socket.io/" onChange={(e) => set({ path: e.target.value })} />
          </Field>
          <div className="pt-6">
            <Switch checked={io.cors ?? false} onChange={(cors) => set({ cors })} label="Allow browsers on other origins (CORS)" />
          </div>
        </div>
        <p className="text-[12px] text-muted">
          Socket.IO 3 and 4 clients connect with long-polling or WebSocket
          {running ? (
            <>
              , e.g. <span className="selectable font-mono text-fg">io("{running.url}/chat")</span>
            </>
          ) : null}
          , to any namespace. Emit to one client or all of them from the traffic panel.
        </p>
      </EditorSection>
    </>
  );
}

function RulesEditor({ rules, onChange }: { rules: SocketIoRule[]; onChange: (rules: SocketIoRule[]) => void }) {
  const update = (i: number, patch: Partial<SocketIoRule>) => onChange(rules.map((r, j) => (j === i ? { ...r, ...patch } : r)));
  return (
    <div className="flex flex-col gap-2" aria-label="Rules">
      {rules.map((rule, i) => {
        const enabled = rule.enabled !== false;
        const match = rule.match ?? "any";
        return (
          <div key={i} className={cx("flex flex-col gap-2 rounded-lg border border-line p-2.5 text-[12.5px]", !enabled && "opacity-50")} data-testid="socketio-rule">
            <div className="flex flex-wrap items-center gap-2">
              <Checkbox checked={enabled} onChange={(v) => update(i, { enabled: v })} title="Use this rule" />
              <span className="text-muted">When a client emits</span>
              <Input aria-label="Event" className={cx(mono, "w-36")} value={rule.event} placeholder="chat, or * for any" onChange={(e) => update(i, { event: e.target.value })} />
              <span className="text-muted">with</span>
              <Select aria-label="Match" className="w-48" value={match} onChange={(e) => update(i, { match: e.target.value as MatchKind })}>
                {MATCHERS.map((m) => (
                  <option key={m.id} value={m.id}>
                    {m.label}
                  </option>
                ))}
              </Select>
              {match !== "any" && (
                <Input
                  aria-label="Pattern"
                  className={cx(mono, "min-w-32 flex-1")}
                  value={rule.pattern ?? ""}
                  placeholder={match === "regex" ? '"id":\\d+' : match === "json" ? "$[0].type == join" : "hello"}
                  onChange={(e) => update(i, { pattern: e.target.value })}
                />
              )}
              <div className="flex-1" />
              <button aria-label="Remove rule" onClick={() => onChange(rules.filter((_, j) => j !== i))} className="rounded p-1 text-faint hover:bg-hover hover:text-danger">
                <Trash2 size={13} />
              </button>
            </div>
            <PatternProblem match={match} pattern={rule.pattern ?? ""} className="pl-6" />
            <div className="grid grid-cols-[auto_minmax(0,1fr)] items-center gap-x-2 gap-y-1.5 pl-6">
              <span className="text-muted">Acknowledge with</span>
              <Input
                aria-label="Acknowledgement"
                className={mono}
                value={rule.ack ?? ""}
                placeholder={'{"ok": true} (when the client asks)'}
                onChange={(e) => update(i, { ack: e.target.value })}
              />
              <span className="text-muted">Then emit</span>
              <Input aria-label="Reply event" className={mono} value={rule.replyEvent ?? ""} placeholder="event (optional)" onChange={(e) => update(i, { replyEvent: e.target.value })} />
              <span className="text-muted">with</span>
              <Input
                aria-label="Reply arguments"
                className={mono}
                value={rule.replyArgs ?? ""}
                placeholder='["you said", {{event.arg0}}]'
                onChange={(e) => update(i, { replyArgs: e.target.value })}
              />
              <span />
              <div className="flex flex-wrap items-center gap-x-4 gap-y-1">
                <label className="flex cursor-pointer items-center gap-1.5 text-muted">
                  <Checkbox checked={rule.broadcast ?? false} onChange={(broadcast) => update(i, { broadcast })} label="To every client of the namespace" />
                  to every client of the namespace
                </label>
                <label className="flex items-center gap-1.5 text-muted">
                  after
                  <NumberInput aria-label="Delay in milliseconds" className="w-20 text-[12px]" value={rule.delayMs ?? 0} onChange={(delayMs) => update(i, { delayMs })} />
                  ms
                </label>
              </div>
            </div>
          </div>
        );
      })}
      <div className="flex items-center gap-3">
        <Button size="sm" icon={<Plus size={13} />} onClick={() => onChange([...rules, { event: "", ack: "", replyEvent: "", replyArgs: "" }])}>
          Add rule
        </Button>
        <span className="text-[11.5px] text-faint">
          Arguments are JSON. Answers can use <code className="font-mono">{"{{event.args}}"}</code>, <code className="font-mono">{"{{event.arg0}}"}</code>,{" "}
          <code className="font-mono">{"{{event.name}}"}</code> (as JSON), <code className="font-mono">{"{{$uuid}}"}</code> and environment variables.
        </span>
      </div>
    </div>
  );
}

const cell = "h-8 w-full min-w-0 bg-transparent px-2 font-mono text-[12px] text-fg outline-none placeholder:text-faint";

/** Events every client of a namespace gets on a timer: tickers, notifications, presence. */
function ScheduleEditor({ schedule, onChange }: { schedule: SocketIoScheduled[]; onChange: (s: SocketIoScheduled[]) => void }) {
  const update = (i: number, patch: Partial<SocketIoScheduled>) => onChange(schedule.map((m, j) => (j === i ? { ...m, ...patch } : m)));
  return (
    <div className="flex flex-col gap-2" data-testid="socketio-schedule">
      {schedule.length > 0 && (
        <div className="overflow-hidden rounded-lg border border-line text-[12.5px]" role="table" aria-label="Scheduled events">
          {schedule.map((m, i) => (
            <div key={i} role="row" className={cx("grid grid-cols-[28px_110px_minmax(0,1fr)_76px_104px_32px] items-center border-b border-line/70 last:border-b-0", m.enabled === false && "opacity-50")}>
              <span role="cell" className="flex justify-center">
                <Checkbox checked={m.enabled !== false} onChange={(v) => update(i, { enabled: v ? undefined : false })} title="Emit this event" />
              </span>
              <span role="cell" className="border-l border-line/70">
                <input aria-label="Scheduled event" className={cell} value={m.event} placeholder="price" onChange={(e) => update(i, { event: e.target.value })} />
              </span>
              <span role="cell" className="border-l border-line/70">
                <input
                  aria-label="Scheduled event arguments"
                  className={cell}
                  value={m.args ?? ""}
                  placeholder='{"price": {{$randomPrice(5, 50)}}}'
                  onChange={(e) => update(i, { args: e.target.value })}
                />
              </span>
              <span role="cell" className="border-l border-line/70">
                <input
                  aria-label="Scheduled event namespace"
                  title="Namespace"
                  className={cell}
                  value={m.namespace ?? "/"}
                  placeholder="/"
                  onChange={(e) => update(i, { namespace: e.target.value.trim() === "/" ? undefined : e.target.value })}
                />
              </span>
              <span role="cell" className="flex items-center border-l border-line/70 pr-2 text-[11.5px] text-faint">
                <NumberInput bare aria-label="Every (ms)" min={10} className={cell} value={m.everyMs} onChange={(everyMs) => update(i, { everyMs: everyMs ?? 10 })} />
                ms
              </span>
              <span role="cell" className="flex justify-center border-l border-line/70">
                <button aria-label="Remove scheduled event" onClick={() => onChange(schedule.filter((_, j) => j !== i))} className="rounded p-1 text-faint hover:bg-hover hover:text-danger">
                  <Trash2 size={13} />
                </button>
              </span>
            </div>
          ))}
        </div>
      )}
      <div className="flex items-center gap-3">
        <Button size="sm" icon={<Plus size={13} />} onClick={() => onChange([...schedule, { event: "", everyMs: 1000 }])}>
          Add scheduled event
        </Button>
        <span className="text-[11.5px] text-faint">
          Emitted to every client of the namespace every so many milliseconds, in any answer mode. Arguments are JSON; variables like{" "}
          <code className="font-mono">{"{{$isoTimestamp}}"}</code> are filled in each time.
        </span>
      </div>
    </div>
  );
}

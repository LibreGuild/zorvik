// WebSocket server settings: how to answer messages, a greeting, and messages sent on a timer.
import { Plus, Trash2 } from "lucide-react";
import type { ScheduledMessage } from "../../bindings/ScheduledMessage";
import type { WsServerConfig } from "../../bindings/WsServerConfig";
import { Button, Checkbox, cx, Field, NumberInput, Segmented } from "../ui";
import type { ServerEditorProps } from "./kinds";
import { ReplyRulesEditor } from "./ReplyRulesEditor";
import { EditorSection } from "./ServerView";
import { REPLY_MODES } from "./SocketServerEditor";

export function WsServerEditor({ server, onChange, running }: ServerEditorProps) {
  const ws: WsServerConfig = server.websocket ?? {};
  const set = (patch: Partial<WsServerConfig>) => onChange((s) => ({ ...s, websocket: { ...(s.websocket ?? {}), ...patch } }));
  const mode = ws.mode ?? "echo";

  return (
    <>
      <EditorSection title="Replies">
        <div>
          <Segmented items={REPLY_MODES.map((m) => ({ id: m.id, label: m.label }))} value={mode} onChange={(mode) => set({ mode })} />
        </div>
        <p className="text-[12px] text-muted">
          {REPLY_MODES.find((m) => m.id === mode)?.hint}
          {mode === "echo" && " Binary messages come back as binary."}
          {mode === "rules" && " Text and binary messages are matched as text; replies are text."}
        </p>
        {mode === "rules" && <ReplyRulesEditor rules={ws.rules ?? []} onChange={(rules) => set({ rules })} closing />}
      </EditorSection>

      <EditorSection title="Greeting">
        <Field label="Sent to each client when it connects" hint="Empty: none. Can use {{$uuid}}, {{$timestamp}} and environment variables.">
          <textarea
            aria-label="Greeting"
            value={ws.greeting ?? ""}
            spellCheck={false}
            rows={Math.min(8, Math.max(2, (ws.greeting ?? "").split("\n").length))}
            placeholder={'{"type": "welcome", "session": "{{$uuid}}"}'}
            onChange={(e) => set({ greeting: e.target.value })}
            className="w-full resize-y rounded-lg border border-line bg-input px-2.5 py-1.5 font-mono text-[12.5px] text-fg outline-none placeholder:text-faint hover:border-line-strong focus:border-accent focus:ring-2 focus:ring-accent-soft"
          />
        </Field>
      </EditorSection>

      <EditorSection title="Scheduled messages">
        <ScheduleEditor schedule={ws.schedule ?? []} onChange={(schedule) => set({ schedule: schedule.length ? schedule : undefined })} />
      </EditorSection>

      <EditorSection title="Clients">
        <p className="text-[12px] text-muted">
          Clients can connect on any path{running ? (
            <>
              , e.g. <span className="selectable font-mono text-fg">{running.url}/</span>
            </>
          ) : null}
          . Messages up to 16 MB; pings are answered. Send to one client or all of them from the traffic panel.
        </p>
      </EditorSection>
    </>
  );
}

const scheduleCell = "h-8 w-full min-w-0 bg-transparent px-2 font-mono text-[12px] text-fg outline-none placeholder:text-faint";

/** Messages every client gets on a timer: tickers, heartbeats, notifications. */
function ScheduleEditor({ schedule, onChange }: { schedule: ScheduledMessage[]; onChange: (s: ScheduledMessage[]) => void }) {
  const update = (i: number, patch: Partial<ScheduledMessage>) => onChange(schedule.map((m, j) => (j === i ? { ...m, ...patch } : m)));
  return (
    <div className="flex flex-col gap-2" data-testid="ws-schedule">
      {schedule.length > 0 && (
        <div className="overflow-hidden rounded-lg border border-line text-[12.5px]" role="table" aria-label="Scheduled messages">
          {schedule.map((m, i) => (
            <div key={i} role="row" className={cx("grid grid-cols-[28px_1fr_120px_32px] items-center border-b border-line/70 last:border-b-0", m.enabled === false && "opacity-50")}>
              <span role="cell" className="flex justify-center">
                <Checkbox checked={m.enabled !== false} onChange={(v) => update(i, { enabled: v ? undefined : false })} title="Send this message" />
              </span>
              <span role="cell" className="border-l border-line/70">
                <input
                  aria-label="Scheduled message"
                  className={scheduleCell}
                  value={m.message}
                  placeholder='{"price": {{$randomPrice(5, 50)}}, "at": "{{$isoTimestamp}}"}'
                  onChange={(e) => update(i, { message: e.target.value })}
                />
              </span>
              <span role="cell" className="flex items-center border-l border-line/70 pr-2 text-[11.5px] text-faint">
                <NumberInput bare aria-label="Every (ms)" min={10} className={scheduleCell} value={m.everyMs} onChange={(everyMs) => update(i, { everyMs: everyMs ?? 10 })} />
                ms
              </span>
              <span role="cell" className="flex justify-center border-l border-line/70">
                <button aria-label="Remove scheduled message" onClick={() => onChange(schedule.filter((_, j) => j !== i))} className="rounded p-1 text-faint hover:bg-hover hover:text-danger">
                  <Trash2 size={13} />
                </button>
              </span>
            </div>
          ))}
        </div>
      )}
      <div className="flex items-center gap-3">
        <Button size="sm" icon={<Plus size={13} />} onClick={() => onChange([...schedule, { message: "", everyMs: 1000 }])}>
          Add scheduled message
        </Button>
        <span className="text-[11.5px] text-faint">
          Each client gets it every so many milliseconds while connected, in any reply mode. Variables like{" "}
          <code className="font-mono">{"{{$isoTimestamp}}"}</code> are filled in each time.
        </span>
      </div>
    </div>
  );
}

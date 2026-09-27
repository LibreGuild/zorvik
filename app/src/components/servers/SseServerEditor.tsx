// Event stream (SSE) server settings: the events each client gets, their pace,
// and whether they repeat.
import { useState } from "react";
import { ArrowDown, ArrowUp, GripVertical, Plus, Radio, Trash2 } from "lucide-react";
import type { SseEventTemplate } from "../../bindings/SseEventTemplate";
import type { SseServerConfig } from "../../bindings/SseServerConfig";
import { Button, cx, Field, Input, Switch } from "../ui";
import type { ServerEditorProps } from "./kinds";
import { moveItem } from "./MockRoutes";
import { EditorSection } from "./ServerView";

const small = "h-7 min-w-0 rounded-md border border-transparent bg-transparent px-2 text-[12.5px] text-fg outline-none placeholder:text-faint hover:border-line focus:border-accent";

export function SseServerEditor({ server, onChange, running }: ServerEditorProps) {
  const sse: SseServerConfig = server.sse ?? {};
  const events = sse.events ?? [];
  const set = (patch: Partial<SseServerConfig>) => onChange((s) => ({ ...s, sse: { ...(s.sse ?? {}), ...patch } }));
  const setEvents = (fn: (events: SseEventTemplate[]) => SseEventTemplate[]) => onChange((s) => ({ ...s, sse: { ...(s.sse ?? {}), events: fn(s.sse?.events ?? []) } }));
  const update = (i: number, patch: Partial<SseEventTemplate>) => setEvents((es) => es.map((e, j) => (j === i ? { ...e, ...patch } : e)));
  const [dragIndex, setDragIndex] = useState<number | null>(null);
  const interval = sse.intervalMs ?? 0;

  return (
    <>
      <EditorSection
        title={`Events${events.length ? ` · ${events.length}` : ""}`}
        right={
          <Button size="sm" icon={<Plus size={13} />} onClick={() => setEvents((es) => [...es, { event: "", data: "" }])}>
            Add event
          </Button>
        }
      >
        {events.length === 0 ? (
          <div className="flex flex-col items-center gap-1.5 rounded-lg border border-dashed border-line px-4 py-6 text-center">
            <Radio size={20} className="text-faint" />
            <div className="text-[12.5px] font-medium text-fg">No events</div>
            <div className="max-w-xs text-[12px] text-muted">Streams stay open and only get what you send from the traffic panel.</div>
          </div>
        ) : (
          <div className="flex flex-col gap-2" aria-label="Events" role="list">
            {events.map((e, i) => (
              <div
                key={i}
                role="listitem"
                data-testid={`sse-event-${i}`}
                onDragOver={(ev) => {
                  if (dragIndex !== null) ev.preventDefault();
                }}
                onDrop={(ev) => {
                  ev.preventDefault();
                  if (dragIndex !== null && dragIndex !== i) setEvents((es) => moveItem(es, dragIndex, i));
                  setDragIndex(null);
                }}
                className={cx("group overflow-hidden rounded-lg border border-line bg-panel/40 has-[textarea:focus]:border-accent", dragIndex === i && "opacity-50")}
              >
                <div className="flex items-center gap-1 border-b border-line/70 pr-1">
                  <span
                    draggable
                    onDragStart={(ev) => {
                      ev.dataTransfer.effectAllowed = "move";
                      ev.dataTransfer.setData("text/plain", String(i));
                      setDragIndex(i);
                    }}
                    onDragEnd={() => setDragIndex(null)}
                    className="flex w-5 shrink-0 cursor-grab justify-center text-faint opacity-0 group-hover:opacity-100"
                    aria-hidden
                  >
                    <GripVertical size={12} />
                  </span>
                  <span className="w-5 shrink-0 text-right font-mono text-[11px] text-faint">{i + 1}</span>
                  <input aria-label="Event name" className={cx(small, "w-40 font-mono")} value={e.event ?? ""} placeholder="message" onChange={(ev) => update(i, { event: ev.target.value })} />
                  <input aria-label="Event id" className={cx(small, "w-28 font-mono")} value={e.id ?? ""} placeholder="id (optional)" onChange={(ev) => update(i, { id: ev.target.value })} />
                  <div className="flex-1" />
                  <button aria-label="Move up" disabled={i === 0} onClick={() => setEvents((es) => moveItem(es, i, i - 1))} className="rounded p-1 text-faint opacity-0 hover:bg-hover hover:text-fg focus-visible:opacity-100 disabled:invisible group-hover:opacity-100">
                    <ArrowUp size={12} />
                  </button>
                  <button aria-label="Move down" disabled={i === events.length - 1} onClick={() => setEvents((es) => moveItem(es, i, i + 1))} className="rounded p-1 text-faint opacity-0 hover:bg-hover hover:text-fg focus-visible:opacity-100 disabled:invisible group-hover:opacity-100">
                    <ArrowDown size={12} />
                  </button>
                  <button aria-label="Remove event" onClick={() => setEvents((es) => es.filter((_, j) => j !== i))} className="rounded p-1 text-faint hover:bg-hover hover:text-danger">
                    <Trash2 size={13} />
                  </button>
                </div>
                <textarea
                  aria-label="Event data"
                  value={e.data}
                  spellCheck={false}
                  rows={Math.min(10, Math.max(2, e.data.split("\n").length))}
                  placeholder={'{"n": 1, "at": "{{$isoTimestamp}}"}'}
                  onChange={(ev) => update(i, { data: ev.target.value })}
                  className="block w-full resize-y bg-transparent px-2.5 py-1.5 font-mono text-[12.5px] text-fg outline-none placeholder:text-faint"
                />
              </div>
            ))}
          </div>
        )}
        <p className="text-[11.5px] text-faint">
          Data can use <code className="font-mono">{"{{$uuid}}"}</code>, <code className="font-mono">{"{{request.query.name}}"}</code> and environment variables; each line
          becomes a <code className="font-mono">data:</code> line.
        </p>
      </EditorSection>

      <EditorSection title="Pace">
        <div className="grid grid-cols-2 items-start gap-3">
          <Field label="Interval (ms)" hint="Pause between events; 0 sends them all at once.">
            <Input
              type="number"
              min={0}
              className="font-mono"
              aria-label="Interval in milliseconds"
              value={interval}
              onChange={(ev) => set({ intervalMs: Math.max(0, Math.trunc(Number(ev.target.value)) || 0) })}
            />
          </Field>
          <div className="flex flex-col gap-1.5 pt-6">
            <Switch checked={sse.repeat ?? false} onChange={(repeat) => set({ repeat })} label="Repeat" />
            <span className="text-[11.5px] text-faint">{interval > 0 ? "Start over after the last event." : "Start over after the last event, one round a second."}</span>
          </div>
        </div>
      </EditorSection>

      <EditorSection title="Clients">
        <p className="text-[12px] text-muted">
          Any GET path opens a stream{running ? (
            <>
              , e.g. <span className="selectable font-mono text-fg">{running.url}/events</span>
            </>
          ) : null}
          . A reconnecting client (Last-Event-ID) continues after the last event it got. Send more events to one client or all of them from the traffic panel.
        </p>
      </EditorSection>
    </>
  );
}

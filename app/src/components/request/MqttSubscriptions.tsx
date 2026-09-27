// MQTT subscriptions: topic filters subscribed on connect. While connected, turning
// a row on or off, changing its QoS or topic, adding or removing it subscribes or
// unsubscribes right away.
import { useMemo, useRef } from "react";
import { ChevronDown, Trash2 } from "lucide-react";
import type { MqttSubscription } from "../../bindings/MqttSubscription";
import type { Request } from "../../bindings/Request";
import { api, errorMessage } from "../../lib/rpc";
import { type Tab, updateDraft } from "../../store/tabs";
import { toast } from "../../store/toasts";
import { VarInput } from "../VarInput";
import { Checkbox, cx, Tooltip } from "../ui";
import type { KindPaneProps } from "./kinds";
import { filterProblem, liveSubscriptions, mqttOf } from "./mqttModel";

async function sendSubscription(tab: Tab, action: "subscribe" | "unsubscribe", filter: string, qos: number) {
  try {
    const topic = (filter.includes("{{") ? await api.renderVariables(filter) : filter).trim();
    if (!topic) return;
    await api.socketSend(tab.stream.connId, action === "subscribe" ? { type: "subscribe", topic, qos } : { type: "unsubscribe", topic });
  } catch (e) {
    toast("error", action === "subscribe" ? "Could not subscribe" : "Could not unsubscribe", errorMessage(e));
  }
}

const qosSelect =
  "h-7 w-full appearance-none bg-transparent px-2 text-[12px] text-fg outline-none hover:bg-hover/60 focus:bg-hover/60 disabled:opacity-50";

export function MqttSubscriptions({ tab }: KindPaneProps) {
  const rows = mqttOf(tab.draft).subscriptions ?? [];
  const open = tab.stream.status === "open";
  const live = useMemo(() => (open ? liveSubscriptions(tab.stream.messages) : null), [open, tab.stream.messages]);
  // Topic of the row being edited, as it was when the field got focus.
  const editing = useRef<{ index: number; topic: string } | null>(null);
  const display: MqttSubscription[] = [...rows, { topic: "", qos: 0 }];

  const save = (next: MqttSubscription[]) =>
    updateDraft(tab.id, (r: Request) => ({ ...r, mqtt: { ...mqttOf(r), subscriptions: next.length ? next : undefined } }));
  const patch = (index: number, change: Partial<MqttSubscription>) =>
    save(display.map((row, i) => (i === index ? { ...row, ...change } : row)).filter((row, i) => i < rows.length || row.topic !== ""));

  const commitTopic = (index: number) => {
    const before = editing.current;
    editing.current = null;
    const row = rows[index];
    if (!open || !row || before?.index !== index || row.enabled === false) return;
    if (before.topic.trim() === row.topic.trim()) return;
    if (before.topic.trim()) void sendSubscription(tab, "unsubscribe", before.topic, row.qos);
    if (row.topic.trim() && !filterProblem(row.topic)) void sendSubscription(tab, "subscribe", row.topic, row.qos);
  };

  return (
    <div className="flex max-w-3xl flex-col gap-2 py-3">
      <p className="px-4 pb-1 text-[12.5px] text-muted">
        Topic filters to receive, e.g. <code className="font-mono">sensors/+/temperature</code> or <code className="font-mono">devices/#</code>.{" "}
        {open ? "Changes apply right away on this connection." : "Subscribed right after connecting."}
      </p>
      <div className="mx-3 overflow-hidden rounded-lg border border-line text-[12.5px]" role="table" aria-label="Subscriptions">
        <div role="row" className="flex items-center border-b border-line/70 text-[11px] uppercase tracking-wide text-faint">
          <div role="columnheader" className="w-8" />
          <div role="columnheader" className="min-w-0 flex-1 px-2 py-1.5 font-semibold">
            Topic filter
          </div>
          <div role="columnheader" className="w-[84px] border-l border-line/70 px-2 py-1.5 font-semibold">
            QoS
          </div>
          <div role="columnheader" className="w-14" />
        </div>
        {display.map((row, i) => {
          const isNew = i === rows.length;
          const enabled = row.enabled !== false;
          const problem = filterProblem(row.topic);
          const topic = row.topic.trim();
          const granted = live && !topic.includes("{{") ? live.get(topic) : undefined;
          return (
            <div
              key={i}
              role="row"
              className={cx("group flex items-center border-b border-line/70 last:border-b-0 hover:bg-hover/40", !enabled && !isNew && "opacity-55")}
            >
              <div role="cell" className="flex w-8 justify-center">
                {!isNew && (
                  <Checkbox
                    checked={enabled}
                    title={enabled ? "Turn off" : "Turn on"}
                    onChange={(on) => {
                      patch(i, { enabled: on ? undefined : false });
                      if (open && topic && !problem) void sendSubscription(tab, on ? "subscribe" : "unsubscribe", row.topic, row.qos);
                    }}
                  />
                )}
              </div>
              <div
                role="cell"
                className={cx("min-w-0 flex-1", problem && "bg-danger/5")}
                onFocus={() => {
                  if (editing.current?.index !== i) editing.current = { index: i, topic: row.topic };
                }}
              >
                <Tooltip content={problem}>
                  <div>
                    <VarInput
                      value={row.topic}
                      onChange={(v) => patch(i, { topic: v })}
                      onBlur={() => commitTopic(i)}
                      onEnter={() => commitTopic(i)}
                      placeholder={isNew ? "Add a topic filter…" : "topic/#"}
                      ariaLabel="Topic filter"
                    />
                  </div>
                </Tooltip>
              </div>
              <div role="cell" className="relative w-[84px] border-l border-line/70">
                <select
                  aria-label="QoS"
                  className={qosSelect}
                  value={row.qos}
                  disabled={isNew}
                  onChange={(e) => {
                    const qos = Number(e.target.value);
                    patch(i, { qos });
                    if (open && enabled && topic && !problem) void sendSubscription(tab, "subscribe", row.topic, qos);
                  }}
                >
                  <option value={0}>QoS 0</option>
                  <option value={1}>QoS 1</option>
                  <option value={2}>QoS 2</option>
                </select>
                <ChevronDown size={12} className="pointer-events-none absolute right-1.5 top-1/2 -translate-y-1/2 text-faint" />
              </div>
              <div role="cell" className="flex w-14 items-center justify-end gap-1 pr-2">
                {live && !isNew && enabled && topic && (
                  <Tooltip content={granted !== undefined ? `Subscribed (QoS ${granted})` : "Not subscribed on this connection"}>
                    <span className={cx("h-2 w-2 rounded-full", granted !== undefined ? "bg-success" : "bg-faint/50")} />
                  </Tooltip>
                )}
                {!isNew && (
                  <button
                    aria-label="Remove subscription"
                    onClick={() => {
                      save(rows.filter((_, j) => j !== i));
                      if (open && enabled && topic && !problem) void sendSubscription(tab, "unsubscribe", row.topic, row.qos);
                    }}
                    className="rounded p-1 text-faint opacity-0 hover:bg-hover hover:text-danger focus:opacity-100 group-hover:opacity-100"
                  >
                    <Trash2 size={13} />
                  </button>
                )}
              </div>
            </div>
          );
        })}
      </div>
      {display.some((r) => filterProblem(r.topic)) && (
        <p className="px-4 text-[11.5px] text-danger">{display.map((r) => filterProblem(r.topic)).find(Boolean)}</p>
      )}
    </div>
  );
}

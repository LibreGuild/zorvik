// MQTT composer controls: the topic to publish to, QoS and retain. Publishing
// itself is the composer's Send (store/tabs.ts wsSend).
import { ChevronDown } from "lucide-react";
import type { MqttOptions } from "../../bindings/MqttOptions";
import { updateDraft } from "../../store/tabs";
import { VarInput } from "../VarInput";
import { Checkbox, cx, Tooltip } from "../ui";
import type { KindPaneProps } from "./kinds";
import { mqttOf, topicProblem } from "./mqttModel";

export function MqttTopicField({ tab }: KindPaneProps) {
  const mqtt = mqttOf(tab.draft);
  const set = (patch: Partial<MqttOptions>) => updateDraft(tab.id, (r) => ({ ...r, mqtt: { ...mqttOf(r), ...patch } }));
  const topic = mqtt.topic ?? "";
  // "Enter a topic" is only worth saying once something was typed or the user tries to send.
  const problem = topic ? topicProblem(topic) : null;
  return (
    <div className="flex min-w-[240px] flex-1 basis-[320px] items-center gap-2">
      <Tooltip content={problem}>
        <div
          className={cx(
            "flex h-7 min-w-0 flex-1 items-center rounded-md border bg-panel-2 focus-within:border-accent",
            problem ? "border-danger" : "border-line",
          )}
        >
          <span className="shrink-0 pl-2 text-[11px] font-semibold text-faint">Topic</span>
          <VarInput value={topic} onChange={(t) => set({ topic: t })} placeholder="devices/42/state" className="flex-1" ariaLabel="Topic" />
        </div>
      </Tooltip>
      <div className="relative shrink-0">
        <select
          aria-label="QoS"
          value={mqtt.qos}
          onChange={(e) => set({ qos: Number(e.target.value) })}
          className="h-7 appearance-none rounded-md border border-line bg-panel-2 pl-2 pr-6 text-[12px] text-fg outline-none hover:border-line-strong focus:border-accent"
        >
          <option value={0}>QoS 0</option>
          <option value={1}>QoS 1</option>
          <option value={2}>QoS 2</option>
        </select>
        <ChevronDown size={12} className="pointer-events-none absolute right-1.5 top-1/2 -translate-y-1/2 text-faint" />
      </div>
      <label className="flex shrink-0 cursor-pointer items-center gap-1.5 text-[12px] text-muted" title="The broker keeps the last retained message and sends it to new subscribers">
        <Checkbox checked={mqtt.retain ?? false} onChange={(retain) => set({ retain: retain || undefined })} label="Retain" />
        Retain
      </label>
    </div>
  );
}

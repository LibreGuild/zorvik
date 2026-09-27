// MQTT connection settings: client id, protocol version, session and keep alive.
import type { MqttOptions as Options } from "../../bindings/MqttOptions";
import type { MqttVersion } from "../../bindings/MqttVersion";
import { updateDraft } from "../../store/tabs";
import { VarInput } from "../VarInput";
import { Banner, Field, Input, Select, Switch } from "../ui";
import type { KindPaneProps } from "./kinds";
import { mqttOf } from "./mqttModel";

export function MqttOptions({ tab }: KindPaneProps) {
  const mqtt = mqttOf(tab.draft);
  const set = (patch: Partial<Options>) => updateDraft(tab.id, (r) => ({ ...r, mqtt: { ...mqttOf(r), ...patch } }));
  const live = tab.stream.status === "open" || tab.stream.status === "connecting";
  return (
    <div className="flex max-w-2xl flex-col gap-4 p-4">
      <p className="text-[12.5px] text-muted">
        Address: <code className="font-mono">mqtt://host:1883</code>, or <code className="font-mono">mqtts://host:8883</code> for TLS (verified with
        this computer's trusted certificates and Settings → Certificates). The user name and password come from Basic auth in the Auth tab.
        Connections are direct, without the proxy.
      </p>
      {live && (
        <div className="-mx-3 -mt-2">
          <Banner tone="info">Connection settings apply the next time you connect.</Banner>
        </div>
      )}
      <div className="grid grid-cols-2 gap-3">
        <Field label="Client ID" hint="Empty: a new random ID (zorvik-…) on every connect. Brokers drop an older connection with the same ID.">
          <div className="rounded-lg border border-line bg-input focus-within:border-accent">
            <VarInput value={mqtt.clientId ?? ""} onChange={(clientId) => set({ clientId })} placeholder="random" ariaLabel="Client ID" />
          </div>
        </Field>
        <Field label="MQTT version">
          <Select value={mqtt.version ?? "v311"} onChange={(e) => set({ version: e.target.value as MqttVersion })}>
            <option value="v311">3.1.1</option>
            <option value="v5">5</option>
          </Select>
        </Field>
        <Field label="Keep alive (seconds)" hint="A ping goes out when nothing else was sent for this long; the connection closes when the broker stops answering. 0 = off.">
          <Input
            type="number"
            min={0}
            max={65535}
            value={mqtt.keepAliveSecs}
            onChange={(e) => set({ keepAliveSecs: Math.min(65535, Math.max(0, Math.trunc(Number(e.target.value)) || 0)) })}
          />
        </Field>
      </div>
      <Switch checked={mqtt.cleanSession ?? true} onChange={(cleanSession) => set({ cleanSession })} label="Clean session" />
      <p className="-mt-2 pl-[46px] text-[11.5px] text-faint">
        Off: the broker keeps your subscriptions and queued QoS 1/2 messages while you are away (MQTT 5: for up to an hour). Use a fixed client
        ID for that.
      </p>
    </div>
  );
}

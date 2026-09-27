// MQTT request helpers without React: defaults, topic checks, live subscriptions.
import type { MqttOptions } from "../../bindings/MqttOptions";
import type { Request } from "../../bindings/Request";
import type { StreamMessage } from "../../store/tabs";

/** The request's MQTT options with the model defaults filled in. */
export function mqttOf(request: Request): MqttOptions {
  return { keepAliveSecs: 30, qos: 0, ...(request.mqtt ?? {}) };
}

/** Why a topic cannot be published to, or null. Variables are checked after rendering. */
export function topicProblem(topic: string): string | null {
  if (topic.includes("{{")) return null;
  if (!topic.trim()) return "Enter a topic";
  if (/[+#]/.test(topic)) return "Wildcards (+ and #) are only for subscriptions";
  return null;
}

/** Why a topic filter cannot be subscribed to, or null. */
export function filterProblem(filter: string): string | null {
  const f = filter.trim();
  if (f.includes("{{") || !f) return null;
  const levels = f.split("/");
  for (let i = 0; i < levels.length; i++) {
    const level = levels[i];
    if (level.length > 1 && /[+#]/.test(level)) return "+ and # must be a whole level, e.g. sensors/+/temp";
    if (level === "#" && i !== levels.length - 1) return "# must be the last level, e.g. sensors/#";
  }
  return null;
}

/**
 * Topic filters the broker confirmed on the current connection, with the granted QoS,
 * read from the connection log ("Subscribed to …" / "Unsubscribed from …" lines after
 * the last "Connecting to …"). Null when the log does not cover the connection (cleared).
 */
export function liveSubscriptions(messages: StreamMessage[]): Map<string, number> | null {
  let start = -1;
  for (let i = messages.length - 1; i >= 0; i--) {
    if (messages[i].direction === "info" && messages[i].text?.startsWith("Connecting to ")) {
      start = i;
      break;
    }
  }
  if (start < 0) return null;
  const live = new Map<string, number>();
  for (let i = start + 1; i < messages.length; i++) {
    const m = messages[i];
    if (m.direction !== "info" || !m.text) continue;
    const sub = /^Subscribed to (.+) \(QoS (\d)/.exec(m.text);
    if (sub) {
      live.set(sub[1], Number(sub[2]));
      continue;
    }
    const unsub = /^Unsubscribed from (.+?)(?: \([^()]*\))?$/.exec(m.text);
    if (unsub) live.delete(unsub[1]);
  }
  return live;
}

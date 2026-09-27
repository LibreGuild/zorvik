import { describe, expect, it } from "vitest";
import type { StreamMessage } from "../../store/tabs";
import { filterProblem, liveSubscriptions, mqttOf, topicProblem } from "./mqttModel";

const info = (text: string, id: number): StreamMessage => ({ id, direction: "info", kind: "info", text, base64: null, size: 0, timestamp: id });

describe("mqtt helpers", () => {
  it("fills in defaults", () => {
    expect(mqttOf({ name: "m", seq: 0, method: "GET", url: "" })).toEqual({ keepAliveSecs: 30, qos: 0 });
    expect(mqttOf({ name: "m", seq: 0, method: "GET", url: "", mqtt: { keepAliveSecs: 5, qos: 1, topic: "t" } }).topic).toBe("t");
  });

  it("checks topics and filters", () => {
    expect(topicProblem("a/b")).toBeNull();
    expect(topicProblem(" ")).toBe("Enter a topic");
    expect(topicProblem("a/+")).toContain("Wildcards");
    expect(topicProblem("{{t}}")).toBeNull();
    expect(filterProblem("a/+/c")).toBeNull();
    expect(filterProblem("a/#")).toBeNull();
    expect(filterProblem("#")).toBeNull();
    expect(filterProblem("a/#/c")).toContain("last level");
    expect(filterProblem("a+")).toContain("whole level");
    expect(filterProblem("")).toBeNull();
  });

  it("reads live subscriptions from the current connection's log", () => {
    const log = [
      info("Connecting to mqtt://old…", 1),
      info("Subscribed to stale (QoS 0)", 2),
      info("Disconnected", 3),
      info("Connecting to mqtt://broker…", 4),
      info("Subscribed to a/# (QoS 1)", 5),
      info("Subscribed to b/+ (QoS 1; asked for 2)", 6),
      info("Subscribed to c (QoS 0)", 7),
      info("Unsubscribed from c", 8),
      info("Unsubscribed from x (no subscription existed)", 9),
    ];
    expect([...liveSubscriptions(log)!.entries()]).toEqual([
      ["a/#", 1],
      ["b/+", 1],
    ]);
    expect(liveSubscriptions(log.slice(1, 3))).toBeNull();
  });
});

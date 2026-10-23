import { describe, expect, it } from "vitest";
import { TOPICS } from "./content";
import { TOPIC_ICONS } from "./media";

describe("docs topics", () => {
  it("each has an icon in the sidebar and on its section", () => {
    const missing = TOPICS.filter((t) => typeof TOPIC_ICONS[t.id] !== "function").map((t) => t.id);
    expect(missing).toEqual([]);
  });
});

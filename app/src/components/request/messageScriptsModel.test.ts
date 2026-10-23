import { describe, expect, it } from "vitest";
import { compact, scriptLabel } from "./messageScriptsModel";

describe("message scripts", () => {
  // The same labels the backend gives in the log and in run results.
  it("are labelled by what runs them, or their name", () => {
    expect(scriptLabel({ script: "" })).toBe("On any message");
    expect(scriptLabel({ script: "", direction: "sent", match: "contains", pattern: "ping" })).toBe('On a sent message containing "ping"');
    expect(scriptLabel({ script: "", event: "joined" })).toBe('On the event "joined"');
    expect(scriptLabel({ script: "", direction: "sent", event: "chat", match: "json", pattern: "$[0].to == bob" })).toBe(
      'On the sent event "chat" with arguments where $[0].to == bob',
    );
    expect(scriptLabel({ script: "", direction: "both", match: "regex", pattern: "^a" })).toBe("On a message matching /^a/ (sent or received)");
    expect(scriptLabel({ script: "", name: " Prices " })).toBe("Prices");
  });

  it("are saved without default values", () => {
    expect(compact({ script: "x", direction: "received", match: "any", enabled: true, name: " ", event: "" })).toEqual({ script: "x" });
    expect(compact({ script: "x", enabled: false, match: "json", pattern: "$.a" })).toEqual({ script: "x", enabled: false, match: "json", pattern: "$.a" });
  });
});

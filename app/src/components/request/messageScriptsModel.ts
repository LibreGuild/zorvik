// Message scripts of WebSocket and Socket.IO requests: their labels (as the backend names them in
// the log and in run results, crates/api/src/message_scripts.rs) and edits to the list.
import type { MatchKind } from "../../bindings/MatchKind";
import type { MessageScript } from "../../bindings/MessageScript";

/** "On a message containing "ping"", "On the event "joined"", or the script's name. */
export function scriptLabel(s: MessageScript): string {
  if (s.name?.trim()) return s.name.trim();
  const direction = s.direction ?? "received";
  const noun = direction === "sent" ? "sent message" : "message";
  const also = direction === "both" ? " (sent or received)" : "";
  const p = s.pattern ?? "";
  const match: MatchKind = s.match ?? "any";
  const any = match === "any" || (match === "contains" && !p);
  const event = s.event?.trim() ?? "";
  if (event) {
    const which = direction === "sent" ? "sent event" : "event";
    const args = any
      ? ""
      : match === "contains"
        ? ` with arguments containing "${p}"`
        : match === "exact"
          ? ` with arguments exactly ${p}`
          : match === "regex"
            ? ` with arguments matching /${p}/`
            : ` with arguments where ${p.trim()}`;
    return `On the ${which} "${event}"${args}${also}`;
  }
  const target = any
    ? `any ${noun}`
    : match === "contains"
      ? `a ${noun} containing "${p}"`
      : match === "exact"
        ? `the ${noun} "${p}"`
        : match === "regex"
          ? `a ${noun} matching /${p}/`
          : `a JSON ${noun} where ${p.trim()}`;
  return `On ${target}${also}`;
}

/** A script as saved: default values left out, so files stay small. */
export function compact(s: MessageScript): MessageScript {
  const out: MessageScript = { script: s.script };
  if (s.name?.trim()) out.name = s.name;
  if (s.direction && s.direction !== "received") out.direction = s.direction;
  if (s.event?.trim()) out.event = s.event;
  if (s.match && s.match !== "any") out.match = s.match;
  if (s.pattern) out.pattern = s.pattern;
  if (s.enabled === false) out.enabled = false;
  return out;
}

export const NEW_SCRIPT: MessageScript = {
  script: 'pm.test("Got a message", () => {\n  pm.expect(pm.message.text()).to.not.be.empty;\n});\n',
};

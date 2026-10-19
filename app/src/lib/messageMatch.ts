// Matching messages in the UI, with the grammar the backend uses (crates/formats/src/matching.rs):
// JSON paths that pick one value ($.items[0].id), conditions ($.type == "welcome"), the
// message log's filter, and pairing replies with the messages they answer.
import type { MatchKind } from "../bindings/MatchKind";

type Step = string | number;

/** `$.a.b[0]['c d']` as steps, or an error message. `$` may be left out. */
export function parsePath(path: string): Step[] | string {
  const bad = (why: string) => `'${path}' is not a JSON path (${why}); use one like $.items[0].id`;
  let rest = path.trim();
  if (!rest) return "give a JSON path, e.g. $.id";
  const dollar = rest.startsWith("$");
  if (dollar) rest = rest.slice(1);
  const steps: Step[] = [];
  let first = !dollar;
  while (rest) {
    if (rest.startsWith("[")) {
      const end = bracketEnd(rest.slice(1));
      if (end < 0) return bad("a [ is never closed");
      const inner = rest.slice(1, end + 1).trim();
      rest = rest.slice(end + 2);
      const quote = inner[0];
      if (quote === "'" || quote === '"') {
        if (inner.length < 2 || !inner.endsWith(quote)) return bad("a quoted name is not closed");
        steps.push(inner.slice(1, -1).split(`\\${quote}`).join(quote));
      } else {
        if (!/^-?\d+$/.test(inner)) return bad("[…] needs a number or a quoted name");
        steps.push(Number(inner));
      }
    } else {
      let after: string;
      if (rest.startsWith(".")) after = rest.slice(1);
      else if (first) after = rest;
      else return bad("expected . or [");
      if (after.startsWith(".")) return bad("wildcards and .. are not supported");
      const m = after.search(/[.[]/);
      const end = m < 0 ? after.length : m;
      const key = after.slice(0, end).trim();
      if (!key) return bad("a name is empty");
      if (key === "*") return bad("wildcards and .. are not supported");
      steps.push(key);
      rest = after.slice(end);
    }
    first = false;
  }
  return steps;
}

function bracketEnd(s: string): number {
  let quote: string | null = null;
  let escaped = false;
  for (let i = 0; i < s.length; i++) {
    const c = s[i];
    if (quote) {
      if (escaped) escaped = false;
      else if (c === "\\") escaped = true;
      else if (c === quote) quote = null;
    } else if (c === "'" || c === '"') quote = c;
    else if (c === "]") return i;
  }
  return -1;
}

/** The value at `steps` (undefined when it isn't there). */
export function valueAt(root: unknown, steps: Step[]): unknown {
  let at: unknown = root;
  for (const step of steps) {
    if (typeof step === "number") {
      if (!Array.isArray(at)) return undefined;
      at = at[step < 0 ? at.length + step : step];
    } else {
      // Own keys only: `$.constructor` is not a value of the message.
      if (at === null || typeof at !== "object" || Array.isArray(at) || !Object.hasOwn(at, step)) return undefined;
      at = (at as Record<string, unknown>)[step];
    }
    if (at === undefined) return undefined;
  }
  return at;
}

/** A value as text: strings as they are, others as JSON; null or nothing: null. */
export function valueText(value: unknown): string | null {
  if (value === undefined || value === null) return null;
  return typeof value === "string" ? value : JSON.stringify(value);
}

export interface Condition {
  steps: Step[];
  /** [equal, expected text (null: null)], or undefined for "is there". */
  compare?: [boolean, string | null];
}

/** `$.path`, `$.path == value` or `$.path != value`; the value is JSON or bare text. */
export function parseCondition(pattern: string): Condition | string {
  let quote: string | null = null;
  for (let i = 0; i < pattern.length; i++) {
    const c = pattern[i];
    if (quote) {
      if (c === quote) quote = null;
    } else if (c === "'" || c === '"') quote = c;
    else if ((c === "=" || c === "!") && pattern[i + 1] === "=") {
      const steps = parsePath(pattern.slice(0, i));
      if (typeof steps === "string") return steps;
      const raw = pattern.slice(i + 2).trim();
      let expected: string | null;
      try {
        expected = valueText(JSON.parse(raw));
      } catch {
        // 'text' in single quotes, like the paths take.
        expected = raw.length >= 2 && raw.startsWith("'") && raw.endsWith("'") ? raw.slice(1, -1) : raw;
      }
      return { steps, compare: [c === "=", expected] };
    }
  }
  const steps = parsePath(pattern);
  return typeof steps === "string" ? steps : { steps };
}

export function conditionHolds(condition: Condition, root: unknown): boolean {
  const actual = valueText(valueAt(root, condition.steps));
  if (!condition.compare) return actual !== null;
  const [equal, expected] = condition.compare;
  return (actual === expected) === equal;
}

/**
 * Why a pattern can't be used, or null when it can. Regular expressions are checked by the
 * backend's engine (see `useRegexProblem` in regexCheck.ts), which differs from JavaScript's.
 */
export function patternProblem(kind: MatchKind, pattern: string): string | null {
  if (kind === "json") {
    const c = parseCondition(pattern);
    return typeof c === "string" ? c : null;
  }
  return null;
}

export function parseJson(text: string | null | undefined): unknown {
  if (!text) return undefined;
  const t = text.trim();
  if (!t.startsWith("{") && !t.startsWith("[")) return undefined;
  try {
    return JSON.parse(t);
  } catch {
    return undefined;
  }
}

/**
 * The message log's filter: words to look for in the text, or a JSON condition when it
 * starts with `$` (`$.type == trade`). Returns a test for a message's text (null: no filter).
 */
export function logFilter(query: string): ((text: string | null, json: () => unknown) => boolean) | null {
  const q = query.trim();
  if (!q) return null;
  if (q.startsWith("$")) {
    const c = parseCondition(q);
    if (typeof c !== "string") return (_text, json) => conditionHolds(c, json());
  }
  const lower = q.toLowerCase();
  return (text) => (text ?? "").toLowerCase().includes(lower);
}

export interface ReplyPair {
  /** The sent message it answers. */
  to: number;
  ms: number;
}

/**
 * Replies paired with the messages they answer: a received message whose value at `key`
 * equals that of an earlier sent one (the first unanswered with that value).
 */
export function pairReplies(
  messages: { id: number; direction: string; timestamp: number }[],
  json: (id: number) => unknown,
  key: string,
): Map<number, ReplyPair> {
  const steps = parsePath(key || "$.id");
  const pairs = new Map<number, ReplyPair>();
  if (typeof steps === "string") return pairs;
  const waiting = new Map<string, { id: number; at: number }[]>();
  for (const m of messages) {
    if (m.direction !== "sent" && m.direction !== "received") continue;
    const root = json(m.id);
    if (root === undefined) continue;
    const value = valueText(valueAt(root, steps));
    if (value === null) continue;
    if (m.direction === "sent") {
      const list = waiting.get(value) ?? [];
      list.push({ id: m.id, at: m.timestamp });
      waiting.set(value, list);
    } else {
      const sent = waiting.get(value)?.shift();
      if (sent) pairs.set(m.id, { to: sent.id, ms: Math.max(0, m.timestamp - sent.at) });
    }
  }
  return pairs;
}

/**
 * `next` with each pair that `previous` already had (same reply, same message, same time) swapped
 * for the earlier object, so rows whose pair did not change see the same object and skip a render.
 */
export function reusePairs(previous: Map<number, ReplyPair>, next: Map<number, ReplyPair>): Map<number, ReplyPair> {
  for (const [id, pair] of next) {
    const before = previous.get(id);
    if (before && before.to === pair.to && before.ms === pair.ms) next.set(id, before);
  }
  return next;
}

/**
 * Socket.IO acknowledgements paired with the emits that asked for them: a sent message noted
 * "asks for an acknowledgement (#3)" and a received one noted "acknowledgement of #3".
 */
export function pairAcks(messages: { id: number; direction: string; timestamp: number; detail?: string | null }[]): Map<number, ReplyPair> {
  const asked = new Map<string, { id: number; at: number }>();
  const pairs = new Map<number, ReplyPair>();
  for (const m of messages) {
    const ask = m.direction === "sent" && m.detail?.match(/^asks for an acknowledgement \(#(\d+)\)/)?.[1];
    if (ask) {
      asked.set(ask, { id: m.id, at: m.timestamp });
      continue;
    }
    const ack = m.direction === "received" && m.detail?.match(/^acknowledgement of #(\d+)/)?.[1];
    const sent = ack ? asked.get(ack) : undefined;
    if (ack && sent) {
      pairs.set(m.id, { to: sent.id, ms: Math.max(0, m.timestamp - sent.at) });
      asked.delete(ack);
    }
  }
  return pairs;
}

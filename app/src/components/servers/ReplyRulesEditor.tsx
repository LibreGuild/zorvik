// "When a message matches, reply with …" rules, shared by the WebSocket, TCP and UDP servers.
import { Plus, Trash2 } from "lucide-react";
import type { MatchKind } from "../../bindings/MatchKind";
import type { PayloadEncoding } from "../../bindings/PayloadEncoding";
import type { ReplyRule } from "../../bindings/ReplyRule";
import { patternProblem } from "../../lib/messageMatch";
import { useRegexProblem } from "../../lib/regexCheck";
import { Button, Checkbox, cx, NumberInput } from "../ui";

const MATCHERS: { id: MatchKind; label: string }[] = [
  { id: "contains", label: "Contains" },
  { id: "exact", label: "Is exactly" },
  { id: "regex", label: "Matches regex" },
  { id: "json", label: "JSON where" },
  { id: "any", label: "Any message" },
];

const cell = "h-8 w-full min-w-0 bg-transparent px-2 font-mono text-[12px] text-fg outline-none placeholder:text-faint";

/** What's wrong with a rule's pattern (a regex the server can't use, a JSON condition that doesn't parse), shown under it. */
export function PatternProblem({ match, pattern, className }: { match: MatchKind; pattern: string; className?: string }) {
  const regex = useRegexProblem(match === "regex" ? pattern : null);
  const problem = regex ?? patternProblem(match, pattern);
  return problem ? <div className={cx("text-[11px] text-warning", className)}>{problem}</div> : null;
}

/** WebSocket close codes a server may send. */
const sendableClose = (code: number) => code === 1000 || (code >= 3000 && code <= 4999);

export function ReplyRulesEditor({
  rules,
  onChange,
  encoding = "text",
  closing = false,
}: {
  rules: ReplyRule[];
  onChange: (rules: ReplyRule[]) => void;
  encoding?: PayloadEncoding;
  /** WebSocket: a rule can close the connection with a code. */
  closing?: boolean;
}) {
  const update = (i: number, patch: Partial<ReplyRule>) => onChange(rules.map((r, j) => (j === i ? { ...r, ...patch } : r)));
  const hex = encoding === "hex";
  const grid = closing ? "grid-cols-[28px_130px_1fr_1fr_76px_84px_32px]" : "grid-cols-[28px_130px_1fr_1fr_76px_32px]";
  return (
    <div className="flex flex-col gap-2">
      {rules.length > 0 && (
        <div className="overflow-hidden rounded-lg border border-line text-[12.5px]" role="table" aria-label="Reply rules">
          <div role="row" className={cx("grid items-center border-b border-line bg-panel-2/60 text-[11px] font-medium text-faint", grid)}>
            <span role="columnheader" />
            <span role="columnheader" className="px-2">When</span>
            <span role="columnheader" className="px-2">Message</span>
            <span role="columnheader" className="px-2">Reply</span>
            <span role="columnheader" className="px-2">Delay ms</span>
            {closing && (
              <span role="columnheader" className="px-2" title="Close the connection after the reply, with this close code">
                Then close
              </span>
            )}
            <span role="columnheader" />
          </div>
          {rules.map((rule, i) => {
            const enabled = rule.enabled !== false;
            return (
              <div
                key={i}
                role="row"
                className={cx("grid items-center border-b border-line/70 last:border-b-0", grid, !enabled && "opacity-50")}
              >
                <span role="cell" className="flex justify-center">
                  <Checkbox checked={enabled} onChange={(v) => update(i, { enabled: v })} title="Use this rule" />
                </span>
                <span role="cell" className="border-l border-line/70">
                  <select
                    aria-label="Match"
                    value={rule.match ?? "contains"}
                    onChange={(e) => update(i, { match: e.target.value as MatchKind })}
                    className="h-8 w-full bg-transparent px-1.5 text-[12px] text-fg outline-none"
                  >
                    {MATCHERS.map((m) => (
                      <option key={m.id} value={m.id}>
                        {m.label}
                      </option>
                    ))}
                  </select>
                </span>
                <span role="cell" className="border-l border-line/70">
                  <input
                    aria-label="Pattern"
                    className={cell}
                    disabled={rule.match === "any"}
                    value={rule.match === "any" ? "" : rule.pattern}
                    placeholder={
                      rule.match === "any" ? "every message" : rule.match === "regex" ? "^GET (\\w+)" : rule.match === "json" ? '$.op == "ping"' : hex ? "01 02" : "ping"
                    }
                    onChange={(e) => update(i, { pattern: e.target.value })}
                  />
                  <PatternProblem match={rule.match ?? "contains"} pattern={rule.pattern} className="px-2 pb-1" />
                </span>
                <span role="cell" className="border-l border-line/70">
                  <input
                    aria-label="Reply"
                    className={cell}
                    value={rule.reply}
                    placeholder={hex ? "ff 00" : "pong {{message}}"}
                    onChange={(e) => update(i, { reply: e.target.value })}
                  />
                </span>
                <span role="cell" className="border-l border-line/70">
                  <NumberInput bare aria-label="Delay in milliseconds" className={cell} value={rule.delayMs ?? 0} onChange={(delayMs) => update(i, { delayMs })} />
                </span>
                {closing && (
                  <span role="cell" className="border-l border-line/70">
                    <NumberInput
                      bare
                      optional
                      aria-label="Close code"
                      min={1000}
                      max={4999}
                      className={cell}
                      value={rule.close ?? undefined}
                      invalid={rule.close != null && !sendableClose(rule.close)}
                      placeholder="no"
                      title="Close the connection after the reply, with this close code (1000, or 3000-4999)"
                      onChange={(close) => update(i, { close })}
                    />
                  </span>
                )}
                <span role="cell" className="flex justify-center border-l border-line/70">
                  <button aria-label="Remove rule" onClick={() => onChange(rules.filter((_, j) => j !== i))} className="rounded p-1 text-faint hover:bg-hover hover:text-danger">
                    <Trash2 size={13} />
                  </button>
                </span>
              </div>
            );
          })}
        </div>
      )}
      <div className="flex items-center gap-3">
        <Button size="sm" icon={<Plus size={13} />} onClick={() => onChange([...rules, { match: "contains", pattern: "", reply: "" }])}>
          Add rule
        </Button>
        <span className="text-[11.5px] text-faint">
          The first matching rule answers.{" "}
          {!hex && (
            <>
              Replies can use <code className="font-mono">{"{{message}}"}</code>, a value of a JSON message like{" "}
              <code className="font-mono">{"{{message.id}}"}</code>, <code className="font-mono">{"{{$uuid}}"}</code> and environment variables.
            </>
          )}
        </span>
      </div>
    </div>
  );
}

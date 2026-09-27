// "When a message matches, reply with …" rules, shared by the WebSocket, TCP and UDP servers.
import { Plus, Trash2 } from "lucide-react";
import type { MatchKind } from "../../bindings/MatchKind";
import type { PayloadEncoding } from "../../bindings/PayloadEncoding";
import type { ReplyRule } from "../../bindings/ReplyRule";
import { Button, Checkbox, cx } from "../ui";

const MATCHERS: { id: MatchKind; label: string }[] = [
  { id: "contains", label: "Contains" },
  { id: "exact", label: "Is exactly" },
  { id: "regex", label: "Matches regex" },
  { id: "any", label: "Any message" },
];

const cell = "h-8 w-full min-w-0 bg-transparent px-2 font-mono text-[12px] text-fg outline-none placeholder:text-faint";

export function ReplyRulesEditor({
  rules,
  onChange,
  encoding = "text",
}: {
  rules: ReplyRule[];
  onChange: (rules: ReplyRule[]) => void;
  encoding?: PayloadEncoding;
}) {
  const update = (i: number, patch: Partial<ReplyRule>) => onChange(rules.map((r, j) => (j === i ? { ...r, ...patch } : r)));
  const hex = encoding === "hex";
  return (
    <div className="flex flex-col gap-2">
      {rules.length > 0 && (
        <div className="overflow-hidden rounded-lg border border-line text-[12.5px]" role="table" aria-label="Reply rules">
          <div role="row" className="grid grid-cols-[28px_130px_1fr_1fr_76px_32px] items-center border-b border-line bg-panel-2/60 text-[11px] font-medium text-faint">
            <span role="columnheader" />
            <span role="columnheader" className="px-2">When</span>
            <span role="columnheader" className="px-2">Message</span>
            <span role="columnheader" className="px-2">Reply</span>
            <span role="columnheader" className="px-2">Delay ms</span>
            <span role="columnheader" />
          </div>
          {rules.map((rule, i) => {
            const enabled = rule.enabled !== false;
            return (
              <div
                key={i}
                role="row"
                className={cx("grid grid-cols-[28px_130px_1fr_1fr_76px_32px] items-center border-b border-line/70 last:border-b-0", !enabled && "opacity-50")}
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
                    placeholder={rule.match === "any" ? "every message" : rule.match === "regex" ? "^GET (\\w+)" : hex ? "01 02" : "ping"}
                    onChange={(e) => update(i, { pattern: e.target.value })}
                  />
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
                  <input
                    aria-label="Delay in milliseconds"
                    type="number"
                    min={0}
                    className={cell}
                    value={rule.delayMs ?? 0}
                    onChange={(e) => update(i, { delayMs: Math.max(0, Math.trunc(Number(e.target.value)) || 0) })}
                  />
                </span>
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
              Replies can use <code className="font-mono">{"{{message}}"}</code>, <code className="font-mono">{"{{$uuid}}"}</code> and environment
              variables.
            </>
          )}
        </span>
      </div>
    </div>
  );
}

// The captures of one load test target: values taken from its responses as
// variables for the same user's later requests (variable, where from, path).
import { Plus, TriangleAlert, X } from "lucide-react";
import type { CaptureFrom } from "../../bindings/CaptureFrom";
import type { LoadCapture } from "../../bindings/LoadCapture";
import type { LoadModel } from "../../bindings/LoadModel";
import { Button, IconButton, Input } from "../ui";
import { CAPTURE_FROM, captureProblem } from "./model";

const selectClass =
  "h-7 w-full rounded-lg border border-line bg-input px-1.5 text-[12px] text-fg outline-none hover:border-line-strong focus:border-accent focus:ring-2 focus:ring-accent-soft";

export function CapturesEditor({
  captures,
  name,
  model,
  onChange,
}: {
  captures: LoadCapture[];
  name: string;
  model: LoadModel;
  onChange: (captures: LoadCapture[]) => void;
}) {
  const update = (i: number, patch: Partial<LoadCapture>) => onChange(captures.map((c, j) => (j === i ? { ...c, ...patch } : c)));
  return (
    <div className="flex flex-col gap-1.5" data-testid="load-captures">
      {model === "arrivalRate" && captures.length > 0 && (
        <p className="text-[11px] leading-snug text-warning">
          With a request rate, every request is on its own: captured values are checked (misses are counted) but no later request uses them. Use virtual users for
          chains like create, then get.
        </p>
      )}
      {captures.length > 0 && (
        <div className="grid grid-cols-[minmax(0,1fr)_96px_minmax(0,1.4fr)_22px] items-center gap-x-1.5 gap-y-1">
          <span className="text-[10.5px] font-medium text-faint">Variable</span>
          <span className="text-[10.5px] font-medium text-faint">From</span>
          <span className="text-[10.5px] font-medium text-faint">Path, header or pattern</span>
          <span />
          {captures.map((c, i) => {
            const problem = captureProblem(c);
            const from = CAPTURE_FROM.find((f) => f.id === c.from) ?? CAPTURE_FROM[0];
            return (
              <div key={i} className="contents" data-testid="load-capture">
                <Input
                  aria-label={`Variable ${i + 1} of ${name}`}
                  value={c.variable}
                  placeholder="orderId"
                  onChange={(e) => update(i, { variable: e.target.value })}
                  className="h-7 font-mono text-[12px]"
                  invalid={!!problem && problem.includes("variable")}
                />
                <select
                  aria-label={`Where capture ${i + 1} of ${name} comes from`}
                  value={c.from}
                  onChange={(e) => update(i, { from: e.target.value as CaptureFrom })}
                  className={selectClass}
                >
                  {CAPTURE_FROM.map((f) => (
                    <option key={f.id} value={f.id}>
                      {f.label}
                    </option>
                  ))}
                </select>
                <Input
                  aria-label={`${from.label} of capture ${i + 1} of ${name}`}
                  value={c.path}
                  placeholder={from.placeholder}
                  onChange={(e) => update(i, { path: e.target.value })}
                  className="h-7 font-mono text-[12px]"
                  invalid={!!problem && !problem.includes("variable")}
                  title={problem ?? undefined}
                />
                <IconButton label={`Remove capture ${i + 1} of ${name}`} onClick={() => onChange(captures.filter((_, j) => j !== i))} size={22}>
                  <X size={12} />
                </IconButton>
                {problem && (
                  <p className="col-span-4 flex items-center gap-1 text-[11px] text-warning">
                    <TriangleAlert size={11} className="shrink-0" />
                    {problem}
                  </p>
                )}
              </div>
            );
          })}
        </div>
      )}
      <div className="flex flex-wrap items-center gap-2">
        <Button size="sm" variant="ghost" icon={<Plus size={12} />} onClick={() => onChange([...captures, { variable: "", from: "json", path: "" }])}>
          Add capture
        </Button>
        <span className="min-w-0 flex-1 text-[11px] leading-snug text-faint">
          Saves a value from each response as <code className="font-mono">{"{{variable}}"}</code> for this user's next requests. JSON paths look like{" "}
          <code className="font-mono">$.items[0].id</code>; a regex uses its first group.
        </span>
      </div>
    </div>
  );
}

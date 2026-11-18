// The small editors a scenario step is made of: name/value rows, a condition's rules, a
// request step's outputs and checks, and the list that picks an output from a response.
import { useEffect, useMemo, useState } from "react";
import { MousePointerClick, Plus, Search, X } from "lucide-react";
import { Popover } from "radix-ui";
import type { CompareOp } from "../../bindings/CompareOp";
import type { Condition } from "../../bindings/Condition";
import type { RunDetails } from "../../bindings/RunDetails";
import type { StepCheck } from "../../bindings/StepCheck";
import type { StepOutput } from "../../bindings/StepOutput";
import type { StepValue } from "../../bindings/StepValue";
import type { ValueFrom } from "../../bindings/ValueFrom";
import { api } from "../../lib/rpc";
import { VarInput } from "../VarInput";
import { Button, Checkbox, IconButton, Input, Segmented } from "../ui";
import { jsonLeaves, nameForPath, OPS, opTakesValue, outputProblem, SOURCES } from "./model";

export const selectClass =
  "h-7 w-full rounded-lg border border-line bg-input px-1.5 text-[12px] text-fg outline-none hover:border-line-strong focus:border-accent focus:ring-2 focus:ring-accent-soft";

/** A variable input in a bordered box, like the other fields of a row. */
export function ValueInput({ value, onChange, placeholder, label }: { value: string; onChange: (v: string) => void; placeholder?: string; label: string }) {
  return (
    <div className="min-w-0 rounded-lg border border-line bg-input hover:border-line-strong focus-within:border-accent focus-within:ring-2 focus-within:ring-accent-soft">
      <VarInput value={value} onChange={onChange} placeholder={placeholder} ariaLabel={label} />
    </div>
  );
}

/** The label over a group of fields, with an optional hint beside it. */
export function FieldLabel({ children, hint }: { children: React.ReactNode; hint?: React.ReactNode }) {
  return (
    <div className="flex flex-wrap items-baseline gap-x-2">
      <span className="text-[11.5px] font-medium text-muted">{children}</span>
      {hint && <span className="min-w-0 text-[11px] leading-snug text-faint">{hint}</span>}
    </div>
  );
}

const VALUE_HINT = "text, {{variable}} or = JavaScript";

/** Name/value rows (inputs, values to set, a scenario's variables). */
export function ValuesEditor({
  values,
  onChange,
  what,
  keyPlaceholder = "name",
  valuePlaceholder = VALUE_HINT,
  add = "Add value",
  suggest,
}: {
  values: StepValue[];
  onChange: (values: StepValue[]) => void;
  /** For labels: "Input", "Variable". */
  what: string;
  keyPlaceholder?: string;
  valuePlaceholder?: string;
  add?: string;
  /** Names offered as rows to add (the variables a request uses). */
  suggest?: string[];
}) {
  const update = (i: number, patch: Partial<StepValue>) => onChange(values.map((v, j) => (j === i ? { ...v, ...patch } : v)));
  const offered = (suggest ?? []).filter((name) => !values.some((v) => v.key.trim() === name));
  return (
    <div className="flex flex-col gap-1.5" data-testid={`scenario-${what.toLowerCase()}s`}>
      {values.length > 0 && (
        <div className="grid grid-cols-[18px_minmax(0,1fr)_minmax(0,1.6fr)_22px] items-center gap-x-1.5 gap-y-1">
          {values.map((v, i) => (
            <div key={i} className="contents">
              <Checkbox checked={v.enabled !== false} onChange={(enabled) => update(i, { enabled })} label={`Use ${what.toLowerCase()} ${v.key || i + 1}`} />
              <Input
                aria-label={`${what} ${i + 1} name`}
                value={v.key}
                placeholder={keyPlaceholder}
                onChange={(e) => update(i, { key: e.target.value })}
                className="h-7 font-mono text-[12px]"
                invalid={/[\s{}]/.test(v.key.trim())}
              />
              <ValueInput value={v.value} onChange={(value) => update(i, { value })} placeholder={valuePlaceholder} label={`${what} ${i + 1} value`} />
              <IconButton label={`Remove ${what.toLowerCase()} ${v.key || i + 1}`} onClick={() => onChange(values.filter((_, j) => j !== i))} size={22}>
                <X size={12} />
              </IconButton>
            </div>
          ))}
        </div>
      )}
      <div className="flex flex-wrap items-center gap-1.5">
        <Button size="sm" variant="ghost" icon={<Plus size={12} />} onClick={() => onChange([...values, { key: "", value: "" }])}>
          {add}
        </Button>
        {offered.map((name) => (
          <button
            key={name}
            type="button"
            title={`Give {{${name}}} a value for this step`}
            onClick={() => onChange([...values, { key: name, value: "" }])}
            className="rounded-md border border-dashed border-line-strong px-1.5 py-0.5 font-mono text-[11px] text-muted hover:border-accent hover:text-accent"
          >
            + {name}
          </button>
        ))}
      </div>
    </div>
  );
}

/** The rules of a condition: value, comparison, value; all of them or any one. */
export function ConditionEditor({ condition, onChange, label }: { condition: Condition; onChange: (c: Condition) => void; label: string }) {
  const rules = condition.rules ?? [];
  const update = (i: number, patch: Partial<Condition["rules"][number]>) => onChange({ ...condition, rules: rules.map((r, j) => (j === i ? { ...r, ...patch } : r)) });
  return (
    <div className="flex flex-col gap-1.5" data-testid="scenario-condition">
      {rules.length > 1 && (
        <div className="flex items-center gap-2 text-[11.5px] text-muted">
          <Segmented
            items={[
              { id: "all", label: "All" },
              { id: "any", label: "Any" },
            ]}
            value={condition.any ? "any" : "all"}
            onChange={(v) => onChange({ ...condition, any: v === "any" ? true : undefined })}
          />
          of these hold
        </div>
      )}
      <div className="grid grid-cols-[minmax(0,1.3fr)_128px_minmax(0,1fr)_22px] items-center gap-x-1.5 gap-y-1">
        {rules.map((r, i) => (
          <div key={i} className="contents">
            <ValueInput value={r.value} onChange={(value) => update(i, { value })} placeholder="{{variable}} or = JavaScript" label={`${label} ${i + 1} value`} />
            <select
              aria-label={`${label} ${i + 1} comparison`}
              value={r.op ?? "equals"}
              onChange={(e) => update(i, { op: e.target.value as CompareOp })}
              className={selectClass}
            >
              {OPS.map((o) => (
                <option key={o.id} value={o.id}>
                  {o.label}
                </option>
              ))}
            </select>
            {opTakesValue(r.op) ? (
              <ValueInput value={r.to ?? ""} onChange={(to) => update(i, { to })} placeholder="value" label={`${label} ${i + 1} compared with`} />
            ) : (
              <span />
            )}
            <IconButton
              label={`Remove ${label.toLowerCase()} ${i + 1}`}
              onClick={() => onChange({ ...condition, rules: rules.filter((_, j) => j !== i) })}
              size={22}
              disabled={rules.length === 1}
            >
              <X size={12} />
            </IconButton>
          </div>
        ))}
      </div>
      <div>
        <Button size="sm" variant="ghost" icon={<Plus size={12} />} onClick={() => onChange({ ...condition, rules: [...rules, { value: "" }] })}>
          Add rule
        </Button>
      </div>
    </div>
  );
}

/** A checkbox with its label beside it. */
export function Option({ checked, onChange, label, title }: { checked: boolean; onChange: (v: boolean) => void; label: string; title?: string }) {
  return (
    <label className="inline-flex cursor-pointer items-center gap-1.5 text-[11.5px] text-muted" title={title}>
      <Checkbox checked={checked} onChange={onChange} label={label} />
      {label}
    </label>
  );
}

/** A request step's outputs: values of its response saved for the steps after it. */
export function OutputsEditor({
  outputs,
  onChange,
  name,
  pick,
}: {
  outputs: StepOutput[];
  onChange: (outputs: StepOutput[]) => void;
  /** The step's name, for labels. */
  name: string;
  /** The step's latest result: an output can be picked from its response. */
  pick?: { runId: string; seq: number };
}) {
  const update = (i: number, patch: Partial<StepOutput>) => onChange(outputs.map((o, j) => (j === i ? { ...o, ...patch } : o)));
  const taken = outputs.map((o) => o.variable.trim());
  return (
    <div className="flex flex-col gap-1.5" data-testid="scenario-outputs">
      {outputs.length > 0 && (
        <div className="grid grid-cols-[minmax(0,1fr)_92px_minmax(0,1.3fr)_22px] items-center gap-x-1.5 gap-y-1">
          {outputs.map((o, i) => {
            const source = SOURCES.find((s) => s.id === (o.from ?? "json")) ?? SOURCES[0];
            const problem = outputProblem(o);
            return (
              <div key={i} className="contents" data-testid="scenario-output">
                <Input
                  aria-label={`Output ${i + 1} of ${name}: variable`}
                  value={o.variable}
                  placeholder="cartId"
                  onChange={(e) => update(i, { variable: e.target.value })}
                  className="h-7 font-mono text-[12px]"
                  invalid={!!problem && problem.includes("ariable")}
                  title={problem ?? undefined}
                />
                <select aria-label={`Output ${i + 1} of ${name}: from`} value={source.id} onChange={(e) => update(i, { from: e.target.value as ValueFrom })} className={selectClass}>
                  {SOURCES.map((s) => (
                    <option key={s.id} value={s.id}>
                      {s.label}
                    </option>
                  ))}
                </select>
                {source.path ? (
                  <Input
                    aria-label={`Output ${i + 1} of ${name}: ${source.label.toLowerCase()}`}
                    value={o.path ?? ""}
                    placeholder={source.placeholder}
                    onChange={(e) => update(i, { path: e.target.value })}
                    className="h-7 font-mono text-[12px]"
                    invalid={!!problem && !problem.includes("ariable")}
                    title={problem ?? undefined}
                  />
                ) : (
                  <span className="text-[11.5px] text-faint">the whole {source.label.toLowerCase()}</span>
                )}
                <IconButton label={`Remove output ${o.variable || i + 1} of ${name}`} onClick={() => onChange(outputs.filter((_, j) => j !== i))} size={22}>
                  <X size={12} />
                </IconButton>
                <div className="col-span-4 -mt-0.5 flex flex-wrap gap-x-4 pb-1 pl-0.5">
                  <Option checked={!!o.secret} onChange={(secret) => update(i, { secret: secret || undefined })} label="Secret" title="Hide the value in results (a token)" />
                  <Option checked={!!o.optional} onChange={(optional) => update(i, { optional: optional || undefined })} label="May be missing" title="Finding nothing is fine: the variable is then not set" />
                  <Option checked={!!o.collect} onChange={(collect) => update(i, { collect: collect || undefined })} label="Add to a list" title="Keep every value under this name as a list (one per round of a loop)" />
                </div>
              </div>
            );
          })}
        </div>
      )}
      <div className="flex flex-wrap items-center gap-1.5">
        <Button size="sm" variant="ghost" icon={<Plus size={12} />} onClick={() => onChange([...outputs, { variable: "" }])}>
          Add output
        </Button>
        {pick ? (
          <ResponsePicker
            pick={pick}
            onPick={(from, path) => {
              const variable = nameForPath(from === "json" ? path : `.${path.replace(/[^\w]/g, "")}`, taken);
              onChange([...outputs, { variable, ...(from === "json" ? {} : { from }), path }]);
            }}
          />
        ) : (
          <span className="text-[11px] text-faint">Run the step once, then pick values from its response.</span>
        )}
      </div>
    </div>
  );
}

/** Pick a value of the step's last response: every value of a JSON body with its path, and the headers. */
function ResponsePicker({ pick, onPick }: { pick: { runId: string; seq: number }; onPick: (from: ValueFrom, path: string) => void }) {
  const [open, setOpen] = useState(false);
  const [q, setQ] = useState("");
  const [details, setDetails] = useState<RunDetails | null | "loading" | "gone">(null);
  useEffect(() => {
    if (!open) return;
    let alive = true;
    setDetails("loading");
    api
      .runDetails(pick.runId, pick.seq)
      .then((d) => alive && setDetails(d ?? "gone"))
      .catch(() => alive && setDetails("gone"));
    return () => {
      alive = false;
    };
  }, [open, pick.runId, pick.seq]);
  const response = details && typeof details === "object" ? details.response : null;
  const leaves = useMemo(() => (response ? jsonLeaves(response.body) : []), [response]);
  const words = q.toLowerCase().split(/\s+/).filter(Boolean);
  const matches = (text: string) => words.every((w) => text.toLowerCase().includes(w));
  const values = leaves.filter((l) => matches(`${l.path} ${l.preview}`));
  const headers = (response?.headers ?? []).filter((h) => matches(`${h.name} ${h.value}`));
  return (
    <Popover.Root
      open={open}
      onOpenChange={(o) => {
        setOpen(o);
        if (!o) setQ("");
      }}
    >
      <Popover.Trigger asChild>
        <Button size="sm" variant="ghost" icon={<MousePointerClick size={12} />} data-testid="scenario-pick-output">
          Pick from the response
        </Button>
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Content
          align="start"
          sideOffset={6}
          collisionPadding={12}
          className="zv-pop z-[90] flex max-h-[min(440px,var(--radix-popover-content-available-height))] w-[min(460px,calc(100vw-24px))] flex-col overflow-hidden rounded-xl border border-line bg-elev shadow-pop"
        >
          <div className="flex items-center gap-2 border-b border-line px-3">
            <Search size={13} className="shrink-0 text-faint" />
            <input
              autoFocus
              value={q}
              onChange={(e) => setQ(e.target.value)}
              placeholder="Search the response"
              aria-label="Search the response"
              className="h-10 min-w-0 flex-1 bg-transparent text-[13px] outline-none placeholder:text-faint"
            />
          </div>
          <div className="min-h-0 flex-1 overflow-auto p-1.5" role="listbox" aria-label="Values of the response">
            {details === "loading" || details === null ? (
              <div className="p-5 text-center text-[12.5px] text-muted">Reading the response…</div>
            ) : !response ? (
              <div className="p-5 text-center text-[12.5px] text-muted">The step's last response is no longer kept. Run it again to pick from it.</div>
            ) : values.length + headers.length === 0 ? (
              <div className="p-5 text-center text-[12.5px] text-muted">
                {leaves.length || q ? `Nothing matches “${q}”.` : "The body isn't JSON: add an output with a regex, or save the whole body."}
              </div>
            ) : (
              <>
                {values.map((l) => (
                  <PickRow
                    key={l.path}
                    path={l.path}
                    preview={l.preview}
                    onClick={() => {
                      onPick("json", l.path);
                      setOpen(false);
                    }}
                  />
                ))}
                {headers.length > 0 && <div className="px-2 pb-0.5 pt-2 text-[10.5px] font-semibold uppercase tracking-wide text-faint">Headers</div>}
                {headers.map((h, i) => (
                  <PickRow
                    key={`${h.name}:${i}`}
                    path={h.name}
                    preview={h.value}
                    onClick={() => {
                      onPick("header", h.name);
                      setOpen(false);
                    }}
                  />
                ))}
              </>
            )}
          </div>
          {response?.bodyTruncated && <div className="border-t border-line px-3 py-1.5 text-[11px] text-faint">Only the first 64 KB of the body are listed.</div>}
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  );
}

function PickRow({ path, preview, onClick }: { path: string; preview: string; onClick: () => void }) {
  return (
    <button type="button" role="option" aria-selected={false} onClick={onClick} className="flex w-full items-baseline gap-3 rounded-lg px-2 py-1 text-left hover:bg-hover" data-testid="scenario-pick-row">
      <span className="min-w-0 shrink-0 truncate font-mono text-[12px] text-fg" style={{ maxWidth: "60%" }}>
        {path}
      </span>
      <span className="min-w-0 flex-1 truncate text-right font-mono text-[11.5px] text-faint">{preview}</span>
    </button>
  );
}

/** A request step's checks: each one is a test in the results. */
export function ChecksEditor({ checks, onChange, name }: { checks: StepCheck[]; onChange: (checks: StepCheck[]) => void; name: string }) {
  const update = (i: number, patch: Partial<StepCheck>) => onChange(checks.map((c, j) => (j === i ? { ...c, ...patch } : c)));
  return (
    <div className="flex flex-col gap-1.5" data-testid="scenario-checks">
      {checks.length > 0 && (
        <div className="grid grid-cols-[18px_92px_minmax(0,1fr)_128px_minmax(0,1fr)_22px] items-center gap-x-1.5 gap-y-1">
          {checks.map((c, i) => {
            const source = SOURCES.find((s) => s.id === (c.from ?? "json")) ?? SOURCES[0];
            return (
              <div key={i} className="contents" data-testid="scenario-check">
                <Checkbox checked={c.enabled !== false} onChange={(enabled) => update(i, { enabled })} label={`Use check ${i + 1} of ${name}`} />
                <select aria-label={`Check ${i + 1} of ${name}: from`} value={source.id} onChange={(e) => update(i, { from: e.target.value as ValueFrom })} className={selectClass}>
                  {SOURCES.map((s) => (
                    <option key={s.id} value={s.id}>
                      {s.label}
                    </option>
                  ))}
                </select>
                {source.path ? (
                  <Input
                    aria-label={`Check ${i + 1} of ${name}: ${source.label.toLowerCase()}`}
                    value={c.path ?? ""}
                    placeholder={source.placeholder}
                    onChange={(e) => update(i, { path: e.target.value })}
                    className="h-7 font-mono text-[12px]"
                    invalid={c.enabled !== false && !(c.path ?? "").trim()}
                  />
                ) : (
                  <span />
                )}
                <select aria-label={`Check ${i + 1} of ${name}: comparison`} value={c.op ?? "equals"} onChange={(e) => update(i, { op: e.target.value as CompareOp })} className={selectClass}>
                  {OPS.map((o) => (
                    <option key={o.id} value={o.id}>
                      {o.label}
                    </option>
                  ))}
                </select>
                {opTakesValue(c.op) ? (
                  <ValueInput value={c.value ?? ""} onChange={(value) => update(i, { value })} placeholder={source.id === "status" ? "200" : "value"} label={`Check ${i + 1} of ${name}: value`} />
                ) : (
                  <span />
                )}
                <IconButton label={`Remove check ${i + 1} of ${name}`} onClick={() => onChange(checks.filter((_, j) => j !== i))} size={22}>
                  <X size={12} />
                </IconButton>
              </div>
            );
          })}
        </div>
      )}
      <div className="flex flex-wrap items-center gap-1.5">
        <Button size="sm" variant="ghost" icon={<Plus size={12} />} onClick={() => onChange([...checks, { from: "status", value: "200" }])}>
          Add check
        </Button>
        {checks.length === 0 && <span className="text-[11px] text-faint">Without checks or tests, the step passes when the status is below 400.</span>}
      </div>
    </div>
  );
}

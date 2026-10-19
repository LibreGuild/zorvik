// A route's answer sequence: different answers for its first calls (503, 503,
// then the route's own answer), to test how a client retries.
import { useState } from "react";
import { ChevronRight, Plus, Trash2 } from "lucide-react";
import type { MockFault } from "../../bindings/MockFault";
import type { MockRoute } from "../../bindings/MockRoute";
import type { MockStep } from "../../bindings/MockStep";
import { COMMON_HEADERS } from "../../lib/http";
import { statusTone, toneText } from "../../lib/format";
import { CodeEditor } from "../CodeEditor";
import { KeyValueEditor } from "../KeyValueEditor";
import { Button, cx, NumberInput, Switch } from "../ui";
import { languageForContentType } from "./MockRoutes";

/** Most answers a sequence may have (as `MAX_SEQUENCE` in crates/formats/src/server.rs). */
const MAX_STEPS = 100;

const STEP_FAULTS: { id: MockFault; label: string }[] = [
  { id: "none", label: "Answer" },
  { id: "error", label: "Error (500)" },
  { id: "reset", label: "Drop the connection" },
  { id: "hang", label: "Never answer" },
];

const small = "h-7 min-w-0 rounded-md border border-line bg-input px-2 font-mono text-[12px] text-fg outline-none placeholder:text-faint hover:border-line-strong focus:border-accent disabled:opacity-50";

export function SequenceEditor({ route, onChange, variables }: { route: MockRoute; onChange: (patch: Partial<MockRoute>) => void; variables: string[] }) {
  const steps = route.sequence ?? [];
  const [open, setOpen] = useState(steps.length > 0);
  const [expanded, setExpanded] = useState<number | null>(null);
  const setSteps = (next: MockStep[]) => onChange(next.length ? { sequence: next } : { sequence: undefined, sequenceRepeat: undefined });
  const update = (i: number, patch: Partial<MockStep>) => setSteps(steps.map((s, j) => (j === i ? { ...s, ...patch } : s)));
  const add = () => {
    setSteps([...steps, { status: 503 }]);
    setOpen(true);
  };

  return (
    <section className="flex flex-col gap-2.5 px-4 py-3" data-testid="route-sequence">
      <button
        aria-expanded={open}
        onClick={() => setOpen(!open)}
        className="flex items-center gap-1 self-start text-[11px] font-semibold uppercase tracking-wide text-faint hover:text-fg"
      >
        <ChevronRight size={13} className={cx("transition-transform", open && "rotate-90")} />
        Answer sequence
        {steps.length > 0 && <span className="ml-1 rounded-full bg-hover px-1.5 text-[10.5px] normal-case tracking-normal text-muted">{steps.length}</span>}
      </button>
      {open && (
        <>
          <p className="text-[12px] text-muted">
            The first calls get these answers in order, later calls the route's own answer, e.g. 503, 503, then 200 to test a client's retries. Counting starts again when the server starts or
            the route changes; the traffic shows which answer each call got.
          </p>
          {steps.length > 0 && (
            <div className="overflow-hidden rounded-lg border border-line" role="list" aria-label="Answer sequence">
              {steps.map((step, i) => {
                const fault = step.fault ?? "none";
                const isOpen = expanded === i;
                return (
                  <div key={i} role="listitem" className="border-b border-line/70" data-testid={`sequence-step-${i}`}>
                    <div className="flex items-center gap-1.5 px-2 py-1">
                      <span className="w-12 shrink-0 text-[11.5px] text-faint">Call {i + 1}</span>
                      <select aria-label={`Call ${i + 1} answer`} className={cx(small, "w-[136px] font-sans")} value={fault} onChange={(e) => update(i, { fault: e.target.value as MockFault })}>
                        {STEP_FAULTS.map((f) => (
                          <option key={f.id} value={f.id}>
                            {f.label}
                          </option>
                        ))}
                      </select>
                      <NumberInput
                        bare
                        aria-label={`Call ${i + 1} status`}
                        min={100}
                        max={999}
                        list="zv-mock-status-codes"
                        disabled={fault !== "none"}
                        className={cx(small, "w-[68px]", fault === "none" && toneText[statusTone(step.status)])}
                        value={fault === "error" ? 500 : step.status}
                        onChange={(status) => update(i, { status: status ?? 100 })}
                      />
                      <NumberInput
                        bare
                        optional
                        aria-label={`Call ${i + 1} delay in milliseconds`}
                        className={cx(small, "w-[96px]")}
                        value={step.delayMs ?? undefined}
                        placeholder={`${route.delayMs ?? 0} ms`}
                        title="Delay in ms (empty: the route's delay)"
                        onChange={(delayMs) => update(i, { delayMs })}
                      />
                      <div className="flex-1" />
                      {fault === "none" && (
                        <button
                          aria-expanded={isOpen}
                          onClick={() => setExpanded(isOpen ? null : i)}
                          className="flex shrink-0 items-center gap-0.5 rounded px-1 py-0.5 text-[11.5px] text-muted hover:bg-hover hover:text-fg"
                        >
                          <ChevronRight size={11} className={cx("transition-transform", isOpen && "rotate-90")} />
                          Body{step.body ? " ·" : ""}
                          {step.headers?.length ? ` ${step.headers.length} header${step.headers.length === 1 ? "" : "s"}` : ""}
                        </button>
                      )}
                      <button aria-label={`Remove call ${i + 1}`} onClick={() => setSteps(steps.filter((_, j) => j !== i))} className="shrink-0 rounded p-1 text-faint hover:bg-hover hover:text-danger">
                        <Trash2 size={12} />
                      </button>
                    </div>
                    {isOpen && fault === "none" && (
                      <div className="flex flex-col gap-2 bg-panel-2/50 px-2 pb-2 pt-1">
                        <div className="h-[120px] overflow-hidden rounded-md border border-line bg-input">
                          <CodeEditor
                            value={step.body ?? ""}
                            onChange={(body) => update(i, { body })}
                            language={languageForContentType("", step.body ?? "")}
                            variables={variables}
                            placeholder={'{"error": "try again later"}'}
                          />
                        </div>
                        <div className="text-[11.5px] text-faint">Extra headers: added to the route's (one of the same name replaces it), e.g. Retry-After.</div>
                        <div className="-mx-3">
                          <KeyValueEditor rows={step.headers ?? []} onChange={(headers) => update(i, { headers: headers.length ? headers : undefined })} keyPlaceholder="Header" valuePlaceholder="Value" keySuggestions={COMMON_HEADERS} />
                        </div>
                      </div>
                    )}
                  </div>
                );
              })}
              <div className="flex items-center gap-1.5 px-2 py-1.5 text-[12px] text-muted">
                <span className="w-12 shrink-0 text-[11.5px] text-faint">{route.sequenceRepeat ? `Call ${steps.length + 1}` : `Then`}</span>
                The route's own answer ({route.status}){route.sequenceRepeat ? ", then the sequence starts over" : ", from then on"}
              </div>
            </div>
          )}
          <div className="flex flex-wrap items-center gap-3">
            <Button size="sm" icon={<Plus size={13} />} onClick={add} disabled={steps.length >= MAX_STEPS}>
              Add answer
            </Button>
            {steps.length > 0 && <Switch checked={route.sequenceRepeat ?? false} onChange={(sequenceRepeat) => onChange({ sequenceRepeat: sequenceRepeat || undefined })} label="Start over after the route's answer" />}
          </div>
        </>
      )}
    </section>
  );
}

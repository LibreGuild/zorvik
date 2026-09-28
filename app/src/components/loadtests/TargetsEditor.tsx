// Which saved requests a load test sends, how often relative to each other, and
// the values each one captures from its responses.
import { memo, useEffect, useMemo, useRef, useState } from "react";
import { Check, Plus, Search, TriangleAlert, Variable, X } from "lucide-react";
import { Popover } from "radix-ui";
import type { LoadModel } from "../../bindings/LoadModel";
import type { LoadTarget } from "../../bindings/LoadTarget";
import type { TreeNode } from "../../bindings/TreeNode";
import { methodColor, methodLabel } from "../../lib/http";
import { Button, Checkbox, cx, IconButton } from "../ui";
import { CapturesEditor } from "./CapturesEditor";
import { captureProblem, flattenRequests, nameFromPath, type RequestEntry, targetShares } from "./model";
import { NumberInput } from "./parts";

const MAX_WEIGHT = 1000;

export const TargetsEditor = memo(function TargetsEditor({
  targets,
  tree,
  model = "virtualUsers",
  onChange,
}: {
  targets: LoadTarget[];
  tree: TreeNode[];
  model?: LoadModel;
  onChange: (fn: (targets: LoadTarget[]) => LoadTarget[]) => void;
}) {
  const all = useMemo(() => flattenRequests(tree), [tree]);
  const byPath = useMemo(() => new Map(all.map((r) => [r.path, r])), [all]);
  const shares = useMemo(() => targetShares(targets), [targets]);
  const add = (paths: string[]) =>
    onChange((list) => [...list, ...paths.filter((p) => !list.some((t) => t.request === p)).map((request) => ({ request }))]);
  const update = (i: number, patch: Partial<LoadTarget>) => onChange((list) => list.map((t, j) => (j === i ? { ...t, ...patch } : t)));
  const remove = (i: number) => onChange((list) => list.filter((_, j) => j !== i));

  return (
    <div className="flex flex-col gap-2" data-testid="load-targets">
      {targets.length === 0 ? (
        <div className="flex flex-col items-center gap-2 rounded-xl border border-dashed border-line-strong px-4 py-5 text-center">
          <div className="text-[12.5px] text-muted">Add the saved HTTP requests this test sends. With several, weights set how often each one is picked.</div>
          <RequestPicker all={all} added={targets} onAdd={add} trigger={<Button size="sm" variant="primary" icon={<Plus size={13} />}>Add requests</Button>} />
        </div>
      ) : (
        <>
          <div className="overflow-hidden rounded-xl border border-line">
            <div className="flex h-7 items-center gap-2 border-b border-line bg-panel-2/60 px-2.5 text-[11px] font-medium text-faint">
              <span className="w-4" />
              <span className="min-w-0 flex-1">Request</span>
              <span className="w-7 text-center" title="Values saved from the responses for the user's next requests">
                <Variable size={12} className="inline" aria-label="Captures" />
              </span>
              <span className="w-[60px] text-right" title="How often this request is picked, relative to the others">
                Weight
              </span>
              <span className="w-10 text-right">Share</span>
              <span className="w-6" />
            </div>
            {targets.map((t, i) => (
              <TargetRow
                key={`${t.request}:${i}`}
                target={t}
                entry={byPath.get(t.request)}
                share={shares[i]}
                model={model}
                onUpdate={(p) => update(i, p)}
                onRemove={() => remove(i)}
              />
            ))}
          </div>
          <div>
            <RequestPicker all={all} added={targets} onAdd={add} trigger={<Button size="sm" variant="ghost" icon={<Plus size={13} />}>Add requests</Button>} />
          </div>
        </>
      )}
    </div>
  );
});

function TargetRow({
  target,
  entry,
  share,
  model,
  onUpdate,
  onRemove,
}: {
  target: LoadTarget;
  entry: RequestEntry | undefined;
  share: number;
  model: LoadModel;
  onUpdate: (patch: Partial<LoadTarget>) => void;
  onRemove: () => void;
}) {
  const enabled = target.enabled !== false;
  const name = entry?.name ?? nameFromPath(target.request);
  const problem = !entry ? "Not found in the collection" : entry.kind !== "http" ? "Only HTTP requests can be load tested" : entry.error ? "The request file can't be read" : null;
  const captures = target.captures ?? [];
  const [open, setOpen] = useState(false);
  const broken = captures.some((c) => captureProblem(c));
  return (
    <div className={cx("border-b border-line/60 last:border-b-0", problem && "bg-warning/6")} data-testid="load-target">
      <div className="flex min-h-[40px] items-center gap-2 px-2.5 py-1">
        <Checkbox checked={enabled} onChange={(v) => onUpdate({ enabled: v })} label={`Send ${name}`} />
        <div className={cx("flex min-w-0 flex-1 items-center gap-2", !enabled && "opacity-50")}>
          {problem ? (
            <TriangleAlert size={13} className="w-[34px] shrink-0 text-warning" />
          ) : (
            <span className="w-[34px] shrink-0 text-right font-mono text-[10px] font-bold" style={{ color: methodColor(entry?.method, entry?.kind) }}>
              {methodLabel(entry?.method, entry?.kind)}
            </span>
          )}
          <div className="min-w-0 flex-1" title={target.request}>
            <div className="truncate text-[12.5px] text-fg">{name}</div>
            <div className={cx("truncate text-[11px]", problem ? "text-warning" : "text-faint")}>{problem ?? (entry?.trail || "Collection")}</div>
          </div>
        </div>
        <button
          type="button"
          aria-label={`Captures of ${name}`}
          aria-expanded={open}
          title={captures.length ? `${captures.length} ${captures.length === 1 ? "capture" : "captures"}` : "Capture values from the response"}
          onClick={() => setOpen((o) => !o)}
          className={cx(
            "flex h-6 w-7 shrink-0 items-center justify-center gap-0.5 rounded-md text-[11px] tabular-nums hover:bg-hover",
            open ? "bg-hover text-fg" : broken ? "text-warning" : captures.length ? "text-accent" : "text-faint",
          )}
        >
          {captures.length ? captures.length : <Variable size={12} />}
        </button>
        <NumberInput
          aria-label={`Weight of ${name}`}
          value={target.weight ?? 1}
          onChange={(v) => onUpdate({ weight: Math.min(MAX_WEIGHT, v ?? 0) })}
          min={0}
          max={MAX_WEIGHT}
          className="h-7 w-[60px] text-right"
          disabled={!enabled}
        />
        <span className="w-10 shrink-0 text-right text-[11.5px] tabular-nums text-muted">{share > 0 ? `${Math.round(share)}%` : "–"}</span>
        <IconButton label={`Remove ${name}`} onClick={onRemove} size={24}>
          <X size={13} />
        </IconButton>
      </div>
      {open && (
        <div className="border-t border-line/40 bg-panel-2/40 px-2.5 py-2 pl-9">
          <CapturesEditor captures={captures} name={name} model={model} onChange={(list) => onUpdate({ captures: list.length ? list : undefined })} />
        </div>
      )}
    </div>
  );
}

/** Popover to pick saved HTTP requests (search, keyboard, several in a row). */
function RequestPicker({ all, added, onAdd, trigger }: { all: RequestEntry[]; added: LoadTarget[]; onAdd: (paths: string[]) => void; trigger: React.ReactNode }) {
  const [open, setOpen] = useState(false);
  const [q, setQ] = useState("");
  const [index, setIndex] = useState(0);
  const list = useRef<HTMLDivElement>(null);
  const http = useMemo(() => all.filter((r) => r.kind === "http" && !r.error), [all]);
  const addedSet = useMemo(() => new Set(added.map((t) => t.request)), [added]);
  const results = useMemo(() => {
    const words = q.toLowerCase().split(/\s+/).filter(Boolean);
    return http.filter((r) => words.every((w) => `${r.method} ${r.trail} ${r.name}`.toLowerCase().includes(w)));
  }, [http, q]);
  const addable = results.filter((r) => !addedSet.has(r.path));
  useEffect(() => setIndex(0), [q, open]);
  useEffect(() => {
    list.current?.querySelector(`[data-index="${index}"]`)?.scrollIntoView({ block: "nearest" });
  }, [index]);
  const choose = (r: RequestEntry | undefined) => {
    if (r && !addedSet.has(r.path)) onAdd([r.path]);
  };
  return (
    <Popover.Root
      open={open}
      onOpenChange={(o) => {
        setOpen(o);
        if (!o) setQ("");
      }}
    >
      <Popover.Trigger asChild>{trigger}</Popover.Trigger>
      <Popover.Portal>
        <Popover.Content
          align="start"
          sideOffset={6}
          collisionPadding={12}
          className="zv-pop z-[90] flex max-h-[min(420px,var(--radix-popover-content-available-height))] w-[min(380px,calc(100vw-24px))] flex-col overflow-hidden rounded-xl border border-line bg-elev shadow-pop"
          onKeyDown={(e) => {
            if (e.key === "ArrowDown") {
              e.preventDefault();
              setIndex((i) => Math.min(i + 1, results.length - 1));
            } else if (e.key === "ArrowUp") {
              e.preventDefault();
              setIndex((i) => Math.max(i - 1, 0));
            } else if (e.key === "Enter" && !e.nativeEvent.isComposing && e.keyCode !== 229) {
              e.preventDefault();
              choose(results[index]);
            }
          }}
        >
          <div className="flex items-center gap-2 border-b border-line px-3">
            <Search size={13} className="shrink-0 text-faint" />
            <input
              autoFocus
              value={q}
              onChange={(e) => setQ(e.target.value)}
              placeholder="Search HTTP requests"
              aria-label="Search requests"
              className="h-10 min-w-0 flex-1 bg-transparent text-[13px] outline-none placeholder:text-faint"
            />
          </div>
          <div ref={list} role="listbox" aria-label="Requests" className="min-h-0 flex-1 overflow-auto p-1.5">
            {http.length === 0 ? (
              <div className="p-5 text-center text-[12.5px] text-muted">No saved HTTP requests yet. Save a request to the collection first.</div>
            ) : results.length === 0 ? (
              <div className="p-5 text-center text-[12.5px] text-muted">No requests match “{q}”.</div>
            ) : (
              results.map((r, i) => {
                const isAdded = addedSet.has(r.path);
                return (
                  <button
                    key={r.path}
                    type="button"
                    role="option"
                    aria-selected={i === index}
                    aria-disabled={isAdded}
                    data-index={i}
                    onMouseEnter={() => setIndex(i)}
                    onClick={() => choose(r)}
                    className={cx("flex w-full items-center gap-2 rounded-lg px-2 py-1.5 text-left", i === index && "bg-hover", isAdded && "opacity-60")}
                  >
                    <span className="w-[34px] shrink-0 text-right font-mono text-[10px] font-bold" style={{ color: methodColor(r.method, r.kind) }}>
                      {methodLabel(r.method, r.kind)}
                    </span>
                    <span className="min-w-0 flex-1">
                      <span className="block truncate text-[12.5px] text-fg">{r.name}</span>
                      {r.trail && <span className="block truncate text-[11px] text-faint">{r.trail}</span>}
                    </span>
                    {isAdded && <Check size={13} className="shrink-0 text-success" aria-label="Added" />}
                  </button>
                );
              })
            )}
          </div>
          <div className="flex items-center gap-2 border-t border-line px-3 py-2 text-[11px] text-faint">
            <span className="min-w-0 flex-1">Only HTTP requests can be load tested.</span>
            {addable.length > 1 && (
              <Button size="sm" variant="secondary" onClick={() => onAdd(addable.map((r) => r.path))}>
                Add {addable.length === results.length ? "all" : "these"} {addable.length}
              </Button>
            )}
          </div>
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  );
}

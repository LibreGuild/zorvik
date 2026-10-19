// JSON compare: what was added, removed or changed between two JSON documents, by path.
// Compared in Rust as you type; key order, formatting and number style don't count.
import { ArrowLeftRight, CheckCircle2, Copy, GitCompareArrows, X } from "lucide-react";
import type { JsonDiff } from "../../bindings/JsonDiff";
import type { JsonDifference } from "../../bindings/JsonDifference";
import { copyText } from "../../lib/platform";
import { api } from "../../lib/rpc";
import { type ToolTab, updateToolState } from "../../store/tabs";
import { toast } from "../../store/toasts";
import { toolState } from "../../store/tools";
import { CodeEditor } from "../CodeEditor";
import { Button, Checkbox, cx, EmptyState, IconButton, Segmented, Spinner } from "../ui";
import { editInputs, useLiveResult } from "./live";
import { DIFF_LABEL, diffAsText, diffCount, type DiffView, visibleDifferences } from "./models";
import { CopyButton, ErrorNote, Hint } from "./parts";
import { RecentResponseMenu } from "./RecentResponseMenu";

interface JsonCompareState {
  left: string;
  right: string;
  ignoreArrayOrder: boolean;
  view: DiffView;
}

const DEFAULTS: JsonCompareState = { left: "", right: "", ignoreArrayOrder: false, view: "all" };

export function JsonCompareTool({ tab }: { tab: ToolTab }) {
  const s = toolState(tab, DEFAULTS);
  const edit = (patch: Partial<JsonCompareState>) => editInputs(tab.id, () => patch);
  const both = s.left.trim() !== "" && s.right.trim() !== "";
  const { left, right, ignoreArrayOrder } = s;
  const live = useLiveResult(tab, both ? () => api.jsonDiff(left, right, ignoreArrayOrder) : null, 300);
  const diff = live.value;

  return (
    <div className="flex min-h-full flex-col pb-6" data-testid="json-compare">
      <div className="grid grid-cols-1 gap-3 px-4 lg:grid-cols-2">
        <Side title="Left" testId="json-left" value={s.left} onChange={(v) => edit({ left: v })} />
        <Side title="Right" testId="json-right" value={s.right} onChange={(v) => edit({ right: v })} />
      </div>
      <div className="flex flex-wrap items-center gap-3 px-4 pb-1 pt-2.5">
        <label className="flex items-center gap-1.5 text-[12.5px] text-muted">
          <Checkbox checked={s.ignoreArrayOrder} onChange={(v) => edit({ ignoreArrayOrder: v })} label="Ignore array order" />
          Ignore array order
        </label>
        <Button size="sm" variant="ghost" icon={<ArrowLeftRight size={13} />} onClick={() => edit({ left: s.right, right: s.left })} disabled={!s.left && !s.right}>
          Swap sides
        </Button>
        <span className="flex-1" />
        {live.busy && both && <Spinner size={13} />}
      </div>

      {!both ? (
        !s.left.trim() && !s.right.trim() ? (
          <EmptyState icon={<GitCompareArrows size={28} />} title="Compare two JSON documents">
            Paste them or pick responses from history to see what was added, removed or changed, path by path. Key order, formatting and 1 vs 1.0 don't count.
          </EmptyState>
        ) : (
          <div className="pt-2">
            <Hint>Add the {s.left.trim() ? "right" : "left"} document to compare.</Hint>
          </div>
        )
      ) : live.error ? (
        <div className="pt-2">
          <ErrorNote>{live.error}</ErrorNote>
        </div>
      ) : diff ? (
        <div className={cx("transition-opacity", live.stale && "opacity-50")} data-testid="json-diff-result">
          {diff.differences.length === 0 ? (
            <div className="mx-4 mt-2 flex items-center gap-2 rounded-lg border border-success/30 bg-success/8 px-3 py-2 text-[12.5px] text-success" data-testid="json-same">
              <CheckCircle2 size={15} />
              The documents are the same.
            </div>
          ) : (
            <Differences tab={tab} view={s.view} diff={diff} />
          )}
        </div>
      ) : null}
    </div>
  );
}

function Side({ title, testId, value, onChange }: { title: string; testId: string; value: string; onChange: (v: string) => void }) {
  return (
    <div className="flex min-w-0 flex-col gap-1.5">
      <div className="flex h-7 items-center gap-1">
        <span className="flex-1 text-[12px] font-medium text-muted">{title}</span>
        <RecentResponseMenu onPick={onChange} />
        {value && (
          <IconButton label={`Clear the ${title.toLowerCase()} document`} onClick={() => onChange("")}>
            <X size={13} />
          </IconButton>
        )}
      </div>
      <div className="h-[260px] overflow-hidden rounded-lg border border-line bg-input focus-within:border-accent" data-testid={testId}>
        <CodeEditor value={value} onChange={onChange} language="json" placeholder={`Paste the ${title.toLowerCase()} JSON`} lineWrapping={false} />
      </div>
    </div>
  );
}

const KIND_STYLE = {
  added: { mark: "+", cls: "text-success" },
  removed: { mark: "−", cls: "text-danger" },
  changed: { mark: "~", cls: "text-warning" },
} as const;

function Differences({ tab, view, diff }: { tab: ToolTab; view: DiffView; diff: JsonDiff }) {
  const total = diff.added + diff.removed + diff.changed;
  const items = (["all", "changed", "added", "removed"] as DiffView[])
    .filter((v) => v === "all" || diffCount(diff, v) > 0)
    .map((v) => ({ id: v, label: `${v === "all" ? "All" : DIFF_LABEL[v]} ${diffCount(diff, v)}` }));
  const shown = visibleDifferences(diff, items.some((i) => i.id === view) ? view : "all");
  return (
    <section className="pt-2">
      <div className="flex flex-wrap items-center gap-2 px-4 pb-2">
        <span className="text-[12.5px] font-semibold text-fg" data-testid="json-diff-summary">
          {total} {total === 1 ? "difference" : "differences"}
        </span>
        <Segmented items={items} value={items.some((i) => i.id === view) ? view : "all"} onChange={(v) => updateToolState(tab.id, (st) => ({ ...st, view: v }))} />
        <span className="flex-1" />
        <Button
          size="sm"
          variant="ghost"
          icon={<Copy size={12} />}
          onClick={() => {
            void copyText(diffAsText(shown));
            toast("success", "Copied to clipboard");
          }}
        >
          Copy as text
        </Button>
      </div>
      {diff.cut && <Hint>Showing the first {diff.differences.length.toLocaleString()} differences.</Hint>}
      <div className="mx-4 overflow-hidden rounded-lg border border-line" role="table" aria-label="Differences">
        <div role="row" className="grid grid-cols-[18px_minmax(0,1.2fr)_minmax(0,1fr)_minmax(0,1fr)] gap-3 border-b border-line bg-panel-2/60 px-3 py-1.5 text-[11px] font-medium text-faint">
          <span role="columnheader" aria-label="Kind" />
          <span role="columnheader">Path</span>
          <span role="columnheader">Left</span>
          <span role="columnheader">Right</span>
        </div>
        {shown.map((d, i) => (
          <Row key={i} d={d} />
        ))}
      </div>
    </section>
  );
}

function Row({ d }: { d: JsonDifference }) {
  const style = KIND_STYLE[d.kind];
  return (
    <div
      role="row"
      data-kind={d.kind}
      className="group grid grid-cols-[18px_minmax(0,1.2fr)_minmax(0,1fr)_minmax(0,1fr)] items-start gap-3 border-b border-line/60 px-3 py-1.5 text-[12px] last:border-0"
    >
      <span role="cell" className={cx("font-mono font-semibold", style.cls)} aria-label={DIFF_LABEL[d.kind]}>
        {style.mark}
      </span>
      <span role="cell" className="flex min-w-0 items-start gap-1">
        <span className="selectable min-w-0 break-all font-mono text-fg">{d.path}</span>
        <CopyButton text={d.path} label="Copy path" className="-my-0.5 opacity-0 group-hover:opacity-100 focus-visible:opacity-100" />
      </span>
      <span role="cell" className={cx("selectable min-w-0 break-all font-mono", d.left == null ? "text-faint" : d.kind === "removed" ? "text-danger" : "text-fg")}>
        {d.left ?? "–"}
      </span>
      <span role="cell" className={cx("selectable min-w-0 break-all font-mono", d.right == null ? "text-faint" : d.kind === "added" ? "text-success" : "text-fg")}>
        {d.right ?? "–"}
      </span>
    </div>
  );
}

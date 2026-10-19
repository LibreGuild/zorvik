// Filter playground: try a JSONPath or jq expression (run in Rust, like the response filter)
// or XPath (run here) on JSON or XML you paste or pick from history, as you type.
import { ListFilter, X } from "lucide-react";
import { api } from "../../lib/rpc";
import { type ToolTab } from "../../store/tabs";
import { toolState } from "../../store/tools";
import { CodeEditor } from "../CodeEditor";
import { FILTER_KINDS, type FilterKind, xpath } from "../response/filterModel";
import { cx, EmptyState, IconButton, Input, Segmented, Spinner } from "../ui";
import { editInputs, useLiveResult } from "./live";
import { FILTER_EXAMPLES, filterError, looksLikeHtml } from "./models";
import { CopyButton } from "./parts";
import { RecentResponseMenu } from "./RecentResponseMenu";

interface FilterState {
  kind: FilterKind;
  expression: string;
  input: string;
}

const DEFAULTS: FilterState = { kind: "jsonPath", expression: "", input: "" };

/** Run a filter on the text; errors read as plain sentences. */
async function runFilter(kind: FilterKind, expression: string, input: string) {
  if (kind === "xpath") return xpath(input, expression, looksLikeHtml(input));
  try {
    return await api.filterText(input, kind, expression);
  } catch (e) {
    throw new Error(filterError(e instanceof Error ? e.message : String(e)));
  }
}

export function FilterPlaygroundTool({ tab }: { tab: ToolTab }) {
  const s = toolState(tab, DEFAULTS);
  const edit = (patch: Partial<FilterState>) => editInputs(tab.id, () => patch);
  const info = FILTER_KINDS.find((k) => k.id === s.kind) ?? FILTER_KINDS[0];
  const { kind, expression, input } = s;
  const ready = expression.trim() !== "" && input.trim() !== "";
  const live = useLiveResult(tab, ready ? () => runFilter(kind, expression.trim(), input) : null, 250);
  const out = live.value;
  const json = s.kind !== "xpath";

  return (
    <div className="flex min-h-full flex-col pb-6" data-testid="filter-playground">
      <div className="flex flex-wrap items-center gap-2 px-4 pb-2">
        <Segmented items={FILTER_KINDS.map((k) => ({ id: k.id, label: k.label }))} value={s.kind} onChange={(k) => edit({ kind: k })} />
        <Input
          autoFocus
          className="min-w-[240px] flex-1 font-mono"
          aria-label="Expression"
          placeholder={info.placeholder}
          value={s.expression}
          onChange={(e) => edit({ expression: e.target.value })}
        />
        <span className="min-w-[80px] text-[12px] tabular-nums text-muted" data-testid="filter-status">
          {!ready || live.error ? null : out && !live.stale ? (
            `${out.count} ${out.count === 1 ? "match" : "matches"}${"cut" in out && out.cut ? " (first ones shown)" : ""}`
          ) : (
            <Spinner size={12} />
          )}
        </span>
      </div>
      <div className="flex flex-wrap items-center gap-1.5 px-4 pb-3" aria-label="Examples">
        <span className="text-[11.5px] text-faint">Try:</span>
        {FILTER_EXAMPLES[s.kind].map((ex) => (
          <button
            key={ex.expression}
            type="button"
            title={ex.label}
            onClick={() => edit({ expression: ex.expression })}
            className="rounded-md bg-panel-2 px-1.5 py-0.5 font-mono text-[11.5px] text-muted outline-none hover:bg-hover hover:text-fg focus-visible:ring-1 focus-visible:ring-accent"
          >
            {ex.expression}
          </button>
        ))}
      </div>

      <div className="grid min-h-[340px] flex-1 grid-cols-1 gap-3 px-4 lg:grid-cols-2">
        <div className="flex min-h-[300px] min-w-0 flex-col gap-1.5">
          <div className="flex h-7 items-center gap-1">
            <span className="flex-1 text-[12px] font-medium text-muted">{json ? "JSON" : "XML or HTML"}</span>
            <RecentResponseMenu onPick={(text) => edit({ input: text })} />
            {s.input && (
              <IconButton label="Clear" onClick={() => edit({ input: "" })}>
                <X size={13} />
              </IconButton>
            )}
          </div>
          <div className="min-h-0 flex-1 overflow-hidden rounded-lg border border-line bg-input focus-within:border-accent" data-testid="filter-input">
            <CodeEditor
              value={s.input}
              onChange={(input) => edit({ input })}
              language={json ? "json" : "xml"}
              placeholder={json ? "Paste JSON to filter" : "Paste XML or HTML to filter"}
            />
          </div>
        </div>
        <div className="flex min-h-[300px] min-w-0 flex-col gap-1.5">
          <div className="flex h-7 items-center gap-1">
            <span className="flex-1 text-[12px] font-medium text-muted">Result</span>
            {out && <CopyButton text={out.text} label="Copy result" />}
          </div>
          <div
            className={cx("min-h-0 flex-1 overflow-hidden rounded-lg border border-line bg-panel-2 transition-opacity", live.stale && "opacity-50")}
            data-testid="filter-output"
          >
            {ready && live.error ? (
              <div role="alert" className="selectable h-full overflow-auto whitespace-pre-wrap break-words p-3 font-mono text-[12px] text-danger">
                {live.error}
              </div>
            ) : ready && out ? (
              <CodeEditor value={out.text} readOnly language={json ? "json" : "xml"} />
            ) : (
              <EmptyState icon={<ListFilter size={24} />} title={ready ? "Filtering…" : "Try an expression"}>
                {json
                  ? "JSONPath and jq run exactly as in the response filter, so an expression that works here works on your responses and in agent requests."
                  : "XPath 1.0 runs on XML, or on HTML leniently."}
              </EmptyState>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

// A request's saved responses: what it answers (documentation), and what mocks built from
// it answer. Added from a response with "Save as example".
import { useState } from "react";
import { BookmarkPlus, Trash2 } from "lucide-react";
import type { Example } from "../../bindings/Example";
import { formatBytes, statusTone, toneBg } from "../../lib/format";
import { type Tab, updateDraft } from "../../store/tabs";
import { CodeEditor, type EditorLanguage } from "../CodeEditor";
import { Badge, cx, EmptyState, IconButton, Input, Select } from "../ui";

function languageOf(example: Example): EditorLanguage {
  const ct = (example.headers ?? []).find((h) => h.key.toLowerCase() === "content-type")?.value.toLowerCase() ?? "";
  if (ct.includes("json")) return "json";
  if (ct.includes("html")) return "html";
  if (ct.includes("xml")) return "xml";
  return "text";
}

export function ExamplesTab({ tab }: { tab: Tab }) {
  const examples = tab.draft.examples ?? [];
  const [selected, setSelected] = useState(0);
  const current = examples[Math.min(selected, examples.length - 1)];
  const change = (index: number, fn: (e: Example) => Example | null) =>
    updateDraft(tab.id, (r) => ({ ...r, examples: (r.examples ?? []).flatMap((e, i) => (i === index ? (fn(e) ?? []) : [e])) }));

  if (!current) {
    return (
      <EmptyState icon={<BookmarkPlus size={26} strokeWidth={1.5} />} title="No examples yet">
        Send the request, then choose <b>Save as example</b> above the response. Examples document what the request answers, and mocks
        built from this request answer with them.
      </EmptyState>
    );
  }
  const index = examples.indexOf(current);
  return (
    <div className="@container flex h-full min-h-0" data-testid="examples">
      {/* A list beside the example where there's room, a menu above it where there isn't. */}
      <ul className="hidden w-52 shrink-0 overflow-auto border-r border-line py-1 @[620px]:block" aria-label="Examples">
        {examples.map((e, i) => (
          <li key={i}>
            <button
              onClick={() => setSelected(i)}
              className={cx("flex w-full items-center gap-2 px-3 py-1.5 text-left text-[12.5px]", i === index ? "bg-selected text-fg" : "text-muted hover:bg-hover")}
            >
              <Badge className={cx("text-[11px]", toneBg[statusTone(e.status)])}>{e.status}</Badge>
              <span className="truncate">{e.name}</span>
            </button>
          </li>
        ))}
      </ul>
      <div className="flex min-w-0 flex-1 flex-col">
        <div className="flex shrink-0 items-center gap-2 border-b border-line px-3 py-1.5">
          {examples.length > 1 && (
            <Select value={String(index)} onChange={(e) => setSelected(Number(e.target.value))} aria-label="Example" className="w-36 shrink-0 @[620px]:hidden">
              {examples.map((e, i) => (
                <option key={i} value={i}>
                  {e.status} · {e.name}
                </option>
              ))}
            </Select>
          )}
          <Input
            value={current.name}
            onChange={(e) => change(index, (x) => ({ ...x, name: e.target.value }))}
            aria-label="Example name"
            className="h-7 min-w-0 flex-1 text-[12.5px]"
          />
          <Input
            type="number"
            value={current.status}
            min={100}
            max={599}
            onChange={(e) => change(index, (x) => ({ ...x, status: Math.min(599, Math.max(100, Number(e.target.value) || 200)) }))}
            aria-label="Status"
            className="h-7 w-20 shrink-0 text-[12.5px]"
          />
          <span className="shrink-0 text-[11.5px] tabular-nums text-faint">{formatBytes(new TextEncoder().encode(current.body ?? "").length)}</span>
          <IconButton label="Delete example" onClick={() => change(index, () => null)}>
            <Trash2 size={14} />
          </IconButton>
        </div>
        {(current.headers ?? []).length > 0 && (
          <div className="max-h-28 shrink-0 overflow-auto border-b border-line px-3 py-1.5 font-mono text-[11.5px] text-muted">
            {(current.headers ?? []).map((h, i) => (
              <div key={i} className="truncate">
                <span className="text-fg">{h.key}</span>: {h.value}
              </div>
            ))}
          </div>
        )}
        <div className="min-h-0 flex-1">
          <CodeEditor value={current.body ?? ""} onChange={(body) => change(index, (x) => ({ ...x, body }))} language={languageOf(current)} />
        </div>
      </div>
    </div>
  );
}

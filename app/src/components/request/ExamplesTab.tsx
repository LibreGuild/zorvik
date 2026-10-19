// A request's saved responses: what it answers (documentation), and what mocks built from
// it answer. Added from a response with "Save as example".
import { useState } from "react";
import { BookmarkPlus, Trash2, Wand2 } from "lucide-react";
import type { Example } from "../../bindings/Example";
import { formatBytes, statusTone, toneBg } from "../../lib/format";
import { exampleContentType, isJsonExample } from "../../store/examples";
import { type Tab, updateDraft } from "../../store/tabs";
import { toast } from "../../store/toasts";
import { CodeEditor, type EditorLanguage } from "../CodeEditor";
import { KeyValueEditor } from "../KeyValueEditor";
import { Badge, Button, cx, EmptyState, IconButton, Input, Segmented, Select } from "../ui";
import { beautifyJson } from "./jsonFormat";

const RESPONSE_HEADERS = ["Content-Type", "Cache-Control", "ETag", "Location", "Retry-After", "WWW-Authenticate", "Access-Control-Allow-Origin", "Link"];

function languageOf(example: Example): EditorLanguage {
  const ct = exampleContentType(example);
  if (ct.includes("json")) return "json";
  if (ct.includes("html")) return "html";
  if (ct.includes("xml")) return "xml";
  return isJsonExample(example) ? "json" : "text";
}

/** A status code field: kept while typed, saved once it is a code from 100 to 599. */
function StatusInput({ value, onChange }: { value: number; onChange: (status: number) => void }) {
  const [text, setText] = useState<string | null>(null);
  const shown = text ?? String(value);
  const valid = /^[1-5]\d\d$/.test(shown.trim());
  return (
    <Input
      inputMode="numeric"
      value={shown}
      invalid={!valid}
      aria-invalid={!valid || undefined}
      title={valid ? undefined : "A status code from 100 to 599"}
      onChange={(e) => {
        setText(e.target.value);
        if (/^[1-5]\d\d$/.test(e.target.value.trim())) onChange(Number(e.target.value.trim()));
      }}
      onBlur={() => setText(null)}
      aria-label="Status"
      className="h-7 w-16 shrink-0 font-mono text-[12.5px] tabular-nums"
    />
  );
}

export function ExamplesTab({ tab }: { tab: Tab }) {
  const examples = tab.draft.examples ?? [];
  const [selected, setSelected] = useState(0);
  const [view, setView] = useState<"body" | "headers">("body");
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
  const headers = current.headers ?? [];
  const headerCount = headers.filter((h) => h.enabled !== false && h.key.trim()).length;
  const beautify = () => {
    const pretty = beautifyJson(current.body ?? "");
    if (pretty === null) toast("error", "Body is not valid JSON", "Fix the body, then try again.");
    else change(index, (x) => ({ ...x, body: pretty }));
  };
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
          {/* A new key per example: what's being typed doesn't carry over to another one. */}
          <StatusInput key={index} value={current.status} onChange={(status) => change(index, (x) => ({ ...x, status }))} />
          <IconButton label="Delete example" onClick={() => change(index, () => null)}>
            <Trash2 size={14} />
          </IconButton>
        </div>
        <div className="flex h-9 shrink-0 items-center gap-2 px-3">
          <Segmented
            items={[
              { id: "body", label: "Body" },
              { id: "headers", label: headerCount ? `Headers (${headerCount})` : "Headers" },
            ]}
            value={view}
            onChange={setView}
          />
          <div className="flex-1" />
          {view === "body" && (
            <>
              <span className="shrink-0 text-[11.5px] tabular-nums text-faint">{formatBytes(new TextEncoder().encode(current.body ?? "").length)}</span>
              {isJsonExample(current) && (
                <Button size="sm" variant="ghost" icon={<Wand2 size={13} />} onClick={beautify}>
                  Beautify
                </Button>
              )}
            </>
          )}
        </div>
        <div className="min-h-0 flex-1 overflow-auto">
          {view === "body" ? (
            <CodeEditor value={current.body ?? ""} onChange={(body) => change(index, (x) => ({ ...x, body }))} language={languageOf(current)} />
          ) : (
            <div data-testid="example-headers">
              <KeyValueEditor
                rows={headers}
                onChange={(rows) => change(index, (x) => ({ ...x, headers: rows }))}
                keyPlaceholder="Header"
                keySuggestions={RESPONSE_HEADERS}
              />
              <p className="px-3 py-2.5 text-[12px] text-faint">
                Mocks built from this request answer with the headers that are switched on. Content-Length and other framing headers are set by the mock.
              </p>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

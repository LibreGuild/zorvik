// Editable key/value table with an always-present empty row and bulk edit.
import { useState } from "react";
import { GripVertical, Info, Trash2 } from "lucide-react";
import type { KeyValue } from "../bindings/KeyValue";
import { VarInput } from "./VarInput";
import { Checkbox, cx, Tooltip } from "./ui";

export interface KvRow extends KeyValue {
  /** Rendered after the value (e.g. a secret toggle). */
  extra?: React.ReactNode;
}

export function KeyValueEditor({
  rows,
  onChange,
  keyPlaceholder = "Key",
  valuePlaceholder = "Value",
  keySuggestions,
  allowDisable = true,
  fixedKeys = false,
  bulk,
  renderValue,
  onEnter,
}: {
  rows: KeyValue[];
  onChange: (rows: KeyValue[]) => void;
  keyPlaceholder?: string;
  valuePlaceholder?: string;
  keySuggestions?: string[];
  allowDisable?: boolean;
  /** Keys are not editable and rows cannot be added/removed (path params). */
  fixedKeys?: boolean;
  bulk?: boolean;
  renderValue?: (row: KeyValue, index: number, update: (patch: Partial<KeyValue>) => void) => React.ReactNode;
  onEnter?: () => void;
}) {
  const [dragIndex, setDragIndex] = useState<number | null>(null);
  const display = fixedKeys ? rows : [...rows, { key: "", value: "" }];

  const update = (index: number, patch: Partial<KeyValue>) => {
    const next = display.map((r, i) => (i === index ? { ...r, ...patch } : r));
    onChange(next.filter((r, i) => i < rows.length || r.key !== "" || r.value !== ""));
  };
  const remove = (index: number) => onChange(rows.filter((_, i) => i !== index));

  if (bulk) return <BulkEditor rows={rows} onChange={onChange} />;

  return (
    <div className="mx-3 overflow-hidden rounded-lg border border-line text-[12.5px]" role="table">
      {display.map((row, i) => {
        const isNew = !fixedKeys && i === rows.length;
        const enabled = row.enabled !== false;
        return (
          <div
            key={i}
            role="row"
            onDragOver={(e) => {
              if (dragIndex !== null) e.preventDefault();
            }}
            onDrop={() => {
              if (dragIndex === null || isNew || dragIndex === i) return;
              const next = [...rows];
              const [moved] = next.splice(dragIndex, 1);
              next.splice(i, 0, moved);
              onChange(next);
              setDragIndex(null);
            }}
            className={cx(
              "group flex items-center border-b border-line/70 last:border-b-0 hover:bg-hover/40",
              !enabled && !isNew && "opacity-55",
            )}
          >
            {/* role="cell": a role="row" without cells is an empty row to screen readers. */}
            <div
              role="cell"
              draggable={!isNew && !fixedKeys}
              onDragStart={() => setDragIndex(i)}
              onDragEnd={() => setDragIndex(null)}
              className={cx("flex w-5 justify-center text-faint", isNew || fixedKeys ? "invisible" : "cursor-grab opacity-0 group-hover:opacity-100")}
            >
              <GripVertical size={12} />
            </div>
            <div role="cell" className="flex w-6 justify-center">
              {allowDisable && !isNew && <Checkbox checked={enabled} onChange={(v) => update(i, { enabled: v })} title={enabled ? "Disable" : "Enable"} />}
            </div>
            <div role="cell" className="flex w-[38%] min-w-0 items-center border-r border-line/70">
              <div className="min-w-0 flex-1">
                {fixedKeys ? (
                  <div className="truncate px-2 font-mono text-[12.5px] leading-7 text-muted">{row.key}</div>
                ) : (
                  <VarInput
                    value={row.key}
                    onChange={(v) => update(i, { key: v })}
                    placeholder={keyPlaceholder}
                    suggestions={keySuggestions}
                    onEnter={onEnter}
                  />
                )}
              </div>
              {row.description && (
                <Tooltip content={<span className="block max-w-[320px] whitespace-pre-wrap">{row.description}</span>}>
                  <span aria-label={row.description} className="mr-1.5 shrink-0 text-faint hover:text-muted" data-testid="kv-description">
                    <Info size={12} />
                  </span>
                </Tooltip>
              )}
            </div>
            <div role="cell" className="min-w-0 flex-1">
              {renderValue ? (
                renderValue(row, i, (patch) => update(i, patch))
              ) : (
                <VarInput value={row.value} onChange={(v) => update(i, { value: v })} placeholder={valuePlaceholder} onEnter={onEnter} />
              )}
            </div>
            <div role="cell" className="flex w-8 justify-center">
              {!isNew && !fixedKeys && (
                <button
                  aria-label="Remove row"
                  onClick={() => remove(i)}
                  className="rounded p-1 text-faint opacity-0 hover:bg-hover hover:text-danger group-hover:opacity-100 focus:opacity-100"
                >
                  <Trash2 size={13} />
                </button>
              )}
            </div>
          </div>
        );
      })}
    </div>
  );
}

/**
 * Parse bulk-edit text: `key: value` per line, `//` disables. A row keeps the description of the
 * existing row with the same key (bulk text can't show it, so every keystroke used to drop it).
 */
export function parseBulk(text: string, previous: KeyValue[]): KeyValue[] {
  const pool = [...previous];
  return text
    .split("\n")
    .filter((l) => l.trim())
    .map((line) => {
      const disabled = line.trimStart().startsWith("//");
      const body = disabled ? line.trimStart().slice(2) : line;
      const idx = body.indexOf(":");
      const key = (idx >= 0 ? body.slice(0, idx) : body).trim();
      const value = idx >= 0 ? body.slice(idx + 1).trim() : "";
      const match = pool.findIndex((r) => r.key === key);
      const description = match >= 0 ? pool.splice(match, 1)[0].description : undefined;
      return { key, value, ...(disabled ? { enabled: false } : {}), ...(description ? { description } : {}) };
    });
}

/** `key: value` per line; lines starting with `//` are disabled. */
function BulkEditor({ rows, onChange }: { rows: KeyValue[]; onChange: (rows: KeyValue[]) => void }) {
  const [text, setText] = useState(() =>
    rows.map((r) => `${r.enabled === false ? "//" : ""}${r.key}: ${r.value}`).join("\n"),
  );
  return (
    <textarea
      value={text}
      spellCheck={false}
      onChange={(e) => {
        setText(e.target.value);
        onChange(parseBulk(e.target.value, rows));
      }}
      placeholder={"Content-Type: application/json\n//X-Disabled: value"}
      className="mx-3 block h-full min-h-[180px] w-[calc(100%-1.5rem)] resize-none rounded-lg border border-line bg-input p-3 font-mono text-[12.5px] text-fg outline-none focus:border-accent"
    />
  );
}

import { Eye, EyeOff, Lock, LockOpen } from "lucide-react";
import { useState } from "react";
import type { KeyValue } from "../../bindings/KeyValue";
import type { Variable } from "../../bindings/Variable";
import { KeyValueEditor } from "../KeyValueEditor";
import { VarInput } from "../VarInput";
import { cx } from "../ui";

/** Variables table with a per-row secret toggle. */
export function VariablesEditor({ variables, onChange }: { variables: Variable[]; onChange: (v: Variable[]) => void }) {
  // By variable name, not row index: deleting or dragging rows must not reveal a different secret.
  const [revealed, setRevealed] = useState<Record<string, boolean>>({});
  return (
    <KeyValueEditor
      rows={variables as unknown as KeyValue[]}
      onChange={(rows) => onChange(rows.map((r) => ({ ...r, key: r.key })) as unknown as Variable[])}
      keyPlaceholder="Variable"
      renderValue={(row, _i, update) => {
        const secret = (row as unknown as Variable).secret ?? false;
        const show = revealed[row.key] ?? false;
        return (
          <div className="flex items-center">
            <VarInput value={row.value} onChange={(value) => update({ value })} placeholder="Value" secret={secret && !show} className="flex-1" />
            {secret && (
              <button
                aria-label={show ? "Hide value" : "Show value"}
                onClick={() => setRevealed({ ...revealed, [row.key]: !show })}
                className="rounded p-1 text-faint hover:bg-hover hover:text-fg"
              >
                {show ? <EyeOff size={13} /> : <Eye size={13} />}
              </button>
            )}
            <button
              aria-label={secret ? "Stored as secret (click to make plain)" : "Plain value (click to make secret)"}
              title={secret ? "Secret: kept on this computer only, never written to workspace files" : "Plain: saved in the workspace file"}
              onClick={() => update({ secret: !secret } as Partial<KeyValue>)}
              className={cx("mr-1 rounded p-1 hover:bg-hover", secret ? "text-warning" : "text-faint hover:text-fg")}
            >
              {secret ? <Lock size={13} /> : <LockOpen size={13} />}
            </button>
          </div>
        );
      }}
    />
  );
}

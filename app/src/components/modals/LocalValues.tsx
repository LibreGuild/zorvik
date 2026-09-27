// "Set by scripts": current values that scripts set with pm.environment.set,
// pm.collectionVariables.set or pm.globals.set. They live on this computer only
// (never in workspace files) and win over the file values.
import { useCallback, useEffect, useState } from "react";
import { Eye, EyeOff, X } from "lucide-react";
import type { LocalValue } from "../../bindings/LocalValue";
import { api, errorMessage } from "../../lib/rpc";
import { toast } from "../../store/toasts";
import { refreshVariables } from "../../store/workspace";
import { Button, cx, IconButton } from "../ui";

export type LocalScope = "environment" | "workspace" | "globals";

/** `secretKeys`: variables marked secret above; their values stay hidden until shown, like there. */
export function LocalValues({ scope, environmentId, secretKeys = [] }: { scope: LocalScope; environmentId?: string; secretKeys?: string[] }) {
  const [values, setValues] = useState<LocalValue[]>([]);
  const [revealed, setRevealed] = useState<Record<string, boolean>>({});
  const load = useCallback(async () => {
    const all = await api.localValues().catch(() => []);
    setValues(all.filter((v) => v.scope === scope && (scope !== "environment" || v.environmentId === environmentId)));
  }, [scope, environmentId]);
  useEffect(() => {
    void load();
  }, [load]);

  const clear = async (key?: string) => {
    try {
      await api.clearLocalValues(scope, environmentId ?? null, key ?? null);
      await load();
      await refreshVariables();
    } catch (e) {
      toast("error", "Could not clear", errorMessage(e));
    }
  };

  if (!values.length) {
    return scope === "globals" ? (
      <p className="px-3 py-2 text-[12px] text-faint">No global values yet. Scripts set them with pm.globals.set; they apply to every workspace.</p>
    ) : null;
  }
  return (
    <div className="px-3 pb-3 pt-2" data-testid="local-values">
      <div className="flex items-center justify-between">
        <div className="text-[11px] font-semibold uppercase tracking-wide text-faint">Set by scripts</div>
        <Button size="sm" variant="ghost" onClick={() => void clear()}>
          Clear all
        </Button>
      </div>
      <p className="pb-1.5 text-[11.5px] text-faint">
        Current values from scripts. They override the values above, stay on this computer and are never written to workspace files.
      </p>
      <div className="overflow-hidden rounded-lg border border-line">
        {values.map((v) => {
          const secret = secretKeys.includes(v.key);
          const hidden = secret && !revealed[v.key] && v.value !== "";
          return (
            <div key={v.key} className="flex items-center gap-2 border-b border-line/60 px-2.5 py-1 text-[12px] last:border-b-0">
              <span className="w-[30%] shrink-0 truncate font-mono text-fg" title={v.key}>
                {v.key}
              </span>
              <span className={cx("min-w-0 flex-1 truncate font-mono text-muted", !hidden && "selectable")} title={hidden ? undefined : v.value}>
                {v.value === "" ? <i className="text-faint">empty</i> : hidden ? "••••••••" : v.value}
              </span>
              {secret && (
                <IconButton label={revealed[v.key] ? `Hide ${v.key}` : `Show ${v.key}`} size={22} onClick={() => setRevealed({ ...revealed, [v.key]: !revealed[v.key] })}>
                  {revealed[v.key] ? <EyeOff size={12} /> : <Eye size={12} />}
                </IconButton>
              )}
              <IconButton label={`Clear ${v.key}`} size={22} onClick={() => void clear(v.key)}>
                <X size={12} />
              </IconButton>
            </div>
          );
        })}
      </div>
    </div>
  );
}

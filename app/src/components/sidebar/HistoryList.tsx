import { useEffect, useMemo, useState } from "react";
import { History as HistoryIcon, Search, Trash2 } from "lucide-react";
import type { HistoryEntry } from "../../bindings/HistoryEntry";
import { formatMs, statusTone, toneText } from "../../lib/format";
import { GRAPHQL_BADGE, methodColor, methodLabel } from "../../lib/http";
import { api, errorMessage } from "../../lib/rpc";
import { confirm } from "../../store/dialogs";
import { isRequestTab, openDraft, useTabs } from "../../store/tabs";
import { toast } from "../../store/toasts";
import { useWorkspace } from "../../store/workspace";
import { cx, EmptyState, IconButton } from "../ui";

function dayLabel(ms: number): string {
  const d = new Date(ms);
  const today = new Date();
  const yesterday = new Date(Date.now() - 86_400_000);
  if (d.toDateString() === today.toDateString()) return "Today";
  if (d.toDateString() === yesterday.toDateString()) return "Yesterday";
  return d.toLocaleDateString(undefined, { weekday: "short", month: "short", day: "numeric" });
}

export function HistoryList() {
  const [entries, setEntries] = useState<HistoryEntry[]>([]);
  const [search, setSearch] = useState("");
  const [loading, setLoading] = useState(true);
  // Reload when any request finishes.
  const finished = useTabs((s) => {
    const tabs = s.tabs.filter(isRequestTab);
    return tabs.filter((t) => t.response.status === "done" || t.response.status === "error").length + tabs.map((t) => (t.response.status === "done" ? t.response.at : 0)).join();
  });
  // History is per workspace; the list stays mounted when switching.
  const workspace = useWorkspace((s) => s.info?.path);

  useEffect(() => {
    let alive = true;
    const t = setTimeout(() => {
      api
        .history(search)
        .then((e) => alive && setEntries(e))
        .catch(() => {})
        .finally(() => alive && setLoading(false));
    }, 150);
    return () => {
      alive = false;
      clearTimeout(t);
    };
  }, [search, finished, workspace]);

  const groups = useMemo(() => {
    const out: { label: string; items: HistoryEntry[] }[] = [];
    for (const e of entries) {
      const label = dayLabel(e.createdAt);
      if (out.at(-1)?.label !== label) out.push({ label, items: [] });
      out.at(-1)!.items.push(e);
    }
    return out;
  }, [entries]);

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex items-center gap-1 px-2 py-2">
        <div className="flex h-7 min-w-0 flex-1 items-center gap-1.5 rounded-lg border border-transparent bg-panel-2 px-2 focus-within:border-accent">
          <Search size={12} className="shrink-0 text-faint" />
          <input
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="Search history"
            className="min-w-0 flex-1 bg-transparent text-[12.5px] outline-none placeholder:text-faint"
          />
        </div>
        <IconButton
          label="Clear history"
          onClick={async () => {
            if (!(await confirm({ title: "Clear history?", message: "All history entries for this workspace will be deleted.", confirmLabel: "Clear", danger: true }))) return;
            try {
              await api.clearHistory();
              setEntries([]);
            } catch (e) {
              toast("error", "Could not clear history", errorMessage(e));
            }
          }}
        >
          <Trash2 size={14} />
        </IconButton>
      </div>
      <div className="min-h-0 flex-1 overflow-auto px-1.5 pb-4">
        {!loading && entries.length === 0 ? (
          <EmptyState icon={<HistoryIcon size={24} strokeWidth={1.5} />} title={search ? "No matches" : "No history yet"}>
            {search ? "Try another search." : "Requests you send show up here."}
          </EmptyState>
        ) : (
          groups.map((g) => (
            <div key={g.label}>
              <div className="px-2 pb-1 pt-3 text-[11px] font-semibold uppercase tracking-wide text-faint">{g.label}</div>
              {g.items.map((e) => (
                <button
                  key={e.id}
                  onClick={() => openDraft({ ...e.request, name: e.request.name || "From history" })}
                  className="group flex w-full items-center gap-2 rounded-lg px-2 py-1.5 text-left hover:bg-hover/70"
                  title={e.url}
                >
                  <span
                    className="w-[34px] shrink-0 text-right font-mono text-[10px] font-bold"
                    style={{ color: e.request.body?.type === "graphql" ? GRAPHQL_BADGE.color : methodColor(e.method, e.request.kind) }}
                  >
                    {e.request.body?.type === "graphql" ? GRAPHQL_BADGE.label : methodLabel(e.method, e.request.kind)}
                  </span>
                  <span className="min-w-0 flex-1">
                    <span className="block truncate font-mono text-[11.5px] text-fg">{e.url.replace(/^https?:\/\//, "")}</span>
                    <span className="flex items-center gap-2 text-[11px] text-faint">
                      {e.status != null ? (
                        <span className={cx("font-semibold", toneText[statusTone(e.status)])}>{e.status}</span>
                      ) : (
                        <span className="text-danger">Failed</span>
                      )}
                      <span>{formatMs(e.durationMs)}</span>
                      <span>{new Date(e.createdAt).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" })}</span>
                    </span>
                  </span>
                </button>
              ))}
            </div>
          ))
        )}
      </div>
    </div>
  );
}

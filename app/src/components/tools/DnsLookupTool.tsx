// DNS lookup tool: any record type, from the system resolver or a server of your
// choice. State lives in the tool tab, so answers survive switching tabs.
import { History, Search, Square } from "lucide-react";
import type { DnsQueryResult } from "../../bindings/DnsQueryResult";
import type { ErrorKind } from "../../bindings/ErrorKind";
import type { Request } from "../../bindings/Request";
import { api, RpcError, errorMessage } from "../../lib/rpc";
import { isToolTab, updateToolState, useTabs } from "../../store/tabs";
import { toast } from "../../store/toasts";
import { ResolverField } from "../request/DnsOptions";
import { RecordTypePicker } from "../request/DnsRecordPicker";
import { DnsError, DnsLoading, DnsResultView } from "../request/DnsResultPane";
import { bareName, isIpAddress, reverseName } from "../request/dnsModel";
import { VarInput } from "../VarInput";
import { Button, cx, EmptyState, Switch } from "../ui";
import type { ToolProps } from "./registry";

interface Recent {
  name: string;
  type: string;
  server: string;
}

interface DnsToolState {
  name: string;
  type: string;
  server: string;
  recursion: boolean;
  status: "idle" | "loading" | "done" | "error";
  startedAt?: number;
  result?: DnsQueryResult;
  error?: { message: string; code: string; kind: ErrorKind | null; durationMs: number };
  recent: Recent[];
}

const MAX_RECENT = 8;

function readState(state: Record<string, unknown>): DnsToolState {
  return { name: "", type: "A", server: "", recursion: true, status: "idle", recent: [], ...(state as Partial<DnsToolState>) };
}

function update(tabId: string, fn: (s: DnsToolState) => Partial<DnsToolState>) {
  updateToolState(tabId, (raw) => {
    const s = readState(raw);
    return { ...s, ...fn(s) };
  });
}

function current(tabId: string): DnsToolState | null {
  const tab = useTabs.getState().tabs.find((t) => t.id === tabId);
  return isToolTab(tab) ? readState(tab.state) : null;
}

async function runQuery(tabId: string) {
  const s = current(tabId);
  if (!s || s.status === "loading") return;
  if (!s.name.trim()) {
    toast("info", "Enter a name to look up");
    return;
  }
  const startedAt = Date.now();
  update(tabId, () => ({ status: "loading", startedAt, error: undefined }));
  // Only the query that set this loading state may settle it.
  const settle = (patch: Partial<DnsToolState>) => {
    if (current(tabId)?.startedAt === startedAt) update(tabId, () => patch);
  };
  const request: Request = {
    name: "DNS lookup",
    kind: "dns",
    seq: 0,
    method: s.type,
    url: s.name,
    dns: { server: s.server, recursion: s.recursion },
  };
  try {
    const result = await api.dnsQuery(tabId, request, null);
    const entry: Recent = { name: s.name.trim(), type: s.type, server: s.server };
    const same = (r: Recent) => r.name === entry.name && r.type === entry.type && r.server === entry.server;
    settle({ status: "done", result, recent: [entry, ...s.recent.filter((r) => !same(r))].slice(0, MAX_RECENT) });
  } catch (e) {
    const err = e instanceof RpcError ? e : new RpcError({ code: "internal", message: errorMessage(e), networkKind: null });
    settle({ status: "error", error: { message: err.message, code: err.code, kind: err.networkKind, durationMs: Date.now() - startedAt } });
  }
}

export function DnsLookupTool({ tab }: ToolProps) {
  const s = readState(tab.state);
  const loading = s.status === "loading";
  const ip = s.name.trim() !== "" && !s.name.includes("{{") && isIpAddress(s.name);
  const query = () => void runQuery(tab.id);
  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex shrink-0 flex-col gap-3 px-4 pb-3 pt-1">
        <div className="flex items-center gap-2">
          <RecordTypePicker boxed value={s.type} onChange={(type) => update(tab.id, () => ({ type }))} />
          <div className="flex h-8 min-w-0 flex-1 items-center rounded-lg border border-line bg-input focus-within:border-accent focus-within:ring-2 focus-within:ring-accent-soft">
            <VarInput
              value={s.name}
              onChange={(name) => update(tab.id, () => ({ name }))}
              onEnter={query}
              autoFocus={!s.name}
              placeholder="example.com, or an IP address for PTR"
              className="flex-1"
              ariaLabel="Name to look up"
            />
          </div>
          {loading ? (
            <Button icon={<Square size={13} />} className="w-[96px]" onClick={() => void api.cancel(tab.id).catch(() => {})}>
              Cancel
            </Button>
          ) : (
            <Button variant="primary" icon={<Search size={14} />} className="w-[96px]" onClick={query}>
              Look up
            </Button>
          )}
        </div>
        {ip && (
          <div className="-mt-1 text-[11.5px] text-muted">
            {s.type === "PTR" ? (
              <>
                Asks for <span className="font-mono text-fg">{reverseName(s.name)}</span>
              </>
            ) : (
              <>
                {bareName(s.name)} is an IP address.{" "}
                <button className="font-medium text-accent hover:underline" onClick={() => update(tab.id, () => ({ type: "PTR" }))}>
                  Look up its name (PTR)
                </button>
              </>
            )}
          </div>
        )}
        <div className="flex flex-wrap items-start gap-x-6 gap-y-2">
          <div className="min-w-[280px] flex-1">
            <ResolverField server={s.server} onChange={(server) => update(tab.id, () => ({ server }))} onEnter={query} />
          </div>
          <div className="pt-1">
            <Switch checked={s.recursion} onChange={(recursion) => update(tab.id, () => ({ recursion }))} label="Recursion" />
          </div>
        </div>
        {s.recent.length > 0 && (
          <div className="flex min-w-0 items-center gap-1.5 overflow-x-auto [scrollbar-width:none]">
            <History size={12} className="shrink-0 text-faint" aria-label="Recent" />
            {s.recent.map((r, i) => (
              <button
                key={i}
                title={r.server ? `${r.type} ${r.name} @ ${r.server}` : `${r.type} ${r.name}`}
                onClick={() => {
                  update(tab.id, () => ({ name: r.name, type: r.type, server: r.server }));
                  void runQuery(tab.id);
                }}
                className={cx(
                  "flex h-6 shrink-0 items-center gap-1.5 rounded-md border border-line bg-panel-2 px-2 text-[11.5px] text-muted hover:border-line-strong hover:text-fg",
                )}
              >
                <span className="font-mono font-semibold" style={{ color: "var(--m-dns)" }}>
                  {r.type}
                </span>
                <span className="max-w-[200px] truncate font-mono">{r.name}</span>
              </button>
            ))}
          </div>
        )}
      </div>
      <div className="min-h-0 flex-1 border-t border-line/70">
        {s.status === "loading" && <DnsLoading startedAt={s.startedAt ?? Date.now()} onCancel={() => void api.cancel(tab.id).catch(() => {})} />}
        {s.status === "error" && s.error && <DnsError {...s.error} />}
        {s.status === "done" && s.result && <DnsResultView result={s.result} />}
        {s.status === "idle" && (
          <EmptyState icon={<Search size={30} strokeWidth={1.5} />} title="Look up a name">
            Every section of the answer is shown, with the flags and response code. Press Enter to look up.
          </EmptyState>
        )}
      </div>
    </div>
  );
}

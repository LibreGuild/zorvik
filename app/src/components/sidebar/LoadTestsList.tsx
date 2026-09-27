// Sidebar: the workspace's load tests, which one runs, and quick start/stop.
import { useMemo, useState } from "react";
import { Activity, Copy, Layers, MoreHorizontal, Pencil, Play, Plus, Search, Square, Trash2, TriangleAlert } from "lucide-react";
import type { LoadTestNode } from "../../bindings/LoadTestNode";
import { api, errorMessage } from "../../lib/rpc";
import {
  deleteLoadTest,
  duplicateLoadTest,
  loadTestFolder,
  newLoadTestPrompt,
  renameLoadTest,
  reorderLoadTests,
  runFor,
  startLoadRun,
  stopLoadRun,
  useLoadTests,
} from "../../store/loadtests";
import { isLoadTestTab, openLoadTest, updateLoadTestTab, useTabs } from "../../store/tabs";
import { toast } from "../../store/toasts";
import { useWorkspace } from "../../store/workspace";
import { formatDuration, MODELS } from "../loadtests/model";
import { ContextMenu, cx, EmptyState, Menu, type MenuEntry } from "../ui";

const newEntries = (): MenuEntry[] => [
  { label: "New load test", icon: <Activity size={14} />, onSelect: () => void newLoadTestPrompt() },
  {
    label: "Load test the collection…",
    icon: <Layers size={14} />,
    onSelect: () => {
      const info = useWorkspace.getState().info;
      if (info) void loadTestFolder(info.tree, "", info.meta.name);
    },
  },
];

export function LoadTestsList() {
  const saved = useLoadTests((s) => s.saved);
  const [query, setQuery] = useState("");
  const q = query.trim().toLowerCase();
  const list = useMemo(() => (q ? saved.filter((n) => n.name.toLowerCase().includes(q)) : saved), [saved, q]);
  const [dragId, setDragId] = useState<string | null>(null);

  return (
    <div className="flex min-h-0 flex-1 flex-col" data-testid="loadtests-list">
      <div className="flex items-center gap-1 px-2 py-2">
        <div className="flex h-7 min-w-0 flex-1 items-center gap-1.5 rounded-lg border border-transparent bg-panel-2 px-2 focus-within:border-accent">
          <Search size={12} className="shrink-0 text-faint" />
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Filter"
            aria-label="Filter load tests"
            className="min-w-0 flex-1 bg-transparent text-[12.5px] outline-none placeholder:text-faint"
          />
        </div>
        <Menu
          align="end"
          trigger={
            <button aria-label="New load test" className="flex h-7 w-7 items-center justify-center rounded-lg text-muted hover:bg-hover hover:text-fg">
              <Plus size={15} />
            </button>
          }
          entries={newEntries()}
        />
      </div>
      <div className="min-h-0 flex-1 overflow-auto px-1.5 pb-6" role="tree" aria-label="Load tests">
        {list.length === 0 ? (
          q ? (
            <div className="p-4 text-center text-[12.5px] text-muted">No load tests match “{query}”.</div>
          ) : (
            <EmptyState icon={<Activity size={24} strokeWidth={1.5} />} title="No load tests yet">
              <div className="mt-1 flex flex-col items-center gap-2">
                <span>
                  Pick saved requests, choose virtual users or a request rate, and see honest latency percentiles, errors and throughput while it runs. Load tests are
                  saved in the workspace.
                </span>
                <span className="text-faint">Tip: right-click a request or folder and choose “Load test…”.</span>
                <button className="text-accent hover:underline" onClick={() => void newLoadTestPrompt()}>
                  New load test
                </button>
              </div>
            </EmptyState>
          )
        ) : (
          list.map((node) => (
            <LoadTestRow
              key={node.id}
              node={node}
              dragging={dragId}
              onDragStart={() => setDragId(node.id)}
              onDragEnd={() => setDragId(null)}
              onDropOn={() => {
                if (!dragId || dragId === node.id) return;
                const ids = saved.map((n) => n.id).filter((id) => id !== dragId);
                ids.splice(ids.indexOf(node.id), 0, dragId);
                void reorderLoadTests(ids);
              }}
            />
          ))
        )}
      </div>
    </div>
  );
}

/** Start a saved test from the list: its open tab's draft when there is one (it may have unsaved edits). */
async function startFromList(node: LoadTestNode) {
  const tab = useTabs.getState().tabs.find((t) => isLoadTestTab(t) && t.testId === node.id);
  try {
    const test = isLoadTestTab(tab) ? tab.draft : await api.readLoadTest(node.id);
    if (isLoadTestTab(tab)) updateLoadTestTab(tab.id, () => ({ runId: null }));
    await startLoadRun(node.id, test);
  } catch (e) {
    toast("error", `Could not start “${node.name}”`, errorMessage(e));
  }
}

function LoadTestRow({
  node,
  dragging,
  onDragStart,
  onDragEnd,
  onDropOn,
}: {
  node: LoadTestNode;
  dragging: string | null;
  onDragStart: () => void;
  onDragEnd: () => void;
  onDropOn: () => void;
}) {
  const running = useLoadTests((s) => !!runFor(node.id, s.active));
  const other = useLoadTests((s) => (s.active && !runFor(node.id, s.active) ? s.active.name : null));
  const starting = useLoadTests((s) => s.starting !== null);
  const active = useTabs((s) => {
    const t = s.tabs.find((x) => x.id === s.activeId);
    return isLoadTestTab(t) && t.testId === node.id;
  });
  const [over, setOver] = useState(false);
  const info = MODELS[node.model];
  const cannotStart = !!node.error || !!other || starting;
  const toggle = () => (running ? void stopLoadRun() : void startFromList(node));
  const entries: MenuEntry[] = [
    running
      ? { label: "Stop", icon: <Square size={14} />, onSelect: () => void stopLoadRun() }
      : { label: other ? `Run (“${other}” is running)` : "Run", icon: <Play size={14} />, disabled: cannotStart, onSelect: () => void startFromList(node) },
    { separator: true },
    { label: "Rename…", icon: <Pencil size={14} />, onSelect: () => void renameLoadTest(node) },
    { label: "Duplicate", icon: <Copy size={14} />, onSelect: () => void duplicateLoadTest(node) },
    { separator: true },
    { label: "Delete", icon: <Trash2 size={14} />, danger: true, onSelect: () => void deleteLoadTest(node) },
  ];
  const meta = `${formatDuration(node.durationSecs)} · ${node.targets} ${node.targets === 1 ? "request" : "requests"} · ${info.label.toLowerCase()}`;
  return (
    <ContextMenu entries={entries}>
      <div
        draggable
        onDragStart={(e) => {
          e.dataTransfer.setData("application/x-zv-loadtest", node.id);
          e.dataTransfer.effectAllowed = "move";
          onDragStart();
        }}
        onDragEnd={onDragEnd}
        onDragOver={(e) => {
          if (!dragging) return;
          e.preventDefault();
          setOver(true);
        }}
        onDragLeave={() => setOver(false)}
        onDrop={(e) => {
          e.preventDefault();
          setOver(false);
          onDropOn();
        }}
        onClick={(e) => {
          if (!e.currentTarget.contains(e.target as Node)) return; // portaled menu clicks
          void openLoadTest(node.id);
        }}
        onKeyDown={(e) => {
          if (e.target !== e.currentTarget) return;
          if (e.key === "Enter") void openLoadTest(node.id);
          if (e.key === "F2") void renameLoadTest(node);
          if (e.key === "Delete" || (e.key === "Backspace" && e.metaKey)) void deleteLoadTest(node);
        }}
        tabIndex={0}
        role="treeitem"
        aria-selected={active}
        title={node.error ?? `${node.name}\n${meta}`}
        data-testid={`loadtest-row-${node.id}`}
        className={cx(
          "group relative flex h-[30px] items-center gap-2 rounded-lg px-1.5 text-[12.5px] outline-none focus-visible:ring-1 focus-visible:ring-accent",
          active ? "bg-elev text-fg" : "text-fg/90 hover:bg-hover/70",
        )}
      >
        {over && <div className="absolute inset-x-1 top-0 h-[2px] rounded bg-accent" />}
        <span className="w-[30px] shrink-0 text-right font-mono text-[10px] font-bold text-accent">{info.short}</span>
        <span className="min-w-0 flex-1 truncate">{node.name}</span>
        {node.error ? (
          <TriangleAlert size={13} className="shrink-0 text-warning" />
        ) : (
          <span className={cx("shrink-0 text-[11px] tabular-nums", running ? "text-accent" : "text-faint")}>
            {formatDuration(node.durationSecs)}
            <span className="text-faint/70"> · </span>
            {node.targets}
          </span>
        )}
        <button
          aria-label={running ? `Stop ${node.name}` : `Run ${node.name}`}
          title={running ? "Stop" : other ? `“${other}” is running` : "Run"}
          disabled={!running && cannotStart}
          onClick={(e) => {
            e.stopPropagation();
            toggle();
          }}
          className={cx(
            "flex h-5 w-5 shrink-0 items-center justify-center rounded hover:bg-panel-2 disabled:opacity-40",
            running ? "text-accent" : "text-faint opacity-0 hover:text-fg focus-visible:opacity-100 group-hover:opacity-100",
          )}
        >
          {running ? (
            <>
              <span className="h-2 w-2 animate-pulse rounded-full bg-accent group-hover:hidden" />
              <Square size={11} className="hidden group-hover:block" />
            </>
          ) : (
            <Play size={12} />
          )}
        </button>
        <Menu
          align="end"
          entries={entries}
          trigger={
            <button
              aria-label="Load test actions"
              onClick={(e) => e.stopPropagation()}
              className="flex h-5 w-5 shrink-0 items-center justify-center rounded text-faint opacity-0 hover:bg-panel-2 hover:text-fg focus-visible:opacity-100 group-hover:opacity-100 data-[state=open]:opacity-100"
            >
              <MoreHorizontal size={13} />
            </button>
          }
        />
      </div>
    </ContextMenu>
  );
}

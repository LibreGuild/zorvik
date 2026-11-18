// Sidebar: the workspace's scenarios, which one runs, and quick run/stop.
import { useMemo, useState } from "react";
import { Copy, Layers, MoreHorizontal, Pencil, Play, Plus, Search, Square, Trash2, TriangleAlert, Workflow } from "lucide-react";
import type { ScenarioNode } from "../../bindings/ScenarioNode";
import { stopRun, useRunner } from "../../store/runner";
import { deleteScenario, duplicateScenario, newScenarioPrompt, renameScenario, reorderScenarios, runScenario, scenarioFromFolder, useScenarios } from "../../store/scenarios";
import { isScenarioTab, openScenario, type ScenarioTab, useTabs } from "../../store/tabs";
import { useWorkspace } from "../../store/workspace";
import { ContextMenu, cx, EmptyState, Menu, type MenuEntry } from "../ui";

const newEntries = (): MenuEntry[] => [
  { label: "New scenario", icon: <Workflow size={14} />, onSelect: () => void newScenarioPrompt() },
  {
    label: "Scenario from the collection…",
    icon: <Layers size={14} />,
    onSelect: () => {
      const info = useWorkspace.getState().info;
      if (info) void scenarioFromFolder(info.tree, "", info.meta.name);
    },
  },
];

export function ScenariosList() {
  const saved = useScenarios((s) => s.saved);
  const [query, setQuery] = useState("");
  const q = query.trim().toLowerCase();
  const list = useMemo(() => (q ? saved.filter((n) => n.name.toLowerCase().includes(q)) : saved), [saved, q]);
  const [dragId, setDragId] = useState<string | null>(null);

  return (
    <div className="flex min-h-0 flex-1 flex-col" data-testid="scenarios-list">
      <div className="flex items-center gap-1 px-2 py-2">
        <div className="flex h-7 min-w-0 flex-1 items-center gap-1.5 rounded-lg border border-transparent bg-panel-2 px-2 focus-within:border-accent">
          <Search size={12} className="shrink-0 text-faint" />
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Filter"
            aria-label="Filter scenarios"
            className="min-w-0 flex-1 bg-transparent text-[12.5px] outline-none placeholder:text-faint"
          />
        </div>
        <Menu
          align="end"
          trigger={
            <button aria-label="New scenario" className="flex h-7 w-7 items-center justify-center rounded-lg text-muted hover:bg-hover hover:text-fg">
              <Plus size={15} />
            </button>
          }
          entries={newEntries()}
        />
      </div>
      <div className="min-h-0 flex-1 overflow-auto px-1.5 pb-6" role="tree" aria-label="Scenarios">
        {list.length === 0 ? (
          q ? (
            <div className="p-4 text-center text-[12.5px] text-muted">No scenarios match “{query}”.</div>
          ) : (
            <EmptyState icon={<Workflow size={24} strokeWidth={1.5} />} title="No scenarios yet">
              <div className="mt-1 flex flex-col items-center gap-2">
                <span>
                  Run saved requests one after the other as steps: pass a value from one response to the next request, check what comes back, branch and loop.
                  Scenarios are saved in the workspace.
                </span>
                <span className="text-faint">Tip: right-click a folder and choose “New scenario…”.</span>
                <button className="text-accent hover:underline" onClick={() => void newScenarioPrompt()}>
                  New scenario
                </button>
              </div>
            </EmptyState>
          )
        ) : (
          list.map((node) => (
            <ScenarioRow
              key={node.id}
              node={node}
              dragging={dragId}
              onDragStart={() => setDragId(node.id)}
              onDragEnd={() => setDragId(null)}
              onDropOn={() => {
                if (!dragId || dragId === node.id) return;
                const ids = saved.map((n) => n.id).filter((id) => id !== dragId);
                ids.splice(ids.indexOf(node.id), 0, dragId);
                void reorderScenarios(ids);
              }}
            />
          ))
        )}
      </div>
    </div>
  );
}

/** Run a saved scenario from the list: in its tab (opened when it isn't), with the tab's draft. */
async function runFromList(node: ScenarioNode) {
  const tabId = await openScenario(node.id);
  const tab = useTabs.getState().tabs.find((t): t is ScenarioTab => isScenarioTab(t) && t.id === tabId);
  if (tab) await runScenario(tab);
}

function ScenarioRow({
  node,
  dragging,
  onDragStart,
  onDragEnd,
  onDropOn,
}: {
  node: ScenarioNode;
  dragging: string | null;
  onDragStart: () => void;
  onDragEnd: () => void;
  onDropOn: () => void;
}) {
  const tabId = useTabs((s) => s.tabs.find((t) => isScenarioTab(t) && t.scenarioId === node.id)?.id ?? null);
  const running = useRunner((s) => !!tabId && s.active === tabId);
  const busy = useRunner((s) => s.active !== null || s.starting !== null);
  const active = useTabs((s) => {
    const t = s.tabs.find((x) => x.id === s.activeId);
    return isScenarioTab(t) && t.scenarioId === node.id;
  });
  const [over, setOver] = useState(false);
  const cannotRun = !!node.error || busy;
  const toggle = () => (running && tabId ? void stopRun(tabId) : void runFromList(node));
  const entries: MenuEntry[] = [
    running && tabId
      ? { label: "Stop", icon: <Square size={14} />, onSelect: () => void stopRun(tabId) }
      : { label: "Run", icon: <Play size={14} />, disabled: cannotRun, onSelect: () => void runFromList(node) },
    { separator: true },
    { label: "Rename…", icon: <Pencil size={14} />, onSelect: () => void renameScenario(node) },
    { label: "Duplicate", icon: <Copy size={14} />, onSelect: () => void duplicateScenario(node) },
    { separator: true },
    { label: "Delete", icon: <Trash2 size={14} />, danger: true, onSelect: () => void deleteScenario(node) },
  ];
  const meta = `${node.steps} ${node.steps === 1 ? "step" : "steps"}`;
  return (
    <ContextMenu entries={entries}>
      <div
        draggable
        onDragStart={(e) => {
          e.dataTransfer.setData("application/x-zv-scenario", node.id);
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
          void openScenario(node.id);
        }}
        onKeyDown={(e) => {
          if (e.target !== e.currentTarget) return;
          if (e.key === "Enter") void openScenario(node.id);
          if (e.key === "F2") void renameScenario(node);
          if (e.key === "Delete" || (e.key === "Backspace" && e.metaKey)) void deleteScenario(node);
        }}
        tabIndex={0}
        role="treeitem"
        aria-selected={active}
        title={node.error ?? `${node.name}\n${meta}`}
        data-testid={`scenario-row-${node.id}`}
        className={cx(
          "group relative flex h-[30px] items-center gap-2 rounded-lg px-1.5 text-[12.5px] outline-none focus-visible:ring-1 focus-visible:ring-accent",
          active ? "bg-elev text-fg" : "text-fg/90 hover:bg-hover/70",
        )}
      >
        {over && <div className="absolute inset-x-1 top-0 h-[2px] rounded bg-accent" />}
        <Workflow size={13} className="ml-1 shrink-0 text-accent" />
        <span className="min-w-0 flex-1 truncate">{node.name}</span>
        {node.error ? (
          <TriangleAlert size={13} className="shrink-0 text-warning" />
        ) : (
          <span className={cx("shrink-0 text-[11px] tabular-nums", running ? "text-accent" : "text-faint")}>{node.steps}</span>
        )}
        <button
          aria-label={running ? `Stop ${node.name}` : `Run ${node.name}`}
          title={running ? "Stop" : busy ? "A run is in progress" : "Run"}
          disabled={!running && cannotRun}
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
              aria-label="Scenario actions"
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

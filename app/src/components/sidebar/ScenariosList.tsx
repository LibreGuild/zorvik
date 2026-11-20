// Sidebar: the workspace's scenarios in their folders, which one runs, and quick run/stop.
import { useMemo, useState } from "react";
import { ChevronRight, Copy, Folder, FolderInput, FolderOpen, FolderPlus, Layers, MoreHorizontal, Pencil, Play, Plus, Search, Square, Trash2, TriangleAlert, Workflow } from "lucide-react";
import type { ScenarioNode } from "../../bindings/ScenarioNode";
import { stopRun, useRunner } from "../../store/runner";
import {
  deleteScenario,
  deleteScenarioFolder,
  duplicateScenario,
  moveScenarioNode,
  newScenarioFolder,
  newScenarioPrompt,
  renameScenario,
  renameScenarioFolder,
  runScenario,
  scenarioFolderKey,
  scenarioFromFolder,
  useScenarios,
} from "../../store/scenarios";
import { isScenarioTab, openScenario, type ScenarioTab, useTabs } from "../../store/tabs";
import { openModal, toggleExpanded, useUi } from "../../store/ui";
import { useWorkspace } from "../../store/workspace";
import { filterScenarios, flattenScenarios, scenarioParent } from "../scenarios/model";
import { ContextMenu, cx, EmptyState, Menu, type MenuEntry } from "../ui";

/** What a dragged row carries: the scenario's or folder's id, and which of the two it is. */
const SCENARIO_DRAG = "application/x-zv-scenario";
const dragged = (e: React.DragEvent) => e.dataTransfer.types.includes(SCENARIO_DRAG);
const draggedNode = (e: React.DragEvent): ScenarioNode | null => {
  try {
    return JSON.parse(e.dataTransfer.getData(SCENARIO_DRAG)) as ScenarioNode;
  } catch {
    return null;
  }
};

const newEntries = (folder: string): MenuEntry[] => [
  { label: "New scenario", icon: <Workflow size={14} />, onSelect: () => void newScenarioPrompt(folder) },
  { label: "New folder", icon: <FolderPlus size={14} />, onSelect: () => void newScenarioFolder(folder) },
  ...(folder
    ? []
    : [
        {
          label: "Scenario from the collection…",
          icon: <Layers size={14} />,
          onSelect: () => {
            const info = useWorkspace.getState().info;
            if (info) void scenarioFromFolder(info.tree, "", info.meta.name);
          },
        },
      ]),
];

export function ScenariosList() {
  const tree = useScenarios((s) => s.tree);
  const [query, setQuery] = useState("");
  const [dropRoot, setDropRoot] = useState(false);
  const q = query.trim().toLowerCase();
  const nodes = useMemo(() => filterScenarios(tree, q), [tree, q]);

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
          entries={newEntries("")}
        />
      </div>
      <ContextMenu entries={newEntries("")}>
        {/* Dropped on the list itself (below the rows): to the end of the top level. */}
        <div
          className={cx("min-h-0 flex-1 overflow-auto px-1.5 pb-6", dropRoot && "bg-accent-soft")}
          role="tree"
          aria-label="Scenarios"
          onDragOver={(e) => {
            if (!dragged(e)) return;
            e.preventDefault();
            setDropRoot(true);
          }}
          onDragLeave={() => setDropRoot(false)}
          onDrop={(e) => {
            setDropRoot(false);
            const node = draggedNode(e);
            if (node) void moveScenarioNode(node, "", null);
          }}
        >
          {nodes.length === 0 ? (
            q ? (
              <div className="p-4 text-center text-[12.5px] text-muted">No scenarios match “{query}”.</div>
            ) : (
              <EmptyState icon={<Workflow size={24} strokeWidth={1.5} />} title="No scenarios yet">
                <div className="mt-1 flex flex-col items-center gap-2">
                  <span>
                    Run saved requests one after the other as steps: pass a value from one response to the next request, check what comes back, branch and loop.
                    Scenarios are saved in the workspace, in folders if you like.
                  </span>
                  <span className="text-faint">Tip: right-click a folder of the collection and choose “New scenario…”.</span>
                  <button className="text-accent hover:underline" onClick={() => void newScenarioPrompt()}>
                    New scenario
                  </button>
                </div>
              </EmptyState>
            )
          ) : (
            nodes.map((n, i) => <ScenarioTreeRow key={`${n.folder ? "f" : "s"}:${n.id}`} node={n} depth={0} index={i} forceOpen={!!q} />)
          )}
        </div>
      </ContextMenu>
    </div>
  );
}

/** Run a saved scenario from the list: in its tab (opened when it isn't), with the tab's draft. */
async function runFromList(node: ScenarioNode) {
  const tabId = await openScenario(node.id);
  const tab = useTabs.getState().tabs.find((t): t is ScenarioTab => isScenarioTab(t) && t.id === tabId);
  if (tab) await runScenario(tab);
}

/** A scenario or a folder, and (open) what is in the folder. */
function ScenarioTreeRow({ node, depth, index, forceOpen }: { node: ScenarioNode; depth: number; index: number; forceOpen: boolean }) {
  const folder = !!node.folder;
  const expanded = useUi((s) => !!s.expanded[scenarioFolderKey(node.id)]);
  const open = folder && (forceOpen || expanded);
  const [drop, setDrop] = useState<"before" | "inside" | null>(null);
  const parent = scenarioParent(node.id);
  const toggle = () => toggleExpanded(scenarioFolderKey(node.id));
  // A folder takes what is dropped on most of its row; the top edge of any row puts it before.
  const where = (e: React.DragEvent): "before" | "inside" => {
    const rect = e.currentTarget.getBoundingClientRect();
    return folder && e.clientY - rect.top > rect.height * 0.3 ? "inside" : "before";
  };
  const row = {
    depth,
    drop,
    onDragStart: (e: React.DragEvent) => {
      e.dataTransfer.setData(SCENARIO_DRAG, JSON.stringify({ id: node.id, name: node.name, folder, steps: 0, seq: 0 } satisfies ScenarioNode));
      e.dataTransfer.effectAllowed = "move";
    },
    onDragOver: (e: React.DragEvent) => {
      if (!dragged(e)) return;
      e.preventDefault();
      e.stopPropagation();
      setDrop(where(e));
    },
    onDragLeave: () => setDrop(null),
    onDrop: (e: React.DragEvent) => {
      e.preventDefault();
      e.stopPropagation();
      // From the drop event itself: the state the last dragover set may not have rendered yet.
      const at = where(e);
      setDrop(null);
      const moved = draggedNode(e);
      if (!moved || (moved.id === node.id && !!moved.folder === folder)) return;
      if (at === "inside") void moveScenarioNode(moved, node.id, null);
      else void moveScenarioNode(moved, parent, index);
    },
  };
  if (!folder) return <ScenarioRow node={node} {...row} />;
  const count = flattenScenarios(node.children ?? []).length;
  const entries: MenuEntry[] = [
    ...newEntries(node.id),
    { separator: true },
    { label: "Rename…", icon: <Pencil size={14} />, onSelect: () => void renameScenarioFolder(node) },
    { label: "Move to…", icon: <FolderInput size={14} />, onSelect: () => openModal({ type: "moveScenario", id: node.id, name: node.name, folder: true }) },
    { separator: true },
    { label: "Delete", icon: <Trash2 size={14} />, danger: true, onSelect: () => void deleteScenarioFolder(node) },
  ];
  return (
    <div>
      <ContextMenu entries={entries}>
        <div
          draggable
          {...dragProps(row)}
          onClick={(e) => {
            if (!e.currentTarget.contains(e.target as Node)) return; // portaled menu clicks
            toggle();
          }}
          onKeyDown={(e) => {
            if (e.target !== e.currentTarget) return;
            if (e.key === "Enter") toggle();
            if (e.key === "F2") void renameScenarioFolder(node);
            if (e.key === "Delete" || (e.key === "Backspace" && e.metaKey)) void deleteScenarioFolder(node);
          }}
          tabIndex={0}
          role="treeitem"
          aria-expanded={open}
          title={node.error ?? node.name}
          data-testid={`scenario-folder-${node.id}`}
          style={{ paddingLeft: 6 + depth * 14 }}
          className={rowClass(false, drop)}
        >
          {drop === "before" && <div className="absolute inset-x-1 top-0 h-[2px] rounded bg-accent" />}
          <ChevronRight size={13} className={cx("shrink-0 text-faint transition-transform", open && "rotate-90")} />
          {open ? <FolderOpen size={14} className="shrink-0 text-muted" /> : <Folder size={14} className="shrink-0 text-muted" />}
          <span className="min-w-0 flex-1 truncate">{node.name}</span>
          {node.error ? <TriangleAlert size={13} className="shrink-0 text-warning" /> : count > 0 && <span className="shrink-0 text-[11px] tabular-nums text-faint">{count}</span>}
          <Actions entries={entries} label="Folder actions" />
        </div>
      </ContextMenu>
      {open &&
        (node.children?.length ? (
          node.children.map((c, i) => <ScenarioTreeRow key={`${c.folder ? "f" : "s"}:${c.id}`} node={c} depth={depth + 1} index={i} forceOpen={forceOpen} />)
        ) : (
          <div style={{ paddingLeft: 34 + depth * 14 }} className="py-1 text-[12px] italic text-faint">
            Empty folder
          </div>
        ))}
    </div>
  );
}

interface RowDrag {
  depth: number;
  drop: "before" | "inside" | null;
  onDragStart: (e: React.DragEvent) => void;
  onDragOver: (e: React.DragEvent) => void;
  onDragLeave: () => void;
  onDrop: (e: React.DragEvent) => void;
}

const dragProps = ({ onDragStart, onDragOver, onDragLeave, onDrop }: RowDrag) => ({ onDragStart, onDragOver, onDragLeave, onDrop });

const rowClass = (active: boolean, drop: RowDrag["drop"]) =>
  cx(
    "group relative flex h-[30px] items-center gap-1.5 rounded-lg pr-1.5 text-[12.5px] outline-none focus-visible:ring-1 focus-visible:ring-accent",
    active ? "bg-elev text-fg" : "text-fg/90 hover:bg-hover/70",
    drop === "inside" && "ring-1 ring-accent",
  );

function Actions({ entries, label }: { entries: MenuEntry[]; label: string }) {
  return (
    <Menu
      align="end"
      entries={entries}
      trigger={
        <button
          aria-label={label}
          onClick={(e) => e.stopPropagation()}
          className="flex h-5 w-5 shrink-0 items-center justify-center rounded text-faint opacity-0 hover:bg-panel-2 hover:text-fg focus-visible:opacity-100 group-hover:opacity-100 data-[state=open]:opacity-100"
        >
          <MoreHorizontal size={13} />
        </button>
      }
    />
  );
}

function ScenarioRow({ node, ...drag }: { node: ScenarioNode } & RowDrag) {
  const tabId = useTabs((s) => s.tabs.find((t) => isScenarioTab(t) && t.scenarioId === node.id)?.id ?? null);
  const running = useRunner((s) => !!tabId && s.active === tabId);
  const busy = useRunner((s) => s.active !== null || s.starting !== null);
  const active = useTabs((s) => {
    const t = s.tabs.find((x) => x.id === s.activeId);
    return isScenarioTab(t) && t.scenarioId === node.id;
  });
  const cannotRun = !!node.error || busy;
  const toggle = () => (running && tabId ? void stopRun(tabId) : void runFromList(node));
  const entries: MenuEntry[] = [
    running && tabId
      ? { label: "Stop", icon: <Square size={14} />, onSelect: () => void stopRun(tabId) }
      : { label: "Run", icon: <Play size={14} />, disabled: cannotRun, onSelect: () => void runFromList(node) },
    { separator: true },
    { label: "Rename…", icon: <Pencil size={14} />, onSelect: () => void renameScenario(node) },
    { label: "Duplicate", icon: <Copy size={14} />, onSelect: () => void duplicateScenario(node) },
    { label: "Move to…", icon: <FolderInput size={14} />, onSelect: () => openModal({ type: "moveScenario", id: node.id, name: node.name, folder: false }) },
    { separator: true },
    { label: "Delete", icon: <Trash2 size={14} />, danger: true, onSelect: () => void deleteScenario(node) },
  ];
  const meta = `${node.steps} ${node.steps === 1 ? "step" : "steps"}`;
  return (
    <ContextMenu entries={entries}>
      <div
        draggable
        {...dragProps(drag)}
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
        style={{ paddingLeft: 6 + drag.depth * 14 }}
        className={rowClass(active, drag.drop)}
      >
        {drag.drop === "before" && <div className="absolute inset-x-1 top-0 h-[2px] rounded bg-accent" />}
        <span className="w-[13px] shrink-0" />
        <Workflow size={13} className="shrink-0 text-accent" />
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
        <Actions entries={entries} label="Scenario actions" />
      </div>
    </ContextMenu>
  );
}

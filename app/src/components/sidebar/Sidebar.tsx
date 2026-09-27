import { useMemo, useState } from "react";
import { Activity, BookOpen, Bot, Boxes, Braces, Cable, ChevronRight, Copy, FilePlus2, Folder, FolderInput, FolderOpen, FolderPlus, History, Import, Layers, ListChecks, MessagesSquare, MoreHorizontal, Network, Pencil, Plus, Radio, RefreshCw, Search, Server as ServerIcon, Settings2, Terminal, Trash2, TriangleAlert, Wrench, Zap } from "lucide-react";
import type { TreeNode } from "../../bindings/TreeNode";
import { GRAPHQL_BADGE, methodColor, methodLabel } from "../../lib/http";
import { api, errorMessage } from "../../lib/rpc";
import { confirm, prompt } from "../../store/dialogs";
import { loadTestFolder, loadTestRequest } from "../../store/loadtests";
import {
  isRequestTab,
  type NewRequestType,
  newRequestDraft,
  onItemDeleted,
  onItemMoved,
  onItemRenamed,
  openRequest,
  openRunner,
  REQUEST_KIND_NAMES,
  useTabs,
} from "../../store/tabs";
import { toast } from "../../store/toasts";
import { openModal, type SidebarSection, toggleExpanded, useUi } from "../../store/ui";
import { refreshTree, useWorkspace } from "../../store/workspace";
import { ContextMenu, cx, EmptyState, Menu, type MenuEntry } from "../ui";
import { AgentsPanel } from "../agents/AgentsPanel";
import { DocsList } from "../docs/DocsList";
import { HistoryList } from "./HistoryList";
import { LoadTestsList } from "./LoadTestsList";
import { ServersList } from "./ServersList";
import { ToolsList } from "./ToolsList";
import { mockFolder } from "../servers/MockDialogs";

export const SIDEBAR_SECTIONS: { id: SidebarSection; label: string; icon: React.ReactNode }[] = [
  { id: "collection", label: "Collection", icon: <Layers size={17} /> },
  { id: "loadtests", label: "Load tests", icon: <Activity size={17} /> },
  { id: "servers", label: "Servers", icon: <ServerIcon size={17} /> },
  { id: "tools", label: "Tools", icon: <Wrench size={17} /> },
  { id: "history", label: "History", icon: <History size={17} /> },
  { id: "agents", label: "AI agents", icon: <Bot size={17} /> },
  { id: "docs", label: "Docs", icon: <BookOpen size={17} /> },
];

export function Sidebar() {
  const tab = useUi((s) => s.sidebarTab);
  const label = SIDEBAR_SECTIONS.find((s) => s.id === tab)?.label;
  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex h-7 shrink-0 items-center px-3 text-[11px] font-semibold uppercase tracking-wide text-faint">{label}</div>
      {tab === "collection" ? (
        <CollectionTree />
      ) : tab === "loadtests" ? (
        <LoadTestsList />
      ) : tab === "servers" ? (
        <ServersList />
      ) : tab === "tools" ? (
        <ToolsList />
      ) : tab === "agents" ? (
        <AgentsPanel />
      ) : tab === "docs" ? (
        <DocsList />
      ) : (
        <HistoryList />
      )}
    </div>
  );
}

/** Kinds of request offered in "New …" menus. */
export const NEW_REQUEST_KINDS: { kind: NewRequestType; label: string; icon: React.ReactNode }[] = [
  { kind: "http", label: "HTTP request", icon: <FilePlus2 size={14} /> },
  { kind: "graphql", label: "GraphQL request", icon: <Braces size={14} /> },
  { kind: "grpc", label: "gRPC request", icon: <Boxes size={14} /> },
  { kind: "websocket", label: "WebSocket", icon: <Zap size={14} /> },
  { kind: "sse", label: "Event stream (SSE)", icon: <Radio size={14} /> },
  { kind: "tcp", label: "TCP connection", icon: <Cable size={14} /> },
  { kind: "udp", label: "UDP socket", icon: <Network size={14} /> },
  { kind: "dns", label: "DNS query", icon: <Search size={14} /> },
  { kind: "mqtt", label: "MQTT client", icon: <MessagesSquare size={14} /> },
];

async function createRequestIn(parent: string, kind: NewRequestType = "http") {
  const name = await prompt({ title: REQUEST_KIND_NAMES[kind], value: newRequestDraft(kind).name, confirmLabel: "Create" });
  if (!name) return;
  try {
    const path = await api.createRequest(parent, { ...newRequestDraft(kind), name });
    if (parent) toggleExpanded(parent, true);
    await refreshTree();
    await openRequest(path);
  } catch (e) {
    toast("error", "Could not create request", errorMessage(e));
  }
}

async function createFolderIn(parent: string) {
  const name = await prompt({ title: "New folder", placeholder: "Folder name", confirmLabel: "Create" });
  if (!name) return;
  try {
    const path = await api.createFolder(parent, name);
    if (parent) toggleExpanded(parent, true);
    toggleExpanded(path, true);
    await refreshTree();
  } catch (e) {
    toast("error", "Could not create folder", errorMessage(e));
  }
}

function filterTree(nodes: TreeNode[], q: string): TreeNode[] {
  if (!q) return nodes;
  const out: TreeNode[] = [];
  for (const n of nodes) {
    if (n.kind === "folder") {
      const children = filterTree(n.children, q);
      if (children.length || n.name.toLowerCase().includes(q)) out.push({ ...n, children: children.length ? children : n.children });
    } else if (n.name.toLowerCase().includes(q) || (n.method ?? "").toLowerCase() === q) {
      out.push(n);
    }
  }
  return out;
}

function CollectionTree() {
  const info = useWorkspace((s) => s.info);
  const [query, setQuery] = useState("");
  const [dropRoot, setDropRoot] = useState(false);
  const q = query.trim().toLowerCase();
  const nodes = useMemo(() => filterTree(info?.tree ?? [], q), [info?.tree, q]);
  if (!info) return null;

  const rootMenu: MenuEntry[] = [
    ...NEW_REQUEST_KINDS.map((k) => ({ label: `New ${k.label}`, icon: k.icon, onSelect: () => createRequestIn("", k.kind) })),
    { label: "New folder", icon: <FolderPlus size={14} />, onSelect: () => createFolderIn("") },
    { separator: true },
    { label: "Import…", icon: <Import size={14} />, onSelect: () => openModal({ type: "import" }) },
    { label: "Run…", icon: <ListChecks size={14} />, onSelect: () => openRunner("", info.meta.name) },
    { label: "Mock the collection…", icon: <ServerIcon size={14} />, onSelect: () => void mockFolder("", info.meta.name) },
    { label: "Load test the collection…", icon: <Activity size={14} />, onSelect: () => void loadTestFolder(info.tree, "", info.meta.name) },
  ];

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex items-center gap-1 px-2 py-2">
        <div className="flex h-7 min-w-0 flex-1 items-center gap-1.5 rounded-lg border border-transparent bg-panel-2 px-2 focus-within:border-accent">
          <Search size={12} className="shrink-0 text-faint" />
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Filter"
            aria-label="Filter requests"
            className="min-w-0 flex-1 bg-transparent text-[12.5px] outline-none placeholder:text-faint"
          />
        </div>
        <Menu
          align="end"
          trigger={
            <button aria-label="New" className="flex h-7 w-7 items-center justify-center rounded-lg text-muted hover:bg-hover hover:text-fg">
              <Plus size={15} />
            </button>
          }
          entries={rootMenu}
        />
      </div>
      <ContextMenu entries={rootMenu}>
        <div
          className={cx("min-h-0 flex-1 overflow-auto px-1.5 pb-6", dropRoot && "bg-accent-soft")}
          onDragOver={(e) => {
            if (e.dataTransfer.types.includes("application/x-zv-item")) {
              e.preventDefault();
              setDropRoot(true);
            }
          }}
          onDragLeave={() => setDropRoot(false)}
          onDrop={(e) => {
            setDropRoot(false);
            const path = e.dataTransfer.getData("application/x-zv-item");
            if (path) void moveItem(path, "", null);
          }}
          data-testid="collection-tree"
        >
          {nodes.length === 0 ? (
            q ? (
              <div className="p-4 text-center text-[12.5px] text-muted">No requests match “{query}”.</div>
            ) : (
              <EmptyState title="No requests yet">
                <div className="mt-1 flex flex-col items-center gap-2">
                  <span>Create a request or import from Postman, OpenAPI or cURL.</span>
                  <div className="flex gap-2">
                    <button className="text-accent hover:underline" onClick={() => createRequestIn("")}>
                      New request
                    </button>
                    <span className="text-faint">·</span>
                    <button className="text-accent hover:underline" onClick={() => openModal({ type: "import" })}>
                      Import
                    </button>
                  </div>
                </div>
              </EmptyState>
            )
          ) : (
            nodes.map((n, i) => <TreeRow key={n.path} node={n} depth={0} forceOpen={!!q} index={i} parent="" />)
          )}
        </div>
      </ContextMenu>
    </div>
  );
}

export async function moveItem(path: string, parent: string, index: number | null) {
  const current = path.includes("/") ? path.slice(0, path.lastIndexOf("/")) : "";
  if (current === parent && index === null) return;
  if (parent === path || parent.startsWith(`${path}/`)) return;
  try {
    const newPath = await api.moveItem(path, parent, index);
    if (newPath !== path) onItemMoved(path, newPath);
    if (parent) toggleExpanded(parent, true);
    await refreshTree();
  } catch (e) {
    toast("error", "Could not move", errorMessage(e));
  }
}

function TreeRow({ node, depth, forceOpen, index, parent }: { node: TreeNode; depth: number; forceOpen: boolean; index: number; parent: string }) {
  const expanded = useUi((s) => s.expanded[node.path] ?? false);
  const activePath = useTabs((s) => {
    const t = s.tabs.find((x) => x.id === s.activeId);
    return isRequestTab(t) ? t.path : undefined;
  });
  const [drop, setDrop] = useState<"before" | "inside" | null>(null);
  const isFolder = node.kind === "folder";
  const open = forceOpen || expanded;

  const rename = async () => {
    const name = await prompt({ title: `Rename ${isFolder ? "folder" : "request"}`, value: node.name, confirmLabel: "Rename" });
    if (!name || name === node.name) return;
    try {
      const newPath = await api.renameItem(node.path, name);
      await onItemRenamed(node.path, newPath);
      if (isFolder && expanded) toggleExpanded(newPath, true);
      await refreshTree();
    } catch (e) {
      toast("error", "Could not rename", errorMessage(e));
    }
  };

  const remove = async () => {
    const ok = await confirm({
      title: `Delete ${isFolder ? "folder" : "request"}?`,
      message: `“${node.name}”${isFolder ? " and everything in it" : ""} will be moved to the trash.`,
      confirmLabel: "Delete",
      danger: true,
    });
    if (!ok) return;
    try {
      await api.deleteItem(node.path);
      onItemDeleted(node.path);
      await refreshTree();
    } catch (e) {
      toast("error", "Could not delete", errorMessage(e));
    }
  };

  const duplicate = async () => {
    try {
      const path = await api.duplicateItem(node.path);
      await refreshTree();
      if (!isFolder) await openRequest(path);
    } catch (e) {
      toast("error", "Could not duplicate", errorMessage(e));
    }
  };

  const entries: MenuEntry[] = isFolder
    ? [
        ...NEW_REQUEST_KINDS.map((k) => ({ label: `New ${k.label}`, icon: k.icon, onSelect: () => createRequestIn(node.path, k.kind) })),
        { label: "New folder", icon: <FolderPlus size={14} />, onSelect: () => createFolderIn(node.path) },
        { separator: true },
        { label: "Folder settings…", icon: <Settings2 size={14} />, onSelect: () => openModal({ type: "folderSettings", path: node.path }) },
        { label: "Import into folder…", icon: <Import size={14} />, onSelect: () => openModal({ type: "import", parent: node.path }) },
        { label: "Run…", icon: <ListChecks size={14} />, onSelect: () => openRunner(node.path, node.name) },
        { label: "Mock this folder…", icon: <ServerIcon size={14} />, onSelect: () => void mockFolder(node.path, node.name) },
        ...(node.fromSpec
          ? [{ label: "Update from API spec…", icon: <RefreshCw size={14} />, onSelect: () => openModal({ type: "specUpdate", folder: node.path, name: node.name }) }]
          : []),
        {
          label: "Load test this folder…",
          icon: <Activity size={14} />,
          onSelect: () => void loadTestFolder(useWorkspace.getState().info?.tree ?? [], node.path, node.name),
        },
        { label: "Rename…", icon: <Pencil size={14} />, onSelect: rename },
        { label: "Duplicate", icon: <Copy size={14} />, onSelect: duplicate },
        { label: "Move to…", icon: <FolderInput size={14} />, onSelect: () => openModal({ type: "move", path: node.path, name: node.name }) },
        { separator: true },
        { label: "Delete", icon: <Trash2 size={14} />, danger: true, onSelect: remove },
      ]
    : [
        { label: "Open", onSelect: () => openRequest(node.path) },
        {
          label: "Copy as cURL or code…",
          icon: <Terminal size={14} />,
          onSelect: async () => {
            await openRequest(node.path);
            const t = useTabs.getState().tabs.find((x) => isRequestTab(x) && x.path === node.path);
            if (t) openModal({ type: "export", tabId: t.id });
          },
        },
        ...((node.requestKind ?? "http") === "http" && !node.error
          ? [{ label: "Load test…", icon: <Activity size={14} />, onSelect: () => void loadTestRequest(node.path, node.name) }]
          : []),
        { label: "Rename…", icon: <Pencil size={14} />, onSelect: rename },
        { label: "Duplicate", icon: <Copy size={14} />, onSelect: duplicate },
        { label: "Move to…", icon: <FolderInput size={14} />, onSelect: () => openModal({ type: "move", path: node.path, name: node.name }) },
        { separator: true },
        { label: "Delete", icon: <Trash2 size={14} />, danger: true, onSelect: remove },
      ];

  return (
    <div>
      <ContextMenu entries={entries}>
        <div
          draggable
          onDragStart={(e) => {
            e.dataTransfer.setData("application/x-zv-item", node.path);
            e.dataTransfer.effectAllowed = "move";
          }}
          onDragOver={(e) => {
            if (!e.dataTransfer.types.includes("application/x-zv-item")) return;
            e.preventDefault();
            e.stopPropagation();
            const rect = e.currentTarget.getBoundingClientRect();
            const y = e.clientY - rect.top;
            setDrop(isFolder && y > rect.height * 0.3 ? "inside" : "before");
          }}
          onDragLeave={() => setDrop(null)}
          onDrop={(e) => {
            e.preventDefault();
            e.stopPropagation();
            const path = e.dataTransfer.getData("application/x-zv-item");
            // From the drop event itself: the state set by the last dragover may not have
            // rendered yet when both arrive together (WebKit), and would say "nowhere".
            const rect = e.currentTarget.getBoundingClientRect();
            const where = isFolder && e.clientY - rect.top > rect.height * 0.3 ? "inside" : "before";
            setDrop(null);
            if (!path || path === node.path) return;
            if (where === "inside") void moveItem(path, node.path, null);
            else void moveItem(path, parent, index);
          }}
          onClick={(e) => {
            // The actions menu is portaled, but React still bubbles its clicks here.
            if (!e.currentTarget.contains(e.target as Node)) return;
            if (isFolder) toggleExpanded(node.path);
            else void openRequest(node.path);
          }}
          onKeyDown={(e) => {
            if (e.target !== e.currentTarget) return; // keys in the actions button/menu
            if (e.key === "Enter") isFolder ? toggleExpanded(node.path) : void openRequest(node.path);
            if (e.key === "F2") void rename();
            if (e.key === "Delete" || (e.key === "Backspace" && e.metaKey)) void remove();
          }}
          tabIndex={0}
          role="treeitem"
          aria-expanded={isFolder ? open : undefined}
          aria-selected={activePath === node.path}
          style={{ paddingLeft: 6 + depth * 14 }}
          className={cx(
            "group relative flex h-[28px] items-center gap-1.5 rounded-lg pr-1 text-[12.5px] outline-none",
            activePath === node.path ? "bg-elev text-fg" : "text-fg/90 hover:bg-hover/70",
            drop === "inside" && "ring-1 ring-accent",
            "focus-visible:ring-1 focus-visible:ring-accent",
          )}
        >
          {drop === "before" && <div className="absolute inset-x-1 top-0 h-[2px] rounded bg-accent" />}
          {isFolder ? (
            <>
              <ChevronRight size={13} className={cx("shrink-0 text-faint transition-transform", open && "rotate-90")} />
              {open ? <FolderOpen size={14} className="shrink-0 text-muted" /> : <Folder size={14} className="shrink-0 text-muted" />}
            </>
          ) : (
            <>
              <span className="w-[13px] shrink-0" />
              <span
                className="w-[34px] shrink-0 text-right font-mono text-[10px] font-bold"
                style={{ color: node.graphql ? GRAPHQL_BADGE.color : methodColor(node.method, node.requestKind) }}
              >
                {node.graphql ? GRAPHQL_BADGE.label : methodLabel(node.method, node.requestKind)}
              </span>
            </>
          )}
          <span
            className={cx("min-w-0 flex-1 truncate", node.removedFromSpec && "text-faint line-through")}
            title={node.removedFromSpec ? `${node.name} (no longer in the API spec)` : node.name}
          >
            {node.name}
          </span>
          {node.error && (
            <span title={node.error}>
              <TriangleAlert size={13} className="shrink-0 text-warning" />
            </span>
          )}
          <Menu
            align="end"
            entries={entries}
            trigger={
              <button
                aria-label="Item actions"
                onClick={(e) => e.stopPropagation()}
                className="flex h-5 w-5 shrink-0 items-center justify-center rounded text-faint opacity-0 hover:bg-panel-2 hover:text-fg focus-visible:opacity-100 group-hover:opacity-100 data-[state=open]:opacity-100"
              >
                <MoreHorizontal size={13} />
              </button>
            }
          />
        </div>
      </ContextMenu>
      {isFolder && open && (
        <div>
          {node.children.length === 0 ? (
            <div style={{ paddingLeft: 34 + depth * 14 }} className="py-1 text-[12px] italic text-faint">
              Empty folder
            </div>
          ) : (
            node.children.map((c, i) => <TreeRow key={c.path} node={c} depth={depth + 1} forceOpen={forceOpen} index={i} parent={node.path} />)
          )}
        </div>
      )}
    </div>
  );
}


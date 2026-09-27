// Sidebar: the workspace's servers, which of them run, and quick start/stop.
import { useMemo, useState } from "react";
import { Copy, FileJson, MoreHorizontal, Pencil, Play, Plus, Search, Square, Trash2, TriangleAlert } from "lucide-react";
import type { ServerNode } from "../../bindings/ServerNode";
import { copyText } from "../../lib/platform";
import { api, errorMessage } from "../../lib/rpc";
import { createServer, deleteServer, duplicateServer, renameServer, reorderServers, runningFor, startServer, stopServer, useServers } from "../../store/servers";
import { isServerTab, openServer, useTabs } from "../../store/tabs";
import { toast } from "../../store/toasts";
import { ContextMenu, cx, EmptyState, Menu, type MenuEntry } from "../ui";
import { SERVER_KIND_ORDER, SERVER_KINDS } from "../servers/kinds";
import { MockDialogs, openMockFromOpenApi } from "../servers/MockDialogs";

const newEntries = (): MenuEntry[] => [
  ...SERVER_KIND_ORDER.map((kind) => ({ label: SERVER_KINDS[kind].label, icon: SERVER_KINDS[kind].icon(14), onSelect: () => void createServer(kind) })),
  { separator: true },
  { label: "Mock from OpenAPI…", icon: <FileJson size={14} />, onSelect: openMockFromOpenApi },
];

export function ServersList() {
  const saved = useServers((s) => s.saved);
  const [query, setQuery] = useState("");
  const q = query.trim().toLowerCase();
  const list = useMemo(() => (q ? saved.filter((n) => n.name.toLowerCase().includes(q) || String(n.port).includes(q)) : saved), [saved, q]);
  const [dragId, setDragId] = useState<string | null>(null);

  return (
    <div className="flex min-h-0 flex-1 flex-col" data-testid="servers-list">
      <div className="flex items-center gap-1 px-2 py-2">
        <div className="flex h-7 min-w-0 flex-1 items-center gap-1.5 rounded-lg border border-transparent bg-panel-2 px-2 focus-within:border-accent">
          <Search size={12} className="shrink-0 text-faint" />
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Filter"
            aria-label="Filter servers"
            className="min-w-0 flex-1 bg-transparent text-[12.5px] outline-none placeholder:text-faint"
          />
        </div>
        <Menu
          align="end"
          trigger={
            <button aria-label="New server" className="flex h-7 w-7 items-center justify-center rounded-lg text-muted hover:bg-hover hover:text-fg">
              <Plus size={15} />
            </button>
          }
          entries={newEntries()}
        />
      </div>
      <div className="min-h-0 flex-1 overflow-auto px-1.5 pb-6">
        {list.length === 0 ? (
          q ? (
            <div className="p-4 text-center text-[12.5px] text-muted">No servers match “{query}”.</div>
          ) : (
            <EmptyState title="No servers yet">
              <div className="mt-1 flex flex-col items-center gap-2">
                <span>Mock an API, or listen for TCP, UDP, WebSocket or DNS traffic. Servers are saved in the workspace.</span>
                <Menu
                  trigger={<button className="text-accent hover:underline">New server</button>}
                  entries={newEntries()}
                />
              </div>
            </EmptyState>
          )
        ) : (
          list.map((node) => (
            <ServerRow
              key={node.id}
              node={node}
              dragging={dragId}
              onDragStart={() => setDragId(node.id)}
              onDragEnd={() => setDragId(null)}
              onDropOn={() => {
                if (!dragId || dragId === node.id) return;
                const ids = saved.map((n) => n.id).filter((id) => id !== dragId);
                ids.splice(ids.indexOf(node.id), 0, dragId);
                void reorderServers(ids);
              }}
            />
          ))
        )}
      </div>
      <MockDialogs />
    </div>
  );
}

function ServerRow({
  node,
  dragging,
  onDragStart,
  onDragEnd,
  onDropOn,
}: {
  node: ServerNode;
  dragging: string | null;
  onDragStart: () => void;
  onDragEnd: () => void;
  onDropOn: () => void;
}) {
  const running = useServers((s) => runningFor(node.id, s.running));
  const active = useTabs((s) => {
    const t = s.tabs.find((x) => x.id === s.activeId);
    return isServerTab(t) && t.serverId === node.id;
  });
  const [over, setOver] = useState(false);
  const info = SERVER_KINDS[node.kind];
  const toggle = async () => {
    if (running) return stopServer(running.runId);
    // Start from the open tab's draft when there is one (it may have unsaved edits).
    const tab = useTabs.getState().tabs.find((t) => isServerTab(t) && t.serverId === node.id);
    try {
      const server = isServerTab(tab) ? tab.draft : await api.readServer(node.id);
      await startServer(node.id, server);
    } catch (e) {
      toast("error", `Could not start "${node.name}"`, errorMessage(e));
    }
  };
  const entries: MenuEntry[] = [
    running
      ? { label: "Stop", icon: <Square size={14} />, onSelect: () => void stopServer(running.runId) }
      : { label: "Start", icon: <Play size={14} />, onSelect: () => void toggle() },
    ...(running ? [{ label: "Copy address", icon: <Copy size={14} />, onSelect: () => void copyText(running.url) }] : []),
    { separator: true },
    { label: "Rename…", icon: <Pencil size={14} />, onSelect: () => void renameServer(node) },
    { label: "Duplicate", icon: <Copy size={14} />, onSelect: () => void duplicateServer(node) },
    { separator: true },
    { label: "Delete", icon: <Trash2 size={14} />, danger: true, onSelect: () => void deleteServer(node) },
  ];
  return (
    <ContextMenu entries={entries}>
      <div
        draggable
        onDragStart={(e) => {
          e.dataTransfer.setData("application/x-zv-server", node.id);
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
          void openServer(node.id);
        }}
        onKeyDown={(e) => {
          if (e.target !== e.currentTarget) return;
          if (e.key === "Enter") void openServer(node.id);
          if (e.key === "F2") void renameServer(node);
          if (e.key === "Delete" || (e.key === "Backspace" && e.metaKey)) void deleteServer(node);
        }}
        tabIndex={0}
        role="treeitem"
        aria-selected={active}
        data-testid={`server-row-${node.id}`}
        className={cx(
          "group relative flex h-[30px] items-center gap-2 rounded-lg px-1.5 text-[12.5px] outline-none focus-visible:ring-1 focus-visible:ring-accent",
          active ? "bg-elev text-fg" : "text-fg/90 hover:bg-hover/70",
        )}
      >
        {over && <div className="absolute inset-x-1 top-0 h-[2px] rounded bg-accent" />}
        <span className="w-[38px] shrink-0 text-right font-mono text-[10px] font-bold" style={{ color: info.color }}>
          {info.short}
        </span>
        <span className="min-w-0 flex-1 truncate" title={node.name}>
          {node.name}
        </span>
        {node.error ? (
          <span title={node.error}>
            <TriangleAlert size={13} className="shrink-0 text-warning" />
          </span>
        ) : (
          <span className={cx("shrink-0 font-mono text-[11px]", running ? "text-success" : "text-faint")}>:{running?.port ?? node.port}</span>
        )}
        <button
          aria-label={running ? `Stop ${node.name}` : `Start ${node.name}`}
          title={running ? "Stop" : "Start"}
          disabled={!!node.error}
          onClick={(e) => {
            e.stopPropagation();
            void toggle();
          }}
          className={cx(
            "flex h-5 w-5 shrink-0 items-center justify-center rounded hover:bg-panel-2",
            running ? "text-success" : "text-faint opacity-0 hover:text-fg focus-visible:opacity-100 group-hover:opacity-100",
          )}
        >
          {running ? (
            <>
              <span className="h-2 w-2 rounded-full bg-success group-hover:hidden" />
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
              aria-label="Server actions"
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

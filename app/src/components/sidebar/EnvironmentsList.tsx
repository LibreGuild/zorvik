// Sidebar: the workspace's variables, globals and environments. A row opens in a tab;
// the mark before an environment makes it the active one.
import { useMemo, useState } from "react";
import { Check, Copy, Globe, Globe2, Layers, MoreHorizontal, Plus, Search, Trash2 } from "lucide-react";
import type { EnvironmentEntry } from "../../bindings/EnvironmentEntry";
import { deleteEnvironment, newEnvironment, toggleActiveEnvironment } from "../../store/environments";
import { isDirty, isVariablesTab, openVariables, useTabs, type VariablesTab } from "../../store/tabs";
import { useWorkspace } from "../../store/workspace";
import { ContextMenu, cx, Menu, type MenuEntry } from "../ui";

/** The open tab of a scope (and environment), if there is one. */
const useTabOf = (scope: VariablesTab["scope"], envId?: string) =>
  useTabs((s) => s.tabs.find((t): t is VariablesTab => isVariablesTab(t) && t.scope === scope && t.envId === envId));

export function EnvironmentsList() {
  const info = useWorkspace((s) => s.info);
  const [query, setQuery] = useState("");
  const q = query.trim().toLowerCase();
  const envs = info?.environments;
  const list = useMemo(() => (q ? (envs ?? []).filter((e) => e.environment.name.toLowerCase().includes(q)) : (envs ?? [])), [envs, q]);
  if (!info) return null;
  return (
    <div className="flex min-h-0 flex-1 flex-col" data-testid="environments-list">
      <div className="flex items-center gap-1 px-2 py-2">
        <div className="flex h-7 min-w-0 flex-1 items-center gap-1.5 rounded-lg border border-transparent bg-panel-2 px-2 focus-within:border-accent">
          <Search size={12} className="shrink-0 text-faint" />
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Filter"
            aria-label="Filter environments"
            className="min-w-0 flex-1 bg-transparent text-[12.5px] outline-none placeholder:text-faint"
          />
        </div>
        <button aria-label="New environment" title="New environment" onClick={() => void newEnvironment()} className="flex h-7 w-7 items-center justify-center rounded-lg text-muted hover:bg-hover hover:text-fg">
          <Plus size={15} />
        </button>
      </div>
      <div className="min-h-0 flex-1 overflow-auto px-1.5 pb-6" role="tree" aria-label="Environments and variables">
        {!q && (
          <>
            <ScopeRow scope="workspace" label="Workspace variables" icon={<Layers size={14} />} count={(info.meta.variables ?? []).length} />
            <ScopeRow scope="globals" label="Globals" icon={<Globe size={14} />} />
            <div className="mb-1 mt-3 px-2 text-[11px] font-semibold uppercase tracking-wide text-faint">Environments</div>
          </>
        )}
        {list.map((e) => (
          <EnvironmentRow key={e.id} env={e} active={info.activeEnvironment === e.id} />
        ))}
        {list.length === 0 &&
          (q ? (
            <div className="p-4 text-center text-[12.5px] text-muted">No environments match “{query}”.</div>
          ) : (
            <div className="flex flex-col items-start gap-1.5 px-2 py-1 text-[12.5px] text-muted">
              <span>No environments yet. Keep what differs between Local, Staging and Production in one each, and switch in the title bar.</span>
              <button className="text-accent hover:underline" onClick={() => void newEnvironment()}>
                New environment
              </button>
            </div>
          ))}
      </div>
    </div>
  );
}

const rowClass = (selected: boolean) =>
  cx(
    "group relative flex h-[30px] items-center gap-2 rounded-lg px-1.5 text-[12.5px] outline-none focus-visible:ring-1 focus-visible:ring-accent",
    selected ? "bg-elev text-fg" : "text-fg/90 hover:bg-hover/70",
  );

/** A dot on a row whose tab has changes that aren't saved. */
const Unsaved = () => <span className="h-1.5 w-1.5 shrink-0 rounded-full bg-accent" aria-label="Unsaved changes" />;

function ScopeRow({ scope, label, icon, count }: { scope: "workspace" | "globals"; label: string; icon: React.ReactNode; count?: number }) {
  const tab = useTabOf(scope);
  const selected = useTabs((s) => !!tab && s.activeId === tab.id);
  return (
    <div
      role="treeitem"
      aria-selected={selected}
      tabIndex={0}
      onClick={() => openVariables(scope)}
      onKeyDown={(e) => e.key === "Enter" && openVariables(scope)}
      className={rowClass(selected)}
      data-testid={`environments-${scope}`}
    >
      <span className="flex w-5 shrink-0 justify-center text-muted">{icon}</span>
      <span className="min-w-0 flex-1 truncate">{label}</span>
      {tab && isDirty(tab) && <Unsaved />}
      {count !== undefined && <span className="shrink-0 pr-1 text-[11px] tabular-nums text-faint">{count}</span>}
    </div>
  );
}

function EnvironmentRow({ env, active }: { env: EnvironmentEntry; active: boolean }) {
  const tab = useTabOf("environment", env.id);
  const selected = useTabs((s) => !!tab && s.activeId === tab.id);
  const name = tab?.draft.name || env.environment.name;
  const open = () => openVariables("environment", env.id);
  const entries: MenuEntry[] = [
    { label: "Open", icon: <Globe2 size={14} />, onSelect: open },
    { label: active ? "Use no environment" : "Set active", icon: <Check size={14} />, onSelect: () => void toggleActiveEnvironment(env.id) },
    { separator: true },
    { label: "Duplicate", icon: <Copy size={14} />, onSelect: () => void newEnvironment(env) },
    { separator: true },
    { label: "Delete", icon: <Trash2 size={14} />, danger: true, onSelect: () => void deleteEnvironment(env) },
  ];
  return (
    <ContextMenu entries={entries}>
      <div
        role="treeitem"
        aria-selected={selected}
        tabIndex={0}
        onClick={(e) => {
          if (!e.currentTarget.contains(e.target as Node)) return; // portaled menu clicks
          open();
        }}
        onKeyDown={(e) => {
          if (e.target !== e.currentTarget) return;
          if (e.key === "Enter") open();
          if (e.key === "Delete" || (e.key === "Backspace" && e.metaKey)) void deleteEnvironment(env);
        }}
        title={`${name}${active ? " (active)" : ""}`}
        className={rowClass(selected)}
        data-testid={`environment-row-${env.id}`}
      >
        <button
          role="checkbox"
          aria-checked={active}
          aria-label={active ? `${name} is active. Use no environment` : `Set ${name} active`}
          title={active ? "Active. Click to use no environment" : "Set active"}
          onClick={(e) => {
            e.stopPropagation();
            void toggleActiveEnvironment(env.id);
          }}
          className={cx(
            "flex h-5 w-5 shrink-0 items-center justify-center rounded-full",
            active ? "bg-accent-soft text-accent" : "text-faint opacity-0 hover:bg-panel-2 hover:text-fg focus-visible:opacity-100 group-hover:opacity-100",
          )}
        >
          <Check size={12} strokeWidth={3} />
        </button>
        <span className={cx("min-w-0 flex-1 truncate", active && "font-medium")}>{name}</span>
        {tab && isDirty(tab) && <Unsaved />}
        <span className="shrink-0 text-[11px] tabular-nums text-faint">{env.environment.variables.length}</span>
        <Menu
          align="end"
          entries={entries}
          trigger={
            <button
              aria-label="Environment actions"
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

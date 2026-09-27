import { memo } from "react";
import { ListChecks, Plus, X } from "lucide-react";
import { GRAPHQL_BADGE, methodColor, methodLabel } from "../../lib/http";
import { modKey } from "../../lib/platform";
import { runFor, useLoadTests } from "../../store/loadtests";
import { useRunner } from "../../store/runner";
import { runningFor, useServers } from "../../store/servers";
import {
  activate,
  type AnyTab,
  closeOtherTabs,
  closeTab,
  isDirty,
  isLoadTestTab,
  isRequestTab,
  isRunnerTab,
  isServerTab,
  newRequest,
  tabTitle,
  useTabs,
} from "../../store/tabs";
import { ModelBadge } from "../loadtests/parts";
import { SERVER_KINDS } from "../servers/kinds";
import { NEW_REQUEST_KINDS } from "../sidebar/Sidebar";
import { toolInfo } from "../tools/registry";
import { ContextMenu, cx, Menu } from "../ui";

export function TabBar() {
  const tabs = useTabs((s) => s.tabs);
  const activeId = useTabs((s) => s.activeId);
  return (
    <div className="flex h-11 shrink-0 items-center gap-1 px-2 pt-1" role="tablist" aria-label="Open tabs">
      <div className="flex min-w-0 flex-1 items-center gap-1 overflow-x-auto [scrollbar-width:none]">
        {tabs.map((t) => (
          <TabItem key={t.id} tab={t} active={t.id === activeId} />
        ))}
      </div>
      <Menu
        align="end"
        trigger={
          <button aria-label="New tab" className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg text-muted hover:bg-hover hover:text-fg">
            <Plus size={15} />
          </button>
        }
        entries={NEW_REQUEST_KINDS.map((k, i) => ({
          label: k.label,
          icon: k.icon,
          shortcut: i === 0 ? `${modKey}+N` : undefined,
          onSelect: () => newRequest(k.kind),
        }))}
      />
    </div>
  );
}

// Memoized: the tab list changes with every keystroke and stream batch; a big server draft is costly to compare.
const TabItem = memo(function TabItem({ tab: t, active }: { tab: AnyTab; active: boolean }) {
  const running = useServers((s) => (isServerTab(t) ? !!runningFor(t.serverId, s.running) : false));
  const loadRunning = useLoadTests((s) => (isLoadTestTab(t) ? !!runFor(t.testId, s.active) : false));
  const collectionRunning = useRunner((s) => isRunnerTab(t) && s.active === t.id);
  const dirty = isDirty(t);
  let badge: React.ReactNode;
  let title: string;
  let live = running || loadRunning || collectionRunning;
  if (isRequestTab(t)) {
    const kind = t.draft.kind ?? "http";
    const graphql = kind === "http" && t.draft.body?.type === "graphql";
    badge = (
      <span className="shrink-0 font-mono text-[10px] font-bold" style={{ color: graphql ? GRAPHQL_BADGE.color : methodColor(t.draft.method, kind) }}>
        {graphql ? GRAPHQL_BADGE.label : methodLabel(t.draft.method, kind)}
      </span>
    );
    title = t.path ?? "Unsaved request";
    live = t.stream.status === "open" || t.response.status === "loading";
  } else if (isServerTab(t)) {
    const info = SERVER_KINDS[t.draft.kind];
    badge = (
      <span className="flex shrink-0 items-center gap-1 font-mono text-[10px] font-bold" style={{ color: info.color }}>
        {info.icon(11)}
        {info.short}
      </span>
    );
    title = `Server · servers/${t.serverId}.yaml`;
  } else if (isLoadTestTab(t)) {
    badge = <ModelBadge model={t.draft.model} icon={11} />;
    title = `Load test · loadtests/${t.testId}.yaml`;
  } else if (isRunnerTab(t)) {
    badge = <ListChecks size={12} className="shrink-0 text-accent" />;
    title = `Run · ${t.folder ? `requests/${t.folder}` : "the whole collection"}`;
  } else {
    const tool = toolInfo(t.tool);
    badge = <span className="shrink-0 text-muted">{tool?.icon(12)}</span>;
    title = tool?.label ?? t.tool;
  }
  const name = isRequestTab(t) || isServerTab(t) || isLoadTestTab(t) || isRunnerTab(t) ? tabTitle(t) : (toolInfo(t.tool)?.label ?? t.tool);
  return (
    <ContextMenu
      entries={[
        { label: "Close", shortcut: `${modKey}+W`, onSelect: () => void closeTab(t.id) },
        { label: "Close others", onSelect: () => void closeOtherTabs(t.id) },
      ]}
    >
      <div
        role="tab"
        aria-selected={active}
        onClick={() => activate(t.id)}
        onAuxClick={(e) => e.button === 1 && void closeTab(t.id)}
        title={title}
        className={cx(
          "group relative flex h-8 min-w-[120px] max-w-[220px] shrink-0 items-center gap-2 rounded-lg pl-3 pr-1.5 text-[12.5px] transition-colors",
          active ? "bg-elev text-fg shadow-sm" : "text-muted hover:bg-hover/70 hover:text-fg",
        )}
      >
        {badge}
        <span className={cx("min-w-0 flex-1 truncate", isRequestTab(t) && !t.path && "italic")}>{name}</span>
        {live && <span className={cx("h-1.5 w-1.5 shrink-0 rounded-full", loadRunning || collectionRunning ? "animate-pulse bg-accent" : "bg-success")} />}
        <button
          aria-label="Close tab"
          onClick={(e) => {
            e.stopPropagation();
            void closeTab(t.id);
          }}
          className="relative flex h-5 w-5 shrink-0 items-center justify-center rounded text-faint hover:bg-hover hover:text-fg"
        >
          {dirty && <span className="absolute h-2 w-2 rounded-full bg-muted group-hover:hidden" />}
          <X size={13} className={cx(dirty && "invisible group-hover:visible")} />
        </button>
      </div>
    </ContextMenu>
  );
});

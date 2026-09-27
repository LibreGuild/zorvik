// Left rail: switches the sidebar between collection, load tests, servers, tools and history.
import { useAgents } from "../../store/agents";
import { useLoadTests } from "../../store/loadtests";
import { useServers } from "../../store/servers";
import { useUi } from "../../store/ui";
import { SIDEBAR_SECTIONS } from "../sidebar/Sidebar";
import { cx, Tooltip } from "../ui";

export function ActivityRail() {
  const section = useUi((s) => s.sidebarTab);
  const running = useServers((s) => s.running.length);
  const loadRunning = useLoadTests((s) => !!s.active);
  const agents = useAgents((s) => s.sessions.length);
  return (
    <nav aria-label="Sidebar sections" className="flex w-11 shrink-0 flex-col items-center gap-1 pt-1">
      {SIDEBAR_SECTIONS.map((s) => (
        <Tooltip key={s.id} content={s.label} side="right">
          <button
            aria-label={s.label}
            aria-pressed={section === s.id}
            onClick={() => useUi.setState({ sidebarTab: s.id })}
            data-testid={`rail-${s.id}`}
            className={cx(
              "relative flex h-9 w-9 items-center justify-center rounded-xl transition-colors",
              section === s.id ? "bg-elev text-fg shadow-sm" : "text-muted hover:bg-hover hover:text-fg",
            )}
          >
            {s.icon}
            {s.id === "loadtests" && loadRunning && (
              <span className="absolute right-1.5 top-1.5 flex h-2.5 w-2.5" data-testid="rail-loadtests-running" aria-label="A load test is running">
                <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-accent opacity-60" />
                <span className="relative inline-flex h-2.5 w-2.5 rounded-full border-2 border-panel bg-accent" />
              </span>
            )}
            {s.id === "agents" && agents > 0 && (
              <span className="absolute right-1.5 top-1.5 h-2.5 w-2.5 rounded-full border-2 border-panel bg-success" data-testid="rail-agents-connected" aria-label="An AI agent is connected" />
            )}
            {s.id === "servers" && running > 0 && (
              <span className="absolute right-1 top-1 flex h-3.5 min-w-3.5 items-center justify-center rounded-full bg-success px-0.5 text-[9px] font-bold leading-none text-white">
                {running}
              </span>
            )}
          </button>
        </Tooltip>
      ))}
    </nav>
  );
}

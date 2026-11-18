// Left rail: switches the sidebar between its sections (in groups, a hairline between them);
// at its foot, the console.
import { Fragment } from "react";
import { Terminal } from "lucide-react";
import { useAgents } from "../../store/agents";
import { toggleConsole, useConsole } from "../../store/console";
import { modKey } from "../../lib/platform";
import { useLoadTests } from "../../store/loadtests";
import { useRunner } from "../../store/runner";
import { useServers } from "../../store/servers";
import { isScenarioTab, useTabs } from "../../store/tabs";
import { useUi } from "../../store/ui";
import { SIDEBAR_SECTIONS } from "../sidebar/Sidebar";
import { cx, Tooltip } from "../ui";

export function ActivityRail() {
  const section = useUi((s) => s.sidebarTab);
  const running = useServers((s) => s.running.length);
  const loadRunning = useLoadTests((s) => !!s.active);
  const runTab = useRunner((s) => s.active);
  const scenarioRunning = useTabs((s) => !!runTab && isScenarioTab(s.tabs.find((t) => t.id === runTab)));
  const agents = useAgents((s) => s.sessions.length);
  const consoleOpen = useUi((s) => s.consoleOpen);
  const unseen = useConsole((s) => s.unseen);
  return (
    <nav aria-label="Sidebar sections" className="flex w-11 shrink-0 flex-col items-center gap-1 pt-1">
      {SIDEBAR_SECTIONS.map((s) => (
        <Fragment key={s.id}>
        {s.group && <span aria-hidden className="my-0.5 h-px w-5 shrink-0 bg-line" />}
        <Tooltip content={s.label} side="right">
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
            {s.id === "scenarios" && scenarioRunning && (
              <span className="absolute right-1.5 top-1.5 flex h-2.5 w-2.5" data-testid="rail-scenarios-running" aria-label="A scenario is running">
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
        </Fragment>
      ))}
      <div className="mt-auto pb-1">
        <Tooltip content={`Console (${modKey}+J)`} side="right">
          <button
            aria-label="Console"
            aria-pressed={consoleOpen}
            onClick={toggleConsole}
            data-testid="rail-console"
            className={cx(
              "relative flex h-9 w-9 items-center justify-center rounded-xl transition-colors",
              consoleOpen ? "bg-elev text-fg shadow-sm" : "text-muted hover:bg-hover hover:text-fg",
            )}
          >
            <Terminal size={17} />
            {unseen > 0 && (
              <span
                className="absolute right-1 top-1 flex h-3.5 min-w-3.5 items-center justify-center rounded-full bg-danger px-0.5 text-[9px] font-bold leading-none text-white"
                data-testid="rail-console-errors"
                aria-label={`${unseen} failed since the console was open`}
              >
                {unseen > 99 ? "99+" : unseen}
              </span>
            )}
          </button>
        </Tooltip>
      </div>
    </nav>
  );
}

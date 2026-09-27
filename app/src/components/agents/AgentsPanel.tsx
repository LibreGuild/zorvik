// Sidebar: the AI agents connected and everything they did (click an entry to open what it touched).
import { Ban, Bot, CheckCircle2, CircleX, Loader2, Unplug } from "lucide-react";
import type { AgentActivity } from "../../bindings/AgentActivity";
import { formatMs } from "../../lib/format";
import { disconnectAgent, openTarget, useAgents } from "../../store/agents";
import { useSettings } from "../../store/settings";
import { openModal } from "../../store/ui";
import { Button, cx, EmptyState, IconButton, Tooltip } from "../ui";
import { AgentSetup } from "./AgentSetup";

function timeOf(ms: number) {
  return new Date(ms).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit", second: "2-digit" });
}

const STATUS: Record<AgentActivity["status"], { icon: React.ReactNode; label: string }> = {
  running: { icon: <Loader2 size={13} className="animate-spin text-accent" />, label: "Running" },
  done: { icon: <CheckCircle2 size={13} className="text-success" />, label: "Done" },
  failed: { icon: <CircleX size={13} className="text-danger" />, label: "Failed" },
  denied: { icon: <Ban size={13} className="text-warning" />, label: "Not allowed" },
};

export function AgentsPanel() {
  const sessions = useAgents((s) => s.sessions);
  const activity = useAgents((s) => s.activity);
  const enabled = useSettings((s) => s.settings?.agents.enabled ?? false);
  const entries = [...activity].reverse();
  return (
    <div className="flex min-h-0 flex-1 flex-col" data-testid="agents-panel">
      {sessions.length > 0 && (
        <div className="flex flex-col gap-0.5 border-b border-line/60 px-2 pb-2">
          {sessions.map((s) => (
            <div key={s.id} className="flex h-8 items-center gap-2 rounded-lg px-2 text-[12.5px]" data-testid="agent-session">
              <span className="h-1.5 w-1.5 shrink-0 animate-pulse rounded-full bg-success" />
              <span className="min-w-0 flex-1 truncate font-medium text-fg">{s.client}</span>
              <span className="text-[11px] text-faint">since {timeOf(s.connectedAt)}</span>
              <IconButton label={`Disconnect ${s.client}`} onClick={() => void disconnectAgent(s.id)}>
                <Unplug size={13} />
              </IconButton>
            </div>
          ))}
        </div>
      )}
      <div className="min-h-0 flex-1 overflow-auto px-2 py-1">
        {entries.length === 0 ? (
          <div className="flex flex-col gap-3 px-1 py-2">
            <EmptyState icon={<Bot size={22} />} title="No AI agent activity yet">
              {enabled ? "Connect an agent below; what it does shows up here." : "Agents ask to be allowed the first time they act."}
            </EmptyState>
            <AgentSetup compact />
            <Button variant="ghost" onClick={() => openModal({ type: "settings" })}>
              AI agent settings…
            </Button>
          </div>
        ) : (
          <ul className="flex flex-col gap-0.5">
            {entries.map((a) => (
              <li key={a.id}>
                <button
                  disabled={!a.target}
                  onClick={() => a.target && void openTarget(a.target, true)}
                  className={cx("flex w-full items-start gap-2 rounded-lg px-2 py-1.5 text-left", a.target ? "hover:bg-hover" : "cursor-default")}
                  data-testid="agent-activity"
                >
                  <Tooltip content={STATUS[a.status].label} side="right">
                    <span className="mt-0.5 shrink-0">{STATUS[a.status].icon}</span>
                  </Tooltip>
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-[12.5px] text-fg">{a.title}</span>
                    {a.detail && <span className="line-clamp-2 block break-words text-[11.5px] text-muted">{a.detail}</span>}
                    <span className="block text-[10.5px] text-faint">
                      {a.client} · {timeOf(a.startedAt)}
                      {a.durationMs != null && ` · ${formatMs(a.durationMs)}`}
                    </span>
                  </span>
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}

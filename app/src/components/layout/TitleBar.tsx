// Custom title bar: replaces the OS title bar. macOS keeps its traffic lights
// (drawn by the OS over our bar, on the left); Windows/Linux get our own
// window controls on the right. Empty areas drag the window.
import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Bot, Check, ChevronDown, Cookie, FolderOpen, Keyboard, Layers, LogOut, Minus, Monitor, Moon, Settings, SlidersHorizontal, Square, Sun, X } from "lucide-react";
import { DropdownMenu } from "radix-ui";
import { isMac, pickFolder } from "../../lib/platform";
import { errorMessage, isTauri } from "../../lib/rpc";
import { disconnectAgent, useAgents } from "../../store/agents";
import { stopLoadRun, useLoadTests } from "../../store/loadtests";
import { saveSettings, useSettings } from "../../store/settings";
import { stopAllServers, stopServer, useServers } from "../../store/servers";
import { openLoadTest, openServer, resetTabs, restoreTabs } from "../../store/tabs";
import { toast } from "../../store/toasts";
import { openModal, useUi } from "../../store/ui";
import { closeWorkspace, openOrInitWorkspace, setActiveEnvironment, useWorkspace } from "../../store/workspace";
import { formatDuration } from "../loadtests/model";
import { useTicker } from "../loadtests/parts";
import { SERVER_KINDS } from "../servers/kinds";
import { cx, IconButton, Menu, Tooltip } from "../ui";

const drag = { "data-tauri-drag-region": true } as Record<string, unknown>;

export function TitleBar() {
  const info = useWorkspace((s) => s.info);
  const customControls = isTauri && !isMac;
  return (
    <div {...drag} className="flex h-11 shrink-0 items-center gap-2 bg-panel select-none">
      <div {...drag} className={cx("flex min-w-0 items-center gap-1.5", isTauri && isMac ? "pl-[84px]" : "pl-3")}>
        <img src="/icon.png" alt="" className="h-5 w-5 shrink-0" draggable={false} />
        {info ? <WorkspaceMenu /> : <span className="px-2 text-[13px] font-semibold text-fg">Zorvik</span>}
      </div>
      <div {...drag} className="h-full min-w-4 flex-1" />
      <div className={cx("flex shrink-0 items-center gap-1", !customControls && "pr-3")}>
        {info && <RightTools />}
        {customControls && <WindowControls />}
      </div>
    </div>
  );
}

function WorkspaceMenu() {
  const info = useWorkspace((s) => s.info)!;
  const recent = useWorkspace((s) => s.recent);
  const switchTo = async (path: string) => {
    resetTabs();
    try {
      await openOrInitWorkspace(path);
    } catch (e) {
      toast("error", "Could not open workspace", errorMessage(e));
    }
    // The new workspace, or the previous one again when opening was cancelled or failed.
    const current = useWorkspace.getState().info?.path;
    if (current) await restoreTabs(current);
  };
  return (
    <Menu
      trigger={
        <button
          className="flex h-7 min-w-0 max-w-[280px] items-center gap-1.5 rounded-lg px-2 text-[13px] font-semibold text-fg hover:bg-hover"
          data-testid="workspace-menu"
          title={info.path}
        >
          <span className="truncate">{info.meta.name}</span>
          <ChevronDown size={13} className="shrink-0 text-faint" />
        </button>
      }
      entries={[
        { label: "Workspace settings…", icon: <SlidersHorizontal size={14} />, onSelect: () => openModal({ type: "workspaceSettings" }) },
        {
          label: "Open workspace…",
          icon: <FolderOpen size={14} />,
          onSelect: async () => {
            const path = await pickFolder("Open workspace folder");
            if (path) await switchTo(path);
          },
        },
        ...recent
          .filter((r) => r.path !== info.path)
          .slice(0, 6)
          .map((r) => ({ label: r.name, icon: <Layers size={14} />, onSelect: () => void switchTo(r.path) })),
        { separator: true as const },
        {
          label: "Close workspace",
          icon: <LogOut size={14} />,
          onSelect: async () => {
            resetTabs();
            await closeWorkspace();
          },
        },
      ]}
    />
  );
}

function RightTools() {
  const settings = useSettings((s) => s.settings);
  const theme = settings?.theme ?? "system";
  const cycleTheme = () => {
    if (!settings) return;
    const next = theme === "system" ? "dark" : theme === "dark" ? "light" : "system";
    void saveSettings({ ...settings, theme: next });
  };
  return (
    <>
      <RunningAgents />
      <RunningLoadTest />
      <RunningServers />
      <EnvironmentPicker />
      <span className="mx-1 h-4 w-px bg-line" />
      <IconButton label="Cookies" onClick={() => openModal({ type: "cookies" })}>
        <Cookie size={15} />
      </IconButton>
      <IconButton label={`Theme: ${theme === "system" ? "match system" : theme}`} onClick={cycleTheme}>
        {theme === "dark" ? <Moon size={15} /> : theme === "light" ? <Sun size={15} /> : <Monitor size={15} />}
      </IconButton>
      <IconButton label="Keyboard shortcuts" onClick={() => openModal({ type: "shortcuts" })}>
        <Keyboard size={15} />
      </IconButton>
      <IconButton label="Settings" onClick={() => openModal({ type: "settings" })}>
        <Settings size={15} />
      </IconButton>
    </>
  );
}

/** "● Claude Code" while an AI agent is connected: click shows its activity, the square disconnects. */
function RunningAgents() {
  const sessions = useAgents((s) => s.sessions);
  const waiting = useAgents((s) => s.pending.length);
  if (!sessions.length) return null;
  const label = sessions.length === 1 ? sessions[0].client : `${sessions.length} agents`;
  return (
    <div className="flex h-7 items-center rounded-lg bg-accent-soft text-accent" data-testid="running-agents">
      <Tooltip content={waiting ? "Waiting for your answer" : "Show what the agent does"}>
        <button
          onClick={() => useUi.setState({ sidebarTab: "agents" })}
          className="flex h-7 max-w-[200px] items-center gap-1.5 rounded-l-lg pl-2.5 pr-1.5 text-[12px] font-medium outline-none hover:bg-accent/10 focus-visible:ring-1 focus-visible:ring-accent"
        >
          <Bot size={13} className="shrink-0" />
          <span className="truncate">{label}</span>
          {waiting > 0 && <span className="rounded bg-warning px-1 text-[10px] font-semibold leading-4 text-white">{waiting}</span>}
        </button>
      </Tooltip>
      <Tooltip content={sessions.length === 1 ? `Disconnect ${label}` : "Disconnect every agent"}>
        <button
          aria-label="Disconnect agents"
          onClick={() => void disconnectAgent()}
          className="flex h-7 w-7 items-center justify-center rounded-r-lg outline-none hover:bg-accent/10 focus-visible:ring-1 focus-visible:ring-accent"
          data-testid="running-agents-disconnect"
        >
          <Square size={11} />
        </button>
      </Tooltip>
    </div>
  );
}

/** "● Load test 42 s" while a load test runs: click opens it, the square stops it. */
function RunningLoadTest() {
  const name = useLoadTests((s) => s.active?.name ?? null);
  const testId = useLoadTests((s) => s.active?.testId ?? null);
  const workspace = useLoadTests((s) => s.active?.workspacePath ?? null);
  const stopping = useLoadTests((s) => !!s.active?.stopping);
  // Whole seconds only: the pill re-renders once a second, not with every snapshot.
  const fromSnapshot = useLoadTests((s) => (s.active?.snapshot ? Math.floor(s.active.snapshot.elapsedMs / 1000) : null));
  const startedAt = useLoadTests((s) => s.active?.startedAt ?? 0);
  const wsPath = useWorkspace((s) => s.info?.path);
  useTicker(name !== null && fromSnapshot === null);
  if (name === null || testId === null) return null;
  const secs = fromSnapshot ?? Math.max(0, Math.floor((Date.now() - startedAt) / 1000));
  const here = workspace === wsPath;
  return (
    <div className="flex h-7 items-center rounded-lg bg-accent-soft text-accent" data-testid="running-load-test">
      <Tooltip content={here ? `Open “${name}”` : `“${name}” runs in another workspace`}>
        <button
          onClick={() => {
            if (!here) return;
            useUi.setState({ sidebarTab: "loadtests" });
            void openLoadTest(testId);
          }}
          className="flex h-7 items-center gap-1.5 rounded-l-lg pl-2.5 pr-1.5 text-[12px] font-medium outline-none hover:bg-accent/10 focus-visible:ring-1 focus-visible:ring-accent"
        >
          <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-accent" />
          {stopping ? "Stopping" : "Load test"} <span className="tabular-nums">{formatDuration(secs)}</span>
        </button>
      </Tooltip>
      <Tooltip content="Stop the load test">
        <button
          aria-label="Stop the load test"
          disabled={stopping}
          onClick={() => void stopLoadRun()}
          className="flex h-7 w-7 items-center justify-center rounded-r-lg outline-none hover:bg-accent/10 focus-visible:ring-1 focus-visible:ring-accent disabled:opacity-50"
          data-testid="running-load-test-stop"
        >
          <Square size={11} />
        </button>
      </Tooltip>
    </div>
  );
}

/** "● 2 running": every running server (any workspace), with open/stop. Hidden when none run. */
function RunningServers() {
  const running = useServers((s) => s.running);
  const wsPath = useWorkspace((s) => s.info?.path);
  if (!running.length) return null;
  return (
    <DropdownMenu.Root modal={false}>
      <DropdownMenu.Trigger asChild>
        <button
          data-testid="running-servers"
          className="flex h-7 items-center gap-1.5 rounded-lg bg-success/12 px-2.5 text-[12px] font-medium text-success outline-none hover:bg-success/20"
        >
          <span className="h-1.5 w-1.5 rounded-full bg-success" />
          {running.length} running
          <ChevronDown size={12} />
        </button>
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content align="end" sideOffset={6} collisionPadding={8} className="zv-pop z-[90] w-80 rounded-xl border border-line bg-elev p-1.5 shadow-pop max-h-[var(--radix-dropdown-menu-content-available-height)] overflow-y-auto overscroll-contain">
          <div className="px-2 pb-1 pt-0.5 text-[11px] font-semibold uppercase tracking-wide text-faint">Running servers</div>
          {running.map((r) => {
            const here = r.workspacePath === wsPath;
            const info = SERVER_KINDS[r.kind];
            // Menu items, not plain buttons: a menu only moves focus between its items (arrow keys).
            return (
              <div key={r.runId} className="flex items-center gap-1">
                <DropdownMenu.Item
                  disabled={!here}
                  title={here ? "Open" : `In workspace “${r.workspaceName}”`}
                  onSelect={() => {
                    useUi.setState({ sidebarTab: "servers" });
                    void openServer(r.serverId);
                  }}
                  className="flex min-w-0 flex-1 items-center gap-2 rounded-lg px-2 py-1.5 outline-none data-[highlighted]:bg-hover"
                >
                  <span className="w-[38px] shrink-0 font-mono text-[10px] font-bold" style={{ color: info.color }}>
                    {info.short}
                  </span>
                  <div className="min-w-0 flex-1">
                    <div className="truncate text-[12.5px] text-fg">{r.name}</div>
                    <div className="truncate font-mono text-[11px] text-muted">
                      {r.url}
                      {!here && ` · ${r.workspaceName}`}
                    </div>
                  </div>
                </DropdownMenu.Item>
                <DropdownMenu.Item
                  aria-label={`Stop ${r.name}`}
                  title="Stop"
                  // Stays open, to stop others too.
                  onSelect={(e) => {
                    e.preventDefault();
                    void stopServer(r.runId);
                  }}
                  className="flex h-7 w-7 shrink-0 items-center justify-center rounded-lg text-muted outline-none data-[highlighted]:bg-hover data-[highlighted]:text-fg"
                >
                  <Square size={12} />
                </DropdownMenu.Item>
              </div>
            );
          })}
          <DropdownMenu.Separator className="my-1 h-px bg-line" />
          <DropdownMenu.Item onSelect={() => void stopAllServers()} className="flex h-8 items-center gap-2 rounded-lg px-2 text-[12.5px] text-danger outline-none data-[highlighted]:bg-hover">
            <Square size={13} /> Stop all
          </DropdownMenu.Item>
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  );
}

function EnvironmentPicker() {
  const info = useWorkspace((s) => s.info)!;
  const active = info.environments.find((e) => e.id === info.activeEnvironment);
  const item = "flex h-8 items-center gap-2 rounded-lg px-2 outline-none data-[highlighted]:bg-hover";
  return (
    <DropdownMenu.Root modal={false}>
      <DropdownMenu.Trigger asChild>
        <button
          data-testid="env-picker"
          className={cx(
            "flex h-7 max-w-[220px] items-center gap-2 rounded-lg px-2.5 text-[12.5px] transition-colors",
            active ? "bg-accent-soft text-fg hover:brightness-110" : "text-muted hover:bg-hover hover:text-fg",
          )}
        >
          <span className={cx("h-1.5 w-1.5 shrink-0 rounded-full", active ? "bg-accent" : "bg-faint")} />
          <span className="truncate">{active?.environment.name ?? "No environment"}</span>
          <ChevronDown size={13} className="shrink-0 text-faint" />
        </button>
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content align="end" sideOffset={6} collisionPadding={8} className="zv-pop z-[90] min-w-[230px] rounded-xl border border-line bg-elev p-1.5 text-[12.5px] shadow-pop max-h-[var(--radix-dropdown-menu-content-available-height)] overflow-y-auto overscroll-contain">
          <DropdownMenu.Item onSelect={() => void setActiveEnvironment(null)} className={item}>
            <span className="w-4">{!info.activeEnvironment && <Check size={13} />}</span>
            <span className="text-muted">No environment</span>
          </DropdownMenu.Item>
          {info.environments.map((e) => (
            <DropdownMenu.Item key={e.id} onSelect={() => void setActiveEnvironment(e.id)} className={item}>
              <span className="w-4">{info.activeEnvironment === e.id && <Check size={13} />}</span>
              <span className="truncate">{e.environment.name}</span>
              <span className="ml-auto text-[11px] text-faint">{e.environment.variables.length}</span>
            </DropdownMenu.Item>
          ))}
          <DropdownMenu.Separator className="my-1 h-px bg-line" />
          <DropdownMenu.Item onSelect={() => openModal({ type: "environments", focus: info.activeEnvironment ?? undefined })} className={item}>
            <span className="w-4" />
            Manage environments…
          </DropdownMenu.Item>
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  );
}

function WindowControls() {
  const [maximized, setMaximized] = useState(false);
  useEffect(() => {
    const win = getCurrentWindow();
    void win.isMaximized().then(setMaximized);
    const unlisten = win.onResized(() => void win.isMaximized().then(setMaximized));
    return () => void unlisten.then((f) => f());
  }, []);
  const btn = "flex h-11 w-[46px] items-center justify-center text-muted transition-colors hover:bg-hover hover:text-fg";
  return (
    <div className="ml-2 flex items-center self-stretch">
      <button aria-label="Minimize" className={btn} onClick={() => void getCurrentWindow().minimize()}>
        <Minus size={15} strokeWidth={1.6} />
      </button>
      <button aria-label={maximized ? "Restore" : "Maximize"} className={btn} onClick={() => void getCurrentWindow().toggleMaximize()}>
        {maximized ? <RestoreIcon /> : <Square size={12} strokeWidth={1.6} />}
      </button>
      <button aria-label="Close" className={cx(btn, "hover:bg-[#c42b1c] hover:text-white")} onClick={() => void getCurrentWindow().close()}>
        <X size={16} strokeWidth={1.6} />
      </button>
    </div>
  );
}

function RestoreIcon() {
  return (
    <svg width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.2">
      <rect x="1" y="3" width="8" height="8" rx="1" />
      <path d="M3 3V2a1 1 0 0 1 1-1h6a1 1 0 0 1 1 1v6a1 1 0 0 1-1 1H9" />
    </svg>
  );
}

import { lazy, Suspense, useEffect, useState } from "react";
import { Sidebar } from "./components/sidebar/Sidebar";
import { Splitter } from "./components/Splitter";
import { TabBar } from "./components/layout/TabBar";
import { TitleBar } from "./components/layout/TitleBar";
import { Welcome } from "./components/layout/Welcome";
import { Workbench } from "./components/layout/Workbench";
import { EnvironmentsModal } from "./components/modals/EnvironmentsModal";
import {
  CommandPalette,
  CookiesModal,
  ExportModal,
  FolderSettingsModal,
  SpecUpdateModal,
  ImportModal,
  MoveModal,
  SaveAsModal,
  ShortcutsModal,
  WorkspaceSettingsModal,
} from "./components/modals/MiscModals";
import { ActivityRail } from "./components/layout/ActivityRail";
import { Celebrations } from "./components/academy/Celebrations";
import { LabGuide } from "./components/academy/LabGuide";
import { AgentConfirmDialog } from "./components/agents/AgentConfirmDialog";
import { Dialogs, Toasts } from "./components/modals/Overlays";
import { SettingsModal } from "./components/modals/SettingsModal";
import { Spinner, TooltipProvider } from "./components/ui";
import { isMac, modKey } from "./lib/platform";
import { errorMessage } from "./lib/rpc";
import { initAcademy, useAcademy, useInBootcamp } from "./store/academy";
import { initUpdates } from "./store/updates";
import { UpdateNotice } from "./components/UpdateNotice";
import { loadAgents } from "./store/agents";
import { useDialogs } from "./store/dialogs";
import { refreshLoadTests, resetLoadTestResults, restoreActiveRun, toggleLoadRun } from "./store/loadtests";
import { toggleRun } from "./store/runner";
import { autoStartServers, refreshRunning, refreshServers, runServerTab } from "./store/servers";
import { loadSettings, useSettings, zoom } from "./store/settings";
import {
  activate,
  closeTab,
  isDirty,
  isLoadTestTab,
  isRequestTab,
  isRunnerTab,
  isServerTab,
  newRequest,
  restoreTabs,
  saveTab,
  send,
  updateLoadTestTab,
  useTabs,
} from "./store/tabs";
import { toast } from "./store/toasts";
import { openModal, useUi } from "./store/ui";
import { initWorkspace, useWorkspace } from "./store/workspace";

// The Academy is its own screen, loaded the first time it opens.
const AcademyView = lazy(() => import("./components/academy/AcademyView"));

function useTheme() {
  const theme = useSettings((s) => s.settings?.theme ?? "system");
  useEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const apply = () => {
      const dark = theme === "dark" || (theme === "system" && media.matches);
      document.documentElement.classList.toggle("dark", dark);
    };
    apply();
    media.addEventListener("change", apply);
    return () => media.removeEventListener("change", apply);
  }, [theme]);
}

/** ⌘+ / ⌘− / ⌘0 (Ctrl elsewhere) zoom everywhere: also on the welcome screen and in dialogs. */
function useZoomKeys() {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(isMac ? e.metaKey : e.ctrlKey) || e.altKey) return;
      const dir = e.key === "=" || e.key === "+" ? 1 : e.key === "-" || e.key === "_" ? -1 : e.key === "0" ? 0 : null;
      if (dir === null) return;
      e.preventDefault();
      zoom(dir);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
}

/** App-wide keys (exported for tests). */
export function useShortcuts(enabled: boolean) {
  useEffect(() => {
    if (!enabled) return;
    const onKey = (e: KeyboardEvent) => {
      // A confirm/prompt is up: keys belong to it (a second Mod+W would replace the "Discard?" dialog).
      if (useDialogs.getState().current) return;
      const modal = useUi.getState().modal.type;
      const modalOpen = modal !== "none";
      // So does a dialog of its own (e.g. "Mock this response"): Mod+Enter must not resend the request behind it.
      if (!modalOpen && document.querySelector('[role="dialog"]')) return;
      // ⌘ on macOS, Ctrl elsewhere: on macOS Ctrl+E/K/N/P are text-editing keys in inputs.
      const mod = isMac ? e.metaKey : e.ctrlKey;
      const { tabs, activeId } = useTabs.getState();
      if (e.ctrlKey && e.key === "Tab" && tabs.length > 1) {
        e.preventDefault();
        const i = tabs.findIndex((t) => t.id === activeId);
        const next = (i + (e.shiftKey ? -1 : 1) + tabs.length) % tabs.length;
        activate(tabs[next].id);
        return;
      }
      if (!mod || e.altKey) return;
      const key = e.key.toLowerCase();
      // Keys belong to an open dialog, the palette's too: it would replace the dialog and lose its unsaved edits.
      if (modalOpen && !(modal === "palette" && (key === "k" || key === "p"))) return;
      switch (key) {
        case "enter": {
          if (!activeId) break;
          e.preventDefault();
          const active = tabs.find((t) => t.id === activeId);
          if (isServerTab(active)) void runServerTab(active);
          else if (isLoadTestTab(active)) {
            // Starting shows the new run, not an earlier one picked from the history.
            updateLoadTestTab(active.id, () => ({ runId: null }));
            void toggleLoadRun(active.testId, active.draft);
          } else if (isRunnerTab(active)) {
            // A held key repeats: it must not stop the run it just started.
            if (!e.repeat) void toggleRun(active);
          } else void send(activeId);
          break;
        }
        case "s":
          e.preventDefault();
          if (activeId) void saveTab(activeId);
          break;
        case "n":
          e.preventDefault();
          newRequest("http");
          break;
        case "w":
          e.preventDefault();
          if (activeId) void closeTab(activeId);
          break;
        case "k":
        case "p":
          e.preventDefault();
          openModal({ type: "palette" });
          break;
        case "e":
          e.preventDefault();
          openModal({ type: "environments" });
          break;
        case ",":
          e.preventDefault();
          openModal({ type: "settings" });
          break;
        case "l": {
          e.preventDefault();
          (document.querySelector('input[aria-label="URL"]') as HTMLInputElement | null)?.focus();
          break;
        }
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [enabled]);
}

function Modals() {
  const modal = useUi((s) => s.modal);
  switch (modal.type) {
    case "settings":
      return <SettingsModal />;
    case "environments":
      return <EnvironmentsModal focus={modal.focus} />;
    case "import":
      return <ImportModal parent={modal.parent} />;
    case "cookies":
      return <CookiesModal />;
    case "workspaceSettings":
      return <WorkspaceSettingsModal />;
    case "folderSettings":
      return <FolderSettingsModal path={modal.path} />;
    case "specUpdate":
      return <SpecUpdateModal folder={modal.folder} name={modal.name} />;
    case "export":
      return <ExportModal tabId={modal.tabId} />;
    case "saveAs":
      return <SaveAsModal tabId={modal.tabId} />;
    case "move":
      return <MoveModal path={modal.path} name={modal.name} />;
    case "palette":
      return <CommandPalette />;
    case "shortcuts":
      return <ShortcutsModal mod={modKey} />;
    default:
      return null;
  }
}

export default function App() {
  const ready = useWorkspace((s) => s.ready);
  const info = useWorkspace((s) => s.info);
  const sidebarWidth = useUi((s) => s.sidebarWidth);
  const academyMode = useAcademy((s) => s.mode === "academy");
  const labRunning = useAcademy((s) => !!s.lab);
  const inBootcamp = useInBootcamp();
  const academy = academyMode && inBootcamp;
  const labGuide = labRunning && inBootcamp;
  const [fatal, setFatal] = useState<string | null>(null);
  useTheme();
  useZoomKeys();
  useShortcuts(!!info && !academy);

  useEffect(() => {
    (async () => {
      try {
        await Promise.all([loadSettings(), initWorkspace(), loadAgents(), initAcademy()]);
        void initUpdates().catch((e) => console.error("updates unavailable", e));
        const ws = useWorkspace.getState().info;
        if (ws) await restoreTabs(ws.path);
      } catch (e) {
        setFatal(errorMessage(e));
      }
    })();
  }, []);

  // Servers and load tests: the saved lists follow the open workspace; what runs is app-wide.
  const wsPath = info?.path;
  useEffect(() => {
    void refreshServers();
    void refreshRunning();
    if (wsPath) void autoStartServers();
    resetLoadTestResults();
    void refreshLoadTests().then(() => restoreActiveRun());
  }, [wsPath]);

  useEffect(() => {
    // Keep unsaved-work warnings honest if the window is closed in a browser.
    const onBeforeUnload = (e: BeforeUnloadEvent) => {
      if (import.meta.env.DEV) return;
      if (useTabs.getState().tabs.some((t) => (isServerTab(t) || isLoadTestTab(t) || (isRequestTab(t) && t.path)) && isDirty(t))) e.preventDefault();
    };
    window.addEventListener("beforeunload", onBeforeUnload);
    return () => window.removeEventListener("beforeunload", onBeforeUnload);
  }, []);

  if (fatal) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-2 p-8 text-center">
        <div className="text-[15px] font-semibold text-danger">Zorvik could not start</div>
        <div className="selectable max-w-lg font-mono text-[12px] text-muted">{fatal}</div>
      </div>
    );
  }
  if (!ready) {
    return (
      <div className="flex h-full items-center justify-center">
        <Spinner size={20} />
      </div>
    );
  }

  return (
    <TooltipProvider>
      <div className="flex h-full flex-col bg-panel">
        <TitleBar />
        {academy ? (
          <div className="min-h-0 flex-1 px-2 pb-2">
            <div className="h-full overflow-hidden rounded-xl border border-line/70 bg-bg">
              <Suspense
                fallback={
                  <div className="flex h-full items-center justify-center">
                    <Spinner size={20} />
                  </div>
                }
              >
                <AcademyView />
              </Suspense>
            </div>
          </div>
        ) : info ? (
          <div className="flex min-h-0 flex-1 pb-2 pr-2">
            <ActivityRail />
            <div style={{ width: sidebarWidth }} className="min-h-0 shrink-0">
              <Sidebar />
            </div>
            <Splitter
              direction="horizontal"
              subtle
              onResize={(d) => useUi.setState((s) => ({ sidebarWidth: Math.min(560, Math.max(200, s.sidebarWidth + d)) }))}
            />
            <main className="flex min-w-0 flex-1 flex-col overflow-hidden rounded-xl border border-line/70 bg-bg shadow-[0_1px_3px_rgb(0_0_0/0.12)]">
              <TabBar />
              <div className="min-h-0 flex-1">
                <Workbench />
              </div>
            </main>
            {labGuide && <LabGuide />}
          </div>
        ) : (
          <div className="min-h-0 flex-1 px-2 pb-2">
            <div className="h-full overflow-hidden rounded-xl border border-line/70 bg-bg">
              <Welcome />
            </div>
          </div>
        )}
      </div>
      <Modals />
      <Dialogs />
      <AgentConfirmDialog />
      <Toasts />
      <Celebrations />
      <UpdateNotice />
    </TooltipProvider>
  );
}

window.addEventListener("unhandledrejection", (e) => {
  console.error(e.reason);
  toast("error", "Unexpected error", errorMessage(e.reason));
});

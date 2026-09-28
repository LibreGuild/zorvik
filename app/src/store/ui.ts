import { create } from "zustand";

export type ModalState =
  | { type: "none" }
  | { type: "settings" }
  | { type: "environments"; focus?: string }
  | { type: "import"; parent?: string }
  | { type: "cookies" }
  | { type: "workspaceSettings" }
  | { type: "folderSettings"; path: string }
  | { type: "specUpdate"; folder: string; name: string }
  | { type: "export"; tabId: string }
  | { type: "saveAs"; tabId: string }
  | { type: "move"; path: string; name: string }
  | { type: "palette" }
  | { type: "shortcuts" };

export type SidebarSection = "collection" | "loadtests" | "servers" | "tools" | "history" | "agents" | "docs";

interface UiState {
  modal: ModalState;
  sidebarTab: SidebarSection;
  sidebarWidth: number;
  /** Fraction of the editor area given to the request pane. */
  split: number;
  layout: "stacked" | "side";
  /** Fraction of a server tab given to its settings (the rest shows traffic). */
  serverSplit: number;
  /** Fraction of a load test tab given to its settings (the rest shows results). */
  loadSplit: number;
  expanded: Record<string, boolean>;
}

const STORAGE_KEY = "zv:ui";

function load(): Partial<UiState> {
  try {
    return JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "{}") as Partial<UiState>;
  } catch {
    return {};
  }
}

const saved = load();

export const useUi = create<UiState>(() => ({
  modal: { type: "none" },
  sidebarTab: saved.sidebarTab ?? "collection",
  sidebarWidth: saved.sidebarWidth ?? 280,
  split: saved.split ?? 0.5,
  layout: saved.layout ?? "side",
  serverSplit: saved.serverSplit ?? 0.5,
  loadSplit: saved.loadSplit ?? 0.42,
  expanded: saved.expanded ?? {},
}));

useUi.subscribe((s) => {
  try {
    const { sidebarTab, sidebarWidth, split, layout, serverSplit, loadSplit, expanded } = s;
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ sidebarTab, sidebarWidth, split, layout, serverSplit, loadSplit, expanded }));
  } catch {
    /* storage unavailable */
  }
});

export const openModal = (modal: ModalState) => useUi.setState({ modal });
export const closeModal = () => useUi.setState({ modal: { type: "none" } });
export const toggleExpanded = (path: string, value?: boolean) =>
  useUi.setState((s) => ({ expanded: { ...s.expanded, [path]: value ?? !s.expanded[path] } }));

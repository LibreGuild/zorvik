// Updates from GitHub Releases (app/src-tauri/src/updates.rs): the state, and the actions the
// Settings → Updates section and the "ready to install" notice take. Desktop app only.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { create } from "zustand";
import { isTauri } from "../lib/rpc";
import { confirm } from "./dialogs";
import { useAcademy } from "./academy";
import { useAgents } from "./agents";
import { useLoadTests } from "./loadtests";
import { useServers } from "./servers";
import { isDirty, isLoadTestTab, isRequestTab, isServerTab, useTabs } from "./tabs";

export type UpdateStatus =
  | { state: "idle" }
  | { state: "checking" }
  | { state: "upToDate"; checkedAt: number }
  /** `byHand`: installing it automatically didn't work (download or signature); link to GitHub instead. */
  | { state: "available"; version: string; notes: string | null; date: string | null; byHand: boolean }
  | { state: "downloading"; version: string; percent: number | null }
  | { state: "ready"; version: string; notes: string | null }
  | { state: "installing" }
  | { state: "failed"; message: string };

export interface UpdateInfo {
  current: string;
  status: UpdateStatus;
  /** This copy can replace itself (not the portable zip, a .deb/.rpm or a disk image). */
  canInstall: boolean;
  whyNot: string | null;
  downloadPage: string;
}

const DISMISSED_KEY = "zv:update-dismissed";

interface UpdatesState {
  info: UpdateInfo | null;
  /** The version whose notice the user closed ("Later"). */
  dismissed: string | null;
}

function loadDismissed(): string | null {
  try {
    return localStorage.getItem(DISMISSED_KEY);
  } catch {
    return null;
  }
}

export const useUpdates = create<UpdatesState>(() => ({ info: null, dismissed: loadDismissed() }));

let started = false;

/** At startup (desktop app only): the current state, then every change. */
export async function initUpdates() {
  if (!isTauri || started) return;
  started = true;
  await listen<UpdateInfo>("zv:update", (e) => useUpdates.setState({ info: e.payload }));
  useUpdates.setState({ info: await invoke<UpdateInfo>("update_info") });
}

export const checkForUpdates = () => invoke("update_check");
export const downloadUpdate = () => invoke("update_download");

/** The newer version, when there is one (available or downloaded). */
export function newVersion(info: UpdateInfo | null): string | null {
  const s = info?.status;
  return s && (s.state === "available" || s.state === "ready" || s.state === "downloading") ? s.version : null;
}

/** A newer version is out that the user downloads from GitHub: this copy can't update itself,
 *  or installing it automatically didn't work. */
export const installsByHand = (info: UpdateInfo) => !info.canInstall || (info.status.state === "available" && info.status.byHand);

/** The release page of a version (the nightly channel has one page, replaced each day). */
export const releaseNotesUrl = (info: UpdateInfo, version: string, channel: "stable" | "nightly") =>
  `${info.downloadPage}/tag/${channel === "nightly" ? "nightly" : `v${version}`}`;

export function dismissNotice(version: string) {
  useUpdates.setState({ dismissed: version });
  try {
    localStorage.setItem(DISMISSED_KEY, version);
  } catch {
    /* storage unavailable */
  }
}

/** What a restart would interrupt, in a few words each. */
export function runningWork(): string[] {
  const work: string[] = [];
  const servers = useServers.getState().running.length;
  if (servers) work.push(servers === 1 ? "a server is running" : `${servers} servers are running`);
  if (useLoadTests.getState().active) work.push("a load test is running");
  const unsaved = useTabs
    .getState()
    .tabs.filter((t) => (isServerTab(t) || isLoadTestTab(t) || (isRequestTab(t) && t.path)) && isDirty(t)).length;
  if (unsaved) work.push(unsaved === 1 ? "a tab has unsaved changes" : `${unsaved} tabs have unsaved changes`);
  const lab = useAcademy.getState().lab;
  if (lab && !lab.finished) work.push("a Bootcamp lab is running");
  if (useAgents.getState().sessions.length) work.push("an AI agent is connected");
  return work;
}

/** Restart into the downloaded version, after asking when something would be interrupted. */
export async function installUpdate(): Promise<boolean> {
  const work = runningWork();
  if (work.length) {
    const list = work.join(", ");
    const ok = await confirm({
      title: "Restart to update?",
      message: `${list.charAt(0).toUpperCase()}${list.slice(1)}. Restarting stops ${work.length === 1 ? "it" : "them"} and closes unsaved changes. The update also installs by itself the next time you quit.`,
      confirmLabel: "Restart and update",
      danger: true,
    });
    if (!ok) return false;
  }
  await invoke("update_install");
  return true;
}

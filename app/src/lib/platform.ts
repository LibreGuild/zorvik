// Native dialogs and OS integration, with browser fallbacks for the dev bridge.
import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import { openUrl as tauriOpenUrl } from "@tauri-apps/plugin-opener";
import { isTauri } from "./rpc";

export async function pickFolder(title: string): Promise<string | null> {
  if (isTauri) {
    const r = await open({ directory: true, multiple: false, title });
    return typeof r === "string" ? r : null;
  }
  return window.prompt(`${title}\nEnter an absolute folder path:`)?.trim() || null;
}

export async function pickFile(title: string, extensions?: string[]): Promise<string | null> {
  if (isTauri) {
    const r = await open({
      multiple: false,
      title,
      filters: extensions ? [{ name: "Supported files", extensions }, { name: "All files", extensions: ["*"] }] : undefined,
    });
    return typeof r === "string" ? r : null;
  }
  return window.prompt(`${title}\nEnter an absolute file path:`)?.trim() || null;
}

export async function pickSavePath(title: string, defaultPath?: string): Promise<string | null> {
  if (isTauri) {
    return (await save({ title, defaultPath })) ?? null;
  }
  return window.prompt(`${title}\nEnter an absolute file path:`, defaultPath)?.trim() || null;
}

export async function openExternal(url: string): Promise<void> {
  if (isTauri) {
    await tauriOpenUrl(url);
  } else {
    window.open(url, "_blank", "noopener");
  }
}

export async function copyText(text: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    const ta = document.createElement("textarea");
    ta.value = text;
    ta.style.position = "fixed";
    ta.style.opacity = "0";
    document.body.appendChild(ta);
    ta.select();
    document.execCommand("copy");
    ta.remove();
  }
}

export const isMac = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent);
export const modKey = isMac ? "⌘" : "Ctrl";

/** Quit the desktop app (stops running servers first). */
export async function quitApp(): Promise<void> {
  if (isTauri) await invoke("quit");
}

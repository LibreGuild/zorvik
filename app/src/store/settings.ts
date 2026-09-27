import { create } from "zustand";
import type { Appearance } from "../bindings/Appearance";
import type { Settings } from "../bindings/Settings";
import { applyFonts, applyZoom, rememberAppearance, stepZoom } from "../lib/appearance";
import { onEvent } from "../lib/events";
import { api, errorMessage } from "../lib/rpc";
import { toast } from "./toasts";

export const useSettings = create<{ settings: Settings | null }>(() => ({ settings: null }));

export async function loadSettings() {
  useSettings.setState({ settings: await api.getSettings() });
}

export async function saveSettings(settings: Settings) {
  await api.saveSettings(settings);
  useSettings.setState({ settings });
}

let zoomSave: ReturnType<typeof setTimeout> | undefined;

/** Zoom in (1), out (-1) or back to 100 % (0): shown now, saved once the keys stop repeating. */
export function zoom(dir: -1 | 0 | 1) {
  const current = useSettings.getState().settings?.appearance.zoom;
  if (current !== undefined) setZoom(stepZoom(current, dir));
}

export function setZoom(percent: number) {
  const s = useSettings.getState().settings;
  if (!s || s.appearance.zoom === percent) return;
  useSettings.setState({ settings: { ...s, appearance: { ...s.appearance, zoom: percent } } });
  clearTimeout(zoomSave);
  zoomSave = setTimeout(() => {
    const latest = useSettings.getState().settings;
    if (latest) api.saveSettings(latest).catch((e) => toast("error", "Could not save the zoom", errorMessage(e)));
  }, 300);
}

const sameFonts = (a: Appearance, b: Appearance) =>
  a.uiFont === b.uiFont && a.codeFont === b.codeFont && a.codeFontSize === b.codeFontSize && a.ligatures === b.ligatures;

// The window follows the saved zoom and fonts. The Settings dialog previews its own fonts while
// open; a zoom change doesn't touch them.
useSettings.subscribe(({ settings }, prev) => {
  const a = settings?.appearance;
  const was = prev.settings?.appearance;
  if (!a || a === was) return;
  if (a.zoom !== was?.zoom) applyZoom(a.zoom);
  if (!was || !sameFonts(a, was)) applyFonts(a);
  rememberAppearance(a);
});

// Changed in the backend (e.g. the user allowed AI agents in their dialog).
onEvent((e) => {
  if (e.type === "settingsChanged") useSettings.setState({ settings: e.settings });
});

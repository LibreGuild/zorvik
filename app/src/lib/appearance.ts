// Zoom and fonts (Settings → General), applied to the whole window.
import { getCurrentWebview } from "@tauri-apps/api/webview";
import type { Appearance } from "../bindings/Appearance";
import { isTauri } from "./rpc";

/** Zoom steps of ⌘+ / ⌘−, in percent (the same as browsers). */
export const ZOOM_LEVELS = [50, 67, 75, 80, 90, 100, 110, 125, 150, 175, 200];
export const CODE_FONT_SIZES = [10, 11, 12, 13, 14, 15, 16, 18, 20, 22, 24];

/** Offered in the font fields when installed on this computer. */
export const UI_FONTS = [
  "Inter",
  "SF Pro Text",
  "Helvetica Neue",
  "Avenir Next",
  "Segoe UI",
  "Segoe UI Variable Text",
  "Arial",
  "Verdana",
  "Tahoma",
  "Roboto",
  "Open Sans",
  "Noto Sans",
  "IBM Plex Sans",
  "Source Sans 3",
  "Ubuntu",
  "Cantarell",
];
export const CODE_FONTS = [
  "JetBrains Mono",
  "Fira Code",
  "Cascadia Code",
  "Cascadia Mono",
  "Consolas",
  "SF Mono",
  "Menlo",
  "Monaco",
  "Source Code Pro",
  "IBM Plex Mono",
  "Roboto Mono",
  "Hack",
  "Iosevka",
  "Ubuntu Mono",
  "DejaVu Sans Mono",
  "Courier New",
];

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

/** The next zoom step up (1) or down (-1) from `zoom`; 0 resets to 100 %. */
export function stepZoom(zoom: number, dir: -1 | 0 | 1): number {
  if (dir === 0) return 100;
  const next = dir > 0 ? ZOOM_LEVELS.find((z) => z > zoom) : [...ZOOM_LEVELS].reverse().find((z) => z < zoom);
  return next ?? zoom;
}

export function applyZoom(percent: number) {
  const zoom = clamp(Math.round(percent) || 100, ZOOM_LEVELS[0], ZOOM_LEVELS[ZOOM_LEVELS.length - 1]) / 100;
  // The webview's own page zoom: text, icons, layout and hit-testing all scale together.
  if (isTauri) void getCurrentWebview().setZoom(zoom).catch(() => {});
  else document.documentElement.style.zoom = zoom === 1 ? "" : String(zoom);
}

/** A font name as one quoted CSS family (nothing that could end the value), or null when empty. */
export function fontFamily(name: string): string | null {
  const clean = name.replace(/["'\\;,{}<>]/g, "").trim();
  return clean ? `"${clean}"` : null;
}

/** Fonts go first in the stacks of styles.css; the system fonts stay as fallbacks. */
export function applyFonts(a: Appearance) {
  const root = document.documentElement.style;
  const set = (prop: string, value: string | null) => (value === null ? root.removeProperty(prop) : root.setProperty(prop, value));
  set("--ui-font-pick", fontFamily(a.uiFont));
  set("--code-font-pick", fontFamily(a.codeFont));
  set("--code-size", `${clamp(a.codeFontSize || 13, 8, 32)}px`);
  set("--code-ligatures", a.ligatures ? "normal" : null);
}

const CACHE = "zv:appearance";

/** Kept so the next start draws at the right zoom and fonts before settings load. */
export function rememberAppearance(a: Appearance) {
  try {
    localStorage.setItem(CACHE, JSON.stringify(a));
  } catch {
    /* storage unavailable */
  }
}

export function restoreAppearance() {
  try {
    const a = JSON.parse(localStorage.getItem(CACHE) ?? "null") as Appearance | null;
    if (!a) return;
    applyZoom(a.zoom);
    applyFonts(a);
  } catch {
    /* nothing saved */
  }
}

let probe: CanvasRenderingContext2D | null | undefined;

/** Whether a font is installed: text in it measures differently from the generic fallbacks. */
export function isFontInstalled(name: string): boolean {
  const family = fontFamily(name);
  if (!family) return true;
  probe ??= document.createElement("canvas").getContext("2d");
  if (!probe) return true;
  const ctx = probe;
  const sample = "mmmmmmmmmmlli10WwQ@#";
  return ["monospace", "serif", "sans-serif"].some((generic) => {
    ctx.font = `72px ${generic}`;
    const base = ctx.measureText(sample).width;
    ctx.font = `72px ${family}, ${generic}`;
    return ctx.measureText(sample).width !== base;
  });
}

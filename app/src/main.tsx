import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import { restoreAppearance } from "./lib/appearance";
import "./styles.css";

// Apply the OS theme before first paint to avoid a flash.
if (window.matchMedia("(prefers-color-scheme: dark)").matches) document.documentElement.classList.add("dark");
// And the last zoom and fonts, so nothing jumps when settings load.
restoreAppearance();

// Hide the browser's own context menu (Reload, Inspect…) except where text can be edited
// or selected; app menus are drawn by Radix and are unaffected.
document.addEventListener("contextmenu", (e) => {
  const target = e.target as HTMLElement | null;
  if (import.meta.env.DEV || target?.closest("input, textarea, [contenteditable], .cm-editor, .selectable")) return;
  e.preventDefault();
});

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);

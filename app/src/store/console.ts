// The console: every request the app sent and what scripts logged, as lines the backend keeps
// (in memory) and sends as `console` events. Headers and bodies stay in the backend until a
// line is opened (`console.details`).
import { create } from "zustand";
import type { ConsoleLine } from "../bindings/ConsoleLine";
import type { StreamEvent } from "../bindings/StreamEvent";
import { onEvent } from "../lib/events";
import { api } from "../lib/rpc";
import { useUi } from "./ui";

/** Lines kept, like the backend: beyond it the oldest go. */
export const MAX_LINES = 5000;

export type ConsoleFilter = "all" | "requests" | "logs" | "errors";

interface ConsoleState {
  lines: ConsoleLine[];
  /** Failed requests and error lines that arrived while the console was closed. */
  unseen: number;
}

export const useConsole = create<ConsoleState>(() => ({ lines: [], unseen: 0 }));

/** A request that failed or was answered with an error status, or an error line. */
export function isProblem(line: ConsoleLine): boolean {
  const item = line.item;
  if (item.type === "log") return item.level === "error";
  return item.error != null || (item.status != null && item.status >= 400);
}

/** `lines` with `added` after them (only lines newer than the last one), the newest `max` kept. */
export function appendLines(lines: ConsoleLine[], added: ConsoleLine[], max = MAX_LINES): ConsoleLine[] {
  let last = lines.length ? lines[lines.length - 1].id : 0;
  const fresh: ConsoleLine[] = [];
  for (const line of added) {
    if (line.id <= last) continue;
    fresh.push(line);
    last = line.id;
  }
  if (!fresh.length) return lines;
  const all = lines.concat(fresh);
  return all.length > max ? all.slice(all.length - max) : all;
}

/** The lines a filter and a search text (any case) let through. */
export function filterLines(lines: ConsoleLine[], filter: ConsoleFilter, search: string): ConsoleLine[] {
  const needle = search.trim().toLowerCase();
  if (filter === "all" && !needle) return lines;
  return lines.filter((line) => {
    const item = line.item;
    if (filter === "requests" && item.type !== "exchange") return false;
    if (filter === "logs" && item.type !== "log") return false;
    if (filter === "errors" && !isProblem(line)) return false;
    if (!needle) return true;
    const texts =
      item.type === "log"
        ? [item.message, line.request]
        : [item.url, item.method, item.protocol, item.error, item.origin, item.status == null ? null : String(item.status), line.request];
    return texts.some((t) => t != null && t.toLowerCase().includes(needle));
  });
}

// Events are applied together every few milliseconds: a collection run can log thousands of lines.
let pending: ConsoleLine[] = [];
let timer: ReturnType<typeof setTimeout> | null = null;

/** Apply the lines that are waiting (exported for tests). */
export function flushConsole() {
  if (timer) clearTimeout(timer);
  timer = null;
  const batch = pending;
  pending = [];
  if (!batch.length) return;
  const open = useUi.getState().consoleOpen;
  useConsole.setState((s) => {
    const lines = appendLines(s.lines, batch);
    if (lines === s.lines) return s;
    const last = s.lines.length ? s.lines[s.lines.length - 1].id : 0;
    const problems = open ? 0 : batch.filter((l) => l.id > last && isProblem(l)).length;
    return { lines, unseen: s.unseen + problems };
  });
}

// Counts the times the console was cleared: a list asked for before a clear is not shown after it.
let clears = 0;

function handle(e: StreamEvent) {
  if (e.type !== "console") return;
  if (e.event.type === "cleared") {
    clears++;
    pending = [];
    useConsole.setState({ lines: [], unseen: 0 });
    return;
  }
  pending.push(e.event.line);
  if (!timer) timer = setTimeout(flushConsole, 60);
}

let started = false;

/** Listen for lines and load the ones the backend already has (it keeps them across a reload of the window). */
export async function initConsole() {
  if (!started) {
    started = true;
    onEvent(handle);
    // Seen once the console is open.
    useUi.subscribe((s, prev) => {
      if (s.consoleOpen && !prev.consoleOpen) useConsole.setState({ unseen: 0 });
    });
  }
  const before = clears;
  const kept = await api.consoleList();
  // Cleared meanwhile (another workspace opened): those lines are gone.
  if (clears !== before) return;
  // Lines that came as events while it loaded are in `kept` too: the newer list wins.
  useConsole.setState((s) => ({ lines: appendLines(kept.slice(-MAX_LINES), s.lines) }));
}

export function toggleConsole() {
  useUi.setState((s) => ({ consoleOpen: !s.consoleOpen }));
}

export async function clearConsole() {
  // The backend answers with a `cleared` event, which empties the list here.
  await api.consoleClear();
}

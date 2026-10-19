// Tools that work as you type (JSON compare, filter playground, regex tester, certificate
// decoder): the inputs live in the tab's state with a revision number that every edit
// bumps; the result is computed in Rust a moment after the last edit and kept in the tab
// for that revision, so switching tabs neither loses it nor runs it again.
import { useEffect } from "react";
import { errorMessage } from "../../lib/rpc";
import { type ToolTab, updateToolState } from "../../store/tabs";

/** A result kept in a tool tab's state (`out`), for the inputs at revision `rev`. */
export interface LiveOut<R> {
  rev: number;
  value: R | null;
  error: string | null;
}

export interface Live<R> {
  /** The latest result (possibly of older inputs: see `stale`). */
  value: R | null;
  error: string | null;
  /** The inputs changed since the shown result; a new one is on its way. */
  stale: boolean;
  busy: boolean;
}

/** Revision of a tool tab's inputs (0 when nothing was typed yet). */
export const revOf = (state: Record<string, unknown>) => (typeof state.rev === "number" ? state.rev : 0);

/** Patch a tool tab's inputs: the revision goes up, so the result is computed again. */
export function editInputs(tabId: string, patch: (state: Record<string, unknown>) => Record<string, unknown>) {
  updateToolState(tabId, (s) => ({ ...s, ...patch(s), rev: revOf(s) + 1 }));
}

/**
 * Computes `run` `delay` ms after the inputs (revision `rev`) last changed; `run` null means
 * there is nothing to compute (the result is cleared). A result arriving after newer edits
 * is dropped.
 */
export function useLiveResult<R>(tab: ToolTab, run: (() => Promise<R>) | null, delay = 200): Live<R> {
  const rev = revOf(tab.state);
  const out = tab.state.out as LiveOut<R> | undefined;
  const current = out?.rev === rev;
  const ready = run !== null;
  useEffect(() => {
    if (!ready) {
      if (out)
        updateToolState(tab.id, (s) => {
          const next = { ...s };
          delete next.out;
          return next;
        });
      return;
    }
    if (current) return;
    const timer = setTimeout(() => {
      void run()
        .then(
          (value): LiveOut<R> => ({ rev, value, error: null }),
          (e): LiveOut<R> => ({ rev, value: null, error: errorMessage(e) }),
        )
        .then((next) => updateToolState(tab.id, (s) => (revOf(s) === rev ? { ...s, out: next } : s)));
    }, delay);
    return () => clearTimeout(timer);
    // `run` belongs to this revision's inputs; it is read when the revision (or readiness) changes.
  }, [tab.id, rev, ready]);
  if (!ready || !out) return { value: null, error: null, stale: false, busy: ready };
  return { value: out.value, error: out.error, stale: !current, busy: !current };
}

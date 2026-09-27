// Promise-based confirm/prompt dialogs rendered by <Dialogs/>.
import { create } from "zustand";

export type DialogRequest =
  | { kind: "confirm"; title: string; message: string; confirmLabel: string; danger: boolean; resolve: (ok: boolean) => void }
  | { kind: "prompt"; title: string; message?: string; value: string; placeholder?: string; confirmLabel: string; resolve: (v: string | null) => void };

export const useDialogs = create<{ current: DialogRequest | null }>(() => ({ current: null }));

/** One dialog at a time: a replaced one counts as cancelled so its caller doesn't wait forever. */
function show(request: DialogRequest) {
  const previous = useDialogs.getState().current;
  useDialogs.setState({ current: request });
  if (previous?.kind === "confirm") previous.resolve(false);
  else previous?.resolve(null);
}

export function confirm(opts: { title: string; message: string; confirmLabel?: string; danger?: boolean }): Promise<boolean> {
  return new Promise((resolve) =>
    show({ kind: "confirm", confirmLabel: opts.confirmLabel ?? "OK", danger: opts.danger ?? false, title: opts.title, message: opts.message, resolve }),
  );
}

export function prompt(opts: { title: string; message?: string; value?: string; placeholder?: string; confirmLabel?: string }): Promise<string | null> {
  return new Promise((resolve) =>
    show({
      kind: "prompt",
      title: opts.title,
      message: opts.message,
      value: opts.value ?? "",
      placeholder: opts.placeholder,
      confirmLabel: opts.confirmLabel ?? "OK",
      resolve,
    }),
  );
}

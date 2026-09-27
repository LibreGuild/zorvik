import { create } from "zustand";
import { newId } from "../lib/ids";

export interface Toast {
  id: string;
  tone: "info" | "success" | "error";
  title: string;
  detail?: string;
}

export const useToasts = create<{ toasts: Toast[] }>(() => ({ toasts: [] }));

export function toast(tone: Toast["tone"], title: string, detail?: string) {
  const id = newId();
  useToasts.setState((s) => ({ toasts: [...s.toasts.slice(-3), { id, tone, title, detail }] }));
  setTimeout(() => dismissToast(id), tone === "error" ? 7000 : 3500);
}

export function dismissToast(id: string) {
  useToasts.setState((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) }));
}

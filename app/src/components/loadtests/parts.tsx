// Small building blocks shared by the load test editor, results and sidebar.
import { useEffect, useLayoutEffect, useRef, useState, type InputHTMLAttributes, type ReactNode } from "react";
import { Activity } from "lucide-react";
import type { LoadModel } from "../../bindings/LoadModel";
import { cx, Input } from "../ui";
import { MODELS } from "./model";

/** The model as a badge: "VU" or "RPS". */
export function ModelBadge({ model, icon, className }: { model: LoadModel; icon?: number; className?: string }) {
  return (
    <span className={cx("inline-flex shrink-0 items-center gap-1 font-mono text-[10px] font-bold text-accent", className)} title={MODELS[model].label}>
      {icon ? <Activity size={icon} /> : null}
      {MODELS[model].short}
    </span>
  );
}

/** Width of an element, kept up to date (0 until measured). */
export function useElementWidth<T extends HTMLElement>(): [React.RefObject<T | null>, number] {
  const ref = useRef<T>(null);
  const [width, setWidth] = useState(0);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    setWidth(Math.round(el.getBoundingClientRect().width));
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver((entries) => {
      const w = Math.round(entries[0]?.contentRect.width ?? 0);
      setWidth((prev) => (prev === w ? prev : w));
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, []);
  return [ref, width];
}

/** Re-render every `ms` while `on` (clocks that must move without new data); returns a counter. */
export function useTicker(on: boolean, ms = 1000): number {
  const [tick, setTick] = useState(0);
  useEffect(() => {
    if (!on) return;
    const t = setInterval(() => setTick((n) => n + 1), ms);
    return () => clearInterval(t);
  }, [on, ms]);
  return tick;
}

/** Whole numbers in the file are u32 (a larger one would make the save fail). */
const MAX_WHOLE = 4_294_967_295;

/**
 * A number field that lets the text be edited freely (empty, "1." …) and reports numbers
 * of at least `min`. Emptying it changes nothing (the value shows again on blur), except
 * `optional` fields, which report `undefined`. Values above `max` are reported and shown
 * as invalid, so a start that fails for it is explained.
 */
export function NumberInput({
  value,
  onChange,
  min = 0,
  max = Number.MAX_SAFE_INTEGER,
  integer = true,
  optional,
  className,
  ...rest
}: Omit<InputHTMLAttributes<HTMLInputElement>, "value" | "onChange" | "min" | "max"> & {
  value: number | undefined;
  onChange: (v: number | undefined) => void;
  min?: number;
  max?: number;
  integer?: boolean;
  optional?: boolean;
}) {
  const [text, setText] = useState("");
  const [focused, setFocused] = useState(false);
  const shown = focused ? text : value === undefined ? "" : String(value);
  const over = value !== undefined && value > max;
  const invalid = over || rest["aria-invalid"] === true;
  return (
    <Input
      {...rest}
      inputMode={integer ? "numeric" : "decimal"}
      className={cx("font-mono tabular-nums", className)}
      invalid={invalid}
      aria-invalid={invalid || undefined}
      value={shown}
      onFocus={(e) => {
        setText(value === undefined ? "" : String(value));
        setFocused(true);
        rest.onFocus?.(e);
      }}
      onBlur={(e) => {
        setFocused(false);
        rest.onBlur?.(e);
      }}
      onChange={(e) => {
        const raw = e.target.value;
        setText(raw);
        const trimmed = raw.trim();
        if (!trimmed) {
          if (optional) onChange(undefined);
          return;
        }
        const n = Number(trimmed);
        if (!Number.isFinite(n)) return;
        onChange(Math.max(min, integer ? Math.min(MAX_WHOLE, Math.trunc(n)) : n));
      }}
    />
  );
}

/** A titled panel on the results side. */
export function Panel({ title, right, children, className, testId }: { title: ReactNode; right?: ReactNode; children: ReactNode; className?: string; testId?: string }) {
  return (
    <section className={cx("min-w-0 rounded-xl border border-line/80 bg-bg", className)} data-testid={testId}>
      <div className="flex min-h-9 flex-wrap items-center gap-x-3 gap-y-1 px-3 pt-2">
        <h3 className="text-[12px] font-semibold text-fg">{title}</h3>
        <div className="flex-1" />
        {right}
      </div>
      <div className="px-3 pb-3 pt-1.5">{children}</div>
    </section>
  );
}

export type StatTone = "success" | "warning" | "danger" | "accent";

/** A number with a caption and an optional second line. */
export function Stat({ label, value, sub, tone, testId, raw }: { label: string; value: ReactNode; sub?: ReactNode; tone?: StatTone; testId?: string; raw?: number }) {
  return (
    <div
      className={cx(
        "min-w-0 rounded-xl border px-3 py-2.5",
        tone === "warning" ? "border-warning/40 bg-warning/8" : tone === "danger" ? "border-danger/35 bg-danger/6" : "border-line/80 bg-panel-2/60",
      )}
      data-testid={testId}
    >
      <div className="truncate text-[11px] font-medium text-muted">{label}</div>
      <div
        className={cx(
          "mt-0.5 truncate font-mono text-[18px] font-semibold leading-6 tabular-nums",
          tone === "success" ? "text-success" : tone === "warning" ? "text-warning" : tone === "danger" ? "text-danger" : tone === "accent" ? "text-accent" : "text-fg",
        )}
        data-testid={testId ? `${testId}-value` : undefined}
        data-value={raw}
      >
        {value}
      </div>
      {sub != null && <div className="mt-0.5 truncate text-[11px] text-faint">{sub}</div>}
    </div>
  );
}

/** PASS / FAIL chip. */
export function Verdict({ passed, size = "sm" }: { passed: boolean; size?: "sm" | "lg" }) {
  return (
    <span
      className={cx(
        "inline-flex shrink-0 items-center rounded-md font-bold tracking-wide",
        size === "lg" ? "px-2 py-0.5 text-[12px]" : "px-1.5 py-px text-[10px]",
        passed ? "bg-success/14 text-success" : "bg-danger/14 text-danger",
      )}
    >
      {passed ? "PASS" : "FAIL"}
    </span>
  );
}

// Building blocks shared by the network tools (layout, key/value rows, copy).
import type { ReactNode } from "react";
import { Check, CircleAlert, Copy } from "lucide-react";
import { copyText } from "../../lib/platform";
import { toast } from "../../store/toasts";
import { cx, IconButton } from "../ui";

/**
 * The inputs row at the top of a tool; Enter in one of its fields runs `onSubmit`. Not a
 * <form>: buttons without a type (Segmented) would submit it when clicked, and Enter would
 * "click" the first of them (switching e.g. the port preset back before running).
 */
export function ToolBar({ children, onSubmit }: { children: ReactNode; onSubmit?: () => void }) {
  return (
    <div
      className="flex flex-wrap items-end gap-2 px-4 pb-3"
      onKeyDown={(e) => {
        if (e.key !== "Enter" || e.nativeEvent.isComposing || e.keyCode === 229 || e.metaKey || e.ctrlKey || e.altKey || e.shiftKey) return;
        if (!(e.target instanceof HTMLInputElement)) return;
        e.preventDefault();
        onSubmit?.();
      }}
    >
      {children}
    </div>
  );
}

/** A caption over a control. `group` for several buttons (a <label> would click the first one). */
export function LabeledField({ label, children, className, group }: { label: string; children: ReactNode; className?: string; group?: boolean }) {
  const Tag = group ? "div" : "label";
  return (
    <Tag role={group ? "group" : undefined} aria-label={group ? label : undefined} className={cx("flex flex-col gap-1", className)}>
      <span className="text-[11.5px] font-medium text-muted">{label}</span>
      {children}
    </Tag>
  );
}

export function Section({ title, right, children }: { title: ReactNode; right?: ReactNode; children: ReactNode }) {
  return (
    <section className="pb-2">
      <div className="flex items-center gap-2 px-4 pb-1.5 pt-3">
        <h3 className="flex-1 text-[11px] font-semibold uppercase tracking-wide text-faint">{title}</h3>
        {right}
      </div>
      {children}
    </section>
  );
}

export function KV({ label, value, mono = true, copy }: { label: string; value: ReactNode; mono?: boolean; copy?: string }) {
  return (
    <div className="group grid grid-cols-[150px_1fr] gap-3 px-4 py-1 text-[12.5px]">
      <div className="text-muted">{label}</div>
      <div className="flex min-w-0 items-start gap-1">
        <div className={cx("selectable min-w-0 break-all text-fg", mono && "font-mono text-[12px]")}>{value}</div>
        {copy && <CopyButton text={copy} label={`Copy ${label.toLowerCase()}`} className="-my-0.5 opacity-0 group-hover:opacity-100 focus-visible:opacity-100" />}
      </div>
    </div>
  );
}

export function CopyButton({ text, label = "Copy", className }: { text: string; label?: string; className?: string }) {
  return (
    <IconButton
      label={label}
      size={22}
      className={className}
      onClick={() => {
        void copyText(text);
        toast("success", "Copied to clipboard");
      }}
    >
      <Copy size={12} />
    </IconButton>
  );
}

export function StatCard({ label, value, tone }: { label: string; value: ReactNode; tone?: "success" | "warning" | "danger" }) {
  return (
    <div className="min-w-[92px] flex-1 rounded-lg bg-panel-2 px-3 py-2">
      <div className="text-[11px] text-muted">{label}</div>
      <div
        className={cx(
          "font-mono text-[15px] font-semibold tabular-nums",
          tone === "success" ? "text-success" : tone === "warning" ? "text-warning" : tone === "danger" ? "text-danger" : "text-fg",
        )}
      >
        {value}
      </div>
    </div>
  );
}

export function ErrorNote({ children }: { children: ReactNode }) {
  return (
    <div role="alert" className="mx-4 mb-3 flex items-start gap-2 rounded-lg border border-danger/30 bg-danger/10 px-3 py-2 text-[12.5px] text-danger">
      <CircleAlert size={14} className="mt-0.5 shrink-0" />
      <div className="selectable min-w-0 break-words">{children}</div>
    </div>
  );
}

export function Hint({ children, icon }: { children: ReactNode; icon?: ReactNode }) {
  return (
    <div className="mx-4 mb-3 flex items-start gap-2 rounded-lg bg-panel-2 px-3 py-2 text-[12px] text-muted">
      {icon && <span className="mt-0.5 shrink-0 text-faint">{icon}</span>}
      <div className="min-w-0">{children}</div>
    </div>
  );
}

/** ✓ / ✗ / – for yes / no / unknown. */
export function YesNo({ value, yes = "Yes", no = "No", unknown = "Not tested" }: { value: boolean | null; yes?: string; no?: string; unknown?: string }) {
  if (value === null) return <span className="text-faint">{unknown}</span>;
  return value ? (
    <span className="inline-flex items-center gap-1 text-success">
      <Check size={13} strokeWidth={2.5} />
      {yes}
    </span>
  ) : (
    <span className="inline-flex items-center gap-1 text-muted">{no}</span>
  );
}

export const textareaClass =
  "selectable w-full resize-none rounded-lg border border-line bg-input px-2.5 py-2 font-mono text-[12.5px] leading-relaxed text-fg outline-none placeholder:text-faint hover:border-line-strong focus:border-accent focus:ring-2 focus:ring-accent-soft";

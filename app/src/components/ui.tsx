// Small design-system primitives shared across the app.
import { forwardRef, useEffect, useRef, useState, type ButtonHTMLAttributes, type InputHTMLAttributes, type ReactNode, type SelectHTMLAttributes } from "react";
import { Dialog as RDialog, DropdownMenu, Tooltip as RTooltip, ContextMenu as RContextMenu } from "radix-ui";
import { Check, ChevronDown, Loader2, X } from "lucide-react";

export function cx(...parts: (string | false | null | undefined)[]): string {
  return parts.filter(Boolean).join(" ");
}

type Variant = "primary" | "secondary" | "ghost" | "danger";
type Size = "sm" | "md";

const variants: Record<Variant, string> = {
  primary: "bg-accent-strong text-accent-fg hover:brightness-110 active:brightness-95 shadow-sm",
  secondary: "bg-panel-2 text-fg border border-line hover:bg-hover hover:border-line-strong",
  ghost: "text-muted hover:text-fg hover:bg-hover",
  danger: "bg-danger text-white hover:brightness-110",
};
const sizes: Record<Size, string> = {
  sm: "h-7 px-2.5 text-[12px] gap-1.5",
  md: "h-8 px-3.5 text-[13px] gap-2",
};

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: Variant;
  size?: Size;
  loading?: boolean;
  icon?: ReactNode;
}

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(function Button(
  { variant = "secondary", size = "md", loading, icon, className, children, disabled, ...rest },
  ref,
) {
  return (
    <button
      ref={ref}
      type="button"
      disabled={disabled || loading}
      className={cx(
        "inline-flex items-center justify-center rounded-lg font-medium whitespace-nowrap transition-[background,filter,border-color,color] select-none",
        "disabled:opacity-50 disabled:pointer-events-none",
        variants[variant],
        sizes[size],
        className,
      )}
      {...rest}
    >
      {loading ? <Loader2 size={14} className="zv-spin" /> : icon}
      {children}
    </button>
  );
});

export const IconButton = forwardRef<HTMLButtonElement, ButtonHTMLAttributes<HTMLButtonElement> & { label: string; active?: boolean; size?: number }>(
  function IconButton({ label, active, className, children, size = 26, ...rest }, ref) {
    return (
      <Tooltip content={label}>
        <button
          ref={ref}
          type="button"
          aria-label={label}
          style={{ width: size, height: size }}
          className={cx(
            "inline-flex shrink-0 items-center justify-center rounded-lg text-muted transition-colors hover:bg-hover hover:text-fg disabled:opacity-40 disabled:pointer-events-none",
            active && "bg-hover text-fg",
            className,
          )}
          {...rest}
        >
          {children}
        </button>
      </Tooltip>
    );
  },
);

export const Input = forwardRef<HTMLInputElement, InputHTMLAttributes<HTMLInputElement> & { invalid?: boolean }>(function Input(
  { className, invalid, ...rest },
  ref,
) {
  return (
    <input
      ref={ref}
      spellCheck={false}
      autoComplete="off"
      autoCorrect="off"
      autoCapitalize="off"
      className={cx(
        "h-8 rounded-lg border bg-input px-2.5 text-[13px] text-fg placeholder:text-faint outline-none transition-colors",
        !/(^|\s)w-/.test(className ?? "") && "w-full",
        "focus:border-accent focus:ring-2 focus:ring-accent-soft",
        invalid ? "border-danger" : "border-line hover:border-line-strong",
        className,
      )}
      {...rest}
    />
  );
});

export function Select({ className, children, ...rest }: SelectHTMLAttributes<HTMLSelectElement>) {
  return (
    <div className={cx("relative inline-flex", className)}>
      <select
        className="h-8 w-full appearance-none rounded-lg border border-line bg-input pl-2.5 pr-7 text-[13px] text-fg outline-none hover:border-line-strong focus:border-accent focus:ring-2 focus:ring-accent-soft"
        {...rest}
      >
        {children}
      </select>
      <ChevronDown size={14} className="pointer-events-none absolute right-2 top-1/2 -translate-y-1/2 text-faint" />
    </div>
  );
}

export function Switch({ checked, onChange, label, disabled }: { checked: boolean; onChange: (v: boolean) => void; label?: ReactNode; disabled?: boolean }) {
  return (
    <label className={cx("inline-flex items-center gap-2.5", disabled ? "opacity-50" : "cursor-pointer")}>
      <button
        type="button"
        role="switch"
        aria-checked={checked}
        disabled={disabled}
        onClick={() => onChange(!checked)}
        className={cx(
          "relative block h-5 w-9 shrink-0 rounded-full p-0 transition-colors",
          checked ? "bg-accent-strong" : "bg-line-strong",
        )}
      >
        <span
          className={cx(
            "absolute left-0.5 top-0.5 block h-4 w-4 rounded-full bg-white shadow-sm transition-transform",
            checked ? "translate-x-4" : "translate-x-0",
          )}
        />
      </button>
      {label && <span className="text-[13px] text-fg">{label}</span>}
    </label>
  );
}

export function Checkbox({ checked, onChange, label, title }: { checked: boolean; onChange: (v: boolean) => void; label?: string; title?: string }) {
  return (
    <button
      type="button"
      role="checkbox"
      aria-checked={checked}
      aria-label={label ?? title}
      title={title}
      onClick={() => onChange(!checked)}
      className={cx(
        "inline-flex h-4 w-4 shrink-0 items-center justify-center rounded border transition-colors",
        checked ? "border-accent-strong bg-accent-strong text-accent-fg" : "border-line-strong bg-input hover:border-accent",
      )}
    >
      {checked && <Check size={11} strokeWidth={3} />}
    </button>
  );
}

export interface TabItem<T extends string> {
  id: T;
  label: ReactNode;
  badge?: ReactNode;
}

export function Tabs<T extends string>({ items, value, onChange, className, right }: { items: TabItem<T>[]; value: T; onChange: (v: T) => void; className?: string; right?: ReactNode }) {
  // In a narrow pane the tabs scroll sideways (no scrollbar): a fade marks each cut edge,
  // the mouse wheel scrolls them, and the selected tab is kept in view.
  const list = useRef<HTMLDivElement>(null);
  const [cut, setCut] = useState({ start: false, end: false });
  useEffect(() => {
    const el = list.current;
    if (!el) return;
    const measure = () => setCut({ start: el.scrollLeft > 1, end: el.scrollLeft + el.clientWidth < el.scrollWidth - 1 });
    const wheel = (e: WheelEvent) => {
      if (Math.abs(e.deltaY) > Math.abs(e.deltaX) && el.scrollWidth > el.clientWidth) {
        el.scrollLeft += e.deltaY;
        e.preventDefault();
      }
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    el.addEventListener("scroll", measure, { passive: true });
    el.addEventListener("wheel", wheel, { passive: false });
    return () => {
      observer.disconnect();
      el.removeEventListener("scroll", measure);
      el.removeEventListener("wheel", wheel);
    };
  }, []);
  useEffect(() => {
    list.current?.querySelector<HTMLElement>('[aria-selected="true"]')?.scrollIntoView?.({ block: "nearest", inline: "nearest" });
  }, [value]);
  const fade = `linear-gradient(to right, ${cut.start ? "transparent, black 24px" : "black"}, ${cut.end ? "black calc(100% - 24px), transparent" : "black"})`;
  return (
    <div className={cx("flex h-10 shrink-0 items-center px-3", className)}>
      <div
        ref={list}
        className="flex min-w-0 flex-1 items-center gap-0.5 overflow-x-auto [scrollbar-width:none]"
        style={cut.start || cut.end ? { maskImage: fade, WebkitMaskImage: fade } : undefined}
        role="tablist"
      >
      {items.map((t) => (
        <button
          key={t.id}
          role="tab"
          aria-selected={value === t.id}
          onClick={() => onChange(t.id)}
          className={cx(
            "flex h-7 shrink-0 items-center gap-1.5 rounded-lg px-2.5 text-[12.5px] font-medium transition-colors",
            value === t.id ? "bg-hover text-fg" : "text-muted hover:bg-hover/60 hover:text-fg",
          )}
        >
          {t.label}
          {t.badge != null && t.badge !== 0 && (
            <span className={cx("rounded-full px-1.5 text-[10.5px] leading-4", value === t.id ? "bg-elev text-muted" : "bg-hover text-faint")}>{t.badge}</span>
          )}
        </button>
      ))}
      </div>
      {right && <div className="flex shrink-0 items-center gap-1 pl-1">{right}</div>}
    </div>
  );
}

export function Segmented<T extends string>({ items, value, onChange }: { items: { id: T; label: ReactNode }[]; value: T; onChange: (v: T) => void }) {
  return (
    <div className="inline-flex rounded-lg bg-panel-2 p-0.5">
      {items.map((i) => (
        <button
          key={i.id}
          type="button"
          aria-pressed={value === i.id}
          onClick={() => onChange(i.id)}
          className={cx(
            "whitespace-nowrap rounded-md px-2 py-0.5 text-[12px] font-medium transition-colors",
            value === i.id ? "bg-elev text-fg shadow-sm" : "text-muted hover:text-fg",
          )}
        >
          {i.label}
        </button>
      ))}
    </div>
  );
}

export function Spinner({ size = 14 }: { size?: number }) {
  return <Loader2 size={size} className="zv-spin text-muted" />;
}

export function Kbd({ children }: { children: ReactNode }) {
  return (
    <kbd className="rounded border border-line bg-panel-2 px-1.5 py-px font-sans text-[11px] text-muted">{children}</kbd>
  );
}

export function EmptyState({ icon, title, children, action }: { icon?: ReactNode; title: ReactNode; children?: ReactNode; action?: ReactNode }) {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-2 p-6 text-center">
      {icon && <div className="mb-1 text-faint">{icon}</div>}
      <div className="text-[13.5px] font-medium text-fg">{title}</div>
      {children && <div className="max-w-sm text-[12.5px] text-muted">{children}</div>}
      {action && <div className="mt-2">{action}</div>}
    </div>
  );
}

export function Tooltip({ content, children, side = "bottom" }: { content: ReactNode; children: ReactNode; side?: "top" | "bottom" | "left" | "right" }) {
  if (!content) return <>{children}</>;
  return (
    <RTooltip.Root delayDuration={450}>
      <RTooltip.Trigger asChild>{children}</RTooltip.Trigger>
      <RTooltip.Portal>
        <RTooltip.Content
          side={side}
          sideOffset={6}
          className="zv-fade z-[100] max-w-xs rounded-lg border border-line bg-elev px-2 py-1 text-[11.5px] text-fg shadow-pop"
        >
          {content}
        </RTooltip.Content>
      </RTooltip.Portal>
    </RTooltip.Root>
  );
}

export const TooltipProvider = RTooltip.Provider;

export interface MenuItem {
  label: ReactNode;
  icon?: ReactNode;
  onSelect?: () => void;
  danger?: boolean;
  disabled?: boolean;
  shortcut?: string;
  separator?: false;
  checked?: boolean;
}
export type MenuEntry = MenuItem | { separator: true };

// Long menus stay inside the window and scroll (Radix measures the room left on the side they open).
const menuContent =
  "zv-pop z-[90] min-w-[190px] overflow-y-auto overscroll-contain rounded-xl border border-line bg-elev p-1.5 text-[12.5px] shadow-pop";
/** Gap kept between a menu and the window's edges. */
const MENU_EDGE = 8;
const menuItem =
  "flex h-8 cursor-default items-center gap-2 rounded-lg px-2 outline-none data-[highlighted]:bg-hover data-[disabled]:opacity-40";

function renderEntries(entries: MenuEntry[], Item: typeof DropdownMenu.Item, Sep: typeof DropdownMenu.Separator) {
  return entries.map((e, i) =>
    "separator" in e && e.separator ? (
      <Sep key={i} className="my-1 h-px bg-line" />
    ) : (
      <Item
        key={i}
        disabled={(e as MenuItem).disabled}
        onSelect={() => (e as MenuItem).onSelect?.()}
        className={cx(menuItem, (e as MenuItem).danger && "text-danger")}
      >
        <span className="flex w-4 justify-center text-muted">
          {(e as MenuItem).checked ? <Check size={13} /> : (e as MenuItem).icon}
        </span>
        <span className="flex-1">{(e as MenuItem).label}</span>
        {(e as MenuItem).shortcut && <span className="text-[11px] text-faint">{(e as MenuItem).shortcut}</span>}
      </Item>
    ),
  );
}

export function Menu({ trigger, entries, align = "start" }: { trigger: ReactNode; entries: MenuEntry[]; align?: "start" | "end" }) {
  return (
    <DropdownMenu.Root modal={false}>
      <DropdownMenu.Trigger asChild>{trigger}</DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content
          align={align}
          sideOffset={4}
          collisionPadding={MENU_EDGE}
          className={cx(menuContent, "max-h-[var(--radix-dropdown-menu-content-available-height)]")}
        >
          {renderEntries(entries, DropdownMenu.Item, DropdownMenu.Separator)}
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  );
}

export function ContextMenu({ children, entries }: { children: ReactNode; entries: MenuEntry[] }) {
  return (
    <RContextMenu.Root modal={false}>
      <RContextMenu.Trigger asChild>{children}</RContextMenu.Trigger>
      <RContextMenu.Portal>
        <RContextMenu.Content
          collisionPadding={MENU_EDGE}
          className={cx(menuContent, "max-h-[var(--radix-context-menu-content-available-height)]")}
        >
          {renderEntries(entries, RContextMenu.Item as unknown as typeof DropdownMenu.Item, RContextMenu.Separator as unknown as typeof DropdownMenu.Separator)}
        </RContextMenu.Content>
      </RContextMenu.Portal>
    </RContextMenu.Root>
  );
}

export function Modal({
  open,
  onClose,
  title,
  description,
  children,
  footer,
  width = 560,
  bodyClassName,
  focusFirstField = true,
  dirty = false,
  dismissOnOutside = true,
}: {
  open: boolean;
  onClose: () => void;
  title: ReactNode;
  description?: ReactNode;
  children: ReactNode;
  footer?: ReactNode;
  width?: number;
  bodyClassName?: string;
  /** Focus the first input on open (off for editors where that invites accidental edits). */
  focusFirstField?: boolean;
  /** Unsaved edits: Escape, a click outside or × ask before closing (Cancel buttons still close). */
  dirty?: boolean;
  /** A click outside closes it (off for questions a stray click must not answer). */
  dismissOnOutside?: boolean;
}) {
  const [askDiscard, setAskDiscard] = useState(false);
  // Only a dirty dialog changes how it closes; a clean one keeps Radix's behavior.
  const holdIfDirty = (e: Event) => {
    if (!dirty) return;
    e.preventDefault();
    setAskDiscard(true);
  };
  return (
    <RDialog.Root open={open} onOpenChange={(o) => !o && (dirty ? setAskDiscard(true) : onClose())}>
      <RDialog.Portal>
        <RDialog.Overlay className="zv-fade fixed inset-0 z-[70] bg-black/40 backdrop-blur-[1px]" />
        <RDialog.Content
          // Only opt out of the description link when there is none (an explicit undefined would drop it).
          {...(description ? {} : { "aria-describedby": undefined })}
          tabIndex={-1}
          onEscapeKeyDown={(e) => {
            // Escape first closes an open autocomplete list (VarInput, CodeMirror) rather than the dialog and its edits.
            if ((e.target as Element | null)?.closest?.("[data-autocomplete-open], .cm-content[aria-controls]")) e.preventDefault();
            else holdIfDirty(e);
          }}
          onInteractOutside={(e) => (dismissOnOutside ? holdIfDirty(e) : e.preventDefault())}
          onOpenAutoFocus={(e) => {
            e.preventDefault();
            const root = e.currentTarget as HTMLElement;
            // Respect a field React already focused (autoFocus).
            if (document.activeElement && document.activeElement !== root && root.contains(document.activeElement)) return;
            const field = focusFirstField ? root.querySelector<HTMLElement>("input:not([type=hidden]), textarea, .cm-content") : null;
            (field ?? root).focus();
          }}
          style={{ width: `min(${width}px, calc(100vw - 32px))` }}
          className="zv-pop fixed left-1/2 top-[8vh] z-[80] flex max-h-[84vh] -translate-x-1/2 flex-col rounded-2xl border border-line bg-bg shadow-pop outline-none"
        >
          <div className="flex items-start gap-3 px-5 pb-2 pt-4">
            <div className="min-w-0 flex-1">
              <RDialog.Title className="text-[14.5px] font-semibold text-fg">{title}</RDialog.Title>
              {description && <RDialog.Description className="mt-0.5 text-[12.5px] text-muted">{description}</RDialog.Description>}
            </div>
            <RDialog.Close asChild>
              <button aria-label="Close" className="rounded-lg p-1 text-muted hover:bg-hover hover:text-fg">
                <X size={16} />
              </button>
            </RDialog.Close>
          </div>
          <div className={cx("min-h-0 flex-1 overflow-auto px-5 py-4", bodyClassName)}>{children}</div>
          {askDiscard && dirty ? (
            <div role="alertdialog" aria-label="Unsaved changes" className="flex items-center justify-end gap-2 border-t border-line/60 px-5 py-3">
              <span className="mr-auto text-[12.5px] text-muted">Discard unsaved changes?</span>
              <Button variant="ghost" onClick={() => setAskDiscard(false)} autoFocus>
                Keep editing
              </Button>
              <Button variant="danger" onClick={onClose}>
                Discard
              </Button>
            </div>
          ) : (
            footer && <div className="flex items-center justify-end gap-2 border-t border-line/60 px-5 py-3">{footer}</div>
          )}
        </RDialog.Content>
      </RDialog.Portal>
    </RDialog.Root>
  );
}

export function Field({ label, hint, children, className }: { label: ReactNode; hint?: ReactNode; children: ReactNode; className?: string }) {
  return (
    <label className={cx("flex flex-col gap-1.5", className)}>
      <span className="text-[12px] font-medium text-muted">{label}</span>
      {children}
      {hint && <span className="text-[11.5px] text-faint">{hint}</span>}
    </label>
  );
}

export function Badge({ children, className, style }: { children: ReactNode; className?: string; style?: React.CSSProperties }) {
  return (
    <span style={style} className={cx("inline-flex items-center rounded px-1.5 py-px text-[11px] font-semibold tabular-nums", className)}>
      {children}
    </span>
  );
}

export function Banner({ tone = "warning", children, action }: { tone?: "warning" | "danger" | "info"; children: ReactNode; action?: ReactNode }) {
  const tones = {
    warning: "border-warning/30 bg-warning/10 text-warning",
    danger: "border-danger/30 bg-danger/10 text-danger",
    info: "border-info/30 bg-info/10 text-info",
  };
  return (
    <div className={cx("mx-3 mt-2 flex items-center gap-3 rounded-lg border px-3 py-2 text-[12.5px]", tones[tone])}>
      <div className="min-w-0 flex-1">{children}</div>
      {action}
    </div>
  );
}

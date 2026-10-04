// Single-line input that highlights {{variables}} and autocompletes them.
// The input text is transparent; a backdrop with identical metrics draws the colors.
import type { DynamicVarInfo } from "../bindings/DynamicVarInfo";
import { forwardRef, useEffect, useImperativeHandle, useLayoutEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { segments } from "../lib/vars";
import { useDynamicCatalog, useVariableNames, useWorkspace } from "../store/workspace";
import { cx } from "./ui";

export interface VarInputProps {
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
  className?: string;
  onEnter?: () => void;
  onBlur?: () => void;
  autoFocus?: boolean;
  /** Extra suggestions shown when not typing a variable (e.g. header names). */
  suggestions?: string[];
  mono?: boolean;
  ariaLabel?: string;
  secret?: boolean;
}

export const VarInput = forwardRef<HTMLInputElement, VarInputProps>(function VarInput(
  { value, onChange, placeholder, className, onEnter, onBlur, autoFocus, suggestions, mono = true, ariaLabel, secret },
  ref,
) {
  const input = useRef<HTMLInputElement>(null);
  const backdrop = useRef<HTMLDivElement>(null);
  const list = useRef<HTMLDivElement>(null);
  useImperativeHandle(ref, () => input.current as HTMLInputElement);
  const { names, known } = useVariableNames();
  const catalog = useDynamicCatalog();
  const [menu, setMenu] = useState<{ items: string[]; index: number; from: number; kind: "var" | "plain" } | null>(null);
  const [hover, setHover] = useState<{ name: string; left: number } | null>(null);
  const variables = useWorkspace((s) => s.variables);

  // The input sits on top of the colored backdrop, so find the variable under the pointer by geometry.
  const onMouseMove = (e: React.MouseEvent) => {
    if (!backdrop.current || !value.includes("{{")) return setHover(null);
    const spans = backdrop.current.querySelectorAll<HTMLElement>("[data-var]");
    for (const span of spans) {
      const r = span.getBoundingClientRect();
      if (e.clientX >= r.left && e.clientX <= r.right && e.clientY >= r.top && e.clientY <= r.bottom) {
        const host = backdrop.current.getBoundingClientRect();
        const name = span.dataset.var!;
        if (hover?.name !== name) setHover({ name, left: r.left - host.left });
        return;
      }
    }
    if (hover) setHover(null);
  };

  const parts = useMemo(() => segments(value), [value]);
  const masked = secret && value.length > 0;

  const syncScroll = () => {
    if (backdrop.current && input.current) backdrop.current.scrollLeft = input.current.scrollLeft;
  };
  // Again once the backdrop holds the new text: during onChange it can still be too short to scroll that far.
  useLayoutEffect(syncScroll, [value]);

  useEffect(() => {
    list.current?.children[menu?.index ?? 0]?.scrollIntoView({ block: "nearest" });
  }, [menu?.index]);

  const updateMenu = (text: string, caret: number) => {
    const before = text.slice(0, caret);
    const open = before.lastIndexOf("{{");
    if (open >= 0 && !before.slice(open).includes("}}")) {
      const prefix = before.slice(open + 2).trimStart();
      if (/^[\w$.-]*$/.test(prefix)) {
        const items = names.filter((n) => n.toLowerCase().includes(prefix.toLowerCase())).slice(0, 50);
        setMenu(items.length ? { items, index: 0, from: open, kind: "var" } : null);
        return;
      }
    }
    if (suggestions && text.length > 0 && caret === text.length) {
      const items = suggestions.filter((s) => s.toLowerCase().startsWith(text.toLowerCase()) && s !== text).slice(0, 12);
      setMenu(items.length ? { items, index: 0, from: 0, kind: "plain" } : null);
      return;
    }
    setMenu(null);
  };

  const apply = (item: string) => {
    if (!menu || !input.current) return;
    const caret = input.current.selectionStart ?? value.length;
    let next: string;
    let pos: number;
    if (menu.kind === "var") {
      const after = value.slice(caret);
      const closing = after.startsWith("}}") ? after.slice(2) : after;
      next = `${value.slice(0, menu.from)}{{${item}}}${closing}`;
      pos = menu.from + item.length + 4;
    } else {
      next = item;
      pos = item.length;
    }
    onChange(next);
    setMenu(null);
    requestAnimationFrame(() => {
      input.current?.setSelectionRange(pos, pos);
      syncScroll();
    });
  };

  const onKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    // Keys during IME composition belong to the IME (Enter commits the text). WebKit reports keyCode 229.
    if (e.nativeEvent.isComposing || e.keyCode === 229) return;
    if (menu) {
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        const d = e.key === "ArrowDown" ? 1 : -1;
        setMenu({ ...menu, index: (menu.index + d + menu.items.length) % menu.items.length });
        return;
      }
      if (e.key === "Enter" || e.key === "Tab") {
        e.preventDefault();
        apply(menu.items[menu.index]);
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        setMenu(null);
        return;
      }
      // The caret leaves the spot the menu was opened for; applying later would splice the wrong text.
      if (e.key === "ArrowLeft" || e.key === "ArrowRight" || e.key === "Home" || e.key === "End") setMenu(null);
    }
    // ⌘/Ctrl+Enter is the app-wide send shortcut; handling it here as well would send twice.
    if (e.key === "Enter" && !e.metaKey && !e.ctrlKey) onEnter?.();
  };

  return (
    <div className={cx("relative min-w-0", className)} onMouseMove={onMouseMove} onMouseLeave={() => setHover(null)}>
      <div
        ref={backdrop}
        aria-hidden
        className={cx(
          "pointer-events-none absolute inset-0 overflow-hidden whitespace-pre px-2 leading-[28px] text-fg",
          mono ? "font-mono text-[12.5px]" : "text-[13px]",
        )}
      >
        {masked
          ? "•".repeat(Math.min(value.length, 40))
          : parts.map((p, i) =>
              p.variable ? (
                <span key={i} data-var={p.variable} className={p.variable.startsWith("$") || known.has(p.variable) ? "zv-var" : "zv-var-missing"}>
                  {p.text}
                </span>
              ) : (
                <span key={i}>{p.text}</span>
              ),
            )}
      </div>
      <input
        ref={input}
        value={value}
        aria-label={ariaLabel ?? placeholder}
        placeholder={placeholder}
        autoFocus={autoFocus}
        spellCheck={false}
        autoComplete="off"
        autoCorrect="off"
        autoCapitalize="off"
        onChange={(e) => {
          onChange(e.target.value);
          updateMenu(e.target.value, e.target.selectionStart ?? e.target.value.length);
          syncScroll();
        }}
        onScroll={syncScroll}
        onKeyDown={onKeyDown}
        onMouseDown={() => setMenu(null)}
        // Lets a surrounding Modal leave Escape to this menu instead of closing.
        data-autocomplete-open={menu ? "" : undefined}
        onKeyUp={syncScroll}
        onBlur={() => {
          setTimeout(() => setMenu(null), 120);
          onBlur?.();
        }}
        className={cx(
          "relative h-7 w-full bg-transparent px-2 text-transparent caret-[var(--text)] outline-none placeholder:text-faint",
          mono ? "font-mono text-[12.5px]" : "text-[13px]",
        )}
      />
      {hover && !menu && (
        <VariableHint name={hover.name} left={hover.left} info={variables.find((v) => v.key === hover.name)} dynamic={catalog.get(hover.name.replace(/\(.*$/, "").trim())} />
      )}
      {menu && (
        <div ref={list} className="zv-pop absolute left-0 top-full z-50 mt-1 max-h-60 min-w-[220px] max-w-[min(28rem,100%)] overflow-auto rounded-lg border border-line bg-elev p-1 shadow-pop">
          {menu.items.map((item, i) => (
            <button
              key={item}
              onMouseDown={(e) => {
                e.preventDefault();
                apply(item);
              }}
              className={cx(
                "flex w-full items-center gap-2 rounded-md px-2 py-1 text-left font-mono text-[12px]",
                i === menu.index ? "bg-hover text-fg" : "text-muted",
              )}
            >
              {menu.kind === "var" && <span className="text-accent">{"{}"}</span>}
              {item}
              {catalog.get(item) && <span className="ml-auto truncate pl-3 font-sans text-[11px] text-faint">{catalog.get(item)!.example}</span>}
            </button>
          ))}
        </div>
      )}
    </div>
  );
});

const SOURCE_LABEL: Record<string, string> = { environment: "the active environment", workspace: "workspace variables", globals: "globals" };

function VariableHint({
  name,
  left,
  info,
  dynamic: about,
}: {
  name: string;
  left: number;
  info?: { value: string; source: string; secret: boolean; local?: boolean };
  dynamic?: DynamicVarInfo;
}) {
  const dynamic = name.startsWith("$");
  return (
    <div
      style={{ left: Math.max(0, left) }}
      className="zv-fade pointer-events-none absolute top-full z-50 mt-1 max-w-[360px] rounded-md border border-line bg-elev px-2.5 py-1.5 text-[12px] shadow-pop"
    >
      <div className="font-mono text-fg">{`{{${name}}}`}</div>
      {dynamic ? (
        about ? (
          <>
            <div className="text-muted">{about.description}</div>
            <div className="font-mono text-faint">e.g. {about.example}</div>
            {about.args && <div className="font-mono text-faint">{`{{${about.name}${about.args}}}`}</div>}
          </>
        ) : (
          <div className="text-muted">Dynamic value, generated on every send</div>
        )
      ) : info ? (
        <>
          <div className="break-all font-mono text-accent">{info.value === "" ? <i className="text-faint">empty</i> : info.value}</div>
          <div className="text-faint">
            From {SOURCE_LABEL[info.source] ?? info.source}
            {info.local && " · set by a script (this computer only)"}
          </div>
        </>
      ) : (
        <div className="text-danger">Not defined in the active environment or workspace</div>
      )}
    </div>
  );
}

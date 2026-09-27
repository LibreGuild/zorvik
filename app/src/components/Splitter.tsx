import { useRef } from "react";
import { cx } from "./ui";

/** Drag handle that reports a new size in pixels along its axis. */
export function Splitter({
  direction,
  onResize,
  onResizeEnd,
  subtle,
}: {
  direction: "horizontal" | "vertical";
  /** Invisible until hovered (used between the sidebar and the main card). */
  subtle?: boolean;
  onResize: (delta: number) => void;
  onResizeEnd?: () => void;
}) {
  const last = useRef(0);
  const horizontal = direction === "horizontal";
  return (
    <div
      role="separator"
      aria-orientation={horizontal ? "vertical" : "horizontal"}
      onPointerDown={(e) => {
        e.preventDefault();
        (e.target as HTMLElement).setPointerCapture(e.pointerId);
        last.current = horizontal ? e.clientX : e.clientY;
        document.body.style.cursor = horizontal ? "col-resize" : "row-resize";
      }}
      onPointerMove={(e) => {
        if (!(e.target as HTMLElement).hasPointerCapture(e.pointerId)) return;
        const pos = horizontal ? e.clientX : e.clientY;
        onResize(pos - last.current);
        last.current = pos;
      }}
      onPointerUp={(e) => (e.target as HTMLElement).releasePointerCapture(e.pointerId)}
      // Also runs on pointercancel, which skips pointerup and would leave the resize cursor stuck.
      onLostPointerCapture={() => {
        document.body.style.cursor = "";
        onResizeEnd?.();
      }}
      className={cx(
        "group relative z-10 shrink-0",
        horizontal ? "w-px cursor-col-resize" : "h-px cursor-row-resize",
        subtle ? (horizontal ? "w-2 bg-transparent" : "h-2 bg-transparent") : "bg-line/70",
      )}
    >
      <div
        className={cx(
          "absolute rounded-full transition-colors group-hover:bg-accent/40",
          horizontal ? "-left-[3px] -right-[3px] inset-y-0" : "-top-[3px] -bottom-[3px] inset-x-0",
        )}
      />
    </div>
  );
}

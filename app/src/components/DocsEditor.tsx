// Docs: Markdown notes about a request, folder, workspace, server or load test. Shown
// rendered (Preview) when there is text; Edit (the switch, a double-click or Enter on the
// preview) opens the text. Rendering makes React elements only (see lib/markdown), except ```mermaid
// blocks: Mermaid draws those (MermaidDiagram, strict mode).
import { memo, useMemo, useState, type ReactNode } from "react";
import { AlertOctagon, AlertTriangle, Image as ImageIcon, Info, Lightbulb, MessageSquareWarning, Pencil } from "lucide-react";
import { type Alert, type Block, type Inline, inlineImage, parseMarkdown, previewPart, safeHref } from "../lib/markdown";
import { openExternal } from "../lib/platform";
import { CodeEditor } from "./CodeEditor";
import { MermaidDiagram } from "./MermaidDiagram";
import { Button, cx, Segmented } from "./ui";

type Mode = "preview" | "edit";

function Link({ href, children }: { href: string | null; children: ReactNode }) {
  // Addresses a link may not open (javascript:, relative paths…) show as text.
  if (!href) return <>{children}</>;
  return (
    <a
      href={href}
      title={href}
      onClick={(e) => {
        e.preventDefault();
        void openExternal(href);
      }}
      onAuxClick={(e) => e.preventDefault()}
      className="text-accent underline decoration-accent/40 underline-offset-2 hover:decoration-accent"
    >
      {children}
    </a>
  );
}

function InlineView({ nodes }: { nodes: Inline[] }) {
  return (
    <>
      {nodes.map((n, i) => {
        switch (n.type) {
          case "text":
            return n.text;
          case "code":
            return (
              <code key={i} className="rounded bg-panel-2 px-1 py-px font-mono text-[0.92em] text-fg">
                {n.text}
              </code>
            );
          case "strong":
            return (
              <strong key={i} className="font-semibold text-fg">
                <InlineView nodes={n.children} />
              </strong>
            );
          case "em":
            return (
              <em key={i}>
                <InlineView nodes={n.children} />
              </em>
            );
          case "del":
            return (
              <del key={i} className="text-faint">
                <InlineView nodes={n.children} />
              </del>
            );
          case "link":
            return (
              <Link key={i} href={n.href}>
                <InlineView nodes={n.children} />
              </Link>
            );
          case "image":
            // Pictures in the text show in place; ones on the web aren't downloaded (a click opens them).
            if (inlineImage(n.src)) return <img key={i} src={n.src} alt={n.alt} className="my-1 inline-block max-w-full rounded" />;
            return (
              <Link key={i} href={safeHref(n.src)}>
                <span className="inline-flex items-baseline gap-1">
                  <ImageIcon size={12} className="self-center" aria-hidden />
                  {n.alt || "Image"}
                </span>
              </Link>
            );
          case "break":
            return <br key={i} />;
        }
      })}
    </>
  );
}

const ALERTS: Record<Alert, { label: string; tone: string; icon: ReactNode }> = {
  note: { label: "Note", tone: "var(--info)", icon: <Info size={13} /> },
  tip: { label: "Tip", tone: "var(--success)", icon: <Lightbulb size={13} /> },
  important: { label: "Important", tone: "var(--accent)", icon: <MessageSquareWarning size={13} /> },
  warning: { label: "Warning", tone: "var(--warning)", icon: <AlertTriangle size={13} /> },
  caution: { label: "Caution", tone: "var(--danger)", icon: <AlertOctagon size={13} /> },
};

const HEADING = [
  "",
  "mt-5 mb-2 border-b border-line pb-1 text-[18px] font-semibold tracking-tight",
  "mt-5 mb-2 border-b border-line pb-1 text-[15.5px] font-semibold",
  "mt-4 mb-1.5 text-[14px] font-semibold",
  "mt-4 mb-1 text-[13px] font-semibold",
  "mt-3 mb-1 text-[12.5px] font-semibold",
  "mt-3 mb-1 text-[12.5px] font-semibold text-muted",
];

function Blocks({ blocks, tight }: { blocks: Block[]; tight?: boolean }) {
  return (
    <>
      {blocks.map((b, i) => (
        <BlockView key={i} block={b} tight={tight} />
      ))}
    </>
  );
}

function BlockView({ block, tight }: { block: Block; tight?: boolean }) {
  switch (block.type) {
    case "heading": {
      const Tag = `h${block.level}` as "h1";
      return (
        <Tag className={cx(HEADING[block.level], "text-fg first:mt-0")}>
          <InlineView nodes={block.content} />
        </Tag>
      );
    }
    case "paragraph":
      return (
        <p className={cx(tight ? "my-0" : "my-2", "first:mt-0 last:mb-0")}>
          <InlineView nodes={block.content} />
        </p>
      );
    case "code":
      if (block.lang === "mermaid") return <MermaidDiagram code={block.code} />;
      return (
        <pre className="my-3 overflow-x-auto rounded-lg border border-line bg-panel px-3 py-2 font-mono text-[12px] leading-relaxed text-fg first:mt-0 last:mb-0">
          <code>{block.code}</code>
        </pre>
      );
    case "quote": {
      const alert = block.alert && ALERTS[block.alert];
      return (
        <blockquote
          className={cx("my-3 border-l-[3px] py-0.5 pl-3 first:mt-0 last:mb-0", !alert && "border-line-strong text-muted")}
          style={alert ? { borderColor: alert.tone } : undefined}
        >
          {alert && (
            <div className="mb-1 flex items-center gap-1.5 text-[12.5px] font-semibold" style={{ color: alert.tone }}>
              {alert.icon}
              {alert.label}
            </div>
          )}
          <Blocks blocks={block.blocks} />
        </blockquote>
      );
    }
    case "list": {
      // A task's box stands where the bullet would be.
      const items = block.items.map((it, i) => (
        <li key={i} className={cx(it.checked !== undefined && "-ml-5 flex list-none items-start gap-1.5", !block.tight && "my-1.5")}>
          {it.checked !== undefined && (
            <input type="checkbox" checked={it.checked} disabled readOnly aria-label={it.checked ? "Done" : "Not done"} className="mt-[3px] accent-[var(--accent)]" />
          )}
          <div className="min-w-0 flex-1">
            <Blocks blocks={it.blocks} tight={block.tight} />
          </div>
        </li>
      ));
      const cls = cx(tight ? "my-0.5" : "my-2", "pl-5 first:mt-0 last:mb-0 marker:text-faint", block.ordered ? "list-decimal" : "list-disc");
      return block.ordered ? (
        <ol className={cls} start={block.start === 1 ? undefined : block.start}>
          {items}
        </ol>
      ) : (
        <ul className={cls}>{items}</ul>
      );
    }
    case "table":
      return (
        <div className="my-3 overflow-x-auto rounded-lg border border-line first:mt-0 last:mb-0">
          <table className="w-full border-collapse text-left text-[12.5px]">
            <thead className="bg-panel-2">
              <tr>
                {block.head.map((h, i) => (
                  <th key={i} className="px-3 py-1.5 font-semibold text-fg" style={{ textAlign: block.align[i] ?? undefined }}>
                    <InlineView nodes={h} />
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {block.rows.map((r, i) => (
                <tr key={i} className="border-t border-line">
                  {r.map((cell, j) => (
                    <td key={j} className="px-3 py-1.5 align-top" style={{ textAlign: block.align[j] ?? undefined }}>
                      <InlineView nodes={cell} />
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      );
    case "rule":
      return <hr className="my-4 border-line" />;
  }
}

/** Markdown rendered as the app's text (no HTML from the source is used). Very long text is rendered in part. */
export const DocsView = memo(function DocsView({ source, className }: { source: string; className?: string }) {
  const { blocks, cut } = useMemo(() => {
    const part = previewPart(source);
    return { blocks: parseMarkdown(part.text), cut: part.cut };
  }, [source]);
  return (
    <div className={cx("selectable text-[13px] leading-[1.65] text-fg", className)} data-testid="docs-view">
      <Blocks blocks={blocks} />
      {cut && (
        <p className="mt-4 border-t border-line pt-2 text-[12.5px] text-muted" data-testid="docs-cut">
          The rest of these docs is too long to preview; switch to Edit to see it all.
        </p>
      )}
    </div>
  );
});

/**
 * Docs with a Preview / Edit switch. `pane` fills its parent (a Docs tab); `section` sits
 * in a settings column, with a bordered box and a fixed-height editor.
 */
export function DocsEditor({
  value,
  onChange,
  placeholder = "Notes in Markdown",
  emptyText = "No docs yet.",
  variant = "pane",
  autoFocus,
}: {
  value: string;
  onChange: (value: string) => void;
  /** Shown in the empty editor. */
  placeholder?: string;
  /** Shown in Preview when there is no text. */
  emptyText?: string;
  variant?: "pane" | "section";
  /** Put the cursor in the editor when it opens empty (a Docs tab someone just opened). */
  autoFocus?: boolean;
}) {
  const section = variant === "section";
  // Preview when there is something to read; an empty pane opens for typing.
  const [mode, setMode] = useState<Mode>(() => (value.trim() || section ? "preview" : "edit"));
  const [focus, setFocus] = useState(!!autoFocus);
  const edit = () => {
    setFocus(true);
    setMode("edit");
  };
  const empty = !value.trim();

  const toolbar = (
    <div className={cx("flex shrink-0 items-center gap-2", section ? "h-8 border-b border-line px-2" : "h-9 px-3")}>
      <span className="truncate text-[11.5px] text-faint">{mode === "edit" ? "Markdown: # heading, **bold**, `code`, - list, [link](https://…), ```mermaid diagrams" : ""}</span>
      <div className="flex-1" />
      <Segmented<Mode>
        items={[
          { id: "preview", label: "Preview" },
          { id: "edit", label: "Edit" },
        ]}
        value={mode}
        onChange={(m) => (m === "edit" ? edit() : setMode("preview"))}
      />
    </div>
  );

  const body =
    mode === "edit" ? (
      <div className={section ? "h-48" : "min-h-0 flex-1"}>
        <CodeEditor value={value} onChange={onChange} placeholder={placeholder} lineNumbers={false} autoFocus={focus} />
      </div>
    ) : empty ? (
      <div className={cx("flex items-center gap-3 text-[12.5px] text-muted", section ? "px-3 py-2.5" : "flex-1 flex-col justify-center p-6 text-center")}>
        <span>{emptyText}</span>
        <Button size="sm" variant="secondary" icon={<Pencil size={12} />} onClick={edit}>
          Write docs
        </Button>
      </div>
    ) : (
      <div
        role="region"
        aria-label="Docs preview. Press Enter to edit."
        tabIndex={0}
        onDoubleClick={(e) => {
          // Not when a link or a picture was double-clicked.
          if (!(e.target as HTMLElement).closest("a, img")) edit();
        }}
        onKeyDown={(e) => {
          if (e.key === "Enter" && e.target === e.currentTarget) {
            e.preventDefault();
            edit();
          }
        }}
        className={cx(
          "overflow-auto outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-accent-soft",
          section ? "max-h-80 px-3 py-2.5" : "min-h-0 flex-1 px-4 pb-6 pt-1",
        )}
        data-testid="docs-preview"
      >
        <DocsView source={value} className="max-w-[820px]" />
      </div>
    );

  return (
    <div className={cx("flex flex-col", section ? "rounded-lg border border-line" : "h-full min-h-0")} data-testid="docs-editor">
      {toolbar}
      {body}
    </div>
  );
}

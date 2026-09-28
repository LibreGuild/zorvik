// The Markdown lessons are written in (docs/academy.md): headings, paragraphs, lists,
// tables, code, callouts and diagrams. Everything becomes React elements; no HTML is
// injected. `{{name}}` in code shows as a variable, with its value while a lab runs.
import { Fragment, memo, useMemo, type ReactNode } from "react";
import { AlertTriangle, Info, Lightbulb, Sparkles } from "lucide-react";
import { openExternal } from "../../lib/platform";
import { cx, Tooltip } from "../ui";
import { DIAGRAMS } from "./Diagrams";

export type Block =
  | { type: "heading"; level: number; text: string }
  | { type: "paragraph"; text: string }
  | { type: "list"; ordered: boolean; items: string[] }
  | { type: "code"; lang: string; code: string }
  | { type: "callout"; kind: string; title: string; body: Block[] }
  | { type: "table"; head: string[]; rows: string[][] }
  | { type: "rule" };

const splitRow = (line: string) =>
  line
    .trim()
    .replace(/^\||\|$/g, "")
    .split("|")
    .map((c) => c.trim());

export function parseMarkdown(source: string): Block[] {
  const lines = source.replace(/\r\n/g, "\n").split("\n");
  const blocks: Block[] = [];
  let para: string[] = [];
  const flush = () => {
    if (para.length) blocks.push({ type: "paragraph", text: para.join(" ") });
    para = [];
  };
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    const trimmed = line.trim();
    const fence = /^```\s*([\w-]*)/.exec(trimmed);
    if (fence) {
      flush();
      const code: string[] = [];
      for (i++; i < lines.length && !lines[i].trim().startsWith("```"); i++) code.push(lines[i]);
      blocks.push({ type: "code", lang: fence[1].toLowerCase(), code: code.join("\n") });
      continue;
    }
    if (!trimmed) {
      flush();
      continue;
    }
    const heading = /^(#{1,4})\s+(.*)$/.exec(trimmed);
    if (heading) {
      flush();
      blocks.push({ type: "heading", level: heading[1].length, text: heading[2] });
      continue;
    }
    if (/^(-{3,}|\*{3,})$/.test(trimmed)) {
      flush();
      blocks.push({ type: "rule" });
      continue;
    }
    if (trimmed.startsWith(">")) {
      flush();
      const quote: string[] = [];
      for (; i < lines.length && lines[i].trim().startsWith(">"); i++) quote.push(lines[i].trim().replace(/^>\s?/, ""));
      i--;
      const head = /^\[!(\w+)\]\s*(.*)$/.exec(quote[0] ?? "");
      blocks.push({
        type: "callout",
        kind: head ? head[1].toLowerCase() : "note",
        title: head ? head[2] : "",
        body: parseMarkdown((head ? quote.slice(1) : quote).join("\n")),
      });
      continue;
    }
    if (trimmed.startsWith("|") && /^\s*\|?\s*:?-{2,}/.test(lines[i + 1] ?? "")) {
      flush();
      const head = splitRow(trimmed);
      const rows: string[][] = [];
      for (i += 2; i < lines.length && lines[i].trim().startsWith("|"); i++) rows.push(splitRow(lines[i]));
      i--;
      blocks.push({ type: "table", head, rows });
      continue;
    }
    const item = /^([-*]|\d+[.)])\s+(.*)$/.exec(trimmed);
    if (item) {
      flush();
      const ordered = /\d/.test(item[1]);
      const items: string[] = [item[2]];
      for (i++; i < lines.length; i++) {
        const next = lines[i];
        const m = /^\s*([-*]|\d+[.)])\s+(.*)$/.exec(next);
        if (m && /\d/.test(m[1]) === ordered) items.push(m[2]);
        else if (next.trim() && /^\s{2,}/.test(next)) items[items.length - 1] += ` ${next.trim()}`;
        else break;
      }
      i--;
      blocks.push({ type: "list", ordered, items });
      continue;
    }
    para.push(trimmed);
  }
  flush();
  return blocks;
}

const INLINE = /(`[^`]+`)|(\*\*(?:[^*]|\*(?!\*))+\*\*)|(\*[^*\s](?:[^*]*[^*\s])?\*)|(\[[^\]]+\]\([^)\s]+\))/g;

/** Inline text: `code`, **bold**, *italic*, [links](https://…). */
export function Inline({ text, vars }: { text: string; vars?: Record<string, string> }): ReactNode {
  const out: ReactNode[] = [];
  let last = 0;
  let k = 0;
  for (const m of text.matchAll(INLINE)) {
    if (m.index > last) out.push(<Fragment key={k++}>{text.slice(last, m.index)}</Fragment>);
    const t = m[0];
    if (m[1]) out.push(<Code key={k++} text={t.slice(1, -1)} vars={vars} />);
    else if (m[2])
      out.push(
        <strong key={k++} className="font-semibold text-fg">
          <Inline text={t.slice(2, -2)} vars={vars} />
        </strong>,
      );
    else if (m[3])
      out.push(
        <em key={k++}>
          <Inline text={t.slice(1, -1)} vars={vars} />
        </em>,
      );
    else {
      const link = /^\[([^\]]+)\]\(([^)\s]+)\)$/.exec(t)!;
      const href = link[2];
      out.push(
        /^https?:\/\//i.test(href) ? (
          <a
            key={k++}
            href={href}
            onClick={(e) => {
              e.preventDefault();
              void openExternal(href);
            }}
            className="text-accent underline decoration-accent/40 underline-offset-2 hover:decoration-accent"
          >
            {link[1]}
          </a>
        ) : (
          <Fragment key={k++}>{link[1]}</Fragment>
        ),
      );
    }
    last = m.index + t.length;
  }
  if (last < text.length) out.push(<Fragment key={k++}>{text.slice(last)}</Fragment>);
  return <>{out}</>;
}

/** Inline code; a lone `{{name}}` is a variable chip. */
function Code({ text, vars }: { text: string; vars?: Record<string, string> }) {
  const v = /^\{\{\s*([\w.-]+)\s*\}\}$/.exec(text);
  if (v) {
    const value = vars?.[v[1]];
    const chip = <code className="zv-var rounded px-1 py-px font-mono text-[0.9em]">{text}</code>;
    return value ? <Tooltip content={<span className="font-mono">{value}</span>}>{chip}</Tooltip> : chip;
  }
  return <code className="selectable rounded bg-panel-2 px-1 py-px font-mono text-[0.9em] text-fg">{text}</code>;
}

const CALLOUTS: Record<string, { icon: ReactNode; tone: string; label: string }> = {
  note: { icon: <Sparkles size={14} />, tone: "var(--info)", label: "Note" },
  tip: { icon: <Lightbulb size={14} />, tone: "var(--success)", label: "Tip" },
  warning: { icon: <AlertTriangle size={14} />, tone: "var(--warning)", label: "Watch out" },
  info: { icon: <Info size={14} />, tone: "var(--info)", label: "Info" },
};

function BlockView({ block, vars }: { block: Block; vars?: Record<string, string> }) {
  switch (block.type) {
    case "heading": {
      const cls = block.level <= 2 ? "mt-8 mb-2 text-[18px] font-semibold tracking-tight" : "mt-6 mb-1.5 text-[15px] font-semibold";
      return block.level <= 2 ? (
        <h2 className={cx(cls, "text-fg")}>
          <Inline text={block.text} vars={vars} />
        </h2>
      ) : (
        <h3 className={cx(cls, "text-fg")}>
          <Inline text={block.text} vars={vars} />
        </h3>
      );
    }
    case "paragraph":
      return (
        <p className="my-3 text-[14px] leading-[1.7] text-muted">
          <Inline text={block.text} vars={vars} />
        </p>
      );
    case "list": {
      const Tag = block.ordered ? "ol" : "ul";
      return (
        <Tag className={cx("my-3 flex flex-col gap-1.5 pl-5 text-[14px] leading-[1.65] text-muted", block.ordered ? "list-decimal" : "list-disc marker:text-accent")}>
          {block.items.map((it, i) => (
            <li key={i} className="pl-1">
              <Inline text={it} vars={vars} />
            </li>
          ))}
        </Tag>
      );
    }
    case "code": {
      const Diagram = DIAGRAMS[block.lang];
      if (Diagram) return <Diagram source={block.code} />;
      return (
        <pre className="selectable my-4 overflow-x-auto rounded-xl border border-line bg-panel px-4 py-3 font-mono text-[12.5px] leading-relaxed text-fg">
          {block.code}
        </pre>
      );
    }
    case "callout": {
      const c = CALLOUTS[block.kind] ?? CALLOUTS.note;
      return (
        <aside className="my-5 rounded-xl border px-4 py-3" style={{ borderColor: `color-mix(in srgb, ${c.tone} 35%, transparent)`, background: `color-mix(in srgb, ${c.tone} 8%, var(--elev))` }}>
          <div className="flex items-center gap-2 text-[13px] font-semibold" style={{ color: c.tone }}>
            {c.icon}
            <Inline text={block.title || c.label} vars={vars} />
          </div>
          <div className="[&>*:first-child]:mt-1.5 [&>*:last-child]:mb-0 [&_p]:text-[13.5px]">
            {block.body.map((b, i) => (
              <BlockView key={i} block={b} vars={vars} />
            ))}
          </div>
        </aside>
      );
    }
    case "table":
      return (
        <div className="my-4 overflow-x-auto rounded-xl border border-line">
          <table className="w-full border-collapse text-left text-[13px]">
            <thead className="bg-panel-2">
              <tr>
                {block.head.map((h, i) => (
                  <th key={i} className="px-3 py-2 font-semibold text-fg">
                    <Inline text={h} vars={vars} />
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {block.rows.map((r, i) => (
                <tr key={i} className="border-t border-line">
                  {r.map((cell, j) => (
                    <td key={j} className="px-3 py-2 align-top text-muted">
                      <Inline text={cell} vars={vars} />
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      );
    case "rule":
      return <hr className="my-6 border-line" />;
  }
}

export const Markdown = memo(function Markdown({ source, vars, className }: { source: string; vars?: Record<string, string>; className?: string }) {
  const blocks = useMemo(() => parseMarkdown(source), [source]);
  return (
    <div className={className}>
      {blocks.map((b, i) => (
        <BlockView key={i} block={b} vars={vars} />
      ))}
    </div>
  );
});

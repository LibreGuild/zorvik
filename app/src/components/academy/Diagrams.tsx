// Lesson diagrams drawn from a few lines of text (see docs/academy.md): sequence, flow,
// layers and anatomy. Plain HTML and CSS, so they follow the theme and wrap long labels;
// they play in step by step when they scroll into view.
import { Fragment, useEffect, useRef, useState, type CSSProperties } from "react";
import { RotateCcw } from "lucide-react";
import { cx } from "../ui";

/** True once the element has been on screen (then stays true). */
function useSeen<T extends Element>() {
  const ref = useRef<T>(null);
  const [seen, setSeen] = useState(false);
  useEffect(() => {
    const el = ref.current;
    if (!el || seen) return;
    if (typeof IntersectionObserver === "undefined") {
      setSeen(true);
      return;
    }
    const io = new IntersectionObserver((entries) => entries.some((e) => e.isIntersecting) && setSeen(true), { threshold: 0.35 });
    io.observe(el);
    return () => io.disconnect();
  }, [seen]);
  return { ref, seen };
}

/** Delay of the i-th part of an animation. */
const stagger = (i: number, seen: boolean, ms = 320): CSSProperties =>
  seen ? { animationDelay: `${i * ms}ms` } : { opacity: 0 };

function Frame({ children, onReplay, label }: { children: React.ReactNode; onReplay?: () => void; label: string }) {
  return (
    <figure className="group relative my-5 overflow-x-auto rounded-xl border border-line bg-panel/60 px-4 pb-4 pt-5" aria-label={label}>
      {children}
      {onReplay && (
        <button
          onClick={onReplay}
          aria-label="Replay the diagram"
          title="Replay"
          className="absolute right-2 top-2 rounded-md p-1 text-faint opacity-0 transition-opacity hover:bg-hover hover:text-fg focus-visible:opacity-100 group-hover:opacity-100"
        >
          <RotateCcw size={13} />
        </button>
      )}
    </figure>
  );
}

// ---- sequence ------------------------------------------------------------------

type SeqRow = { kind: "message"; from: number; to: number; text: string; reply: boolean } | { kind: "note"; from: number; to: number; text: string };

export function parseSequence(source: string): { participants: string[]; rows: SeqRow[] } {
  const participants: string[] = [];
  const index = (name: string) => {
    const n = name.trim();
    let i = participants.findIndex((p) => p.toLowerCase() === n.toLowerCase());
    if (i < 0) {
      participants.push(n);
      i = participants.length - 1;
    }
    return i;
  };
  const rows: SeqRow[] = [];
  for (const raw of source.split("\n")) {
    const line = raw.trim();
    if (!line || line.startsWith("#")) continue;
    const p = /^participants\s*:\s*(.+)$/i.exec(line);
    if (p) {
      p[1].split(",").forEach((n) => n.trim() && index(n));
      continue;
    }
    const note = /^note\s+over\s+([^:]+):\s*(.*)$/i.exec(line);
    if (note) {
      const who = note[1].split(",").map(index);
      rows.push({ kind: "note", from: Math.min(...who), to: Math.max(...who), text: note[2] });
      continue;
    }
    const m = /^(.+?)\s*(-->|->)\s*(.+?)\s*:\s*(.*)$/.exec(line);
    if (m) rows.push({ kind: "message", from: index(m[1]), to: index(m[3]), text: m[4], reply: m[2] === "-->" });
  }
  return { participants, rows };
}

function Sequence({ source }: { source: string }) {
  const { participants, rows } = parseSequence(source);
  const [run, setRun] = useState(0);
  const { ref, seen } = useSeen<HTMLDivElement>();
  const n = Math.max(participants.length, 1);
  const col = (i: number) => `${i + 1} / span 1`;
  return (
    <Frame label="Sequence diagram" onReplay={() => setRun((r) => r + 1)}>
      <div ref={ref} key={run} className="relative min-w-[420px]" style={{ display: "grid", gridTemplateColumns: `repeat(${n}, minmax(0, 1fr))`, rowGap: 6 }}>
        {/* Lifelines */}
        {participants.map((_, i) => (
          <div key={`l${i}`} aria-hidden className="pointer-events-none flex justify-center" style={{ gridColumn: col(i), gridRow: `1 / span ${rows.length + 2}` }}>
            <div className="h-full w-px border-l border-dashed border-line-strong" />
          </div>
        ))}
        {participants.map((p, i) => (
          <div key={`p${i}`} className="relative z-[1] flex justify-center px-1" style={{ gridColumn: col(i), gridRow: 1 }}>
            <div className="rounded-lg border border-line-strong bg-elev px-3 py-1.5 text-center text-[12.5px] font-semibold text-fg shadow-sm">{p}</div>
          </div>
        ))}
        {rows.map((r, i) => {
          const lo = Math.min(r.from, r.to);
          const span = Math.abs(r.to - r.from) + 1;
          const style = { gridColumn: `${lo + 1} / span ${span}`, gridRow: i + 2 };
          if (r.kind === "note") {
            return (
              <div key={i} className={cx("relative z-[1] flex justify-center px-2 py-1", seen && "zv-seq-in")} style={{ ...style, ...stagger(i, seen) }}>
                <div className="max-w-full rounded-md border border-warning/40 bg-[color-mix(in_srgb,var(--warning)_12%,var(--elev))] px-2.5 py-1 text-center text-[12px] text-fg">
                  {r.text}
                </div>
              </div>
            );
          }
          const self = r.from === r.to;
          // The arrow runs between the centers of the first and last column it spans.
          const inset = `${50 / span}%`;
          const right = r.to > r.from;
          const color = r.reply ? "var(--success)" : "var(--accent)";
          return (
            <div
              key={i}
              className={cx("relative z-[1] pb-2 pt-1", seen && "zv-seq-in")}
              style={{ ...style, ...stagger(i, seen), paddingLeft: self ? "50%" : inset, paddingRight: self ? "8%" : inset }}
            >
              <div className="pb-1 text-center text-[12px] leading-snug text-fg">
                <span className="rounded bg-panel/90 px-1 font-mono text-[11.5px]">{r.text}</span>
              </div>
              <div className="relative h-0" style={{ borderTop: `2px ${r.reply ? "dashed" : "solid"} ${color}` }}>
                <span
                  aria-hidden
                  className="absolute -top-[6px] h-0 w-0"
                  style={
                    right || self
                      ? { right: -2, borderTop: "5px solid transparent", borderBottom: "5px solid transparent", borderLeft: `8px solid ${color}` }
                      : { left: -2, borderTop: "5px solid transparent", borderBottom: "5px solid transparent", borderRight: `8px solid ${color}` }
                  }
                />
              </div>
            </div>
          );
        })}
        <div style={{ gridColumn: "1 / -1", gridRow: rows.length + 2, height: 8 }} />
      </div>
    </Frame>
  );
}

// ---- flow ------------------------------------------------------------------------

export function parseFlow(source: string): { nodes: string[]; labels: string[] }[] {
  return source
    .split("\n")
    .map((l) => l.trim())
    .filter((l) => l && !l.startsWith("#"))
    .map((line) => {
      const nodes: string[] = [];
      const labels: string[] = [];
      const re = /\s*-(?:\[([^\]]*)\]-)?>\s*/g;
      let last = 0;
      for (let m = re.exec(line); m; m = re.exec(line)) {
        nodes.push(line.slice(last, m.index).trim());
        labels.push(m[1] ?? "");
        last = m.index + m[0].length;
      }
      nodes.push(line.slice(last).trim());
      return { nodes, labels };
    });
}

function Flow({ source }: { source: string }) {
  const rows = parseFlow(source);
  const [run, setRun] = useState(0);
  const { ref, seen } = useSeen<HTMLDivElement>();
  let k = 0;
  return (
    <Frame label="Flow diagram" onReplay={() => setRun((r) => r + 1)}>
      <div ref={ref} key={run} className="flex flex-col gap-3">
        {rows.map((row, r) => (
          <div key={r} className="flex flex-wrap items-center gap-y-2">
            {row.nodes.map((node, i) => (
              <Fragment key={i}>
                {i > 0 && (
                  <div className={cx("flex min-w-[44px] flex-col items-center px-1.5", seen && "zv-seq-in")} style={stagger(k++, seen, 180)}>
                    {row.labels[i - 1] && <span className="mb-0.5 text-[11px] font-medium text-accent">{row.labels[i - 1]}</span>}
                    <span className="flex w-full items-center text-accent" aria-hidden>
                      <span className="h-[2px] flex-1 bg-current" />
                      <span className="h-0 w-0 border-y-[5px] border-l-[8px] border-y-transparent border-l-current" />
                    </span>
                  </div>
                )}
                <div
                  className={cx("rounded-lg border border-line-strong bg-elev px-3 py-1.5 text-[12.5px] font-medium text-fg shadow-sm", seen && "zv-seq-in")}
                  style={stagger(k++, seen, 180)}
                >
                  {node}
                </div>
              </Fragment>
            ))}
          </div>
        ))}
      </div>
    </Frame>
  );
}

// ---- layers ------------------------------------------------------------------------

function Layers({ source }: { source: string }) {
  const rows = source
    .split("\n")
    .map((l) => l.trim())
    .filter(Boolean)
    .map((l) => {
      const [name, items = ""] = l.split("|");
      return { name: name.trim(), items: items.split(",").map((s) => s.trim()).filter(Boolean) };
    });
  const { ref, seen } = useSeen<HTMLDivElement>();
  return (
    <Frame label="Layers diagram">
      <div ref={ref} className="flex flex-col gap-1.5">
        {rows.map((r, i) => {
          const mix = rows.length > 1 ? Math.round(10 + (i / (rows.length - 1)) * 22) : 16;
          return (
            <div
              key={i}
              className={cx("flex flex-wrap items-center gap-2 rounded-lg border border-line px-3 py-2", seen && "zv-seq-in")}
              style={{ background: `color-mix(in srgb, var(--accent) ${mix}%, var(--elev))`, ...stagger(i, seen, 150) }}
            >
              <span className="w-[120px] shrink-0 text-[12.5px] font-semibold text-fg">{r.name}</span>
              {r.items.map((it) => (
                <span key={it} className="rounded-md bg-elev/80 px-2 py-0.5 font-mono text-[11.5px] text-fg">
                  {it}
                </span>
              ))}
            </div>
          );
        })}
      </div>
    </Frame>
  );
}

// ---- anatomy ------------------------------------------------------------------------

const ANATOMY_COLORS = ["var(--accent)", "var(--info)", "var(--success)", "var(--m-patch)", "var(--warning)", "var(--m-options)"];

function Anatomy({ source }: { source: string }) {
  const rows = source
    .split("\n")
    .filter((l) => l.trim())
    .map((l) => {
      const at = l.lastIndexOf(" | ");
      return at < 0 ? { code: l, note: "" } : { code: l.slice(0, at), note: l.slice(at + 3).trim() };
    });
  const { ref, seen } = useSeen<HTMLDivElement>();
  return (
    <Frame label="Anatomy diagram">
      <div ref={ref} className="flex flex-col gap-1">
        {rows.map((r, i) => {
          const color = ANATOMY_COLORS[i % ANATOMY_COLORS.length];
          return (
            <div key={i} className={cx("grid items-center gap-x-3 gap-y-0.5 sm:grid-cols-[minmax(0,1.3fr)_minmax(0,1fr)]", seen && "zv-seq-in")} style={stagger(i, seen, 200)}>
              <code className="selectable block overflow-x-auto whitespace-pre rounded-md border-l-[3px] bg-elev px-2.5 py-1 font-mono text-[12px] text-fg" style={{ borderLeftColor: color }}>
                {r.code || " "}
              </code>
              {r.note && (
                <span className="flex items-center gap-1.5 text-[12px] font-medium" style={{ color }}>
                  <span aria-hidden className="hidden sm:inline">←</span>
                  {r.note}
                </span>
              )}
            </div>
          );
        })}
      </div>
    </Frame>
  );
}

export const DIAGRAMS: Record<string, (props: { source: string }) => React.ReactNode> = {
  sequence: Sequence,
  flow: Flow,
  layers: Layers,
  anatomy: Anatomy,
};

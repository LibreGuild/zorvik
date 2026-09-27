// The in-app docs (a tab): what Zorvik can do, feature by feature, with illustrations.
import { Fragment, useEffect, useRef } from "react";
import { ArrowRight, Check, Lightbulb, Rocket } from "lucide-react";
import { isMac } from "../../lib/platform";
import type { ToolTab } from "../../store/tabs";
import { useWorkspace } from "../../store/workspace";
import { cx, Kbd } from "../ui";
import { QUICK_START, SHORTCUTS, TOPICS } from "./content";
import { docImage, TOPIC_ICONS } from "./media";

/** `[mod+N]` → a keycap, `code` → monospace. */
export function Rich({ text }: { text: string }) {
  const parts = text.split(/(\[[^\]]+\]|`[^`]+`)/g);
  return (
    <>
      {parts.map((part, i) => {
        if (part.startsWith("[") && part.endsWith("]")) return <Keys key={i} combo={part.slice(1, -1)} />;
        if (part.startsWith("`") && part.endsWith("`")) {
          return (
            <code key={i} className="rounded bg-panel-2 px-1 py-px font-mono text-[0.9em] text-fg">
              {part.slice(1, -1)}
            </code>
          );
        }
        return <Fragment key={i}>{part}</Fragment>;
      })}
    </>
  );
}

function Keys({ combo }: { combo: string }) {
  // "mod++" is ⌘ and +: split on the + signs that join keys.
  const keys = combo.split(/\+(?=.)/).map((k) => (k === "mod" ? (isMac ? "⌘" : "Ctrl") : k === "Enter" && isMac ? "↩" : k));
  return (
    <span className="inline-flex items-center gap-0.5 align-middle">
      {keys.map((k, i) => (
        <Kbd key={i}>{k}</Kbd>
      ))}
    </span>
  );
}

export function DocsView({ tab }: { tab: ToolTab }) {
  const scroller = useRef<HTMLDivElement>(null);
  const version = useWorkspace((s) => s.appInfo?.version);
  const topic = typeof tab.state.topic === "string" ? tab.state.topic : null;
  const at = tab.state.at;

  // Jump to the topic picked in the sidebar (again when the same one is picked twice).
  useEffect(() => {
    const el = topic ? scroller.current?.querySelector<HTMLElement>(`#doc-${topic}`) : null;
    if (el) el.scrollIntoView({ behavior: "smooth", block: "start" });
    else if (!topic) scroller.current?.scrollTo({ top: 0, behavior: "smooth" });
  }, [topic, at]);

  const go = (id: string) => scroller.current?.querySelector<HTMLElement>(`#doc-${id}`)?.scrollIntoView({ behavior: "smooth", block: "start" });

  return (
    <div ref={scroller} className="h-full overflow-auto bg-bg" data-testid="docs">
      <div className="mx-auto max-w-[1080px] px-8 pb-24 pt-8">
        {/* Hero */}
        <header className="grid items-center gap-8 md:grid-cols-[1.05fr_1fr]">
          <div>
            <div className="text-[11px] font-semibold uppercase tracking-[0.14em] text-accent">Docs</div>
            <h1 className="mt-2 text-[30px] font-semibold leading-tight tracking-tight text-fg">Everything Zorvik can do</h1>
            <p className="mt-3 text-[14.5px] leading-relaxed text-muted">
              One workbench for every wire: build and send requests, test and mock APIs, load test them, and inspect the network, all from a
              folder of files you keep in Git.
            </p>
            <div className="mt-6 rounded-xl border border-line bg-elev p-4">
              <div className="mb-3 flex items-center gap-2 text-[12.5px] font-semibold text-fg">
                <Rocket size={14} className="text-accent" /> Start in a minute
              </div>
              <ol className="flex flex-col gap-2.5">
                {QUICK_START.map((step, i) => (
                  <li key={i} className="flex gap-3 text-[13px] leading-relaxed text-muted">
                    <span className="mt-px flex h-5 w-5 shrink-0 items-center justify-center rounded-full bg-accent-soft text-[11px] font-semibold text-accent">
                      {i + 1}
                    </span>
                    <span>
                      <Rich text={step} />
                    </span>
                  </li>
                ))}
              </ol>
            </div>
          </div>
          <img src={docImage("docs-hero")} alt="" className="w-full select-none" draggable={false} />
        </header>

        {/* Feature grid */}
        <h2 className="mb-4 mt-14 text-[13px] font-semibold uppercase tracking-[0.12em] text-faint">Explore</h2>
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
          {TOPICS.map((t) => (
            <button
              key={t.id}
              onClick={() => go(t.id)}
              className="group flex flex-col overflow-hidden rounded-xl border border-line bg-elev text-left transition-colors hover:border-accent"
              data-testid={`docs-card-${t.id}`}
            >
              <div className="flex h-28 items-center justify-center bg-panel-2/60 px-4">
                <img src={docImage(t.image)} alt="" className="h-full w-auto select-none object-contain transition-transform group-hover:scale-[1.03]" draggable={false} />
              </div>
              <div className="flex flex-1 flex-col gap-1 p-3.5">
                <div className="flex items-center gap-2 text-[13.5px] font-semibold text-fg">
                  <span className="text-accent">{TOPIC_ICONS[t.id]?.(14)}</span>
                  {t.title}
                </div>
                <div className="text-[12.5px] leading-snug text-muted">
                  <Rich text={t.tagline} />
                </div>
              </div>
            </button>
          ))}
        </div>

        {/* Topics */}
        {TOPICS.map((t, i) => (
          <section key={t.id} id={`doc-${t.id}`} className="scroll-mt-6 border-t border-line/60 pt-12 mt-12" data-testid={`doc-${t.id}`}>
            <div className={cx("grid items-start gap-10 md:grid-cols-[1fr_1.1fr]", i % 2 === 1 && "md:[&>*:first-child]:order-2")}>
              <img src={docImage(t.image)} alt="" className="mx-auto w-full max-w-[460px] select-none md:sticky md:top-6" draggable={false} />
              <div>
                <div className="flex items-center gap-2 text-[11px] font-semibold uppercase tracking-[0.14em] text-accent">
                  {TOPIC_ICONS[t.id]?.(13)} {t.group}
                </div>
                <h2 className="mt-2 text-[22px] font-semibold tracking-tight text-fg">{t.title}</h2>
                <p className="mt-1.5 text-[15px] leading-snug text-fg/80">
                  <Rich text={t.tagline} />
                </p>
                <p className="mt-4 text-[13.5px] leading-relaxed text-muted">
                  <Rich text={t.intro} />
                </p>
                <ul className="mt-5 flex flex-col gap-2">
                  {t.features.map((f, j) => (
                    <li key={j} className="flex gap-2.5 text-[13px] leading-relaxed text-fg/90">
                      <Check size={14} className="mt-[3px] shrink-0 text-success" />
                      <span>
                        <Rich text={f} />
                      </span>
                    </li>
                  ))}
                </ul>
                <div className="mt-5 rounded-xl border border-accent/25 bg-accent-soft px-4 py-3">
                  <div className="mb-1.5 flex items-center gap-1.5 text-[11.5px] font-semibold uppercase tracking-wide text-accent">
                    <ArrowRight size={13} /> Try it
                  </div>
                  {t.tryIt.map((step, j) => (
                    <p key={j} className="text-[13px] leading-relaxed text-fg/90">
                      <Rich text={step} />
                    </p>
                  ))}
                </div>
                {t.tip && (
                  <div className="mt-3 flex gap-2.5 rounded-xl border border-line bg-elev px-4 py-3 text-[12.5px] leading-relaxed text-muted">
                    <Lightbulb size={14} className="mt-[3px] shrink-0 text-warning" />
                    <span>
                      <Rich text={t.tip} />
                    </span>
                  </div>
                )}
              </div>
            </div>
          </section>
        ))}

        {/* Shortcuts */}
        <section id="doc-shortcuts" className="mt-12 scroll-mt-6 border-t border-line/60 pt-12">
          <h2 className="text-[22px] font-semibold tracking-tight text-fg">Keyboard shortcuts</h2>
          <p className="mt-1.5 text-[13.5px] text-muted">The ones worth remembering.</p>
          <div className="mt-5 grid grid-cols-1 gap-2 sm:grid-cols-2 lg:grid-cols-3">
            {SHORTCUTS.map(([combo, label]) => (
              <div key={combo} className="flex items-center justify-between rounded-lg border border-line bg-elev px-3 py-2 text-[13px] text-fg">
                {label}
                <Keys combo={combo} />
              </div>
            ))}
          </div>
        </section>

        <footer className="mt-16 text-center text-[12px] text-faint">
          Zorvik {version} · open source by Libre Guild · more in the <Rich text="`docs/`" /> folder of the repository
        </footer>
      </div>
    </div>
  );
}

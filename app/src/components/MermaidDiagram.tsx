// A Mermaid diagram in docs (a ```mermaid code block). Mermaid loads on first use. Docs come
// from workspace files (Git): it runs with securityLevel "strict" (labels sanitized, no clicks,
// no scripts), and an unreadable diagram shows its text with Mermaid's error.
import { memo, useEffect, useRef, useState } from "react";
import { cx } from "./ui";

type Mermaid = typeof import("mermaid").default;

let loading: Promise<Mermaid> | null = null;
let themed: "dark" | "default" | null = null;
let counter = 0;

function mermaid(): Promise<Mermaid> {
  loading ??= import("mermaid").then((m) => m.default);
  return loading;
}

/** Diagrams follow the app's light or dark theme. */
function theme(): "dark" | "default" {
  return document.documentElement.classList.contains("dark") ? "dark" : "default";
}

async function render(code: string): Promise<string> {
  const m = await mermaid();
  const wanted = theme();
  if (themed !== wanted) {
    m.initialize({ startOnLoad: false, securityLevel: "strict", theme: wanted, fontFamily: "inherit" });
    themed = wanted;
  }
  counter += 1;
  const { svg } = await m.render(`zv-mermaid-${counter}`, code);
  return svg;
}

export const MermaidDiagram = memo(function MermaidDiagram({ code }: { code: string }) {
  const host = useRef<HTMLDivElement>(null);
  const [error, setError] = useState<string | null>(null);
  const [dark, setDark] = useState(theme);

  // Drawn again when the app switches between light and dark.
  useEffect(() => {
    const observer = new MutationObserver(() => setDark(theme()));
    observer.observe(document.documentElement, { attributes: true, attributeFilter: ["class"] });
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    let live = true;
    render(code).then(
      (svg) => {
        if (!live || !host.current) return;
        // Mermaid's own output in strict mode (sanitized by it), not text from the docs.
        host.current.innerHTML = svg;
        setError(null);
      },
      (e: unknown) => {
        if (!live) return;
        if (host.current) host.current.innerHTML = "";
        setError(e instanceof Error ? e.message : String(e));
      },
    );
    return () => {
      live = false;
    };
  }, [code, dark]);

  return (
    <figure className="my-3 first:mt-0 last:mb-0" data-testid="mermaid">
      <div ref={host} role="img" aria-label="Diagram" className={cx("flex justify-center overflow-x-auto [&_svg]:h-auto [&_svg]:max-w-full", error && "hidden")} />
      {error && (
        <div className="rounded-lg border border-warning/50 bg-panel px-3 py-2">
          <div className="mb-1 text-[11.5px] text-warning">This Mermaid diagram can't be drawn: {error.split("\n")[0]}</div>
          <pre className="overflow-x-auto font-mono text-[12px] leading-relaxed text-fg">
            <code>{code}</code>
          </pre>
        </div>
      )}
    </figure>
  );
});

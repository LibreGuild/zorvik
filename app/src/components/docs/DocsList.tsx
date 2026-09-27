// Sidebar: the docs' topics; picking one opens the Docs tab at it.
import { BookOpen, Keyboard } from "lucide-react";
import { openDocs } from "../../store/tabs";
import { TOPICS } from "./content";
import { TOPIC_ICONS } from "./media";

function Item({ icon, label, hint, onClick, testId }: { icon: React.ReactNode; label: string; hint?: string; onClick: () => void; testId?: string }) {
  return (
    <button onClick={onClick} className="flex w-full items-center gap-2.5 rounded-lg px-2 py-1.5 text-left hover:bg-hover" data-testid={testId}>
      <span className="flex h-6 w-6 shrink-0 items-center justify-center rounded-md bg-panel-2 text-accent">{icon}</span>
      <span className="min-w-0">
        <span className="block truncate text-[12.5px] text-fg">{label}</span>
        {hint && <span className="block truncate text-[11px] text-faint">{hint}</span>}
      </span>
    </button>
  );
}

export function DocsList() {
  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-auto px-2 py-1" data-testid="docs-list">
      <Item icon={<BookOpen size={13} />} label="Start here" hint="Overview and quick start" onClick={() => openDocs(null)} testId="docs-start" />
      <div className="mb-1 mt-3 px-2 text-[10.5px] font-semibold uppercase tracking-wide text-faint">Features</div>
      {TOPICS.map((t) => (
        <Item key={t.id} icon={TOPIC_ICONS[t.id]?.(13)} label={t.title} onClick={() => openDocs(t.id)} testId={`docs-topic-${t.id}`} />
      ))}
      <div className="mb-1 mt-3 px-2 text-[10.5px] font-semibold uppercase tracking-wide text-faint">Reference</div>
      <Item icon={<Keyboard size={13} />} label="Keyboard shortcuts" onClick={() => openDocs("shortcuts")} />
    </div>
  );
}

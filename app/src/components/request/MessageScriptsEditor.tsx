// Message scripts of a WebSocket or Socket.IO request: a list (each runs on the messages that
// match its direction, event and pattern) and the selected script's settings and code.
import { useState } from "react";
import { ChevronDown, Plus, Trash2 } from "lucide-react";
import type { MatchKind } from "../../bindings/MatchKind";
import type { MessageDirection } from "../../bindings/MessageDirection";
import type { MessageScript } from "../../bindings/MessageScript";
import { patternProblem } from "../../lib/messageMatch";
import { useRegexProblem } from "../../lib/regexCheck";
import { CodeEditor } from "../CodeEditor";
import { Button, Checkbox, cx, IconButton, Menu, Select } from "../ui";
import { MATCH_KINDS } from "./ConnectionEditors";
import { compact, NEW_SCRIPT, scriptLabel } from "./messageScriptsModel";
import { MESSAGE_SNIPPETS } from "./scriptsModel";
import { pmCompletion } from "./ScriptsEditor";

const DIRECTIONS: { id: MessageDirection; label: string }[] = [
  { id: "received", label: "Received" },
  { id: "sent", label: "Sent" },
  { id: "both", label: "Sent or received" },
];

const MATCHES: { id: MatchKind; label: string; placeholder: string }[] = [
  { id: "any", label: "any message", placeholder: "" },
  ...MATCH_KINDS.filter((k) => k.id !== "any"),
];

const field = "h-7 rounded-md border border-line bg-panel-2 px-2 font-mono text-[12px] text-fg outline-none focus:border-accent";

export function MessageScriptsEditor({
  scripts,
  onChange,
  socketio,
}: {
  scripts: MessageScript[];
  onChange: (scripts: MessageScript[] | undefined) => void;
  socketio: boolean;
}) {
  const [at, setAt] = useState(0);
  const selected = Math.min(at, scripts.length - 1);
  const current = scripts[selected];
  const set = (list: MessageScript[]) => onChange(list.length ? list.map(compact) : undefined);
  const update = (patch: Partial<MessageScript>) => set(scripts.map((s, i) => (i === selected ? { ...s, ...patch } : s)));
  const add = () => {
    set([...scripts, NEW_SCRIPT]);
    setAt(scripts.length);
  };
  const regexProblem = useRegexProblem(current?.match === "regex" ? (current.pattern ?? "") : null);
  if (!current) {
    return (
      <div className="flex flex-col items-start gap-3 p-4 text-[12.5px] text-muted" data-testid="message-scripts">
        <p className="max-w-xl">
          Run JavaScript on the messages that match{socketio ? " (an event, its arguments)" : ""}: test them with{" "}
          <code className="font-mono">pm.test</code>, keep values with <code className="font-mono">pm.environment.set</code>, and answer with{" "}
          <code className="font-mono">{socketio ? "pm.connection.emit(event, …args)" : "pm.connection.send(data)"}</code>. <code className="font-mono">pm.message</code>{" "}
          is the message. They run while connected and in collection runs, where their tests count.
        </p>
        <Button size="sm" icon={<Plus size={13} />} onClick={add}>
          Add a message script
        </Button>
      </div>
    );
  }
  const match = current.match ?? "any";
  const problem = match === "regex" ? regexProblem : patternProblem(match, current.pattern ?? "");
  return (
    // Side by side when there is room, else the list above the script.
    <div className="@container h-full min-h-0" data-testid="message-scripts">
      <div className="flex h-full min-h-0 flex-col @xl:flex-row">
      <div className="flex max-h-36 shrink-0 flex-col border-b border-line/60 @xl:max-h-none @xl:w-56 @xl:border-b-0 @xl:border-r">
        <ul className="min-h-0 flex-1 overflow-auto py-1" aria-label="Message scripts">
          {scripts.map((s, i) => (
            <li key={i} className={cx("flex items-center gap-1.5 px-2", i === selected && "bg-accent-soft")}>
              <Checkbox checked={s.enabled !== false} onChange={(on) => set(scripts.map((x, j) => (j === i ? { ...x, enabled: on } : x)))} label="Enabled" />
              <button
                onClick={() => setAt(i)}
                aria-current={i === selected || undefined}
                className={cx("min-w-0 flex-1 truncate py-1.5 text-left text-[12px]", i === selected ? "text-accent" : "text-fg", s.enabled === false && "opacity-60")}
                title={scriptLabel(s)}
              >
                {scriptLabel(s)}
              </button>
            </li>
          ))}
        </ul>
        <div className="border-t border-line/60 p-2">
          <Button size="sm" variant="ghost" icon={<Plus size={13} />} onClick={add}>
            Add script
          </Button>
        </div>
      </div>
      <div className="flex min-w-0 flex-1 flex-col">
        <div className="flex shrink-0 flex-wrap items-center gap-x-2 gap-y-1.5 border-b border-line/60 px-3 py-2 text-[12px] text-muted">
          <input
            aria-label="Script name"
            value={current.name ?? ""}
            onChange={(e) => update({ name: e.target.value })}
            placeholder="Name (optional)"
            className={cx(field, "w-40 font-sans")}
          />
          <span>runs on</span>
          <Select aria-label="Direction" value={current.direction ?? "received"} onChange={(e) => update({ direction: e.target.value as MessageDirection })} className="h-7 w-40 [&_select]:h-7 [&_select]:text-[12px]">
            {DIRECTIONS.map((d) => (
              <option key={d.id} value={d.id}>
                {d.label}
              </option>
            ))}
          </Select>
          {socketio && (
            <input aria-label="Event" value={current.event ?? ""} onChange={(e) => update({ event: e.target.value })} placeholder="any event" className={cx(field, "w-32")} />
          )}
          <span>{socketio ? "with arguments that" : "messages that"}</span>
          <Select aria-label="Match" value={match} onChange={(e) => update({ match: e.target.value as MatchKind })} className="h-7 w-36 [&_select]:h-7 [&_select]:text-[12px]">
            {MATCHES.map((k) => (
              <option key={k.id} value={k.id}>
                {k.label}
              </option>
            ))}
          </Select>
          {match !== "any" && (
            <input
              aria-label="Pattern"
              value={current.pattern ?? ""}
              onChange={(e) => update({ pattern: e.target.value })}
              placeholder={socketio && match === "json" ? "$[0].room == lobby" : MATCHES.find((k) => k.id === match)?.placeholder}
              className={cx(field, "min-w-40 flex-1")}
            />
          )}
          <div className="flex-1" />
          <Menu
            align="end"
            trigger={
              <Button size="sm" variant="ghost">
                Snippets <ChevronDown size={12} />
              </Button>
            }
            entries={MESSAGE_SNIPPETS.map((s) => ({
              label: s.label,
              onSelect: () => update({ script: current.script.trim() ? `${current.script.replace(/\s*$/, "")}\n\n${s.code}\n` : `${s.code}\n` }),
            }))}
          />
          <IconButton
            label="Remove script"
            onClick={() => {
              set(scripts.filter((_, i) => i !== selected));
              setAt(Math.max(0, selected - 1));
            }}
          >
            <Trash2 size={13} />
          </IconButton>
        </div>
        {problem && <div className="px-3 pt-1 text-[11.5px] text-warning">{problem}</div>}
        <div className="min-h-0 flex-1">
          {/* Keyed: each script keeps its own undo history. */}
          <CodeEditor
            key={selected}
            value={current.script}
            onChange={(script) => update({ script })}
            language="javascript"
            extensions={pmCompletion}
            placeholder={socketio ? '// e.g. pm.connection.emit("ack", pm.message.args[0]);' : "// e.g. pm.connection.send({ ack: pm.message.json().id });"}
          />
        </div>
      </div>
      </div>
    </div>
  );
}

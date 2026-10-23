// Pre-request / post-response JavaScript of a request, folder or the workspace,
// with completions for the `pm` API (see scriptsModel.ts).
import { useState } from "react";
import type { CompletionContext } from "@codemirror/autocomplete";
import { javascriptLanguage } from "@codemirror/lang-javascript";
import { syntaxTree } from "@codemirror/language";
import { ChevronDown } from "lucide-react";
import type { MessageScript } from "../../bindings/MessageScript";
import type { Scripts } from "../../bindings/Scripts";
import { CodeEditor } from "../CodeEditor";
import { Button, Menu, Segmented } from "../ui";
import { MessageScriptsEditor } from "./MessageScriptsEditor";
import { completeMembers, hasScripts, type ScriptEvent, SNIPPETS } from "./scriptsModel";

const NO_COMPLETION = new Set(["String", "TemplateString", "LineComment", "BlockComment", "RegExp"]);

/** `pm.…`, `console.…` and `pm.expect(…).…` members; stable, so the editor doesn't reconfigure. */
export const pmCompletion = javascriptLanguage.data.of({
  autocomplete: (ctx: CompletionContext) => {
    if (NO_COMPLETION.has(syntaxTree(ctx.state).resolveInner(ctx.pos, -1).name)) return null;
    const line = ctx.state.doc.lineAt(ctx.pos);
    const before = line.text.slice(0, ctx.pos - line.from);
    const result = completeMembers(before);
    if (!result || (result.from === before.length && !before.endsWith(".") && !ctx.explicit)) return null;
    return { from: line.from + result.from, options: result.options, validFor: /^[\w$]*$/ };
  },
});

type Where = "request" | "folder" | "workspace";

/** Tab label: "Scripts", with a dot when there are any. */
export function ScriptsTabLabel({ scripts, more }: { scripts: Scripts | undefined; more?: boolean }) {
  return (
    <>
      Scripts
      {(hasScripts(scripts) || more) && <span className="h-1.5 w-1.5 rounded-full bg-accent" data-testid="scripts-dot" />}
    </>
  );
}

const HINTS: Record<ScriptEvent, Record<Where, string>> = {
  preRequest: {
    request: "Runs before sending, after workspace and folder scripts. Can change the request and set variables.",
    folder: "Runs before every request in this folder, after workspace and outer folder scripts.",
    workspace: "Runs first, before every request in the workspace.",
  },
  postResponse: {
    request: "Runs on the response, after workspace and folder scripts. Add tests with pm.test.",
    folder: "Runs on the response of every request in this folder.",
    workspace: "Runs first on the response of every request in the workspace.",
  },
};

/** Hints of a live connection's scripts (WebSocket, Socket.IO). */
const LIVE_HINTS: Record<ScriptEvent | "onMessage", string> = {
  preRequest: "Runs before connecting, after workspace and folder scripts. Can change the URL and headers and set variables.",
  onMessage: "Runs on each message that matches, while connected and in collection runs.",
  postResponse: "Runs in collection runs after the connection closes: pm.response.json() is the transcript of messages.",
};

/** gRPC requests: their scripts run in collection runs, not on Send. */
const RUN_HINTS: Record<ScriptEvent, string> = {
  preRequest: "Runs before the call in collection runs and zorvik run (not on Send). Can set variables and change the message.",
  postResponse: "Runs on the answer in collection runs and zorvik run (not on Send): pm.response.json() is the message, or a stream's messages.",
};

/** A WebSocket or Socket.IO request: its message scripts too. */
export interface LiveScripts {
  socketio: boolean;
  onMessage: MessageScript[];
  onChange: (scripts: MessageScript[] | undefined) => void;
}

const Dot = ({ on }: { on: boolean }) => (on ? <span className="ml-1.5 inline-block h-1.5 w-1.5 rounded-full bg-accent align-middle" /> : null);

export function ScriptsEditor({
  scripts,
  onChange,
  where,
  live,
  runsOnly,
}: {
  scripts: Scripts;
  onChange: (s: Scripts) => void;
  where: Where;
  live?: LiveScripts;
  /** The scripts run only in collection runs (gRPC). */
  runsOnly?: boolean;
}) {
  const [shown, setEvent] = useState<ScriptEvent | "onMessage">(() => {
    if (live?.onMessage.length && !scripts.preRequest?.trim()) return "onMessage";
    return !scripts.preRequest?.trim() && scripts.postResponse?.trim() ? "postResponse" : "preRequest";
  });
  const messages = live && shown === "onMessage";
  const event: ScriptEvent = shown === "onMessage" ? "preRequest" : shown;
  const code = scripts[event] ?? "";
  const hint = live ? LIVE_HINTS[shown] : runsOnly ? RUN_HINTS[event] : HINTS[event][where];
  const set = (text: string) => {
    const next: Scripts = { ...scripts, [event]: text };
    // Empty scripts are left out of the file.
    if (!next.preRequest) delete next.preRequest;
    if (!next.postResponse) delete next.postResponse;
    onChange(next);
  };
  return (
    <div className="flex h-full min-h-0 flex-col" data-testid="scripts-editor">
      <div className="flex h-9 shrink-0 items-center gap-2 px-3">
        <Segmented<ScriptEvent | "onMessage">
          items={
            live
              ? [
                  { id: "preRequest", label: <>Before connecting<Dot on={!!scripts.preRequest?.trim()} /></> },
                  { id: "onMessage", label: <>On message<Dot on={live.onMessage.length > 0} /></> },
                  { id: "postResponse", label: <>In runs<Dot on={!!scripts.postResponse?.trim()} /></> },
                ]
              : [
                  { id: "preRequest", label: <>Pre-request<Dot on={!!scripts.preRequest?.trim()} /></> },
                  { id: "postResponse", label: <>Post-response<Dot on={!!scripts.postResponse?.trim()} /></> },
                ]
          }
          value={shown}
          onChange={setEvent}
        />
        <span className="min-w-0 flex-1 truncate text-[11.5px] text-faint" title={hint}>
          {hint}
        </span>
        {!messages && (
        <Menu
          align="end"
          trigger={
            <Button size="sm" variant="ghost">
              Snippets <ChevronDown size={12} />
            </Button>
          }
          entries={SNIPPETS[event].map((s) => ({
            label: s.label,
            onSelect: () => set(code.trim() ? `${code.replace(/\s*$/, "")}\n\n${s.code}\n` : `${s.code}\n`),
          }))}
        />
        )}
      </div>
      {messages ? (
        <div className="min-h-0 flex-1 border-t border-line/60">
          <MessageScriptsEditor scripts={live.onMessage} onChange={live.onChange} socketio={live.socketio} />
        </div>
      ) : (
      <div className="min-h-0 flex-1">
        {/* Keyed: each script keeps its own undo history. */}
        <CodeEditor
          key={event}
          value={code}
          onChange={set}
          language="javascript"
          extensions={pmCompletion}
          placeholder={
            event === "preRequest"
              ? "// e.g. pm.environment.set(\"token\", \"…\");"
              : "// e.g. pm.test(\"Status is 200\", () => pm.response.to.have.status(200));"
          }
        />
      </div>
      )}
    </div>
  );
}

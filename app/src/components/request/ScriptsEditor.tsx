// Pre-request / post-response JavaScript of a request, folder or the workspace,
// with completions for the `pm` API (see scriptsModel.ts).
import { useState } from "react";
import type { CompletionContext } from "@codemirror/autocomplete";
import { javascriptLanguage } from "@codemirror/lang-javascript";
import { syntaxTree } from "@codemirror/language";
import { ChevronDown } from "lucide-react";
import type { Scripts } from "../../bindings/Scripts";
import { CodeEditor } from "../CodeEditor";
import { Button, Menu, Segmented } from "../ui";
import { completeMembers, hasScripts, type ScriptEvent, SNIPPETS } from "./scriptsModel";

const NO_COMPLETION = new Set(["String", "TemplateString", "LineComment", "BlockComment", "RegExp"]);

/** `pm.…`, `console.…` and `pm.expect(…).…` members; stable, so the editor doesn't reconfigure. */
const pmCompletion = javascriptLanguage.data.of({
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
export function ScriptsTabLabel({ scripts }: { scripts: Scripts | undefined }) {
  return (
    <>
      Scripts
      {hasScripts(scripts) && <span className="h-1.5 w-1.5 rounded-full bg-accent" data-testid="scripts-dot" />}
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

const Dot = ({ on }: { on: boolean }) => (on ? <span className="ml-1.5 inline-block h-1.5 w-1.5 rounded-full bg-accent align-middle" /> : null);

export function ScriptsEditor({ scripts, onChange, where }: { scripts: Scripts; onChange: (s: Scripts) => void; where: Where }) {
  const [event, setEvent] = useState<ScriptEvent>(!scripts.preRequest?.trim() && scripts.postResponse?.trim() ? "postResponse" : "preRequest");
  const code = scripts[event] ?? "";
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
        <Segmented
          items={[
            { id: "preRequest", label: <>Pre-request<Dot on={!!scripts.preRequest?.trim()} /></> },
            { id: "postResponse", label: <>Post-response<Dot on={!!scripts.postResponse?.trim()} /></> },
          ]}
          value={event}
          onChange={setEvent}
        />
        <span className="min-w-0 flex-1 truncate text-[11.5px] text-faint" title={HINTS[event][where]}>
          {HINTS[event][where]}
        </span>
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
      </div>
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
    </div>
  );
}

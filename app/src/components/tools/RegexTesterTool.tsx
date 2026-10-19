// Regex tester: a pattern run in Rust, with the regex syntax and limits the app's server
// reply rules, wait steps and load test captures use. Matches are highlighted in the
// test text and listed with their groups.
import { useMemo } from "react";
import { type Extension, StateField } from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView } from "@codemirror/view";
import { Regex, X } from "lucide-react";
import type { RegexFlags } from "../../bindings/RegexFlags";
import type { RegexTestResult } from "../../bindings/RegexTestResult";
import { openExternal } from "../../lib/platform";
import { api } from "../../lib/rpc";
import { type ToolTab } from "../../store/tabs";
import { toolState } from "../../store/tools";
import { CodeEditor } from "../CodeEditor";
import { cx, EmptyState, IconButton, Input, Spinner, Tooltip } from "../ui";
import { editInputs, useLiveResult } from "./live";
import { matchRanges, NO_FLAGS, REGEX_FLAGS, showText } from "./models";
import { CopyButton, Section } from "./parts";

interface RegexState {
  pattern: string;
  flags: RegexFlags;
  text: string;
}

const DEFAULTS: RegexState = { pattern: "", flags: NO_FLAGS, text: "" };

const SYNTAX_URL = "https://docs.rs/regex/latest/regex/#syntax";

const marks = [Decoration.mark({ class: "cm-rx-match" }), Decoration.mark({ class: "cm-rx-match cm-rx-alt" })];
const markTheme = EditorView.baseTheme({
  ".cm-rx-match": { backgroundColor: "color-mix(in srgb, var(--info) 22%, transparent)", boxShadow: "inset 0 -1px var(--info)", borderRadius: "2px" },
  ".cm-rx-alt": { backgroundColor: "color-mix(in srgb, var(--success) 22%, transparent)", boxShadow: "inset 0 -1px var(--success)" },
});

/** The matches as editor highlights; they follow edits until the next result arrives. */
function highlights(result: RegexTestResult | null): Extension {
  if (!result) return [];
  const field = StateField.define<DecorationSet>({
    create: (state) => Decoration.set(matchRanges(result.matches, state.doc.length).map((r) => marks[r.index % 2].range(r.from, r.to))),
    update: (deco, tr) => deco.map(tr.changes),
    provide: (f) => EditorView.decorations.from(f),
  });
  return [field, markTheme];
}

export function RegexTesterTool({ tab }: { tab: ToolTab }) {
  const s = toolState(tab, DEFAULTS);
  const flags = { ...NO_FLAGS, ...s.flags };
  const edit = (patch: Partial<RegexState>) => editInputs(tab.id, () => patch);
  const { pattern, text } = s;
  const ready = pattern !== "";
  const live = useLiveResult(tab, ready ? () => api.regexTest(pattern, flags, text) : null, 150);
  const result = live.value;
  const extension = useMemo(() => highlights(result), [result]);
  const groupNames = result?.groupNames ?? [];

  return (
    <div className="flex min-h-full flex-col pb-6" data-testid="regex-tester">
      <div className="flex flex-wrap items-end gap-2 px-4 pb-2">
        <label className="flex min-w-[260px] flex-1 flex-col gap-1">
          <span className="text-[11.5px] font-medium text-muted">Pattern</span>
          <Input
            autoFocus
            className="font-mono"
            aria-label="Pattern"
            placeholder={String.raw`e.g. "id":\s*(\d+)  or  token=(?P<token>\w+)`}
            value={s.pattern}
            invalid={ready && !!live.error}
            onChange={(e) => edit({ pattern: e.target.value })}
          />
        </label>
        <div role="group" aria-label="Flags" className="flex h-8 items-center gap-0.5 rounded-lg bg-panel-2 p-0.5">
          {REGEX_FLAGS.map((f) => (
            <Tooltip key={f.key} content={`${f.label} (${f.letter})`}>
              <button
                type="button"
                aria-pressed={flags[f.key]}
                aria-label={f.label}
                onClick={() => edit({ flags: { ...flags, [f.key]: !flags[f.key] } })}
                className={cx(
                  "h-7 w-7 rounded-md font-mono text-[12.5px] outline-none focus-visible:ring-1 focus-visible:ring-accent",
                  flags[f.key] ? "bg-elev font-semibold text-accent shadow-sm" : "text-muted hover:text-fg",
                )}
              >
                {f.letter}
              </button>
            </Tooltip>
          ))}
        </div>
        <span className="flex h-8 min-w-[90px] items-center text-[12px] tabular-nums text-muted" data-testid="regex-status">
          {!ready || live.error ? null : result && !live.stale ? (
            result.count === 0 ? (
              "No match"
            ) : (
              `${result.count.toLocaleString()}${result.countCapped ? "+" : ""} ${result.count === 1 ? "match" : "matches"}`
            )
          ) : (
            <Spinner size={12} />
          )}
        </span>
      </div>
      <p className="px-4 pb-2 text-[11.5px] text-faint">
        Rust regex syntax, as server reply rules, wait steps and load test captures use it: no look-around or backreferences.{" "}
        <button type="button" className="text-accent underline-offset-2 hover:underline" onClick={() => void openExternal(SYNTAX_URL)}>
          Syntax
        </button>
      </p>
      {ready && live.error && (
        <pre role="alert" className="selectable mx-4 mb-3 overflow-x-auto rounded-lg border border-danger/30 bg-danger/10 px-3 py-2 font-mono text-[12px] text-danger">
          {live.error}
        </pre>
      )}
      {result && result.pattern !== s.pattern && !live.error && (
        <div className="flex items-center gap-2 px-4 pb-2 text-[12px] text-muted">
          <span>With the flags written in, for rules and captures:</span>
          <code className="selectable rounded bg-panel-2 px-1.5 py-0.5 font-mono text-[12px] text-fg">{result.pattern}</code>
          <CopyButton text={result.pattern} label="Copy pattern" />
        </div>
      )}

      <div className="flex flex-col gap-1.5 px-4">
        <div className="flex h-7 items-center gap-1">
          <span className="flex-1 text-[12px] font-medium text-muted">Test text</span>
          {s.text && (
            <IconButton label="Clear the test text" onClick={() => edit({ text: "" })}>
              <X size={13} />
            </IconButton>
          )}
        </div>
        <div className="h-[220px] overflow-hidden rounded-lg border border-line bg-input focus-within:border-accent" data-testid="regex-text">
          <CodeEditor value={s.text} onChange={(t) => edit({ text: t })} placeholder="Paste a response body, a message or any text to match" extensions={extension} />
        </div>
      </div>

      {!ready ? (
        <EmptyState icon={<Regex size={28} />} title="Test a regular expression">
          Type a pattern and some text: matches are highlighted and listed with their groups, the way reply rules, wait steps and load test captures will see them.
        </EmptyState>
      ) : result && !live.error ? (
        <div className={cx("transition-opacity", live.stale && "opacity-50")} data-testid="regex-result">
          {result.count > 0 && (
            <div className="mx-4 mt-3 grid gap-1 rounded-lg bg-panel-2 px-3 py-2 text-[12px]">
              <div>
                <span className="text-muted">A reply rule or wait step with this pattern </span>
                <span className="font-medium text-success">matches this text</span>
              </div>
              <div className="flex min-w-0 items-start gap-1">
                <span className="shrink-0 text-muted">A load test capture takes </span>
                <code className="selectable min-w-0 break-all font-mono text-fg" data-testid="regex-capture">
                  {result.capture != null ? showText(result.capture) : "nothing"}
                </code>
                <span className="shrink-0 text-faint">({groupNames.length > 0 ? "the first group of the first match" : "the first match"})</span>
              </div>
            </div>
          )}
          {result.matches.length > 0 && (
            <Section title={result.matches.length < result.count ? `Matches · first ${result.matches.length.toLocaleString()} shown` : "Matches"}>
              <table className="mx-4 w-[calc(100%-2rem)] text-[12.5px]" data-testid="regex-matches">
                <thead>
                  <tr className="text-left text-[11px] text-faint">
                    <th className="w-8 pb-1 font-medium">#</th>
                    <th className="pb-1 pr-3 font-medium">Match</th>
                    <th className="w-24 pb-1 pr-3 font-medium">Position</th>
                    {groupNames.length > 0 && <th className="pb-1 font-medium">Groups</th>}
                  </tr>
                </thead>
                <tbody>
                  {result.matches.slice(0, 200).map((m, i) => (
                    <tr key={i} className="border-b border-line/50 align-top last:border-0">
                      <td className="py-1 text-faint tabular-nums">{i + 1}</td>
                      <td className="selectable break-all py-1 pr-3 font-mono text-[12px] text-fg">{showText(m.text)}</td>
                      <td className="py-1 pr-3 font-mono text-[11.5px] text-muted tabular-nums">
                        {m.start}–{m.end}
                      </td>
                      {groupNames.length > 0 && (
                        <td className="py-1">
                          <div className="flex flex-wrap gap-1">
                            {m.groups.map((g, gi) => (
                              <span key={gi} className="inline-flex max-w-full items-baseline gap-1 rounded bg-hover px-1.5 font-mono text-[11.5px]">
                                <span className="text-faint">{groupNames[gi] ?? gi + 1}</span>
                                <span className={cx("selectable break-all", g ? "text-fg" : "text-faint")}>{g ? showText(g.text) : "no part"}</span>
                              </span>
                            ))}
                          </div>
                        </td>
                      )}
                    </tr>
                  ))}
                </tbody>
              </table>
              {result.matches.length > 200 && (
                <p className="px-4 pt-1.5 text-[11.5px] text-faint">
                  The first 200 are listed here; {result.matches.length.toLocaleString()} are highlighted in the text.
                </p>
              )}
            </Section>
          )}
        </div>
      ) : null}
    </div>
  );
}

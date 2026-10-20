// CodeMirror 6 wrapper: created once, updated through compartments so typing
// never re-creates the editor.
import { useEffect, useRef } from "react";
import { autocompletion, closeBrackets, closeBracketsKeymap, completionKeymap, type CompletionContext } from "@codemirror/autocomplete";
import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { html } from "@codemirror/lang-html";
import { javascript } from "@codemirror/lang-javascript";
import { json } from "@codemirror/lang-json";
import { xml } from "@codemirror/lang-xml";
import { c, csharp, dart, java, kotlin } from "@codemirror/legacy-modes/mode/clike";
import { go } from "@codemirror/legacy-modes/mode/go";
import { powerShell } from "@codemirror/legacy-modes/mode/powershell";
import { python } from "@codemirror/legacy-modes/mode/python";
import { ruby } from "@codemirror/legacy-modes/mode/ruby";
import { rust } from "@codemirror/legacy-modes/mode/rust";
import { shell } from "@codemirror/legacy-modes/mode/shell";
import { swift } from "@codemirror/legacy-modes/mode/swift";
import {
  bracketMatching,
  foldGutter,
  foldKeymap,
  HighlightStyle,
  indentOnInput,
  StreamLanguage,
  syntaxHighlighting,
} from "@codemirror/language";
import { highlightSelectionMatches, search, searchKeymap } from "@codemirror/search";
import { Compartment, EditorState, type Extension, Prec, StateEffect, StateField, type Text } from "@codemirror/state";
import {
  Decoration,
  type DecorationSet,
  drawSelection,
  EditorView,
  highlightActiveLine,
  highlightActiveLineGutter,
  keymap,
  lineNumbers,
  placeholder as cmPlaceholder,
  ViewPlugin,
  type ViewUpdate,
  WidgetType,
} from "@codemirror/view";
import { tags as t } from "@lezer/highlight";

export type EditorLanguage =
  | "json"
  | "xml"
  | "html"
  | "javascript"
  | "text"
  // Generated code (highlighting only).
  | "shell"
  | "powershell"
  | "python"
  | "go"
  | "java"
  | "kotlin"
  | "swift"
  | "csharp"
  | "php"
  | "ruby"
  | "rust"
  | "dart"
  | "c";

/** Highlighting of generated code (CodeMirror's stream modes). PHP reads well enough as C. */
const LEGACY_MODES = { shell, powershell: powerShell, python, go, java, kotlin, swift, csharp, php: c, ruby, rust, dart, c } as const;

const highlight = HighlightStyle.define([
  { tag: t.string, color: "var(--syn-string)" },
  { tag: [t.number, t.integer, t.float], color: "var(--syn-number)" },
  { tag: t.bool, color: "var(--syn-bool)" },
  { tag: t.null, color: "var(--syn-null)" },
  { tag: [t.propertyName, t.attributeName], color: "var(--syn-property)" },
  { tag: [t.keyword, t.operatorKeyword, t.definitionKeyword], color: "var(--syn-keyword)" },
  { tag: [t.tagName, t.angleBracket], color: "var(--syn-tag)" },
  { tag: [t.comment, t.lineComment, t.blockComment], color: "var(--syn-comment)", fontStyle: "italic" },
  { tag: [t.punctuation, t.separator, t.bracket], color: "var(--muted)" },
]);

function languageExtension(lang: EditorLanguage): Extension {
  switch (lang) {
    case "json":
      return json();
    case "xml":
      return xml();
    case "html":
      return html();
    case "javascript":
      return javascript();
    case "text":
      return [];
    default:
      return StreamLanguage.define(LEGACY_MODES[lang]);
  }
}

// ---- very long lines --------------------------------------------------------------
// A browser lays out a line as one piece: a single line of megabytes (an image as base64 in a
// JSON response) takes WebKit about a minute to draw, and every time the editor is shown again.
// Such a line keeps all its text in the document (copying, selecting and searching see all of
// it), but only its start and end are drawn, with a marker that shows more on request.

/** Lines longer than this are shortened. */
export const LONG_LINE = 10_000;
/** What a shortened line shows of its start and end, and how much more each "Show more" adds. */
const KEEP_START = 1_000;
const KEEP_END = 80;
const SHOW_MORE = 50_000;

/** "Show more" on the line starting at `from`. */
export const showMore = StateEffect.define<number>();

class HiddenText extends WidgetType {
  constructor(
    readonly hidden: number,
    readonly lineFrom: number,
  ) {
    super();
  }
  eq(other: HiddenText) {
    return other.hidden === this.hidden && other.lineFrom === this.lineFrom;
  }
  toDOM(view: EditorView) {
    const wrap = document.createElement("span");
    wrap.className = "zv-hidden-text";
    wrap.textContent = `… ${hiddenSize(this.hidden)} more `;
    const more = document.createElement("button");
    more.type = "button";
    more.className = "zv-show-more";
    more.textContent = "Show more";
    more.title = "Draw more of this line (very long lines are slow to draw; Copy and Save have all of it)";
    more.addEventListener("mousedown", (e) => e.preventDefault());
    more.addEventListener("click", () => view.dispatch({ effects: showMore.of(this.lineFrom) }));
    wrap.append(more);
    return wrap;
  }
  ignoreEvent() {
    return true;
  }
}

function hiddenSize(chars: number): string {
  if (chars < 10_000) return `${chars.toLocaleString()} characters`;
  const mb = chars / (1024 * 1024);
  return mb >= 1 ? `${mb.toFixed(1)} MB` : `${Math.round(chars / 1024)} KB`;
}

type LongLines = { shown: Map<number, number>; decorations: DecorationSet };

/** Decorations shortening the long lines among lines `first` to `last` (1-based), in order. */
function shortenRange(doc: Text, first: number, last: number, shown: Map<number, number>) {
  const ranges = [];
  for (let n = first; n <= last; n++) {
    const line = doc.line(n);
    if (line.length <= LONG_LINE) continue;
    const start = line.from + Math.min(line.length, KEEP_START + (shown.get(line.from) ?? 0));
    const end = line.to - KEEP_END;
    if (end - start < KEEP_START) continue;
    ranges.push(Decoration.replace({ widget: new HiddenText(end - start, line.from) }).range(start, end));
  }
  return ranges;
}

function shortenLines(doc: Text, shown: Map<number, number>): DecorationSet {
  // Most documents have no long line: skip the walk when even the whole text is short.
  return doc.length > LONG_LINE ? Decoration.set(shortenRange(doc, 1, doc.lines, shown)) : Decoration.none;
}

export const longLines = StateField.define<LongLines>({
  create: (state) => ({ shown: new Map(), decorations: shortenLines(state.doc, new Map()) }),
  update(value, tr) {
    const more = tr.effects.filter((e) => e.is(showMore));
    if (!tr.docChanged && !more.length) return value;
    // Keep each line's "Show more" through edits (by where the line starts now).
    let shown = tr.docChanged ? new Map([...value.shown].map(([from, n]) => [tr.changes.mapPos(from), n])) : value.shown;
    if (more.length) {
      shown = new Map(shown);
      for (const e of more) shown.set(e.value, (shown.get(e.value) ?? 0) + SHOW_MORE);
      return { shown, decorations: shortenLines(tr.state.doc, shown) };
    }
    // Typing: look again only at the lines the change touched, not the whole document.
    const doc = tr.state.doc;
    let decorations = value.decorations.map(tr.changes);
    tr.changes.iterChangedRanges((_fromA, _toA, fromB, toB) => {
      const first = doc.lineAt(fromB);
      const last = doc.lineAt(toB);
      decorations = decorations.update({
        filterFrom: first.from,
        filterTo: last.to,
        filter: () => false,
        add: shortenRange(doc, first.number, last.number, shown),
      });
    });
    return { shown, decorations };
  },
  provide: (field) => EditorView.decorations.from(field, (v) => v.decorations),
});

// ---- {{variable}} highlighting ------------------------------------------------

const refreshVars = StateEffect.define<null>();
const VAR_RE = /\{\{\s*([^{}\n]+?)\s*\}\}/g;

function variablePlugin(known: { current: Set<string> }) {
  const okMark = Decoration.mark({ class: "zv-var" });
  const badMark = Decoration.mark({ class: "zv-var-missing" });
  const build = (view: EditorView): DecorationSet => {
    const ranges = [];
    for (const { from, to } of view.visibleRanges) {
      const text = view.state.doc.sliceString(from, to);
      for (const m of text.matchAll(VAR_RE)) {
        const name = m[1].trim();
        const start = from + (m.index ?? 0);
        const ok = name.startsWith("$") || known.current.has(name);
        ranges.push((ok ? okMark : badMark).range(start, start + m[0].length));
      }
    }
    return Decoration.set(ranges, true);
  };
  return ViewPlugin.fromClass(
    class {
      decorations: DecorationSet;
      constructor(view: EditorView) {
        this.decorations = build(view);
      }
      update(u: ViewUpdate) {
        if (u.docChanged || u.viewportChanged || u.transactions.some((tr) => tr.effects.some((e) => e.is(refreshVars)))) {
          this.decorations = build(u.view);
        }
      }
    },
    { decorations: (v) => v.decorations },
  );
}

function variableCompletion(names: { current: string[] }) {
  return (ctx: CompletionContext) => {
    const m = ctx.matchBefore(/\{\{\s*[\w$.-]*/);
    if (!m) return null;
    const prefixLen = m.text.match(/^\{\{\s*/)?.[0].length ?? 2;
    return {
      from: m.from + prefixLen,
      options: names.current.map((n) => ({ label: n, apply: `${n}}}`, type: n.startsWith("$") ? "function" : "variable" })),
      validFor: /^[\w$.-]*$/,
    };
  };
}

export interface CodeEditorProps {
  value: string;
  onChange?: (value: string) => void;
  language?: EditorLanguage;
  readOnly?: boolean;
  placeholder?: string;
  lineWrapping?: boolean;
  lineNumbers?: boolean;
  /** Highlight {{variables}}; names that are defined. */
  variables?: string[];
  onSubmit?: () => void;
  className?: string;
  autoFocus?: boolean;
  /** Extra extensions (e.g. a language with its own completions, which are then offered next to {{variables}}). */
  extensions?: Extension;
}

export function CodeEditor({
  value,
  onChange,
  language = "text",
  readOnly = false,
  placeholder,
  lineWrapping = true,
  lineNumbers: showLineNumbers = true,
  variables,
  onSubmit,
  className,
  autoFocus,
  extensions: extra,
}: CodeEditorProps) {
  const host = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  const onChangeRef = useRef(onChange);
  const onSubmitRef = useRef(onSubmit);
  const known = useRef(new Set(variables ?? []));
  const names = useRef<string[]>(variables ?? []);
  const compartments = useRef({
    lang: new Compartment(),
    readOnly: new Compartment(),
    wrap: new Compartment(),
    placeholder: new Compartment(),
    extra: new Compartment(),
  });
  onChangeRef.current = onChange;
  onSubmitRef.current = onSubmit;

  useEffect(() => {
    if (!host.current) return;
    const c = compartments.current;
    const extensions: Extension[] = [
      showLineNumbers ? [lineNumbers(), highlightActiveLineGutter(), foldGutter()] : [],
      history(),
      drawSelection(),
      indentOnInput(),
      bracketMatching(),
      closeBrackets(),
      highlightActiveLine(),
      highlightSelectionMatches(),
      search({ top: true }),
      syntaxHighlighting(highlight),
      longLines,
      // ⌘/Ctrl+Enter submits. An editor with its own onSubmit owns the shortcut: stop it reaching the
      // app-wide ⌘Enter handler, which would send the request a second time (or reconnect a WebSocket).
      Prec.highest(
        EditorView.domEventHandlers({
          keydown: (e) => {
            if (e.key !== "Enter" || !(e.metaKey || e.ctrlKey) || e.altKey) return false;
            if (onSubmitRef.current) {
              e.stopPropagation();
              onSubmitRef.current();
            }
            return true;
          },
        }),
      ),
      keymap.of([...closeBracketsKeymap, ...defaultKeymap, ...searchKeymap, ...historyKeymap, ...foldKeymap, ...completionKeymap, indentWithTab]),
      c.lang.of(languageExtension(language)),
      // Read-only views keep an editable (focusable) DOM so ⌘F search, ⌘A and keyboard selection work;
      // the readOnly state is what blocks edits.
      c.readOnly.of(EditorState.readOnly.of(readOnly)),
      c.wrap.of(lineWrapping ? EditorView.lineWrapping : []),
      c.placeholder.of(placeholder ? cmPlaceholder(placeholder) : []),
      c.extra.of(extra ?? []),
      EditorView.updateListener.of((u) => {
        if (u.docChanged) onChangeRef.current?.(u.state.doc.toString());
      }),
      EditorView.theme({ "&": { height: "100%" }, ".cm-content": { padding: "8px 0" } }),
    ];
    if (variables) {
      const completeVariables = variableCompletion(names);
      extensions.push(
        variablePlugin(known),
        extra
          ? [autocompletion({ icons: false }), EditorState.languageData.of(() => [{ autocomplete: completeVariables }])]
          : autocompletion({ override: [completeVariables], icons: false }),
      );
    } else if (extra) {
      extensions.push(autocompletion({ icons: false }));
    }
    view.current = new EditorView({ parent: host.current, state: EditorState.create({ doc: value, extensions }) });
    if (autoFocus) view.current.focus();
    return () => {
      view.current?.destroy();
      view.current = null;
    };
    // Created once; props below are applied through compartments.
  }, []);

  // External value changes (switching tabs, formatting, reloads).
  useEffect(() => {
    const v = view.current;
    if (v && v.state.doc.toString() !== value) {
      v.dispatch({ changes: { from: 0, to: v.state.doc.length, insert: value } });
    }
  }, [value]);

  useEffect(() => {
    view.current?.dispatch({ effects: compartments.current.lang.reconfigure(languageExtension(language)) });
  }, [language]);

  useEffect(() => {
    view.current?.dispatch({ effects: compartments.current.readOnly.reconfigure(EditorState.readOnly.of(readOnly)) });
  }, [readOnly]);

  useEffect(() => {
    view.current?.dispatch({ effects: compartments.current.wrap.reconfigure(lineWrapping ? EditorView.lineWrapping : []) });
  }, [lineWrapping]);

  useEffect(() => {
    view.current?.dispatch({ effects: compartments.current.placeholder.reconfigure(placeholder ? cmPlaceholder(placeholder) : []) });
  }, [placeholder]);

  useEffect(() => {
    view.current?.dispatch({ effects: compartments.current.extra.reconfigure(extra ?? []) });
  }, [extra]);

  useEffect(() => {
    if (!variables) return;
    known.current = new Set(variables);
    names.current = variables;
    view.current?.dispatch({ effects: refreshVars.of(null) });
  }, [variables]);

  return <div ref={host} className={className ?? "h-full min-h-0 overflow-hidden"} data-testid="code-editor" />;
}

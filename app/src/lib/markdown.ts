// Markdown for the notes people write about requests, folders, servers and load tests
// (their Docs). Parsed to a small tree that is rendered as React elements: HTML in the
// text stays text, and links keep only http(s) and mailto addresses. Workspace files
// come from Git, so the text is untrusted. Covers what notes use: headings, paragraphs,
// lists (nested, tasks), code, quotes (and GitHub alerts), tables, rules, emphasis,
// links and images.

export type Inline =
  | { type: "text"; text: string }
  | { type: "code"; text: string }
  | { type: "strong" | "em" | "del"; children: Inline[] }
  /** `href` is null when the address isn't one a link may open (javascript:, relative paths…). */
  | { type: "link"; href: string | null; children: Inline[] }
  | { type: "image"; src: string; alt: string }
  | { type: "break" };

export type Align = "left" | "center" | "right" | null;
export type Alert = "note" | "tip" | "important" | "warning" | "caution";

export interface ListItem {
  /** A task list item: `- [ ]` or `- [x]`. */
  checked?: boolean;
  blocks: Block[];
}

export type Block =
  | { type: "heading"; level: number; content: Inline[] }
  | { type: "paragraph"; content: Inline[] }
  | { type: "code"; lang: string; code: string }
  | { type: "quote"; alert?: Alert; blocks: Block[] }
  | { type: "list"; ordered: boolean; start: number; tight: boolean; items: ListItem[] }
  | { type: "table"; align: Align[]; head: Inline[][]; rows: Inline[][][] }
  | { type: "rule" };

/** Quotes and lists nested deeper than this are shown as text. */
const MAX_DEPTH = 12;

/** Most characters of Docs that Preview renders: more would keep the window busy. */
export const MAX_PREVIEW = 200_000;

/** Bare and `<…>` addresses longer than this stay text. */
const MAX_URL = 2048;

/** Addresses a link may open (in the system browser). */
export function safeHref(href: string): string | null {
  const h = href.trim();
  return /^(https?:\/\/[^\s]+|mailto:[^\s]+)$/i.test(h) ? h : null;
}

/** Images shown in place: embedded pictures only. Nothing is downloaded. */
export function inlineImage(src: string): boolean {
  return /^data:image\/(png|jpe?g|gif|webp);base64,[a-z0-9+/=\s]+$/i.test(src);
}

const FENCE = /^ {0,3}(`{3,}|~{3,})\s*([^\s`]*)/;
const HEADING = /^ {0,3}(#{1,6})(?:[ \t]+(.*))?$/;
const RULE = /^ {0,3}([-*_])(?:[ \t]*\1){2,}[ \t]*$/;
const QUOTE = /^ {0,3}>/;
const ITEM = /^( {0,3})([-*+]|(\d{1,9})[.)])( +|$)(.*)$/;
const SETEXT = /^ {0,3}(=+|-+)[ \t]*$/;
const DELIMITER_CELL = /^:?-+:?$/;
const ALERT = /^\s*\[!(note|tip|important|warning|caution)\]\s*$/i;

const indentOf = (line: string) => line.length - line.trimStart().length;

interface ItemStart {
  ordered: boolean;
  /** `-`, `*`, `+`, `.` or `)`: a different one starts a new list. */
  mark: string;
  start: number;
  /** Where the item's content starts; later lines indented this far belong to it. */
  contentIndent: number;
  first: string;
}

function itemStart(line: string): ItemStart | null {
  const m = ITEM.exec(line);
  if (!m || RULE.test(line)) return null;
  const spaces = m[4].length;
  return {
    ordered: m[3] !== undefined,
    mark: m[2].slice(-1),
    start: m[3] !== undefined ? Number(m[3]) : 1,
    contentIndent: m[1].length + m[2].length + Math.min(Math.max(spaces, 1), 4),
    first: m[5],
  };
}

/** Whether `line` starts a block other than a paragraph (so it ends one). */
function startsBlock(line: string): boolean {
  return FENCE.test(line) || HEADING.test(line) || RULE.test(line) || QUOTE.test(line) || itemStart(line) !== null;
}

export function parseMarkdown(source: string): Block[] {
  return parseBlocks(source.replace(/\r\n?/g, "\n").replace(/\t/g, "    ").split("\n"), 0);
}

/** The part of `source` that Preview renders (whole lines where it can), and whether some was left out. */
export function previewPart(source: string): { text: string; cut: boolean } {
  if (source.length <= MAX_PREVIEW) return { text: source, cut: false };
  const line = source.lastIndexOf("\n", MAX_PREVIEW);
  let end = line > MAX_PREVIEW / 2 ? line : MAX_PREVIEW;
  // Not between the halves of a surrogate pair.
  const code = source.charCodeAt(end - 1);
  if (code >= 0xd800 && code <= 0xdbff) end--;
  return { text: source.slice(0, end), cut: true };
}

// The checks below walk the text once: Docs come from workspace files (untrusted), and
// regular expressions that can match a long run of the same characters in several ways
// (`\s*\|?\s*`, `(^|\s+)#+\s*$`) take quadratic time on such runs.

/** A table's delimiter row: `| :--- | ---: |`. */
function isTableDelimiter(line: string): boolean {
  let row = line.trim();
  if (row.startsWith("|")) row = row.slice(1);
  if (row.endsWith("|")) row = row.slice(0, -1);
  return row.split("|").every((cell) => DELIMITER_CELL.test(cell.trim()));
}

/** A heading's text without its closing `#`s (`## Title ##`). */
function withoutClosingHashes(text: string): string {
  let end = text.length;
  while (end > 0 && SPACE.test(text[end - 1])) end--;
  let start = end;
  while (start > 0 && text[start - 1] === "#") start--;
  if (start === end || (start > 0 && !SPACE.test(text[start - 1]))) return text;
  return text.slice(0, start);
}

function splitCells(row: string): string[] {
  let text = row.trim();
  if (text.startsWith("|")) text = text.slice(1);
  if (text.endsWith("|") && !text.endsWith("\\|")) text = text.slice(0, -1);
  const cells: string[] = [];
  let cell = "";
  let code = false;
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (c === "\\" && text[i + 1] === "|") {
      cell += "|";
      i++;
    } else if (c === "`") {
      code = !code;
      cell += c;
    } else if (c === "|" && !code) {
      cells.push(cell.trim());
      cell = "";
    } else cell += c;
  }
  cells.push(cell.trim());
  return cells;
}

function parseBlocks(lines: string[], depth: number): Block[] {
  const blocks: Block[] = [];
  let para: string[] = [];
  const flush = () => {
    if (para.length) blocks.push({ type: "paragraph", content: parseInline(para.join("\n")) });
    para = [];
  };

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    if (!line.trim()) {
      flush();
      continue;
    }

    const fence = FENCE.exec(line);
    if (fence) {
      flush();
      const close = new RegExp(`^ {0,3}${fence[1][0] === "`" ? "`" : "~"}{${fence[1].length},}\\s*$`);
      const strip = indentOf(line);
      const code: string[] = [];
      for (i++; i < lines.length && !close.test(lines[i]); i++) {
        code.push(lines[i].slice(Math.min(strip, indentOf(lines[i]))));
      }
      blocks.push({ type: "code", lang: fence[2].toLowerCase(), code: code.join("\n") });
      continue;
    }

    if (para.length && SETEXT.test(line)) {
      blocks.push({ type: "heading", level: line.trim()[0] === "=" ? 1 : 2, content: parseInline(para.join("\n")) });
      para = [];
      continue;
    }

    const heading = HEADING.exec(line);
    if (heading) {
      flush();
      const text = withoutClosingHashes(heading[2] ?? "").trim();
      blocks.push({ type: "heading", level: heading[1].length, content: parseInline(text) });
      continue;
    }

    if (RULE.test(line)) {
      flush();
      blocks.push({ type: "rule" });
      continue;
    }

    if (QUOTE.test(line) && depth < MAX_DEPTH) {
      flush();
      const quoted: string[] = [];
      for (; i < lines.length && QUOTE.test(lines[i]); i++) quoted.push(lines[i].replace(/^ {0,3}> ?/, ""));
      i--;
      const alert = ALERT.exec(quoted[0] ?? "");
      blocks.push({
        type: "quote",
        alert: alert ? (alert[1].toLowerCase() as Alert) : undefined,
        blocks: parseBlocks(alert ? quoted.slice(1) : quoted, depth + 1),
      });
      continue;
    }

    if (line.includes("|") && (lines[i + 1] ?? "").includes("-") && isTableDelimiter(lines[i + 1])) {
      const head = splitCells(line);
      const delimiter = splitCells(lines[i + 1]);
      if (head.length === delimiter.length) {
        flush();
        const align: Align[] = delimiter.map((d) => {
          const left = d.startsWith(":");
          const right = d.endsWith(":");
          return left && right ? "center" : right ? "right" : left ? "left" : null;
        });
        const rows: Inline[][][] = [];
        for (i += 2; i < lines.length && lines[i].trim() && !startsBlock(lines[i]); i++) {
          const cells = splitCells(lines[i]);
          rows.push(head.map((_, c) => parseInline(cells[c] ?? "")));
        }
        i--;
        blocks.push({ type: "table", align, head: head.map((h) => parseInline(h)), rows });
        continue;
      }
    }

    const item = itemStart(line);
    // Only a bullet or "1." with text may interrupt a paragraph ("in\n2024. we…" stays text).
    if (item && depth < MAX_DEPTH && !(para.length && ((item.ordered && item.start !== 1) || !item.first.trim()))) {
      flush();
      const items: ListItem[] = [];
      let tight = true;
      let next: ItemStart | null = item;
      let j = i;
      while (next && next.ordered === item.ordered && next.mark === item.mark) {
        const own: string[] = [next.first];
        let fenced = FENCE.test(next.first);
        // A blank line between blocks of the item (outside code) makes the list loose.
        let gap = false;
        let k = j + 1;
        for (; k < lines.length; k++) {
          const l = lines[k];
          if (!l.trim()) {
            own.push("");
            continue;
          }
          if (indentOf(l) >= next.contentIndent) {
            const inner = l.slice(next.contentIndent);
            if (!fenced && !own[own.length - 1].trim()) gap = true;
            if (FENCE.test(inner)) fenced = !fenced;
            own.push(inner);
            continue;
          }
          // A paragraph may go on without the indent ("lazy" lines).
          const last = own[own.length - 1];
          if (!fenced && last.trim() && !FENCE.test(last) && !startsBlock(l)) {
            own.push(l.trim());
            continue;
          }
          break;
        }
        let end = own.length;
        while (end > 0 && !own[end - 1].trim()) end--;
        const content = own.slice(0, end);
        if (gap) tight = false;
        const task = /^\[([ xX])\][ \t]+/.exec(content[0] ?? "");
        if (task) content[0] = content[0].slice(task[0].length);
        items.push({ checked: task ? task[1] !== " " : undefined, blocks: parseBlocks(content, depth + 1) });
        j = k;
        next = j < lines.length ? itemStart(lines[j]) : null;
        // So does a blank line between items.
        if (next && end < own.length) tight = false;
      }
      blocks.push({ type: "list", ordered: item.ordered, start: item.start, tight, items });
      i = j - 1;
      continue;
    }

    // A paragraph line (an indented one too: indented code blocks aren't supported).
    para.push(line.trim());
  }
  flush();
  return blocks;
}

// ---- Inline ---------------------------------------------------------------------------
// A scanner rather than one big regular expression: each opener looks for its closer once,
// a search that ran isn't run again for the openers it already answers, and a link's
// destination is read once, so odd text stays fast.

const ESCAPABLE = /[!"#$%&'()*+,\-./:;<=>?@[\\\]^_`{|}~]/;
const WORD = /[\p{L}\p{N}]/u;
const SPACE = /\s/;
const DESTINATION = /\s*<?([^()\s<>]*(?:\([^()\s]*\)[^()\s<>]*)*)>?(?:\s+"[^"\n]{0,300}")?\s*\)/y;
const AUTOLINK = /<((?:https?:\/\/|mailto:)[^\s<>]+)>/y;
const BARE_URL = /https?:\/\/[^\s<>]+/y;
/** Emphasis nested deeper than this is shown as text. */
const MAX_INLINE_DEPTH = 8;

const isSpace = (c: string | undefined) => c === undefined || SPACE.test(c);
const isWord = (c: string | undefined) => c !== undefined && WORD.test(c);

/** Text with line breaks: two trailing spaces or a backslash before a newline break the line; other newlines are spaces. */
function pushText(out: Inline[], text: string) {
  if (!text) return;
  let start = 0;
  for (let nl = text.indexOf("\n"); nl !== -1; nl = text.indexOf("\n", nl + 1)) {
    let end = nl;
    if (text[nl - 1] === "\\") end = nl - 1;
    else {
      while (end > start && text[end - 1] === " ") end--;
      if (nl - end < 2) continue;
    }
    pushLine(out, text.slice(start, end));
    out.push({ type: "break" });
    start = nl + 1;
  }
  pushLine(out, text.slice(start));
}

/** Text without hard breaks: a newline and the spaces and tabs around it become one space. */
function pushLine(out: Inline[], text: string) {
  let t = "";
  let from = 0;
  for (let nl = text.indexOf("\n"); nl !== -1; nl = text.indexOf("\n", from)) {
    let a = nl;
    while (a > from && (text[a - 1] === " " || text[a - 1] === "\t")) a--;
    let b = nl + 1;
    while (text[b] === " " || text[b] === "\t") b++;
    t += `${text.slice(from, a)} `;
    from = b;
  }
  t += text.slice(from);
  const last = out[out.length - 1];
  if (last?.type === "text") last.text += t;
  else if (t) out.push({ type: "text", text: t });
}

/** A bare address without the punctuation ending the sentence around it, or a ")" that closes nothing. */
function trimUrl(url: string): string {
  let open = 0;
  let close = 0;
  for (let k = 0; k < url.length; k++) {
    if (url[k] === "(") open++;
    else if (url[k] === ")") close++;
  }
  let end = url.length;
  while (end > 0) {
    const ch = url[end - 1];
    if (`.,:;!?'"*_~`.includes(ch)) end--;
    else if (ch === ")" && close > open) {
      end--;
      close--;
    } else break;
  }
  return url.slice(0, end);
}

export function parseInline(text: string, inLink = false, depth = 0): Inline[] {
  const out: Inline[] = [];
  if (depth > MAX_INLINE_DEPTH) {
    pushText(out, text);
    return out;
  }
  let pending = "";
  const emit = (node: Inline) => {
    pushText(out, pending);
    pending = "";
    out.push(node);
  };
  // The last search for each closer (`ok` depends only on the token and the place): one
  // that found `at` from `from` answers every start up to `at`; one that found nothing
  // (-1) answers every start after `from`.
  const searched = new Map<string, { from: number; at: number }>();
  const closer = (token: string, from: number, ok: (j: number) => boolean): number => {
    const last = searched.get(token);
    if (last && from >= last.from && (last.at === -1 || from <= last.at)) return last.at;
    let at = -1;
    for (let j = text.indexOf(token, from); j !== -1; j = text.indexOf(token, j + 1)) {
      if (ok(j)) {
        at = j;
        break;
      }
    }
    searched.set(token, { from, at });
    return at;
  };
  // Runs of backticks by length (their starts, in order), listed on the first code span.
  let ticks: Map<number, number[]> | null = null;
  /** Start of the first run of exactly `n` backticks at or after `from`, or -1. */
  const tickRun = (n: number, from: number): number => {
    if (!ticks) {
      ticks = new Map();
      for (let j = text.indexOf("`"); j !== -1; ) {
        let k = j + 1;
        while (text[k] === "`") k++;
        const starts = ticks.get(k - j);
        if (starts) starts.push(j);
        else ticks.set(k - j, [j]);
        j = text.indexOf("`", k);
      }
    }
    const starts = ticks.get(n) ?? [];
    let lo = 0;
    let hi = starts.length;
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      if (starts[mid] < from) lo = mid + 1;
      else hi = mid;
    }
    return lo < starts.length ? starts[lo] : -1;
  };
  // Destinations read so far, by the `]` they follow: many `[` can share one.
  const destinations = new Map<number, { href: string; end: number } | null>();
  /** `[text](destination)` starting at `at`: the text's end and the destination, or null. */
  const link = (at: number): { close: number; href: string; end: number } | null => {
    const close = closer("]", at + 1, (j) => text[j - 1] !== "\\");
    if (close === -1 || text[close + 1] !== "(") return null;
    let dest = destinations.get(close);
    if (dest === undefined) {
      DESTINATION.lastIndex = close + 2;
      const m = DESTINATION.exec(text);
      dest = m ? { href: m[1], end: DESTINATION.lastIndex } : null;
      destinations.set(close, dest);
    }
    return dest && { close, ...dest };
  };

  let i = 0;
  while (i < text.length) {
    const c = text[i];
    if (c === "\\" && ESCAPABLE.test(text[i + 1] ?? "")) {
      pending += text[i + 1];
      i += 2;
      continue;
    }

    if (c === "`") {
      let n = 1;
      while (text[i + n] === "`") n++;
      const j = tickRun(n, i + n);
      if (j === -1) {
        pending += "`".repeat(n);
        i += n;
        continue;
      }
      const code = text.slice(i + n, j).replace(/\n/g, " ");
      // One space on each side is padding, unless the code is only spaces.
      const padded = code.length > 2 && code[0] === " " && code[code.length - 1] === " " && /[^ ]/.test(code);
      emit({ type: "code", text: padded ? code.slice(1, -1) : code });
      i = j + n;
      continue;
    }

    if (c === "!" && text[i + 1] === "[") {
      const l = link(i + 1);
      if (l) {
        emit({ type: "image", alt: text.slice(i + 2, l.close).replace(/\\(.)/g, "$1"), src: l.href });
        i = l.end;
        continue;
      }
    }

    if (c === "[" && !inLink) {
      const l = link(i);
      if (l) {
        emit({ type: "link", href: safeHref(l.href), children: parseInline(text.slice(i + 1, l.close), true, depth + 1) });
        i = l.end;
        continue;
      }
    }

    if (c === "<" && !inLink) {
      AUTOLINK.lastIndex = i;
      const m = AUTOLINK.exec(text);
      if (m && m[1].length <= MAX_URL) {
        emit({ type: "link", href: safeHref(m[1]), children: [{ type: "text", text: m[1].replace(/^mailto:/i, "") }] });
        i = AUTOLINK.lastIndex;
        continue;
      }
    }

    if (c === "h" && !inLink && !isWord(text[i - 1])) {
      BARE_URL.lastIndex = i;
      const m = BARE_URL.exec(text);
      if (m && m[0].length > MAX_URL) {
        // Too long for an address: text (all of it, so it isn't read again from each "h").
        pending += m[0];
        i += m[0].length;
        continue;
      }
      // Trailing punctuation ends the sentence, not the address; so does a ")" that closes nothing.
      const url = m ? trimUrl(m[0]) : "";
      if (/^https?:\/\/./.test(url)) {
        emit({ type: "link", href: safeHref(url), children: [{ type: "text", text: url }] });
        i += url.length;
        continue;
      }
    }

    if (c === "~" && text[i + 1] === "~" && !isSpace(text[i + 2])) {
      const j = closer("~~", i + 3, (j) => !isSpace(text[j - 1]));
      if (j !== -1) {
        emit({ type: "del", children: parseInline(text.slice(i + 2, j), inLink, depth + 1) });
        i = j + 2;
        continue;
      }
    }

    if (c === "*" || c === "_") {
      let n = 1;
      while (text[i + n] === c) n++;
      // `_` only emphasises whole words: snake_case stays as it is.
      const opens = !isSpace(text[i + n]) && (c === "*" || !isWord(text[i - 1]));
      const closes = (j: number, size: number) => !isSpace(text[j - 1]) && (c === "*" || !isWord(text[j + size]));
      if (opens && n >= 2) {
        const j = closer(c + c, i + 3, (j) => closes(j, 2));
        if (j !== -1) {
          emit({ type: "strong", children: parseInline(text.slice(i + 2, j), inLink, depth + 1) });
          i = j + 2;
          continue;
        }
      }
      if (opens) {
        const j = closer(c, i + 2, (j) => text[j - 1] !== c && text[j + 1] !== c && closes(j, 1));
        if (j !== -1) {
          emit({ type: "em", children: parseInline(text.slice(i + 1, j), inLink, depth + 1) });
          i = j + 1;
          continue;
        }
      }
      pending += c.repeat(n);
      i += n;
      continue;
    }

    pending += c;
    i++;
  }
  pushText(out, pending);
  return out;
}

/** The plain text of inline content (for labels and tests). */
export function inlineText(nodes: Inline[]): string {
  return nodes
    .map((n) => (n.type === "text" || n.type === "code" ? n.text : n.type === "image" ? n.alt : n.type === "break" ? "\n" : inlineText(n.children)))
    .join("");
}

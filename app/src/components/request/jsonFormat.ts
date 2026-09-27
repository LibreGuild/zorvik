// JSON pretty-printing that keeps the text as typed: numbers (no float/bigint
// rounding), string escapes and {{variables}} (inside or outside strings).

const VAR_AT = /\{\{[^{}]*\}\}/y;

/** Length of the `{{variable}}` starting at `i`, or 0. */
function varAt(text: string, i: number): number {
  VAR_AT.lastIndex = i;
  return VAR_AT.exec(text)?.[0].length ?? 0;
}

/** End index (exclusive) of the string literal starting at `i` (a `"`). */
function stringEnd(text: string, i: number): number {
  let j = i + 1;
  while (j < text.length && text[j] !== '"') j += text[j] === "\\" ? 2 : 1;
  return j + 1;
}

/** Re-indent JSON with two spaces; null when it isn't valid JSON (variables count as values). */
export function beautifyJson(text: string): string | null {
  // Validate with variables outside strings replaced by a string (valid as a key or a value).
  let masked = "";
  for (let i = 0; i < text.length; ) {
    if (text[i] === '"') {
      const end = stringEnd(text, i);
      masked += text.slice(i, end);
      i = end;
    } else if (varAt(text, i)) {
      masked += '""';
      i += varAt(text, i);
    } else {
      masked += text[i++];
    }
  }
  try {
    JSON.parse(masked);
  } catch {
    return null;
  }

  let out = "";
  let depth = 0;
  const newline = () => `\n${"  ".repeat(depth)}`;
  for (let i = 0; i < text.length; ) {
    const c = text[i];
    if (c === '"') {
      const end = stringEnd(text, i);
      out += text.slice(i, end);
      i = end;
    } else if (varAt(text, i)) {
      out += text.slice(i, i + varAt(text, i));
      i += varAt(text, i);
    } else if (c === "{" || c === "[") {
      let j = i + 1;
      while (/\s/.test(text[j] ?? "")) j++;
      if (text[j] === (c === "{" ? "}" : "]")) {
        out += c + text[j];
        i = j + 1;
      } else {
        depth++;
        out += c + newline();
        i++;
      }
    } else if (c === "}" || c === "]") {
      depth--;
      out += newline() + c;
      i++;
    } else if (c === ",") {
      out += `,${newline()}`;
      i++;
    } else if (c === ":") {
      out += ": ";
      i++;
    } else {
      if (!/\s/.test(c)) out += c;
      i++;
    }
  }
  return out;
}

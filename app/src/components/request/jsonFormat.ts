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

/** Text that looks like a JSON object or array. */
function looksLikeJson(text: string): boolean {
  const t = text.trim();
  return (t.startsWith("{") && t.endsWith("}")) || (t.startsWith("[") && t.endsWith("]"));
}

/**
 * A received message, pretty: JSON re-indented, and string values that hold JSON themselves
 * (a Socket.IO argument sent as `JSON.stringify(...)`, a message wrapped in another) expanded in
 * place. Null when the text isn't JSON. Without such strings the text keeps its numbers exactly
 * (see `beautifyJson`); with them, it is parsed (integers beyond 2^53 are rounded).
 */
export function prettyMessage(text: string): string | null {
  const plain = beautifyJson(text);
  if (plain === null) return null;
  let nested = false;
  const expand = (value: unknown, depth: number): unknown => {
    if (typeof value === "string" && depth < 20 && looksLikeJson(value)) {
      try {
        const inner: unknown = JSON.parse(value);
        nested = true;
        return expand(inner, depth + 1);
      } catch {
        return value;
      }
    }
    if (Array.isArray(value)) return value.map((v) => expand(v, depth));
    if (value && typeof value === "object") {
      return Object.fromEntries(Object.entries(value).map(([k, v]) => [k, expand(v, depth)]));
    }
    return value;
  };
  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch {
    // Variables outside strings: valid for beautifyJson only.
    return plain;
  }
  const expanded = expand(parsed, 0);
  return nested ? JSON.stringify(expanded, null, 2) : plain;
}

export interface Segment {
  text: string;
  /** Variable name when this segment is a `{{var}}` reference. */
  variable?: string;
}

const VAR_RE = /\{\{\s*([^{}\n]+?)\s*\}\}/g;

export function segments(text: string): Segment[] {
  const out: Segment[] = [];
  let last = 0;
  for (const m of text.matchAll(VAR_RE)) {
    const start = m.index ?? 0;
    if (start > last) out.push({ text: text.slice(last, start) });
    out.push({ text: m[0], variable: m[1].trim() });
    last = start + m[0].length;
  }
  if (last < text.length) out.push({ text: text.slice(last) });
  return out;
}

export function variableNames(text: string): string[] {
  return [...text.matchAll(VAR_RE)].map((m) => m[1].trim());
}

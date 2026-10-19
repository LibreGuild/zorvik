// Logic of the offline tools' views (JSON compare, filter playground, regex tester,
// certificate decoder). The work itself happens in Rust; this shapes what is shown.
import type { DecodedItem } from "../../bindings/DecodedItem";
import type { JsonDiff } from "../../bindings/JsonDiff";
import type { JsonDiffKind } from "../../bindings/JsonDiffKind";
import type { JsonDifference } from "../../bindings/JsonDifference";
import type { RegexFlags } from "../../bindings/RegexFlags";
import type { RegexMatch } from "../../bindings/RegexMatch";
import type { FilterKind } from "../response/filterModel";

// ---- JSON compare ------------------------------------------------------------------

export type DiffView = "all" | JsonDiffKind;

export const DIFF_LABEL: Record<JsonDiffKind, string> = { changed: "Changed", added: "Added", removed: "Removed" };

export function visibleDifferences(diff: JsonDiff, view: DiffView): JsonDifference[] {
  return view === "all" ? diff.differences : diff.differences.filter((d) => d.kind === view);
}

export function diffCount(diff: JsonDiff, view: DiffView): number {
  return view === "all" ? diff.added + diff.removed + diff.changed : diff[view];
}

/** The differences as plain text, one per line: `~ $.id: 7 → "7"`. */
export function diffAsText(differences: JsonDifference[]): string {
  return differences
    .map((d) =>
      d.kind === "added" ? `+ ${d.path}: ${d.right}` : d.kind === "removed" ? `- ${d.path}: ${d.left}` : `~ ${d.path}: ${d.left} → ${d.right}`,
    )
    .join("\n");
}

// ---- filter playground ---------------------------------------------------------------

/** Ready-made expressions to start from, per language. */
export const FILTER_EXAMPLES: Record<FilterKind, { expression: string; label: string }[]> = {
  jsonPath: [
    { expression: "$.*", label: "Top-level values" },
    { expression: "$..id", label: "Every id, at any depth" },
    { expression: "$.items[0]", label: "First item" },
    { expression: "$.items[-1]", label: "Last item" },
    { expression: "$.items[?@.price > 10].name", label: "Names of items over 10" },
    { expression: "$.items[?length(@.tags) > 0]", label: "Items with tags" },
  ],
  jq: [
    { expression: "keys", label: "Top-level keys" },
    { expression: ".items | length", label: "Number of items" },
    { expression: ".items[] | select(.price > 10) | .name", label: "Names of items over 10" },
    { expression: ".items | map({id, name})", label: "Only id and name" },
    { expression: "[.items[].price] | add", label: "Sum of prices" },
    { expression: ".items | sort_by(.price) | reverse | .[0]", label: "Most expensive item" },
  ],
  xpath: [
    { expression: "//item/name", label: "Every item's name" },
    { expression: "count(//item)", label: "Number of items" },
    { expression: "//item[price > 10]", label: "Items over 10" },
    { expression: "//*[@id]", label: "Elements with an id" },
  ],
};

/** The Rust filter speaks of "the response"; here the text was pasted. */
export function filterError(message: string): string {
  return message.replace(/^The response /, "The text ");
}

/** HTML is parsed as HTML (lenient); other text as XML. */
export function looksLikeHtml(text: string): boolean {
  return /^\s*(<!doctype html|<html[\s>])/i.test(text);
}

// ---- regex tester ------------------------------------------------------------------

export const REGEX_FLAGS: { key: keyof RegexFlags; letter: string; label: string }[] = [
  { key: "ignoreCase", letter: "i", label: "Ignore case" },
  { key: "multiLine", letter: "m", label: "^ and $ match at each line" },
  { key: "dotAll", letter: "s", label: ". matches line breaks" },
  { key: "ignoreWhitespace", letter: "x", label: "Ignore whitespace and # comments in the pattern" },
];

export const NO_FLAGS: RegexFlags = { ignoreCase: false, multiLine: false, dotAll: false, ignoreWhitespace: false };

/** Ranges to highlight in the text: each match, clamped to the text and without overlaps. */
export function matchRanges(matches: RegexMatch[], length: number): { from: number; to: number; index: number }[] {
  const out: { from: number; to: number; index: number }[] = [];
  let last = 0;
  matches.forEach((m, index) => {
    const from = Math.max(m.start, last);
    const to = Math.min(m.end, length);
    if (to > from) {
      out.push({ from, to, index });
      last = to;
    }
  });
  return out;
}

/** A value for display: line breaks and tabs made visible, empty marked. */
export function showText(text: string): string {
  if (text === "") return "(empty)";
  return text.replace(/\r/g, "\\r").replace(/\n/g, "\\n").replace(/\t/g, "\\t");
}

// ---- certificate decoder -----------------------------------------------------------------

/** A short title for a decoded item. */
export function itemTitle(item: DecodedItem): string {
  if (item.certificate) return item.certificate.details.commonName ?? (item.certificate.details.subject || "Certificate");
  if (item.request) return item.request.commonName ?? (item.request.subject || "Certificate request");
  if (item.key) return keyDescription(item.key.keyType, item.key.keyBits, item.key.keyCurve) || (item.key.private ? "Private key" : "Public key");
  return item.label || "Unknown block";
}

/** `EC · P-256 · 256 bits` */
export function keyDescription(type: string, bits: number | null, curve: string | null): string {
  return [type, curve, bits ? `${bits} bits` : null].filter(Boolean).join(" · ");
}

/** The kind of an item, as a caption: `Server certificate`, `Root certificate`, … */
export function itemRole(items: DecodedItem[], index: number): string {
  const item = items[index];
  if (item.certificate) {
    const d = item.certificate.details;
    if (d.selfSigned && d.isCa) return "Root certificate";
    if (d.isCa) return "Intermediate certificate";
    return "Certificate";
  }
  if (item.kind === "request") return "Certificate request";
  if (item.kind === "privateKey") return item.key?.encrypted ? "Encrypted private key" : "Private key";
  if (item.kind === "publicKey") return "Public key";
  return item.label || "Other";
}

/** Which items a key belongs to, in words: `certificate 1 (api.example.com)`. */
export function keyOwners(items: DecodedItem[], matches: number[]): string {
  return matches
    .map((i) => {
      const it = items[i];
      if (!it) return null;
      const what = it.kind === "request" ? "request" : "certificate";
      return `${what} ${i + 1} (${itemTitle(it)})`;
    })
    .filter(Boolean)
    .join(", ");
}

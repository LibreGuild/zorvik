// Pure helpers for the API catalog (Import → API catalog): category labels, search, the labels
// and wording an entry shows, and picking groups of a large API. No React, no store.
import type { CatalogCategory } from "../../bindings/CatalogCategory";
import type { CatalogEntry } from "../../bindings/CatalogEntry";
import type { CatalogLicense } from "../../bindings/CatalogLicense";
import type { CatalogPreview } from "../../bindings/CatalogPreview";
import type { OperationGroup } from "../../bindings/OperationGroup";

/** Categories in the order the chips show them. */
export const CATEGORIES: { id: CatalogCategory; label: string }[] = [
  { id: "ai", label: "AI" },
  { id: "cloud", label: "Cloud" },
  { id: "devTools", label: "Developer tools" },
  { id: "monitoring", label: "Monitoring" },
  { id: "data", label: "Databases & search" },
  { id: "messaging", label: "Messaging & email" },
  { id: "productivity", label: "Productivity" },
  { id: "payments", label: "Payments" },
  { id: "commerce", label: "Commerce" },
  { id: "marketing", label: "CRM & marketing" },
  { id: "identity", label: "Identity" },
  { id: "media", label: "Maps & media" },
];

export const CATEGORY_LABELS = Object.fromEntries(CATEGORIES.map((c) => [c.id, c.label])) as Record<CatalogCategory, string>;

export type CategoryFilter = CatalogCategory | "all";

/** Larger APIs than this (and in more than one group) let the user pick the groups to import. */
export const PICK_GROUPS_ABOVE = 300;

function words(query: string): string[] {
  return query.toLowerCase().split(/\s+/).filter(Boolean);
}

function matches(entry: CatalogEntry, terms: string[]): boolean {
  const text = `${entry.name} ${entry.id} ${entry.description} ${CATEGORY_LABELS[entry.category] ?? ""}`.toLowerCase();
  return terms.every((t) => text.includes(t));
}

const byName = (a: CatalogEntry, b: CatalogEntry) => a.name.localeCompare(b.name, undefined, { sensitivity: "base" });

/**
 * The entries in `category` matching every word of `query` (in the name, id, description or
 * category), by name; while searching, names that start with the query come first.
 */
export function searchCatalog(entries: CatalogEntry[], query: string, category: CategoryFilter): CatalogEntry[] {
  const terms = words(query);
  const found = entries.filter((e) => (category === "all" || e.category === category) && matches(e, terms));
  const q = terms.join(" ");
  const rank = (e: CatalogEntry) => {
    if (!q) return 0;
    const name = e.name.toLowerCase();
    return name.startsWith(q) || e.id === q ? 0 : name.includes(q) ? 1 : 2;
  };
  return found.sort((a, b) => rank(a) - rank(b) || byName(a, b));
}

/** How many entries match `query` in each category, and in all of them. */
export function categoryCounts(entries: CatalogEntry[], query: string): Record<CategoryFilter, number> {
  const terms = words(query);
  const counts = Object.fromEntries([["all", 0], ...CATEGORIES.map((c) => [c.id, 0])]) as Record<CategoryFilter, number>;
  for (const e of entries) {
    if (!matches(e, terms)) continue;
    counts.all += 1;
    counts[e.category] = (counts[e.category] ?? 0) + 1;
  }
  return counts;
}

/** The small labels a list row shows: the format, "OpenAI-compatible", "Link only". */
export function entryLabels(entry: CatalogEntry): string[] {
  const labels = [entry.format];
  if (entry.source.type === "compatible") labels.push("OpenAI-compatible");
  if (entry.source.type === "link") labels.push("Link only");
  return labels.filter((l, i) => l.trim() && labels.findIndex((x) => x.toLowerCase() === l.toLowerCase()) === i);
}

/** The host of `url` ("raw.githubusercontent.com"), or the text itself when it isn't a URL. */
export function hostOf(url: string): string {
  try {
    return new URL(url).host || url;
  } catch {
    return url;
  }
}

/** `url` when it is a web address (http or https), the only links the catalog opens; otherwise null. */
export function webUrl(url: string | undefined): string | null {
  if (!url) return null;
  try {
    const { protocol } = new URL(url);
    return protocol === "https:" || protocol === "http:" ? url : null;
  } catch {
    return null;
  }
}

/** The entry whose description `entry` uses (an OpenAI-compatible API uses OpenAI's). */
function owner(entry: CatalogEntry, entries: CatalogEntry[]): CatalogEntry | undefined {
  const source = entry.source;
  return source.type === "compatible" ? entries.find((e) => e.id === source.of) : entry;
}

/** Where the URL of the description `entry` imports is, when it is downloaded. */
export function downloadUrl(entry: CatalogEntry, entries: CatalogEntry[]): string | null {
  const source = owner(entry, entries)?.source;
  return source?.type === "download" ? source.url : null;
}

/** Where an entry's requests come from, in a sentence; null for link-only entries. */
export function sourceText(entry: CatalogEntry, entries: CatalogEntry[]): string | null {
  switch (entry.source.type) {
    case "download":
      return `Downloads from ${hostOf(entry.source.url)}`;
    case "compatible":
      return `Uses the ${owner(entry, entries)?.name ?? entry.source.of} API description`;
    case "starter":
      return "Written for Zorvik";
    case "graphql":
      return "One GraphQL request; the editor completes from the schema";
    case "link":
      return null;
  }
}

/** What the window says while an import starts. */
export function progressText(entry: CatalogEntry, entries: CatalogEntry[]): string {
  const url = downloadUrl(entry, entries);
  return url ? `Downloading from ${hostOf(url)}…` : `Importing ${entry.name}…`;
}

/** The license of the description an entry imports (an OpenAI-compatible API: OpenAI's). */
export function licenseOf(entry: CatalogEntry, entries: CatalogEntry[]): CatalogLicense | undefined {
  return entry.license ?? (entry.source.type === "compatible" ? owner(entry, entries)?.license : undefined);
}

// ---- groups of a large API ---------------------------------------------------------------

/** The user picks groups when the API is large and comes in several of them. */
export function needsPicking(preview: CatalogPreview): boolean {
  return preview.groups.length > 1 && preview.operations > PICK_GROUPS_ABOVE;
}

/** Groups ticked to begin with: the entry's suggestion, or all of them. */
export function initialTicked(preview: CatalogPreview): Set<string> {
  const known = new Set(preview.groups.map((g) => g.tag));
  const suggested = preview.selected.filter((t) => known.has(t));
  return new Set(suggested.length ? suggested : preview.groups.map((g) => g.tag));
}

/** A group's name as shown ("" is the operations without one). */
export function groupLabel(tag: string): string {
  return tag || "Other";
}

export function filterGroups(groups: OperationGroup[], query: string): OperationGroup[] {
  const terms = words(query);
  return groups.filter((g) => terms.every((t) => groupLabel(g.tag).toLowerCase().includes(t)));
}

/** Requests the ticked groups bring in. */
export function tickedTotal(groups: OperationGroup[], ticked: Set<string>): number {
  return groups.reduce((n, g) => n + (ticked.has(g.tag) ? g.operations : 0), 0);
}

/** Tick (or untick) every group in `shown`, keeping the others as they are. */
export function tickAll(ticked: Set<string>, shown: OperationGroup[], on: boolean): Set<string> {
  const next = new Set(ticked);
  for (const g of shown) {
    if (on) next.add(g.tag);
    else next.delete(g.tag);
  }
  return next;
}

/** The ticked groups, in the order the API lists them. */
export function tickedTags(groups: OperationGroup[], ticked: Set<string>): string[] {
  return groups.filter((g) => ticked.has(g.tag)).map((g) => g.tag);
}

export function requestCount(n: number): string {
  return `${n.toLocaleString("en-US")} ${n === 1 ? "request" : "requests"}`;
}

/** `path` and each folder above it, outermost first ("A/B" → ["A", "A/B"]), to reveal it in the tree. */
export function foldersTo(path: string): string[] {
  const parts = path.split("/").filter(Boolean);
  return parts.map((_, i) => parts.slice(0, i + 1).join("/"));
}

// GraphQL schemas for the body editor, introspected through the request's own URL, headers
// and auth (`graphql.schema`), built with graphql-js and kept in memory per URL and environment
// until refreshed. Also the Schema panel's layout and per-tab navigation, and editor helpers
// (operation names, prettify, when a URL may be introspected without asking).
import { buildClientSchema, type GraphQLSchema, type IntrospectionQuery, Kind, Lexer, parse, print, Source, type Token, TokenKind } from "graphql";
import { create } from "zustand";
import type { Request } from "../bindings/Request";
import { api, errorMessage } from "../lib/rpc";

export interface SchemaEntry {
  status: "loading" | "loaded" | "error";
  /** Last schema that loaded (kept while refreshing and after a failed refresh). */
  schema: GraphQLSchema | null;
  /** Introspected URL (variables resolved). */
  url: string | null;
  fetchedAt: number | null;
  /** The server only understood the older introspection query. */
  legacy: boolean;
  error: string | null;
}

interface GraphqlState {
  /** By schemaKey(). */
  schemas: Record<string, SchemaEntry>;
  /** The Schema panel is shown (in every GraphQL editor). */
  panelOpen: boolean;
  panelWidth: number;
  /** Fraction of the editor height given to the query (the rest shows variables). */
  querySplit: number;
  /** Types opened in each tab's Schema panel, newest last (empty: the root types). */
  docs: Record<string, string[]>;
}

const STORAGE_KEY = "zv:graphql";

function loadLayout(): Partial<Pick<GraphqlState, "panelOpen" | "panelWidth" | "querySplit">> {
  try {
    return JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "{}") as Partial<GraphqlState>;
  } catch {
    return {};
  }
}

const saved = loadLayout();

export const useGraphql = create<GraphqlState>(() => ({
  schemas: {},
  panelOpen: saved.panelOpen ?? false,
  panelWidth: saved.panelWidth ?? 300,
  querySplit: saved.querySplit ?? 0.65,
  docs: {},
}));

const set = useGraphql.setState;

useGraphql.subscribe((s) => {
  try {
    const { panelOpen, panelWidth, querySplit } = s;
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ panelOpen, panelWidth, querySplit }));
  } catch {
    /* storage unavailable */
  }
});

/**
 * Cache key of a request's schema: its URL as typed, in the open workspace (the same
 * `{{baseUrl}}` differs between workspaces) and the active environment.
 */
export function schemaKey(workspace: string | null | undefined, url: string, environment: string | null | undefined): string {
  return `${workspace ?? ""}\n${environment ?? ""}\n${url.trim()}`;
}

const EMPTY: SchemaEntry = { status: "loading", schema: null, url: null, fetchedAt: null, legacy: false, error: null };

/** Schemas kept (large ones take tens of MB); the least recently loaded go first. */
export const MAX_SCHEMAS = 16;

function setEntry(key: string, fn: (e: SchemaEntry) => SchemaEntry) {
  set((s) => {
    const { [key]: current, ...others } = s.schemas;
    const keys = Object.keys(others);
    for (const old of keys.slice(0, Math.max(0, keys.length - (MAX_SCHEMAS - 1)))) delete others[old];
    return { schemas: { ...others, [key]: fn(current ?? EMPTY) } };
  });
}

const pending = new Map<string, Promise<void>>();
/** Latest load per key: an older load that finishes later is dropped. */
const latest = new Map<string, number>();
let loads = 0;

/** Introspect the request's schema (the backend answers from its cache unless `refresh`). */
export function loadSchema(key: string, request: Request, path: string | null, refresh = false): Promise<void> {
  const running = pending.get(key);
  if (running && !refresh) return running;
  const id = ++loads;
  latest.set(key, id);
  setEntry(key, (e) => ({ ...e, status: "loading", error: null }));
  const done = api
    .graphqlSchema(request, path, refresh)
    .then(
      (result) => {
        if (latest.get(key) !== id) return;
        let schema: GraphQLSchema;
        try {
          schema = buildClientSchema(result.data as unknown as IntrospectionQuery);
        } catch (e) {
          const message = `The server's schema could not be read: ${(e as Error).message}`;
          setEntry(key, (prev) => ({ ...prev, status: "error", error: message }));
          return;
        }
        setEntry(key, () => ({ status: "loaded", schema, url: result.url, fetchedAt: result.fetchedAt, legacy: result.legacy, error: null }));
      },
      (e) => {
        if (latest.get(key) === id) setEntry(key, (prev) => ({ ...prev, status: "error", error: errorMessage(e) }));
      },
    )
    .finally(() => {
      if (pending.get(key) === done) pending.delete(key);
    });
  pending.set(key, done);
  return done;
}

// ---- Schema panel -------------------------------------------------------------

export function setPanelOpen(panelOpen: boolean) {
  set({ panelOpen });
}

/** Open `typeName` in the tab's Schema panel (and show the panel). */
export function showType(tabId: string, typeName: string) {
  set((s) => {
    const stack = s.docs[tabId] ?? [];
    if (stack[stack.length - 1] === typeName) return { panelOpen: true };
    return { panelOpen: true, docs: { ...s.docs, [tabId]: [...stack, typeName].slice(-50) } };
  });
}

/** Back one type; `home` goes to the root types. */
export function docsBack(tabId: string, home = false) {
  set((s) => ({ docs: { ...s.docs, [tabId]: home ? [] : (s.docs[tabId] ?? []).slice(0, -1) } }));
}

/** `[User!]!` → `User`. */
export const namedType = (type: string) => type.replace(/[[\]!\s]/g, "");

// ---- editor helpers -------------------------------------------------------------

/** Names of the query's named operations, in order; null when it doesn't parse. */
export function operationNames(query: string): string[] | null {
  if (!query.trim()) return [];
  try {
    const doc = parse(query, { noLocation: true });
    return doc.definitions.flatMap((d) => (d.kind === Kind.OPERATION_DEFINITION && d.name ? [d.name.value] : []));
  } catch {
    return null;
  }
}

/** The operation name to keep after an edit: a name that no longer exists is dropped. */
export function keptOperationName(query: string, current: string | null | undefined): string | undefined {
  if (!current) return undefined;
  const names = operationNames(query);
  // Mid-edit (doesn't parse): leave it alone.
  return names === null || names.includes(current) ? current : undefined;
}

/** The query reformatted, or an error message. graphql-js drops comments; `comments` says there were some. */
export function prettifyQuery(query: string): { text: string; comments: boolean } | { error: string } {
  try {
    const text = print(parse(query));
    return { text, comments: hasComments(query) };
  } catch (e) {
    return { error: (e as Error).message };
  }
}

function hasComments(query: string): boolean {
  const lexer = new Lexer(new Source(query));
  const start = lexer.token;
  while (lexer.advance().kind !== TokenKind.EOF);
  // advance() skips comments, but they stay in the token list.
  for (let token: Token | null = start; token; token = token.next) {
    if (token.kind === TokenKind.COMMENT) return true;
  }
  return false;
}

/**
 * Whether a URL may be introspected without asking: it is set, and every {{variable}} in its
 * host is defined (an undefined one would only fail with a confusing error).
 */
export function canAutoLoad(url: string, known: Set<string>): boolean {
  const u = url.trim();
  if (!u) return false;
  const scheme = u.match(/^([a-z][a-z0-9+.-]*):\/\//i);
  if (scheme && !/^https?$/i.test(scheme[1])) return false;
  const host = (scheme ? u.slice(scheme[0].length) : u).split(/[/?#]/)[0];
  if (!host) return false;
  for (const m of host.matchAll(/\{\{\s*([^{}\s]+)\s*\}\}/g)) {
    if (!m[1].startsWith("$") && !known.has(m[1])) return false;
  }
  return !/\{\{|\}\}/.test(host.replace(/\{\{\s*[^{}\s]+\s*\}\}/g, ""));
}

// URL <-> query-params table sync. The raw URL (as typed) is the source of
// truth for enabled params; disabled params live in `disabledParams`.
import type { KeyValue } from "../bindings/KeyValue";

export interface UrlParts {
  base: string;
  query: string | null;
  hash: string;
}

export function splitUrl(url: string): UrlParts {
  let rest = url;
  let hash = "";
  const h = rest.indexOf("#");
  if (h >= 0) {
    hash = rest.slice(h);
    rest = rest.slice(0, h);
  }
  const q = rest.indexOf("?");
  if (q < 0) return { base: rest, query: null, hash };
  return { base: rest.slice(0, q), query: rest.slice(q + 1), hash };
}

export function parseQuery(query: string | null): KeyValue[] {
  if (!query) return [];
  return query
    .split("&")
    .filter((p) => p.length > 0)
    .map((p) => {
      const eq = p.indexOf("=");
      return eq < 0 ? { key: p, value: "" } : { key: p.slice(0, eq), value: p.slice(eq + 1) };
    });
}

const escapeKey = (s: string) => s.replace(/[&#=]/g, (c) => encodeURIComponent(c));
const escapeValue = (s: string) => s.replace(/[&#]/g, (c) => encodeURIComponent(c));

function buildQuery(params: KeyValue[]): string {
  return params.map((p) => (p.value === "" ? escapeKey(p.key) : `${escapeKey(p.key)}=${escapeValue(p.value)}`)).join("&");
}

/** Table rows: enabled params parsed from the URL, then disabled ones. */
export function paramRows(url: string, disabled: KeyValue[] | undefined): KeyValue[] {
  const enabled = parseQuery(splitUrl(url).query);
  return [...enabled, ...(disabled ?? []).map((d) => ({ ...d, enabled: false }))];
}

/** Rebuild the URL and disabled list from edited table rows. */
export function applyParamRows(url: string, rows: KeyValue[]): { url: string; disabledParams: KeyValue[] } {
  const { base, hash } = splitUrl(url);
  const on = rows.filter((r) => r.enabled !== false && (r.key !== "" || r.value !== ""));
  const off = rows.filter((r) => r.enabled === false && (r.key !== "" || r.value !== ""));
  const query = buildQuery(on);
  return {
    url: `${base}${on.length ? `?${query}` : ""}${hash}`,
    disabledParams: off.map(({ key, value, description }) => ({ key, value, enabled: false, ...(description ? { description } : {}) })),
  };
}

/** `:name` segments in the URL path (not the host:port). */
export function pathParamNames(url: string): string[] {
  const { base } = splitUrl(url);
  const afterScheme = base.includes("://") ? base.slice(base.indexOf("://") + 3) : base;
  const slash = afterScheme.indexOf("/");
  if (slash < 0) return [];
  const names: string[] = [];
  for (const seg of afterScheme.slice(slash + 1).split("/")) {
    if (seg.startsWith(":") && seg.length > 1) {
      const name = seg.slice(1);
      if (!names.includes(name)) names.push(name);
    }
  }
  return names;
}

/** Keep path-param values for names still in the URL, add new ones in order. */
export function syncPathParams(url: string, existing: KeyValue[] | undefined): KeyValue[] {
  const names = pathParamNames(url);
  return names.map((name) => existing?.find((p) => p.key === name) ?? { key: name, value: "" });
}

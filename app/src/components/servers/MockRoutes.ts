// Pure helpers for mock routes: paths from request URLs, a route from a
// response or from a proxied answer in the traffic, labels and list moves.
// Kept free of React for unit tests.
import type { Header } from "../../bindings/Header";
import type { HttpExchange } from "../../bindings/HttpExchange";
import type { KeyValue } from "../../bindings/KeyValue";
import type { MockRoute } from "../../bindings/MockRoute";
import type { Request } from "../../bindings/Request";
import type { SendResult } from "../../bindings/SendResult";
import type { TrafficEntry } from "../../bindings/TrafficEntry";
import { parseCondition } from "../../lib/messageMatch";
import type { EditorLanguage } from "../CodeEditor";

/** Common answers offered in the status field. */
export const STATUS_CODES: { code: number; label: string }[] = [
  { code: 200, label: "OK" },
  { code: 201, label: "Created" },
  { code: 202, label: "Accepted" },
  { code: 204, label: "No Content" },
  { code: 301, label: "Moved Permanently" },
  { code: 302, label: "Found" },
  { code: 304, label: "Not Modified" },
  { code: 400, label: "Bad Request" },
  { code: 401, label: "Unauthorized" },
  { code: 403, label: "Forbidden" },
  { code: 404, label: "Not Found" },
  { code: 405, label: "Method Not Allowed" },
  { code: 409, label: "Conflict" },
  { code: 422, label: "Unprocessable Content" },
  { code: 429, label: "Too Many Requests" },
  { code: 500, label: "Internal Server Error" },
  { code: 502, label: "Bad Gateway" },
  { code: 503, label: "Service Unavailable" },
  { code: 504, label: "Gateway Timeout" },
];

/** Methods offered for a route; `*` answers any method. */
export const ROUTE_METHODS = ["*", "GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];

export const methodText = (method: string | undefined) => {
  const m = (method ?? "").trim();
  return !m || m === "*" ? "ANY" : m.toUpperCase();
};

/** Index of the `}}` closing a `{{` at `open`, or -1. */
function closeOf(s: string, open: number): number {
  return s.indexOf("}}", open + 2);
}

/** First index of `ch` outside `{{…}}`, or -1. */
function indexOutsideBraces(s: string, ch: string): number {
  for (let i = 0; i < s.length; i++) {
    if (s.startsWith("{{", i)) {
      const end = closeOf(s, i);
      if (end < 0) return s.indexOf(ch, i);
      i = end + 1;
    } else if (s[i] === ch) {
      return i;
    }
  }
  return -1;
}

function segment(s: string): string {
  const open = s.indexOf("{{");
  const end = open >= 0 ? closeOf(s, open) : -1;
  let name: string | null = null;
  if (open >= 0 && end > open) name = s.slice(open + 2, end).trim();
  else if (s.startsWith("{") && s.endsWith("}") && s.length > 2) name = s.slice(1, -1).trim();
  if (name === null) return s;
  const clean = name.replace(/^\$+/, "").replace(/[^A-Za-z0-9_-]/g, "_");
  return clean ? `:${clean}` : ":param";
}

/**
 * The route path for a request URL (mirrors `route_path` in crates/formats/src/mock.rs):
 * scheme, host and a leading `{{baseUrl}}` are dropped, and so are query and fragment;
 * `{{id}}` / `{id}` segments become `:id`.
 */
export function routePathFromUrl(url: string): string {
  let u = url.trim();
  const hash = u.indexOf("#");
  if (hash >= 0) u = u.slice(0, hash);
  const q = indexOutsideBraces(u, "?");
  if (q >= 0) u = u.slice(0, q);
  let path: string;
  const scheme = u.indexOf("://");
  if (scheme >= 0 && !u.slice(0, scheme).includes("/")) {
    const rest = u.slice(scheme + 3);
    const slash = rest.indexOf("/");
    path = slash >= 0 ? rest.slice(slash) : "";
  } else if (u.startsWith("/")) {
    path = u;
  } else {
    const slash = indexOutsideBraces(u, "/");
    path = slash >= 0 ? u.slice(slash) : "";
  }
  const segments = path.split("/").filter(Boolean).map(segment);
  return `/${segments.join("/")}`;
}

/** Parameter names of a route path (`:id`, `{id}`, and `*` for a trailing wildcard). */
export function pathParams(path: string): string[] {
  const names: string[] = [];
  const parts = path.split("?")[0].split("/").filter(Boolean);
  parts.forEach((p, i) => {
    const name = p === "*" && i === parts.length - 1 ? "*" : paramName(p);
    if (name && !names.includes(name)) names.push(name);
  });
  return names;
}

/** The parameter a route path segment names, as the server matches it (`param_segment` in
 * crates/servers/src/http.rs): `:id`, `{id}`, and a name with a custom method after it, as in
 * Google APIs (`:name:generateContent`, `{name}:cancel`). */
function paramName(segment: string): string | null {
  let whole: string;
  let name: string;
  let suffix: string;
  if (segment.startsWith(":")) {
    whole = segment.slice(1);
    const colon = whole.indexOf(":");
    [name, suffix] = colon < 0 ? [whole, ""] : [whole.slice(0, colon), whole.slice(colon)];
  } else if (segment.startsWith("{") && !segment.startsWith("{{")) {
    const rest = segment.slice(1);
    const close = rest.indexOf("}");
    if (close < 0) return null;
    whole = rest.endsWith("}") ? rest.slice(0, -1) : rest;
    [name, suffix] = [rest.slice(0, close), rest.slice(close + 1)];
  } else {
    return null;
  }
  // A verb follows a name (`:name:verb`); `name.:ext` or `a::b` start another parameter.
  const another = /[:{]/.test(suffix.slice(1)) || (suffix.startsWith(":") && !/[\p{L}\p{N}_]$/u.test(name));
  return (another ? whole : name) || null;
}

/** `{{request.…}}` names a route's templates can use (for autocomplete and highlighting). */
export function requestPlaceholders(route: MockRoute): string[] {
  const names = ["request.method", "request.path", "request.url", "request.body", "request.json"];
  for (const p of pathParams(route.path)) names.push(`request.params.${p}`);
  for (const q of route.matchQuery ?? []) if (q.key.trim()) names.push(`request.query.${q.key.trim()}`);
  for (const h of route.matchHeaders ?? []) if (h.key.trim()) names.push(`request.headers.${h.key.trim().toLowerCase()}`);
  // The value the JSON condition looks at: `$.user.id == 7` offers `request.json.user.id`.
  const condition = route.matchJson?.trim() ? parseCondition(route.matchJson) : null;
  if (condition && typeof condition !== "string" && condition.steps.length) {
    const path = condition.steps.map((s) => (typeof s === "number" ? `[${s}]` : /^[A-Za-z_][\w-]*$/.test(s) ? `.${s}` : `['${s}']`)).join("");
    names.push(`request.json${path}`);
  }
  return [...new Set(names)];
}

export function contentTypeOf(route: MockRoute): string {
  return route.headers?.find((h) => h.enabled !== false && h.key.trim().toLowerCase() === "content-type")?.value ?? "";
}

export function languageForContentType(contentType: string, body = ""): EditorLanguage {
  const ct = contentType.toLowerCase();
  if (ct.includes("json")) return "json";
  if (ct.includes("html")) return "html";
  if (ct.includes("xml")) return "xml";
  if (ct.includes("javascript")) return "javascript";
  if (!ct) {
    const t = body.trimStart();
    if (t.startsWith("{") || t.startsWith("[")) return "json";
    if (t.startsWith("<")) return /<html|<!doctype/i.test(t.slice(0, 200)) ? "html" : "xml";
  }
  return "text";
}

/** The full address of a route on a running server (parameters left as written). */
export function routeUrl(base: string, path: string): string {
  const p = path.trim() || "/";
  return `${base.replace(/\/+$/, "")}${p.startsWith("/") ? p : `/${p}`}`;
}

/** Response headers not replayed by a mock: connection-level ones, framing, and ones that would stick to localhost. */
const SKIPPED_HEADERS = new Set([
  "connection",
  "keep-alive",
  "proxy-connection",
  "transfer-encoding",
  "upgrade",
  "te",
  "trailer",
  "content-length",
  "content-encoding",
  "date",
  "alt-svc",
  "strict-transport-security",
  "report-to",
  "nel",
]);

// Recorded answers go into server files that are shared with the workspace (Git), so
// values that are often credentials are left out of them.

/** Words in a name (`api_key`, `accessToken`, `X-Auth-Token`) that mark its value as a credential. */
const SECRET_WORDS = new Set([
  "token",
  "secret",
  "password",
  "passwd",
  "pwd",
  "pass",
  "passphrase",
  "apikey",
  "signature",
  "sig",
  "credential",
  "credentials",
  "auth",
  "authorization",
  "jwt",
  "bearer",
  "session",
  "sessionid",
  "sid",
  "jsessionid",
  "phpsessid",
  "otp",
  "hmac",
  "verifier",
  "cookie",
]);
/** Words before `token` that make it a place in a list (`pageToken`, `next_token`), not a credential. */
const PAGE_WORDS = new Set(["page", "next", "continuation", "sync", "cursor"]);
/** Words before `key` that make it a credential (`api_key`, `X-Functions-Key`); a lone `key` is one too. */
const KEY_WORDS = new Set(["api", "access", "secret", "private", "client", "consumer", "app", "auth", "license", "subscription", "master", "signing", "sas", "function", "functions"]);
/** Words before `code` that make it a credential (`auth_code`); a lone `code` (OAuth) is one too. */
const CODE_WORDS = new Set(["auth", "oauth", "authorization", "access", "verification", "verify", "reset", "login", "confirmation", "otp", "mfa", "2fa"]);
/** Last words of names that describe a credential rather than hold one (`token_type`, `oauth_signature_method`). */
const ABOUT_WORDS = new Set(["type", "method", "mode", "url", "uri", "endpoint", "ttl", "expires", "expiry", "expiration", "lifetime", "length", "format", "kind", "name", "version", "hint"]);

/** Whether a query parameter or header name usually carries a credential: `api_key`, `access_token`, `code`, `X-Auth-Token`. */
export function credentialName(name: string): boolean {
  const words = name
    .replace(/([a-z0-9])([A-Z])/g, "$1 $2")
    .toLowerCase()
    .split(/[^a-z0-9]+/)
    .filter(Boolean);
  if (words.length > 1 && ABOUT_WORDS.has(words[words.length - 1])) return false;
  return words.some((w, i) => {
    if (w === "token") return !PAGE_WORDS.has(words[i - 1]);
    if (SECRET_WORDS.has(w)) return true;
    if (w === "key") return words.length === 1 || KEY_WORDS.has(words[i - 1]);
    if (w === "code") return words.length === 1 || CODE_WORDS.has(words[i - 1]);
    return false;
  });
}

const decodeName = (name: string) => {
  try {
    return decodeURIComponent(name.replace(/\+/g, " "));
  } catch {
    return name;
  }
};

/** `url` with the values of its credential-like query and fragment parameters left empty (`?code=&state=x`). */
function withoutCredentials(url: string): string {
  const start = url.search(/[?#]/);
  if (start < 0) return url;
  const parts = url.slice(start + 1).split(/([&#])/);
  for (let k = 0; k < parts.length; k += 2) {
    const eq = parts[k].indexOf("=");
    if (eq >= 0 && eq < parts[k].length - 1 && credentialName(decodeName(parts[k].slice(0, eq)))) parts[k] = parts[k].slice(0, eq + 1);
  }
  return url.slice(0, start + 1) + parts.join("");
}

const SECRET_HEADERS_NOTE = "Headers that look like credentials (Authorization, tokens, keys) were left out. Add them to the route if the mock needs them.";
const LOCATION_NOTE = "Values that look like credentials (tokens, keys, codes) were left out of Location headers.";
const QUERY_NOTE = "Query values that look like credentials (tokens, keys, signatures, codes) were left out: those routes match any value there.";
const TEMPLATE_NOTE = "Some answers contain {{…}}: the mock fills these in (from variables or the request) when it answers.";

/** Response headers a route keeps from a recorded answer (cookies are left to the caller's note), and notes on what changed. */
function recordedHeaders(headers: Header[]): { headers: KeyValue[]; notes: string[] } {
  const kept: KeyValue[] = [];
  const notes = new Set<string>();
  for (const h of headers) {
    const name = h.name.trim().toLowerCase();
    if (SKIPPED_HEADERS.has(name) || name === "set-cookie") continue;
    // `Access-Control-Allow-Credentials` and the like say how to send credentials; they hold none.
    if (!name.startsWith("access-control-") && credentialName(name)) {
      notes.add(SECRET_HEADERS_NOTE);
      continue;
    }
    const value = name === "location" || name === "content-location" ? withoutCredentials(h.value) : h.value;
    if (value !== h.value) notes.add(LOCATION_NOTE);
    kept.push({ key: h.name, value });
  }
  return { headers: kept, notes: [...notes] };
}

/** Whether a recorded answer has text the mock would read as a template. */
const hasTemplate = (route: MockRoute) => !!route.body?.includes("{{") || !!route.headers?.some((h) => h.key.includes("{{") || h.value.includes("{{"));

/** A route answering like `result` did to `request`, plus notes on what could not be kept. */
export function routeFromResponse(request: Request, result: SendResult): { route: MockRoute; notes: string[] } {
  const method = (request.method || "GET").toUpperCase();
  const notes: string[] = [];
  // Cookies are often sessions: not copied into a server file that is shared with the workspace.
  if (result.meta.headers.some((h) => h.name.toLowerCase() === "set-cookie")) {
    notes.push("Set-Cookie headers were left out: they often hold a session. Add them to the route if the mock needs them.");
  }
  const recorded = recordedHeaders(result.meta.headers);
  const headers = recorded.headers;
  notes.push(...recorded.notes);
  let body = "";
  if (method !== "HEAD" && result.body.size > 0) {
    if (result.body.kind === "text") {
      body = result.body.text ?? "";
      if (result.body.displayTruncated || result.body.downloadTruncated) notes.push("The body is large: only the part shown in the response is used.");
    } else {
      notes.push("Binary bodies can't be mocked yet: the route answers with an empty body.");
    }
  }
  const route: MockRoute = { method, path: routePathFromUrl(request.url), status: result.meta.status, headers, body };
  if (request.name.trim()) route.name = request.name.trim();
  if (hasTemplate(route)) notes.push(TEMPLATE_NOTE);
  return { route, notes };
}

/** A route answering like a proxied answer in the traffic: the request's method, path and
 * query (as conditions), the backend's status, headers and body. `null` when the answer can't be
 * replayed (no answer, or a cut, binary or compressed body). */
export function routeFromExchange(x: HttpExchange): { route: MockRoute; notes: string[] } | null {
  if (!x.status || !x.responseBodyComplete) return null;
  const [path, query = ""] = splitTarget(x.path);
  const notes: string[] = [];
  if (x.responseHeaders.some((h) => h.name.toLowerCase() === "set-cookie")) {
    notes.push("Set-Cookie headers were left out: they often hold a session.");
  }
  const recorded = recordedHeaders(x.responseHeaders);
  notes.push(...recorded.notes);
  const route: MockRoute = { method: x.method.toUpperCase(), path, status: x.status, headers: recorded.headers, body: x.responseBody };
  // The client's credentials (`?api_key=…`) aren't saved: an empty value matches any value.
  let leftOut = false;
  const matchQuery = queryPairs(query).map((q) => {
    if (!q.value || !credentialName(q.key)) return q;
    leftOut = true;
    return { key: q.key, value: "" };
  });
  if (leftOut) notes.push(QUERY_NOTE);
  if (matchQuery.length) route.matchQuery = matchQuery;
  if (hasTemplate(route)) notes.push(TEMPLATE_NOTE);
  return { route, notes };
}

/** Routes for the proxied answers in `entries` (oldest first), one per method, path and query,
 * leaving out ones `existing` routes already answer the same way. */
export function routesFromTraffic(entries: TrafficEntry[], existing: MockRoute[]): { routes: MockRoute[]; skipped: number; notes: string[] } {
  const key = (r: MockRoute) =>
    `${r.method.trim().toUpperCase()} ${r.path.trim()}?${(r.matchQuery ?? [])
      .filter((q) => q.enabled !== false)
      .map((q) => `${q.key}=${q.value}`)
      .sort()
      .join("&")}`;
  const seen = new Set(existing.filter((r) => r.enabled !== false).map(key));
  const routes: MockRoute[] = [];
  const notes = new Set<string>();
  let skipped = 0;
  for (const e of entries) {
    if (e.http?.note !== "proxy" || !e.http.status) continue;
    const made = routeFromExchange(e.http);
    if (!made) {
      skipped++;
      continue;
    }
    const k = key(made.route);
    if (seen.has(k)) continue;
    seen.add(k);
    routes.push(made.route);
    for (const n of made.notes) notes.add(n);
  }
  // A route with query conditions first, so the one without them doesn't answer its requests.
  routes.sort((a, b) => Number(!!b.matchQuery?.length) - Number(!!a.matchQuery?.length));
  return { routes, skipped, notes: [...notes] };
}

function splitTarget(target: string): [string, string?] {
  const q = target.indexOf("?");
  return q < 0 ? [target || "/"] : [target.slice(0, q) || "/", target.slice(q + 1)];
}

function queryPairs(query: string): KeyValue[] {
  if (!query) return [];
  const pairs: KeyValue[] = [];
  for (const [key, value] of new URLSearchParams(query)) pairs.push({ key, value });
  return pairs;
}

/** Move an item (drag and drop). */
export function moveItem<T>(list: T[], from: number, to: number): T[] {
  if (from === to || from < 0 || from >= list.length) return list;
  const next = [...list];
  const [item] = next.splice(from, 1);
  next.splice(Math.max(0, Math.min(to, next.length)), 0, item);
  return next;
}

/** Where the selected index ends up after `moveItem(list, from, to)`. */
export function followIndex(selected: number, from: number, to: number): number {
  if (selected === from) return to;
  if (from < selected && to >= selected) return selected - 1;
  if (from > selected && to <= selected) return selected + 1;
  return selected;
}

/** A path not used by another route (`/new`, `/new-2`, …). */
export function unusedPath(routes: MockRoute[], base = "/new"): string {
  const used = new Set(routes.map((r) => r.path.trim()));
  if (!used.has(base)) return base;
  let n = 2;
  while (used.has(`${base}-${n}`)) n++;
  return `${base}-${n}`;
}

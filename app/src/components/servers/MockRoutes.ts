// Pure helpers for mock routes: paths from request URLs, a route from a
// response, labels and list moves. Kept free of React for unit tests.
import type { MockRoute } from "../../bindings/MockRoute";
import type { Request } from "../../bindings/Request";
import type { SendResult } from "../../bindings/SendResult";
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
    let name: string | null = null;
    if (p.startsWith(":") && p.length > 1) name = p.slice(1);
    else if (p.startsWith("{") && p.endsWith("}") && !p.startsWith("{{") && p.length > 2) name = p.slice(1, -1);
    else if (p === "*" && i === parts.length - 1) name = "*";
    if (name && !names.includes(name)) names.push(name);
  });
  return names;
}

/** `{{request.…}}` names a route's templates can use (for autocomplete and highlighting). */
export function requestPlaceholders(route: MockRoute): string[] {
  const names = ["request.method", "request.path", "request.url", "request.body"];
  for (const p of pathParams(route.path)) names.push(`request.params.${p}`);
  for (const q of route.matchQuery ?? []) if (q.key.trim()) names.push(`request.query.${q.key.trim()}`);
  for (const h of route.matchHeaders ?? []) if (h.key.trim()) names.push(`request.headers.${h.key.trim().toLowerCase()}`);
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

/** A route answering like `result` did to `request`, plus notes on what could not be kept. */
export function routeFromResponse(request: Request, result: SendResult): { route: MockRoute; notes: string[] } {
  const method = (request.method || "GET").toUpperCase();
  const notes: string[] = [];
  const headers = result.meta.headers
    .filter((h) => !SKIPPED_HEADERS.has(h.name.toLowerCase()) && h.name.toLowerCase() !== "set-cookie")
    .map((h) => ({ key: h.name, value: h.value }));
  // Cookies are often sessions: not copied into a server file that is shared with the workspace.
  if (result.meta.headers.some((h) => h.name.toLowerCase() === "set-cookie")) {
    notes.push("Set-Cookie headers were left out: they often hold a session. Add them to the route if the mock needs them.");
  }
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
  return { route, notes };
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

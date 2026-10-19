import { describe, expect, it } from "vitest";
import type { HttpExchange } from "../../bindings/HttpExchange";
import type { Request } from "../../bindings/Request";
import type { SendResult } from "../../bindings/SendResult";
import type { TrafficEntry } from "../../bindings/TrafficEntry";
import {
  credentialName,
  followIndex,
  languageForContentType,
  moveItem,
  pathParams,
  requestPlaceholders,
  routeFromExchange,
  routeFromResponse,
  routePathFromUrl,
  routesFromTraffic,
  routeUrl,
  unusedPath,
} from "./MockRoutes";

describe("mock routes", () => {
  it("turns request URLs into route paths (like the Rust side)", () => {
    const cases: [string, string][] = [
      ["https://api.test/users/{{id}}?x=1", "/users/:id"],
      ["{{baseUrl}}/pets/:petId", "/pets/:petId"],
      ["{{ baseUrl }}/a/{{$uuid}}/b#frag", "/a/:uuid/b"],
      ["localhost:3000/api/v1/", "/api/v1"],
      ["http://localhost:3000", "/"],
      ["{{host}}:{{port}}/x", "/x"],
      ["/relative//path", "/relative/path"],
      ["{{baseUrl}}", "/"],
      ["{{base}}/search?q={{q}}", "/search"],
      ["{{base}}/files/{name}/user-{{id}}", "/files/:name/:id"],
      ["", "/"],
    ];
    for (const [url, path] of cases) expect(routePathFromUrl(url), url).toBe(path);
  });

  it("lists path parameters and template names", () => {
    expect(pathParams("/users/:id/files/*")).toEqual(["id", "*"]);
    expect(pathParams("/a/{b}/*/c")).toEqual(["b"]);
    expect(
      requestPlaceholders({ method: "GET", path: "/u/:id", status: 200, matchQuery: [{ key: "q", value: "" }], matchHeaders: [{ key: "X-Key", value: "" }] }),
    ).toEqual(["request.method", "request.path", "request.url", "request.body", "request.json", "request.params.id", "request.query.q", "request.headers.x-key"]);
    // The JSON condition's value is offered too.
    expect(requestPlaceholders({ method: "POST", path: "/", status: 200, matchJson: "$.user['first name'] == Ada" })).toContain("request.json.user['first name']");
    expect(requestPlaceholders({ method: "POST", path: "/", status: 200, matchJson: "items[0].id" })).toContain("request.json.items[0].id");
    expect(requestPlaceholders({ method: "POST", path: "/", status: 200, matchJson: "$..x" })).not.toContain("request.json.x");
  });

  it("builds routes from proxied answers in the traffic", () => {
    const exchange = (path: string, patch: Partial<HttpExchange> = {}): HttpExchange => ({
      method: "get",
      path,
      httpVersion: "HTTP/1.1",
      requestHeaders: [{ name: "Authorization", value: "Bearer secret" }],
      requestBody: "",
      status: 200,
      responseHeaders: [
        { name: "Content-Type", value: "application/json" },
        { name: "Content-Length", value: "2" },
        { name: "Date", value: "today" },
        { name: "Set-Cookie", value: "session=1" },
        { name: "X-Backend", value: "real" },
      ],
      responseBody: "{}",
      durationMs: 1,
      route: null,
      note: "proxy",
      responseBodyComplete: true,
      ...patch,
    });
    const made = routeFromExchange(exchange("/users?page=2&q=a+b"))!;
    expect(made.route).toEqual({
      method: "GET",
      path: "/users",
      status: 200,
      headers: [
        { key: "Content-Type", value: "application/json" },
        { key: "X-Backend", value: "real" },
      ],
      body: "{}",
      matchQuery: [
        { key: "page", value: "2" },
        { key: "q", value: "a b" },
      ],
    });
    expect(made.notes[0]).toMatch(/Set-Cookie/);
    expect(routeFromExchange(exchange("/big", { responseBodyComplete: false }))).toBeNull();
    expect(routeFromExchange(exchange("/hung", { status: 0 }))).toBeNull();

    const entry = (id: number, http: HttpExchange) => ({ id, http }) as unknown as TrafficEntry;
    const { routes, skipped, notes } = routesFromTraffic(
      [
        entry(1, exchange("/users")),
        entry(2, exchange("/users?page=2")),
        entry(3, exchange("/users")), // the same again
        entry(4, exchange("/local", { note: null, route: "GET /local" })), // a route answered
        entry(5, exchange("/gzip", { responseBodyComplete: false })),
        entry(6, exchange("/known")),
      ],
      [{ method: "GET", path: "/known", status: 200 }],
    );
    expect(routes.map((r) => `${r.path}${r.matchQuery ? "?" : ""}`)).toEqual(["/users?", "/users"]);
    expect(skipped).toBe(1);
    expect(notes).toHaveLength(1);
  });

  it("leaves credentials out of routes made from proxied answers", () => {
    const exchange = (path: string, patch: Partial<HttpExchange> = {}): HttpExchange => ({
      method: "GET",
      path,
      httpVersion: "HTTP/1.1",
      requestHeaders: [],
      requestBody: "",
      status: 200,
      responseHeaders: [{ name: "Content-Type", value: "application/json" }],
      responseBody: "{}",
      durationMs: 1,
      route: null,
      note: "proxy",
      responseBodyComplete: true,
      ...patch,
    });

    // The client's keys and tokens: the condition stays, with no value (any value matches).
    const made = routeFromExchange(exchange("/search?q=cats&api_key=k-123&access_token=t-456&code=c-789&sig=abc&country_code=NL&pageToken=p2&key="))!;
    expect(made.route.matchQuery).toEqual([
      { key: "q", value: "cats" },
      { key: "api_key", value: "" },
      { key: "access_token", value: "" },
      { key: "code", value: "" },
      { key: "sig", value: "" },
      { key: "country_code", value: "NL" },
      { key: "pageToken", value: "p2" },
      { key: "key", value: "" },
    ]);
    expect(JSON.stringify(made.route)).not.toMatch(/k-123|t-456|c-789|abc/);
    expect(made.notes).toEqual([expect.stringMatching(/Query values that look like credentials/)]);
    // Nothing to leave out: no note.
    expect(routeFromExchange(exchange("/search?q=cats"))!.notes).toEqual([]);

    // The backend's credential headers are dropped; Location keeps its address without them.
    const answer = routeFromExchange(
      exchange("/login", {
        status: 302,
        responseHeaders: [
          { name: "Location", value: "https://app.test/callback?code=c-1&state=s1#access_token=t-2&token_type=bearer" },
          { name: "Authorization", value: "Bearer b-3" },
          { name: "Proxy-Authorization", value: "Basic p-4" },
          { name: "X-Auth-Token", value: "x-5" },
          { name: "X-Api-Key", value: "x-6" },
          { name: "WWW-Authenticate", value: "Bearer" },
          { name: "Access-Control-Allow-Credentials", value: "true" },
          { name: "X-Request-Id", value: "r-1" },
        ],
      }),
    )!;
    expect(answer.route.headers).toEqual([
      { key: "Location", value: "https://app.test/callback?code=&state=s1#access_token=&token_type=bearer" },
      { key: "WWW-Authenticate", value: "Bearer" },
      { key: "Access-Control-Allow-Credentials", value: "true" },
      { key: "X-Request-Id", value: "r-1" },
    ]);
    expect(answer.notes).toEqual([expect.stringMatching(/Location headers/), expect.stringMatching(/^Headers that look like credentials/)]);
    // A Location without credentials is kept as it is.
    const plain = routeFromExchange(exchange("/old", { status: 301, responseHeaders: [{ name: "Location", value: "/new?page=2#top" }] }))!;
    expect(plain.route.headers).toEqual([{ key: "Location", value: "/new?page=2#top" }]);
    expect(plain.notes).toEqual([]);

    // A backend answer with {{…}} becomes a template when the mock replays it: say so.
    const templated = routeFromExchange(exchange("/t", { responseBody: '{"hint": "{{request.headers.authorization}}"}' }))!;
    expect(templated.route.body).toBe('{"hint": "{{request.headers.authorization}}"}');
    expect(templated.notes).toEqual([expect.stringMatching(/\{\{…\}\}/)]);

    // Requests that differ only in a credential share one route.
    const entry = (id: number, http: HttpExchange) => ({ id, http }) as unknown as TrafficEntry;
    const { routes, notes } = routesFromTraffic([entry(1, exchange("/me?token=a")), entry(2, exchange("/me?token=b"))], []);
    expect(routes.map((r) => r.matchQuery)).toEqual([[{ key: "token", value: "" }]]);
    expect(notes).toHaveLength(1);
  });

  it("tells credential names from others", () => {
    for (const name of ["api_key", "apiKey", "APIKey", "X-Api-Key", "access_token", "accessToken", "refresh-token", "token", "key", "secret", "client_secret", "password", "pwd", "signature", "X-Amz-Signature", "X-Amz-Security-Token", "sig", "code", "auth_code", "auth", "Authorization", "Proxy-Authorization", "X-Auth-Token", "session_id", "JSESSIONID", "jwt", "code_verifier", "subscription-key"]) {
      expect(credentialName(name), name).toBe(true);
    }
    for (const name of ["q", "page", "pageToken", "next_token", "country_code", "status_code", "sort_key", "idempotency_key", "Content-Type", "WWW-Authenticate", "X-Request-Id", "state", "keyword", "monkey", "Public-Key-Pins", "token_type", "oauth_signature_method", "token_type_hint", "auth_url"]) {
      expect(credentialName(name), name).toBe(false);
    }
  });

  it("builds a route from a response", () => {
    const request = { name: "Get user", method: "get", url: "{{baseUrl}}/users/{{id}}?full=1", seq: 0 } as Request;
    const result = {
      meta: {
        status: 201,
        headers: [
          { name: "Content-Type", value: "application/json" },
          { name: "Content-Length", value: "12" },
          { name: "Content-Encoding", value: "gzip" },
          { name: "Transfer-Encoding", value: "chunked" },
          { name: "X-Rate-Limit", value: "10" },
        ],
      },
      body: { kind: "text", size: 12, text: '{"id": "7"}', displayTruncated: false, downloadTruncated: false },
    } as unknown as SendResult;
    const { route, notes } = routeFromResponse(request, result);
    expect(route).toEqual({
      name: "Get user",
      method: "GET",
      path: "/users/:id",
      status: 201,
      headers: [
        { key: "Content-Type", value: "application/json" },
        { key: "X-Rate-Limit", value: "10" },
      ],
      body: '{"id": "7"}',
    });
    expect(notes).toEqual([]);

    // Session cookies don't go into the (shared) server file.
    const withCookie = { ...result, meta: { ...result.meta, headers: [...result.meta.headers, { name: "Set-Cookie", value: "session=secret" }] } } as SendResult;
    const c = routeFromResponse(request, withCookie);
    expect(c.route.headers?.some((h) => h.key === "Set-Cookie")).toBe(false);
    expect(c.notes[0]).toMatch(/Set-Cookie/);

    const binary = { ...result, body: { ...result.body, kind: "binary", text: null } } as SendResult;
    const b = routeFromResponse(request, binary);
    expect(b.route.body).toBe("");
    expect(b.notes[0]).toMatch(/Binary/);

    // Credential headers stay out of the (shared) server file too.
    const withToken = { ...result, meta: { ...result.meta, headers: [...result.meta.headers, { name: "X-Auth-Token", value: "secret" }] } } as SendResult;
    const t = routeFromResponse(request, withToken);
    expect(t.route.headers?.some((h) => h.key === "X-Auth-Token")).toBe(false);
    expect(t.notes).toEqual([expect.stringMatching(/^Headers that look like credentials/)]);
  });

  it("moves items and keeps the selection on the same item", () => {
    const list = ["a", "b", "c", "d"];
    expect(moveItem(list, 0, 2)).toEqual(["b", "c", "a", "d"]);
    expect(moveItem(list, 3, 0)).toEqual(["d", "a", "b", "c"]);
    for (const [from, to] of [
      [0, 2],
      [3, 0],
      [1, 3],
      [2, 2],
    ]) {
      for (let selected = 0; selected < list.length; selected++) {
        const moved = moveItem(list, from, to);
        expect(moved[followIndex(selected, from, to)]).toBe(list[selected]);
      }
    }
  });

  it("small helpers", () => {
    expect(routeUrl("http://127.0.0.1:3000/", "users/:id")).toBe("http://127.0.0.1:3000/users/:id");
    expect(unusedPath([{ method: "GET", path: "/new", status: 200 }])).toBe("/new-2");
    expect(languageForContentType("application/problem+json")).toBe("json");
    expect(languageForContentType("", "<!doctype html><html>")).toBe("html");
    expect(languageForContentType("text/plain", "{}")).toBe("text");
  });
});

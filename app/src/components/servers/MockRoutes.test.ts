import { describe, expect, it } from "vitest";
import type { Request } from "../../bindings/Request";
import type { SendResult } from "../../bindings/SendResult";
import {
  followIndex,
  languageForContentType,
  moveItem,
  pathParams,
  requestPlaceholders,
  routeFromResponse,
  routePathFromUrl,
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
    ).toEqual(["request.method", "request.path", "request.url", "request.body", "request.params.id", "request.query.q", "request.headers.x-key"]);
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

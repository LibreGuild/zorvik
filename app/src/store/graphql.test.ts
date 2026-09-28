import { beforeEach, describe, expect, it, vi } from "vitest";
import { buildSchema, introspectionFromSchema } from "graphql";
import type { GraphqlSchema } from "../bindings/GraphqlSchema";
import type { Request } from "../bindings/Request";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("../lib/rpc", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/rpc")>();
  return { ...actual, api: { graphqlSchema: vi.fn() } };
});

const { api, RpcError } = await import("../lib/rpc");
const gql = await import("./graphql");
const { canAutoLoad, docsBack, isSubscription, keptOperationName, loadSchema, MAX_SCHEMAS, namedType, operationNames, operations, operationType, prettifyQuery, schemaKey, showType, useGraphql } = gql;
const graphqlSchema = api.graphqlSchema as unknown as ReturnType<typeof vi.fn>;

const SDL = `
  type Query { hello(name: String): String, user(id: ID!): User }
  type User { id: ID!, name: String, posts: [Post] }
  type Post { title: String }
  type Mutation { echo(text: String!): String }
`;

function result(sdl = SDL, extra: Partial<GraphqlSchema> = {}): GraphqlSchema {
  const data = introspectionFromSchema(buildSchema(sdl)) as unknown as Record<string, unknown>;
  return { data, url: "http://x.test/graphql", fetchedAt: 1000, cached: false, legacy: false, ...extra };
}

const request: Request = { name: "g", kind: "http", seq: 0, method: "POST", url: "{{base}}/graphql", body: { type: "graphql", graphql: { query: "{ hello }" } } };

/** A promise resolved (or rejected) from outside. */
function deferred<T>() {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

beforeEach(() => {
  useGraphql.setState({ schemas: {}, docs: {}, panelOpen: false });
  graphqlSchema.mockReset();
});

describe("operations", () => {
  it("lists named operations, null while the query does not parse", () => {
    expect(operationNames("query A { hello } mutation B { echo(text: \"x\") } { hello } fragment F on User { id }")).toEqual(["A", "B"]);
    expect(operationNames("  ")).toEqual([]);
    expect(operationNames("query A { hello")).toBeNull();
  });

  it("drops an operation name that no longer exists", () => {
    expect(keptOperationName("query A { a } query B { b }", "B")).toBe("B");
    expect(keptOperationName("query C { a }", "B")).toBeUndefined();
    // Mid-edit: left alone.
    expect(keptOperationName("query C { a", "B")).toBe("B");
    expect(keptOperationName("{ a }", undefined)).toBeUndefined();
  });

  it("prettifies, and says when comments are lost", () => {
    expect(prettifyQuery("query A($id:ID!){user(id:$id){id name}}")).toEqual({
      text: "query A($id: ID!) {\n  user(id: $id) {\n    id\n    name\n  }\n}",
      comments: false,
    });
    expect(prettifyQuery("# users\n{ hello }")).toEqual({ text: "{\n  hello\n}", comments: true });
    expect(prettifyQuery('{ hello(name: "# not a comment") }')).toMatchObject({ comments: false });
    expect(prettifyQuery("{ hello(")).toHaveProperty("error");
  });

  it("finds the named type of a wrapped type", () => {
    expect(namedType("[User!]!")).toBe("User");
  });

  it("knows which operation runs, also while it is being typed", () => {
    const doc = `# query X { a }
      query Get($id: ID = "a { b") @cached(ttl: {s: 1}) { user(id: $id) { ...F } }
      fragment F on User { id name(format: "} subscription X {") }
      mutation Save { save(input: """block \\""" subscription {""") }
      subscription($room: ID!) { messages(room: $room) }`;
    expect(operations(doc)).toEqual([
      { type: "query", name: "Get" },
      { type: "mutation", name: "Save" },
      { type: "subscription", name: null },
    ]);
    expect(operations("{ ping }")).toEqual([{ type: "query", name: null }]);
    expect(operations("subscription Live { ticks {")).toEqual([{ type: "subscription", name: "Live" }]);
    expect(operationType("query A { a } subscription B { b }", "B")).toBe("subscription");
    expect(operationType("query A { a } subscription B { b }", null)).toBeNull();
    expect(operationType("subscription { b }", "")).toBe("subscription");
  });

  it("sends subscriptions over a live connection", () => {
    const sub = (query: string, kind: Request["kind"] = "http"): Request => ({ ...request, kind, body: { type: "graphql", graphql: { query } } });
    expect(isSubscription(sub("subscription { tick }"))).toBe(true);
    expect(isSubscription(sub("query { tick }"))).toBe(false);
    expect(isSubscription(sub("subscription { tick }", "websocket"))).toBe(false);
    expect(isSubscription({ ...request, body: { type: "json", text: "subscription { tick }" } })).toBe(false);
  });
});

describe("auto-load", () => {
  const known = new Set(["base", "tenant"]);

  it("needs a URL whose host variables are all defined", () => {
    expect(canAutoLoad("", known)).toBe(false);
    expect(canAutoLoad("https://api.example.com/graphql", known)).toBe(true);
    expect(canAutoLoad("{{base}}/graphql", known)).toBe(true);
    expect(canAutoLoad("{{ base }}/graphql", known)).toBe(true);
    expect(canAutoLoad("{{baseUrl}}/graphql", known)).toBe(false);
    expect(canAutoLoad("https://{{tenant}}.example.com/graphql", known)).toBe(true);
    expect(canAutoLoad("https://{{other}}.example.com/graphql", known)).toBe(false);
    // Only the host matters; path variables are reported when sending.
    expect(canAutoLoad("https://api.example.com/{{version}}/graphql", known)).toBe(true);
    expect(canAutoLoad("http://localhost:4000/graphql?x={{nope}}", known)).toBe(true);
    expect(canAutoLoad("https://{{$randomDomainName}}/graphql", known)).toBe(true);
    expect(canAutoLoad("https://{{base/graphql", known)).toBe(false);
    expect(canAutoLoad("ws://api.example.com/graphql", known)).toBe(false);
    expect(canAutoLoad("https:///graphql", known)).toBe(false);
  });

  it("keys schemas by workspace, URL and environment", () => {
    expect(schemaKey("/ws/a", " {{base}}/graphql ", "dev")).toBe(schemaKey("/ws/a", "{{base}}/graphql", "dev"));
    expect(schemaKey("/ws/a", "{{base}}/graphql", "dev")).not.toBe(schemaKey("/ws/a", "{{base}}/graphql", "prod"));
    // The same URL and environment name in another workspace can be another server.
    expect(schemaKey("/ws/a", "{{base}}/graphql", "dev")).not.toBe(schemaKey("/ws/b", "{{base}}/graphql", "dev"));
    expect(schemaKey(null, "{{base}}/graphql", null)).toBe(schemaKey(undefined, "{{base}}/graphql", undefined));
  });
});

describe("loading schemas", () => {
  const key = schemaKey("/ws", request.url, null);
  const entry = () => useGraphql.getState().schemas[key];

  it("builds the schema and shares a load in progress", async () => {
    const reply = deferred<GraphqlSchema>();
    graphqlSchema.mockReturnValue(reply.promise);
    const first = loadSchema(key, request, "g.yaml");
    const second = loadSchema(key, request, "g.yaml");
    expect(entry().status).toBe("loading");
    reply.resolve(result());
    await Promise.all([first, second]);
    expect(graphqlSchema).toHaveBeenCalledTimes(1);
    expect(graphqlSchema).toHaveBeenCalledWith(request, "g.yaml", false);
    const e = entry();
    expect(e.status).toBe("loaded");
    expect(e.url).toBe("http://x.test/graphql");
    expect(e.fetchedAt).toBe(1000);
    expect(Object.keys(e.schema!.getQueryType()!.getFields())).toEqual(["hello", "user"]);
    expect(e.schema!.getMutationType()?.name).toBe("Mutation");
  });

  it("keeps the last schema while refreshing and after a failed refresh", async () => {
    graphqlSchema.mockResolvedValueOnce(result());
    await loadSchema(key, request, null);
    const reply = deferred<GraphqlSchema>();
    graphqlSchema.mockReturnValueOnce(reply.promise);
    const refreshing = loadSchema(key, request, null, true);
    expect(graphqlSchema).toHaveBeenLastCalledWith(request, null, true);
    expect(entry().status).toBe("loading");
    expect(entry().schema).not.toBeNull();
    reply.reject(new RpcError({ code: "graphql", message: "The schema request failed (HTTP 401): Not authenticated", networkKind: null }));
    await refreshing;
    expect(entry()).toMatchObject({ status: "error", error: "The schema request failed (HTTP 401): Not authenticated" });
    expect(entry().schema?.getType("User")).toBeTruthy();
  });

  it("drops an older answer that arrives after a newer one", async () => {
    const slow = deferred<GraphqlSchema>();
    graphqlSchema.mockReturnValueOnce(slow.promise).mockResolvedValueOnce(result("type Query { fresh: Int }", { fetchedAt: 2000 }));
    const first = loadSchema(key, request, null);
    await loadSchema(key, request, null, true);
    slow.resolve(result());
    await first;
    expect(entry().fetchedAt).toBe(2000);
    expect(Object.keys(entry().schema!.getQueryType()!.getFields())).toEqual(["fresh"]);
  });

  it("keeps a bounded number of schemas, dropping the least recently loaded", async () => {
    graphqlSchema.mockResolvedValue(result());
    const keys = Array.from({ length: MAX_SCHEMAS + 2 }, (_, i) => schemaKey("/ws", `http://h${i}.test/graphql`, null));
    for (const k of keys.slice(0, MAX_SCHEMAS)) await loadSchema(k, request, null);
    await loadSchema(keys[0], request, null, true); // refreshed: now the newest
    for (const k of keys.slice(MAX_SCHEMAS)) await loadSchema(k, request, null);
    const kept = Object.keys(useGraphql.getState().schemas);
    expect(kept).toHaveLength(MAX_SCHEMAS);
    expect(kept).toContain(keys[0]);
    expect(kept).not.toContain(keys[1]);
    expect(kept).not.toContain(keys[2]);
    expect(kept.slice(-2)).toEqual(keys.slice(MAX_SCHEMAS));
  });

  it("reports a schema graphql-js cannot read", async () => {
    graphqlSchema.mockResolvedValueOnce({ ...result(), data: { __schema: { queryType: { name: "Query" }, types: [] } } });
    await loadSchema(key, request, null);
    expect(entry().status).toBe("error");
    expect(entry().error).toMatch(/could not be read/);
  });
});

describe("schema panel", () => {
  it("opens types per tab, with back and home", () => {
    showType("t1", "User");
    showType("t1", "User");
    showType("t1", "Post");
    showType("t2", "Query");
    expect(useGraphql.getState().docs).toEqual({ t1: ["User", "Post"], t2: ["Query"] });
    expect(useGraphql.getState().panelOpen).toBe(true);
    docsBack("t1");
    expect(useGraphql.getState().docs.t1).toEqual(["User"]);
    docsBack("t2", true);
    expect(useGraphql.getState().docs.t2).toEqual([]);
  });
});

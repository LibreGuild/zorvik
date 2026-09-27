// Render the GraphQL body editor with a loaded schema (no backend): operation picker, the
// query editor with cm6-graphql, the Schema panel and its navigation, switching body types.
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { buildSchema, introspectionFromSchema } from "graphql";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Body } from "../../bindings/Body";
import type { Request } from "../../bindings/Request";
import type { Tab } from "../../store/tabs";

vi.mock("../../lib/events", () => ({ onEvent: () => () => {} }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("../../lib/rpc", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../lib/rpc")>();
  return { ...actual, api: { graphqlSchema: vi.fn() } };
});

const { api } = await import("../../lib/rpc");
const { TooltipProvider } = await import("../ui");
const { BodyEditor } = await import("./BodyEditor");
const { schemaKey, useGraphql } = await import("../../store/graphql");
const { openDraft, useTabs } = await import("../../store/tabs");
const graphqlSchema = api.graphqlSchema as unknown as ReturnType<typeof vi.fn>;

const SDL = `
  "Entry point"
  type Query { "Greets" hello(name: String = "world"): String, user(id: ID!): User, old: Int @deprecated(reason: "Gone") }
  type User { id: ID!, name: String, role: Role }
  enum Role { ADMIN, GUEST }
`;
const data = introspectionFromSchema(buildSchema(SDL)) as unknown as Record<string, unknown>;

function draft(body: Body): Request {
  return { name: "g", kind: "http", seq: 0, method: "POST", url: "https://api.example.test/graphql", body };
}

/** Render the body editor of a new tab, re-rendering with the tab's latest draft. */
function renderBody(body: Body) {
  const id = openDraft(draft(body));
  const tab = () => useTabs.getState().tabs.find((t) => t.id === id) as Tab;
  const view = () => (
    <TooltipProvider>
      <BodyEditor
        tab={tab()}
        body={tab().draft.body!}
        onChange={(b) => useTabs.setState((s) => ({ tabs: s.tabs.map((t) => (t.id === id ? { ...(t as Tab), draft: { ...(t as Tab).draft, body: b } } : t)) }))}
        onSubmit={() => {}}
      />
    </TooltipProvider>
  );
  const r = render(view());
  unsubscribe.push(useTabs.subscribe(() => tab() && r.rerender(view())));
  return { tab };
}

const unsubscribe: (() => void)[] = [];

// jsdom has no layout; CodeMirror measures ranges for lint markers and tooltips.
Range.prototype.getClientRects = () => [] as unknown as DOMRectList;
Range.prototype.getBoundingClientRect = () => new DOMRect();

beforeEach(() => {
  vi.useFakeTimers();
  useTabs.setState({ tabs: [], activeId: null });
  useGraphql.setState({ schemas: {}, docs: {}, panelOpen: false });
  graphqlSchema.mockReset();
  graphqlSchema.mockResolvedValue({ data, url: "https://api.example.test/graphql", fetchedAt: 1, cached: false, legacy: false });
});

afterEach(() => {
  unsubscribe.splice(0).forEach((u) => u());
  cleanup();
  vi.useRealTimers();
});

describe("GraphQL body", () => {
  it("loads the schema by itself and shows it in the Schema panel", async () => {
    const { tab } = renderBody({ type: "graphql", graphql: { query: "query A { hello } query B { user(id: 1) { name } }" } });
    expect(screen.getByTestId("graphql-query")).toBeTruthy();
    expect(screen.getByTestId("graphql-variables")).toBeTruthy();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
    expect(graphqlSchema).toHaveBeenCalledTimes(1);
    expect(graphqlSchema.mock.calls[0][0].url).toBe("https://api.example.test/graphql");
    expect(useGraphql.getState().schemas[schemaKey(null, tab().draft.url, null)].status).toBe("loaded");

    // Two named operations: pick one.
    const picker = screen.getByLabelText("Operation") as HTMLSelectElement;
    expect([...picker.options].map((o) => o.value)).toEqual(["", "A", "B"]);
    fireEvent.change(picker, { target: { value: "B" } });
    expect(tab().draft.body?.graphql?.operationName).toBe("B");

    // The panel: root types → Query → User.
    fireEvent.click(screen.getByRole("button", { name: /^Schema:/ }));
    const panel = screen.getByRole("complementary", { name: "GraphQL schema" });
    expect(within(panel).getByRole("status").textContent).toMatch(/Schema loaded · 7 types/);
    fireEvent.click(within(panel).getAllByRole("button", { name: "Query" })[0]);
    // The clicked link is gone; keyboard focus goes to the type shown.
    expect(panel.contains(document.activeElement) && document.activeElement?.getAttribute("tabindex")).toBe("-1");
    const fields = within(panel).getAllByTestId("graphql-schema-field");
    expect(fields.map((f) => f.textContent?.split(/\s/)[0])).toEqual(["hello(name:", "user(id:", "old:"]);
    expect(fields[0].textContent).toContain('= "world"');
    expect(fields[2].textContent).toContain("Deprecated: Gone");
    fireEvent.click(within(fields[1]).getByRole("button", { name: "User" }));
    expect(useGraphql.getState().docs[tab().id]).toEqual(["Query", "User"]);
    expect(within(panel).getByTestId("graphql-schema-type").textContent).toContain("role");

    // Search finds fields.
    fireEvent.change(within(panel).getByLabelText("Search schema"), { target: { value: "rol" } });
    expect(within(panel).getByRole("button", { name: /User\.role/ })).toBeTruthy();
  });

  it("keeps the operation picker while the query is mid-edit", () => {
    const { tab } = renderBody({ type: "graphql", graphql: { query: "query A { hello } query B { hello }", operationName: "B" } });
    const setQuery = (query: string) =>
      act(() =>
        useTabs.setState((s) => ({
          tabs: s.tabs.map((t) => ({ ...(t as Tab), draft: { ...(t as Tab).draft, body: { type: "graphql", graphql: { ...(t as Tab).draft.body?.graphql, query } } } })),
        })),
      );
    setQuery("query A { hello } query B { user(");
    expect([...(screen.getByLabelText("Operation") as HTMLSelectElement).options].map((o) => o.value)).toEqual(["", "A", "B"]);
    expect(tab().draft.body?.graphql?.operationName).toBe("B");
    setQuery("query A { hello }");
    expect(screen.queryByLabelText("Operation")).toBeNull();
  });

  it("validates the query against the loaded schema", async () => {
    renderBody({ type: "graphql", graphql: { query: "{ hello nope }" } });
    const query = screen.getByTestId("graphql-query");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(3000);
    });
    expect(graphqlSchema).toHaveBeenCalledTimes(1);
    const marked = [...query.querySelectorAll(".cm-lintRange-error")].map((e) => e.textContent);
    expect(marked).toEqual(["nope"]);
  });

  it("does not introspect while the host has an undefined variable", async () => {
    const { tab } = renderBody({ type: "graphql", graphql: { query: "{ hello }" } });
    act(() => useTabs.setState((s) => ({ tabs: s.tabs.map((t) => ({ ...(t as Tab), draft: { ...(t as Tab).draft, url: "{{nope}}/graphql" } })) })));
    graphqlSchema.mockClear();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
    expect(tab().draft.url).toBe("{{nope}}/graphql");
    expect(graphqlSchema.mock.calls.filter((c) => c[0].url === "{{nope}}/graphql")).toHaveLength(0);
  });

  it("switching a GET request to GraphQL makes it a POST and keeps a JSON operation", () => {
    const id = openDraft({ ...draft({ type: "json", text: '{"query": "{ hello }", "variables": {"a": 1}}' }), method: "GET" });
    const tab = () => useTabs.getState().tabs.find((t) => t.id === id) as Tab;
    render(
      <TooltipProvider>
        <BodyEditor tab={tab()} body={tab().draft.body!} onChange={() => {}} onSubmit={() => {}} />
      </TooltipProvider>,
    );
    fireEvent.change(screen.getByLabelText("Body type"), { target: { value: "graphql" } });
    expect(tab().draft.method).toBe("POST");
    expect(tab().draft.body?.type).toBe("graphql");
    expect(tab().draft.body?.graphql).toEqual({ query: "{ hello }", variables: '{\n  "a": 1\n}', operationName: undefined });
  });
});

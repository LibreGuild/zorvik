// Import → API catalog: imports that race the window closing or another import, where focus
// goes when the details change, what redraws, and which links it opens.
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { CatalogEntry } from "../../bindings/CatalogEntry";
import type { CatalogImport } from "../../bindings/CatalogImport";
import type { CatalogPreview } from "../../bindings/CatalogPreview";
import type { WorkspaceInfo } from "../../bindings/WorkspaceInfo";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("../../lib/events", () => ({ onEvent: () => () => {} }));
vi.mock("../../lib/platform", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/platform")>()),
  openExternal: vi.fn(() => Promise.resolve()),
}));
vi.mock("../../lib/rpc", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/rpc")>()),
  api: { catalogList: vi.fn(), catalogPreview: vi.fn(), catalogImport: vi.fn() },
}));
vi.mock("./model", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./model")>();
  return { ...actual, entryLabels: vi.fn(actual.entryLabels) };
});

const { api } = await import("../../lib/rpc");
const { openExternal } = await import("../../lib/platform");
const { entryLabels } = await import("./model");
const { useWorkspace } = await import("../../store/workspace");
const { CatalogTab } = await import("./CatalogTab");

function entry(id: string, name: string, patch: Partial<CatalogEntry> = {}): CatalogEntry {
  return {
    id,
    name,
    category: "devTools",
    description: `${name} API`,
    website: `https://docs.example.com/${id}`,
    auth: "Bearer token",
    format: "OpenAPI 3.0",
    license: { name: "MIT", url: "https://example.com/LICENSE" },
    source: { type: "download", url: `https://raw.githubusercontent.com/${id}/openapi.json` },
    ...patch,
  };
}

const ENTRIES: CatalogEntry[] = [
  entry("github", "GitHub"),
  entry("stripe", "Stripe", { category: "payments" }),
  entry("oddlinks", "Odd Links", { website: "javascript:alert(1)", license: { name: "Custom", url: "file:///etc/passwd" } }),
  ...Array.from({ length: 40 }, (_, i) => entry(`filler-${i}`, `Filler ${i}`)),
];

const SMALL: CatalogPreview = { operations: 12, groups: [], selected: [] };
const LARGE: CatalogPreview = {
  operations: 420,
  groups: [
    { tag: "repos", operations: 300 },
    { tag: "issues", operations: 120 },
  ],
  selected: [],
};
const IMPORTED: CatalogImport = {
  summary: { name: "Stripe", requests: 12, folders: 1, environments: 1, workspaceVariables: 0, warnings: [], folderPath: "Stripe" },
};

/** A promise and the function that settles it. */
function later<T>() {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { promise, resolve };
}

const flush = () => act(() => new Promise((r) => setTimeout(r, 0)));

function show(props: Partial<Parameters<typeof CatalogTab>[0]> = {}) {
  const onImported = vi.fn(() => Promise.resolve());
  const onBusyChange = vi.fn();
  const view = render(<CatalogTab parent="" onImported={onImported} onImportFile={() => {}} onBusyChange={onBusyChange} {...props} />);
  return { ...view, onImported, onBusyChange };
}

async function select(name: string) {
  fireEvent.change(await screen.findByLabelText("Search APIs"), { target: { value: name } });
}

const importButton = () => screen.getByRole("button", { name: "Import" });

beforeEach(() => {
  vi.mocked(api.catalogList).mockResolvedValue(ENTRIES);
  vi.mocked(api.catalogPreview).mockReset();
  vi.mocked(api.catalogImport).mockReset();
  vi.mocked(openExternal).mockClear();
  useWorkspace.setState({ info: { path: "/ws" } as WorkspaceInfo });
});
afterEach(cleanup);

describe("importing from the catalog", () => {
  it("closing the window while the description downloads cancels the import", async () => {
    const preview = later<CatalogPreview>();
    vi.mocked(api.catalogPreview).mockReturnValue(preview.promise);
    const { unmount, onBusyChange, onImported } = show();
    await select("stripe");
    fireEvent.click(importButton());
    await flush();
    expect(onBusyChange).toHaveBeenLastCalledWith(true);
    unmount();
    preview.resolve(SMALL);
    await flush();
    expect(api.catalogImport).not.toHaveBeenCalled();
    expect(onImported).not.toHaveBeenCalled();
  });

  it("imports once the description is in, and says when it is done", async () => {
    const imported = later<CatalogImport>();
    vi.mocked(api.catalogPreview).mockResolvedValue(SMALL);
    vi.mocked(api.catalogImport).mockReturnValue(imported.promise);
    const { onImported, onBusyChange } = show({ parent: "APIs" });
    await select("stripe");
    fireEvent.click(importButton());
    await flush();
    expect(api.catalogImport).toHaveBeenCalledWith("stripe", "APIs");
    expect(screen.getByRole("status").textContent).toBe("Adding Stripe…");
    expect(onBusyChange).toHaveBeenLastCalledWith(true);
    imported.resolve(IMPORTED);
    await flush();
    expect(onImported).toHaveBeenCalledWith(IMPORTED);
    expect(onBusyChange).toHaveBeenLastCalledWith(false);
  });

  it("waits while another import in the window runs", async () => {
    show({ blocked: true });
    await select("stripe");
    expect((importButton() as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(importButton());
    await flush();
    expect(api.catalogPreview).not.toHaveBeenCalled();
  });
});

describe("a large API", () => {
  it("moves focus to the groups, and back to Import", async () => {
    vi.mocked(api.catalogPreview).mockResolvedValue(LARGE);
    show();
    await select("github");
    importButton().focus();
    fireEvent.click(importButton());
    expect(await screen.findByLabelText("Search groups")).toBe(document.activeElement);
    fireEvent.click(screen.getByRole("button", { name: "Back" }));
    expect(importButton()).toBe(document.activeElement);
  });

  it("doesn't move focus to groups shown for another API", async () => {
    const preview = later<CatalogPreview>();
    vi.mocked(api.catalogPreview).mockReturnValue(preview.promise);
    show();
    await select("github");
    fireEvent.click(importButton());
    await flush();
    // Picks another API while GitHub's description downloads.
    fireEvent.change(screen.getByLabelText("Search APIs"), { target: { value: "stripe" } });
    const search = screen.getByLabelText("Search APIs");
    search.focus();
    preview.resolve(LARGE);
    await flush();
    expect(document.activeElement).toBe(search);
    expect(screen.queryByLabelText("Search groups")).toBeNull();
  });

  it("ticking groups doesn't redraw the list of APIs", async () => {
    vi.mocked(api.catalogPreview).mockResolvedValue(LARGE);
    show();
    await select("");
    fireEvent.click(importButton());
    await screen.findByLabelText("Search groups");
    vi.mocked(entryLabels).mockClear();
    fireEvent.click(screen.getByRole("checkbox", { name: "issues" }));
    fireEvent.change(screen.getByLabelText("Search groups"), { target: { value: "rep" } });
    expect(entryLabels).not.toHaveBeenCalled();
    // Choosing another API redraws only the two rows whose selection changed.
    fireEvent.click(screen.getByRole("option", { name: /Filler 7/ }));
    expect(entryLabels).toHaveBeenCalledTimes(2);
  });
});

describe("the list", () => {
  it("tells assistive technology which API the search box's arrow keys pick", async () => {
    show();
    await select("filler 1");
    const search = screen.getByLabelText("Search APIs");
    const listbox = screen.getByRole("listbox", { name: "APIs" });
    expect(search.getAttribute("aria-controls")).toBe(listbox.id);
    const options = screen.getAllByRole("option");
    expect(search.getAttribute("aria-activedescendant")).toBe(options[0].id);
    fireEvent.keyDown(search, { key: "ArrowDown" });
    expect(search.getAttribute("aria-activedescendant")).toBe(options[1].id);
    expect(options[1].getAttribute("aria-selected")).toBe("true");
  });

  it("says when nothing matches, outside the list of options", async () => {
    show();
    await select("nothing is called this");
    const listbox = screen.getByRole("listbox", { name: "APIs" });
    expect(listbox.children).toHaveLength(0);
    expect(screen.getByRole("status").textContent).toContain("No API matches “nothing is called this”");
    expect(screen.getByLabelText("Search APIs").getAttribute("aria-activedescendant")).toBeNull();
  });

  it("opens only web links", async () => {
    show();
    await select("github");
    fireEvent.click(screen.getByRole("button", { name: "MIT" }));
    expect(openExternal).toHaveBeenCalledWith("https://example.com/LICENSE");

    await select("odd links");
    expect(screen.getByRole("heading", { name: "Odd Links" })).toBeTruthy();
    expect(screen.queryByText("API docs")).toBeNull();
    expect(screen.getByText("Custom").tagName).not.toBe("BUTTON");
    expect(screen.queryByRole("button", { name: "Custom" })).toBeNull();
  });
});

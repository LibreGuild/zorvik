// Docs: Preview when there is text, Edit when there isn't; rendered Markdown never runs
// what an (untrusted) workspace file holds.
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(() => Promise.resolve(null)) }));
const openExternal = vi.fn((_url: string) => Promise.resolve());
vi.mock("../lib/platform", () => ({ openExternal: (url: string) => openExternal(url) }));

const mermaid = {
  initialize: vi.fn(),
  render: vi.fn(async (_id: string, code: string) => {
    if (code.includes("oops")) throw new Error("Parse error on line 1:\nexpecting graph");
    return { svg: '<svg data-testid="diagram-svg"></svg>' };
  }),
};
vi.mock("mermaid", () => ({ default: mermaid }));

const { DocsEditor, DocsView } = await import("./DocsEditor");

afterEach(() => {
  cleanup();
  openExternal.mockClear();
});

describe("rendered docs", () => {
  it("draws mermaid code blocks in Mermaid's strict mode, and shows the text of one it can't draw", async () => {
    render(<DocsView source={"```mermaid\ngraph TD; A-->B\n```\n\n```mermaid\noops\n```\n\n```js\nmermaid()\n```"} />);
    expect(await screen.findByTestId("diagram-svg")).toBeTruthy();
    expect(mermaid.initialize).toHaveBeenCalledWith(expect.objectContaining({ securityLevel: "strict", startOnLoad: false }));
    expect(await screen.findByText(/can't be drawn: Parse error on line 1:$/)).toBeTruthy();
    expect(screen.getByText("oops")).toBeTruthy();
    // Other code blocks stay code.
    expect(screen.getByText("mermaid()").tagName).toBe("CODE");
  });

  it("shows HTML as text and runs nothing", () => {
    const { container } = render(
      <DocsView source={'<script>window.hacked = 1</script>\n\n<img src=x onerror="window.hacked = 1">\n\n<a href="javascript:alert(1)">x</a>'} />,
    );
    expect(container.querySelector("script, img, iframe, a")).toBeNull();
    expect(container.textContent).toContain("<script>window.hacked = 1</script>");
    expect((window as { hacked?: number }).hacked).toBeUndefined();
  });

  it("opens web links in the browser and leaves other addresses as text", () => {
    const { container } = render(<DocsView source="[guide](https://example.com/guide) [bad](javascript:alert(1)) [file](file:///etc/passwd)" />);
    const anchors = [...container.querySelectorAll("a")];
    expect(anchors.map((a) => a.getAttribute("href"))).toEqual(["https://example.com/guide"]);
    expect(anchors[0].title).toBe("https://example.com/guide");
    expect(container.textContent).toContain("bad");
    fireEvent.click(anchors[0]);
    expect(openExternal).toHaveBeenCalledWith("https://example.com/guide");
  });

  it("shows embedded pictures and doesn't download others", () => {
    const { container } = render(<DocsView source="![dot](data:image/png;base64,iVBORw0KGgo=) ![remote](https://example.com/a.png)" />);
    expect([...container.querySelectorAll("img")].map((i) => i.getAttribute("alt"))).toEqual(["dot"]);
    expect(container.querySelector("a")?.getAttribute("href")).toBe("https://example.com/a.png");
  });

  it("renders headings, lists, tables, quotes and code", () => {
    const { container } = render(
      <DocsView source={"## Users\n\n- **one**\n- [ ] two\n\n| a | b |\n|---|--:|\n| 1 | 2 |\n\n> [!TIP]\n> Use `{{id}}`.\n\n```\nGET /users\n```"} />,
    );
    expect(container.querySelector("h2")?.textContent).toBe("Users");
    expect(container.querySelectorAll("li")).toHaveLength(2);
    expect(container.querySelector("strong")?.textContent).toBe("one");
    expect(container.querySelector("input[type=checkbox]")).not.toBeNull();
    expect(container.querySelector("td:last-child")?.getAttribute("style")).toContain("right");
    expect(container.querySelector("blockquote")?.textContent).toContain("Tip");
    expect(container.querySelector("pre")?.textContent).toBe("GET /users");
  });

  it("previews the start of very long docs and says how to see the rest", () => {
    const { container, rerender } = render(<DocsView source={"Short notes."} />);
    expect(screen.queryByTestId("docs-cut")).toBeNull();
    rerender(<DocsView source={`# Start\n\n${"Line of notes.\n".repeat(20_000)}\nThe end.`} />);
    expect(container.querySelector("h1")?.textContent).toBe("Start");
    expect(container.textContent).not.toContain("The end.");
    expect(screen.getByTestId("docs-cut").textContent).toBe("The rest of these docs is too long to preview; switch to Edit to see it all.");
  });
});

describe("docs editor", () => {
  it("opens in Preview when there is text", () => {
    render(<DocsEditor value="# Hello" onChange={() => {}} />);
    expect(screen.getByTestId("docs-preview").textContent).toContain("Hello");
    expect(screen.queryByTestId("code-editor")).toBeNull();
    expect(screen.getByRole("button", { name: "Preview" }).getAttribute("aria-pressed")).toBe("true");
  });

  it("opens for typing when empty", () => {
    render(<DocsEditor value="" onChange={() => {}} />);
    expect(screen.getByTestId("code-editor")).not.toBeNull();
    expect(screen.getByRole("button", { name: "Edit" }).getAttribute("aria-pressed")).toBe("true");
  });

  it("switches to Edit on a double-click, on Enter, and with the switch", () => {
    render(<DocsEditor value="Some *notes*" onChange={() => {}} />);
    fireEvent.doubleClick(screen.getByText("notes"));
    expect(screen.getByTestId("code-editor")).not.toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Preview" }));
    expect(screen.queryByTestId("code-editor")).toBeNull();
    fireEvent.keyDown(screen.getByTestId("docs-preview"), { key: "Enter" });
    expect(screen.getByTestId("code-editor")).not.toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Preview" }));
    fireEvent.click(screen.getByRole("button", { name: "Edit" }));
    expect(screen.getByTestId("code-editor")).not.toBeNull();
  });

  it("a section without text offers to write docs", () => {
    render(<DocsEditor value="" onChange={() => {}} variant="section" emptyText="No docs for this server yet." />);
    expect(screen.getByText("No docs for this server yet.")).not.toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Write docs" }));
    expect(screen.getByTestId("code-editor")).not.toBeNull();
  });
});

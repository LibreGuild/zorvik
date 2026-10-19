// The Markdown of Docs: what it understands, and that untrusted text stays text.
import { describe, expect, it } from "vitest";
import { type Block, type Inline, inlineImage, inlineText, MAX_PREVIEW, parseInline, parseMarkdown, previewPart, safeHref } from "./markdown";

const text = (b: Block) => ("content" in b ? inlineText(b.content) : "");
const links = (nodes: Inline[]): { href: string | null; text: string }[] =>
  nodes.flatMap((n) =>
    n.type === "link" ? [{ href: n.href, text: inlineText(n.children) }] : "children" in n ? links(n.children) : [],
  );

describe("blocks", () => {
  it("reads headings, paragraphs and rules", () => {
    const blocks = parseMarkdown("# Users\n\nLists the users,\nnewest first.\n\n---\n###### Small ##\nTitle\n===");
    expect(blocks.map((b) => b.type)).toEqual(["heading", "paragraph", "rule", "heading", "heading"]);
    expect(blocks[0]).toMatchObject({ level: 1 });
    expect(text(blocks[1])).toBe("Lists the users, newest first.");
    expect(blocks[3]).toMatchObject({ level: 6 });
    expect(text(blocks[3])).toBe("Small");
    expect(blocks[4]).toMatchObject({ level: 1 });
    expect(text(blocks[4])).toBe("Title");
    // Not a heading without the space.
    expect(parseMarkdown("#hashtag")[0].type).toBe("paragraph");
  });

  it("keeps code blocks as written", () => {
    const [code] = parseMarkdown("```json\n{\n  \"id\": 1\n}\n```");
    expect(code).toEqual({ type: "code", lang: "json", code: '{\n  "id": 1\n}' });
    // An unclosed fence runs to the end.
    expect(parseMarkdown("~~~\n<b>not bold</b>")[0]).toEqual({ type: "code", lang: "", code: "<b>not bold</b>" });
  });

  it("nests lists by indentation, with tasks and lazy lines", () => {
    const [list] = parseMarkdown("- one\n  - one.a\n  - one.b\n- [x] two\ncontinues\n- [ ] three");
    if (list.type !== "list") throw new Error("not a list");
    expect(list.ordered).toBe(false);
    expect(list.tight).toBe(true);
    expect(list.items).toHaveLength(3);
    const nested = list.items[0].blocks[1];
    expect(nested.type === "list" && nested.items.map((i) => text(i.blocks[0]))).toEqual(["one.a", "one.b"]);
    expect(list.items[1]).toMatchObject({ checked: true });
    expect(text(list.items[1].blocks[0])).toBe("two continues");
    expect(list.items[2]).toMatchObject({ checked: false });
  });

  it("numbers ordered lists from their first number and keeps loose lists apart", () => {
    const [list] = parseMarkdown("3. three\n\n4. four");
    expect(list).toMatchObject({ type: "list", ordered: true, start: 3, tight: false });
    // A different marker starts another list.
    expect(parseMarkdown("- a\n+ b").map((b) => b.type)).toEqual(["list", "list"]);
    // Only "1." may interrupt a paragraph.
    expect(parseMarkdown("It was\n2024. A year")).toHaveLength(1);
  });

  it("reads tables with alignment, escaped pipes and short rows", () => {
    const [table] = parseMarkdown("| Name | Type | Note |\n|:-----|:----:|-----:|\n| `a\\|b` | string |\n| id | int | x |");
    if (table.type !== "table") throw new Error("not a table");
    expect(table.align).toEqual(["left", "center", "right"]);
    expect(table.head.map(inlineText)).toEqual(["Name", "Type", "Note"]);
    expect(table.rows.map((r) => r.map(inlineText))).toEqual([
      ["a|b", "string", ""],
      ["id", "int", "x"],
    ]);
  });

  it("reads quotes and GitHub alerts", () => {
    const [quote, alert] = parseMarkdown("> quoted\n> text\n\n> [!WARNING]\n> Deletes the user.");
    expect(quote).toMatchObject({ type: "quote", alert: undefined });
    expect(quote.type === "quote" && text(quote.blocks[0])).toBe("quoted text");
    expect(alert).toMatchObject({ type: "quote", alert: "warning" });
    expect(alert.type === "quote" && text(alert.blocks[0])).toBe("Deletes the user.");
  });

  it("drops a heading's closing hashes only when they stand apart", () => {
    const headings = parseMarkdown("# a #\n## b ##   \n# c#\n# ###\n# d \\#\n#   \n# e # f").map(text);
    expect(headings).toEqual(["a", "b", "c#", "", "d #", "", "e # f"]);
  });

  it("knows a table's delimiter row", () => {
    const isTable = (delimiter: string) => parseMarkdown(`a | b\n${delimiter}`)[0].type === "table";
    for (const ok of ["---|---", "|---|---|", "  | :-- |  --: |  ", ":-:|-", "-|-"]) expect(isTable(ok), ok).toBe(true);
    for (const no of ["- -|---", "---||---", "-:-|---", "--- | x", ": -|---", "---|-x-"]) expect(isTable(no), no).toBe(false);
  });
});

describe("inline", () => {
  it("reads emphasis, code and strikethrough", () => {
    expect(parseInline("**bold** and *it* and _it_ and ~~gone~~ and `x * y`")).toEqual([
      { type: "strong", children: [{ type: "text", text: "bold" }] },
      { type: "text", text: " and " },
      { type: "em", children: [{ type: "text", text: "it" }] },
      { type: "text", text: " and " },
      { type: "em", children: [{ type: "text", text: "it" }] },
      { type: "text", text: " and " },
      { type: "del", children: [{ type: "text", text: "gone" }] },
      { type: "text", text: " and " },
      { type: "code", text: "x * y" },
    ]);
    expect(parseInline("*a **b** c*")).toEqual([
      { type: "em", children: [{ type: "text", text: "a " }, { type: "strong", children: [{ type: "text", text: "b" }] }, { type: "text", text: " c" }] },
    ]);
  });

  it("leaves snake_case, lone stars and escapes as text", () => {
    expect(parseInline("user_id and page_size")).toEqual([{ type: "text", text: "user_id and page_size" }]);
    expect(parseInline("2 * 3 * 4")).toEqual([{ type: "text", text: "2 * 3 * 4" }]);
    expect(parseInline("\\*not em\\*")).toEqual([{ type: "text", text: "*not em*" }]);
  });

  it("breaks lines on two trailing spaces or a backslash", () => {
    expect(parseInline("a  \nb\\\nc\nd")).toEqual([
      { type: "text", text: "a" },
      { type: "break" },
      { type: "text", text: "b" },
      { type: "break" },
      { type: "text", text: "c d" },
    ]);
    // Spaces and tabs around a newline become one space; two spaces before it break the line.
    expect(parseInline("a \t\n\t b\n  \nc  \\\nd")).toEqual([
      { type: "text", text: "a b " },
      { type: "break" },
      { type: "text", text: "c  " },
      { type: "break" },
      { type: "text", text: "d" },
    ]);
  });

  it("drops one space of padding in code", () => {
    const code = (s: string) => parseInline(s)[0];
    expect(code("` a `")).toEqual({ type: "code", text: "a" });
    expect(code("`  a  `")).toEqual({ type: "code", text: " a " });
    expect(code("` a`")).toEqual({ type: "code", text: " a" });
    expect(code("`   `")).toEqual({ type: "code", text: "   " });
    expect(code("``a ` b``")).toEqual({ type: "code", text: "a ` b" });
    expect(parseInline("```a``")).toEqual([{ type: "text", text: "```a``" }]);
  });

  it("links web and mail addresses, bare ones too", () => {
    expect(links(parseInline("See [the guide](https://example.com/a_(b)) or <mailto:team@example.com>."))).toEqual([
      { href: "https://example.com/a_(b)", text: "the guide" },
      { href: "mailto:team@example.com", text: "team@example.com" },
    ]);
    expect(links(parseInline("Docs: https://example.com/x?y=1. (also https://example.com/z)"))).toEqual([
      { href: "https://example.com/x?y=1", text: "https://example.com/x?y=1" },
      { href: "https://example.com/z", text: "https://example.com/z" },
    ]);
    // Punctuation and unmatched ")" at the end, in any order, aren't part of the address.
    expect(links(parseInline("(see https://example.com/a_(b).) and https://example.com/c?d=e*)!"))).toEqual([
      { href: "https://example.com/a_(b)", text: "https://example.com/a_(b)" },
      { href: "https://example.com/c?d=e", text: "https://example.com/c?d=e" },
    ]);
    // Very long addresses stay text.
    const long = `https://example.com/${"a".repeat(2100)}`;
    expect(links(parseInline(`${long} <${long}>`))).toEqual([]);
    const fits = `https://example.com/${"a".repeat(2000)}`;
    expect(links(parseInline(`${fits} <${fits}>`)).map((l) => l.href)).toEqual([fits, fits]);
  });

  it("gives no address to links that may not open", () => {
    for (const href of ["javascript:alert(1)", "JaVaScRiPt:alert(1)", "data:text/html,<script>alert(1)</script>", "vbscript:x", "file:///etc/passwd", "./other.md", "//evil.example"]) {
      expect(safeHref(href)).toBeNull();
    }
    const nodes = parseInline("[click](javascript:alert(document.cookie))");
    expect(nodes[0]).toMatchObject({ type: "link", href: null });
    expect(safeHref("https://example.com")).toBe("https://example.com");
  });

  it("keeps HTML as text", () => {
    const blocks = parseMarkdown('<script>alert(1)</script>\n\n<img src=x onerror="alert(1)">');
    expect(blocks.map(text)).toEqual(["<script>alert(1)</script>", '<img src=x onerror="alert(1)">']);
  });

  it("shows only embedded pictures in place", () => {
    expect(parseInline("![logo](https://example.com/logo.png)")).toEqual([{ type: "image", alt: "logo", src: "https://example.com/logo.png" }]);
    expect(inlineImage("data:image/png;base64,iVBORw0KGgo=")).toBe(true);
    expect(inlineImage("https://example.com/logo.png")).toBe(false);
    expect(inlineImage("data:image/svg+xml;base64,PHN2Zz4=")).toBe(false);
  });

  it("stays fast on text made to be slow", () => {
    const started = performance.now();
    parseMarkdown(`${"*a _b `c [d ~~e ".repeat(20_000)}\n\n${"[".repeat(50_000)}\n\n${"> ".repeat(5_000)}x\n\n${"- ".repeat(5_000)}x`);
    expect(performance.now() - started).toBeLessThan(2000);
  });

  // Long runs of one character used to take seconds to minutes (the whole window waits).
  it.each([
    ["balanced and closing parentheses after an address", `http://a${"()".repeat(40_000)}${")".repeat(40_000)}`],
    ["closing parentheses after an address", `http://a${")".repeat(40_000)}`],
    ["punctuation inside an address", `http://a${".".repeat(80_000)}b`],
    ["many addresses in one long one", `${"http://a/".repeat(15_000)}${")".repeat(20_000)}`],
    ["spaces in a heading", `# a${" ".repeat(80_000)}b`],
    ["spaces in text", `a${" ".repeat(80_000)}b`],
    ["spaces and tabs in text", `a${" \t".repeat(40_000)}b`],
    ["spaces before a table's delimiter", `a|b\n${" ".repeat(80_000)}x`],
    ["spaces in a table's delimiter", `a|b\n-${" ".repeat(80_000)}x`],
    ["escaped brackets", `${"[\\]".repeat(40_000)}]`],
    ["brackets sharing one destination", `${"[".repeat(40_000)}](${"a".repeat(80_000)}`],
    ["images sharing one destination", `${"![".repeat(40_000)}](${"a".repeat(80_000)}`],
    ["spaces in a destination", `[a](b${" ".repeat(80_000)}x`],
    ["padded code", `\` ${"a".repeat(80_000)}\``],
    ["backtick runs of every length", Array.from({ length: 480 }, (_, i) => `${"`".repeat(i + 1)}a`).join("")],
  ])("stays fast on %s", (_, source) => {
    const started = performance.now();
    parseMarkdown(source);
    expect(performance.now() - started).toBeLessThan(500);
  });
});

describe("preview", () => {
  it("renders all of short docs and whole lines of long ones", () => {
    expect(previewPart("# Hi")).toEqual({ text: "# Hi", cut: false });
    const line = `${"a".repeat(99)}\n`;
    const long = line.repeat(MAX_PREVIEW / 100 + 10);
    const part = previewPart(long);
    expect(part.cut).toBe(true);
    expect(part.text.length).toBeLessThanOrEqual(MAX_PREVIEW);
    expect(part.text.length).toBeGreaterThan(MAX_PREVIEW - 100);
    expect(part.text.endsWith("a")).toBe(true);
    // One long line: cut at the limit, not inside a character.
    expect(previewPart("a".repeat(MAX_PREVIEW + 5)).text).toHaveLength(MAX_PREVIEW);
    const emoji = `${"a".repeat(MAX_PREVIEW - 1)}😀😀`;
    expect(previewPart(emoji).text).toBe("a".repeat(MAX_PREVIEW - 1));
  });
});

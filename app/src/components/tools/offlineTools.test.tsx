// The offline tools (JSON compare, filter playground, regex tester, certificate decoder):
// what they send to Rust as you type, and how they show its answers. The editor is a
// plain textarea here.
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { CertDecodeReport } from "../../bindings/CertDecodeReport";
import type { DecodedItem } from "../../bindings/DecodedItem";
import type { JsonDiff } from "../../bindings/JsonDiff";
import type { RegexTestResult } from "../../bindings/RegexTestResult";
import type { TlsCertificate } from "../../bindings/TlsCertificate";
import type { ToolTab } from "../../store/tabs";

vi.mock("../../lib/events", () => ({ onEvent: () => () => {} }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("../CodeEditor", () => ({
  CodeEditor: ({ value, onChange, placeholder, readOnly }: { value: string; onChange?: (v: string) => void; placeholder?: string; readOnly?: boolean }) => (
    <textarea aria-label={placeholder ?? "Result"} value={value} readOnly={readOnly} onChange={(e) => onChange?.(e.target.value)} />
  ),
}));
vi.mock("../../lib/rpc", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../lib/rpc")>();
  const fn = () => vi.fn((..._args: unknown[]): Promise<unknown> => Promise.resolve(null));
  return { ...actual, api: { jsonDiff: fn(), filterText: fn(), regexTest: fn(), certDecode: fn(), recentResponses: fn(), recentResponse: fn() } };
});

const { api, RpcError } = await import("../../lib/rpc");
const mocked = api as unknown as Record<string, ReturnType<typeof vi.fn>>;
const { TooltipProvider } = await import("../ui");
const { JsonCompareTool } = await import("./JsonCompareTool");
const { FilterPlaygroundTool } = await import("./FilterPlaygroundTool");
const { RegexTesterTool } = await import("./RegexTesterTool");
const { CertDecoderTool } = await import("./CertDecoderTool");
const { useTabs } = await import("../../store/tabs");

beforeEach(() => Object.values(mocked).forEach((m) => m.mockReset()));
afterEach(cleanup);

function renderTool(View: (p: { tab: ToolTab }) => React.ReactNode, tool: string, state: Record<string, unknown> = {}) {
  const tab: ToolTab = { type: "tool", id: `t-${tool}`, tool, state };
  useTabs.setState({ tabs: [tab], activeId: tab.id });
  const Current = () => {
    const current = useTabs((s) => s.tabs.find((t) => t.id === tab.id)) as ToolTab;
    return <View tab={current} />;
  };
  return render(
    <TooltipProvider>
      <Current />
    </TooltipProvider>,
  );
}

const type = (label: string | RegExp, value: string) => fireEvent.change(screen.getByLabelText(label), { target: { value } });

describe("JSON compare", () => {
  const diff: JsonDiff = {
    differences: [
      { path: "$.id", kind: "changed", left: "7", right: '"7"' },
      { path: "$.gone", kind: "removed", left: "true", right: null },
      { path: "$.tags[1]", kind: "added", left: null, right: '"new"' },
    ],
    added: 1,
    removed: 1,
    changed: 1,
    cut: false,
  };

  it("compares once both sides are there and lists the differences", async () => {
    mocked.jsonDiff.mockResolvedValue(diff);
    renderTool(JsonCompareTool, "jsonCompare");
    expect(screen.getByText("Compare two JSON documents")).toBeTruthy();
    type("Paste the left JSON", '{"id": 7}');
    expect(screen.getByText(/Add the right document/)).toBeTruthy();
    type("Paste the right JSON", '{"id": "7"}');
    await vi.waitFor(() => expect(screen.getByTestId("json-diff-summary").textContent).toBe("3 differences"));
    expect(mocked.jsonDiff).toHaveBeenCalledTimes(1);
    expect(mocked.jsonDiff).toHaveBeenCalledWith('{"id": 7}', '{"id": "7"}', false);
    expect(screen.getAllByRole("row")).toHaveLength(4); // header + 3
    // Only the added ones.
    fireEvent.click(screen.getByText("Added 1"));
    const rows = screen.getAllByRole("row").slice(1);
    expect(rows.map((r) => r.getAttribute("data-kind"))).toEqual(["added"]);
    expect(rows[0].textContent).toContain("$.tags[1]");
  });

  it("says when the documents are the same, and shows errors", async () => {
    mocked.jsonDiff.mockResolvedValueOnce({ differences: [], added: 0, removed: 0, changed: 0, cut: false });
    renderTool(JsonCompareTool, "jsonCompare", { left: "[1]", right: "[1.0]", rev: 1 });
    await vi.waitFor(() => expect(screen.getByTestId("json-same")).toBeTruthy());
    mocked.jsonDiff.mockRejectedValueOnce(new RpcError({ code: "invalidInput", message: "The left document isn't valid JSON: EOF", networkKind: null }));
    fireEvent.click(screen.getByLabelText("Ignore array order"));
    await vi.waitFor(() => expect(screen.getByRole("alert").textContent).toContain("left document isn't valid JSON"));
    expect(mocked.jsonDiff).toHaveBeenLastCalledWith("[1]", "[1.0]", true);
  });

  it("keeps the result in the tab: showing it again doesn't compare again", async () => {
    mocked.jsonDiff.mockResolvedValue(diff);
    renderTool(JsonCompareTool, "jsonCompare", { left: "{}", right: "[]", rev: 3 });
    await vi.waitFor(() => expect(screen.getByTestId("json-diff-summary")).toBeTruthy());
    cleanup();
    const state = (useTabs.getState().tabs[0] as ToolTab).state;
    renderTool(JsonCompareTool, "jsonCompare", state);
    expect(screen.getByTestId("json-diff-summary").textContent).toBe("3 differences");
    await new Promise((r) => setTimeout(r, 350));
    expect(mocked.jsonDiff).toHaveBeenCalledTimes(1);
  });
});

describe("filter playground", () => {
  it("runs JSONPath and jq in Rust, with errors about the text", async () => {
    mocked.filterText.mockResolvedValueOnce({ text: "[\n  1,\n  2\n]", count: 2, cut: false });
    renderTool(FilterPlaygroundTool, "filter");
    type("Paste JSON to filter", '{"items": [{"id": 1}, {"id": 2}]}');
    fireEvent.click(screen.getByText("$..id"));
    await vi.waitFor(() => expect(screen.getByTestId("filter-status").textContent).toBe("2 matches"));
    expect(mocked.filterText).toHaveBeenCalledWith('{"items": [{"id": 1}, {"id": 2}]}', "jsonPath", "$..id");
    expect((screen.getByLabelText("Result") as HTMLTextAreaElement).value).toContain("2");

    mocked.filterText.mockRejectedValueOnce(new RpcError({ code: "filter", message: "The response isn't valid JSON (EOF), so it can't be filtered", networkKind: null }));
    fireEvent.click(screen.getByText("jq"));
    type("Expression", ".items | length");
    await vi.waitFor(() => expect(screen.getByRole("alert").textContent).toBe("The text isn't valid JSON (EOF), so it can't be filtered"));
    expect(mocked.filterText).toHaveBeenLastCalledWith(expect.any(String), "jq", ".items | length");
  });

  it("runs XPath here", async () => {
    renderTool(FilterPlaygroundTool, "filter", { kind: "xpath" });
    type("Paste XML or HTML to filter", "<a><b>1</b><b>2</b></a>");
    type("Expression", "count(//b)");
    await vi.waitFor(() => expect(screen.getByTestId("filter-status").textContent).toBe("1 match"));
    expect((screen.getByLabelText("Result") as HTMLTextAreaElement).value).toBe("2");
    expect(mocked.filterText).not.toHaveBeenCalled();
  });
});

describe("regex tester", () => {
  const result: RegexTestResult = {
    pattern: "(?i)id=(?P<n>\\d+)",
    groupNames: ["n"],
    matches: [
      { start: 0, end: 4, text: "ID=7", groups: [{ start: 3, end: 4, text: "7" }] },
      { start: 5, end: 9, text: "id=8", groups: [{ start: 8, end: 9, text: "8" }] },
    ],
    count: 2,
    countCapped: false,
    capture: "7",
  };

  it("sends the pattern, flags and text, and lists matches with groups", async () => {
    mocked.regexTest.mockResolvedValue(result);
    renderTool(RegexTesterTool, "regex");
    expect(screen.getByText("Test a regular expression")).toBeTruthy();
    type(/Paste a response body/, "ID=7 id=8");
    type("Pattern", "id=(?P<n>\\d+)");
    fireEvent.click(screen.getByRole("button", { name: "Ignore case" }));
    expect(screen.getByRole("button", { name: "Ignore case" }).getAttribute("aria-pressed")).toBe("true");
    await vi.waitFor(() => expect(screen.getByTestId("regex-status").textContent).toBe("2 matches"));
    expect(mocked.regexTest).toHaveBeenLastCalledWith("id=(?P<n>\\d+)", { ignoreCase: true, multiLine: false, dotAll: false, ignoreWhitespace: false }, "ID=7 id=8");
    expect(screen.getByTestId("regex-capture").textContent).toBe("7");
    expect(screen.getByText("(?i)id=(?P<n>\\d+)")).toBeTruthy(); // the pattern with its flags, to copy
    const rows = screen.getByTestId("regex-matches").querySelectorAll("tbody tr");
    expect(rows).toHaveLength(2);
    expect(rows[1].textContent).toContain("id=8");
    expect(rows[1].textContent).toContain("n8");
  });

  it("shows the regex crate's explanation of a bad pattern", async () => {
    mocked.regexTest.mockRejectedValue(new RpcError({ code: "regex", message: "regex parse error:\n    (\n    ^\nerror: unclosed group", networkKind: null }));
    renderTool(RegexTesterTool, "regex");
    type("Pattern", "(");
    await vi.waitFor(() => expect(screen.getByRole("alert").textContent).toContain("unclosed group"));
    expect(screen.queryByTestId("regex-result")).toBeNull();
  });
});

describe("certificate decoder", () => {
  const details: TlsCertificate = {
    subject: "CN=api.example.test",
    commonName: "api.example.test",
    issuer: "CN=Test Root",
    issuerCommonName: "Test Root",
    notBefore: "2026-01-01T00:00:00Z",
    notAfter: "2027-01-01T00:00:00Z",
    daysLeft: 200,
    notYetValid: false,
    subjectAltNames: ["api.example.test"],
    keyType: "EC",
    keyBits: 256,
    keyCurve: "P-256",
    signatureAlgorithm: "ECDSA with SHA-256",
    sha256: "AB:CD",
    serial: "01",
    isCa: false,
    selfSigned: false,
    parseError: null,
  };
  const base = { certificate: null, request: null, key: null, note: null, error: null };
  const items: DecodedItem[] = [
    {
      ...base,
      kind: "certificate",
      label: "CERTIFICATE",
      certificate: { details, version: 3, keyUsage: ["Digital Signature"], extendedKeyUsage: ["Server authentication"], pem: "-----BEGIN CERTIFICATE-----\n" },
    },
    { ...base, kind: "privateKey", label: "PRIVATE KEY", key: { keyType: "EC", keyBits: 256, keyCurve: "P-256", private: true, encrypted: false, matches: [0] } },
    { ...base, kind: "other", label: "X509 CRL", note: "A certificate revocation list. Its contents aren't shown here." },
  ];
  const report: CertDecodeReport = {
    items,
    warnings: [{ level: "warning", code: "chainOrder", message: "'Other Root' did not issue 'api.example.test', the certificate before it." }],
  };

  it("decodes pasted PEM and shows each item and the findings", async () => {
    mocked.certDecode.mockResolvedValue(report);
    renderTool(CertDecoderTool, "certs");
    expect(screen.getByText("Decode a certificate")).toBeTruthy();
    type("PEM text", "-----BEGIN CERTIFICATE-----\nMII…");
    await vi.waitFor(() => expect(screen.getByTestId("cert-report")).toBeTruthy());
    expect(mocked.certDecode).toHaveBeenCalledWith({ text: "-----BEGIN CERTIFICATE-----\nMII…" });
    expect(screen.getByText(/did not issue/)).toBeTruthy();
    expect(screen.getByText("1 · Certificate")).toBeTruthy();
    expect(screen.getByText("Server authentication")).toBeTruthy();
    expect(screen.getByText("X.509 v3")).toBeTruthy();
    expect(screen.getByText("2 · Private key")).toBeTruthy();
    expect(screen.getByText("certificate 1 (api.example.test)")).toBeTruthy();
    expect(screen.getByText(/revocation list/)).toBeTruthy();
  });

  it("reads a file instead of the text when one is open", async () => {
    mocked.certDecode.mockRejectedValue(new RpcError({ code: "invalidInput", message: "This looks like a PKCS#12 file", networkKind: null }));
    renderTool(CertDecoderTool, "certs", { path: "/certs/client.p12", rev: 1 });
    expect(screen.getByText("client.p12")).toBeTruthy();
    await vi.waitFor(() => expect(screen.getByRole("alert").textContent).toContain("PKCS#12"));
    expect(mocked.certDecode).toHaveBeenCalledWith({ path: "/certs/client.p12" });
    fireEvent.click(screen.getByText("Paste text instead"));
    expect(screen.getByLabelText("PEM text")).toBeTruthy();
  });
});

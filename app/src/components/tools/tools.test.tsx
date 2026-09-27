// Render the network tools with realistic state (no backend): they show what
// the state holds and never crash on partial or empty results.
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { TlsReport } from "../../bindings/TlsReport";
import type { ToolTab } from "../../store/tabs";

vi.mock("../../lib/events", () => ({ onEvent: () => () => {} }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("../../lib/rpc", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../lib/rpc")>();
  const netInterfaces = vi.fn(() => Promise.resolve([]));
  const portCheck = vi.fn(() => Promise.resolve({ address: "10.0.0.5", total: 1 }));
  const ping = vi.fn(() => Promise.resolve({ mode: "icmp", address: "10.0.0.5", port: null, note: null }));
  return { ...actual, api: { netInterfaces, portCheck, ping, cancelTool: vi.fn(() => Promise.resolve(null)) } };
});

const { api } = await import("../../lib/rpc");

const { TooltipProvider } = await import("../ui");
const { TlsInspectorTool } = await import("./TlsInspectorTool");
const { PortCheckTool } = await import("./PortCheckTool");
const { PingTool } = await import("./PingTool");
const { InterfacesTool } = await import("./InterfacesTool");
const { EncodersTool } = await import("./EncodersTool");
const { useTabs } = await import("../../store/tabs");

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

const cert = {
  subject: "CN=example.com",
  commonName: "example.com",
  issuer: "CN=Example CA",
  issuerCommonName: "Example CA",
  notBefore: "2026-01-01T00:00:00Z",
  notAfter: "2026-10-10T00:00:00Z",
  daysLeft: 12,
  notYetValid: false,
  subjectAltNames: ["example.com", "www.example.com"],
  keyType: "EC",
  keyBits: 256,
  keyCurve: "P-256",
  signatureAlgorithm: "ECDSA with SHA-256",
  sha256: "AB:CD",
  serial: "01:02",
  isCa: false,
  selfSigned: false,
  parseError: null,
};

const report: TlsReport = {
  host: "example.com",
  port: 443,
  serverName: "example.com",
  remoteAddr: "93.184.216.34:443",
  proxy: null,
  version: "TLS 1.3",
  cipher: "TLS13_AES_128_GCM_SHA256",
  keyExchange: "X25519",
  alpn: "h2",
  trusted: false,
  trustError: "Not issued by a trusted certificate authority",
  hostnameMatches: true,
  ocspStapled: true,
  chain: [cert, { ...cert, commonName: null, subject: "CN=Broken", parseError: "Could not parse the certificate: bad DER" }],
  versions: [
    { version: "TLS 1.3", supported: true, detail: null },
    { version: "TLS 1.2", supported: false, detail: "Refused by the server (ProtocolVersion)" },
    { version: "TLS 1.1", supported: null, detail: "Can't be tested" },
    { version: "TLS 1.0", supported: null, detail: "Can't be tested" },
  ],
  ciphers: [
    { name: "TLS13_AES_128_GCM_SHA256", version: "TLS 1.3", accepted: true, forwardSecrecy: true, detail: null },
    { name: "TLS13_AES_256_GCM_SHA384", version: "TLS 1.3", accepted: null, forwardSecrecy: true, detail: "No answer within 5s" },
  ],
  warnings: [
    { level: "danger", code: "untrusted", message: "Not trusted. Unknown issuer." },
    { level: "warning", code: "expiresSoon", message: "The certificate expires in 12 days (2026-10-10)." },
  ],
  handshakeMs: 45.2,
  durationMs: 310,
};

describe("tool views", () => {
  it("TLS inspector shows the verdict, chain, versions and suites", () => {
    renderTool(TlsInspectorTool, "tls", { host: "example.com", status: "done", report });
    expect(screen.getByText("Not trusted")).toBeTruthy();
    // In the summary and on the server certificate's card.
    expect(screen.getAllByText("Expires in 12 days")).toHaveLength(2);
    expect(screen.getByText("Not trusted. Unknown issuer.")).toBeTruthy();
    expect(screen.getAllByTestId("tls-certificate")).toHaveLength(1);
    expect(screen.getByText(/bad DER/)).toBeTruthy();
    expect(screen.getByText("Not supported")).toBeTruthy();
    expect(screen.getAllByText("Can't be tested")).toHaveLength(2);
    expect(screen.getByText("No answer")).toBeTruthy();
  });

  it("TLS inspector starts empty and validates input", () => {
    renderTool(TlsInspectorTool, "tls");
    expect(screen.getByText("Inspect a server's TLS")).toBeTruthy();
    fireEvent.click(screen.getByTestId("tls-inspect"));
    expect(screen.getByRole("alert").textContent).toMatch(/Enter a host/);
  });

  it("port check lists open ports first with a summary", () => {
    renderTool(PortCheckTool, "ports", {
      host: "10.0.0.5",
      status: "done",
      address: "10.0.0.5",
      total: 3,
      results: [
        { port: 22, open: false, ms: 1, error: "refused", message: null },
        { port: 443, open: true, ms: 3, error: null, message: null },
        { port: 80, open: false, ms: 2000, error: "timeout", message: null },
      ],
      summary: { address: "10.0.0.5", open: [443], refused: 1, timedOut: 1, errors: 0, checked: 3, total: 3, durationMs: 2001, cancelled: false },
    });
    const rows = screen.getByTestId("port-results").querySelectorAll("tbody tr");
    expect(Array.from(rows).map((r) => r.firstElementChild?.textContent)).toEqual(["443", "22", "80"]);
    expect(screen.getByText("HTTPS")).toBeTruthy();
    expect(screen.getByText("No answer", { selector: "span" })).toBeTruthy();
  });

  it("choosing options doesn't run a tool; Enter runs it with them", async () => {
    renderTool(PortCheckTool, "ports", { host: "10.0.0.5" });
    fireEvent.click(screen.getByText("Top 20"));
    fireEvent.click(screen.getByText("Custom"));
    expect(api.portCheck).not.toHaveBeenCalled();
    fireEvent.change(screen.getByLabelText("Custom ports"), { target: { value: "22" } });
    fireEvent.keyDown(screen.getByLabelText("Host"), { key: "Enter" });
    await vi.waitFor(() => expect(api.portCheck).toHaveBeenCalledWith(expect.any(String), "10.0.0.5", "22", 2000));
    cleanup();

    renderTool(PingTool, "ping");
    fireEvent.click(screen.getByText("TCP"));
    expect(api.ping).not.toHaveBeenCalled();
    expect(screen.queryByRole("alert")).toBeNull(); // not "Enter a host"
  });

  it("port check warns before scanning many ports on a public host", () => {
    renderTool(PortCheckTool, "ports", { host: "example.com", preset: "custom", custom: "1-500" });
    expect(screen.getByText(/Only scan hosts you're allowed to test/)).toBeTruthy();
  });

  it("ping shows stats, the chart and the fallback note", () => {
    renderTool(PingTool, "ping", {
      host: "example.com",
      status: "running",
      runId: "r",
      started: { mode: "tcp", address: "93.184.216.34", port: 443, note: "ICMP is not permitted for this user. Using TCP connect time to port 443 instead." },
      replies: [
        { seq: 1, ms: 12.5, ttl: null, error: null },
        { seq: 2, ms: null, ttl: null, error: "Timed out" },
      ],
      stats: { sent: 2, received: 1, sum: 12.5, min: 12.5, max: 12.5 },
    });
    expect(screen.getByText(/ICMP is not permitted/)).toBeTruthy();
    expect(screen.getByText("50%")).toBeTruthy();
    expect(screen.getByRole("img", { name: "Round-trip times" })).toBeTruthy();
    expect(screen.getByText("Timed out")).toBeTruthy();
    expect(screen.getByText("Stop")).toBeTruthy();
  });

  it("interfaces show addresses and the best one for other devices", () => {
    renderTool(InterfacesTool, "interfaces", {
      status: "done",
      list: [
        { name: "en0", loopback: false, up: true, addresses: [{ ip: "192.168.1.20", prefix: 24, family: "ipv4", linkLocal: false }, { ip: "fe80::1", prefix: 64, family: "ipv6", linkLocal: true }] },
        { name: "lo0", loopback: true, up: true, addresses: [{ ip: "127.0.0.1", prefix: 8, family: "ipv4", linkLocal: false }] },
      ],
    });
    expect(screen.getByText(/Best for other devices/).textContent).toContain("192.168.1.20");
    expect(screen.getByText("loopback")).toBeTruthy();
    expect(screen.getByText("link-local")).toBeTruthy();
    expect(screen.getByText(/start the server on/)).toBeTruthy();
  });

  it("encoders convert as you type and switch modes", () => {
    renderTool(EncodersTool, "encoders");
    fireEvent.change(screen.getByLabelText("Input"), { target: { value: "héllo" } });
    expect((screen.getByLabelText("Output") as HTMLTextAreaElement).value).toBe("aMOpbGxv");
    fireEvent.click(screen.getByText("Decode"));
    expect(screen.getByRole("alert").textContent).toMatch(/Base64/);
    fireEvent.click(screen.getByText("Timestamp"));
    fireEvent.change(screen.getByLabelText("Timestamp or date"), { target: { value: "0" } });
    expect(screen.getByText("1970-01-01T00:00:00.000Z")).toBeTruthy();
    fireEvent.click(screen.getByText("JWT"));
    fireEvent.change(screen.getByLabelText("Token"), { target: { value: "abc.def" } });
    expect(screen.getByText(/three parts/)).toBeTruthy();
  });

  it("judges a JWT's expiry when it is pasted, not when the panel opened", () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    try {
      vi.setSystemTime(new Date("2026-01-01T00:00:00Z"));
      renderTool(EncodersTool, "encoders", { mode: "jwt" });
      vi.setSystemTime(new Date("2026-01-01T02:00:00Z"));
      const part = (v: object) => btoa(JSON.stringify(v)).replace(/=+$/, "").replace(/\+/g, "-").replace(/\//g, "_");
      const token = [part({ alg: "HS256" }), part({ exp: Date.parse("2026-01-01T01:00:00Z") / 1000 }), "sig"].join(".");
      fireEvent.change(screen.getByLabelText("Token"), { target: { value: token } });
      expect(screen.getByText("Expired")).toBeTruthy();
    } finally {
      vi.useRealTimers();
    }
  });
});

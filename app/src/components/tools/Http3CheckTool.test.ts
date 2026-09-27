import { describe, expect, it } from "vitest";
import type { ErrorKind } from "../../bindings/ErrorKind";
import { isH3Protocol, normalizeCheckUrl, parseAltSvc, verdictFor, type Outcome } from "./Http3CheckTool";

const ok: Outcome = {
  ok: true,
  status: 200,
  statusText: "OK",
  httpVersion: "HTTP/3",
  url: "https://example.com/",
  timing: { redirectMs: 0, dnsMs: 1, connectMs: 10, tlsMs: 0, ttfbMs: 5, downloadMs: 1, totalMs: 17 },
  tls: null,
  remoteAddr: "93.184.215.14:443",
  altSvc: null,
};
const failed = (kind: ErrorKind): Outcome => ({ ok: false, kind, message: "boom" });

describe("Alt-Svc parsing", () => {
  it("reads h3 entries with max-age and quoted authorities", () => {
    const { clear, entries } = parseAltSvc('h3=":443"; ma=86400, h3-29=":443"; ma=3600, h2="alt.example.com:443"');
    expect(clear).toBe(false);
    expect(entries).toEqual([
      { protocol: "h3", authority: ":443", maxAge: 86400 },
      { protocol: "h3-29", authority: ":443", maxAge: 3600 },
      { protocol: "h2", authority: "alt.example.com:443", maxAge: 86400 },
    ]);
    expect(entries.filter((e) => isH3Protocol(e.protocol)).map((e) => e.protocol)).toEqual(["h3", "h3-29"]);
  });

  it("handles clear, commas inside quotes, percent-encoding and junk", () => {
    expect(parseAltSvc("clear")).toEqual({ clear: true, entries: [] });
    expect(parseAltSvc('h3="a,b:443"; persist=1').entries).toEqual([{ protocol: "h3", authority: "a,b:443", maxAge: 86400 }]);
    expect(parseAltSvc("h3%2D29=\":8443\"").entries[0].protocol).toBe("h3-29");
    expect(parseAltSvc(" , garbage, =x").entries).toEqual([]);
    expect(parseAltSvc('h3=":443"; ma=-5').entries[0].maxAge).toBe(86400);
    expect(isH3Protocol("h2")).toBe(false);
    expect(isH3Protocol("h3x")).toBe(false);
  });
});

describe("HTTP/3 check", () => {
  it("normalizes the URL to https", () => {
    expect(normalizeCheckUrl(" example.com/x ")).toEqual({ url: "https://example.com/x", note: null });
    expect(normalizeCheckUrl("https://a.test")).toEqual({ url: "https://a.test", note: null });
    expect(normalizeCheckUrl("http://a.test/p").url).toBe("https://a.test/p");
    expect(normalizeCheckUrl("http://a.test/p").note).toMatch(/https/);
  });

  it("picks a verdict", () => {
    expect(verdictFor(ok, ok, true).title).toBe("Speaks HTTP/3");
    expect(verdictFor(ok, ok, false).tone).toBe("success");
    expect(verdictFor(ok, failed("connect"), true).title).toBe("Advertises HTTP/3 but the QUIC connection failed (UDP blocked?)");
    expect(verdictFor(ok, failed("timeout"), true).tone).toBe("warning");
    expect(verdictFor(ok, failed("tls"), true).title).toMatch(/request failed/);
    expect(verdictFor(ok, failed("connect"), false).title).toBe("No HTTP/3");
    expect(verdictFor(ok, failed("proxy"), true).title).toMatch(/proxy/);
    expect(verdictFor(failed("dns"), failed("dns"), false).title).toBe("Could not reach the site");
  });
});

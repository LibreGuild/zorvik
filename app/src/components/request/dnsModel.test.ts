import { describe, expect, it } from "vitest";
import { bareName, formatTtl, isIpAddress, presetFor, rcodeTone, resolverProblem, resolverProtocol, reverseName, zoneText } from "./dnsModel";

describe("reverseName", () => {
  it("reverses IPv4 addresses", () => {
    expect(reverseName("1.2.3.4")).toBe("4.3.2.1.in-addr.arpa");
    expect(reverseName(" 192.0.2.10 ")).toBe("10.2.0.192.in-addr.arpa");
    expect(reverseName("256.1.1.1")).toBeNull();
    expect(reverseName("1.2.3")).toBeNull();
  });

  it("expands IPv6 addresses to nibbles", () => {
    expect(reverseName("::1")).toBe(`1.${"0.".repeat(31)}ip6.arpa`);
    expect(reverseName("2001:db8::567:89ab")).toBe("b.a.9.8.7.6.5.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.8.b.d.0.1.0.0.2.ip6.arpa");
    expect(reverseName("[2001:db8::1]:53")).toBe(reverseName("2001:db8::1"));
    expect(reverseName("::ffff:1.2.3.4")).toBe(reverseName("::ffff:102:304"));
    expect(reverseName("1::2::3")).toBeNull();
    expect(reverseName("1:2:3:4:5:6:7:8:9")).toBeNull();
    expect(reverseName("g::1")).toBeNull();
  });

  it("ignores names", () => {
    expect(reverseName("example.com")).toBeNull();
    expect(isIpAddress("example.com")).toBe(false);
    expect(isIpAddress("https://10.0.0.1:8443/x")).toBe(true);
  });
});

describe("bareName", () => {
  it("strips scheme, user, port and path like the backend", () => {
    expect(bareName("https://user@api.example.com:8443/x?y#z")).toBe("api.example.com");
    expect(bareName("example.com:53")).toBe("example.com");
    expect(bareName("2001:db8::1")).toBe("2001:db8::1");
    expect(bareName("_sip._tcp.example.com")).toBe("_sip._tcp.example.com");
  });
});

describe("resolvers", () => {
  it("recognises presets", () => {
    expect(presetFor("")).toBe("system");
    expect(presetFor(undefined)).toBe("system");
    expect(presetFor(" 1.1.1.1 ")).toBe("cloudflare");
    expect(presetFor("8.8.8.8:53")).toBe("google");
    expect(presetFor("https://dns.google/dns-query")).toBe("google-doh");
    expect(presetFor("192.168.1.1")).toBe("custom");
  });

  it("tells the protocol", () => {
    expect(resolverProtocol("")).toBe("System");
    expect(resolverProtocol("[2606:4700::1111]:53")).toBe("UDP");
    expect(resolverProtocol("tcp://1.1.1.1")).toBe("TCP");
    expect(resolverProtocol("TLS://dns.google")).toBe("DoT");
    expect(resolverProtocol("https://cloudflare-dns.com/dns-query")).toBe("DoH");
    expect(resolverProtocol("ftp://x")).toBeNull();
  });

  it("flags obvious mistakes", () => {
    expect(resolverProblem("1.1.1.1")).toBeNull();
    expect(resolverProblem("{{dns}}")).toBeNull();
    expect(resolverProblem("ftp://x")).toContain("udp://");
    expect(resolverProblem("1.1.1.1:70000")).toContain("65535");
    expect(resolverProblem("tls://1.1.1.1:853")).toBeNull();
    expect(resolverProblem("1.1.1 .1")).toContain("spaces");
  });
});

describe("display", () => {
  it("formats TTLs and zone text", () => {
    expect(formatTtl(59)).toBe("59s");
    expect(formatTtl(90)).toBe("1m 30s");
    expect(formatTtl(86400 + 7200 + 5)).toBe("1d 2h");
    expect(zoneText([{ name: "a.test.", type: "A", class: "IN", ttl: 60, data: "192.0.2.1" }])).toBe("a.test.\t60\tIN\tA\t192.0.2.1");
    expect(rcodeTone("NXDOMAIN")).toBe("warning");
    expect(rcodeTone("SERVFAIL")).toBe("danger");
    expect(rcodeTone("RCODE42")).toBe("muted");
  });
});

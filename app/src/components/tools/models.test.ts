import { describe, expect, it } from "vitest";
import type { DecodedItem } from "../../bindings/DecodedItem";
import type { JsonDiff } from "../../bindings/JsonDiff";
import type { RegexMatch } from "../../bindings/RegexMatch";
import { diffAsText, diffCount, filterError, itemRole, itemTitle, keyDescription, keyOwners, looksLikeHtml, matchRanges, showText, visibleDifferences } from "./models";
import { shortUrl } from "./RecentResponseMenu";

describe("JSON compare", () => {
  const diff: JsonDiff = {
    differences: [
      { path: "$.id", kind: "changed", left: "7", right: '"7"' },
      { path: "$['odd key']", kind: "removed", left: "true", right: null },
      { path: "$.tags[1]", kind: "added", left: null, right: '"new"' },
    ],
    added: 1,
    removed: 1,
    changed: 4, // more than listed (the list was cut)
    cut: true,
  };

  it("filters by kind and counts from the totals", () => {
    expect(visibleDifferences(diff, "all")).toHaveLength(3);
    expect(visibleDifferences(diff, "added").map((d) => d.path)).toEqual(["$.tags[1]"]);
    expect(diffCount(diff, "all")).toBe(6);
    expect(diffCount(diff, "changed")).toBe(4);
  });

  it("writes differences as text", () => {
    expect(diffAsText(diff.differences)).toBe(`~ $.id: 7 → "7"\n- $['odd key']: true\n+ $.tags[1]: "new"`);
  });
});

describe("filter playground", () => {
  it("speaks of the text, not a response", () => {
    expect(filterError("The response isn't valid JSON (EOF), so it can't be filtered")).toBe("The text isn't valid JSON (EOF), so it can't be filtered");
    expect(filterError("Not a valid JSONPath expression: x")).toBe("Not a valid JSONPath expression: x");
  });

  it("parses HTML leniently, everything else as XML", () => {
    expect(looksLikeHtml("  <!DOCTYPE html><html>")).toBe(true);
    expect(looksLikeHtml("<html lang=en>")).toBe(true);
    expect(looksLikeHtml("<?xml version='1.0'?><html/>")).toBe(false);
    expect(looksLikeHtml("<htmlish/>")).toBe(false);
  });
});

describe("regex tester", () => {
  const m = (start: number, end: number): RegexMatch => ({ start, end, text: "", groups: [] });

  it("highlights matches inside the text, without overlaps or empty ranges", () => {
    expect(matchRanges([m(0, 2), m(2, 2), m(3, 9)], 6)).toEqual([
      { from: 0, to: 2, index: 0 },
      { from: 3, to: 6, index: 2 },
    ]);
    expect(matchRanges([m(0, 4), m(2, 5)], 10)).toEqual([
      { from: 0, to: 4, index: 0 },
      { from: 4, to: 5, index: 1 },
    ]);
    expect(matchRanges([m(8, 9)], 5)).toEqual([]);
  });

  it("makes whitespace in values visible", () => {
    expect(showText("a\r\n\tb")).toBe("a\\r\\n\\tb");
    expect(showText("")).toBe("(empty)");
  });
});

describe("certificate decoder", () => {
  const base = { certificate: null, request: null, key: null, note: null, error: null, label: "" };
  const cert = (cn: string | null, selfSigned: boolean, isCa: boolean): DecodedItem => ({
    ...base,
    kind: "certificate",
    label: "CERTIFICATE",
    certificate: {
      details: {
        subject: cn ? `CN=${cn}` : "O=Org",
        commonName: cn,
        issuer: "",
        issuerCommonName: null,
        notBefore: "",
        notAfter: "",
        daysLeft: 1,
        notYetValid: false,
        subjectAltNames: [],
        keyType: "RSA",
        keyBits: 2048,
        keyCurve: null,
        signatureAlgorithm: "",
        sha256: "",
        serial: "",
        isCa,
        selfSigned,
        parseError: null,
      },
      version: 3,
      keyUsage: [],
      extendedKeyUsage: [],
      pem: "",
    },
  });
  const items: DecodedItem[] = [
    cert("leaf.test", false, false),
    cert(null, false, true),
    cert("Root", true, true),
    {
      ...base,
      kind: "request",
      request: { subject: "CN=req.test", commonName: "req.test", subjectAltNames: [], keyType: "EC", keyBits: 256, keyCurve: "P-256", signatureAlgorithm: "" },
    },
    { ...base, kind: "privateKey", key: { keyType: "EC", keyBits: 256, keyCurve: "P-256", private: true, encrypted: false, matches: [0, 3] } },
    { ...base, kind: "privateKey", key: { keyType: "", keyBits: null, keyCurve: null, private: true, encrypted: true, matches: [] } },
    { ...base, kind: "other", label: "X509 CRL" },
  ];

  it("names each item and its role", () => {
    expect(items.map((_, i) => itemRole(items, i))).toEqual([
      "Certificate",
      "Intermediate certificate",
      "Root certificate",
      "Certificate request",
      "Private key",
      "Encrypted private key",
      "X509 CRL",
    ]);
    expect(items.map(itemTitle)).toEqual(["leaf.test", "O=Org", "Root", "req.test", "EC · P-256 · 256 bits", "Private key", "X509 CRL"]);
    expect(keyDescription("RSA", 2048, null)).toBe("RSA · 2048 bits");
  });

  it("says which items a key belongs to", () => {
    expect(keyOwners(items, [0, 3])).toBe("certificate 1 (leaf.test), request 4 (req.test)");
    expect(keyOwners(items, [42])).toBe("");
  });
});

describe("recent responses", () => {
  it("shortens URLs for the menu", () => {
    expect(shortUrl("https://api.example.com/v1/users?x=1")).toBe("api.example.com/v1/users?x=1");
    expect(shortUrl("api.example.com")).toBe("api.example.com");
  });
});

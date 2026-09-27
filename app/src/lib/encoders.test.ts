// @vitest-environment node
// (Node's WebCrypto provides crypto.subtle for the hash helpers.)
import { describe, expect, it } from "vitest";
import {
  base64Decode,
  base64Encode,
  base64ToBytes,
  decodeJwt,
  digestHex,
  hexToBytes,
  hexToText,
  parseTimestamp,
  relativeTime,
  textToHex,
  urlDecode,
  urlEncode,
} from "./encoders";

describe("base64", () => {
  it("round-trips UTF-8 text", () => {
    const text = "héllo wörld 👋 — ✓";
    expect(base64Decode(base64Encode(text)).text).toBe(text);
    expect(base64Encode("hi?>")).toBe("aGk/Pg==");
    expect(base64Encode("hi?>", true)).toBe("aGk_Pg");
    expect(base64Decode("aGk_Pg").text).toBe("hi?>");
  });

  it("tolerates whitespace, missing padding and line breaks", () => {
    expect(base64Decode(" aGVs\nbG8 ").text).toBe("hello");
    expect(base64Decode("aGVsbG8").text).toBe("hello");
    expect(base64Decode("").text).toBe("");
  });

  it("shows hex for bytes that are not text", () => {
    const r = base64Decode("/wD+");
    expect(r.text).toBeNull();
    expect(r.hex).toBe("ff 00 fe");
    expect(r.size).toBe(3);
  });

  it("explains invalid input", () => {
    expect(() => base64ToBytes("abc$")).toThrow(/unexpected character '\$'/);
    expect(() => base64ToBytes("abcde")).toThrow(/length/);
    expect(() => base64ToBytes("ab=c")).toThrow(/padding/);
  });

  it("handles large inputs", () => {
    const big = "x".repeat(300_000);
    expect(base64Decode(base64Encode(big)).text).toBe(big);
  });
});

describe("url", () => {
  it("encodes components and decodes form-style input", () => {
    expect(urlEncode("a b&c=d/é")).toBe("a%20b%26c%3Dd%2F%C3%A9");
    expect(urlDecode("a%20b%26c%3Dd%2F%C3%A9")).toBe("a b&c=d/é");
    expect(urlDecode("a+b")).toBe("a b");
    expect(urlDecode("a+b", false)).toBe("a+b");
  });

  it("explains malformed escapes", () => {
    expect(() => urlDecode("100%zz")).toThrow(/position 4/);
    expect(() => urlDecode("%E0%A4%A")).toThrow(/position/);
    expect(() => urlDecode("%C3%28")).toThrow(/UTF-8/);
  });
});

describe("hex", () => {
  it("converts text to hex and back", () => {
    expect(textToHex("Hi é")).toBe("48 69 20 c3 a9");
    expect(textToHex("Hi", "")).toBe("4869");
    expect(hexToText("48 69 20 c3 a9").text).toBe("Hi é");
  });

  it("accepts common hex notations", () => {
    for (const input of ["4869", "48 69", "48:69", "0x48 0x69", "\\x48\\x69", "48-69", "  4869\n"]) {
      expect(Array.from(hexToBytes(input))).toEqual([0x48, 0x69]);
    }
    expect(hexToText("ff").text).toBeNull();
  });

  it("rejects invalid hex", () => {
    expect(() => hexToBytes("486")).toThrow(/odd/);
    expect(() => hexToBytes("48zz")).toThrow(/'z'/);
    expect(() => hexToBytes("100x20")).toThrow(/'x'/);
  });
});

describe("jwt", () => {
  const b64url = (v: unknown) => base64Encode(JSON.stringify(v), true);
  const now = Date.UTC(2024, 0, 15);
  const token = [
    b64url({ alg: "HS256", typ: "JWT" }),
    b64url({ sub: "123", name: "Zoë", iat: now / 1000 - 3600, exp: now / 1000 - 60 }),
    "c2lnbmF0dXJl",
  ].join(".");

  it("decodes header, payload and times", () => {
    const jwt = decodeJwt(`Bearer ${token}`, now);
    expect(jwt.algorithm).toBe("HS256");
    expect((jwt.payload as { name: string }).name).toBe("Zoë");
    expect(jwt.payloadJson).toContain('\n  "sub": "123"');
    expect(jwt.times.map((t) => t.claim)).toEqual(["iat", "exp"]);
    expect(jwt.times[1].date.toISOString()).toBe("2024-01-14T23:59:00.000Z");
    expect(jwt.expired).toBe(true);
    expect(jwt.notYetValid).toBe(false);
    expect(jwt.signature).toBe("c2lnbmF0dXJl");
  });

  it("explains tokens it can't read", () => {
    expect(() => decodeJwt("")).toThrow(/Paste/);
    expect(() => decodeJwt("abc.def")).toThrow(/three parts/);
    expect(() => decodeJwt("a.b.c.d.e")).toThrow(/encrypted/);
    expect(() => decodeJwt(`${b64url({ alg: "none" })}.bm90IGpzb24.`)).toThrow(/payload is not JSON/);
    expect(() => decodeJwt("$$$.e30.x")).toThrow(/header is not valid/);
  });

  it("accepts unsigned tokens and non-object payloads", () => {
    const jwt = decodeJwt(`${b64url({ alg: "none" })}.${b64url([1, 2])}.`);
    expect(jwt.signature).toBe("");
    expect(jwt.times).toEqual([]);
  });
});

describe("hashes", () => {
  it("matches known digests", async () => {
    expect(await digestHex("SHA-1", "abc")).toBe("a9993e364706816aba3e25717850c26c9cd0d89d");
    expect(await digestHex("SHA-256", "abc")).toBe("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    expect(await digestHex("SHA-512", "")).toBe(
      "cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e",
    );
    // UTF-8, not UTF-16.
    expect(await digestHex("SHA-256", "é")).toBe("4a99557e4033c3539de2eb65472017cad5f9557f7a0625a09f1c3f6e2ba69c4c");
  });
});

describe("timestamps", () => {
  const now = Date.UTC(2023, 10, 14, 22, 13, 20);

  it("guesses the unit from the size", () => {
    expect(parseTimestamp("1700000000", now)).toMatchObject({ input: "s", iso: "2023-11-14T22:13:20.000Z", unixMs: 1700000000000 });
    expect(parseTimestamp("1700000000123", now)).toMatchObject({ input: "ms", unixSeconds: 1700000000 });
    expect(parseTimestamp("1700000000000000", now).input).toBe("µs");
    expect(parseTimestamp("1700000000000000000", now).iso).toBe("2023-11-14T22:13:20.000Z");
    expect(parseTimestamp("0", now).iso).toBe("1970-01-01T00:00:00.000Z");
    expect(parseTimestamp("-86400", now).iso).toBe("1969-12-31T00:00:00.000Z");
  });

  it("reads date strings", () => {
    const t = parseTimestamp("2024-01-31T12:00:00Z", now);
    expect(t.input).toBe("date");
    expect(t.unixSeconds).toBe(1706702400);
    expect(parseTimestamp("Tue, 14 Nov 2023 22:13:20 GMT", now).unixSeconds).toBe(1700000000);
  });

  it("rejects nonsense and out-of-range values", () => {
    expect(() => parseTimestamp("", now)).toThrow(/Enter/);
    expect(() => parseTimestamp("yesterday-ish", now)).toThrow(/Not a date/);
    expect(() => parseTimestamp("99999999999999999999999", now)).toThrow(/out of range/);
  });

  it("describes relative times", () => {
    expect(relativeTime(now - 3 * 3600_000, now)).toBe("3 hours ago");
    expect(relativeTime(now + 2 * 86400_000, now)).toBe("in 2 days");
    expect(relativeTime(now - 86400_000, now)).toBe("yesterday");
    expect(relativeTime(now + 400, now)).toBe("now");
    expect(parseTimestamp("1700000000", now).relative).toBe("now");
  });
});

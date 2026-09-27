// Pure helpers for the Encoders tool: Base64, URL, hex, JWT, hashes, timestamps.
// Text is always UTF-8. Decoders throw an Error with a message for the user.

const encoder = new TextEncoder();

export function utf8(text: string): Uint8Array {
  return encoder.encode(text);
}

/** Decode UTF-8, or null when the bytes are not valid UTF-8 text. */
export function decodeUtf8(bytes: Uint8Array): string | null {
  try {
    return new TextDecoder("utf-8", { fatal: true }).decode(bytes);
  } catch {
    return null;
  }
}

// ---- Base64 ------------------------------------------------------------------

export function bytesToBase64(bytes: Uint8Array, urlSafe = false): string {
  let binary = "";
  // Chunked: String.fromCharCode(...big array) overflows the call stack.
  for (let i = 0; i < bytes.length; i += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  }
  const b64 = btoa(binary);
  return urlSafe ? b64.replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "") : b64;
}

/** Accepts standard and URL-safe alphabets, with or without padding, ignoring whitespace. */
export function base64ToBytes(input: string): Uint8Array {
  const clean = input.replace(/\s+/g, "").replace(/-/g, "+").replace(/_/g, "/");
  if (!/^[A-Za-z0-9+/]*={0,2}$/.test(clean)) {
    const bad = clean.match(/[^A-Za-z0-9+/=]/)?.[0];
    throw new Error(bad ? `Not valid Base64: unexpected character '${bad}'` : "Not valid Base64: misplaced '=' padding");
  }
  const body = clean.replace(/=+$/, "");
  if (body.length % 4 === 1) throw new Error("Not valid Base64: the length is wrong (a character is missing or extra)");
  const padded = body + "=".repeat((4 - (body.length % 4)) % 4);
  let binary: string;
  try {
    binary = atob(padded);
  } catch {
    throw new Error("Not valid Base64");
  }
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return bytes;
}

export function base64Encode(text: string, urlSafe = false): string {
  return bytesToBase64(utf8(text), urlSafe);
}

/** Decoded text, or the bytes as hex when they are not UTF-8 text. */
export function base64Decode(input: string): { text: string | null; hex: string; size: number } {
  const bytes = base64ToBytes(input);
  return { text: decodeUtf8(bytes), hex: bytesToHex(bytes), size: bytes.length };
}

// ---- URL ---------------------------------------------------------------------

/** Percent-encode for use inside a URL component (query value, path segment). */
export function urlEncode(text: string): string {
  return encodeURIComponent(text);
}

/** Percent-decode; `+` is a space as in form data unless `plusIsSpace` is false. */
export function urlDecode(text: string, plusIsSpace = true): string {
  const input = plusIsSpace ? text.replace(/\+/g, " ") : text;
  try {
    return decodeURIComponent(input);
  } catch {
    const bad = input.match(/%(?![0-9a-fA-F]{2})/);
    throw new Error(
      bad
        ? `Not valid URL encoding: '%' at position ${(bad.index ?? 0) + 1} is not followed by two hex digits`
        : "Not valid URL encoding: the escapes are not UTF-8 text",
    );
  }
}

// ---- Hex ---------------------------------------------------------------------

export function bytesToHex(bytes: Uint8Array, separator = " "): string {
  return Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join(separator);
}

export function textToHex(text: string, separator = " "): string {
  return bytesToHex(utf8(text), separator);
}

/** Accepts `48656c6c6f`, `48 65 6c`, `48:65:6c`, `0x48 0x65` and `\x48\x65`. */
export function hexToBytes(input: string): Uint8Array {
  const clean = input
    .trim()
    .replace(/(^|[^0-9a-f])0x/gi, "$1")
    .replace(/\\x/gi, "")
    .replace(/[\s:,-]+/g, "");
  if (!/^[0-9a-fA-F]*$/.test(clean)) {
    const bad = clean.match(/[^0-9a-fA-F]/)?.[0];
    throw new Error(`Not valid hex: unexpected character '${bad}'`);
  }
  if (clean.length % 2 !== 0) throw new Error("Not valid hex: odd number of digits");
  const bytes = new Uint8Array(clean.length / 2);
  for (let i = 0; i < bytes.length; i++) bytes[i] = parseInt(clean.slice(i * 2, i * 2 + 2), 16);
  return bytes;
}

export function hexToText(input: string): { text: string | null; size: number } {
  const bytes = hexToBytes(input);
  return { text: decodeUtf8(bytes), size: bytes.length };
}

// ---- JWT ---------------------------------------------------------------------

export interface JwtClaimTime {
  claim: "exp" | "iat" | "nbf";
  label: string;
  /** Seconds since the epoch, as in the token. */
  value: number;
  date: Date;
}

export interface DecodedJwt {
  header: unknown;
  payload: unknown;
  headerJson: string;
  payloadJson: string;
  /** The signature part as sent (base64url); it is not verified. */
  signature: string;
  algorithm: string | null;
  times: JwtClaimTime[];
  /** `exp` is in the past. */
  expired: boolean;
  /** `nbf` is in the future. */
  notYetValid: boolean;
}

const TIME_CLAIMS: { claim: JwtClaimTime["claim"]; label: string }[] = [
  { claim: "iat", label: "Issued at" },
  { claim: "nbf", label: "Not before" },
  { claim: "exp", label: "Expires" },
];

function jwtPart(part: string, name: string): { value: unknown; json: string } {
  let bytes: Uint8Array;
  try {
    bytes = base64ToBytes(part);
  } catch {
    throw new Error(`The ${name} is not valid Base64URL`);
  }
  const text = decodeUtf8(bytes);
  if (text === null) throw new Error(`The ${name} is not UTF-8 text`);
  try {
    const value: unknown = JSON.parse(text);
    return { value, json: JSON.stringify(value, null, 2) };
  } catch {
    throw new Error(`The ${name} is not JSON`);
  }
}

/** Decode (not verify) a JWT. Accepts a leading `Bearer `. */
export function decodeJwt(token: string, now = Date.now()): DecodedJwt {
  const raw = token.trim().replace(/^bearer\s+/i, "");
  if (!raw) throw new Error("Paste a token (three parts separated by dots)");
  const parts = raw.split(".");
  if (parts.length === 5) throw new Error("This is an encrypted token (JWE); its contents can't be read without the key");
  if (parts.length !== 3) throw new Error(`A JWT has three parts separated by dots; this has ${parts.length}`);
  const header = jwtPart(parts[0], "header");
  const payload = jwtPart(parts[1], "payload");
  const claims = payload.value && typeof payload.value === "object" && !Array.isArray(payload.value) ? (payload.value as Record<string, unknown>) : {};
  const times: JwtClaimTime[] = [];
  for (const { claim, label } of TIME_CLAIMS) {
    const v = claims[claim];
    if (typeof v === "number" && Number.isFinite(v)) {
      const date = new Date(v * 1000);
      if (!Number.isNaN(date.getTime())) times.push({ claim, label, value: v, date });
    }
  }
  const exp = times.find((t) => t.claim === "exp");
  const nbf = times.find((t) => t.claim === "nbf");
  const alg = (header.value as Record<string, unknown> | null)?.alg;
  return {
    header: header.value,
    payload: payload.value,
    headerJson: header.json,
    payloadJson: payload.json,
    signature: parts[2],
    algorithm: typeof alg === "string" ? alg : null,
    times,
    expired: !!exp && exp.date.getTime() < now,
    notYetValid: !!nbf && nbf.date.getTime() > now,
  };
}

// ---- Hashes ------------------------------------------------------------------

export type HashAlgorithm = "SHA-1" | "SHA-256" | "SHA-512";
export const HASH_ALGORITHMS: HashAlgorithm[] = ["SHA-1", "SHA-256", "SHA-512"];

/** Hex digest of the UTF-8 text. */
export async function digestHex(algorithm: HashAlgorithm, text: string): Promise<string> {
  const data = utf8(text);
  const hash = await crypto.subtle.digest(algorithm, data.buffer.slice(data.byteOffset, data.byteOffset + data.byteLength) as ArrayBuffer);
  return bytesToHex(new Uint8Array(hash), "");
}

// ---- Timestamps --------------------------------------------------------------

export type TimestampUnit = "s" | "ms" | "µs" | "ns";

export interface TimestampInfo {
  date: Date;
  /** How the input was read: a number in this unit, or a date string. */
  input: TimestampUnit | "date";
  unixSeconds: number;
  unixMs: number;
  iso: string;
  local: string;
  relative: string;
}

/** Latest representable date is ±8.64e15 ms from the epoch. */
const MAX_MS = 8.64e15;

/**
 * Read a Unix timestamp (seconds, ms, µs or ns — guessed from the size, as
 * tools like jwt.io and epochconverter do) or a date string (ISO 8601, RFC 2822).
 */
export function parseTimestamp(input: string, now = Date.now()): TimestampInfo {
  const text = input.trim();
  if (!text) throw new Error("Enter a Unix timestamp or a date");
  let ms: number;
  let unit: TimestampInfo["input"];
  if (/^-?\d+(\.\d+)?$/.test(text)) {
    const n = Number(text);
    const abs = Math.abs(n);
    if (abs < 1e11) [ms, unit] = [n * 1000, "s"];
    else if (abs < 1e14) [ms, unit] = [n, "ms"];
    else if (abs < 1e17) [ms, unit] = [n / 1000, "µs"];
    else [ms, unit] = [n / 1e6, "ns"];
  } else {
    ms = Date.parse(text);
    unit = "date";
    if (Number.isNaN(ms)) throw new Error("Not a date or timestamp (try 1700000000 or 2024-01-31T12:00:00Z)");
  }
  if (!Number.isFinite(ms) || Math.abs(ms) > MAX_MS) throw new Error("That date is out of range");
  const date = new Date(ms);
  return {
    date,
    input: unit,
    unixSeconds: Math.floor(ms / 1000),
    unixMs: Math.round(ms),
    iso: date.toISOString(),
    local: date.toLocaleString(undefined, { dateStyle: "full", timeStyle: "long" }),
    relative: relativeTime(ms, now),
  };
}

const UNITS: [Intl.RelativeTimeFormatUnit, number][] = [
  ["year", 365 * 24 * 3600 * 1000],
  ["month", 30 * 24 * 3600 * 1000],
  ["week", 7 * 24 * 3600 * 1000],
  ["day", 24 * 3600 * 1000],
  ["hour", 3600 * 1000],
  ["minute", 60 * 1000],
  ["second", 1000],
];

/** "in 3 days", "5 minutes ago", "now". */
export function relativeTime(ms: number, now = Date.now()): string {
  const diff = ms - now;
  const fmt = new Intl.RelativeTimeFormat("en", { numeric: "auto" });
  for (const [unit, size] of UNITS) {
    if (Math.abs(diff) >= size) return fmt.format(Math.trunc(diff / size), unit);
  }
  return "now";
}

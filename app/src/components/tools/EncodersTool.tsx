// Encoders: Base64, URL, hex, JWT decoding, hashes and Unix timestamps. Runs
// entirely in the UI; inputs are kept per mode in the tab.
import { useEffect, useMemo, useState } from "react";
import { ArrowLeftRight, Clock, ShieldAlert, X } from "lucide-react";
import {
  base64Decode,
  base64Encode,
  type DecodedJwt,
  decodeJwt,
  digestHex,
  HASH_ALGORITHMS,
  hexToText,
  parseTimestamp,
  relativeTime,
  textToHex,
  type TimestampInfo,
  urlDecode,
  urlEncode,
} from "../../lib/encoders";
import { type ToolTab, updateToolState } from "../../store/tabs";
import { toolState } from "../../store/tools";
import { Badge, Button, Checkbox, cx, IconButton, Input, Segmented } from "../ui";
import { CopyButton, KV, Section, textareaClass } from "./parts";

type Mode = "base64" | "url" | "hex" | "jwt" | "hash" | "time";
type Codec = "base64" | "url" | "hex";

interface EncodersState {
  mode: Mode;
  inputs: Record<Mode, string>;
  /** Decode (true) or encode (false), per codec. */
  decode: Record<Codec, boolean>;
  urlSafe: boolean;
}

const DEFAULTS: EncodersState = {
  mode: "base64",
  inputs: { base64: "", url: "", hex: "", jwt: "", hash: "", time: "" },
  decode: { base64: false, url: false, hex: false },
  urlSafe: false,
};

const MODES: { id: Mode; label: string }[] = [
  { id: "base64", label: "Base64" },
  { id: "url", label: "URL" },
  { id: "hex", label: "Hex" },
  { id: "jwt", label: "JWT" },
  { id: "hash", label: "Hash" },
  { id: "time", label: "Timestamp" },
];

export function EncodersTool({ tab }: { tab: ToolTab }) {
  const s = toolState(tab, DEFAULTS);
  const set = (fn: (s: EncodersState) => Partial<EncodersState>) =>
    updateToolState(tab.id, (st) => {
      const cur = { ...DEFAULTS, ...(st as Partial<EncodersState>) };
      return { ...cur, ...fn(cur) };
    });
  const input = s.inputs[s.mode] ?? "";
  const setInput = (value: string, mode: Mode = s.mode) => set((c) => ({ inputs: { ...DEFAULTS.inputs, ...c.inputs, [mode]: value } }));

  return (
    <div className="flex min-h-full flex-col pb-4" data-testid="encoders-tool">
      <div className="px-4 pb-3">
        <Segmented items={MODES} value={s.mode} onChange={(mode) => set(() => ({ mode }))} />
      </div>
      {s.mode === "base64" || s.mode === "url" || s.mode === "hex" ? (
        <CodecPanel
          codec={s.mode}
          input={input}
          decode={s.decode[s.mode] ?? false}
          urlSafe={s.urlSafe}
          onInput={setInput}
          onDecode={(decode) => set((c) => ({ decode: { ...DEFAULTS.decode, ...c.decode, [s.mode]: decode } }))}
          onUrlSafe={(urlSafe) => set(() => ({ urlSafe }))}
          onSwap={(output) =>
            set((c) => ({
              inputs: { ...DEFAULTS.inputs, ...c.inputs, [s.mode]: output },
              decode: { ...DEFAULTS.decode, ...c.decode, [s.mode]: !c.decode[s.mode as Codec] },
            }))
          }
        />
      ) : s.mode === "jwt" ? (
        <JwtPanel input={input} onInput={setInput} />
      ) : s.mode === "hash" ? (
        <HashPanel input={input} onInput={setInput} />
      ) : (
        <TimePanel input={input} onInput={setInput} />
      )}
    </div>
  );
}

// ---- Base64 / URL / hex ------------------------------------------------------------

type Output = { text: string; note?: string } | { error: string };

function convert(codec: Codec, decode: boolean, input: string, urlSafe: boolean): Output {
  try {
    if (codec === "base64") {
      if (!decode) return { text: base64Encode(input, urlSafe) };
      const r = base64Decode(input);
      return r.text !== null ? { text: r.text } : { text: r.hex, note: `Not UTF-8 text: showing the ${r.size} bytes as hex` };
    }
    if (codec === "url") return { text: decode ? urlDecode(input) : urlEncode(input) };
    if (!decode) return { text: textToHex(input) };
    const r = hexToText(input);
    return r.text !== null ? { text: r.text } : { error: `The ${r.size} bytes are not UTF-8 text` };
  } catch (e) {
    return { error: (e as Error).message };
  }
}

const CODEC_HINTS: Record<Codec, [string, string]> = {
  base64: ["Text to encode", "Base64 to decode (standard or URL-safe, padding optional)"],
  url: ["Text to percent-encode (for a query value or path segment)", "Percent-encoded text (+ is read as a space)"],
  hex: ["Text to convert to hex (UTF-8 bytes)", "Hex bytes, e.g. 48 65 6c 6c 6f, 0x48 0x65 or 48:65"],
};

function CodecPanel({
  codec,
  input,
  decode,
  urlSafe,
  onInput,
  onDecode,
  onUrlSafe,
  onSwap,
}: {
  codec: Codec;
  input: string;
  decode: boolean;
  urlSafe: boolean;
  onInput: (v: string) => void;
  onDecode: (v: boolean) => void;
  onUrlSafe: (v: boolean) => void;
  onSwap: (output: string) => void;
}) {
  const out = useMemo(() => convert(codec, decode, input, urlSafe), [codec, decode, input, urlSafe]);
  const text = "text" in out ? out.text : "";
  return (
    <div className="grid min-h-[320px] flex-1 grid-cols-1 gap-3 px-4 lg:grid-cols-[1fr_auto_1fr]">
      <div className="flex min-h-0 flex-col gap-1.5">
        <div className="flex h-7 items-center gap-2">
          <Segmented
            items={[
              { id: "encode", label: "Encode" },
              { id: "decode", label: "Decode" },
            ]}
            value={decode ? "decode" : "encode"}
            onChange={(v) => onDecode(v === "decode")}
          />
          {codec === "base64" && !decode && (
            <label className="flex items-center gap-1.5 text-[12px] text-muted">
              <Checkbox checked={urlSafe} onChange={onUrlSafe} label="URL-safe" />
              URL-safe
            </label>
          )}
          <span className="flex-1" />
          {input && (
            <IconButton label="Clear" onClick={() => onInput("")}>
              <X size={13} />
            </IconButton>
          )}
        </div>
        <textarea
          autoFocus
          spellCheck={false}
          aria-label="Input"
          className={cx(textareaClass, "min-h-[160px] flex-1")}
          placeholder={CODEC_HINTS[codec][decode ? 1 : 0]}
          value={input}
          onChange={(e) => onInput(e.target.value)}
        />
      </div>
      <div className="flex items-center justify-center">
        <IconButton label="Use the result as input and switch direction" onClick={() => onSwap(text)} disabled={!("text" in out)}>
          <ArrowLeftRight size={15} />
        </IconButton>
      </div>
      <div className="flex min-h-0 flex-col gap-1.5">
        <div className="flex h-7 items-center gap-2">
          <span className="text-[12px] font-medium text-muted">{decode ? "Decoded" : "Encoded"}</span>
          {"note" in out && out.note && <span className="truncate text-[11.5px] text-warning">{out.note}</span>}
          <span className="flex-1" />
          {text && <CopyButton text={text} label="Copy result" />}
        </div>
        {"error" in out ? (
          <div role="alert" className={cx(textareaClass, "min-h-[160px] flex-1 border-danger/40 font-sans text-danger")}>
            {out.error}
          </div>
        ) : (
          <textarea readOnly spellCheck={false} aria-label="Output" className={cx(textareaClass, "min-h-[160px] flex-1 bg-panel-2")} value={text} />
        )}
      </div>
    </div>
  );
}

// ---- JWT -------------------------------------------------------------------------------

function JwtPanel({ input, onInput }: { input: string; onInput: (v: string) => void }) {
  const result = useMemo((): { jwt: DecodedJwt; now: number } | { error: string } | null => {
    if (!input.trim()) return null;
    // The clock is read with each token, not once when the panel opened ("Expired" must be current).
    const now = Date.now();
    try {
      return { jwt: decodeJwt(input, now), now };
    } catch (e) {
      return { error: (e as Error).message };
    }
  }, [input]);
  return (
    <div className="flex flex-col gap-3">
      <div className="px-4">
        <textarea
          autoFocus
          spellCheck={false}
          aria-label="Token"
          rows={4}
          className={cx(textareaClass, "break-all")}
          placeholder="Paste a JWT (eyJhbGciOi…), with or without “Bearer ”"
          value={input}
          onChange={(e) => onInput(e.target.value)}
        />
      </div>
      {result && "error" in result && <p className="px-4 text-[12.5px] text-danger">{result.error}</p>}
      {result && "jwt" in result && (
        <>
          <div className="flex flex-wrap items-center gap-2 px-4">
            {result.jwt.algorithm && <Badge className="bg-hover text-fg">{result.jwt.algorithm}</Badge>}
            {result.jwt.expired && <Badge className="bg-danger/12 text-danger">Expired</Badge>}
            {result.jwt.notYetValid && <Badge className="bg-warning/14 text-warning">Not valid yet</Badge>}
            {!result.jwt.expired && !result.jwt.notYetValid && result.jwt.times.some((t) => t.claim === "exp") && (
              <Badge className="bg-success/12 text-success">Not expired</Badge>
            )}
            <span className="inline-flex items-center gap-1 text-[12px] text-muted">
              <ShieldAlert size={13} />
              Signature not verified: anyone can create a token that decodes like this.
            </span>
          </div>
          {result.jwt.times.length > 0 && (
            <Section title="Times">
              {result.jwt.times.map((t) => (
                <KV
                  key={t.claim}
                  label={`${t.label} (${t.claim})`}
                  mono={false}
                  value={
                    <span>
                      {t.date.toLocaleString(undefined, { dateStyle: "medium", timeStyle: "long" })}
                      <span className="text-faint"> · {relativeTime(t.date.getTime(), result.now)} · </span>
                      <span className="font-mono text-[12px] text-faint">{t.value}</span>
                    </span>
                  }
                />
              ))}
            </Section>
          )}
          <div className="grid grid-cols-1 gap-3 px-4 lg:grid-cols-2">
            <JsonBlock title="Header" json={result.jwt.headerJson} />
            <JsonBlock title="Payload" json={result.jwt.payloadJson} />
          </div>
        </>
      )}
    </div>
  );
}

function JsonBlock({ title, json }: { title: string; json: string }) {
  return (
    <div className="flex min-w-0 flex-col gap-1.5">
      <div className="flex h-6 items-center">
        <span className="flex-1 text-[11px] font-semibold uppercase tracking-wide text-faint">{title}</span>
        <CopyButton text={json} label={`Copy ${title.toLowerCase()}`} />
      </div>
      <pre className="selectable max-h-[420px] overflow-auto rounded-lg bg-panel-2 px-3 py-2 font-mono text-[12px] leading-relaxed text-fg">{json}</pre>
    </div>
  );
}

// ---- Hashes ----------------------------------------------------------------------------

function HashPanel({ input, onInput }: { input: string; onInput: (v: string) => void }) {
  const [hashes, setHashes] = useState<Record<string, string>>({});
  useEffect(() => {
    let cancelled = false;
    // Debounced so typing in a large text stays smooth.
    const t = setTimeout(() => {
      void Promise.all(HASH_ALGORITHMS.map(async (a) => [a, await digestHex(a, input)] as const))
        .then((list) => !cancelled && setHashes(Object.fromEntries(list)))
        .catch(() => !cancelled && setHashes({}));
    }, 120);
    return () => {
      cancelled = true;
      clearTimeout(t);
    };
  }, [input]);
  return (
    <div className="flex flex-col gap-3">
      <div className="px-4">
        <textarea
          autoFocus
          spellCheck={false}
          aria-label="Text to hash"
          rows={5}
          className={textareaClass}
          placeholder="Text to hash (as UTF-8)"
          value={input}
          onChange={(e) => onInput(e.target.value)}
        />
      </div>
      <div>
        {HASH_ALGORITHMS.map((a) => (
          <KV key={a} label={a} value={hashes[a] ?? "…"} copy={hashes[a]} />
        ))}
      </div>
      <p className="px-4 text-[11.5px] text-faint">SHA-1 is for checksums and legacy systems only; don't rely on it for security.</p>
    </div>
  );
}

// ---- Timestamps --------------------------------------------------------------------------

const UNIT_LABEL = { s: "seconds", ms: "milliseconds", "µs": "microseconds", ns: "nanoseconds", date: "a date" };

function TimePanel({ input, onInput }: { input: string; onInput: (v: string) => void }) {
  const [now, setNow] = useState(() => Date.now());
  const result = useMemo((): { info: TimestampInfo } | { error: string } | null => {
    if (!input.trim()) return null;
    try {
      return { info: parseTimestamp(input, now) };
    } catch (e) {
      return { error: (e as Error).message };
    }
  }, [input, now]);
  return (
    <div className="flex flex-col gap-3">
      <div className="flex items-center gap-2 px-4">
        <Input
          autoFocus
          className="max-w-[420px] font-mono"
          aria-label="Timestamp or date"
          placeholder="1700000000, 1700000000000 or 2024-01-31T12:00:00Z"
          value={input}
          onChange={(e) => {
            setNow(Date.now());
            onInput(e.target.value);
          }}
        />
        <Button
          icon={<Clock size={13} />}
          onClick={() => {
            const t = Date.now();
            setNow(t);
            onInput(String(Math.floor(t / 1000)));
          }}
        >
          Now
        </Button>
      </div>
      {result && "error" in result && <p className="px-4 text-[12.5px] text-danger">{result.error}</p>}
      {result && "info" in result && (
        <div>
          <p className="px-4 pb-1 text-[12px] text-muted">Read as {UNIT_LABEL[result.info.input]}.</p>
          <KV label="Unix seconds" value={String(result.info.unixSeconds)} copy={String(result.info.unixSeconds)} />
          <KV label="Unix milliseconds" value={String(result.info.unixMs)} copy={String(result.info.unixMs)} />
          <KV label="ISO 8601 (UTC)" value={result.info.iso} copy={result.info.iso} />
          <KV label="Local time" value={result.info.local} mono={false} copy={result.info.local} />
          <KV label="Relative" value={result.info.relative} mono={false} />
        </div>
      )}
    </div>
  );
}

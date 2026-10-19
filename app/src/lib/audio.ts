// Pure audio helpers for streaming the microphone: the formats on offer, sample
// encoders (PCM, G.711 μ-law and A-law), a streaming resampler and a WAV header.

export type AudioFormat = "pcm16" | "pcm32f" | "mulaw" | "alaw" | "webm-opus" | "ogg-opus" | "mp4-aac";

export interface AudioFormatInfo {
  id: AudioFormat;
  label: string;
  /** Raw samples, encoded here. */
  bytesPerSample?: number;
  /** WAV format code of raw samples. */
  wavCode?: number;
  /** Compressed by the system (MediaRecorder) in this container. */
  mime?: string;
  /** Sample rate picked when switching to this format. */
  defaultRate?: number;
}

export const AUDIO_FORMATS: AudioFormatInfo[] = [
  { id: "pcm16", label: "PCM 16-bit (s16le)", bytesPerSample: 2, wavCode: 1, defaultRate: 16000 },
  { id: "pcm32f", label: "PCM 32-bit float (f32le)", bytesPerSample: 4, wavCode: 3, defaultRate: 48000 },
  { id: "mulaw", label: "G.711 μ-law", bytesPerSample: 1, wavCode: 7, defaultRate: 8000 },
  { id: "alaw", label: "G.711 A-law", bytesPerSample: 1, wavCode: 6, defaultRate: 8000 },
  { id: "webm-opus", label: "Opus (WebM)", mime: "audio/webm;codecs=opus" },
  { id: "ogg-opus", label: "Opus (Ogg)", mime: "audio/ogg;codecs=opus" },
  { id: "mp4-aac", label: "AAC (MP4)", mime: "audio/mp4" },
];

export const SAMPLE_RATES = [8000, 16000, 22050, 24000, 32000, 44100, 48000];
export const CHUNK_MS = [10, 20, 40, 60, 100, 250, 500, 1000];
export const BITRATES = [16000, 24000, 32000, 48000, 64000, 96000, 128000];

export function formatInfo(id: AudioFormat): AudioFormatInfo {
  return AUDIO_FORMATS.find((f) => f.id === id) ?? AUDIO_FORMATS[0];
}

/** Bytes in one chunk of raw audio. */
export function rawChunkBytes(format: AudioFormat, sampleRate: number, channels: number, chunkMs: number): number {
  return Math.max(1, Math.round((sampleRate * chunkMs) / 1000)) * channels * (formatInfo(format).bytesPerSample ?? 1);
}

function toInt16(s: number): number {
  const c = Math.max(-1, Math.min(1, s));
  return Math.round(c < 0 ? c * 0x8000 : c * 0x7fff);
}

// G.711 after the reference implementation (Sun Microsystems, g711.c).
const ULAW_SEG_END = [0x3f, 0x7f, 0xff, 0x1ff, 0x3ff, 0x7ff, 0xfff, 0x1fff];
const ALAW_SEG_END = [0x1f, 0x3f, 0x7f, 0xff, 0x1ff, 0x3ff, 0x7ff, 0xfff];

function segment(value: number, ends: number[]): number {
  let seg = 0;
  while (seg < ends.length && value > ends[seg]) seg++;
  return seg;
}

/** A 16-bit linear sample as G.711 μ-law. */
export function linearToMulaw(sample: number): number {
  let pcm = sample >> 2;
  let mask = 0xff;
  if (pcm < 0) {
    pcm = -pcm;
    mask = 0x7f;
  }
  pcm = Math.min(pcm, 8159) + (0x84 >> 2);
  const seg = segment(pcm, ULAW_SEG_END);
  if (seg >= 8) return 0x7f ^ mask;
  return (((seg << 4) | ((pcm >> (seg + 1)) & 0x0f)) ^ mask) & 0xff;
}

/** A 16-bit linear sample as G.711 A-law. */
export function linearToAlaw(sample: number): number {
  let pcm = sample >> 3;
  let mask = 0xd5;
  if (pcm < 0) {
    pcm = -pcm - 1;
    mask = 0x55;
  }
  const seg = segment(pcm, ALAW_SEG_END);
  if (seg >= 8) return 0x7f ^ mask;
  const quant = seg < 2 ? (pcm >> 1) & 0x0f : (pcm >> seg) & 0x0f;
  return (((seg << 4) | quant) ^ mask) & 0xff;
}

/** Interleaved float samples (-1…1) in a raw format, little-endian. */
export function encodeSamples(format: AudioFormat, samples: Float32Array): Uint8Array {
  const bytes = formatInfo(format).bytesPerSample ?? 2;
  const out = new Uint8Array(samples.length * bytes);
  const view = new DataView(out.buffer);
  for (let i = 0; i < samples.length; i++) {
    const s = samples[i];
    if (format === "pcm32f") view.setFloat32(i * 4, s, true);
    else if (format === "mulaw") out[i] = linearToMulaw(toInt16(s));
    else if (format === "alaw") out[i] = linearToAlaw(toInt16(s));
    else view.setInt16(i * 2, toInt16(s), true);
  }
  return out;
}

/** Channels of equal length, as one interleaved array. */
export function interleave(channels: Float32Array[]): Float32Array {
  if (channels.length === 1) return channels[0];
  const frames = channels[0].length;
  const out = new Float32Array(frames * channels.length);
  for (let f = 0; f < frames; f++) for (let c = 0; c < channels.length; c++) out[f * channels.length + c] = channels[c][f];
  return out;
}

/**
 * Changes the sample rate of a stream that arrives in blocks: linear interpolation, with a
 * two-pole low-pass in front when lowering the rate (so high tones don't fold into speech).
 */
export class Resampler {
  private readonly step: number;
  private readonly alpha: number;
  private pos = 1;
  private readonly prev: number[];
  private readonly lp1: number[];
  private readonly lp2: number[];

  constructor(
    readonly from: number,
    readonly to: number,
    channels: number,
  ) {
    this.step = from / to;
    this.alpha = to < from ? 1 - Math.exp((-2 * Math.PI * 0.45 * to) / from) : 1;
    this.prev = new Array(channels).fill(0);
    this.lp1 = new Array(channels).fill(0);
    this.lp2 = new Array(channels).fill(0);
  }

  /** One block of samples per channel in, the resampled block per channel out. */
  process(input: Float32Array[]): Float32Array[] {
    if (this.from === this.to) return input;
    const n = input[0]?.length ?? 0;
    const count = Math.max(0, Math.floor((n - this.pos) / this.step) + 1);
    const out = input.map((samples, c) => {
      // Sample i of the block is buf[i + 1]; buf[0] is the last sample of the block before.
      const buf = new Float32Array(n + 1);
      buf[0] = this.prev[c];
      let a = this.lp1[c];
      let b = this.lp2[c];
      for (let i = 0; i < n; i++) {
        a += this.alpha * (samples[i] - a);
        b += this.alpha * (a - b);
        buf[i + 1] = b;
      }
      this.lp1[c] = a;
      this.lp2[c] = b;
      this.prev[c] = buf[n];
      const result = new Float32Array(count);
      let t = this.pos;
      for (let k = 0; k < count; k++, t += this.step) {
        const i = Math.floor(t);
        const frac = t - i;
        result[k] = i + 1 <= n ? buf[i] + (buf[i + 1] - buf[i]) * frac : buf[i];
      }
      return result;
    });
    this.pos = this.pos + count * this.step - n;
    return out;
  }
}

/** A WAV header for a stream of raw samples of unknown length (sizes 0xFFFFFFFF). */
export function wavHeader(format: AudioFormat, sampleRate: number, channels: number): Uint8Array {
  const info = formatInfo(format);
  const bytes = info.bytesPerSample ?? 2;
  const out = new Uint8Array(44);
  const view = new DataView(out.buffer);
  const ascii = (at: number, text: string) => [...text].forEach((ch, i) => (out[at + i] = ch.charCodeAt(0)));
  ascii(0, "RIFF");
  view.setUint32(4, 0xffffffff, true);
  ascii(8, "WAVEfmt ");
  view.setUint32(16, 16, true);
  view.setUint16(20, info.wavCode ?? 1, true);
  view.setUint16(22, channels, true);
  view.setUint32(24, sampleRate, true);
  view.setUint32(28, sampleRate * channels * bytes, true);
  view.setUint16(32, channels * bytes, true);
  view.setUint16(34, bytes * 8, true);
  ascii(36, "data");
  view.setUint32(40, 0xffffffff, true);
  return out;
}

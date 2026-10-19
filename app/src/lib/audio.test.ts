import { describe, expect, it } from "vitest";
import { encodeSamples, interleave, linearToAlaw, linearToMulaw, rawChunkBytes, Resampler, wavHeader } from "./audio";

describe("G.711", () => {
  it("encodes silence and the loudest samples like the reference tables", () => {
    expect(linearToMulaw(0)).toBe(0xff);
    expect(linearToMulaw(32767)).toBe(0x80);
    expect(linearToMulaw(-32768)).toBe(0x00);
    expect(linearToAlaw(0)).toBe(0xd5);
    expect(linearToAlaw(-1)).toBe(0x55);
    expect(linearToAlaw(32767)).toBe(0xaa);
    expect(linearToAlaw(-32768)).toBe(0x2a);
  });

  it("keeps louder samples in higher segments", () => {
    const codes = [100, 1000, 10000].map((s) => (~linearToMulaw(s) & 0x70) >> 4);
    expect(codes[0]).toBeLessThan(codes[1]);
    expect(codes[1]).toBeLessThan(codes[2]);
  });
});

describe("encodeSamples", () => {
  it("writes little-endian PCM and floats, clamped to the 16-bit range", () => {
    expect([...encodeSamples("pcm16", new Float32Array([0, 1, -1, 2]))]).toEqual([0, 0, 0xff, 0x7f, 0x00, 0x80, 0xff, 0x7f]);
    const f = encodeSamples("pcm32f", new Float32Array([0.5]));
    expect(new DataView(f.buffer).getFloat32(0, true)).toBe(0.5);
    expect([...encodeSamples("mulaw", new Float32Array([0, 0]))]).toEqual([0xff, 0xff]);
    expect([...encodeSamples("alaw", new Float32Array([0]))]).toEqual([0xd5]);
  });

  it("interleaves channels and sizes chunks by duration", () => {
    expect([...interleave([new Float32Array([1, 2]), new Float32Array([3, 4])])]).toEqual([1, 3, 2, 4]);
    expect(rawChunkBytes("pcm16", 16000, 1, 20)).toBe(640);
    expect(rawChunkBytes("mulaw", 8000, 1, 20)).toBe(160);
    expect(rawChunkBytes("pcm32f", 48000, 2, 10)).toBe(3840);
  });
});

describe("Resampler", () => {
  it("gives the expected number of samples across blocks", () => {
    const r = new Resampler(48000, 16000, 1);
    let total = 0;
    for (let i = 0; i < 100; i++) total += r.process([new Float32Array(128)])[0].length;
    expect(Math.abs(total - (100 * 128) / 3)).toBeLessThanOrEqual(1);
    const up = new Resampler(8000, 48000, 2);
    const out = up.process([new Float32Array(80), new Float32Array(80)]);
    expect(out).toHaveLength(2);
    expect(Math.abs(out[0].length - 480)).toBeLessThanOrEqual(6);
  });

  it("passes a steady level through and keeps a low tone", () => {
    const r = new Resampler(48000, 16000, 1);
    let last: Float32Array = new Float32Array();
    for (let i = 0; i < 20; i++) last = r.process([new Float32Array(480).fill(0.5)])[0];
    expect(last.every((s) => Math.abs(s - 0.5) < 1e-3)).toBe(true);
    expect(new Resampler(16000, 16000, 1).process([new Float32Array([0.25])])[0][0]).toBe(0.25);
  });
});

describe("wavHeader", () => {
  it("describes a stream of unknown length", () => {
    const h = wavHeader("mulaw", 8000, 1);
    const view = new DataView(h.buffer);
    expect(String.fromCharCode(...h.subarray(0, 4), ...h.subarray(8, 16), ...h.subarray(36, 40))).toBe("RIFFWAVEfmt data");
    expect([view.getUint16(20, true), view.getUint16(22, true), view.getUint32(24, true), view.getUint32(28, true)]).toEqual([7, 1, 8000, 8000]);
    expect([view.getUint16(32, true), view.getUint16(34, true), view.getUint32(40, true)]).toEqual([1, 8, 0xffffffff]);
    expect(new DataView(wavHeader("pcm16", 16000, 2).buffer).getUint32(28, true)).toBe(64000);
  });
});

// Opening the microphone: a picked device that's gone gives way to the default one, and a
// failure after access was granted lets go of the microphone and the audio context.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MicOptions } from "./mic";

const { startMic } = await import("./mic");

const options: MicOptions = {
  deviceId: "usb-mic",
  format: "pcm16",
  sampleRate: 16000,
  channels: 1,
  chunkMs: 20,
  bitrate: 32000,
  wavHeader: false,
  echoCancellation: true,
  noiseSuppression: true,
  autoGainControl: true,
};

const handlers = { onChunk: vi.fn(), onLevel: vi.fn(), onEnded: vi.fn() };
const track = { stop: vi.fn(), addEventListener: vi.fn() };
const getUserMedia = vi.fn();
const contexts: { close: ReturnType<typeof vi.fn> }[] = [];
let workletFails = false;

const node = () => ({ connect: (next: unknown) => next });

beforeEach(() => {
  contexts.length = 0;
  workletFails = false;
  track.stop.mockClear();
  getUserMedia.mockReset();
  getUserMedia.mockResolvedValue({ getAudioTracks: () => [track] });
  vi.stubGlobal("navigator", { mediaDevices: { getUserMedia } });
  vi.stubGlobal(
    "AudioContext",
    class {
      sampleRate = 48000;
      destination = {};
      audioWorklet = { addModule: vi.fn(() => Promise.resolve()) };
      close = vi.fn(() => Promise.resolve());
      constructor() {
        contexts.push(this);
      }
      resume = vi.fn(() => Promise.resolve());
      createMediaStreamSource = node;
      createGain = () => ({ ...node(), gain: { value: 1 } });
    },
  );
  vi.stubGlobal(
    "AudioWorkletNode",
    class {
      port = { onmessage: null };
      connect = (next: unknown) => next;
      constructor() {
        if (workletFails) throw new Error("No such processor");
      }
    },
  );
});
afterEach(() => vi.unstubAllGlobals());

describe("startMic", () => {
  it("uses the default microphone when the picked one is gone", async () => {
    getUserMedia.mockRejectedValueOnce(Object.assign(new Error("no device"), { name: "OverconstrainedError" }));
    const session = await startMic(options, handlers);
    expect(getUserMedia).toHaveBeenCalledTimes(2);
    expect(getUserMedia.mock.calls[0][0].audio.deviceId).toEqual({ exact: "usb-mic" });
    expect(getUserMedia.mock.calls[1][0].audio.deviceId).toBeUndefined();
    await session.stop();
    expect(track.stop).toHaveBeenCalled();
  });

  it("doesn't retry when access is denied", async () => {
    getUserMedia.mockRejectedValueOnce(Object.assign(new Error("denied"), { name: "NotAllowedError" }));
    await expect(startMic(options, handlers)).rejects.toThrow("Microphone access was denied");
    expect(getUserMedia).toHaveBeenCalledTimes(1);
    expect(contexts[0].close).toHaveBeenCalled();
  });

  it("lets go of the microphone when capture can't start", async () => {
    workletFails = true;
    await expect(startMic({ ...options, deviceId: "" }, handlers)).rejects.toThrow("Could not start audio capture: No such processor");
    expect(track.stop).toHaveBeenCalled();
    expect(contexts[0].close).toHaveBeenCalled();
  });
});

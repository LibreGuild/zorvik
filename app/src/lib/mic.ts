// Microphone capture for streaming over a connection. Raw formats (PCM, G.711) are
// resampled and encoded here from an AudioWorklet's samples (public/mic-worklet.js);
// Opus and AAC come from the system's MediaRecorder. Chunks are handed out in order.
import { type AudioFormat, encodeSamples, formatInfo, interleave, rawChunkBytes, Resampler, wavHeader } from "./audio";

export interface MicOptions {
  /** "" = the system's default microphone. */
  deviceId: string;
  format: AudioFormat;
  /** Raw formats. */
  sampleRate: number;
  channels: 1 | 2;
  chunkMs: number;
  /** Compressed formats, in bits per second. */
  bitrate: number;
  /** Raw formats: send a WAV header before the samples. */
  wavHeader: boolean;
  echoCancellation: boolean;
  noiseSuppression: boolean;
  autoGainControl: boolean;
}

export interface MicHandlers {
  onChunk: (bytes: Uint8Array) => void;
  /** Peak level (0…1), about 10 times a second. */
  onLevel: (peak: number) => void;
  /** The microphone stopped on its own (unplugged, access revoked). */
  onEnded: (message: string) => void;
}

export interface MicSession {
  /** Stop recording; the last partial chunk is sent first. */
  stop(): Promise<void>;
}

/** Compressed formats this system can record. */
export function supportedEncodedFormats(): AudioFormat[] {
  if (typeof MediaRecorder === "undefined") return [];
  return (["webm-opus", "ogg-opus", "mp4-aac"] as AudioFormat[]).filter((f) => MediaRecorder.isTypeSupported(formatInfo(f).mime ?? ""));
}

export function micAvailable(): boolean {
  return typeof navigator !== "undefined" && !!navigator.mediaDevices?.getUserMedia && typeof AudioWorkletNode !== "undefined";
}

/** Microphones (names only show once the user allowed access). */
export async function listMicrophones(): Promise<{ id: string; label: string }[]> {
  if (!navigator.mediaDevices?.enumerateDevices) return [];
  const devices = await navigator.mediaDevices.enumerateDevices();
  return devices
    .filter((d) => d.kind === "audioinput" && d.deviceId && d.deviceId !== "default")
    .map((d, i) => ({ id: d.deviceId, label: d.label || `Microphone ${i + 1}` }));
}

/** The microphone's audio; a microphone picked earlier that's gone (unplugged) gives way to the default one. */
async function openStream(o: MicOptions): Promise<MediaStream> {
  const audio = (deviceId: string): MediaTrackConstraints => ({
    deviceId: deviceId ? { exact: deviceId } : undefined,
    channelCount: { ideal: o.channels },
    echoCancellation: o.echoCancellation,
    noiseSuppression: o.noiseSuppression,
    autoGainControl: o.autoGainControl,
  });
  try {
    return await navigator.mediaDevices.getUserMedia({ audio: audio(o.deviceId) });
  } catch (e) {
    const name = errorName(e);
    if (!o.deviceId || (name !== "OverconstrainedError" && name !== "NotFoundError")) throw e;
    return navigator.mediaDevices.getUserMedia({ audio: audio("") });
  }
}

/** Call from a click: browsers only start audio in answer to the user. */
export async function startMic(o: MicOptions, handlers: MicHandlers): Promise<MicSession> {
  if (!micAvailable()) throw new Error("This window can't use a microphone.");
  // Created before the permission prompt, while the click still counts as the user's.
  const ctx = new AudioContext();
  let stream: MediaStream;
  try {
    stream = await openStream(o);
  } catch (e) {
    void ctx.close().catch(() => {});
    throw new Error(micError(e));
  }
  const tracks = stream.getAudioTracks();
  let stopped = false;
  const cleanup = () => {
    stopped = true;
    tracks.forEach((t) => t.stop());
    void ctx.close().catch(() => {});
  };
  // Anything failing from here on lets go of the microphone and the audio context.
  try {
    return await capture(o, handlers, ctx, stream, () => stopped, cleanup);
  } catch (e) {
    cleanup();
    throw new Error(`Could not start audio capture: ${e instanceof Error ? e.message : String(e)}`);
  }
}

/** Audio from the stream as chunks: raw formats through the worklet, compressed ones from a MediaRecorder. */
async function capture(o: MicOptions, handlers: MicHandlers, ctx: AudioContext, stream: MediaStream, stopped: () => boolean, cleanup: () => void): Promise<MicSession> {
  const info = formatInfo(o.format);
  const tracks = stream.getAudioTracks();
  await ctx.resume();
  await ctx.audioWorklet.addModule(new URL("mic-worklet.js", document.baseURI).href);
  const source = ctx.createMediaStreamSource(stream);
  // "explicit" makes Web Audio mix the input to the channels asked for (mono ↔ stereo).
  const node = new AudioWorkletNode(ctx, "zorvik-mic", { channelCount: o.channels, channelCountMode: "explicit", numberOfOutputs: 1 });
  const mute = ctx.createGain();
  mute.gain.value = 0;
  // Connected to the output (silently) so every system keeps pulling samples through it.
  source.connect(node).connect(mute).connect(ctx.destination);

  let peak = 0;
  let reported = performance.now();
  const level = (channels: Float32Array[]) => {
    for (const ch of channels) for (let i = 0; i < ch.length; i++) peak = Math.max(peak, Math.abs(ch[i]));
    const now = performance.now();
    if (now - reported >= 100) {
      handlers.onLevel(Math.min(1, peak));
      peak = 0;
      reported = now;
    }
  };
  for (const t of tracks) {
    t.addEventListener("ended", () => {
      if (stopped()) return;
      cleanup();
      handlers.onEnded("The microphone stopped (unplugged, or access was turned off).");
    });
  }

  if (info.mime) {
    const recorder = new MediaRecorder(stream, { mimeType: info.mime, audioBitsPerSecond: o.bitrate });
    // Blobs are read in order, so chunks go out in order.
    let reading = Promise.resolve();
    recorder.ondataavailable = (e) => {
      if (!e.data.size) return;
      const blob = e.data;
      reading = reading.then(async () => handlers.onChunk(new Uint8Array(await blob.arrayBuffer())));
    };
    node.port.onmessage = (e: MessageEvent<Float32Array[]>) => level(e.data);
    recorder.start(o.chunkMs);
    return {
      async stop() {
        if (recorder.state !== "inactive") {
          const done = new Promise((resolve) => recorder.addEventListener("stop", resolve, { once: true }));
          recorder.stop();
          await done;
        }
        await reading;
        cleanup();
      },
    };
  }

  const resampler = new Resampler(ctx.sampleRate, o.sampleRate, o.channels);
  const chunkBytes = rawChunkBytes(o.format, o.sampleRate, o.channels, o.chunkMs);
  let pending = new Uint8Array(0);
  if (o.wavHeader) handlers.onChunk(wavHeader(o.format, o.sampleRate, o.channels));
  node.port.onmessage = (e: MessageEvent<Float32Array[]>) => {
    if (stopped()) return;
    level(e.data);
    const encoded = encodeSamples(o.format, interleave(resampler.process(e.data)));
    const joined = new Uint8Array(pending.length + encoded.length);
    joined.set(pending);
    joined.set(encoded, pending.length);
    let at = 0;
    for (; joined.length - at >= chunkBytes; at += chunkBytes) handlers.onChunk(joined.slice(at, at + chunkBytes));
    pending = joined.slice(at);
  };
  return {
    async stop() {
      if (stopped()) return;
      node.port.onmessage = null;
      cleanup();
      if (pending.length) handlers.onChunk(pending);
    },
  };
}

/** An error's name ("NotAllowedError"); OverconstrainedError isn't a DOMException everywhere. */
function errorName(e: unknown): string {
  return typeof e === "object" && e !== null && "name" in e ? String(e.name) : "";
}

function micError(e: unknown): string {
  const name = errorName(e);
  if (name === "NotAllowedError" || name === "SecurityError")
    return "Microphone access was denied. Allow Zorvik to use the microphone in your system's privacy settings, then try again.";
  if (name === "NotFoundError" || name === "OverconstrainedError") return "No microphone found. Connect one, or pick another in the list.";
  if (name === "NotReadableError") return "The microphone is busy or not working (another app may be using it).";
  return `Could not open the microphone: ${e instanceof Error ? e.message : String(e)}`;
}

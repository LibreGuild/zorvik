import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Request } from "../bindings/Request";
import type { StreamEvent } from "../bindings/StreamEvent";
import type { WavInfo } from "../bindings/WavInfo";

const handlers: ((e: StreamEvent) => void)[] = [];
vi.mock("../lib/events", () => ({ onEvent: (h: (e: StreamEvent) => void) => handlers.push(h) }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("../lib/platform", async (importOriginal) => ({ ...(await importOriginal<object>()), pickFile: vi.fn(() => Promise.resolve("/files/voice.wav")) }));
let micHandlers: import("../lib/mic").MicHandlers | null = null;
vi.mock("../lib/mic", () => ({
  startMic: vi.fn(async (_o: unknown, h: import("../lib/mic").MicHandlers) => {
    micHandlers = h;
    h.onChunk(new Uint8Array([1]));
    // Stopping hands out the last partial chunk.
    return { stop: async () => h.onChunk(new Uint8Array([9])) };
  }),
}));
vi.mock("../lib/rpc", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/rpc")>();
  const done = () => vi.fn((..._args: unknown[]): Promise<unknown> => Promise.resolve(null));
  return {
    ...actual,
    api: {
      wsConnect: done(),
      wsSend: done(),
      wsClose: done(),
      socketConnect: done(),
      socketSend: done(),
      socketClose: done(),
      renderVariables: vi.fn((t: string) => Promise.resolve(t.replace("{{room}}", "lobby"))),
      fileStreamInfo: done(),
      fileStreamStart: vi.fn(() => Promise.resolve({ total: 3200 })),
      fileStreamStop: done(),
      wsRepeat: done(),
      wsStopRepeat: done(),
      saveRequest: done(),
      grpcSend: done(),
      grpcCancel: done(),
      grpcEnd: done(),
    },
  };
});

const { api } = await import("../lib/rpc");
const { audioPace, chooseStreamFile, composerOf, describeWav, filePlan, fileProblem, isRecording, patchFile, startFileStream, startRecording, stopRecording } =
  await import("./composer");
const { connect, openDraft, resetTabs, sendChunk, updateTab, useTabs } = await import("./tabs");
const { startMic } = await import("../lib/mic");
const { useDialogs } = await import("./dialogs");
const { activeRepeat, closeWithCode, loadIntoComposer, saveComposerAsMessage, startRepeat, stopRepeat } = await import("./composer");

/** Answer the dialog that is open (after it opens). */
async function answer(value: string | null) {
  await vi.waitFor(() => expect(useDialogs.getState().current).not.toBeNull());
  const d = useDialogs.getState().current!;
  useDialogs.setState({ current: null });
  if (d.kind === "prompt") d.resolve(value);
}
type Tab = import("./tabs").Tab;

const mocked = api as unknown as Record<keyof typeof api, ReturnType<typeof vi.fn>>;
const tabById = (id: string) => useTabs.getState().tabs.find((t) => t.id === id)! as Tab;

const wav: WavInfo = { encoding: "pcm", channels: 1, sampleRate: 16000, bitsPerSample: 16, blockAlign: 2, byteRate: 32000, dataOffset: 44, dataSize: 3200 };

async function connected(request: Request): Promise<string> {
  const id = openDraft(request);
  mocked.wsConnect.mockResolvedValueOnce({ meta: { status: 101, statusText: "Switching Protocols", headers: [] }, timing: { totalMs: 1 }, unresolved: [] });
  mocked.socketConnect.mockResolvedValueOnce({ opened: { protocol: "MQTT 3.1.1", remoteAddr: "h:1883", timing: { totalMs: 1 } } });
  await connect(id);
  return id;
}

beforeEach(() => {
  resetTabs();
  vi.clearAllMocks();
});

describe("pacing", () => {
  it("paces a WAV file as real-time audio in whole sample frames, header skipped", () => {
    expect(audioPace(wav)).toEqual({ chunkSize: 640, intervalMs: 20, offset: 44, length: 3200 });
    expect(audioPace({ ...wav, channels: 2, blockAlign: 6, byteRate: 6 * 44100, sampleRate: 44100, bitsPerSample: 24 }, 10).chunkSize % 6).toBe(0);
    expect(describeWav(wav)).toBe("WAV · PCM 16-bit · 16 kHz · mono");
  });

  it("says what the settings send", () => {
    const file = { ...composerOf("x").file, info: { name: "f", size: 1000, wav: null }, chunkSize: 300, intervalMs: 50, offset: 100 };
    expect(filePlan(file)).toEqual({ bytesPerSecond: 6000, messages: 3, passMs: 100 });
    expect(filePlan({ ...file, length: 10 }).messages).toBe(1);
  });
});

describe("file streams", () => {
  it("picks a WAV file paced as audio, then streams it as MQTT publishes to the topic", async () => {
    const id = await connected({ name: "m", seq: 1, method: "GET", url: "mqtt://h", kind: "mqtt", mqtt: { keepAliveSecs: 30, qos: 1, topic: "audio/{{room}}" } });
    mocked.fileStreamInfo.mockResolvedValueOnce({ name: "voice.wav", size: 3244, wav });
    await chooseStreamFile(id);
    expect(composerOf(id).file).toMatchObject({ path: "/files/voice.wav", chunkSize: 640, intervalMs: 20, offset: 44, length: 3200 });

    await startFileStream(id);
    const connId = tabById(id).stream.connId;
    expect(mocked.fileStreamStart).toHaveBeenCalledWith({
      connId,
      path: "/files/voice.wav",
      chunkSize: 640,
      intervalMs: 20,
      offset: 44,
      length: 3200,
      repeat: false,
      message: { type: "publish", topic: "audio/lobby", qos: 1, retain: false },
    });
    expect(composerOf(id).file.run?.status).toBe("running");

    const progress = { sent: 1280, messages: 2, pass: 1, position: 1280, total: 3200 };
    for (const h of handlers) h({ type: "fileStream", connId, event: { type: "progress", ...progress } });
    expect(composerOf(id).file.run?.progress).toEqual(progress);
    for (const h of handlers) h({ type: "fileStream", connId, event: { type: "ended", progress, stopped: true, error: "The connection closed" } });
    expect(composerOf(id).file.run).toMatchObject({ status: "error", error: "The connection closed" });
  });

  it("sends whole numbers of bytes and milliseconds", async () => {
    const id = await connected({ name: "w", seq: 1, method: "GET", url: "ws://h", kind: "websocket" });
    patchFile(id, { path: "/f", info: { name: "f", size: 1000, wav: null }, chunkSize: 10.6, intervalMs: 2.7, offset: 3.2, length: 99.9 });
    await startFileStream(id);
    expect(mocked.fileStreamStart).toHaveBeenCalledWith(expect.objectContaining({ chunkSize: 10, intervalMs: 2, offset: 3, length: 99 }));
  });

  it("won't start without what the connection needs", async () => {
    const id = await connected({ name: "u", seq: 1, method: "GET", url: "udp://h:9", kind: "udp" });
    patchFile(id, { path: "/f", info: { name: "f", size: 100_000, wav: null }, chunkSize: 70_000 });
    expect(fileProblem(tabById(id), composerOf(id).file)).toMatch(/UDP datagram/);
    await startFileStream(id);
    expect(mocked.fileStreamStart).not.toHaveBeenCalled();

    const tcp = await connected({ name: "t", seq: 1, method: "GET", url: "tcp://h:9", kind: "tcp", socket: { framing: "lengthPrefixed", lengthBytes: 1 } });
    patchFile(tcp, { path: "/f", info: { name: "f", size: 1000, wav: null }, chunkSize: 256 });
    expect(fileProblem(tabById(tcp), composerOf(tcp).file)).toMatch(/at most 255 bytes/);

    const io = await connected({ name: "s", seq: 1, method: "GET", url: "http://h", kind: "socketio", socketio: {} });
    patchFile(io, { path: "/f", info: { name: "f", size: 10, wav: null } });
    await startFileStream(io);
    expect(mocked.fileStreamStart).not.toHaveBeenCalled();
  });
});

describe("sendChunk", () => {
  it("sends a binary message, an emit or a publish by connection", async () => {
    const ws: Request = { name: "w", seq: 1, method: "GET", url: "ws://h", kind: "websocket" };
    await sendChunk("c", ws, { type: "binary" }, "AQI=");
    expect(mocked.wsSend).toHaveBeenLastCalledWith("c", { type: "binary", base64: "AQI=" });
    await sendChunk("c", { ...ws, kind: "socketio" }, { type: "emit", event: "audio", ack: false }, "AQI=");
    expect(mocked.socketSend).toHaveBeenLastCalledWith("c", { type: "emit", event: "audio", args: "", base64: "AQI=", ack: false });
    await sendChunk("c", { ...ws, kind: "tcp" }, { type: "binary" }, "AQI=");
    expect(mocked.socketSend).toHaveBeenLastCalledWith("c", { type: "binary", base64: "AQI=" });
  });
});

describe("recording", () => {
  it("doesn't open the microphone when stopped before it started", async () => {
    const id = await connected({ name: "w", seq: 1, method: "GET", url: "ws://h", kind: "websocket" });
    const starting = startRecording(id);
    const stopping = stopRecording(id);
    await Promise.all([starting, stopping]);
    expect(startMic).not.toHaveBeenCalled();
    expect(isRecording(id)).toBe(false);
    expect(composerOf(id).mic.run).toBeNull();
  });

  it("sends chunks in order, the last one on stop, and stops when sending fails", async () => {
    const id = await connected({ name: "w", seq: 1, method: "GET", url: "ws://h", kind: "websocket" });
    const connId = tabById(id).stream.connId;
    await startRecording(id);
    expect(composerOf(id).mic.run?.status).toBe("recording");
    micHandlers!.onChunk(new Uint8Array([2]));
    await stopRecording(id);
    expect(mocked.wsSend.mock.calls.map((c) => c[1])).toEqual(["AQ==", "Ag==", "CQ=="].map((base64) => ({ type: "binary", base64 })));
    expect(mocked.wsSend.mock.calls.every((c) => c[0] === connId)).toBe(true);
    expect(composerOf(id).mic.run).toBeNull();
    expect(isRecording(id)).toBe(false);

    mocked.wsSend.mockClear();
    await startRecording(id);
    mocked.wsSend.mockRejectedValueOnce(new Error("WebSocket is closed"));
    micHandlers!.onChunk(new Uint8Array([3]));
    await vi.waitFor(() => expect(isRecording(id)).toBe(false));
    expect(composerOf(id).mic.error).toBe("Sending stopped: WebSocket is closed");
    // Only the first chunk went out: nothing after the failure.
    expect(mocked.wsSend).toHaveBeenCalledTimes(2);
  });
});

describe("streaming into a gRPC call", () => {
  it("sends a file or the microphone on an open client stream, each chunk in the chunk field", async () => {
    const { useGrpc } = await import("./grpc");
    const { grpcChunk } = await import("./tabs");
    expect(grpcChunk("audio.content", "AAE=")).toBe('{"audio":{"content":"AAE="}}');
    const id = openDraft({ name: "s", seq: 1, method: "speech.Speech/Stream", url: "grpcs://h", kind: "grpc" });
    patchFile(id, { path: "/a.raw", info: { name: "a.raw", size: 3200, wav: null }, chunkSize: 320, intervalMs: 10 });
    // No stream open yet: nothing starts.
    await startFileStream(id);
    expect(mocked.fileStreamStart).not.toHaveBeenCalled();
    const method = { path: "speech.Speech/Stream", inputType: "In", outputType: "Out", clientStreaming: true, serverStreaming: true, example: "{}" };
    const call = { id: "call-1", method, streaming: true, state: "open", clientEnded: false } as unknown as import("./grpc").GrpcCall;
    useGrpc.setState({ calls: { [id]: call } });
    // The field that carries the bytes is needed first.
    await startFileStream(id);
    expect(mocked.fileStreamStart).not.toHaveBeenCalled();
    updateTab(id, (t) => ({ draft: { ...t.draft, grpc: { chunkField: "audioContent" } } }));
    await startFileStream(id);
    expect(mocked.fileStreamStart).toHaveBeenCalledWith(expect.objectContaining({ connId: "call-1", message: { type: "grpc", field: "audioContent" } }));
    // The microphone's chunks go out as gRPC messages, and recording stops when the client side ends.
    await startRecording(id);
    await vi.waitFor(() => expect(mocked.grpcSend).toHaveBeenCalledWith("call-1", JSON.stringify({ audioContent: "AQ==" })));
    useGrpc.setState({ calls: { [id]: { ...call, clientEnded: true } } });
    await vi.waitFor(() => expect(isRecording(id)).toBe(false));
    // When the whole file has been sent, the stream ends (the server then gives its last answers).
    useGrpc.setState({ calls: { [id]: call } });
    for (const h of handlers) h({ type: "fileStream", connId: "call-1", event: { type: "ended", progress: { sent: 3200, messages: 10, pass: 1, position: 3200, total: 3200 }, stopped: false, error: null } });
    await vi.waitFor(() => expect(mocked.grpcEnd).toHaveBeenCalledWith("call-1"));
  });
});

describe("saved messages, repeating sends and closing", () => {
  it("saves the composer as a message, loads one back and sends one every so often", async () => {
    const id = await connected({ name: "w", seq: 1, method: "GET", url: "ws://h", kind: "websocket", body: { type: "json", text: '{"op": 1}' } });
    const saving = saveComposerAsMessage(id);
    await answer("Login");
    await saving;
    expect(tabById(id).draft.websocket?.messages).toEqual([{ name: "Login", format: "json", text: '{"op": 1}' }]);

    loadIntoComposer(id, { name: "Raw", format: "binary", text: "01 02" });
    expect(composerOf(id)).toMatchObject({ binary: true, source: "hex" });
    expect(tabById(id).draft.body).toMatchObject({ type: "text", text: "01 02" });

    await startRepeat(id, 5000);
    const connId = tabById(id).stream.connId;
    expect(mocked.wsRepeat).toHaveBeenCalledWith(connId, { name: "", format: "binary", text: "01 02" }, 5000);
    expect(composerOf(id).repeat).toEqual({ everyMs: 5000, connId });
    await stopRepeat(id);
    expect(mocked.wsStopRepeat).toHaveBeenCalled();
    expect(composerOf(id).repeat).toBeNull();
  });

  it("keeps a saved emit in the request's file at once, leaving other draft changes unsaved", async () => {
    const saved: Request = { name: "io", seq: 1, method: "GET", url: "http://h", kind: "socketio", socketio: { event: "join" } };
    const id = openDraft(structuredClone(saved));
    updateTab(id, () => ({ path: "io.yaml", saved }));
    updateTab(id, (t) => ({ draft: { ...t.draft, url: "http://changed", body: { type: "json", text: '["lobby"]' } } }));
    const saving = saveComposerAsMessage(id);
    await answer("Join");
    await saving;
    const emit = { name: "Join", format: "json", text: '["lobby"]', event: "join" };
    expect(mocked.saveRequest).toHaveBeenCalledWith("io.yaml", { ...saved, socketio: { event: "join", messages: [emit] } });
    expect(tabById(id).saved?.socketio?.messages).toEqual([emit]);
    expect(tabById(id).saved?.url).toBe("http://h");
    expect(tabById(id).draft.socketio?.messages).toEqual([emit]);
  });

  it("stops showing a repeat the backend stopped or whose connection closed", async () => {
    const id = await connected({ name: "w", seq: 1, method: "GET", url: "ws://h", kind: "websocket", body: { type: "text", text: "ping" } });
    const connId = tabById(id).stream.connId;
    await startRepeat(id, 1000);
    // A step's saved message failing isn't the composer's repeat.
    for (const h of handlers) h({ type: "ws", connId, event: { type: "error", message: "Stopped sending Heartbeat every 1 s: broken pipe" } });
    expect(activeRepeat(tabById(id), composerOf(id))).toEqual({ everyMs: 1000, connId });
    for (const h of handlers) h({ type: "ws", connId, event: { type: "error", message: "Stopped sending the message every 1 s: broken pipe" } });
    expect(composerOf(id).repeat).toBeNull();

    // Closed while the backend was starting it: nothing shows as repeating.
    mocked.wsRepeat.mockImplementationOnce(async () => {
      updateTab(id, (t) => ({ stream: { ...t.stream, status: "closed" } }));
      return null;
    });
    await startRepeat(id, 1000);
    expect(composerOf(id).repeat).toBeNull();
  });

  it("closes with the code and reason typed", async () => {
    const id = await connected({ name: "w", seq: 1, method: "GET", url: "ws://h", kind: "websocket" });
    const closing = closeWithCode(id);
    await answer("4001 Session expired");
    await closing;
    expect(mocked.wsClose).toHaveBeenCalledWith(tabById(id).stream.connId, 4001, "Session expired");
    mocked.wsClose.mockClear();
    const refused = closeWithCode(id);
    await answer("1006");
    await refused;
    expect(mocked.wsClose).not.toHaveBeenCalled();
  });
});

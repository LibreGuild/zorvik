// The stream composer's state per request tab, beside the draft: binary mode and where its bytes
// come from (typed hex, a file the backend streams, or the microphone), the file stream's
// progress ("fileStream" events) and the recording. A closed tab or connection stops both. A file
// or the microphone can also feed a gRPC client or bidirectional stream (`openConnection`).
import { create } from "zustand";
import type { ChunkMessage } from "../bindings/ChunkMessage";
import type { FileStreamInfo } from "../bindings/FileStreamInfo";
import type { FileStreamProgress } from "../bindings/FileStreamProgress";
import type { MessageFormat } from "../bindings/MessageFormat";
import type { Request } from "../bindings/Request";
import type { SavedMessage } from "../bindings/SavedMessage";
import type { WavInfo } from "../bindings/WavInfo";
import { bytesToBase64 } from "../lib/encoders";
import { onEvent } from "../lib/events";
import { type MicOptions, type MicSession, startMic as openMic } from "../lib/mic";
import { pickFile } from "../lib/platform";
import { api, errorMessage } from "../lib/rpc";
import { prompt } from "./dialogs";
import { endStream, openCall, useGrpc } from "./grpc";
import { chunkTarget, hexToBase64, isRequestTab, sendChunk, type Tab, updateDraft, updateTab, useTabs, wsSend } from "./tabs";
import { toast } from "./toasts";

export type BinarySource = "hex" | "file" | "mic";

/** The largest UDP datagram over IPv4. */
export const MAX_UDP_PAYLOAD = 65507;

export interface FileRun {
  status: "running" | "done" | "stopped" | "error";
  progress: FileStreamProgress;
  startedAt: number;
  endedAt: number | null;
  error: string | null;
}

export interface FileState {
  path: string | null;
  info: FileStreamInfo | null;
  /** Bytes per message. */
  chunkSize: number;
  intervalMs: number;
  /** Bytes skipped at the start (a header). */
  offset: number;
  /** Bytes sent from the offset; null = to the end. */
  length: number | null;
  repeat: boolean;
  run: FileRun | null;
}

export interface MicRun {
  status: "starting" | "recording" | "stopping";
  startedAt: number;
  bytes: number;
  messages: number;
  level: number;
}

export interface MicState {
  options: MicOptions;
  run: MicRun | null;
  error: string | null;
}

export interface ComposerState {
  binary: boolean;
  source: BinarySource;
  file: FileState;
  mic: MicState;
  /** The composer's message is being sent every `everyMs` on the connection `connId`. */
  repeat: { everyMs: number; connId: string } | null;
  /** gRPC: end the stream (half-close) when the file has been sent or the recording stops. */
  endWhenDone: boolean;
}

const MIC_KEY = "zorvik.mic";

export const DEFAULT_MIC: MicOptions = {
  deviceId: "",
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

/** The microphone settings used last (they're the same for every connection). */
function rememberedMic(): MicOptions {
  try {
    return { ...DEFAULT_MIC, ...(JSON.parse(localStorage.getItem(MIC_KEY) ?? "{}") as Partial<MicOptions>) };
  } catch {
    return DEFAULT_MIC;
  }
}

function initial(): ComposerState {
  return {
    binary: false,
    source: "hex",
    file: { path: null, info: null, chunkSize: 1024, intervalMs: 100, offset: 0, length: null, repeat: false, run: null },
    mic: { options: rememberedMic(), run: null, error: null },
    repeat: null,
    endWhenDone: true,
  };
}

export const useComposer = create<{ byTab: Record<string, ComposerState> }>(() => ({ byTab: {} }));

const EMPTY = initial();

/** The tab's composer state (defaults until something is set). */
export function useComposerState(tabId: string): ComposerState {
  return useComposer((s) => s.byTab[tabId] ?? EMPTY);
}

export function composerOf(tabId: string): ComposerState {
  return useComposer.getState().byTab[tabId] ?? initial();
}

export function patchComposer(tabId: string, fn: (s: ComposerState) => Partial<ComposerState>) {
  useComposer.setState((st) => {
    const cur = st.byTab[tabId] ?? initial();
    return { byTab: { ...st.byTab, [tabId]: { ...cur, ...fn(cur) } } };
  });
}

export function patchFile(tabId: string, patch: Partial<FileState>) {
  patchComposer(tabId, (s) => ({ file: { ...s.file, ...patch } }));
}

export function setMicOptions(tabId: string, patch: Partial<MicOptions>) {
  patchComposer(tabId, (s) => {
    const options = { ...s.mic.options, ...patch };
    try {
      localStorage.setItem(MIC_KEY, JSON.stringify(options));
    } catch {
      // Not remembered; this connection still uses them.
    }
    return { mic: { ...s.mic, options, error: null } };
  });
}

function requestTab(tabId: string): Tab | undefined {
  const tab = useTabs.getState().tabs.find((t) => t.id === tabId);
  return isRequestTab(tab) ? tab : undefined;
}

/** Messages that carry an audio file's samples in real time: `frameMs` of audio each, header skipped. */
export function audioPace(wav: WavInfo, frameMs = 20): Pick<FileState, "chunkSize" | "intervalMs" | "offset" | "length"> {
  const block = Math.max(1, wav.blockAlign);
  const chunk = Math.max(block, Math.floor((wav.byteRate * frameMs) / 1000 / block) * block);
  return { chunkSize: chunk, intervalMs: frameMs, offset: wav.dataOffset, length: wav.dataSize };
}

/** Bytes one pass sends. */
export function passBytes(file: FileState): number {
  if (!file.info) return 0;
  return Math.max(0, Math.min(file.info.size - file.offset, file.length ?? Number.MAX_SAFE_INTEGER));
}

/** What the settings send: bytes per second, messages and time per pass. */
export function filePlan(file: FileState): { bytesPerSecond: number; messages: number; passMs: number } {
  const bytes = passBytes(file);
  const chunk = Math.max(1, Math.floor(file.chunkSize) || 1);
  const interval = Math.max(1, Math.floor(file.intervalMs) || 1);
  const messages = Math.ceil(bytes / chunk);
  // The first message goes at once, the others one interval apart.
  return { bytesPerSecond: (chunk * 1000) / interval, messages, passMs: Math.max(0, messages - 1) * interval };
}

const WAV_ENCODINGS: Record<string, string> = { pcm: "PCM", float: "float", alaw: "A-law", mulaw: "μ-law" };

/** "WAV · PCM 16-bit · 16 kHz · mono". */
export function describeWav(wav: WavInfo): string {
  const encoding = WAV_ENCODINGS[wav.encoding] ?? wav.encoding;
  const channels = wav.channels === 1 ? "mono" : wav.channels === 2 ? "stereo" : `${wav.channels} channels`;
  return ["WAV", `${encoding} ${wav.bitsPerSample}-bit`, `${wav.sampleRate / 1000} kHz`, channels].join(" · ");
}

// ---- saved messages, repeating sends, ping, close (WebSocket) --------------------------------------

/** A name not taken yet: "Message", then "Message (2)". */
export function freeName(taken: string[], base: string): string {
  if (!taken.includes(base)) return base;
  for (let n = 2; ; n++) if (!taken.includes(`${base} (${n})`)) return `${base} (${n})`;
}

/** Put a saved message in the composer (its format becomes the composer's mode). */
export function loadIntoComposer(tabId: string, message: SavedMessage) {
  const binary = message.format === "binary";
  patchComposer(tabId, () => (binary ? { binary: true, source: "hex" } : { binary: false }));
  updateDraft(tabId, (r) => ({
    ...r,
    body: { ...(r.body ?? { type: "text" }), type: message.format === "json" ? "json" : "text", text: message.text },
    // Socket.IO: the composer emits the message's event, asking for an acknowledgement when it does.
    ...(r.kind === "socketio" ? { socketio: { ...(r.socketio ?? {}), event: message.event ?? "", ack: message.ack || undefined } } : {}),
  }));
}

export async function sendSaved(tabId: string, message: SavedMessage) {
  const tab = requestTab(tabId);
  if (tab?.draft.kind !== "socketio") {
    await wsSend(tabId, message.text, message.format === "binary");
    return;
  }
  if (tab.stream.status !== "open") return;
  try {
    const render = (t: string) => (t.includes("{{") ? api.renderVariables(t) : Promise.resolve(t));
    const event = (await render(message.event ?? "")).trim();
    if (!event) {
      toast("info", `“${message.name}” has no event to emit`);
      return;
    }
    const binary = message.format === "binary" ? hexToBase64(message.text) : null;
    if (message.format === "binary" && binary === null) {
      toast("error", "Invalid hex", "Binary arguments are written as hex bytes, e.g. 48 65 6c 6c 6f");
      return;
    }
    const text = binary !== null ? "" : await render(message.text);
    const args = binary !== null || message.format === "json" ? text : JSON.stringify(text);
    await api.socketSend(tab.stream.connId, { type: "emit", event, args, base64: binary, ack: message.ack ?? false });
  } catch (e) {
    toast("error", "Emit failed", errorMessage(e));
  }
}

/** The saved messages of a WebSocket or Socket.IO request. */
function savedOf(r: Request): SavedMessage[] {
  return (r.kind === "socketio" ? r.socketio?.messages : r.websocket?.messages) ?? [];
}

/** The composer's message as a saved message (unnamed). */
function composerMessage(tab: Tab): SavedMessage {
  const c = composerOf(tab.id);
  const format: MessageFormat = c.binary ? "binary" : tab.draft.body?.type === "json" ? "json" : "text";
  const io = tab.draft.kind === "socketio" ? { event: tab.draft.socketio?.event ?? "", ack: tab.draft.socketio?.ack || undefined } : {};
  return { name: "", format, text: tab.draft.body?.text ?? "", ...io };
}

/** Save what's in the composer with the request, under a name the user gives (replacing one with that name). */
export async function saveComposerAsMessage(tabId: string) {
  const tab = requestTab(tabId);
  if (!tab) return;
  const message = composerMessage(tab);
  const io = tab.draft.kind === "socketio";
  if (io ? !message.event?.trim() : !message.text.trim()) {
    toast("info", io ? "Enter the event to emit first" : "Type a message first");
    return;
  }
  const taken = savedOf(tab.draft).map((m) => m.name);
  const name = (
    await prompt({
      title: io ? "Save as emit" : "Save as message",
      message: `Saved ${io ? "emits" : "messages"} are kept with the request: send them from this menu or from On connect steps.`,
      value: freeName(taken, io ? message.event!.trim() : "Message"),
      confirmLabel: "Save",
    })
  )?.trim();
  if (!name) return;
  const saved = { ...message, name, format: message.format === "text" ? undefined : message.format };
  const withMessage = (r: Request): Request => {
    const list = savedOf(r);
    const messages = list.some((m) => m.name === name) ? list.map((m) => (m.name === name ? saved : m)) : [...list, saved];
    return r.kind === "socketio" ? { ...r, socketio: { ...(r.socketio ?? {}), messages } } : { ...r, websocket: { ...(r.websocket ?? {}), messages } };
  };
  updateDraft(tabId, withMessage);
  // A saved request keeps it in its file right away (where AI agents and teammates see it),
  // leaving the draft's other changes unsaved.
  const current = requestTab(tabId);
  if (current?.path && current.saved && !current.orphaned) {
    const file = withMessage(current.saved);
    try {
      await api.saveRequest(current.path, file);
      updateTab(tabId, () => ({ saved: file }));
      toast("success", `Saved “${name}” with the request`);
    } catch (e) {
      toast("error", `Could not save “${name}”`, errorMessage(e));
    }
    return;
  }
  toast("success", `Saved “${name}”`, "Save the request to keep it.");
}

/**
 * The tab's open connection that a file or the microphone sends on: its WebSocket or socket
 * session, or a gRPC stream that still takes messages (a client or bidirectional one).
 */
export function openConnection(tab: Tab): string | null {
  if (tab.draft.kind === "grpc") {
    const call = openCall(tab.id);
    return call && !call.clientEnded && call.method?.clientStreaming ? call.id : null;
  }
  return tab.stream.status === "open" ? tab.stream.connId : null;
}

/** Whether the tab's connection is still `connId` and open. */
function stillOpen(tabId: string, connId: string): boolean {
  const tab = requestTab(tabId);
  return !!tab && openConnection(tab) === connId;
}

/** The composer's repeating send on the tab's open connection, or null (it ends with its connection). */
export function activeRepeat(tab: Tab, c: ComposerState): ComposerState["repeat"] {
  return c.repeat && tab.stream.status === "open" && tab.stream.connId === c.repeat.connId ? c.repeat : null;
}

export async function startRepeat(tabId: string, everyMs: number) {
  const tab = requestTab(tabId);
  if (!tab || tab.stream.status !== "open") return;
  const message = composerMessage(tab);
  const io = tab.draft.kind === "socketio";
  if (io ? !message.event?.trim() : !message.text.trim()) {
    toast("info", io ? "Enter the event to emit first" : "Type the message to send first");
    return;
  }
  const { connId } = tab.stream;
  try {
    if (tab.draft.kind === "websocket") await api.wsRepeat(connId, message, everyMs);
    else await api.socketRepeat(connId, message, everyMs);
    // A connection that closed meanwhile took the repeat with it.
    if (stillOpen(tabId, connId)) patchComposer(tabId, () => ({ repeat: { everyMs, connId } }));
  } catch (e) {
    toast("error", "Can't send repeatedly", errorMessage(e));
  }
}

export async function stopRepeat(tabId: string) {
  const tab = requestTab(tabId);
  patchComposer(tabId, () => ({ repeat: null }));
  if (!tab) return;
  if (tab.draft.kind === "websocket") await api.wsStopRepeat(tab.stream.connId).catch(() => {});
  else await api.socketStopRepeat(tab.stream.connId).catch(() => {});
}

export async function sendPing(tabId: string) {
  const tab = requestTab(tabId);
  if (tab?.stream.status === "open") await api.wsSend(tab.stream.connId, { type: "ping" }).catch((e) => toast("error", "Ping failed", errorMessage(e)));
}

/** Close with a code and reason the user types: "4001 Session expired". */
export async function closeWithCode(tabId: string) {
  const tab = requestTab(tabId);
  if (!tab || tab.stream.status !== "open") return;
  const answer = await prompt({
    title: "Close with a code",
    message: "The close code, then an optional reason: 1000 is a normal close; 4000 to 4999 are yours to define.",
    value: "1000",
    placeholder: "4001 Session expired",
    confirmLabel: "Close connection",
  });
  if (answer == null) return;
  const m = answer.trim().match(/^(\d+)\s*(.*)$/);
  const code = m ? Number(m[1]) : NaN;
  if (!(code === 1000 || (code >= 3000 && code <= 4999))) {
    toast("error", "Use 1000, or a code from 3000 to 4999");
    return;
  }
  await api.wsClose(tab.stream.connId, code, m![2]).catch((e) => toast("error", "Close failed", errorMessage(e)));
}

// ---- file streams -----------------------------------------------------------------------------

export async function chooseStreamFile(tabId: string) {
  const path = await pickFile("Choose a file to stream");
  if (!path) return;
  try {
    const info = await api.fileStreamInfo(path);
    // A WAV file is most likely meant to play in real time: pace it as audio (still editable).
    const pace = info.wav && info.wav.byteRate > 0 ? audioPace(info.wav) : { offset: 0, length: null };
    patchFile(tabId, { path, info, ...pace, run: null });
  } catch (e) {
    toast("error", "Can't stream that file", errorMessage(e));
  }
}

/** Why the file can't stream now, or null when it can. */
export function fileProblem(tab: Tab, file: FileState): string | null {
  if (!file.info) return "Choose a file to stream";
  if (passBytes(file) <= 0) return "Nothing to send after the start byte";
  if (!(file.chunkSize >= 1)) return "Enter how many bytes each message carries";
  if (!(file.intervalMs >= 1)) return "Enter how often to send (at least 1 ms)";
  if (tab.draft.kind === "udp" && file.chunkSize > MAX_UDP_PAYLOAD) return `A UDP datagram carries at most ${MAX_UDP_PAYLOAD.toLocaleString()} bytes`;
  const socket = tab.draft.socket;
  if (tab.draft.kind === "tcp" && socket?.framing === "lengthPrefixed" && socket.lengthBytes !== 4) {
    const max = socket.lengthBytes === 1 ? 255 : 65535;
    if (file.chunkSize > max) return `With a ${socket.lengthBytes ?? 2}-byte length prefix, a message carries at most ${max.toLocaleString()} bytes`;
  }
  return null;
}

export async function startFileStream(tabId: string) {
  const tab = requestTab(tabId);
  const file = composerOf(tabId).file;
  const connId = tab ? openConnection(tab) : null;
  if (!tab || !connId || !file.path) return;
  const problem = fileProblem(tab, file);
  if (problem) {
    toast("info", problem);
    return;
  }
  const target = await chunkTarget(tab);
  // The connection changed, or another start won, while the target was worked out.
  if (!stillOpen(tabId, connId) || composerOf(tabId).file.run?.status === "running") return;
  if (typeof target === "string") {
    toast("info", target);
    return;
  }
  // Running before the call returns: a short file can end (its event arrives) before that.
  const progress = { sent: 0, messages: 0, pass: 1, position: 0, total: passBytes(file) };
  patchFile(tabId, { run: { status: "running", progress, startedAt: Date.now(), endedAt: null, error: null } });
  try {
    const { total } = await api.fileStreamStart({
      connId,
      path: file.path,
      // Whole numbers for the backend (fields and presets give them; saved state may not).
      chunkSize: Math.max(1, Math.floor(file.chunkSize)),
      intervalMs: Math.max(1, Math.floor(file.intervalMs)),
      offset: Math.max(0, Math.floor(file.offset)),
      length: file.length === null ? null : Math.max(1, Math.floor(file.length)),
      repeat: file.repeat,
      message: target,
    });
    const run = composerOf(tabId).file.run;
    if (run) patchFile(tabId, { run: { ...run, progress: { ...run.progress, total } } });
  } catch (e) {
    patchFile(tabId, { run: null });
    toast("error", "Can't start the stream", errorMessage(e));
  }
}

export async function stopFileStream(tabId: string) {
  const tab = requestTab(tabId);
  const connId = tab && (openConnection(tab) ?? (tab.draft.kind === "grpc" ? useGrpc.getState().calls[tab.id]?.id : tab.stream.connId));
  if (connId) await api.fileStreamStop(connId).catch(() => {});
}

/** How the backend says a repeating send stopped by itself (a send failed); the composer's message has no name. */
const REPEAT_STOPPED = "Stopped sending the message every ";

onEvent((e) => {
  if ((e.type === "ws" || e.type === "socket") && e.event.type === "error" && e.event.message.startsWith(REPEAT_STOPPED)) {
    for (const [tabId, c] of Object.entries(useComposer.getState().byTab)) {
      if (c.repeat?.connId === e.connId) patchComposer(tabId, () => ({ repeat: null }));
    }
    return;
  }
  if (e.type !== "fileStream") return;
  const calls = useGrpc.getState().calls;
  const tab = useTabs.getState().tabs.find((t) => isRequestTab(t) && (t.stream.connId === e.connId || calls[t.id]?.id === e.connId));
  const run = tab ? composerOf(tab.id).file.run : null;
  if (!tab || !run) return;
  if (e.event.type === "progress") {
    const { type: _, ...progress } = e.event;
    patchFile(tab.id, { run: { ...run, progress } });
  } else {
    const { progress, stopped, error } = e.event;
    const status = error ? "error" : stopped ? "stopped" : "done";
    patchFile(tab.id, { run: { ...run, progress, status, endedAt: Date.now(), error } });
    // A gRPC stream ends once the whole file is sent: a speech service then gives its final results.
    if (status === "done" && isRequestTab(tab) && tab.draft.kind === "grpc" && composerOf(tab.id).endWhenDone) void endStream(tab.id);
  }
});

// ---- microphone -------------------------------------------------------------------------------

interface Recording {
  session: MicSession | null;
  connId: string;
  stopping: boolean;
  /** Sending failed or the recording was dropped: later chunks are not sent. */
  dropped: boolean;
  /** Chunks go out one at a time, so the server gets them in order. */
  queue: Promise<void>;
}

const recordings = new Map<string, Recording>();

export async function startRecording(tabId: string) {
  const tab = requestTab(tabId);
  const connId = tab ? openConnection(tab) : null;
  if (!tab || !connId || recordings.has(tabId)) return;
  const options = composerOf(tabId).mic.options;
  const request = tab.draft;
  // Taken before the first await, so a second click can't start another recording.
  const rec: Recording = { session: null, connId, stopping: false, dropped: false, queue: Promise.resolve() };
  recordings.set(tabId, rec);
  const target: ChunkMessage | string = await chunkTarget(tab);
  // Stopped, or the connection changed, while the target was worked out: the microphone isn't opened.
  if (recordings.get(tabId) !== rec || rec.stopping || !stillOpen(tabId, connId)) {
    if (recordings.get(tabId) === rec) recordings.delete(tabId);
    return;
  }
  if (typeof target === "string") {
    recordings.delete(tabId);
    toast("info", target);
    return;
  }
  patchComposer(tabId, (s) => ({ mic: { ...s.mic, error: null, run: { status: "starting", startedAt: Date.now(), bytes: 0, messages: 0, level: 0 } } }));
  let bytes = 0;
  let messages = 0;
  const fail = (message: string) => void stopRecording(tabId, message);
  try {
    rec.session = await openMic(options, {
      onChunk: (chunk) => {
        if (rec.dropped) return;
        const base64 = bytesToBase64(chunk);
        rec.queue = rec.queue
          .then(async () => {
            if (rec.dropped) return;
            await sendChunk(connId, request, target, base64);
            bytes += chunk.length;
            messages += 1;
          })
          .catch((e) => {
            if (rec.dropped) return;
            rec.dropped = true;
            fail(`Sending stopped: ${errorMessage(e)}`);
          });
      },
      onLevel: (level) =>
        patchComposer(tabId, (s) => (s.mic.run ? { mic: { ...s.mic, run: { ...s.mic.run, level, bytes, messages } } } : {})),
      onEnded: fail,
    });
  } catch (e) {
    // Only this recording: another may have started after a stop during the permission prompt.
    if (recordings.get(tabId) === rec) {
      recordings.delete(tabId);
      patchComposer(tabId, (s) => ({ mic: { ...s.mic, run: null, error: errorMessage(e) } }));
    }
    return;
  }
  // Stopped (or the tab closed) while the permission prompt was open: nothing is sent.
  if (recordings.get(tabId) !== rec || rec.stopping) {
    rec.dropped = true;
    await rec.session.stop();
    return;
  }
  patchComposer(tabId, (s) => (s.mic.run ? { mic: { ...s.mic, run: { ...s.mic.run, status: "recording" } } } : {}));
}

export async function stopRecording(tabId: string, error: string | null = null) {
  const rec = recordings.get(tabId);
  if (!rec || rec.stopping) return;
  rec.stopping = true;
  patchComposer(tabId, (s) => ({ mic: { ...s.mic, error, run: s.mic.run && { ...s.mic.run, status: "stopping" } } }));
  // The last partial chunk still goes out (unless sending already failed).
  await rec.session?.stop().catch(() => {});
  await rec.queue;
  recordings.delete(tabId);
  const tab = requestTab(tabId);
  if (!error && tab?.draft.kind === "grpc" && composerOf(tabId).endWhenDone && openConnection(tab) === rec.connId) await endStream(tabId);
  if (requestTab(tabId)) patchComposer(tabId, (s) => ({ mic: { ...s.mic, run: null } }));
}

export function isRecording(tabId: string): boolean {
  return recordings.has(tabId);
}

/** Recordings whose connection closed (or whose gRPC stream stopped taking messages) stop. */
function stopOrphanRecordings() {
  for (const [tabId, rec] of recordings) {
    const tab = useTabs.getState().tabs.find((t) => t.id === tabId);
    if (!isRequestTab(tab) || openConnection(tab) !== rec.connId) void stopRecording(tabId);
  }
}

useGrpc.subscribe((state, prev) => {
  if (state.calls !== prev.calls) stopOrphanRecordings();
});

// A recording ends with its connection; a closed tab drops its composer state.
useTabs.subscribe((state, prev) => {
  if (state.tabs === prev.tabs) return;
  stopOrphanRecordings();
  const byTab = useComposer.getState().byTab;
  // A repeating send ends with its connection (the backend stops it).
  for (const [tabId, c] of Object.entries(byTab)) {
    const tab = state.tabs.find((t) => t.id === tabId);
    if (c.repeat && (!isRequestTab(tab) || !activeRepeat(tab, c))) patchComposer(tabId, () => ({ repeat: null }));
  }
  const gone = Object.keys(byTab).filter((id) => !state.tabs.some((t) => t.id === id));
  if (gone.length) {
    useComposer.setState({ byTab: Object.fromEntries(Object.entries(byTab).filter(([id]) => !gone.includes(id))) });
  }
});

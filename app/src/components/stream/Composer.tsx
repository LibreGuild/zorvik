// The message composer of a live connection: text, JSON or binary (typed hex, a file, the
// microphone). WebSocket requests add saved messages, sending every few seconds, and ping.
import { Bookmark, ChevronDown, Mic, Play, Radio, Repeat, Save, Send, Square } from "lucide-react";
import {
  activeRepeat,
  type BinarySource,
  fileProblem,
  loadIntoComposer,
  patchComposer,
  saveComposerAsMessage,
  sendPing,
  startFileStream,
  startRecording,
  startRepeat,
  stopFileStream,
  stopRecording,
  stopRepeat,
  useComposerState,
} from "../../store/composer";
import { prompt } from "../../store/dialogs";
import { type Tab, updateDraft, updateTab, wsSend } from "../../store/tabs";
import { toast } from "../../store/toasts";
import { useVariableNames } from "../../store/workspace";
import { modKey } from "../../lib/platform";
import { CodeEditor } from "../CodeEditor";
import { COMPOSER_EXTRAS } from "../request/kinds";
import { Button, cx, IconButton, Menu, type MenuEntry, Segmented } from "../ui";
import { FileStreamPanel, MicPanel } from "./BinarySources";

const SOURCES = [
  { id: "hex", label: "Hex" },
  { id: "file", label: "File" },
  { id: "mic", label: "Microphone" },
] as const;

const EVERY: { ms: number; label: string }[] = [
  { ms: 1000, label: "every second" },
  { ms: 5000, label: "every 5 seconds" },
  { ms: 30000, label: "every 30 seconds" },
];

function everyText(ms: number): string {
  return ms < 1000 || ms % 100 !== 0 ? `${ms} ms` : `${ms / 1000} s`;
}

async function sendEveryCustom(tabId: string) {
  const answer = await prompt({ title: "Send every…", message: "Milliseconds between sends (10 or more). {{variables}} are filled in each time.", value: "2000", confirmLabel: "Start" });
  if (answer == null) return;
  const ms = Number(answer.trim());
  if (!Number.isFinite(ms) || ms < 10) {
    toast("error", "Enter at least 10 milliseconds");
    return;
  }
  await startRepeat(tabId, Math.round(ms));
}

export function Composer({ tab }: { tab: Tab }) {
  const { names } = useVariableNames();
  const kind = tab.draft.kind ?? "http";
  const Extra = COMPOSER_EXTRAS[kind];
  const ws = kind === "websocket";
  // Saved messages: WebSocket and Socket.IO; sending every so often: those and TCP/UDP.
  const saving = ws || kind === "socketio";
  const repeats = saving || kind === "tcp" || kind === "udp";
  const verb = kind === "socketio" ? "Emit" : "Send";
  const composer = useComposerState(tab.id);
  const mode = composer.binary ? "binary" : tab.draft.body?.type === "json" ? "json" : "text";
  const source: BinarySource = composer.binary ? composer.source : "hex";
  const text = tab.draft.body?.text ?? "";
  const open = tab.stream.status === "open";
  const streaming = composer.file.run?.status === "running";
  const micBusy = composer.mic.run != null;
  // Only a repeat of the connection that is open now.
  const repeating = activeRepeat(tab, composer);
  const saved = (kind === "socketio" ? tab.draft.socketio?.messages : tab.draft.websocket?.messages) ?? [];
  // A Socket.IO event may have no arguments.
  const ready = open && (text.length > 0 || kind === "socketio");
  const submit = () => {
    if (ready && !repeating) void wsSend(tab.id, text, mode === "binary");
  };
  const sendMenu: MenuEntry[] = [
    ...EVERY.map((e) => ({ label: `${verb} ${e.label}`, icon: <Repeat size={13} />, disabled: !ready, onSelect: () => void startRepeat(tab.id, e.ms) })),
    { label: `${verb} every…`, icon: <Repeat size={13} />, disabled: !ready, onSelect: () => void sendEveryCustom(tab.id) },
    ...(ws ? ([{ separator: true }, { label: "Send a ping", icon: <Radio size={13} />, disabled: !open, onSelect: () => void sendPing(tab.id) }] as MenuEntry[]) : []),
  ];
  const action =
    source === "file" ? (
      streaming ? (
        <Button size="sm" variant="danger" icon={<Square size={11} />} onClick={() => void stopFileStream(tab.id)}>
          Stop
        </Button>
      ) : (
        <Button size="sm" variant="primary" icon={<Play size={13} />} disabled={!open || fileProblem(tab, composer.file) !== null} onClick={() => void startFileStream(tab.id)}>
          Start streaming
        </Button>
      )
    ) : source === "mic" ? (
      micBusy ? (
        <Button size="sm" variant="danger" icon={<Square size={11} />} disabled={composer.mic.run?.status === "stopping"} onClick={() => void stopRecording(tab.id)}>
          Stop recording
        </Button>
      ) : (
        <Button size="sm" variant="primary" icon={<Mic size={13} />} disabled={!open} onClick={() => void startRecording(tab.id)}>
          Start recording
        </Button>
      )
    ) : repeating ? (
      <>
        <span className="flex items-center gap-1 text-[11.5px] font-medium text-accent" data-testid="repeating">
          <Repeat size={12} className="animate-pulse" />
          {kind === "socketio" ? "Emitting" : "Sending"} every {everyText(repeating.everyMs)}
        </span>
        <Button size="sm" variant="danger" icon={<Square size={11} />} onClick={() => void stopRepeat(tab.id)}>
          Stop
        </Button>
      </>
    ) : (
      <>
        <span className="text-[11px] text-faint">{modKey}+Enter</span>
        <div className="flex">
          <Button size="sm" variant="primary" icon={<Send size={13} />} disabled={!ready} onClick={submit} className={cx(repeats && "rounded-r-none")}>
            {kind === "socketio" ? "Emit" : "Send"}
          </Button>
          {repeats && (
            <Menu
              align="end"
              trigger={
                <button aria-label="More ways to send" className="flex h-7 items-center rounded-r-lg border-l border-white/20 bg-accent-strong px-1.5 text-accent-fg hover:brightness-110">
                  <ChevronDown size={13} />
                </button>
              }
              entries={sendMenu}
            />
          )}
        </div>
      </>
    );
  const busy = streaming || micBusy || !!repeating;
  return (
    <div className={cx("m-3 flex shrink-0 flex-col overflow-hidden rounded-xl border border-line bg-input", source === "hex" && "h-48")} data-testid="composer">
      <div className="flex min-h-9 flex-wrap items-center gap-x-2 gap-y-1 px-3 py-1">
        {Extra && <Extra tab={tab} />}
        {saving && (
          <Menu
            trigger={
              <IconButton label={kind === "socketio" ? "Saved emits" : "Saved messages"} disabled={busy}>
                <Bookmark size={14} />
              </IconButton>
            }
            entries={[
              ...saved.map((m) => ({
                label: (
                  <span className="flex items-center gap-2">
                    <span className="truncate">{m.name || "Unnamed"}</span>
                    {m.event && <span className="font-mono text-[10.5px] text-faint">{m.event}</span>}
                    {m.format && m.format !== "text" && <span className="text-[10.5px] uppercase text-faint">{m.format === "binary" ? "hex" : m.format}</span>}
                  </span>
                ),
                onSelect: () => loadIntoComposer(tab.id, m),
              })),
              ...(saved.length ? [{ separator: true as const }] : []),
              { label: kind === "socketio" ? "Save as emit…" : "Save as message…", icon: <Save size={13} />, onSelect: () => void saveComposerAsMessage(tab.id) },
              {
                label: saved.length ? (kind === "socketio" ? "Edit saved emits" : "Edit saved messages") : kind === "socketio" ? "Saved emits…" : "Saved messages…",
                icon: <Bookmark size={13} />,
                onSelect: () => updateTab(tab.id, () => ({ requestTab: "messages" })),
              },
            ]}
          />
        )}
        <div className={cx("contents", busy && "[&>*]:pointer-events-none [&>*]:opacity-60")}>
          <Segmented
            items={[
              { id: "text", label: "Text" },
              { id: "json", label: "JSON" },
              { id: "binary", label: "Binary" },
            ]}
            value={mode}
            onChange={(m) => {
              patchComposer(tab.id, () => ({ binary: m === "binary" }));
              if (m !== "binary") updateDraft(tab.id, (r) => ({ ...r, body: { ...(r.body ?? { type: "text" }), type: m === "json" ? "json" : "text" } }));
            }}
          />
          {mode === "binary" && <Segmented items={[...SOURCES]} value={source} onChange={(s) => patchComposer(tab.id, () => ({ source: s }))} />}
        </div>
        <div className="flex-1" />
        {action}
      </div>
      {source === "file" ? (
        <FileStreamPanel tab={tab} />
      ) : source === "mic" ? (
        <MicPanel tab={tab} locked={micBusy} />
      ) : (
        <div className="min-h-0 flex-1">
          <CodeEditor
            value={text}
            onChange={(v) => updateDraft(tab.id, (r) => ({ ...r, body: { ...(r.body ?? { type: "text" }), text: v } }))}
            language={mode === "json" ? "json" : "text"}
            variables={names}
            onSubmit={submit}
            lineNumbers={false}
            placeholder={
              mode === "binary"
                ? "48 65 6c 6c 6f"
                : kind === "socketio"
                  ? mode === "json"
                    ? 'Arguments as JSON, e.g. ["hi", {"n": 1}]'
                    : "Text, sent as one argument"
                  : "Message to send"
            }
          />
        </div>
      )}
    </div>
  );
}

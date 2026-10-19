// The composer's binary sources besides typed hex: a file streamed in paced messages, and the
// microphone streamed as audio. Each panel has its settings, what they send, and live progress.
import { useEffect, useState } from "react";
import { AudioLines, FileUp, Mic, Repeat } from "lucide-react";
import { AUDIO_FORMATS, type AudioFormat, BITRATES, CHUNK_MS, formatInfo, rawChunkBytes, SAMPLE_RATES } from "../../lib/audio";
import { formatBytes, formatMs } from "../../lib/format";
import { listMicrophones, supportedEncodedFormats } from "../../lib/mic";
import {
  audioPace,
  chooseStreamFile,
  describeWav,
  type FileState,
  filePlan,
  fileProblem,
  patchFile,
  setMicOptions,
  useComposerState,
} from "../../store/composer";
import type { Tab } from "../../store/tabs";
import { Button, Checkbox, cx, Menu, NumberInput, Segmented, Select } from "../ui";

const numberField =
  "h-7 w-20 rounded-md border border-line bg-panel-2 px-2 text-right font-mono text-[12px] text-fg outline-none focus:border-accent disabled:opacity-60";

const PACE_PRESETS: { label: string; chunkSize: number; intervalMs: number }[] = [
  { label: "1 KB every 100 ms", chunkSize: 1024, intervalMs: 100 },
  { label: "4 KB every second", chunkSize: 4096, intervalMs: 1000 },
  { label: "64 KB every second", chunkSize: 65536, intervalMs: 1000 },
  { label: "1 MB every second", chunkSize: 1 << 20, intervalMs: 1000 },
  { label: "As fast as possible (64 KB every 1 ms)", chunkSize: 65536, intervalMs: 1 },
];

function rate(bytesPerSecond: number): string {
  return `${formatBytes(Math.round(bytesPerSecond))}/s`;
}

export function FileStreamPanel({ tab }: { tab: Tab }) {
  const { file } = useComposerState(tab.id);
  const running = file.run?.status === "running";
  const set = (patch: Partial<FileState>) => patchFile(tab.id, patch);
  const wav = file.info?.wav ?? null;
  const plan = filePlan(file);
  const problem = file.info ? fileProblem(tab, file) : null;
  return (
    <div className="flex flex-col gap-2 px-3 pb-3 pt-1 text-[12px]" data-testid="file-stream">
      <div className="flex min-w-0 items-center gap-2">
        <FileUp size={14} className="shrink-0 text-faint" />
        {file.info ? (
          <>
            <span className="min-w-0 truncate font-medium text-fg" title={file.path ?? ""}>
              {file.info.name}
            </span>
            <span className="shrink-0 text-muted">{formatBytes(file.info.size)}</span>
            {wav && <span className="shrink-0 truncate rounded bg-accent-soft px-1.5 text-[11px] font-semibold text-accent">{describeWav(wav)}</span>}
          </>
        ) : (
          <span className="text-muted">No file chosen. Any file works: audio, video, images, captures.</span>
        )}
        <div className="flex-1" />
        <Button size="sm" disabled={running} onClick={() => void chooseStreamFile(tab.id)}>
          {file.info ? "Change…" : "Choose file…"}
        </Button>
      </div>
      {file.info && (
        <>
          <div className="flex flex-wrap items-center gap-x-2 gap-y-1.5 text-muted">
            <span>Send</span>
            <NumberInput bare aria-label="Bytes per message" className={numberField} min={1} value={file.chunkSize} disabled={running} onChange={(v) => set({ chunkSize: v ?? 1 })} />
            <span>bytes every</span>
            <NumberInput bare aria-label="Interval in milliseconds" className={numberField} min={1} value={file.intervalMs} disabled={running} onChange={(v) => set({ intervalMs: v ?? 1 })} />
            <span>ms</span>
            <Menu
              trigger={
                <Button size="sm" variant="ghost" disabled={running}>
                  Presets
                </Button>
              }
              entries={[
                ...(wav && wav.byteRate > 0
                  ? [10, 20, 40, 100].map((ms) => ({
                      label: `Real-time audio, ${ms} ms per message`,
                      icon: <AudioLines size={13} />,
                      onSelect: () => set(audioPace(wav, ms)),
                    }))
                  : []),
                ...PACE_PRESETS.map((p) => ({ label: p.label, onSelect: () => set({ chunkSize: p.chunkSize, intervalMs: p.intervalMs }) })),
              ]}
            />
            <span className="text-faint">
              {rate(plan.bytesPerSecond)} · {plan.messages.toLocaleString()} {plan.messages === 1 ? "message" : "messages"} · {formatMs(plan.passMs)}
            </span>
          </div>
          <div className="flex flex-wrap items-center gap-x-2 gap-y-1.5 text-muted">
            <span>From byte</span>
            <NumberInput bare aria-label="Start byte" className={numberField} max={Number.MAX_SAFE_INTEGER} value={file.offset} disabled={running} onChange={(v) => set({ offset: v ?? 0 })} />
            <span>for</span>
            <NumberInput
              bare
              optional
              aria-label="Bytes to send"
              className={numberField}
              min={1}
              max={Number.MAX_SAFE_INTEGER}
              value={file.length ?? undefined}
              placeholder="all"
              disabled={running}
              onChange={(length) => set({ length: length ?? null })}
            />
            <span>bytes</span>
            {wav && (file.offset !== wav.dataOffset || file.length !== wav.dataSize) && (
              <Button size="sm" variant="ghost" disabled={running} onClick={() => set({ offset: wav.dataOffset, length: wav.dataSize })}>
                Samples only
              </Button>
            )}
            {wav && file.offset === wav.dataOffset && <span className="text-faint">(the WAV header is skipped)</span>}
            <div className="flex-1" />
            <label className="flex cursor-pointer items-center gap-1.5">
              <Checkbox checked={file.repeat} onChange={(repeat) => !running && set({ repeat })} label="Loop until stopped" />
              <Repeat size={12} />
              Loop until stopped
            </label>
          </div>
          {problem && <div className="text-warning">{problem}</div>}
          {file.run && <FileProgress file={file} />}
        </>
      )}
    </div>
  );
}

function FileProgress({ file }: { file: FileState }) {
  const run = file.run!;
  const running = run.status === "running";
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    if (!running) return;
    const t = setInterval(() => setNow(Date.now()), 250);
    return () => clearInterval(t);
  }, [running]);
  const { progress } = run;
  const share = progress.total ? Math.min(1, progress.position / progress.total) : 0;
  const done = run.status === "done";
  const elapsed = (run.endedAt ?? now) - run.startedAt;
  const label = { running: "Streaming", done: "Sent", stopped: "Stopped", error: "Failed" }[run.status];
  return (
    <div className="flex flex-col gap-1" data-testid="file-stream-progress">
      <div className="h-1.5 overflow-hidden rounded-full bg-panel-2">
        <div
          className={cx("h-full rounded-full transition-[width]", run.status === "error" ? "bg-danger" : done ? "bg-success" : "bg-accent")}
          style={{ width: `${(done ? 1 : share) * 100}%` }}
        />
      </div>
      <div className="flex flex-wrap gap-x-2 text-muted">
        <span className={cx("font-medium", run.status === "error" ? "text-danger" : "text-fg")}>{label}</span>
        <span className="tabular-nums">
          {formatBytes(progress.sent)} in {progress.messages.toLocaleString()} {progress.messages === 1 ? "message" : "messages"}
          {file.repeat && ` · pass ${progress.pass}`} · {formatMs(elapsed)}
        </span>
        {run.error && <span className="text-danger">{run.error}</span>}
      </div>
    </div>
  );
}

const CHANNEL_ITEMS = [
  { id: "1", label: "Mono" },
  { id: "2", label: "Stereo" },
] as const;

export function MicPanel({ tab, locked }: { tab: Tab; locked: boolean }) {
  const { mic } = useComposerState(tab.id);
  const o = mic.options;
  const info = formatInfo(o.format);
  const raw = !info.mime;
  const [devices, setDevices] = useState<{ id: string; label: string }[]>([]);
  const [encoded] = useState(supportedEncodedFormats);
  const recording = mic.run?.status === "recording";
  const set = (patch: Parameters<typeof setMicOptions>[1]) => setMicOptions(tab.id, patch);
  const refreshDevices = () => void listMicrophones().then(setDevices, () => {});
  useEffect(refreshDevices, [mic.run?.status]);
  const chunk = rawChunkBytes(o.format, o.sampleRate, o.channels, o.chunkMs);
  return (
    <div className="flex flex-col gap-2 px-3 pb-3 pt-1 text-[12px]" data-testid="mic-stream">
      <div className="flex flex-wrap items-center gap-2">
        <Select aria-label="Microphone" value={o.deviceId} disabled={locked} onFocus={refreshDevices} onChange={(e) => set({ deviceId: e.target.value })} className="h-7 w-48 [&_select]:h-7 [&_select]:text-[12px]">
          <option value="">Default microphone</option>
          {devices.map((d) => (
            <option key={d.id} value={d.id}>
              {d.label}
            </option>
          ))}
        </Select>
        <Select
          aria-label="Audio format"
          value={o.format}
          disabled={locked}
          onChange={(e) => {
            const format = e.target.value as AudioFormat;
            const next = formatInfo(format);
            set({ format, ...(next.defaultRate ? { sampleRate: next.defaultRate } : {}), ...(next.mime && o.chunkMs < 20 ? { chunkMs: 100 } : {}) });
          }}
          className="h-7 w-52 [&_select]:h-7 [&_select]:text-[12px]"
        >
          {AUDIO_FORMATS.map((f) => (
            <option key={f.id} value={f.id} disabled={!!f.mime && !encoded.includes(f.id)}>
              {f.label}
              {f.mime && !encoded.includes(f.id) ? " (not available here)" : ""}
            </option>
          ))}
        </Select>
        {raw ? (
          <Select aria-label="Sample rate" value={String(o.sampleRate)} disabled={locked} onChange={(e) => set({ sampleRate: Number(e.target.value) })} className="h-7 w-28 [&_select]:h-7 [&_select]:text-[12px]">
            {SAMPLE_RATES.map((r) => (
              <option key={r} value={r}>
                {r / 1000} kHz
              </option>
            ))}
          </Select>
        ) : (
          <Select aria-label="Bitrate" value={String(o.bitrate)} disabled={locked} onChange={(e) => set({ bitrate: Number(e.target.value) })} className="h-7 w-28 [&_select]:h-7 [&_select]:text-[12px]">
            {BITRATES.map((b) => (
              <option key={b} value={b}>
                {b / 1000} kbps
              </option>
            ))}
          </Select>
        )}
        <Select aria-label="Message length" value={String(o.chunkMs)} disabled={locked} onChange={(e) => set({ chunkMs: Number(e.target.value) })} className="h-7 w-32 [&_select]:h-7 [&_select]:text-[12px]">
          {CHUNK_MS.map((ms) => (
            <option key={ms} value={ms}>
              every {ms} ms
            </option>
          ))}
        </Select>
        {raw && (
          <div className={cx(locked && "pointer-events-none opacity-60")}>
            <Segmented items={[...CHANNEL_ITEMS]} value={String(o.channels) as "1" | "2"} onChange={(c) => set({ channels: Number(c) as 1 | 2 })} />
          </div>
        )}
      </div>
      <div className={cx("flex flex-wrap items-center gap-x-4 gap-y-1.5 text-muted", locked && "pointer-events-none opacity-60")}>
        {raw && (
          <Toggle checked={o.wavHeader} onChange={(wavHeader) => set({ wavHeader })} label="WAV header first" title="Send a 44-byte WAV header before the samples, so the stream plays as a .wav file" />
        )}
        <Toggle checked={o.echoCancellation} onChange={(echoCancellation) => set({ echoCancellation })} label="Echo cancellation" />
        <Toggle checked={o.noiseSuppression} onChange={(noiseSuppression) => set({ noiseSuppression })} label="Noise suppression" />
        <Toggle checked={o.autoGainControl} onChange={(autoGainControl) => set({ autoGainControl })} label="Auto gain" />
      </div>
      <div className="text-faint">
        {raw
          ? `${formatBytes(chunk)} every ${o.chunkMs} ms (${rate((chunk * 1000) / o.chunkMs)}): ${info.label}, ${o.sampleRate / 1000} kHz, ${o.channels === 1 ? "mono" : "stereo, interleaved"}.`
          : `${info.label} at ${o.bitrate / 1000} kbps, about every ${o.chunkMs} ms. The first message carries the container header; later ones only play after it.`}
      </div>
      {mic.run && (
        <div className="flex items-center gap-2" data-testid="mic-status">
          <Mic size={13} className={recording ? "animate-pulse text-danger" : "text-faint"} />
          <div className="h-1.5 w-32 overflow-hidden rounded-full bg-panel-2" aria-label="Input level">
            <div className="h-full rounded-full bg-success transition-[width] duration-100" style={{ width: `${Math.round(Math.sqrt(mic.run.level) * 100)}%` }} />
          </div>
          <span className="tabular-nums text-muted">
            {mic.run.status === "starting" ? "Waiting for the microphone…" : mic.run.status === "stopping" ? "Stopping…" : <Elapsed since={mic.run.startedAt} />}
            {mic.run.messages > 0 && ` · ${formatBytes(mic.run.bytes)} in ${mic.run.messages.toLocaleString()} messages`}
          </span>
        </div>
      )}
      {mic.error && <div className="text-danger">{mic.error}</div>}
    </div>
  );
}

function Toggle({ checked, onChange, label, title }: { checked: boolean; onChange: (v: boolean) => void; label: string; title?: string }) {
  return (
    <label className="flex cursor-pointer items-center gap-1.5" title={title}>
      <Checkbox checked={checked} onChange={onChange} label={label} />
      {label}
    </label>
  );
}

function Elapsed({ since }: { since: number }) {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), 250);
    return () => clearInterval(t);
  }, []);
  const s = Math.max(0, Math.floor((now - since) / 1000));
  return <>{`Recording ${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`}</>;
}

// Ping: round-trip times over ICMP, or TCP connect time when ICMP is not
// allowed. Live replies, a small chart and totals.
import { Gauge, Info, Play, Square } from "lucide-react";
import type { PingMode } from "../../bindings/PingMode";
import type { PingReply } from "../../bindings/PingReply";
import { formatMs } from "../../lib/format";
import { type ToolTab, updateToolState } from "../../store/tabs";
import { PING_DEFAULTS, type PingState, startPing, stopRun, toolState } from "../../store/tools";
import { Button, cx, EmptyState, Input, Segmented, Select } from "../ui";
import { ErrorNote, Hint, LabeledField, StatCard, ToolBar } from "./parts";

const COUNTS = [4, 10, 50, 100, 0];
const INTERVALS = [500, 1000, 2000, 5000];
const MODES: { id: PingMode; label: string }[] = [
  { id: "auto", label: "Auto" },
  { id: "icmp", label: "ICMP" },
  { id: "tcp", label: "TCP" },
];

export function PingTool({ tab }: { tab: ToolTab }) {
  const s = toolState(tab, PING_DEFAULTS);
  const running = s.status === "running";
  const set = (patch: Partial<PingState>) => updateToolState(tab.id, (st) => ({ ...st, ...patch }));
  const hasRun = running || s.started !== null || s.replies.length > 0;

  return (
    <div className="pb-6" data-testid="ping-tool">
      <ToolBar onSubmit={() => void startPing(tab.id)}>
        <LabeledField label="Host" className="min-w-[200px] flex-[2]">
          <Input autoFocus className="font-mono" value={s.host} placeholder="example.com or 192.168.1.1" aria-label="Host" onChange={(e) => set({ host: e.target.value })} />
        </LabeledField>
        <LabeledField label="Using" group>
          <div className="flex h-8 items-center">
            <Segmented items={MODES} value={s.mode} onChange={(mode) => set({ mode })} />
          </div>
        </LabeledField>
        {s.mode !== "icmp" && (
          <LabeledField label={s.mode === "tcp" ? "TCP port" : "TCP port (fallback)"}>
            <Input
              type="number"
              min={1}
              max={65535}
              className="w-24 font-mono"
              aria-label="TCP port"
              value={s.port}
              onChange={(e) => set({ port: Math.min(65535, Math.max(1, Math.trunc(Number(e.target.value)) || 1)) })}
            />
          </LabeledField>
        )}
        <LabeledField label="Count">
          <Select value={String(s.count)} onChange={(e) => set({ count: Number(e.target.value) })} aria-label="Count">
            {COUNTS.map((c) => (
              <option key={c} value={c}>
                {c === 0 ? "Until stopped" : c}
              </option>
            ))}
          </Select>
        </LabeledField>
        <LabeledField label="Every">
          <Select value={String(s.intervalMs)} onChange={(e) => set({ intervalMs: Number(e.target.value) })} aria-label="Interval">
            {INTERVALS.map((i) => (
              <option key={i} value={i}>
                {i < 1000 ? `${i} ms` : `${i / 1000} s`}
              </option>
            ))}
          </Select>
        </LabeledField>
        {running ? (
          <Button icon={<Square size={12} />} onClick={() => void stopRun(tab.id)} className="w-[104px]">
            Stop
          </Button>
        ) : (
          <Button variant="primary" onClick={() => void startPing(tab.id)} icon={<Play size={13} />} className="w-[104px]" data-testid="ping-start">
            Start
          </Button>
        )}
      </ToolBar>

      {s.status === "error" && s.error && <ErrorNote>{s.error}</ErrorNote>}
      {s.started?.note && <Hint icon={<Info size={13} />}>{s.started.note}</Hint>}

      {hasRun ? (
        <Results s={s} running={running} />
      ) : (
        s.status !== "error" && (
          <EmptyState icon={<Gauge size={28} />} title="Measure round-trip times">
            Sends ICMP echo requests like the <code>ping</code> command. When ICMP isn't allowed, Auto measures how long a TCP connection to the port takes
            instead.
          </EmptyState>
        )
      )}
    </div>
  );
}

function Results({ s, running }: { s: PingState; running: boolean }) {
  const { stats, summary, started } = s;
  const loss = stats.sent ? ((stats.sent - stats.received) * 100) / stats.sent : 0;
  const avg = stats.received ? stats.sum / stats.received : null;
  return (
    <>
      <div className="flex items-center gap-2 px-4 pb-3 text-[12px] text-muted">
        {started ? (
          <span>
            {started.mode === "tcp" ? "TCP connect to " : "ICMP echo to "}
            <span className="selectable font-mono text-fg">{started.port != null ? `${started.address}:${started.port}` : started.address}</span>
          </span>
        ) : (
          <span>Resolving…</span>
        )}
        {running && <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-accent" aria-label="running" />}
        {summary?.cancelled && <span>· stopped</span>}
      </div>
      <div className="flex flex-wrap gap-2 px-4 pb-3">
        <StatCard label="Sent" value={stats.sent} />
        <StatCard label="Received" value={stats.received} />
        <StatCard label="Loss" value={`${loss.toFixed(loss > 0 && loss < 10 ? 1 : 0)}%`} tone={loss === 0 ? (stats.sent ? "success" : undefined) : loss < 100 ? "warning" : "danger"} />
        <StatCard label="Min" value={formatMs(stats.min)} />
        <StatCard label="Avg" value={formatMs(avg)} />
        <StatCard label="Max" value={formatMs(stats.max)} />
        {summary?.jitterMs != null && <StatCard label="Jitter" value={formatMs(summary.jitterMs)} />}
      </div>
      {s.replies.length > 0 && <Chart replies={s.replies} />}
      <table className="mx-4 w-[calc(100%-2rem)] text-[12.5px]" data-testid="ping-replies">
        <thead>
          <tr className="border-b border-line text-left text-[11px] text-faint">
            <th className="w-16 pb-1.5 font-medium">#</th>
            <th className="w-28 pb-1.5 font-medium">Time</th>
            {started?.mode !== "tcp" && <th className="w-16 pb-1.5 font-medium">TTL</th>}
            <th className="pb-1.5 font-medium">Result</th>
          </tr>
        </thead>
        <tbody>
          {s.replies
            .slice(-200)
            .reverse()
            .map((r) => (
              <tr key={r.seq} className="border-b border-line/40 last:border-0">
                <td className="py-1 font-mono text-[12px] text-muted">{r.seq}</td>
                <td className="py-1 font-mono text-[12px] tabular-nums text-fg">{r.ms != null ? formatMs(r.ms) : "–"}</td>
                {started?.mode !== "tcp" && <td className="py-1 font-mono text-[12px] text-muted">{r.ttl ?? "–"}</td>}
                <td className={cx("py-1", r.error ? "text-danger" : "text-success")}>{r.error ?? "Reply"}</td>
              </tr>
            ))}
        </tbody>
      </table>
      {s.replies.length > 200 && <p className="px-4 pt-2 text-[11.5px] text-faint">Showing the latest 200 replies; totals cover all of them.</p>}
    </>
  );
}

/** Bars for the latest replies; lost ones are red ticks at full height. */
function Chart({ replies }: { replies: PingReply[] }) {
  const shown = replies.slice(-80);
  const max = Math.max(1, ...shown.map((r) => r.ms ?? 0));
  const W = 640;
  const H = 72;
  const slot = W / 80;
  const bar = Math.max(2, slot - 2);
  return (
    <div className="px-4 pb-3">
      <div className="rounded-lg bg-panel-2 px-2 pb-1 pt-2">
        <svg viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" className="block h-[72px] w-full" role="img" aria-label="Round-trip times">
          {shown.map((r, i) => {
            const x = i * slot + (slot - bar) / 2;
            if (r.ms == null) {
              return (
                <rect key={r.seq} x={x} y={0} width={bar} height={H} fill="var(--danger)" opacity={0.35}>
                  <title>{`#${r.seq}: ${r.error ?? "no reply"}`}</title>
                </rect>
              );
            }
            const h = Math.max(2, (r.ms / max) * (H - 4));
            return (
              <rect key={r.seq} x={x} y={H - h} width={bar} height={h} rx={1} fill="var(--accent)">
                <title>{`#${r.seq}: ${formatMs(r.ms)}`}</title>
              </rect>
            );
          })}
        </svg>
        <div className="flex justify-between pt-0.5 text-[10.5px] text-faint">
          <span>last {shown.length}</span>
          <span>max {formatMs(max)}</span>
        </div>
      </div>
    </div>
  );
}

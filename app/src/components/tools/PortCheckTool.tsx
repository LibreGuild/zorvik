// Port check: which TCP ports on a host accept connections. Results stream in;
// open ports are listed first.
import { useMemo } from "react";
import { Cable, Play, ShieldAlert, Square } from "lucide-react";
import type { PortResult } from "../../bindings/PortResult";
import { formatMs } from "../../lib/format";
import { type ToolTab, updateToolState } from "../../store/tabs";
import { countPorts, isLocalHost, PORT_DEFAULTS, PORT_PRESETS, type PortPreset, portSpec, startPortCheck, stopRun, toolState } from "../../store/tools";
import { Badge, Button, cx, EmptyState, Input, Segmented, Select, Tooltip } from "../ui";
import { ErrorNote, Hint, LabeledField, StatCard, ToolBar } from "./parts";

/** Well-known services, to label results. */
const SERVICES: Record<number, string> = {
  20: "FTP data", 21: "FTP", 22: "SSH", 23: "Telnet", 25: "SMTP", 53: "DNS", 80: "HTTP", 110: "POP3", 111: "RPC", 135: "MS RPC",
  139: "NetBIOS", 143: "IMAP", 389: "LDAP", 443: "HTTPS", 445: "SMB", 465: "SMTPS", 587: "SMTP submission", 636: "LDAPS",
  993: "IMAPS", 995: "POP3S", 1433: "SQL Server", 1521: "Oracle", 1723: "PPTP", 1883: "MQTT", 2375: "Docker", 2376: "Docker TLS",
  3000: "Dev server", 3306: "MySQL", 3389: "RDP", 5000: "Dev server", 5173: "Vite", 5432: "PostgreSQL", 5672: "AMQP",
  5900: "VNC", 6379: "Redis", 8000: "HTTP alt", 8080: "HTTP alt", 8443: "HTTPS alt", 8883: "MQTT TLS", 9000: "HTTP alt",
  9092: "Kafka", 9200: "Elasticsearch", 11211: "Memcached", 27017: "MongoDB",
};

const TIMEOUTS = [500, 1000, 2000, 5000];

function sortResults(results: PortResult[]): PortResult[] {
  return [...results].sort((a, b) => Number(b.open) - Number(a.open) || a.port - b.port);
}

export function PortCheckTool({ tab }: { tab: ToolTab }) {
  const s = toolState(tab, PORT_DEFAULTS);
  const running = s.status === "running";
  const set = (patch: Partial<typeof s>) => updateToolState(tab.id, (st) => ({ ...st, ...patch }));
  const spec = portSpec(s);
  const count = countPorts(spec);
  const sorted = useMemo(() => sortResults(s.results), [s.results]);
  const counts = useMemo(() => {
    const c = { open: 0, refused: 0, timeout: 0, other: 0 };
    for (const r of s.results) {
      if (r.open) c.open++;
      else if (r.error === "refused") c.refused++;
      else if (r.error === "timeout") c.timeout++;
      else c.other++;
    }
    return c;
  }, [s.results]);
  const manyOnPublic = typeof count === "number" && count > 100 && s.host.trim() !== "" && !isLocalHost(s.host);
  const done = s.results.length;
  const total = s.total || (typeof count === "number" ? count : 0);

  return (
    <div className="pb-6" data-testid="port-check">
      <ToolBar onSubmit={() => void startPortCheck(tab.id)}>
        <LabeledField label="Host" className="min-w-[200px] flex-[2]">
          <Input autoFocus className="font-mono" value={s.host} placeholder="192.168.1.20 or example.com" aria-label="Host" onChange={(e) => set({ host: e.target.value })} />
        </LabeledField>
        <LabeledField label="Ports" group>
          <div className="flex h-8 items-center">
            <Segmented<PortPreset> items={PORT_PRESETS.map((p) => ({ id: p.id, label: p.label }))} value={s.preset} onChange={(preset) => set({ preset })} />
          </div>
        </LabeledField>
        {s.preset === "custom" && (
          <LabeledField label="Custom ports" className="min-w-[160px] flex-1">
            <Input
              className="font-mono"
              value={s.custom}
              invalid={s.custom.trim() !== "" && typeof count === "string"}
              placeholder="22,80,8000-8100"
              aria-label="Custom ports"
              onChange={(e) => set({ custom: e.target.value })}
            />
          </LabeledField>
        )}
        <LabeledField label="Timeout">
          <Select value={String(s.timeoutMs)} onChange={(e) => set({ timeoutMs: Number(e.target.value) })} aria-label="Timeout per port">
            {TIMEOUTS.map((t) => (
              <option key={t} value={t}>
                {t < 1000 ? `${t} ms` : `${t / 1000} s`}
              </option>
            ))}
          </Select>
        </LabeledField>
        {running ? (
          <Button icon={<Square size={12} />} onClick={() => void stopRun(tab.id)} className="w-[104px]">
            Stop
          </Button>
        ) : (
          <Button variant="primary" onClick={() => void startPortCheck(tab.id)} icon={<Play size={13} />} className="w-[104px]" data-testid="port-check-start">
            Check
          </Button>
        )}
      </ToolBar>

      {s.preset === "custom" && typeof count === "string" && s.custom.trim() !== "" && <p className="-mt-1 px-4 pb-2 text-[11.5px] text-danger">{count}</p>}
      {manyOnPublic && <Hint icon={<ShieldAlert size={13} />}>Only scan hosts you're allowed to test. Scanning other people's servers can be seen as an attack.</Hint>}
      {s.status === "error" && s.error && <ErrorNote>{s.error}</ErrorNote>}

      {(running || s.summary || done > 0) && (
        <>
          <div className="px-4 pb-3">
            <div className="flex items-baseline gap-2 pb-1.5 text-[12px] text-muted">
              <span>
                {s.address ? (
                  <>
                    Checking <span className="selectable font-mono text-fg">{s.address}</span>
                  </>
                ) : (
                  "Resolving…"
                )}
              </span>
              <span className="flex-1" />
              <span className="tabular-nums">
                {done} / {total}
                {s.summary && ` · ${formatMs(s.summary.durationMs)}`}
                {s.summary?.cancelled && " · stopped"}
              </span>
            </div>
            <div className="h-1 overflow-hidden rounded-full bg-panel-2">
              <div className={cx("h-full rounded-full transition-[width]", s.summary ? "bg-success/70" : "bg-accent")} style={{ width: `${total ? (done / total) * 100 : 0}%` }} />
            </div>
          </div>
          <div className="flex flex-wrap gap-2 px-4 pb-3">
            <StatCard label="Open" value={counts.open} tone={counts.open > 0 ? "success" : undefined} />
            <StatCard label="Closed" value={counts.refused} />
            <StatCard label="No answer" value={counts.timeout} tone={counts.timeout > 0 ? "warning" : undefined} />
            {counts.other > 0 && <StatCard label="Errors" value={counts.other} tone="danger" />}
          </div>
          <table className="mx-4 w-[calc(100%-2rem)] text-[12.5px]" data-testid="port-results">
            <thead>
              <tr className="border-b border-line text-left text-[11px] text-faint">
                <th className="w-20 pb-1.5 font-medium">Port</th>
                <th className="pb-1.5 font-medium">Service</th>
                <th className="pb-1.5 font-medium">Status</th>
                <th className="w-24 pb-1.5 text-right font-medium">Time</th>
              </tr>
            </thead>
            <tbody>
              {sorted.map((r) => (
                <tr key={r.port} className="border-b border-line/40 last:border-0">
                  <td className="selectable py-1 font-mono text-[12px] text-fg">{r.port}</td>
                  <td className="py-1 text-muted">{SERVICES[r.port] ?? ""}</td>
                  <td className="py-1">
                    <PortStatus result={r} />
                  </td>
                  <td className="py-1 text-right font-mono text-[12px] tabular-nums text-muted">{formatMs(r.ms)}</td>
                </tr>
              ))}
            </tbody>
          </table>
          {s.summary && s.summary.open.length === 0 && done > 0 && <p className="px-4 pt-3 text-[12px] text-muted">No open ports found.</p>}
        </>
      )}

      {!running && !s.summary && done === 0 && s.status !== "error" && (
        <EmptyState icon={<Cable size={28} />} title="Check which ports are open">
          Connects to each TCP port (up to 1024 per check, 64 at a time). <b>Closed</b> means the host refused; <b>no answer</b> usually means a firewall
          dropped the connection.
        </EmptyState>
      )}
    </div>
  );
}

function PortStatus({ result }: { result: PortResult }) {
  if (result.open) return <Badge className="bg-success/12 text-success">Open</Badge>;
  if (result.error === "refused") return <span className="text-muted">Closed</span>;
  if (result.error === "timeout") return <span className="text-warning">No answer</span>;
  return (
    <Tooltip content={result.message}>
      <span className="text-danger">Error{result.message ? `: ${result.message}` : ""}</span>
    </Tooltip>
  );
}

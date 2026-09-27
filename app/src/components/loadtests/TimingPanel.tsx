// Where the requests' time went: connect, time to first byte (server plus one
// network round trip), transfer, and what servers reported in Server-Timing.
import type { MetricsSummary } from "../../bindings/MetricsSummary";
import type { PhaseSummary } from "../../bindings/PhaseSummary";
import { cx } from "../ui";
import { formatCount, formatLatencyNumber, formatPercent, newConnectionShare } from "./model";
import { Panel } from "./parts";

const PHASES: { key: "connect" | "ttfb" | "transfer" | "server"; label: string; hint: string }[] = [
  { key: "ttfb", label: "Time to first byte", hint: "Request sent to first byte: the server's time plus one network round trip" },
  { key: "server", label: "Server-reported", hint: "From the Server-Timing header: its total, or the sum of its durations" },
  { key: "transfer", label: "Transfer", hint: "First byte to last byte of the response" },
  { key: "connect", label: "Connect", hint: "DNS, TCP and TLS, only for requests that opened a new connection" },
];

export function TimingPanel({ totals }: { totals: MetricsSummary }) {
  const timing = totals.timing;
  const th = "whitespace-nowrap px-1.5 py-1.5 text-right font-medium";
  const td = "whitespace-nowrap px-1.5 py-1.5 text-right font-mono tabular-nums";
  // Server-reported only when a response had the header: no claim about pure server time without it.
  const rows = PHASES.filter((p) => p.key !== "server" || timing.server.count > 0);
  return (
    <Panel
      title="Timing"
      testId="load-timing"
      right={<span className="text-[11.5px] text-muted">{`New connection for ${formatPercent(newConnectionShare(totals))} of requests`}</span>}
    >
      <div className="-mx-1 overflow-x-auto">
        <table className="w-full border-collapse text-[12px]">
          <thead>
            <tr className="border-b border-line text-[11px] text-faint">
              <th className="px-2 py-1.5 text-left font-medium">Phase</th>
              <th className={th}>Requests</th>
              <th className={th}>p50 ms</th>
              <th className={th}>p95 ms</th>
              <th className={th}>p99 ms</th>
              <th className={th}>max ms</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((p) => {
              const s: PhaseSummary = timing[p.key];
              const none = s.count === 0;
              return (
                <tr key={p.key} className="border-b border-line/50 last:border-b-0" data-testid={`timing-${p.key}`}>
                  <td className="px-2 py-1.5">
                    <div className="text-fg" title={p.hint}>
                      {p.label}
                    </div>
                  </td>
                  <td className={cx(td, "text-muted")}>{formatCount(s.count)}</td>
                  <td className={cx(td, "text-fg")}>{none ? "–" : formatLatencyNumber(s.p50)}</td>
                  <td className={cx(td, "font-semibold text-fg")}>{none ? "–" : formatLatencyNumber(s.p95)}</td>
                  <td className={cx(td, "text-fg")}>{none ? "–" : formatLatencyNumber(s.p99)}</td>
                  <td className={cx(td, "text-muted")}>{none ? "–" : formatLatencyNumber(s.max)}</td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
      <p className="mt-2 text-[11.5px] leading-snug text-faint">
        {timing.server.count > 0
          ? "Time to first byte is the server's time plus the network; server-reported is what the server says it spent (Server-Timing)."
          : "Time to first byte is the server's time plus one network round trip. To see the server's own time, have it send a Server-Timing header."}
      </p>
    </Panel>
  );
}

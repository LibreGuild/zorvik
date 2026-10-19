// TCP relay settings: the server every client connection is relayed to, and
// faults (delay, bandwidth limit, dropped connections) for resilience tests.
import { useState } from "react";
import { ChevronRight, Copy } from "lucide-react";
import type { TcpProxyConfig } from "../../bindings/TcpProxyConfig";
import { copyText } from "../../lib/platform";
import { cx, Field, Input, NumberInput, Switch, Tooltip } from "../ui";
import type { ServerEditorProps } from "./kinds";
import { EditorSection } from "./ServerView";

/** Why a target cannot be used (empty is fine while typing). */
function targetProblem(target: string): string | undefined {
  const t = target.trim().replace(/^(tcp|tls):\/\//i, "");
  if (!t) return undefined;
  if (/\s/.test(t) || t.includes("/")) return "Enter host:port, e.g. 127.0.0.1:5432";
  const port = t.match(/:(\d+)$/)?.[1];
  if (!port || Number(port) < 1 || Number(port) > 65535) return "Add the port, e.g. localhost:5432";
  if (t.split(":").length > 2 && !t.startsWith("[")) return "Put IPv6 addresses in brackets, e.g. [::1]:5432";
  return undefined;
}

type FaultField = "delayMs" | "bytesPerSecond" | "dropAfterMs" | "dropAfterBytes";

const FAULT_FIELDS: { id: FaultField; label: string; hint: string }[] = [
  { id: "delayMs", label: "Delay per chunk (ms)", hint: "Each chunk of data waits this long, both ways." },
  { id: "bytesPerSecond", label: "Bandwidth (bytes/s)", hint: "Most bytes a second in each direction." },
  { id: "dropAfterMs", label: "Drop after (ms)", hint: "Closes both connections this long after the target connected." },
  { id: "dropAfterBytes", label: "Drop after (bytes)", hint: "Closes both connections once this many bytes went through." },
];

export function RelayEditor({ server, onChange, running }: ServerEditorProps) {
  const proxy: TcpProxyConfig = server.proxy ?? { target: "" };
  const set = (patch: Partial<TcpProxyConfig>) => onChange((s) => ({ ...s, proxy: { ...(s.proxy ?? { target: "" }), ...patch } }));
  const problem = targetProblem(proxy.target);
  const address = running?.url.replace(/^[a-z]+:\/\//, "");
  const faults = FAULT_FIELDS.filter((f) => (proxy[f.id] ?? 0) > 0).length;
  const [showFaults, setShowFaults] = useState(faults > 0);

  return (
    <>
      <EditorSection title="Target">
        <Field label="Target server" hint={problem ?? "Each client connection is relayed to this host:port. Changes apply to new connections."}>
          <Input
            className="font-mono"
            aria-label="Target server"
            invalid={!!problem}
            value={proxy.target}
            placeholder="127.0.0.1:5432"
            onChange={(e) => set({ target: e.target.value })}
          />
        </Field>
        <div className="flex flex-col gap-1">
          <Switch checked={proxy.upstreamTls ?? false} onChange={(upstreamTls) => set({ upstreamTls })} label="Connect to the target with TLS" />
          <p className="pl-[46px] text-[11.5px] text-faint">
            Clients still talk plain TCP to the relay, so you see the decrypted traffic. The certificate is checked unless “Verify TLS certificates” is off in Settings.
          </p>
        </div>
      </EditorSection>

      <section className="flex flex-col gap-2.5 px-4 py-3">
        <button
          aria-expanded={showFaults}
          onClick={() => setShowFaults(!showFaults)}
          className="flex items-center gap-1 self-start text-[11px] font-semibold uppercase tracking-wide text-faint hover:text-fg"
        >
          <ChevronRight size={13} className={cx("transition-transform", showFaults && "rotate-90")} />
          Faults
          {faults > 0 && <span className="ml-1 rounded-full bg-hover px-1.5 text-[10.5px] normal-case tracking-normal text-muted">{faults}</span>}
        </button>
        {showFaults && (
          <>
            <p className="text-[12px] text-muted">Test how a client copes with a slow or unreliable network. 0 turns a fault off; changes apply to new connections.</p>
            <div className="grid grid-cols-2 items-start gap-3">
              {FAULT_FIELDS.map((f) => (
                <Field key={f.id} label={f.label} hint={f.hint}>
                  <NumberInput aria-label={f.label} value={proxy[f.id] ?? 0} onChange={(v) => set({ [f.id]: v || undefined })} />
                </Field>
              ))}
            </div>
          </>
        )}
      </section>

      <EditorSection title="How to use">
        {running && address ? (
          <div className="flex flex-col gap-1.5">
            <p className="text-[12px] text-muted">Point your client at this address instead of the target; traffic in both directions shows on the right.</p>
            <div className="flex items-center gap-2 rounded-lg border border-line bg-panel-2/60 px-2.5 py-1.5">
              <code className="selectable min-w-0 flex-1 truncate font-mono text-[12px] text-fg">{address}</code>
              <Tooltip content="Copy address">
                <button aria-label="Copy relay address" onClick={() => void copyText(address)} className="rounded p-0.5 text-faint hover:bg-hover hover:text-fg">
                  <Copy size={12} />
                </button>
              </Tooltip>
            </div>
          </div>
        ) : (
          <p className="text-[12px] text-muted">Start the relay, then point your client at its address instead of the target. Traffic in both directions shows on the right.</p>
        )}
      </EditorSection>
    </>
  );
}

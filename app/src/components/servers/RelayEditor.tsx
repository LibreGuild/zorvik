// TCP relay settings: the server every client connection is relayed to.
import { Copy } from "lucide-react";
import type { TcpProxyConfig } from "../../bindings/TcpProxyConfig";
import { copyText } from "../../lib/platform";
import { Field, Input, Switch, Tooltip } from "../ui";
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

export function RelayEditor({ server, onChange, running }: ServerEditorProps) {
  const proxy: TcpProxyConfig = server.proxy ?? { target: "" };
  const set = (patch: Partial<TcpProxyConfig>) => onChange((s) => ({ ...s, proxy: { ...(s.proxy ?? { target: "" }), ...patch } }));
  const problem = targetProblem(proxy.target);
  const address = running?.url.replace(/^[a-z]+:\/\//, "");

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

// Resolver settings of a DNS request (and the DNS lookup tool): which server
// answers, recursion, timeout.
import { useState } from "react";
import type { Request } from "../../bindings/Request";
import { updateDraft } from "../../store/tabs";
import { VarInput } from "../VarInput";
import { cx, Field, Input, Switch } from "../ui";
import { presetFor, RESOLVER_PRESETS, resolverProblem, resolverProtocol } from "./dnsModel";
import type { KindPaneProps } from "./kinds";

const CUSTOM_HINT = "An IP or host (UDP, port 53), or tcp://host, tls://host (DNS over TLS, port 853), https://host/dns-query (DNS over HTTPS).";

/** Preset buttons plus a custom address field. */
export function ResolverField({ server, onChange, onEnter }: { server: string; onChange: (server: string) => void; onEnter?: () => void }) {
  const preset = presetFor(server);
  // "Custom" stays open while the typed value happens to match a preset (e.g. typing 1.1.1.1).
  const [custom, setCustom] = useState(preset === "custom");
  const selected = custom ? "custom" : preset;
  const protocol = resolverProtocol(server);
  const problem = selected === "custom" ? resolverProblem(server) : null;
  const hint = selected === "custom" ? CUSTOM_HINT : RESOLVER_PRESETS.find((p) => p.id === selected)?.hint;
  const chip = (active: boolean) =>
    cx(
      "h-7 rounded-lg border px-2.5 text-[12px] font-medium transition-colors",
      active ? "border-accent bg-accent-soft text-accent" : "border-line bg-panel-2 text-muted hover:border-line-strong hover:text-fg",
    );
  return (
    <div className="flex flex-col gap-2">
      <div className="flex flex-wrap gap-1.5" role="radiogroup" aria-label="Resolver">
        {RESOLVER_PRESETS.map((p) => (
          <button
            key={p.id}
            role="radio"
            aria-checked={selected === p.id}
            className={chip(selected === p.id)}
            onClick={() => {
              setCustom(false);
              onChange(p.server);
            }}
          >
            {p.label}
          </button>
        ))}
        <button role="radio" aria-checked={selected === "custom"} className={chip(selected === "custom")} onClick={() => setCustom(true)}>
          Custom…
        </button>
      </div>
      {selected === "custom" && (
        <div className="flex items-center gap-2">
          <div className={cx("min-w-0 flex-1 rounded-lg border bg-input focus-within:border-accent", problem ? "border-danger" : "border-line")}>
            <VarInput
              value={server}
              onChange={onChange}
              onEnter={onEnter}
              autoFocus={!server}
              placeholder="192.168.1.1 · tls://dns.example · https://dns.example/dns-query"
              ariaLabel="DNS server"
            />
          </div>
          {protocol && protocol !== "System" && !problem && (
            <span className="shrink-0 rounded bg-hover px-1.5 py-px text-[11px] font-semibold text-muted">{protocol}</span>
          )}
        </div>
      )}
      <div className={cx("text-[11.5px]", problem ? "text-danger" : "text-faint")}>{problem ?? hint}</div>
    </div>
  );
}

/** Empty = the 5 s default; otherwise whole milliseconds. */
const timeoutValue = (v: string): number | undefined => (v.trim() === "" ? undefined : Math.max(0, Math.trunc(Number(v)) || 0));

export function DnsOptions({ tab }: KindPaneProps) {
  const req = tab.draft;
  const dns = req.dns ?? {};
  const update = (fn: (r: Request) => Request) => updateDraft(tab.id, fn);
  return (
    <div className="flex max-w-2xl flex-col gap-4 p-4">
      <p className="text-[12.5px] text-muted">
        Type the name in the address field (an IP address for PTR) and pick the record type left of it. NXDOMAIN and other response codes are shown
        as answers.
      </p>
      {/* Not a Field: its <label> would forward clicks on the caption to the first preset button. */}
      <div className="flex flex-col gap-1.5">
        <span className="text-[12px] font-medium text-muted">Resolver</span>
        <ResolverField server={dns.server ?? ""} onChange={(server) => update((r) => ({ ...r, dns: { ...(r.dns ?? {}), server } }))} />
      </div>
      <Switch
        checked={dns.recursion ?? true}
        onChange={(recursion) => update((r) => ({ ...r, dns: { ...(r.dns ?? {}), recursion } }))}
        label="Ask for recursion (RD)"
      />
      <p className="-mt-2 pl-[46px] text-[11.5px] text-faint">Turn off to see only what the server itself knows, e.g. when asking an authoritative server.</p>
      <Field label="Timeout (ms)" hint="Whole query, including a retry over TCP. Empty = 5000.">
        <Input
          type="number"
          min={0}
          className="w-44"
          placeholder="5000"
          value={req.settings?.timeoutMs ?? ""}
          onChange={(e) =>
            update((r) => {
              const settings = { ...(r.settings ?? {}), timeoutMs: timeoutValue(e.target.value) };
              if (settings.timeoutMs === undefined) delete settings.timeoutMs;
              return { ...r, settings };
            })
          }
        />
      </Field>
    </div>
  );
}

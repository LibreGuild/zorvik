// DNS server settings: the records it answers with, where other names go, and how to try it.
import { useState } from "react";
import { Copy, Plus, Trash2 } from "lucide-react";
import type { DnsRecord } from "../../bindings/DnsRecord";
import type { DnsServerConfig } from "../../bindings/DnsServerConfig";
import { copyText } from "../../lib/platform";
import { Button, Checkbox, cx, Field, Input, Select, Tooltip } from "../ui";
import type { ServerEditorProps } from "./kinds";
import { EditorSection } from "./ServerView";

const TYPES = ["A", "AAAA", "CNAME", "TXT", "MX", "NS", "PTR", "SRV", "CAA"] as const;

const VALUE_HINTS: Record<string, string> = {
  A: "127.0.0.1",
  AAAA: "::1",
  CNAME: "api.example.test",
  TXT: "v=spf1 -all",
  MX: "10 mail.example.test",
  NS: "ns1.example.test",
  PTR: "api.example.test",
  SRV: "10 5 5060 sip.example.test",
  CAA: "0 issue letsencrypt.org",
};

/** Obvious mistakes in a value (the server reports the rest when it starts). */
const VALUE_CHECKS: Record<string, { test: RegExp; hint: string }> = {
  A: { test: /^(25[0-5]|2[0-4]\d|1?\d?\d)(\.(25[0-5]|2[0-4]\d|1?\d?\d)){3}$/, hint: "An IPv4 address, e.g. 127.0.0.1" },
  AAAA: { test: /^[0-9a-f:.]*:[0-9a-f:.]*$/i, hint: "An IPv6 address, e.g. ::1" },
  MX: { test: /^\d{1,5}\s+\S+$/, hint: "Preference and host, e.g. 10 mail.example.test" },
  SRV: { test: /^\d{1,5}\s+\d{1,5}\s+\d{1,5}\s+\S+$/, hint: "Priority, weight, port and target, e.g. 10 5 5060 sip.example.test" },
  CAA: { test: /^\d{1,3}\s+[a-z0-9]{1,15}\s+\S.*$/i, hint: "Flags, tag and value, e.g. 0 issue letsencrypt.org" },
};

function valueProblem(type: string, value: string): string | undefined {
  const v = value.trim();
  const check = VALUE_CHECKS[type];
  if (!v || !check) return undefined;
  return check.test.test(v) ? undefined : check.hint;
}

function nameProblem(name: string): string | undefined {
  const n = name.trim();
  if (!n) return undefined;
  if (/\s/.test(n)) return "Names cannot contain spaces";
  if (n.includes("*") && n !== "*" && !/^\*\.[^*]+$/.test(n)) return "A wildcard must be the first label, e.g. *.example.test";
  return undefined;
}

const cell = "h-8 w-full min-w-0 bg-transparent px-2 font-mono text-[12px] text-fg outline-none placeholder:text-faint";

type UpstreamChoice = "none" | "system" | "server";

const UPSTREAMS: { id: UpstreamChoice; label: string; hint: string }[] = [
  { id: "none", label: "Answer NXDOMAIN (no forwarding)", hint: "Names without a record get “no such name”." },
  { id: "system", label: "This computer's resolver", hint: "Addresses (A, AAAA) come from this computer's DNS settings; other types are not available." },
  { id: "server", label: "Another server…", hint: "Questions without a record are forwarded, e.g. to 1.1.1.1 or 192.168.1.1:53." },
];

/** Host and port of a running server's address (`dns://127.0.0.1:1053`). */
function splitAddress(url: string): { host: string; port: string } {
  const rest = url.replace(/^[a-z]+:\/\//, "");
  const i = rest.lastIndexOf(":");
  return { host: rest.slice(0, i).replace(/^\[|\]$/g, ""), port: rest.slice(i + 1) };
}

/** A name worth asking for in the example commands. */
function exampleName(records: DnsRecord[]): string {
  const r = records.find((r) => r.enabled !== false && r.name.trim() && (r.type === "A" || r.type === "AAAA")) ?? records.find((r) => r.name.trim());
  const name = r?.name.trim().replace(/\.$/, "") ?? "";
  if (!name || name === "*") return "api.example.test";
  return name.startsWith("*.") ? `test.${name.slice(2)}` : name;
}

export function DnsServerEditor({ server, onChange, running }: ServerEditorProps) {
  const dns: DnsServerConfig = server.dns ?? {};
  const records = dns.records ?? [];
  const upstream = dns.upstream ?? "";
  const set = (patch: Partial<DnsServerConfig>) => onChange((s) => ({ ...s, dns: { ...(s.dns ?? {}), ...patch } }));
  const setRecords = (fn: (records: DnsRecord[]) => DnsRecord[]) =>
    onChange((s) => ({ ...s, dns: { ...(s.dns ?? {}), records: fn(s.dns?.records ?? []) } }));
  const update = (i: number, patch: Partial<DnsRecord>) => setRecords((rs) => rs.map((r, j) => (j === i ? { ...r, ...patch } : r)));

  // "Another server…" with nothing typed yet still shows the address field.
  const [editingServer, setEditingServer] = useState(false);
  const choice: UpstreamChoice =
    upstream.trim().toLowerCase() === "system" ? "system" : upstream.trim() || editingServer ? "server" : "none";
  const chooseUpstream = (next: UpstreamChoice) => {
    setEditingServer(next === "server");
    if (next === "none") set({ upstream: "" });
    else if (next === "system") set({ upstream: "system" });
    else if (choice !== "server") set({ upstream: "" });
  };

  return (
    <>
      <EditorSection title="Records">
        {records.length > 0 && (
          <div className="overflow-hidden rounded-lg border border-line text-[12.5px]" role="table" aria-label="DNS records">
            <div role="row" className="grid grid-cols-[28px_minmax(0,1.6fr)_84px_minmax(0,1fr)_64px_32px] items-center border-b border-line bg-panel-2/60 text-[11px] font-medium text-faint">
              <span role="columnheader" />
              <span role="columnheader" className="px-2">Name</span>
              <span role="columnheader" className="px-2">Type</span>
              <span role="columnheader" className="px-2">Value</span>
              <span role="columnheader" className="px-2">TTL s</span>
              <span role="columnheader" />
            </div>
            {records.map((record, i) => {
              const enabled = record.enabled !== false;
              const badName = nameProblem(record.name);
              const badValue = valueProblem(record.type, record.value);
              return (
                <div
                  key={i}
                  role="row"
                  className={cx("grid grid-cols-[28px_minmax(0,1.6fr)_84px_minmax(0,1fr)_64px_32px] items-center border-b border-line/70 last:border-b-0", !enabled && "opacity-50")}
                >
                  <span role="cell" className="flex justify-center">
                    <Checkbox checked={enabled} onChange={(v) => update(i, { enabled: v })} title="Use this record" />
                  </span>
                  <span role="cell" className="border-l border-line/70" title={badName}>
                    <input
                      aria-label="Name"
                      aria-invalid={!!badName}
                      className={cx(cell, badName && "text-danger")}
                      value={record.name}
                      placeholder={record.type === "PTR" ? "127.0.0.1" : "api.example.test"}
                      onChange={(e) => update(i, { name: e.target.value })}
                    />
                  </span>
                  <span role="cell" className="border-l border-line/70">
                    <select
                      aria-label="Type"
                      value={record.type}
                      onChange={(e) => update(i, { type: e.target.value })}
                      className="h-8 w-full bg-transparent px-1.5 text-[12px] text-fg outline-none"
                    >
                      {!TYPES.includes(record.type as (typeof TYPES)[number]) && <option value={record.type}>{record.type || "?"}</option>}
                      {TYPES.map((t) => (
                        <option key={t} value={t}>
                          {t}
                        </option>
                      ))}
                    </select>
                  </span>
                  <span role="cell" className="border-l border-line/70" title={badValue}>
                    <input
                      aria-label="Value"
                      aria-invalid={!!badValue}
                      className={cx(cell, badValue && "text-danger")}
                      value={record.value}
                      placeholder={VALUE_HINTS[record.type] ?? ""}
                      onChange={(e) => update(i, { value: e.target.value })}
                    />
                  </span>
                  <span role="cell" className="border-l border-line/70">
                    <input
                      aria-label="TTL in seconds"
                      type="number"
                      min={0}
                      className={cell}
                      value={record.ttl}
                      onChange={(e) => update(i, { ttl: Math.min(2147483647, Math.max(0, Math.trunc(Number(e.target.value)) || 0)) })}
                    />
                  </span>
                  <span role="cell" className="flex justify-center border-l border-line/70">
                    <button aria-label="Remove record" onClick={() => setRecords((rs) => rs.filter((_, j) => j !== i))} className="rounded p-1 text-faint hover:bg-hover hover:text-danger">
                      <Trash2 size={13} />
                    </button>
                  </span>
                </div>
              );
            })}
          </div>
        )}
        <div className="flex items-center gap-3">
          <Button size="sm" icon={<Plus size={13} />} onClick={() => setRecords((rs) => [...rs, { name: "", type: "A", value: "", ttl: 60 }])}>
            Add record
          </Button>
          <span className="text-[11.5px] text-faint">
            Names match in any case; <code className="font-mono">*.example.test</code> covers every name below it. CNAMEs are followed within these records.
          </span>
        </div>
      </EditorSection>

      <EditorSection title="Other names">
        <Field label="Names without a record" hint={UPSTREAMS.find((u) => u.id === choice)?.hint}>
          <div className="flex flex-wrap gap-2">
            <Select value={choice} aria-label="Names without a record" onChange={(e) => chooseUpstream(e.target.value as UpstreamChoice)} className="w-64">
              {UPSTREAMS.map((u) => (
                <option key={u.id} value={u.id}>
                  {u.label}
                </option>
              ))}
            </Select>
            {choice === "server" && (
              <Input
                className="w-56 font-mono"
                aria-label="Upstream DNS server"
                autoFocus={editingServer && !upstream}
                value={upstream}
                placeholder="1.1.1.1 or [2606:4700::1111]:53"
                onChange={(e) => set({ upstream: e.target.value })}
              />
            )}
          </div>
        </Field>
      </EditorSection>

      <EditorSection title="Try it">
        {running ? <TryCommands url={running.url} name={exampleName(records)} /> : <p className="text-[12px] text-muted">Start the server to get commands that query it.</p>}
      </EditorSection>
    </>
  );
}

function TryCommands({ url, name }: { url: string; name: string }) {
  const { host, port } = splitAddress(url);
  const commands = [
    { label: "macOS / Linux", command: `dig @${host} -p ${port} ${name}` },
    { label: "Windows", command: `nslookup -port=${port} ${name} ${host}` },
  ];
  return (
    <div className="flex flex-col gap-2">
      {commands.map((c) => (
        <div key={c.label} className="flex flex-col gap-1">
          <span className="text-[11.5px] font-medium text-muted">{c.label}</span>
          <div className="flex items-center gap-2 rounded-lg border border-line bg-panel-2/60 px-2.5 py-1.5">
            <code className="selectable min-w-0 flex-1 truncate font-mono text-[12px] text-fg">{c.command}</code>
            <Tooltip content="Copy command">
              <button aria-label={`Copy ${c.label} command`} onClick={() => void copyText(c.command)} className="rounded p-0.5 text-faint hover:bg-hover hover:text-fg">
                <Copy size={12} />
              </button>
            </Tooltip>
          </div>
        </div>
      ))}
      <p className="text-[11.5px] text-faint">Answers show on the right. Add <code className="font-mono">+tcp</code> to dig to ask over TCP.</p>
    </div>
  );
}

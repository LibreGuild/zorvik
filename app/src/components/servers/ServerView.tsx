// A server tab: listener settings, the kind's configuration and live traffic.
import { useEffect, useMemo, useRef, useState } from "react";
import { Copy, Loader2, Play, RotateCw, Save, Square } from "lucide-react";
import type { Server } from "../../bindings/Server";
import { copyText, modKey } from "../../lib/platform";
import { applyToRunning, restartServer, runningFor, serverKey, startServer, stopServer, useServers } from "../../store/servers";
import { isDirty, saveTab, type ServerTab, updateServerDraft } from "../../store/tabs";
import { useUi } from "../../store/ui";
import { useWorkspace } from "../../store/workspace";
import { Splitter } from "../Splitter";
import { Banner, Button, cx, IconButton, Input, Switch, Tooltip } from "../ui";
import { SERVER_KINDS } from "./kinds";
import { TrafficPanel } from "./TrafficPanel";

export function EditorSection({ title, children, right }: { title: string; children: React.ReactNode; right?: React.ReactNode }) {
  return (
    <section className="flex flex-col gap-2.5 px-4 py-3">
      <div className="flex items-center justify-between">
        <h3 className="text-[11px] font-semibold uppercase tracking-wide text-faint">{title}</h3>
        {right}
      </div>
      {children}
    </section>
  );
}

const HOSTS = [
  { id: "127.0.0.1", label: "This computer only (127.0.0.1)" },
  { id: "0.0.0.0", label: "Other devices too (0.0.0.0)" },
];

export function ServerView({ tab }: { tab: ServerTab }) {
  const server = tab.draft;
  const info = SERVER_KINDS[server.kind];
  const wsPath = useWorkspace((s) => s.info?.path ?? "");
  const running = useServers((s) => runningFor(tab.serverId, s.running));
  const busy = useServers((s) => !!s.busy[serverKey(wsPath, tab.serverId)]);
  const split = useUi((s) => s.serverSplit);
  const area = useRef<HTMLDivElement>(null);
  const [restartNeeded, setRestartNeeded] = useState(false);
  // Not on every stats update (4×/s): a big mock is costly to compare.
  const dirty = useMemo(() => isDirty(tab), [tab]);
  const change = (fn: (s: Server) => Server) => updateServerDraft(tab.id, fn);

  // Edits apply to the running server as you type (except listener changes, which need a restart).
  const runId = running?.runId;
  const pendingApply = useRef<(() => void) | null>(null);
  useEffect(() => {
    if (!runId) {
      pendingApply.current = null;
      setRestartNeeded(false);
      return;
    }
    const apply = () => {
      pendingApply.current = null;
      void applyToRunning(tab.serverId, server).then((applied) => setRestartNeeded(!applied));
    };
    pendingApply.current = apply;
    const t = setTimeout(apply, 350);
    return () => clearTimeout(t);
  }, [server, runId, tab.serverId]);
  // Switching or closing the tab right after an edit still applies it.
  useEffect(() => () => pendingApply.current?.(), []);

  const start = () => void startServer(tab.serverId, server);
  const customHost = !HOSTS.some((h) => h.id === server.host);

  return (
    <div className="flex h-full min-h-0 flex-col bg-bg" data-testid="server-view">
      {tab.orphaned && <Banner tone="warning">This server's file was deleted or could not be reloaded. Save to write it again.</Banner>}
      <div className="flex shrink-0 flex-wrap items-center gap-2 px-3 pb-2 pt-2">
        <span className="flex h-9 items-center gap-2 rounded-xl bg-panel-2 px-3 text-[12px] font-bold" style={{ color: info.color }}>
          {info.icon(15)}
          {info.short}
        </span>
        <div className="min-w-0 flex-1">
          <div className="truncate text-[14px] font-semibold text-fg">{server.name}</div>
          <div className="flex items-center gap-1.5 text-[12px] text-muted" data-testid="server-status">
            <span className={cx("h-2 w-2 shrink-0 rounded-full", running ? "bg-success" : "bg-faint")} />
            {running ? (
              <>
                <span>Running at</span>
                <span className="selectable font-mono text-fg">{running.url}</span>
                <Tooltip content="Copy address">
                  <button aria-label="Copy address" onClick={() => void copyText(running.url)} className="rounded p-0.5 text-faint hover:bg-hover hover:text-fg">
                    <Copy size={12} />
                  </button>
                </Tooltip>
              </>
            ) : (
              <span>Stopped</span>
            )}
          </div>
        </div>
        {running ? (
          <>
            <Button variant="secondary" icon={busy ? <Loader2 size={14} className="zv-spin" /> : <RotateCw size={14} />} disabled={busy} onClick={() => void restartServer(tab.serverId, server)} className="h-9 rounded-xl">
              Restart
            </Button>
            <Button variant="secondary" icon={<Square size={13} />} disabled={busy} onClick={() => void stopServer(running.runId)} className="h-9 rounded-xl" data-testid="server-stop">
              Stop
            </Button>
          </>
        ) : (
          <Button variant="primary" icon={busy ? <Loader2 size={14} className="zv-spin" /> : <Play size={14} />} disabled={busy} onClick={start} className="h-9 w-[104px] rounded-xl" title={`Start (${modKey}+Enter)`} data-testid="server-start">
            Start
          </Button>
        )}
        <IconButton label={`Save (${modKey}+S)`} onClick={() => void saveTab(tab.id)} size={36} className={cx("rounded-xl", dirty && "text-accent")}>
          <Save size={16} />
        </IconButton>
      </div>

      <div className="flex shrink-0 flex-wrap items-end gap-3 px-3 pb-3">
        <label className="flex flex-col gap-1">
          <span className="text-[11.5px] font-medium text-muted">Listen on</span>
          <div className="flex gap-1.5">
            <select
              aria-label="Listen on"
              value={customHost ? "custom" : server.host}
              onChange={(e) => change((s) => ({ ...s, host: e.target.value === "custom" ? "" : e.target.value }))}
              className="h-8 rounded-lg border border-line bg-input px-2 text-[12.5px] text-fg outline-none focus:border-accent"
            >
              {HOSTS.map((h) => (
                <option key={h.id} value={h.id}>
                  {h.label}
                </option>
              ))}
              <option value="custom">Another address…</option>
            </select>
            {customHost && (
              <Input className="w-40 font-mono" value={server.host} placeholder="192.168.1.20" onChange={(e) => change((s) => ({ ...s, host: e.target.value.trim() }))} />
            )}
          </div>
        </label>
        <label className="flex flex-col gap-1">
          <span className="text-[11.5px] font-medium text-muted">Port</span>
          <Input
            type="number"
            min={0}
            max={65535}
            className="w-24 font-mono"
            aria-label="Port"
            value={server.port}
            onChange={(e) => change((s) => ({ ...s, port: Math.min(65535, Math.max(0, Math.trunc(Number(e.target.value)) || 0)) }))}
          />
        </label>
        {server.kind === "http" || server.kind === "websocket" || server.kind === "sse" || server.kind === "tcp" ? (
          <div className="flex h-8 items-center">
            <Switch
              checked={server.tls?.enabled ?? false}
              onChange={(enabled) => change((s) => ({ ...s, tls: { ...(s.tls ?? { enabled: false }), enabled } }))}
              label="TLS"
            />
          </div>
        ) : null}
        {server.tls?.enabled && (
          <>
            <label className="flex flex-col gap-1">
              <span className="text-[11.5px] font-medium text-muted">Certificate (PEM)</span>
              <Input className="w-56 font-mono" value={server.tls.certPath ?? ""} placeholder="self-signed" onChange={(e) => change((s) => ({ ...s, tls: { ...s.tls!, certPath: e.target.value } }))} />
            </label>
            <label className="flex flex-col gap-1">
              <span className="text-[11.5px] font-medium text-muted">Key (PEM)</span>
              <Input className="w-56 font-mono" value={server.tls.keyPath ?? ""} placeholder="self-signed" onChange={(e) => change((s) => ({ ...s, tls: { ...s.tls!, keyPath: e.target.value } }))} />
            </label>
          </>
        )}
        <div className="flex h-8 items-center">
          <Switch checked={server.autoStart ?? false} onChange={(autoStart) => change((s) => ({ ...s, autoStart }))} label="Start with workspace" />
        </div>
      </div>
      {server.host === "0.0.0.0" && (
        <Banner tone="info">Other devices on your network can reach this server. Windows may ask to allow it through the firewall.</Banner>
      )}
      {restartNeeded && running && (
        <Banner
          tone="warning"
          action={
            <Button size="sm" onClick={() => void restartServer(tab.serverId, server)}>
              Restart
            </Button>
          }
        >
          Restart the server to use the new address, port or TLS setting.
        </Banner>
      )}

      <div ref={area} className="flex min-h-0 flex-1 border-t border-line/60">
        <div style={{ width: `${split * 100}%` }} className="min-h-0 min-w-0 shrink-0 overflow-auto pb-6">
          <info.Editor server={server} onChange={change} running={running} tabId={tab.id} />
        </div>
        <Splitter
          direction="horizontal"
          onResize={(delta) => {
            const el = area.current;
            if (!el) return;
            useUi.setState((s) => ({ serverSplit: Math.min(0.75, Math.max(0.25, s.serverSplit + delta / el.clientWidth)) }));
          }}
        />
        <div className="min-h-0 min-w-0 flex-1 overflow-hidden">
          <TrafficPanel serverId={tab.serverId} server={server} running={running} />
        </div>
      </div>
    </div>
  );
}

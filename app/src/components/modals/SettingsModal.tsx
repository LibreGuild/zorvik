import { useEffect, useId, useMemo, useState } from "react";
import { Minus, Plus } from "lucide-react";
import type { ProxyMode } from "../../bindings/ProxyMode";
import type { Settings } from "../../bindings/Settings";
import { applyFonts, CODE_FONT_SIZES, CODE_FONTS, isFontInstalled, UI_FONTS, ZOOM_LEVELS } from "../../lib/appearance";
import { modKey, pickFile } from "../../lib/platform";
import type { AppInfo } from "../../bindings/AppInfo";
import { api, errorMessage, isTauri, RpcError } from "../../lib/rpc";
import { saveSettings, setZoom, useSettings, zoom } from "../../store/settings";
import { toast } from "../../store/toasts";
import { closeModal, useUi } from "../../store/ui";
import { useWorkspace } from "../../store/workspace";
import { AgentSetup } from "../agents/AgentSetup";
import { UpdateStatusRow } from "./UpdateSettings";
import { checkForUpdates } from "../../store/updates";
import { Button, cx, IconButton, Input, Modal, Select, Switch } from "../ui";

type Section = "general" | "requests" | "proxy" | "certificates" | "data" | "updates" | "agents";

const SECTIONS: { id: Section; label: string }[] = [
  { id: "general", label: "General" },
  { id: "requests", label: "Requests" },
  { id: "proxy", label: "Proxy" },
  { id: "certificates", label: "Certificates" },
  { id: "data", label: "Data & privacy" },
  { id: "updates", label: "Updates" },
  { id: "agents", label: "AI agents" },
];

function Row({ label, hint, children }: { label: string; hint?: React.ReactNode; children: React.ReactNode }) {
  return (
    <div className="grid grid-cols-[220px_1fr] items-start gap-4 border-b border-line/50 py-3 last:border-0">
      <div>
        <div className="text-[13px] text-fg">{label}</div>
        {hint && <div className="mt-0.5 text-[11.5px] leading-snug text-faint">{hint}</div>}
      </div>
      <div className="min-w-0">{children}</div>
    </div>
  );
}

function Unit({ children, unit }: { children: React.ReactNode; unit: string }) {
  return (
    <div className="flex items-center gap-2">
      {children}
      <span className="text-[12px] text-faint">{unit}</span>
    </div>
  );
}

/**
 * Whole number from a number input (the backend wants integers). Minimums are applied on blur,
 * not per keystroke: clamping "5" up to 100 made it impossible to type "500".
 */
const whole = (v: string, max = Number.MAX_SAFE_INTEGER) => Math.min(max, Math.max(0, Math.trunc(Number(v)) || 0));

/** Where `zorvik` is, and (macOS) a way to put it on PATH. */
function CliStatus() {
  const appInfo = useWorkspace((s) => s.appInfo);
  const [busy, setBusy] = useState(false);
  if (!appInfo?.cliPath) {
    return (
      <span className="text-[12px] text-faint">
        {appInfo?.platform === "macos"
          ? "Not found next to the app. Move Zorvik to Applications and open it from there."
          : "Not found next to the app (the AppImage has none: use the .deb or .rpm for the command line)."}
      </span>
    );
  }
  return (
    <div className="flex flex-col gap-2">
      <div className="selectable break-all rounded-md border border-line bg-panel-2 px-2.5 py-1.5 font-mono text-[12px] text-muted">{appInfo.cliPath}</div>
      {appInfo.cliOnPath ? (
        <span className="text-[12px] text-success">On PATH: type zorvik in a terminal.</span>
      ) : appInfo.platform === "macos" ? (
        <div>
          <Button
            loading={busy}
            onClick={async () => {
              setBusy(true);
              try {
                const info = await api.call<AppInfo>("app.installCli");
                useWorkspace.setState({ appInfo: info });
                toast("success", "zorvik is on your PATH", "Open a new terminal to use it.");
              } catch (e) {
                if (!(e instanceof RpcError && e.code === "cancelled")) toast("error", "Could not add zorvik to PATH", errorMessage(e));
              } finally {
                setBusy(false);
              }
            }}
          >
            Add zorvik to PATH…
          </Button>
          <div className="mt-1 text-[11px] text-faint">Links it into /usr/local/bin (asks for your password).</div>
        </div>
      ) : (
        <span className="text-[12px] text-faint">Not on PATH in this window yet; new terminals find it after installing.</span>
      )}
    </div>
  );
}

function PathInput({ value, onChange, placeholder }: { value: string; onChange: (v: string) => void; placeholder: string }) {
  return (
    <div className="flex gap-2">
      <Input value={value} onChange={(e) => onChange(e.target.value)} placeholder={placeholder} />
      <Button
        onClick={async () => {
          const p = await pickFile("Choose PEM file", ["pem", "crt", "cer", "key"]);
          if (p) onChange(p);
        }}
      >
        Browse…
      </Button>
      {value && (
        <Button variant="ghost" onClick={() => onChange("")}>
          Clear
        </Button>
      )}
    </div>
  );
}

/** Zoom applies (and is saved) right away, like ⌘+ / ⌘−. */
function ZoomControl() {
  const value = useSettings((s) => s.settings?.appearance.zoom ?? 100);
  const levels = ZOOM_LEVELS.includes(value) ? ZOOM_LEVELS : [...ZOOM_LEVELS, value].sort((a, b) => a - b);
  return (
    <div className="flex items-center gap-1.5">
      <IconButton size={32} label="Zoom out" onClick={() => zoom(-1)} disabled={value <= ZOOM_LEVELS[0]}>
        <Minus size={14} />
      </IconButton>
      <Select value={value} onChange={(e) => setZoom(Number(e.target.value))} className="w-24" aria-label="Zoom">
        {levels.map((z) => (
          <option key={z} value={z}>
            {z} %
          </option>
        ))}
      </Select>
      <IconButton size={32} label="Zoom in" onClick={() => zoom(1)} disabled={value >= ZOOM_LEVELS[ZOOM_LEVELS.length - 1]}>
        <Plus size={14} />
      </IconButton>
      {value !== 100 && (
        <Button variant="ghost" onClick={() => zoom(0)}>
          Reset
        </Button>
      )}
    </div>
  );
}

/** A font name, with the suggestions installed here; a name that isn't installed says so. */
function FontInput({ value, onChange, placeholder, suggestions, label }: { value: string; onChange: (v: string) => void; placeholder: string; suggestions: string[]; label: string }) {
  const listId = useId();
  const installed = useMemo(() => suggestions.filter(isFontInstalled), [suggestions]);
  const missing = value.trim() !== "" && !isFontInstalled(value);
  return (
    <div>
      <div className="flex gap-2">
        <Input value={value} onChange={(e) => onChange(e.target.value)} placeholder={placeholder} list={listId} className="w-60" aria-label={label} />
        {value && (
          <Button variant="ghost" onClick={() => onChange("")}>
            Default
          </Button>
        )}
      </div>
      <datalist id={listId}>
        {installed.map((f) => (
          <option key={f} value={f} />
        ))}
      </datalist>
      {missing && <div className="mt-1 text-[11.5px] text-warning">Not installed on this computer: the default font is used.</div>}
    </div>
  );
}

/** `edited` (from `base`) moved onto `latest`: fields the user changed keep their value, the others take the latest. */
export function rebase<T>(base: T, edited: T, latest: T): T {
  if (JSON.stringify(edited) === JSON.stringify(base)) return latest;
  const plain = (v: unknown): v is Record<string, unknown> => !!v && typeof v === "object" && !Array.isArray(v);
  if (!plain(base) || !plain(edited) || !plain(latest)) return edited;
  const out: Record<string, unknown> = { ...latest };
  for (const key of Object.keys(edited)) out[key] = rebase(base[key], edited[key], latest[key]);
  return out as T;
}

export function SettingsModal() {
  const original = useSettings((s) => s.settings);
  const appInfo = useWorkspace((s) => s.appInfo);
  const [s, setS] = useState<Settings | null>(original);
  // Settings changed elsewhere while this is open (e.g. "Allow AI agents" answered): take them
  // in, keeping what the user changed here, so saving doesn't undo them.
  const [base, setBase] = useState(original);
  if (original !== base) {
    setBase(original);
    setS(s && base && original ? rebase(base, s, original) : original);
  }
  const [section, setSection] = useState<Section>("general");
  const layout = useUi((u) => u.layout);
  const [saving, setSaving] = useState(false);
  // Font changes show at once while the dialog is open; closing without saving puts the saved ones back.
  const draftLook = s?.appearance;
  useEffect(() => {
    if (draftLook) applyFonts(draftLook);
  }, [draftLook]);
  useEffect(
    () => () => {
      const saved = useSettings.getState().settings?.appearance;
      if (saved) applyFonts(saved);
    },
    [],
  );
  if (!s || !original) return null;

  const req = s.request;
  const setReq = (patch: Partial<Settings["request"]>) => setS({ ...s, request: { ...req, ...patch } });
  const look = s.appearance;
  const setLook = (patch: Partial<Settings["appearance"]>) => setS({ ...s, appearance: { ...look, ...patch } });
  const agents = s.agents;
  const setAgents = (patch: Partial<Settings["agents"]>) => setS({ ...s, agents: { ...agents, ...patch } });
  const dirty = JSON.stringify(s) !== JSON.stringify(original);
  const proxy = s.proxy;

  const save = async () => {
    setSaving(true);
    try {
      await saveSettings(s);
      // Turned on, or a different channel: look now instead of in a few hours.
      const was = original.updates;
      if (isTauri && s.updates.mode !== "off" && (was.mode !== s.updates.mode || was.channel !== s.updates.channel)) void checkForUpdates();
      toast("success", "Settings saved");
      closeModal();
    } catch (e) {
      toast("error", "Could not save settings", errorMessage(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <Modal
      open
      onClose={closeModal}
      title="Settings"
      width={860}
      bodyClassName="p-0"
      focusFirstField={false}
      footer={
        <>
          <span className="mr-auto text-[11.5px] text-faint">
            Zorvik {appInfo?.version}
            {appInfo?.build ? ` · ${appInfo.build}` : ""}
          </span>
          <Button variant="ghost" onClick={closeModal}>
            Cancel
          </Button>
          <Button variant="primary" onClick={save} loading={saving} disabled={!dirty}>
            Save
          </Button>
        </>
      }
    >
      <div className="flex h-[480px]">
        <nav className="w-48 shrink-0 p-2">
          {SECTIONS.map((x) => (
            <button
              key={x.id}
              onClick={() => setSection(x.id)}
              className={cx("flex h-8 w-full items-center rounded-lg px-2.5 text-left text-[12.5px]", section === x.id ? "bg-hover text-fg" : "text-muted hover:bg-hover/50 hover:text-fg")}
            >
              {x.label}
            </button>
          ))}
        </nav>
        <div className="min-w-0 flex-1 overflow-auto px-6 py-2">
          {section === "general" && (
            <>
              <Row label="Theme">
                <Select value={s.theme} onChange={(e) => setS({ ...s, theme: e.target.value as Settings["theme"] })} className="w-48">
                  <option value="system">Match system</option>
                  <option value="dark">Dark</option>
                  <option value="light">Light</option>
                </Select>
              </Row>
              <Row label="Response position" hint="Applies immediately.">
                <Select value={layout} onChange={(e) => useUi.setState({ layout: e.target.value as "side" | "stacked" })} className="w-48">
                  <option value="side">Beside the request</option>
                  <option value="stacked">Below the request</option>
                </Select>
              </Row>
              <Row label="Zoom" hint={`Makes everything bigger or smaller. ${modKey}+ zooms in, ${modKey}− out, ${modKey}0 resets.`}>
                <ZoomControl />
              </Row>
              <Row label="Interface font" hint="Menus, lists, forms and tabs.">
                <FontInput value={look.uiFont} onChange={(uiFont) => setLook({ uiFont })} placeholder="System font" suggestions={UI_FONTS} label="Interface font" />
              </Row>
              <Row label="Code font" hint="Editors, response bodies, headers and other code.">
                <FontInput value={look.codeFont} onChange={(codeFont) => setLook({ codeFont })} placeholder="System monospace" suggestions={CODE_FONTS} label="Code font" />
              </Row>
              <Row label="Code text size" hint="In editors and response bodies, on top of the zoom.">
                <Select value={look.codeFontSize} onChange={(e) => setLook({ codeFontSize: Number(e.target.value) })} className="w-24" aria-label="Code text size">
                  {(CODE_FONT_SIZES.includes(look.codeFontSize) ? CODE_FONT_SIZES : [...CODE_FONT_SIZES, look.codeFontSize].sort((a, b) => a - b)).map((px) => (
                    <option key={px} value={px}>
                      {px} px
                    </option>
                  ))}
                </Select>
              </Row>
              <Row label="Ligatures" hint="Draw => != >= as single symbols, with code fonts that have them (Fira Code, JetBrains Mono, Cascadia Code).">
                <Switch checked={look.ligatures} onChange={(ligatures) => setLook({ ligatures })} />
              </Row>
            </>
          )}
          {section === "requests" && (
            <>
              <Row label="Request timeout" hint="For the whole request. 0 = no limit.">
                <Unit unit="ms">
                  <Input type="number" min={0} className="w-40" value={req.timeoutMs} onChange={(e) => setReq({ timeoutMs: whole(e.target.value) })} />
                </Unit>
              </Row>
              <Row label="Connect timeout" hint="DNS + TCP + TLS.">
                <Unit unit="ms">
                  <Input
                    type="number"
                    min={100}
                    className="w-40"
                    value={req.connectTimeoutMs}
                    onChange={(e) => setReq({ connectTimeoutMs: whole(e.target.value) })}
                    onBlur={() => setReq({ connectTimeoutMs: Math.max(100, req.connectTimeoutMs) })}
                  />
                </Unit>
              </Row>
              <Row label="Follow redirects">
                <Switch checked={req.followRedirects} onChange={(followRedirects) => setReq({ followRedirects })} />
              </Row>
              <Row label="Max redirects">
                <Input type="number" min={0} max={100} className="w-40" value={req.maxRedirects} onChange={(e) => setReq({ maxRedirects: whole(e.target.value) })} />
              </Row>
              <Row label="Verify TLS certificates" hint="Turn off only for testing against servers you trust.">
                <Switch checked={req.verifyTls} onChange={(verifyTls) => setReq({ verifyTls })} />
              </Row>
              <Row label="HTTP version" hint={req.httpVersion === "http3" ? "HTTP/3 runs over QUIC (UDP): https only and never through a proxy." : undefined}>
                <Select value={req.httpVersion} onChange={(e) => setReq({ httpVersion: e.target.value as Settings["request"]["httpVersion"] })} className="w-60">
                  <option value="auto">Auto (HTTP/2 when offered)</option>
                  <option value="http1">HTTP/1.1 only</option>
                  <option value="http2">HTTP/2 only</option>
                  <option value="http3">HTTP/3 (QUIC)</option>
                </Select>
              </Row>
              <Row label="Decompress responses" hint="gzip, deflate, br and zstd.">
                <Switch checked={req.decompress} onChange={(decompress) => setReq({ decompress })} />
              </Row>
              <Row label="Default headers" hint="Send User-Agent, Accept and Accept-Encoding when not set.">
                <Switch checked={req.sendDefaultHeaders} onChange={(sendDefaultHeaders) => setReq({ sendDefaultHeaders })} />
              </Row>
              <Row label="Max response size" hint="Larger responses are cut.">
                <Unit unit="MB">
                  <Input
                    type="number"
                    min={1}
                    max={2048}
                    className="w-40"
                    value={req.maxResponseMb}
                    onChange={(e) => setReq({ maxResponseMb: whole(e.target.value, 2048) })}
                    onBlur={() => setReq({ maxResponseMb: Math.max(1, req.maxResponseMb) })}
                  />
                </Unit>
              </Row>
              <Row label="Script time limit" hint="For each pre-request or post-response script (100 ms to 60 s).">
                <Unit unit="ms">
                  <Input
                    type="number"
                    min={100}
                    max={60000}
                    className="w-40"
                    value={s.scriptTimeoutMs}
                    onChange={(e) => setS({ ...s, scriptTimeoutMs: whole(e.target.value, 60000) })}
                    onBlur={() => setS({ ...s, scriptTimeoutMs: Math.max(100, s.scriptTimeoutMs) })}
                  />
                </Unit>
              </Row>
            </>
          )}
          {section === "proxy" && (
            <>
              <Row
                label="Proxy"
                hint="System uses HTTP(S)_PROXY / NO_PROXY, then the OS settings (Windows Internet Options, macOS Network). PAC scripts, SOCKS and NTLM/Kerberos proxy auth are not supported yet."
              >
                <Select
                  value={proxy.mode}
                  onChange={(e) => {
                    const mode = e.target.value as ProxyMode["mode"];
                    setS({ ...s, proxy: mode === "manual" ? { mode, url: "", bypass: "" } : ({ mode } as ProxyMode) });
                  }}
                  className="w-60"
                >
                  <option value="system">Use system proxy</option>
                  <option value="none">No proxy</option>
                  <option value="manual">Manual</option>
                </Select>
              </Row>
              {proxy.mode === "manual" && (
                <>
                  <Row label="Proxy URL" hint="http://host:port, with optional user:password@ for Basic auth.">
                    <Input value={proxy.url} onChange={(e) => setS({ ...s, proxy: { ...proxy, url: e.target.value } })} placeholder="http://proxy.corp.local:8080" />
                  </Row>
                  <Row label="Bypass" hint="Comma-separated hosts, e.g. *.corp.local, 10.0.0.0/8, <local>. localhost is always direct.">
                    <Input value={proxy.bypass} onChange={(e) => setS({ ...s, proxy: { ...proxy, bypass: e.target.value } })} placeholder="*.internal, 10.*" />
                  </Row>
                </>
              )}
            </>
          )}
          {section === "certificates" && (
            <>
              <Row label="Extra CA certificate" hint="PEM file trusted in addition to the OS trust store (for internal or self-signed CAs). OS-installed corporate CAs are already trusted.">
                <PathInput value={s.tls.caCertPath} onChange={(caCertPath) => setS({ ...s, tls: { ...s.tls, caCertPath } })} placeholder="/path/to/ca.pem" />
              </Row>
              <Row label="Client certificate" hint="PEM certificate for mutual TLS.">
                <PathInput value={s.tls.clientCertPath} onChange={(clientCertPath) => setS({ ...s, tls: { ...s.tls, clientCertPath } })} placeholder="/path/to/client.crt" />
              </Row>
              <Row label="Client key" hint="PEM private key for the client certificate.">
                <PathInput value={s.tls.clientKeyPath} onChange={(clientKeyPath) => setS({ ...s, tls: { ...s.tls, clientKeyPath } })} placeholder="/path/to/client.key" />
              </Row>
            </>
          )}
          {section === "data" && (
            <>
              <Row label="Cookie jar" hint="Store cookies from responses and send them on later requests (per workspace).">
                <Switch checked={s.cookieJar} onChange={(cookieJar) => setS({ ...s, cookieJar })} />
              </Row>
              <Row
                label="Files outside the workspace"
                hint="Let requests upload body files from anywhere on this computer. Off: only files inside the workspace folder, so a shared workspace can't send your private files."
              >
                <Switch checked={s.filesOutsideWorkspace} onChange={(filesOutsideWorkspace) => setS({ ...s, filesOutsideWorkspace })} />
              </Row>
              <Row label="History size" hint="Entries kept per workspace.">
                <Input
                  type="number"
                  min={10}
                  max={100000}
                  className="w-40"
                  value={s.historyLimit}
                  onChange={(e) => setS({ ...s, historyLimit: whole(e.target.value) })}
                  onBlur={() => setS({ ...s, historyLimit: Math.max(10, s.historyLimit) })}
                />
              </Row>
              <Row label="App data folder" hint="Settings, history, cookies, OAuth tokens and secret variable values.">
                <div className="selectable break-all rounded-md border border-line bg-panel-2 px-2.5 py-1.5 font-mono text-[12px] text-muted">{appInfo?.dataDir}</div>
              </Row>
            </>
          )}
          {section === "updates" && (
            <>
              <Row label="This version" hint={isTauri ? undefined : "Updates are handled by the desktop app."}>
                <UpdateStatusRow version={appInfo?.version ?? ""} build={appInfo?.build ?? null} />
              </Row>
              <Row label="Updates" hint="Automatic: new versions download in the background and install when you restart or quit Zorvik. Nothing restarts without asking.">
                <Select
                  value={s.updates.mode}
                  onChange={(e) => setS({ ...s, updates: { ...s.updates, mode: e.target.value as Settings["updates"]["mode"] } })}
                  className="w-60"
                  aria-label="Updates"
                >
                  <option value="automatic">Automatic</option>
                  <option value="notify">Tell me, don't download</option>
                  <option value="off">Off</option>
                </Select>
              </Row>
              <Row label="Channel" hint="Nightly: a build of the newest code every day there are changes. Less tested; use it to try what's coming.">
                <Select
                  value={s.updates.channel}
                  onChange={(e) => setS({ ...s, updates: { ...s.updates, channel: e.target.value as Settings["updates"]["channel"] } })}
                  className="w-60"
                  aria-label="Update channel"
                >
                  <option value="stable">Stable releases</option>
                  <option value="nightly">Nightly builds</option>
                </Select>
              </Row>
              <Row label="What is sent" hint="The update check is the only connection Zorvik makes by itself.">
                <p className="m-0 text-[12.5px] leading-relaxed text-muted">
                  Zorvik reads one file, <span className="font-mono text-[11.5px]">latest.json</span>, from the project's releases on GitHub
                  (github.com/LibreGuild/zorvik) and downloads new versions from there. No account, no ID, nothing about you, this computer or
                  your work is sent, and no other server is involved. Downloads are checked against the project's signing key before they install.
                </p>
              </Row>
            </>
          )}
          {section === "agents" && (
            <>
              <Row
                label="Allow AI agents"
                hint="Claude Code, Codex, Gemini CLI, Cursor and others control Zorvik through zorvik mcp. When off, the first thing an agent does asks you."
              >
                <Switch checked={agents.enabled} onChange={(enabled) => setAgents({ enabled })} />
              </Row>
              <Row label="Edits by agents" hint="Creating and changing requests, folders, environments and load tests. Deleting always asks.">
                <Select value={agents.changes} onChange={(e) => setAgents({ changes: e.target.value as Settings["agents"]["changes"] })} className="w-60">
                  <option value="allow">Allow</option>
                  <option value="ask">Ask me</option>
                </Select>
              </Row>
              <Row
                label="Requests sent by agents"
                hint="Also collection runs and schema downloads. Outside hosts: not this computer or a private network; asked once per host while the agent is connected. Load tests and servers always ask."
              >
                <Select value={agents.traffic} onChange={(e) => setAgents({ traffic: e.target.value as Settings["agents"]["traffic"] })} className="w-60">
                  <option value="askOutside">Ask for outside hosts</option>
                  <option value="ask">Ask every time</option>
                  <option value="allow">Allow</option>
                </Select>
              </Row>
              <Row label="Follow agents" hint="Open what an agent works on: its requests, their responses, runs and load tests.">
                <Switch checked={agents.follow} onChange={(follow) => setAgents({ follow })} />
              </Row>
              <Row
                label="Work without the app"
                hint="When Zorvik is closed, agents use it in the background instead of opening it. Nothing can be approved then, so actions that would ask are refused."
              >
                <Switch checked={agents.headless} onChange={(headless) => setAgents({ headless })} />
              </Row>
              <Row label="Command-line tool" hint="zorvik runs collections and load tests in a terminal or CI; agents start zorvik mcp.">
                <CliStatus />
              </Row>
              <Row label="Connect an agent" hint="Run the command for your agent once (or add the JSON to its MCP settings).">
                <AgentSetup />
              </Row>
            </>
          )}
        </div>
      </div>
    </Modal>
  );
}

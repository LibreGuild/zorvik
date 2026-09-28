import { useState } from "react";
import { Info } from "lucide-react";
import type { KeyValue } from "../../bindings/KeyValue";
import type { Request } from "../../bindings/Request";
import type { RequestSettings } from "../../bindings/RequestSettings";
import { COMMON_HEADERS } from "../../lib/http";
import { applyParamRows, paramRows, syncPathParams } from "../../lib/url";
import { send, type Tab, updateDraft, updateTab } from "../../store/tabs";
import { CodeEditor } from "../CodeEditor";
import { KeyValueEditor } from "../KeyValueEditor";
import { Button, cx, Input, Select, Switch, Tabs } from "../ui";
import { AuthEditor } from "./AuthEditor";
import { BodyEditor } from "./BodyEditor";
import { ExamplesTab } from "./ExamplesTab";
import { KIND_EDITOR_TABS } from "./kinds";
import { ScriptsEditor, ScriptsTabLabel } from "./ScriptsEditor";

const AUTH_LABEL: Record<string, string> = {
  inherit: "Inherit",
  none: "None",
  basic: "Basic",
  bearer: "Bearer",
  apiKey: "API key",
  oauth2: "OAuth 2",
  oauth1: "OAuth 1",
  jwt: "JWT",
  digest: "Digest",
  ntlm: "NTLM",
  awsSigV4: "AWS",
  hawk: "Hawk",
  akamaiEdgeGrid: "EdgeGrid",
  asap: "ASAP",
};

const count = (rows: KeyValue[] | undefined) => (rows ?? []).filter((r) => r.enabled !== false && r.key.trim()).length;

const BODY_LABEL: Record<string, string> = {
  json: "JSON",
  text: "Text",
  xml: "XML",
  formUrlencoded: "Form",
  multipart: "Multipart",
  binary: "File",
  graphql: "GraphQL",
};

export function RequestEditor({ tab, toolbar }: { tab: Tab; toolbar?: React.ReactNode }) {
  const req = tab.draft;
  const kind = req.kind ?? "http";
  const update = (fn: (r: Request) => Request) => updateDraft(tab.id, fn);
  const params = paramRows(req.url, req.disabledParams, req.paramDescriptions);
  // Derived from the URL so `:name` segments always get a row, even in imported files.
  const pathParams = syncPathParams(req.url, req.pathParams);
  const submit = () => send(tab.id);

  const kindTabs = KIND_EDITOR_TABS[kind];
  const authItem = { id: "auth", label: "Auth", badge: req.auth && req.auth.type !== "inherit" ? AUTH_LABEL[req.auth.type] : undefined };
  const items = kindTabs
    ? [
        ...kindTabs.map((t) => ({ id: t.id, label: t.label })),
        ...(kind === "mqtt" || kind === "grpc" ? [authItem] : []),
        ...(kind === "dns" ? [] : [{ id: "settings", label: "Settings" }]),
        { id: "docs", label: "Docs" },
      ]
    : [
        { id: "params", label: "Params", badge: count(params) + pathParams.length },
        { id: "headers", label: "Headers", badge: count(req.headers) },
        ...(kind === "http" ? [{ id: "body", label: "Body", badge: req.body && req.body.type !== "none" ? BODY_LABEL[req.body.type] : undefined }] : []),
        authItem,
        { id: "settings", label: "Settings" },
        ...(kind === "http" ? [{ id: "scripts", label: <ScriptsTabLabel scripts={req.scripts} /> }] : []),
        { id: "docs", label: "Docs" },
        ...(kind === "http" ? [{ id: "examples", label: "Examples", badge: req.examples?.length || undefined }] : []),
      ];
  const current = items.some((i) => i.id === tab.requestTab) ? tab.requestTab : items[0].id;
  const KindView = kindTabs?.find((t) => t.id === current)?.View;

  return (
    <div className="flex h-full min-h-0 flex-col">
      <Tabs items={items} value={current} onChange={(requestTab) => updateTab(tab.id, () => ({ requestTab }))} right={toolbar} />
      <div className="min-h-0 flex-1 overflow-auto">
        {KindView && <KindView tab={tab} />}
        {current === "params" && (
          <div>
            <SectionTitle>Query parameters</SectionTitle>
            <KeyValueEditor
              rows={params}
              onChange={(rows) => update((r) => ({ ...r, ...applyParamRows(r.url, rows) }))}
              keyPlaceholder="Parameter"
              onEnter={submit}
            />
            {pathParams.length > 0 && (
              <>
                <SectionTitle>Path variables</SectionTitle>
                <KeyValueEditor
                  rows={pathParams}
                  fixedKeys
                  allowDisable={false}
                  onChange={(pathParams) => update((r) => ({ ...r, pathParams }))}
                  onEnter={submit}
                />
              </>
            )}
            <Hint>
              Use <code className="font-mono">{"{{variable}}"}</code> anywhere, and <code className="font-mono">:name</code> segments in the
              path for path variables.
            </Hint>
          </div>
        )}
        {current === "headers" && <HeadersTab tab={tab} />}
        {current === "body" && kind === "http" && (
          <BodyEditor body={req.body ?? { type: "none" }} onChange={(body) => update((r) => ({ ...r, body }))} onSubmit={submit} tab={tab} />
        )}
        {current === "auth" && (
          <AuthEditor auth={req.auth ?? { type: "inherit" }} onChange={(auth) => update((r) => ({ ...r, auth }))} path={tab.path} />
        )}
        {current === "settings" && (
          <SettingsTab settings={req.settings ?? {}} onChange={(settings) => update((r) => ({ ...r, settings }))} httpOnly={kind === "http" || kind === "sse" || kind === "websocket"}
            timeout={kind !== "tcp" && kind !== "udp" && kind !== "mqtt"}
            kind={kind}
          />
        )}
        {current === "scripts" && kind === "http" && (
          <ScriptsEditor where="request" scripts={req.scripts ?? {}} onChange={(scripts) => update((r) => ({ ...r, scripts }))} />
        )}
        {current === "examples" && kind === "http" && <ExamplesTab tab={tab} />}
        {current === "docs" && (
          <CodeEditor
            value={req.docs ?? ""}
            onChange={(docs) => update((r) => ({ ...r, docs }))}
            placeholder="Notes about this request (Markdown). Saved with the request."
            lineNumbers={false}
          />
        )}
      </div>
    </div>
  );
}

function SectionTitle({ children }: { children: React.ReactNode }) {
  return <div className="px-3 pb-1.5 pt-3 text-[11px] font-semibold uppercase tracking-wide text-faint">{children}</div>;
}

function Hint({ children }: { children: React.ReactNode }) {
  return (
    <div className="flex items-start gap-2 px-3 py-3 text-[12px] text-faint">
      <Info size={13} className="mt-0.5 shrink-0" />
      <div>{children}</div>
    </div>
  );
}

function HeadersTab({ tab }: { tab: Tab }) {
  const [bulk, setBulk] = useState(false);
  const req = tab.draft;
  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center justify-between px-3 pb-1 pt-2">
        <div className="text-[11px] font-semibold uppercase tracking-wide text-faint">Headers</div>
        <Button size="sm" variant="ghost" onClick={() => setBulk(!bulk)}>
          {bulk ? "Table" : "Bulk edit"}
        </Button>
      </div>
      <div className={cx(bulk && "flex-1")}>
        <KeyValueEditor
          key={bulk ? "bulk" : "table"}
          bulk={bulk}
          rows={req.headers ?? []}
          onChange={(headers) => updateDraft(tab.id, (r) => ({ ...r, headers }))}
          keyPlaceholder="Header"
          keySuggestions={COMMON_HEADERS}
          onEnter={() => send(tab.id)}
        />
      </div>
      <Hint>
        Added automatically unless you set them: User-Agent, Accept, Accept-Encoding, Content-Type (from the body), Authorization (from Auth), Cookie
        (from the cookie jar). Folder and workspace headers are inherited.
      </Hint>
    </div>
  );
}

type Tri = "default" | "on" | "off";
const tri = (v: boolean | undefined): Tri => (v === undefined ? "default" : v ? "on" : "off");
const fromTri = (v: Tri): boolean | undefined => (v === "default" ? undefined : v === "on");
/** Empty = app default; otherwise a whole number (the backend rejects 1.5 for these integer fields on send/save). */
const wholeOrDefault = (v: string): number | undefined => (v === "" ? undefined : Math.max(0, Math.trunc(Number(v)) || 0));

/** `httpOnly` false (TCP/UDP/MQTT/gRPC): only the settings that apply to raw connections; `timeout`: the
 *  request has an overall deadline (HTTP kinds and gRPC calls, not long-lived sockets). */
function SettingsTab({
  settings,
  onChange,
  httpOnly = true,
  timeout = true,
  kind,
}: {
  settings: RequestSettings;
  onChange: (s: RequestSettings) => void;
  httpOnly?: boolean;
  timeout?: boolean;
  kind: string;
}) {
  const set = (patch: Partial<RequestSettings>) => {
    const next: RequestSettings = { ...settings, ...patch };
    for (const k of Object.keys(next) as (keyof RequestSettings)[]) if (next[k] === undefined) delete next[k];
    onChange(next);
  };
  return (
    <div className="flex max-w-2xl flex-col gap-4 p-4">
      <p className="text-[12.5px] text-muted">
        Overrides for this request only. Defaults are in Settings{httpOnly ? "" : " (connect timeout and proxy apply to this connection too)"}.
      </p>
      {timeout && (
      <SettingRow label="Timeout" hint="Milliseconds for the whole request. Empty = app default, 0 = no limit.">
        <Input
          type="number"
          min={0}
          className="w-44"
          value={settings.timeoutMs ?? ""}
          placeholder="default"
          onChange={(e) => set({ timeoutMs: wholeOrDefault(e.target.value) })}
        />
      </SettingRow>
      )}
      {httpOnly && (
        <>
      <SettingRow label="Follow redirects">
        <TriSelect value={settings.followRedirects} onPick={(followRedirects) => set({ followRedirects })} />
      </SettingRow>
      <SettingRow label="Max redirects">
        <Input
          type="number"
          min={0}
          max={100}
          className="w-44"
          value={settings.maxRedirects ?? ""}
          placeholder="default"
          onChange={(e) => set({ maxRedirects: wholeOrDefault(e.target.value) })}
        />
      </SettingRow>
        </>
      )}
      <SettingRow label="Verify TLS certificates">
        <TriSelect value={settings.verifyTls} onPick={(verifyTls) => set({ verifyTls })} />
      </SettingRow>
      {httpOnly && (
        <>
      <SettingRow label="HTTP version" hint={settings.httpVersion === "http3" ? "QUIC over UDP: https only, no proxy." : undefined}>
        <Select
          value={settings.httpVersion ?? "default"}
          onChange={(e) => set({ httpVersion: e.target.value === "default" ? undefined : (e.target.value as RequestSettings["httpVersion"]) })}
          className="w-44"
        >
          <option value="default">App default</option>
          <option value="auto">Auto (HTTP/2 if offered)</option>
          <option value="http1">HTTP/1.1 only</option>
          <option value="http2">HTTP/2 only</option>
          <option value="http3">HTTP/3 (QUIC)</option>
        </Select>
      </SettingRow>
      <SettingRow label="Decompress responses">
        <TriSelect value={settings.decompress} onPick={(decompress) => set({ decompress })} />
      </SettingRow>
        </>
      )}
      {kind === "sse" && <StreamSettings settings={settings} set={set} />}
      {(kind === "http" || kind === "sse") && <RepeatSettings settings={settings} set={set} />}
    </div>
  );
}

/** When a collection run (or an AI agent) stops reading an event stream. */
function StreamSettings({ settings, set }: { settings: RequestSettings; set: (p: Partial<RequestSettings>) => void }) {
  const stream = settings.stream ?? { maxEvents: 100, timeoutMs: 10000 };
  const change = (patch: Partial<typeof stream>) => set({ stream: { ...stream, ...patch } });
  return (
    <>
      <h3 className="mt-2 text-[11.5px] font-semibold uppercase tracking-wide text-faint">In collection runs</h3>
      <p className="-mt-2 text-[12px] text-muted">
        A run reads events until the first of these, then its post-response scripts test them (<code className="font-mono">pm.response.events</code>).
      </p>
      <SettingRow label="Stop at event" hint="Event name; empty = any event may be the last.">
        <Input className="w-56 font-mono" value={stream.event ?? ""} placeholder="e.g. done" onChange={(e) => change({ event: e.target.value || undefined })} data-testid="stream-event" />
      </SettingRow>
      <SettingRow label="Stop after" hint="Events; 0 = only the time limit.">
        <Input type="number" min={0} className="w-44" value={stream.maxEvents} onChange={(e) => change({ maxEvents: Math.max(0, Math.trunc(Number(e.target.value)) || 0) })} />
      </SettingRow>
      <SettingRow label="Time limit" hint="Milliseconds.">
        <Input type="number" min={100} className="w-44" value={stream.timeoutMs} onChange={(e) => change({ timeoutMs: Math.max(100, Math.trunc(Number(e.target.value)) || 0) })} />
      </SettingRow>
    </>
  );
}

/** "Send again until…" for polling in collection runs. */
function RepeatSettings({ settings, set }: { settings: RequestSettings; set: (p: Partial<RequestSettings>) => void }) {
  const repeat = settings.repeat;
  const change = (patch: Partial<NonNullable<RequestSettings["repeat"]>>) =>
    set({ repeat: { condition: "", intervalMs: 1000, timeoutMs: 30000, ...repeat, ...patch } });
  return (
    <>
      <h3 className="mt-2 text-[11.5px] font-semibold uppercase tracking-wide text-faint">Repeat until</h3>
      <SettingRow label="Repeat in collection runs" hint="Send again until the condition holds, e.g. to wait for a job to finish.">
        <Switch
          checked={!!repeat}
          onChange={(on) => (on ? change({}) : set({ repeat: undefined }))}
          label={repeat ? "On" : "Off"}
        />
      </SettingRow>
      {repeat && (
        <>
          <SettingRow label="Condition" hint="JavaScript checked after the post-response scripts. Empty: until this request's tests pass.">
            <Input
              className="w-full font-mono"
              value={repeat.condition}
              placeholder={'pm.response.json().status === "done"'}
              onChange={(e) => change({ condition: e.target.value })}
              data-testid="repeat-condition"
            />
          </SettingRow>
          <SettingRow label="Every" hint="Milliseconds between sends.">
            <Input type="number" min={0} className="w-44" value={repeat.intervalMs} onChange={(e) => change({ intervalMs: Math.max(0, Math.trunc(Number(e.target.value)) || 0) })} />
          </SettingRow>
          <SettingRow label="Give up after" hint="Milliseconds; then the request fails.">
            <Input type="number" min={0} className="w-44" value={repeat.timeoutMs} onChange={(e) => change({ timeoutMs: Math.max(0, Math.trunc(Number(e.target.value)) || 0) })} />
          </SettingRow>
        </>
      )}
    </>
  );
}

function TriSelect({ value, onPick }: { value: boolean | undefined; onPick: (v: boolean | undefined) => void }) {
  return (
    <Select value={tri(value)} onChange={(e) => onPick(fromTri(e.target.value as Tri))} className="w-44">
      <option value="default">App default</option>
      <option value="on">On</option>
      <option value="off">Off</option>
    </Select>
  );
}

function SettingRow({ label, hint, children }: { label: string; hint?: string; children: React.ReactNode }) {
  return (
    <div className="grid grid-cols-[200px_1fr] items-start gap-3">
      <div className="pt-1.5 text-[12.5px] text-fg">
        {label}
        {hint && <div className="mt-0.5 text-[11.5px] text-faint">{hint}</div>}
      </div>
      <div>{children}</div>
    </div>
  );
}

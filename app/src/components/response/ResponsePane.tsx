import { memo, useEffect, useMemo, useState } from "react";
import { AlertTriangle, Braces, Clock, Copy, Download, FileCode2, Globe, Lock, Send, ShieldAlert, Unplug, WifiOff, WrapText } from "lucide-react";
import type { ErrorKind } from "../../bindings/ErrorKind";
import type { Header } from "../../bindings/Header";
import type { SendResult } from "../../bindings/SendResult";
import { formatBytes, formatMs, statusTone, toneBg } from "../../lib/format";
import { copyText, modKey, pickSavePath } from "../../lib/platform";
import { api, errorMessage } from "../../lib/rpc";
import { cancel, type Tab, updateTab } from "../../store/tabs";
import { toast } from "../../store/toasts";
import { openModal } from "../../store/ui";
import { CodeEditor, type EditorLanguage } from "../CodeEditor";
import { Badge, Banner, Button, cx, EmptyState, IconButton, Kbd, Segmented, Spinner, type TabItem, Tabs } from "../ui";
import { MockResponseButton } from "../servers/MockDialogs";
import { InfoView, TimingView } from "./ResponseInfo";
import { ConsoleView, describeFailure, TestsLabel, TestsView } from "./ScriptResults";

export function ResponsePane({ tab }: { tab: Tab }) {
  const r = tab.response;
  if (r.status === "idle") {
    return (
      <EmptyState icon={<Send size={30} strokeWidth={1.5} />} title="Send a request to see the response">
        <div className="mt-2 flex flex-col items-center gap-1.5">
          <span>
            <Kbd>{modKey}</Kbd> <Kbd>Enter</Kbd> send · <Kbd>{modKey}</Kbd> <Kbd>S</Kbd> save · <Kbd>{modKey}</Kbd> <Kbd>K</Kbd> go to…
          </span>
        </div>
      </EmptyState>
    );
  }
  if (r.status === "loading") return <Loading startedAt={r.startedAt} onCancel={() => cancel(tab.id)} />;
  if (r.status === "error") return <ErrorView kind={r.kind} code={r.code} message={r.message} durationMs={r.durationMs} />;
  if (r.status === "other") return null; // shown by that kind's own pane
  return <ResultView tabId={tab.id} responseTab={tab.responseTab} result={r.result} />;
}

function Loading({ startedAt, onCancel }: { startedAt: number; onCancel: () => void }) {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), 100);
    return () => clearInterval(t);
  }, []);
  return (
    <div className="flex h-full flex-col items-center justify-center gap-3">
      <Spinner size={22} />
      <div className="font-mono text-[13px] tabular-nums text-muted">{formatMs(now - startedAt)}</div>
      <Button size="sm" onClick={onCancel}>
        Cancel
      </Button>
    </div>
  );
}

type ErrorInfo = { title: string; icon: React.ReactNode; hint?: string };

const ERROR_INFO: Partial<Record<ErrorKind, ErrorInfo>> = {
  dns: { title: "Could not resolve host", icon: <Globe size={28} />, hint: "Check the host name, your network/VPN, and DNS settings." },
  connect: { title: "Could not connect", icon: <Unplug size={28} />, hint: "Is the server running and reachable on that port? A firewall or proxy may block it." },
  tls: { title: "TLS / certificate error", icon: <ShieldAlert size={28} />, hint: "For corporate or self-signed certificates, add the CA in Settings → Certificates, or turn off verification for this request." },
  proxy: { title: "Proxy error", icon: <WifiOff size={28} />, hint: "Check proxy settings in Settings → Proxy." },
  timeout: { title: "Request timed out", icon: <Clock size={28} />, hint: "Increase the timeout in the request's Settings tab or in app Settings." },
  cancelled: { title: "Request cancelled", icon: <AlertTriangle size={28} /> },
  tooManyRedirects: { title: "Too many redirects", icon: <AlertTriangle size={28} /> },
  notAllowed: { title: "Not approved", icon: <AlertTriangle size={28} />, hint: "An AI agent's request went to a host you haven't approved." },
  invalidRequest: { title: "Invalid request", icon: <AlertTriangle size={28} /> },
  protocol: { title: "Protocol error", icon: <AlertTriangle size={28} /> },
};

function ErrorView({ kind, code, message, durationMs }: { kind: ErrorKind | null; code: string; message: string; durationMs: number }) {
  const undefinedVar = code === "undefinedVariable";
  const info: ErrorInfo = undefinedVar
    ? { title: "Undefined variable", icon: <Braces size={28} /> }
    : code === "script"
      ? { title: "Pre-request script failed", icon: <FileCode2 size={28} />, hint: "Nothing was sent. Fix the script in the Scripts tab of the request, its folder or the workspace." }
      : ((kind && ERROR_INFO[kind]) ?? { title: "Request failed", icon: <AlertTriangle size={28} /> });
  const cancelled = kind === "cancelled";
  return (
    <div className="flex h-full flex-col items-center justify-center gap-3 p-8 text-center">
      <div className={cancelled ? "text-faint" : "text-danger"}>{info.icon}</div>
      <div className="text-[14px] font-semibold text-fg">{info.title}</div>
      {!cancelled && (
        <div className="selectable max-w-xl whitespace-pre-wrap break-words rounded-xl border border-line bg-panel-2 px-3.5 py-2.5 text-left font-mono text-[12px] text-muted">
          {message}
        </div>
      )}
      {info.hint && <div className="max-w-md text-[12.5px] text-muted">{info.hint}</div>}
      <div className="flex items-center gap-2">
        {!undefinedVar && <span className="text-[11.5px] text-faint">after {formatMs(durationMs)}</span>}
        {undefinedVar && (
          <Button size="sm" onClick={() => openModal({ type: "environments" })}>
            Open environments
          </Button>
        )}
        {(kind === "tls" || kind === "proxy") && (
          <Button size="sm" variant="ghost" onClick={() => openModal({ type: "settings" })}>
            Open settings
          </Button>
        )}
      </div>
    </div>
  );
}

function languageFor(contentType: string | null): EditorLanguage {
  const ct = (contentType ?? "").toLowerCase();
  if (ct.includes("json")) return "json";
  if (ct.includes("html")) return "html";
  if (ct.includes("xml")) return "xml";
  if (ct.includes("javascript")) return "javascript";
  return "text";
}

// Memoized on the result: the tab object changes on every keystroke in the request editor, and
// re-rendering a large response (thousands of header rows) each time makes typing lag.
const ResultView = memo(function ResultView({ tabId, responseTab, result }: { tabId: string; responseTab: string; result: SendResult }) {
  const { meta, timing, body } = result;
  const tone = statusTone(meta.status);
  const items: TabItem<string>[] = [
    { id: "body", label: "Body" },
    { id: "headers", label: "Headers", badge: meta.headers.length },
    { id: "cookies", label: "Cookies", badge: meta.cookies.length },
    { id: "timing", label: "Timing" },
    { id: "info", label: "Info" },
  ];
  const scripts = result.scripts;
  if (scripts?.tests.length) items.splice(1, 0, { id: "tests", label: <TestsLabel report={scripts} /> });
  if (scripts && (scripts.console.length || scripts.errors.length)) {
    items.push({ id: "console", label: "Console", badge: scripts.console.length + scripts.errors.length });
  }
  const current = items.some((i) => i.id === responseTab) ? responseTab : "body";
  return (
    <div className="flex h-full min-h-0 flex-col" data-testid="response">
      <div className="flex h-11 shrink-0 items-center gap-3 px-3 pt-1">
        <Badge className={cx("text-[12px]", toneBg[tone])}>
          <span data-testid="response-status">
            {meta.status} {meta.statusText}
          </span>
        </Badge>
        <span className="text-[12px] tabular-nums text-muted" title="Total time">
          {formatMs(timing.totalMs)}
        </span>
        <span className="text-[12px] tabular-nums text-muted" title={`Wire size ${formatBytes(result.bodyWireSize)}`}>
          {formatBytes(body.size)}
        </span>
        <span className="text-[12px] text-faint">{meta.httpVersion}</span>
        {meta.tls && <Lock size={12} className="text-success" aria-label="TLS" />}
        {meta.redirects.length > 0 && <span className="text-[12px] text-faint">{meta.redirects.length} redirect{meta.redirects.length > 1 ? "s" : ""}</span>}
        <div className="flex-1" />
        <MockResponseButton tabId={tabId} result={result} />
      </div>
      {result.unresolved.length > 0 && (
        <Banner tone="warning">
          Sent with undefined values: <span className="font-mono">{result.unresolved.map((v) => (v.startsWith(":") ? v : `{{${v}}}`)).join(", ")}</span>
        </Banner>
      )}
      {scripts?.errors.map((f, i) => (
        <Banner key={i} tone="danger">
          {describeFailure(f)}
        </Banner>
      ))}
      <Tabs items={items} value={current} onChange={(responseTab) => updateTab(tabId, () => ({ responseTab }))} />
      <div className="min-h-0 flex-1 overflow-auto">
        {current === "body" && <BodyView result={result} />}
        {current === "tests" && scripts && <TestsView report={scripts} />}
        {current === "console" && scripts && <ConsoleView report={scripts} />}
        {current === "headers" && <HeaderTable headers={meta.headers} />}
        {current === "cookies" && <CookiesTable result={result} />}
        {current === "timing" && <TimingView timing={timing} protocol={meta.httpVersion} />}
        {current === "info" && <InfoView meta={meta} />}
      </div>
    </div>
  );
});

function BodyView({ result }: { result: SendResult }) {
  const { body } = result;
  const isHtml = (body.contentType ?? "").toLowerCase().includes("html");
  const modes = [
    ...(body.pretty ? [{ id: "pretty", label: "Pretty" }] : []),
    { id: "raw", label: "Raw" },
    ...(isHtml || body.kind === "image" ? [{ id: "preview", label: "Preview" }] : []),
  ];
  const [mode, setMode] = useState<string>(body.kind === "image" ? "preview" : body.pretty ? "pretty" : "raw");
  const [wrap, setWrap] = useState(true);
  const effective = modes.some((m) => m.id === mode) ? mode : modes[0].id;

  const save = async () => {
    const path = await pickSavePath("Save response body", "response");
    if (!path) return;
    try {
      await api.saveResponse(result.responseId, path);
      toast("success", "Response saved", path);
    } catch (e) {
      toast("error", "Could not save response", errorMessage(e));
    }
  };

  const text = effective === "pretty" ? (body.pretty ?? "") : (body.text ?? "");
  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex h-9 shrink-0 items-center gap-2 px-3">
        {body.kind === "text" || body.kind === "image" ? (
          <Segmented items={modes} value={effective} onChange={setMode} />
        ) : (
          <span className="text-[12px] text-muted">Binary · hex preview</span>
        )}
        <span className="truncate text-[11.5px] text-faint">{body.contentType}</span>
        <div className="flex-1" />
        {body.kind === "text" && (
          <IconButton label={wrap ? "Don't wrap lines" : "Wrap lines"} active={wrap} onClick={() => setWrap(!wrap)}>
            <WrapText size={14} />
          </IconButton>
        )}
        {body.kind === "text" && (
          <IconButton
            label="Copy body"
            onClick={() => {
              void copyText(text);
              toast("success", "Copied to clipboard");
            }}
          >
            <Copy size={14} />
          </IconButton>
        )}
        <IconButton label="Save to file…" onClick={save}>
          <Download size={14} />
        </IconButton>
      </div>
      {body.downloadTruncated && <Banner tone="warning">The response was larger than the size limit and was cut. Raise the limit in Settings.</Banner>}
      {body.displayTruncated && <Banner tone="info">Only the first part is shown. Use Save to file to get the whole body.</Banner>}
      {body.decodeWarning && <Banner tone="warning">{body.decodeWarning}</Banner>}
      <div className="min-h-0 flex-1">
        {body.size === 0 ? (
          <EmptyState title="Empty body">The response has no content.</EmptyState>
        ) : body.kind === "image" ? (
          <div className="flex h-full items-center justify-center overflow-auto bg-[repeating-conic-gradient(var(--panel-2)_0%_25%,transparent_0%_50%)] bg-[length:16px_16px] p-4">
            {body.base64 ? (
              <img alt="Response" src={`data:${body.contentType};base64,${body.base64}`} className="max-h-full max-w-full object-contain" />
            ) : (
              <span className="text-muted">Image too large to preview</span>
            )}
          </div>
        ) : body.kind === "binary" ? (
          <HexView base64={body.base64 ?? ""} />
        ) : effective === "preview" ? (
          // Sandboxed: no scripts, no same-origin access.
          <iframe title="HTML preview" sandbox="" srcDoc={body.text ?? ""} className="h-full w-full bg-white" />
        ) : (
          <CodeEditor value={text} readOnly language={effective === "pretty" ? "json" : languageFor(body.contentType)} lineWrapping={wrap} />
        )}
      </div>
    </div>
  );
}

function HexView({ base64 }: { base64: string }) {
  const lines = useMemo(() => {
    const bin = atob(base64);
    const out: string[] = [];
    for (let off = 0; off < bin.length; off += 16) {
      const chunk = bin.slice(off, off + 16);
      const hex = [...chunk].map((c) => c.charCodeAt(0).toString(16).padStart(2, "0")).join(" ");
      const ascii = [...chunk].map((c) => (c.charCodeAt(0) >= 32 && c.charCodeAt(0) < 127 ? c : ".")).join("");
      out.push(`${off.toString(16).padStart(8, "0")}  ${hex.padEnd(48)}  ${ascii}`);
    }
    return out.join("\n");
  }, [base64]);
  return <CodeEditor value={lines} readOnly lineNumbers={false} lineWrapping={false} />;
}

export function HeaderTable({ headers }: { headers: Header[] }) {
  if (!headers.length) return <EmptyState title="No headers" />;
  return (
    <table className="selectable w-full border-collapse text-[12.5px]">
      <tbody>
        {headers.map((h, i) => (
          <tr key={i} className="group border-b border-line/60 align-top last:border-b-0 hover:bg-hover/40">
            <td className="w-[34%] px-3 py-1.5 font-mono font-medium text-fg">{h.name}</td>
            <td className="break-all px-3 py-1.5 font-mono text-muted">{h.value}</td>
            <td className="w-8 pr-2">
              <button
                aria-label="Copy value"
                onClick={() => void copyText(h.value)}
                className="rounded p-1 text-faint opacity-0 hover:bg-hover hover:text-fg group-hover:opacity-100"
              >
                <Copy size={12} />
              </button>
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function CookiesTable({ result }: { result: SendResult }) {
  const cookies = result.meta.cookies;
  if (!cookies.length) return <EmptyState title="No cookies set by this response" />;
  return (
    <table className="selectable w-full border-collapse text-[12.5px]">
      <thead>
        <tr className="border-b border-line/60 text-left text-[11px] uppercase tracking-wide text-faint">
          {["Name", "Value", "Domain", "Path", "Expires", "Flags"].map((h) => (
            <th key={h} className="px-3 py-1.5 font-semibold">
              {h}
            </th>
          ))}
        </tr>
      </thead>
      <tbody>
        {cookies.map((c, i) => (
          <tr key={i} className="border-b border-line/60 align-top last:border-b-0">
            <td className="px-3 py-1.5 font-mono text-fg">{c.name}</td>
            <td className="max-w-[240px] break-all px-3 py-1.5 font-mono text-muted">{c.value}</td>
            <td className="px-3 py-1.5 text-muted">{c.domain}</td>
            <td className="px-3 py-1.5 text-muted">{c.path}</td>
            <td className="px-3 py-1.5 text-muted">{c.expires ? new Date(c.expires).toLocaleString() : "Session"}</td>
            <td className="px-3 py-1.5 text-muted">{[c.secure && "Secure", c.httpOnly && "HttpOnly", c.sameSite].filter(Boolean).join(", ")}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

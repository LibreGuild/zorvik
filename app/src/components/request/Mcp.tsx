// MCP requests: the Call tab (tool, resource or prompt, and its arguments), the Connection tab
// (transport; a program's folder and environment), and the result pane: the call's answer, its
// tests, what the server offers (from the tab's session) and every message of the session.
import { memo, useEffect, useMemo, useState } from "react";
import { ArrowDownLeft, ArrowUpRight, Boxes, FileText, MessageSquareText, Plug, PlugZap, RefreshCw, Terminal, Trash2, Wrench } from "lucide-react";
import type { McpCallKind } from "../../bindings/McpCallKind";
import type { McpOptions } from "../../bindings/McpOptions";
import type { McpTransport } from "../../bindings/McpTransport";
import type { SendResult } from "../../bindings/SendResult";
import { formatBytes, formatMs, statusTone, toneBg } from "../../lib/format";
import { errorMessage } from "../../lib/rpc";
import {
  argumentSkeleton,
  argumentsOf,
  blankArguments,
  type CatalogItem,
  catalogItems,
  clearLog,
  connectMcp,
  disconnectMcp,
  isProgram,
  type McpLogEntry,
  type McpSession,
  refreshCatalog,
  selectCall,
  targetOf,
  useMcp,
} from "../../store/mcp";
import { type Tab, updateDraft, updateTab } from "../../store/tabs";
import { toast } from "../../store/toasts";
import { useVariableNames } from "../../store/workspace";
import { CodeEditor } from "../CodeEditor";
import { KeyValueEditor } from "../KeyValueEditor";
import { ResponsePane } from "../response/ResponsePane";
import { ConsoleView, describeFailure, TestsLabel, TestsView } from "../response/ScriptResults";
import { VarInput } from "../VarInput";
import { Badge, Banner, Button, cx, EmptyState, Field, Input, Segmented, Select, Tabs, type TabItem } from "../ui";
import type { KindPaneProps } from "./kinds";

const CALL_KINDS: { id: McpCallKind; label: string }[] = [
  { id: "tool", label: "Tool" },
  { id: "resource", label: "Resource" },
  { id: "prompt", label: "Prompt" },
];

const NAME_LABEL: Record<McpCallKind, string> = { tool: "Tool", resource: "Resource URI", prompt: "Prompt" };
const NAME_PLACEHOLDER: Record<McpCallKind, string> = { tool: "get_weather", resource: "file:///docs/readme.md or users://{id}", prompt: "code_review" };

const TRANSPORTS: { id: McpTransport; label: string }[] = [
  { id: "auto", label: "Automatic: HTTP for http(s):// addresses, otherwise a program" },
  { id: "streamableHttp", label: "Streamable HTTP" },
  { id: "sse", label: "HTTP+SSE (older servers)" },
  { id: "stdio", label: "Program (stdio)" },
];

function optionsOf(tab: Tab): McpOptions {
  return tab.draft.mcp ?? {};
}

function setOptions(tab: Tab, patch: Partial<McpOptions>) {
  updateDraft(tab.id, (r) => ({ ...r, mcp: { ...(r.mcp ?? {}), ...patch } }));
}

function useSession(tabId: string): McpSession | undefined {
  return useMcp((s) => s.sessions[tabId]);
}

async function connect(tab: Tab) {
  try {
    await connectMcp(tab);
  } catch (e) {
    toast("error", "Could not connect", errorMessage(e));
  }
}

// ---- editor tabs ---------------------------------------------------------------

export function McpCallTab({ tab }: KindPaneProps) {
  const { names } = useVariableNames();
  const mcp = optionsOf(tab);
  const kind = mcp.call ?? "tool";
  const session = useSession(tab.id);
  const items = useMemo(() => catalogItems(session?.catalog, kind), [session?.catalog, kind]);
  const item = items.find((i) => i.name === (mcp.name ?? "").trim());
  const args = argumentsOf(item, kind);
  return (
    <div className="flex h-full min-h-0 flex-col gap-3 p-4">
      <div className="flex flex-wrap items-end gap-3">
        <Field label="Call">
          <Segmented items={CALL_KINDS} value={kind} onChange={(call) => setOptions(tab, { call })} />
        </Field>
        <Field label={NAME_LABEL[kind]} className="min-w-[240px] flex-1">
          <div className="flex h-8 items-center rounded-lg border border-line bg-input focus-within:border-accent">
            <VarInput
              value={mcp.name ?? ""}
              onChange={(name) => setOptions(tab, { name })}
              placeholder={NAME_PLACEHOLDER[kind]}
              suggestions={items.map((i) => i.name)}
              ariaLabel={NAME_LABEL[kind]}
              className="flex-1"
            />
          </div>
        </Field>
      </div>
      {item ? (
        <div className="rounded-lg border border-line bg-panel-2 px-3 py-2 text-[12.5px]" data-testid="mcp-selected">
          {(item.title || item.description) && (
            <div className="text-muted">
              {item.title && <span className="font-medium text-fg">{item.title}. </span>}
              {item.description}
            </div>
          )}
          {args.length > 0 && (
            <ul className="mt-1.5 flex flex-col gap-0.5">
              {args.map((a) => (
                <li key={a.name} className="font-mono text-[12px]">
                  <span className="text-fg">{a.name}</span>
                  {a.required && <span className="text-danger">*</span>}
                  <span className="text-faint">: {a.type}</span>
                  {a.description && <span className="font-sans text-muted"> — {a.description}</span>}
                </li>
              ))}
            </ul>
          )}
        </div>
      ) : (
        <p className="text-[12px] text-faint">
          {session?.status === "open"
            ? `Pick one of the server's ${kind === "resource" ? "resources" : `${kind}s`} (the name field suggests them), or type its name.`
            : "Connect (in the result pane) to see what the server offers, or type the name."}
        </p>
      )}
      <div className="flex items-center justify-between">
        <div className="text-[11px] font-semibold uppercase tracking-wide text-faint">Arguments (JSON)</div>
        {item && args.length > 0 && (
          <Button
            size="sm"
            variant="ghost"
            onClick={() => {
              if (!blankArguments(mcp.arguments)) toast("info", "Arguments replaced", "They now start from the schema.");
              setOptions(tab, { arguments: argumentSkeleton(item, kind) });
            }}
          >
            Start from the schema
          </Button>
        )}
      </div>
      <div className="min-h-[120px] flex-1 overflow-hidden rounded-lg border border-line bg-input focus-within:border-accent" data-testid="mcp-arguments">
        <CodeEditor
          value={mcp.arguments ?? ""}
          onChange={(value) => setOptions(tab, { arguments: value })}
          language="json"
          variables={names}
          placeholder={kind === "prompt" ? '{"code": "{{snippet}}"}' : '{"city": "{{city}}"}'}
        />
      </div>
      <p className="text-[12px] text-faint">
        {kind === "prompt"
          ? "Prompt arguments are sent as text."
          : kind === "resource"
            ? "A URI template's {parts} are filled in from the arguments."
            : "Values can use {{variables}}."}{" "}
        Send makes the call; post-response scripts see the answer as the body (<code className="font-mono">pm.response.json()</code>).
      </p>
    </div>
  );
}

export function McpConnectionTab({ tab }: KindPaneProps) {
  const mcp = optionsOf(tab);
  const program = isProgram(tab.draft.url, mcp.transport);
  const session = useSession(tab.id);
  const live = session?.status === "open" || session?.status === "connecting";
  return (
    <div className="flex max-w-2xl flex-col gap-4 p-4">
      <p className="text-[12.5px] text-muted">
        The address is the server's URL (<code className="font-mono">http://localhost:3000/mcp</code>) or, for a local server, the command that starts it (
        <code className="font-mono">npx -y @modelcontextprotocol/server-everything</code>). Zorvik asks before it starts a program for the first time.
      </p>
      {live && (
        <div className="-mx-3 -mt-2">
          <Banner tone="info">Connection settings apply the next time you connect.</Banner>
        </div>
      )}
      <Field label="Transport">
        <Select aria-label="Transport" value={mcp.transport ?? "auto"} onChange={(e) => setOptions(tab, { transport: e.target.value as McpTransport })}>
          {TRANSPORTS.map((t) => (
            <option key={t.id} value={t.id}>
              {t.label}
            </option>
          ))}
        </Select>
      </Field>
      {program ? (
        <>
          <Field label="Folder" hint="Where the program starts, relative to the workspace folder (empty: the workspace folder).">
            <Input aria-label="Folder" className="font-mono text-[12.5px]" value={mcp.cwd ?? ""} placeholder="." onChange={(e) => setOptions(tab, { cwd: e.target.value })} />
          </Field>
          <div>
            <div className="pb-1 text-[12px] font-medium text-muted">Environment</div>
            <KeyValueEditor rows={mcp.env ?? []} onChange={(env) => setOptions(tab, { env })} keyPlaceholder="Variable" valuePlaceholder="Value (can use {{variables}})" />
            <p className="px-1 pt-1.5 text-[12px] text-faint">
              Added to the environment Zorvik runs in. Put tokens in secret variables, e.g. <code className="font-mono">{"{{githubToken}}"}</code>.
            </p>
          </div>
        </>
      ) : (
        <p className="text-[12px] text-faint">
          Headers and Auth (their tabs) go with every HTTP request of the session, e.g. a bearer token for a remote MCP server.
        </p>
      )}
    </div>
  );
}

// ---- result pane ---------------------------------------------------------------

type PaneTab = "result" | "tests" | "console" | "server" | "messages";

export function McpResultPane({ tab }: KindPaneProps) {
  const session = useSession(tab.id);
  const r = tab.response;
  // A new answer (or error) shows itself, even while the server's catalog or the log is open.
  const at = r.status === "done" || r.status === "error" ? r.at : null;
  useEffect(() => {
    if (at != null) updateTab(tab.id, (t) => (t.responseTab === "server" || t.responseTab === "messages" ? { responseTab: "result" } : {}));
  }, [at, tab.id]);
  const result = r.status === "done" ? r.result : null;
  const scripts = result?.scripts;
  const items: TabItem<PaneTab>[] = [
    { id: "result", label: "Result" },
    ...(scripts?.tests.length ? [{ id: "tests" as const, label: <TestsLabel report={scripts} /> }] : []),
    ...(scripts && (scripts.console.length || scripts.errors.length) ? [{ id: "console" as const, label: "Console", badge: scripts.console.length + scripts.errors.length }] : []),
    { id: "server", label: "Server" },
    { id: "messages", label: "Messages", badge: session?.log.length || undefined },
  ];
  const current = (items.some((i) => i.id === tab.responseTab) ? tab.responseTab : "result") as PaneTab;
  return (
    <div className="flex h-full min-h-0 flex-col" data-testid="mcp-pane">
      <SessionBar tab={tab} session={session} />
      <Tabs items={items} value={current} onChange={(responseTab) => updateTab(tab.id, () => ({ responseTab }))} />
      <div className="min-h-0 flex-1 overflow-auto">
        {current === "result" &&
          (result ? (
            <CallResult result={result} />
          ) : r.status === "idle" ? (
            <EmptyState icon={<Wrench size={28} strokeWidth={1.5} />} title="Send to make the call">
              Connect to see the server's tools, resources and prompts, pick one in the Call tab, then Send.
            </EmptyState>
          ) : (
            <ResponsePane tab={tab} />
          ))}
        {current === "tests" && scripts && <TestsView report={scripts} />}
        {current === "console" && scripts && <ConsoleView report={scripts} />}
        {current === "server" && <ServerView tab={tab} session={session} />}
        {current === "messages" && <MessagesView tabId={tab.id} session={session} />}
      </div>
    </div>
  );
}

function SessionBar({ tab, session }: { tab: Tab; session: McpSession | undefined }) {
  const status = session?.status ?? "closed";
  const moved = status === "open" && session?.target !== targetOf(tab.draft);
  return (
    <div className="flex min-h-11 shrink-0 flex-wrap items-center gap-x-3 gap-y-1 px-3 py-1.5 text-[12px]" data-testid="mcp-session">
      <span className={cx("h-2 w-2 shrink-0 rounded-full", status === "open" ? "bg-success" : status === "connecting" ? "bg-warning" : "bg-line-strong")} />
      {status === "open" && session?.info ? (
        <span className="min-w-0 truncate text-muted">
          <span className="font-medium text-fg">{session.info.title || session.info.name}</span> {session.info.version} · {session.info.transport} · MCP{" "}
          {session.info.protocolVersion}
          {session.info.pid != null && ` · pid ${session.info.pid}`}
        </span>
      ) : status === "connecting" ? (
        <span className="text-muted">Connecting…</span>
      ) : (
        <span className="min-w-0 truncate text-faint">{session?.error ? `Not connected: ${session.error}` : "Not connected: each Send connects, calls and disconnects"}</span>
      )}
      <div className="flex-1" />
      {moved && <span className="text-warning">The address changed: Send connects anew until you reconnect</span>}
      {status === "closed" ? (
        <Button size="sm" variant="primary" icon={<PlugZap size={13} />} onClick={() => void connect(tab)}>
          Connect
        </Button>
      ) : (
        <>
          {moved && (
            <Button size="sm" icon={<RefreshCw size={13} />} onClick={() => void connect(tab)}>
              Reconnect
            </Button>
          )}
          <Button size="sm" icon={<Plug size={13} />} onClick={() => void disconnectMcp(tab.id)}>
            Disconnect
          </Button>
        </>
      )}
    </div>
  );
}

// ---- the call's answer ----------------------------------------------------------------

type Json = Record<string, unknown>;

function parse(text: string | null | undefined): Json | null {
  try {
    const v = JSON.parse(text ?? "");
    return v && typeof v === "object" && !Array.isArray(v) ? (v as Json) : null;
  } catch {
    return null;
  }
}

const CallResult = memo(function CallResult({ result }: { result: SendResult }) {
  const [raw, setRaw] = useState(false);
  const { meta, timing, body } = result;
  const answer = parse(body.text);
  return (
    <div className="flex min-h-full flex-col" data-testid="mcp-result">
      <div className="flex h-10 shrink-0 items-center gap-3 px-3">
        <Badge className={cx("text-[12px]", toneBg[statusTone(meta.status)])}>
          <span data-testid="response-status">{meta.status === 200 ? "OK" : meta.statusText}</span>
        </Badge>
        <span className="text-[12px] tabular-nums text-muted">{formatMs(timing.totalMs)}</span>
        <span className="text-[12px] tabular-nums text-muted">{formatBytes(body.size)}</span>
        <span className="truncate text-[12px] text-faint">{meta.request.method}</span>
        <div className="flex-1" />
        <Segmented
          items={[
            { id: "view", label: "View" },
            { id: "raw", label: "JSON" },
          ]}
          value={raw ? "raw" : "view"}
          onChange={(v) => setRaw(v === "raw")}
        />
      </div>
      {result.unresolved.length > 0 && (
        <Banner tone="warning">
          Sent with undefined values: <span className="font-mono">{result.unresolved.map((v) => `{{${v}}}`).join(", ")}</span>
        </Banner>
      )}
      {result.scripts?.errors.map((f, i) => (
        <Banner key={i} tone="danger">
          {describeFailure(f)}
        </Banner>
      ))}
      {raw || !answer ? (
        <div className="min-h-0 flex-1">
          <CodeEditor value={body.pretty ?? body.text ?? ""} language="json" readOnly />
        </div>
      ) : (
        <AnswerView answer={answer} />
      )}
    </div>
  );
});

function AnswerView({ answer }: { answer: Json }) {
  const error = answer.error as Json | undefined;
  if (error) {
    return (
      <div className="m-3 rounded-lg border border-danger/40 bg-danger/5 p-3 text-[12.5px]" data-testid="mcp-error">
        <div className="font-semibold text-danger">
          Error {String(error.code)}: {String(error.message ?? "")}
        </div>
        {error.data !== undefined && error.data !== null && <pre className="selectable mt-2 whitespace-pre-wrap font-mono text-[12px] text-muted">{pretty(error.data)}</pre>}
      </div>
    );
  }
  const content = Array.isArray(answer.content) ? (answer.content as Json[]) : null;
  const contents = Array.isArray(answer.contents) ? (answer.contents as Json[]) : null;
  const messages = Array.isArray(answer.messages) ? (answer.messages as Json[]) : null;
  return (
    <div className="flex flex-col gap-2 p-3">
      {answer.isError === true && <Banner tone="danger">The tool reported that it failed (isError).</Banner>}
      {typeof answer.description === "string" && answer.description && <p className="text-[12.5px] text-muted">{answer.description}</p>}
      {content?.map((c, i) => <ContentBlock key={i} block={c} />)}
      {answer.structuredContent !== undefined && (
        <Block title="Structured content">
          <pre className="selectable whitespace-pre-wrap font-mono text-[12px]">{pretty(answer.structuredContent)}</pre>
        </Block>
      )}
      {contents?.map((c, i) => <ResourceContents key={i} contents={c} />)}
      {messages?.map((m, i) => (
        <Block key={i} title={String(m.role ?? "")}>
          <ContentBlock block={(m.content ?? {}) as Json} bare />
        </Block>
      ))}
      {content?.length === 0 && <p className="text-[12.5px] text-faint">The answer has no content.</p>}
    </div>
  );
}

function pretty(v: unknown): string {
  return typeof v === "string" ? v : JSON.stringify(v, null, 2);
}

function Block({ title, children }: { title: React.ReactNode; children: React.ReactNode }) {
  return (
    <div className="overflow-hidden rounded-lg border border-line">
      <div className="border-b border-line bg-panel-2 px-2.5 py-1 text-[11px] font-semibold uppercase tracking-wide text-faint">{title}</div>
      <div className="p-2.5">{children}</div>
    </div>
  );
}

/** Text shown as it is (JSON in it pretty-printed), images and audio played, resources linked or embedded. */
function ContentBlock({ block, bare = false }: { block: Json; bare?: boolean }) {
  const type = String(block.type ?? "");
  let inner: React.ReactNode;
  if (type === "text") {
    const text = String(block.text ?? "");
    const json = parse(text) ?? (/^\s*\[/.test(text) ? safeJson(text) : null);
    inner = <pre className="selectable whitespace-pre-wrap break-words font-mono text-[12.5px] text-fg" data-testid="mcp-text">{json ? JSON.stringify(json, null, 2) : text}</pre>;
  } else if (type === "image" && typeof block.data === "string") {
    inner = <img alt="Image from the server" className="max-h-80 max-w-full rounded" src={`data:${String(block.mimeType ?? "image/png")};base64,${block.data}`} />;
  } else if (type === "audio" && typeof block.data === "string") {
    inner = <audio controls src={`data:${String(block.mimeType ?? "audio/wav")};base64,${block.data}`} />;
  } else if (type === "resource_link") {
    inner = (
      <div className="text-[12.5px]">
        <span className="font-mono text-fg">{String(block.uri ?? "")}</span>
        {typeof block.description === "string" && <span className="text-muted"> — {block.description}</span>}
      </div>
    );
  } else if (type === "resource" && block.resource && typeof block.resource === "object") {
    inner = <ResourceContents contents={block.resource as Json} bare />;
  } else {
    inner = <pre className="selectable whitespace-pre-wrap font-mono text-[12px]">{pretty(block)}</pre>;
  }
  if (bare) return <>{inner}</>;
  return <Block title={type || "content"}>{inner}</Block>;
}

function safeJson(text: string): unknown {
  try {
    return JSON.parse(text);
  } catch {
    return null;
  }
}

function ResourceContents({ contents, bare = false }: { contents: Json; bare?: boolean }) {
  const body =
    typeof contents.text === "string" ? (
      <pre className="selectable whitespace-pre-wrap break-words font-mono text-[12.5px]">{contents.text}</pre>
    ) : typeof contents.blob === "string" ? (
      <p className="text-[12px] text-muted">Binary data, {formatBytes(Math.floor((contents.blob.length * 3) / 4))}</p>
    ) : null;
  const title = (
    <span className="normal-case tracking-normal">
      <span className="font-mono">{String(contents.uri ?? "")}</span>
      {typeof contents.mimeType === "string" && <span className="font-normal"> · {contents.mimeType}</span>}
    </span>
  );
  if (bare) {
    return (
      <div>
        <div className="pb-1 text-[11px] text-faint">{title}</div>
        {body}
      </div>
    );
  }
  return <Block title={title}>{body}</Block>;
}

// ---- what the server offers ---------------------------------------------------------------

function ServerView({ tab, session }: { tab: Tab; session: McpSession | undefined }) {
  const [filter, setFilter] = useState("");
  if (!session || session.status !== "open" || !session.info) {
    return (
      <EmptyState
        icon={<Boxes size={28} strokeWidth={1.5} />}
        title="Connect to see what the server offers"
        action={
          session?.status === "connecting" ? undefined : (
            <Button variant="primary" icon={<PlugZap size={14} />} onClick={() => void connect(tab)}>
              Connect
            </Button>
          )
        }
      >
        Its tools with their input schemas, resources and prompts. Pick one to call it.
      </EmptyState>
    );
  }
  const { info, catalog } = session;
  const words = filter.toLowerCase().split(/\s+/).filter(Boolean);
  const match = (i: CatalogItem) => words.every((w) => `${i.name} ${i.title} ${i.description}`.toLowerCase().includes(w));
  const sections: { kind: McpCallKind; title: string; icon: React.ReactNode; items: CatalogItem[] }[] = [
    { kind: "tool", title: "Tools", icon: <Wrench size={13} />, items: catalogItems(catalog, "tool") },
    { kind: "resource", title: "Resources", icon: <FileText size={13} />, items: catalogItems(catalog, "resource") },
    { kind: "prompt", title: "Prompts", icon: <MessageSquareText size={13} />, items: catalogItems(catalog, "prompt") },
  ];
  const selected = tab.draft.mcp?.name ?? "";
  const selectedKind = tab.draft.mcp?.call ?? "tool";
  return (
    <div className="flex flex-col gap-3 p-3" data-testid="mcp-catalog">
      {info.instructions && (
        <div className="rounded-lg border border-line bg-panel-2 px-3 py-2 text-[12.5px]">
          <div className="pb-0.5 text-[11px] font-semibold uppercase tracking-wide text-faint">Instructions</div>
          <div className="selectable whitespace-pre-wrap text-muted">{info.instructions}</div>
        </div>
      )}
      <div className="flex items-center gap-2">
        <Input value={filter} onChange={(e) => setFilter(e.target.value)} placeholder="Filter" aria-label="Filter" className="h-8 max-w-xs" />
        <Button size="sm" variant="ghost" icon={<RefreshCw size={13} />} loading={session.catalogLoading} onClick={() => void refreshCatalog(tab.id)}>
          Refresh
        </Button>
      </div>
      {catalog?.problems.map((p, i) => (
        <div key={i} className="-mx-3">
          <Banner tone="warning">{p}</Banner>
        </div>
      ))}
      {sections.map((s) => {
        const shown = s.items.filter(match);
        if (!s.items.length) return null;
        return (
          <div key={s.kind}>
            <div className="flex items-center gap-1.5 pb-1 text-[11px] font-semibold uppercase tracking-wide text-faint">
              {s.icon} {s.title} <span className="font-normal">{s.items.length}</span>
            </div>
            <div className="flex flex-col divide-y divide-line overflow-hidden rounded-lg border border-line">
              {shown.map((i) => (
                <button
                  key={`${i.template}:${i.name}`}
                  type="button"
                  onClick={() => {
                    selectCall(tab.id, s.kind, i);
                    updateTab(tab.id, () => ({ requestTab: "call" }));
                  }}
                  className={cx("flex flex-col items-start gap-0.5 px-3 py-2 text-left hover:bg-hover", selected === i.name && selectedKind === s.kind && "bg-accent-soft")}
                  data-testid={`mcp-item-${s.kind}`}
                >
                  <span className="font-mono text-[12.5px] text-fg">
                    {i.name}
                    {i.template && <span className="ml-1.5 font-sans text-[11px] text-faint">template</span>}
                  </span>
                  {(i.title || i.description) && <span className="line-clamp-2 text-[12px] text-muted">{i.title ? `${i.title}. ${i.description}` : i.description}</span>}
                </button>
              ))}
              {!shown.length && <div className="px-3 py-2 text-[12px] text-faint">Nothing matches.</div>}
            </div>
          </div>
        );
      })}
      {catalog && sections.every((s) => !s.items.length) && !session.catalogLoading && <p className="text-[12.5px] text-faint">The server offers no tools, resources or prompts.</p>}
      <details className="text-[12px] text-muted">
        <summary className="cursor-pointer select-none text-faint">Capabilities</summary>
        <pre className="selectable mt-1 whitespace-pre-wrap font-mono text-[12px]">{JSON.stringify(info.capabilities, null, 2)}</pre>
        {info.sessionId && <div className="mt-1">Session: <span className="font-mono">{info.sessionId}</span></div>}
        <div className="mt-1">Connected in {formatMs(info.connectMs)}</div>
      </details>
    </div>
  );
}

// ---- messages -------------------------------------------------------------------------------

function MessagesView({ tabId, session }: { tabId: string; session: McpSession | undefined }) {
  const [open, setOpen] = useState<number | null>(null);
  const log = session?.log ?? [];
  if (!log.length) {
    return (
      <EmptyState icon={<Terminal size={28} strokeWidth={1.5} />} title="No messages yet">
        Connect: every JSON-RPC message of the session shows here, with a program's own log (stderr).
      </EmptyState>
    );
  }
  // Answers name the request they answer.
  const methods = new Map<string, string>();
  for (const e of log) if (e.event.type === "message" && e.event.id && e.event.method) methods.set(`${e.event.direction}:${e.event.id}`, e.event.method);
  return (
    <div className="flex flex-col" data-testid="mcp-messages">
      <div className="flex items-center justify-between px-3 py-1.5 text-[12px] text-faint">
        <span>{session?.dropped ? `${session.dropped} earlier not kept` : `${log.length} entries`}</span>
        <Button size="sm" variant="ghost" icon={<Trash2 size={13} />} onClick={() => clearLog(tabId)}>
          Clear
        </Button>
      </div>
      {log.map((entry) => (
        <LogRow key={entry.seq} entry={entry} answers={methods} open={open === entry.seq} onToggle={() => setOpen(open === entry.seq ? null : entry.seq)} />
      ))}
    </div>
  );
}

function LogRow({ entry, answers, open, onToggle }: { entry: McpLogEntry; answers: Map<string, string>; open: boolean; onToggle: () => void }) {
  const e = entry.event;
  const time = new Date("timestamp" in e ? e.timestamp : entry.at).toLocaleTimeString([], { hour12: false });
  if (e.type !== "message") {
    const text = e.type === "error" ? e.message : e.type === "closed" ? `Closed: ${e.reason}` : e.text;
    return (
      <div className={cx("flex gap-2 border-t border-line px-3 py-1 font-mono text-[12px]", e.type === "error" ? "text-danger" : e.type === "stderr" ? "text-muted" : "text-faint")}>
        <span className="shrink-0 text-faint">{time}</span>
        <span className="shrink-0">{e.type === "stderr" ? "log" : e.type}</span>
        <span className="selectable min-w-0 whitespace-pre-wrap break-words">{text}</span>
      </div>
    );
  }
  const sent = e.direction === "sent";
  const answerTo = !e.method && e.id ? answers.get(`${sent ? "received" : "sent"}:${e.id}`) : undefined;
  const label = e.method ?? (answerTo ? `answer to ${answerTo}` : "answer");
  const failed = !e.method && e.text.includes('"error"');
  return (
    <div className="border-t border-line">
      <button type="button" onClick={onToggle} className="flex w-full items-center gap-2 px-3 py-1 text-left font-mono text-[12px] hover:bg-hover">
        <span className="shrink-0 text-faint">{time}</span>
        {sent ? <ArrowUpRight size={13} className="shrink-0 text-info" aria-label="Sent" /> : <ArrowDownLeft size={13} className="shrink-0 text-success" aria-label="Received" />}
        <span className={cx("min-w-0 flex-1 truncate", failed ? "text-danger" : "text-fg")}>
          {label}
          {e.id && <span className="text-faint"> #{e.id}</span>}
        </span>
        <span className="shrink-0 text-faint">{formatBytes(e.size)}</span>
      </button>
      {open && <pre className="selectable max-h-96 overflow-auto whitespace-pre-wrap break-words bg-panel-2 px-3 py-2 font-mono text-[12px]">{pretty(safeJson(e.text) ?? e.text)}</pre>}
    </div>
  );
}

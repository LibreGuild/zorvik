// Mock API settings: the routes (list + the selected route's answer), what
// happens to requests no route matches, and CORS.
import { useMemo, useState } from "react";
import { ChevronRight, Clock, Copy, CopyPlus, GripVertical, MoreHorizontal, Plus, Route as RouteIcon, Trash2, Wand2, Zap } from "lucide-react";
import type { HttpMockConfig } from "../../bindings/HttpMockConfig";
import type { KeyValue } from "../../bindings/KeyValue";
import type { MockFallback } from "../../bindings/MockFallback";
import type { MockFault } from "../../bindings/MockFault";
import type { MockRoute } from "../../bindings/MockRoute";
import { statusTone, toneText } from "../../lib/format";
import { COMMON_HEADERS, methodColor } from "../../lib/http";
import { copyText } from "../../lib/platform";
import { isServerTab, updateServerTab, useTabs } from "../../store/tabs";
import { toast } from "../../store/toasts";
import { useVariableNames } from "../../store/workspace";
import { CodeEditor } from "../CodeEditor";
import { KeyValueEditor } from "../KeyValueEditor";
import { beautifyJson } from "../request/jsonFormat";
import { VarInput } from "../VarInput";
import { Button, Checkbox, cx, Field, IconButton, Input, Menu, Segmented, Select, Switch, Tooltip } from "../ui";
import type { ServerEditorProps } from "./kinds";
import {
  contentTypeOf,
  followIndex,
  languageForContentType,
  methodText,
  moveItem,
  requestPlaceholders,
  ROUTE_METHODS,
  routeUrl,
  STATUS_CODES,
  unusedPath,
} from "./MockRoutes";
import { EditorSection } from "./ServerView";

const FAULTS: { id: MockFault; label: string; hint: string }[] = [
  { id: "none", label: "None", hint: "" },
  { id: "error", label: "Error (500)", hint: "Answers 500 Internal Server Error instead." },
  { id: "reset", label: "Drop the connection", hint: "Closes the connection without answering." },
  { id: "hang", label: "Never answer", hint: "Keeps the request open until the client gives up." },
];

const FALLBACKS: { id: MockFallback; label: string }[] = [
  { id: "notFound", label: "Answer 404" },
  { id: "proxy", label: "Forward to a backend" },
];

const cell = "h-7 w-full min-w-0 bg-transparent px-2 font-mono text-[12.5px] text-fg outline-none placeholder:text-faint";

const newRoute = (routes: MockRoute[]): MockRoute => ({
  method: "GET",
  path: unusedPath(routes),
  status: 200,
  headers: [{ key: "Content-Type", value: "application/json" }],
  body: "{\n  \n}",
});

/** The selected route is kept in the tab (`section`), so it survives switching tabs. */
function useSelectedRoute(tabId: string, count: number): [number, (i: number) => void] {
  const section = useTabs((s) => {
    const t = s.tabs.find((x) => x.id === tabId);
    return isServerTab(t) ? t.section : undefined;
  });
  const parsed = Number(section?.startsWith("route:") ? section.slice(6) : 0);
  const index = count === 0 ? -1 : Math.min(Math.max(0, Number.isFinite(parsed) ? parsed : 0), count - 1);
  return [index, (i: number) => updateServerTab(tabId, () => ({ section: `route:${i}` }))];
}

export function HttpMockEditor({ server, onChange, running, tabId }: ServerEditorProps) {
  const http: HttpMockConfig = server.http ?? { routes: [] };
  const routes = http.routes;
  const [selected, select] = useSelectedRoute(tabId, routes.length);
  const setHttp = (fn: (h: HttpMockConfig) => HttpMockConfig) => onChange((s) => ({ ...s, http: fn(s.http ?? { routes: [] }) }));
  const setRoutes = (fn: (routes: MockRoute[]) => MockRoute[]) => setHttp((h) => ({ ...h, routes: fn(h.routes) }));
  const updateRoute = (i: number, patch: Partial<MockRoute>) => setRoutes((rs) => rs.map((r, j) => (j === i ? { ...r, ...patch } : r)));

  const add = () => {
    setRoutes((rs) => [...rs, newRoute(rs)]);
    select(routes.length);
  };
  const duplicate = (i: number) => {
    setRoutes((rs) => [...rs.slice(0, i + 1), structuredClone(rs[i]), ...rs.slice(i + 1)]);
    select(i + 1);
  };
  const remove = (i: number) => {
    setRoutes((rs) => rs.filter((_, j) => j !== i));
    if (selected >= i && selected > 0) select(selected - 1);
  };
  const move = (from: number, to: number) => {
    setRoutes((rs) => moveItem(rs, from, to));
    select(followIndex(selected, from, to));
  };
  const route = selected >= 0 ? routes[selected] : undefined;

  return (
    <>
      <EditorSection
        title={`Routes${routes.length ? ` · ${routes.length}` : ""}`}
        right={
          <Button size="sm" icon={<Plus size={13} />} onClick={add}>
            Add route
          </Button>
        }
      >
        {routes.length === 0 ? (
          <div className="flex flex-col items-center gap-1.5 rounded-lg border border-dashed border-line px-4 py-6 text-center">
            <RouteIcon size={20} className="text-faint" />
            <div className="text-[12.5px] font-medium text-fg">No routes yet</div>
            <div className="max-w-xs text-[12px] text-muted">Every request gets the answer for unmatched requests below. Add a route to answer a path.</div>
          </div>
        ) : (
          <RouteList routes={routes} selected={selected} onSelect={select} onToggle={(i, enabled) => updateRoute(i, { enabled })} onMove={move} onDuplicate={duplicate} onRemove={remove} />
        )}
        {routes.length > 1 && <p className="text-[11.5px] text-faint">The first enabled route that matches answers. Drag to reorder.</p>}
      </EditorSection>

      {route && <RouteEditor key={selected} route={route} onChange={(patch) => updateRoute(selected, patch)} baseUrl={running?.url} />}

      <EditorSection title="Requests no route matches">
        <div>
          <Segmented items={FALLBACKS} value={http.fallback ?? "notFound"} onChange={(fallback) => setHttp((h) => ({ ...h, fallback }))} />
        </div>
        {(http.fallback ?? "notFound") === "notFound" ? (
          <p className="text-[12px] text-muted">Answered with 404 and a JSON list of the routes, so a typo is easy to spot.</p>
        ) : (
          <Field label="Backend URL" hint="Unmatched requests go there unchanged (method, path, query, headers, body); its answer comes back as it is. Redirects are passed on, not followed.">
            <div className="flex h-8 items-center rounded-lg border border-line bg-input hover:border-line-strong focus-within:border-accent">
              <VarInput value={http.proxyUrl ?? ""} onChange={(proxyUrl) => setHttp((h) => ({ ...h, proxyUrl }))} placeholder="https://api.example.com or {{baseUrl}}" ariaLabel="Backend URL" />
            </div>
          </Field>
        )}
      </EditorSection>

      <EditorSection title="Browser access">
        <Switch checked={http.cors ?? false} onChange={(cors) => setHttp((h) => ({ ...h, cors }))} label="Allow cross-origin requests (CORS)" />
        <p className="text-[12px] text-muted">Answers preflight requests and adds Access-Control-Allow-* headers to every answer, so a web app on another origin can call the mock.</p>
      </EditorSection>
    </>
  );
}

function RouteList({
  routes,
  selected,
  onSelect,
  onToggle,
  onMove,
  onDuplicate,
  onRemove,
}: {
  routes: MockRoute[];
  selected: number;
  onSelect: (i: number) => void;
  onToggle: (i: number, enabled: boolean) => void;
  onMove: (from: number, to: number) => void;
  onDuplicate: (i: number) => void;
  onRemove: (i: number) => void;
}) {
  const [dragIndex, setDragIndex] = useState<number | null>(null);
  const [overIndex, setOverIndex] = useState<number | null>(null);
  return (
    <div
      role="listbox"
      aria-label="Routes"
      className="overflow-hidden rounded-lg border border-line"
      onKeyDown={(e) => {
        const next = e.key === "ArrowDown" ? selected + 1 : e.key === "ArrowUp" ? selected - 1 : -1;
        if (next < 0 || next >= routes.length || e.target instanceof HTMLInputElement) return;
        e.preventDefault();
        onSelect(next);
        e.currentTarget.querySelectorAll<HTMLElement>('[role="option"]')[next]?.focus();
      }}
    >
      {routes.map((r, i) => {
        const enabled = r.enabled !== false;
        const active = i === selected;
        const fault = r.fault && r.fault !== "none";
        return (
          <div
            key={i}
            role="option"
            aria-selected={active}
            tabIndex={active ? 0 : -1}
            data-testid={`mock-route-${i}`}
            onClick={() => onSelect(i)}
            onDragOver={(e) => {
              if (dragIndex === null) return;
              e.preventDefault();
              setOverIndex(i);
            }}
            onDragLeave={() => setOverIndex((o) => (o === i ? null : o))}
            onDrop={(e) => {
              e.preventDefault();
              if (dragIndex !== null && dragIndex !== i) onMove(dragIndex, i);
              setDragIndex(null);
              setOverIndex(null);
            }}
            className={cx(
              "group relative flex h-8 cursor-default items-center gap-1.5 border-b border-line/70 pr-1 text-[12.5px] outline-none last:border-b-0",
              active ? "bg-hover" : "hover:bg-hover/50",
              "focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-accent",
            )}
          >
            {overIndex === i && dragIndex !== null && dragIndex !== i && (
              <div className={cx("absolute inset-x-1 h-[2px] rounded bg-accent", dragIndex < i ? "bottom-0" : "top-0")} />
            )}
            <span
              draggable
              onDragStart={(e) => {
                e.dataTransfer.effectAllowed = "move";
                e.dataTransfer.setData("text/plain", String(i));
                setDragIndex(i);
              }}
              onDragEnd={() => {
                setDragIndex(null);
                setOverIndex(null);
              }}
              className="flex w-4 shrink-0 cursor-grab justify-center pl-1 text-faint opacity-0 group-hover:opacity-100"
              aria-hidden
            >
              <GripVertical size={12} />
            </span>
            <span onClick={(e) => e.stopPropagation()} className="flex">
              <Checkbox checked={enabled} onChange={(v) => onToggle(i, v)} title={enabled ? "Turn this route off" : "Turn this route on"} />
            </span>
            <span className={cx("w-[46px] shrink-0 text-right font-mono text-[10.5px] font-bold", !enabled && "opacity-50")} style={{ color: r.method.trim() === "*" ? "var(--muted)" : methodColor(r.method) }}>
              {methodText(r.method)}
            </span>
            <span className={cx("min-w-0 flex-1 truncate font-mono", !enabled && "text-faint line-through decoration-faint/60")} title={r.path}>
              {r.path || "/"}
              {r.name && <span className="ml-2 font-sans text-[11.5px] text-faint">{r.name}</span>}
            </span>
            {(r.delayMs ?? 0) > 0 && (
              <Tooltip content={`Waits ${r.delayMs} ms`}>
                <Clock size={12} className="shrink-0 text-faint" />
              </Tooltip>
            )}
            {fault && (
              <Tooltip content={`Fault: ${FAULTS.find((f) => f.id === r.fault)?.label ?? r.fault} (${r.faultPercent ?? 100}%)`}>
                <Zap size={12} className="shrink-0 text-warning" />
              </Tooltip>
            )}
            <span className={cx("w-8 shrink-0 text-right font-mono text-[11.5px] tabular-nums", toneText[statusTone(r.status)])}>{r.status}</span>
            <span onClick={(e) => e.stopPropagation()} className="flex">
              <Menu
                align="end"
                trigger={
                  <button aria-label="Route actions" className="flex h-6 w-6 items-center justify-center rounded text-faint opacity-0 hover:bg-panel-2 hover:text-fg focus-visible:opacity-100 group-hover:opacity-100 data-[state=open]:opacity-100">
                    <MoreHorizontal size={13} />
                  </button>
                }
                entries={[
                  { label: "Duplicate", icon: <CopyPlus size={14} />, onSelect: () => onDuplicate(i) },
                  { label: "Move up", disabled: i === 0, onSelect: () => onMove(i, i - 1) },
                  { label: "Move down", disabled: i === routes.length - 1, onSelect: () => onMove(i, i + 1) },
                  { separator: true },
                  { label: "Delete", icon: <Trash2 size={14} />, danger: true, onSelect: () => onRemove(i) },
                ]}
              />
            </span>
          </div>
        );
      })}
    </div>
  );
}

function RouteEditor({ route, onChange, baseUrl }: { route: MockRoute; onChange: (patch: Partial<MockRoute>) => void; baseUrl?: string }) {
  const { names } = useVariableNames();
  const contentType = contentTypeOf(route);
  const language = languageForContentType(contentType, route.body ?? "");
  // Stable while typing in the body, so the editor doesn't re-highlight on every key.
  const placeholders = requestPlaceholders(route).join("\n");
  const variables = useMemo(() => [...names, ...placeholders.split("\n")], [names, placeholders]);
  const conditions = (route.matchQuery ?? []).filter((q) => q.key.trim()).length + (route.matchHeaders ?? []).filter((h) => h.key.trim()).length + (route.matchBody ? 1 : 0);
  const [showConditions, setShowConditions] = useState(conditions > 0);
  const fault = route.fault ?? "none";
  const bodyless = route.status === 204 || route.status === 304 || (route.status >= 100 && route.status < 200);
  const url = baseUrl ? routeUrl(baseUrl, route.path) : null;
  const customMethod = !ROUTE_METHODS.includes(route.method.trim().toUpperCase()) && route.method.trim() !== "*";

  const formatBody = () => {
    const pretty = beautifyJson(route.body ?? "");
    if (pretty === null) toast("error", "The body is not valid JSON");
    else onChange({ body: pretty });
  };

  return (
    <>
      <EditorSection title="Route">
        <div className="flex gap-2">
          <Select
            aria-label="Method"
            className="w-[108px] shrink-0"
            value={customMethod ? route.method : route.method.trim() === "*" ? "*" : route.method.toUpperCase()}
            onChange={(e) => onChange({ method: e.target.value })}
          >
            {ROUTE_METHODS.map((m) => (
              <option key={m} value={m}>
                {methodText(m)}
              </option>
            ))}
            {customMethod && <option value={route.method}>{route.method.toUpperCase()}</option>}
          </Select>
          <Input
            aria-label="Path"
            className="min-w-0 flex-1 font-mono"
            value={route.path}
            placeholder="/users/:id"
            onChange={(e) => onChange({ path: e.target.value })}
          />
          <Input
            aria-label="Status"
            type="number"
            min={100}
            max={999}
            list="zv-mock-status-codes"
            className="w-[84px] shrink-0 font-mono"
            value={route.status}
            onChange={(e) => onChange({ status: Math.min(999, Math.max(0, Math.trunc(Number(e.target.value)) || 0)) })}
          />
          <datalist id="zv-mock-status-codes">
            {STATUS_CODES.map((s) => (
              <option key={s.code} value={s.code}>
                {s.label}
              </option>
            ))}
          </datalist>
        </div>
        {url ? (
          <div className="flex min-w-0 items-center gap-1.5 text-[11.5px] text-muted">
            <span className="shrink-0">Call it at</span>
            <span className="selectable truncate font-mono text-fg" title={url}>
              {url}
            </span>
            <Tooltip content="Copy address">
              <button aria-label="Copy route address" onClick={() => void copyText(url)} className="shrink-0 rounded p-0.5 text-faint hover:bg-hover hover:text-fg">
                <Copy size={11} />
              </button>
            </Tooltip>
          </div>
        ) : (
          <p className="text-[11.5px] text-faint">
            <code className="font-mono">:name</code> matches one path segment (<code className="font-mono">{"{{request.params.name}}"}</code>), a final{" "}
            <code className="font-mono">*</code> matches the rest. Paths are case-sensitive; a trailing slash doesn't matter.
          </p>
        )}
        <Field label="Name" hint="Shown in the traffic log instead of the path (optional).">
          <Input value={route.name ?? ""} placeholder={`${methodText(route.method)} ${route.path}`} onChange={(e) => onChange({ name: e.target.value })} />
        </Field>
      </EditorSection>

      <EditorSection title="Response headers">
        <div className="-mx-3">
          <KeyValueEditor
            rows={route.headers ?? []}
            onChange={(headers) => onChange({ headers })}
            keyPlaceholder="Header"
            valuePlaceholder="Value"
            keySuggestions={COMMON_HEADERS}
            renderValue={(row: KeyValue, _i, update) => (
              <input aria-label="Header value" className={cell} spellCheck={false} value={row.value} placeholder="Value" onChange={(e) => update({ value: e.target.value })} />
            )}
          />
        </div>
      </EditorSection>

      <EditorSection
        title="Response body"
        right={
          language === "json" && !bodyless ? (
            <IconButton label="Format JSON" onClick={formatBody}>
              <Wand2 size={13} />
            </IconButton>
          ) : undefined
        }
      >
        {bodyless ? (
          <p className="text-[12px] text-muted">A {route.status} answer has no body.</p>
        ) : (
          <>
            <div className="h-[220px] overflow-hidden rounded-lg border border-line bg-input" data-testid="mock-route-body">
              <CodeEditor
                value={route.body ?? ""}
                onChange={(body) => onChange({ body })}
                language={language}
                variables={variables}
                placeholder={'{\n  "id": "{{request.params.id}}"\n}'}
              />
            </div>
            <p className="text-[11.5px] text-faint">
              Use <code className="font-mono">{"{{request.params.id}}"}</code>, <code className="font-mono">{"{{request.query.q}}"}</code>,{" "}
              <code className="font-mono">{"{{request.headers.name}}"}</code>, <code className="font-mono">{"{{request.body}}"}</code>, <code className="font-mono">{"{{$uuid}}"}</code> and
              environment variables. {contentType ? "" : "Without a Content-Type header, JSON, XML, HTML or plain text is detected."}
            </p>
          </>
        )}
      </EditorSection>

      <EditorSection title="Behavior">
        <div className="grid grid-cols-[minmax(0,1fr)_minmax(0,1.3fr)_minmax(0,0.8fr)] gap-3">
          <Field label="Delay (ms)">
            <Input
              type="number"
              min={0}
              className="font-mono"
              aria-label="Delay in milliseconds"
              value={route.delayMs ?? 0}
              onChange={(e) => onChange({ delayMs: Math.max(0, Math.trunc(Number(e.target.value)) || 0) })}
            />
          </Field>
          <Field label="Fault">
            <Select value={fault} onChange={(e) => onChange({ fault: e.target.value as MockFault })} aria-label="Fault">
              {FAULTS.map((f) => (
                <option key={f.id} value={f.id}>
                  {f.label}
                </option>
              ))}
            </Select>
          </Field>
          <Field label="How often (%)">
            <Input
              type="number"
              min={0}
              max={100}
              disabled={fault === "none"}
              className="font-mono disabled:opacity-50"
              aria-label="Fault percent"
              value={route.faultPercent ?? 100}
              onChange={(e) => onChange({ faultPercent: Math.min(100, Math.max(0, Math.trunc(Number(e.target.value)) || 0)) })}
            />
          </Field>
        </div>
        {fault !== "none" && <p className="text-[12px] text-muted">{FAULTS.find((f) => f.id === fault)?.hint} The delay still applies first.</p>}
      </EditorSection>

      <section className="flex flex-col gap-2.5 px-4 py-3">
        <button
          aria-expanded={showConditions}
          onClick={() => setShowConditions(!showConditions)}
          className="flex items-center gap-1 self-start text-[11px] font-semibold uppercase tracking-wide text-faint hover:text-fg"
        >
          <ChevronRight size={13} className={cx("transition-transform", showConditions && "rotate-90")} />
          Match only when
          {conditions > 0 && <span className="ml-1 rounded-full bg-hover px-1.5 text-[10.5px] normal-case tracking-normal text-muted">{conditions}</span>}
        </button>
        {showConditions && (
          <>
            <p className="text-[12px] text-muted">All conditions must hold. An empty value only requires the parameter or header to be there. Values can use environment variables.</p>
            <div className="text-[12px] font-medium text-muted">Query parameters</div>
            <div className="-mx-3">
              <KeyValueEditor rows={route.matchQuery ?? []} onChange={(matchQuery) => onChange({ matchQuery })} keyPlaceholder="Parameter" valuePlaceholder="Any value" />
            </div>
            <div className="text-[12px] font-medium text-muted">Headers</div>
            <div className="-mx-3">
              <KeyValueEditor rows={route.matchHeaders ?? []} onChange={(matchHeaders) => onChange({ matchHeaders })} keyPlaceholder="Header" valuePlaceholder="Any value" keySuggestions={COMMON_HEADERS} />
            </div>
            <Field label="Body contains">
              <Input className="font-mono" value={route.matchBody ?? ""} placeholder='"type": "admin"' onChange={(e) => onChange({ matchBody: e.target.value })} />
            </Field>
          </>
        )}
      </section>
    </>
  );
}

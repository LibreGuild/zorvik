// GraphQL body: query editor (cm6-graphql: highlighting, schema-aware completion, validation,
// hover docs, ⌘-click opens the type), Variables (JSON), operation picker, Prettify and the
// Schema panel. The schema is introspected with the request's own URL, headers and auth and
// loads by itself once the URL's host is known (store/graphql.ts).
import { useEffect, useMemo, useRef, useState } from "react";
import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { type Diagnostic, linter } from "@codemirror/lint";
import type { EditorState } from "@codemirror/state";
import { type EditorView, hoverTooltip, ViewPlugin } from "@codemirror/view";
import { tags as t } from "@lezer/highlight";
import { getSchema, graphql, graphqlLanguage, offsetToPos, updateSchema } from "cm6-graphql";
import { getHoverInformation } from "graphql-language-service";
import { RefreshCw, Wand2 } from "lucide-react";
import type { Body } from "../../bindings/Body";
import type { GraphqlBody } from "../../bindings/GraphqlBody";
import type { GraphqlTransport } from "../../bindings/GraphqlTransport";
import {
  canAutoLoad,
  isSubscription,
  keptOperationName,
  loadSchema,
  namedType,
  operationNames,
  prettifyQuery,
  schemaKey,
  setPanelOpen,
  showType,
  useGraphql,
} from "../../store/graphql";
import type { Tab } from "../../store/tabs";
import { toast } from "../../store/toasts";
import { useVariableNames, useWorkspace } from "../../store/workspace";
import { CodeEditor } from "../CodeEditor";
import { Splitter } from "../Splitter";
import { VarInput } from "../VarInput";
import { Button, cx, IconButton, Segmented, Select, Tooltip } from "../ui";
import { SchemaPanel, schemaStatus, StatusDot } from "./GraphqlSchemaPanel";
import { beautifyJson } from "./jsonFormat";

const TRANSPORTS: { id: GraphqlTransport; label: string; hint: string }[] = [
  { id: "websocket", label: "WebSocket", hint: "graphql-transport-ws: the graphql-ws library, Apollo Server 4+, Hasura and most servers." },
  { id: "websocketLegacy", label: "WebSocket (legacy)", hint: "subscriptions-transport-ws (subprotocol graphql-ws): Apollo Server 2 and 3." },
  { id: "sse", label: "SSE", hint: "graphql-sse over Server-Sent Events: GraphQL Yoga and others." },
];

/** Wait after the URL changes before introspecting it by itself. */
const AUTO_LOAD_DELAY_MS = 700;

const clamp = (v: number, min: number, max: number) => Math.min(max, Math.max(min, v));

/** The tab's schema entry and a refresh that asks the server again. */
function useRequestSchema(tab: Tab) {
  const workspace = useWorkspace((s) => s.info?.path ?? null);
  const environment = useWorkspace((s) => s.info?.activeEnvironment ?? null);
  const key = schemaKey(workspace, tab.draft.url, environment);
  const entry = useGraphql((s) => s.schemas[key]);
  return { key, entry, refresh: () => void loadSchema(key, tab.draft, tab.path, true) };
}

// Names the shared highlight style leaves plain.
const graphqlHighlight = syntaxHighlighting(
  HighlightStyle.define(
    [
      { tag: t.atom, color: "var(--syn-tag)" },
      { tag: t.variableName, color: "var(--syn-bool)" },
      { tag: t.modifier, color: "var(--syn-keyword)" },
      { tag: t.special(t.name), color: "var(--syn-number)" },
    ],
    { scope: graphqlLanguage },
  ),
);

function hoverText(info: unknown): string {
  if (typeof info === "string") return info;
  if (Array.isArray(info)) return info.map(hoverText).join("\n\n");
  if (info && typeof info === "object" && "value" in info) return String((info as { value: unknown }).value);
  return "";
}

/** Docs of the field, argument or type under the pointer. */
const graphqlHover = hoverTooltip((view, pos) => {
  const schema = getSchema(view.state);
  if (!schema) return null;
  let text: string;
  try {
    text = hoverText(getHoverInformation(schema, view.state.doc.toString(), offsetToPos(view.state.doc, pos)));
  } catch {
    return null;
  }
  text = text.replace(/```\w*\n?/g, "").trim();
  if (!text) return null;
  const word = view.state.wordAt(pos);
  return {
    pos: word?.from ?? pos,
    end: word?.to ?? pos,
    above: true,
    create: () => {
      const dom = document.createElement("div");
      dom.className = "zv-gql-hover";
      dom.textContent = text;
      return { dom };
    },
  };
});

function blankIsFine(diagnostics: readonly Diagnostic[], state: EditorState): Diagnostic[] {
  return state.doc.toString().trim() ? [...diagnostics] : [];
}

/** Controls for the body editor's toolbar row: operation picker, Prettify, schema state. */
export function GraphqlToolbar({ tab, body, onChange }: { tab: Tab; body: Body; onChange: (b: Body) => void }) {
  const { entry, refresh } = useRequestSchema(tab);
  const panelOpen = useGraphql((s) => s.panelOpen);
  const gql = body.graphql ?? {};
  // Mid-edit (the query doesn't parse) the picker keeps the last operations instead of disappearing.
  const parsedNames = useRef<string[]>([]);
  const names = useMemo(() => {
    parsedNames.current = operationNames(gql.query ?? "") ?? parsedNames.current;
    return parsedNames.current;
  }, [gql.query]);
  const status = schemaStatus(entry);
  const setGql = (patch: Partial<GraphqlBody>) => onChange({ ...body, graphql: { ...gql, ...patch } });

  const prettify = () => {
    const result = prettifyQuery(gql.query ?? "");
    if ("error" in result) {
      toast("error", "The query has a syntax error", result.error);
      return;
    }
    const variables = gql.variables?.trim() ? (beautifyJson(gql.variables) ?? gql.variables) : gql.variables;
    setGql({ query: result.text, variables });
    if (result.comments) toast("info", "Comments were removed", "Undo with the editor's undo shortcut to get them back.");
  };

  return (
    <>
      {names.length > 1 && (
        <Select
          value={gql.operationName && names.includes(gql.operationName) ? gql.operationName : ""}
          onChange={(e) => setGql({ operationName: e.target.value || undefined })}
          className="w-44 min-w-0 shrink"
          aria-label="Operation"
        >
          <option value="">Choose operation…</option>
          {names.map((n) => (
            <option key={n} value={n}>
              {n}
            </option>
          ))}
        </Select>
      )}
      <div className="flex-1" />
      <Button size="sm" variant="ghost" icon={<Wand2 size={13} />} onClick={prettify}>
        Prettify
      </Button>
      <IconButton label="Refresh schema" onClick={refresh} disabled={!tab.draft.url.trim() || entry?.status === "loading"}>
        <RefreshCw size={13} className={entry?.status === "loading" ? "zv-spin" : undefined} />
      </IconButton>
      <Tooltip content={status.text}>
        <Button
          size="sm"
          variant="ghost"
          icon={<StatusDot tone={status.tone} />}
          className={cx(panelOpen && "bg-hover text-fg")}
          aria-pressed={panelOpen}
          aria-label={`Schema: ${status.text}`}
          onClick={() => setPanelOpen(!panelOpen)}
        >
          Schema
        </Button>
      </Tooltip>
    </>
  );
}

export function GraphqlEditor({ tab, body, onChange, onSubmit }: { tab: Tab; body: Body; onChange: (b: Body) => void; onSubmit: () => void }) {
  const { names, known } = useVariableNames();
  const { key, entry, refresh } = useRequestSchema(tab);
  const panelOpen = useGraphql((s) => s.panelOpen);
  const panelWidth = useGraphql((s) => s.panelWidth);
  const querySplit = useGraphql((s) => s.querySplit);
  const gql = body.graphql ?? {};
  const setGql = (patch: Partial<GraphqlBody>) => onChange({ ...body, graphql: { ...gql, ...patch } });
  const area = useRef<HTMLDivElement>(null);
  const column = useRef<HTMLDivElement>(null);
  const subscription = isSubscription(tab.draft);
  const [pane, setPane] = useState<"variables" | "connection">("variables");

  // Introspect by itself once the URL settles, unless its host still has undefined variables.
  const latest = useRef(tab);
  latest.current = tab;
  const autoLoad = !entry && canAutoLoad(tab.draft.url, known);
  useEffect(() => {
    if (!autoLoad) return;
    const timer = setTimeout(() => {
      if (!useGraphql.getState().schemas[key]) void loadSchema(key, latest.current.draft, latest.current.path);
    }, AUTO_LOAD_DELAY_MS);
    return () => clearTimeout(timer);
  }, [key, autoLoad]);

  // The schema reaches the editor through cm6-graphql's state (reconfiguring would keep the old one).
  const view = useRef<EditorView | null>(null);
  const tabId = tab.id;
  const extensions = useMemo(
    () => [
      graphql(undefined, {
        onShowInDocs: (_field?: string, type?: string, parentType?: string) => {
          const target = type ? namedType(type) : parentType;
          if (target) showType(tabId, target);
        },
      }),
      graphqlHighlight,
      graphqlHover,
      // An empty query is not an error ("Unexpected <EOF>") until something is typed.
      linter(null, { markerFilter: blankIsFine, tooltipFilter: blankIsFine }),
      ViewPlugin.define((v) => {
        view.current = v;
        return {
          destroy: () => {
            if (view.current === v) view.current = null;
          },
        };
      }),
    ],
    [tabId],
  );
  const schema = entry?.schema ?? null;
  useEffect(() => {
    if (view.current) updateSchema(view.current, schema ?? undefined);
  }, [schema]);

  return (
    <div ref={area} className="flex h-full min-h-0">
      <div ref={column} className="flex min-w-0 flex-1 flex-col">
        <div style={{ height: `${querySplit * 100}%` }} className="min-h-0 shrink-0 overflow-hidden" data-testid="graphql-query">
          <CodeEditor
            value={gql.query ?? ""}
            onChange={(query) => setGql({ query, operationName: keptOperationName(query, gql.operationName) })}
            variables={names}
            extensions={extensions}
            onSubmit={onSubmit}
            placeholder={"query {\n  field\n}"}
          />
        </div>
        <Splitter
          direction="vertical"
          onResize={(delta) => {
            const height = column.current?.clientHeight;
            if (height) useGraphql.setState((s) => ({ querySplit: clamp(s.querySplit + delta / height, 0.15, 0.9) }));
          }}
        />
        <div className="flex min-h-0 flex-1 flex-col" data-testid="graphql-variables">
          <div className="flex h-7 shrink-0 items-center gap-2 px-3">
            {subscription ? (
              <Segmented
                items={[
                  { id: "variables", label: "Variables" },
                  { id: "connection", label: "Subscription" },
                ]}
                value={pane}
                onChange={setPane}
              />
            ) : (
              <span className="text-[11px] font-semibold uppercase tracking-wide text-faint">Variables</span>
            )}
          </div>
          <div className="min-h-0 flex-1 overflow-hidden">
            {subscription && pane === "connection" ? (
              <SubscriptionSettings gql={gql} setGql={setGql} names={names} onSubmit={onSubmit} />
            ) : (
              <CodeEditor
                value={gql.variables ?? ""}
                onChange={(variables) => setGql({ variables })}
                language="json"
                variables={names}
                onSubmit={onSubmit}
                placeholder={'{\n  "id": "{{userId}}"\n}'}
              />
            )}
          </div>
        </div>
      </div>
      {panelOpen && (
        <>
          <Splitter
            direction="horizontal"
            onResize={(delta) => {
              const width = area.current?.clientWidth ?? 1000;
              useGraphql.setState((s) => ({ panelWidth: clamp(s.panelWidth - delta, 200, Math.max(200, width * 0.5)) }));
            }}
          />
          {/* At most half the area, so the query stays editable in a narrow pane. */}
          <div style={{ width: panelWidth }} className="min-h-0 max-w-[50%] shrink-0 overflow-hidden">
            <SchemaPanel tabId={tab.id} entry={entry} canRefresh={!!tab.draft.url.trim()} onRefresh={refresh} />
          </div>
        </>
      )}
    </div>
  );
}

/** How a subscription connects: the transport, its URL when not the request's, the connection params. */
function SubscriptionSettings({
  gql,
  setGql,
  names,
  onSubmit,
}: {
  gql: GraphqlBody;
  setGql: (patch: Partial<GraphqlBody>) => void;
  names: string[];
  onSubmit: () => void;
}) {
  const transport = gql.transport ?? "websocket";
  const websocket = transport !== "sse";
  return (
    <div className="flex h-full min-h-0 flex-col gap-2 overflow-auto px-3 pb-2" data-testid="graphql-subscription">
      <div className="flex flex-wrap items-center gap-2">
        <Segmented items={TRANSPORTS.map((t) => ({ id: t.id, label: t.label }))} value={transport} onChange={(t) => setGql({ transport: t === "websocket" ? undefined : t })} />
      </div>
      <p className="text-[11.5px] text-faint">{TRANSPORTS.find((t) => t.id === transport)?.hint}</p>
      <div className="flex h-8 shrink-0 items-center rounded-lg border border-line bg-input focus-within:border-accent">
        <span className="shrink-0 pl-2.5 text-[11px] font-semibold text-faint">URL</span>
        <VarInput
          value={gql.subscriptionUrl ?? ""}
          onChange={(subscriptionUrl) => setGql({ subscriptionUrl: subscriptionUrl || undefined })}
          placeholder={websocket ? "Empty: the request URL, as ws:// (e.g. ws://localhost:4000/graphql)" : "Empty: the request URL"}
          className="flex-1"
          ariaLabel="Subscription URL"
        />
      </div>
      {websocket && (
        <div className="flex min-h-[90px] flex-1 flex-col">
          <div className="pb-1 text-[11px] font-semibold text-faint">Connection params (JSON, sent in connection_init; often the auth token)</div>
          <div className="min-h-0 flex-1 overflow-hidden rounded-lg border border-line" data-testid="graphql-connection-params">
            <CodeEditor
              value={gql.connectionParams ?? ""}
              onChange={(connectionParams) => setGql({ connectionParams: connectionParams || undefined })}
              language="json"
              variables={names}
              onSubmit={onSubmit}
              lineNumbers={false}
              placeholder={'{"authToken": "{{token}}"}'}
            />
          </div>
        </div>
      )}
    </div>
  );
}

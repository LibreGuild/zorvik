// Schema explorer beside the GraphQL editor: root types → fields with arguments, types,
// descriptions and deprecations; a type name opens that type; search finds types and fields.
import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ArrowLeft, Home, RefreshCw, Search } from "lucide-react";
import {
  getNamedType,
  type GraphQLArgument,
  type GraphQLField,
  type GraphQLInputField,
  type GraphQLNamedType,
  type GraphQLSchema,
  type GraphQLType,
  isEnumType,
  isInputObjectType,
  isInterfaceType,
  isObjectType,
  isScalarType,
  isUnionType,
} from "graphql";
import { docsBack, type SchemaEntry, showType, useGraphql } from "../../store/graphql";
import { cx, IconButton, Input } from "../ui";

export type SchemaTone = "none" | "loading" | "ok" | "error";

/** One-line schema state for the toolbar and the panel. */
export function schemaStatus(entry: SchemaEntry | undefined): { tone: SchemaTone; text: string } {
  if (!entry) return { tone: "none", text: "Schema not loaded" };
  if (entry.status === "loading") return { tone: "loading", text: entry.schema ? "Refreshing schema…" : "Loading schema…" };
  if (entry.status === "error") return { tone: "error", text: entry.error ?? "The schema could not be loaded" };
  const types = entry.schema ? userTypes(entry.schema).length : 0;
  return { tone: "ok", text: `Schema loaded · ${types} types` };
}

export function StatusDot({ tone }: { tone: SchemaTone }) {
  const color = { none: "bg-faint/50", loading: "bg-warning", ok: "bg-success", error: "bg-danger" }[tone];
  return <span className={cx("inline-block h-2 w-2 shrink-0 rounded-full", color, tone === "loading" && "animate-pulse")} />;
}

const typeLists = new WeakMap<GraphQLSchema, GraphQLNamedType[]>();

/** The schema's own types by name; computed once per schema (large ones have thousands, and the status is shown on every keystroke). */
function userTypes(schema: GraphQLSchema): GraphQLNamedType[] {
  let types = typeLists.get(schema);
  if (!types) {
    types = Object.values(schema.getTypeMap())
      .filter((t) => !t.name.startsWith("__"))
      .sort((a, b) => a.name.localeCompare(b.name));
    typeLists.set(schema, types);
  }
  return types;
}

const KIND_LABEL = (t: GraphQLNamedType): string =>
  isObjectType(t) ? "type" : isInterfaceType(t) ? "interface" : isUnionType(t) ? "union" : isEnumType(t) ? "enum" : isInputObjectType(t) ? "input" : "scalar";

const NO_TYPES: string[] = [];
const MAX_RESULTS = 100;

export function SchemaPanel({ tabId, entry, canRefresh, onRefresh }: { tabId: string; entry: SchemaEntry | undefined; canRefresh: boolean; onRefresh: () => void }) {
  const stack = useGraphql((s) => s.docs[tabId]) ?? NO_TYPES;
  const [search, setSearch] = useState("");
  const schema = entry?.schema ?? null;
  const current = schema && stack.length ? (schema.getType(stack[stack.length - 1]) ?? null) : null;
  const status = schemaStatus(entry);
  // Keyboard focus moves to the content when the control that was used goes away.
  const content = useRef<HTMLDivElement>(null);
  // Stable, so the (possibly huge) type lists below don't re-render while the query is typed.
  const open = useCallback(
    (name: string) => {
      setSearch("");
      showType(tabId, name);
      content.current?.focus({ preventScroll: true });
    },
    [tabId],
  );
  const back = (home: boolean) => {
    docsBack(tabId, home);
    if (home || stack.length === 1) content.current?.focus({ preventScroll: true });
  };
  // Each type (and the search results) starts at the top.
  const searching = search.trim() !== "";
  useEffect(() => {
    if (content.current) content.current.scrollTop = 0;
  }, [current, searching]);

  return (
    <div className="flex h-full min-h-0 flex-col border-l border-line bg-panel" role="complementary" aria-label="GraphQL schema" data-testid="graphql-schema">
      <div className="flex h-9 shrink-0 items-center gap-0.5 px-2">
        {stack.length > 0 && (
          <>
            <IconButton label="Back" onClick={() => back(false)}>
              <ArrowLeft size={13} />
            </IconButton>
            <IconButton label="Root types" onClick={() => back(true)}>
              <Home size={13} />
            </IconButton>
          </>
        )}
        <div className="min-w-0 flex-1 truncate px-1 text-[12.5px] font-semibold text-fg">{current ? current.name : "Schema"}</div>
        <IconButton label="Refresh schema" onClick={onRefresh} disabled={!canRefresh || entry?.status === "loading"}>
          <RefreshCw size={13} className={entry?.status === "loading" ? "zv-spin" : undefined} />
        </IconButton>
      </div>
      <div className="flex items-start gap-2 px-3 pb-2 text-[11.5px] text-muted" role="status" data-testid="graphql-schema-status">
        <span className="mt-[5px]">
          <StatusDot tone={status.tone} />
        </span>
        <span className="min-w-0 break-words">
          {status.text}
          {entry?.status === "loaded" && entry.fetchedAt && <> · {new Date(entry.fetchedAt).toLocaleTimeString()}</>}
          {entry?.legacy && <span className="text-faint"> (older introspection)</span>}
        </span>
      </div>
      {schema && (
        <div className="relative px-2 pb-2">
          <Search size={13} className="pointer-events-none absolute left-4 top-1/2 -translate-y-[calc(50%+4px)] text-faint" />
          <Input value={search} onChange={(e) => setSearch(e.target.value)} placeholder="Search types and fields" className="h-7 pl-7 text-[12px]" aria-label="Search schema" />
        </div>
      )}
      <div ref={content} tabIndex={-1} className="selectable min-h-0 flex-1 overflow-auto px-3 pb-3 text-[12px] outline-none">
        {!schema ? (
          <p className="pt-2 text-muted">
            {entry?.status === "loading"
              ? "Asking the server for its schema…"
              : "The schema is read from the server (introspection) with this request's URL, headers and auth. Set the URL, then refresh."}
          </p>
        ) : search.trim() ? (
          <SearchResults schema={schema} query={search.trim().toLowerCase()} onOpen={open} />
        ) : current ? (
          <TypeDetails type={current} onOpen={open} />
        ) : (
          <RootTypes schema={schema} onOpen={open} />
        )}
      </div>
    </div>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="pt-3">
      <div className="pb-1 text-[10.5px] font-semibold uppercase tracking-wide text-faint">{title}</div>
      {children}
    </div>
  );
}

function TypeLink({ type, onOpen }: { type: GraphQLType; onOpen: (name: string) => void }) {
  const text = String(type);
  const name = getNamedType(type).name;
  const at = text.indexOf(name);
  return (
    <span className="font-mono">
      {text.slice(0, at)}
      <button className="text-[var(--syn-property)] hover:underline" onClick={() => onOpen(name)}>
        {name}
      </button>
      {text.slice(at + name.length)}
    </span>
  );
}

function Description({ text }: { text: string | null | undefined }) {
  return text ? <div className="mt-0.5 whitespace-pre-wrap text-muted">{text}</div> : null;
}

function Deprecation({ reason }: { reason: string | null | undefined }) {
  return reason === undefined || reason === null ? null : <div className="mt-0.5 text-warning">Deprecated{reason ? `: ${reason}` : ""}</div>;
}

const RootTypes = memo(function RootTypes({ schema, onOpen }: { schema: GraphQLSchema; onOpen: (name: string) => void }) {
  const roots = [
    ["query", schema.getQueryType()],
    ["mutation", schema.getMutationType()],
    ["subscription", schema.getSubscriptionType()],
  ] as const;
  return (
    <>
      {schema.description && <Description text={schema.description} />}
      <Section title="Root types">
        {roots.map(([label, type]) =>
          type ? (
            <div key={label} className="py-0.5 font-mono">
              <span className="text-[var(--syn-keyword)]">{label}</span>: <TypeLink type={type} onOpen={onOpen} />
            </div>
          ) : null,
        )}
      </Section>
      <Section title="All types">
        {userTypes(schema).map((t) => (
          <div key={t.name} className="flex items-baseline gap-2 py-0.5">
            <TypeLink type={t} onOpen={onOpen} />
            <span className="text-[11px] text-faint">{KIND_LABEL(t)}</span>
          </div>
        ))}
      </Section>
    </>
  );
});

function Args({ args, onOpen }: { args: readonly GraphQLArgument[]; onOpen: (name: string) => void }) {
  if (!args.length) return null;
  return (
    <>
      (
      {args.map((a, i) => (
        <span key={a.name} className={cx(a.deprecationReason != null && "line-through")}>
          {i > 0 && ", "}
          <span className="text-[var(--syn-string)]">{a.name}</span>: <TypeLink type={a.type} onOpen={onOpen} />
          {a.defaultValue !== undefined && <span className="text-faint"> = {JSON.stringify(a.defaultValue)}</span>}
        </span>
      ))}
      )
    </>
  );
}

function FieldRow({ field, onOpen }: { field: GraphQLField<unknown, unknown> | GraphQLInputField; onOpen: (name: string) => void }) {
  const args = "args" in field ? field.args : [];
  const deprecated = field.deprecationReason;
  return (
    <div className="border-b border-line/60 py-1.5 last:border-b-0" data-testid="graphql-schema-field">
      <div className="break-words font-mono">
        <span className={cx("text-[var(--syn-property)]", deprecated != null && "line-through opacity-70")}>{field.name}</span>
        <Args args={args} onOpen={onOpen} />: <TypeLink type={field.type} onOpen={onOpen} />
        {"defaultValue" in field && field.defaultValue !== undefined && <span className="text-faint"> = {JSON.stringify(field.defaultValue)}</span>}
      </div>
      <Description text={field.description} />
      {args
        .filter((a) => a.description)
        .map((a) => (
          <div key={a.name} className="mt-0.5 pl-3 text-muted">
            <span className="font-mono text-[var(--syn-string)]">{a.name}</span> — {a.description}
          </div>
        ))}
      <Deprecation reason={deprecated} />
    </div>
  );
}

const TypeDetails = memo(function TypeDetails({ type, onOpen }: { type: GraphQLNamedType; onOpen: (name: string) => void }) {
  return (
    <div data-testid="graphql-schema-type">
      <div className="pt-1 font-mono text-faint">{KIND_LABEL(type)}</div>
      <Description text={type.description} />
      {isScalarType(type) && type.specifiedByURL && (
        <div className="mt-1 break-all text-muted">Specified by {type.specifiedByURL}</div>
      )}
      {(isObjectType(type) || isInterfaceType(type)) && type.getInterfaces().length > 0 && (
        <Section title="Implements">
          {type.getInterfaces().map((i) => (
            <div key={i.name}>
              <TypeLink type={i} onOpen={onOpen} />
            </div>
          ))}
        </Section>
      )}
      {(isObjectType(type) || isInterfaceType(type) || isInputObjectType(type)) && (
        <Section title="Fields">
          {Object.values(type.getFields()).map((f) => (
            <FieldRow key={f.name} field={f} onOpen={onOpen} />
          ))}
        </Section>
      )}
      {isUnionType(type) && (
        <Section title="Possible types">
          {type.getTypes().map((t) => (
            <div key={t.name}>
              <TypeLink type={t} onOpen={onOpen} />
            </div>
          ))}
        </Section>
      )}
      {isEnumType(type) && (
        <Section title="Values">
          {type.getValues().map((v) => (
            <div key={v.name} className="border-b border-line/60 py-1.5 last:border-b-0">
              <span className={cx("font-mono text-[var(--syn-number)]", v.deprecationReason != null && "line-through opacity-70")}>{v.name}</span>
              <Description text={v.description} />
              <Deprecation reason={v.deprecationReason} />
            </div>
          ))}
        </Section>
      )}
    </div>
  );
});

const SearchResults = memo(function SearchResults({ schema, query, onOpen }: { schema: GraphQLSchema; query: string; onOpen: (name: string) => void }) {
  const results = useMemo(() => {
    const out: { type: GraphQLNamedType; field?: GraphQLField<unknown, unknown> | GraphQLInputField }[] = [];
    for (const type of userTypes(schema)) {
      if (type.name.toLowerCase().includes(query)) out.push({ type });
      if (isObjectType(type) || isInterfaceType(type) || isInputObjectType(type)) {
        for (const field of Object.values(type.getFields())) {
          if (field.name.toLowerCase().includes(query)) out.push({ type, field });
        }
      }
      if (out.length >= MAX_RESULTS) break;
    }
    return out.slice(0, MAX_RESULTS);
  }, [schema, query]);
  if (!results.length) return <p className="pt-2 text-muted">Nothing matches.</p>;
  return (
    <div className="pt-1">
      {results.map(({ type, field }) => (
        <button
          key={`${type.name}.${field?.name ?? ""}`}
          className="block w-full truncate rounded px-1 py-0.5 text-left font-mono hover:bg-hover"
          onClick={() => onOpen(type.name)}
        >
          <span className="text-[var(--syn-property)]">{type.name}</span>
          {field ? (
            <>
              .{field.name}
              <span className="text-faint">: {String(field.type)}</span>
            </>
          ) : (
            <span className="text-faint"> {KIND_LABEL(type)}</span>
          )}
        </button>
      ))}
    </div>
  );
});

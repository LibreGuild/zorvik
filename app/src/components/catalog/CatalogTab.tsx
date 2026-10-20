// Import → API catalog: well-known APIs, imported in one click. The list ships with the app;
// nothing is downloaded until the user clicks Import.
import { memo, useCallback, useEffect, useId, useMemo, useRef, useState } from "react";
import { CircleAlert, ExternalLink, FileJson, Info, Library, Search } from "lucide-react";
import type { CatalogEntry } from "../../bindings/CatalogEntry";
import type { CatalogImport } from "../../bindings/CatalogImport";
import type { CatalogPreview } from "../../bindings/CatalogPreview";
import { openExternal, pickFolder } from "../../lib/platform";
import { api, errorMessage, RpcError } from "../../lib/rpc";
import { restoreTabs } from "../../store/tabs";
import { toast } from "../../store/toasts";
import { openOrInitWorkspace, useWorkspace } from "../../store/workspace";
import { Button, Checkbox, cx, EmptyState, Input, Spinner } from "../ui";
import {
  CATEGORIES,
  CATEGORY_LABELS,
  type CategoryFilter,
  categoryCounts,
  entryLabels,
  filterGroups,
  groupLabel,
  hostOf,
  initialTicked,
  licenseOf,
  needsPicking,
  progressText,
  requestCount,
  searchCatalog,
  sourceText,
  tickAll,
  tickedTags,
  tickedTotal,
  webUrl,
} from "./model";

// The list ships with the app and doesn't change while it runs: loaded once.
let loaded: CatalogEntry[] | null = null;

function useCatalog() {
  const [entries, setEntries] = useState(loaded);
  const [error, setError] = useState<string | null>(null);
  const load = useCallback(() => {
    setError(null);
    api
      .catalogList()
      .then((list) => {
        loaded = list;
        setEntries(list);
      })
      .catch((e) => setError(errorMessage(e)));
  }, []);
  useEffect(() => {
    if (!loaded) load();
  }, [load]);
  return { entries, error, retry: load };
}

/** Imported requests are saved in a workspace: with none open (the welcome screen), choose its folder first. */
async function ensureWorkspace(): Promise<boolean> {
  if (useWorkspace.getState().info) return true;
  const path = await pickFolder("Choose a folder for the workspace");
  if (!path) return false;
  try {
    if (!(await openOrInitWorkspace(path))) return false;
    await restoreTabs(path);
    return true;
  } catch (e) {
    toast("error", "Could not open workspace", errorMessage(e));
    return false;
  }
}

interface Failure {
  id: string;
  code: string;
  message: string;
}

interface Pick {
  id: string;
  preview: CatalogPreview;
  ticked: Set<string>;
  query: string;
}

export function CatalogTab({
  parent,
  blocked = false,
  onBusyChange,
  onImported,
  onImportFile,
}: {
  /** Folder to import into ("" = top level). */
  parent: string;
  /** Another import in the window is running: wait for it. */
  blocked?: boolean;
  /** Told when an import from the catalog starts and ends (the window's other imports wait for it). */
  onBusyChange?: (busy: boolean) => void;
  onImported: (result: CatalogImport) => Promise<void>;
  /** Switch to importing a file (the File tab). */
  onImportFile: () => void;
}) {
  const { entries, error, retry } = useCatalog();
  const hasWorkspace = useWorkspace((s) => !!s.info);
  const [query, setQuery] = useState("");
  const [category, setCategory] = useState<CategoryFilter>("all");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  // One import at a time: the step it is at, for the entry it is for.
  const [job, setJob] = useState<{ id: string; text: string } | null>(null);
  const running = useRef(false);
  const [pick, setPick] = useState<Pick | null>(null);
  const [failure, setFailure] = useState<Failure | null>(null);
  const root = useRef<HTMLDivElement>(null);
  const ids = useId();
  const optionId = (id: string) => `${ids}-api-${id}`;
  // Closing the window cancels an import that hasn't started writing (it is still downloading).
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);
  const busy = job !== null;
  useEffect(() => {
    onBusyChange?.(busy);
  }, [busy, onBusyChange]);
  // Where focus goes when the details change under it (the button pressed is gone).
  const refocus = useRef<"groups" | "import" | null>(null);
  useEffect(() => {
    const to = refocus.current;
    if (!to) return;
    refocus.current = null;
    root.current?.querySelector<HTMLElement>(`[data-focus="${to}"]`)?.focus();
  }, [pick]);

  const all = entries ?? [];
  const shown = useMemo(() => searchCatalog(all, query, category), [all, query, category]);
  const counts = useMemo(() => categoryCounts(all, query), [all, query]);
  const current = shown.find((e) => e.id === selectedId) ?? shown[0] ?? null;

  /** Run one step of importing `entry`; a click while another runs does nothing. */
  const step = async (entry: CatalogEntry, text: string, fn: () => Promise<void>) => {
    if (running.current || blocked) return;
    running.current = true;
    setFailure(null);
    try {
      if (!(await ensureWorkspace())) return;
      setJob({ id: entry.id, text });
      await fn();
    } catch (e) {
      const code = e instanceof RpcError ? e.code : "internal";
      setFailure({ id: entry.id, code, message: errorMessage(e) });
    } finally {
      running.current = false;
      setJob(null);
    }
  };

  const start = (entry: CatalogEntry) =>
    step(entry, progressText(entry, all), async () => {
      const preview = await api.catalogPreview(entry.id);
      if (!alive.current) return;
      if (needsPicking(preview)) {
        refocus.current = "groups";
        setPick({ id: entry.id, preview, ticked: initialTicked(preview), query: "" });
        return;
      }
      setJob({ id: entry.id, text: `Adding ${entry.name}…` });
      await onImported(await api.catalogImport(entry.id, parent));
    });

  const importPicked = (entry: CatalogEntry, p: Pick) =>
    step(entry, `Adding ${requestCount(tickedTotal(p.preview.groups, p.ticked))}…`, async () => {
      await onImported(await api.catalogImport(entry.id, parent, tickedTags(p.preview.groups, p.ticked)));
      setPick(null);
    });

  /** Move the selection by a step (Up/Down) or to an end (Home/End); `focus` moves focus with it. */
  const move = (to: number | "first" | "last", focus: boolean) => {
    if (!shown.length) return;
    const at = current ? shown.indexOf(current) : -1;
    const index = to === "first" ? 0 : to === "last" ? shown.length - 1 : Math.min(shown.length - 1, Math.max(0, at + to));
    const next = shown[index];
    setSelectedId(next.id);
    const el = document.getElementById(optionId(next.id));
    if (focus) el?.focus();
    else el?.scrollIntoView?.({ block: "nearest" });
  };

  const keys = (e: React.KeyboardEvent, focus: boolean) => {
    const to = e.key === "ArrowDown" ? 1 : e.key === "ArrowUp" ? -1 : focus && e.key === "Home" ? "first" : focus && e.key === "End" ? "last" : null;
    if (to === null) return;
    e.preventDefault();
    move(to, focus);
  };

  if (error) {
    return (
      <EmptyState icon={<CircleAlert size={22} />} title="Couldn't load the API catalog" action={<Button onClick={retry}>Try again</Button>}>
        {error}
      </EmptyState>
    );
  }
  if (!entries) {
    return (
      <div className="flex h-60 items-center justify-center">
        <Spinner size={18} />
      </div>
    );
  }

  return (
    <div ref={root} className="flex min-h-0 flex-1 flex-col gap-3 p-5" data-testid="catalog">
      <div className="relative shrink-0">
        <Search size={14} className="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-faint" />
        <Input
          aria-label="Search APIs"
          // Up and Down here move through the list: say which API that picks.
          aria-controls={`${ids}-apis`}
          aria-activedescendant={current ? optionId(current.id) : undefined}
          value={query}
          autoFocus
          placeholder="Search APIs by name, what they do or category"
          className="pl-8"
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => keys(e, false)}
        />
      </div>
      <div role="group" aria-label="Categories" className="flex shrink-0 flex-wrap gap-1.5">
        {[{ id: "all" as const, label: "All" }, ...CATEGORIES].map((c) => (
          <button
            key={c.id}
            type="button"
            aria-pressed={category === c.id}
            aria-label={`${c.label}, ${counts[c.id]} ${counts[c.id] === 1 ? "API" : "APIs"}`}
            onClick={() => setCategory(c.id)}
            className={cx(
              "flex items-center gap-1.5 rounded-full border px-2.5 py-0.5 text-[12px] transition-colors",
              category === c.id ? "border-accent bg-accent-soft font-medium text-accent" : "border-line text-muted hover:bg-hover hover:text-fg",
              category !== c.id && !counts[c.id] && "opacity-60",
            )}
          >
            {c.label}
            <span className="tabular-nums text-[11px] text-faint">{counts[c.id]}</span>
          </button>
        ))}
      </div>
      <div className="flex h-[440px] min-h-[200px] overflow-hidden rounded-lg border border-line">
        <div className="w-[290px] shrink-0 overflow-y-auto border-r border-line p-1">
          <div role="listbox" id={`${ids}-apis`} aria-label="APIs" onKeyDown={(e) => keys(e, true)}>
            {/* Rows are memoized: typing or ticking groups redraws only the rows that change. */}
            {shown.map((e) => (
              <CatalogRow key={e.id} entry={e} id={optionId(e.id)} selected={e.id === current?.id} busy={job?.id === e.id} onChoose={setSelectedId} />
            ))}
          </div>
          {!shown.length && (
            <p role="status" className="px-3 py-6 text-center text-[12.5px] text-muted">
              No API matches {query.trim() ? `“${query.trim()}”` : "this category"}
              {category !== "all" ? ` in ${CATEGORY_LABELS[category]}` : ""}.
            </p>
          )}
        </div>
        <div className="min-w-0 flex-1">
          {current ? (
            <Details
              entry={current}
              entries={all}
              hasWorkspace={hasWorkspace}
              job={job}
              locked={blocked || busy}
              failure={failure?.id === current.id ? failure : null}
              pick={pick?.id === current.id ? pick : null}
              onImport={() => void start(current)}
              onPick={(p) => setPick(p)}
              onImportPicked={(p) => void importPicked(current, p)}
              onCancelPick={() => {
                refocus.current = "import";
                setPick(null);
              }}
              onImportFile={onImportFile}
            />
          ) : (
            <EmptyState icon={<Library size={22} strokeWidth={1.5} />} title="Nothing to show">
              Try another search, or import the API's description as a file.
            </EmptyState>
          )}
        </div>
      </div>
    </div>
  );
}

/** One API in the list. */
const CatalogRow = memo(function CatalogRow({
  entry,
  id,
  selected,
  busy,
  onChoose,
}: {
  entry: CatalogEntry;
  id: string;
  selected: boolean;
  /** Its import is running. */
  busy: boolean;
  onChoose: (id: string) => void;
}) {
  return (
    <button
      id={id}
      type="button"
      role="option"
      aria-selected={selected}
      tabIndex={selected ? 0 : -1}
      onClick={() => onChoose(entry.id)}
      className={cx(
        "flex w-full flex-col gap-0.5 rounded-md px-2.5 py-2 text-left outline-none focus-visible:ring-1 focus-visible:ring-accent",
        selected ? "bg-accent-soft" : "hover:bg-hover",
      )}
    >
      <span className="flex w-full items-center gap-2">
        <span className={cx("min-w-0 flex-1 truncate text-[13px] font-medium", selected ? "text-accent" : "text-fg")}>{entry.name}</span>
        {busy && <Spinner size={12} />}
      </span>
      <span className="line-clamp-1 text-[11.5px] text-muted">{entry.description}</span>
      <span className="mt-0.5 flex flex-wrap gap-1">
        {entryLabels(entry).map((l) => (
          <span key={l} className="rounded bg-panel-2 px-1.5 text-[10.5px] leading-4 text-faint">
            {l}
          </span>
        ))}
      </span>
    </button>
  );
});

function Details({
  entry,
  entries,
  hasWorkspace,
  job,
  locked,
  failure,
  pick,
  onImport,
  onPick,
  onImportPicked,
  onCancelPick,
  onImportFile,
}: {
  entry: CatalogEntry;
  entries: CatalogEntry[];
  hasWorkspace: boolean;
  job: { id: string; text: string } | null;
  /** An import is running (this one or another): Import waits. */
  locked: boolean;
  failure: Failure | null;
  pick: Pick | null;
  onImport: () => void;
  onPick: (p: Pick) => void;
  onImportPicked: (p: Pick) => void;
  onCancelPick: () => void;
  onImportFile: () => void;
}) {
  const license = licenseOf(entry, entries);
  const source = sourceText(entry, entries);
  const jobText = job?.id === entry.id ? job.text : null;
  const busy = jobText !== null;
  const website = webUrl(entry.website);
  const licenseUrl = webUrl(license?.url);
  const docs = website ? () => void openExternal(website) : undefined;
  const headingId = `catalog-${entry.id}-name`;
  const problem = failure && (
    <Notice
      tone={failure.code === "catalogLink" ? "info" : "danger"}
      title={failure.code === "catalogLink" ? `Zorvik can't import ${entry.name}` : `Couldn't import ${entry.name}`}
      message={failure.message}
      onDocs={docs}
      onImportFile={failure.code === "catalogLink" ? undefined : onImportFile}
    />
  );
  return (
    <section aria-labelledby={headingId} className="flex h-full flex-col gap-4 overflow-y-auto p-4" data-testid="catalog-detail">
      <div className="shrink-0">
        <h3 id={headingId} className="text-[15px] font-semibold text-fg">
          {entry.name}
        </h3>
        <div className="text-[11.5px] text-faint">{CATEGORY_LABELS[entry.category]}</div>
        <p className="mt-2 text-[13px] text-muted">{entry.description}</p>
      </div>
      {pick ? (
        // Picking groups takes the room the details had.
        <>
          {problem}
          <GroupPicker entry={entry} pick={pick} jobText={jobText} locked={locked} onChange={onPick} onImport={() => onImportPicked(pick)} onBack={onCancelPick} />
        </>
      ) : (
        <>
          <dl className="grid shrink-0 grid-cols-[max-content_minmax(0,1fr)] gap-x-4 gap-y-1.5 text-[12.5px]">
            <dt className="text-faint">Signs in with</dt>
            <dd className="text-fg">{entry.auth}</dd>
            <dt className="text-faint">Format</dt>
            <dd className="text-fg">{entry.format}</dd>
            {license && (
              <>
                <dt className="text-faint">License</dt>
                <dd className="text-fg">
                  {licenseUrl ? (
                    <button type="button" className="inline-flex items-center gap-1 text-accent hover:underline" onClick={() => void openExternal(licenseUrl)}>
                      {license.name}
                      <ExternalLink size={11} />
                    </button>
                  ) : (
                    license.name
                  )}
                </dd>
              </>
            )}
            {/* "Written for Zorvik" is often the format already. */}
            {source && source.toLowerCase() !== entry.format.toLowerCase() && (
              <>
                <dt className="text-faint">Source</dt>
                <dd className="text-fg">{source}</dd>
              </>
            )}
            {website && (
              <>
                <dt className="text-faint">API docs</dt>
                <dd>
                  <button type="button" className="inline-flex max-w-full items-center gap-1 text-accent hover:underline" onClick={docs} title={website}>
                    <span className="truncate">{hostOf(website)}</span>
                    <ExternalLink size={11} className="shrink-0" />
                  </button>
                </dd>
              </>
            )}
          </dl>
          {entry.source.type === "link" ? (
            <Notice tone="info" title={`Zorvik can't import ${entry.name}`} message={entry.source.reason} onDocs={docs} />
          ) : (
            <div className="flex shrink-0 flex-col gap-3">
              <div className="flex items-center gap-3">
                <Button variant="primary" loading={busy} disabled={locked} onClick={onImport} data-focus="import">
                  Import
                </Button>
                {jobText !== null ? (
                  <span role="status" className="text-[12.5px] text-muted">
                    {jobText}
                  </span>
                ) : (
                  !hasWorkspace && <span className="text-[12px] text-faint">Requests are saved in a workspace: you'll choose its folder first.</span>
                )}
              </div>
              {problem}
            </div>
          )}
        </>
      )}
    </section>
  );
}

/** Why an entry can't be imported, or what went wrong, with what to do instead. */
function Notice({
  tone,
  title,
  message,
  onDocs,
  onImportFile,
}: {
  tone: "danger" | "info";
  title: string;
  message: string;
  /** Left out when the entry has no web page to open. */
  onDocs?: () => void;
  onImportFile?: () => void;
}) {
  const card = useRef<HTMLDivElement>(null);
  // A problem shows below the details: keep it in view.
  useEffect(() => {
    card.current?.scrollIntoView?.({ block: "nearest" });
  }, [message]);
  return (
    <div
      ref={card}
      role={tone === "danger" ? "alert" : undefined}
      className={cx("shrink-0 rounded-lg border p-3", tone === "danger" ? "border-danger/30 bg-danger/10" : "border-line bg-panel-2")}
      data-testid="catalog-notice"
    >
      <div className="flex gap-2">
        {tone === "danger" ? <CircleAlert size={15} className="mt-px shrink-0 text-danger" /> : <Info size={15} className="mt-px shrink-0 text-muted" />}
        <div className="min-w-0">
          <div className="text-[12.5px] font-semibold text-fg">{title}</div>
          <p className="selectable mt-0.5 break-words text-[12.5px] text-muted">{message}</p>
        </div>
      </div>
      {(onDocs || onImportFile) && (
        <div className="mt-3 flex flex-wrap gap-2 pl-[23px]">
          {onDocs && (
            <Button size="sm" icon={<ExternalLink size={13} />} onClick={onDocs}>
              Open API docs
            </Button>
          )}
          {onImportFile && (
            <Button size="sm" variant="ghost" icon={<FileJson size={13} />} onClick={onImportFile}>
              Import a file instead
            </Button>
          )}
        </div>
      )}
    </div>
  );
}

/** A large API: pick the groups (first tags) to import. */
function GroupPicker({
  entry,
  pick,
  jobText,
  locked,
  onChange,
  onImport,
  onBack,
}: {
  entry: CatalogEntry;
  pick: Pick;
  /** What the import is doing, while it runs. */
  jobText: string | null;
  /** An import is running (this one or another): Import waits. */
  locked: boolean;
  onChange: (p: Pick) => void;
  onImport: () => void;
  onBack: () => void;
}) {
  const { groups, operations } = pick.preview;
  const busy = jobText !== null;
  const shown = filterGroups(groups, pick.query);
  const total = tickedTotal(groups, pick.ticked);
  const toggle = (tag: string, on: boolean) => {
    const ticked = new Set(pick.ticked);
    if (on) ticked.add(tag);
    else ticked.delete(tag);
    onChange({ ...pick, ticked });
  };
  return (
    <div className="flex min-h-[250px] flex-1 flex-col gap-2.5" data-testid="catalog-groups">
      <div>
        <div className="text-[13px] font-semibold text-fg">Choose what to import</div>
        <p className="mt-0.5 text-[12.5px] text-muted">
          {entry.name} has {requestCount(operations)} in {groups.length} groups. Pick the groups you need.
        </p>
      </div>
      <div className="flex items-center gap-1.5">
        <div className="relative min-w-0 flex-1">
          <Search size={13} className="pointer-events-none absolute left-2 top-1/2 -translate-y-1/2 text-faint" />
          <Input aria-label="Search groups" data-focus="groups" value={pick.query} placeholder="Search groups" className="h-7 pl-7 text-[12.5px]" onChange={(e) => onChange({ ...pick, query: e.target.value })} />
        </div>
        <Button size="sm" variant="ghost" disabled={busy} onClick={() => onChange({ ...pick, ticked: tickAll(pick.ticked, shown, true) })}>
          Select all
        </Button>
        <Button size="sm" variant="ghost" disabled={busy} onClick={() => onChange({ ...pick, ticked: tickAll(pick.ticked, shown, false) })}>
          None
        </Button>
      </div>
      <ul aria-label="Groups" className="min-h-[96px] flex-1 overflow-y-auto rounded-lg border border-line py-1">
        {shown.map((g) => (
          <li key={g.tag}>
            <label className="flex cursor-pointer items-center gap-2.5 px-2.5 py-1 text-[12.5px] hover:bg-hover">
              <Checkbox checked={pick.ticked.has(g.tag)} onChange={(on) => toggle(g.tag, on)} label={groupLabel(g.tag)} />
              <span className="min-w-0 flex-1 truncate text-fg">{groupLabel(g.tag)}</span>
              <span className="shrink-0 tabular-nums text-[11.5px] text-faint">{g.operations.toLocaleString("en-US")}</span>
            </label>
          </li>
        ))}
        {!shown.length && <li className="px-2.5 py-2 text-[12px] text-muted">No group matches “{pick.query.trim()}”.</li>}
      </ul>
      <div className="flex items-center gap-2">
        <span className="mr-auto text-[12.5px] text-muted" role={busy ? "status" : undefined} data-testid="catalog-groups-total">
          {jobText ?? `${requestCount(total)} selected`}
        </span>
        <Button variant="ghost" disabled={busy} onClick={onBack}>
          Back
        </Button>
        <Button variant="primary" loading={busy} disabled={locked || !pick.ticked.size || total === 0} onClick={onImport}>
          Import
        </Button>
      </div>
    </div>
  );
}

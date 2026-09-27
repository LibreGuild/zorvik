// gRPC Service / Method picker in the URL bar (in place of the HTTP method): services from
// server reflection or the request's .proto files, searchable, grouped by service, with the
// streaming type of each method. Opening it loads the services when they aren't loaded yet.
import { useEffect, useId, useMemo, useRef, useState } from "react";
import { ChevronDown, CircleAlert, RefreshCw, Search } from "lucide-react";
import { Popover } from "radix-ui";
import type { GrpcMethod } from "../../bindings/GrpcMethod";
import { CALL_KIND_LABEL, callKind, filterServices, findMethod, loadServices, selectMethod, shortMethod, sourceKey, useGrpc, usesProtoFiles } from "../../store/grpc";
import { updateTab } from "../../store/tabs";
import { useWorkspace } from "../../store/workspace";
import { Button, cx, IconButton, Spinner } from "../ui";
import type { KindPaneProps } from "./kinds";

const COLOR = "var(--m-grpc)";

export function StreamBadge({ method, className }: { method: Pick<GrpcMethod, "clientStreaming" | "serverStreaming">; className?: string }) {
  const kind = callKind(method);
  if (kind === "unary") return null;
  return (
    <span className={cx("shrink-0 rounded bg-accent-soft px-1.5 text-[10px] font-semibold text-accent", className)} title={CALL_KIND_LABEL[kind]}>
      {kind === "bidi" ? "bidi" : kind === "server" ? "server stream" : "client stream"}
    </span>
  );
}

/** URL bar prefix of gRPC requests: the method is stored as `package.Service/Method`. */
export function GrpcMethodPrefix({ tab }: KindPaneProps) {
  const environment = useWorkspace((s) => s.info?.activeEnvironment);
  const key = sourceKey(tab.draft, environment);
  const entry = useGrpc((s) => s.services[key]);
  const [open, setOpen] = useState(false);
  const [q, setQ] = useState("");
  const [index, setIndex] = useState(0);
  const list = useRef<HTMLDivElement>(null);
  // Focus stays in the search field; screen readers follow the highlighted option through these ids.
  const listId = useId();
  const optionId = (i: number) => `${listId}-${i}`;
  const services = entry?.services ?? [];
  const results = useMemo(() => filterServices(services, q), [services, q]);
  const flat = useMemo(() => results.flatMap((s) => s.methods), [results]);
  const indexOf = useMemo(() => new Map(flat.map((m, i) => [m.path, i])), [flat]);
  const current = findMethod(services, tab.draft.method);
  const method = tab.draft.method.trim();

  useEffect(() => setIndex(0), [q, open]);
  useEffect(() => {
    list.current?.querySelector(`[data-index="${index}"]`)?.scrollIntoView({ block: "nearest" });
  }, [index]);

  const onOpenChange = (o: boolean) => {
    setOpen(o);
    if (!o) setQ("");
    // Load on first open; after an error, only the Retry button tries again.
    if (o && !entry && (tab.draft.url.trim() || usesProtoFiles(tab.draft))) void loadServices(tab);
  };
  const choose = (m: GrpcMethod | undefined) => {
    if (!m) return;
    selectMethod(tab.id, m);
    updateTab(tab.id, () => ({ requestTab: "message" }));
    setOpen(false);
    setQ("");
  };
  const fromFiles = usesProtoFiles(tab.draft);

  return (
    <Popover.Root open={open} onOpenChange={onOpenChange}>
      <Popover.Trigger asChild>
        <button
          aria-label="gRPC method"
          title={method || "Pick a method"}
          className="flex h-full min-w-[92px] max-w-[260px] items-center gap-1.5 rounded-l-xl border-r border-line pl-3 pr-2 outline-none hover:bg-hover"
        >
          <span className="shrink-0 font-mono text-[11px] font-bold" style={{ color: COLOR }}>
            gRPC
          </span>
          <span className={cx("min-w-0 truncate font-mono text-[12px]", method ? "text-fg" : "text-faint")} data-testid="grpc-method">
            {method ? shortMethod(method) : "Pick method"}
          </span>
          {current && <StreamBadge method={current} />}
          <ChevronDown size={13} className="shrink-0 text-faint" />
        </button>
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Content
          align="start"
          sideOffset={6}
          collisionPadding={12}
          className="zv-pop z-[90] flex max-h-[min(460px,var(--radix-popover-content-available-height))] w-[min(460px,calc(100vw-24px))] flex-col overflow-hidden rounded-xl border border-line bg-elev shadow-pop"
          onKeyDown={(e) => {
            if (e.key === "ArrowDown") {
              e.preventDefault();
              setIndex((i) => Math.min(i + 1, flat.length - 1));
            } else if (e.key === "ArrowUp") {
              e.preventDefault();
              setIndex((i) => Math.max(i - 1, 0));
            } else if (e.key === "Enter" && !e.nativeEvent.isComposing && e.keyCode !== 229) {
              e.preventDefault();
              choose(flat[index]);
            }
          }}
        >
          <div className="flex items-center gap-2 border-b border-line pl-3 pr-1.5">
            <Search size={13} className="shrink-0 text-faint" />
            <input
              autoFocus
              value={q}
              onChange={(e) => setQ(e.target.value)}
              placeholder="Search methods"
              aria-label="Search methods"
              aria-controls={listId}
              aria-activedescendant={flat[index] ? optionId(index) : undefined}
              className="h-10 min-w-0 flex-1 bg-transparent text-[13px] outline-none placeholder:text-faint"
            />
            <IconButton label={fromFiles ? "Reload the .proto files" : "Reload from the server (reflection)"} onClick={() => void loadServices(tab, true)}>
              {entry?.status === "loading" ? <Spinner size={13} /> : <RefreshCw size={13} />}
            </IconButton>
          </div>
          <div ref={list} id={listId} role="listbox" aria-label="Methods" className="min-h-0 flex-1 overflow-auto p-1.5" data-testid="grpc-methods">
            {entry?.status === "error" && (
              <div className="m-1 flex items-start gap-2 rounded-lg border border-danger/30 bg-danger/5 px-3 py-2 text-[12px] text-fg" data-testid="grpc-services-error">
                <CircleAlert size={14} className="mt-0.5 shrink-0 text-danger" />
                <div className="min-w-0 flex-1">
                  <div className="selectable whitespace-pre-wrap break-words">{entry.error}</div>
                  <Button size="sm" variant="ghost" className="-ml-2 mt-1" onClick={() => void loadServices(tab, true)}>
                    Retry
                  </Button>
                </div>
              </div>
            )}
            {!entry ? (
              <div className="flex flex-col items-center gap-2 p-5 text-center text-[12.5px] text-muted">
                {tab.draft.url.trim() || fromFiles
                  ? "Services are not loaded yet."
                  : "Enter the server URL (grpc://host:port) to load its services through reflection, or add .proto files in the Proto files tab."}
                {(tab.draft.url.trim() || fromFiles) && (
                  <Button size="sm" onClick={() => void loadServices(tab)}>
                    Load services
                  </Button>
                )}
              </div>
            ) : entry.status === "loading" && services.length === 0 ? (
              <div className="flex items-center justify-center gap-2 p-5 text-[12.5px] text-muted">
                <Spinner /> Loading services…
              </div>
            ) : services.length === 0 && entry.status === "loaded" ? (
              <div className="p-5 text-center text-[12.5px] text-muted">The {entry.source ?? "server"} has no services.</div>
            ) : results.length === 0 && services.length > 0 ? (
              <div className="p-5 text-center text-[12.5px] text-muted">No methods match “{q}”.</div>
            ) : (
              results.map((s) => (
                <div key={s.name} className="mb-1">
                  <div className="truncate px-2 pb-0.5 pt-1.5 font-mono text-[11px] font-semibold text-faint" title={s.name}>
                    {s.name}
                  </div>
                  {s.methods.map((m) => {
                    const i = indexOf.get(m.path) ?? 0;
                    return (
                      <button
                        key={m.path}
                        type="button"
                        id={optionId(i)}
                        role="option"
                        aria-selected={i === index}
                        data-index={i}
                        onMouseEnter={() => setIndex(i)}
                        onClick={() => choose(m)}
                        className={cx("flex w-full items-center gap-2 rounded-lg px-2 py-1.5 text-left", i === index && "bg-hover", m.path === method && "text-accent")}
                      >
                        <span className="min-w-0 flex-1">
                          <span className="block truncate font-mono text-[12.5px]">{m.name}</span>
                          <span className="block truncate font-mono text-[10.5px] text-faint">
                            {m.inputType.split(".").pop()} → {m.outputType.split(".").pop()}
                          </span>
                        </span>
                        <StreamBadge method={m} />
                      </button>
                    );
                  })}
                </div>
              ))
            )}
          </div>
          <div className="flex items-center gap-2 border-t border-line px-3 py-2 text-[11px] text-faint">
            <span className="min-w-0 flex-1 truncate">
              {entry?.source ? `From ${entry.source}` : fromFiles ? "From the .proto files" : "From server reflection"}
            </span>
          </div>
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  );
}

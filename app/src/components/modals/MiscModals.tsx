import { useEffect, useMemo, useRef, useState } from "react";
import { Cookie, FileJson, Globe, GraduationCap, ListChecks, Search, Terminal, Trash2 } from "lucide-react";
import type { CookieInfo } from "../../bindings/CookieInfo";
import type { SpecUpdate } from "../../bindings/SpecUpdate";
import type { FolderMeta } from "../../bindings/FolderMeta";
import type { ImportSummary } from "../../bindings/ImportSummary";
import type { TreeNode } from "../../bindings/TreeNode";
import type { WorkspaceMeta } from "../../bindings/WorkspaceMeta";
import { GRAPHQL_BADGE, methodColor, methodLabel } from "../../lib/http";
import { copyText, pickFile } from "../../lib/platform";
import { api, errorMessage, RpcError } from "../../lib/rpc";
import { confirm } from "../../store/dialogs";
import { useLoadTests } from "../../store/loadtests";
import { useServers } from "../../store/servers";
import { isRequestTab, openDraft, openLoadTest, openRequest, openRunner, openServer, openTool, saveTabAs, useTabs } from "../../store/tabs";
import { formatDuration, MODELS } from "../loadtests/model";
import { SERVER_KINDS } from "../servers/kinds";
import { TOOLS } from "../tools/registry";
import { toast } from "../../store/toasts";
import { openAcademy } from "../../store/academy";
import { closeModal, toggleExpanded } from "../../store/ui";
import { refreshEnvironments, refreshTree, reloadWorkspace, useWorkspace } from "../../store/workspace";
import { CodeEditor } from "../CodeEditor";
import { KeyValueEditor } from "../KeyValueEditor";
import { AuthEditor } from "../request/AuthEditor";
import { ScriptsEditor, ScriptsTabLabel } from "../request/ScriptsEditor";
import { moveItem } from "../sidebar/Sidebar";
import { Button, cx, EmptyState, Field, Input, Kbd, Modal, Segmented, Select, Spinner, Switch, Tabs } from "../ui";
import { findVariant, initialVariant, rememberVariant, searchLanguages } from "./exportTargets";

// Stable fallback: a selector returning a fresh [] on every call makes React re-render forever.
const NO_NODES: TreeNode[] = [];

function folderOptions(nodes: TreeNode[], depth = 0): { path: string; label: string }[] {
  return nodes
    .filter((n) => n.kind === "folder")
    .flatMap((n) => [{ path: n.path, label: `${"  ".repeat(depth)}${n.name}` }, ...folderOptions(n.children, depth + 1)]);
}

function FolderSelect({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  const tree = useWorkspace((s) => s.info?.tree ?? NO_NODES);
  const options = useMemo(() => folderOptions(tree), [tree]);
  return (
    <Select value={value} onChange={(e) => onChange(e.target.value)} className="w-full">
      <option value="">Workspace root</option>
      {options.map((o) => (
        <option key={o.path} value={o.path}>
          {o.label.replace(/^( +)/, (m) => "  ".repeat(m.length / 2))}
        </option>
      ))}
    </Select>
  );
}

// ---- Import --------------------------------------------------------------------

export function ImportModal({ parent: initialParent }: { parent?: string }) {
  const [tab, setTab] = useState<"file" | "curl" | "url">("file");
  const [parent, setParent] = useState(initialParent ?? "");
  const [curl, setCurl] = useState("");
  const [url, setUrl] = useState("");
  const [busy, setBusy] = useState(false);
  const [summary, setSummary] = useState<ImportSummary | null>(null);
  const [dragging, setDragging] = useState(false);
  // An OpenAPI document without a full server URL: the import again, once the user gives one.
  const [needsBase, setNeedsBase] = useState<{ message: string; retry: (baseUrl: string) => Promise<ImportSummary> } | null>(null);
  const [baseUrl, setBaseUrl] = useState("");

  const finish = async (s: ImportSummary) => {
    setSummary(s);
    if (s.folderPath) toggleExpanded(s.folderPath, true);
    await refreshTree();
    if (s.environments) await refreshEnvironments();
  };

  const run = async (fn: () => Promise<void>) => {
    setBusy(true);
    try {
      await fn();
    } catch (e) {
      toast("error", "Import failed", errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  /** Import with `start`; when the API's base URL is missing, ask for it and retry with it. */
  const importing = (start: (baseUrl?: string) => Promise<ImportSummary>) =>
    run(async () => {
      try {
        await finish(await start());
      } catch (e) {
        if (e instanceof RpcError && e.code === "needsBaseUrl") {
          setNeedsBase({ message: e.message, retry: (b) => start(b) });
          return;
        }
        throw e;
      }
    });

  return (
    <Modal
      open
      onClose={closeModal}
      title="Import"
      description="Postman collections & environments, OpenAPI 3 / Swagger 2 (JSON or YAML), or a cURL command."
      width={640}
      bodyClassName="p-0"
      footer={
        summary ? (
          <Button variant="primary" onClick={closeModal}>
            Done
          </Button>
        ) : (
          <>
            <Button variant="ghost" onClick={closeModal}>
              Cancel
            </Button>
            {tab === "curl" && (
              <Button
                variant="primary"
                loading={busy}
                disabled={!curl.trim()}
                onClick={() =>
                  run(async () => {
                    const r = await api.importCurl(curl);
                    openDraft(r.request);
                    if (r.warnings.length) toast("info", "Imported with notes", r.warnings.join("\n"));
                    closeModal();
                  })
                }
              >
                Open as new request
              </Button>
            )}
            {needsBase ? (
              <Button
                variant="primary"
                loading={busy}
                disabled={!baseUrl.trim().includes("://")}
                onClick={() =>
                  run(async () => {
                    await finish(await needsBase.retry(baseUrl.trim()));
                    setNeedsBase(null);
                  })
                }
              >
                Import
              </Button>
            ) : (
              tab === "url" && (
                <Button variant="primary" loading={busy} disabled={!url.trim()} onClick={() => importing((b) => api.importUrl(url.trim(), parent, b))}>
                  Import
                </Button>
              )
            )}
          </>
        )
      }
    >
      {summary ? (
        <div className="p-5">
          <div className="text-[14px] font-semibold text-fg">Imported “{summary.name}”</div>
          <div className="mt-1 text-[13px] text-muted">
            {summary.requests} requests · {summary.folders} folders{summary.environments ? ` · ${summary.environments} environment` : ""}
            {summary.workspaceVariables ? ` · ${summary.workspaceVariables} workspace variable${summary.workspaceVariables === 1 ? "" : "s"}` : ""}
          </div>
          {summary.warnings.length > 0 && (
            <div className="mt-4">
              <div className="mb-1.5 text-[12px] font-medium text-warning">{summary.warnings.length} note{summary.warnings.length > 1 ? "s" : ""}</div>
              <ul className="selectable max-h-60 list-disc overflow-auto rounded-md border border-line bg-panel-2 py-2 pl-7 pr-3 text-[12px] text-muted">
                {summary.warnings.map((w, i) => (
                  <li key={i}>{w}</li>
                ))}
              </ul>
            </div>
          )}
        </div>
      ) : (
        <>
          <Tabs
            items={[
              { id: "file", label: "File" },
              { id: "curl", label: "cURL" },
              { id: "url", label: "OpenAPI URL" },
            ]}
            value={tab}
            onChange={setTab}
          />
          <div className="flex flex-col gap-4 p-5">
            {needsBase && (
              <div className="rounded-lg border border-warning/30 bg-warning/10 p-3" data-testid="import-base-url">
                <p className="text-[12.5px] text-fg">{needsBase.message}</p>
                <div className="mt-2.5">
                  <Input
                    value={baseUrl}
                    onChange={(e) => setBaseUrl(e.target.value)}
                    placeholder="https://api.example.com"
                    aria-label="Base URL"
                    autoFocus
                  />
                </div>
                <p className="mt-1.5 text-[11.5px] text-faint">It goes into the new environment as baseUrl; you can change it there later.</p>
              </div>
            )}
            {tab !== "curl" && (
              <Field label="Import into">
                <FolderSelect value={parent} onChange={setParent} />
              </Field>
            )}
            {tab === "file" && (
              <button
                disabled={busy}
                onClick={() =>
                  run(async () => {
                    const path = await pickFile("Choose a file to import", ["json", "yaml", "yml", "txt", "sh"]);
                    if (path) await importing((b) => api.importFile(path, parent, b));
                  })
                }
                onDragOver={(e) => {
                  e.preventDefault();
                  setDragging(true);
                }}
                onDragLeave={() => setDragging(false)}
                onDrop={(e) => {
                  e.preventDefault();
                  setDragging(false);
                  const file = e.dataTransfer.files[0];
                  if (file) void file.text().then((text) => importing((b) => api.importText(text, parent, b)));
                }}
                className={cx(
                  "flex h-36 flex-col items-center justify-center gap-2 rounded-xl border-2 border-dashed text-muted transition-colors hover:border-accent hover:text-fg",
                  dragging ? "border-accent bg-accent-soft text-fg" : "border-line-strong",
                )}
              >
                {busy ? <Spinner size={20} /> : <FileJson size={24} />}
                <span className="text-[13px] font-medium">Choose a file, or drop it here</span>
                <span className="text-[11.5px] text-faint">Postman, OpenAPI / Swagger (JSON or YAML) — detected automatically</span>
              </button>
            )}
            {tab === "curl" && (
              <div className="h-56 overflow-hidden rounded-md border border-line bg-input">
                <CodeEditor value={curl} onChange={setCurl} placeholder="curl 'https://api.example.com' -H 'Accept: application/json'" lineNumbers={false} autoFocus />
              </div>
            )}
            {tab === "url" && (
              <Field label="OpenAPI / Swagger URL">
                <Input value={url} onChange={(e) => setUrl(e.target.value)} placeholder="https://petstore3.swagger.io/api/v3/openapi.json" autoFocus />
              </Field>
            )}
          </div>
        </>
      )}
    </Modal>
  );
}

// ---- Export (cURL or code) -----------------------------------------------------

export function ExportModal({ tabId }: { tabId: string }) {
  const tab = useTabs((s) => {
    const t = s.tabs.find((x) => x.id === tabId);
    return isRequestTab(t) ? t : undefined;
  });
  const [variantId, setVariantId] = useState(() => initialVariant(navigator.userAgent.includes("Windows")));
  const [query, setQuery] = useState("");
  const [resolveVars, setResolveVars] = useState(true);
  const [text, setText] = useState("");
  const [error, setError] = useState<string | null>(null);
  const draft = tab?.draft;
  const path = tab?.path ?? null;
  const found = findVariant(variantId) ?? findVariant("curl-bash")!;
  const { language, variant } = found;
  const languages = useMemo(() => searchLanguages(query), [query]);
  const choose = (id: string) => {
    setVariantId(id);
    rememberVariant(id);
  };
  // Depends on the request, not the whole tab (which changes with every streamed message);
  // `alive` drops a slower, older result so the text always matches the chosen options.
  useEffect(() => {
    if (!draft) return;
    let alive = true;
    const target = variant.target;
    (target.kind === "curl"
      ? api.exportCurl(draft, path, target.flavor, resolveVars)
      : api.exportSnippet(draft, path, target.language, resolveVars)
    )
      .then((t) => {
        if (!alive) return;
        setText(t);
        setError(null);
      })
      .catch((e) => {
        if (!alive) return;
        setText("");
        setError(errorMessage(e));
      });
    return () => {
      alive = false;
    };
  }, [draft, path, variant, resolveVars]);
  if (!tab) return null;
  // Up and Down in the search box move through the languages shown.
  const step = (by: number) => {
    const at = languages.findIndex((l) => l.id === language.id);
    const next = languages[Math.min(languages.length - 1, Math.max(0, (at < 0 ? -1 : at) + by))];
    if (next) choose(next.variants[0].id);
  };
  const groups = (["Command line", "Code"] as const).map((g) => ({ group: g, items: languages.filter((l) => l.group === g) })).filter((g) => g.items.length);
  return (
    <Modal
      open
      onClose={closeModal}
      title="Copy as cURL or code"
      width={880}
      footer={
        <>
          <Button variant="ghost" onClick={closeModal}>
            Close
          </Button>
          <Button
            variant="primary"
            icon={<Terminal size={14} />}
            disabled={!text}
            onClick={() => {
              void copyText(text);
              toast("success", "Copied to clipboard");
              closeModal();
            }}
          >
            Copy
          </Button>
        </>
      }
    >
      <div className="flex h-[440px] min-h-0 gap-3">
        <div className="flex w-48 shrink-0 flex-col gap-2">
          <div className="relative">
            <Search size={13} className="pointer-events-none absolute left-2 top-1/2 -translate-y-1/2 text-faint" />
            <Input
              aria-label="Search languages"
              value={query}
              autoFocus
              placeholder="Search"
              className="pl-7"
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "ArrowDown" || e.key === "ArrowUp") {
                  e.preventDefault();
                  step(e.key === "ArrowDown" ? 1 : -1);
                }
              }}
            />
          </div>
          <div role="listbox" aria-label="Language" className="min-h-0 flex-1 overflow-y-auto pr-1">
            {groups.map(({ group, items }) => (
              <div key={group} role="group" aria-label={group} className="mb-2">
                <div className="px-2 pb-1 pt-1.5 text-[10.5px] font-semibold uppercase tracking-wide text-faint">{group}</div>
                {items.map((l) => (
                  <button
                    key={l.id}
                    role="option"
                    aria-selected={l.id === language.id}
                    onClick={() => choose(l.variants[0].id)}
                    className={cx(
                      "flex h-7 w-full items-center justify-between rounded-md px-2 text-left text-[12.5px]",
                      l.id === language.id ? "bg-accent-soft font-medium text-accent" : "text-fg hover:bg-hover",
                    )}
                  >
                    {l.label}
                    {l.variants.length > 1 && <span className="text-[11px] text-faint">{l.variants.length}</span>}
                  </button>
                ))}
              </div>
            ))}
            {!groups.length && <div className="px-2 py-3 text-[12px] text-muted">No language matches “{query}”.</div>}
          </div>
        </div>
        <div className="flex min-w-0 flex-1 flex-col gap-2">
          <div className="flex min-h-8 flex-wrap items-center gap-3">
            {language.variants.length > 1 ? (
              <Segmented items={language.variants.map((v) => ({ id: v.id, label: v.label }))} value={variant.id} onChange={choose} />
            ) : (
              <span className="text-[12.5px] text-muted">{variant.label}</span>
            )}
            <div className="flex-1" />
            <Switch checked={resolveVars} onChange={setResolveVars} label="Substitute variables" />
          </div>
          {error ? (
            <div className="rounded-md border border-danger/30 bg-danger/10 p-3 text-[12.5px] text-danger">{error}</div>
          ) : (
            <div className="min-h-0 flex-1 overflow-hidden rounded-md border border-line bg-panel-2" data-testid="export-code">
              <CodeEditor value={text} readOnly lineNumbers={false} language={language.highlight} />
            </div>
          )}
          {resolveVars && <p className="text-[11.5px] text-faint">Contains resolved values — including secrets — so be careful where you paste it.</p>}
        </div>
      </div>
    </Modal>
  );
}

// ---- Save as ---------------------------------------------------------------------

export function SaveAsModal({ tabId }: { tabId: string }) {
  const tab = useTabs((s) => {
    const t = s.tabs.find((x) => x.id === tabId);
    return isRequestTab(t) ? t : undefined;
  });
  const [name, setName] = useState(tab?.draft.name ?? "");
  const [parent, setParent] = useState(tab?.path?.includes("/") ? tab.path.slice(0, tab.path.lastIndexOf("/")) : "");
  const [busy, setBusy] = useState(false);
  if (!tab) return null;
  const submit = async () => {
    // Enter can fire again while saving; a second save would create a duplicate request.
    if (!name.trim() || busy) return;
    setBusy(true);
    const ok = await saveTabAs(tabId, parent, name.trim());
    setBusy(false);
    if (ok) {
      if (parent) toggleExpanded(parent, true);
      closeModal();
    }
  };
  return (
    <Modal
      open
      onClose={closeModal}
      title="Save request"
      width={480}
      footer={
        <>
          <Button variant="ghost" onClick={closeModal}>
            Cancel
          </Button>
          <Button variant="primary" onClick={submit} loading={busy} disabled={!name.trim()}>
            Save
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-4">
        <Field label="Name">
          <Input
            value={name}
            onChange={(e) => setName(e.target.value)}
            autoFocus
            onKeyDown={(e) => e.key === "Enter" && !e.nativeEvent.isComposing && e.keyCode !== 229 && submit()}
          />
        </Field>
        <Field label="Folder">
          <FolderSelect value={parent} onChange={setParent} />
        </Field>
      </div>
    </Modal>
  );
}

// ---- Move ------------------------------------------------------------------------

export function MoveModal({ path, name }: { path: string; name: string }) {
  const current = path.includes("/") ? path.slice(0, path.lastIndexOf("/")) : "";
  const [parent, setParent] = useState(current);
  const [busy, setBusy] = useState(false);
  const invalid = parent === path || parent.startsWith(`${path}/`);
  return (
    <Modal
      open
      onClose={closeModal}
      title={`Move “${name}”`}
      width={480}
      footer={
        <>
          <Button variant="ghost" onClick={closeModal}>
            Cancel
          </Button>
          <Button
            variant="primary"
            loading={busy}
            disabled={invalid || parent === current}
            onClick={async () => {
              setBusy(true);
              await moveItem(path, parent, null);
              setBusy(false);
              closeModal();
            }}
          >
            Move
          </Button>
        </>
      }
    >
      <Field label="Destination folder" hint={invalid ? "A folder cannot be moved into itself." : undefined}>
        <FolderSelect value={parent} onChange={setParent} />
      </Field>
    </Modal>
  );
}

// ---- Cookies -----------------------------------------------------------------------

export function CookiesModal() {
  const [cookies, setCookies] = useState<CookieInfo[] | null>(null);
  const load = () => api.cookies().then(setCookies).catch((e) => toast("error", "Could not load cookies", errorMessage(e)));
  const change = async (fn: () => Promise<unknown>) => {
    try {
      await fn();
    } catch (e) {
      toast("error", "Could not update cookies", errorMessage(e));
    }
    await load();
  };
  useEffect(() => {
    void load();
  }, []);
  const domains = useMemo(() => {
    const map = new Map<string, CookieInfo[]>();
    for (const c of cookies ?? []) map.set(c.domain, [...(map.get(c.domain) ?? []), c]);
    return [...map.entries()];
  }, [cookies]);
  return (
    <Modal
      open
      onClose={closeModal}
      title="Cookies"
      description="Cookies stored by responses in this workspace. They are sent automatically on matching requests."
      width={760}
      footer={
        <>
          <Button
            variant="ghost"
            icon={<Trash2 size={14} />}
            className="mr-auto"
            disabled={!cookies?.length}
            onClick={async () => {
              const ok = await confirm({
                title: "Clear all cookies?",
                message: "Every cookie stored for this workspace is deleted. This cannot be undone.",
                confirmLabel: "Clear all",
                danger: true,
              });
              if (ok) await change(() => api.clearCookies());
            }}
          >
            Clear all
          </Button>
          <Button variant="primary" onClick={closeModal}>
            Done
          </Button>
        </>
      }
    >
      {cookies === null ? (
        <div className="flex justify-center p-6">
          <Spinner />
        </div>
      ) : cookies.length === 0 ? (
        <EmptyState icon={<Cookie size={24} strokeWidth={1.5} />} title="No cookies stored" />
      ) : (
        <div className="flex flex-col gap-4">
          {domains.map(([domain, list]) => (
            <div key={domain} className="overflow-hidden rounded-lg border border-line">
              <div className="flex items-center gap-2 border-b border-line bg-panel-2 px-3 py-1.5 text-[12.5px] font-medium text-fg">
                <Globe size={13} className="text-muted" /> {domain}
              </div>
              {list.map((c) => (
                <div key={`${c.path}:${c.name}`} className="group flex items-center gap-3 border-b border-line px-3 py-1.5 text-[12.5px] last:border-0">
                  <span className="w-40 shrink-0 truncate font-mono text-fg">{c.name}</span>
                  <span className="selectable min-w-0 flex-1 truncate font-mono text-muted">{c.value}</span>
                  <span className="shrink-0 text-[11px] text-faint">{c.expires ? new Date(c.expires).toLocaleDateString() : "session"}</span>
                  <button
                    aria-label="Delete cookie"
                    onClick={() => change(() => api.deleteCookie(c))}
                    className="rounded p-1 text-faint opacity-0 hover:bg-hover hover:text-danger group-hover:opacity-100"
                  >
                    <Trash2 size={13} />
                  </button>
                </div>
              ))}
            </div>
          ))}
        </div>
      )}
    </Modal>
  );
}

// ---- Workspace / folder settings --------------------------------------------------------

export function WorkspaceSettingsModal() {
  const info = useWorkspace((s) => s.info)!;
  const [meta, setMeta] = useState<WorkspaceMeta>(info.meta);
  const dirty = useMemo(() => JSON.stringify(meta) !== JSON.stringify(info.meta), [meta, info.meta]);
  const [tab, setTab] = useState<"general" | "auth" | "headers" | "scripts">("general");
  const [busy, setBusy] = useState(false);
  const save = async () => {
    setBusy(true);
    try {
      await api.saveWorkspaceMeta(meta);
      await reloadWorkspace();
      toast("success", "Workspace saved");
      closeModal();
    } catch (e) {
      toast("error", "Could not save workspace", errorMessage(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Modal
      open
      onClose={closeModal}
      title="Workspace settings"
      width={760}
      focusFirstField={false}
      dirty={dirty}
      bodyClassName="p-0"
      footer={
        <>
          <Button variant="ghost" onClick={closeModal}>
            Cancel
          </Button>
          <Button variant="primary" onClick={save} loading={busy} disabled={!meta.name.trim()}>
            Save
          </Button>
        </>
      }
    >
      <Tabs
        items={[
          { id: "general", label: "General" },
          { id: "auth", label: "Default auth" },
          { id: "headers", label: "Default headers" },
          { id: "scripts", label: <ScriptsTabLabel scripts={meta.scripts} /> },
        ]}
        value={tab}
        onChange={setTab}
      />
      <div className="h-[380px] overflow-auto">
        {tab === "general" && (
          <div className="flex flex-col gap-4 p-5">
            <Field label="Name">
              <Input value={meta.name} onChange={(e) => setMeta({ ...meta, name: e.target.value })} />
            </Field>
            <Field label="Location">
              <div className="selectable break-all rounded-md border border-line bg-panel-2 px-2.5 py-1.5 font-mono text-[12px] text-muted">{info.path}</div>
            </Field>
            <p className="text-[12px] text-faint">Workspace variables are edited under Environments.</p>
          </div>
        )}
        {tab === "auth" && <AuthEditor auth={meta.auth ?? { type: "none" }} onChange={(auth) => setMeta({ ...meta, auth })} allowInherit={false} />}
        {tab === "headers" && (
          <div className="pt-2">
            <KeyValueEditor rows={meta.headers ?? []} onChange={(headers) => setMeta({ ...meta, headers })} keyPlaceholder="Header" />
            <p className="p-3 text-[12px] text-faint">Sent with every request in the workspace unless a folder or request sets the same header.</p>
          </div>
        )}
        {tab === "scripts" && <ScriptsEditor where="workspace" scripts={meta.scripts ?? {}} onChange={(scripts) => setMeta({ ...meta, scripts })} />}
      </div>
    </Modal>
  );
}

export function FolderSettingsModal({ path }: { path: string }) {
  const [meta, setMeta] = useState<FolderMeta | null>(null);
  const [loaded, setLoaded] = useState<FolderMeta | null>(null);
  const [tab, setTab] = useState<"auth" | "headers" | "scripts" | "docs" | "spec">("auth");
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    api
      .readFolder(path)
      .then((m) => {
        setMeta(m);
        setLoaded(m);
      })
      .catch((e) => {
        toast("error", "Could not load folder", errorMessage(e));
        closeModal();
      });
  }, [path]);
  if (!meta) return null;
  const save = async () => {
    setBusy(true);
    try {
      await api.saveFolder(path, meta);
      await refreshTree();
      toast("success", "Folder saved");
      closeModal();
    } catch (e) {
      toast("error", "Could not save folder", errorMessage(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Modal
      open
      onClose={closeModal}
      title={`Folder: ${meta.name}`}
      description="Auth, headers and scripts here apply to every request inside this folder."
      width={760}
      focusFirstField={false}
      dirty={JSON.stringify(meta) !== JSON.stringify(loaded)}
      bodyClassName="p-0"
      footer={
        <>
          <Button variant="ghost" onClick={closeModal}>
            Cancel
          </Button>
          <Button variant="primary" onClick={save} loading={busy}>
            Save
          </Button>
        </>
      }
    >
      <Tabs
        items={[
          { id: "auth", label: "Auth" },
          { id: "headers", label: "Headers" },
          { id: "scripts", label: <ScriptsTabLabel scripts={meta.scripts} /> },
          { id: "docs", label: "Docs" },
          ...(meta.openapi ? [{ id: "spec" as const, label: "API spec" }] : []),
        ]}
        value={tab}
        onChange={setTab}
      />
      <div className="h-[380px] overflow-auto">
        {tab === "spec" && meta.openapi && (
          <div className="flex flex-col gap-4 p-5 text-[12.5px]">
            <p className="text-muted">
              This folder was imported from an OpenAPI document, kept at <code className="font-mono text-fg">{meta.openapi.spec}</code>
              {meta.openapi.source ? (
                <>
                  {" "}
                  (from <span className="break-all font-mono text-fg">{meta.openapi.source}</span>)
                </>
              ) : null}
              .
            </p>
            <Switch
              checked={meta.openapi.validate !== false}
              onChange={(on) => setMeta({ ...meta, openapi: { ...meta.openapi!, validate: on ? undefined : false } })}
              label="Check responses against the spec"
            />
            <p className="-mt-2 text-[11.5px] text-faint">
              Each response to a request imported from it gets a test, “Matches the API spec”: is the status documented, and does a JSON body match the
              documented schema?
            </p>
          </div>
        )}
        {tab === "auth" && <AuthEditor auth={meta.auth ?? { type: "inherit" }} onChange={(auth) => setMeta({ ...meta, auth })} path={`${path}/_`} />}
        {tab === "headers" && (
          <div className="pt-2">
            <KeyValueEditor rows={meta.headers ?? []} onChange={(headers) => setMeta({ ...meta, headers })} keyPlaceholder="Header" />
          </div>
        )}
        {tab === "scripts" && <ScriptsEditor where="folder" scripts={meta.scripts ?? {}} onChange={(scripts) => setMeta({ ...meta, scripts })} />}
        {tab === "docs" && <CodeEditor value={meta.docs ?? ""} onChange={(docs) => setMeta({ ...meta, docs })} placeholder="Notes about this folder" lineNumbers={false} />}
      </div>
    </Modal>
  );
}

// ---- Update from an API spec ---------------------------------------------------------------

export function SpecUpdateModal({ folder, name }: { folder: string; name: string }) {
  const [source, setSource] = useState<{ kind: "url" | "file" | "text"; value: string }>({ kind: "url", value: "" });
  const [plan, setPlan] = useState<SpecUpdate | null>(null);
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState<SpecUpdate | null>(null);
  useEffect(() => {
    api
      .readFolder(folder)
      .then((m) => {
        const from = m.openapi?.source ?? "";
        if (from) setSource({ kind: /^https?:\/\//.test(from) ? "url" : "file", value: from });
      })
      .catch(() => {});
  }, [folder]);
  const from = () =>
    source.kind === "url" ? { url: source.value.trim() } : source.kind === "file" ? { path: source.value.trim() } : { text: source.value };
  const run = async (apply: boolean) => {
    setBusy(true);
    try {
      const result = await api.specUpdate(folder, from(), apply);
      if (apply) {
        setDone(result);
        await refreshTree();
        await refreshEnvironments();
      } else setPlan(result);
    } catch (e) {
      toast("error", apply ? "Could not update" : "Could not read the document", errorMessage(e));
    } finally {
      setBusy(false);
    }
  };
  const result = done ?? plan;
  const nothing = result && !result.added.length && !result.changed.length && !result.removed.length && !result.restored.length && !result.variables.length;
  return (
    <Modal
      open
      onClose={closeModal}
      title={`Update “${name}” from its API spec`}
      description="New operations are added. Changed ones are updated where you haven't edited them; your docs, scripts and edits stay. Operations the spec no longer has are kept and marked."
      width={680}
      footer={
        done ? (
          <Button variant="primary" onClick={closeModal}>
            Done
          </Button>
        ) : (
          <>
            <Button variant="ghost" onClick={closeModal}>
              Cancel
            </Button>
            {plan && !nothing ? (
              <Button variant="primary" loading={busy} onClick={() => void run(true)} data-testid="spec-update-apply">
                Update
              </Button>
            ) : (
              <Button variant="primary" loading={busy} disabled={!source.value.trim()} onClick={() => void run(false)} data-testid="spec-update-preview">
                Preview changes
              </Button>
            )}
          </>
        )
      }
    >
      {!result && (
        <div className="flex flex-col gap-3">
          <Tabs
            items={[
              { id: "url", label: "URL" },
              { id: "file", label: "File" },
              { id: "text", label: "Paste" },
            ]}
            value={source.kind}
            onChange={(kind) => setSource({ kind, value: kind === source.kind ? source.value : "" })}
          />
          {source.kind === "url" && (
            <Input value={source.value} onChange={(e) => setSource({ ...source, value: e.target.value })} placeholder="https://api.example.com/openapi.json" autoFocus />
          )}
          {source.kind === "file" && (
            <div className="flex gap-2">
              <Input value={source.value} onChange={(e) => setSource({ ...source, value: e.target.value })} placeholder="openapi.yaml" />
              <Button
                onClick={async () => {
                  const p = await pickFile("Choose the new OpenAPI document", ["json", "yaml", "yml"]);
                  if (p) setSource({ kind: "file", value: p });
                }}
              >
                Choose…
              </Button>
            </div>
          )}
          {source.kind === "text" && (
            <div className="h-56 overflow-hidden rounded-md border border-line bg-input">
              <CodeEditor value={source.value} onChange={(value) => setSource({ kind: "text", value })} placeholder="openapi: 3.0.0 …" lineNumbers={false} />
            </div>
          )}
        </div>
      )}
      {result && (
        <div className="flex flex-col gap-3 text-[12.5px]" data-testid="spec-update-plan">
          <div className="font-medium text-fg">
            {done ? "Updated. " : ""}
            {nothing
              ? "Nothing to change: the folder already matches this version."
              : `${result.added.length} added · ${result.changed.length + result.restored.length} changed · ${result.removed.length} removed · ${result.unchanged} unchanged`}
          </div>
          {result.warnings.map((w, i) => (
            <p key={i} className="rounded-md border border-warning/30 bg-warning/10 p-2.5 text-[12px] text-fg">
              {w}
            </p>
          ))}
          <SpecList title="Added" tone="text-success" items={result.added.map((c) => ({ key: c.operation, text: c.operation, note: c.name }))} />
          <SpecList
            title="Changed"
            tone="text-accent"
            items={[...result.changed, ...result.restored].map((c) => ({
              key: c.operation,
              text: c.operation,
              note: [c.fields.length ? `updates ${c.fields.join(", ")}` : "", c.kept.length ? `keeps your ${c.kept.join(", ")}` : ""].filter(Boolean).join(" · "),
            }))}
          />
          <SpecList
            title="No longer in the spec (kept, marked)"
            tone="text-danger"
            items={result.removed.map((c) => ({ key: c.operation, text: c.operation, note: c.path ?? "" }))}
          />
          {result.variables.length > 0 && <p className="text-muted">New environment variables: {result.variables.join(", ")}</p>}
        </div>
      )}
    </Modal>
  );
}

function SpecList({ title, tone, items }: { title: string; tone: string; items: { key: string; text: string; note: string }[] }) {
  if (!items.length) return null;
  return (
    <div>
      <div className={cx("mb-1 text-[11.5px] font-semibold uppercase tracking-wide", tone)}>{title}</div>
      <ul className="max-h-40 overflow-auto rounded-md border border-line bg-panel-2 py-1">
        {items.map((i) => (
          <li key={i.key} className="flex gap-3 px-3 py-1">
            <span className="shrink-0 font-mono text-[12px] text-fg">{i.text}</span>
            <span className="min-w-0 truncate text-faint">{i.note}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}

// ---- Command palette ----------------------------------------------------------------------

function flatten(nodes: TreeNode[], trail: string[] = []): { node: TreeNode; trail: string }[] {
  return nodes.flatMap((n) =>
    n.kind === "folder" ? flatten(n.children, [...trail, n.name]) : [{ node: n, trail: trail.join(" / ") }],
  );
}

interface PaletteItem {
  key: string;
  badge: React.ReactNode;
  color: string;
  name: string;
  trail: string;
  /** Extra words to match. */
  terms: string;
  open: () => void;
}

export function CommandPalette() {
  const tree = useWorkspace((s) => s.info?.tree ?? NO_NODES);
  const workspaceName = useWorkspace((s) => s.info?.meta.name ?? "");
  const servers = useServers((s) => s.saved);
  const loadTests = useLoadTests((s) => s.saved);
  const [q, setQ] = useState("");
  const [index, setIndex] = useState(0);
  const list = useRef<HTMLDivElement>(null);
  const all = useMemo<PaletteItem[]>(
    () => [
      {
        key: "run:collection",
        badge: <ListChecks size={13} />,
        color: "var(--accent)",
        name: "Run collection",
        trail: "Runner",
        terms: "runner run collection tests",
        open: () => openRunner("", workspaceName),
      },
      {
        key: "academy",
        badge: <GraduationCap size={13} />,
        color: "var(--accent)",
        name: "Open the Academy",
        trail: "Training Bootcamp",
        terms: "academy bootcamp learn course lessons training tutorial",
        open: () => void openAcademy(),
      },
      ...flatten(tree).map(({ node, trail }) => ({
        key: `r:${node.path}`,
        badge: node.graphql ? GRAPHQL_BADGE.label : methodLabel(node.method, node.requestKind),
        color: node.graphql ? GRAPHQL_BADGE.color : methodColor(node.method, node.requestKind),
        name: node.name,
        trail,
        terms: node.method ?? "",
        open: () => void openRequest(node.path),
      })),
      ...servers.map((n) => ({
        key: `s:${n.id}`,
        badge: SERVER_KINDS[n.kind].short,
        color: SERVER_KINDS[n.kind].color,
        name: n.name,
        trail: `Server · :${n.port}`,
        terms: "server",
        open: () => void openServer(n.id),
      })),
      ...loadTests.map((n) => ({
        key: `l:${n.id}`,
        badge: MODELS[n.model].short,
        color: "var(--accent)",
        name: n.name,
        trail: `Load test · ${formatDuration(n.durationSecs)} · ${n.targets} ${n.targets === 1 ? "request" : "requests"}`,
        terms: "load test",
        open: () => void openLoadTest(n.id),
      })),
      ...TOOLS.map((t) => ({
        key: `t:${t.id}`,
        badge: t.icon(13),
        color: "var(--muted)",
        name: t.label,
        trail: "Tool",
        terms: "tool",
        open: () => openTool(t.id),
      })),
    ],
    [tree, servers, loadTests, workspaceName],
  );
  const results = useMemo(() => {
    const words = q.toLowerCase().split(/\s+/).filter(Boolean);
    return all.filter((item) => words.every((w) => `${item.terms} ${item.trail} ${item.name}`.toLowerCase().includes(w))).slice(0, 100);
  }, [all, q]);
  useEffect(() => setIndex(0), [q]);
  useEffect(() => {
    list.current?.querySelector(`[data-index="${index}"]`)?.scrollIntoView({ block: "nearest" });
  }, [index]);
  const choose = (item: PaletteItem) => {
    closeModal();
    item.open();
  };
  return (
    <Modal open onClose={closeModal} title="Go to" width={600} bodyClassName="p-0">
      <div className="flex items-center gap-2 border-b border-line px-4">
        <Search size={15} className="text-faint" />
        <input
          autoFocus
          value={q}
          onChange={(e) => setQ(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "ArrowDown") {
              e.preventDefault();
              setIndex((i) => Math.min(i + 1, results.length - 1));
            } else if (e.key === "ArrowUp") {
              e.preventDefault();
              setIndex((i) => Math.max(i - 1, 0));
            } else if (e.key === "Enter" && !e.nativeEvent.isComposing && e.keyCode !== 229 && results[index]) {
              choose(results[index]);
            }
          }}
          placeholder="Search requests, load tests, servers and tools…"
          className="h-11 flex-1 bg-transparent text-[14px] outline-none placeholder:text-faint"
        />
      </div>
      <div ref={list} className="max-h-[50vh] overflow-auto p-1.5">
        {results.length === 0 ? (
          <div className="p-6 text-center text-[12.5px] text-muted">Nothing found</div>
        ) : (
          results.map((item, i) => (
            <button
              key={item.key}
              data-index={i}
              onMouseEnter={() => setIndex(i)}
              onClick={() => choose(item)}
              className={cx("flex w-full items-center gap-3 rounded-md px-3 py-2 text-left", i === index ? "bg-hover" : "")}
            >
              <span className="flex w-10 shrink-0 font-mono text-[10.5px] font-bold" style={{ color: item.color }}>
                {item.badge}
              </span>
              <span className="min-w-0 flex-1 truncate text-[13px] text-fg">{item.name}</span>
              <span className="max-w-[40%] shrink-0 truncate text-[11.5px] text-faint">{item.trail}</span>
            </button>
          ))
        )}
      </div>
    </Modal>
  );
}

export function ShortcutsModal({ mod }: { mod: string }) {
  const rows: [string, string[]][] = [
    ["Send request, connect, start a server, load test or run", [mod, "Enter"]],
    ["Save request", [mod, "S"]],
    ["New request", [mod, "N"]],
    ["Close tab", [mod, "W"]],
    ["Go to request, load test, server or tool", [mod, "K"]],
    ["Focus URL", [mod, "L"]],
    ["Next / previous tab", ["Ctrl", "Tab"]],
    ["Environments", [mod, "E"]],
    ["Settings", [mod, ","]],
    ["Search in editor / response", [mod, "F"]],
    ["Zoom in / out", [mod, "+", "−"]],
    ["Reset zoom", [mod, "0"]],
  ];
  return (
    <Modal open onClose={closeModal} title="Keyboard shortcuts" width={460}>
      <div className="flex flex-col">
        {rows.map(([label, keys]) => (
          <div key={label} className="flex items-center justify-between border-b border-line py-2 text-[13px] last:border-0">
            <span className="text-fg">{label}</span>
            <span className="flex gap-1">
              {keys.map((k) => (
                <Kbd key={k}>{k}</Kbd>
              ))}
            </span>
          </div>
        ))}
      </div>
    </Modal>
  );
}

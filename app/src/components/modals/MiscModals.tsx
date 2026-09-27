import { useEffect, useMemo, useRef, useState } from "react";
import { Cookie, FileJson, Globe, ListChecks, Search, Terminal, Trash2 } from "lucide-react";
import type { CookieInfo } from "../../bindings/CookieInfo";
import type { CurlFlavor } from "../../bindings/CurlFlavor";
import type { FolderMeta } from "../../bindings/FolderMeta";
import type { ImportSummary } from "../../bindings/ImportSummary";
import type { TreeNode } from "../../bindings/TreeNode";
import type { WorkspaceMeta } from "../../bindings/WorkspaceMeta";
import { GRAPHQL_BADGE, methodColor, methodLabel } from "../../lib/http";
import { copyText, pickFile } from "../../lib/platform";
import { api, errorMessage } from "../../lib/rpc";
import { confirm } from "../../store/dialogs";
import { useLoadTests } from "../../store/loadtests";
import { useServers } from "../../store/servers";
import { isRequestTab, openDraft, openLoadTest, openRequest, openRunner, openServer, openTool, saveTabAs, useTabs } from "../../store/tabs";
import { formatDuration, MODELS } from "../loadtests/model";
import { SERVER_KINDS } from "../servers/kinds";
import { TOOLS } from "../tools/registry";
import { toast } from "../../store/toasts";
import { closeModal, toggleExpanded } from "../../store/ui";
import { refreshEnvironments, refreshTree, reloadWorkspace, useWorkspace } from "../../store/workspace";
import { CodeEditor } from "../CodeEditor";
import { KeyValueEditor } from "../KeyValueEditor";
import { AuthEditor } from "../request/AuthEditor";
import { ScriptsEditor, ScriptsTabLabel } from "../request/ScriptsEditor";
import { moveItem } from "../sidebar/Sidebar";
import { Button, cx, EmptyState, Field, Input, Kbd, Modal, Select, Spinner, Switch, Tabs } from "../ui";

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
            {tab === "url" && (
              <Button variant="primary" loading={busy} disabled={!url.trim()} onClick={() => run(async () => finish(await api.importUrl(url.trim(), parent)))}>
                Import
              </Button>
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
                    if (path) await finish(await api.importFile(path, parent));
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
                  if (file) void run(async () => finish(await api.importText(await file.text(), parent)));
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

// ---- Export (cURL) ---------------------------------------------------------------

export function ExportModal({ tabId }: { tabId: string }) {
  const tab = useTabs((s) => {
    const t = s.tabs.find((x) => x.id === tabId);
    return isRequestTab(t) ? t : undefined;
  });
  const [flavor, setFlavor] = useState<CurlFlavor>(navigator.userAgent.includes("Windows") ? "cmd" : "bash");
  const [resolveVars, setResolveVars] = useState(true);
  const [text, setText] = useState("");
  const [error, setError] = useState<string | null>(null);
  const draft = tab?.draft;
  const path = tab?.path ?? null;
  // Depends on the request, not the whole tab (which changes with every streamed message);
  // `alive` drops a slower, older result so the text always matches the chosen options.
  useEffect(() => {
    if (!draft) return;
    let alive = true;
    api
      .exportCurl(draft, path, flavor, resolveVars)
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
  }, [draft, path, flavor, resolveVars]);
  if (!tab) return null;
  return (
    <Modal
      open
      onClose={closeModal}
      title="Copy as cURL"
      width={760}
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
      <div className="mb-3 flex items-center gap-4">
        <Select value={flavor} onChange={(e) => setFlavor(e.target.value as CurlFlavor)} className="w-56">
          <option value="bash">bash / zsh (macOS, Linux)</option>
          <option value="cmd">Windows Command Prompt</option>
          <option value="powerShell">Windows PowerShell</option>
        </Select>
        <Switch checked={resolveVars} onChange={setResolveVars} label="Substitute variables" />
      </div>
      {error ? (
        <div className="rounded-md border border-danger/30 bg-danger/10 p-3 text-[12.5px] text-danger">{error}</div>
      ) : (
        <div className="h-72 overflow-hidden rounded-md border border-line bg-panel-2">
          <CodeEditor value={text} readOnly lineNumbers={false} />
        </div>
      )}
      {resolveVars && <p className="mt-2 text-[11.5px] text-faint">Contains resolved values — including secrets — so be careful where you paste it.</p>}
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
  const [tab, setTab] = useState<"auth" | "headers" | "scripts" | "docs">("auth");
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
        ]}
        value={tab}
        onChange={setTab}
      />
      <div className="h-[380px] overflow-auto">
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

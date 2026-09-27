// Building mock APIs from what the workspace has: a folder of requests, an
// OpenAPI document, or a response the user just got.
import { useMemo, useState } from "react";
import { FileUp, Server as ServerIcon } from "lucide-react";
import { create } from "zustand";
import type { MockCreated } from "../../bindings/MockCreated";
import type { MockRoute } from "../../bindings/MockRoute";
import type { SendResult } from "../../bindings/SendResult";
import type { Server } from "../../bindings/Server";
import { methodColor } from "../../lib/http";
import { pickFile } from "../../lib/platform";
import { api, errorMessage } from "../../lib/rpc";
import { prompt } from "../../store/dialogs";
import { newServerDraft, refreshServers, useServers } from "../../store/servers";
import { isRequestTab, isServerTab, openServer, updateServerTab, useTabs } from "../../store/tabs";
import { toast } from "../../store/toasts";
import { useUi } from "../../store/ui";
import { Banner, Button, Field, Input, Modal, Segmented, Select } from "../ui";
import { methodText, routeFromResponse } from "./MockRoutes";

function created(result: MockCreated, name: string) {
  const warnings = result.warnings.length;
  const detail = warnings
    ? `${warnings} part${warnings === 1 ? "" : "s"} of the document could not be used: ${result.warnings.slice(0, 2).join("; ")}${warnings > 2 ? "; …" : ""}`
    : undefined;
  toast(warnings ? "info" : "success", `Created "${name}" with ${result.routes} route${result.routes === 1 ? "" : "s"}`, detail);
}

async function openCreated(id: string) {
  await refreshServers();
  await openServer(id);
  useUi.setState({ sidebarTab: "servers" });
}

/** "Mock this folder…" (folder "" = the whole collection). */
export async function mockFolder(folder: string, folderName: string) {
  const name = await prompt({
    title: folder ? `Mock "${folderName}"` : "Mock the collection",
    message: "A mock API with one route per HTTP request, answering 200 with {} until you fill in the answers.",
    value: `${folderName} mock`,
    confirmLabel: "Create mock",
  });
  if (!name) return;
  try {
    const result = await api.mockFromFolder(folder, name);
    created(result, name);
    await openCreated(result.id);
  } catch (e) {
    toast("error", "Could not create the mock", errorMessage(e));
  }
}

// ---- mock from OpenAPI --------------------------------------------------------------------

const useOpenApiDialog = create<{ open: boolean }>(() => ({ open: false }));

/** "Mock from OpenAPI…" in the servers list. */
export const openMockFromOpenApi = () => useOpenApiDialog.setState({ open: true });

type Source = "file" | "url" | "paste";

const fileStem = (path: string) => (path.split(/[\\/]/).pop() ?? "").replace(/\.(ya?ml|json)$/i, "");

/** Rendered once by the servers list. */
export function MockDialogs() {
  const open = useOpenApiDialog((s) => s.open);
  return open ? <MockFromOpenApiModal onClose={() => useOpenApiDialog.setState({ open: false })} /> : null;
}

function MockFromOpenApiModal({ onClose }: { onClose: () => void }) {
  const [source, setSource] = useState<Source>("file");
  const [path, setPath] = useState("");
  const [url, setUrl] = useState("");
  const [text, setText] = useState("");
  const [name, setName] = useState("");
  const [nameTouched, setNameTouched] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const ready = (source === "file" ? path : source === "url" ? url : text).trim() !== "";
  const choose = async () => {
    const picked = await pickFile("OpenAPI or Swagger document", ["json", "yaml", "yml"]);
    if (!picked) return;
    setPath(picked);
    if (!nameTouched && fileStem(picked)) setName(`${fileStem(picked)} mock`);
  };

  const submit = async () => {
    if (!ready || busy) return;
    const finalName = name.trim() || "API mock";
    setBusy(true);
    setError(null);
    try {
      const doc = source === "file" ? { path: path.trim() } : source === "url" ? { url: url.trim() } : { text };
      const result = await api.mockFromOpenApi(doc, finalName);
      created(result, finalName);
      onClose();
      await openCreated(result.id);
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal
      open
      onClose={onClose}
      title="Mock from OpenAPI"
      description="One route per operation, answering with its first success response: the document's example, or one made from the schema."
      width={560}
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" loading={busy} disabled={!ready} onClick={() => void submit()} data-testid="mock-openapi-create">
            Create mock
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-4">
        <div>
          <Segmented
            items={[
              { id: "file", label: "File" },
              { id: "url", label: "URL" },
              { id: "paste", label: "Paste" },
            ]}
            value={source}
            onChange={(s) => {
              setSource(s);
              setError(null);
            }}
          />
        </div>
        {source === "file" && (
          <Field label="Document" hint="OpenAPI 3.x or Swagger 2.0, JSON or YAML (up to 50 MB).">
            <div className="flex gap-2">
              <Input readOnly value={path} placeholder="No file chosen" className="min-w-0 flex-1 font-mono" onClick={() => void choose()} />
              <Button icon={<FileUp size={14} />} onClick={() => void choose()}>
                Choose…
              </Button>
            </div>
          </Field>
        )}
        {source === "url" && (
          <Field label="Document URL" hint="Downloaded with your proxy and certificate settings.">
            <Input
              autoFocus
              value={url}
              placeholder="https://petstore3.swagger.io/api/v3/openapi.json"
              className="font-mono"
              onChange={(e) => setUrl(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") void submit();
              }}
            />
          </Field>
        )}
        {source === "paste" && (
          <Field label="Document">
            <textarea
              autoFocus
              value={text}
              spellCheck={false}
              placeholder={"openapi: 3.0.0\ninfo: {title: My API, version: \"1\"}\npaths: …"}
              onChange={(e) => setText(e.target.value)}
              className="h-48 w-full resize-y rounded-lg border border-line bg-input p-2.5 font-mono text-[12px] text-fg outline-none placeholder:text-faint focus:border-accent focus:ring-2 focus:ring-accent-soft"
            />
          </Field>
        )}
        <Field label="Server name">
          <Input
            value={name}
            placeholder="API mock"
            onChange={(e) => {
              setName(e.target.value);
              setNameTouched(true);
            }}
            onKeyDown={(e) => {
              if (e.key === "Enter") void submit();
            }}
          />
        </Field>
        {error && <div className="-mx-3 -mt-2">{<Banner tone="danger">{error}</Banner>}</div>}
      </div>
    </Modal>
  );
}

// ---- mock this response -------------------------------------------------------------------

const NEW = "\u0000new";

/** "Mock" button in the response header: adds a route answering like this response. */
export function MockResponseButton({ tabId, result }: { tabId: string; result: SendResult }) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <Button size="sm" variant="ghost" icon={<ServerIcon size={13} />} onClick={() => setOpen(true)} title="Mock this response: add a route to a mock API that answers like this" data-testid="mock-response">
        Mock
      </Button>
      {open && <MockResponseModal tabId={tabId} result={result} onClose={() => setOpen(false)} />}
    </>
  );
}

function MockResponseModal({ tabId, result, onClose }: { tabId: string; result: SendResult; onClose: () => void }) {
  const tab = useTabs((s) => s.tabs.find((t) => t.id === tabId));
  const saved = useServers((s) => s.saved);
  const mocks = useMemo(() => saved.filter((n) => n.kind === "http" && !n.error), [saved]);
  const initial = useMemo(() => (isRequestTab(tab) ? routeFromResponse(tab.draft, result) : null), [tab, result]);
  const [path, setPath] = useState(initial?.route.path ?? "/");
  const [target, setTarget] = useState<string>(mocks[0]?.id ?? NEW);
  const [name, setName] = useState("Mock API");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  if (!initial) return null;
  const route: MockRoute = { ...initial.route, path: path.trim() || "/" };

  const submit = async () => {
    setBusy(true);
    setError(null);
    try {
      if (target === NEW) {
        const serverName = name.trim() || "Mock API";
        const id = await api.createServer({ ...newServerDraft("http", serverName), http: { routes: [route] } });
        toast("success", `Created "${serverName}"`, `${methodText(route.method)} ${route.path} → ${route.status}`);
        onClose();
        await refreshServers();
        await openServer(id);
      } else {
        const id = await api.mockAddRoute(target, route);
        // Open tabs of that server get the route too (keeping their unsaved edits).
        const add = (s: Server): Server => ({ ...s, http: { ...(s.http ?? { routes: [] }), routes: [...(s.http?.routes ?? []), route] } });
        const open = useTabs.getState().tabs.filter((t) => isServerTab(t) && t.serverId === id);
        for (const t of open) updateServerTab(t.id, (st) => ({ saved: add(st.saved), draft: add(st.draft), section: `route:${(st.draft.http?.routes.length ?? 0)}` }));
        const serverName = mocks.find((m) => m.id === target)?.name ?? id;
        toast("success", `Route added to "${serverName}"`, `${methodText(route.method)} ${route.path} → ${route.status}`);
        onClose();
        await openServer(id);
      }
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal
      open
      onClose={onClose}
      title="Mock this response"
      description="A mock API route that answers this request with this status, headers and body."
      width={520}
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" loading={busy} onClick={() => void submit()} data-testid="mock-response-add">
            {target === NEW ? "Create mock" : "Add route"}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-4">
        <Field label="Route" hint="Use :name for a path segment that changes (e.g. /users/:id).">
          <div className="flex items-center gap-2">
            <span className="w-14 shrink-0 text-right font-mono text-[11.5px] font-bold" style={{ color: methodColor(route.method) }}>
              {methodText(route.method)}
            </span>
            <Input value={path} className="min-w-0 flex-1 font-mono" aria-label="Route path" onChange={(e) => setPath(e.target.value)} />
            <span className="shrink-0 font-mono text-[12px] text-muted">→ {route.status}</span>
          </div>
        </Field>
        <Field label="Add to">
          <Select value={target} onChange={(e) => setTarget(e.target.value)} aria-label="Mock server">
            {mocks.map((m) => (
              <option key={m.id} value={m.id}>
                {m.name} (:{m.port})
              </option>
            ))}
            <option value={NEW}>New mock API…</option>
          </Select>
        </Field>
        {target === NEW && (
          <Field label="Name of the new mock API">
            <Input autoFocus value={name} onChange={(e) => setName(e.target.value)} />
          </Field>
        )}
        <p className="text-[12px] text-muted">
          {route.headers?.length ?? 0} header{route.headers?.length === 1 ? "" : "s"} and {route.body ? `a ${new Blob([route.body]).size.toLocaleString()} byte body` : "no body"}. Connection headers,
          Content-Length and Content-Encoding are left out (the mock sets its own).
        </p>
        {initial.notes.map((n) => (
          <div key={n} className="-mx-3 -mt-2">
            <Banner tone="info">{n}</Banner>
          </div>
        ))}
        {error && (
          <div className="-mx-3 -mt-2">
            <Banner tone="danger">{error}</Banner>
          </div>
        )}
      </div>
    </Modal>
  );
}

import { useEffect, useMemo, useState } from "react";
import { Copy, Globe, Globe2, Layers, Plus, Trash2 } from "lucide-react";
import type { EnvironmentEntry } from "../../bindings/EnvironmentEntry";
import type { Variable } from "../../bindings/Variable";
import { api, errorMessage } from "../../lib/rpc";
import { confirm } from "../../store/dialogs";
import { toast } from "../../store/toasts";
import { closeModal } from "../../store/ui";
import { refreshEnvironments, reloadWorkspace, useWorkspace } from "../../store/workspace";
import { Button, cx, Input, Modal } from "../ui";
import { LocalValues } from "./LocalValues";
import { VariablesEditor } from "./VariablesEditor";

const WORKSPACE = "__workspace__";
/** Globals: only ever set by scripts, kept on this computer. */
const GLOBALS = "__globals__";

export function EnvironmentsModal({ focus }: { focus?: string }) {
  const info = useWorkspace((s) => s.info)!;
  const [selected, setSelected] = useState<string>(focus ?? info.environments[0]?.id ?? WORKSPACE);
  const [drafts, setDrafts] = useState<Record<string, { name: string; variables: Variable[] }>>({});
  const [saving, setSaving] = useState(false);

  const envs = info.environments;
  const current = useMemo(() => {
    if (drafts[selected]) return drafts[selected];
    if (selected === WORKSPACE) return { name: info.meta.name, variables: info.meta.variables ?? [] };
    const e = envs.find((x) => x.id === selected);
    return e ? { name: e.environment.name, variables: e.environment.variables } : null;
  }, [drafts, selected, envs, info.meta]);

  useEffect(() => {
    if (selected !== WORKSPACE && selected !== GLOBALS && !envs.some((e) => e.id === selected)) setSelected(envs[0]?.id ?? WORKSPACE);
  }, [envs, selected]);

  const dirty = Object.keys(drafts).length > 0;
  const edit = (patch: Partial<{ name: string; variables: Variable[] }>) => current && setDrafts({ ...drafts, [selected]: { ...current, ...patch } });

  const saveAll = async () => {
    setSaving(true);
    // Track what got saved: after a partial failure only the rest stays a draft (a renamed
    // environment has a new id, so retrying it under the old one would fail with "not found").
    const unsaved = { ...drafts };
    let renamed: string | null = null;
    try {
      for (const [id, d] of Object.entries(drafts)) {
        const variables = d.variables.filter((v) => v.key.trim());
        if (id === WORKSPACE) {
          await api.saveWorkspaceMeta({ ...info.meta, variables });
        } else {
          const newId = await api.saveEnvironment(id, { name: d.name.trim() || "Untitled", variables });
          if (newId !== id && selected === id) renamed = newId;
        }
        delete unsaved[id];
      }
      toast("success", "Saved");
    } catch (e) {
      toast("error", "Could not save", errorMessage(e));
    } finally {
      setDrafts(unsaved);
      await reloadWorkspace().catch(() => {});
      await refreshEnvironments().catch(() => {});
      // Only now: before the reload the new id isn't listed and the effect above would jump to the first environment.
      if (renamed) setSelected(renamed);
      setSaving(false);
    }
  };

  const create = async (from?: EnvironmentEntry) => {
    try {
      const id = await api.createEnvironment(
        from ? { name: `${from.environment.name} copy`, variables: from.environment.variables } : { name: "New environment", variables: [] },
      );
      await refreshEnvironments();
      setSelected(id);
    } catch (e) {
      toast("error", "Could not create environment", errorMessage(e));
    }
  };

  const remove = async (id: string) => {
    const env = envs.find((e) => e.id === id);
    if (!env) return;
    if (!(await confirm({ title: "Delete environment?", message: `“${env.environment.name}” will be moved to the trash.`, confirmLabel: "Delete", danger: true }))) return;
    try {
      await api.deleteEnvironment(id);
      const next = { ...drafts };
      delete next[id];
      setDrafts(next);
      await refreshEnvironments();
    } catch (e) {
      toast("error", "Could not delete", errorMessage(e));
    }
  };

  const close = async () => {
    if (dirty && !(await confirm({ title: "Discard changes?", message: "You have unsaved variable changes.", confirmLabel: "Discard", danger: true }))) return;
    closeModal();
  };

  return (
    <Modal
      open
      onClose={close}
      title="Environments & variables"
      description="Use {{name}} in URLs, headers, bodies and auth. The active environment overrides workspace variables."
      width={920}
      bodyClassName="p-0"
      focusFirstField={false}
      footer={
        <>
          <span className="mr-auto text-[11.5px] text-faint">Secret values stay on this computer and are never written to workspace files.</span>
          <Button variant="ghost" onClick={close}>
            Close
          </Button>
          <Button variant="primary" onClick={saveAll} loading={saving} disabled={!dirty}>
            Save changes
          </Button>
        </>
      }
    >
      <div className="flex h-[480px]">
        <div className="flex w-56 shrink-0 flex-col p-2">
          <button
            onClick={() => setSelected(WORKSPACE)}
            className={cx("flex h-8 items-center gap-2 rounded-lg px-2 text-left text-[12.5px]", selected === WORKSPACE ? "bg-hover text-fg" : "text-muted hover:text-fg")}
          >
            <Layers size={14} />
            <span className="flex-1 truncate">Workspace variables</span>
            {drafts[WORKSPACE] && <span className="h-1.5 w-1.5 rounded-full bg-accent" />}
          </button>
          <button
            onClick={() => setSelected(GLOBALS)}
            className={cx("flex h-8 items-center gap-2 rounded-lg px-2 text-left text-[12.5px]", selected === GLOBALS ? "bg-hover text-fg" : "text-muted hover:text-fg")}
          >
            <Globe size={14} />
            <span className="flex-1 truncate">Globals</span>
          </button>
          <div className="mb-1 mt-3 flex items-center justify-between px-2">
            <span className="text-[11px] font-semibold uppercase tracking-wide text-faint">Environments</span>
            <button aria-label="New environment" onClick={() => create()} className="rounded p-0.5 text-muted hover:bg-hover hover:text-fg">
              <Plus size={14} />
            </button>
          </div>
          <div className="min-h-0 flex-1 overflow-auto">
            {envs.map((e) => (
              <button
                key={e.id}
                onClick={() => setSelected(e.id)}
                className={cx("group flex h-8 w-full items-center gap-2 rounded-lg px-2 text-left text-[12.5px]", selected === e.id ? "bg-hover text-fg" : "text-muted hover:text-fg")}
              >
                <Globe2 size={14} className={info.activeEnvironment === e.id ? "text-accent" : undefined} />
                <span className="flex-1 truncate">{drafts[e.id]?.name ?? e.environment.name}</span>
                {drafts[e.id] && <span className="h-1.5 w-1.5 rounded-full bg-accent" />}
              </button>
            ))}
            {envs.length === 0 && <div className="px-2 py-2 text-[12px] text-faint">No environments yet.</div>}
          </div>
        </div>
        <div className="flex min-w-0 flex-1 flex-col">
          {current ? (
            <>
              <div className="flex items-center gap-2 p-3">
                {selected === WORKSPACE ? (
                  <div className="flex-1 text-[13px] font-semibold text-fg">Workspace variables</div>
                ) : (
                  <>
                    <Input value={current.name} onChange={(e) => edit({ name: e.target.value })} className="max-w-xs font-medium" aria-label="Environment name" />
                    <div className="flex-1" />
                    <Button size="sm" variant="ghost" icon={<Copy size={13} />} onClick={() => create(envs.find((e) => e.id === selected))}>
                      Duplicate
                    </Button>
                    <Button size="sm" variant="ghost" icon={<Trash2 size={13} />} onClick={() => remove(selected)}>
                      Delete
                    </Button>
                  </>
                )}
              </div>
              <div className="min-h-0 flex-1 overflow-auto">
                {/* Keyed so revealed secrets don't carry over to another environment. */}
                <VariablesEditor key={selected} variables={current.variables} onChange={(variables) => edit({ variables })} />
                <LocalValues
                  key={`local-${selected}`}
                  scope={selected === WORKSPACE ? "workspace" : "environment"}
                  environmentId={selected === WORKSPACE ? undefined : selected}
                  secretKeys={current.variables.filter((v) => v.secret).map((v) => v.key.trim())}
                />
              </div>
            </>
          ) : selected === GLOBALS ? (
            <div className="min-h-0 flex-1 overflow-auto">
              <div className="p-3 text-[13px] font-semibold text-fg">Globals</div>
              <LocalValues scope="globals" />
            </div>
          ) : null}
        </div>
      </div>
    </Modal>
  );
}

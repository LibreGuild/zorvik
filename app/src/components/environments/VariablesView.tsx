// A variables tab: an environment (its name and variables), the workspace's variables,
// or globals, with the values scripts set under them.
import { Check, Copy, Globe, Globe2, Layers, Save, Trash2 } from "lucide-react";
import { modKey } from "../../lib/platform";
import { deleteEnvironment, newEnvironment, toggleActiveEnvironment } from "../../store/environments";
import { isDirty, saveTab, updateVariablesTab, type VariablesTab } from "../../store/tabs";
import { useWorkspace } from "../../store/workspace";
import { LocalValues } from "../modals/LocalValues";
import { VariablesEditor } from "../modals/VariablesEditor";
import { Banner, Button, cx, IconButton, Input } from "../ui";

const ABOUT: Record<VariablesTab["scope"], string> = {
  environment: "Use {{name}} in URLs, headers, bodies and auth. The active environment's values win over workspace variables.",
  workspace: "For every environment: use {{name}} in URLs, headers, bodies and auth. The active environment's values win over these.",
  globals: "Values scripts set with pm.globals.set. They apply to every workspace and are kept on this computer only.",
};

export function VariablesView({ tab }: { tab: VariablesTab }) {
  const env = useWorkspace((s) => (tab.scope === "environment" ? s.info?.environments.find((e) => e.id === tab.envId) : undefined));
  const active = useWorkspace((s) => tab.scope === "environment" && !!tab.envId && s.info?.activeEnvironment === tab.envId);
  const dirty = isDirty(tab);
  const edit = (patch: Partial<VariablesTab["draft"]>) => updateVariablesTab(tab.id, (t) => ({ draft: { ...t.draft, ...patch } }));
  const filled = tab.draft.variables.filter((v) => v.key.trim()).length;

  return (
    <div className="flex h-full min-h-0 flex-col bg-bg" data-testid="variables-view">
      {tab.orphaned && <Banner tone="warning">This environment's file was deleted. Save to write it again.</Banner>}
      <div className="flex shrink-0 flex-wrap items-center gap-2 px-3 pb-2 pt-2">
        <span className="flex h-9 items-center gap-2 rounded-xl bg-panel-2 px-3 text-[12px] font-bold text-accent">
          {tab.scope === "environment" ? <Globe2 size={15} /> : tab.scope === "workspace" ? <Layers size={15} /> : <Globe size={15} />}
          {tab.scope === "environment" ? "Environment" : tab.scope === "workspace" ? "Workspace" : "Globals"}
        </span>
        <div className="min-w-0 flex-1">
          {tab.scope === "environment" ? (
            <Input value={tab.draft.name} onChange={(e) => edit({ name: e.target.value })} className="h-8 max-w-sm text-[14px] font-semibold" aria-label="Environment name" />
          ) : (
            <div className="truncate text-[14px] font-semibold text-fg">{tab.draft.name}</div>
          )}
          <div className="flex min-w-0 items-center gap-1.5 pt-0.5 text-[12px] text-muted" data-testid="variables-status">
            {tab.scope === "environment" && <span className={cx("h-2 w-2 shrink-0 rounded-full", active ? "bg-accent" : "bg-faint")} />}
            <span className="truncate">
              {tab.scope === "environment" ? `${active ? "Active" : "Not active"} · ` : ""}
              {tab.scope === "globals" ? "Set by scripts" : `${filled} ${filled === 1 ? "variable" : "variables"}`}
            </span>
          </div>
        </div>
        {tab.scope === "environment" && env && (
          <>
            <Button
              variant={active ? "secondary" : "primary"}
              icon={active ? <Check size={14} /> : undefined}
              onClick={() => void toggleActiveEnvironment(env.id)}
              className="h-9 rounded-xl"
              title={active ? "The active environment. Click to use none." : "Use this environment's values in requests"}
              data-testid="environment-activate"
            >
              {active ? "Active" : "Set active"}
            </Button>
            <IconButton label="Duplicate" onClick={() => void newEnvironment(env)} size={36} className="rounded-xl">
              <Copy size={15} />
            </IconButton>
            <IconButton label="Delete" onClick={() => void deleteEnvironment(env)} size={36} className="rounded-xl">
              <Trash2 size={15} />
            </IconButton>
          </>
        )}
        {tab.scope !== "globals" && (
          <IconButton
            label={`${dirty ? "Save unsaved changes" : "Save"} (${modKey}+S)`}
            onClick={() => void saveTab(tab.id)}
            size={36}
            className={cx("rounded-xl", dirty && "text-accent")}
            data-testid="variables-save"
          >
            <Save size={16} />
          </IconButton>
        )}
      </div>
      <div className="min-h-0 flex-1 overflow-auto border-t border-line/60 pb-8 pt-3">
        <p className="px-4 pb-3 text-[12px] leading-relaxed text-muted">{ABOUT[tab.scope]}</p>
        {tab.scope === "globals" ? (
          <LocalValues scope="globals" />
        ) : (
          <>
            {/* Keyed so revealed secrets don't carry over when the tab shows another environment. */}
            <VariablesEditor key={`${tab.scope}:${tab.envId ?? ""}`} variables={tab.draft.variables} onChange={(variables) => edit({ variables })} />
            <LocalValues
              key={`local:${tab.scope}:${tab.envId ?? ""}`}
              scope={tab.scope}
              environmentId={tab.scope === "environment" ? tab.envId : undefined}
              secretKeys={tab.draft.variables.filter((v) => v.secret).map((v) => v.key.trim())}
            />
            <p className="px-4 pt-3 text-[11.5px] leading-snug text-faint">Secret values stay on this computer and are never written to workspace files.</p>
          </>
        )}
      </div>
    </div>
  );
}

import { useState } from "react";
import { ArrowRight, FolderOpen, FolderPlus, GraduationCap, Layers, X } from "lucide-react";
import { formatRelative } from "../../lib/format";
import { pickFolder } from "../../lib/platform";
import { api, errorMessage } from "../../lib/rpc";
import { isBootcampPath, openAcademy, useAcademy } from "../../store/academy";
import { restoreTabs } from "../../store/tabs";
import { toast } from "../../store/toasts";
import { createWorkspace, openOrInitWorkspace, useWorkspace } from "../../store/workspace";
import { Button, Field, Input, Modal } from "../ui";
import welcomeArt from "../../assets/welcome-art.webp";

/** The Training Bootcamp, first on the welcome screen. */
function BootcampCard() {
  const progress = useAcademy((s) => s.progress);
  const started = !!progress && (progress.xp > 0 || !!progress.lastLesson);
  return (
    <button
      onClick={() => void openAcademy()}
      className="group relative mb-3 flex w-full items-center gap-4 overflow-hidden rounded-xl border border-accent/40 p-4 text-left transition-[border-color,box-shadow] hover:border-accent hover:shadow-sm"
      style={{ background: "linear-gradient(120deg, color-mix(in srgb, var(--accent) 14%, var(--elev)), var(--elev) 70%)" }}
      data-testid="welcome-bootcamp"
    >
      <span className="flex h-11 w-11 shrink-0 items-center justify-center rounded-xl bg-accent text-accent-fg shadow-sm">
        <GraduationCap size={22} />
      </span>
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2 text-[14px] font-semibold text-fg">
          Training Bootcamp
          {progress && started && <span className="rounded-full bg-accent-soft px-1.5 text-[10.5px] font-semibold text-accent">Level {progress.level} · {progress.rank}</span>}
        </div>
        <div className="text-[12px] text-muted">
          {started && progress
            ? `${progress.completedLessons} of ${progress.totalLessons} lessons done. Pick up where you left off.`
            : "New to APIs and networks? Learn hands-on: short lessons, real labs, badges and a certificate."}
        </div>
      </div>
      <ArrowRight size={16} className="shrink-0 text-accent transition-transform group-hover:translate-x-0.5" />
    </button>
  );
}

export function Welcome() {
  const recent = useWorkspace((s) => s.recent).filter((r) => !isBootcampPath(r.path));
  const version = useWorkspace((s) => s.appInfo?.version);
  const [creating, setCreating] = useState(false);

  const open = async (path: string) => {
    try {
      if (await openOrInitWorkspace(path)) await restoreTabs(path);
    } catch (e) {
      toast("error", "Could not open workspace", errorMessage(e));
    }
  };

  return (
    // Scrolls from the top when the window is short (centering inside the scroller would cut the art off).
    <div className="h-full overflow-auto bg-bg">
      <div className="mx-auto flex min-h-full w-full max-w-[584px] flex-col justify-center px-8 py-8">
        <img
          src={welcomeArt}
          alt=""
          draggable={false}
          className="pointer-events-none mx-auto -mb-2 h-44 w-auto select-none opacity-80 dark:opacity-70"
          data-testid="welcome-art"
        />
        <div className="mb-8 flex items-center gap-4">
          <img src="/icon.png" alt="" className="h-14 w-14" />
          <div>
            <h1 className="text-[22px] font-semibold tracking-tight text-fg">Zorvik</h1>
            <p className="text-[13px] text-muted">Build, test, mock and load test APIs. Every protocol, one workbench.</p>
          </div>
        </div>
        <BootcampCard />
        <div className="grid grid-cols-2 gap-3">
          <button
            onClick={() => setCreating(true)}
            className="flex flex-col items-start gap-2 rounded-xl border border-line bg-elev p-4 text-left transition-colors hover:border-accent"
          >
            <FolderPlus size={20} className="text-accent" />
            <div className="text-[13.5px] font-semibold text-fg">New workspace</div>
            <div className="text-[12px] text-muted">Create a folder for your requests. Commit it to Git to share.</div>
          </button>
          <button
            onClick={async () => {
              const path = await pickFolder("Open workspace folder");
              if (path) await open(path);
            }}
            className="flex flex-col items-start gap-2 rounded-xl border border-line bg-elev p-4 text-left transition-colors hover:border-accent"
          >
            <FolderOpen size={20} className="text-accent" />
            <div className="text-[13.5px] font-semibold text-fg">Open workspace</div>
            <div className="text-[12px] text-muted">Open an existing folder that contains zorvik.yaml.</div>
          </button>
        </div>
        {recent.length > 0 && (
          <div className="mt-8">
            <div className="mb-2 text-[11px] font-semibold uppercase tracking-wide text-faint">Recent</div>
            <div className="overflow-hidden rounded-xl border border-line bg-elev">
              {recent.map((r) => (
                <div key={r.path} className="group flex items-center gap-3 border-b border-line px-3 py-2.5 last:border-0 hover:bg-panel-2">
                  <Layers size={15} className="shrink-0 text-muted" />
                  <button className="min-w-0 flex-1 text-left" onClick={() => open(r.path)}>
                    <div className="truncate text-[13px] font-medium text-fg">{r.name}</div>
                    <div className="truncate text-[11.5px] text-faint">{r.path}</div>
                  </button>
                  <span className="shrink-0 text-[11px] text-faint">{formatRelative(r.openedAt)}</span>
                  <button
                    aria-label="Remove from recent"
                    onClick={async () => {
                      await api.removeRecent(r.path);
                      useWorkspace.setState({ recent: await api.recentWorkspaces() });
                    }}
                    className="rounded p-1 text-faint opacity-0 hover:bg-hover hover:text-fg focus-visible:opacity-100 group-hover:opacity-100"
                  >
                    <X size={13} />
                  </button>
                </div>
              ))}
            </div>
          </div>
        )}
        <div className="mt-8 text-center text-[11px] text-faint">Version {version}</div>
      </div>
      <CreateWorkspaceModal open={creating} onClose={() => setCreating(false)} />
    </div>
  );
}

function CreateWorkspaceModal({ open, onClose }: { open: boolean; onClose: () => void }) {
  const [name, setName] = useState("My Workspace");
  const [path, setPath] = useState("");
  const [busy, setBusy] = useState(false);
  const submit = async () => {
    if (!path.trim() || busy) return; // Enter while creating would try to create it twice
    setBusy(true);
    try {
      await createWorkspace(path.trim(), name.trim() || "My Workspace");
      await restoreTabs(path.trim());
      onClose();
    } catch (e) {
      toast("error", "Could not create workspace", errorMessage(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Modal
      open={open}
      onClose={onClose}
      title="New workspace"
      description="A workspace is a folder of plain YAML files: requests, folders and environments."
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" onClick={submit} loading={busy} disabled={!path.trim()}>
            Create workspace
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-4">
        <Field label="Name">
          <Input value={name} onChange={(e) => setName(e.target.value)} autoFocus />
        </Field>
        <Field label="Folder" hint="Choose an empty folder, or an existing project folder (e.g. a Git repo).">
          <div className="flex gap-2">
            <Input value={path} onChange={(e) => setPath(e.target.value)} placeholder="/path/to/folder" onKeyDown={(e) => e.key === "Enter" && submit()} />
            <Button
              onClick={async () => {
                const p = await pickFolder("Choose workspace folder");
                if (p) setPath(p);
              }}
            >
              Browse…
            </Button>
          </div>
        </Field>
      </div>
    </Modal>
  );
}

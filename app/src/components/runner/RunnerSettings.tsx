// The settings side of a runner tab: which requests run (and in which order),
// iterations, delay, data file with a preview, and options.
import { memo, useMemo, useState } from "react";
import { FileSpreadsheet, GripVertical, TriangleAlert, X } from "lucide-react";
import { methodColor, methodLabel } from "../../lib/http";
import type { RequestEntry } from "../loadtests/model";
import { type DataState, MAX_DELAY_MS, MAX_ITERATIONS, moveInOrder, orderRequests, pickDataFile, setDataFile, settingsOf, updateSettings, useRunner } from "../../store/runner";
import type { RunnerTab } from "../../store/tabs";
import { openModal } from "../../store/ui";
import { useWorkspace } from "../../store/workspace";
import { NumberInput } from "../loadtests/parts";
import { EditorSection } from "../servers/ServerView";
import { Button, Checkbox, cx, IconButton, Switch } from "../ui";

export const RunnerSettingsPane = memo(function RunnerSettingsPane({ tab, entries, running }: { tab: RunnerTab; entries: RequestEntry[]; running: boolean }) {
  const settings = useRunner((s) => settingsOf(tab.id, s));
  const data = useRunner((s) => s.data[tab.id]);
  const environment = useWorkspace((s) => s.info?.environments.find((e) => e.id === s.info?.activeEnvironment)?.environment.name ?? null);
  const ordered = useMemo(() => orderRequests(entries, settings.order), [entries, settings.order]);
  const excluded = useMemo(() => new Set(settings.excluded), [settings.excluded]);
  const selected = ordered.filter((e) => !excluded.has(e.path)).length;
  const rows = data?.status === "ready" ? data.preview.count : null;

  return (
    <div className="flex flex-col pb-8" data-testid="runner-settings">
      {running && <p className="mx-4 mt-3 rounded-lg bg-panel-2 px-3 py-2 text-[12px] text-muted">A run is in progress. Changes apply to the next run.</p>}
      <EditorSection
        title="Requests"
        right={
          entries.length ? (
            <div className="flex items-center gap-2 text-[11px]">
              <span className="text-faint">{`${selected} of ${entries.length}`}</span>
              <button className="text-accent hover:underline" onClick={() => updateSettings(tab.id, (s) => ({ ...s, excluded: [] }))}>
                All
              </button>
              <button className="text-accent hover:underline" onClick={() => updateSettings(tab.id, (s) => ({ ...s, excluded: entries.map((e) => e.path) }))}>
                None
              </button>
            </div>
          ) : undefined
        }
      >
        {entries.length ? (
          <RequestList tabId={tab.id} ordered={ordered} excluded={excluded} />
        ) : (
          <p className="rounded-xl border border-dashed border-line-strong px-4 py-5 text-center text-[12.5px] text-muted">
            No HTTP requests here. WebSocket, gRPC and other kinds don't run in the runner.
          </p>
        )}
      </EditorSection>

      <EditorSection title="Iterations">
        <div className="grid grid-cols-[repeat(auto-fill,minmax(170px,1fr))] gap-3">
          <Labeled label="Iterations" hint={rows != null ? `Empty: one per data row (${rows})` : "How often the requests run"}>
            <NumberInput
              aria-label="Iterations"
              optional
              placeholder={String(rows ?? 1)}
              value={settings.iterations ?? undefined}
              min={1}
              max={MAX_ITERATIONS}
              onChange={(v) => updateSettings(tab.id, (s) => ({ ...s, iterations: v === undefined ? null : Math.min(MAX_ITERATIONS, v) }))}
            />
          </Labeled>
          <Labeled label="Delay (ms)" hint="Pause between two requests">
            <NumberInput
              aria-label="Delay in milliseconds"
              value={settings.delayMs}
              min={0}
              max={MAX_DELAY_MS}
              onChange={(v) => updateSettings(tab.id, (s) => ({ ...s, delayMs: Math.min(MAX_DELAY_MS, v ?? 0) }))}
            />
          </Labeled>
        </div>
      </EditorSection>

      <EditorSection title="Data file">
        <DataFile tabId={tab.id} file={settings.dataFile} data={data} />
      </EditorSection>

      <EditorSection title="Options">
        <Switch checked={settings.stopOnFailure} onChange={(stopOnFailure) => updateSettings(tab.id, (s) => ({ ...s, stopOnFailure }))} label="Stop on the first failure" />
        <p className="text-[11.5px] leading-snug text-faint">
          A request fails when it can't be sent, a script fails or a test fails. Without tests, an HTTP status of 400 or more fails it.
        </p>
        <div className="flex items-center gap-2 pt-1 text-[12.5px]">
          <span className="text-muted">Environment</span>
          <span className={cx("font-medium", environment ? "text-fg" : "text-faint")} data-testid="runner-environment">
            {environment ?? "None"}
          </span>
          <button className="text-[12px] text-accent hover:underline" onClick={() => openModal({ type: "environments" })}>
            Environments…
          </button>
        </div>
        <p className="text-[11.5px] leading-snug text-faint">The active environment (title bar) is used. Values scripts set are kept like in single sends.</p>
      </EditorSection>
    </div>
  );
});

/** Checkbox list in run order; drag a row (or Alt+↑/↓ on its handle) to reorder. */
function RequestList({ tabId, ordered, excluded }: { tabId: string; ordered: RequestEntry[]; excluded: Set<string> }) {
  const [dragging, setDragging] = useState<number | null>(null);
  const [over, setOver] = useState<number | null>(null);
  const paths = ordered.map((e) => e.path);
  const move = (from: number, to: number) => updateSettings(tabId, (s) => ({ ...s, order: moveInOrder(paths, from, to) }));
  const toggle = (path: string, on: boolean) =>
    updateSettings(tabId, (s) => ({ ...s, excluded: on ? s.excluded.filter((p) => p !== path) : [...s.excluded, path] }));
  return (
    <div className="overflow-hidden rounded-xl border border-line" data-testid="runner-requests">
      {ordered.map((e, i) => {
        const on = !excluded.has(e.path);
        return (
          <div
            key={e.path}
            draggable
            onDragStart={(ev) => {
              setDragging(i);
              ev.dataTransfer.effectAllowed = "move";
              ev.dataTransfer.setData("text/plain", e.path);
            }}
            onDragOver={(ev) => {
              if (dragging === null) return;
              ev.preventDefault();
              setOver(i);
            }}
            onDrop={(ev) => {
              ev.preventDefault();
              if (dragging !== null) move(dragging, i);
              setDragging(null);
              setOver(null);
            }}
            onDragEnd={() => {
              setDragging(null);
              setOver(null);
            }}
            className={cx(
              "flex min-h-[38px] items-center gap-2 border-b border-line/60 px-2 py-1 last:border-b-0",
              dragging === i && "opacity-40",
              over === i && dragging !== null && dragging !== i && (dragging < i ? "shadow-[inset_0_-2px_0_var(--accent)]" : "shadow-[inset_0_2px_0_var(--accent)]"),
            )}
            data-testid="runner-request"
          >
            <button
              aria-label={`Move ${e.name} (Alt+Up / Alt+Down)`}
              title="Drag to reorder (Alt+↑/↓)"
              className="flex h-6 w-4 shrink-0 cursor-grab items-center justify-center text-faint hover:text-fg"
              onKeyDown={(ev) => {
                if (!ev.altKey || (ev.key !== "ArrowUp" && ev.key !== "ArrowDown")) return;
                ev.preventDefault();
                move(i, ev.key === "ArrowUp" ? i - 1 : i + 1);
              }}
            >
              <GripVertical size={13} />
            </button>
            <Checkbox checked={on} onChange={(v) => toggle(e.path, v)} label={`Run ${e.name}`} />
            <span className="w-[34px] shrink-0 text-right font-mono text-[10px] font-bold" style={{ color: methodColor(e.method, e.kind) }}>
              {methodLabel(e.method, e.kind)}
            </span>
            <div className={cx("min-w-0 flex-1", !on && "opacity-50")} title={e.path}>
              <div className="truncate text-[12.5px] text-fg">{e.name}</div>
              {e.trail && <div className="truncate text-[11px] text-faint">{e.trail}</div>}
            </div>
            <span className="w-6 shrink-0 text-right text-[11px] tabular-nums text-faint">{i + 1}</span>
          </div>
        );
      })}
    </div>
  );
}

function DataFile({ tabId, file, data }: { tabId: string; file: string | null; data: DataState | undefined }) {
  if (!file) {
    return (
      <div className="flex flex-col gap-2">
        <p className="text-[12px] leading-relaxed text-muted">
          A CSV file with a header row, or a JSON array of objects. Each row is one iteration; its columns are variables (<code className="font-mono">{"{{name}}"}</code>,{" "}
          <code className="font-mono">pm.iterationData</code>).
        </p>
        <div>
          <Button size="sm" icon={<FileSpreadsheet size={13} />} onClick={() => void pickDataFile(tabId)}>
            Choose file…
          </Button>
        </div>
      </div>
    );
  }
  return (
    <div className="flex flex-col gap-2" data-testid="runner-data">
      <div className="flex items-center gap-2 rounded-lg border border-line bg-panel-2/60 px-2.5 py-1.5">
        <FileSpreadsheet size={14} className="shrink-0 text-muted" />
        <span className="min-w-0 flex-1 truncate font-mono text-[12px] text-fg" title={file}>
          {file}
        </span>
        {data?.status === "ready" && (
          <span className="shrink-0 text-[11px] text-faint">
            {data.preview.format.toUpperCase()} · {data.preview.count} {data.preview.count === 1 ? "row" : "rows"}
          </span>
        )}
        <button className="shrink-0 text-[12px] text-accent hover:underline" onClick={() => void pickDataFile(tabId)}>
          Change
        </button>
        <IconButton label="Remove the data file" onClick={() => void setDataFile(tabId, null)} size={22}>
          <X size={13} />
        </IconButton>
      </div>
      {data?.status === "loading" && <p className="text-[12px] text-muted">Reading…</p>}
      {data?.status === "error" && (
        <p className="flex items-start gap-1.5 text-[12px] text-danger">
          <TriangleAlert size={13} className="mt-0.5 shrink-0" />
          {data.message}
        </p>
      )}
      {data?.status === "ready" && <Preview columns={data.preview.columns} rows={data.preview.rows} count={data.preview.count} />}
    </div>
  );
}

function Preview({ columns, rows, count }: { columns: string[]; rows: string[][]; count: number }) {
  return (
    <div className="overflow-auto rounded-xl border border-line" data-testid="runner-data-preview">
      <table className="w-full border-collapse text-left text-[11.5px]">
        <thead>
          <tr className="bg-panel-2/60">
            <th className="w-8 px-2 py-1 font-medium text-faint">#</th>
            {columns.map((c) => (
              <th key={c} className="whitespace-nowrap px-2 py-1 font-mono font-medium text-muted">
                {c}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((row, i) => (
            <tr key={i} className="border-t border-line/60">
              <td className="px-2 py-1 tabular-nums text-faint">{i + 1}</td>
              {row.map((v, j) => (
                <td key={j} className="max-w-[220px] truncate px-2 py-1 font-mono text-fg" title={v}>
                  {v}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
      {count > rows.length && <div className="border-t border-line/60 px-2 py-1 text-[11px] text-faint">{`… ${count - rows.length} more`}</div>}
    </div>
  );
}

function Labeled({ label, hint, children }: { label: string; hint?: string; children: React.ReactNode }) {
  return (
    <label className="flex min-w-0 flex-col gap-1">
      <span className="text-[11.5px] font-medium text-muted">{label}</span>
      {children}
      {hint && <span className="text-[11px] leading-snug text-faint">{hint}</span>}
    </label>
  );
}

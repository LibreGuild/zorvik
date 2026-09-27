// A load test's data file: CSV or JSON rows, one per virtual user (or per
// request in the request-rate model). Same file rules and preview as the runner.
import { useEffect, useState } from "react";
import { FileSpreadsheet, TriangleAlert, X } from "lucide-react";
import type { DataPreview } from "../../bindings/DataPreview";
import type { LoadModel } from "../../bindings/LoadModel";
import { pickFile } from "../../lib/platform";
import { api, errorMessage } from "../../lib/rpc";
import { workspaceRelative } from "../../store/grpc";
import { useWorkspace } from "../../store/workspace";
import { Button, IconButton } from "../ui";

type Preview = { status: "loading" } | { status: "ready"; preview: DataPreview } | { status: "error"; message: string };

export function DataFileField({ file, model, onChange }: { file: string | undefined; model: LoadModel; onChange: (file: string | undefined) => void }) {
  const [preview, setPreview] = useState<Preview | null>(null);
  const path = file?.trim() || null;

  useEffect(() => {
    if (!path) {
      setPreview(null);
      return;
    }
    let alive = true;
    setPreview({ status: "loading" });
    api
      .previewRunData(path)
      .then((p) => alive && setPreview({ status: "ready", preview: p }))
      .catch((e) => alive && setPreview({ status: "error", message: errorMessage(e) }));
    return () => {
      alive = false;
    };
  }, [path]);

  const choose = async () => {
    const picked = await pickFile("Choose a data file (CSV or JSON)", ["csv", "json"]);
    if (picked) onChange(workspaceRelative(picked, useWorkspace.getState().info?.path));
  };

  const perUser =
    model === "virtualUsers"
      ? "Each virtual user takes the next row and keeps it (user 1 row 1, user 2 row 2 …; with more users than rows, they start over)."
      : "Each request takes the next row, starting over at the end.";

  if (!path) {
    return (
      <div className="flex flex-col gap-2" data-testid="load-data">
        <p className="text-[12px] leading-relaxed text-muted">
          Optional: a CSV file with a header row, or a JSON array of objects, so users don't all send the same values. Columns are variables (
          <code className="font-mono">{"{{name}}"}</code>) that override environment variables. {perUser}
        </p>
        <div>
          <Button size="sm" icon={<FileSpreadsheet size={13} />} onClick={() => void choose()}>
            Choose file…
          </Button>
        </div>
      </div>
    );
  }
  return (
    <div className="flex flex-col gap-2" data-testid="load-data">
      <div className="flex items-center gap-2 rounded-lg border border-line bg-panel-2/60 px-2.5 py-1.5">
        <FileSpreadsheet size={14} className="shrink-0 text-muted" />
        <span className="min-w-0 flex-1 truncate font-mono text-[12px] text-fg" title={path}>
          {path}
        </span>
        {preview?.status === "ready" && (
          <span className="shrink-0 text-[11px] text-faint">
            {preview.preview.format.toUpperCase()} · {preview.preview.count} {preview.preview.count === 1 ? "row" : "rows"}
          </span>
        )}
        <button className="shrink-0 text-[12px] text-accent hover:underline" onClick={() => void choose()}>
          Change
        </button>
        <IconButton label="Remove the data file" onClick={() => onChange(undefined)} size={22}>
          <X size={13} />
        </IconButton>
      </div>
      {preview?.status === "loading" && <p className="text-[12px] text-muted">Reading…</p>}
      {preview?.status === "error" && (
        <p className="flex items-start gap-1.5 text-[12px] text-danger">
          <TriangleAlert size={13} className="mt-0.5 shrink-0" />
          {preview.message}
        </p>
      )}
      {preview?.status === "ready" && <PreviewTable preview={preview.preview} />}
      <p className="text-[11.5px] leading-snug text-faint">{perUser}</p>
    </div>
  );
}

function PreviewTable({ preview }: { preview: DataPreview }) {
  const { columns, rows, count } = preview;
  return (
    <div className="overflow-auto rounded-xl border border-line" data-testid="load-data-preview">
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

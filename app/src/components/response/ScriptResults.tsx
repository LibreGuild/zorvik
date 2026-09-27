// Response tabs for scripts: test results (pm.test) and console output.
import { CircleCheck, CircleMinus, CircleX } from "lucide-react";
import type { ConsoleLevel } from "../../bindings/ConsoleLevel";
import type { ScriptFailure } from "../../bindings/ScriptFailure";
import type { ScriptReport } from "../../bindings/ScriptReport";
import { testSummary } from "../request/scriptsModel";
import { cx, EmptyState } from "../ui";

export const describeFailure = (f: ScriptFailure) => `${f.script} failed${f.line != null ? ` at line ${f.line}` : ""}: ${f.message}`;

/** "Tests 3/4" with the count colored by outcome. */
export function TestsLabel({ report }: { report: ScriptReport }) {
  const s = testSummary(report);
  return (
    <span>
      Tests <span className={s.failed ? "text-danger" : "text-success"}>{`${s.passed}/${s.total}`}</span>
    </span>
  );
}

export function TestsView({ report }: { report: ScriptReport }) {
  const s = testSummary(report);
  if (!report.tests.length) return <EmptyState title="No tests">Add tests with pm.test in a post-response script.</EmptyState>;
  return (
    <div className="selectable" data-testid="tests">
      <div className="px-3 pb-1 pt-2 text-[12px] text-muted">
        {s.passed} passed · {s.failed} failed{s.skipped ? ` · ${s.skipped} skipped` : ""}
      </div>
      {report.tests.map((t, i) => (
        <div key={i} className="flex items-start gap-2 border-b border-line/60 px-3 py-1.5 text-[12.5px] last:border-b-0">
          {t.skipped ? (
            <CircleMinus size={14} className="mt-0.5 shrink-0 text-faint" aria-label="Skipped" />
          ) : t.passed ? (
            <CircleCheck size={14} className="mt-0.5 shrink-0 text-success" aria-label="Passed" />
          ) : (
            <CircleX size={14} className="mt-0.5 shrink-0 text-danger" aria-label="Failed" />
          )}
          <div className="min-w-0 flex-1">
            <div className={cx("break-words", t.skipped ? "text-faint" : "text-fg")}>{t.name}</div>
            {t.error && <div className="break-words font-mono text-[11.5px] text-danger">{t.error}</div>}
          </div>
        </div>
      ))}
    </div>
  );
}

const LEVEL_CLASS: Record<ConsoleLevel, string> = {
  log: "text-fg",
  info: "text-fg",
  debug: "text-faint",
  warn: "text-warning",
  error: "text-danger",
};

export function ConsoleView({ report }: { report: ScriptReport }) {
  if (!report.console.length && !report.errors.length) return <EmptyState title="No console output">console.log output from scripts shows here.</EmptyState>;
  return (
    <div className="selectable font-mono text-[12px]" data-testid="console">
      {report.console.map((c, i) => (
        <div key={i} className={cx("flex gap-3 border-b border-line/60 px-3 py-1", LEVEL_CLASS[c.level])}>
          <span className="w-10 shrink-0 text-[10.5px] uppercase text-faint">{c.level}</span>
          <span className="min-w-0 flex-1 whitespace-pre-wrap break-words">{c.message}</span>
        </div>
      ))}
      {report.errors.map((f, i) => (
        <div key={`e${i}`} className="flex gap-3 border-b border-line/60 px-3 py-1 text-danger">
          <span className="w-10 shrink-0 text-[10.5px] uppercase text-faint">error</span>
          <span className="min-w-0 flex-1 whitespace-pre-wrap break-words">{describeFailure(f)}</span>
        </div>
      ))}
    </div>
  );
}

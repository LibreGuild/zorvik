// The Lab Guide: docked beside the workbench while a lab runs. It shows the current step,
// ticks steps off as the backend sees them done, and offers hints and "Do it for me".
import { useEffect, useMemo, useRef, useState } from "react";
import { BookOpen, Check, ChevronLeft, ChevronRight, Copy, FlaskConical, Lightbulb, Loader2, MoreHorizontal, PartyPopper, RefreshCw, Server, Square, Wand2 } from "lucide-react";
import type { LabView } from "../../bindings/LabView";
import type { StepView } from "../../bindings/StepView";
import { copyText } from "../../lib/platform";
import { errorMessage } from "../../lib/rpc";
import { answerStep, checkNow, doStep, openAcademy, showHint, stopLab, useAcademy } from "../../store/academy";
import { openServer } from "../../store/tabs";
import { toast } from "../../store/toasts";
import { useUi } from "../../store/ui";
import { Button, cx, IconButton, Input, Menu, Tooltip } from "../ui";
import { Inline, Markdown } from "./Markdown";

export function LabGuide() {
  const lab = useAcademy((s) => s.lab);
  const folded = useAcademy((s) => s.guideFolded);
  if (!lab) return null;
  const done = lab.steps.filter((s) => s.done).length;
  if (folded) {
    return (
      <button
        onClick={() => useAcademy.setState({ guideFolded: false })}
        className="ml-2 flex w-9 shrink-0 flex-col items-center gap-2 rounded-xl border border-line/70 bg-bg py-3 text-accent shadow-[0_1px_3px_rgb(0_0_0/0.12)] hover:bg-panel"
        aria-label="Show the Lab Guide"
        data-testid="lab-guide-unfold"
      >
        <ChevronLeft size={15} />
        <FlaskConical size={15} />
        <span className="text-[11px] font-semibold tabular-nums [writing-mode:vertical-rl]">
          Lab {done}/{lab.steps.length}
        </span>
      </button>
    );
  }
  // A new lab starts with a fresh guide (no typed answer or error left from the last one).
  return <Guide key={`${lab.lesson}:${lab.startedAt}`} lab={lab} />;
}

function Guide({ lab }: { lab: LabView }) {
  const current = lab.steps.findIndex((s) => !s.done);
  const done = lab.steps.filter((s) => s.done).length;
  const vars = useMemo(() => Object.fromEntries(lab.vars.map((v) => [v.key, v.value])), [lab.vars]);
  const list = useRef<HTMLDivElement>(null);
  // Keep the current step in view as steps get done.
  useEffect(() => {
    list.current?.querySelector<HTMLElement>("[data-current=true]")?.scrollIntoView({ block: "nearest", behavior: "smooth" });
  }, [current]);
  return (
    <aside className="ml-2 flex w-[clamp(270px,27vw,350px)] shrink-0 flex-col overflow-hidden rounded-xl border border-line/70 bg-bg shadow-[0_1px_3px_rgb(0_0_0/0.12)]" aria-label="Lab Guide" data-testid="lab-guide">
      <div className="border-b border-line/70 px-3.5 pb-3 pt-3" style={{ background: "linear-gradient(135deg, color-mix(in srgb, var(--accent) 12%, var(--bg)), var(--bg))" }}>
        <div className="flex items-center gap-1.5">
          <span className="flex items-center gap-1 rounded-md bg-accent px-1.5 py-px text-[10px] font-bold uppercase tracking-wide text-accent-fg">
            <FlaskConical size={11} /> Lab
          </span>
          <span className="text-[11.5px] font-medium tabular-nums text-muted" data-testid="lab-progress">
            Step {Math.min(done + 1, lab.steps.length)} of {lab.steps.length}
          </span>
          <span className="ml-auto flex items-center">
            <Menu
              align="end"
              trigger={
                <button aria-label="Lab menu" className="rounded-md p-1 text-muted hover:bg-hover hover:text-fg">
                  <MoreHorizontal size={15} />
                </button>
              }
              entries={[
                { label: "Back to the lesson", icon: <BookOpen size={14} />, onSelect: () => void openAcademy({ kind: "lesson", id: lab.lesson }) },
                { label: "Check again", icon: <RefreshCw size={14} />, onSelect: () => void checkNow() },
                { separator: true },
                { label: "Stop lab", icon: <Square size={13} />, danger: true, onSelect: () => void stopLab() },
              ]}
            />
            <IconButton label="Fold the Lab Guide" onClick={() => useAcademy.setState({ guideFolded: true })}>
              <ChevronRight size={15} />
            </IconButton>
          </span>
        </div>
        <div className="mt-1.5 text-[14px] font-semibold leading-snug text-fg">{lab.title}</div>
        <div className="mt-0.5 text-[12px] leading-snug text-muted">{lab.goal}</div>
        <div className="mt-2.5 flex gap-1" aria-hidden>
          {lab.steps.map((s, i) => (
            <span key={i} className={cx("h-1.5 flex-1 rounded-full transition-colors duration-500", s.done ? "bg-success" : i === current ? "bg-accent" : "bg-panel-2")} />
          ))}
        </div>
      </div>
      <div ref={list} className="min-h-0 flex-1 overflow-y-auto px-3 py-3">
        {lab.finished && <Finished lab={lab} />}
        <ol className="flex flex-col gap-2">
          {lab.steps.map((s, i) => (
            <Step key={i} index={i} step={s} current={i === current} vars={vars} />
          ))}
        </ol>
        <LabInfo lab={lab} />
      </div>
    </aside>
  );
}

function Finished({ lab }: { lab: LabView }) {
  return (
    <div className="zv-pop mb-3 rounded-xl border border-success/40 bg-success/10 px-3.5 py-3" data-testid="lab-finished">
      <div className="flex items-center gap-2 text-[13.5px] font-semibold text-fg">
        <PartyPopper size={16} className="text-success" /> Lab complete!
      </div>
      <div className="mt-1 text-[12.5px] leading-snug text-muted">Head back to the lesson for the quick check to finish it.</div>
      <Button size="sm" variant="primary" className="mt-2.5 w-full" icon={<BookOpen size={13} />} onClick={() => void openAcademy({ kind: "lesson", id: lab.lesson })}>
        Back to the lesson
      </Button>
    </div>
  );
}

function Step({ index, step, current, vars }: { index: number; step: StepView; current: boolean; vars: Record<string, string> }) {
  const [busy, setBusy] = useState<"hint" | "do" | "answer" | "check" | null>(null);
  const [answer, setAnswer] = useState("");
  const [wrong, setWrong] = useState(false);
  const run = async (what: typeof busy, f: () => Promise<unknown>) => {
    setBusy(what);
    try {
      await f();
    } catch (e) {
      toast("error", "Something went wrong", errorMessage(e));
    } finally {
      setBusy(null);
    }
  };
  const submit = () =>
    run("answer", async () => {
      const ok = await answerStep(index, answer);
      setWrong(!ok);
    });
  const future = !step.done && !current;
  return (
    <li
      data-current={current}
      data-testid={`lab-step-${index}`}
      data-done={step.done}
      className={cx(
        "rounded-xl border px-3 py-2.5 transition-colors",
        step.done ? "zv-step-done border-success/30 bg-success/[0.06]" : current ? "border-accent/50 bg-elev shadow-sm" : "border-line/60 bg-transparent",
      )}
    >
      <div className="flex gap-2.5">
        <span
          className={cx(
            "mt-px flex h-5 w-5 shrink-0 items-center justify-center rounded-full text-[10.5px] font-bold",
            step.done ? "bg-success text-white" : current ? "bg-accent text-accent-fg" : "border border-line-strong text-faint",
          )}
        >
          {step.done ? <Check size={12} /> : index + 1}
        </span>
        <div className={cx("min-w-0 flex-1 text-[12.5px] leading-relaxed", future ? "text-faint" : step.done ? "text-muted" : "text-fg")}>
          {current ? <Markdown source={step.text} vars={vars} className="[&_p]:my-0 [&_p]:text-[12.5px] [&_p]:leading-relaxed [&_p]:text-fg [&_p+p]:mt-2" /> : <Inline text={step.text.split("\n")[0]} vars={vars} />}
          {step.done && step.assisted && <div className="mt-0.5 text-[11px] text-faint">Done for you (no XP)</div>}
        </div>
      </div>
      {current && (
        <div className="mt-2.5 pl-[30px]">
          {step.hints.map((h, i) => (
            <div key={i} className="zv-pop mb-2 flex gap-2 rounded-lg bg-[color-mix(in_srgb,var(--warning)_10%,var(--elev))] px-2.5 py-1.5 text-[12px] leading-snug text-fg">
              <Lightbulb size={13} className="mt-0.5 shrink-0 text-warning" />
              <span>
                <Inline text={h} vars={vars} />
              </span>
            </div>
          ))}
          {step.answer && (
            <form
              className="mb-2 flex gap-1.5"
              onSubmit={(e) => {
                e.preventDefault();
                if (answer.trim()) void submit();
              }}
            >
              <Input
                value={answer}
                onChange={(e) => {
                  setAnswer(e.target.value);
                  setWrong(false);
                }}
                placeholder="Your answer"
                aria-label="Your answer"
                invalid={wrong}
                className="h-7 text-[12.5px]"
                data-testid="lab-answer"
              />
              <Button type="submit" size="sm" variant="primary" loading={busy === "answer"} disabled={!answer.trim()}>
                Check
              </Button>
            </form>
          )}
          {wrong && <div className="mb-2 text-[11.5px] text-danger">Not that one. Look again, or open a hint.</div>}
          {!step.answer && (
            <div className="mb-2 flex items-center gap-1.5 text-[11.5px] text-faint" aria-live="polite">
              <span className="relative flex h-2 w-2">
                <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-accent opacity-60" />
                <span className="relative inline-flex h-2 w-2 rounded-full bg-accent" />
              </span>
              Waiting for you to do it in the workbench…
            </div>
          )}
          <div className="flex flex-wrap gap-1">
            {step.hints.length < step.hintsTotal && (
              <Button size="sm" variant="ghost" icon={<Lightbulb size={13} />} loading={busy === "hint"} onClick={() => void run("hint", () => showHint(index))} data-testid="lab-hint">
                {step.hints.length === step.hintsTotal - 1 ? "Show me how" : `Hint (${step.hintsTotal - step.hints.length} left)`}
              </Button>
            )}
            {step.probe && (
              <Button size="sm" variant="ghost" icon={<RefreshCw size={12} />} loading={busy === "check"} onClick={() => void run("check", checkNow)}>
                Check now
              </Button>
            )}
            <Tooltip content="Runs the step for you (no XP for this step)">
              <Button size="sm" variant="ghost" icon={busy === "do" ? <Loader2 size={13} className="zv-spin" /> : <Wand2 size={13} />} disabled={!!busy} onClick={() => void run("do", () => doStep(index))} data-testid="lab-do-it">
                Do it for me
              </Button>
            </Tooltip>
          </div>
        </div>
      )}
    </li>
  );
}

function LabInfo({ lab }: { lab: LabView }) {
  if (!lab.vars.length && !lab.servers.length) return null;
  return (
    <div className="mt-4 rounded-xl border border-line/70 bg-panel/50 px-3 py-2.5">
      <div className="mb-1.5 text-[10.5px] font-semibold uppercase tracking-wide text-faint">Lab environment</div>
      <div className="flex flex-col gap-1">
        {lab.vars
          .filter((v) => !v.key.endsWith("_host") && !v.key.endsWith("_port"))
          .map((v) => (
            <div key={v.key} className="group flex items-center gap-2 text-[11.5px]">
              <code className="zv-var shrink-0 rounded px-1 font-mono">{`{{${v.key}}}`}</code>
              <span className="selectable min-w-0 flex-1 truncate font-mono text-muted" title={v.value}>
                {v.value}
              </span>
              <button
                aria-label={`Copy ${v.key}`}
                onClick={() => void copyText(v.value).then(() => toast("success", "Copied", v.value))}
                className="rounded p-0.5 text-faint opacity-0 hover:text-fg focus-visible:opacity-100 group-hover:opacity-100"
              >
                <Copy size={12} />
              </button>
            </div>
          ))}
      </div>
      {lab.servers.length > 0 && (
        <div className="mt-2 flex flex-col gap-1 border-t border-line/60 pt-2">
          {lab.servers.map((s) => (
            <button
              key={s.serverId}
              onClick={() => {
                useUi.setState({ sidebarTab: "servers" });
                void openServer(s.serverId);
              }}
              className="flex items-center gap-2 rounded-md px-1 py-0.5 text-left text-[11.5px] text-muted hover:bg-hover hover:text-fg"
              title="Open the server and its traffic"
            >
              <Server size={12} className="shrink-0 text-success" />
              <span className="truncate">{s.name}</span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
